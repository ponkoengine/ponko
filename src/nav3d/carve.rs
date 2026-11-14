use super::shape::{self, Footprint};
use super::*;

/// The tile grid a bake produces, computed the way the backend's tile baker
/// does so both bake paths agree on tile positions and polygon references.
pub(super) struct TileGrid {
    origin: [f32; 3],
    tile_world: f32,
    pub(super) count: usize,
    /// The tile width in cells that was chosen.
    pub(super) tile_size: u32,
    border: u32,
}
impl TileGrid {
    /// Lays out the tiles, growing `tile_size` (at most to 255) when the level
    /// would otherwise need more than `MAX_TILES`.
    pub(super) fn new(verts: &[f32], s: &NavBakeSettings) -> Result<Self, NavBakeError> {
        let (min, max) = recast::calc_bounds(verts);
        let cells =
            |axis: usize| ((f64::from(max[axis]) - f64::from(min[axis])) / f64::from(s.cell_size) + 0.5).floor();
        let (gw, gh) = (cells(0), cells(2));
        // Voxel coordinates are 32-bit in the backend.
        if gw.max(gh) >= f64::from(i32::MAX) / 2.0 {
            return Err(error(format!(
                "navigation bake grid is too large; raise NavBakeSettings::cell_size (now {})",
                s.cell_size
            )));
        }
        let tiles = |ts: u32| {
            let ts = f64::from(ts);
            (
                ((gw + ts - 1.0) / ts).floor().max(1.0),
                ((gh + ts - 1.0) / ts).floor().max(1.0),
            )
        };
        let tile_size = (s.tile_size..=255).find(|&ts| {
            let (tw, th) = tiles(ts);
            tw * th <= MAX_TILES as f64
        });
        let Some(tile_size) = tile_size else {
            let (tw, th) = tiles(255);
            return Err(error(format!(
                "navigation bake needs {tw} x {th} tiles even at the largest tile_size (255), but a navmesh \
                 holds at most {MAX_TILES} tiles; raise NavBakeSettings::cell_size (now {}) or bake the level \
                 in separate meshes",
                s.cell_size
            )));
        };
        let (tw, th) = tiles(tile_size);
        Ok(Self {
            origin: min,
            tile_world: tile_size as f32 * s.cell_size,
            count: (tw * th) as usize,
            tile_size,
            border: (s.agent_radius / s.cell_size).ceil() as u32 + 3,
        })
    }
    /// Cap on one tile's navmesh bytes. A tile has at most `(size + 2 * border)^2`
    /// cells and a polygon mesh needs well under 64 bytes per cell.
    pub(super) fn max_tile_bytes(&self) -> usize {
        let side = (self.tile_size + 2 * self.border) as usize;
        (side * side * 64).max(1 << 20)
    }
    /// The navmesh layout the backend's tile baker would choose for this grid.
    pub(super) fn layout(&self) -> detour::NavMeshParams {
        let tile_bits = self.count.next_power_of_two().trailing_zeros();
        detour::NavMeshParams {
            orig: self.origin,
            tile_width: self.tile_world,
            tile_height: self.tile_world,
            max_tiles: 1 << tile_bits,
            max_polys: 1 << (22 - tile_bits).min(20),
        }
    }
}

/// One cached heightfield layer: the walkable floor of one tile at one height
/// band, stored with the raw codec.
struct CachedLayer {
    tx: i32,
    ty: i32,
    tlayer: i32,
    min: Vec3,
    max: Vec3,
    data: Vec<u8>,
}

/// What a layer rebuild needs from the bake.
#[derive(Clone, Copy)]
struct LayerParams {
    cs: f32,
    ch: f32,
    agent_radius: f32,
    agent_height: f32,
    max_climb: f32,
    /// `max_climb` in cells.
    climb: i32,
    /// Contour simplification, in cells (the backend tile cache's default).
    max_error: f32,
    detail_sample_dist: f32,
    detail_sample_error: f32,
}

