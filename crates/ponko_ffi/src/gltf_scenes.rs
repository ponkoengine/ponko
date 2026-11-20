//! glTF files as scenes: every node an entity, with what its custom
//! properties (`ponko:` keys in the node's extras) ask for. See
//! `GltfRoot::spawn` in Rust and the guide's "From Blender to the game".
use std::ffi::c_char;

use ponko::d3::{GltfNode, GltfOptions, GltfRoot, GltfScene, SpawnPoint};
use ponko_ecs::Entity;

use crate::ctx::{PonkoCtx, ctx_access, out, text};
use crate::three::{PonkoTransform3D, transform_from_c};
use crate::{PonkoStatus, PonkoWorld, access, boundary, engine_error, error, invalid, write_text};

/// Spawns the glTF file at `path` as a scene placed at `at`: a root entity
/// and an entity for every node, attached to its parent, with its model,
/// light and what its custom properties ask for (components by name,
/// colliders, spawn points, prefabs, levels of detail, layers). With
/// `without_saved_objects`, the prefab nodes are left out (a world save
/// holds them). Exporting the file again with hot reload on updates it in
/// place. Errors name the file and the node.
/// ```
/// let at = ponko_ffi::ponko_transform3d_at(ponko_ffi::PonkoVec3 { x: 0.0, y: 0.0, z: 0.0 });
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_spawn_gltf(std::ptr::null_mut(), c"a.glb".as_ptr(), at, false, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// Live ctx; `path` NUL-terminated; `out_root` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_spawn_gltf(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    at: PonkoTransform3D,
    without_saved_objects: bool,
    out_root: *mut u64,
) -> PonkoStatus {
    boundary(|| {
        if out_root.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let path = text(path)?;
            let mut options = GltfOptions::default();
            if without_saved_objects {
                options = options.without_saved_objects();
            }
            ctx_access(ctx, |c| {
                let root = GltfRoot::spawn(c, path, transform_from_c(at), options).map_err(engine_error)?;
                out(out_root, root.to_bits())
            })
        }
    })
}

/// Despawns a scene spawned with `ponko_ctx_spawn_gltf`: its root, every
/// node's entity and the meshes made for it. `PONKO_NOT_FOUND` if `root`
/// isn't such a root.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_despawn_gltf(std::ptr::null_mut(), 0) } as u32, 1);
/// ```
/// # Safety
/// Live ctx.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_despawn_gltf(ctx: *mut PonkoCtx, root: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let root = Entity::from_bits(root);
            if c.world.has::<GltfRoot>(root) {
                c.despawn_prefab(root);
                Ok(())
            } else {
                Err(error(
                    PonkoStatus::PONKO_NOT_FOUND,
                    "not the root of a spawned glTF scene",
                ))
            }
        })
    })
}

/// The entity of the node at `path` (names from the top of the file down,
/// joined with `/`) or of the first node named `path`, in the scene whose
/// root is `root`. `PONKO_NOT_FOUND` if there is none.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_gltf_node_entity(std::ptr::null_mut(), 0, c"Door".as_ptr(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `world` null or live; `path` NUL-terminated; `out_entity` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_gltf_node_entity(
    world: *mut PonkoWorld,
    root: u64,
    path: *const c_char,
    out_entity: *mut u64,
) -> PonkoStatus {
    boundary(|| {
        if out_entity.is_null() {
            return Err(invalid("output is null"));
        }
        // SAFETY: forwarded caller contract.
        unsafe {
            let path = text(path)?;
            access(world, |w, _| {
                let scene = w
                    .get::<GltfRoot>(Entity::from_bits(root))
                    .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "not the root of a spawned glTF scene"))?;
                let e = scene
                    .entity(path)
                    .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no node at that path in the scene"))?;
                out(out_entity, e.to_bits())
            })
        }
    })
}

