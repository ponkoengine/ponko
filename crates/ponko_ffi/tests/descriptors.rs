//! Versioned descriptors (STABILITY.md, "Versioned descriptors"): the
//! header rules (`struct_size` and `version` first, refused when wrong,
//! only the caller's version's fields read or written), and every versioned
//! entry point: lighting with the sun's shadow size, materials with a
//! stencil, navmesh bakes, 2D colliders with collision groups, and 2D and
//! 3D queries filtered by groups. Game calls run headless inside callbacks
//! that record what they saw; the tests check afterwards.
use ponko_ffi::*;
use std::ffi::{CStr, c_void};
use std::mem::offset_of;
use std::ptr::{null, null_mut};

use PonkoStatus::*;

fn last_error() -> String {
    // SAFETY: the library's NUL-terminated message.
    unsafe { CStr::from_ptr(ponko_last_error()) }
        .to_string_lossy()
        .into_owned()
}

/// A descriptor of a newer header: this one's fields, then fields this
/// library doesn't know.
#[repr(C)]
struct Newer<T> {
    known: T,
    unknown: [u8; 32],
}

fn header<T>(version: u32) -> T {
    // SAFETY: every descriptor is plain data, all-zero valid.
    let mut d: T = unsafe { std::mem::zeroed() };
    let words = (&raw mut d).cast::<u32>();
    unsafe {
        words.write(size_of::<T>() as u32);
        words.add(1).write(version);
    }
    d
}