/// Every tile's heightfield layers, kept so carving can rebuild a tile from
/// them without rasterizing source triangles again, and the queue of layers
/// waiting for a rebuild.
///
/// Rebuilding marks the carving obstacles' exact footprints on a copy of the
/// layer, then runs the backend's layer pipeline (regions, contours,
/// polygons) and builds the polygons' detail heights from the layer's floor
/// heights with the same sampling as a normal bake.
pub(super) struct LayerCache {
    layers: Vec<CachedLayer>,
    /// Layers waiting for a rebuild, oldest first, from `head`.
    queue: Vec<usize>,
    head: usize,
    queued: Vec<bool>,
    params: LayerParams,
    ws: Workspace,
    /// Scratch for the obstacles that touch the layer being rebuilt.
    touching: Vec<Footprint>,
}

impl LayerCache {
    /// Whether layer rebuilds are queued.
    pub(super) fn pending(&self) -> bool {
        self.head < self.queue.len()
    }

    /// Queues every layer whose bounds meet `[min, max]`.
    fn queue_overlapping(&mut self, min: Vec3, max: Vec3) {
        for (i, l) in self.layers.iter().enumerate() {
            if !self.queued[i] && l.max.cmpge(min).all() && l.min.cmple(max).all() {
                self.queued[i] = true;
                self.queue.push(i);
            }
        }
    }

    /// The world bounds an obstacle can carve in: its footprint grown by the
    /// agent radius and half a cell, from the agent height below it to its top.
    fn reach(&self, f: &Footprint) -> (Vec3, Vec3) {
        let p = &self.params;
        let (min, max) = shape::reach_bounds(f, p.agent_radius + p.cs, p.agent_height + p.ch);
        (min, max + Vec3::Y * p.ch)
    }

    /// Rebuilds the front queued layer into `mesh`, carving `obstacles`. On an
    /// error the layer stays queued and the mesh keeps its old tile.
    fn rebuild_next(&mut self, mesh: &mut detour::NavMesh, obstacles: &[NavObstacle3D]) -> Result<(), NavBakeError> {
        let i = self.queue[self.head];
        self.touching.clear();
        let l = &self.layers[i];
        for o in obstacles {
            let f = Footprint::of(o).ok_or_else(|| error("invalid carving obstacle"))?;
            let (min, max) = self.reach(&f);
            if l.max.cmpge(min).all() && l.min.cmple(max).all() {
                self.touching.push(f);
            }
        }
        let data = build_tile(&mut self.ws, &self.params, &l.data, &self.touching)?;
        replace_tile(mesh, l, data)?;
        self.queued[i] = false;
        self.head += 1;
        if self.head == self.queue.len() {
            self.queue.clear();
            self.head = 0;
        }
        Ok(())
    }
}

/// Swaps the navmesh tile of `layer` for freshly built data (none: no polygons).
fn replace_tile(mesh: &mut detour::NavMesh, layer: &CachedLayer, data: Option<Vec<u8>>) -> Result<(), NavBakeError> {
    if let Some(k) = mesh.tile_index_at(layer.tx, layer.ty, layer.tlayer) {
        mesh.remove_tile(mesh.tile_ref(k)).map_err(error)?;
    }
    if let Some(data) = data {
        mesh.add_tile(data).map_err(error)?;
    }
    Ok(())
}