/// The custom properties of the glTF node `entity` was spawned from, as
/// a JSON object (the game's own keys too). Writes its length to `out_len`
/// and the text to `buffer` when it fits. `PONKO_NOT_FOUND` if `entity`
/// isn't a node's.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_gltf_node_extras(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `world` null or live; `buffer` holds `capacity` bytes (or null with 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_gltf_node_extras(
    world: *mut PonkoWorld,
    entity: u64,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        access(world, |w, _| {
            let node = w
                .get::<GltfNode>(Entity::from_bits(entity))
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "entity is not a glTF node"))?;
            let json = serde_json::Value::Object(node.extras.clone()).to_string();
            write_text(buffer, capacity, out_len, &json)
        })
    })
}

/// The name of the spawn point `entity` (a node with `ponko:spawn`), as
/// `ponko_gltf_node_extras`. `PONKO_NOT_FOUND` if it isn't one.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_spawn_point_name(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// As `ponko_gltf_node_extras`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_spawn_point_name(
    world: *mut PonkoWorld,
    entity: u64,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        access(world, |w, _| {
            let point = w
                .get::<SpawnPoint>(Entity::from_bits(entity))
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "entity is not a spawn point"))?;
            write_text(buffer, capacity, out_len, &point.name)
        })
    })
}

/// Reads the glTF file at `path` node by node and checks its custom
/// properties against the game's reflected components, without spawning
/// anything (`GltfScene::load`): for tools, or to place things your own
/// way. Writes it as a JSON object, as `ponko_gltf_node_extras` writes
/// text: `"nodes"`, parents before their children, each with its
/// `"name"`, `"path"`, `"parent"` (an index into the list, or null),
/// `"transform"` (in its parent's space) and `"world"` (in the file's
/// space) as `{"position", "rotation", "scale"}`, `"extras"` (every
/// custom property), `"mesh"` (`{"corners", "triangles"}`, or null) and
/// `"light"` (whether it has one); and `"spawn_points"`, each
/// `{"name", "transform"}` in the file's space. Errors name the file and
/// the node, as `ponko_ctx_spawn_gltf`'s do.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_gltf_scene_load(std::ptr::null_mut(), c"a.glb".as_ptr(), std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// Live ctx; `path` NUL-terminated; `buffer` holds `capacity` bytes (or null with 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_gltf_scene_load(
    ctx: *mut PonkoCtx,
    path: *const c_char,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_len.is_null() {
            return Err(invalid("output is null"));
        }
        let path = text(path)?;
        ctx_access(ctx, |c| {
            let scene = GltfScene::load(c, path).map_err(engine_error)?;
            write_text(buffer, capacity, out_len, &scene_json(&scene).to_string())
        })
    })
}

/// The JSON Schema of the custom properties the game reads
/// (`gltf_properties_schema`): the `ponko:` keys and every reflected
/// component, as `ponko schema` writes it, for an editor or a script to
/// check properties before exporting. Written as `ponko_gltf_node_extras`
/// writes text.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_gltf_properties_schema(std::ptr::null_mut(), std::ptr::null_mut(), 0, std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `world` null or live; `buffer` holds `capacity` bytes (or null with 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_gltf_properties_schema(
    world: *mut PonkoWorld,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        access(world, |w, _| {
            let schema = ponko::d3::gltf_properties_schema(w).to_string();
            write_text(buffer, capacity, out_len, &schema)
        })
    })
}

/// A `GltfScene` as the JSON `ponko_gltf_scene_load` writes.
fn scene_json(scene: &GltfScene) -> serde_json::Value {
    use serde_json::json;
    let place = |t: &ponko::d3::Transform| {
        json!({
            "position": t.position.to_array(),
            "rotation": t.rotation.to_array(),
            "scale": t.scale.to_array(),
        })
    };
    let nodes: Vec<serde_json::Value> = scene
        .nodes
        .iter()
        .map(|n| {
            json!({
                "name": n.name,
                "path": n.path,
                "parent": n.parent,
                "transform": place(&n.transform),
                "world": place(&n.world),
                "extras": n.extras,
                "mesh": n.mesh.as_ref().map(|m| json!({ "corners": m.positions.len(), "triangles": m.indices.len() / 3 })),
                "light": n.light.is_some(),
            })
        })
        .collect();
    let spawn_points: Vec<serde_json::Value> = scene
        .spawn_points()
        .map(|(name, at)| json!({ "name": name, "transform": place(&at) }))
        .collect();
    json!({ "nodes": nodes, "spawn_points": spawn_points })
}