#[test]
fn a_descriptor_with_a_bad_header_is_refused_and_left_alone() {
    unsafe {
        // Version 0: the header wasn't set.
        let mut l: PonkoLightingEx = std::mem::zeroed();
        assert_eq!(ponko_lighting_ex_defaults(&mut l), PONKO_INVALID_ARGUMENT);
        assert!(last_error().contains("version is 0"), "{}", last_error());
        assert_eq!(l.lighting.sun_intensity, 0.0, "nothing written");
        // A version newer than the library: the program needs a newer library.
        let mut l: PonkoLightingEx = header(PONKO_LIGHTING_EX_VERSION + 1);
        assert_eq!(ponko_lighting_ex_defaults(&mut l), PONKO_INVALID_ARGUMENT);
        assert!(last_error().contains("newer"), "{}", last_error());
        // struct_size smaller than its version's fields.
        let mut m: PonkoMaterialDescEx = header(PONKO_MATERIAL_DESC_EX_VERSION);
        m.struct_size = 8;
        assert_eq!(ponko_material_desc_ex_defaults(&mut m), PONKO_INVALID_ARGUMENT);
        assert!(last_error().contains("struct_size"), "{}", last_error());
        // Null.
        assert_eq!(ponko_nav_bake_settings_ex_defaults(null_mut()), PONKO_INVALID_ARGUMENT);
        assert_eq!(
            ponko_collider_ex_from(ponko_collider_circle(1.0), null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        assert_eq!(ponko_query_filter_ex_defaults(null_mut()), PONKO_INVALID_ARGUMENT);
    }
}

#[test]
fn a_newer_header_s_larger_struct_works_and_its_unknown_tail_is_untouched() {
    unsafe {
        let mut n = Newer {
            known: header::<PonkoQueryFilterEx>(PONKO_QUERY_FILTER_EX_VERSION),
            unknown: [0xAB; 32],
        };
        n.known.struct_size = size_of::<Newer<PonkoQueryFilterEx>>() as u32;
        assert_eq!(ponko_query_filter_ex_defaults(&mut n.known), PONKO_OK);
        assert_eq!(n.known.groups.memberships, u32::MAX);
        assert_eq!(
            n.known.struct_size,
            size_of::<Newer<PonkoQueryFilterEx>>() as u32,
            "the header stays"
        );
        assert_eq!(
            n.unknown, [0xAB; 32],
            "fields past the caller's version are not written"
        );

        let mut n = Newer {
            known: header::<PonkoLightingEx>(PONKO_LIGHTING_EX_VERSION),
            unknown: [0xCD; 32],
        };
        n.known.struct_size = size_of::<Newer<PonkoLightingEx>>() as u32;
        assert_eq!(ponko_lighting_ex_defaults(&mut n.known), PONKO_OK);
        assert!((n.known.shadow_sun_size - 0.53).abs() < 1e-6);
        assert_eq!(n.unknown, [0xCD; 32]);
    }
}

#[test]
fn defaults_match_the_abi_2_defaults() {
    unsafe {
        let mut m: PonkoMaterialDescEx = header(PONKO_MATERIAL_DESC_EX_VERSION);
        assert_eq!(ponko_material_desc_ex_defaults(&mut m), PONKO_OK);
        let white = PonkoColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        };
        assert_eq!(m.desc, ponko_material_desc(white));
        assert!(!m.has_stencil);
        // Version 3: lit, and every layer off.
        assert!(!m.unlit);
        let l = m.layers;
        assert_eq!((l.clearcoat, l.iridescence, l.anisotropy), (0.0, 0.0, 0.0));
        assert_eq!((l.specular, l.ior, l.iridescence_ior), (1.0, 1.5, 1.3));
        assert_eq!(l.iridescence_thickness, [100.0, 400.0]);
        assert!(!l.has_clearcoat_map && !l.has_anisotropy_map);
        // A version 2 caller's smaller struct: its fields only.
        let mut v2: PonkoMaterialDescEx = header(2);
        v2.struct_size = std::mem::offset_of!(PonkoMaterialDescEx, unlit) as u32;
        v2.unlit = true;
        assert_eq!(ponko_material_desc_ex_defaults(&mut v2), PONKO_OK);
        assert_eq!(v2.desc.roughness, 0.6);
        assert!(v2.unlit, "fields past version 2 are not written");
        assert_eq!(m.stencil, ponko_stencil_default());
        assert!(!m.masked_tint);

        let mut s: PonkoNavBakeSettingsEx = header(PONKO_NAV_BAKE_SETTINGS_EX_VERSION);
        assert_eq!(ponko_nav_bake_settings_ex_defaults(&mut s), PONKO_OK);
        assert_eq!(s.settings, ponko_nav_bake_settings_default());

        let mut c: PonkoColliderEx = header(PONKO_COLLIDER_EX_VERSION);
        assert_eq!(
            ponko_collider_ex_from(ponko_collider_rect(PonkoVec2 { x: 2.0, y: 3.0 }), &mut c),
            PONKO_OK
        );
        assert_eq!(c.collider, ponko_collider_rect(PonkoVec2 { x: 2.0, y: 3.0 }));
        assert_eq!(c.groups, ponko_collision_groups_all());
        assert_eq!(c.point_count, 0);

        let mut f: PonkoQueryFilterEx = header(PONKO_QUERY_FILTER_EX_VERSION);
        assert_eq!(ponko_query_filter_ex_defaults(&mut f), PONKO_OK);
        assert_eq!(
            (f.exclude, f.sensors, f.groups),
            (false, false, ponko_collision_groups_all())
        );
    }
}

// ---- lighting, materials, navmesh bakes ---------------------------------------

#[derive(Default)]
struct Seen {
    failures: Vec<String>,
    sun_size_read: f32,
    sun_size_after_bad: f32,
    intensity_after_bad: f32,
    abi2_still_reads: bool,
    stencil_material_live: bool,
}

macro_rules! expect {
    ($seen:expr, $status:expr, $want:expr) => {{
        let got = $status;
        if got != $want {
            $seen.failures.push(format!(
                "line {}: {} gave {got:?}, wanted {:?} ({})",
                line!(),
                stringify!($status),
                $want,
                last_error()
            ));
        }
    }};
}

unsafe extern "C" fn three_d(ctx: *mut PonkoCtx, user: *mut c_void) {
    let seen = unsafe { &mut *user.cast::<Seen>() };
    unsafe {
        // Lighting: shadow_sun_size reaches the engine and comes back.
        let mut l: PonkoLightingEx = header(PONKO_LIGHTING_EX_VERSION);
        expect!(seen, ponko_ctx_lighting_ex(ctx, &mut l), PONKO_OK);
        l.shadow_sun_size = 2.0;
        l.lighting.shadow_filter = 4;
        l.lighting.sun_intensity = 3.0;
        expect!(seen, ponko_ctx_set_lighting_ex(ctx, &l), PONKO_OK);
        let mut back: PonkoLightingEx = header(PONKO_LIGHTING_EX_VERSION);
        expect!(seen, ponko_ctx_lighting_ex(ctx, &mut back), PONKO_OK);
        seen.sun_size_read = back.shadow_sun_size;
        // The ABI 2 call reads the same lighting.
        let mut old: PonkoLighting = std::mem::zeroed();
        expect!(seen, ponko_ctx_lighting(ctx, &mut old), PONKO_OK);
        seen.abi2_still_reads = old.sun_intensity == 3.0 && old.shadow_filter == 4;
        // Out of range: refused, nothing changes (the ABI 2 part neither).
        let mut bad = back;
        bad.shadow_sun_size = 11.0;
        bad.lighting.sun_intensity = 9.0;
        expect!(seen, ponko_ctx_set_lighting_ex(ctx, &bad), PONKO_INVALID_ARGUMENT);
        let mut bad = back;
        bad.shadow_sun_step = -1.0;
        expect!(seen, ponko_ctx_set_lighting_ex(ctx, &bad), PONKO_INVALID_ARGUMENT);
        let mut bad = back;
        bad.lighting.shadow_cascades = 9;
        expect!(seen, ponko_ctx_set_lighting_ex(ctx, &bad), PONKO_INVALID_ARGUMENT);
        let mut after: PonkoLightingEx = header(PONKO_LIGHTING_EX_VERSION);
        expect!(seen, ponko_ctx_lighting_ex(ctx, &mut after), PONKO_OK);
        seen.sun_size_after_bad = after.shadow_sun_size;
        seen.intensity_after_bad = after.lighting.sun_intensity;

        // Materials: with and without a stencil, updated, refused when wrong.
        let mut d: PonkoMaterialDescEx = header(PONKO_MATERIAL_DESC_EX_VERSION);
        expect!(seen, ponko_material_desc_ex_defaults(&mut d), PONKO_OK);
        d.has_stencil = true;
        d.stencil = ponko_stencil_mark(3);
        let mut m = PonkoMaterial::default();
        expect!(seen, ponko_ctx_create_material_ex(ctx, &d, &mut m), PONKO_OK);
        expect!(
            seen,
            ponko_ctx_handle_is_live(
                ctx,
                PonkoHandleKind::PONKO_HANDLE_MATERIAL as u32,
                m.id,
                &mut seen.stencil_material_live
            ),
            PONKO_OK
        );
        d.has_stencil = false;
        d.desc.roughness = 0.2;
        expect!(seen, ponko_ctx_update_material_ex(ctx, m, &d), PONKO_OK);
        d.has_stencil = true;
        d.stencil.front.compare = 8;
        expect!(seen, ponko_ctx_update_material_ex(ctx, m, &d), PONKO_INVALID_ARGUMENT);
        expect!(
            seen,
            ponko_ctx_create_material_ex(ctx, &d, null_mut()),
            PONKO_INVALID_ARGUMENT
        );
        // Version 2: the model's colour masked by the vertex colour's alpha.
        // A version 1 descriptor (shorter) still makes a material.
        d.has_stencil = false;
        d.masked_tint = true;
        let mut masked = PonkoMaterial::default();
        expect!(seen, ponko_ctx_create_material_ex(ctx, &d, &mut masked), PONKO_OK);
        expect!(seen, ponko_ctx_update_material_ex(ctx, masked, &d), PONKO_OK);
        let mut v1 = d;
        (v1.version, v1.struct_size) = (1, offset_of!(PonkoMaterialDescEx, masked_tint) as u32);
        expect!(seen, ponko_ctx_create_material_ex(ctx, &v1, &mut masked), PONKO_OK);
        d.masked_tint = false;
        // Version 3: unlit, and layers (refused out of range).
        d.has_stencil = false;
        d.unlit = true;
        d.layers.clearcoat = 1.0;
        d.layers.sheen_color = PonkoColor {
            r: 0.5,
            g: 0.2,
            b: 0.2,
            a: 1.0,
        };
        let mut layered = PonkoMaterial::default();
        expect!(seen, ponko_ctx_create_material_ex(ctx, &d, &mut layered), PONKO_OK);
        d.layers.ior = 0.5;
        expect!(
            seen,
            ponko_ctx_update_material_ex(ctx, layered, &d),
            PONKO_INVALID_ARGUMENT
        );
        d.layers.ior = 1.5;
        let foreign = PonkoMaterial { id: 9999 };
        expect!(
            seen,
            ponko_ctx_update_material_ex(ctx, foreign, &d),
            PONKO_INVALID_ARGUMENT
        );

        // A navmesh bake with versioned settings, null for the defaults.
        let sources = ponko_nav_sources_new();
        let floor = ponko_mesh_data_plane(20.0);
        let at = ponko_transform3d_at(PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 });
        expect!(seen, ponko_nav_sources_add_mesh(sources, floor, at), PONKO_OK);
        ponko_mesh_data_free(floor);
        let mut s: PonkoNavBakeSettingsEx = header(PONKO_NAV_BAKE_SETTINGS_EX_VERSION);
        expect!(seen, ponko_nav_bake_settings_ex_defaults(&mut s), PONKO_OK);
        s.settings.agent_radius = 0.6;
        expect!(seen, ponko_ctx_bake_navmesh_ex(ctx, sources, &s), PONKO_OK);
        expect!(seen, ponko_ctx_bake_navmesh_ex(ctx, sources, null()), PONKO_OK);
        s.settings.cell_size = 0.0;
        expect!(
            seen,
            ponko_ctx_bake_navmesh_ex(ctx, sources, &s),
            PONKO_INVALID_ARGUMENT
        );
        let unset: PonkoNavBakeSettingsEx = std::mem::zeroed();
        expect!(
            seen,
            ponko_ctx_bake_navmesh_ex(ctx, sources, &unset),
            PONKO_INVALID_ARGUMENT
        );
        let (mut p, mut found) = (PonkoVec3::default(), false);
        expect!(
            seen,
            ponko_ctx_navmesh_sample_position(ctx, PonkoVec3 { x: 1.0, y: 0.5, z: 1.0 }, 2.0, &mut p, &mut found),
            PONKO_OK
        );
        if !found {
            seen.failures.push("the versioned bake left no walkable floor".into());
        }
        ponko_nav_sources_free(sources);
    }
}