/// Builds the navmesh tile of one cached layer with `carve` marked
/// unwalkable: the backend's layer meshing, then a detail mesh sampled from
/// the layer's floor heights. `None` when nothing walkable is left.
fn build_tile(
    ws: &mut Workspace,
    p: &LayerParams,
    data: &[u8],
    carve: &[Footprint],
) -> Result<Option<Vec<u8>>, NavBakeError> {
    let mut layer = tilecache::decompress_layer(ws, &tilecache::RawCompressor, data).map_err(error)?;
    let h = layer.header().clone();
    if !carve.is_empty() {
        let mut areas = layer.areas().to_vec();
        let w = usize::from(h.width);
        let margin = p.agent_radius + p.cs * 0.5;
        let mut changed = false;
        for (i, (area, &height)) in areas.iter_mut().zip(layer.heights()).enumerate() {
            if *area == 0 {
                continue;
            }
            let at = Vec2::new(
                h.bmin[0] + ((i % w) as f32 + 0.5) * p.cs,
                h.bmin[2] + ((i / w) as f32 + 0.5) * p.cs,
            );
            let y = h.bmin[1] + f32::from(height) * p.ch;
            if carve
                .iter()
                .any(|f| y >= f.bottom - p.agent_height - p.ch && y <= f.top + p.ch && f.reaches(at, margin))
            {
                *area = 0;
                changed = true;
            }
        }
        if changed {
            let marked = tilecache::build_layer(
                ws,
                &tilecache::RawCompressor,
                &h,
                layer.heights(),
                &areas,
                layer.connections(),
            )
            .map_err(error)?;
            layer = tilecache::decompress_layer(ws, &tilecache::RawCompressor, &marked).map_err(error)?;
        }
    }
    tilecache::build_regions(ws, &mut layer, p.climb).map_err(error)?;
    let contours = tilecache::build_contours(ws, &layer, p.climb, p.max_error).map_err(error)?;
    let mut polys = tilecache::build_poly_mesh(ws, &contours).map_err(error)?;
    if polys.areas.is_empty() {
        return Ok(None);
    }
    // As the backend's default mesh process: walkable area 63 is ground (0)
    // and every polygon gets flag 1.
    for (a, f) in polys.areas.iter_mut().zip(&mut polys.flags) {
        if *a == 63 {
            *a = 0;
        }
        *f = 1;
    }
    let detail = layer_detail(ws, p, &layer, &polys)?;
    let params = detour::NavMeshCreateParams {
        verts: &polys.verts,
        vert_count: i32::try_from(polys.verts.len() / 3).map_err(error)?,
        polys: &polys.polys,
        poly_flags: &polys.flags,
        poly_areas: &polys.areas,
        poly_count: i32::try_from(polys.areas.len()).map_err(error)?,
        nvp: 6,
        detail_meshes: &detail.meshes,
        detail_verts: &detail.verts,
        detail_verts_count: i32::try_from(detail.verts.len() / 3).map_err(error)?,
        detail_tris: &detail.tris,
        detail_tri_count: i32::try_from(detail.tris.len() / 4).map_err(error)?,
        tile_x: h.tx,
        tile_y: h.ty,
        tile_layer: h.tlayer,
        bmin: h.bmin,
        bmax: h.bmax,
        walkable_height: p.agent_height,
        walkable_radius: p.agent_radius,
        walkable_climb: p.max_climb,
        cs: p.cs,
        ch: p.ch,
        build_bv_tree: true,
        ..detour::NavMeshCreateParams::default()
    };
    detour::create_nav_mesh_data(&params).map(Some).map_err(error)
}

/// The detail mesh of a layer's polygons: Recast's detail builder run on the
/// layer seen as a one-span-per-cell compact heightfield.
fn layer_detail(
    ws: &mut Workspace,
    p: &LayerParams,
    layer: &tilecache::Layer,
    polys: &tilecache::LayerPolyMesh,
) -> Result<recast::PolyMeshDetail, NavBakeError> {
    let h = layer.header();
    let (w, d) = (usize::from(h.width), usize::from(h.height));
    let heights = layer.heights();
    let cons = layer.connections();
    let mut cells = vec![recast::CompactCell::default(); w * d];
    let mut spans = Vec::new();
    let mut areas = Vec::new();
    for (i, cell) in cells.iter_mut().enumerate() {
        if heights[i] == 255 {
            continue;
        }
        cell.index = spans.len() as u32;
        cell.count = 1;
        let (x, z) = (i % w, i / w);
        let mut span = recast::CompactSpan {
            y: u16::from(heights[i]),
            // One region for every span: the detail builder then reads every
            // floor height inside a polygon's bounds directly.
            reg: 1,
            con: 0,
            h: 255,
        };
        // Low four bits of a layer connection: neighbours within the layer,
        // in Recast's direction order (-x, +z, +x, -z).
        for dir in 0..4 {
            let (nx, nz) = match dir {
                0 => (x.wrapping_sub(1), z),
                1 => (x, z + 1),
                2 => (x + 1, z),
                _ => (x, z.wrapping_sub(1)),
            };
            let linked = cons[i] & (1 << dir) != 0 && nx < w && nz < d && heights[nx + nz * w] != 255;
            span.set_con(dir, if linked { 0 } else { recast::NOT_CONNECTED });
        }
        spans.push(span);
        areas.push(layer.areas()[i].max(1));
    }
    let chf = recast::CompactHeightfield {
        width: w as i32,
        height: d as i32,
        span_count: spans.len() as i32,
        walkable_height: (p.agent_height / p.ch).ceil() as i32,
        walkable_climb: p.climb,
        border_size: 0,
        max_distance: 0,
        max_regions: 2,
        bmin: h.bmin,
        bmax: h.bmax,
        cs: p.cs,
        ch: p.ch,
        cells,
        spans,
        dist: Vec::new(),
        areas,
    };
    let npolys = polys.areas.len();
    let mesh = recast::PolyMesh {
        verts: polys.verts.clone(),
        polys: polys.polys.clone(),
        regs: vec![1; npolys],
        flags: polys.flags.clone(),
        areas: polys.areas.clone(),
        nverts: (polys.verts.len() / 3) as i32,
        npolys: npolys as i32,
        maxpolys: npolys as i32,
        nvp: 6,
        bmin: h.bmin,
        bmax: h.bmax,
        cs: p.cs,
        ch: p.ch,
        border_size: 0,
        max_edge_error: p.max_error,
    };
    recast::PolyMeshDetail::build(ws, &mesh, &chf, p.detail_sample_dist, p.detail_sample_error).map_err(error)
}