#[test]
fn lighting_materials_and_bakes_work_through_their_versioned_calls() {
    let mut seen = Seen::default();
    unsafe {
        let g = ponko_game_new(c"descriptors".as_ptr(), 64, 64);
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 0, Some(three_d), (&raw mut seen).cast(), &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    assert!(seen.failures.is_empty(), "{:#?}", seen.failures);
    assert_eq!(seen.sun_size_read, 2.0);
    assert!(seen.abi2_still_reads);
    assert_eq!(
        (seen.sun_size_after_bad, seen.intensity_after_bad),
        (2.0, 3.0),
        "a refused set changes nothing"
    );
    assert!(seen.stencil_material_live);
}

// ---- collision groups ---------------------------------------------------------

#[cfg(all(feature = "physics", feature = "physics3d"))]
const WALL: u32 = 1 << 1;
#[cfg(all(feature = "physics", feature = "physics3d"))]
const TERRAIN: u32 = 1 << 2;

#[cfg(all(feature = "physics", feature = "physics3d"))]
#[derive(Default)]
struct Groups {
    failures: Vec<String>,
    wall: u64,
    terrain: u64,
    wall3d: u64,
    terrain3d: u64,
    read_back: Option<PonkoColliderEx>,
    polygon_points: u32,
    hits2d: Vec<u64>,
    hits3d: Vec<u64>,
    overlaps: Vec<u64>,
}

#[cfg(all(feature = "physics", feature = "physics3d"))]
unsafe extern "C" fn spawn_groups(ctx: *mut PonkoCtx, user: *mut c_void) {
    let seen = unsafe { &mut *user.cast::<Groups>() };
    unsafe {
        let w = ponko_ctx_world(ctx);
        let (mut transform, mut body, mut transform3d, mut body3d, mut collider3d) = (0, 0, 0, 0, 0);
        expect!(seen, ponko_transform_component(w, &mut transform), PONKO_OK);
        expect!(seen, ponko_rigid_body_component(w, &mut body), PONKO_OK);
        expect!(seen, ponko_transform3d_component(w, &mut transform3d), PONKO_OK);
        expect!(seen, ponko_rigid_body3d_component(w, &mut body3d), PONKO_OK);
        expect!(seen, ponko_collider3d_component(w, &mut collider3d), PONKO_OK);
        let fixed = ponko_rigid_body(PonkoBodyKind::PONKO_BODY_FIXED as u32);
        let mut spawn2d = |x: f32| {
            let at = PonkoTransform2D {
                position: PonkoVec2 { x, y: 0.0 },
                ..Default::default()
            };
            let parts = [
                PonkoValue {
                    component: transform,
                    data: (&raw const at).cast(),
                    size: size_of::<PonkoTransform2D>(),
                },
                PonkoValue {
                    component: body,
                    data: (&raw const fixed).cast(),
                    size: size_of::<PonkoRigidBody>(),
                },
            ];
            let mut e = 0;
            expect!(seen, ponko_spawn(w, parts.as_ptr(), 2, &mut e), PONKO_OK);
            e
        };
        // A wall at x 100 and, behind it, a terrain post: each collider and
        // its groups set in one call.
        let wall = spawn2d(100.0);
        let terrain = spawn2d(300.0);
        let mut c: PonkoColliderEx = header(PONKO_COLLIDER_EX_VERSION);
        expect!(
            seen,
            ponko_collider_ex_from(ponko_collider_rect(PonkoVec2 { x: 20.0, y: 200.0 }), &mut c),
            PONKO_OK
        );
        c.groups = PonkoCollisionGroups {
            memberships: WALL,
            filter: u32::MAX,
        };
        expect!(seen, ponko_set_collider_ex(w, wall, &c), PONKO_OK);
        // The post is a polygon (a diamond), in the terrain group.
        expect!(
            seen,
            ponko_collider_ex_from(ponko_collider_circle(1.0), &mut c),
            PONKO_OK
        );
        c.collider.shape = PonkoShapeKind::PONKO_SHAPE_POLYGON as u32;
        c.point_count = 4;
        c.points[..4].copy_from_slice(&[
            PonkoVec2 { x: -10.0, y: 0.0 },
            PonkoVec2 { x: 0.0, y: -10.0 },
            PonkoVec2 { x: 10.0, y: 0.0 },
            PonkoVec2 { x: 0.0, y: 10.0 },
        ]);
        c.groups = PonkoCollisionGroups {
            memberships: TERRAIN,
            filter: u32::MAX,
        };
        expect!(seen, ponko_set_collider_ex(w, terrain, &c), PONKO_OK);
        // Points that don't fit the shape are refused.
        let mut bad = c;
        bad.point_count = 0;
        expect!(seen, ponko_set_collider_ex(w, terrain, &bad), PONKO_INVALID_ARGUMENT);
        let mut bad = c;
        bad.point_count = 9;
        expect!(seen, ponko_set_collider_ex(w, terrain, &bad), PONKO_INVALID_ARGUMENT);
        // Read back in one call.
        let mut back: PonkoColliderEx = header(PONKO_COLLIDER_EX_VERSION);
        expect!(seen, ponko_get_collider_ex(w, terrain, &mut back), PONKO_OK);
        seen.polygon_points = back.point_count;
        expect!(seen, ponko_get_collider_ex(w, wall, &mut back), PONKO_OK);
        seen.read_back = Some(back);
        let mut none = 0;
        expect!(seen, ponko_spawn(w, null(), 0, &mut none), PONKO_OK);
        expect!(seen, ponko_get_collider_ex(w, none, &mut back), PONKO_NOT_FOUND);
        (seen.wall, seen.terrain) = (wall, terrain);

        // The same in 3D: the collider has its groups already.
        let fixed3d = ponko_rigid_body3d(PonkoBodyKind::PONKO_BODY_FIXED as u32);
        let mut spawn3d = |x: f32, groups: u32| {
            let at = ponko_transform3d_at(PonkoVec3 { x, y: 0.0, z: 0.0 });
            let mut shape = ponko_collider3d_sphere(1.0);
            shape.groups = PonkoCollisionGroups {
                memberships: groups,
                filter: u32::MAX,
            };
            let parts = [
                PonkoValue {
                    component: transform3d,
                    data: (&raw const at).cast(),
                    size: size_of::<PonkoTransform3D>(),
                },
                PonkoValue {
                    component: body3d,
                    data: (&raw const fixed3d).cast(),
                    size: size_of::<PonkoRigidBody3D>(),
                },
                PonkoValue {
                    component: collider3d,
                    data: (&raw const shape).cast(),
                    size: size_of::<PonkoCollider3D>(),
                },
            ];
            let mut e = 0;
            expect!(seen, ponko_spawn(w, parts.as_ptr(), 3, &mut e), PONKO_OK);
            e
        };
        seen.wall3d = spawn3d(10.0, WALL);
        seen.terrain3d = spawn3d(30.0, TERRAIN);
    }
}

#[cfg(all(feature = "physics", feature = "physics3d"))]
unsafe extern "C" fn ask_groups(ctx: *mut PonkoCtx, user: *mut c_void) {
    let seen = unsafe { &mut *user.cast::<Groups>() };
    if unsafe { ponko_ctx_sim_tick(ctx) } != 1 {
        return;
    }
    unsafe {
        let mut f: PonkoQueryFilterEx = header(PONKO_QUERY_FILTER_EX_VERSION);
        expect!(seen, ponko_query_filter_ex_defaults(&mut f), PONKO_OK);
        let mut terrain_only = f;
        terrain_only.groups = PonkoCollisionGroups {
            memberships: u32::MAX,
            filter: TERRAIN,
        };
        let o2 = PonkoVec2 { x: 0.0, y: 0.0 };
        let right2 = PonkoVec2 { x: 1.0, y: 0.0 };
        for filter in [null(), &raw const f, &raw const terrain_only] {
            let (mut found, mut hit) = (false, PonkoRayHit::default());
            expect!(
                seen,
                ponko_ctx_cast_ray2d_ex(ctx, o2, right2, 1000.0, filter, &mut found, &mut hit),
                PONKO_OK
            );
            seen.hits2d.push(if found { ponko_entity_bits(hit.entity) } else { 0 });
        }
        let o3 = PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 };
        let right3 = PonkoVec3 { x: 1.0, y: 0.0, z: 0.0 };
        for filter in [null(), &raw const f, &raw const terrain_only] {
            let (mut found, mut hit) = (false, PonkoHit3D::default());
            expect!(
                seen,
                ponko_ctx_cast_ray3d_ex(ctx, o3, right3, 1000.0, filter, &mut found, &mut hit),
                PONKO_OK
            );
            seen.hits3d.push(if found { ponko_entity_bits(hit.entity) } else { 0 });
        }
        // An overlap seeing only the wall group, and one looking through the wall.
        let mut walls = f;
        walls.groups.filter = WALL;
        let mut through = f;
        (through.exclude, through.exclude_entity) = (true, seen.wall3d);
        for filter in [&raw const walls, &raw const through] {
            let mut buf = [PonkoEntity::default(); 4];
            let mut n = 0;
            expect!(
                seen,
                ponko_ctx_overlap_sphere3d_ex(
                    ctx,
                    PonkoVec3 {
                        x: 20.0,
                        y: 0.0,
                        z: 0.0
                    },
                    15.0,
                    filter,
                    buf.as_mut_ptr(),
                    4,
                    &mut n
                ),
                PONKO_OK
            );
            let mut got: Vec<u64> = buf[..n].iter().map(|e| ponko_entity_bits(*e)).collect();
            got.sort_unstable();
            seen.overlaps.extend(got);
            seen.overlaps.push(0);
        }
        // Every other versioned query answers through the same filter: the
        // terrain-only filter never reports the wall.
        let tf = &raw const terrain_only;
        let q = PonkoQuat {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
        let mut found2 = Vec::new();
        let mut found3 = Vec::new();
        let (mut found, mut hit2, mut hit3) = (false, PonkoRayHit::default(), PonkoHit3D::default());
        let mut point2 = std::mem::zeroed::<PonkoPointHit2D>();
        let mut point3 = std::mem::zeroed::<PonkoPointHit3D>();
        let mut hits2 = [PonkoRayHit::default(); 4];
        let mut hits3 = [PonkoHit3D::default(); 4];
        let mut ents = [PonkoEntity::default(); 4];
        let mut pens2 = [std::mem::zeroed::<PonkoPenetration2D>(); 4];
        let mut pens3 = [std::mem::zeroed::<PonkoPenetration3D>(); 4];
        let mut n = 0;
        let v = |x: f32| PonkoVec2 { x, y: 0.0 };
        let w3 = |x: f32| PonkoVec3 { x, y: 0.0, z: 0.0 };
        let statuses = [
            ponko_ctx_cast_ray2d_all_ex(ctx, o2, right2, 1000.0, tf, hits2.as_mut_ptr(), 4, &mut n),
            ponko_ctx_cast_circle2d_ex(ctx, o2, 1.0, right2, 1000.0, tf, &mut found, &mut hit2),
            ponko_ctx_cast_box2d_ex(ctx, o2, v(2.0), 0.0, right2, 1000.0, tf, &mut found, &mut hit2),
            ponko_ctx_cast_capsule2d_ex(ctx, o2, 2.0, 1.0, right2, 1000.0, tf, &mut found, &mut hit2),
            ponko_ctx_overlap_point2d_ex(ctx, v(300.0), tf, ents.as_mut_ptr(), 4, &mut n),
            ponko_ctx_overlap_circle2d_ex(ctx, v(100.0), 5.0, tf, ents.as_mut_ptr(), 4, &mut n),
            ponko_ctx_overlap_box2d_ex(ctx, v(100.0), v(5.0), 0.0, tf, ents.as_mut_ptr(), 4, &mut n),
            ponko_ctx_overlap_capsule2d_ex(ctx, v(100.0), 2.0, 2.0, tf, ents.as_mut_ptr(), 4, &mut n),
            ponko_ctx_closest_point2d_ex(ctx, v(150.0), 1000.0, tf, &mut found, &mut point2),
            ponko_ctx_penetrations2d_ex(
                ctx,
                ponko_collider_circle(5.0),
                v(100.0),
                0.0,
                tf,
                pens2.as_mut_ptr(),
                4,
                &mut n,
            ),
            ponko_ctx_cast_ray3d_all_ex(ctx, o3, right3, 1000.0, tf, hits3.as_mut_ptr(), 4, &mut n),
            ponko_ctx_cast_sphere3d_ex(ctx, o3, 0.5, right3, 1000.0, tf, &mut found, &mut hit3),
            ponko_ctx_cast_capsule3d_ex(ctx, o3, 1.0, 0.5, right3, 1000.0, tf, &mut found, &mut hit3),
            ponko_ctx_cast_box3d_ex(ctx, o3, w3(1.0), q, right3, 1000.0, tf, &mut found, &mut hit3),
            ponko_ctx_overlap_point3d_ex(ctx, w3(30.0), tf, ents.as_mut_ptr(), 4, &mut n),
            ponko_ctx_overlap_box3d_ex(ctx, w3(10.0), w3(1.0), q, tf, ents.as_mut_ptr(), 4, &mut n),
            ponko_ctx_overlap_capsule3d_ex(ctx, w3(10.0), 1.0, 1.0, tf, ents.as_mut_ptr(), 4, &mut n),
            ponko_ctx_closest_point3d_ex(ctx, w3(20.0), 1000.0, tf, &mut found, &mut point3),
            ponko_ctx_penetrations3d_ex(
                ctx,
                ponko_collider3d_sphere(1.0),
                w3(10.0),
                q,
                tf,
                pens3.as_mut_ptr(),
                4,
                &mut n,
            ),
        ];
        for (i, status) in statuses.into_iter().enumerate() {
            if status != PONKO_OK {
                seen.failures
                    .push(format!("versioned query {i} gave {status:?}: {}", last_error()));
            }
        }
        // The answers that name an entity name the terrain, never the wall.
        let (mut found_a, mut hit_a) = (false, PonkoRayHit::default());
        ponko_ctx_cast_circle2d_ex(ctx, o2, 1.0, right2, 1000.0, tf, &mut found_a, &mut hit_a);
        found2.push(found_a.then(|| ponko_entity_bits(hit_a.entity)));
        ponko_ctx_closest_point2d_ex(ctx, v(150.0), 1000.0, tf, &mut found_a, &mut point2);
        found2.push(found_a.then(|| ponko_entity_bits(point2.entity)));
        let (mut found_b, mut hit_b) = (false, PonkoHit3D::default());
        ponko_ctx_cast_sphere3d_ex(ctx, o3, 0.5, right3, 1000.0, tf, &mut found_b, &mut hit_b);
        found3.push(found_b.then(|| ponko_entity_bits(hit_b.entity)));
        ponko_ctx_closest_point3d_ex(ctx, w3(20.0), 1000.0, tf, &mut found_b, &mut point3);
        found3.push(found_b.then(|| ponko_entity_bits(point3.entity)));
        if found2 != [Some(seen.terrain); 2] || found3 != [Some(seen.terrain3d); 2] {
            seen.failures
                .push(format!("terrain-only queries saw {found2:?} / {found3:?}"));
        }
        // A filter with a bad header is refused.
        let unset: PonkoQueryFilterEx = std::mem::zeroed();
        let (mut found, mut hit) = (false, PonkoRayHit::default());
        expect!(
            seen,
            ponko_ctx_cast_ray2d_ex(ctx, o2, right2, 1000.0, &unset, &mut found, &mut hit),
            PONKO_INVALID_ARGUMENT
        );
    }
}

#[cfg(all(feature = "physics", feature = "physics3d"))]
#[test]
fn collision_groups_reach_colliders_and_queries_through_versioned_descriptors() {
    let mut seen = Groups::default();
    unsafe {
        let g = ponko_game_new(c"groups".as_ptr(), 1, 1);
        let user: *mut c_void = (&raw mut seen).cast();
        assert_eq!(ponko_game_physics(g, PonkoVec2::default()), PONKO_OK);
        assert_eq!(ponko_game_physics3d(g, PonkoVec3::default()), PONKO_OK);
        assert_eq!(
            ponko_game_sim_system(g, c"ask".as_ptr(), 1, Some(ask_groups), user),
            PONKO_OK
        );
        let mut world = null_mut();
        assert_eq!(
            ponko_game_run_headless(g, 2, Some(spawn_groups), user, &mut world),
            PONKO_OK
        );
        ponko_world_free(world);
    }
    assert!(seen.failures.is_empty(), "{:#?}", seen.failures);
    let back = seen.read_back.expect("read back");
    assert_eq!(
        (back.groups.memberships, back.collider.shape),
        (WALL, PonkoShapeKind::PONKO_SHAPE_RECT as u32)
    );
    assert_eq!(seen.polygon_points, 4);
    // Default filters see the wall first; a terrain-only filter sees past it.
    assert_eq!(seen.hits2d, [seen.wall, seen.wall, seen.terrain]);
    assert_eq!(seen.hits3d, [seen.wall3d, seen.wall3d, seen.terrain3d]);
    assert_eq!(
        seen.overlaps,
        [seen.wall3d, 0, seen.terrain3d, 0],
        "wall-only, then everything but the wall"
    );
}