/// Bakes the tiles from cached heightfield layers, keeping the layers so
/// carving can rebuild them.
pub(super) fn bake_layers(
    verts: &[f32],
    tris: &[i32],
    config: &bake::BakeConfig,
    layout: detour::NavMeshParams,
) -> Result<(detour::NavMesh, LayerCache), NavBakeError> {
    let c = config.recast;
    let params = LayerParams {
        cs: c.cs,
        ch: c.ch,
        agent_radius: config.walkable_radius,
        agent_height: config.walkable_height,
        max_climb: config.walkable_climb,
        climb: c.walkable_climb,
        max_error: 1.3,
        detail_sample_dist: c.detail_sample_dist,
        detail_sample_error: c.detail_sample_max_error,
    };
    let mut mesh = detour::NavMesh::new(&detour::NavMeshParams {
        max_tiles: layout.max_tiles * 4,
        ..layout
    })
    .map_err(error)?;
    let mut ws = Workspace::new();
    let mut areas = vec![0; tris.len() / 3];
    recast::mark_walkable_triangles(&mut ws, c.walkable_slope_angle, verts, tris, &mut areas).map_err(error)?;
    let max = verts
        .as_chunks::<3>()
        .0
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |m, v| m.max(Vec3::from_slice(v)));
    let origin = Vec3::from_array(layout.orig);
    let nx = ((max.x - origin.x) / layout.tile_width).ceil() as i32;
    let nz = ((max.z - origin.z) / layout.tile_height).ceil() as i32;
    let border = c.walkable_radius + 3;
    let mut layers = Vec::new();
    // Layer rebuilds get the same per-tile memory and work budget as a bake.
    let mut tile_ws = Workspace::with_limits(Limits {
        max_bytes: config.tile_limits.max_bytes,
        max_work: config.tile_limits.max_work,
        ..Limits::default()
    });
    for z in 0..nz {
        for x in 0..nx {
            let mut lo = origin + Vec3::new(x as f32 * layout.tile_width, 0.0, z as f32 * layout.tile_height);
            let mut hi = lo + Vec3::new(layout.tile_width, max.y - origin.y + c.ch, layout.tile_height);
            lo -= Vec3::new(border as f32 * c.cs, 0.0, border as f32 * c.cs);
            hi += Vec3::new(border as f32 * c.cs, 0.0, border as f32 * c.cs);
            let size = c.tile_size + 2 * border;
            let mut hf = recast::Heightfield::new(&mut ws, size, size, lo.to_array(), hi.to_array(), c.cs, c.ch)
                .map_err(error)?;
            hf.rasterize_triangles(&mut ws, verts, tris, &areas, c.walkable_climb)
                .map_err(error)?;
            hf.filter_low_hanging_walkable_obstacles(&mut ws, c.walkable_climb)
                .map_err(error)?;
            hf.filter_ledge_spans(&mut ws, c.walkable_height, c.walkable_climb)
                .map_err(error)?;
            hf.filter_walkable_low_height_spans(&mut ws, c.walkable_height)
                .map_err(error)?;
            let mut chf =
                recast::CompactHeightfield::build(&mut ws, c.walkable_height, c.walkable_climb, &hf).map_err(error)?;
            recast::erode_walkable_area(&mut ws, c.walkable_radius, &mut chf).map_err(error)?;
            let set = recast::HeightfieldLayerSet::build(&mut ws, &chf, border, c.walkable_height).map_err(error)?;
            for (i, l) in set.layers.iter().enumerate() {
                let mut h = tilecache::LayerHeader::new(l.width as u8, l.height as u8);
                h.tx = x;
                h.ty = z;
                h.tlayer = i as i32;
                h.bmin = l.bmin;
                h.bmax = l.bmax;
                h.hmin = l.hmin as u16;
                h.hmax = l.hmax as u16;
                h.minx = l.minx as u8;
                h.maxx = l.maxx as u8;
                h.miny = l.miny as u8;
                h.maxy = l.maxy as u8;
                let data =
                    tilecache::build_layer(&mut ws, &tilecache::RawCompressor, &h, &l.heights, &l.areas, &l.cons)
                        .map_err(error)?;
                let layer = CachedLayer {
                    tx: x,
                    ty: z,
                    tlayer: i as i32,
                    min: Vec3::from_array(l.bmin),
                    max: Vec3::from_array(l.bmax),
                    data,
                };
                replace_tile(&mut mesh, &layer, build_tile(&mut tile_ws, &params, &layer.data, &[])?)?;
                layers.push(layer);
            }
        }
    }
    let count = layers.len();
    Ok((
        mesh,
        LayerCache {
            layers,
            queue: Vec::new(),
            head: 0,
            queued: vec![false; count],
            params,
            ws: tile_ws,
            touching: Vec::new(),
        },
    ))
}

impl NavState {
    /// The carving obstacles wanted now, in order.
    fn desired_carving(&self) -> impl Iterator<Item = &NavObstacle3D> {
        self.obstacles.iter().filter(|o| o.carve)
    }
    pub(super) fn carving_pending(&self) -> bool {
        self.cache.as_ref().is_some_and(LayerCache::pending) || self.carving_changed()
    }
    pub(super) fn carving_changed(&self) -> bool {
        if self.cache.is_none() {
            return false;
        }
        let threshold = self.settings.cell_size * 0.5;
        let mut desired = self.desired_carving();
        for b in &self.carved {
            let Some(a) = desired.next() else {
                return true;
            };
            if shape::carve_moved(a, b, threshold) {
                return true;
            }
        }
        desired.next().is_some()
    }
    /// Takes every changed carving obstacle into `carved` and queues the
    /// layers its old and new footprints touch. Ordered obstacle lists supply
    /// identity; obstacles that moved less than half a cell keep their last
    /// carved place. An invalid obstacle is an error and changes nothing.
    fn sync_carving(&mut self) -> Result<(), NavBakeError> {
        if !self.carving_changed() {
            return Ok(());
        }
        let threshold = self.settings.cell_size * 0.5;
        let desired: Vec<NavObstacle3D> = self.desired_carving().copied().collect();
        let mut wanted = Vec::with_capacity(desired.len());
        for o in &desired {
            wanted.push(Footprint::of(o).ok_or_else(|| error("invalid carving obstacle"))?);
        }
        let Some(cache) = &mut self.cache else {
            return Ok(());
        };
        let count = desired.len().max(self.carved.len());
        for i in 0..count {
            let old = self.carved.get(i);
            let new = desired.get(i);
            if let (Some(a), Some(b)) = (new, old)
                && !shape::carve_moved(a, b, threshold)
            {
                continue;
            }
            if let Some(f) = old.and_then(Footprint::of) {
                let (min, max) = cache.reach(&f);
                cache.queue_overlapping(min, max);
            }
            if new.is_some() {
                let (min, max) = cache.reach(&wanted[i]);
                cache.queue_overlapping(min, max);
            }
        }
        let mut carved = Vec::with_capacity(desired.len());
        for (i, o) in desired.iter().enumerate() {
            match self.carved.get(i) {
                Some(b) if !shape::carve_moved(o, b, threshold) => carved.push(*b),
                _ => carved.push(*o),
            }
        }
        self.carved = carved;
        Ok(())
    }
    pub(super) fn update_carving(&mut self, budget: usize) -> Result<usize, NavBakeError> {
        let mut count = 0;
        if self.cache.is_none() {
            return Ok(0);
        }
        if budget > 0 {
            self.sync_carving()?;
        }
        while count < budget {
            let cache = self.cache.as_mut().expect("checked above");
            if !cache.pending() {
                break;
            }
            cache.rebuild_next(&mut self.mesh, &self.carved)?;
            count += 1;
        }
        let recover = self.carve_failed;
        self.carve_failed = false;
        if count > 0 || recover {
            self.polygons.clear();
            for i in 0..self.mesh.max_tiles() as usize {
                if let Some(tile) = self.mesh.tile(i) {
                    let base = self.mesh.poly_ref_base(i).unwrap();
                    for (j, p) in tile.polys.iter().enumerate() {
                        let mut min = Vec3::splat(f32::INFINITY);
                        let mut max = Vec3::splat(f32::NEG_INFINITY);
                        for &v in &p.verts[..p.vert_count as usize] {
                            let at = v as usize * 3;
                            let v = Vec3::from_slice(&tile.verts[at..at + 3]);
                            min = min.min(v);
                            max = max.max(v);
                        }
                        self.polygons.push(PolygonBounds {
                            reference: base | j as detour::PolyRef,
                            min,
                            max,
                        });
                    }
                }
            }
            self.apply_obstacles();
        }
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn grid_for(size: f32, s: &NavBakeSettings) -> Result<TileGrid, NavBakeError> {
        let h = size / 2.0;
        TileGrid::new(&[-h, 0.0, -h, h, 0.0, h], s)
    }
    #[test]
    fn grid_layout_matches_the_backend_tile_baker() {
        for (size, tile_size, cell_size) in [
            (20.0, 32, 0.2),
            (40.0, 64, 0.2),
            (333.0, 48, 0.3),
            (7.0, 255, 0.1),
            (600.0, 8, 0.5),
        ] {
            let s = NavBakeSettings {
                tile_size,
                cell_size,
                ..Default::default()
            };
            let h = size / 2.0;
            let verts = [-h, 0.0, -h, -h, 0.0, h, h, 0.0, h, h, 0.0, -h];
            let tris = [0, 1, 2, 0, 2, 3];
            let grid = TileGrid::new(&verts, &s).unwrap();
            let mut config = bake::BakeConfig::default();
            config.recast.cs = cell_size;
            config.recast.tile_size = grid.tile_size as i32;
            let baked = bake::tiles(&bake::Input::new(&verts, &tris), &config, &mut Workspace::new()).unwrap();
            assert_eq!(grid.layout(), baked.params, "{size} {tile_size} {cell_size}");
            assert_eq!(grid.count, baked.tiles.len());
        }
    }
    #[test]
    fn tiles_grow_for_big_levels_until_the_largest_size_cannot_fit() {
        let s = NavBakeSettings::default();
        assert_eq!(grid_for(800.0, &s).unwrap().tile_size, 32);
        let grown = grid_for(1700.0, &s).unwrap();
        assert!(grown.tile_size > 32 && grown.count <= MAX_TILES);
        assert_eq!(
            grid_for(1700.0, &NavBakeSettings { tile_size: 100, ..s })
                .unwrap()
                .tile_size,
            100
        );
        // 255 cells of 0.2 m: 51 m tiles, 16384 of them, 6.5 km square.
        assert!(grid_for(6500.0, &s).unwrap().tile_size >= 250);
        let message = grid_for(7000.0, &s).err().unwrap().to_string();
        assert!(message.contains("16384") && message.contains("cell_size"), "{message}");
    }
    fn refs(s: &NavState) -> Vec<Option<detour::PolyRef>> {
        (0..s.mesh.max_tiles() as usize)
            .map(|i| s.mesh.poly_ref_base(i))
            .collect()
    }
    #[test]
    fn rebuilding_preserves_untouched_tiles_and_respects_budget() {
        let floor = NavSource::from_mesh(&MeshData::plane(20.0), Transform3D::default());
        let settings = NavBakeSettings {
            carving: true,
            ..Default::default()
        };
        let mut s = NavState::bake(&[floor], settings).unwrap();
        let before = refs(&s);
        let mut o = NavObstacle3D::new(Vec3::new(0.0, 1.0, 0.0), Vec3::ONE);
        o.carve = true;
        s.set_obstacles(&[o]);
        let mut total = 0;
        while s.carving_pending() {
            let old = refs(&s);
            let n = s.update_carving(1).unwrap();
            let changed = old.iter().zip(refs(&s)).filter(|(a, b)| **a != *b).count();
            assert!(changed <= 1, "{changed}");
            total += n;
        }
        assert!(total > 0 && total <= 8);
        let changed = before.iter().zip(refs(&s)).filter(|(a, b)| **a != *b).count();
        assert_eq!(changed, total);
        assert!(changed < before.iter().filter(|r| r.is_some()).count());
    }
    /// The largest gap between the mesh and `height` over a grid of points.
    fn ground_error(s: &NavState, height: impl Fn(f32, f32) -> f32) -> f32 {
        let mut worst = 0.0f32;
        for i in -8..=8 {
            for j in -8..=8 {
                let (x, z) = (i as f32 * 0.7, j as f32 * 0.7);
                let Some(p) = s.sample_position(Vec3::new(x, height(x, z), z), 2.0) else {
                    return f32::INFINITY;
                };
                worst = worst.max((p.y - height(p.x, p.z)).abs());
            }
        }
        worst
    }
    #[test]
    fn layer_tiles_follow_curved_ground_only_with_detail_heights() {
        let height = |x: f32, z: f32| 2.0 * (-(x * x + z * z) / 24.5).exp();
        let mut triangles = Vec::new();
        let at = |i: i32, j: i32| {
            let (x, z) = (i as f32 * 0.5 - 12.0, j as f32 * 0.5 - 12.0);
            Vec3::new(x, height(x, z), z)
        };
        for i in 0..48 {
            for j in 0..48 {
                let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j), at(i + 1, j + 1));
                triangles.extend([[a, b, c], [c, b, d]]);
            }
        }
        let settings = NavBakeSettings {
            carving: true,
            ..Default::default()
        };
        let mut s = NavState::bake(&[NavSource { triangles }], settings).unwrap();
        let with = ground_error(&s, height);
        // Negative control: the same layers without detail samples keep only
        // the polygons' corner heights, as carved tiles did before.
        let cache = s.cache.as_mut().unwrap();
        cache.params.detail_sample_dist = 0.0;
        cache.queue_overlapping(Vec3::splat(-100.0), Vec3::splat(100.0));
        assert!(s.update_carving(usize::MAX).unwrap() > 0);
        let without = ground_error(&s, height);
        assert!(
            with < 0.3 && without > 2.0 * with,
            "with detail {with}, without {without}"
        );
    }
}
