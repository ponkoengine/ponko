// Ponko for C++17: a header-only wrapper over ponko.h.
//
// Mirrors the Rust API: ponko::game is Game, ponko::ctx is Ctx,
// ponko::world_view is World. Components are plain structs, found by their
// C++ type: world.spawn(Health{100}), world.get<Health>(e) and
// world.each([](ponko::transform2d& t, const Health& h) { ... }).
// Errors become exceptions; exceptions thrown by your systems and callbacks
// are caught before they reach the engine and rethrown from
// run()/run_headless()/each()/query().
#ifndef PONKO_HPP
#define PONKO_HPP
#include "ponko.h"
#include <algorithm>
#include <array>
#include <limits>
#include <atomic>
#include <climits>
#include <cmath>
#include <cstdlib>
#include <cstring>
#include <cstddef>
#include <cstdint>
#include <exception>
#include <fstream>
#include <functional>
#include <new>
#include <sstream>
#include <memory>
#include <mutex>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <initializer_list>
#include <limits>
#include <tuple>
#include <type_traits>
#include <typeindex>
#include <unordered_map>
#include <utility>
#include <vector>

// ---- math on the C types (global, so it works without `using namespace`) ----
/// Component-wise arithmetic on the C vector types (global, so it works without `using namespace ponko`);
/// `==` compares exactly, with no tolerance.
///@{
inline PonkoVec2 operator+(PonkoVec2 a, PonkoVec2 b) { return {a.x + b.x, a.y + b.y}; }
inline PonkoVec2 operator-(PonkoVec2 a, PonkoVec2 b) { return {a.x - b.x, a.y - b.y}; }
inline PonkoVec2 operator-(PonkoVec2 a) { return {-a.x, -a.y}; }
inline PonkoVec2 operator*(PonkoVec2 a, float s) { return {a.x * s, a.y * s}; }
inline PonkoVec2 operator*(float s, PonkoVec2 a) { return {a.x * s, a.y * s}; }
inline PonkoVec2 operator/(PonkoVec2 a, float s) { return {a.x / s, a.y / s}; }
inline PonkoVec2& operator+=(PonkoVec2& a, PonkoVec2 b) { a.x += b.x; a.y += b.y; return a; }
inline PonkoVec2& operator-=(PonkoVec2& a, PonkoVec2 b) { a.x -= b.x; a.y -= b.y; return a; }
inline PonkoVec2& operator*=(PonkoVec2& a, float s) { a.x *= s; a.y *= s; return a; }
inline bool operator==(PonkoVec2 a, PonkoVec2 b) { return a.x == b.x && a.y == b.y; }
inline bool operator!=(PonkoVec2 a, PonkoVec2 b) { return !(a == b); }
inline PonkoVec3 operator+(PonkoVec3 a, PonkoVec3 b) { return {a.x + b.x, a.y + b.y, a.z + b.z}; }
inline PonkoVec3 operator-(PonkoVec3 a, PonkoVec3 b) { return {a.x - b.x, a.y - b.y, a.z - b.z}; }
inline PonkoVec3 operator-(PonkoVec3 a) { return {-a.x, -a.y, -a.z}; }
inline PonkoVec3 operator*(PonkoVec3 a, float s) { return {a.x * s, a.y * s, a.z * s}; }
inline PonkoVec3 operator*(float s, PonkoVec3 a) { return {a.x * s, a.y * s, a.z * s}; }
inline PonkoVec3 operator/(PonkoVec3 a, float s) { return {a.x / s, a.y / s, a.z / s}; }
inline PonkoVec3& operator+=(PonkoVec3& a, PonkoVec3 b) { a.x += b.x; a.y += b.y; a.z += b.z; return a; }
inline PonkoVec3& operator-=(PonkoVec3& a, PonkoVec3 b) { a.x -= b.x; a.y -= b.y; a.z -= b.z; return a; }
inline PonkoVec3& operator*=(PonkoVec3& a, float s) { a.x *= s; a.y *= s; a.z *= s; return a; }
inline bool operator==(PonkoVec3 a, PonkoVec3 b) { return a.x == b.x && a.y == b.y && a.z == b.z; }
inline bool operator!=(PonkoVec3 a, PonkoVec3 b) { return !(a == b); }
///@}
/// `a` after `b`: rotating by the result is rotating by `b`, then by `a`.
inline PonkoQuat operator*(PonkoQuat a, PonkoQuat b) {
    return {a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y, a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
            a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w, a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z};
}
/// Rotates `a` further by `b` first (`a = a * b`).
inline PonkoQuat& operator*=(PonkoQuat& a, PonkoQuat b) { a = a * b; return a; }
/// `v` rotated by `q`.
inline PonkoVec3 operator*(PonkoQuat q, PonkoVec3 v) {
    const PonkoVec3 u{q.x, q.y, q.z};
    const PonkoVec3 t{2 * (u.y * v.z - u.z * v.y), 2 * (u.z * v.x - u.x * v.z), 2 * (u.x * v.y - u.y * v.x)};
    return v + q.w * t + PonkoVec3{u.y * t.z - u.z * t.y, u.z * t.x - u.x * t.z, u.x * t.y - u.y * t.x};
}

namespace ponko {
/// An entity id (Rust `Entity::to_bits`): generation in the high 32 bits, index in the low 32. Ids of
/// despawned entities stay dead; never make one up.
using entity = std::uint64_t;
/// A number for a code mod's function (ctx::call_mod).
inline PonkoModValue mod_i32(std::int32_t v) { return PonkoModValue{PONKO_MOD_I32, v, 0.0}; }
/// An i64 for a code mod's function: an entity, for example.
inline PonkoModValue mod_i64(std::int64_t v) { return PonkoModValue{PONKO_MOD_I64, v, 0.0}; }
/// An f32 for a code mod's function.
inline PonkoModValue mod_f32(float v) { return PonkoModValue{PONKO_MOD_F32, 0, v}; }
/// An f64 for a code mod's function.
inline PonkoModValue mod_f64(double v) { return PonkoModValue{PONKO_MOD_F64, 0, v}; }
/// A 2D vector (Rust `Vec2`); in 2D worlds +Y points down the screen.
using vec2 = PonkoVec2;
/// An RGBA color, each channel 0..1 (Rust `Color`); see rgb, rgba and `colors`.
using color = PonkoColor;
/// A texture handle (Rust `Texture`) from ctx::load_texture or ctx::create_texture: an id plus its size in
/// pixels. Copyable; the engine owns the pixels. Never make one up.
using texture = PonkoTexture;
/// A render target (Rust `RenderTarget`) from ctx::create_render_target: a texture the GPU draws into
/// (target cameras, compute passes, target effects, UI panels). It is its own type, so a texture from
/// load_texture doesn't compile where a render target is wanted. It is still a texture, with the same
/// layout, and converts to one, so materials and draws read it as it is. ctx::render_target_of turns a
/// target read back from the engine (a target_camera's `target`) into one again, checked.
struct render_target : PonkoTexture {
    /// The target as a texture (it also converts implicitly).
    constexpr ponko::texture texture() const noexcept { return {id, width, height}; }
};
static_assert(sizeof(render_target) == sizeof(texture) && alignof(render_target) == alignof(texture),
              "a render_target is a texture and nothing more");
/// The built-in `Transform2D` component: position (pixels), rotation (radians, clockwise) and scale. Same
/// layout as Rust, so it is a query column.
using transform2d = PonkoTransform2D;
/// The built-in `Attached` component (Rust `Attached`): keeps an entity at `offset` from another; see
/// attach_to. Same layout as Rust, so it is a query column.
using attached = PonkoAttached;
/// The built-in `Sprite` component (Rust `Sprite`): a texture or a region of one, drawn at the entity's
/// transform2d. Get and set it like any component; it can't be a query column (the engine stores it in a Rust
/// layout).
using sprite = PonkoSprite;
// ---- 2D lights (Rust `d2::PointLight`, `d2::SpotLight`, `d2::DirectionalLight`, `d2::Occluder`).
/// The built-in `PointLight2D` component: light from the entity's 2D transform, fading to nothing at
/// `radius` pixels; `shadows` makes occluders in reach cast shadows. Start from point_light2d_default().
/// Lighting turns on with the first light or an ambient light (ctx::set_ambient_light2d).
using point_light2d = PonkoPointLight2D;
/// A point light with the engine's defaults (Rust `d2::PointLight::default()`).
inline point_light2d point_light2d_default() { return ponko_point_light2d_default(); }
/// The built-in `SpotLight2D` component: a cone of light along the entity's rotation, `angle` radians to
/// each side, its edge softened by `softness`. Start from spot_light2d_default().
using spot_light2d = PonkoSpotLight2D;
/// A spot light with the engine's defaults (Rust `d2::SpotLight::default()`).
inline spot_light2d spot_light2d_default() { return ponko_spot_light2d_default(); }
/// The built-in `DirectionalLight2D` component: light from far away (a sun or moon) shining along
/// `direction` over the whole view; needs no transform. Start from directional_light2d_default().
using directional_light2d = PonkoDirectionalLight2D;
/// A directional light with the engine's defaults (Rust `d2::DirectionalLight::default()`).
inline directional_light2d directional_light2d_default() { return ponko_directional_light2d_default(); }
// ---- Sprite blend modes and sprite materials (Rust `BlendMode`, `SpriteShader`, `SpriteMaterial`).
/// How a sprite combines with what's behind it (Rust `BlendMode`). Also a component: an entity with a sprite
/// and one blends that way. Each mode is its own batch.
enum class sprite_blend : std::uint32_t {
    /// Covers what's behind by the pixel's alpha (the default).
    alpha = PONKO_BLEND_ALPHA,
    /// Adds the color, weighted by alpha: glows, fire, sparks, light. Never darkens.
    additive = PONKO_BLEND_ADDITIVE,
    /// Multiplies what's behind by the color: shadows, tinted glass. White leaves it unchanged.
    multiply = PONKO_BLEND_MULTIPLY,
    /// Like alpha, for colors already multiplied by their alpha.
    premultiplied = PONKO_BLEND_PREMULTIPLIED,
};
/// A sprite shader: WGSL with a `fn fragment(p: SpritePixel) -> vec4<f32>` hook (ctx::load_sprite_shader,
/// ctx::create_sprite_shader). A handle; free it with ctx::free.
using sprite_shader = PonkoSpriteShader;
/// A look for sprites (Rust `SpriteMaterial`): a sprite shader, its params and extra textures, from
/// ctx::create_sprite_material. Also a component: an entity with a sprite and one draws with it. A handle;
/// free it with ctx::free.
using sprite_material = PonkoSpriteMaterial;
/// What a sprite material is made of (Rust `SpriteMaterialDesc`): the shader (if `has_shader`), 16 floats
/// the shader reads as `material.params[0..4]`, and two extra textures. Start from sprite_material_of.
using sprite_material_desc = PonkoSpriteMaterialDesc;
/// A sprite material drawn with `shader`: params zero, no extra textures.
inline sprite_material_desc sprite_material_of(sprite_shader shader) {
    sprite_material_desc d = ponko_sprite_material_desc_default();
    d.has_shader = true;
    d.shader = shader;
    return d;
}
/// Sets `material.params[index]` (0 to 3) of `d`; throws std::out_of_range for a bigger index.
inline sprite_material_desc& set_param(sprite_material_desc& d, std::size_t index, float x, float y, float z, float w) {
    if (index > 3) throw std::out_of_range("a sprite material has params 0 to 3");
    float* p = d.params + index * 4;
    p[0] = x; p[1] = y; p[2] = z; p[3] = w;
    return d;
}
/// Sets extra texture `index` (0 or 1, `material_sample0`/`1` in the shader) of `d`; throws
/// std::out_of_range for a bigger index.
inline sprite_material_desc& set_texture(sprite_material_desc& d, std::size_t index, PonkoTexture t) {
    if (index > 1) throw std::out_of_range("a sprite material has textures 0 and 1");
    d.has_textures[index] = true;
    d.textures[index] = t;
    return d;
}
// ---- end of sprite blend modes and sprite materials
/// The 2D camera (Rust `Camera`): which world point is at the window's top-left, and the zoom (window pixels
/// per world unit).
using camera = PonkoCamera;
/// A point or direction in 3D (Rust `Vec3`): right-handed, +Y up, meters.
using vec3 = PonkoVec3;
/// A rotation (unit quaternion, Rust `Quat`). `{0, 0, 0, 1}` is none (quat_identity()); multiply with `*`.
using quat = PonkoQuat;
/// The built-in `Transform3D` component (Rust `Transform3D`): position, rotation and scale. A plain value and
/// a query column; start from transform3d_at.
using transform3d = PonkoTransform3D;
/// The built-in `Attached3D` component (Rust `attach::Attached3D`): keeps an entity at `offset` (a transform in the
/// other's space) from another; see attach3d_to. Same layout as Rust, so it is a query column.
using attached3d = PonkoAttached3D;
/// One joint of a skeleton (Rust `Joint`): its name, the joint it hangs
/// from (an earlier index; -1 for a root) and where it sits relative to it
/// at rest.
struct joint {
    std::string name;
    int parent = -1;
    transform3d rest{{0, 0, 0, 1}, {0, 0, 0}, {1, 1, 1}, {0, 0}};
};
/// The built-in `Model` component (Rust `Model`): draws `mesh` where the entity's transform3d is, its
/// materials' colors times `color`. A plain value and a query column; start from model_of.
using model = PonkoModel;
/// No mesh (Rust `Mesh::NONE`): a model showing it draws nothing, and no
/// mesh is ever made with its id.
constexpr PonkoMesh mesh_none{PONKO_MESH_NONE};
/// No resource of handle type `H` (Rust `Handle::NONE`): never live, and
/// freeing it does nothing. `none<mesh>()` is `mesh_none`.
template<class H> constexpr H none() { H h{}; h.id = PONKO_HANDLE_NONE; return h; }
/// How many handles of each kind are live (Rust `LiveCounts`).
using live_counts = PonkoLiveCounts;
/// A class of asset handle in the inventory (Rust `AssetClass`).
enum class asset_class : std::uint32_t {
    texture = PONKO_ASSET_CLASS_TEXTURE, render_target = PONKO_ASSET_CLASS_RENDER_TARGET,
    texture_array = PONKO_ASSET_CLASS_TEXTURE_ARRAY, material = PONKO_ASSET_CLASS_MATERIAL,
    shader = PONKO_ASSET_CLASS_SHADER, mesh = PONKO_ASSET_CLASS_MESH, gpu_buffer = PONKO_ASSET_CLASS_GPU_BUFFER,
    effect = PONKO_ASSET_CLASS_EFFECT, compute = PONKO_ASSET_CLASS_COMPUTE, sound = PONKO_ASSET_CLASS_SOUND
};
/// Who made a handle (Rust `AssetOwner`), in `asset_record::owner`.
enum class asset_owner : std::uint32_t {
    code = PONKO_ASSET_OWNER_CODE, file = PONKO_ASSET_OWNER_FILE, engine = PONKO_ASSET_OWNER_ENGINE
};
/// Live handles of one class and their estimated bytes (Rust `AssetCount`).
using asset_count = PonkoAssetCount;
/// One live handle with its bytes, age and owner (Rust `AssetRecord`).
using asset_record = PonkoAssetRecord;
/// Models and triangles drawn for one mesh in the last frame.
using mesh_cost = PonkoMeshCost;
/// State, handle and current GPU resolution of a streamed asset.
using streamed_asset = PonkoStreamedAsset;
/// A kind of asset with its own residency budget (Rust `ResidencyKind`).
enum class residency_kind : std::uint32_t { mesh = PONKO_RESIDENCY_MESH, texture = PONKO_RESIDENCY_TEXTURE };
/// Where a streamed asset is (Rust `AssetState`).
enum class asset_state : std::uint32_t {
    unknown = PONKO_ASSET_UNKNOWN, loading = PONKO_ASSET_LOADING, uploading = PONKO_ASSET_UPLOADING,
    streaming = PONKO_ASSET_STREAMING, resident = PONKO_ASSET_RESIDENT, evicted = PONKO_ASSET_EVICTED,
    failed = PONKO_ASSET_FAILED,
};
/// A kind's residency budget and counters (Rust `ResidencyStats`).
using residency_stats = PonkoResidencyStats;
/// The upload queue's budget and counters (Rust `UploadStats`).
using upload_stats = PonkoUploadStats;
/// The upload queue's counters in the versioned form (PonkoUploadStatsEx), from ctx::upload_stats_ex:
/// `stats` is upload_stats, plus the game-thread decode slices and the GPU's own upload times.
using upload_stats_ex = PonkoUploadStatsEx;
/// Where an asset group is (Rust `GroupState`).
enum class group_state : std::uint32_t {
    loading = PONKO_GROUP_LOADING, ready = PONKO_GROUP_READY, failed = PONKO_GROUP_FAILED,
    cancelled = PONKO_GROUP_CANCELLED,
};
/// How far an asset group got: assets and bytes loaded (Rust `GroupProgress`).
using group_progress = PonkoAssetGroupProgress;
/// What the game thread's asset group step cost (Rust `GroupStats`).
using group_stats = PonkoAssetGroupStats;
/// One mesh of a LOD level, placed and tinted (Rust `LodRenderer`).
using lod_renderer = PonkoLodRenderer;
/// A LOD renderer drawing mesh `m` with no offset and no tint (Rust `LodRenderer::new`).
inline lod_renderer lod_renderer_of(PonkoMesh m) { return ponko_lod_renderer(m); }
/// One level of a LOD group (Rust `LodLevel`): its renderers are drawn while
/// the group covers at least `screen_height` of the screen's height. With
/// `custom_shadow`, `shadow_casters` cast the shadow instead (none: no shadow).
struct lod_level {
    float screen_height = 0;
    float fade_width = 0;
    std::vector<lod_renderer> renderers;
    bool custom_shadow = false;
    std::vector<lod_renderer> shadow_casters;
};
/// Global LOD settings (Rust `ctx.scene3d.lod`): `bias` scales every group's screen height.
using lod_settings = PonkoLodSettings;
/// Which level a group shows: kind 0 culled, 1 `level`, 2 fading from `level` to `to` (-1: nothing).
using lod_pick = PonkoLodPick;
/// The fraction of the screen's height something `size` tall covers at `distance`.
inline float lod_screen_height_at(float size, float distance, float fov_y) { return ponko_lod_screen_height_at(size, distance, fov_y); }
/// The distance at which something `size` tall covers `height` of the screen.
inline float lod_distance_at(float size, float height, float fov_y) { return ponko_lod_distance_at(size, height, fov_y); }
/// The built-in `PointLight3D` component (Rust `PointLight3D`): light from the entity's position, fading to
/// nothing at `range` meters (transform scale ignored).
using point_light3d = PonkoPointLight3D;
/// The built-in `ReflectionProbe3D` component (Rust `ReflectionProbe3D`): an axis-aligned box whose
/// reflections models inside it use; empty content falls back to the sky. Start from
/// reflection_probe3d_default().
using reflection_probe3d = PonkoReflectionProbe3D;
/// A reflection probe with the engine's defaults (Rust `ReflectionProbe3D::default()`).
inline reflection_probe3d reflection_probe3d_default() { return ponko_reflection_probe3d_default(); }
/// The built-in `SpotLight3D` component (Rust `SpotLight3D`): a cone of light along the entity's local -Z;
/// angles are half angles in radians, range in meters (transform scale ignored).
using spot_light3d = PonkoSpotLight3D;
/// A box that paints an image onto the opaque surfaces inside it (Rust
/// `Decal3D`); start from decal3d_default(). Projected along its -Z.
using decal3d = PonkoDecal3D;
/// A decal with the engine's defaults (Rust `Decal3D::default()`): a unit box, every layer, no image.
inline decal3d decal3d_default() { return ponko_decal3d_default(); }
/// `image` / `normal_image` value that selects no image.
constexpr std::uint32_t decal_no_image = PONKO_DECAL_NO_IMAGE;
/// Which decal layers a model takes (Rust `ReceivesDecals`); models without
/// it take all. {0} takes none.
using receives_decals = PonkoReceivesDecals;
/// Draws a model with its lightmap from the baked lighting set with
/// ctx::set_baked_lighting (Rust `Lightmapped`): its index in the bake.
using lightmapped = PonkoLightmapped;
/// A square of heightfield terrain, drawn more coarsely further from the
/// camera (Rust `TerrainTile`); made with ctx::create_terrain_tile.
using terrain_tile = PonkoTerrainTile;
/// Several same-size images in one texture (Rust `TextureArray`).
using texture_array = PonkoTextureArray;
/// What a texture array's pixels hold (Rust `TextureKind`).
enum class texture_kind : uint32_t { color = 0, data = 1 };
/// Ground layers and a weight map for ctx::create_terrain_material (Rust
/// `TerrainMaterialDesc`); start from terrain_material_of.
using terrain_material_desc = PonkoTerrainMaterialDesc;
/// Ground `layers` blended by a `weights` map lying over `size` world units
/// from `corner` (x, z); no normal maps, each layer repeating every 4 units.
inline terrain_material_desc terrain_material_of(texture_array layers, texture_array weights, vec2 corner, vec2 size) {
    return ponko_terrain_material_desc(layers, weights, corner, size);
}
/// A mesh on the GPU (Rust `Mesh`), from ctx::create_mesh or ctx::load_mesh. A copyable handle; free it with
/// ctx::free_mesh. mesh_none draws nothing.
using mesh = PonkoMesh;
/// Frame timing over the last few seconds, in ms (`gpu_ms` -1 where the GPU can't time itself).
using frame_stats = PonkoFrameStats;
/// When a world indexes the archetypes of repeated queries; see
/// world_view::set_query_cache_policy.
using query_cache_policy = PonkoQueryCachePolicy;
/// The policy a new world starts with (Rust `QueryCachePolicy::new()`).
inline query_cache_policy query_cache_policy_default() { return ponko_query_cache_policy_default(); }
/// The policy that turns the archetype index off (Rust `QueryCachePolicy::off()`): every query tests
/// every archetype, hinted or not, and the world keeps no index memory.
inline query_cache_policy query_cache_policy_off() { return ponko_query_cache_policy_off(); }
/// What a world's archetype index holds; see world_view::query_cache_stats.
using query_cache_stats = PonkoQueryCacheStats;
/// Bytes the renderer uploaded over the last drawn frame, by kind, and the
/// buffers it made (`reallocations`: buffers and textures replaced because outgrown).
using gpu_uploads = PonkoGpuUploads;
/// A material handle (Rust `Material`) from ctx::create_material. Copyable; free it with ctx::free_material.
using material = PonkoMaterial;
/// What a material is made from (Rust `MaterialDesc`): color, roughness, metallic, emissive, textures (each
/// used when its `has_` flag is set) and `blend` (ponko::blend). Start from material_of.
using material_desc = PonkoMaterialDesc;
/// A material's stencil state (Rust `Stencil`): start from stencil_once, stencil_mark, stencil_inside or
/// stencil_outside, and pass it to ctx::create_material or ctx::update_material.
using stencil = PonkoStencil;
/// One side's stencil test and operations (Rust `StencilFace`): PonkoStencilCompare and PonkoStencilOp values.
using stencil_face = PonkoStencilFace;
/// A material shader in WGSL (ctx::load_shader, ctx::create_shader).
using shader = PonkoShader;
/// A compute pass shader in WGSL (ctx::load_compute, ctx::create_compute).
using compute = PonkoCompute;
/// A static instance batch (Rust `InstanceBatch`): many copies of one mesh drawn in few calls, from
/// ctx::create_instance_batch. A handle; free it with ctx::free_instance_batch.
using instance_batch = PonkoInstanceBatch;
/// Settings of a static instance batch (Rust `InstanceBatchOptions`); materials come from the mesh.
using instance_batch_options = PonkoInstanceBatchOptions;
/// One cell of an instance batch (Rust `InstanceCell`): instances added, replaced and freed on their own,
/// from ctx::add_instance_cell. A handle; free it with ctx::free_instance_cell (freeing its batch frees it).
using instance_cell = PonkoInstanceCell;
/// One level of an instance batch (Rust `InstanceLevel`): its mesh, the half-open band of distances from the
/// origin it is drawn in, and whether it is drawn in the camera, the shadows or both.
using instance_level = PonkoInstanceLevel;
/// A crowd (Rust `Crowd`): thousands of skinned characters drawn in a few calls, from ctx::create_crowd. A
/// handle; free it with ctx::free_crowd.
using crowd = PonkoCrowd;
/// One level of detail of a crowd (Rust `CrowdLevel`): a skinned mesh, the half-open band of distances from
/// each view's eye it is drawn in, whether it is drawn in the camera and the shadows, and how many joints
/// move each corner (1 to 4).
using crowd_level = PonkoCrowdLevel;
/// One character of a crowd (Rust `CrowdInstance`): its column-major transform, tint, clip and time, a clip it
/// blends to, and how fast its clips play on. Start from crowd_instance_at.
using crowd_instance = PonkoCrowdInstance;
/// Settings of a crowd (Rust `CrowdOptions`).
using crowd_options = PonkoCrowdOptions;
/// What a navmesh's crowd has done since it was baked (Rust `NavCrowdStats`), from ctx::navmesh_crowd_stats:
/// crowd steps, agents moved and the backend's work, all only growing.
using nav_crowd_stats = PonkoNavCrowdStats;
/// Heap allocations counted while profiling was on (Rust `AllocCounts`), from thread_alloc_counts.
using alloc_counts = PonkoAllocCounts;
/// How often a game times its GPU passes (Rust `GpuTiming`), for ctx::set_gpu_timing. `automatic` leaves it
/// to the engine (one frame in 16, every frame while profiling is on), `every` times every Nth frame and
/// `off` never times the GPU.
enum class gpu_timing_kind : std::uint32_t {
    /// The engine decides (the default).
    automatic = PONKO_GPU_TIMING_AUTO,
    /// Every `every`th frame; 1 is every frame.
    every = PONKO_GPU_TIMING_EVERY,
    /// No GPU times at all.
    off = PONKO_GPU_TIMING_OFF,
};
/// The choice ctx::set_gpu_timing made and ctx::gpu_timing reads back (Rust `GpuTiming`); `every` matters
/// only for gpu_timing_kind::every. Build one with gpu_timing_auto(), gpu_timing_every(n) or gpu_timing_off().
struct gpu_timing {
    gpu_timing_kind kind = gpu_timing_kind::automatic;
    std::uint32_t every = 0;
    /// Equal when both made the same choice.
    friend bool operator==(const gpu_timing& a, const gpu_timing& b) noexcept { return a.kind == b.kind && a.every == b.every; }
    /// Different choices.
    friend bool operator!=(const gpu_timing& a, const gpu_timing& b) noexcept { return !(a == b); }
};
/// Let the engine decide how often the GPU passes are timed (Rust `GpuTiming::Auto`).
inline gpu_timing gpu_timing_auto() noexcept { return {gpu_timing_kind::automatic, 0}; }
/// Time every `n`th frame (Rust `GpuTiming::Every`; `1` is every frame, 0 counts as 1).
inline gpu_timing gpu_timing_every(std::uint32_t n) noexcept { return {gpu_timing_kind::every, n}; }
/// Never time the GPU (Rust `GpuTiming::Off`).
inline gpu_timing gpu_timing_off() noexcept { return {gpu_timing_kind::off, 0}; }
/// Why the latest frame has no usable GPU time (Rust `GpuUntimed`), from ctx::gpu_untimed; `none` when it has
/// one. More reasons may be added.
enum class gpu_untimed : std::uint32_t {
    /// The latest frame's GPU time is usable.
    none = PONKO_GPU_UNTIMED_NONE,
    /// No GPU: a headless run, or before the renderer opened one.
    no_gpu = PONKO_GPU_UNTIMED_NO_GPU,
    /// The GPU or its backend can't write timestamps.
    no_timestamps = PONKO_GPU_UNTIMED_NO_TIMESTAMPS,
    /// The timer was refused a timestamp query set: the process holds as many as the engine takes here.
    query_set_budget = PONKO_GPU_UNTIMED_QUERY_SET_BUDGET,
    /// GPU timing is off (gpu_timing_off()) and nothing else reads GPU times.
    off = PONKO_GPU_UNTIMED_OFF,
    /// No timed frame's timestamps have come back yet.
    not_ready = PONKO_GPU_UNTIMED_NOT_READY,
    /// The GPU device was lost.
    device_lost = PONKO_GPU_UNTIMED_DEVICE_LOST,
};
/// A clip to bake for crowds (Rust `CrowdClip`), from crowd_clip_of.
using crowd_clip = PonkoCrowdClip;
/// A GPU array of four-float elements (Rust `GpuBuffer`) for compute passes, from ctx::create_gpu_buffer. A
/// handle; free it with ctx::free_gpu_buffer.
using gpu_buffer = PonkoGpuBuffer;
/// One compute pass run each frame (Rust `ComputePass`); see compute_pass_of.
using compute_pass = PonkoComputePass;
/// A screen effect shader in WGSL (ctx::load_effect, ctx::create_effect).
using effect = PonkoEffect;
/// One screen effect run over the lit 3D frame (Rust `ScreenEffect`); see screen_effect_of.
using screen_effect = PonkoScreenEffect;
/// An effect over the lit 3D frame that writes a render target instead of the frame (Rust `TargetEffect`).
using target_effect = PonkoTargetEffect;
/// One key of a keyed distance curve for positional sound (Rust `AudioRolloff::Keyed`): `gain` at
/// `distance`, 0 at min_distance and 1 at max_distance.
using audio_key = PonkoAudioKey;
/// The latest compile or hot reload of one of the game's shaders (Rust
/// `ShaderStatus`; ctx::shader_status). `info.kind` is PONKO_SHADER_MATERIAL,
/// _EFFECT, _COMPUTE or _SPRITE; `info.outcome` PONKO_SHADER_COMPILED, _RELOADED,
/// _FAILED or _RELOAD_FAILED; `info.line` and `info.column` place the first
/// error in the game's own WGSL (0: no place).
struct shader_status {
    /// The path it was loaded from, or the name given to create_*.
    std::string shader;
    PonkoShaderStatus info;
    /// Its errors as a compiler prints them (`water.wgsl:4:12: error: ...`); empty when it worked.
    std::string errors;
    /// Whether the latest compile or reload worked.
    bool ok() const noexcept { return info.ok; }
};
/// How a material covers what's behind it (`material_desc::blend`).
enum class blend : uint32_t { opaque = 0, transparent = 1, cutout = 2 };
/// The 3D camera (Rust `Camera3D`, `ctx.scene3d.camera`): transform, vertical field of view in radians, near
/// and far planes in meters. Read and change it with ctx::camera3d / ctx::set_camera3d.
using camera3d = PonkoCamera3D;
/// A camera drawing the 3D world into a render target each frame (Rust
/// `TargetCamera`); see target_camera_of and ctx::reflection_camera.
using target_camera = PonkoTargetCamera;
/// The sun, the sky's light, shadows and global illumination (bounced
/// light, the gi_ fields); read and change it with ctx::lighting /
/// ctx::set_lighting.
using lighting = PonkoLighting;
/// Post-processing of the 3D frame (Rust `Post`); read and change it with
/// ctx::sky / ctx::set_sky: the sky behind the 3D world and the fog.
using sky = PonkoSky;
/// Which way sunlight travels at `hour` o'clock, for a day and night cycle.
inline vec3 sun_direction_at(float hour) { return ponko_sun_direction_at(hour); }
/// ctx::post / ctx::set_post. tonemap: see ponko::tonemap.
using post = PonkoPost;
/// Post AA mode (`post::anti_aliasing`).
/// `taa` is temporal: sub-pixel jitter and a reprojected history (WebGL2 draws FXAA).
enum class anti_aliasing : uint32_t { none = 0, fxaa = 1, smaa = 2, taa = 3 };
/// Quality settings that trade looks for speed, switched at run time (Rust
/// `GraphicsQuality`): the 3D resolution scale, shadows, AO, screen-space
/// reflections, global illumination and antialiasing.
/// Get a preset with graphics_quality_preset(); apply it with
/// ctx::set_graphics_quality(); read the current one with ctx::graphics_quality().
using graphics_quality = PonkoGraphicsQuality;
/// The presets (Rust `GraphicsQuality`); `custom` is what graphics_quality_level()
/// says of settings that are none of them (Rust `SceneSettings::quality` is `None`).
enum class quality_level : uint32_t { low = 0, medium = 1, high = 2, ultra = 3, custom = 4 };
/// A class of GPU (Rust `GpuTier`), from the weakest the engine runs on:
/// WebGL2 (no compute), baseline (compute: every screen-space effect and GPU
/// culling) and desktop (also GPU skinning, decals and global illumination).
/// See gpu_tier_report() and ctx::gpu_tier().
enum class gpu_tier : uint32_t { webgl2 = 0, baseline = 1, desktop = 2 };
/// A renderer feature that depends on the GPU (Rust `GpuFeature`).
enum class gpu_feature : uint32_t {
    timestamps = 0, compute = 1, storage_buffers = 2, compute_culling = 3, gpu_skinning = 4,
    occlusion_culling = 5, temporal_antialiasing = 6, ambient_occlusion = 7, contact_shadows = 8,
    screen_space_reflections = 9, global_illumination = 10, decals = 11, multisampling = 12
};
/// How HDR light maps to the screen (`post::tonemap`).
enum class tonemap : uint32_t { aces = 0, agx = 1, neutral = 2, none = 3 };
/// Options for ctx::draw_ex (Rust `DrawParams`): tint, rotation, scale, pivot, region, flip and z. Start from
/// draw_defaults().
using draw_params = PonkoDrawParams;
/// A spatial-grid hit: an entity and its position.
using grid_hit = PonkoGridHit;
/// An item of a ponko::grid: entity, position and a number of your own (an index into your own arrays, for
/// example).
using grid_item = PonkoGridItem;
/// A keyboard key (Rust `Key`), by position on a US layout (`PONKO_KEY_A`, ...).
using key = PonkoKey;
/// A mouse button (Rust `MouseButton`).
using mouse = PonkoMouseButton;
/// A gamepad button (Rust `GamepadButton`), by position: `PONKO_GAMEPAD_SOUTH` is A on Xbox, Cross on
/// PlayStation.
using gamepad_button = PonkoGamepadButton;
/// A gamepad axis (Rust `GamepadAxis`): sticks -1..1 (y down is positive), triggers 0..1.
using gamepad_axis = PonkoGamepadAxis;
/// A gamepad, by slot (0 is player one): `for (ponko::gamepad pad : ctx.gamepads())`.
struct gamepad { std::uint32_t slot; };
/// The connected gamepads (ctx::gamepads), oldest slot first. No allocation.
struct gamepad_list {
    std::array<gamepad, PONKO_MAX_GAMEPADS> pads{};
    std::uint32_t count = 0;
    /// Iterate the connected gamepads; size() is how many there are.
    ///@{
    const gamepad* begin() const noexcept { return pads.data(); }
    const gamepad* end() const noexcept { return pads.data() + count; }
    std::size_t size() const noexcept { return count; }
    bool empty() const noexcept { return count == 0; }
    ///@}
};
/// Which stick, for ctx::gamepad_stick.
enum class stick { left, right };
/// A touch gesture (Rust `Gesture`), for action_binding::gesture and ctx::gesture_events.
enum class gesture : std::uint32_t { tap, double_tap, long_press, drag, pinch, rotate, swipe_left, swipe_right, swipe_up, swipe_down };
/// A gesture's amount this frame (Rust `GestureAxis`), for action_binding::gesture_axis: pinch spread
/// (0.1 is 10 % farther apart), rotation in radians (clockwise positive), drag across and down in pixels.
enum class gesture_axis : std::uint32_t { pinch, rotate, drag_x, drag_y };
/// Where a finger is in its life (Rust `TouchPhase`), for ctx::feed_touch.
enum class touch_phase : std::uint32_t { started, moved, ended, cancelled };
/// When on-screen controls show (Rust `TouchShow`): on touch devices only (the default), always, never.
enum class touch_show : std::uint32_t { automatic, always, never };
/// One finger on the screen (Rust `Touch`): `id`, `position`, `start`, `delta` this frame, and
/// `claimed` when it is on an on-screen control.
using touch = PonkoTouch;
/// A gesture that happened this frame: `gesture` (a ponko::gesture value) and where, `position`.
using gesture_event = PonkoGestureEvent;
/// The thresholds gestures are recognized with (Rust `GestureSettings`): pixels and seconds.
using gesture_settings = PonkoGestureSettings;
/// Whether a game writes crash reports (game::crash_reports): the build's default (on in release
/// builds with a window), on everywhere (also headless), off everywhere.
enum class crash_mode : std::uint32_t { build_default, on, off };
/// What stopped the game (`crash_report::kind`, Rust `CrashKind`).
enum class crash_kind : std::uint32_t { panic, error, native };
/// One crash report, as game::crash_reports' hook gets it (Rust `CrashReport`): `kind` (a
/// crash_kind value), `message`, `game_version`, `file` and `path` (null when not saved), `text` (the
/// whole file), and more. Its strings live only while the hook runs.
using crash_report = PonkoCrashReport;
/// An axis-aligned rectangle from `min` (top-left) to `max` (Rust `Rect`); see rect_at.
using rect = PonkoRect;
/// How far in from each edge of the window a phone's notch, home bar and rounded corners cover it, in
/// logical pixels (Rust `SafeInsets`): `top`, `right`, `bottom`, `left`. See ctx::window_safe_area.
using safe_insets = PonkoSafeInsets;
/// A font handle (Rust `Font`) from ctx::load_font; `{0}` is the built-in font.
using font = PonkoFont;
/// Alignment within a world text block.
enum class text_align3d : uint32_t { left = 0, center = 1, right = 2 };
/// Which strings are shaped (Rust `TextShaping`), for ctx::set_text_shaping: `automatic` shapes every
/// non-ASCII string and keeps ASCII on the per-character fast path; `always` shapes ASCII too (ligatures).
enum class text_shaping : uint32_t { automatic = PONKO_TEXT_SHAPING_AUTO, always = PONKO_TEXT_SHAPING_ALWAYS };
/// The form of a word that goes with a number (Rust `PluralCategory`), by the Unicode CLDR's plural
/// categories: English has `one` and `other`, Arabic all six. See ponko::plural.
enum class plural_category : uint32_t {
    zero = PONKO_PLURAL_ZERO, one = PONKO_PLURAL_ONE, two = PONKO_PLURAL_TWO,
    few = PONKO_PLURAL_FEW, many = PONKO_PLURAL_MANY, other = PONKO_PLURAL_OTHER
};
/// One argument of ctx::format and ui::tree::set_text_key (Rust `(name, Arg)`): the placeholder
/// `{name}` it fills and its value. Make one with ponko::arg; an argument named `count` picks a
/// plural form. The strings are borrowed: keep them alive for the call.
using string_arg = PonkoStringArg;
/// A text argument for the placeholder `{name}`.
inline string_arg arg(const char* name, const char* text) { return string_arg{name, PONKO_STRING_ARG_TEXT, text, 0, 0.0}; }
/// A whole-number argument for `{name}`, written with the language's digits and separators.
inline string_arg arg(const char* name, long long value) { return string_arg{name, PONKO_STRING_ARG_INT, nullptr, value, 0.0}; }
/// A whole-number argument for `{name}` (an `int`).
inline string_arg arg(const char* name, int value) { return arg(name, static_cast<long long>(value)); }
/// A number argument for `{name}`: as many decimals as the placeholder asks for (`{time:1}`).
inline string_arg arg(const char* name, double value) { return string_arg{name, PONKO_STRING_ARG_NUMBER, nullptr, 0, value}; }
/// A language with a string table: its code (`"en"`) and its name for a menu (`"English"`).
struct string_language {
    std::string code; ///< The language's code.
    std::string name; ///< Its name, from the table's `name`.
};
/// Where a system runs in the loop, in this order. `input` through `sim_last`
/// run once per fixed tick (also headless); `frame` through `draw` once per
/// rendered frame. sim_system uses after_physics, system frame, late_system late.
enum class phase : uint32_t {
    input = PONKO_PHASE_INPUT,
    before_physics = PONKO_PHASE_BEFORE_PHYSICS,
    physics = PONKO_PHASE_PHYSICS,
    after_physics = PONKO_PHASE_AFTER_PHYSICS,
    sim_last = PONKO_PHASE_SIM_LAST,
    frame = PONKO_PHASE_FRAME,
    animate = PONKO_PHASE_ANIMATE,
    late = PONKO_PHASE_LATE,
    draw = PONKO_PHASE_DRAW,
};
/// A system's removal handle (Rust `SystemId`): from game::add_system, or a
/// constant in `systems` for a built-in. Ids are never reused.
struct system_id {
    std::uint32_t bits = 0;
    /// Two ids are equal when they name the same system.
    ///@{
    friend bool operator==(system_id a, system_id b) { return a.bits == b.bits; }
    friend bool operator!=(system_id a, system_id b) { return a.bits != b.bits; }
    ///@}
};
/// The built-in systems' ids (Rust `Systems`), for game::remove. Removing
/// one that isn't installed does nothing.
namespace systems {
/// The 2D physics step; 3D physics; moving 3D bodies smoothly between ticks; rebuilding the
/// spatial grid; sprite animation; tilemap and sprite drawing; attachments; navmesh agents.
///@{
inline constexpr system_id physics{PONKO_SYSTEM_PHYSICS};
inline constexpr system_id physics3d{PONKO_SYSTEM_PHYSICS3D};
inline constexpr system_id interpolate_physics3d{PONKO_SYSTEM_INTERPOLATE_PHYSICS3D};
inline constexpr system_id rebuild_spatial_grid{PONKO_SYSTEM_REBUILD_SPATIAL_GRID};
inline constexpr system_id animate_sprites{PONKO_SYSTEM_ANIMATE_SPRITES};
inline constexpr system_id draw_tilemaps{PONKO_SYSTEM_DRAW_TILEMAPS};
inline constexpr system_id draw_sprites{PONKO_SYSTEM_DRAW_SPRITES};
inline constexpr system_id follow_attachments{PONKO_SYSTEM_FOLLOW_ATTACHMENTS};
inline constexpr system_id step_navmesh3d{PONKO_SYSTEM_STEP_NAVMESH3D};
///@}
/// Animators posing skeletons; stays at the end of phase::animate.
inline constexpr system_id update_animators{PONKO_SYSTEM_UPDATE_ANIMATORS};
/// Particle emitters and trails; stays at the end of phase::animate.
inline constexpr system_id update_particles{PONKO_SYSTEM_UPDATE_PARTICLES};
/// Positional audio; stays at the end of phase::draw, after the drawing systems.
inline constexpr system_id update_audio3d{PONKO_SYSTEM_UPDATE_AUDIO3D};
/// 2D particle drawing, after the sprites.
inline constexpr system_id draw_particles{PONKO_SYSTEM_DRAW_PARTICLES};
/// 2D skeletons advancing and posing; stays at the end of phase::animate.
inline constexpr system_id update_skeletons2d{PONKO_SYSTEM_UPDATE_SKELETONS2D};
} // namespace systems
/// Engine services an access declaration can name (see ponko::access).
enum class service : uint32_t {
    input = PONKO_SERVICE_INPUT,
    time = PONKO_SERVICE_TIME,
    random = PONKO_SERVICE_RANDOM,
    gfx = PONKO_SERVICE_GFX,
    audio = PONKO_SERVICE_AUDIO,
    assets = PONKO_SERVICE_ASSETS,
    ui = PONKO_SERVICE_UI,
    perf = PONKO_SERVICE_PERF,
    camera = PONKO_SERVICE_CAMERA,
    spatial = PONKO_SERVICE_SPATIAL,
    physics = PONKO_SERVICE_PHYSICS,
    physics3d = PONKO_SERVICE_PHYSICS3D,
    net = PONKO_SERVICE_NET,
};
/// Owned world text, in meters. Settings follow Rust Text3D defaults.
struct text3d {
    std::string text;
    ponko::font font{};
    float size = 0.25f;
    PonkoColor color{1, 1, 1, 1};
    PonkoColor outline{0, 0, 0, 0};
    float outline_width = 0, softness = 0;
    text_align3d align = text_align3d::center;
    bool billboard = false;
    /// Text `value` with the defaults: white, centered, 0.25 m lines, the built-in font, no outline, not a
    /// billboard (Rust `Text3D::new`).
    explicit text3d(std::string value = "") : text(std::move(value)) {}
};

/// Options for ctx::draw_text_ex (Rust `TextParams`): font, size in pixels, color. Start from
/// text_defaults().
using text_params = PonkoTextParams;
/// Colors and sizes of the immediate-mode UI widgets (Rust `UiStyle`); start from ui_style_defaults().
using ui_style = PonkoUiStyle;
/// A sound handle (Rust `Sound`) from ctx::load_sound, ctx::load_stream or ctx::create_sound. Copyable; free
/// it with ctx::free_sound.
using sound = PonkoSound;
/// One playback of a sound (Rust `Playing`), from ctx::play_sound, to stop or change it.
using playing = PonkoPlaying;
/// Options for ctx::play_sound (Rust `SoundParams`): volume (1 is full), speed, looping and bus. Start from
/// sound_defaults().
using sound_params = PonkoSoundParams;
/// A mixer bus (Rust `AudioBus`) from ctx::audio_bus; zero is `sfx`, where ordinary sounds play.
using audio_bus = PonkoAudioBus;
/// A sound started this tick or frame (Rust `SoundStarted`), from ctx::sounds_started: its playback, the
/// sound, its bus, and the volume (after distance fall-off for a sound in 3D), speed and looping it began with.
using sound_started = PonkoSoundStarted;
/// A voice's low-pass filter and reverb send (Rust `AudioEffects`). Start from audio_effects_defaults().
using audio_effects = PonkoAudioEffects;
/// Which effect a bus_effect is (Rust `BusEffect`'s variants).
using bus_effect_kind = PonkoBusEffectKind;
/// When a music change happens (Rust `Quantize`): now, on the next beat or on the next bar line.
using quantize = PonkoQuantize;
/// How many sounds are heard and how many are virtual (Rust `VoiceCount`), from ctx::audio_voice_count.
struct voice_count {
    std::uint32_t real = 0;         ///< Mixed and heard.
    std::uint32_t virtualized = 0;  ///< Followed but not mixed: over the voice limits, or inaudible.
};
/// Which frequencies a bus filter lets through (Rust `FilterMode`).
using filter_mode = PonkoFilterMode;
/// One effect in a bus's chain (Rust `BusEffect`); `kind` says which fields it reads. Start from
/// bus_effect_defaults(kind).
using bus_effect = PonkoBusEffect;
/// Sidechain ducking (Rust `Ducking`): while `key` is loud, the bus drops to `gain`. Start from
/// ducking_defaults(key).
using ducking = PonkoDucking;
/// New settings for effect `index` of a bus's chain, in a snapshot (Rust `EffectOverride`).
using effect_override = PonkoEffectOverride;
/// What a snapshot changes on one bus (Rust `BusSnapshot`), for ctx::add_audio_snapshot.
struct bus_snapshot {
    audio_bus bus{};                       ///< The bus.
    std::optional<float> volume;           ///< Its volume while the snapshot is on; empty keeps its own.
    std::vector<effect_override> effects;  ///< New settings for effects in its chain.
};
/// Settings of a positional sound (Rust `AudioSettings3D`): fade distances in meters, rolloff, bus. Start
/// from audio_settings3d_defaults().
using audio_settings3d = PonkoAudioSettings3D;
/// Where the ears are (Rust `AudioListener3D`): +X right, -Z forward.
using audio_listener3d = PonkoAudioListener3D;
/// Same-size frames in one texture (Rust `SpriteSheet`), numbered left to right, top to bottom; make one with
/// sheet_of.
using sprite_sheet = PonkoSpriteSheet;
/// Frame animation of an entity with a sprite (Rust `Animation`); the built-in animate_sprites system
/// advances it. A query column; make one with animation_of.
using animation = PonkoAnimation;
/// The built-in `RigidBody` component (Rust `RigidBody`): makes an entity a 2D physics body (it also needs a
/// collider and a transform2d). Make one with dynamic_body, fixed_body or kinematic_body.
using rigid_body = PonkoRigidBody;
/// The built-in `Collider` component (Rust `Collider`): the solid shape of a 2D body in pixels, centered on
/// the entity plus `offset`. Make one with circle_collider, rect_collider, capsule_collider or
/// polygon_collider.
using collider = PonkoCollider;
/// The built-in `Velocity` component (Rust `Velocity`), in pixels and radians per second: read it after a
/// step, write it to set the speed. A query column.
using velocity = PonkoVelocity;
/// What a 2D ray or shape cast hit (ctx::cast_ray, cast_circle, ...): the entity, the point, the surface
/// normal and the distance along the way.
using ray_hit = PonkoRayHit;
/// The nearest point of a 2D collider to a point (ctx::closest_point; Rust `d2::PointHit`).
using point_hit2d = PonkoPointHit2D;
/// How deep a shape sinks into one 2D collider and the way out (ctx::penetrations; Rust `d2::Penetration`).
using penetration2d = PonkoPenetration2D;
/// Two colliders that started or stopped touching (ctx::collisions).
using collision = PonkoCollision;
/// The built-in `RigidBody3D` component (Rust `RigidBody3D`): makes an entity a 3D physics body (it also
/// needs a collider3d and a transform3d). Make one with dynamic_body3d, fixed_body3d or kinematic_body3d.
using rigid_body3d = PonkoRigidBody3D;
/// The built-in `Collider3D` component (Rust `Collider3D`): a body's shape in meters, its surface and its
/// groups. Make one with sphere_collider, box_collider, capsule_collider3d, mesh_collider or
/// scaled_mesh_collider.
using collider3d = PonkoCollider3D;
/// The built-in `Velocity3D` component (Rust `Velocity3D`), meters and radians per second. A query column.
using velocity3d = PonkoVelocity3D;
/// The built-in `Character3D` component (Rust `Character3D`): a walking capsule whose transform3d is at its
/// feet. Set `velocity`, read `grounded` after the step. Make one with character().
using character3d = PonkoCharacter3D;
/// The built-in `Joint3D` component (Rust `Joint3D`): joins two bodies. Axes and drives are indexed by
/// joint_axis; make one with fixed_joint3d and friends.
using joint3d = PonkoJoint3D;
/// The built-in `ContactModifier3D` component (Rust `ContactModifier3D`): changes how a body's contacts
/// behave (one-way, conveyor, hook). Make one with default_contact_modifier3d and friends.
using contact_modifier3d = PonkoContactModifier3D;
/// A motor on one axis of a joint (Rust `JointDrive3D`): a spring toward `target_position` (`stiffness`) and
/// toward `target_velocity` (`damping`). See velocity_drive and spring_drive.
using joint_drive3d = PonkoJointDrive3D;
/// A convex hull, triangle mesh or heightfield for 3D colliders (Rust `CollisionMesh`), from
/// ctx::heightfield_collider and friends. A handle owned by the physics world.
using collision_mesh = PonkoCollisionMesh;
/// Which colliders touch which (Rust `CollisionGroups`): a collider is in the groups of `memberships` (a bit
/// each) and touches colliders whose memberships share a bit with its `filter`; both sides must agree. See
/// groups() and collision_groups_all().
using collision_groups = PonkoCollisionGroups;
/// The C form of a 3D query filter; build one with ponko::filter3d.
using query_filter3d = PonkoQueryFilter3D;
/// What a 3D ray or shape cast hit (Rust `Hit3D`): entity, point, normal and distance in meters.
using hit3d = PonkoHit3D;
/// The nearest point of a 3D collider to a point (ctx::closest_point3d; Rust `d3::PointHit`).
using point_hit3d = PonkoPointHit3D;
/// How deep a shape sinks into one 3D collider and the way out (ctx::penetrations3d; Rust `d3::Penetration`).
using penetration3d = PonkoPenetration3D;
/// The built-in `Vehicle3D` component (Rust `Vehicle3D`): up to 8 raycast wheels holding up a dynamic body's
/// chassis, facing -Z. Make one with vehicle().
using vehicle3d = PonkoVehicle3D;
/// One wheel of a vehicle3d (Rust `Wheel3D`): setup, inputs you set each tick (steer, engine force, brake)
/// and outputs from the last physics step. Make one with wheel().
using wheel3d = PonkoWheel3D;
/// How much grip a tyre has for how much it slips (Rust `FrictionCurve`); see friction_curve_forward,
/// friction_curve_sideways and evaluate.
using friction_curve = PonkoFrictionCurve;
/// The built-in `TerrainCollider` component (Rust `TerrainCollider`): makes a terrain tile's entity solid.
/// Make one with solid_terrain.
using terrain_collider = PonkoTerrainCollider;
/// The built-in `ColliderFocus` component (Rust `ColliderFocus`): keeps nearby static colliders in the
/// physics world while collider streaming is on. Make one with focus_colliders.
using collider_focus = PonkoColliderFocus;
/// How a texture is sampled when scaled (ctx::set_texture_filter): smooth or pixelated.
enum class texture_filter { linear, nearest };

/// The length of `v`.
inline float length(vec2 v) { return std::sqrt(v.x * v.x + v.y * v.y); }
/// The squared length of `v` (cheaper than length, for comparisons).
inline float length_squared(vec2 v) { return v.x * v.x + v.y * v.y; }
/// The dot product of `a` and `b`.
inline float dot(vec2 a, vec2 b) { return a.x * b.x + a.y * b.y; }
/// `v` scaled to length 1, or `fallback` when `v` is zero or not finite.
inline vec2 normalize_or(vec2 v, vec2 fallback) { float l = length(v); return l > 0 && std::isfinite(l) ? v / l : fallback; }
/// The unit vector at `radians` (clockwise from +X in a y-down 2D world).
inline vec2 from_angle(float radians) { return {std::cos(radians), std::sin(radians)}; }
/// The angle of `v` in radians, as from_angle takes it.
inline float to_angle(vec2 v) { return std::atan2(v.y, v.x); }
/// `v` wrapped into [0, m) on each axis, also for negative values (Rust `Vec2::rem_euclid`).
inline vec2 rem_euclid(vec2 v, vec2 m) {
    auto r = [](float a, float b) { float x = std::fmod(a, b); return x < 0 ? x + b : x; };
    return {r(v.x, m.x), r(v.y, m.y)};
}
/// The id other calls take, from an entity in a query chunk.
inline entity bits(PonkoEntity e) { return entity(e.generation) << 32 | e.index; }
/// The entity struct (as in query chunks and ponko::attached) for an id.
inline PonkoEntity entity_of(entity e) { return {static_cast<std::uint32_t>(e), static_cast<std::uint32_t>(e >> 32)}; }

/// Physics bodies (game::physics): moved by gravity and collisions, never
/// moving, or moved only by you.
///@{
inline rigid_body dynamic_body() { return ponko_rigid_body(PONKO_BODY_DYNAMIC); }
inline rigid_body fixed_body() { return ponko_rigid_body(PONKO_BODY_FIXED); }
inline rigid_body kinematic_body() { return ponko_rigid_body(PONKO_BODY_KINEMATIC); }
///@}
/// Collider shapes, in pixels; change density, friction, restitution,
/// sensor and offset on the result.
///@{
inline collider circle_collider(float radius) { return ponko_collider_circle(radius); }
inline collider rect_collider(vec2 size) { return ponko_collider_rect(size); }
inline collider capsule_collider(float height, float radius) { return ponko_collider_capsule(height, radius); }
///@}
/// A convex polygon collider: the convex hull of 3 to 8 corners around the
/// entity's position (Rust: `Collider::polygon`). Spawn or set it like any
/// collider; `world.collider_points(e)` reads the corners back.
struct polygon : collider {
    std::array<vec2, 8> points{};
    std::uint32_t count = 0;
};
/// A convex polygon collider from 3 to 8 `corners` around the entity's position, with the default density,
/// friction and restitution. Throws std::invalid_argument for fewer than 3 or more than 8 corners.
inline polygon polygon_collider(std::initializer_list<vec2> corners) {
    if (corners.size() < 3 || corners.size() > 8) throw std::invalid_argument("polygon_collider: 3 to 8 points");
    polygon p;
    static_cast<collider&>(p) = ponko_collider_circle(1.0f);  // the default settings
    std::copy(corners.begin(), corners.end(), p.points.begin());
    p.count = static_cast<std::uint32_t>(corners.size());
    return p;
}

/// An opaque color, each channel 0..1.
inline color rgb(float r, float g, float b) { return {r, g, b, 1}; }
/// A color with alpha `a` (0 transparent, 1 opaque), each channel 0..1.
inline color rgba(float r, float g, float b, float a) { return {r, g, b, a}; }
/// Named colors, including the engine's dark background `ponko_dark`.
namespace colors {
/// Opaque basics, fully transparent, and the engine's dark background.
constexpr color white{1, 1, 1, 1}, black{0, 0, 0, 1}, gray{.5f, .5f, .5f, 1}, red{1, 0, 0, 1},
    green{0, 1, 0, 1}, blue{0, 0, 1, 1}, yellow{1, 1, 0, 1}, transparent{0, 0, 0, 0}, ponko_dark{.11f, .12f, .15f, 1};
}
/// Follows `to` at `offset` (in its space: turned by its rotation, stretched
/// by its scale), turned `rotation` more. Needs game::follow_attachments().
inline attached attach_to(entity to, vec2 offset = {0, 0}, float rotation = 0) { return {entity_of(to), offset, rotation}; }
/// A transform at `position`, no rotation, scale 1.
inline transform2d transform_at(vec2 position, float rotation = 0, vec2 scale = {1, 1}) { return {position, rotation, scale}; }
/// A sprite showing all of `t`, centered, white, z 0.
inline sprite sprite_of(texture t, color tint = colors::white, float z = 0) {
    sprite s{}; s.texture = t; s.color = tint; s.z = z; s.pivot = {.5f, .5f}; return s;
}
/// Where 2D coordinates are: `world` units through the camera, or `screen`
/// window pixels over the world (Rust `DrawSpace`). Immediate text is always in window pixels.
enum class draw_space { world, screen };
/// Returns `params` drawing in `space`, leaving every other setting as it was (Rust `DrawParams::with_space`).
inline draw_params with_space(draw_params params, draw_space space) {
    return ponko_draw_params_with_space(params, space == draw_space::screen ? PONKO_DRAW_SPACE_SCREEN : PONKO_DRAW_SPACE_WORLD);
}
/// The default draw options for ctx::draw_ex: white, no rotation, scale 1, centered, whole texture, z 0.
inline draw_params draw_defaults() { return ponko_draw_params_default(); }

// 3D (Rust `Transform3D`, `Model`, `MaterialDesc`, `Quat`).
/// A rotation of `angle` radians around `axis` (Rust `Quat::from_axis_angle`).
inline quat quat_from_axis_angle(vec3 axis, float angle) { return ponko_quat_from_axis_angle(axis, angle); }
/// No rotation (Rust `Quat::IDENTITY`), `{0, 0, 0, 1}`.
inline quat quat_identity() { return ponko_quat_identity(); }
/// Rotations around one axis (Rust `Quat::from_rotation_x` and friends).
///@{
inline quat quat_from_rotation_x(float angle) { return ponko_quat_from_axis_angle({1, 0, 0}, angle); }
inline quat quat_from_rotation_y(float angle) { return ponko_quat_from_axis_angle({0, 1, 0}, angle); }
inline quat quat_from_rotation_z(float angle) { return ponko_quat_from_axis_angle({0, 0, 1}, angle); }
///@}
/// A 3D transform at `position`, turned by `rotation`, scaled by `scale` on every axis.
inline transform3d transform3d_at(vec3 position, quat rotation = {0, 0, 0, 1}, float scale = 1) {
    transform3d t = ponko_transform3d_at(position); t.rotation = rotation; t.scale = {scale, scale, scale}; return t;
}
/// Follows `to` at `offset` (in its space: turned by its rotation, stretched
/// by its scale, chains placed parents first). Needs game::follow_attachments().
inline attached3d attach3d_to(entity to, transform3d offset = ponko_transform3d_at({0, 0, 0})) {
    return {entity_of(to), {0, 0}, offset};
}
/// `t` turned so its -Z axis points at `target`, +Y up.
inline transform3d looking_at(const transform3d& t, vec3 target) { return ponko_transform3d_looking_at(t, target); }
/// The direction `t` faces: its -Z axis.
inline vec3 forward(const transform3d& t) { return ponko_transform3d_forward(t); }
/// 3D physics bodies (game::physics3d), like the 2D ones.
///@{
inline rigid_body3d dynamic_body3d() { return ponko_rigid_body3d(PONKO_BODY_DYNAMIC); }
inline rigid_body3d fixed_body3d() { return ponko_rigid_body3d(PONKO_BODY_FIXED); }
inline rigid_body3d kinematic_body3d() { return ponko_rigid_body3d(PONKO_BODY_KINEMATIC); }
///@}
/// 3D collider shapes, in meters; change density, friction, restitution,
/// sensor, offset and groups on the result.
///@{
inline collider3d sphere_collider(float radius) { return ponko_collider3d_sphere(radius); }
inline collider3d box_collider(vec3 size) { return ponko_collider3d_box(size); }
inline collider3d capsule_collider3d(float height, float radius) { return ponko_collider3d_capsule(height, radius); }
///@}
/// A collider of a heightfield, convex hull or triangle mesh made by
/// ctx::heightfield_collider and friends.
inline collider3d mesh_collider(collision_mesh m) { return ponko_collider3d_mesh(m); }
/// A collider of `m` scaled by `scale` where it is used (Rust
/// `Collider3D::scaled_mesh`), so one mesh serves colliders of every size: a
/// negative component mirrors it (a heightfield along X and Z; its Y takes
/// the size only). Colliders of one mesh at one scale share one scaled copy.
inline collider3d scaled_mesh_collider(collision_mesh m, vec3 scale) { return ponko_collider3d_scaled_mesh(m, scale); }
/// Makes a terrain tile's entity solid: a collider of `m` (its heightfield)
/// at the tile's transform3d, turned and scaled with it (Rust
/// `TerrainCollider::new`). Change friction, restitution and groups on the result.
inline terrain_collider solid_terrain(collision_mesh m) { return ponko_terrain_collider(m); }
/// Keeps static colliders within `radius` of its entity, until they're
/// beyond `keep_radius` of every focus, once ctx::stream_colliders is on
/// (Rust `ColliderFocus::new`).
inline collider_focus focus_colliders(float radius, float keep_radius) { return ponko_collider_focus(radius, keep_radius); }
/// A walking capsule `height` tall and `radius` wide, its transform3d at
/// its feet: set `velocity`, read `grounded`.
inline character3d character(float height = 1.8f, float radius = 0.3f) { return ponko_character3d(height, radius); }
/// Index of each joint3d axis in `axes` and `drives`: local X is the hinge, slider and twist axis.
namespace joint_axis {
/// Linear axes first (meters), then angular axes (radians).
constexpr std::size_t linear_x = 0, linear_y = 1, linear_z = 2, angular_x = 3, angular_y = 4, angular_z = 5;
}
/// Welds bodies `a` and `b` together.
inline joint3d fixed_joint3d(entity a, entity b) { return ponko_joint3d_fixed(a, b); }
/// A ball joint: the frames stay together, turning freely.
inline joint3d spherical_joint3d(entity a, entity b) { return ponko_joint3d_spherical(a, b); }
/// A hinge about the frames' X axis.
inline joint3d revolute_joint3d(entity a, entity b) { return ponko_joint3d_revolute(a, b); }
/// A slider along the frames' X axis.
inline joint3d prismatic_joint3d(entity a, entity b) { return ponko_joint3d_prismatic(a, b); }
/// Every axis free: set them yourself.
inline joint3d generic_joint3d(entity a, entity b) { return ponko_joint3d_generic(a, b); }
/// Limits one axis of `j` to [min, max] (meters, or radians for turning axes).
inline joint3d with_limits(joint3d j, std::size_t axis, float min, float max) {
    j.axes[axis] = {PONKO_JOINT_LIMITED, min, max}; return j;
}
/// Puts a motor on one axis of `j`.
inline joint3d with_drive(joint3d j, std::size_t axis, joint_drive3d d) { d.enabled = true; j.drives[axis] = d; return j; }
/// A motor turning (or sliding) at `velocity`, with this much `damping` strength.
inline joint_drive3d velocity_drive(float velocity, float damping) {
    return {true, 0, velocity, 0, damping, std::numeric_limits<float>::infinity()};
}
/// A spring toward `position`.
inline joint_drive3d spring_drive(float position, float stiffness, float damping) {
    return {true, position, 0, stiffness, damping, std::numeric_limits<float>::infinity()};
}
/// Gives a 3D body contact dominance: it pushes bodies with lower values without being pushed back.
inline rigid_body3d with_dominance(rigid_body3d b, std::int8_t dominance) { b.dominance = dominance; return b; }
/// Gives a 3D body this mass in kg, whatever its collider's density.
inline rigid_body3d with_mass(rigid_body3d b, float kg) { b.mass = kg; return b; }
/// Moves a 3D body's center of mass to `center`, in its own frame (cars want it low).
inline rigid_body3d with_center_of_mass(rigid_body3d b, vec3 center) {
    b.has_center_of_mass = true; b.center_of_mass = center; return b;
}
/// A raycast wheel of `radius` mounted at `position` (its center at full
/// compression, in the chassis's frame) with the usual suspension and tyres.
inline wheel3d wheel(vec3 position, float radius) { return ponko_wheel3d(position, radius); }
/// A vehicle on these wheels (at most 8), for an entity with a dynamic
/// rigid_body3d, a collider3d and a transform3d facing -Z.
inline vehicle3d vehicle(std::initializer_list<wheel3d> wheels) {
    vehicle3d v = ponko_vehicle3d();
    for (const wheel3d& w : wheels)
        if (ponko_vehicle3d_add_wheel(&v, w) != PONKO_OK) throw std::invalid_argument("a vehicle has at most 8 wheels");
    return v;
}
/// Where to draw `w` for a chassis at `chassis`: steer, spin and suspension travel included.
inline transform3d wheel_pose(const wheel3d& w, const transform3d& chassis) { return ponko_wheel3d_pose(&w, chassis); }
/// One bone of a ragdoll (Rust `RagdollBone`): the joint it starts at and
/// the capsule standing in for it, which runs to the joint's first child.
/// `ragdoll_bone("spine", 0.1f).swing(0.5f)`.
class ragdoll_bone {
public:
    /// A bone that starts at `joint` (a joint name of the skeleton), its capsule `radius` meters wide, with
    /// the default limits and density.
    ragdoll_bone(std::string joint, float radius) : joint_(std::move(joint)), raw_(ponko_ragdoll_bone(nullptr, radius)) {}
    /// How far it may swing away from its parent, in radians (default 0.7).
    ragdoll_bone swing(float radians) const { auto b = *this; b.raw_.swing = radians; return b; }
    /// How far it may twist around itself, either way, in radians (default 0.3).
    ragdoll_bone twist(float radians) const { auto b = *this; b.raw_.twist = radians; return b; }
    /// kg per cubic meter (default 1000).
    ragdoll_bone density(float kg_per_m3) const { auto b = *this; b.raw_.density = kg_per_m3; return b; }
    /// The C struct, pointing at this bone's joint name.
    PonkoRagdollBone raw() const { auto r = raw_; r.joint = joint_.c_str(); return r; }
private:
    std::string joint_;
    PonkoRagdollBone raw_;
};
/// A contact modifier that does nothing (Rust `ContactModifier3D::default()`).
/// Set its fields to combine effects, e.g. `.hook = true`.
inline contact_modifier3d default_contact_modifier3d() { return ponko_contact_modifier3d_default(); }
/// Solid only from the side `up` (a local direction) points to: things pass
/// up through it and land on top (Rust `ContactModifier3D::one_way`). Set
/// `.one_way_angle` (radians, default 1.2) to change how far a contact may
/// lean from `up` and still count.
inline contact_modifier3d one_way_modifier3d(vec3 up) { return ponko_contact_modifier3d_one_way(up); }
/// A surface carrying what touches it at `velocity` (local to the body, m/s),
/// like a conveyor belt (Rust `ContactModifier3D::conveyor`).
inline contact_modifier3d conveyor_modifier3d(vec3 velocity) { return ponko_contact_modifier3d_conveyor(velocity); }
/// A modifier whose contacts go to the hook set with `ctx::set_contact_hook3d`
/// (Rust `ContactModifier3D::hook`).
inline contact_modifier3d hook_modifier3d() { return ponko_contact_modifier3d_hook(); }
/// How fast a wheel spins, in revolutions per minute.
inline float rpm(const wheel3d& w) { return w.angular_velocity * 60.0f / 6.2831853f; }
/// The grip of a tyre curve at `slip` (either sign), stiffness included.
inline float evaluate(const friction_curve& c, float slip) { return ponko_friction_curve_evaluate(c, slip); }
/// The usual forward (rolling) grip (Rust `FrictionCurve::FORWARD`): peak 1 at slip 0.4, sliding 0.5
/// from 0.8. wheel() starts from it.
inline friction_curve friction_curve_forward() { return ponko_friction_curve_forward(); }
/// The usual sideways grip (Rust `FrictionCurve::SIDEWAYS`): peak 1 at 0.2 rad of slip angle, sliding
/// 0.75 from 0.5 rad. wheel() starts from it.
inline friction_curve friction_curve_sideways() { return ponko_friction_curve_sideways(); }
/// In groups `memberships`, touching colliders in groups `filter`.
inline collision_groups groups(std::uint32_t memberships, std::uint32_t filter) { return {memberships, filter}; }
/// In every group, touching every group: what a new collider has (Rust `CollisionGroups::default()`).
inline collision_groups collision_groups_all() { return ponko_collision_groups_all(); }
/// What a 2D query sees: every solid collider (no sensors) in every group by default (Rust `d2::QueryFilter`).
struct filter2d {
    PonkoQueryFilter2D raw{};
    /// The groups the query touches as; the C calls take them next to `raw`.
    collision_groups raw_groups = ponko_collision_groups_all();
    /// Ignores entity `e` (say, the one casting).
    filter2d& exclude(entity e) { raw.exclude = true; raw.exclude_entity = e; return *this; }
    /// Sees what a collider in these groups would touch.
    filter2d& groups(collision_groups g) { raw_groups = g; return *this; }
    /// Sees sensors too.
    filter2d& with_sensors() { raw.sensors = true; return *this; }
};

/// What a 3D query sees: every solid collider (no sensors) by default.
struct filter3d {
    query_filter3d raw = ponko_query_filter3d();
    /// Looks through `e` (say, the one casting).
    filter3d& exclude(entity e) { raw.exclude = true; raw.exclude_entity = e; return *this; }
    /// Sees what a collider in these groups would touch.
    filter3d& groups(collision_groups g) { raw.groups = g; return *this; }
    /// Sees sensors too.
    filter3d& with_sensors() { raw.sensors = true; return *this; }
};

/// Draws `m` tinted by `tint` (white: its own colors).
inline model model_of(mesh m, color tint = colors::white) { model x = ponko_model(m); x.color = tint; return x; }
/// A plain material of one color: roughness 0.6, not metallic. Change
/// roughness, metallic, emissive or set a texture on the result.
inline material_desc material_of(color c) { return ponko_material_desc(c); }
/// A stencil that tests and writes nothing (Rust `Stencil::default`).
inline stencil stencil_default() { return ponko_stencil_default(); }
/// Draws everywhere and writes `reference` where it drew (Rust `Stencil::mark`).
inline stencil stencil_mark(std::uint8_t reference) { return ponko_stencil_mark(reference); }
/// Draws only where the stencil holds `reference` (Rust `Stencil::inside`).
inline stencil stencil_inside(std::uint8_t reference) { return ponko_stencil_inside(reference); }
/// Draws only where the stencil doesn't hold `reference` (Rust `Stencil::outside`).
inline stencil stencil_outside(std::uint8_t reference) { return ponko_stencil_outside(reference); }
/// Draws each pixel at most once, writing `reference` there (Rust `Stencil::once`).
inline stencil stencil_once(std::uint8_t reference) { return ponko_stencil_once(reference); }
/// Runs `e`, reading its values and textures from `m` (or from the default
/// material when `m` is left out).
///@{
inline screen_effect screen_effect_of(effect e) { return {e, false, {}}; }
inline screen_effect screen_effect_of(effect e, material m) { return {e, true, m}; }
///@}
/// Runs `fn update(i: u32)` on every buffer element, with optional material inputs.
///@{
inline compute_pass buffer_compute_pass(compute c, gpu_buffer b) { return {true, b, c, {}, false, {}}; }
inline compute_pass buffer_compute_pass(compute c, gpu_buffer b, material m) { return {true, b, c, {}, true, m}; }
///@}
/// Runs `c` on render target `target`, reading its values and textures
/// from `m` (or from the default material when `m` is left out).
///@{
inline compute_pass compute_pass_of(compute c, render_target target) { return {false, {}, c, target, false, {}}; }
inline compute_pass compute_pass_of(compute c, render_target target, material m) { return {false, {}, c, target, true, m}; }
///@}
/// Runs `e` over the lit frame into render target `target`, reading its values and textures from `m`
/// (or from the default material when `m` is left out).
///@{
inline target_effect target_effect_of(effect e, render_target target) { return {e, target, false, {}}; }
inline target_effect target_effect_of(effect e, render_target target, material m) { return {e, target, true, m}; }
///@}
/// `c` drawing into render target `target` (ctx::create_render_target),
/// with no clip plane.
inline target_camera target_camera_of(const camera3d& c, render_target target) { return {c, target, false, {0, 0, 0, 0}}; }
/// Built-in font, 16 px, white.
inline text_params text_defaults() { return ponko_text_params_default(); }
/// Full volume, normal speed, once.
inline sound_params sound_defaults() { return ponko_sound_params_default(); }
/// No low-pass, no reverb.
inline audio_effects audio_effects_defaults() { return ponko_audio_effects_default(); }
/// Full volume within 1 unit, silent at 100, natural fade, on the `sfx` bus.
inline audio_settings3d audio_settings3d_defaults() { return ponko_audio_settings3d_default(); }
/// An effect of `kind` with every field at its default (a low-pass at 20 kHz, a half-wet reverb, ...).
inline bus_effect bus_effect_defaults(bus_effect_kind kind) { return ponko_bus_effect_default(kind); }
/// Ducking by `key`: down to 0.3 in 0.05 s, back up in 0.5 s, for anything audible.
inline ducking ducking_defaults(audio_bus key) { return ponko_ducking_default(key); }
/// The default widget colors and sizes.
inline ui_style ui_style_defaults() { return ponko_ui_style_default(); }
/// A rectangle from its top-left corner and size.
inline rect rect_at(vec2 min, vec2 size) { return {min, min + size}; }
/// Splits `t` into `w` x `h` frames, numbered left to right, top to bottom.
inline sprite_sheet sheet_of(texture t, std::uint32_t w, std::uint32_t h) {
    sprite_sheet s{};
    if (ponko_sprite_sheet(t, w, h, &s) != PONKO_OK) throw std::invalid_argument(ponko_last_error());
    return s;
}
/// Frame `i` of a sheet in texture pixels (wraps past the end).
inline rect frame_of(const sprite_sheet& s, std::uint32_t i) { return ponko_sprite_sheet_frame(s, i); }
/// A sprite showing frame `i` of a sheet.
inline sprite sprite_of(const sprite_sheet& s, std::uint32_t i, color tint = colors::white, float z = 0) {
    sprite out = sprite_of(s.texture, tint, z); out.has_region = true; out.region = frame_of(s, i); return out;
}
/// Frames `first` .. `first + count - 1` at `fps`, looping (`.looping = false` holds the last one).
inline animation animation_of(const sprite_sheet& s, std::uint32_t first, std::uint32_t count, float fps) {
    return animation{s, first, count > 0 ? count : 1, fps, true, 0};
}
/// The frame an animation shows now.
inline std::uint32_t current_frame(const animation& a) {
    const float t = a.time * a.fps;
    const std::uint32_t n = t > 0 ? std::uint32_t(t) : 0;
    return a.first + (a.looping ? n % a.frames : std::min(n, a.frames - 1));
}
/// Whether a non-looping animation has played to its end.
inline bool finished(const animation& a) { return !a.looping && a.time * a.fps >= float(a.frames); }
/// A tile value that draws nothing.
constexpr std::uint16_t tile_empty = PONKO_TILE_EMPTY;
/// Which entities take their turn this tick (ctx::turns).
struct turns {
    PonkoTurns raw{1, 0, 0};
    /// Whether entity `e` takes its turn this tick.
    ///@{
    bool includes(PonkoEntity e) const noexcept { return e.index % raw.every == raw.now; }
    bool includes(entity e) const noexcept { return std::uint32_t(e) % raw.every == raw.now; }
    ///@}
    /// Seconds a whole round covers (`every` ticks).
    float dt() const noexcept { return raw.dt; }
};

/// The engine's random number generator (xoshiro256++), bit-identical to Rust's `Rng`,
/// so a game gives the same numbers in both languages.
class rng {
    std::uint64_t s_[4];
    static std::uint64_t rotl(std::uint64_t x, int k) { return (x << k) | (x >> (64 - k)); }
public:
    /// The SplitMix64 step used for seeding and for stream(): spreads the bits of `z`.
    static std::uint64_t mix(std::uint64_t z) {
        z += 0x9E3779B97F4A7C15ull;
        z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9ull;
        z = (z ^ (z >> 27)) * 0x94D049BB133111EBull;
        return z ^ (z >> 31);
    }
    /// A generator seeded with `seed`; the same seed gives the same numbers as Rust's `Rng::new`.
    explicit rng(std::uint64_t seed) {
        s_[0] = mix(seed); s_[1] = mix(s_[0]); s_[2] = mix(s_[1]); s_[3] = mix(s_[2]);
    }
    /// The next 64 random bits.
    std::uint64_t u64() {
        const std::uint64_t out = rotl(s_[0] + s_[3], 23) + s_[0];
        const std::uint64_t t = s_[1] << 17;
        s_[2] ^= s_[0]; s_[3] ^= s_[1]; s_[1] ^= s_[2]; s_[0] ^= s_[3]; s_[2] ^= t; s_[3] = rotl(s_[3], 45);
        return out;
    }
    /// 0 <= x < 1.
    float f32() { return float(u64() >> 40) * (1.0f / float(1u << 24)); }
    /// `low` <= x < `high`.
    float range(float low, float high) { return low + (high - low) * f32(); }
    /// 0 <= x < n.
    std::uint32_t below(std::uint32_t n) { return std::uint32_t(((u64() >> 32) * std::uint64_t(n)) >> 32); }
    /// True with probability `p` (0..1).
    bool chance(float p) { return f32() < p; }
    /// An angle in radians, 0 <= x < 2π.
    float angle() { return f32() * 6.28318530717958647692f; }
    /// A random unit vector, every direction equally likely. Bit-identical to Rust's
    /// `Rng::unit_vec2` on every platform: integers and `sqrt`, no `sin` or `cos`.
    vec2 unit_vec2() {
        constexpr std::int64_t half = std::int64_t(1) << 23;
        for (;;) {
            const std::uint64_t bits = u64();
            const std::int64_t x = std::int64_t(bits >> 40) - half;
            const std::int64_t y = std::int64_t((bits >> 16) & 0xFFFFFFu) - half;
            const std::int64_t d2 = x * x + y * y;
            if (d2 != 0 && d2 <= half * half) {
                const double d = std::sqrt(double(d2));
                return {float(double(x) / d), float(double(y) / d)};
            }
        }
    }
};
/// This tick and system's random streams (Rust `Random`): `of(e)` per entity,
/// `stream(n)` for anything else. Same numbers on every run with the same seed.
class random {
    std::uint64_t base_;
public:
    /// The streams of a tick and system; ctx::random gives one, so you rarely make it yourself.
    explicit random(std::uint64_t bits) : base_(bits) {}
    /// Stream `n` of this tick and system: a fresh generator, the same for the same `n`.
    rng stream(std::uint64_t n) const { return rng(base_ ^ rng::mix(n)); }
    /// The stream of entity `e`: the same entity gets the same numbers this tick, whatever the iteration
    /// order.
    rng of(entity e) const { return stream(e); }
};

class ctx;
class contact_pair3d;
/// What the wrapper throws when an engine call fails (Rust's `ponko::error::Error`):
/// `what()` is `ponko_last_error()`, `kind()` is `ponko_last_error_kind()`
/// (the C side of Rust's `ponko::error::ErrorKind`), `path()` the asset the failure
/// is about, if any (`ponko_last_error_path()`). It is a `std::runtime_error`,
/// so existing `catch (const std::runtime_error&)` blocks still catch it.
class error : public std::runtime_error {
public:
    /// An error with `message`, `kind` and the asset `path` it is about (empty: none). The wrapper makes
    /// these from ponko_last_error(); you rarely need to.
    error(const char* message, PonkoErrorKind kind, const char* path = "")
        : std::runtime_error(message), kind_(kind), path_(path ? path : "") {}
    /// What kind of failure it was; match on this, never on the message. New kinds can arrive in any release,
    /// so a `switch` needs a `default:`.
    PonkoErrorKind kind() const noexcept { return kind_; }
    /// The asset path the failure is about (as passed to the call), or empty.
    const std::string& path() const noexcept { return path_; }
private:
    PonkoErrorKind kind_;
    std::string path_;
};
/// A short name for an error kind, for logs: "not found", "shader", ...
inline const char* kind_name(PonkoErrorKind kind) noexcept { return ponko_error_kind_name(static_cast<std::uint32_t>(kind)); }

namespace detail {
/// The last failure on this thread as an exception.
inline ponko::error last_error() { return ponko::error(ponko_last_error(), ponko_last_error_kind(), ponko_last_error_path()); }
inline void check(PonkoStatus status) {
    if (status != PONKO_OK) throw last_error();
}
/// Throws if the loaded library's ABI version differs from the one this header
/// was compiled against (`PONKO_ABI_VERSION`), e.g. a stale ponko_ffi next to
/// a newer ponko.hpp. Takes both numbers so tests can try a mismatch.
inline void check_abi_version(std::uint32_t header, std::uint32_t loaded) {
    if (header != loaded) {
        throw ponko::error(("ponko_ffi reports C ABI version " + std::to_string(loaded) + " but ponko.h expects version " +
                            std::to_string(header) + ": the library and the headers come from different Ponko releases")
                               .c_str(),
                           PONKO_ERROR_UNSUPPORTED);
    }
}
/// The startup check: asks the library once, then compares on every call (a
/// mismatch keeps failing instead of only the first caller seeing it).
inline void check_abi_version() {
    static const std::uint32_t loaded = ponko_abi_version();
    check_abi_version(PONKO_ABI_VERSION, loaded);
}
}  // namespace detail

/// Writes a crash report for the game's own fatal error, `message`, and runs the hook (Rust
/// `ponko::crash::report`): for a game about to stop without crashing. False when reports are off.
inline bool report_crash(const char* message) {
    const PonkoStatus status = ponko_crash_report(message);
    if (status == PONKO_NOT_FOUND) return false;
    detail::check(status);
    return true;
}

// Versioned descriptors (STABILITY.md): structs that start with struct_size and version, so a release can
// add fields at the end. The helpers below fill both header fields for this header's version and the
// defaults; change the fields you need and pass the struct by reference.

/// A material description that can grow (PonkoMaterialDescEx): `desc` is material_desc, plus a stencil.
using material_desc_ex = PonkoMaterialDescEx;
/// Navmesh bake settings that can grow (PonkoNavBakeSettingsEx): `settings` is nav_bake_settings.
using nav_bake_settings_ex = PonkoNavBakeSettingsEx;
/// Lighting that can grow (PonkoLightingEx): `lighting` is ponko::lighting, plus shadow_sun_size and
/// shadow_sun_step.
using lighting_ex = PonkoLightingEx;
/// A 2D collider that can grow (PonkoColliderEx): `collider`, its collision groups and polygon corners.
using collider_ex = PonkoColliderEx;
/// A 2D or 3D query filter that can grow (PonkoQueryFilterEx), for the ponko_ctx_*_ex queries.
using query_filter_ex = PonkoQueryFilterEx;
/// Post-processing that can grow (PonkoPostEx): `post` is ponko::post, plus split toning
/// (split_toning_shadows, split_toning_highlights, split_toning_balance), depth of field (dof_enabled,
/// dof_focus_distance, dof_focus_range, dof_blur), the sample count (msaa) and, from version 2, thin
/// objects in screen-space reflections (ssr_thin_objects: whole without TAA, about 0.2 ms more at
/// 2560x1440; the high and ultra presets turn it on) and, from version 3, the upscaler (a ponko::upscaler;
/// set it with `static_cast<std::uint32_t>(...)`).
using post_ex = PonkoPostEx;
/// HDR display output settings that can grow (PonkoHdrOutput): enabled, paper_white_nits, peak_nits
/// (0: the display's) and, from version 2, the HDR10 metadata max_content_light_nits and
/// max_frame_average_nits (0: the peak, paper white). Off by default; see hdr_output_defaults,
/// game::hdr_output, ctx::set_hdr_output.
using hdr_output = PonkoHdrOutput;
/// What the window does with the HDR settings (PonkoHdrStatus): available, active, encoding, reason,
/// paper white, peak and headroom as used, what the display reports (0 unknown) and, from version 2,
/// the HDR10 metadata the display was given (metadata_sent, then the four values).
using hdr_status = PonkoHdrStatus;
/// How a 3D frame drawn below the window's size (`render_scale` under 1) is brought up to it (the
/// `upscaler` of post_ex, Rust `Upscaler`).
enum class upscaler : std::uint32_t {
    /// One pass stretches the finished frame, sharpened by `sharpen`.
    bilinear = PONKO_UPSCALER_BILINEAR,
    /// Temporal upscaling on TAA's history and motion vectors: needs anti_aliasing::taa.
    temporal = PONKO_UPSCALER_TEMPORAL,
};
/// Whether a camera draws `ctx.debug`'s 3D lines and labels (the `debug_draw` of camera3d_ex and
/// target_camera_ex, Rust `Camera::debug_draw`); set it with `static_cast<std::uint32_t>(...)`.
enum class camera_debug_draw : std::uint32_t {
    /// As before the flag: the window's camera draws them, a target camera while it draws particles.
    as_default = PONKO_DEBUG_DRAW_DEFAULT,
    /// Always (on the default layer).
    on = PONKO_DEBUG_DRAW_ON,
    /// Never.
    off = PONKO_DEBUG_DRAW_OFF,
};
/// The 3D camera that can grow (PonkoCamera3DEx): `camera` is ponko::camera3d, plus occlusion_culling,
/// particles (whether it draws particles, trails and lines) and debug_draw (a ponko::camera_debug_draw).
using camera3d_ex = PonkoCamera3DEx;
/// A target camera that can grow (PonkoTargetCameraEx): `target_camera` is ponko::target_camera, plus
/// particles and debug_draw (a ponko::camera_debug_draw: whether it draws `ctx.debug`'s 3D drawing, whatever
/// particles says).
using target_camera_ex = PonkoTargetCameraEx;

/// A material_desc_ex holding `d` (no stencil).
inline material_desc_ex material_desc_ex_of(const PonkoMaterialDesc& d) {
    material_desc_ex x{};
    x.struct_size = sizeof(material_desc_ex);
    x.version = PONKO_MATERIAL_DESC_EX_VERSION;
    detail::check(ponko_material_desc_ex_defaults(&x));
    x.desc = d;
    return x;
}
/// The default navmesh bake, in its versioned form.
inline nav_bake_settings_ex nav_bake_ex_defaults() {
    nav_bake_settings_ex x{};
    x.struct_size = sizeof(nav_bake_settings_ex);
    x.version = PONKO_NAV_BAKE_SETTINGS_EX_VERSION;
    detail::check(ponko_nav_bake_settings_ex_defaults(&x));
    return x;
}
/// A lighting_ex with only its header set, to read into (ctx::lighting_ex does).
inline lighting_ex lighting_ex_header() {
    lighting_ex x{};
    x.struct_size = sizeof(lighting_ex);
    x.version = PONKO_LIGHTING_EX_VERSION;
    return x;
}
/// The lighting a new game starts with, in its versioned form.
inline lighting_ex lighting_ex_defaults() {
    lighting_ex x = lighting_ex_header();
    detail::check(ponko_lighting_ex_defaults(&x));
    return x;
}
/// A collider_ex with only its header set, to read into (world_view::get_collider_ex does).
inline collider_ex collider_ex_header() {
    collider_ex x{};
    x.struct_size = sizeof(collider_ex);
    x.version = PONKO_COLLIDER_EX_VERSION;
    return x;
}
/// `c` in its versioned form, in every collision group (set `groups` to change that).
inline collider_ex collider_ex_of(const PonkoCollider& c) {
    collider_ex x = collider_ex_header();
    detail::check(ponko_collider_ex_from(c, &x));
    return x;
}
/// A query filter that sees every solid collider in every group (no sensors), in its versioned form.
inline query_filter_ex query_filter_ex_defaults() {
    query_filter_ex x{};
    x.struct_size = sizeof(query_filter_ex);
    x.version = PONKO_QUERY_FILTER_EX_VERSION;
    detail::check(ponko_query_filter_ex_defaults(&x));
    return x;
}
/// `f` in its versioned form.
inline query_filter_ex query_filter_ex_of(const filter2d& f) {
    query_filter_ex x = query_filter_ex_defaults();
    x.exclude = f.raw.exclude;
    x.exclude_entity = f.raw.exclude_entity;
    x.groups = f.raw_groups;
    x.sensors = f.raw.sensors;
    return x;
}
/// `f` in its versioned form.
inline query_filter_ex query_filter_ex_of(const filter3d& f) {
    query_filter_ex x = query_filter_ex_defaults();
    x.exclude = f.raw.exclude;
    x.exclude_entity = f.raw.exclude_entity;
    x.groups = f.raw.groups;
    x.sensors = f.raw.sensors;
    return x;
}

/// HDR output settings with their defaults (off, 200-nit paper white, the display's peak).
inline hdr_output hdr_output_defaults() {
    hdr_output h{};
    h.struct_size = sizeof(hdr_output);
    h.version = PONKO_HDR_OUTPUT_VERSION;
    detail::check(ponko_hdr_output_defaults(&h));
    return h;
}

/// A post_ex with only its header set, to read into (ctx::post_ex does).
inline post_ex post_ex_header() {
    post_ex x{};
    x.struct_size = sizeof(post_ex);
    x.version = PONKO_POST_EX_VERSION;
    return x;
}
/// The post-processing a new game starts with, in its versioned form.
inline post_ex post_ex_defaults() {
    post_ex x = post_ex_header();
    detail::check(ponko_post_ex_defaults(&x));
    return x;
}
/// The settings `t` of the way from `a` to `b` (Rust `Post::lerp`), `t` clamped to 0 to 1: for a transition
/// between two looks. Amounts and colors blend; an effect on at one end and off at the other fades its
/// strength to nothing at the end where it is off; the tone map, lookup table, antialiasing, sample count,
/// render scale and ambient occlusion's quality switch halfway. Throws `ponko::error` for settings out of
/// range.
inline post_ex post_lerp(const post_ex& a, const post_ex& b, float t) {
    post_ex out = post_ex_header();
    detail::check(ponko_post_ex_lerp(&a, &b, t, &out));
    return out;
}
/// A box in the world with post settings of its own (Rust `PostVolume`): the camera inside it sees them,
/// and over `blend_distance` outside it they fade back into what is around it.
struct post_volume {
    /// The box's corner with the smallest coordinates.
    vec3 min;
    /// The box's corner with the largest coordinates.
    vec3 max;
    /// How far outside the box, in meters, its settings fade out, smoothly; 0 switches at the box's faces.
    float blend_distance;
    /// The settings inside the box.
    post_ex post;
};
/// A post_volume of `size` around `center`.
inline post_volume post_volume_around(vec3 center, vec3 size, float blend_distance, const post_ex& post) {
    const vec3 half{std::fabs(size.x) * 0.5f, std::fabs(size.y) * 0.5f, std::fabs(size.z) * 0.5f};
    return {{center.x - half.x, center.y - half.y, center.z - half.z},
            {center.x + half.x, center.y + half.y, center.z + half.z}, blend_distance, post};
}
/// `base` with `volumes` blended over it by where `at` (the camera's position) is, in order: where volumes
/// overlap, the later one wins, so list a room after the building around it (Rust `PostVolume::blend`). Give
/// the result to ctx::set_post every frame after the camera moved. Throws `ponko::error` for a volume with
/// its min past its max, a negative blend distance, or settings out of range.
inline post_ex blend_post_volumes(const post_ex& base, const std::vector<post_volume>& volumes, vec3 at) {
    std::vector<PonkoPostVolume> raw;
    raw.reserve(volumes.size());
    for (const post_volume& v : volumes) raw.push_back(PonkoPostVolume{v.min, v.max, v.blend_distance, &v.post});
    post_ex out = post_ex_header();
    detail::check(ponko_post_blend_volumes(&base, raw.data(), raw.size(), at, &out));
    return out;
}
/// A camera3d_ex holding `c`, with no occlusion culling, particles drawn and camera_debug_draw::as_default (set
/// `particles` and `debug_draw` to change that).
inline camera3d_ex camera3d_ex_of(const camera3d& c) {
    camera3d_ex x{};
    x.struct_size = sizeof(camera3d_ex);
    x.version = PONKO_CAMERA3D_EX_VERSION;
    detail::check(ponko_camera3d_ex_from(&c, &x));
    return x;
}
/// A target_camera_ex holding `t`, with particles drawn and camera_debug_draw::as_default (set `particles` and
/// `debug_draw` to change that).
inline target_camera_ex target_camera_ex_of(const target_camera& t) {
    target_camera_ex x{};
    x.struct_size = sizeof(target_camera_ex);
    x.version = PONKO_TARGET_CAMERA_EX_VERSION;
    detail::check(ponko_target_camera_ex_from(&t, &x));
    return x;
}

/// The levels of an instance batch drawing copies of a LOD group the way the group would (Rust
/// `LodGroup::instance_levels`): `levels` as for ctx::set_lod_group, each copy scaled by `scale`, for a
/// camera of vertical field of view `fov_y`, under `settings`. std::nullopt when the group can't be a
/// batch (off-center, offset or tinted renderers, or nothing drawn). Give the result to
/// ctx::create_instance_batch_levels.
inline std::optional<std::vector<instance_level>> lod_instance_levels(float size, const std::vector<lod_level>& levels,
    float scale, float fov_y, lod_settings settings, vec3 center = {0, 0, 0}) {
    std::vector<PonkoLodLevel> raw;
    raw.reserve(levels.size());
    for (const lod_level& l : levels) {
        raw.push_back(PonkoLodLevel{l.screen_height, l.fade_width, l.renderers.data(), l.renderers.size(),
                                     l.custom_shadow, l.shadow_casters.data(), l.shadow_casters.size()});
    }
    std::size_t n = 0;
    const PonkoStatus status = ponko_lod_instance_levels(size, center, raw.data(), raw.size(), scale, fov_y, settings, nullptr, 0, &n);
    if (status == PONKO_NOT_FOUND) return std::nullopt;
    detail::check(status);
    std::vector<instance_level> out(n);
    detail::check(ponko_lod_instance_levels(size, center, raw.data(), raw.size(), scale, fov_y, settings, out.data(), out.size(), &n));
    return out;
}

/// What the `try_` loaders return, Rust's `Result<T, ponko::error::Error>`: the
/// value, or the error that kept it from loading. Nothing is thrown until
/// you ask a failed result for its value.
///
///     auto skin = ctx.try_load_texture("skins/gold.png");
///     if (skin) use(*skin);
///     else if (skin.kind() == PONKO_ERROR_NOT_FOUND) { /* no skin installed */ }
template<class T> class result {
    std::optional<T> value_;
    std::optional<ponko::error> error_;
public:
    /// A result holding `value`.
    result(T value) : value_(std::move(value)) {}
    /// A failed result holding `e`.
    result(ponko::error e) : error_(std::move(e)) {}
    /// Whether it holds a value.
    bool ok() const noexcept { return value_.has_value(); }
    /// Same as ok(): `if (auto t = ctx.try_load_texture(path))`.
    explicit operator bool() const noexcept { return ok(); }
    /// The value; throws the error if it failed.
    T& value() & { if (!value_) throw *error_; return *value_; }
    ///@{
    const T& value() const& { if (!value_) throw *error_; return *value_; }
    T value() && { if (!value_) throw *error_; return std::move(*value_); }
    ///@}
    /// The value, as with value(): throws the error if it failed.
    ///@{
    T& operator*() & { return value(); }
    const T& operator*() const& { return value(); }
    const T* operator->() const { return &value(); }
    ///@}
    /// The value, or `fallback` if it failed. Never throws.
    T value_or(T fallback) const { return value_ ? *value_ : fallback; }
    /// The error; std::logic_error if it succeeded.
    const ponko::error& error() const {
        if (!error_) throw std::logic_error("ponko::result holds a value, not an error");
        return *error_;
    }
    /// The error's kind, or PONKO_ERROR_NONE if it succeeded.
    PonkoErrorKind kind() const noexcept { return error_ ? error_->kind() : PONKO_ERROR_NONE; }
};

/// What a `try_` loader that returns nothing gives back, Rust's
/// `Result<(), ponko::error::Error>`: success, or the error that kept it from
/// loading.
///
///     if (auto table = ctx.try_load_strings("lang/fr.json"); !table) log(table.error().what());
template<> class result<void> {
    std::optional<ponko::error> error_;
public:
    /// A successful result.
    result() = default;
    /// A failed result holding `e`.
    result(ponko::error e) : error_(std::move(e)) {}
    /// Whether it succeeded.
    bool ok() const noexcept { return !error_.has_value(); }
    /// Same as ok().
    explicit operator bool() const noexcept { return ok(); }
    /// Does nothing if it succeeded; throws the error if it failed.
    void value() const { if (error_) throw *error_; }
    /// The error; std::logic_error if it succeeded.
    const ponko::error& error() const {
        if (!error_) throw std::logic_error("ponko::result holds a value, not an error");
        return *error_;
    }
    /// The error's kind, or PONKO_ERROR_NONE if it succeeded.
    PonkoErrorKind kind() const noexcept { return error_ ? error_->kind() : PONKO_ERROR_NONE; }
};

/// The outcome of the latest input scenario run (Rust's `ScenarioReport`):
/// the C fields, plus `passed()`. See `world_view::scenario_report`.
struct scenario_report : PonkoScenarioReport {
    /// Whether the run ended with every check held.
    bool passed() const noexcept { return status == PONKO_SCENARIO_PASSED; }
};

namespace detail {
/// `value` if `status` is PONKO_OK, else the last error, as a result. Make
/// the call that fills `value` in its own statement first: the order in which
/// function arguments are evaluated is unspecified.
template<class T> ponko::result<T> attempt(PonkoStatus status, T value) {
    if (status == PONKO_OK) return ponko::result<T>(std::move(value));
    return ponko::result<T>(last_error());
}
/// Success if `status` is PONKO_OK, else the last error, as a `result<void>`.
inline ponko::result<void> attempt(PonkoStatus status) {
    if (status == PONKO_OK) return ponko::result<void>();
    return ponko::result<void>(last_error());
}
/// An owned C query filter (see with, without, or_, all_of).
struct filter_deleter { void operator()(PonkoFilter* f) const noexcept { ponko_filter_free(f); } };
using filter_ptr = std::unique_ptr<PonkoFilter, filter_deleter>;
/// The `PonkoHandleKind` of each handle type, for `ctx::is_live` and `ctx::free`.
template<class H> struct handle_kind;
template<> struct handle_kind<PonkoTexture> { static constexpr std::uint32_t value = PONKO_HANDLE_TEXTURE; };
template<> struct handle_kind<PonkoSound> { static constexpr std::uint32_t value = PONKO_HANDLE_SOUND; };
template<> struct handle_kind<PonkoFont> { static constexpr std::uint32_t value = PONKO_HANDLE_FONT; };
template<> struct handle_kind<PonkoMesh> { static constexpr std::uint32_t value = PONKO_HANDLE_MESH; };
template<> struct handle_kind<PonkoMaterial> { static constexpr std::uint32_t value = PONKO_HANDLE_MATERIAL; };
template<> struct handle_kind<PonkoShader> { static constexpr std::uint32_t value = PONKO_HANDLE_SHADER; };
template<> struct handle_kind<PonkoCompute> { static constexpr std::uint32_t value = PONKO_HANDLE_COMPUTE; };
template<> struct handle_kind<PonkoEffect> { static constexpr std::uint32_t value = PONKO_HANDLE_EFFECT; };
template<> struct handle_kind<PonkoGpuBuffer> { static constexpr std::uint32_t value = PONKO_HANDLE_GPU_BUFFER; };
template<> struct handle_kind<PonkoInstanceBatch> { static constexpr std::uint32_t value = PONKO_HANDLE_INSTANCE_BATCH; };
template<> struct handle_kind<PonkoTextureArray> { static constexpr std::uint32_t value = PONKO_HANDLE_TEXTURE_ARRAY; };
template<> struct handle_kind<PonkoSpriteShader> { static constexpr std::uint32_t value = PONKO_HANDLE_SPRITE_SHADER; };
template<> struct handle_kind<PonkoSpriteMaterial> { static constexpr std::uint32_t value = PONKO_HANDLE_SPRITE_MATERIAL; };
template<> struct handle_kind<ponko::render_target> { static constexpr std::uint32_t value = PONKO_HANDLE_RENDER_TARGET; };
template<class T> constexpr bool pod = std::is_trivially_copyable<T>::value &&
    std::is_trivially_destructible<T>::value && std::is_standard_layout<T>::value;
template<class T> using bare = typename std::remove_cv<typename std::remove_reference<T>::type>::type;

/// Reads text a C function writes into a buffer (call with a null buffer
/// to learn the size, then again to fill it; asks again if it grew).
template<class F> std::string read_text(F&& fill) {
    std::size_t capacity = 0;
    for (;;) {
        std::size_t len = 0;
        std::string s(capacity + 1, '\0');
        check(fill(capacity ? &s[0] : nullptr, capacity, &len));
        if (len < capacity || (capacity == 0 && len == 0)) { s.resize(len); return s; }
        capacity = len + 1;
    }
}

/// A small number per C++ type, so a world finds a type's component id by
/// indexing an array instead of a hash map.
inline std::size_t next_type_slot() { static std::atomic<std::size_t> next{0}; return next++; }
template<class T> std::size_t type_slot() { static const std::size_t slot = next_type_slot(); return slot; }

/// The name a component gets unless reflect() names it: the C++ type's name
/// with its namespaces, as the compiler spells it ("Health", "game::Unit").
template<class T> std::string make_type_name() {
#if defined(_MSC_VER) && !defined(__clang__)
    std::string s = __FUNCSIG__;
    const auto open = s.find("make_type_name<") + 15;
    s = s.substr(open, s.rfind(">(") - open);
    for (const char* prefix : {"struct ", "class ", "enum ", "union "})
        for (std::size_t at; (at = s.find(prefix)) != std::string::npos;) s.erase(at, std::strlen(prefix));
    return s;
#else
    const std::string s = __PRETTY_FUNCTION__;
    const auto open = s.find("T = ") + 4;
    return s.substr(open, s.find_first_of(";]", open) - open);
#endif
}
template<class T> const std::string& type_name() {
    static const std::string name = make_type_name<T>();
    return name;
}

/// The first exception thrown by a callback, kept until the C call returns.
struct first_error {
    std::mutex lock;
    std::exception_ptr error;
    std::atomic<bool> set{false};
    void capture() noexcept {
        std::lock_guard<std::mutex> guard(lock);
        if (!error) error = std::current_exception();
        set.store(true);
    }
    void rethrow() { if (error) std::rethrow_exception(error); }
};

/// Component ids of one world, by C++ type. Shared by a game, its ctx and
/// the world run_headless returns.
struct identity {
    std::vector<std::uint32_t> ids;  // type_slot -> id + 1 (0: not registered yet)
    std::unordered_map<std::uint32_t, std::type_index> types;  // id -> the C++ type using it
    // The with/without/or_/all_of filters of queries, built once per filter list.
    std::mutex filters_lock;
    std::unordered_map<std::type_index, filter_ptr> filters;
    // Commands and events whose type was first used inside a query, when the
    // world can't register it: replayed right after the system returns.
    std::mutex later_lock;
    std::vector<std::function<void(ctx&)>> later;
    // The contact hook (ctx::set_contact_hook3d) lives here, not in a ctx:
    // a ctx only lasts one system call, the hook as long as the game. An
    // exception it throws is kept in hook_error and rethrown by the game.
    std::function<void(contact_pair3d&)> contact_hook;
    // The terrain rebuild (ctx::set_terrain_rebuild), for the same reason,
    // already turned into what the C callback returns.
    std::function<PonkoCollisionMeshData*(const PonkoLoadedWorld*, std::uint64_t)> terrain_rebuild;
    first_error hook_error;
};

/// The parameter types of a function, lambda or function object, and
/// whether calling it can change it (a `mutable` lambda can).
template<class F> struct params : params<decltype(&F::operator())> {};
template<class R, class... A> struct params<R (*)(A...)> { using type = std::tuple<A...>; static constexpr bool changes = false; };
template<class R, class... A> struct params<R (*)(A...) noexcept> { using type = std::tuple<A...>; static constexpr bool changes = false; };
template<class C, class R, class... A> struct params<R (C::*)(A...)> { using type = std::tuple<A...>; static constexpr bool changes = true; };
template<class C, class R, class... A> struct params<R (C::*)(A...) const> { using type = std::tuple<A...>; static constexpr bool changes = false; };
template<class C, class R, class... A> struct params<R (C::*)(A...) noexcept> { using type = std::tuple<A...>; static constexpr bool changes = true; };
template<class C, class R, class... A> struct params<R (C::*)(A...) const noexcept> { using type = std::tuple<A...>; static constexpr bool changes = false; };

template<class M> struct field_type;
template<> struct field_type<float> { static constexpr PonkoFieldType value = PONKO_FIELD_F32; };
template<> struct field_type<double> { static constexpr PonkoFieldType value = PONKO_FIELD_F64; };
template<> struct field_type<std::int8_t> { static constexpr PonkoFieldType value = PONKO_FIELD_I8; };
template<> struct field_type<std::int16_t> { static constexpr PonkoFieldType value = PONKO_FIELD_I16; };
template<> struct field_type<std::int32_t> { static constexpr PonkoFieldType value = PONKO_FIELD_I32; };
template<> struct field_type<std::int64_t> { static constexpr PonkoFieldType value = PONKO_FIELD_I64; };
template<> struct field_type<std::uint8_t> { static constexpr PonkoFieldType value = PONKO_FIELD_U8; };
template<> struct field_type<std::uint16_t> { static constexpr PonkoFieldType value = PONKO_FIELD_U16; };
template<> struct field_type<std::uint32_t> { static constexpr PonkoFieldType value = PONKO_FIELD_U32; };
template<> struct field_type<std::uint64_t> { static constexpr PonkoFieldType value = PONKO_FIELD_U64; };
template<> struct field_type<bool> { static constexpr PonkoFieldType value = PONKO_FIELD_BOOL; };
template<> struct field_type<PonkoVec2> { static constexpr PonkoFieldType value = PONKO_FIELD_VEC2; };
template<> struct field_type<PonkoColor> { static constexpr PonkoFieldType value = PONKO_FIELD_COLOR; };
}
/// The settings of a quality preset (low, medium, high or ultra; throws for
/// quality_level::custom). Change single fields before applying them.
inline graphics_quality graphics_quality_preset(quality_level level) {
    graphics_quality q{}; detail::check(ponko_graphics_quality_preset(uint32_t(level), &q)); return q;
}
/// Which preset `q` is, or quality_level::custom. Throws if a field is out of range.
inline quality_level graphics_quality_level(const graphics_quality& q) {
    uint32_t level = 0; detail::check(ponko_graphics_quality_level(&q, &level)); return quality_level(level);
}
/// The quality meant for tier `t` (Rust `SceneSettings::set_recommended_quality`):
/// low with FXAA, medium or high, asking for nothing the tier lacks.
inline graphics_quality graphics_quality_for_tier(gpu_tier t) {
    graphics_quality q{}; detail::check(ponko_graphics_quality_for_tier(uint32_t(t), &q)); return q;
}
/// The JSON report a device with exactly tier `t`'s minimum limits gives
/// (Rust `GpuTier::report`): what the tier guarantees, and for the other
/// features the fallback and the reason. Needs no GPU.
inline std::string gpu_tier_report(gpu_tier t) {
    return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_gpu_tier_report(uint32_t(t), b, c, n); });
}
/// The plural form `count` takes in `language` (Rust `ponko::locale::plural_category`): "en" gives
/// `one` for 1, "ru" gives `few` for 3, "ar" gives `many` for 11.
inline plural_category plural(const char* language, double count) {
    PonkoPluralCategory c{}; detail::check(ponko_plural_category(language, count, &c)); return plural_category(c);
}
/// Whether every GPU of tier `t` has feature `f`. Needs no GPU.
inline bool gpu_tier_has(gpu_tier t, gpu_feature f) {
    bool on = false; detail::check(ponko_gpu_tier_feature(uint32_t(t), uint32_t(f), &on)); return on;
}
/// One described field of a component, from ponko::field.
template<class T> struct field_info { PonkoField raw; };
/// Describes member `m` of component `T` for reflection: tools and agents
/// see it as JSON under `name`, and world saves include it. Fields of type
/// float, double, (u)int8..64_t, bool, vec2 and color; entities are uint64_t.
template<class T, class M> field_info<T> field(const char* name, M T::*m) {
    static_assert(std::is_default_constructible<T>::value, "reflected components must be default-constructible");
    const T sample{};
    const auto offset = std::size_t(reinterpret_cast<const char*>(&(sample.*m)) - reinterpret_cast<const char*>(&sample));
    return {{name, std::uint32_t(detail::field_type<typename std::remove_cv<M>::type>::value), offset}};
}
/// Where a game's time and allocations went: frame times, systems, your
/// profile scopes, allocations, the slowest frames and warnings with advice.
/// From ctx::perf_report() or game::profile_headless(). It is the report's
/// JSON; text() reads well, check() and regressions() are for tests and CI.
class perf_report {
    std::string json_;
public:
    /// An empty report (no JSON).
    perf_report() = default;
    /// A report from its JSON (as json() or save() wrote it); not checked until used.
    explicit perf_report(std::string json) : json_(std::move(json)) {}
    /// The report as JSON, to save and compare later.
    const std::string& json() const noexcept { return json_; }
    /// The report for people to read.
    std::string text() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_perf_report_text(json_.c_str(), b, c, n); });
    }
    /// What is over `budget_json` (e.g. `{"frame_p99_ms": 2, "allocs_per_frame": 0,
    /// "systems": {"movement": 0.5}, "scopes": {"ai/pathing": 1}}`), one line
    /// each. Empty: every limit holds.
    std::string check(const std::string& budget_json) const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_perf_check(json_.c_str(), budget_json.c_str(), b, c, n); });
    }
    /// True if every limit in `budget_json` holds.
    bool within(const std::string& budget_json) const { return check(budget_json).empty(); }
    /// What got slower than in `baseline` (an earlier report), by more than
    /// `tolerance` (0.2 is 20%), one line each. Empty: nothing did.
    std::string regressions(const perf_report& baseline, float tolerance = 0.2f) const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_perf_compare(json_.c_str(), baseline.json_.c_str(), tolerance, b, c, n); });
    }
    /// Writes the JSON to `path`.
    void save(const char* path) const {
        std::ofstream out(path, std::ios::binary);
        out << json_;
        if (!out) throw std::runtime_error(std::string("could not write ") + path);
    }
    /// Reads a report written by save().
    static perf_report load(const char* path) {
        std::ifstream in(path, std::ios::binary);
        if (!in) throw std::runtime_error(std::string("could not read ") + path);
        std::stringstream text;
        text << in.rdbuf();
        return perf_report(text.str());
    }
};

/// The allocations counted so far on the calling thread while profiling was on (Rust
/// `perf::thread_alloc_counts`): read it twice on one thread and subtract to count what the code between
/// allocated there. A PONKO_PROFILE_SCOPE counts its own the same way.
inline alloc_counts thread_alloc_counts() noexcept { return ponko_thread_alloc_counts(); }

/// Bytes allocated minus bytes freed while profiling was on, for the whole process (Rust
/// `perf::net_alloc_bytes`): the library's memory, and the program's own with PONKO_COUNT_CPP_ALLOCATIONS.
/// Read it twice with profiling on and subtract: what the code between still holds. A loop that gives
/// back what it takes reads (close to) zero however long it runs; a leak grows with every pass.
inline std::int64_t net_alloc_bytes() noexcept { return ponko_net_alloc_bytes(); }

/// net_alloc_bytes for the calling thread (Rust `perf::thread_net_alloc_bytes`): bytes allocated minus bytes
/// freed on it while profiling was on, with what its parallel queries' workers did for it. Two readings on one
/// thread, subtracted, are what the code between them still holds, whatever other threads do meanwhile.
inline std::int64_t thread_net_alloc_bytes() noexcept { return ponko_thread_net_alloc_bytes(); }

/// Times the scope it lives in (see PONKO_PROFILE_SCOPE).
class profile_scope_guard {
    const PonkoProfileScope* scope_;
    std::uint64_t token_;
public:
    /// Starts timing `scope` (a static from PONKO_PROFILE_SCOPE); the destructor stops it.
    explicit profile_scope_guard(const PonkoProfileScope* scope) noexcept : scope_(scope), token_(ponko_profile_begin(scope)) {}
    ~profile_scope_guard() { ponko_profile_end(scope_, token_); }
    /// Not copyable: one guard times one scope once.
    ///@{
    profile_scope_guard(const profile_scope_guard&) = delete;
    profile_scope_guard& operator=(const profile_scope_guard&) = delete;
    ///@}
};

class world_view;
class ctx;
class hook_world;
/// A query filter: only entities whose `T` changed since the calling system
/// last ran. Use it as `world.each<ponko::changed<Health>>(fn)`, or in the
/// list of `query` and `count`. A filter also requires the entity to have
/// `T`, and starts tracking `T` (see world_view::track_changes). Several
/// filters must all pass; see any_of.
///
/// What counts as a change: spawn and set (also their `_later` forms), a
/// query that takes the component by `&` (every row it visits, written or
/// not; `const&` and copies only read), and mark_changed.
template<class T> struct changed {};
/// A query filter: only entities that got their `T` since the calling system
/// last ran: spawned with it, or `set` when they had none.
template<class T> struct added {};
/// A query filter that passes when at least one of `Fs` does:
/// `world.each<ponko::any_of<ponko::changed<Pos>, ponko::changed<Vel>>>(fn)`.
/// The entity must have every component the filters name. It must be the
/// query's only filter.
template<class... Fs> struct any_of {};

/// A query filter: only entities that have a `T` (it isn't a column). Rust:
/// `With<T>`. Filters in one list must all pass:
/// `world.each<ponko::with<Player>, ponko::without<Dead>>(fn)`.
template<class T> struct with {};
/// A query filter: only entities that don't have a `T`. Rust: `Without<T>`.
template<class T> struct without {};
/// A query filter that passes when every one of `Fs` does (an AND group,
/// which inside or_ is one arm). Rust: a tuple of filters. `all_of<>` passes
/// for every entity. Name a filter you use in many places:
/// `using living = ponko::all_of<ponko::or_<ponko::with<Player>, ponko::with<Enemy>>, ponko::without<Dead>>;`.
/// Hold with, without, all_of and or_ filters.
template<class... Fs> struct all_of {};
/// A query filter that passes when at least one of `Fs` does (`or` is a C++
/// keyword, so the name has an underscore). Rust: `Or<(..)>`. An arm can be
/// a `with`, a `without`, an all_of group or another or_:
///
///     world.each<ponko::or_<ponko::all_of<ponko::with<Player>, ponko::without<Ghost>>,
///                           ponko::with<Enemy>>>(fn);
///
/// `or_<>` passes for no entity. A query takes at most 4 clauses with 16
/// component predicates between them: an or_ of all_of groups is expanded, so
/// `(a AND b) OR c` is two clauses of two predicates each. A query over the
/// limits throws std::runtime_error. A component named twice is one slot,
/// and `with` and `without` of one component pass for no entity, as for the
/// Rust tuple. Unlike any_of, which picks rows by what changed,
/// or_ picks entities by the components they have; the two combine in one
/// query list.
template<class... Fs> struct or_ {};

/// A query hint, in the filter list: index this query in the world's
/// archetype index on its first use, however many archetypes it matches.
/// Rust: `query.cached()`.
///
///     world.each<ponko::cached, ponko::with<Boss>>([](Health& h) { ... });
///
/// Most queries don't need it: a world with many archetypes indexes the
/// queries that repeat by itself (see world_view::set_query_cache_policy).
/// A hint never changes which entities a query visits or their order.
struct cached {};
/// A query hint, in the filter list: test every archetype for this query
/// instead of using the world's archetype index. Rust: `query.uncached()`.
struct uncached {};

namespace ai { class brain_row; class blackboard_row; }

namespace detail {

template<class T> struct is_filter : std::false_type {};
template<class T> struct is_filter<changed<T>> : std::true_type {};
template<class T> struct is_filter<added<T>> : std::true_type {};
template<class... Fs> struct is_filter<any_of<Fs...>> : std::true_type {};
template<class T> struct is_filter<with<T>> : std::true_type {};
template<class T> struct is_filter<without<T>> : std::true_type {};
template<class... Fs> struct is_filter<all_of<Fs...>> : std::true_type {};
template<class... Fs> struct is_filter<or_<Fs...>> : std::true_type {};
template<> struct is_filter<cached> : std::true_type {};
template<> struct is_filter<uncached> : std::true_type {};
/// cached and uncached: they say how a query finds its archetypes, not
/// which entities or rows it visits.
template<class T> struct is_hint : std::false_type {};
template<> struct is_hint<cached> : std::true_type {};
template<> struct is_hint<uncached> : std::true_type {};
/// with, without, all_of and or_: the filters that pick entities by the
/// components they have, not rows by what changed.
template<class T> struct is_presence : std::false_type {};
template<class T> struct is_presence<with<T>> : std::true_type {};
template<class T> struct is_presence<without<T>> : std::true_type {};
template<class... Fs> struct is_presence<all_of<Fs...>> : std::true_type {};
template<class... Fs> struct is_presence<or_<Fs...>> : std::true_type {};
template<class T> struct is_any : std::false_type {};
template<class... Fs> struct is_any<any_of<Fs...>> : std::true_type {};
template<class... Ts> struct types {};
template<class A, class B> struct joined;
template<class... A, class... B> struct joined<types<A...>, types<B...>> { using type = types<A..., B...>; };
/// The types among `Ts` that are query filters (`Filters`), or the rest.
template<bool Filters, class... Ts> struct pick { using type = types<>; };
template<bool Filters, class T, class... Rest> struct pick<Filters, T, Rest...> {
    using type = typename joined<typename std::conditional<is_filter<T>::value == Filters, types<T>, types<>>::type,
                                 typename pick<Filters, Rest...>::type>::type;
};
/// The presence filters (see is_presence) among `Fs`.
template<class... Fs> struct presence { using type = types<>; };
template<class F, class... Rest> struct presence<F, Rest...> {
    using type = typename joined<typename std::conditional<is_presence<F>::value, types<F>, types<>>::type,
                                 typename presence<Rest...>::type>::type;
};
/// ai::brain_row and ai::blackboard_row: each() parameters that are the
/// world brain and blackboard columns, handed over as row objects.
template<class T> struct is_ai_row : std::false_type {};
template<> struct is_ai_row<ai::brain_row> : std::true_type {};
template<> struct is_ai_row<ai::blackboard_row> : std::true_type {};
/// The column type of an each() parameter: `T` for `T&` (written), `const T`
/// for `const T&` and copies (only read). Brain and blackboard rows are
/// always written: their calls change them.
template<class P> using column = typename std::conditional<
    is_ai_row<bare<P>>::value ||
        (std::is_lvalue_reference<P>::value && !std::is_const<typename std::remove_reference<P>::type>::value),
    bare<P>, const bare<P>>::type;
/// The component ids and options of one query.
struct query_plan {
    std::array<std::uint32_t, 32> ids{};
    std::size_t count = 0;
    PonkoQueryOptions options = ponko_query_options_default();
    std::uint32_t cache = PONKO_QUERY_CACHE_AUTO;
    /// The bit of column `id`, added as a read-only column if it's new.
    std::uint32_t bit(std::uint32_t id) {
        for (std::size_t i = 0; i < count; ++i) if (ids[i] == id) return std::uint32_t(1) << i;
        if (count == ids.size()) throw std::length_error("a query takes at most 32 components");
        ids[count] = id;
        return std::uint32_t(1) << count++;
    }
};
}  // namespace detail

/// One archetype's rows in a query: a column (array) per component, in the
/// order the query lists them. Valid only during the query callback. With a
/// changed/added filter, a chunk is a run of consecutive rows that pass.
template<class... Ts> class chunk {
    const PonkoChunk* raw_;
    template<std::size_t... I> std::tuple<Ts*...> columns(std::index_sequence<I...>) const noexcept {
        return std::tuple<Ts*...>{static_cast<Ts*>(raw_->columns[I])...};
    }
public:
    /// Wraps a C chunk; queries make these for you.
    explicit chunk(const PonkoChunk* raw) : raw_(raw) {}
    /// How many rows it has (each column holds this many values).
    std::size_t size() const noexcept { return raw_->count; }
    /// Where row 0 is in the whole query (rows are numbered 0..count() in the
    /// same order every time): row `i` of a query_par chunk can fill slot
    /// `first() + i` of a buffer sized with world::count().
    std::size_t first() const noexcept { return raw_->first; }
    /// The C chunk (columns, entities, count).
    const PonkoChunk* raw_chunk() const noexcept { return raw_; }
    /// All columns at once: `auto [pos, vel] = chunk.columns();`
    std::tuple<Ts*...> columns() const noexcept { return columns(std::index_sequence_for<Ts...>{}); }
    /// The entity in row `index`.
    ponko::entity entity(std::size_t index) const {
        if (index >= raw_->count) throw std::out_of_range("chunk entity index");
        return bits(raw_->entities[index]);
    }
    /// The entity array (`index`, `generation`); `bits()` turns one into an id.
    const PonkoEntity* entities() const noexcept { return raw_->entities; }
};
/// A tilemap on an entity (world_view::tilemap). Tile writes go straight
/// into the engine's map; change how it's drawn with `style()` + `apply()`.
class tilemap_ref {
    PonkoWorld* world_;
    entity e_;
    PonkoTileMap map_;
public:
    /// The tilemap on `e` in `world`; throws ponko::error if `e` has none. world_view::tilemap makes these.
    tilemap_ref(PonkoWorld* world, entity e) : world_(world), e_(e), map_{} {
        detail::check(ponko_tilemap_get(world, e, &map_));
    }
    /// Width of the map, in tiles.
    std::uint32_t width() const noexcept { return map_.width; }
    /// Height of the map, in tiles.
    std::uint32_t height() const noexcept { return map_.height; }
    /// All tiles, row by row (`width() * height()`); tile_empty is a hole.
    std::uint16_t* tiles() const noexcept { return map_.tiles; }
    /// Sets one tile (ignored outside the map); tile_empty clears it.
    void set(std::uint32_t x, std::uint32_t y, std::uint16_t frame) noexcept {
        if (x < map_.width && y < map_.height) map_.tiles[std::size_t(y) * map_.width + x] = frame;
    }
    /// The frame at (x, y), or nothing for holes and outside the map.
    std::optional<std::uint16_t> get(std::uint32_t x, std::uint32_t y) const noexcept {
        if (x >= map_.width || y >= map_.height) return std::nullopt;
        const std::uint16_t t = map_.tiles[std::size_t(y) * map_.width + x];
        if (t == tile_empty) return std::nullopt;
        return t;
    }
    /// The tile under world point `p` for a map whose top-left is `origin`.
    std::optional<std::array<std::uint32_t, 2>> tile_at(vec2 origin, vec2 p) const noexcept {
        const float tx = std::floor((p.x - origin.x) / map_.tile_size.x), ty = std::floor((p.y - origin.y) / map_.tile_size.y);
        if (!(tx >= 0 && ty >= 0 && tx < float(map_.width) && ty < float(map_.height))) return std::nullopt;
        return std::array<std::uint32_t, 2>{{std::uint32_t(tx), std::uint32_t(ty)}};
    }
    /// Sheet, tile size, z and tint; change them, then call apply().
    PonkoTileMap& style() noexcept { return map_; }
    /// Writes the style() changes back to the engine (Rust `TileMap` fields); throws ponko::error if the
    /// entity lost its map.
    void apply() { detail::check(ponko_tilemap_update(world_, e_, &map_)); }
};
namespace detail {
/// A C++ conversion for a save migration, owned by the world (freed by `free`).
struct json_convert {
    std::function<std::string(const std::string&)> fn;
    std::string result;
    static std::uint32_t call(void* user, const char* json, const char** out_json) {
        auto* self = static_cast<json_convert*>(user);
        try {
            self->result = self->fn(json);
            *out_json = self->result.c_str();
            return PONKO_OK;
        } catch (const std::exception& e) {
            self->result = e.what();
            *out_json = self->result.c_str();
        } catch (...) {
            *out_json = nullptr;
        }
        return PONKO_INVALID_ARGUMENT;
    }
    static void free(void* user) { delete static_cast<json_convert*>(user); }
};
} // namespace detail

/// The changes from one game save version to the next (Rust
/// `ponko::reflect::Migration`), from world_view::migration. Each call adds a
/// change at once; they run in the order added. Names are the JSON ones: a
/// component's reflected name and its field names. Conversions take a saved
/// value's JSON text and return the new one's; an exception fails the load
/// with its message.
class save_migration {
    PonkoWorld* world_;
    std::uint32_t from_;
    using convert = std::function<std::string(const std::string&)>;
    template<class F> save_migration& with(convert fn, F add) {
        auto* box = new detail::json_convert{std::move(fn), {}};
        detail::check(add(&detail::json_convert::call, static_cast<void*>(box), &detail::json_convert::free));
        return *this;
    }
    friend class world_view;
    save_migration(PonkoWorld* world, std::uint32_t from) : world_(world), from_(from) {}
public:
    /// Component `from` is called `to` now.
    save_migration& rename_component(const char* from, const char* to) { detail::check(ponko_save_migration_rename_component(world_, from_, from, to)); return *this; }
    /// Component `name` is gone (an entity left with nothing is dropped).
    save_migration& remove_component(const char* name) { detail::check(ponko_save_migration_remove_component(world_, from_, name)); return *this; }
    /// Field `from` of `component` is called `to` now.
    save_migration& rename_field(const char* component, const char* from, const char* to) { detail::check(ponko_save_migration_rename_field(world_, from_, component, from, to)); return *this; }
    /// `component` has a new field: older saves get `value_json` for it.
    save_migration& add_field(const char* component, const char* field, const char* value_json) { detail::check(ponko_save_migration_add_field(world_, from_, component, field, value_json)); return *this; }
    /// `component` lost its field `field`.
    save_migration& remove_field(const char* component, const char* field) { detail::check(ponko_save_migration_remove_field(world_, from_, component, field)); return *this; }
    /// Component `from` became several: `parts_json` is `[["Regen", ["hp_regen"]], ...]`.
    save_migration& split_component(const char* from, const char* parts_json) { detail::check(ponko_save_migration_split_component(world_, from_, from, parts_json)); return *this; }
    /// Components `from` became one, `into`, with all their fields.
    save_migration& merge_components(std::initializer_list<const char*> from, const char* into) {
        std::vector<const char*> names(from);
        detail::check(ponko_save_migration_merge_components(world_, from_, names.data(), names.size(), into));
        return *this;
    }
    /// Resource `from` is called `to` now.
    save_migration& rename_resource(const char* from, const char* to) { detail::check(ponko_save_migration_rename_resource(world_, from_, from, to)); return *this; }
    /// Resource `name` is gone.
    save_migration& remove_resource(const char* name) { detail::check(ponko_save_migration_remove_resource(world_, from_, name)); return *this; }
    /// Field `field` of `component` changed its type or meaning: `fn` turns each saved value into the new one.
    save_migration& convert_field(const char* component, const char* field, convert fn) {
        return with(std::move(fn), [&](PonkoJsonConvertFn f, void* u, PonkoUserFreeFn free) { return ponko_save_migration_convert_field(world_, from_, component, field, f, u, free); });
    }
    /// Replaces each value of `component` with what `fn` makes of it.
    save_migration& convert_component(const char* component, convert fn) {
        return with(std::move(fn), [&](PonkoJsonConvertFn f, void* u, PonkoUserFreeFn free) { return ponko_save_migration_convert_component(world_, from_, component, f, u, free); });
    }
    /// Replaces the saved value of resource `resource` with what `fn` makes of it.
    save_migration& convert_resource(const char* resource, convert fn) {
        return with(std::move(fn), [&](PonkoJsonConvertFn f, void* u, PonkoUserFreeFn free) { return ponko_save_migration_convert_resource(world_, from_, resource, f, u, free); });
    }
    /// `fn` gets each entity's components (a JSON object by name) and returns them changed.
    save_migration& change_entities(convert fn) {
        return with(std::move(fn), [&](PonkoJsonConvertFn f, void* u, PonkoUserFreeFn free) { return ponko_save_migration_change_entities(world_, from_, f, u, free); });
    }
    /// `fn` gets the whole save's JSON and returns it changed: the last resort.
    save_migration& change_save(convert fn) {
        return with(std::move(fn), [&](PonkoJsonConvertFn f, void* u, PonkoUserFreeFn free) { return ponko_save_migration_change_save(world_, from_, f, u, free); });
    }
};

/// A copy of a world in memory (Rust `WorldSnapshot`): every entity and
/// component, the entity allocator and the change history. Save into the
/// same snapshot again and again: it reuses its memory.
/// world_view::save_snapshot fills it, world_view::restore_snapshot puts it back.
class world_snapshot {
    PonkoWorldSnapshot* raw_;
public:
    /// An empty snapshot.
    world_snapshot() : raw_(ponko_world_snapshot_new()) {}
    ~world_snapshot() { ponko_world_snapshot_free(raw_); }
    /// Not copyable: it owns its memory.
    ///@{
    world_snapshot(const world_snapshot&) = delete;
    world_snapshot& operator=(const world_snapshot&) = delete;
    ///@}
    /// Entities in the snapshot.
    std::size_t size() const { std::size_t n = 0; detail::check(ponko_world_snapshot_len(raw_, &n)); return n; }
    /// The C handle, for ponko_world_save_snapshot and ponko_world_restore_snapshot.
    PonkoWorldSnapshot* raw() const noexcept { return raw_; }
};

/// Settings for ctx::rollback_start for player `player` of `players`: 2 ticks of input delay, rollbacks of
/// up to 8 ticks, world hashes compared every 60 ticks. Change the fields before starting.
inline PonkoRollbackConfig rollback_config(std::uint32_t players, std::uint32_t player) noexcept {
    return ponko_rollback_config_default(players, player);
}

/// The entities and their components (Rust `World`). Components are plain
/// structs, found by their C++ type: no registration needed. A view given to
/// a system must not escape the call.
class world_view {
    friend class world;
    friend class ctx;
protected:
    PonkoWorld* raw_;
    std::shared_ptr<detail::identity> owner_;
    world_view(PonkoWorld* raw, std::shared_ptr<detail::identity> owner) : raw_(raw), owner_(std::move(owner)) {}
    void valid() const { if (!raw_) throw std::logic_error("world has been moved from"); }
    std::vector<entity> hierarchy_list(PonkoStatus (*list)(PonkoWorld*, uint64_t, uint64_t*, std::size_t, std::size_t*),
                                       entity e) const {
        valid();
        std::size_t n = 0;
        detail::check(list(raw_, e, nullptr, 0, &n));
        std::vector<entity> out(n);
        if (n != 0) detail::check(list(raw_, e, out.data(), out.size(), &n));
        out.resize(n < out.size() ? n : out.size());
        return out;
    }

    template<class T> static void check_component() {
        static_assert(detail::pod<T>, "components must be plain structs: trivially copyable, standard layout");
        static_assert(!std::is_empty<T>::value || sizeof(T) == 1, "empty components must have size 1");
    }
    /// Registers `T` in `world` under `name` (or its type name) and remembers
    /// its id. Nothing if the world is busy (inside a query).
    template<class T> static std::optional<std::uint32_t> try_register(PonkoWorld* world, detail::identity& owner, const char* name) {
        check_component<T>();
        std::uint32_t id = 0;
        PonkoStatus status;
        if constexpr (std::is_same<T, transform2d>::value) status = ponko_transform_component(world, &id);
        else if constexpr (std::is_same<T, sprite>::value) status = ponko_sprite_component(world, &id);
        else if constexpr (std::is_same<T, sprite_blend>::value) status = ponko_blend_mode_component(world, &id);
        else if constexpr (std::is_same<T, sprite_material>::value) status = ponko_sprite_material_component(world, &id);
        else if constexpr (std::is_same<T, attached>::value) status = ponko_attached_component(world, &id);
        else if constexpr (std::is_same<T, attached3d>::value) status = ponko_attached3d_component(world, &id);
        else if constexpr (std::is_same<T, animation>::value) status = ponko_animation_component(world, &id);
        else if constexpr (std::is_same<T, rigid_body>::value) status = ponko_rigid_body_component(world, &id);
        else if constexpr (std::is_same<T, collider>::value) status = ponko_collider_component(world, &id);
        else if constexpr (std::is_same<T, velocity>::value) status = ponko_velocity_component(world, &id);
        else if constexpr (std::is_same<T, rigid_body3d>::value) status = ponko_rigid_body3d_component(world, &id);
        else if constexpr (std::is_same<T, collider3d>::value) status = ponko_collider3d_component(world, &id);
        else if constexpr (std::is_same<T, velocity3d>::value) status = ponko_velocity3d_component(world, &id);
        else if constexpr (std::is_same<T, character3d>::value) status = ponko_character3d_component(world, &id);
        else if constexpr (std::is_same<T, joint3d>::value) status = ponko_joint3d_component(world, &id);
        else if constexpr (std::is_same<T, contact_modifier3d>::value) status = ponko_contact_modifier3d_component(world, &id);
        else if constexpr (std::is_same<T, vehicle3d>::value) status = ponko_vehicle3d_component(world, &id);
        else if constexpr (std::is_same<T, terrain_collider>::value) status = ponko_terrain_collider_component(world, &id);
        else if constexpr (std::is_same<T, collider_focus>::value) status = ponko_collider_focus_component(world, &id);
        else if constexpr (std::is_same<T, transform3d>::value) status = ponko_transform3d_component(world, &id);
        else if constexpr (std::is_same<T, reflection_probe3d>::value) status = ponko_reflection_probe3d_component(world, &id);
        else if constexpr (std::is_same<T, decal3d>::value) status = ponko_decal3d_component(world, &id);
        else if constexpr (std::is_same<T, receives_decals>::value) status = ponko_receives_decals_component(world, &id);
        else if constexpr (std::is_same<T, lightmapped>::value) status = ponko_lightmapped_component(world, &id);
        else if constexpr (std::is_same<T, point_light2d>::value) status = ponko_point_light2d_component(world, &id);
        else if constexpr (std::is_same<T, spot_light2d>::value) status = ponko_spot_light2d_component(world, &id);
        else if constexpr (std::is_same<T, directional_light2d>::value) status = ponko_directional_light2d_component(world, &id);
        else if constexpr (std::is_same<T, point_light3d>::value) status = ponko_point_light3d_component(world, &id);
        else if constexpr (std::is_same<T, spot_light3d>::value) status = ponko_spot_light3d_component(world, &id);
        else if constexpr (std::is_same<T, model>::value) status = ponko_model_component(world, &id);
        else if constexpr (std::is_same<T, terrain_tile>::value) status = ponko_terrain_tile_component(world, &id);
        else if constexpr (std::is_same<T, ai::brain_row>::value) status = ponko_ai_brain_component(world, &id);
        else if constexpr (std::is_same<T, ai::blackboard_row>::value) status = ponko_ai_blackboard_component(world, &id);
        else status = ponko_component_register(world, name ? name : detail::type_name<T>().c_str(), sizeof(T), alignof(T), &id);
        if (status == PONKO_BUSY) return std::nullopt;
        detail::check(status);
        const std::type_index type(typeid(T));
        const auto known = owner.types.emplace(id, type);
        if (!known.second && known.first->second != type)
            throw std::invalid_argument("component name " + std::string(name ? name : detail::type_name<T>()) +
                                        " is already used by a different C++ type");
        const std::size_t slot = detail::type_slot<T>();
        if (owner.ids.size() <= slot) owner.ids.resize(slot + 1, 0);
        if (owner.ids[slot] != 0 && owner.ids[slot] != id + 1)
            throw std::logic_error("reflect<" + detail::type_name<T>() + ">(name, ...) must come before the component is first used");
        owner.ids[slot] = id + 1;
        return id;
    }
    template<class T> static std::uint32_t register_type(PonkoWorld* world, detail::identity& owner, const char* name) {
        if (auto id = try_register<T>(world, owner, name)) return *id;
        throw std::logic_error(detail::type_name<T>() + " is used for the first time while the world is borrowed by a query");
    }
    /// The component id of `T` in this world, registering it on first use;
    /// nothing if it's new and the world is busy.
    template<class T> static std::optional<std::uint32_t> find_id(PonkoWorld* world, detail::identity& owner) {
        const std::size_t slot = detail::type_slot<T>();
        if (slot < owner.ids.size() && owner.ids[slot] != 0) return owner.ids[slot] - 1;
        return try_register<T>(world, owner, nullptr);
    }
    template<class T> static std::uint32_t id_of(PonkoWorld* world, detail::identity& owner) {
        const std::size_t slot = detail::type_slot<T>();
        if (slot < owner.ids.size() && owner.ids[slot] != 0) return owner.ids[slot] - 1;
        return register_type<T>(world, owner, nullptr);
    }
    template<class T> std::uint32_t id() const { valid(); return id_of<T>(raw_, *owner_); }

    template<class T> void add_filter(detail::query_plan& plan, changed<T>*) const { plan.options.changed |= plan.bit(id<T>()); }
    template<class T> void add_filter(detail::query_plan& plan, added<T>*) const { plan.options.added |= plan.bit(id<T>()); }
    template<class... Fs> void add_filter(detail::query_plan& plan, any_of<Fs...>*) const {
        static_assert(!std::disjunction<detail::is_any<Fs>...>::value, "any_of can't hold another any_of: list its filters directly");
        static_assert(!std::disjunction<detail::is_presence<Fs>...>::value,
                      "any_of holds changed<T> and added<T>; use or_<...> to pick entities by the components they have");
        plan.options.any = true;
        (add_filter(plan, static_cast<Fs*>(nullptr)), ...);
    }
    void add_filter(detail::query_plan& plan, cached*) const { plan.cache = PONKO_QUERY_CACHE_ALWAYS; }
    void add_filter(detail::query_plan& plan, uncached*) const { plan.cache = PONKO_QUERY_CACHE_NEVER; }
    // with, without, all_of and or_ pick entities, not rows: they are not part of the plan.
    template<class F> std::enable_if_t<detail::is_presence<F>::value> add_filter(detail::query_plan&, F*) const {}

    /// The C filter of a presence filter, kept alive in `keep`.
    template<class T> PonkoFilter* build_filter(std::vector<detail::filter_ptr>& keep, with<T>*) const {
        return keep_filter(keep, ponko_filter_with(id<T>()));
    }
    template<class T> PonkoFilter* build_filter(std::vector<detail::filter_ptr>& keep, without<T>*) const {
        return keep_filter(keep, ponko_filter_without(id<T>()));
    }
    template<class... Fs> PonkoFilter* build_filter(std::vector<detail::filter_ptr>& keep, all_of<Fs...>*) const {
        if constexpr (sizeof...(Fs) == 0) {
            return keep_filter(keep, ponko_filter_always());
        } else {
            static_assert(std::conjunction<detail::is_presence<Fs>...>::value,
                          "all_of holds with, without, all_of and or_ filters");
            const std::array<const PonkoFilter*, sizeof...(Fs)> parts{{build_filter(keep, static_cast<Fs*>(nullptr))...}};
            return keep_filter(keep, ponko_filter_all(parts.data(), parts.size()));
        }
    }
    template<class... Fs> PonkoFilter* build_filter(std::vector<detail::filter_ptr>& keep, or_<Fs...>*) const {
        if constexpr (sizeof...(Fs) == 0) {
            return keep_filter(keep, ponko_filter_never());
        } else {
            static_assert(std::conjunction<detail::is_presence<Fs>...>::value,
                          "or_ holds with, without, all_of and or_ filters");
            const std::array<const PonkoFilter*, sizeof...(Fs)> parts{{build_filter(keep, static_cast<Fs*>(nullptr))...}};
            return keep_filter(keep, ponko_filter_any(parts.data(), parts.size()));
        }
    }
    static PonkoFilter* keep_filter(std::vector<detail::filter_ptr>& keep, PonkoFilter* built) {
        if (!built) throw std::runtime_error(ponko_last_error());
        keep.emplace_back(built);
        return built;
    }
    /// The filter of a query's presence filters `Ps` (all of them must pass),
    /// built on first use for this world and kept.
    template<class... Ps> const PonkoFilter* presence_filter(detail::types<Ps...>) const {
        std::lock_guard<std::mutex> guard(owner_->filters_lock);
        const std::type_index key(typeid(detail::types<Ps...>));
        auto found = owner_->filters.find(key);
        if (found != owner_->filters.end()) return found->second.get();
        std::vector<detail::filter_ptr> keep;  // the parts: the filter holds copies
        const PonkoFilter* all = build_filter(keep, static_cast<all_of<Ps...>*>(nullptr));
        owner_->filters.emplace(key, std::move(keep.back()));  // the last one built is the whole filter
        return all;
    }
    /// Runs `fn(chunk<Ts...>)` for every archetype with all of `Ts`, or for
    /// every run of rows that pass the filters `Fs`. A `const` column is only
    /// read; the others count as changed for every row visited.
    template<class... Ts, class... Fs, class F> void run_query(detail::types<Ts...>, detail::types<Fs...>, bool parallel, F& fn) {
        static_assert(sizeof...(Ts) + (std::size_t(!detail::is_hint<Fs>::value) + ... + 0) > 0, "a query needs at least one component");
        static_assert((std::size_t(detail::is_hint<Fs>::value) + ... + 0) <= 1, "a query takes one of ponko::cached and ponko::uncached");
        static_assert(sizeof...(Ts) <= 32, "a query takes at most 32 components");
        static_assert(!std::disjunction<std::is_same<detail::bare<Ts>, sprite>...>::value,
                      "sprite is not a query column: query transform2d (and your own components) instead");
        static_assert(!std::disjunction<std::is_same<detail::bare<Ts>, rigid_body>..., std::is_same<detail::bare<Ts>, collider>...>::value,
                      "rigid_body and collider are not query columns: use get/set (velocity is fine)");
        static_assert(!std::disjunction<std::is_same<detail::bare<Ts>, rigid_body3d>..., std::is_same<detail::bare<Ts>, collider3d>...,
                                        std::is_same<detail::bare<Ts>, character3d>..., std::is_same<detail::bare<Ts>, joint3d>...,
                                        std::is_same<detail::bare<Ts>, contact_modifier3d>...>::value,
                      "rigid_body3d, collider3d, character3d, joint3d and contact_modifier3d are not query columns: use get/set (velocity3d is fine)");
        static_assert(!std::disjunction<detail::is_any<Fs>...>::value ||
                          (std::size_t(!detail::is_presence<Fs>::value && !detail::is_hint<Fs>::value) + ... + 0) == 1,
                      "any_of must be the query's only changed/added filter");
        valid();
        detail::query_plan plan;
        plan.options.writes = 0;
        plan.options.parallel = parallel;
        ((plan.options.writes |= std::uint32_t(!std::is_const<Ts>::value) << plan.count,
          plan.ids[plan.count++] = id<detail::bare<Ts>>()), ...);
        (add_filter(plan, static_cast<Fs*>(nullptr)), ...);
        struct context { F* fn; detail::first_error error; } c{&fn, {}};
        auto trampoline = [](const PonkoChunk* raw, void* user) noexcept {
            auto& c = *static_cast<context*>(user);
            if (c.error.set.load()) return;
            try { (*c.fn)(chunk<Ts...>(raw)); } catch (...) { c.error.capture(); }
        };
        PonkoStatus status;
        if constexpr (std::disjunction<detail::is_hint<Fs>...>::value) {
            const PonkoFilter* filter = nullptr;
            if constexpr (std::disjunction<detail::is_presence<Fs>...>::value) {
                filter = presence_filter(typename detail::presence<Fs...>::type{});
            }
            status = ponko_query_cached(raw_, plan.ids.data(), plan.count, filter, &plan.options, plan.cache, trampoline, &c);
        } else if constexpr (std::disjunction<detail::is_presence<Fs>...>::value) {
            const PonkoFilter* filter = presence_filter(typename detail::presence<Fs...>::type{});
            status = ponko_query_filtered(raw_, plan.ids.data(), plan.count, filter, &plan.options, trampoline, &c);
        } else {
            status = ponko_query_with(raw_, plan.ids.data(), plan.count, &plan.options, trampoline, &c);
        }
        c.error.rethrow();
        detail::check(status);
    }
    template<class... Ts, class... Fs> std::size_t count_rows(detail::types<Ts...>, detail::types<Fs...> filters) {
        std::size_t n = 0;
        auto add = [&n](chunk<const detail::bare<Ts>...> c) { n += c.size(); };
        run_query(detail::types<const detail::bare<Ts>...>{}, filters, false, add);
        return n;
    }
    /// each(): calls `fn` per row with the row's entity (if `fn` asks for one)
    /// and its components. Inlined into a plain loop over the columns.
    template<bool WithEntity, class... Ts> struct each_row {
        template<class F, std::size_t... I> static void chunk_rows(F& fn, const PonkoChunk* raw, std::index_sequence<I...> rows) {
            // A local copy of a function that can't change itself does the
            // same, and lets the compiler keep its captures in registers:
            // writes to components can't touch a local the engine never saw.
            if constexpr (!detail::params<F>::changes && std::is_copy_constructible<F>::value) {
                const F local(fn);
                rows_of(local, raw, rows);
            } else {
                rows_of(fn, raw, rows);
            }
        }
        /// Row `i` of column `column`: the value, or a brain or blackboard row object.
        template<class T> static decltype(auto) cell(const PonkoChunk* raw, std::size_t column, T* values, std::size_t i) {
            if constexpr (detail::is_ai_row<std::remove_const_t<T>>::value) return std::remove_const_t<T>::at(raw, column, i);
            else return (values[i]);
        }
        template<class F, std::size_t... I> static void rows_of(F& fn, const PonkoChunk* raw, std::index_sequence<I...>) {
            const std::tuple<Ts*...> cols{static_cast<Ts*>(raw->columns[I])...};
            const std::size_t n = raw->count;
            if constexpr (WithEntity) {
                const PonkoEntity* es = raw->entities;
                for (std::size_t i = 0; i < n; ++i)
                    fn(entity(es[i].generation) << 32 | es[i].index, cell(raw, I, std::get<I>(cols), i)...);
            } else {
                for (std::size_t i = 0; i < n; ++i) fn(cell(raw, I, std::get<I>(cols), i)...);
            }
        }
        template<class F, class Filters> static void run(world_view& w, Filters filters, bool parallel, F& fn) {
            auto per_chunk = [&fn](chunk<Ts...> c) { chunk_rows(fn, c.raw_chunk(), std::index_sequence_for<Ts...>{}); };
            w.run_query(detail::types<Ts...>{}, filters, parallel, per_chunk);
        }
    };
    template<class Args> struct each_of;
    template<class First, class... Rest> struct each_of<std::tuple<First, Rest...>> {
        static constexpr bool with_entity = std::is_same<First, entity>::value;
        template<class F, class Filters> static void run(world_view& w, Filters filters, bool parallel, F& fn) {
            if constexpr (with_entity) each_row<true, detail::column<Rest>...>::run(w, filters, parallel, fn);
            else each_row<false, detail::column<First>, detail::column<Rest>...>::run(w, filters, parallel, fn);
        }
    };
public:
    // ---- entities and components ----
    /// A new entity with these components: `world.spawn(ponko::transform_at({0, 0}), Health{100})`.
    template<class... Ts> entity spawn(const Ts&... components) {
        valid();
        constexpr std::size_t polygons = (std::size_t{std::is_same<Ts, polygon>::value} + ... + 0);
        static_assert(polygons <= 1, "one collider per entity");
        entity result = 0;
        if constexpr (polygons == 0) {
            std::array<PonkoValue, sizeof...(Ts)> args{{PonkoValue{id<Ts>(), &components, sizeof(Ts)}...}};
            detail::check(ponko_spawn(raw_, args.data(), args.size(), &result));
        } else {
            // C has no polygon component value: spawn the rest, then add it.
            std::array<PonkoValue, sizeof...(Ts) - 1> args{};
            std::size_t n = 0;
            const polygon* shape = nullptr;
            auto add = [&](const auto& c) {
                using C = std::decay_t<decltype(c)>;
                if constexpr (std::is_same<C, polygon>::value) shape = &c;
                else args[n++] = PonkoValue{id<C>(), &c, sizeof(C)};
            };
            (add(components), ...);
            detail::check(ponko_spawn(raw_, args.data(), args.size(), &result));
            const auto status = ponko_set_polygon_collider(raw_, result, shape->points.data(), shape->count, *shape);
            if (status != PONKO_OK) ponko_despawn(raw_, result);
            detail::check(status);
        }
        return result;
    }
    /// Despawns `e` and drops its components (Rust `World::despawn`). Throws ponko::error if `e` is already
    /// dead; its id never comes back to life.
    void despawn(entity e) { valid(); detail::check(ponko_despawn(raw_, e)); }
    /// Whether `e` is alive (a despawned id stays dead, even after its slot is reused).
    bool alive(entity e) const { valid(); bool result = false; detail::check(ponko_is_alive(raw_, e, &result)); return result; }
    // The hierarchy (Rust `WorldHierarchy`): the link is an attachment, children are kept in order.
    /// Makes `child` hang off `parent`, last among its children, without moving it. Throws if either is
    /// dead, the link would make a loop, or they aren't both 2D or both 3D.
    void set_parent(entity child, entity parent) { valid(); detail::check(ponko_set_parent(raw_, child, parent)); }
    /// Makes `child` a root where it is now; false if it had no parent.
    bool remove_parent(entity child) { valid(); bool had = false; detail::check(ponko_remove_parent(raw_, child, &had)); return had; }
    /// The entity `e` hangs off, or nothing for a root.
    std::optional<entity> parent(entity e) const {
        valid();
        entity p = 0;
        const PonkoStatus status = ponko_parent(raw_, e, &p);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return p;
    }
    /// `e`'s children, in order.
    std::vector<entity> children(entity e) const { return hierarchy_list(ponko_children, e); }
    /// Everything under `e`, depth first: each entity before its children, siblings in order.
    std::vector<entity> descendants(entity e) const { return hierarchy_list(ponko_descendants, e); }
    /// The entities above `e`, its parent first.
    std::vector<entity> ancestors(entity e) const { return hierarchy_list(ponko_ancestors, e); }
    /// Moves `child` to position `index` among its siblings (last if past the end). Throws if it has no parent.
    void set_child_index(entity child, std::size_t index) { valid(); detail::check(ponko_set_child_index(raw_, child, index)); }
    /// Despawns `e` and everything under it; returns how many entities that was.
    std::size_t despawn_recursive(entity e) { valid(); std::size_t n = 0; detail::check(ponko_despawn_recursive(raw_, e, &n)); return n; }
    /// Brings children lists and the transforms of changed subtrees up to date now
    /// (game::follow_attachments() does it once per tick).
    void update_hierarchy() { valid(); detail::check(ponko_update_hierarchy(raw_)); }
    /// Whether world saves hold `e` (Rust `NotSaved`): false leaves it out of
    /// saves, scenes and save slots, true puts it back. Levels mark everything
    /// but their prefab objects. Throws if `e` is dead.
    void set_saved(entity e, bool saved) { valid(); detail::check(ponko_world_set_saved(raw_, e, saved)); }
    /// Whether world saves hold `e` (see set_saved). Throws if `e` is dead.
    bool saved(entity e) const { valid(); bool result = false; detail::check(ponko_world_is_saved(raw_, e, &result)); return result; }
    /// Whether only the shadow maps draw `e`'s model (Rust `ShadowOnly`):
    /// true hides it from every camera while its shadow still falls, false
    /// shows it again. Saves and scene files keep it. Throws if `e` is dead.
    void set_shadow_only(entity e, bool shadow_only) { valid(); detail::check(ponko_world_set_shadow_only(raw_, e, shadow_only)); }
    /// Whether only the shadow maps draw `e`'s model (see set_shadow_only).
    /// Throws if `e` is dead.
    bool shadow_only(entity e) const { valid(); bool result = false; detail::check(ponko_world_is_shadow_only(raw_, e, &result)); return result; }
    /// Puts `e` (a 3D model or a 2D sprite) on render layers `layers` (Rust `RenderLayers`, bits): a camera draws
    /// it if its layers share one; a 3D model a camera leaves out still casts its shadow there.
    /// render_layers_default takes the component off. Throws if `e` is dead.
    void set_render_layers(entity e, std::uint32_t layers) { valid(); detail::check(ponko_world_set_render_layers(raw_, e, layers)); }
    /// The render layers `e` is on (render_layers_default without any). Throws if `e` is dead.
    std::uint32_t render_layers(entity e) const { valid(); std::uint32_t result = 0; detail::check(ponko_world_render_layers(raw_, e, &result)); return result; }
    /// How many entities are alive.
    std::size_t size() const { valid(); std::size_t result = 0; detail::check(ponko_world_len(raw_, &result)); return result; }
    /// A copy of `e`'s `T`; throws if `e` is dead or has no `T` (see try_get).
    template<class T> T get(entity e) const {
        T result{}; detail::check(ponko_get(raw_, e, id<T>(), &result, sizeof(T))); return result;
    }
    /// A copy of `e`'s `T`, or nothing if `e` is dead or has no `T`.
    template<class T> std::optional<T> try_get(entity e) const {
        T result{};
        const auto status = ponko_get(raw_, e, id<T>(), &result, sizeof(T));
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return result;
    }
    /// Whether `e` is alive and has a `T`.
    template<class T> bool has(entity e) const { return try_get<T>(e).has_value(); }
    /// Adds or replaces `e`'s component of this type: `world.set(e, Health{50})`.
    template<class T> void set(entity e, const T& component) {
        detail::check(ponko_set(raw_, e, id<T>(), &component, sizeof(T)));
    }
    /// Removes `e`'s `T` (nothing if it has none). Throws ponko::error if `e` is dead.
    template<class T> void remove(entity e) { detail::check(ponko_remove(raw_, e, id<T>())); }
    /// Gives `e` a polygon collider (see polygon_collider).
    void set(entity e, const polygon& shape) {
        detail::check(ponko_set_polygon_collider(raw_, e, shape.points.data(), shape.count, shape));
    }
    /// Puts `e`'s 2D collider in collision groups `g` (Rust `Collider::groups`). Setting a collider with
    /// set() gives it every group again, so set groups after it.
    void set_collider_groups(entity e, collision_groups g) { detail::check(ponko_set_collider_groups(raw_, e, g)); }
    /// Gives `e` a 2D collider with its groups (and polygon corners) in one call (collider_ex).
    void set(entity e, const collider_ex& c) { detail::check(ponko_set_collider_ex(raw_, e, &c)); }
    /// `e`'s 2D collider with its groups and polygon corners. Throws (kind not found) if it has none.
    collider_ex get_collider_ex(entity e) const {
        collider_ex c = collider_ex_header();
        detail::check(ponko_get_collider_ex(raw_, e, &c));
        return c;
    }
    /// `e`'s 2D collider groups.
    collision_groups collider_groups(entity e) const {
        collision_groups g{};
        detail::check(ponko_collider_groups(raw_, e, &g));
        return g;
    }
    /// `e`'s polygon corners (empty if its collider isn't a polygon).
    std::vector<vec2> collider_points(entity e) const {
        vec2 points[8];
        std::uint32_t n = 0;
        detail::check(ponko_collider_points(raw_, e, points, &n));
        return std::vector<vec2>(points, points + n);
    }

    // ---- resources: one value of a type per world, not on any entity ----
    /// Adds or replaces the world's `T`: `world.set_resource(Score{0, 0})`.
    template<class T> void set_resource(const T& value) {
        static_assert(detail::pod<T>, "resources are plain structs (trivially copyable, no destructor)");
        valid();
        detail::check(ponko_resource_set(raw_, detail::type_name<T>().c_str(), &value, sizeof(T), alignof(T)));
    }
    /// A copy of the world's `T`; throws if it has none (see try_resource).
    template<class T> T resource() const {
        auto value = try_resource<T>();
        if (!value) throw std::runtime_error("the world has no " + detail::type_name<T>() + " resource");
        return *value;
    }
    /// A copy of the world's `T`, or nothing.
    template<class T> std::optional<T> try_resource() const {
        static_assert(detail::pod<T>, "resources are plain structs (trivially copyable, no destructor)");
        valid();
        T result{};
        const auto status = ponko_resource_get(raw_, detail::type_name<T>().c_str(), &result, sizeof(T));
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return result;
    }
    /// Removes the world's `T`; returns whether it had one.
    template<class T> bool remove_resource() {
        valid();
        const auto status = ponko_resource_remove(raw_, detail::type_name<T>().c_str());
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// Lends the world's `T` and the world next to it to `fn(world_view&, T&)`
    /// (Rust's `resource_scope`) and returns what `fn` returns. Change `T` in
    /// place while you query, spawn and use other resources: run a query in
    /// `fn` to use `T` in its loop, nest scopes for several resources. Inside,
    /// the world has no `T`; it is back, with the changes, when `fn` returns
    /// or throws. Throws if the world has no `T`.
    template<class T, class F> auto resource_scope(F fn) -> decltype(fn(std::declval<world_view&>(), std::declval<T&>())) {
        static_assert(detail::pod<T>, "resources are plain structs (trivially copyable, no destructor)");
        valid();
        using R = decltype(fn(std::declval<world_view&>(), std::declval<T&>()));
        struct state {
            F* fn;
            std::shared_ptr<detail::identity> owner;
            std::exception_ptr error;
            std::optional<typename std::conditional<std::is_void<R>::value, int, R>::type> result;
        } st{&fn, owner_, nullptr, std::nullopt};
        auto trampoline = [](PonkoWorld* w, void* value, void* user) noexcept {
            auto& s = *static_cast<state*>(user);
            try {
                world_view view(w, s.owner);
                if constexpr (std::is_void<R>::value) (*s.fn)(view, *static_cast<T*>(value));
                else s.result.emplace((*s.fn)(view, *static_cast<T*>(value)));
            } catch (...) { s.error = std::current_exception(); }
        };
        const auto status = ponko_resource_scope(raw_, detail::type_name<T>().c_str(), sizeof(T), trampoline, &st);
        if (st.error) std::rethrow_exception(st.error);
        detail::check(status);
        if constexpr (!std::is_void<R>::value) return std::move(*st.result);
    }

    /// The latest input scenario run's report, kept in the world; nothing if
    /// no scenario ran here.
    std::optional<ponko::scenario_report> scenario_report() const {
        valid();
        ponko::scenario_report r{};
        const auto status = ponko_world_scenario_report(raw_, &r);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return r;
    }
    /// The same report as JSON, with the failed check's message, expected
    /// and actual values; empty if no scenario ran here.
    std::string scenario_report_json() const {
        valid();
        std::size_t len = 0;
        const auto status = ponko_world_scenario_report_json(raw_, nullptr, 0, &len);
        if (status == PONKO_NOT_FOUND) return {};
        detail::check(status);
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_world_scenario_report_json(raw_, b, c, l); });
    }

    // ---- queries ----
    /// Calls `fn` for every entity that has all the components `fn` takes:
    ///
    ///     world.each([](ponko::transform2d& t, const Velocity& v) { t.position += v.value; });
    ///
    /// Take components by `&` to change them, by `const&` to read them. A
    /// first `ponko::entity` parameter gets the entity. As fast as a
    /// hand-written loop over the columns (see query).
    ///
    /// Filters narrow it to what changed since the calling system last ran
    /// (see changed, added, any_of), or to the entities that have or lack
    /// components, without fetching them (see with, without, or_, all_of):
    ///
    ///     world.each<ponko::changed<Health>>([](ponko::entity e, const Health& h) { ... });
    ///     world.each<ponko::with<Player>, ponko::without<Dead>>([](Health& h) { ... });
    ///
    /// A component taken by `&` counts as changed for every entity visited,
    /// written or not; one taken by `const&` or by value doesn't.
    template<class... Filters, class F> void each(F&& fn) {
        static_assert(std::conjunction<detail::is_filter<Filters>...>::value,
                      "each<...> takes filters: ponko::changed<T>, ponko::added<T>, ponko::any_of<...>, ponko::with<T>, ponko::without<T>, ponko::all_of<...>, ponko::or_<...>, ponko::cached or ponko::uncached");
        using args = typename detail::params<detail::bare<F>>::type;
        each_of<args>::run(*this, detail::types<Filters...>{}, false, fn);
    }
    /// Like each, but on all CPU cores at once: `fn` must be thread-safe and
    /// only touch its own entity's components (plus ctx::dt/random and
    /// spatial queries).
    template<class... Filters, class F> void each_par(F&& fn) {
        static_assert(std::conjunction<detail::is_filter<Filters>...>::value,
                      "each_par<...> takes filters: ponko::changed<T>, ponko::added<T>, ponko::any_of<...>, ponko::with<T>, ponko::without<T>, ponko::all_of<...>, ponko::or_<...>, ponko::cached or ponko::uncached");
        using args = typename detail::params<detail::bare<F>>::type;
        each_of<args>::run(*this, detail::types<Filters...>{}, true, fn);
    }
    /// The lower-level loop: calls `fn(chunk)` for every archetype with all of
    /// `Ts`, with a column (array) per component, for code that works on
    /// whole arrays:
    ///
    ///     world.query<Position, Velocity>([](auto chunk) {
    ///         auto [pos, vel] = chunk.columns();
    ///         for (std::size_t i = 0; i < chunk.size(); ++i) pos[i].x += vel[i].x;
    ///     });
    ///
    /// A `const` column (`query<Position, const Velocity>`) is only read; the
    /// others count as changed. Filters go in the same list and aren't
    /// columns: `query<Position, ponko::changed<Position>>` gets the runs of
    /// rows whose Position changed.
    template<class... Ts, class F> void query(F&& fn) {
        static_assert(!std::disjunction<detail::is_ai_row<detail::bare<Ts>>...>::value,
                      "ai::brain_row and ai::blackboard_row are each() parameters; chunk loops over brains use ai::query");
        run_query(typename detail::pick<false, Ts...>::type{}, typename detail::pick<true, Ts...>::type{}, false, fn);
    }
    /// query on all CPU cores at once (see each_par).
    template<class... Ts, class F> void query_par(F&& fn) {
        static_assert(!std::disjunction<detail::is_ai_row<detail::bare<Ts>>...>::value,
                      "ai::brain_row and ai::blackboard_row are each_par() parameters; chunk loops over brains use ai::query_par");
        run_query(typename detail::pick<false, Ts...>::type{}, typename detail::pick<true, Ts...>::type{}, true, fn);
    }
    /// How many entities have all of these components (and pass these
    /// filters: `count<ponko::changed<Health>>()`). Counting changes nothing.
    template<class... Ts> std::size_t count() {
        return count_rows(typename detail::pick<false, Ts...>::type{}, typename detail::pick<true, Ts...>::type{});
    }
    /// The C component id of `T` in this world, registering it on first
    /// use: for C calls that take ids, such as ai::query's columns.
    template<class T> std::uint32_t component_id() const { valid(); return id<T>(); }

    // ---- the archetype index ----
    /// Sets when this world indexes the archetypes of repeated queries, and
    /// forgets what was indexed so far. Start from
    /// `ponko_query_cache_policy_default()`; `ponko_query_cache_policy_off()`
    /// turns the index off for the whole world. Query results never depend
    /// on it. Rust: `World::set_query_cache_policy`.
    void set_query_cache_policy(const ponko::query_cache_policy& policy) {
        detail::check(ponko_world_set_query_cache_policy(raw_, &policy));
    }
    /// The policy this world uses.
    ponko::query_cache_policy query_cache_policy() {
        ponko::query_cache_policy policy = ponko_query_cache_policy_default();
        detail::check(ponko_world_query_cache_policy(raw_, &policy));
        return policy;
    }
    /// What the archetype index holds now: queries remembered and indexed,
    /// its bytes, and how often it answered a query.
    ponko::query_cache_stats query_cache_stats() {
        ponko::query_cache_stats stats{};
        detail::check(ponko_world_query_cache_stats(raw_, &stats));
        return stats;
    }

    // ---- change detection ----
    /// Starts recording when `T` values are added, changed and removed, for
    /// the changed/added filters and removed(). Until then a component keeps
    /// no change data and costs nothing. Values that exist already count as
    /// added and changed now. Using a filter on `T` does this too.
    template<class T> void track_changes() { detail::check(ponko_component_track_changes(raw_, id<T>())); }

    // ---- required components ----
    /// Makes `T` require `R` in this world: from now on every spawn and set
    /// that gives an entity a `T` also gives it an `R`, a copy of `value`, if
    /// it lacks one. What `R` requires comes too; values the spawn gives, or
    /// the entity has, win. Entities that have a `T` already are left as they
    /// are. `R` is a plain value: a C++ component, or a built-in such as
    /// transform2d or velocity (not sprite or rigid_body). The engine's own
    /// requirements (a body brings a transform and a velocity, a collider a
    /// fixed body) need no call.
    template<class T, class R> void require(const R& value) {
        static_assert(std::is_trivially_copyable<R>::value, "a required value is copied byte for byte");
        detail::check(ponko_component_require(raw_, id<T>(), id<R>(), &value));
    }
    /// Marks `e`'s `T` as changed now, for a write the world can't see.
    /// False if `e` is dead or has no `T`.
    template<class T> bool mark_changed(entity e) {
        const auto status = ponko_mark_changed(raw_, e, id<T>());
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// The entities that lost their `T` since the calling system last ran
    /// (it was removed, or the entity despawned), oldest first. An entity is
    /// listed once per removal and may have a `T` again. `T` must be tracked
    /// (track_changes) or this throws.
    template<class T> std::vector<entity> removed() const { return removed_since<T>(PONKO_SINCE_LAST_RUN); }
    /// removed() for a reader that keeps its own tick (see advance_change_tick).
    template<class T> std::vector<entity> removed_since(std::uint64_t tick) const {
        const std::uint32_t component = id<T>();
        std::size_t n = 0;
        detail::check(ponko_removed(raw_, component, tick, nullptr, 0, &n));
        std::vector<entity> out(n);
        if (n) detail::check(ponko_removed(raw_, component, tick, out.data(), out.size(), &n));
        return out;
    }
    /// The world's current change tick: changes made now carry it.
    std::uint64_t change_tick() const { valid(); std::uint64_t tick = 0; detail::check(ponko_world_change_tick(raw_, &tick)); return tick; }
    /// Ends the current change tick and returns it. Code that isn't a game
    /// system keeps its place this way: `set_changes_since(seen)`, look, then
    /// `seen = advance_change_tick()`. A game does this around every system.
    std::uint64_t advance_change_tick() { valid(); std::uint64_t tick = 0; detail::check(ponko_world_advance_change_tick(raw_, &tick)); return tick; }
    /// The tick the filters and removed() compare with: inside a game system,
    /// when that system last finished; otherwise 0 (everything) unless set.
    std::uint64_t changes_since() const { valid(); std::uint64_t tick = 0; detail::check(ponko_world_changes_since(raw_, &tick)); return tick; }
    /// Sets the tick the changed/added filters and removed() compare with (see changes_since), for code
    /// outside a game system.
    void set_changes_since(std::uint64_t tick) { valid(); detail::check(ponko_world_set_changes_since(raw_, tick)); }

    // ---- reflection and saves ----
    /// Describes `T`'s fields, so tools and agents can read and write it as
    /// JSON (`ponko-mcp`) and `save()` includes it. It's saved under its
    /// C++ type name, or under `name` (call this before `T` is first used).
    template<class T> void reflect(std::initializer_list<field_info<T>> fields) { reflect<T>(nullptr, fields); }
    /// reflect() under `name` instead of the C++ type name (null: the type name). Describing a type again
    /// replaces its fields. Throws ponko::error when the engine rejects the description.
    template<class T> void reflect(const char* name, std::initializer_list<field_info<T>> fields) {
        valid();
        const std::uint32_t id = register_type<T>(raw_, *owner_, name);
        std::vector<PonkoField> raw;
        raw.reserve(fields.size());
        for (const auto& f : fields) raw.push_back(f.raw);
        detail::check(ponko_component_reflect(raw_, id, raw.data(), raw.size()));
    }
    /// Describes resource `T`'s fields, so tools and agents can read and
    /// set it and `save()` includes it (under its C++ type name).
    template<class T> void reflect_resource(std::initializer_list<field_info<T>> fields) {
        static_assert(detail::pod<T>, "resources are plain structs (trivially copyable, no destructor)");
        valid();
        std::vector<PonkoField> raw;
        raw.reserve(fields.size());
        for (const auto& f : fields) raw.push_back(f.raw);
        detail::check(ponko_resource_reflect(raw_, detail::type_name<T>().c_str(), sizeof(T), alignof(T), raw.data(), raw.size()));
    }
    /// Every entity with reflected components as JSON, with its id.
    std::string save() const {
        valid(); std::size_t len = 0;
        detail::check(ponko_world_save(raw_, nullptr, 0, &len));
        std::string out(len + 1, '\0');
        detail::check(ponko_world_save(raw_, &out[0], out.size(), &len));
        out.resize(len);
        return out;
    }
    /// One number for everything reflected (Rust `world_hash`): every
    /// reflected component of every entity and every reflected resource,
    /// bit for bit. Compare it between runs, peers or a replay.
    std::uint64_t hash() const { valid(); std::uint64_t h = 0; detail::check(ponko_world_hash(raw_, &h)); return h; }
    /// Copies this world into `into` (Rust `World::save_snapshot`). Throws (and saves nothing) if a
    /// component can't be copied: plain structs can.
    void save_snapshot(world_snapshot& into) { valid(); detail::check(ponko_world_save_snapshot(raw_, into.raw())); }
    /// Makes this world what it was when `from` was saved from it (Rust `World::restore_snapshot`): the same
    /// entities with the same ids, the same ids for the next spawns. Throws for another world's snapshot.
    void restore_snapshot(const world_snapshot& from) { valid(); detail::check(ponko_world_restore_snapshot(raw_, from.raw())); }
    /// Replaces every entity with those in `json` (from save()), with the
    /// same ids. On error (an exception) the world is unchanged.
    void load(const std::string& json) { valid(); detail::check(ponko_world_load(raw_, json.c_str())); }

    /// Describes reflected component `name`'s JSON shape for discovery (the
    /// `capabilities` answer, see ctx::capabilities_json), encoded as Rust's
    /// ValueSchema: `{"Object":[["hp","Number"]]}`. Throws for invalid JSON or
    /// a name that isn't reflected; saving and loading don't change.
    void describe_component(const char* name, const char* schema) {
        valid();
        detail::check(ponko_world_describe_component(raw_, name, schema));
    }
    /// describe_component() for reflected resource `name`.
    void describe_resource(const char* name, const char* schema) {
        valid();
        detail::check(ponko_world_describe_resource(raw_, name, schema));
    }

    // ---- save migrations ----
    /// Sets the game's save version (Rust `SaveMigrations`): what save()
    /// records as "game_version", and the newest a load accepts (saves
    /// without one are version 0). Raise it, with a migration() from the old
    /// version, whenever a change to the game's components breaks older saves.
    void set_save_version(std::uint32_t version) { valid(); detail::check(ponko_world_set_save_version(raw_, version)); }
    /// The game's save version (0 before set_save_version).
    std::uint32_t save_version() const { valid(); std::uint32_t v = 0; detail::check(ponko_world_save_version(raw_, &v)); return v; }
    /// The changes from save version `from` to `from + 1`, which every load
    /// of an older save runs (set_save_version above `from` first).
    save_migration migration(std::uint32_t from) { valid(); return save_migration(raw_, from); }
    /// `json` (a world save) as the game's version would have written it.
    /// Throws for a save of a newer game version (kind PONKO_ERROR_UNSUPPORTED).
    std::string migrate_save(const std::string& json) const {
        valid();
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_world_migrate_save(raw_, json.c_str(), b, c, n); });
    }

    // ---- the rest ----
    /// Gives `e` an empty `width` x `height` tilemap (its Transform2D is the top-left).
    tilemap_ref add_tilemap(entity e, const sprite_sheet& sheet, std::uint32_t width, std::uint32_t height);
    /// `e`'s tilemap; throws if it has none. Valid until `e` loses its map.
    tilemap_ref tilemap(entity e);
    /// Reorders Transform2D entities so that neighbors are close in memory.
    void sort_spatial(float cell_size) { valid(); detail::check(ponko_world_sort_spatial(raw_, cell_size)); }

    // ---- component hooks ----
    /// Calls `f(hook_world&, entity, const T& value)` whenever a `T` arrives
    /// on an entity that had none: every spawn and every set that adds it,
    /// from C++, C or Rust (Rust `World::on_add`). It runs inside that call,
    /// once the entity is complete. `T` must be a plain component (not a
    /// sprite, body or collider). Hooks stay for the life of the world; see
    /// hook_world for what one may do. An exception a hook throws is kept and
    /// rethrown when the system returns (or by game::run); in a world of your
    /// own, by rethrow_hook_error().
    template<class T, class F> void on_add(F f) { add_hook<T, false>(ponko_component_on_add, std::move(f)); }
    /// Calls `f(hook_world&, entity, const T& old, const T& next)` whenever a
    /// set is about to overwrite an entity's `T` (Rust `World::on_replace`):
    /// `old` is still in the world. Writes through queries aren't replacements.
    template<class T, class F> void on_replace(F f) { add_hook<T, true>(ponko_component_on_replace, std::move(f)); }
    /// Calls `f(hook_world&, entity, const T& value)` whenever a `T` is about
    /// to leave an entity: remove, despawn or clear (Rust `World::on_remove`),
    /// with the value still in place.
    template<class T, class F> void on_remove(F f) { add_hook<T, false>(ponko_component_on_remove, std::move(f)); }
    /// Rethrows the first exception a component hook of this world threw,
    /// if one did (systems and game::run do this for you).
    void rethrow_hook_error() const { valid(); owner_->hook_error.rethrow(); }
private:
    using hook_register = PonkoStatus (*)(PonkoWorld*, std::uint32_t, PonkoHookFn, void*, PonkoFreeUserFn);
    template<class T, bool Replace, class F> void add_hook(hook_register reg, F f);
public:
};
/// Out of line: tilemap_ref is defined after world_view.
inline tilemap_ref world_view::add_tilemap(entity e, const sprite_sheet& sheet, std::uint32_t width, std::uint32_t height) {
    valid(); detail::check(ponko_tilemap_add(raw_, e, sheet, width, height)); return tilemap_ref(raw_, e);
}
/// Out of line: tilemap_ref is defined after world_view.
inline tilemap_ref world_view::tilemap(entity e) { valid(); return tilemap_ref(raw_, e); }
/// The world as a component hook sees it (world_view::on_add), valid while
/// the hook runs. The world is busy with the call that ran the hook: read and
/// write components in place here, and record structural changes, which are
/// applied before that call returns (Rust `HookWorld`).
class hook_world {
    PonkoHookWorld* raw_;
    detail::identity* owner_;
    template<class T> std::uint32_t id() const {
        const std::size_t slot = detail::type_slot<T>();
        if (slot < owner_->ids.size() && owner_->ids[slot] != 0) return owner_->ids[slot] - 1;
        throw std::logic_error(detail::type_name<T>() + " is used for the first time inside a component hook; use it on the world first");
    }
public:
    /// Wraps the C handle a hook got (ponko_component_on_add) and the
    /// world's C++ identity; world_view::on_add does this for you.
    hook_world(PonkoHookWorld* raw, detail::identity* owner) : raw_(raw), owner_(owner) {}
    /// `e`'s `T`, or nothing if it has none (ponko_hook_get).
    template<class T> std::optional<T> get(entity e) const {
        T value{};
        const PonkoStatus status = ponko_hook_get(raw_, e, id<T>(), &value, sizeof(T));
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return value;
    }
    /// Overwrites `e`'s `T` in place (it must have one; ponko_hook_write).
    /// No hook runs; change detection sees it.
    template<class T> void write(entity e, const T& value) { detail::check(ponko_hook_write(raw_, e, id<T>(), &value, sizeof(T))); }
    /// Adds or replaces `e`'s `T` once the operation is done (ponko_hook_set_later).
    template<class T> void set_later(entity e, const T& value) {
        PonkoValue v{id<T>(), &value, sizeof(T)};
        detail::check(ponko_hook_set_later(raw_, e, &v));
    }
    /// Removes `e`'s `T` once the operation is done (ponko_hook_remove_later).
    template<class T> void remove_later(entity e) { detail::check(ponko_hook_remove_later(raw_, e, id<T>())); }
    /// Despawns `e` once the operation is done (ponko_hook_despawn_later).
    void despawn_later(entity e) { detail::check(ponko_hook_despawn_later(raw_, e)); }
    /// Spawns an entity with these components once the operation is done;
    /// its id is good at once (ponko_hook_spawn_later).
    template<class... Ts> entity spawn_later(const Ts&... components) {
        std::array<PonkoValue, sizeof...(Ts)> args{{PonkoValue{id<Ts>(), &components, sizeof(Ts)}...}};
        entity e = 0;
        detail::check(ponko_hook_spawn_later(raw_, args.data(), args.size(), &e));
        return e;
    }
    /// Whether `e` is alive (ponko_hook_is_alive).
    bool alive(entity e) const { bool a = false; detail::check(ponko_hook_is_alive(raw_, e, &a)); return a; }
};
/// The world just loaded, as a terrain rebuild sees it
/// (ctx::set_terrain_rebuild): read-only, valid only during the call.
class loaded_world {
    const PonkoLoadedWorld* raw_;
    detail::identity* owner_;
public:
    /// Wraps the C handle a rebuild got and the world's C++ identity;
    /// ctx::set_terrain_rebuild does this for you.
    loaded_world(const PonkoLoadedWorld* raw, detail::identity* owner) : raw_(raw), owner_(owner) {}
    /// `e`'s `T`, or nothing if it has none (ponko_loaded_world_get). `T`
    /// must have been used on the world before the load.
    template<class T> std::optional<T> get(entity e) const {
        const std::size_t slot = detail::type_slot<T>();
        if (slot >= owner_->ids.size() || owner_->ids[slot] == 0)
            throw std::logic_error(detail::type_name<T>() + " is used for the first time inside a terrain rebuild; use it on the world first");
        T value{};
        const PonkoStatus status = ponko_loaded_world_get(raw_, e, owner_->ids[slot] - 1, &value, sizeof(T));
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return value;
    }
};
/// Out of line: hook_world is defined after world_view.
template<class T, bool Replace, class F> void world_view::add_hook(hook_register reg, F f) {
    valid();
    world_view::check_component<T>();
    const std::uint32_t component = id<T>();
    struct holder { F fn; std::shared_ptr<detail::identity> owner; };
    auto* h = new holder{std::move(f), owner_};
    auto call = [](PonkoHookWorld* raw, const PonkoHookEvent* event, void* user) noexcept {
        auto& hd = *static_cast<holder*>(user);
        if (hd.owner->hook_error.set.load()) return;
        try {
            hook_world w(raw, hd.owner.get());
            T value{};
            std::memcpy(static_cast<void*>(&value), event->value, sizeof(T));
            if constexpr (Replace) {
                T next{};
                std::memcpy(static_cast<void*>(&next), event->new_value, sizeof(T));
                hd.fn(w, entity(event->entity), static_cast<const T&>(value), static_cast<const T&>(next));
            } else {
                hd.fn(w, entity(event->entity), static_cast<const T&>(value));
            }
        } catch (...) { hd.owner->hook_error.capture(); }
    };
    auto free = [](void* user) noexcept { delete static_cast<holder*>(user); };
    const PonkoStatus status = reg(raw_, component, call, h, free);
    if (status != PONKO_OK) delete h;
    detail::check(status);
}
/// An owned world (from ponko::game::run_headless, or empty).
class world final : public world_view {
    friend class game;
    world(PonkoWorld* raw, std::shared_ptr<detail::identity> owner) : world_view(raw, std::move(owner)) {}
public:
    /// A new, empty world (Rust `World::new`). Throws ponko::error if the library's ABI version differs from
    /// this header's.
    world() : world_view(nullptr, std::make_shared<detail::identity>()) {
        detail::check_abi_version();
        raw_ = ponko_world_new();
        if (!raw_) throw std::runtime_error(ponko_last_error());
    }
    ~world() { if (raw_) (void)ponko_world_free(raw_); }
    /// Not copyable: a world owns its entities.
    ///@{
    world(const world&) = delete;
    world& operator=(const world&) = delete;
    ///@}
    /// Takes `other`'s entities; `other` is left empty and throws std::logic_error when used.
    world(world&& other) noexcept : world_view(other.raw_, std::move(other.owner_)) { other.raw_ = nullptr; }
    /// Frees this world's entities and takes `other`'s; `other` is left empty.
    world& operator=(world&& other) {
        if (this != &other) {
            if (raw_) detail::check(ponko_world_free(raw_));
            raw_ = other.raw_; owner_ = std::move(other.owner_); other.raw_ = nullptr;
        }
        return *this;
    }
};

/// A read-only view of a spatial grid that answers queries inline (no call
/// into the engine per query), from any thread. Get one after the grid is
/// rebuilt: `grid::view()` or `ctx::spatial_view()`; it expires on the next
/// rebuild. `Entry` is grid_item (owned grids) or grid_hit (the built-in grid).
template<class Entry> class grid_view {
    PonkoGridView v_{};
    static std::int32_t cell(float x) {
        // floor, then saturate like Rust's `as i32` (NaN -> 0).
        const float f = std::floor(x);
        if (f != f) return 0;
        if (f >= 2147483648.0f) return INT32_MAX;
        if (f <= -2147483648.0f) return INT32_MIN;
        return std::int32_t(f);
    }
    static std::uint64_t pack(std::int64_t cx, std::int64_t cy) {
        return std::uint64_t(std::uint32_t(cx)) << 32 | std::uint32_t(cy);
    }
    std::size_t bucket(std::int64_t cx, std::int64_t cy) const {
        const std::uint32_t ux = std::uint32_t(cx), uy = std::uint32_t(cy);
        const std::uint64_t local = ((std::uint64_t(uy) & ((1ull << v_.y_bits) - 1)) << v_.x_bits) |
                                    (std::uint64_t(ux) & ((1ull << v_.x_bits) - 1));
        const std::uint64_t tx = ux >> v_.x_bits, ty = uy >> v_.y_bits;
        const std::uint64_t h = tx * 0x9E3779B97F4A7C15ull ^ ty * 0xC2B2AE3D27D4EB4Full;
        return std::size_t((local ^ (h >> 32)) & v_.bucket_mask);
    }
    const Entry* entries() const { return static_cast<const Entry*>(v_.entries); }
public:
    /// An empty view: no entries.
    grid_view() = default;
    /// Wraps a C view; throws std::invalid_argument if its entries aren't `Entry`s. grid::view and
    /// ctx::spatial_view make these.
    explicit grid_view(const PonkoGridView& v) : v_(v) {
        if (v.len && v.entry_size != sizeof(Entry)) throw std::invalid_argument("grid_view entry type does not match the grid");
    }
    /// How many entries the grid held when the view was taken.
    std::size_t size() const noexcept { return v_.len; }
    /// Calls `f(const Entry&)` for every entry within `radius` of `center` (inclusive).
    template<class F> void for_each_in_radius(vec2 center, float radius, F&& f) const {
        if (v_.len == 0 || !(radius >= 0)) return;
        const float r2 = radius * radius;
        const std::int64_t x0 = cell((center.x - radius) * v_.inv_cell), y0 = cell((center.y - radius) * v_.inv_cell);
        const std::int64_t x1 = cell((center.x + radius) * v_.inv_cell), y1 = cell((center.y + radius) * v_.inv_cell);
        const Entry* e = entries();
        if ((x1 - x0 + 1) * (y1 - y0 + 1) > std::int64_t(v_.len)) {
            for (std::size_t i = 0; i < v_.len; ++i)
                if (length_squared(e[i].position - center) <= r2) f(e[i]);
            return;
        }
        for (std::int64_t cy = y0; cy <= y1; ++cy) {
            for (std::int64_t cx = x0; cx <= x1; ++cx) {
                const std::size_t b = bucket(cx, cy);
                const std::uint64_t key = pack(cx, cy);
                for (std::uint32_t i = v_.starts[b], end = v_.starts[b + 1]; i < end; ++i)
                    if (v_.cells[i] == key && length_squared(e[i].position - center) <= r2) f(e[i]);
            }
        }
    }
};

/// A spatial grid you own: rebuild it from items, query it from any thread.
class grid {
    PonkoGrid* raw_;
public:
    /// An empty grid of square cells `cell_size` world units wide (Rust `SpatialGrid::new`); throws
    /// std::invalid_argument unless it is positive and finite. The destructor frees it.
    explicit grid(float cell_size) : raw_(ponko_grid_new(cell_size)) { if (!raw_) throw std::invalid_argument(ponko_last_error()); }
    ~grid() { ponko_grid_free(raw_); }
    /// Not copyable: the grid owns its cells.
    ///@{
    grid(const grid&) = delete;
    grid& operator=(const grid&) = delete;
    ///@}
    /// Replaces every item with `items` (Rust `SpatialGrid::rebuild`); earlier views expire.
    void rebuild(const std::vector<grid_item>& items) { detail::check(ponko_grid_rebuild(raw_, items.data(), items.size())); }
    /// How many items it holds.
    std::size_t size() const { return ponko_grid_len(raw_); }
    /// An inline-query view, valid until the next rebuild. Fastest for hot loops.
    grid_view<grid_item> view() const { PonkoGridView v{}; detail::check(ponko_grid_view(raw_, &v)); return grid_view<grid_item>(v); }
    /// Items within `radius` of `center` into `out` (reuses its capacity).
    void radius(vec2 center, float r, std::vector<grid_item>& out) const {
        std::size_t n = 0;
        out.resize(out.capacity());
        detail::check(ponko_grid_radius(raw_, center, r, out.data(), out.size(), &n));
        if (n > out.size()) { out.resize(n); detail::check(ponko_grid_radius(raw_, center, r, out.data(), out.size(), &n)); }
        out.resize(n);
    }
};

/// Walking costs for flow fields: 1 is normal ground, up to 255 slower,
/// nav_grid::wall can't be entered.
class nav_grid {
    PonkoNavGrid* raw_;
    std::uint8_t* costs_ = nullptr;
    std::uint32_t width_, height_;
    float cell_size_;
    vec2 origin_{0, 0};
public:
    /// The cost of a cell that can't be entered.
    static constexpr std::uint8_t wall = PONKO_NAV_WALL;
    /// A `width` x `height` grid of cells `cell_size` world units wide, every cost 1 (Rust `NavGrid::new`).
    /// Throws std::invalid_argument for an empty size or a cell size that isn't positive. The destructor
    /// frees it.
    nav_grid(std::uint32_t width, std::uint32_t height, float cell_size)
        : raw_(ponko_nav_new(width, height, cell_size)), width_(width), height_(height), cell_size_(cell_size) {
        if (!raw_) throw std::invalid_argument(ponko_last_error());
        std::size_t len = 0;
        detail::check(ponko_nav_costs(raw_, &costs_, &len));
    }
    ~nav_grid() { ponko_nav_free(raw_); }
    /// Not copyable: the grid owns its costs.
    ///@{
    nav_grid(const nav_grid&) = delete;
    nav_grid& operator=(const nav_grid&) = delete;
    ///@}
    /// The C grid, for ctx::flow_field and the C API. Owned by this object.
    const PonkoNavGrid* raw() const noexcept { return raw_; }
    /// Width in cells.
    std::uint32_t width() const noexcept { return width_; }
    /// Height in cells.
    std::uint32_t height() const noexcept { return height_; }
    /// Cell size in world units.
    float cell_size() const noexcept { return cell_size_; }
    /// Where cell (0, 0)'s top-left corner is. Default (0, 0).
    void set_origin(vec2 o) { detail::check(ponko_nav_set_origin(raw_, o)); origin_ = o; }
    /// Where cell (0, 0)'s top-left corner is.
    vec2 origin() const noexcept { return origin_; }
    /// Sets the cost of a cell; cells outside the grid are ignored.
    void set_cost(std::uint32_t x, std::uint32_t y, std::uint8_t cost) noexcept {
        if (x < width_ && y < height_) costs_[std::size_t(y) * width_ + x] = cost;
    }
    /// The cost of a cell, or nothing outside the grid.
    std::optional<std::uint8_t> cost(std::uint32_t x, std::uint32_t y) const noexcept {
        if (x >= width_ || y >= height_) return std::nullopt;
        return costs_[std::size_t(y) * width_ + x];
    }
    /// All costs row by row, for filling a map at once.
    std::uint8_t* costs() noexcept { return costs_; }
    /// The cell under a world point, or nothing outside the grid.
    std::optional<std::array<std::uint32_t, 2>> cell_at(vec2 p) const noexcept {
        const float cx = std::floor((p.x - origin_.x) / cell_size_), cy = std::floor((p.y - origin_.y) / cell_size_);
        if (!(cx >= 0 && cy >= 0 && cx < float(width_) && cy < float(height_))) return std::nullopt;
        return std::array<std::uint32_t, 2>{{std::uint32_t(cx), std::uint32_t(cy)}};
    }
    /// Inside the grid and not a wall.
    bool walkable(vec2 p) const noexcept {
        const auto c = cell_at(p);
        return c && costs_[std::size_t((*c)[1]) * width_ + (*c)[0]] != wall;
    }
    /// The world position of a cell's center.
    vec2 center_of(std::uint32_t x, std::uint32_t y) const noexcept {
        return origin_ + vec2{(float(x) + 0.5f) * cell_size_, (float(y) + 0.5f) * cell_size_};
    }
};

/// One shape a mesh can blend toward (Rust `MorphTarget`; a blend shape:
/// a smile, a dent): one offset per corner in `positions`, plus one normal
/// offset per corner in `normals` (empty when normals don't change).
struct morph_target {
    /// What the shape is called, to find it with ctx::morph_target.
    std::string name;
    /// One offset per corner.
    std::vector<vec3> positions;
    /// One normal offset per corner; empty when normals don't change.
    std::vector<vec3> normals;
};

class ctx;
/// Triangles on the CPU (Rust `MeshData`), before ctx::create_mesh puts them on the GPU. Move-only; the
/// destructor frees them.
class mesh_data {
    friend class ctx;
    PonkoMeshData* raw_;
    explicit mesh_data(PonkoMeshData* raw) : raw_(raw) { if (!raw_) detail::check(PONKO_INVALID_ARGUMENT); }
public:
    /// Corners and three indices per triangle, counter-clockwise seen from
    /// the front. Without normals they're computed; without texture
    /// coordinates they're zero. Throws if they don't fit together.
    mesh_data(const std::vector<vec3>& positions, const std::vector<std::uint32_t>& indices,
              const std::vector<vec3>& normals = {}, const std::vector<vec2>& uvs = {})
        : raw_(nullptr) {
        if ((!normals.empty() && normals.size() != positions.size()) || (!uvs.empty() && uvs.size() != positions.size()))
            throw std::invalid_argument("mesh_data: one normal and texture coordinate per corner");
        raw_ = ponko_mesh_data_new(positions.data(), normals.empty() ? nullptr : normals.data(),
                                    uvs.empty() ? nullptr : uvs.data(), positions.size(), indices.data(), indices.size());
        if (!raw_) detail::check(PONKO_INVALID_ARGUMENT);
    }
    /// A cube `size` wide around the origin.
    static mesh_data cube(float size) { return mesh_data(ponko_mesh_data_cube(size)); }
    /// A flat square `size` wide on the XZ plane, facing up: ground.
    static mesh_data plane(float size) { return mesh_data(ponko_mesh_data_plane(size)); }
    /// A sphere of `radius` from `segments` slices around.
    static mesh_data sphere(float radius, std::uint32_t segments) { return mesh_data(ponko_mesh_data_sphere(radius, segments)); }
    /// A square of terrain `size` wide from n x n heights, row after row
    /// along +Z (NaN: a hole), using every `step`-th one, with skirts at the
    /// edges. Throws unless the heights are a square grid of at least 2 x 2.
    static mesh_data heightfield(const std::vector<float>& heights, float size, std::uint32_t step = 1) {
        return mesh_data(ponko_mesh_data_heightfield(heights.data(), heights.size(), size, step));
    }
    ~mesh_data() { ponko_mesh_data_free(raw_); }
    /// Takes `o`'s triangles; `o` is left empty.
    mesh_data(mesh_data&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps triangles with `o`; the destructor of `o` frees the old ones.
    mesh_data& operator=(mesh_data&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: it owns its arrays.
    ///@{
    mesh_data(const mesh_data&) = delete;
    mesh_data& operator=(const mesh_data&) = delete;
    ///@}
    /// One color per corner, multiplying the material's color; empty
    /// removes them. Throws unless there's one per corner.
    void set_colors(const std::vector<color>& colors) {
        detail::check(ponko_mesh_data_set_colors(raw_, colors.empty() ? nullptr : colors.data(), colors.size()));
    }
    /// Up to four joints per corner and how much each moves it, for
    /// ctx::create_skinned_mesh; empty removes them. Throws unless there's
    /// one of each per corner.
    void set_skin(const std::vector<std::array<std::uint16_t, 4>>& joints,
                  const std::vector<std::array<float, 4>>& weights) {
        if (joints.size() != weights.size()) throw std::invalid_argument("mesh_data::set_skin: one weight set per joint set");
        detail::check(ponko_mesh_data_set_skin(raw_, joints.empty() ? nullptr : joints.front().data(),
                                                weights.empty() ? nullptr : weights.front().data(), joints.size()));
    }
    /// Links existing target deltas as a frame of a piecewise linear channel.
    void add_morph_frame(std::size_t channel, std::size_t target, float weight) {
        detail::check(ponko_mesh_data_add_morph_frame(raw_, channel, target, weight));
    }
    /// Adds a morph target (Rust `MeshData::morph_targets`): a shape the
    /// mesh can blend toward. Targets are numbered in the order they're
    /// added. Throws unless `positions` (and `normals`, when given) have one
    /// offset per corner.
    void add_morph_target(const morph_target& target) {
        if (!target.normals.empty() && target.normals.size() != target.positions.size())
            throw std::invalid_argument("mesh_data::add_morph_target: one normal offset per corner");
        detail::check(ponko_mesh_data_add_morph_target(raw_, target.name.c_str(), target.positions.data(),
                                                        target.normals.empty() ? nullptr : target.normals.data(),
                                                        target.positions.size()));
    }
    /// How many morph targets it has (Rust `MeshData::morph_targets.len()`).
    std::size_t morph_target_count() const noexcept { return ponko_mesh_data_morph_target_count(raw_); }
    /// Removes every morph target (and the morph frames that use them).
    void clear_morph_targets() { detail::check(ponko_mesh_data_clear_morph_targets(raw_)); }
    /// Gives it lightmap coordinates, `texels_per_unit` lightmap texels per world unit, for a
    /// light bake (ponko_mesh_data_generate_lightmap_uvs); returns the square's side in texels.
    std::uint32_t generate_lightmap_uvs(float texels_per_unit) {
        std::uint32_t side = 0; detail::check(ponko_mesh_data_generate_lightmap_uvs(raw_, texels_per_unit, &side)); return side;
    }
    /// Its arrays, valid while it lives.
    PonkoMeshView view() const { PonkoMeshView v{}; detail::check(ponko_mesh_data_view(raw_, &v)); return v; }
    /// A decal (Rust `MeshData::decal`): the part of this mesh inside a box,
    /// as mesh data of its own with an image projected straight onto it.
    /// `projector` (16 floats, column after column) maps the unit cube
    /// (-0.5 to 0.5 on each axis) onto the box in this mesh's space, so its
    /// scale is the decal's size and depth; the image is projected along the
    /// box's -Z, its top along +Y, and only faces turned toward +Z get it.
    /// Corners are lifted `lift` meters along their normals (a few
    /// millimeters is typical). It has no triangles (`view().index_count ==
    /// 0`) when the box misses the mesh; don't make a mesh from an empty
    /// one. Throws if the projector isn't finite.
    mesh_data decal(const std::array<float, 16>& projector, float lift) const {
        return mesh_data(ponko_mesh_data_decal(raw_, projector.data(), lift));
    }
    /// The C mesh data, for the C API. Owned by this object.
    const PonkoMeshData* raw() const noexcept { return raw_; }
};

/// Which way a message travels (Rust `Lane`): reliable and ordered, or unreliable (it may be lost or arrive
/// out of order; for state sent every tick).
using lane = PonkoLane;
/// Traffic counters of a net_messages endpoint (Rust `MessageStats`): bytes and messages sent and received,
/// packets lost and resent.
using message_stats = PonkoMessageStats;
/// One event from net_messages::next_event (bytes and reason copied).
struct message_event {
    PonkoMessageEventKind kind;
    std::uint64_t peer;
    lane on;
    std::vector<std::uint8_t> bytes;
    std::string reason;
};

/// Steam lobby visibility (feature steam in the library).
enum class steam_lobby_visibility : std::uint32_t { friends_only = 0, public_lobby = 1, private_lobby = 2 };
/// Owned asynchronous Steam lobby. Poll each frame; id is zero until ready.
class steam_lobby {
    PonkoSteamLobby* raw_ = nullptr;
    explicit steam_lobby(PonkoSteamLobby* raw) : raw_(raw) {}
public:
    ~steam_lobby() { ponko_steam_lobby_free(raw_); }
    /// Takes `o`'s lobby; `o` is left empty.
    steam_lobby(steam_lobby&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps lobbies with `o`.
    steam_lobby& operator=(steam_lobby&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: one object owns the lobby, and destroying it leaves the lobby.
    ///@{
    steam_lobby(const steam_lobby&) = delete;
    steam_lobby& operator=(const steam_lobby&) = delete;
    ///@}
    /// Starts creating a lobby for Steam app `app_id` with room for `max_members`; id() stays 0 until poll()
    /// sees it made. Throws ponko::error when the library was built without the `steam` feature or Steam
    /// isn't running.
    static steam_lobby create(std::uint32_t app_id, steam_lobby_visibility visibility, std::uint32_t max_members) {
        PonkoSteamLobby* p = nullptr;
        detail::check(ponko_steam_lobby_create(app_id, static_cast<std::uint32_t>(visibility), max_members, &p));
        return steam_lobby(p);
    }
    /// Starts joining lobby `id` of Steam app `app_id`; poll() until id() is set. Throws ponko::error when
    /// the library was built without the `steam` feature or Steam isn't running.
    static steam_lobby join(std::uint32_t app_id, std::uint64_t id) {
        PonkoSteamLobby* p = nullptr; detail::check(ponko_steam_lobby_join(app_id, id, &p)); return steam_lobby(p);
    }

    /// Runs Steam's callbacks: call it every frame.
    void poll() { detail::check(ponko_steam_lobby_poll(raw_)); }
    /// Leaves the lobby now (the destructor also leaves it).
    void leave() { detail::check(ponko_steam_lobby_leave(raw_)); }

    /// The lobby's Steam id, or 0 until it is ready.
    std::uint64_t id() const { std::uint64_t v = 0; detail::check(ponko_steam_lobby_id(raw_, &v)); return v; }
    /// Opens Steam's overlay to invite friends to this lobby.
    void invite() const { detail::check(ponko_steam_lobby_invite(raw_)); }
    /// Sets lobby metadata `key` to `value` (the owner only), for lobby searches and members to read.
    void set_metadata(const char* key, const char* value) { detail::check(ponko_steam_lobby_set_metadata(raw_, key, value)); }
    /// The value of metadata `key`, or nothing if the lobby has none.
    std::optional<std::string> metadata(const char* key) const {
        std::size_t n = 0; detail::check(ponko_steam_lobby_metadata(raw_, key, nullptr, 0, &n));
        if (n == 0) return std::nullopt;
        std::vector<char> b(n); detail::check(ponko_steam_lobby_metadata(raw_, key, b.data(), b.size(), &n));
        return std::string(b.data());
    }
    /// The Steam ids of the lobby's members.
    std::vector<std::uint64_t> members() const {
        std::size_t n = 0; detail::check(ponko_steam_lobby_members(raw_, nullptr, 0, &n));
        std::vector<std::uint64_t> b(n); detail::check(ponko_steam_lobby_members(raw_, b.data(), b.size(), &n));
        b.resize(n); return b;
    }
};

/// What prediction::reconcile did with a server state (Rust `Reconciled`).
template<class State> struct reconciled {
    /// PONKO_RECONCILE_IGNORED, PONKO_RECONCILE_CONFIRMED or PONKO_RECONCILE_CORRECTED.
    PonkoReconcileOutcome outcome = PONKO_RECONCILE_IGNORED;
    /// Inputs replayed on top of the server's state, with a correction.
    std::uint32_t replayed = 0;
    /// The predicted state just before a correction (for smoothing); unset otherwise.
    std::optional<State> before;
    /// Whether the predicted state changed.
    bool corrected() const noexcept { return outcome == PONKO_RECONCILE_CORRECTED; }
};

/// Client-side prediction of a state the client controls (Rust `net::Prediction`): predict() applies each
/// tick's input at once, keeps the inputs the server hasn't confirmed, and reconcile() replays them on top of
/// the server's state when it disagrees. `State` and `Input` are trivially copyable; states are compared
/// byte for byte, so give `State` no padding (or keep it zeroed) and make the step produce the same bytes on
/// both sides. The step is any callable `void(State&, const Input&)`, the same one the server runs; replay
/// calls it with the stored inputs, oldest first, so a deterministic step replays deterministically.
///
/// An exception from the step stops the call: the remaining replay steps are skipped, the exception is
/// rethrown from predict()/reconcile(), and the predicted state is whatever the finished steps left; call
/// reset() with a state you trust before predicting again. The step must not call this prediction.
/// Move-only; the destructor frees it. Steady predict-and-reconcile loops allocate nothing.
template<class State, class Input> class prediction {
    static_assert(std::is_trivially_copyable<State>::value, "prediction: State must be trivially copyable");
    static_assert(std::is_trivially_copyable<Input>::value, "prediction: Input must be trivially copyable");
    PonkoPrediction* raw_ = nullptr;
    template<class Step> struct call {
        Step* step;
        std::exception_ptr error;
    };
    template<class Step> static bool trampoline(void* user, void* state, std::size_t, const void* input, std::size_t) noexcept {
        auto* c = static_cast<call<Step>*>(user);
        try {
            State s;
            Input i;
            std::memcpy(&s, state, sizeof(State));
            std::memcpy(&i, input, sizeof(Input));
            (*c->step)(s, static_cast<const Input&>(i));
            std::memcpy(state, &s, sizeof(State));
            return true;
        } catch (...) {
            c->error = std::current_exception();
            return false;
        }
    }
    template<class Step> static void finish(PonkoStatus status, const call<Step>& c) {
        if (c.error) std::rethrow_exception(c.error);
        detail::check(status);
    }
public:
    /// Predicting from `initial`, keeping up to `capacity` unconfirmed inputs (0: 256; at most 32768).
    explicit prediction(const State& initial, std::size_t capacity = 0) {
        detail::check(ponko_prediction_new(&initial, sizeof(State), sizeof(Input), capacity, &raw_));
    }
    ~prediction() { ponko_prediction_free(raw_); }
    /// Takes `o`'s prediction; `o` is left empty.
    prediction(prediction&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps predictions with `o`.
    prediction& operator=(prediction&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable.
    ///@{
    prediction(const prediction&) = delete;
    prediction& operator=(const prediction&) = delete;
    ///@}
    /// Runs one tick: `step` applies `input` to the predicted state; returns the input's sequence number.
    template<class Step> std::uint16_t predict(const Input& input, Step&& step) {
        using S = std::remove_reference_t<Step>;
        call<S> c{&step, nullptr};
        std::uint16_t seq = 0;
        finish(ponko_prediction_predict(raw_, &input, &trampoline<S>, &c, &seq), c);
        return seq;
    }
    /// Takes the server's `state` after input `ack`; replays the newer inputs with `step` if it differs.
    template<class Step> reconciled<State> reconcile(std::uint16_t ack, const State& state, Step&& step) {
        using S = std::remove_reference_t<Step>;
        call<S> c{&step, nullptr};
        PonkoReconciled r{};
        State before{};
        finish(ponko_prediction_reconcile(raw_, ack, &state, &trampoline<S>, &c, &r, &before), c);
        reconciled<State> out;
        out.outcome = static_cast<PonkoReconcileOutcome>(r.outcome);
        out.replayed = r.replayed;
        if (out.corrected()) out.before = before;
        return out;
    }
    /// The predicted state after the newest input.
    State state() const { State s{}; detail::check(ponko_prediction_state(raw_, &s)); return s; }
    /// The predicted state one tick earlier, to blend from.
    State previous() const { State s{}; detail::check(ponko_prediction_previous(raw_, &s)); return s; }
    /// Sets the predicted state outright and forgets the unconfirmed inputs.
    void reset(const State& s) { detail::check(ponko_prediction_reset(raw_, &s)); }
    /// Copies up to `capacity` unconfirmed inputs, oldest first, with their sequence numbers (either array
    /// may be null); returns how many there are. Allocates nothing.
    std::size_t unacked(std::uint16_t* seqs, Input* inputs, std::size_t capacity) const {
        std::size_t n = 0;
        detail::check(ponko_prediction_unacked(raw_, seqs, inputs, capacity, &n));
        return n;
    }
    /// The unconfirmed inputs, oldest first, with their sequence numbers.
    std::vector<std::pair<std::uint16_t, Input>> unacked() const {
        std::size_t n = unacked(nullptr, nullptr, 0);
        std::vector<std::uint16_t> seqs(n);
        std::vector<Input> inputs(n);
        n = std::min(n, unacked(seqs.data(), inputs.data(), n));
        std::vector<std::pair<std::uint16_t, Input>> out;
        out.reserve(n);
        for (std::size_t i = 0; i < n; ++i) out.emplace_back(seqs[i], inputs[i]);
        return out;
    }
    /// The newest input the server confirmed, once one was.
    std::optional<std::uint16_t> acked() const {
        bool has = false;
        std::uint16_t ack = 0;
        detail::check(ponko_prediction_acked(raw_, &has, &ack));
        return has ? std::optional<std::uint16_t>(ack) : std::nullopt;
    }
    /// The sequence number the next predict() gives.
    std::uint16_t next_seq() const { std::uint16_t s = 0; detail::check(ponko_prediction_next_seq(raw_, &s)); return s; }
    /// What it did so far.
    PonkoPredictionStats stats() const { PonkoPredictionStats s{}; detail::check(ponko_prediction_stats(raw_, &s)); return s; }
    /// The C prediction, for the C API. Owned by this object.
    PonkoPrediction* raw() const noexcept { return raw_; }
};

/// The server's side of prediction (Rust `net::InputQueue`): one client's inputs in sequence order, whatever
/// order the packets came in; duplicates and late ones are dropped. `Input` is trivially copyable.
/// Move-only; the destructor frees it.
template<class Input> class input_queue {
    static_assert(std::is_trivially_copyable<Input>::value, "input_queue: Input must be trivially copyable");
    PonkoInputQueue* raw_ = nullptr;
public:
    /// An empty queue holding up to `capacity` inputs (0: 256; at most 32768).
    explicit input_queue(std::size_t capacity = 0) { detail::check(ponko_input_queue_new(sizeof(Input), capacity, &raw_)); }
    ~input_queue() { ponko_input_queue_free(raw_); }
    /// Takes `o`'s queue; `o` is left empty.
    input_queue(input_queue&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps queues with `o`.
    input_queue& operator=(input_queue&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable.
    ///@{
    input_queue(const input_queue&) = delete;
    input_queue& operator=(const input_queue&) = delete;
    ///@}
    /// Queues input `seq`: PONKO_INPUT_ADDED, or why it was dropped.
    PonkoInputPush push(std::uint16_t seq, const Input& input) {
        std::uint32_t r = 0;
        detail::check(ponko_input_queue_push(raw_, seq, &input, &r));
        return static_cast<PonkoInputPush>(r);
    }
    /// Takes the oldest queued input and its sequence number.
    std::optional<std::pair<std::uint16_t, Input>> pop() {
        bool found = false;
        std::uint16_t seq = 0;
        Input input{};
        detail::check(ponko_input_queue_pop(raw_, &found, &seq, &input));
        if (!found) return std::nullopt;
        return std::make_pair(seq, input);
    }
    /// The number of the last input taken: answer the client with it and your state.
    std::optional<std::uint16_t> ack() const {
        bool has = false;
        std::uint16_t a = 0;
        detail::check(ponko_input_queue_ack(raw_, &has, &a));
        return has ? std::optional<std::uint16_t>(a) : std::nullopt;
    }
    /// The C queue, for the C API. Owned by this object.
    PonkoInputQueue* raw() const noexcept { return raw_; }
};

/// What an in-memory link does to datagrams (PonkoLoopbackLink, Rust `LoopbackLink`): `loss` (a share, 0
/// to 1), `delay` and `jitter` (seconds) and `seed`. Start from loopback_link_defaults(), a perfect link; the
/// same seed loses and delays the same datagrams on every run. For net_messages::loopback.
using loopback_link = PonkoLoopbackLink;
/// A perfect loopback_link (nothing lost or delayed), its header set: change the fields you need.
inline loopback_link loopback_link_defaults() {
    loopback_link l{};
    l.struct_size = sizeof(loopback_link);
    l.version = PONKO_LOOPBACK_LINK_VERSION;
    detail::check(ponko_loopback_link_defaults(&l));
    return l;
}

/// A message endpoint the game owns (Rust `NetMessages`): poll it every frame, send bytes on a reliable or
/// unreliable lane, read events. The host is peer 0. Plain UDP is not authenticated or encrypted; see
/// steam_host for Steam relays. Move-only; the destructor disconnects and frees it.
class net_messages {
    PonkoNetMessages* raw_ = nullptr;
    explicit net_messages(PonkoNetMessages* raw) : raw_(raw) {}
    friend class lobby;
public:
    ~net_messages() { ponko_net_messages_free(raw_); }
    /// Takes `o`'s endpoint; `o` is left empty.
    net_messages(net_messages&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps endpoints with `o`.
    net_messages& operator=(net_messages&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: one object owns the connection.
    ///@{
    net_messages(const net_messages&) = delete;
    net_messages& operator=(const net_messages&) = delete;
    ///@}
    /// Hosts at `addr` ("0.0.0.0:7777"; port 0 picks one) for up to `max_clients`.
    static net_messages host(const char* addr, std::size_t max_clients) {
        PonkoNetMessages* m = nullptr; detail::check(ponko_net_messages_host(addr, max_clients, &m)); return net_messages(m);
    }
    /// Joins the host at `addr`.
    static net_messages join(const char* addr) {
        PonkoNetMessages* m = nullptr; detail::check(ponko_net_messages_join(addr, &m)); return net_messages(m);
    }
    /// `players` endpoints joined in memory (the first hosts), for tests.
    static std::vector<net_messages> loopback(std::size_t players) {
        std::vector<PonkoNetMessages*> raw(players);
        detail::check(ponko_net_messages_loopback(players, raw.data()));
        std::vector<net_messages> out;
        for (auto* m : raw) out.push_back(net_messages(m));
        return out;
    }
    /// `players` endpoints joined in memory over a link that loses, delays and reorders datagrams as
    /// `link` says, the same ones on every run: a game's netcode on a bad network, in a test. Step them
    /// together with poll_elapsed (or game_clock) so the delays repeat exactly.
    ///
    ///     auto link = ponko::loopback_link_defaults();
    ///     link.loss = 0.1; link.jitter = 0.05; link.seed = 3;
    ///     auto nets = ponko::net_messages::loopback(2, link);
    static std::vector<net_messages> loopback(std::size_t players, const loopback_link& link) {
        std::vector<PonkoNetMessages*> raw(players);
        detail::check(ponko_net_messages_loopback_link(players, &link, raw.data()));
        std::vector<net_messages> out;
        for (auto* m : raw) out.push_back(net_messages(m));
        return out;
    }
    /// The lobby member id behind connected `peer` of a WebRTC endpoint (lobby::rtc_host, lobby::rtc_join), or 0.
    std::uint32_t rtc_peer(std::uint64_t peer) const { std::uint32_t m = 0; detail::check(ponko_net_messages_rtc_peer(raw_, peer, &m)); return m; }
    /// The Steam id of connected `peer`, or 0 for an unknown or non-Steam peer.
    std::uint64_t steam_peer(std::uint64_t peer) const { std::uint64_t id = 0; detail::check(ponko_net_messages_steam_peer(raw_, peer, &id)); return id; }
    /// Steam relays plus a friends-only lobby, for 1..=249 clients.
    static net_messages steam_host(std::uint32_t app_id, std::size_t max_clients) {
        PonkoNetMessages* p = nullptr; detail::check(ponko_net_messages_steam_host(app_id, max_clients, &p)); return net_messages(p);
    }
    /// Joins Steam lobby `lobby` of app `app_id` through Steam relays. Throws ponko::error without the
    /// `steam` feature or a running Steam.
    static net_messages steam_join(std::uint32_t app_id, std::uint64_t lobby) {
        PonkoNetMessages* p = nullptr; detail::check(ponko_net_messages_steam_join(app_id, lobby, &p)); return net_messages(p);
    }

    /// The lobby id of a Steam endpoint (0 until ready). Throws ponko::error for a non-Steam endpoint.
    std::uint64_t steam_lobby_id() const { std::uint64_t v = 0; detail::check(ponko_net_messages_steam_lobby_id(raw_, &v)); return v; }
    /// Opens Steam's invite overlay for this endpoint's lobby. Throws ponko::error for a non-Steam endpoint.
    void steam_invite() const { detail::check(ponko_net_messages_steam_lobby_invite(raw_)); }
    /// Sets this endpoint's lobby metadata `key` to `value`. Throws ponko::error for a non-Steam endpoint.
    void steam_set_metadata(const char* key, const char* value) { detail::check(ponko_net_messages_steam_lobby_set_metadata(raw_, key, value)); }
    /// The value of this endpoint's lobby metadata `key`, or nothing. Throws ponko::error for a non-Steam
    /// endpoint.
    std::optional<std::string> steam_metadata(const char* key) const {
        std::size_t n = 0; detail::check(ponko_net_messages_steam_lobby_metadata(raw_, key, nullptr, 0, &n));
        if (n == 0) return std::nullopt;
        std::vector<char> b(n); detail::check(ponko_net_messages_steam_lobby_metadata(raw_, key, b.data(), b.size(), &n));
        return std::string(b.data());
    }
    /// The Steam ids of this endpoint's lobby members. Throws ponko::error for a non-Steam endpoint.
    std::vector<std::uint64_t> steam_members() const {
        std::size_t n = 0; detail::check(ponko_net_messages_steam_lobby_members(raw_, nullptr, 0, &n));
        std::vector<std::uint64_t> b(n); detail::check(ponko_net_messages_steam_lobby_members(raw_, b.data(), b.size(), &n));
        b.resize(n); return b;
    }

    /// How long a silent peer may stay connected.
    void set_timeout(std::uint64_t ms) { detail::check(ponko_net_messages_set_timeout(raw_, ms)); }
    /// Once a game polls it (game::messages), count the game's time instead of the wall clock: a
    /// headless run then plays the same every time. For tests, bots and replays.
    void game_clock() { detail::check(ponko_net_messages_game_clock(raw_)); }
    /// Gives up the handle without freeing it (game::messages takes it).
    PonkoNetMessages* release() noexcept { PonkoNetMessages* p = raw_; raw_ = nullptr; return p; }
    /// The UDP port this endpoint listens on (0 for loopback ones).
    std::uint16_t local_port() const { std::uint16_t p = 0; detail::check(ponko_net_messages_local_port(raw_, &p)); return p; }
    /// Sends `len` bytes (at most 1 MiB) to `peer`.
    void send(std::uint64_t peer, lane l, const void* bytes, std::size_t len) {
        detail::check(ponko_net_messages_send(raw_, peer, l, static_cast<const std::uint8_t*>(bytes), len));
    }
    /// Sends `text`'s bytes to `peer` on lane `l`.
    void send(std::uint64_t peer, lane l, const std::string& text) { send(peer, l, text.data(), text.size()); }
    /// Sends `len` bytes (at most 1 MiB) to every connected peer on lane `l`.
    void send_to_all(lane l, const void* bytes, std::size_t len) {
        detail::check(ponko_net_messages_send_to_all(raw_, l, static_cast<const std::uint8_t*>(bytes), len));
    }
    /// Sends `text`'s bytes to every connected peer on lane `l`.
    void send_to_all(lane l, const std::string& text) { send_to_all(l, text.data(), text.size()); }
    /// Sends what's queued and receives what arrived; then read next_event until empty.
    void poll() { detail::check(ponko_net_messages_poll(raw_)); }
    /// poll() on a clock of your own: protocol time moves on by `seconds`, not by the wall clock. For tests
    /// that step loopback endpoints frame by frame, and replays; don't mix it with poll() on one endpoint.
    void poll_elapsed(double seconds) { detail::check(ponko_net_messages_poll_elapsed(raw_, seconds)); }
    /// The oldest event, or std::nullopt.
    std::optional<message_event> next_event() {
        PonkoMessageEvent e{};
        bool found = false;
        detail::check(ponko_net_messages_next_event(raw_, &e, &found));
        if (!found) return std::nullopt;
        return message_event{e.kind, e.peer, e.lane, std::vector<std::uint8_t>(e.bytes, e.bytes + e.len), std::string(e.reason)};
    }
    /// Disconnects `peer`, telling it so.
    void disconnect(std::uint64_t peer) { detail::check(ponko_net_messages_disconnect(raw_, peer)); }
    /// Traffic counters since the endpoint opened.
    message_stats stats() const { message_stats s{}; detail::check(ponko_net_messages_stats(raw_, &s)); return s; }
};

/// Why a lobby server refused a request (Rust `LobbyError`).
using lobby_error = PonkoLobbyError;
/// Why a lobby client is out of its room (Rust `LeaveReason`).
using lobby_leave_reason = PonkoLobbyLeaveReason;
/// What a lobby_event is.
using lobby_event_kind = PonkoLobbyEventKind;
/// A lobby client's state now: connected, its member id, its room (code, host, size, members).
using lobby_state = PonkoLobbyState;
/// One event from lobby::next_event (bytes and text copied).
struct lobby_event {
    /// What happened.
    lobby_event_kind kind;
    /// The member it is about (0 when none): the host for joined, the sender for relayed.
    std::uint32_t member;
    /// The lane a relayed message came on.
    lane on;
    /// The relayed bytes (empty for other events).
    std::vector<std::uint8_t> bytes;
    /// The room code (joined), room text (meta changed) or why the connection closed (disconnected).
    std::string text;
    /// Why a request was refused (refused).
    lobby_error error;
    /// Why the client left its room (left).
    lobby_leave_reason reason;
};

/// A client of a lobby server (Rust `ponko::net::lobby::LobbyClient`, the `ponko-lobby` program): rooms with
/// six-character codes, matchmaking queues, host migration, and a relay for players who can't connect directly.
/// Poll it every frame and read next_event until empty. Requests made before the server answered are sent once it
/// does; what it refuses arrives as a refused event. Move-only; the destructor frees it.
class lobby {
    PonkoLobby* raw_ = nullptr;
    explicit lobby(PonkoLobby* raw) : raw_(raw) {}
public:
    ~lobby() { ponko_lobby_free(raw_); }
    /// Takes `o`'s client; `o` is left empty.
    lobby(lobby&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps clients with `o`.
    lobby& operator=(lobby&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: one object owns the connection.
    ///@{
    lobby(const lobby&) = delete;
    lobby& operator=(const lobby&) = delete;
    ///@}
    /// Starts connecting to a lobby server over UDP ("lobby.example.com:7700").
    static lobby join(const char* addr) {
        PonkoLobby* l = nullptr; detail::check(ponko_lobby_join(addr, &l)); return lobby(l);
    }
    /// Sends what's queued and turns what arrived into events.
    void poll() { detail::check(ponko_lobby_poll(raw_)); }
    /// The oldest event, or std::nullopt.
    std::optional<lobby_event> next_event() {
        PonkoLobbyEvent e{};
        bool found = false;
        detail::check(ponko_lobby_next_event(raw_, &e, &found));
        if (!found) return std::nullopt;
        return lobby_event{e.kind, e.member, e.lane, std::vector<std::uint8_t>(e.bytes, e.bytes + e.len),
                           std::string(e.text), e.error, e.reason};
    }
    /// Makes a room for up to `size` members and joins it as host; the joined event brings its code.
    void create_room(std::uint16_t size) { detail::check(ponko_lobby_create_room(raw_, size)); }
    /// Joins the room with `code` (any letter case).
    void join_room(const char* code) { detail::check(ponko_lobby_join_room(raw_, code)); }
    /// Leaves the room; if this client hosted it, the server picks the next host.
    void leave_room() { detail::check(ponko_lobby_leave_room(raw_)); }
    /// Waits in the matchmaking queue `mode` for a room of `size`; `rating` is for the server's rule.
    void queue(const char* mode, std::uint16_t size, std::int32_t rating = 0) {
        detail::check(ponko_lobby_queue(raw_, mode, size, rating));
    }
    /// Leaves the matchmaking queue.
    void cancel_queue() { detail::check(ponko_lobby_cancel_queue(raw_)); }
    /// Sets the room's text (host only).
    void set_meta(const char* meta) { detail::check(ponko_lobby_set_meta(raw_, meta)); }
    /// Sends `len` bytes through the relay to member `to` of this room (0: every other member).
    void relay(std::uint32_t to, lane l, const void* bytes, std::size_t len) {
        detail::check(ponko_lobby_relay(raw_, to, l, static_cast<const std::uint8_t*>(bytes), len));
    }
    /// Sends `text`'s bytes through the relay to member `to` (0: every other member).
    void relay(std::uint32_t to, lane l, const std::string& text) { relay(to, l, text.data(), text.size()); }
    /// Closes the connection, telling the server.
    void disconnect() { detail::check(ponko_lobby_disconnect(raw_)); }
    /// A message endpoint that hosts up to `max_clients` room members over WebRTC (browser players too), signaled
    /// through this client's room. `stun_urls`: space-separated "stun:host:port" URLs, or null; `bind`: the UDP
    /// socket's address, or null for any port. Keep polling this client. Throws ponko::error without the `webrtc`
    /// feature.
    net_messages rtc_host(const char* stun_urls, std::size_t max_clients, const char* bind = nullptr) {
        PonkoNetMessages* m = nullptr; detail::check(ponko_net_messages_rtc_host(raw_, stun_urls, bind, max_clients, &m));
        return net_messages(m);
    }
    /// A message endpoint joining the host that member `host` (the room's host) runs, over WebRTC signaled
    /// through this client. Settings as rtc_host.
    net_messages rtc_join(std::uint32_t host, const char* stun_urls, const char* bind = nullptr) {
        PonkoNetMessages* m = nullptr; detail::check(ponko_net_messages_rtc_join(raw_, host, stun_urls, bind, &m));
        return net_messages(m);
    }
    /// The client's state now.
    lobby_state state() const { lobby_state s{}; detail::check(ponko_lobby_state(raw_, &s)); return s; }
    /// The room's members, in the order they came in (empty outside a room).
    std::vector<std::uint32_t> members() const {
        std::size_t n = 0; detail::check(ponko_lobby_members(raw_, nullptr, 0, &n));
        std::vector<std::uint32_t> out(n);
        if (n) detail::check(ponko_lobby_members(raw_, out.data(), out.size(), &n));
        return out;
    }
    /// The room's text (empty outside a room).
    std::string room_meta() const {
        std::size_t n = 0; detail::check(ponko_lobby_room_meta(raw_, nullptr, 0, &n));
        std::vector<char> b(n + 1); detail::check(ponko_lobby_room_meta(raw_, b.data(), b.size(), &n));
        return std::string(b.data(), n);
    }
};

// ---- Replication helpers (sit on top of net_messages; the packet format stays yours) ----

/// Is `a` newer than `b` among 16-bit sequence numbers that wrap around
/// (Rust `seq_newer`)? Equal numbers are not newer.
inline bool seq_newer(std::uint16_t a, std::uint16_t b) { return ponko_seq_newer(a, b); }
/// How far `a` is ahead of `b` among wrapping 16-bit sequence numbers;
/// negative when behind.
inline std::int16_t seq_diff(std::uint16_t a, std::uint16_t b) { return ponko_seq_diff(a, b); }
/// A state update's authority epoch and wrapping sequence number (Rust `UpdateStamp`).
using update_stamp = PonkoUpdateStamp;
/// Is stamp `a` newer than `b`: a newer epoch, or the same epoch and a newer sequence?
inline bool stamp_newer(update_stamp a, update_stamp b) { return ponko_stamp_newer(a, b); }

/// How an update compares with an authority record (Rust `AuthorityCheck`).
enum class authority_check : std::uint32_t {
    accept = PONKO_AUTHORITY_ACCEPT,           ///< From the owner, current epoch: apply it.
    stale = PONKO_AUTHORITY_STALE,             ///< Older epoch, before a handoff: drop it.
    wrong_owner = PONKO_AUTHORITY_WRONG_OWNER, ///< Current epoch, not from the owner: drop it.
    ahead = PONKO_AUTHORITY_AHEAD,             ///< Newer epoch than we know: the handoff announcement is still on its way.
};
/// Who owns an entity: an owner peer and an epoch (Rust `Authority`). A
/// handoff bumps the epoch, and updates stamped with an older one are
/// rejected, so a late packet from the previous owner can't undo it. Peer 0 is the host.
class authority {
    PonkoAuthority a_;
public:
    /// An entity owned by peer `owner`, epoch 0 (Rust `Authority::new`).
    explicit authority(std::uint64_t owner) : a_(ponko_authority_new(owner)) {}
    /// The peer that owns the entity now.
    std::uint64_t owner() const noexcept { return a_.owner; }
    /// How many handoffs it went through (wraps at 65536).
    std::uint16_t epoch() const noexcept { return a_.epoch; }
    /// Gives the entity to `new_owner`; returns the new epoch to announce reliably.
    std::uint16_t handoff(std::uint64_t new_owner) {
        std::uint16_t epoch = 0; detail::check(ponko_authority_handoff(&a_, new_owner, &epoch)); return epoch;
    }
    /// Applies a handoff announced by another peer; true if it changed the
    /// record. Older epochs are ignored; the same epoch with another owner is
    /// a race that the lower owner id wins.
    bool adopt(std::uint64_t new_owner, std::uint16_t epoch) {
        bool changed = false; detail::check(ponko_authority_adopt(&a_, new_owner, epoch, &changed)); return changed;
    }
    /// Judges an update from peer `from` stamped with `epoch`.
    authority_check check(std::uint64_t from, std::uint16_t epoch) const {
        PonkoAuthorityCheck c{}; detail::check(ponko_authority_check(&a_, from, epoch, &c)); return static_cast<authority_check>(c);
    }
};

/// "Latest wins per entity" for unreliable state updates (Rust `LatestWins`):
/// accepts an update only if it is newer than all accepted before for that key.
class latest_wins {
    PonkoLatestWins* raw_ = ponko_latest_wins_new();
public:
    /// An empty record.
    latest_wins() = default;
    ~latest_wins() { ponko_latest_wins_free(raw_); }
    /// Takes `o`'s record; `o` is left empty.
    latest_wins(latest_wins&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps records with `o`.
    latest_wins& operator=(latest_wins&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: it owns its table.
    ///@{
    latest_wins(const latest_wins&) = delete;
    latest_wins& operator=(const latest_wins&) = delete;
    ///@}
    /// True if the update should be applied, false if it should be dropped.
    bool accept(std::uint64_t key, update_stamp stamp) {
        bool ok = false; detail::check(ponko_latest_wins_accept(raw_, key, stamp, &ok)); return ok;
    }
    /// For streams without epochs.
    bool accept(std::uint64_t key, std::uint16_t seq) { return accept(key, update_stamp{0, seq}); }
    /// The entity is gone: its next update is accepted.
    void forget(std::uint64_t key) { detail::check(ponko_latest_wins_forget(raw_, key)); }
    /// Forgets every key.
    void clear() { detail::check(ponko_latest_wins_clear(raw_)); }
    /// How many keys it remembers.
    std::size_t size() const { return ponko_latest_wins_len(raw_); }
};

/// A position, rotation and optional velocity (Rust `Pose`).
using pose = PonkoPose;
/// A pose at `position`, not rotated, without a velocity.
inline pose pose_at(vec3 position) { pose p{}; p.position = position; p.rotation = {0, 0, 0, 1}; return p; }
/// The same pose moving at `velocity` (units per second). With a velocity
/// on both sides of a gap, the path is a smooth curve instead of a line.
inline pose moving(pose p, vec3 velocity) { p.velocity = velocity; p.has_velocity = true; return p; }
/// Delay, extrapolation limit and capacity of a snapshot stream.
using snapshot_config = PonkoSnapshotConfig;
/// 0.1 s delay, 0.25 s extrapolation, 64 snapshots.
inline snapshot_config snapshot_defaults() { return ponko_snapshot_config_default(); }
/// What happened to a pushed snapshot (Rust `SnapshotPush`).
enum class snapshot_push : std::uint32_t {
    added = PONKO_SNAPSHOT_ADDED, duplicate = PONKO_SNAPSHOT_DUPLICATE,
    too_old = PONKO_SNAPSHOT_TOO_OLD, invalid = PONKO_SNAPSHOT_INVALID,
};
/// How a sampled pose was made (Rust `SnapshotState`).
enum class snapshot_state : std::uint32_t {
    interpolated = PONKO_SNAPSHOT_INTERPOLATED, ///< Between two snapshots.
    extrapolated = PONKO_SNAPSHOT_EXTRAPOLATED, ///< Predicted past the newest, for a bounded time.
    held = PONKO_SNAPSHOT_HELD,                 ///< Standing still: before the oldest or past the limit.
};
/// A pose read from a snapshot_stream and how it was made.
struct snapshot_sample { pose value; snapshot_state state; };

/// The snapshots of one remote entity plus an estimate of the sender's clock
/// (Rust `SnapshotStream<Pose>`): push each decoded update with the sender's
/// timestamp and your clock, sample once a frame with your clock. The
/// entity is shown `delay` seconds in the past, interpolated between
/// snapshots; late packets are extrapolated for a bounded time, then held.
class snapshot_stream {
    PonkoSnapshotStream* raw_ = nullptr;
public:
    /// An empty stream with `config` (snapshot_defaults() if left out). Throws ponko::error if the config is
    /// invalid (negative delay, zero capacity).
    explicit snapshot_stream(const snapshot_config& config = ponko_snapshot_config_default()) {
        detail::check(ponko_snapshot_stream_new(&config, &raw_));
    }
    ~snapshot_stream() { ponko_snapshot_stream_free(raw_); }
    /// Takes `o`'s snapshots; `o` is left empty.
    snapshot_stream(snapshot_stream&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps snapshots with `o`.
    snapshot_stream& operator=(snapshot_stream&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: it owns its buffer.
    ///@{
    snapshot_stream(const snapshot_stream&) = delete;
    snapshot_stream& operator=(const snapshot_stream&) = delete;
    ///@}
    /// Stores a snapshot stamped `remote_time` (sender's clock, seconds) that
    /// arrived at `local_now` (your clock, seconds). Any order, duplicates fine.
    snapshot_push push(double remote_time, double local_now, const pose& p) {
        PonkoSnapshotPush r{}; detail::check(ponko_snapshot_stream_push(raw_, remote_time, local_now, &p, &r));
        return static_cast<snapshot_push>(r);
    }
    /// The pose to show at your time `now`; std::nullopt until the first snapshot.
    std::optional<snapshot_sample> sample(double now) const {
        PonkoPose p{}; PonkoSnapshotState st{}; bool found = false;
        detail::check(ponko_snapshot_stream_sample(raw_, now, &p, &st, &found));
        if (!found) return std::nullopt;
        return snapshot_sample{p, static_cast<snapshot_state>(st)};
    }
    /// How far in the past to show the entity, in seconds.
    void set_delay(double seconds) { detail::check(ponko_snapshot_stream_set_delay(raw_, seconds)); }
    /// Forgets the snapshots, keeping the clock estimate: a teleport or respawn.
    void clear() { detail::check(ponko_snapshot_stream_clear(raw_)); }
    /// Forgets snapshots and clock estimate: a new connection.
    void reset() { detail::check(ponko_snapshot_stream_reset(raw_)); }
    /// How many snapshots it holds.
    std::size_t size() const { return ponko_snapshot_stream_len(raw_); }
    /// Your time minus the sender's, once a snapshot arrived.
    std::optional<double> clock_offset() const {
        double o = 0; bool known = false; detail::check(ponko_snapshot_stream_clock_offset(raw_, &o, &known));
        return known ? std::optional<double>(o) : std::nullopt;
    }
};

/// One ring of interest (Rust `InterestTier`): entities within `radius` are sent every `every` ticks.
using interest_tier = PonkoInterestTier;
/// Rings of interest around each viewer, and how sticky their edges are.
using interest_config = PonkoInterestConfig;
/// Somebody who looks at the world (Rust `InterestViewer`): a peer and where. A peer may have several; an
/// entity counts by its nearest.
using interest_viewer = PonkoInterestViewer;
/// An entity in the world (Rust `InterestEntity`): your id for it (unique) and its position.
using interest_entity = PonkoInterestEntity;
/// One entity a peer is interested in (Rust `InterestEntry`): its id, its ring, and whether to send it on
/// this tick.
using interest_entry = PonkoInterestEntry;
/// An entity entering or leaving a peer's interest (Rust `InterestChange`).
using interest_change = PonkoInterestChange;
/// 40 units every tick, 120 units every 4th tick, hysteresis 5, staggered.
inline interest_config interest_defaults() { return ponko_interest_config_default(); }
/// Which entities each peer should be sent, and how often, by distance
/// (Rust `Interest`): rings around each viewer, a hysteresis at their edges
/// so boundary entities don't flicker, and a spatial grid so thousands of
/// entities are cheap. Call update once per send tick, then read entries per
/// peer for what to send and changes for who needs a spawn or despawn.
class interest {
    PonkoInterest* raw_ = nullptr;
public:
    /// A manager with `config` (interest_defaults() if left out). Throws ponko::error if the rings are
    /// invalid: radii not positive and increasing, an `every` of 0, or a hysteresis not below the first
    /// radius.
    explicit interest(const interest_config& config = ponko_interest_config_default()) {
        detail::check(ponko_interest_new(&config, &raw_));
    }
    ~interest() { ponko_interest_free(raw_); }
    /// Takes `o`'s state; `o` is left empty.
    interest(interest&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps state with `o`.
    interest& operator=(interest&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: it owns its grid and tables.
    ///@{
    interest(const interest&) = delete;
    interest& operator=(const interest&) = delete;
    ///@}
    /// Recomputes every peer's interest for send tick `tick`. Peers without a
    /// viewer are forgotten and their entities reported as left.
    void update(std::uint64_t tick, const std::vector<interest_viewer>& viewers, const std::vector<interest_entity>& entities) {
        detail::check(ponko_interest_update(raw_, tick, viewers.data(), viewers.size(), entities.data(), entities.size()));
    }
    /// Everything `peer` is interested in as of the last update; `send` marks
    /// what to send on this tick.
    std::vector<interest_entry> entries(std::uint64_t peer) const {
        std::size_t n = 0; detail::check(ponko_interest_entries(raw_, peer, nullptr, 0, &n));
        std::vector<interest_entry> out(n);
        detail::check(ponko_interest_entries(raw_, peer, out.data(), out.size(), &n));
        return out;
    }
    /// Entities that entered or left a peer's interest in the last update.
    std::vector<interest_change> changes() const {
        std::size_t n = 0; detail::check(ponko_interest_changes(raw_, nullptr, 0, &n));
        std::vector<interest_change> out(n);
        detail::check(ponko_interest_changes(raw_, out.data(), out.size(), &n));
        return out;
    }
    /// A peer disconnected: forget it, without reporting its entities as left.
    void remove_peer(std::uint64_t peer) { detail::check(ponko_interest_remove_peer(raw_, peer)); }
};

/// How a navmesh is baked (Rust `NavBakeSettings`): agent radius and height, the steepest walkable slope,
/// cell sizes in meters. Start from nav_bake_defaults().
using nav_bake_settings = PonkoNavBakeSettings;
/// A 0.3 m wide, 1.8 m tall agent on 45 degree slopes.
inline nav_bake_settings nav_bake_defaults() { return ponko_nav_bake_settings_default(); }

/// Walkable geometry to bake into a navmesh (ctx::bake_navmesh).
class nav_sources {
    PonkoNavSources* raw_ = ponko_nav_sources_new();
public:
    /// An empty set of walkable geometry.
    nav_sources() = default;
    ~nav_sources() { ponko_nav_sources_free(raw_); }
    /// Takes `o`'s geometry; `o` is left empty.
    nav_sources(nav_sources&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps geometry with `o`.
    nav_sources& operator=(nav_sources&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: it owns its triangles.
    ///@{
    nav_sources(const nav_sources&) = delete;
    nav_sources& operator=(const nav_sources&) = delete;
    ///@}
    /// World-space triangles: three corners each, counter-clockwise seen from above.
    nav_sources& add_triangles(const std::vector<vec3>& corners) {
        if (corners.size() % 3) throw std::invalid_argument("nav_sources::add_triangles: three corners per triangle");
        detail::check(ponko_nav_sources_add_triangles(raw_, corners.data(), corners.size() / 3));
        return *this;
    }
    /// A mesh's triangles placed by `t`.
    nav_sources& add_mesh(const mesh_data& data, const transform3d& t) {
        detail::check(ponko_nav_sources_add_mesh(raw_, data.raw(), t));
        return *this;
    }
    /// The C sources, for the C API. Owned by this object.
    const PonkoNavSources* raw() const noexcept { return raw_; }
};

/// How particles, trails and lines cover what's behind them: alpha blending, or adding light (fire, sparks).
enum class particle_blend : std::uint32_t { alpha = 0, additive = 1 };
/// Whether particles live in the world (they stay behind when the emitter moves) or move with the emitter.
enum class particle_space : std::uint32_t { world = 0, local = 1 };
/// A point of a curve over a particle's life (Rust `Curve` key): `t` 0 at birth to 1 at death.
struct particle_key { float t; float value; };
/// A point of a color gradient over a particle's life: `t` 0 at birth to 1 at death.
struct particle_color_key { float t; color value; };

/// A particle emitter's settings (Rust `ParticleSettings3D`), built by
/// chaining: `particle_settings().rate(40).cone(0.3f).blend(particle_blend::additive)`.
class particle_settings {
    PonkoParticleSettings3D c_ = ponko_particle_settings3d_default();
    std::vector<PonkoParticleKey> size_keys_;
    std::vector<PonkoParticleColorKey> color_keys_;
    float softness_ = 0.0f;
    particle_settings& shape(PonkoParticleShape3D kind, vec3 size) {
        c_.shape = static_cast<std::uint32_t>(kind);
        c_.shape_size = size;
        return *this;
    }
public:
    /// Particles per second; 0 emits only bursts.
    particle_settings& rate(float per_second) { c_.rate = per_second; return *this; }
    /// Particles emitted at once on the first frame.
    particle_settings& burst(std::uint32_t count) { c_.burst = count; return *this; }
    /// Seconds of emission, and whether it starts again after.
    particle_settings& duration(float seconds, bool looping = true) { c_.duration = seconds; c_.looping = looping; return *this; }
    /// Each particle lives a random time between `min` and `max` seconds.
    particle_settings& lifetime(float min, float max) { c_.lifetime_min = min; c_.lifetime_max = max; return *this; }
    /// Start speed in m/s, random between `min` and `max`.
    particle_settings& speed(float min, float max) { c_.speed_min = min; c_.speed_max = max; return *this; }
    /// Start size in meters, random between `min` and `max` (scaled by size_over_life).
    particle_settings& size(float min, float max) { c_.size_min = min; c_.size_max = max; return *this; }
    /// Start rotation in radians, random between `min` and `max`.
    particle_settings& rotation(float min, float max) { c_.rotation_min = min; c_.rotation_max = max; return *this; }
    /// From the origin, along +Y (the default).
    particle_settings& point() { return shape(PONKO_PARTICLES_POINT, {0, 0, 0}); }
    /// Inside a ball, moving outward.
    particle_settings& sphere(float radius) { return shape(PONKO_PARTICLES_SPHERE, {radius, 0, 0}); }
    /// From the origin, inside a cone around +Y with this half angle (radians).
    particle_settings& cone(float angle) { return shape(PONKO_PARTICLES_CONE, {angle, 0, 0}); }
    /// Inside a box, moving along +Y.
    particle_settings& box(vec3 half_extents) { return shape(PONKO_PARTICLES_BOX, half_extents); }
    /// Acceleration in m/s² (e.g. {0, -9.8f, 0}); none by default.
    particle_settings& gravity(vec3 acceleration) { c_.gravity = acceleration; return *this; }
    /// Velocity damping per second.
    particle_settings& drag(float per_second) { c_.drag = per_second; return *this; }
    /// Size multiplier over normalized lifetime (0 birth, 1 death).
    particle_settings& size_over_life(std::initializer_list<particle_key> keys) {
        size_keys_.clear();
        for (auto k : keys) size_keys_.push_back({k.t, k.value});
        return *this;
    }
    /// Tint over normalized lifetime (0 birth, 1 death).
    particle_settings& color_over_life(std::initializer_list<particle_color_key> keys) {
        color_keys_.clear();
        for (auto k : keys) color_keys_.push_back({k.t, k.value});
        return *this;
    }
    /// The particle image; with `columns` x `rows` sprite sheet frames played over lifetime.
    particle_settings& image(texture t, std::uint32_t columns = 1, std::uint32_t rows = 1) {
        c_.has_texture = true;
        c_.texture = t;
        c_.sheet_columns = columns;
        c_.sheet_rows = rows;
        return *this;
    }
    /// How particles cover what's behind them (default alpha).
    particle_settings& blend(particle_blend b) { c_.blend = static_cast<std::uint32_t>(b); return *this; }
    /// Most living particles; storage is made up front.
    particle_settings& max_particles(std::uint32_t n) { c_.max_particles = n; return *this; }
    /// Whether particles stay in the world or move with the emitter (default world).
    particle_settings& space(particle_space s) { c_.space = static_cast<std::uint32_t>(s); return *this; }
    /// Equal seeds give equal particles.
    particle_settings& seed(std::uint64_t s) { c_.seed = s; return *this; }
    /// Soft particles: over this many meters in front of the scene behind them, particles fade out, so smoke
    /// meets a wall or the ground without a hard edge. 0 (the default) draws them hard. It reads the scene's
    /// depth, which costs a copy of the frame; target cameras draw particles without it.
    particle_settings& softness(float meters) { softness_ = meters; return *this; }
    /// The C settings (ABI 2: without softness); its key pointers live as long as this object.
    PonkoParticleSettings3D raw() const {
        PonkoParticleSettings3D c = c_;
        c.size_keys = size_keys_.data();
        c.size_key_count = size_keys_.size();
        c.color_keys = color_keys_.data();
        c.color_key_count = color_keys_.size();
        return c;
    }
    /// The versioned C settings (PonkoParticleSettings3DEx): raw() plus softness.
    PonkoParticleSettings3DEx raw_ex() const {
        PonkoParticleSettings3DEx x{};
        x.struct_size = sizeof(PonkoParticleSettings3DEx);
        x.version = PONKO_PARTICLE_SETTINGS3D_EX_VERSION;
        detail::check(ponko_particle_settings3d_ex_defaults(&x));
        x.settings = raw();
        x.softness = softness_;
        return x;
    }
};

/// A 2D particle emitter's settings (Rust `d2::ParticleSettings`), in pixels, built by chaining:
/// `particle_settings2d().rate(40).cone(0.3f).blend(sprite_blend::additive).z(2)`. Particles draw with the
/// sprites at `z`. Give it to an entity with a 2D transform with ctx::set_particle_emitter2d.
class particle_settings2d {
    PonkoParticleSettings2D c_ = ponko_particle_settings2d_default();
    std::vector<PonkoParticleKey> size_keys_;
    std::vector<PonkoParticleColorKey> color_keys_;
    particle_settings2d& shape(PonkoParticleShape2D kind, vec2 size) {
        c_.shape = static_cast<std::uint32_t>(kind);
        c_.shape_size = size;
        return *this;
    }
public:
    /// Particles per second; 0 emits only bursts.
    particle_settings2d& rate(float per_second) { c_.rate = per_second; return *this; }
    /// Particles emitted at once on the first frame.
    particle_settings2d& burst(std::uint32_t count) { c_.burst = count; return *this; }
    /// Seconds of emission, and whether it starts again after.
    particle_settings2d& duration(float seconds, bool looping = true) { c_.duration = seconds; c_.looping = looping; return *this; }
    /// Each particle lives a random time between `min` and `max` seconds.
    particle_settings2d& lifetime(float min, float max) { c_.lifetime_min = min; c_.lifetime_max = max; return *this; }
    /// Start speed in pixels per second, random between `min` and `max`.
    particle_settings2d& speed(float min, float max) { c_.speed_min = min; c_.speed_max = max; return *this; }
    /// Start width in pixels, random between `min` and `max` (scaled by size_over_life).
    particle_settings2d& size(float min, float max) { c_.size_min = min; c_.size_max = max; return *this; }
    /// Start rotation in radians, random between `min` and `max`.
    particle_settings2d& rotation(float min, float max) { c_.rotation_min = min; c_.rotation_max = max; return *this; }
    /// From the origin, moving up (the default).
    particle_settings2d& point() { return shape(PONKO_PARTICLES2D_POINT, {0, 0}); }
    /// Inside a disc, moving outward.
    particle_settings2d& circle(float radius) { return shape(PONKO_PARTICLES2D_CIRCLE, {radius, 0}); }
    /// From the origin, up to this many radians away from up.
    particle_settings2d& cone(float angle) { return shape(PONKO_PARTICLES2D_CONE, {angle, 0}); }
    /// Inside a rectangle, moving up.
    particle_settings2d& rect(vec2 half_extents) { return shape(PONKO_PARTICLES2D_RECT, half_extents); }
    /// Acceleration in pixels/s² (y down); {0, 200} by default.
    particle_settings2d& gravity(vec2 acceleration) { c_.gravity = acceleration; return *this; }
    /// Velocity damping per second.
    particle_settings2d& drag(float per_second) { c_.drag = per_second; return *this; }
    /// Size multiplier over normalized lifetime (0 birth, 1 death).
    particle_settings2d& size_over_life(std::initializer_list<particle_key> keys) {
        size_keys_.clear();
        for (auto k : keys) size_keys_.push_back({k.t, k.value});
        return *this;
    }
    /// Tint over normalized lifetime (0 birth, 1 death).
    particle_settings2d& color_over_life(std::initializer_list<particle_color_key> keys) {
        color_keys_.clear();
        for (auto k : keys) color_keys_.push_back({k.t, k.value});
        return *this;
    }
    /// The particle image; with `columns` x `rows` sprite sheet frames played over lifetime.
    particle_settings2d& image(texture t, std::uint32_t columns = 1, std::uint32_t rows = 1) {
        c_.has_texture = true;
        c_.texture = t;
        c_.sheet_columns = columns;
        c_.sheet_rows = rows;
        return *this;
    }
    /// How particles cover what's behind them, as for sprites (default alpha).
    particle_settings2d& blend(sprite_blend b) { c_.blend = static_cast<std::uint32_t>(b); return *this; }
    /// A sprite material (custom shader) for the particles.
    particle_settings2d& material(sprite_material m) { c_.material = m; return *this; }
    /// Draw order among sprites: higher is on top.
    particle_settings2d& z(float order) { c_.z = order; return *this; }
    /// Most living particles; storage is made up front.
    particle_settings2d& max_particles(std::uint32_t n) { c_.max_particles = n; return *this; }
    /// Whether particles stay in the world or move with the emitter (default world).
    particle_settings2d& space(particle_space s) { c_.space = static_cast<std::uint32_t>(s); return *this; }
    /// Equal seeds give equal particles.
    particle_settings2d& seed(std::uint64_t s) { c_.seed = s; return *this; }
    /// The C settings; its key pointers live as long as this object.
    PonkoParticleSettings2D raw() const {
        PonkoParticleSettings2D c = c_;
        c.size_keys = size_keys_.data();
        c.size_key_count = size_keys_.size();
        c.color_keys = color_keys_.data();
        c.color_key_count = color_keys_.size();
        return c;
    }
};

/// A ribbon left behind its entity as it moves (Rust `Trail3D`), built by
/// chaining: `trail3d(0.25f, 0.05f).blend(particle_blend::additive)`. Give it
/// to an entity with ctx::set_trail3d.
class trail3d {
    PonkoTrail3D c_;
    std::vector<PonkoParticleKey> width_keys_;
    std::vector<PonkoParticleColorKey> color_keys_;
public:
    /// A ribbon `width` meters wide (narrowing to 0 at its end) whose points last `lifetime` seconds.
    trail3d(float lifetime, float width) : c_(ponko_trail3d_new(lifetime, width)) {}
    /// Width in meters over age (0 the newest end, 1 the oldest).
    trail3d& width_over_age(std::initializer_list<particle_key> keys) {
        width_keys_.clear();
        for (auto k : keys) width_keys_.push_back({k.t, k.value});
        return *this;
    }
    /// Color over age (0 the newest end, 1 the oldest).
    trail3d& color_over_age(std::initializer_list<particle_color_key> keys) {
        color_keys_.clear();
        for (auto k : keys) color_keys_.push_back({k.t, k.value});
        return *this;
    }
    /// The ribbon image: u is each point's age over the lifetime, v across.
    trail3d& image(texture t) { c_.has_texture = true; c_.texture = t; return *this; }
    /// How the ribbon covers what's behind it (default alpha).
    trail3d& blend(particle_blend b) { c_.blend = static_cast<std::uint32_t>(b); return *this; }
    /// A new point is kept only after the entity moved this far (meters) from the last one.
    trail3d& min_distance(float meters) { c_.min_distance = meters; return *this; }
    /// At most this many points (at least 2); the oldest go first.
    trail3d& max_points(std::uint32_t n) { c_.max_points = n; return *this; }
    /// While false, no new points are added and the ribbon fades out.
    trail3d& emitting(bool on) { c_.emitting = on; return *this; }
    /// The C trail; its key pointers live as long as this object.
    PonkoTrail3D raw() const {
        PonkoTrail3D c = c_;
        c.width_keys = width_keys_.data();
        c.width_key_count = width_keys_.size();
        c.color_keys = color_keys_.data();
        c.color_key_count = color_keys_.size();
        return c;
    }
};

/// A line through points in its entity's space, drawn as a camera-facing
/// strip (Rust `Line3D`): beams, cables, ropes. Give it to an entity with
/// ctx::set_line3d, again any time to change it.
class line3d {
    PonkoLine3D c_;
    std::vector<vec3> points_;
public:
    /// A `c` colored line `width` meters wide through `points` (at least two to draw anything).
    line3d(std::vector<vec3> points, float width, color c) : c_(ponko_line3d_new(width, c)), points_(std::move(points)) {}
    /// The line image: u along it, v across.
    line3d& image(texture t) { c_.has_texture = true; c_.texture = t; return *this; }
    /// How the line covers what's behind it (default alpha).
    line3d& blend(particle_blend b) { c_.blend = static_cast<std::uint32_t>(b); return *this; }
    /// Meters of line per repeat of the image; 0 stretches it once over the whole line.
    line3d& texture_length(float meters) { c_.texture_length = meters; return *this; }
    /// The C line; its points live as long as this object.
    PonkoLine3D raw() const {
        PonkoLine3D c = c_;
        c.points = points_.data();
        c.point_count = points_.size();
        return c;
    }
};

/// The retained UI (Rust `Ui`, reached with `ctx.ui_retained()`): a tree of
/// panels, labels, buttons, images, scroll areas, text inputs, toggles and
/// sliders, laid out by flexbox, grid and absolute positions.
namespace ui {
/// A node's id, never reused: the id of a removed node fails.
using id = std::uint64_t;
/// "No node": a detached root's parent.
constexpr id none = PONKO_UI_NONE;
/// A flex or grid length: automatic, in points, or a fraction of the parent.
using length = PonkoUiLength;
/// A length the layout decides (CSS `auto`).
inline length automatic() { return {static_cast<std::uint32_t>(PONKO_UI_LENGTH_AUTO), 0.0f}; }
/// A length in points (UI pixels before scaling).
inline length points(float v) { return {static_cast<std::uint32_t>(PONKO_UI_LENGTH_POINTS), v}; }
/// A fraction of the parent: `percent(0.5f)` is half.
inline length percent(float fraction) { return {static_cast<std::uint32_t>(PONKO_UI_LENGTH_PERCENT), fraction}; }
/// One grid row or column.
using track = PonkoUiTrack;
/// A track sized by its content and the free space (CSS `auto`).
inline track track_auto() { return {static_cast<std::uint32_t>(PONKO_UI_TRACK_AUTO), 0.0f}; }
/// A track `v` points wide.
inline track track_points(float v) { return {static_cast<std::uint32_t>(PONKO_UI_TRACK_POINTS), v}; }
/// A track a fraction of the grid's size: `track_percent(0.25f)` is a quarter.
inline track track_percent(float fraction) { return {static_cast<std::uint32_t>(PONKO_UI_TRACK_PERCENT), fraction}; }
/// A share of the free space, like CSS `fr`.
inline track fr(float share) { return {static_cast<std::uint32_t>(PONKO_UI_TRACK_FR), share}; }
/// A track as narrow as its content can get (CSS `min-content`).
inline track min_content() { return {static_cast<std::uint32_t>(PONKO_UI_TRACK_MIN_CONTENT), 0.0f}; }
/// A track as wide as its content wants (CSS `max-content`).
inline track max_content() { return {static_cast<std::uint32_t>(PONKO_UI_TRACK_MAX_CONTENT), 0.0f}; }
/// A single template track or an owned, non-nested repeat fragment.
class template_track {
    track single_{};
    std::uint32_t repetition_ = 0, count_ = 0;
    std::vector<track> tracks_;
    std::vector<std::string> names_;
    mutable std::vector<const char*> raw_names_;
public:
    /// One track (implicit, so a track can stand where a template_track is expected).
    template_track(track t) : single_(t) {}
    /// A repeat fragment: `mode` 1 repeats `count` times, 2 fills (auto-fill), 3 fits (auto-fit). Use repeat,
    /// auto_fill and auto_fit instead.
    template_track(std::uint32_t mode, std::uint32_t count, std::initializer_list<track> tracks)
        : repetition_(mode), count_(count), tracks_(tracks) {}
    /// Names on every line in a repeat fragment, including both edges.
    template_track& names(std::initializer_list<std::string> names) { names_.assign(names); return *this; }
    /// The C track; its pointers live until this object changes or is destroyed, or raw() is called again.
    PonkoUiTemplateTrack raw() const {
        raw_names_.clear(); for (const auto& n : names_) raw_names_.push_back(n.c_str());
        return {single_, repetition_, count_, tracks_.data(), tracks_.size(), raw_names_.data(), raw_names_.size()};
    }
};
/// `tracks` repeated `count` times (CSS `repeat(count, ...)`).
///@{
inline template_track repeat(std::uint32_t count, std::initializer_list<track> tracks) { return {1, count, tracks}; }
inline template_track repeat(std::uint32_t count, track t) { return repeat(count, {t}); }
///@}
/// `tracks` repeated as often as they fit, keeping empty tracks (CSS `repeat(auto-fill, ...)`).
///@{
inline template_track auto_fill(std::initializer_list<track> tracks) { return {2, 0, tracks}; }
inline template_track auto_fill(track t) { return auto_fill({t}); }
///@}
/// `tracks` repeated as often as they fit, collapsing empty tracks (CSS `repeat(auto-fit, ...)`).
///@{
inline template_track auto_fit(std::initializer_list<track> tracks) { return {3, 0, tracks}; }
inline template_track auto_fit(track t) { return auto_fit({t}); }
///@}
/// Where a grid item starts or ends: placed automatically, at a numbered line (from 1; negative counts from the end), or across `n` tracks.
using grid_line = PonkoUiGridLine;
/// Placed by the grid's auto-placement.
inline grid_line grid_auto() { return {static_cast<std::uint32_t>(PONKO_UI_GRID_AUTO), 0}; }
/// At line `number` (from 1; negative counts from the end).
inline grid_line line(int number) { return {static_cast<std::uint32_t>(PONKO_UI_GRID_LINE), number}; }
/// Across `tracks` tracks from wherever the other edge is.
inline grid_line span(int tracks) { return {static_cast<std::uint32_t>(PONKO_UI_GRID_SPAN), tracks}; }

/// How a node lays out its children: flexbox, grid, or not shown at all.
enum class display : std::uint32_t { flex = 0, grid = 1, none = 2 };
/// `flow` keeps a node in normal flow; `relative` also nudges it by its inset and anchors absolute children.
enum class position : std::uint32_t { flow = 0, relative = 1, absolute = 2, fixed = 3 };
/// The main axis of a flex container.
enum class direction : std::uint32_t { row = 0, column = 1, row_reverse = 2, column_reverse = 3 };
/// Whether flex items wrap onto more lines when they don't fit.
enum class wrap : std::uint32_t { no_wrap = 0, wrap = 1, wrap_reverse = 2 };
/// Alignment of items on one axis. For `align_self` and `justify_self`, `normal` follows the parent.
enum class align : std::uint32_t { normal = 0, start = 1, end = 2, flex_start = 3, flex_end = 4, center = 5, baseline = 6, stretch = 7 };
/// How lines or items share the free space.
enum class content : std::uint32_t { normal = 0, start = 1, end = 2, flex_start = 3, flex_end = 4, center = 5, stretch = 6, space_between = 7, space_evenly = 8, space_around = 9 };
/// Horizontal text alignment within a node.
enum class text_align : std::uint32_t { start = 0, center = 1, end = 2 };

/// A node's layout (Rust `ui_layout::Style`), built by chaining:
/// `ui::layout().column().size(ui::percent(1), ui::points(40)).padding(8).gap(4)`.
class layout {
    PonkoUiLayout c_ = ponko_ui_layout_default();
    std::vector<template_track> columns_, rows_;
    std::vector<PonkoUiTrack> auto_columns_, auto_rows_;
    std::vector<std::string> areas_, column_names_, row_names_;
    std::optional<std::string> area_;
    mutable std::vector<PonkoUiTemplateTrack> raw_columns_, raw_rows_;
    mutable std::vector<const char*> raw_areas_, raw_column_names_, raw_row_names_;
    static void strings(const std::vector<std::string>& from, std::vector<const char*>& to) {
        to.clear(); for (const auto& s : from) to.push_back(s.c_str());
    }
    static PonkoUiSides sides(length top, length right, length bottom, length left) { return {top, right, bottom, left}; }
public:
    /// The default layout: flex row, automatic size, in flow.
    layout() = default;
    /// Starts from a C layout, such as a preset (`ui::row(30)`, `ui::fixed(220, 40)`).
    explicit layout(const PonkoUiLayout& c) : c_(c) {}
    /// How it lays out its children (flex, grid, none).
    layout& display(ui::display d) { c_.display = static_cast<std::uint32_t>(d); return *this; }
    /// How it is positioned (flow, relative, absolute, fixed).
    layout& position(ui::position p) { c_.position = static_cast<std::uint32_t>(p); return *this; }
    /// Out of flow, placed by `inset` inside the closest relative ancestor.
    layout& absolute() { return position(ui::position::absolute); }
    /// Not shown and takes no space (display none).
    layout& hidden() { return display(ui::display::none); }
    /// Direction of its flex children.
    layout& direction(ui::direction d) { c_.flex_direction = static_cast<std::uint32_t>(d); return *this; }
    /// Children left to right.
    layout& row() { return direction(ui::direction::row); }
    /// Children top to bottom.
    layout& column() { return direction(ui::direction::column); }
    /// Whether flex children wrap.
    layout& wrap(ui::wrap w) { c_.flex_wrap = static_cast<std::uint32_t>(w); return *this; }
    /// Share of the free main-axis space this node takes.
    layout& grow(float g) { c_.flex_grow = g; return *this; }
    /// How much this node shrinks when space is short (default 1; 0 keeps its size).
    layout& shrink(float s) { c_.flex_shrink = s; return *this; }
    /// Main-axis size before growing or shrinking (CSS `flex-basis`).
    layout& basis(length l) { c_.flex_basis = l; return *this; }
    /// Width and height.
    layout& size(length width, length height) { c_.width = width; c_.height = height; return *this; }
    /// Width and height in points.
    layout& size(float width, float height) { return size(points(width), points(height)); }
    /// Width.
    layout& width(length l) { c_.width = l; return *this; }
    /// Height.
    layout& height(length l) { c_.height = l; return *this; }
    /// Smallest width and height.
    layout& min_size(length width, length height) { c_.min_width = width; c_.min_height = height; return *this; }
    /// Largest width and height.
    layout& max_size(length width, length height) { c_.max_width = width; c_.max_height = height; return *this; }
    /// Width over height to keep when only one is set.
    layout& aspect_ratio(float width_over_height) { c_.aspect_ratio = width_over_height; return *this; }
    /// Outside space; `automatic()` margins absorb free space (to center a node).
    layout& margin(length all) { c_.margin = sides(all, all, all, all); return *this; }
    /// Outside space on each side, CSS order (top, right, bottom, left).
    layout& margin(length top, length right, length bottom, length left) { c_.margin = sides(top, right, bottom, left); return *this; }
    /// Outside space on every side, in points.
    layout& margin(float all) { return margin(points(all)); }
    /// Inside space on every side.
    layout& padding(length all) { c_.padding = sides(all, all, all, all); return *this; }
    /// Inside space on each side, CSS order (top, right, bottom, left).
    layout& padding(length top, length right, length bottom, length left) { c_.padding = sides(top, right, bottom, left); return *this; }
    /// Inside space on every side, in points.
    layout& padding(float all) { return padding(points(all)); }
    /// Offsets from the parent's edges for relative, absolute and fixed nodes.
    layout& inset(length top, length right, length bottom, length left) { c_.inset = sides(top, right, bottom, left); return *this; }
    /// Space between columns and between rows.
    layout& gap(length columns, length rows) { c_.gap = {columns, rows}; return *this; }
    /// Space between children, in points, both ways.
    layout& gap(float both) { return gap(points(both), points(both)); }
    /// Puts its top edge `y` points below its parent's top instead of where
    /// the preset put it (a `centered` dialog near the top); in a row or
    /// column it is a margin above it.
    layout& top(float y) {
        if (c_.position == static_cast<std::uint32_t>(ui::position::absolute)) { c_.inset.top = points(y); c_.margin.top = points(0); }
        else c_.margin.top = points(y);
        return *this;
    }
    /// Puts its left edge `x` points right of its parent's left; see `top`.
    layout& left(float x) {
        if (c_.position == static_cast<std::uint32_t>(ui::position::absolute)) { c_.inset.left = points(x); c_.margin.left = points(0); }
        else c_.margin.left = points(x);
        return *this;
    }
    /// Alignment of its children on the cross axis (CSS `align-items`).
    layout& align_items(align a) { c_.align_items = static_cast<std::uint32_t>(a); return *this; }
    /// Its own cross-axis alignment, overriding the parent's align_items.
    layout& align_self(align a) { c_.align_self = static_cast<std::uint32_t>(a); return *this; }
    /// Alignment of its grid children within their cells along the row (CSS `justify-items`).
    layout& justify_items(align a) { c_.justify_items = static_cast<std::uint32_t>(a); return *this; }
    /// Its own alignment within its grid cell along the row.
    layout& justify_self(align a) { c_.justify_self = static_cast<std::uint32_t>(a); return *this; }
    /// How lines share the cross-axis free space (CSS `align-content`).
    layout& align_content(content c) { c_.align_content = static_cast<std::uint32_t>(c); return *this; }
    /// How children share the main-axis free space (CSS `justify-content`).
    layout& justify_content(content c) { c_.justify_content = static_cast<std::uint32_t>(c); return *this; }
    /// A grid with these columns (rows are added as needed, see `auto_rows`).
    layout& grid(std::initializer_list<template_track> columns) { display(ui::display::grid); columns_.assign(columns); return *this; }
    /// The explicit rows of a grid (see grid for the columns).
    layout& grid_rows(std::initializer_list<template_track> rows) { rows_.assign(rows); return *this; }
    /// Size of columns beyond the explicit ones; they repeat.
    layout& auto_columns(std::initializer_list<track> columns) { auto_columns_.assign(columns); return *this; }
    /// Size of rows beyond the explicit ones; they repeat.
    layout& auto_rows(std::initializer_list<track> rows) { auto_rows_.assign(rows); return *this; }
    /// For a grid item: where it starts and ends along the columns.
    layout& grid_column(grid_line start, grid_line end = grid_auto()) { c_.grid_column_start = start; c_.grid_column_end = end; return *this; }
    /// For a grid item: where it starts and ends along the rows.
    layout& grid_row(grid_line start, grid_line end = grid_auto()) { c_.grid_row_start = start; c_.grid_row_end = end; return *this; }
    /// CSS-like template rows. Strings are owned by this builder.
    layout& areas(std::initializer_list<std::string> rows) { areas_.assign(rows); return *this; }
    /// For a grid item: the named area it fills (see areas).
    layout& area(std::string name) { area_ = std::move(name); return *this; }
    /// One space-separated set of names per template boundary (empty means unnamed).
    layout& column_names(std::initializer_list<std::string> names) { column_names_.assign(names); return *this; }
    /// One space-separated set of names per row boundary (empty means unnamed).
    layout& row_names(std::initializer_list<std::string> names) { row_names_.assign(names); return *this; }
    /// The C layout; pointers remain valid until this builder is changed, destroyed, or raw() is called again.
    PonkoUiLayout raw() const {
        PonkoUiLayout c = c_;
        raw_columns_.clear(); for (const auto& t : columns_) raw_columns_.push_back(t.raw());
        raw_rows_.clear(); for (const auto& t : rows_) raw_rows_.push_back(t.raw());
        c.grid_template_columns = raw_columns_.data(); c.grid_template_column_count = raw_columns_.size();
        c.grid_template_rows = raw_rows_.data(); c.grid_template_row_count = raw_rows_.size();
        strings(areas_, raw_areas_); strings(column_names_, raw_column_names_); strings(row_names_, raw_row_names_);
        c.grid_template_areas = raw_areas_.data(); c.grid_template_area_count = raw_areas_.size();
        c.grid_column_names = raw_column_names_.data(); c.grid_column_name_count = raw_column_names_.size();
        c.grid_row_names = raw_row_names_.data(); c.grid_row_name_count = raw_row_names_.size();
        c.grid_area = area_ ? area_->c_str() : nullptr;
        c.grid_auto_columns = auto_columns_.data();
        c.grid_auto_column_count = auto_columns_.size();
        c.grid_auto_rows = auto_rows_.data();
        c.grid_auto_row_count = auto_rows_.size();
        return c;
    }
};

/// A node's looks (Rust `UiPaint`), built by chaining:
/// `ui::paint().background(c).border(c2, 2).font_size(24).align(ui::text_align::center)`.
class paint {
    PonkoUiPaint c_ = ponko_ui_paint_default();
public:
    /// Background color.
    paint& background(ponko::color c) { c_.background = c; return *this; }
    /// Background while hovered or focused.
    paint& hover(ponko::color c) { c_.hover = c; return *this; }
    /// Background while the pointer is held on it.
    paint& pressed(ponko::color c) { c_.pressed = c; return *this; }
    /// Background while disabled.
    paint& disabled(ponko::color c) { c_.disabled = c; return *this; }
    /// Border and focus-ring color, and thickness in points.
    paint& border(ponko::color c, float width) { c_.border = c; c_.border_width = width; return *this; }
    /// Border thickness in points, in the default border color.
    paint& border_width(float width) { c_.border_width = width; return *this; }
    /// Text and image tint.
    paint& color(ponko::color c) { c_.color = c; return *this; }
    /// Text font (from `ctx::load_font`) and line height in points.
    paint& font(ponko::font f, float size) { c_.font = f; c_.font_size = size; return *this; }
    /// Line height in points, in the current font.
    paint& font_size(float size) { c_.font_size = size; return *this; }
    /// Break text at word boundaries.
    paint& wrap(bool on = true) { c_.wrap = on; return *this; }
    /// Text alignment.
    paint& align(text_align a) { c_.align = static_cast<std::uint32_t>(a); return *this; }
    /// An image or background texture; with `slice` > 0 it is drawn as nine patches with this many pixels of border.
    paint& image(ponko::texture t, float slice = 0.0f) { c_.has_texture = true; c_.texture = t; c_.slice = slice; return *this; }
    /// The C paint.
    PonkoUiPaint raw() const { return c_; }
};

// Layout presets (Rust `UiLayout`): the common shapes, so no screen needs
// its own helpers. Refine them by chaining: `ui::fill().padding(24).gap(12)`.
/// A full-width row `height` points tall; its children go left to right.
inline layout row(float height) { return layout(ponko_ui_layout_row(height)); }
/// A full-height column `width` points wide; its children stack.
inline layout column(float width) { return layout(ponko_ui_layout_column(width)); }
/// All of the parent (the window for a screen root), shrinking to what its siblings leave.
inline layout fill() { return layout(ponko_ui_layout_fill()); }
/// Exactly `width` x `height` points, never shrunk.
inline layout fixed(float width, float height) { return layout(ponko_ui_layout_fixed(width, height)); }
/// Out of the flow at `x`, `y`, `width` x `height` in fractions of the parent.
inline layout anchored(float x, float y, float width, float height) { return layout(ponko_ui_layout_anchored(x, y, width, height)); }
/// `width` x `height` points in the middle of the parent: a dialog.
inline layout centered(float width, float height) { return layout(ponko_ui_layout_centered(width, height)); }

/// The colors and text defaults of a tree (Rust `UiTheme`): new nodes take
/// their paint from it, and the screen's immediate widgets follow it.
using theme = PonkoUiTheme;
/// The engine's default theme.
inline theme default_theme() { return ponko_ui_theme_default(); }
/// A whole theme (Rust `UiTheme`): `theme` plus spacing, corner radius and
/// tooltip delay. Start from `dark_theme()` or `light_theme()`, change what
/// you like (a custom theme), then `tree::set_theme`.
using theme_ex = PonkoUiThemeEx;
/// An empty `theme_ex` with its header set, for the calls that fill one.
inline theme_ex blank_theme_ex() { theme_ex t{}; t.struct_size = sizeof(theme_ex); t.version = PONKO_UI_THEME_EX_VERSION; return t; }
/// The dark theme: the default colors, controls a shade lighter than panels, round corners.
inline theme_ex dark_theme() { theme_ex t = blank_theme_ex(); detail::check(ponko_ui_theme_ex_dark(&t)); return t; }
/// The light theme: light panels, white controls, dark text, a blue accent, round corners.
inline theme_ex light_theme() { theme_ex t = blank_theme_ex(); detail::check(ponko_ui_theme_ex_light(&t)); return t; }
/// An easing curve (Rust `Bezier`): a CSS-style cubic bezier from (0, 0) to
/// (1, 1) through (`x1`, `y1`) and (`x2`, `y2`); `named_ease` gives the usual ones.
using ease = PonkoUiEase;
/// The named easing curves.
enum class ease_name : std::uint32_t {
    linear = PONKO_UI_EASE_LINEAR,       ///< A straight line.
    ease = PONKO_UI_EASE_EASE,           ///< CSS `ease`: quick start, slow settle.
    ease_in = PONKO_UI_EASE_IN,          ///< Slow start: things leaving.
    ease_out = PONKO_UI_EASE_OUT,        ///< Slow end: things arriving, most UI changes.
    ease_in_out = PONKO_UI_EASE_IN_OUT,  ///< Slow at both ends.
    back_out = PONKO_UI_EASE_BACK_OUT,   ///< Overshoots a little, then settles.
};
/// A named easing curve.
inline ease named_ease(ease_name name) { return ponko_ui_ease(static_cast<std::uint32_t>(name)); }
/// How long a change takes and how it eases (Rust `UiTransition`); zero seconds is at once.
using transition = PonkoUiTransition;
/// A transition of `seconds`, eased out.
inline transition make_transition(float seconds) { return ponko_ui_transition(seconds); }
/// A transition of `seconds` with the curve `name`.
inline transition make_transition(float seconds, ease_name name) { transition t = ponko_ui_transition(seconds); t.ease = named_ease(name); return t; }
/// How far `t` has gone `elapsed` seconds after it started: 0, eased, then exactly 1.
inline float progress(const transition& t, float elapsed) { float k = 0.0f; detail::check(ponko_ui_transition_progress(&t, elapsed, &k)); return k; }
/// How a screen comes on or goes off (Rust `UiScreenTransition`): its pose while away and the timing.
using screen_transition = PonkoUiScreenTransition;
/// No screen transition: shows and goes at once.
inline screen_transition no_screen_transition() { return screen_transition{transition{0.0f, named_ease(ease_name::linear)}, 1.0f, vec2{0.0f, 0.0f}, 1.0f}; }
/// A fade over `seconds`.
inline screen_transition screen_fade(float seconds) { return ponko_ui_screen_fade(seconds); }
/// A slide from or to `offset` UI units away over `seconds`.
inline screen_transition screen_slide(vec2 offset, float seconds) { return ponko_ui_screen_slide(offset, seconds); }
/// A zoom from or to `scale` times its size over `seconds`.
inline screen_transition screen_zoom(float scale, float seconds) { return ponko_ui_screen_zoom(scale, seconds); }
/// What a tween changes (Rust `UiProperty`).
enum class property : std::uint32_t {
    opacity = PONKO_UI_PROPERTY_OPACITY,        ///< 0 to 1, with its children (drawn).
    scale = PONKO_UI_PROPERTY_SCALE,            ///< Around its centre, with its children (drawn).
    offset = PONKO_UI_PROPERTY_OFFSET,          ///< From its place, UI units (drawn).
    background = PONKO_UI_PROPERTY_BACKGROUND,  ///< Its paint's background.
    text_color = PONKO_UI_PROPERTY_TEXT_COLOR,  ///< Its paint's text color.
    width = PONKO_UI_PROPERTY_WIDTH,            ///< Its layout width.
    height = PONKO_UI_PROPERTY_HEIGHT,          ///< Its layout height.
};
/// One property going to a value (Rust `UiTween`); `start` places it on a timeline (or delays it).
using tween = PonkoUiTween;
/// A tween of `p` to the number `to` (opacity, scale, width, height) over `seconds`.
inline tween tween_to(property p, float to, float seconds = 0.25f) { tween t = ponko_ui_tween_defaults(static_cast<std::uint32_t>(p)); t.to.number = to; t.seconds = seconds; return t; }
/// A tween of the offset to `to` over `seconds`.
inline tween tween_to(vec2 to, float seconds = 0.25f) { tween t = ponko_ui_tween_defaults(PONKO_UI_PROPERTY_OFFSET); t.to.point = to; t.seconds = seconds; return t; }
/// A tween of `p` to the color `to` (background, text color) over `seconds`.
inline tween tween_to(property p, color to, float seconds = 0.25f) { tween t = ponko_ui_tween_defaults(static_cast<std::uint32_t>(p)); t.to.color = to; t.seconds = seconds; return t; }
/// How often a timeline plays (Rust `UiRepeat`).
enum class repeat_mode : std::uint32_t {
    once = PONKO_UI_REPEAT_ONCE,            ///< Once.
    times = PONKO_UI_REPEAT_TIMES,          ///< A given number of times.
    loop = PONKO_UI_REPEAT_LOOP,            ///< From the start again until stopped.
    ping_pong = PONKO_UI_REPEAT_PING_PONG,  ///< Forward and back until stopped.
};
/// How a node is drawn now (Rust `UiDrawn`): rectangle after animation, opacity, scale, fill.
using drawn_state = PonkoUiDrawn;
/// "No item": what `tree::selected` gives when nothing is chosen.
constexpr std::uint64_t no_index = PONKO_UI_NO_INDEX;
/// Which way split panes divide (`tree::add_split`).
enum class axis : std::uint32_t {
    /// Side by side; the divider moves left and right.
    horizontal = PONKO_UI_AXIS_HORIZONTAL,
    /// One above the other; the divider moves up and down.
    vertical = PONKO_UI_AXIS_VERTICAL,
};

/// How a pushed screen treats the screens below it (`tree::push_screen`).
/// More modes may be added: give a `switch` on it a `default`.
enum class screen_mode : std::uint32_t {
    /// A menu or page: drawn as it is, no overlay.
    modeless = PONKO_UI_SCREEN_MODE_MODELESS,
    /// A dialog: dims the screens below, owns the pointer outside it, a dialog to screen readers.
    modal = PONKO_UI_SCREEN_MODE_MODAL,
};

/// What a ui::event is about.
/// More kinds may be added: give a `switch` on it a `default`.
enum class event_kind : std::uint32_t { clicked = 0, changed = 1, submitted = 2, screen_popped = 3, chosen = 4, dropped = 5, rows_shown = 6, animation_done = 7 };
/// Something that happened: a button was clicked, a value changed, Enter was
/// pressed in an input (`submitted`), Cancel popped a screen (`node` is its root),
/// a context-menu entry was chosen (`index`), a dragged node was dropped on
/// `other`, a list showed other items in its rows, or a tween or timeline on
/// `node` ended (`animation_done`).
struct event {
    event_kind kind;
    id node;
    /// The drop target of `dropped`; `none` otherwise.
    id other = PONKO_UI_NONE;
    /// The chosen entry of `chosen`; 0 otherwise.
    std::uint64_t index = 0;
};
/// One frame of input for `tree::frame`: `ui::default_input()`, then set what happened.
using input = PonkoUiInput;
/// Input with nothing happening: no pointer, no keys.
inline input default_input() { return ponko_ui_input_default(); }
/// One frame of input with what the widget set reads (`input` plus the frame
/// time, the secondary button, the menu key and the press a key-binding field
/// records): `ui::default_input_ex()`, then set what happened.
using input_ex = PonkoUiInputEx;
/// Input with nothing happening, in the layout that can grow.
inline input_ex default_input_ex() { input_ex i{}; i.struct_size = sizeof(input_ex); i.version = PONKO_UI_INPUT_EX_VERSION; detail::check(ponko_ui_input_ex_default(&i)); return i; }
/// An IME composition for `tree::frame` (Rust's `UiInput::preedit`,
/// `preedit_cursor` and `composing`): a preedit string, its caret, and
/// whether a composition went on this frame.
using composition = PonkoUiComposition;

/// Whether a node's changes are announced while something else has focus.
enum class live : std::uint32_t { off = 0, polite = 1, assertive = 2 };
/// What a node is to assistive technology.
/// More roles may be added: give a `switch` on it a `default`.
enum class role : std::uint32_t {
    group = 0, dialog = 1, label = 2, button = 3, image = 4, scroll_view = 5, text_input = 6, checkbox = 7, slider = 8,
    combo_box = 9, tab_list = 10, tab = 11, tab_panel = 12, radio_group = 13, radio_button = 14, progress_bar = 15,
    spinner = 16, spin_button = 17, list = 18, list_item = 19, grid = 20, grid_cell = 21, menu = 22, menu_item = 23,
    splitter = 24,
};
/// What `tree::access_action` asks the UI to do.
enum class access_action_kind : std::uint32_t { focus = 0, activate = 1, set_value = 2, increment = 3, decrement = 4, set_text = 5 };
class access;

/// The UI of a context (Rust `ctx.ui`); get it with ctx::ui_retained() or ctx::panel_ui(entity). A
/// cheap handle: copy it freely while the system runs, but don't keep it past the system call. Calls throw
/// ponko::error for a removed node.
class tree {
    friend class access;
    PonkoCtx* raw_;
    entity target_;
    static std::optional<id> maybe(id n) { return n == none ? std::nullopt : std::optional<id>(n); }
public:
    /// The screen's UI, or (with `target`) the UI of a world-space panel: `ctx::panel_ui(entity)`.
    explicit tree(PonkoCtx* raw, entity target = PONKO_UI_SCREEN) : raw_(raw), target_(target) {}

    // Adding nodes. `parent` is `ui::none` for a detached root (a screen).
    /// A panel: a box that holds and lays out other nodes. Returns its id; throws ponko::error if `parent`
    /// doesn't exist.
    id add_panel(id parent, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_panel(raw_, target_, parent, &c, &n)); return n;
    }
    /// A label showing `text`. Returns its id; throws ponko::error if `parent` doesn't exist.
    id add_label(id parent, const char* text, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_label(raw_, target_, parent, text, &c, &n)); return n;
    }
    /// A button; activating it makes a `clicked` event.
    id add_button(id parent, const char* text, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_button(raw_, target_, parent, text, &c, &n)); return n;
    }
    /// An image showing texture `t`. Returns its id; throws ponko::error if `parent` doesn't exist.
    id add_image(id parent, ponko::texture t, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_image(raw_, target_, parent, t, &c, &n)); return n;
    }
    /// A clipped area scrolled by wheel, drag and a bar. Give it a bounded size.
    id add_scroll(id parent, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_scroll(raw_, target_, parent, &c, &n)); return n;
    }
    /// A single-line input; typing makes `changed` events, Enter `submitted`.
    id add_text_input(id parent, const char* text = "", const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_text_input(raw_, target_, parent, text, &c, &n)); return n;
    }
    /// A checkbox; toggling it makes a `changed` event.
    id add_toggle(id parent, const char* text, bool on = false, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_toggle(raw_, target_, parent, text, on, &c, &n)); return n;
    }
    /// A slider from 0 to 1; moving it makes a `changed` event.
    id add_slider(id parent, float value = 0.0f, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_slider(raw_, target_, parent, value, &c, &n)); return n;
    }
    /// A title: a label in the theme's title color and size, which it keeps following.
    id add_title(id parent, const char* text, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_title(raw_, target_, parent, text, &c, &n)); return n;
    }
    /// Removes a node and everything under it; their ids stay invalid.
    void remove(id node) { detail::check(ponko_ctx_ui_remove(raw_, target_, node)); }

    // Changing nodes. No setter makes an event.
    /// Replaces a node's layout. Throws ponko::error for a removed node or an invalid grid template.
    void set_layout(id node, const layout& l) { const PonkoUiLayout c = l.raw(); detail::check(ponko_ctx_ui_set_layout(raw_, target_, node, &c)); }
    /// Names grid areas, one string per row (CSS `grid-template-areas`: `{"head head", "side main"}`).
    void set_grid_template_areas(id node, std::initializer_list<const char*> rows) { detail::check(ponko_ctx_ui_set_grid_template_areas(raw_, target_, node, rows.begin(), rows.size())); }
    /// Names the grid lines of `axis` (0 columns, 1 rows): one space-separated set of names per line.
    void set_grid_line_names(id node, std::uint32_t axis, std::initializer_list<const char*> names) { detail::check(ponko_ctx_ui_set_grid_line_names(raw_, target_, node, axis, names.begin(), names.size())); }
    /// Places a grid item in the named area `name`.
    void set_grid_area(id node, const char* name) { detail::check(ponko_ctx_ui_set_grid_area(raw_, target_, node, name)); }
    /// Puts one edge of a grid item on the `occurrence`th line named `name` (0: the first): axis 0 columns, 1
    /// rows; `end` false the start edge, true the end edge.
    void set_grid_named_line(id node, std::uint32_t axis, bool end, const char* name, int occurrence = 0) { detail::check(ponko_ctx_ui_set_grid_named_line(raw_, target_, node, axis, end, name, occurrence)); }
    /// One edge spans to the `count`th line named `name` (CSS `span count name`), counted from the other edge,
    /// which needs a line number (`ui::line`): axis 0 columns/1 rows, `end` false the start edge, true the end edge.
    void set_grid_named_span(id node, std::uint32_t axis, bool end, const char* name, std::uint32_t count = 1) { detail::check(ponko_ctx_ui_set_grid_named_span(raw_, target_, node, axis, end, name, count)); }
    /// Sets the paint fields `p` sets; the others keep following the theme.
    void set_paint(id node, const paint& p) { const PonkoUiPaint c = p.raw(); detail::check(ponko_ctx_ui_set_paint(raw_, target_, node, &c)); }
    /// Sets a label's, button's or text input's text.
    void set_text(id node, const char* text) { detail::check(ponko_ctx_ui_set_text(raw_, target_, node, text)); }
    /// Makes `node`'s text the string table key `key` with `args` (Rust `set_text_key`): it shows the
    /// current language's text and font from the next frame on and follows when the language changes.
    /// set_text replaces the key with plain text.
    void set_text_key(id node, const char* key, const std::vector<string_arg>& args = {}) {
        detail::check(ponko_ctx_ui_set_text_key(raw_, target_, node, key, args.data(), args.size()));
    }
    /// Gives every node that holds a key its text in the current language now (Rust `localize`): the
    /// engine does it before each frame; call it when driving the UI by hand.
    void localize() { detail::check(ponko_ctx_ui_localize(raw_, target_)); }
    /// A label's, button's or text input's text.
    std::string text(id node) const {
        std::size_t len = 0;
        detail::check(ponko_ctx_ui_text(raw_, target_, node, nullptr, 0, &len));
        std::string out(len + 1, '\0');
        detail::check(ponko_ctx_ui_text(raw_, target_, node, &out[0], out.size(), &len));
        out.resize(len);
        return out;
    }
    /// The node's paint as drawn: the theme's, plus what was set on it.
    PonkoUiPaint resolved_paint(id node) const { PonkoUiPaint p = ponko_ui_paint_default(); detail::check(ponko_ctx_ui_paint(raw_, target_, node, &p)); return p; }
    /// The tree's theme.
    ui::theme theme() const { ui::theme t = default_theme(); detail::check(ponko_ctx_ui_theme(raw_, target_, &t)); return t; }
    /// Replaces the theme; nodes keep the paint fields set on them.
    void set_theme(const ui::theme& t) { detail::check(ponko_ctx_ui_set_theme(raw_, target_, &t)); }
    /// Names a node, so code that didn't build it finds it: no list of ids to keep.
    /// A name belongs to one node; an empty name takes it off.
    void set_name(id node, const char* name) { detail::check(ponko_ctx_ui_set_name(raw_, target_, node, name)); }
    /// The node with this name, if any.
    std::optional<id> find(const char* name) const { id n = none; detail::check(ponko_ctx_ui_find(raw_, target_, name, &n)); return maybe(n); }
    /// The node with this name, for one that must be there (Rust's `Ui::try_find`): resolve it once after
    /// building and keep the id. A name no node has fails, and the error lists the node names; `.value()` throws it.
    result<id> try_find(const char* name) const {
        id n = none; const PonkoStatus status = ponko_ctx_ui_try_find(raw_, target_, name, &n); return detail::attempt(status, n);
    }
    /// A node's name (empty without one).
    std::string name(id node) const {
        std::size_t len = 0;
        detail::check(ponko_ctx_ui_name(raw_, target_, node, nullptr, 0, &len));
        std::string out(len + 1, '\0');
        detail::check(ponko_ctx_ui_name(raw_, target_, node, &out[0], out.size(), &len));
        out.resize(len);
        return out;
    }
    // Accessibility: what a screen reader reads. A name is the node's text
    // unless one is set; sliders, text inputs, images and icon-only buttons
    // need one. None of these change layout, drawing, focus or events.
    /// Sets what a screen reader calls the node; empty goes back to its text.
    void set_accessible_name(id node, const char* name) { detail::check(ponko_ctx_ui_set_accessible_name(raw_, target_, node, name)); }
    /// What a screen reader calls the node: its accessible name, or its text.
    std::string accessible_name(id node) const {
        std::size_t len = 0;
        detail::check(ponko_ctx_ui_accessible_name(raw_, target_, node, nullptr, 0, &len));
        std::string out(len + 1, '\0');
        detail::check(ponko_ctx_ui_accessible_name(raw_, target_, node, &out[0], out.size(), &len));
        out.resize(len);
        return out;
    }
    /// Read after the name: what a control does, or a hint. Empty removes it.
    void set_accessible_description(id node, const char* description) { detail::check(ponko_ctx_ui_set_accessible_description(raw_, target_, node, description)); }
    /// The node's accessible description (empty without one).
    std::string accessible_description(id node) const {
        std::size_t len = 0;
        detail::check(ponko_ctx_ui_accessible_description(raw_, target_, node, nullptr, 0, &len));
        std::string out(len + 1, '\0');
        detail::check(ponko_ctx_ui_accessible_description(raw_, target_, node, &out[0], out.size(), &len));
        out.resize(len);
        return out;
    }
    /// Announce a node's changes while something else has focus (a status line).
    void set_live_region(id node, live l) { detail::check(ponko_ctx_ui_set_live_region(raw_, target_, node, static_cast<std::uint32_t>(l))); }
    /// How the node's changes are announced.
    live live_region(id node) const { PonkoUiLive l = PONKO_UI_LIVE_OFF; detail::check(ponko_ctx_ui_live_region(raw_, target_, node, &l)); return static_cast<live>(l); }
    /// A request from assistive technology, applied by the UI's next frame:
    /// it does what the matching key does and queues the same events.
    void access_action(access_action_kind kind, id node, float value = 0.0f, const char* text = nullptr) {
        detail::check(ponko_ctx_ui_access_action(raw_, target_, static_cast<std::uint32_t>(kind), node, value, text));
    }
    /// The accessibility semantics as text, one line per exposed node: for
    /// golden tests and tools.
    std::string access_text() const {
        std::size_t len = 0;
        detail::check(ponko_ctx_ui_access_text(raw_, target_, nullptr, 0, &len));
        std::string out(len + 1, '\0');
        detail::check(ponko_ctx_ui_access_text(raw_, target_, &out[0], out.size(), &len));
        out.resize(len);
        return out;
    }

    /// A disabled node and everything under it ignore input.
    void set_enabled(id node, bool on) { detail::check(ponko_ctx_ui_set_enabled(raw_, target_, node, on)); }
    /// A slider's value (0 to 1) or a toggle's (0 off, 1 on).
    void set_value(id node, float value) { detail::check(ponko_ctx_ui_set_value(raw_, target_, node, value)); }
    /// A slider's value (0 to 1) or a toggle's (0 off, 1 on).
    float value(id node) const { float v = 0; detail::check(ponko_ctx_ui_value(raw_, target_, node, &v)); return v; }
    /// Scrolls a scroll area to `offset` points (clamped to its content).
    void set_scroll(id node, vec2 offset) { detail::check(ponko_ctx_ui_set_scroll(raw_, target_, node, offset)); }
    /// A scroll area's offset in points.
    vec2 scroll(id node) const { vec2 v{}; detail::check(ponko_ctx_ui_scroll(raw_, target_, node, &v)); return v; }
    /// A node's rectangle in window points, as of the last layout.
    ponko::rect rect(id node) const { ponko::rect r{}; detail::check(ponko_ctx_ui_rect(raw_, target_, node, &r)); return r; }

    // The widget set (Rust `Ui::dropdown`, `tabs`, `list`, ...). Builders return the new node; `l`
    // replaces the layout the widget starts with.
    /// A dropdown choosing one of `options`; choosing makes a `changed` event, read it with `selected`.
    id add_dropdown(id parent, const std::vector<const char*>& options, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_dropdown(raw_, target_, parent, options.data(), options.size(), &c, &n)); return n;
    }
    /// A tab strip: a bar of tabs over the page of the chosen one. Add tabs with `add_tab`.
    id add_tabs(id parent) { id n = none; detail::check(ponko_ctx_ui_add_tabs(raw_, target_, parent, nullptr, &n)); return n; }
    /// Adds a tab titled `title` to a tab strip; returns its page (a column panel: add the tab's content to it).
    id add_tab(id tabs, const char* title) { id n = none; detail::check(ponko_ctx_ui_add_tab(raw_, target_, tabs, title, &n)); return n; }
    /// Tab number `index` of a tab strip, if there is one.
    std::optional<id> tab_button(id tabs, std::uint64_t index) const {
        id n = none; const PonkoStatus status = ponko_ctx_ui_tab_button(raw_, target_, tabs, index, &n);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return n;
    }
    /// A radio group: a column of radio buttons, none checked at first. Add them with `add_radio`.
    id add_radio_group(id parent, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_radio_group(raw_, target_, parent, &c, &n)); return n;
    }
    /// Adds a radio button labelled `text` to a radio group.
    id add_radio(id group, const char* text, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_radio(raw_, target_, group, text, &c, &n)); return n;
    }
    /// A progress bar filled to `value` (0 to 1; change it with `set_value`).
    id add_progress_bar(id parent, float value, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_progress_bar(raw_, target_, parent, value, &c, &n)); return n;
    }
    /// A spinner: a busy indicator that turns while shown.
    id add_spinner(id parent) { id n = none; detail::check(ponko_ctx_ui_add_spinner(raw_, target_, parent, nullptr, &n)); return n; }
    /// A number field showing `value` (0 to 100 in steps of 1 until `set_number_range`).
    id add_number_field(id parent, double value, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_number_field(raw_, target_, parent, value, &c, &n)); return n;
    }
    /// A list of `row_height` point rows, or with `columns` > 1 a grid. It keeps nodes only for the rows on
    /// screen, so a million items cost what ten do. Fill it with `set_items` or `set_list_len`.
    id add_list(id parent, float row_height, std::uint32_t columns = 1, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_list(raw_, target_, parent, columns, row_height, &c, &n)); return n;
    }
    /// Split panes: two panes and a divider between them. `value` is the first pane's share of
    /// the length the divider leaves (0.5, equal panes, at first).
    id add_split(id parent, ui::axis a, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_split(raw_, target_, parent, static_cast<PonkoUiAxis>(a), &c, &n)); return n;
    }
    /// The first (left or top) and second pane of split panes.
    std::pair<id, id> split_panes(id split) const { id a = none, b = none; detail::check(ponko_ctx_ui_split_panes(raw_, target_, split, &a, &b)); return {a, b}; }
    /// A key-binding field showing `binding` (or none); activating it records the next press.
    id add_key_binding(id parent, std::optional<PonkoActionBinding> binding = std::nullopt, const layout& l = {}) {
        id n = none; const PonkoUiLayout c = l.raw();
        detail::check(ponko_ctx_ui_add_key_binding(raw_, target_, parent, binding ? &*binding : nullptr, &c, &n)); return n;
    }
    /// The options of a dropdown, the items of a list or the entries of a context menu.
    void set_items(id node, const std::vector<const char*>& items) { detail::check(ponko_ctx_ui_set_items(raw_, target_, node, items.data(), items.size())); }
    /// Item `index` of a dropdown or list.
    std::string item(id node, std::uint64_t index) const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ctx_ui_item(raw_, target_, node, index, b, c, l); });
    }
    /// The chosen option, tab, radio button or list item, if any.
    std::optional<std::uint64_t> selected(id node) const {
        std::uint64_t i = no_index; detail::check(ponko_ctx_ui_selected(raw_, target_, node, &i));
        return i == no_index ? std::nullopt : std::optional<std::uint64_t>(i);
    }
    /// Chooses `index` (or nothing) without an event.
    void set_selected(id node, std::optional<std::uint64_t> index) { detail::check(ponko_ctx_ui_set_selected(raw_, target_, node, index.value_or(no_index))); }
    /// A number field's value.
    double number(id node) const { double v = 0; detail::check(ponko_ctx_ui_number(raw_, target_, node, &v)); return v; }
    /// Sets a number field's value, kept in its range, without an event.
    void set_number(id node, double value) { detail::check(ponko_ctx_ui_set_number(raw_, target_, node, value)); }
    /// A number field's range and step.
    void set_number_range(id node, double min, double max, double step) { detail::check(ponko_ctx_ui_set_number_range(raw_, target_, node, min, max, step)); }
    /// A number field's limits and step.
    struct number_limits {
        double min; ///< Minimum accepted value.
        double max; ///< Maximum accepted value.
        double step; ///< Value increment.
    };
    /// Reads a number field's limits and step.
    number_limits number_range(id node) const {
        number_limits range{};
        detail::check(ponko_ctx_ui_number_range(raw_, target_, node, &range.min, &range.max, &range.step));
        return range;
    }
    /// How many items a list has, for rows drawn without text.
    void set_list_len(id node, std::uint64_t len) { detail::check(ponko_ctx_ui_set_list_len(raw_, target_, node, len)); }
    /// The total number of items, including those outside the viewport.
    std::uint64_t list_len(id node) const {
        std::uint64_t n = 0;
        detail::check(ponko_ctx_ui_list_len(raw_, target_, node, &n));
        return n;
    }
    /// The item a list row shows now, if any (rows are reused: look again after `rows_shown`).
    std::optional<std::uint64_t> list_item(id row) const {
        std::uint64_t i = no_index; detail::check(ponko_ctx_ui_list_item(raw_, target_, row, &i));
        return i == no_index ? std::nullopt : std::optional<std::uint64_t>(i);
    }
    /// Scrolls a list so item `index` is on screen.
    void scroll_to_item(id node, std::uint64_t index) { detail::check(ponko_ctx_ui_scroll_to_item(raw_, target_, node, index)); }
    /// A key-binding field's binding, if it has one.
    std::optional<PonkoActionBinding> binding(id node) const {
        PonkoActionBinding b{}; bool bound = false; detail::check(ponko_ctx_ui_binding(raw_, target_, node, &b, &bound));
        return bound ? std::optional<PonkoActionBinding>(b) : std::nullopt;
    }
    /// Sets a key-binding field's binding (or none) without an event.
    void set_binding(id node, std::optional<PonkoActionBinding> b) { detail::check(ponko_ctx_ui_set_binding(raw_, target_, node, b ? &*b : nullptr)); }
    /// Whether a key-binding field is listening for a press.
    bool is_capturing(id node) const { bool b = false; detail::check(ponko_ctx_ui_is_capturing(raw_, target_, node, &b)); return b; }
    /// A node's tooltip (empty for none): shown on hover after the theme's delay, at once on key focus.
    void set_tooltip(id node, const char* text) { detail::check(ponko_ctx_ui_set_tooltip(raw_, target_, node, text)); }
    /// The tooltip assigned to a node, if present.
    std::optional<std::string> tooltip(id node) const {
        std::size_t len = 0;
        auto status = ponko_ctx_ui_tooltip(raw_, target_, node, nullptr, 0, &len);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        std::string text(len + 1, '\0');
        detail::check(ponko_ctx_ui_tooltip(raw_, target_, node, text.data(), text.size(), &len));
        text.resize(len);
        return text;
    }
    /// The node whose tooltip shows now, and where.
    std::optional<std::pair<id, ponko::rect>> shown_tooltip() const {
        id n = none; ponko::rect r{}; const PonkoStatus status = ponko_ctx_ui_shown_tooltip(raw_, target_, &n, &r);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return std::make_pair(n, r);
    }
    /// A node's context menu (no entries for none): the right button or the menu key opens it; choosing
    /// an entry makes a `chosen` event.
    void set_context_menu(id node, const std::vector<const char*>& entries) { detail::check(ponko_ctx_ui_set_context_menu(raw_, target_, node, entries.data(), entries.size())); }
    /// Opens a node's context menu below it.
    void open_context_menu(id node) { detail::check(ponko_ctx_ui_open_context_menu(raw_, target_, node)); }
    /// The node whose pop-up (dropdown options, context menu) is open, if any.
    std::optional<id> popup_owner() const { id n = none; detail::check(ponko_ctx_ui_popup_owner(raw_, target_, &n)); return maybe(n); }
    /// Closes the open pop-up, choosing nothing.
    void close_popup() { detail::check(ponko_ctx_ui_close_popup(raw_, target_)); }
    /// Lets the pointer drag `node` and confirm carry it (`dropped` events on drop targets).
    void set_draggable(id node, bool on = true) { detail::check(ponko_ctx_ui_set_draggable(raw_, target_, node, on)); }
    /// Makes `node` a drop target for dragged nodes.
    void set_drop_target(id node, bool on = true) { detail::check(ponko_ctx_ui_set_drop_target(raw_, target_, node, on)); }
    /// The node being dragged or carried, if any.
    std::optional<id> dragging() const { id n = none; detail::check(ponko_ctx_ui_dragging(raw_, target_, &n)); return maybe(n); }
    /// Moves `node` under `parent` as its last child (a dropped item into its new container).
    void set_parent(id node, id parent) { detail::check(ponko_ctx_ui_set_parent(raw_, target_, node, parent)); }
    /// The tree's whole theme, tokens included.
    ui::theme_ex theme_ex() const { ui::theme_ex t = blank_theme_ex(); detail::check(ponko_ctx_ui_theme_ex(raw_, target_, &t)); return t; }
    /// Replaces the whole theme, tokens included.
    void set_theme(const ui::theme_ex& t) { detail::check(ponko_ctx_ui_set_theme_ex(raw_, target_, &t)); }

    // Animation, on the UI clock (the frame time, running while the simulation is paused).
    /// Plays `t` on `node`: its property goes to `t.to`, replacing whatever moved it; an
    /// `animation_done` event follows.
    void animate(id node, const ui::tween& t) { detail::check(ponko_ctx_ui_animate(raw_, target_, node, &t)); }
    /// Plays a timeline of `steps` (each at its `start`) on `node`, `how` often (`n` times for `repeat_mode::times`).
    void play(id node, const std::vector<ui::tween>& steps, ui::repeat_mode how = ui::repeat_mode::once, std::uint32_t n = 0) {
        detail::check(ponko_ctx_ui_play(raw_, target_, node, steps.data(), steps.size(), static_cast<std::uint32_t>(how), n));
    }
    /// Stops the tweens and timelines on `node` where they are.
    void stop_animations(id node) { detail::check(ponko_ctx_ui_stop_animations(raw_, target_, node)); }
    /// Whether a tween, timeline or screen transition moves `node`.
    bool is_animating(id node) const { bool b = false; detail::check(ponko_ctx_ui_is_animating(raw_, target_, node, &b)); return b; }
    /// How screen `root` comes on when pushed and goes off when popped.
    void set_screen_transition(id root, const ui::screen_transition& enter, const ui::screen_transition& exit) {
        detail::check(ponko_ctx_ui_set_screen_transition(raw_, target_, root, &enter, &exit));
    }
    /// The transitions screen `root` comes on and goes off with.
    std::pair<ui::screen_transition, ui::screen_transition> screen_transition(id root) const {
        ui::screen_transition enter{}, exit{};
        detail::check(ponko_ctx_ui_screen_transition(raw_, target_, root, &enter, &exit));
        return {enter, exit};
    }
    /// How `node` is drawn as of the last layout.
    ui::drawn_state drawn(id node) const { ui::drawn_state d{}; detail::check(ponko_ctx_ui_drawn(raw_, target_, node, &d)); return d; }
    /// Seconds on the UI clock.
    double clock() const { double s = 0.0; detail::check(ponko_ctx_ui_clock(raw_, target_, &s)); return s; }
    /// Reduced motion: state and screen transitions happen at once.
    void set_reduced_motion(bool on) { detail::check(ponko_ctx_ui_set_reduced_motion(raw_, target_, on)); }
    /// Whether reduced motion is on.
    bool reduced_motion() const { bool b = false; detail::check(ponko_ctx_ui_reduced_motion(raw_, target_, &b)); return b; }

    // Screens and focus. Only the top screen takes input; a modal one also owns the pointer outside it.
    /// Shows the tree under `root` as the top screen, in `mode` (`screen_mode::modal` blocks the pointer outside it too).
    void push_screen(id root, screen_mode mode = screen_mode::modeless) {
        detail::check(ponko_ctx_ui_push_screen_with_mode(raw_, target_, root, static_cast<std::uint32_t>(mode)));
    }
    /// Shows the tree under `root` as a modal top screen: a dialog.
    void push_modal(id root) { push_screen(root, screen_mode::modal); }
    /// Shows the tree under `root` as a modeless top screen: a menu or page.
    void push_modeless(id root) { push_screen(root, screen_mode::modeless); }
    /// The old form: `true` is `screen_mode::modal`.
    [[deprecated("use push_modal(root), push_modeless(root) or push_screen(root, ponko::ui::screen_mode)")]]
    /// Compatibility overload: true requests a modal screen.
    void push_screen(id root, bool modal) { push_screen(root, modal ? screen_mode::modal : screen_mode::modeless); }
    /// Pops the top screen; the nodes stay and can be pushed again.
    std::optional<id> pop_screen() { id n = none; detail::check(ponko_ctx_ui_pop_screen(raw_, target_, &n)); return maybe(n); }
    /// The top screen's root, if a screen is open.
    std::optional<id> top_screen() const { id n = none; detail::check(ponko_ctx_ui_top_screen(raw_, target_, &n)); return maybe(n); }
    /// The mode `root` was pushed with, if it is on the screen stack.
    std::optional<screen_mode> mode_of_screen(id root) const {
        std::uint32_t mode = 0;
        auto status = ponko_ctx_ui_screen_mode(raw_, target_, root, &mode);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return static_cast<screen_mode>(mode);
    }
    /// Whether `node` is shown: on a screen of the stack, neither it nor a node above it hidden.
    bool is_shown(id node) const { bool s = false; detail::check(ponko_ctx_ui_is_shown(raw_, target_, node, &s)); return s; }
    /// How many times the tree has been laid out (`Ui::layout_passes`): a layout runs only when something changed since the last one, so the count stays the same over a still screen's frames.
    std::uint64_t layout_passes() const { std::uint64_t n = 0; detail::check(ponko_ctx_ui_layout_passes(raw_, target_, &n)); return n; }
    /// The focused node, if any.
    std::optional<id> focus() const { id n = none; detail::check(ponko_ctx_ui_focus(raw_, target_, &n)); return maybe(n); }
    /// Focuses `node` (an enabled, shown button, text input, toggle or slider on the top screen), as Tab would.
    void set_focus(id node) { detail::check(ponko_ctx_ui_set_focus(raw_, target_, node)); }
    /// Takes the focus away.
    void clear_focus() { detail::check(ponko_ctx_ui_clear_focus(raw_, target_)); }
    /// Marks `node` as the one its screen opens on: every push of it focuses `node`.
    void set_initial_focus(id node, bool on = true) { detail::check(ponko_ctx_ui_set_initial_focus(raw_, target_, node, on)); }
    /// Makes button `node` also close its screen when pressed, as Escape would (a Back, Close or Resume button).
    void set_closes_screen(id node, bool on = true) { detail::check(ponko_ctx_ui_set_closes_screen(raw_, target_, node, on)); }
    /// Whether pressing `node` closes its screen.
    bool closes_screen(id node) const { bool on = false; detail::check(ponko_ctx_ui_closes_screen(raw_, target_, node, &on)); return on; }
    /// Lays root `node` out inside the window's safe area, clear of a phone's notch and home bar (`on` false: in the whole window, the default).
    void set_inside_safe_area(id node, bool on = true) { detail::check(ponko_ctx_ui_set_inside_safe_area(raw_, target_, node, on)); }
    /// Whether root `node` is laid out inside the safe area.
    bool inside_safe_area(id node) const { bool on = false; detail::check(ponko_ctx_ui_inside_safe_area(raw_, target_, node, &on)); return on; }
    /// Sets the part of the viewport, in its pixels, that roots made with set_inside_safe_area are laid out
    /// in (Rust `Ui::set_safe_area`). The engine sets the screen's from ctx::window_safe_area every frame,
    /// so this is for a UI of the game's own, a world panel's. A rectangle that isn't finite counts as none.
    void set_safe_area(ponko::rect area) { detail::check(ponko_ctx_ui_set_safe_area(raw_, target_, &area)); }
    /// Clears the safe area: roots inside it are laid out in the whole viewport again.
    void clear_safe_area() { detail::check(ponko_ctx_ui_set_safe_area(raw_, target_, nullptr)); }
    /// The safe area set with set_safe_area, or nothing: roots inside it use the whole viewport.
    std::optional<ponko::rect> safe_area() const {
        ponko::rect r{};
        const auto status = ponko_ctx_ui_safe_area(raw_, target_, &r);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return r;
    }
    /// Fades `node` and everything inside it as one image (drawn opaque into an offscreen layer shown at its opacity), so overlapping children don't show through each other (`on` false: piece by piece, the default).
    void set_fade_as_group(id node, bool on = true) { detail::check(ponko_ctx_ui_set_fade_as_group(raw_, target_, node, on)); }
    /// Whether `node` fades as one image.
    bool fades_as_group(id node) const { bool on = false; detail::check(ponko_ctx_ui_fades_as_group(raw_, target_, node, &on)); return on; }
    /// Shows the tree under `root` as a HUD: drawn below every screen, never closed by Escape, never takes input.
    void show_hud(id root) { detail::check(ponko_ctx_ui_show_hud(raw_, target_, root)); }
    /// Takes a HUD off the screen; its nodes stay.
    void hide_hud(id root) { detail::check(ponko_ctx_ui_hide_hud(raw_, target_, root)); }
    /// Whether `node` is a HUD's root.
    bool is_hud(id node) const { bool b = false; detail::check(ponko_ctx_ui_is_hud(raw_, target_, node, &b)); return b; }
    /// The topmost node under `point` (window points), as of the last layout.
    std::optional<id> hit_test(vec2 point) const { id n = none; detail::check(ponko_ctx_ui_hit_test(raw_, target_, point, &n)); return maybe(n); }
    /// A scale on top of the window's own, from 0.25 to 8.
    void set_scale(float scale) { detail::check(ponko_ctx_ui_set_scale(raw_, target_, scale)); }
    /// The UI owns the pointer (it is over a control or a modal screen is open): skip clicks on the world.
    bool wants_pointer() const { bool b = false; detail::check(ponko_ctx_ui_wants_pointer(raw_, target_, &b)); return b; }
    /// The UI owns the keyboard and gamepad (a screen is open): skip the game's keys. The context's, not a panel's.
    bool wants_keyboard() const { bool b = false; detail::check(ponko_ctx_ui_wants_keyboard(raw_, &b)); return b; }

    // Events. Take them every frame; unread ones expire at the next UI frame.
    /// The oldest unread event, or nothing.
    std::optional<event> next_event() {
        PonkoUiEventEx e{};
        e.struct_size = sizeof(e); e.version = PONKO_UI_EVENT_EX_VERSION;
        const PonkoStatus status = ponko_ctx_ui_next_event_ex(raw_, target_, &e);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return event{static_cast<event_kind>(e.kind), e.id, e.other, e.index};
    }
    /// Takes the queued events and gives the last click's node, if any: for
    /// screens that react only to clicks (the other events are dropped).
    std::optional<id> take_click() { id n = none; detail::check(ponko_ctx_ui_take_click(raw_, target_, &n)); return maybe(n); }
    /// Takes every queued event.
    std::vector<event> events() {
        std::vector<event> all;
        while (auto e = next_event()) all.push_back(*e);
        return all;
    }

    // Driving the UI by hand (headless tests, custom hosts): the engine already does this every frame.
    /// Lays out the tree for a `viewport` in points, without handling input.
    void layout_for(vec2 viewport) { detail::check(ponko_ctx_ui_layout(raw_, target_, viewport)); }
    /// Lays out and handles one frame of `in`; this drops unread events.
    void frame(vec2 viewport, const input& in = default_input()) { detail::check(ponko_ctx_ui_frame(raw_, target_, viewport, &in)); }
    /// Lays out and handles one frame of `in` with the widget set's input (frame time, secondary button,
    /// menu key, a captured press).
    void frame(vec2 viewport, const input_ex& in) { detail::check(ponko_ctx_ui_frame_ex(raw_, target_, viewport, &in)); }
    /// Typed text for the engine's next UI frame (the context's, not a panel's).
    void insert_text(const char* text) { detail::check(ponko_ctx_ui_insert_text(raw_, text)); }
    /// Lays out and handles one frame of `in` with an IME composition `c`: the focused text input
    /// that takes IME shows `c.preedit` at its caret.
    void frame(vec2 viewport, const input& in, const composition& c) { detail::check(ponko_ctx_ui_frame_ime(raw_, target_, viewport, &in, &c)); }

    // IME composition (Rust `set_ime_allowed`, `preedit`, `ime_area`, ...).
    /// Lets the text input `node` take IME composition (the default), or not (digits, a key binding, a password).
    void set_ime_allowed(id node, bool allowed) { detail::check(ponko_ctx_ui_set_ime_allowed(raw_, target_, node, allowed)); }
    /// Whether the text input `node` takes IME composition.
    bool ime_allowed(id node) const { bool b = false; detail::check(ponko_ctx_ui_ime_allowed(raw_, target_, node, &b)); return b; }
    /// Whether the focus is on a text input that takes IME: a custom host allows IME for its window then.
    bool wants_ime() const { bool b = false; detail::check(ponko_ctx_ui_wants_ime(raw_, target_, &b)); return b; }
    /// The IME preedit `node` shows at its caret; empty when it shows none.
    std::string preedit(id node) const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ctx_ui_preedit(raw_, target_, node, b, c, l); });
    }
    /// The focused IME text input's caret in window points, as of the last frame: where the candidate window goes.
    std::optional<ponko::rect> ime_area() const { bool has = false; ponko::rect r{}; detail::check(ponko_ctx_ui_ime_area(raw_, target_, &has, &r)); return has ? std::optional<ponko::rect>(r) : std::nullopt; }
    /// Whether the last frame took the focus off a text input mid-composition (then cancel the composition).
    bool composition_interrupted() const { bool b = false; detail::check(ponko_ctx_ui_composition_interrupted(raw_, target_, &b)); return b; }
};

/// A snapshot of one UI's accessibility semantics (roles, names, values,
/// states, focus) that knows what changed since its last `update`: what an
/// accessibility adapter or a test reads. Use one per UI; while nothing
/// changed, `update` allocates nothing.
class access {
    PonkoUiAccess* raw_;
    using reader = PonkoStatus (*)(PonkoUiAccess*, std::uint64_t, char*, std::size_t, std::size_t*);
    std::string read(reader f, id node) const {
        std::size_t len = 0;
        detail::check(f(raw_, node, nullptr, 0, &len));
        std::string out(len + 1, '\0');
        detail::check(f(raw_, node, &out[0], out.size(), &len));
        out.resize(len);
        return out;
    }
public:
    /// One exposed node; `role`, `value_kind` and `live` hold the C enums.
    using node = PonkoUiAccessNode;
    /// What an update found: the changed and removed nodes, in tree order.
    struct changes {
        bool any = false;
        bool screens_changed = false;
        bool focus_changed = false;
        std::vector<id> changed;
        std::vector<id> removed;
    };
    /// An empty snapshot; the first update reports every node. Throws std::runtime_error if the library can't
    /// make one.
    access() : raw_(ponko_ui_access_new()) { if (!raw_) throw std::runtime_error(ponko_last_error()); }
    ~access() { ponko_ui_access_free(raw_); }
    /// Takes `o`'s snapshot; `o` is left empty.
    access(access&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps snapshots with `o`.
    access& operator=(access&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: it owns its snapshot.
    ///@{
    access(const access&) = delete;
    access& operator=(const access&) = delete;
    ///@}
    /// Looks at the UI (after its frame) and says what changed since the
    /// last look. The first look reports every exposed node.
    changes update(const tree& ui) {
        PonkoUiAccessChanges c{};
        detail::check(ponko_ui_access_update(raw_, ui.raw_, ui.target_, &c));
        changes out{c.any, c.screens_changed, c.focus_changed, {}, {}};
        out.changed.resize(c.changed_count);
        for (std::size_t i = 0; i < out.changed.size(); ++i) detail::check(ponko_ui_access_changed(raw_, i, &out.changed[i]));
        out.removed.resize(c.removed_count);
        for (std::size_t i = 0; i < out.removed.size(); ++i) detail::check(ponko_ui_access_removed(raw_, i, &out.removed[i]));
        return out;
    }
    /// Forgets what was seen: the next update reports every node again.
    void reset() { detail::check(ponko_ui_access_reset(raw_)); }
    /// How many nodes are exposed.
    std::size_t size() const { std::size_t n = 0; detail::check(ponko_ui_access_count(raw_, &n)); return n; }
    /// Node number `index`, in tree order (a parent before its children).
    node at(std::size_t index) const { node n{}; detail::check(ponko_ui_access_node(raw_, index, &n)); return n; }
    /// The node for a UI node, unless it is hidden, removed or covered by a modal screen.
    std::optional<node> find(id ui_node) const {
        node n{};
        const PonkoStatus status = ponko_ui_access_find(raw_, ui_node, &n);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return n;
    }
    /// An exposed node's children, in layout order.
    std::vector<id> children(id ui_node) const {
        node n{};
        detail::check(ponko_ui_access_find(raw_, ui_node, &n));
        std::vector<id> out(n.child_count);
        for (std::size_t i = 0; i < out.size(); ++i) detail::check(ponko_ui_access_child(raw_, ui_node, i, &out[i]));
        return out;
    }
    /// An exposed node's accessible name (its text unless one was set).
    std::string name(id ui_node) const { return read(ponko_ui_access_name, ui_node); }
    /// An exposed node's accessible description.
    std::string description(id ui_node) const { return read(ponko_ui_access_description, ui_node); }
    /// A text input's text; empty for a node whose value is not text.
    std::string value_text(id ui_node) const { return read(ponko_ui_access_value_text, ui_node); }
    /// What the widget set adds to a node's state: chosen, open, its place in its set, a number's range.
    using state = PonkoUiAccessStateEx;
    /// An exposed node's widget state.
    state state_of(id ui_node) const {
        state s{}; s.struct_size = sizeof(s); s.version = PONKO_UI_ACCESS_STATE_EX_VERSION;
        detail::check(ponko_ui_access_state_ex(raw_, ui_node, &s)); return s;
    }
    /// The focused exposed node, if any.
    std::optional<id> focus() const { id n = PONKO_UI_NONE; detail::check(ponko_ui_access_focus(raw_, &n)); return n == PONKO_UI_NONE ? std::nullopt : std::optional<id>(n); }
};
}  // namespace ui

/// An animation clip (Rust `AnimationClip`): from ctx::load_clip (a glTF file with skins) or made empty for
/// morph tracks. Move-only; the destructor frees it.
class clip {
    PonkoClip* raw_ = nullptr;
public:
    /// Takes ownership of a C clip (from the C API); the destructor frees it.
    explicit clip(PonkoClip* raw) : raw_(raw) {}
    /// Empty clip for procedural morph animation.
    explicit clip(const char* name) { detail::check(ponko_clip_new(name, &raw_)); }
    /// Scalar keys: interpolation 0 STEP, 1 LINEAR, 2 cubic in/value/out triples.
    void set_morph_track(const char* name, const std::vector<float>& times,
                         const std::vector<float>& values, unsigned interpolation = 1) {
        if (interpolation > 2 || values.size() != times.size() * (interpolation == 2 ? 3 : 1))
            throw std::invalid_argument("clip::set_morph_track: invalid key count or interpolation");
        detail::check(ponko_clip_set_morph_track(raw_, name, times.data(), values.data(), times.size(), interpolation));
    }
    ~clip() { ponko_clip_free(raw_); }
    /// Takes `o`'s clip; `o` is left empty.
    clip(clip&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps clips with `o`.
    clip& operator=(clip&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: it owns its keyframes. motion::of copies it.
    ///@{
    clip(const clip&) = delete;
    clip& operator=(const clip&) = delete;
    ///@}
    /// Its length in seconds.
    float duration() const noexcept { return ponko_clip_duration(raw_); }
    /// The C clip, for the C API. Owned by this object.
    const PonkoClip* raw() const noexcept { return raw_; }
};

/// One joint's keys for one channel (PonkoJointTrack), for ctx::create_clip. `times` and `values` point into
/// arrays the caller keeps until create_clip returns: four floats per value, three values per key for
/// CUBICSPLINE.
using joint_track = PonkoJointTrack;
/// Which part of a joint's pose a joint_track animates.
using joint_channel = PonkoJointChannel;
/// A joint_track over `times` and `values` (4 floats per key; 12 for interpolation 2, cubic). Both
/// vectors must outlive the create_clip call.
inline joint_track joint_track_of(std::uint32_t joint, joint_channel channel, const std::vector<float>& times,
                                  const std::vector<float>& values, std::uint32_t interpolation = 1) {
    if (interpolation > 2 || values.size() != times.size() * 4 * (interpolation == 2 ? 3 : 1))
        throw std::invalid_argument("joint_track_of: four floats per key (twelve for cubic), interpolation 0 to 2");
    return joint_track{joint, static_cast<std::uint32_t>(channel), interpolation, times.data(), values.data(), times.size()};
}
/// Live handles of every kind in the versioned form (PonkoLiveCountsEx): `counts` is live_counts, plus
/// crowds, instance_cells, collision_meshes, sprite_shaders, sprite_materials and sky_shaders.
using live_counts_ex = PonkoLiveCountsEx;

/// A clip to bake for crowds: `c` (copied by the bake), looping or holding its last frame.
inline crowd_clip crowd_clip_of(const clip& c, bool looping = true) noexcept { return crowd_clip{c.raw(), looping}; }

/// A crowd character at `position`: white, playing clip 0 from its start at its own pace.
inline crowd_instance crowd_instance_at(vec3 position) noexcept { return ponko_crowd_instance_at(position); }

/// Clips baked for crowds (Rust `CrowdAnimation`), from ctx::bake_crowd_animation. Move-only; the destructor
/// frees it. Crowds keep what they need: it may go once they are made.
class crowd_animation {
    PonkoCrowdAnimation* raw_ = nullptr;
public:
    /// Takes ownership of a C bake (from the C API); the destructor frees it.
    explicit crowd_animation(PonkoCrowdAnimation* raw) : raw_(raw) {}
    ~crowd_animation() { ponko_crowd_animation_free(raw_); }
    /// Takes `o`'s bake; `o` is left empty.
    crowd_animation(crowd_animation&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps bakes with `o`.
    crowd_animation& operator=(crowd_animation&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: it owns the baked frames.
    ///@{
    crowd_animation(const crowd_animation&) = delete;
    crowd_animation& operator=(const crowd_animation&) = delete;
    ///@}
    /// The index of the clip called `name`, for crowd_instance::clip and blend_clip.
    std::optional<std::uint16_t> clip(const char* name) const {
        std::uint16_t index = 0;
        const PonkoStatus status = ponko_crowd_animation_clip(raw_, name, &index);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return index;
    }
    /// The C bake, for the C API. Owned by this object.
    const PonkoCrowdAnimation* raw() const noexcept { return raw_; }
};

/// A bone of a humanoid skeleton (Rust `HumanBone`), for humanoid_map.
using human_bone = PonkoHumanBone;
/// The bone's name in words, such as "left upper leg".
inline const char* human_bone_name(human_bone b) noexcept { return ponko_human_bone_name(static_cast<std::uint32_t>(b)); }
/// Whether retargeting needs `b` mapped on both skeletons: the hips, spine, head, and each arm and leg down to
/// the hand and foot (Rust `HumanBone::is_required`).
inline bool human_bone_is_required(human_bone b) noexcept { return ponko_human_bone_is_required(static_cast<std::uint32_t>(b)); }

/// Which joint of a skeleton plays each human_bone (Rust `HumanoidMap`): empty, or from
/// ctx::detect_humanoid_map. Move-only; the destructor frees it.
class humanoid_map {
    PonkoHumanoidMap* raw_ = nullptr;
public:
    /// An empty map: no bone mapped.
    humanoid_map() { detail::check(ponko_humanoid_map_new(&raw_)); }
    /// Takes ownership of a C map (from the C API).
    explicit humanoid_map(PonkoHumanoidMap* raw) noexcept : raw_(raw) {}
    ~humanoid_map() { ponko_humanoid_map_free(raw_); }
    /// Takes `o`'s map; `o` is left empty.
    humanoid_map(humanoid_map&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps maps with `o`.
    humanoid_map& operator=(humanoid_map&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable.
    ///@{
    humanoid_map(const humanoid_map&) = delete;
    humanoid_map& operator=(const humanoid_map&) = delete;
    ///@}
    /// The joint mapped to `b`, if any.
    std::optional<std::uint32_t> get(human_bone b) const {
        std::int32_t j = -1;
        detail::check(ponko_humanoid_map_get(raw_, static_cast<std::uint32_t>(b), &j));
        return j < 0 ? std::nullopt : std::optional<std::uint32_t>(static_cast<std::uint32_t>(j));
    }
    /// Maps `b` to joint `joint` (checked against the skeleton when a retargeter is made).
    void set(human_bone b, std::uint32_t joint) {
        if (joint > static_cast<std::uint32_t>(INT32_MAX)) throw std::out_of_range("humanoid_map::set: joint index too large");
        detail::check(ponko_humanoid_map_set(raw_, static_cast<std::uint32_t>(b), static_cast<std::int32_t>(joint)));
    }
    /// Unmaps `b`.
    void unset(human_bone b) { detail::check(ponko_humanoid_map_set(raw_, static_cast<std::uint32_t>(b), -1)); }
    /// The required bones it doesn't map (empty: retargeting can use it).
    std::vector<human_bone> missing() const {
        std::array<std::uint32_t, 64> bones{};
        std::size_t n = 0;
        detail::check(ponko_humanoid_map_missing(raw_, bones.data(), bones.size(), &n));
        std::vector<human_bone> out;
        for (std::size_t i = 0; i < n && i < bones.size(); ++i) out.push_back(static_cast<human_bone>(bones[i]));
        return out;
    }
    /// The C map, for the C API. Owned by this object.
    const PonkoHumanoidMap* raw() const noexcept { return raw_; }
};

/// Prepared retargeting from one humanoid skeleton to another (Rust `Retargeter`), from
/// ctx::create_retargeter. It copies both skeletons' rest data, so it outlives their entities.
/// Move-only; the destructor frees it.
class retargeter {
    PonkoRetargeter* raw_ = nullptr;
public:
    /// Takes ownership of a C retargeter (from the C API).
    explicit retargeter(PonkoRetargeter* raw) noexcept : raw_(raw) {}
    ~retargeter() { ponko_retargeter_free(raw_); }
    /// Takes `o`'s retargeter; `o` is left empty.
    retargeter(retargeter&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps retargeters with `o`.
    retargeter& operator=(retargeter&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable.
    ///@{
    retargeter(const retargeter&) = delete;
    retargeter& operator=(const retargeter&) = delete;
    ///@}
    /// How much further the target moves than the source (leg length ratio).
    float scale() const { float s = 0; detail::check(ponko_retargeter_scale(raw_, &s)); return s; }
    /// Whether the feet are placed with two-bone IK (on by default).
    void set_foot_ik(bool on) { detail::check(ponko_retargeter_set_foot_ik(raw_, on)); }
    /// Keys per second added where the source's keys are further apart (default 30).
    void set_sample_rate(float rate) { detail::check(ponko_retargeter_set_sample_rate(raw_, rate)); }
    /// `c` (made for the source skeleton) as a new clip for the target skeleton. Samples the whole clip:
    /// do it once per clip, at load.
    ponko::clip retarget(const ponko::clip& c) {
        PonkoClip* out = nullptr;
        detail::check(ponko_retarget_clip(raw_, c.raw(), &out));
        return ponko::clip(out);
    }
    /// The C retargeter, for the C API. Owned by this object.
    PonkoRetargeter* raw() const noexcept { return raw_; }
};

/// A motion for an animation state: a clip, or a 1D/2D blend tree of motions.
/// Handing it to a tree or a state moves it.
class motion {
    PonkoMotion* raw_ = nullptr;
    explicit motion(PonkoMotion* raw) : raw_(raw) {}
public:
    ~motion() { ponko_motion_free(raw_); }
    /// Takes `o`'s motion; `o` is left empty.
    motion(motion&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps motions with `o`.
    motion& operator=(motion&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: handing a motion over moves it.
    ///@{
    motion(const motion&) = delete;
    motion& operator=(const motion&) = delete;
    ///@}
    /// Plays `c` (copied).
    static motion of(const clip& c) { PonkoMotion* m = nullptr; detail::check(ponko_motion_clip(c.raw(), &m)); return motion(m); }
    /// A 1D blend tree on float parameter `parameter`.
    static motion blend1d(std::size_t parameter) { PonkoMotion* m = nullptr; detail::check(ponko_motion_blend1d(parameter, &m)); return motion(m); }
    /// A 2D blend tree on float parameters `x` and `y`.
    static motion blend2d(std::size_t x, std::size_t y) { PonkoMotion* m = nullptr; detail::check(ponko_motion_blend2d(x, y, &m)); return motion(m); }
    /// Adds `child` to a 1D tree at `threshold` (in increasing order).
    motion& add(float threshold, motion child, float time_scale = 1) {
        detail::check(ponko_motion_blend1d_add(raw_, threshold, child.release(), time_scale)); return *this;
    }
    /// Adds `child` to a 2D tree at point (x, y).
    motion& add(float x, float y, motion child, float time_scale = 1) {
        detail::check(ponko_motion_blend2d_add(raw_, x, y, child.release(), time_scale)); return *this;
    }
    /// Gives up ownership: the caller (a C call) now owns the C motion.
    PonkoMotion* release() noexcept { auto r = raw_; raw_ = nullptr; return r; }
};

/// How a layer's pose combines with the layers under it: replacing them (weighted), or added on top.
enum class layer_blend : std::uint32_t { override_pose = 0, additive = 1 };
/// How root motion is handled (Rust `RootMotion`): left in the pose (off), measured for you to read
/// (extract), or applied to the entity's transform (apply).
enum class root_motion : std::uint32_t { off = 0, extract = 1, apply = 2 };

/// A finished, immutable animation graph (Rust `AnimationGraph`), shared by the animators made from it.
/// Move-only; the destructor frees it.
class animation_graph {
    PonkoAnimationGraph* raw_ = nullptr;
public:
    /// Takes ownership of a C graph (from animation_graph_builder::build); the destructor frees it.
    explicit animation_graph(PonkoAnimationGraph* raw) : raw_(raw) {}
    ~animation_graph() { ponko_animation_graph_free(raw_); }
    /// Takes `o`'s graph; `o` is left empty.
    animation_graph(animation_graph&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps graphs with `o`.
    animation_graph& operator=(animation_graph&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Not copyable: animators share a graph through ctx; build one per kind of character.
    ///@{
    animation_graph(const animation_graph&) = delete;
    animation_graph& operator=(const animation_graph&) = delete;
    ///@}
    /// The C graph, for the C API. Owned by this object.
    const PonkoAnimationGraph* raw() const noexcept { return raw_; }
};

/// Builds an animation_graph: parameters, then states, then transitions.
/// Indices count from 0 in the order added.
class animation_graph_builder {
    PonkoAnimationGraphBuilder* raw_ = ponko_animation_graph_builder_new();
public:
    /// An empty builder.
    animation_graph_builder() = default;
    ~animation_graph_builder() { ponko_animation_graph_builder_free(raw_); }
    /// Not copyable: it owns what was added so far.
    ///@{
    animation_graph_builder(const animation_graph_builder&) = delete;
    animation_graph_builder& operator=(const animation_graph_builder&) = delete;
    ///@}
    /// A float parameter (for blend trees and conditions), starting at `value`. Returns its index.
    std::size_t add_float(float value = 0) { std::size_t i = 0; detail::check(ponko_animation_graph_add_float(raw_, value, &i)); return i; }
    /// A bool parameter, starting at `value`. Returns its index.
    std::size_t add_bool(bool value = false) { std::size_t i = 0; detail::check(ponko_animation_graph_add_bool(raw_, value, &i)); return i; }
    /// A trigger parameter: set by animator_set_trigger, cleared when a transition uses it. Returns its
    /// index.
    std::size_t add_trigger() { std::size_t i = 0; detail::check(ponko_animation_graph_add_trigger(raw_, &i)); return i; }
    /// A state playing `m`; its speed is also multiplied by `speed_parameter` if given.
    std::size_t add_state(motion m, float speed = 1, std::optional<std::size_t> speed_parameter = std::nullopt, bool looping = true) {
        std::size_t i = 0;
        auto p = speed_parameter ? static_cast<std::int64_t>(*speed_parameter) : -1;
        detail::check(ponko_animation_graph_add_state(raw_, m.release(), speed, p, looping, &i));
        return i;
    }
    /// An event on `state` at normalized time `time` (0 to 1).
    void add_event(std::size_t state, float time, const char* name) { detail::check(ponko_animation_graph_add_event(raw_, state, time, name)); }
    /// A transition from `from` (std::nullopt: any state) to `to`, crossfading over `duration` seconds.
    std::size_t add_transition(std::optional<std::size_t> from, std::size_t to, float duration,
                               std::optional<float> exit_time = std::nullopt, float offset = 0) {
        std::size_t i = 0;
        auto f = from ? static_cast<std::int64_t>(*from) : -1;
        detail::check(ponko_animation_graph_add_transition(raw_, f, to, exit_time ? *exit_time : -1.0f, duration, offset, &i));
        return i;
    }
    /// A condition on `transition`; all must hold.
    void add_condition(std::size_t transition, PonkoAnimationCondition kind, std::size_t parameter, float value = 0) {
        detail::check(ponko_animation_graph_add_condition(raw_, transition, kind, parameter, value));
    }
    /// Checks and finishes the graph, starting in `initial`; the builder is used up.
    animation_graph build(std::size_t initial = 0) {
        PonkoAnimationGraph* g = nullptr;
        auto b = raw_;
        raw_ = nullptr;
        detail::check(ponko_animation_graph_build(b, initial, &g));
        return animation_graph(g);
    }
};

/// Inline lookups into a flow field, from any thread (flow_field::view()).
/// Expires when the field is computed again.
class flow_view {
    PonkoFlowView v_{};
public:
    /// An empty view: every direction is zero.
    flow_view() = default;
    /// Wraps a C view; flow_field::view makes these.
    explicit flow_view(const PonkoFlowView& v) : v_(v) {}
    /// Unit direction toward the nearest goal, or zero (goal, wall, outside, unreachable).
    vec2 direction_at(vec2 p) const noexcept {
        if (!v_.cells) return {0, 0};
        const float cx = std::floor((p.x - v_.origin.x) / v_.cell_size), cy = std::floor((p.y - v_.origin.y) / v_.cell_size);
        if (!(cx >= 0 && cy >= 0 && cx < float(v_.width) && cy < float(v_.height))) return {0, 0};
        return v_.directions[v_.cells[(std::size_t(cy) + 1) * v_.stride + std::size_t(cx) + 1]];
    }
};

/// The way to the nearest goal from every cell of a nav_grid (Rust `FlowField`).
class flow_field {
    PonkoFlowField* raw_ = ponko_flow_new();
public:
    /// An empty field: every direction is zero until compute(). The destructor frees it.
    flow_field() = default;
    ~flow_field() { ponko_flow_free(raw_); }
    /// Not copyable: it owns its cells.
    ///@{
    flow_field(const flow_field&) = delete;
    flow_field& operator=(const flow_field&) = delete;
    ///@}
    /// Recomputes for these goals (world points); reuses its memory.
    void compute(const nav_grid& nav, const std::vector<vec2>& goals) {
        detail::check(ponko_flow_compute(raw_, nav.raw(), goals.data(), goals.size()));
    }
    /// Recomputes for a single goal (a world point).
    void compute(const nav_grid& nav, vec2 goal) { detail::check(ponko_flow_compute(raw_, nav.raw(), &goal, 1)); }
    /// Unit direction toward the nearest goal at world point `p`, or zero (on a goal, a wall, outside,
    /// unreachable).
    vec2 direction_at(vec2 p) const noexcept { return ponko_flow_direction(raw_, p); }
    /// Length of the shortest way to a goal in world units, or nothing if none can be reached.
    std::optional<float> distance_at(vec2 p) const {
        float d = 0;
        const auto status = ponko_flow_distance(raw_, p, &d);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return d;
    }
    /// Inline lookups for hot loops (query_par), valid until the next compute.
    flow_view view() const { PonkoFlowView v{}; detail::check(ponko_flow_view(raw_, &v)); return flow_view(v); }
};

/// Events read by ctx::read: a borrowed array, valid until the next send of
/// the type or the end of the system.
template<class T> class events_view {
    const T* data_;
    std::size_t size_;
public:
    /// A view of `size` events at `data`; ctx::read makes these.
    events_view(const T* data, std::size_t size) noexcept : data_(data), size_(size) {}
    /// Iterate the events, oldest first.
    ///@{
    const T* begin() const noexcept { return data_; }
    const T* end() const noexcept { return data_ + size_; }
    ///@}
    /// How many events there are.
    std::size_t size() const noexcept { return size_; }
    /// Whether there are none.
    bool empty() const noexcept { return size_ == 0; }
    /// Event `i` (unchecked: `i` must be below size()).
    const T& operator[](std::size_t i) const noexcept { return data_[i]; }
};

/// One cell of a cell_streamer's grid: it covers x * cell_size to
/// (x + 1) * cell_size along X, and the same along Z.
using cell = PonkoCell;

/// Which cells of a square grid on the ground (X and Z) to have loaded
/// around a point, such as the camera (Rust `CellStreamer`): those within
/// load_radius, started nearest first, and kept until they're beyond
/// keep_radius. It only keeps the list; the game loads and drops each cell.
class cell_streamer {
    std::unique_ptr<PonkoCellStreamer, void (*)(PonkoCellStreamer*)> raw_;
    float cell_size_;
    static PonkoCellStreamer* make(float cell_size, float load_radius, float keep_radius) {
        PonkoCellStreamer* s = ponko_cells_new(cell_size, load_radius, keep_radius);
        if (!s) throw std::invalid_argument(ponko_last_error());
        return s;
    }
    events_view<cell> list(PonkoStatus (*f)(PonkoCellStreamer*, const PonkoCell**, std::size_t*)) const {
        const PonkoCell* c = nullptr; std::size_t n = 0;
        detail::check(f(raw_.get(), &c, &n));
        return {c, n};
    }
public:
    /// cell_size above 0, radii not negative (keep_radius is raised to load_radius).
    cell_streamer(float cell_size, float load_radius, float keep_radius)
        : raw_(make(cell_size, load_radius, keep_radius), ponko_cells_free), cell_size_(cell_size) {}
    /// Works out the cells to load and drop around `at` (its height is
    /// ignored): at most max_new new ones, nearest first.
    void update(vec3 at, std::size_t max_new) { detail::check(ponko_cells_update(raw_.get(), at, max_new)); }
    /// What the last update started loading, nearest first; valid until the next update.
    events_view<cell> to_load() const { return list(ponko_cells_to_load); }
    /// What the last update dropped; valid until the next update.
    events_view<cell> to_drop() const { return list(ponko_cells_to_drop); }
    /// Whether cell `c` is loaded (started by an update and not dropped since).
    bool is_loaded(cell c) const {
        bool loaded = false; detail::check(ponko_cells_is_loaded(raw_.get(), c, &loaded, nullptr)); return loaded;
    }
    /// How many cells are loaded.
    std::size_t loaded_count() const {
        bool loaded = false; std::size_t n = 0;
        detail::check(ponko_cells_is_loaded(raw_.get(), cell{0, 0}, &loaded, &n)); return n;
    }
    /// Where a cell starts: its corner with the lowest X and Z, at height 0.
    vec3 corner(cell c) const noexcept { return {float(c.x) * cell_size_, 0.0f, float(c.z) * cell_size_}; }
    /// Forgets every cell, as if none were loaded.
    void clear() { detail::check(ponko_cells_clear(raw_.get())); }
};

/// A terrain tile's heights, for finding the ground under a point on the
/// CPU (Rust `Heightfield`): coordinates from the tile's corner, heights
/// following the drawn triangles.
class heightfield {
    std::unique_ptr<PonkoHeightfield, void (*)(PonkoHeightfield*)> raw_;
    static PonkoHeightfield* make(const std::vector<float>& heights, float size) {
        PonkoHeightfield* g = ponko_heightfield_new(heights.data(), heights.size(), size);
        if (!g) throw std::invalid_argument(ponko_last_error());
        return g;
    }
public:
    /// n x n heights (row after row along +Z, NaN for a hole), `size` wide.
    heightfield(const std::vector<float>& heights, float size) : raw_(make(heights, size), ponko_heightfield_free) {}
    /// The height at (x, z); nothing off the tile or over a hole.
    std::optional<float> height_at(float x, float z) const {
        bool found = false; float y = 0;
        detail::check(ponko_heightfield_at(raw_.get(), x, z, &found, &y, nullptr));
        if (!found) return std::nullopt;
        return y;
    }
    /// Which way the flat triangle at (x, z) faces; nothing off the tile or over a hole.
    std::optional<vec3> normal_at(float x, float z) const {
        bool found = false; float y = 0; vec3 n{};
        detail::check(ponko_heightfield_at(raw_.get(), x, z, &found, &y, &n));
        if (!found) return std::nullopt;
        return n;
    }
};

/// A terrain tile's meshes at every detail level, built on the CPU (Rust
/// `TerrainTileData`): make it on any thread (std::async), then
/// ctx::upload_terrain_tile on the game's.
class terrain_tile_data {
    std::unique_ptr<PonkoTerrainTileData, void (*)(PonkoTerrainTileData*)> raw_;
    static PonkoTerrainTileData* make(const std::vector<float>& heights, float size) {
        PonkoTerrainTileData* d = ponko_terrain_tile_data_new(heights.data(), heights.size(), size);
        if (!d) throw std::invalid_argument(ponko_last_error());
        return d;
    }
public:
    /// n x n heights (row after row along +Z, NaN for a hole), `size` wide.
    terrain_tile_data(const std::vector<float>& heights, float size)
        : raw_(make(heights, size), ponko_terrain_tile_data_free) {}
    /// The C data, for the C API. Owned by this object.
    const PonkoTerrainTileData* raw() const noexcept { return raw_.get(); }
};

/// A convex hull, triangle mesh or heightfield built on the CPU (Rust
/// `CollisionMeshData`): make it on any thread (std::async), then
/// ctx::add_collision_mesh on the game's, which is quick.
class collision_mesh_data {
    friend class ctx;  // hands the data to the engine from a terrain rebuild
    std::unique_ptr<PonkoCollisionMeshData, void (*)(PonkoCollisionMeshData*)> raw_;
    explicit collision_mesh_data(PonkoCollisionMeshData* d) : raw_(d, ponko_collision_mesh_data_free) {
        if (!d) throw std::invalid_argument(ponko_last_error());
    }
public:
    /// A heightfield from n x n heights `size` wide (row after row along +Z,
    /// NaN for a hole), as ctx::heightfield_collider makes it.
    static collision_mesh_data heightfield(const std::vector<float>& heights, float size) {
        return collision_mesh_data(ponko_collision_mesh_data_heightfield(heights.data(), heights.size(), size));
    }
    /// A triangle mesh: three indices into `vertices` per triangle.
    static collision_mesh_data trimesh(const std::vector<vec3>& vertices, const std::vector<std::uint32_t>& indices) {
        return collision_mesh_data(
            ponko_collision_mesh_data_trimesh(vertices.data(), vertices.size(), indices.data(), indices.size()));
    }
    /// A convex hull around `points`.
    static collision_mesh_data convex_hull(const std::vector<vec3>& points) {
        return collision_mesh_data(ponko_collision_mesh_data_convex_hull(points.data(), points.size()));
    }
    /// The C data, for the C API. Owned by this object.
    const PonkoCollisionMeshData* raw() const noexcept { return raw_.get(); }
};

/// One physical source for a named action. Configure once or when rebinding.
struct action_binding {
    PonkoActionBinding raw{};
    /// Key `k` (a button, or 1 for an axis).
    static action_binding key(ponko::key k) { return {{PONKO_ACTION_KEY, 0, static_cast<std::uint32_t>(k), 0, 0, 0, 0}}; }
    /// Mouse button `b`.
    static action_binding mouse(ponko::mouse b) { return {{PONKO_ACTION_MOUSE, 0, static_cast<std::uint32_t>(b), 0, 0, 0, 0}}; }
    /// Button `b` of gamepad `p`.
    static action_binding button(gamepad p, gamepad_button b) { return {{PONKO_ACTION_GAMEPAD_BUTTON, p.slot, static_cast<std::uint32_t>(b), 0, 0, 0, 0}}; }
    /// Two keys as an axis: `negative` -1, `positive` +1, both 0.
    static action_binding keys(ponko::key negative, ponko::key positive) { return {{PONKO_ACTION_KEYS, 0, static_cast<std::uint32_t>(negative), static_cast<std::uint32_t>(positive), 0, 0, 0}}; }
    /// Axis `a` of gamepad `p`; values within `deadzone` of 0 read 0.
    static action_binding axis(gamepad p, gamepad_axis a, float deadzone = 0.15f) { return {{PONKO_ACTION_GAMEPAD_AXIS, p.slot, static_cast<std::uint32_t>(a), 0, 0, 0, deadzone}}; }
    /// Four keys as a vector axis (WASD style): left, right, up, down.
    static action_binding keys2(ponko::key left, ponko::key right, ponko::key up, ponko::key down) { return {{PONKO_ACTION_KEYS2, 0, static_cast<std::uint32_t>(left), static_cast<std::uint32_t>(right), static_cast<std::uint32_t>(up), static_cast<std::uint32_t>(down), 0}}; }
    /// Stick `s` of gamepad `p` as a vector axis, with a radial `deadzone`.
    static action_binding stick(gamepad p, ponko::stick s, float deadzone = 0.15f) { return {{PONKO_ACTION_GAMEPAD_STICK, p.slot, s == ponko::stick::left ? 0u : 1u, 0, 0, 0, deadzone}}; }
    /// Touch gesture `g` as a button: a tap or a swipe for one frame, a long press, drag, pinch or
    /// rotation while it lasts.
    static action_binding gesture(ponko::gesture g) { return {{PONKO_ACTION_GESTURE, 0, static_cast<std::uint32_t>(g), 0, 0, 0, 0}}; }
    /// A gesture's amount this frame as a scalar axis, times `scale`.
    static action_binding gesture_axis(ponko::gesture_axis a, float scale = 1.0f) { return {{PONKO_ACTION_GESTURE_AXIS, 0, static_cast<std::uint32_t>(a), 0, 0, 0, scale}}; }
    /// A one-finger drag's movement this frame, in pixels times `scale`, as a vector axis.
    static action_binding drag(float scale = 1.0f) { return {{PONKO_ACTION_DRAG, 0, 0, 0, 0, 0, scale}}; }
};

/// A button, scalar axis, or vector axis with several alternative sources.
struct action {
    std::uint32_t kind;
    std::vector<PonkoActionBinding> bindings;
    /// A button action: down while any of `sources` is.
    static action button(std::initializer_list<action_binding> sources) { return make(0, sources); }
    /// A scalar axis action (-1 to 1): the source with the largest magnitude wins.
    static action axis(std::initializer_list<action_binding> sources) { return make(1, sources); }
    /// A vector axis action (each component -1 to 1, length at most 1).
    static action axis2(std::initializer_list<action_binding> sources) { return make(2, sources); }
private:
    static action make(std::uint32_t kind, std::initializer_list<action_binding> sources) {
        action a{kind, {}};
        a.bindings.reserve(sources.size());
        for (const auto& s : sources) a.bindings.push_back(s.raw);
        return a;
    }
};

/// Owned, serializable bindings. Install with ctx::set_actions; edit and reinstall to rebind.
class action_map {
    PonkoActionMap* raw_ = nullptr;
public:
    /// An empty map. Throws std::runtime_error if the library can't make one.
    action_map() : raw_(ponko_action_map_new()) { if (!raw_) throw std::runtime_error(ponko_last_error()); }
    ~action_map() { ponko_action_map_free(raw_); }
    /// Not copyable: it owns its bindings. to_json and from_json make a copy.
    ///@{
    action_map(const action_map&) = delete;
    action_map& operator=(const action_map&) = delete;
    ///@}
    /// Takes `other`'s bindings; `other` is left empty.
    action_map(action_map&& other) noexcept : raw_(std::exchange(other.raw_, nullptr)) {}
    /// Frees this map's bindings and takes `other`'s.
    action_map& operator=(action_map&& other) noexcept { if (this != &other) { ponko_action_map_free(raw_); raw_ = std::exchange(other.raw_, nullptr); } return *this; }
    /// Binds action `name` (replacing an action of that name). Throws ponko::error for an empty name or an
    /// invalid binding.
    void set(const char* name, const action& a) { detail::check(ponko_action_map_set(raw_, name, a.kind, a.bindings.data(), a.bindings.size())); }
    /// Removes action `name` (nothing if there is none).
    void remove(const char* name) { detail::check(ponko_action_map_remove(raw_, name)); }
    /// The number of actions (Rust `ActionMap::len`).
    std::size_t size() const { std::size_t n = 0; detail::check(ponko_action_map_count(raw_, &n)); return n; }
    /// Every action's name, in name order (Rust `ActionMap::iter`): with get(), how a rebinding screen
    /// lists the actions and their sources.
    std::vector<std::string> names() const {
        std::vector<std::string> list(size());
        for (std::size_t i = 0; i < list.size(); ++i) {
            list[i] = detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_action_map_name(raw_, i, b, c, n); });
        }
        return list;
    }
    /// The bindings of action `name` (Rust `ActionMap::get`), or std::nullopt if there is none.
    std::optional<action> get(const char* name) const {
        action a{0, {}};
        std::size_t n = 0;
        const PonkoStatus status = ponko_action_map_get(raw_, name, &a.kind, nullptr, 0, &n);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        a.bindings.resize(n);
        detail::check(ponko_action_map_get(raw_, name, &a.kind, a.bindings.data(), a.bindings.size(), &n));
        return a;
    }
    /// The bindings in the versioned JSON format Rust and tools read (Rust `ActionMap::to_json`).
    std::string to_json() const {
        std::size_t len = 0;
        detail::check(ponko_action_map_save(raw_, nullptr, 0, &len));
        std::string json(len + 1, '\0');
        detail::check(ponko_action_map_save(raw_, &json[0], json.size(), &len));
        json.resize(len); return json;
    }
    /// Reads bindings from JSON written by to_json (or Rust). Throws ponko::error for invalid JSON or an
    /// unknown version.
    static action_map from_json(const std::string& json) {
        action_map result;
        detail::check(ponko_action_map_load(result.raw_, json.c_str()));
        return result;
    }
    /// The C map, for the C API. Owned by this object.
    PonkoActionMap* raw() const noexcept { return raw_; }
};

/// What a system declares it reads and writes: components, resources and
/// events by C++ type (their registered names) or by name, commands and
/// services. Pass it to game::add with a system; debug builds stop the game
/// if the system uses anything it didn't declare. Copyable. Build it at
/// registration, not per frame: every step copies the declaration and keeps
/// each name for the rest of the program.
class access {
    PonkoAccess* raw_ = nullptr;
    access& step(PonkoStatus s) { detail::check(s); return *this; }
public:
    /// An empty declaration (uses nothing). Throws std::runtime_error if the library can't make one.
    access() : raw_(ponko_access_new()) { if (!raw_) throw std::runtime_error(ponko_last_error()); }
    ~access() { ponko_access_free(raw_); }
    /// A copy of `other`.
    access(const access& other) { detail::check(ponko_access_clone(other.raw_, &raw_)); }
    /// Replaces this declaration with a copy of `other`.
    access& operator=(const access& other) {
        if (this != &other) { access copy(other); std::swap(raw_, copy.raw_); }
        return *this;
    }
    /// What built-in system `id` (systems::physics, systems::step_navmesh3d, ...) declares it touches
    /// (Rust `systems::step_physics_access` and the others); the built-ins without a declaration of their
    /// own are exclusive. Compare yours with it (conflicts_before) to see whether a system could run
    /// beside the step. Throws ponko::error for an unknown id or a physics step the library lacks.
    static access builtin(system_id id) {
        PonkoAccess* raw = nullptr;
        detail::check(ponko_builtin_access(id.bits, &raw));
        return access(raw);
    }
    /// Takes `other`'s declaration; `other` is left empty.
    access(access&& other) noexcept : raw_(std::exchange(other.raw_, nullptr)) {}
    /// Frees this declaration and takes `other`'s.
    access& operator=(access&& other) noexcept { if (this != &other) { ponko_access_free(raw_); raw_ = std::exchange(other.raw_, nullptr); } return *this; }
    /// Reads component T.
    template<class T> access& read() { return read_component(detail::type_name<T>().c_str()); }
    /// Reads and writes component T.
    template<class T> access& write() { return write_component(detail::type_name<T>().c_str()); }
    /// Reads resource T.
    template<class T> access& read_resource() { return read_resource(detail::type_name<T>().c_str()); }
    /// Reads and writes resource T.
    template<class T> access& write_resource() { return write_resource(detail::type_name<T>().c_str()); }
    /// Reads T events.
    template<class T> access& read_events() { return read_events(detail::type_name<T>().c_str()); }
    /// Sends T events (doesn't cover reading them).
    template<class T> access& send_events() { return send_events(detail::type_name<T>().c_str()); }
    /// Reads the component registered under `name`.
    access& read_component(const char* name) { return step(ponko_access_read_component(raw_, name)); }
    /// Reads and writes the component registered under `name`.
    access& write_component(const char* name) { return step(ponko_access_write_component(raw_, name)); }
    /// Reads the resource registered under `name`.
    access& read_resource(const char* name) { return step(ponko_access_read_resource(raw_, name)); }
    /// Reads and writes the resource registered under `name`.
    access& write_resource(const char* name) { return step(ponko_access_write_resource(raw_, name)); }
    /// Reads events registered under `name`.
    access& read_events(const char* name) { return step(ponko_access_read_events(raw_, name)); }
    /// Sends events registered under `name`.
    access& send_events(const char* name) { return step(ponko_access_send_events(raw_, name)); }
    /// Records commands (ctx::*_later).
    access& commands() { return step(ponko_access_commands(raw_)); }
    /// Uses `s`: reading it, or also changing it.
    access& read_service(service s) { return step(ponko_access_service(raw_, static_cast<std::uint32_t>(s), false)); }
    /// Uses and changes service `s`.
    access& write_service(service s) { return step(ponko_access_service(raw_, static_cast<std::uint32_t>(s), true)); }
    /// Number of reasons this system, registered before `later`, can't share
    /// a parallel batch with it (0: it could). `ordered = false` checks
    /// either order.
    std::size_t conflicts_before(const access& later, bool ordered = true) const {
        std::size_t n = 0;
        detail::check(ponko_access_conflicts(raw_, later.raw_, ordered, &n));
        return n;
    }
    /// The C declaration, for the C API. Owned by this object.
    const PonkoAccess* raw() const noexcept { return raw_; }
private:
    explicit access(PonkoAccess* raw) noexcept : raw_(raw) {}
};

/// One input that went down this frame (ctx::pressed_this_frame): a key, mouse button, gamepad
/// button or gamepad axis, described the way `action_binding` is, so a rebinding screen can bind it
/// with `action::button({p.binding()})` (or `action::axis` for an axis).
struct pressed_input {
    /// The C description (a PonkoActionBinding of kind key, mouse, gamepad button or gamepad axis).
    PonkoActionBinding raw{};
    /// Whether it is a keyboard key.
    bool is_key() const noexcept { return raw.kind == PONKO_ACTION_KEY; }
    /// Whether it is a mouse button.
    bool is_mouse() const noexcept { return raw.kind == PONKO_ACTION_MOUSE; }
    /// Whether it is a gamepad button.
    bool is_gamepad_button() const noexcept { return raw.kind == PONKO_ACTION_GAMEPAD_BUTTON; }
    /// Whether it is a stick or trigger (bind it with `action::axis`).
    bool is_gamepad_axis() const noexcept { return raw.kind == PONKO_ACTION_GAMEPAD_AXIS; }
    /// Whether it is a touch gesture (bind it with `action::button`).
    bool is_gesture() const noexcept { return raw.kind == PONKO_ACTION_GESTURE; }
    /// The gesture; meaningful when is_gesture().
    ponko::gesture gesture() const noexcept { return static_cast<ponko::gesture>(raw.first); }
    /// The key; meaningful when is_key().
    ponko::key key() const noexcept { return static_cast<ponko::key>(raw.first); }
    /// The mouse button; meaningful when is_mouse().
    ponko::mouse mouse() const noexcept { return static_cast<ponko::mouse>(raw.first); }
    /// The gamepad (slot) of a gamepad button or axis.
    gamepad pad() const noexcept { return gamepad{raw.pad}; }
    /// The gamepad button; meaningful when is_gamepad_button().
    gamepad_button button() const noexcept { return static_cast<gamepad_button>(raw.first); }
    /// The gamepad axis; meaningful when is_gamepad_axis().
    gamepad_axis axis() const noexcept { return static_cast<gamepad_axis>(raw.first); }
    /// The binding to put in an action: `action::button({p.binding()})`, or `action::axis` for an axis.
    action_binding binding() const noexcept { return action_binding{raw}; }
};

/// A controller's button-label family (Rust `ControllerFamily`): which name and shape a prompt shows.
enum class controller_family : std::uint32_t { generic = 0, xbox = 1, playstation = 2, nintendo = 3 };
/// What a connected controller is (Rust `GamepadInfo`, its name from ctx::gamepad_name): `family`,
/// USB ids when `has_ids`, and what it can do (`rumble`, `trigger_rumble`, `motion`).
using gamepad_info = PonkoGamepadInfo;
/// How hard each motor runs, 0..1 (Rust `MotorLevels`).
using motor_levels = PonkoMotorLevels;
/// A rumble request for ctx::rumble (Rust `Rumble`): body motors `strong` (low frequency) and `weak`
/// (high frequency), trigger motors where there are any, each 0..1, for `seconds` of real time.
struct rumble {
    float strong = 0, weak = 0, left_trigger = 0, right_trigger = 0, seconds = 0.2f;
};
/// One gyro (rad/s) and accelerometer (m/s², gravity included) reading (Rust `GamepadMotion`).
/// Axes: x right, y up, z toward the player.
struct gamepad_motion {
    vec3 gyro{}, accel{};
};
/// The device a person played with (Rust `InputDevice`): ctx::last_device, a player's device.
struct input_device {
    std::uint32_t kind = PONKO_DEVICE_KEYBOARD_MOUSE;
    std::uint32_t slot = 0;
    /// The keyboard and mouse.
    static input_device keyboard_mouse() noexcept { return {PONKO_DEVICE_KEYBOARD_MOUSE, 0}; }
    /// Gamepad `p`.
    static input_device of(gamepad p) noexcept { return {PONKO_DEVICE_GAMEPAD, p.slot}; }
    /// Whether it is a gamepad (then `gamepad_of()` is it).
    bool is_gamepad() const noexcept { return kind == PONKO_DEVICE_GAMEPAD; }
    /// The gamepad, for a gamepad device.
    gamepad gamepad_of() const noexcept { return gamepad{slot}; }
    /// Whether both are the same device.
    bool operator==(const input_device& o) const noexcept { return kind == o.kind && slot == o.slot; }
    /// Whether they are different devices.
    bool operator!=(const input_device& o) const noexcept { return !(*this == o); }
};
/// One button-prompt image (Rust `Glyph`): `kind` a PonkoGlyphKind, `family` a controller_family for
/// the gamepad kinds, `code` the key, mouse button, gamepad button, stick (0 left, 1 right) or axis.
using glyph = PonkoGlyph;
/// The name a player reads for `g`: "A", "Cross", "Space", "Left Mouse".
inline std::string glyph_label(glyph g) {
    return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_glyph_label(g, b, c, n); });
}
/// One key of a keyboard layout, for ctx::set_key_labels: `key` (a PonkoKey) and the Unicode
/// `character` it types without a modifier (`{PONKO_KEY_W, 'z'}` on a French keyboard).
using layout_key = PonkoKeyLabel;
/// The glyphs of one action's binding (ctx::action_glyphs): at most four, no allocation.
struct glyphs {
    std::array<glyph, 4> items{};
    std::size_t count = 0;
    /// Iterate the glyphs in reading order.
    ///@{
    const glyph* begin() const noexcept { return items.data(); }
    const glyph* end() const noexcept { return items.data() + count; }
    std::size_t size() const noexcept { return count; }
    bool empty() const noexcept { return count == 0; }
    ///@}
    /// The labels joined with '/': "W/A/S/D", a text-only prompt.
    std::string label() const {
        std::string out;
        for (std::size_t i = 0; i < count; ++i) { if (i) out += '/'; out += glyph_label(items[i]); }
        return out;
    }
};
/// Every glyph drawn into one RGBA image, a cell each (Rust `GlyphAtlas`), with the engine's own
/// simple shapes. Make it once; draw prompts with ctx::draw_prompt.
class glyph_atlas {
    PonkoGlyphAtlas* raw_;
public:
    /// Draws every glyph at `cell` pixels square (16..256).
    explicit glyph_atlas(std::uint32_t cell = 48) : raw_(ponko_glyph_atlas_new(cell)) {}
    ~glyph_atlas() { ponko_glyph_atlas_free(raw_); }
    /// Not copyable: it owns its image.
    glyph_atlas(const glyph_atlas&) = delete;
    /// Not assignable by copy.
    glyph_atlas& operator=(const glyph_atlas&) = delete;
    /// Takes `o`'s image; `o` is left empty.
    glyph_atlas(glyph_atlas&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Swaps images with `o`; the destructor of `o` frees the old one.
    glyph_atlas& operator=(glyph_atlas&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Where `g` is in the image, in pixels: a draw region.
    rect region(glyph g) const { rect r{}; detail::check(ponko_glyph_atlas_region(raw_, g, &r)); return r; }
    /// The RGBA8 image (owned by the atlas), its width and height.
    const std::uint8_t* pixels(std::uint32_t& width, std::uint32_t& height) const {
        const std::uint8_t* p = nullptr; detail::check(ponko_glyph_atlas_pixels(raw_, &p, &width, &height)); return p;
    }
    /// The C handle.
    PonkoGlyphAtlas* raw() const noexcept { return raw_; }
};
/// How on-screen controls look (Rust `TouchStyle`): the colors of a stick's base and knob and of a
/// button idle, held and its label, which tint the images (drawn white); an image of the game's own
/// in place of the engine's disc (`has_image`, `image`); a size. For touch_controls::style and
/// control_style.
struct touch_style : PonkoTouchStyle {
    /// The engine's look: see-through white discs, size 1.
    touch_style() : PonkoTouchStyle(ponko_touch_style_default()) {}
};
/// Where an on-screen control's centre sits (Rust `TouchPlace`): at `anchor`, a point of the view
/// from (0, 0) top left to (1, 1) bottom right, moved by `offset` UI units.
struct touch_place : PonkoTouchPlace {
    /// Centred `offset` UI units from `anchor`.
    touch_place(vec2 anchor, vec2 offset) : PonkoTouchPlace{anchor, offset} {}
};
/// What the player changed about one on-screen control (Rust `TouchAdjust`): where they put it
/// (`placed`, `place`) and its size, times the game's. From ctx::touch_layout_control.
using touch_adjust = PonkoTouchAdjust;

/// On-screen controls for ctx::set_touch_controls (Rust `TouchControls`): sticks in the bottom
/// corners, round buttons at the bottom right, small buttons in the top right corner, each driving a
/// named action while a finger holds it. Shown on touch devices only, unless show() says otherwise;
/// they step aside while a UI screen is open, unless over_screens(true).
class touch_controls {
public:
    /// A stick at the bottom left for vector or scalar action `action`.
    touch_controls& stick(std::string action) { return add(PONKO_TOUCH_STICK, std::move(action), {}); }
    /// A stick at the bottom right (look or aim).
    touch_controls& right_stick(std::string action) { return add(PONKO_TOUCH_RIGHT_STICK, std::move(action), {}); }
    /// A round button labelled `label` at the bottom right for button action `action`.
    touch_controls& button(std::string action, std::string label) { return add(PONKO_TOUCH_BUTTON, std::move(action), std::move(label)); }
    /// A small button in the top right corner (pause, map).
    touch_controls& corner_button(std::string action, std::string label) { return add(PONKO_TOUCH_CORNER_BUTTON, std::move(action), std::move(label)); }
    /// When they show.
    touch_controls& show(touch_show when) { show_ = when; return *this; }
    /// Whether they stay over open UI screens.
    touch_controls& over_screens(bool on) { over_screens_ = on; return *this; }
    /// How every control looks (the engine's see-through discs by default).
    touch_controls& style(const touch_style& look) { style_ = look; return *this; }
    /// How the control driving `action` looks, over style().
    touch_controls& control_style(std::string action, const touch_style& look) {
        styles_.push_back({std::move(action), look});
        return *this;
    }
    /// Puts the control driving `action` at `where` instead of where the engine lays it out;
    /// the player can still move it.
    touch_controls& place(std::string action, touch_place where) {
        places_.push_back({std::move(action), where});
        return *this;
    }
    /// The descriptor ctx::set_touch_controls passes, borrowing `list`, `styles` and `places`
    /// (filled here) and this object's strings.
    PonkoTouchControlsDesc desc(std::vector<PonkoTouchControl>& list, std::vector<PonkoTouchControlStyle>& styles,
                                std::vector<PonkoTouchControlPlace>& places) const {
        list = raw();
        styles.clear();
        for (const auto& s : styles_) styles.push_back({s.first.c_str(), s.second});
        places.clear();
        for (const auto& p : places_) places.push_back({p.first.c_str(), p.second});
        PonkoTouchControlsDesc d{};
        d.struct_size = sizeof d;
        d.version = PONKO_TOUCH_CONTROLS_DESC_VERSION;
        detail::check(ponko_touch_controls_desc_defaults(&d));
        d.controls = list.data(); d.count = list.size();
        d.show = static_cast<std::uint32_t>(show_); d.over_screens = over_screens_;
        d.style = style_;
        d.styles = styles.data(); d.style_count = styles.size();
        d.places = places.data(); d.place_count = places.size();
        return d;
    }
    /// The C descriptions, borrowing this object's strings.
    std::vector<PonkoTouchControl> raw() const {
        std::vector<PonkoTouchControl> list;
        list.reserve(entries_.size());
        for (const auto& e : entries_) list.push_back({e.kind, e.action.c_str(), e.label.c_str()});
        return list;
    }
    /// When they show.
    touch_show when() const noexcept { return show_; }
    /// Whether they stay over open UI screens.
    bool over_screens() const noexcept { return over_screens_; }
private:
    struct entry { std::uint32_t kind; std::string action, label; };
    touch_controls& add(std::uint32_t kind, std::string action, std::string label) {
        entries_.push_back({kind, std::move(action), std::move(label)});
        return *this;
    }
    std::vector<entry> entries_;
    touch_show show_ = touch_show::automatic;
    bool over_screens_ = false;
    touch_style style_;
    std::vector<std::pair<std::string, PonkoTouchStyle>> styles_;
    std::vector<std::pair<std::string, PonkoTouchPlace>> places_;
};

/// Manual input for tests and replay (same state as live input).
struct input_event {
    std::uint32_t kind, slot, input;
    float value;
    /// The pointer position (mouse_move) or wheel lines (wheel); unused by the other kinds.
    vec2 point{};
    /// Key `k` going down or up.
    static input_event key(ponko::key k, bool down) { return {0, 0, static_cast<std::uint32_t>(k), down ? 1.0f : 0.0f}; }
    /// Mouse button `b` going down or up.
    static input_event mouse(ponko::mouse b, bool down) { return {1, 0, static_cast<std::uint32_t>(b), down ? 1.0f : 0.0f}; }
    /// Button `b` of gamepad `p` going down or up.
    static input_event button(gamepad p, gamepad_button b, bool down) { return {2, p.slot, static_cast<std::uint32_t>(b), down ? 1.0f : 0.0f}; }
    /// Axis `a` of gamepad `p` moving to `value`.
    static input_event axis(gamepad p, gamepad_axis a, float value) { return {3, p.slot, static_cast<std::uint32_t>(a), value}; }
    /// The window lost focus: every held key and button is released.
    static input_event focus_lost() { return {4, 0, 0, 0}; }
    /// Gamepad `p` was unplugged: its buttons and axes are released.
    static input_event disconnected(gamepad p) { return {5, p.slot, 0, 0}; }
    /// The pointer moved to `at`, in window pixels (Rust `InputEvent::MouseMove`): what a headless click aims at.
    static input_event mouse_move(vec2 at) { return {6, 0, 0, 0, at}; }
    /// The wheel turned by `lines` this frame; positive y is away from the user (Rust `InputEvent::MouseWheel`).
    static input_event wheel(vec2 lines) { return {7, 0, 0, 0, lines}; }
    /// Gamepad `p` was plugged in (generic family), for tests and replays without a backend.
    static input_event connected(gamepad p) { return {8, p.slot, 0, 0}; }
};

/// A named action resolved once (Rust's `ActionId`), from `ctx::action_id`.
/// Queries with it skip the name lookup; `none()` reads false/zero. An id
/// belongs to the game it came from and stays valid when bindings change.
struct action_id {
    std::uint32_t bits = PONKO_ACTION_NONE;
    /// No action: queries with it read false and zero.
    static constexpr action_id none() noexcept { return action_id{}; }
    /// Whether this is none().
    constexpr bool is_none() const noexcept { return bits == PONKO_ACTION_NONE; }
    /// Two ids are equal when they name the same action.
    ///@{
    friend constexpr bool operator==(action_id a, action_id b) noexcept { return a.bits == b.bits; }
    friend constexpr bool operator!=(action_id a, action_id b) noexcept { return a.bits != b.bits; }
    ///@}
};

/// A mixer snapshot resolved once (Rust's `SnapshotId`), from
/// `ctx::audio_snapshot_id`. The snapshot calls that take it skip the name
/// lookup. It belongs to the game it came from and stays valid when the
/// snapshot is replaced.
struct audio_snapshot_id {
    std::uint32_t bits = UINT32_MAX;
    /// Two ids are equal when they name the same snapshot.
    ///@{
    friend constexpr bool operator==(audio_snapshot_id a, audio_snapshot_id b) noexcept { return a.bits == b.bits; }
    friend constexpr bool operator!=(audio_snapshot_id a, audio_snapshot_id b) noexcept { return a.bits != b.bits; }
    ///@}
};

/// What an input scenario checks at a tick (Rust's `ScenarioCheck`).
class scenario_check {
    friend class input_scenario;
    enum class kind { action, resource, count, ui } kind_;
    std::string name_, pointer_, equals_;
    int pressed_ = -1, down_ = -1, released_ = -1;
    std::uint64_t count_ = 0;
    int exists_ = -1, shown_ = -1, focused_ = -1;
    std::optional<std::string> text_;
    explicit scenario_check(kind k, std::string name) : kind_(k), name_(std::move(name)) {}
public:
    /// An action's edges and held state; chain `.pressed(true)`, `.down(true)`, `.released(false)`.
    static scenario_check action(std::string name) { return scenario_check(kind::action, std::move(name)); }
    /// A field of a reflected resource by JSON pointer (`"/points"`), compared as JSON.
    static scenario_check resource(std::string name, std::string pointer, std::string equals_json) {
        scenario_check c(kind::resource, std::move(name));
        c.pointer_ = std::move(pointer); c.equals_ = std::move(equals_json);
        return c;
    }
    /// How many entities have a reflected component.
    static scenario_check count(std::string component, std::uint64_t n) {
        scenario_check c(kind::count, std::move(component));
        c.count_ = n;
        return c;
    }
    /// Expects the action was (or wasn't) pressed on this tick.
    scenario_check& pressed(bool expected) { pressed_ = expected ? 1 : 0; return *this; }
    /// Expects the action is (or isn't) held.
    scenario_check& down(bool expected) { down_ = expected ? 1 : 0; return *this; }
    /// Expects the action was (or wasn't) released on this tick.
    scenario_check& released(bool expected) { released_ = expected ? 1 : 0; return *this; }
    /// A node of the retained UI by name; chain `.exists(true)`,
    /// `.shown(true)`, `.focused(true)`, `.text("Back")`.
    static scenario_check ui(std::string node) { return scenario_check(kind::ui, std::move(node)); }
    /// Expects the UI node to exist (or not).
    scenario_check& exists(bool expected) { exists_ = expected ? 1 : 0; return *this; }
    /// Expects the UI node to be shown (or not): on a screen of the stack, not hidden.
    scenario_check& shown(bool expected) { shown_ = expected ? 1 : 0; return *this; }
    /// Expects the UI node to have the focus (or not).
    scenario_check& focused(bool expected) { focused_ = expected ? 1 : 0; return *this; }
    /// Expects the UI node's text.
    scenario_check& text(std::string expected) { text_ = std::move(expected); return *this; }
};

/// One IME composition step (Rust's `ImeEvent`), for `ctx::feed_ime` and
/// `input_scenario::ime`: make it with `ime_preedit`, `ime_commit`,
/// `ime_enabled` or `ime_disabled`. Its text is read only during the call.
using ime_event = PonkoImeEvent;
/// The composition so far is `text`, its caret at bytes `start..end` of it; an empty `text` cancels it.
inline ime_event ime_preedit(const char* text, std::size_t start, std::size_t end) {
    ime_event e{}; e.kind = PONKO_IME_PREEDIT; e.text = text; e.has_cursor = true; e.cursor_start = start; e.cursor_end = end; return e;
}
/// The composition so far is `text`, with no caret shown; `ime_preedit("")` cancels it.
inline ime_event ime_preedit(const char* text) { ime_event e{}; e.kind = PONKO_IME_PREEDIT; e.text = text; return e; }
/// The composition ended with `text`, which is typed.
inline ime_event ime_commit(const char* text) { ime_event e{}; e.kind = PONKO_IME_COMMIT; e.text = text; return e; }
/// The window system turned IME on.
inline ime_event ime_enabled() { ime_event e{}; e.kind = PONKO_IME_ENABLED; return e; }
/// The window system turned IME off; a composition is dropped.
inline ime_event ime_disabled() { ime_event e{}; e.kind = PONKO_IME_DISABLED; return e; }

/// Raw input and checks scheduled by simulation tick, for reproducible
/// headless tests (Rust's `InputScenario`). Run it with
/// `game::run_headless_scenario`, `game::input_scenario` or
/// `ctx::start_scenario`; each takes a copy.
class input_scenario {
    PonkoInputScenario* raw_;
    input_scenario& step(PonkoStatus s) { detail::check(s); return *this; }
public:
    /// An empty scenario: no steps, no checks.
    input_scenario() : raw_(ponko_input_scenario_new()) {}
    ~input_scenario() { ponko_input_scenario_free(raw_); }
    /// Not copyable: it owns its steps. to_json and from_json make a copy.
    ///@{
    input_scenario(const input_scenario&) = delete;
    input_scenario& operator=(const input_scenario&) = delete;
    ///@}
    /// Takes `other`'s steps; `other` is left empty.
    input_scenario(input_scenario&& other) noexcept : raw_(std::exchange(other.raw_, nullptr)) {}
    /// Frees this scenario's steps and takes `other`'s.
    input_scenario& operator=(input_scenario&& other) noexcept {
        if (this != &other) { ponko_input_scenario_free(raw_); raw_ = std::exchange(other.raw_, nullptr); }
        return *this;
    }
    /// Reads the versioned JSON format that Rust and tools use.
    static input_scenario from_json(const std::string& json) {
        PonkoInputScenario* raw = nullptr;
        detail::check(ponko_input_scenario_from_json(json.c_str(), &raw));
        input_scenario s; ponko_input_scenario_free(s.raw_); s.raw_ = raw;
        return s;
    }
    /// The scenario in the versioned JSON format that from_json, Rust and tools read.
    std::string to_json() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_input_scenario_to_json(raw_, b, c, l); });
    }
    /// Feeds `e` before the simulation systems of tick `tick` run.
    input_scenario& at(std::uint64_t tick, input_event e) {
        switch (e.kind) {
        case 6: return step(ponko_input_scenario_mouse_move(raw_, tick, e.point));
        case 7: return step(ponko_input_scenario_mouse_wheel(raw_, tick, e.point));
        default: return step(ponko_input_scenario_at(raw_, tick, e.kind, e.slot, e.input, e.value));
        }
    }
    /// Feeds a finger's event (as ctx::feed_touch) before the simulation systems of tick `tick` run.
    input_scenario& touch(std::uint64_t tick, std::uint64_t id, touch_phase phase, vec2 at) {
        return step(ponko_input_scenario_touch(raw_, tick, id, static_cast<std::uint32_t>(phase), at));
    }
    /// Feeds a trackpad gesture (as ctx::feed_trackpad) before the simulation systems of tick `tick` run.
    input_scenario& trackpad(std::uint64_t tick, gesture g, touch_phase phase, float amount) {
        return step(ponko_input_scenario_trackpad(raw_, tick, static_cast<std::uint32_t>(g), static_cast<std::uint32_t>(phase), amount));
    }
    /// A tap at `at` on tick `tick`: a finger down and up within the tick (a tap gesture, or a press of the
    /// on-screen button under it). Each finger gets an id of its own.
    input_scenario& tap(std::uint64_t tick, vec2 at) { return step(ponko_input_scenario_tap(raw_, tick, at)); }
    /// A finger held still at `at` from tick `from` until it lifts on tick `to`.
    input_scenario& touch_hold(std::uint64_t from, std::uint64_t to, vec2 at) {
        return step(ponko_input_scenario_touch_hold(raw_, from, to, at));
    }
    /// A finger down at `start` on tick `from`, moving in a straight line each tick, lifting at `end` on
    /// tick `to`: a drag, a swipe, or an on-screen stick pushed and let go.
    input_scenario& touch_drag(std::uint64_t from, std::uint64_t to, vec2 start, vec2 end) {
        return step(ponko_input_scenario_touch_drag(raw_, from, to, start, end));
    }
    /// Feeds the window's safe area insets on tick `tick`, as a phone reports them (as ctx::window_feed_safe_insets),
    /// before that tick's simulation systems run: a layout for a notched phone is checked without a device. They stay
    /// until another step changes them.
    input_scenario& safe_insets(std::uint64_t tick, ponko::safe_insets insets) {
        return step(ponko_input_scenario_safe_insets(raw_, tick, insets));
    }
    /// Checks `c` after the simulation systems of tick `tick` ran.
    input_scenario& expect(std::uint64_t tick, const scenario_check& c) {
        switch (c.kind_) {
        case scenario_check::kind::action:
            return step(ponko_input_scenario_expect_action(raw_, tick, c.name_.c_str(), c.pressed_, c.down_, c.released_));
        case scenario_check::kind::resource:
            return step(ponko_input_scenario_expect_resource(raw_, tick, c.name_.c_str(), c.pointer_.c_str(), c.equals_.c_str()));
        case scenario_check::kind::count:
            return step(ponko_input_scenario_expect_count(raw_, tick, c.name_.c_str(), c.count_));
        case scenario_check::kind::ui:
            return step(ponko_input_scenario_expect_ui(raw_, tick, c.name_.c_str(), c.exists_, c.shown_, c.focused_,
                                                       c.text_ ? c.text_->c_str() : nullptr));
        }
        throw std::logic_error("unknown scenario check");
    }
    /// Clicks the retained UI's node named `node` in the frame of tick
    /// `tick` (needs `game::headless_frames` or a window); the run fails
    /// there if it can't be clicked.
    input_scenario& click(std::uint64_t tick, const char* node) { return step(ponko_input_scenario_click(raw_, tick, node)); }
    /// Clicks the text input named `node` and types `typed` at its end, in the frame of tick `tick`.
    input_scenario& type_text(std::uint64_t tick, const char* node, const char* typed) {
        return step(ponko_input_scenario_type_text(raw_, tick, node, typed));
    }
    /// Feeds the IME step `e` on tick `tick`, after its input steps: replays a composition into the focused text input.
    input_scenario& ime(std::uint64_t tick, const ime_event& e) { return step(ponko_input_scenario_ime(raw_, tick, &e)); }
    /// Names the scenario in its report.
    input_scenario& named(const char* id) { return step(ponko_input_scenario_named(raw_, id)); }
    /// Lasts `ticks` ticks (default: through the last step's tick).
    input_scenario& duration(std::uint64_t ticks) { return step(ponko_input_scenario_duration(raw_, ticks)); }
    /// Releases inputs still held when the run finishes normally.
    input_scenario& release_when_done() { return step(ponko_input_scenario_release_when_done(raw_)); }
    /// How many ticks a run lasts.
    std::uint64_t ticks() const { std::uint64_t t = 0; detail::check(ponko_input_scenario_info(raw_, &t, nullptr)); return t; }
    /// How many steps (inputs and checks) it holds.
    std::size_t step_count() const { std::size_t n = 0; detail::check(ponko_input_scenario_info(raw_, nullptr, &n)); return n; }
    /// The C scenario, for the C API. Owned by this object.
    const PonkoInputScenario* raw() const noexcept { return raw_; }
private:
    friend class ctx;
    explicit input_scenario(PonkoInputScenario* raw) noexcept : raw_(raw) {}
};

/// Files in memory, to add with `ctx::add_asset_source` (Rust's
/// `MemorySource`): assets compiled into the program, generated at startup,
/// or made up in a test. The game gets a copy.
class memory_source {
    PonkoMemorySource* raw_;
public:
    /// An empty source named `id` (the `id:` prefix of its paths). Throws ponko::error for an empty or
    /// invalid id.
    explicit memory_source(const char* id) : raw_(ponko_memory_source_new(id)) {
        if (!raw_) throw detail::last_error();
    }
    ~memory_source() { ponko_memory_source_free(raw_); }
    /// Not copyable: it owns its files.
    ///@{
    memory_source(const memory_source&) = delete;
    memory_source& operator=(const memory_source&) = delete;
    ///@}
    /// Takes `other`'s files; `other` is left empty.
    memory_source(memory_source&& other) noexcept : raw_(std::exchange(other.raw_, nullptr)) {}
    /// Frees this source's files and takes `other`'s.
    memory_source& operator=(memory_source&& other) noexcept {
        if (this != &other) { ponko_memory_source_free(raw_); raw_ = std::exchange(other.raw_, nullptr); }
        return *this;
    }
    /// Adds (or replaces) file `name` with a copy of `len` bytes at `data`.
    memory_source& insert(const char* name, const void* data, std::size_t len) {
        detail::check(ponko_memory_source_insert(raw_, name, static_cast<const std::uint8_t*>(data), len));
        return *this;
    }
    /// Adds (or replaces) file `name` with a copy of `bytes`, builder style.
    memory_source& with(const char* name, const std::string& bytes) { return insert(name, bytes.data(), bytes.size()); }
    /// Adds (or replaces) file `name` with a copy of `bytes`, builder style.
    memory_source& with(const char* name, const std::vector<std::uint8_t>& bytes) { return insert(name, bytes.data(), bytes.size()); }
    /// Bytes written out: `with("blob.bin", {1, 2, 3})` (without this, the braces
    /// could make either a string or a vector).
    memory_source& with(const char* name, std::initializer_list<std::uint8_t> bytes) { return insert(name, bytes.begin(), bytes.size()); }
    /// The C source, for the C API. Owned by this object.
    const PonkoMemorySource* raw() const noexcept { return raw_; }
};

/// A set of assets to load together in the background (Rust `AssetGroup`):
/// add assets, then start it with `ctx::start_asset_group`, which takes it.
/// Methods throw ponko::error on bad arguments.
class asset_group {
    PonkoAssetGroup* raw_;
    bool owned_;
    asset_group& add(PonkoAssetGroupKind kind, const char* path) {
        detail::check(ponko_asset_group_add(raw_, static_cast<std::uint32_t>(kind), path));
        return *this;
    }
public:
    /// An empty group called `name` (for `ctx::find_asset_group`).
    explicit asset_group(const char* name) : raw_(ponko_asset_group_new(name)), owned_(true) {
        if (!raw_) throw detail::last_error();
    }
    /// A group the engine owns, to fill (a loading state's builder gets one).
    static asset_group borrowed(PonkoAssetGroup* raw) noexcept { return asset_group(raw, false); }
    ~asset_group() { if (owned_) ponko_asset_group_free(raw_); }
    /// Not copyable: it owns its list.
    ///@{
    asset_group(const asset_group&) = delete;
    asset_group& operator=(const asset_group&) = delete;
    ///@}
    /// Takes `other`'s list; `other` is left empty.
    asset_group(asset_group&& other) noexcept : raw_(std::exchange(other.raw_, nullptr)), owned_(other.owned_) {}
    /// Frees this group and takes `other`'s list.
    asset_group& operator=(asset_group&& other) noexcept {
        if (this != &other) {
            if (owned_) ponko_asset_group_free(raw_);
            raw_ = std::exchange(other.raw_, nullptr); owned_ = other.owned_;
        }
        return *this;
    }
    /// A texture as `ctx::load_texture` loads it, which then returns it.
    asset_group& texture(const char* path) { return add(PONKO_GROUP_TEXTURE, path); }
    /// A texture streamed under the texture residency budget (`ctx::stream_texture`).
    asset_group& streamed_texture(const char* path) { return add(PONKO_GROUP_STREAMED_TEXTURE, path); }
    /// A mesh file streamed under the mesh residency budget (`ctx::load_mesh` then returns it).
    asset_group& mesh(const char* path) { return add(PONKO_GROUP_MESH, path); }
    /// A sound decoded on a worker (`ctx::load_sound` then returns it).
    asset_group& sound(const char* path) { return add(PONKO_GROUP_SOUND, path); }
    /// The file's bytes, for your own decoder (`ctx::take_asset_group_bytes`).
    asset_group& bytes(const char* path) { return add(PONKO_GROUP_BYTES, path); }
    /// A file of your own decoded a piece at a time (Rust `AssetGroup::with_sliced`): `start(data,
    /// len)` makes a loader `L` from the file's bytes, and its `bool step()` does the next bounded
    /// piece, true once it is all done (throw to fail the group). Without worker threads each frame
    /// steps it within its budget; with workers it runs on one, so touch only the loader. Take it with
    /// `ctx::take_asset_group_sliced<L>`.
    template<class F> asset_group& sliced(const char* path, F start) {
        using L = std::decay_t<decltype(start(std::declval<const std::uint8_t*>(), std::size_t{}))>;
        struct holder { F start; };
        PonkoSliceLoader loader{};
        loader.struct_size = sizeof loader;
        loader.version = PONKO_SLICE_LOADER_VERSION;
        loader.start = [](void* user, const std::uint8_t* data, std::size_t len, void** out) noexcept -> std::uint32_t {
            try { *out = new L(static_cast<holder*>(user)->start(data, len)); return PONKO_OK; }
            catch (...) { return PONKO_INVALID_ARGUMENT; }
        };
        loader.step = [](void*, void* state, bool* done) noexcept -> std::uint32_t {
            try { *done = static_cast<L*>(state)->step(); return PONKO_OK; }
            catch (...) { return PONKO_INVALID_ARGUMENT; }
        };
        loader.free_state = [](void*, void* state) noexcept { delete static_cast<L*>(state); };
        auto* h = new holder{std::move(start)};
        loader.user = h;
        loader.free_user = [](void* user) noexcept { delete static_cast<holder*>(user); };
        const PonkoStatus status = ponko_asset_group_add_sliced(raw_, path, &loader);
        if (status != PONKO_OK) { delete h; detail::check(status); }
        return *this;
    }
    /// Decodes a file in a single call, on a worker when available. The callable
    /// receives bytes valid for that call and returns the value by type. It must
    /// not access the game context. Take the result with ctx::take_asset_group_decoded.
    template<class F> asset_group& decoded(const char* path, F decode) {
        using T = std::decay_t<decltype(decode(std::declval<const std::uint8_t*>(), std::size_t{}))>;
        auto callback = std::make_unique<F>(std::move(decode));
        PonkoSliceLoader loader{};
        loader.struct_size = sizeof loader;
        loader.version = PONKO_SLICE_LOADER_VERSION;
        loader.user = callback.get();
        loader.start = [](void* user, const std::uint8_t* data, std::size_t len, void** out) noexcept -> std::uint32_t {
            try { *out = new T((*static_cast<F*>(user))(data, len)); return PONKO_OK; }
            catch (...) { return PONKO_INVALID_ARGUMENT; }
        };
        loader.free_state = [](void*, void* state) noexcept { delete static_cast<T*>(state); };
        loader.free_user = [](void* user) noexcept { delete static_cast<F*>(user); };
        detail::check(ponko_asset_group_add_decoder(raw_, path, &loader));
        callback.release();
        return *this;
    }
    /// Gives up ownership of the C group (to start it). Null afterwards.
    PonkoAssetGroup* release() noexcept { return std::exchange(raw_, nullptr); }
private:
    asset_group(PonkoAssetGroup* raw, bool owned) noexcept : raw_(raw), owned_(owned) {}
};

/// One contact point of a pair: where, and how far apart the bodies are there.
struct contact_point3d {
    vec3 position;
    float distance;
};
/// The contacts between two bodies in one step, as a contact hook sees them
/// (Rust `ContactPair3D`). Only valid inside the hook call; changes last for
/// this step. Calls throw on bad arguments (a non-finite value).
class contact_pair3d {
public:
    /// Wraps a C pair; the contact hook makes these for you.
    explicit contact_pair3d(PonkoContactPair3D* raw) noexcept : raw_(raw) {}
    /// Not copyable: it stands for the engine's pair during one hook call.
    ///@{
    contact_pair3d(const contact_pair3d&) = delete;
    contact_pair3d& operator=(const contact_pair3d&) = delete;
    ///@}
    /// The two bodies (Rust `a` and `b`).
    entity a() const { return both().first; }
    /// The second body.
    entity b() const { return both().second; }
    /// The contact direction in the world, from `a` toward `b`.
    vec3 normal() const { vec3 n{}; detail::check(ponko_contact_pair3d_normal(raw_, &n)); return n; }
    /// How many contact points there are (0 once ignored).
    std::size_t point_count() const { std::size_t n = 0; detail::check(ponko_contact_pair3d_point_count(raw_, &n)); return n; }
    /// Point `i`, or nothing if there is no such point.
    std::optional<contact_point3d> point(std::size_t i) const {
        contact_point3d p{};
        auto status = ponko_contact_pair3d_point(raw_, i, &p.position, &p.distance);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return p;
    }
    /// Drops every contact: the bodies pass through each other this step.
    void ignore() { detail::check(ponko_contact_pair3d_ignore(raw_)); }
    /// Drops point `i` (later points may change places); past the end does nothing.
    void remove_point(std::size_t i) { detail::check(ponko_contact_pair3d_remove_point(raw_, i)); }
    /// The pair's friction coefficient.
    float friction() const { float f = 0; detail::check(ponko_contact_pair3d_friction(raw_, &f)); return f; }
    /// Sets the pair's friction coefficient for this step (0 or more).
    void set_friction(float friction) { detail::check(ponko_contact_pair3d_set_friction(raw_, friction)); }
    /// The pair's restitution (bounciness, 0 to 1).
    float restitution() const { float r = 0; detail::check(ponko_contact_pair3d_restitution(raw_, &r)); return r; }
    /// Sets the pair's restitution for this step (0 to 1).
    void set_restitution(float restitution) { detail::check(ponko_contact_pair3d_set_restitution(raw_, restitution)); }
    /// Makes `b` slide along `a`'s surface at `velocity` (world, m/s).
    void set_surface_velocity(vec3 velocity) { detail::check(ponko_contact_pair3d_set_surface_velocity(raw_, velocity)); }
    /// The C pair, for the C API.
    PonkoContactPair3D* raw() const noexcept { return raw_; }
private:
    std::pair<entity, entity> both() const {
        entity a = 0, b = 0;
        detail::check(ponko_contact_pair3d_entities(raw_, &a, &b));
        return {a, b};
    }
    PonkoContactPair3D* raw_;
};
/// A part of the window, in fractions of its size (Rust `ponko::camera::Viewport`): `{0, 0, 1, 1}` is the
/// whole window, `{0.5, 0, 0.5, 1}` its right half.
using viewport = PonkoViewport;
/// Player `index`'s part of a split screen for `players` players: the halves for two, the quarters for three
/// and four (left to right, top to bottom), equal columns for more.
inline viewport split_viewport(std::uint32_t players, std::uint32_t index) { return ponko_viewport_split(players, index); }
/// Render layers (Rust `RenderLayers`), one bit each: what an entity without layers is on (layer 0).
inline constexpr std::uint32_t render_layers_default = PONKO_RENDER_LAYERS_DEFAULT;
/// Every render layer: what a camera draws unless told otherwise.
inline constexpr std::uint32_t render_layers_all = PONKO_RENDER_LAYERS_ALL;
/// Render layer `n` alone (0 to 31).
inline constexpr std::uint32_t render_layer(std::uint32_t n) { return 1u << (n % 32u); }
/// A 2D camera and where it draws (Rust `d2::View`): its camera, rectangle, layers and clear. Start from
/// ctx::camera_view (the main camera) or the defaults, change it, then ctx::set_view2d or set_camera_view.
struct view2d : PonkoView2D {
    /// The defaults: a camera at 0, 0, zoom 1, the whole window, every layer, no clear.
    view2d() : PonkoView2D{} {
        struct_size = sizeof(PonkoView2D);
        version = PONKO_VIEW2D_VERSION;
        detail::check(ponko_view2d_defaults(this));
    }
    /// Fills its rectangle with `c` before drawing (views only).
    view2d& with_clear(color c) { has_clear = true; clear = c; return *this; }
};
/// A 3D camera and where it draws (Rust `d3::View`): its camera, rectangle, layers, clear and post settings.
/// Each view is a whole frame of its camera at its rectangle's size; the sun's shadow maps are shared.
struct view3d : PonkoView3D {
    /// The defaults: the default camera, the whole window, every layer, the game's clear and post settings.
    view3d() : PonkoView3D{} {
        struct_size = sizeof(PonkoView3D);
        version = PONKO_VIEW3D_VERSION;
        detail::check(ponko_view3d_defaults(this));
    }
    /// Its own post-processing (views only; MSAA is the game's).
    view3d& with_post(const PonkoPost& p) { has_post = true; post = p; return *this; }
    /// The color behind a flat sky, instead of the game's (views only).
    view3d& with_clear(color c) { has_clear = true; clear = c; return *this; }
};
/// Debug drawing (Rust `ctx.debug`): what ctx::debug() returns. Lines,
/// shapes and labels in 2D (vec2: world units, or window pixels with
/// style.screen) and 3D (vec3), kept until the system that drew them runs
/// again, or style.seconds. Off, every call returns at once.
using debug_style = PonkoDebugStyle;
/// Versioned style for 2D debug drawing in a named coordinate space.
using debug_style_ex = PonkoDebugStyleEx;
/// A built-in view of debug_draw::show (PONKO_DEBUG_VIEW_*).
using debug_view = PonkoDebugView;
/// Lines and labels drawn now.
using debug_stats = PonkoDebugStats;
/// Returns `style` with its 2D points in `space`, every other setting as it
/// was: the draw_space sprite draws take (Rust `DebugShape::space`).
inline debug_style with_space(debug_style style, draw_space space) {
    return ponko_debug_style_with_space(style, space == draw_space::screen ? PONKO_DRAW_SPACE_SCREEN : PONKO_DRAW_SPACE_WORLD);
}

/// Debug drawing service borrowed from a context.
class debug_draw {
public:
    /// Wraps a callback's context.
    explicit debug_draw(PonkoCtx* raw) noexcept : raw_(raw) {}
    /// The default style: until the system runs again, 1 px wide, labels 14 px, depth-tested, in the world.
    static debug_style style() noexcept { return ponko_debug_style_default(); }
    /// Initializes the versioned 2D style in the given coordinate space.
    static debug_style_ex style_ex(draw_space space = draw_space::world) {
        debug_style_ex s{};
        s.struct_size = sizeof s;
        s.version = PONKO_DEBUG_STYLE_EX_VERSION;
        detail::check(ponko_debug_style_ex_defaults(&s));
        s.space = space == draw_space::screen ? PONKO_DRAW_SPACE_SCREEN : PONKO_DRAW_SPACE_WORLD;
        return s;
    }
    /// Turns drawing on or off (off forgets everything drawn).
    void set_enabled(bool on) { detail::check(ponko_ctx_debug_set_enabled(raw_, on)); }
    /// Whether drawing is on.
    bool enabled() const { bool on = false; detail::check(ponko_ctx_debug_enabled(raw_, &on)); return on; }
    /// Forgets everything drawn, timed shapes included.
    void clear() { detail::check(ponko_ctx_debug_clear(raw_)); }
    /// Turns a built-in view on or off.
    void show(debug_view view, bool on) { detail::check(ponko_ctx_debug_show(raw_, view, on)); }
    /// Whether a built-in view is on.
    bool showing(debug_view view) const { bool on = false; detail::check(ponko_ctx_debug_showing(raw_, view, &on)); return on; }
    /// Whether a named toggle is active while debug drawing is enabled.
    bool toggled(const char* name) const {
        bool on = false;
        detail::check(ponko_ctx_debug_toggled(raw_, name, &on));
        return on;
    }
    /// Sets the stored value of a named debug toggle.
    void set_toggle(const char* name, bool on) { detail::check(ponko_ctx_debug_set_toggle(raw_, name, on)); }
    /// Lines and labels drawn now (the built-in views not counted).
    debug_stats stats() const { debug_stats s{}; detail::check(ponko_ctx_debug_stats(raw_, &s)); return s; }

    /// A line from a to b.
    void line(vec2 a, vec2 b, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_line2d(raw_, a, b, c, &s)); }
    /// A line from a to b.
    void line(vec3 a, vec3 b, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_line3d(raw_, a, b, c, &s)); }
    /// A line from origin along direction.
    void ray(vec2 origin, vec2 direction, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_ray2d(raw_, origin, direction, c, &s)); }
    /// A line from origin along direction.
    void ray(vec3 origin, vec3 direction, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_ray3d(raw_, origin, direction, c, &s)); }
    /// An arrow from from to to, its head at to.
    void arrow(vec2 from, vec2 to, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_arrow2d(raw_, from, to, c, &s)); }
    /// An arrow from from to to, its head at to.
    void arrow(vec3 from, vec3 to, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_arrow3d(raw_, from, to, c, &s)); }
    /// A rectangle of size around center.
    void box(vec2 center, vec2 size, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_box2d(raw_, center, size, c, &s)); }
    /// A box of size around center, turned by rotation.
    void box(vec3 center, vec3 size, quat rotation, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_box3d(raw_, center, size, rotation, c, &s)); }
    /// A circle.
    void circle(vec2 center, float radius, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_circle2d(raw_, center, radius, c, &s)); }
    /// A sphere: a ring around each axis.
    void sphere(vec3 center, float radius, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_sphere3d(raw_, center, radius, c, &s)); }
    /// The points within radius of the segment from a to b.
    void capsule(vec2 a, vec2 b, float radius, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_capsule2d(raw_, a, b, radius, c, &s)); }
    /// The points within radius of the segment from a to b.
    void capsule(vec3 a, vec3 b, float radius, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_capsule3d(raw_, a, b, radius, c, &s)); }
    /// Lines through count points, back to the first when closed.
    void polyline(const vec2* points, std::size_t count, bool closed, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_polyline2d(raw_, points, count, closed, c, &s)); }
    /// Lines through count points, back to the first when closed.
    void polyline(const vec3* points, std::size_t count, bool closed, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_polyline3d(raw_, points, count, closed, c, &s)); }
    /// A grid of cells by cells squares of side cell around center (on the ground in 3D).
    void grid(vec2 center, float cell, std::uint32_t cells, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_grid2d(raw_, center, cell, cells, c, &s)); }
    /// A grid of cells by cells squares of side cell around center (on the ground in 3D).
    void grid(vec3 center, float cell, std::uint32_t cells, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_grid3d(raw_, center, cell, cells, c, &s)); }
    /// What the scene's camera sees.
    void frustum(color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_frustum3d(raw_, nullptr, c, &s)); }
    /// What camera sees.
    void frustum(const camera3d& camera, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_frustum3d(raw_, &camera, c, &s)); }
    /// A label whose top-left corner is at a point.
    void text(vec2 at, const char* label, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_text2d(raw_, at, label, c, &s)); }
    /// A label whose top-left corner is where a 3D point is on the screen.
    void text(vec3 at, const char* label, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_text3d(raw_, at, label, c, &s)); }
    /// A 3D label hidden behind nearer surfaces, facing the camera, its top-left corner at a point (Rust `DebugShape::depth_tested`).
    void text_depth_tested(vec3 at, const char* label, color c, const debug_style& s = style()) { detail::check(ponko_ctx_debug_text3d_depth_tested(raw_, at, label, c, &s)); }
    /// How far from the 3D camera the colliders view draws mesh and terrain triangles (default 50).
    void set_collider_distance(float distance) { detail::check(ponko_ctx_debug_set_collider_distance(raw_, distance)); }
    /// See set_collider_distance.
    float collider_distance() const { float d = 0; detail::check(ponko_ctx_debug_collider_distance(raw_, &d)); return d; }
    /// The axes of a transform: x red, y green, z blue.
    void axes(vec3 at, quat rotation, float length, const debug_style& s = style()) { detail::check(ponko_ctx_debug_axes3d(raw_, at, rotation, length, &s)); }

    /// Draws a 2D line with the versioned style.
    void line_ex(vec2 a, vec2 b, color c, const debug_style_ex& s) {
        detail::check(ponko_ctx_debug_line2d_ex(raw_, a, b, c, &s));
    }

    /// Draws a 2D ray with the versioned style.
    void ray_ex(vec2 origin, vec2 direction, color c, const debug_style_ex& s) {
        detail::check(ponko_ctx_debug_ray2d_ex(raw_, origin, direction, c, &s));
    }

    /// Draws a 2D arrow with the versioned style.
    void arrow_ex(vec2 from, vec2 to, color c, const debug_style_ex& s) {
        detail::check(ponko_ctx_debug_arrow2d_ex(raw_, from, to, c, &s));
    }

    /// Draws a 2D box with the versioned style.
    void box_ex(vec2 center, vec2 size, color c, const debug_style_ex& s) {
        detail::check(ponko_ctx_debug_box2d_ex(raw_, center, size, c, &s));
    }

    /// Draws a 2D circle with the versioned style.
    void circle_ex(vec2 center, float radius, color c, const debug_style_ex& s) {
        detail::check(ponko_ctx_debug_circle2d_ex(raw_, center, radius, c, &s));
    }

    /// Draws a 2D capsule with the versioned style.
    void capsule_ex(vec2 a, vec2 b, float radius, color c, const debug_style_ex& s) {
        detail::check(ponko_ctx_debug_capsule2d_ex(raw_, a, b, radius, c, &s));
    }

    /// Draws a 2D polyline with the versioned style.
    void polyline_ex(const vec2* points, std::size_t count, bool closed, color c, const debug_style_ex& s) {
        detail::check(ponko_ctx_debug_polyline2d_ex(raw_, points, count, closed, c, &s));
    }

    /// Draws a 2D grid with the versioned style.
    void grid_ex(vec2 center, float cell, std::uint32_t cells, color c, const debug_style_ex& s) {
        detail::check(ponko_ctx_debug_grid2d_ex(raw_, center, cell, cells, c, &s));
    }

    /// Draws a 2D text with the versioned style.
    void text_ex(vec2 at, const char* label, color c, const debug_style_ex& s) {
        detail::check(ponko_ctx_debug_text2d_ex(raw_, at, label, c, &s));
    }

private:
    PonkoCtx* raw_;
};

/// How the window is shown (Rust `ponko::window::WindowMode`'s kinds).
enum class window_mode_kind : std::uint32_t {
    windowed = PONKO_WINDOW_MODE_WINDOWED,      ///< An ordinary desktop window.
    borderless = PONKO_WINDOW_MODE_BORDERLESS,  ///< Borderless fullscreen on a monitor.
    exclusive = PONKO_WINDOW_MODE_EXCLUSIVE,    ///< Exclusive fullscreen in an advertised video mode.
};
/// Cursor capture (Rust `CursorGrab`).
enum class cursor_grab : std::uint32_t {
    none = PONKO_CURSOR_GRAB_NONE,          ///< No capture.
    confined = PONKO_CURSOR_GRAB_CONFINED,  ///< Kept inside the window.
    locked = PONKO_CURSOR_GRAB_LOCKED,      ///< Locked for relative motion (confined where the OS needs it).
};
/// System cursors (Rust `CursorIcon`), the CSS cursor set. `default_` has an underscore because
/// `default` is a C++ keyword.
enum class cursor_icon : std::uint32_t {
    /// The system cursors, in the order of `PonkoCursorIcon`.
    ///@{
    default_ = PONKO_CURSOR_DEFAULT,
    context_menu = PONKO_CURSOR_CONTEXT_MENU,
    help = PONKO_CURSOR_HELP,
    pointer = PONKO_CURSOR_POINTER,
    progress = PONKO_CURSOR_PROGRESS,
    wait = PONKO_CURSOR_WAIT,
    cell = PONKO_CURSOR_CELL,
    crosshair = PONKO_CURSOR_CROSSHAIR,
    text = PONKO_CURSOR_TEXT,
    vertical_text = PONKO_CURSOR_VERTICAL_TEXT,
    alias = PONKO_CURSOR_ALIAS,
    copy = PONKO_CURSOR_COPY,
    move = PONKO_CURSOR_MOVE,
    no_drop = PONKO_CURSOR_NO_DROP,
    not_allowed = PONKO_CURSOR_NOT_ALLOWED,
    grab = PONKO_CURSOR_GRAB,
    grabbing = PONKO_CURSOR_GRABBING,
    e_resize = PONKO_CURSOR_E_RESIZE,
    n_resize = PONKO_CURSOR_N_RESIZE,
    ne_resize = PONKO_CURSOR_NE_RESIZE,
    nw_resize = PONKO_CURSOR_NW_RESIZE,
    s_resize = PONKO_CURSOR_S_RESIZE,
    se_resize = PONKO_CURSOR_SE_RESIZE,
    sw_resize = PONKO_CURSOR_SW_RESIZE,
    w_resize = PONKO_CURSOR_W_RESIZE,
    ew_resize = PONKO_CURSOR_EW_RESIZE,
    ns_resize = PONKO_CURSOR_NS_RESIZE,
    nesw_resize = PONKO_CURSOR_NESW_RESIZE,
    nwse_resize = PONKO_CURSOR_NWSE_RESIZE,
    col_resize = PONKO_CURSOR_COL_RESIZE,
    row_resize = PONKO_CURSOR_ROW_RESIZE,
    all_scroll = PONKO_CURSOR_ALL_SCROLL,
    zoom_in = PONKO_CURSOR_ZOOM_IN,
    zoom_out = PONKO_CURSOR_ZOOM_OUT,
    dnd_ask = PONKO_CURSOR_DND_ASK,
    all_resize = PONKO_CURSOR_ALL_RESIZE,
    ///@}
};
/// An exclusive fullscreen mode: physical pixels, refresh in millihertz, bit depth.
using window_video_mode = PonkoWindowVideoModeEx;
/// The requested logical size, limits, physical position and desktop flags.
using window_properties = PonkoWindowPropertiesEx;
/// What the OS reports: physical size, DPI scale, focus, minimized and the current monitor.
using window_state = PonkoWindowStateEx;
/// A monitor's physical origin and size, DPI scale and refresh (the name is read separately).
using window_monitor = PonkoWindowMonitorEx;
/// How the window is shown (Rust `WindowMode`). Make one with windowed(), borderless() or exclusive().
struct window_mode {
    /// Which kind of mode.
    window_mode_kind kind = window_mode_kind::windowed;
    /// The monitor index; unset for borderless means the current monitor.
    std::optional<std::uint32_t> monitor;
    /// The advertised mode, for exclusive only (pick one from ctx::window_video_modes).
    window_video_mode video{};
    /// An ordinary desktop window.
    static window_mode windowed() { return {}; }
    /// Borderless fullscreen on `monitor`, or on the current monitor if unset.
    static window_mode borderless(std::optional<std::uint32_t> monitor = {}) {
        window_mode m;
        m.kind = window_mode_kind::borderless;
        m.monitor = monitor;
        return m;
    }
    /// Exclusive fullscreen in `video`, one of the modes `monitor` advertises.
    static window_mode exclusive(std::uint32_t monitor, const window_video_mode& video) {
        window_mode m;
        m.kind = window_mode_kind::exclusive;
        m.monitor = monitor;
        m.video = video;
        return m;
    }
};
/// RGBA8 pixels for a window icon or a custom cursor (copied by the engine).
struct window_image {
    /// Width in image pixels.
    std::uint32_t width = 0;
    /// Height in image pixels.
    std::uint32_t height = 0;
    /// Row-major RGBA8 bytes, width * height * 4 of them.
    std::vector<std::uint8_t> rgba;
    /// Cursor hotspot X in image pixels; ignored for an icon.
    std::uint32_t hotspot_x = 0;
    /// Cursor hotspot Y in image pixels; ignored for an icon.
    std::uint32_t hotspot_y = 0;
};
/// What a window event reports (Rust `WindowEvent`).
enum class window_event_kind : std::uint32_t {
    focused = PONKO_WINDOW_EVENT_FOCUSED,                            ///< Focus changed (`value`).
    minimized = PONKO_WINDOW_EVENT_MINIMIZED,                        ///< Minimized state changed (`value`).
    close_requested = PONKO_WINDOW_EVENT_CLOSE_REQUESTED,            ///< The OS asked to close the window.
    file_dropped = PONKO_WINDOW_EVENT_FILE_DROPPED,                  ///< A file was dropped (`path`).
    scale_factor_changed = PONKO_WINDOW_EVENT_SCALE_FACTOR_CHANGED,  ///< The DPI scale changed (`scale_factor`).
};
/// One window event of this frame (Rust `WindowEvent`).
struct window_event {
    /// What happened.
    window_event_kind kind{};
    /// The new focus or minimized state, for those kinds.
    bool value = false;
    /// The new DPI scale, for scale_factor_changed.
    float scale_factor = 1.0;
    /// The dropped file's path, for file_dropped.
    std::string path;
};
namespace detail {
inline PonkoWindowImageEx window_image_desc(const window_image& image) {
    PonkoWindowImageEx d{};
    d.struct_size = sizeof d;
    d.version = PONKO_WINDOW_IMAGE_EX_VERSION;
    check(ponko_window_image_ex_defaults(&d));
    d.width = image.width;
    d.height = image.height;
    d.rgba = image.rgba.data();
    d.rgba_len = image.rgba.size();
    d.hotspot_x = image.hotspot_x;
    d.hotspot_y = image.hotspot_y;
    return d;
}
template<class T> T window_desc(std::uint32_t version, PonkoStatus (*defaults)(T*)) {
    T d{};
    d.struct_size = sizeof d;
    d.version = version;
    check(defaults(&d));
    return d;
}
} // namespace detail

/// A read-only view of the 3D navmesh (PonkoNavView), from ctx::nav_view: its reads touch nothing else, so
/// they work inside world.each and world.query loops and from the _par versions, where the ctx's own navmesh
/// calls throw. Valid until the system returns, and only until the navmesh is baked again.
class nav_view {
    PonkoNavView view_{};
public:
    /// Wraps a C view.
    explicit nav_view(PonkoNavView v) noexcept : view_(v) {}
    /// The nearest walkable point within `max_distance` of `point`, or std::nullopt.
    std::optional<vec3> sample_position(vec3 point, float max_distance) const {
        vec3 p{}; bool found = false;
        detail::check(ponko_nav_view_sample_position(&view_, point, max_distance, &p, &found));
        if (!found) return std::nullopt;
        return p;
    }
    /// Snaps every point of `points` in place in one call; `found[i]` says whether point i had a walkable
    /// point within `max_distance` (if not, it is left as it was).
    void sample_positions(std::vector<vec3>& points, float max_distance, std::vector<bool>& found) const {
        std::unique_ptr<bool[]> flags(new bool[points.size() + 1]);
        detail::check(ponko_nav_view_sample_positions(&view_, points.data(), points.size(), max_distance,
                                                      points.data(), flags.get()));
        found.assign(flags.get(), flags.get() + points.size());
    }
    /// A path's corners from `from` to `to` into `out` (reusing its capacity), start and end included;
    /// empty when there is none.
    void find_path(vec3 from, vec3 to, std::vector<vec3>& out) const {
        std::size_t len = 0;
        out.resize(out.capacity() < 16 ? 16 : out.capacity());
        detail::check(ponko_nav_view_find_path(&view_, from, to, out.data(), out.size(), &len));
        if (len > out.size()) {
            out.resize(len);
            detail::check(ponko_nav_view_find_path(&view_, from, to, out.data(), out.size(), &len));
        }
        out.resize(len);
    }
    /// The C view.
    const PonkoNavView& raw() const noexcept { return view_; }
};

/// The sky kit's settings (PonkoSkyKit, Rust `SkyKit`): clouds that drift and cast shadows, a moon with
/// phases and stars over the built-in atmosphere. ctx::set_sky_kit turns it on (or off, with `enabled`
/// false).
using sky_kit = PonkoSkyKit;
/// The sky kit's defaults, `enabled` true (Rust `SkyKit::default()`).
inline sky_kit sky_kit_defaults() { return detail::window_desc<sky_kit>(PONKO_SKY_KIT_VERSION, ponko_sky_kit_defaults); }
/// The share of the moon's face that is lit at `phase`: 0 new, 1 full (Rust `Moon::illumination`).
inline float moon_illumination(float phase) { return ponko_moon_illumination(phase); }

/// A day and night cycle (PonkoDayNight, Rust `DayNight`): a clock the game keeps that moves the sun
/// through the day and the moon through its phases. Its fields (hour, day, hours_per_second,
/// days_per_moon, first_phase, sun_color, sun_intensity, night_light) are public; call
/// ctx::update_day_night once a frame.
struct day_night : PonkoDayNight {
    /// A cycle at `hour` o'clock on day 0, with the default speed, moon and light.
    explicit day_night(float hour = 12.0f) : PonkoDayNight{} {
        struct_size = sizeof(PonkoDayNight);
        version = PONKO_DAY_NIGHT_VERSION;
        detail::check(ponko_day_night_new(hour, this));
    }
    /// Moves the clock on by `seconds` of real time, counting the days.
    void advance(float seconds) { detail::check(ponko_day_night_advance(this, seconds)); }
    /// The moon's phase at the cycle's time: 0 new, 0.5 full.
    float moon_phase() const { float p = 0; detail::check(ponko_day_night_moon_phase(this, &p)); return p; }
};

/// What a system gets (Rust `Ctx`): the world, time, input, randomness, drawing, audio, physics and the other
/// engine services. Valid only during the call; don't keep it. Calls throw ponko::error when the engine
/// reports a failure.
class ctx {
    PonkoCtx* raw_;
    std::shared_ptr<detail::identity> owner_;
    gamepad_list gamepads_changed(bool disconnected) const {
        gamepad_list list; std::size_t n = 0;
        std::array<std::uint32_t, PONKO_MAX_GAMEPADS> slots{};
        detail::check(ponko_ctx_gamepads_changed(raw_, disconnected, slots.data(), slots.size(), &n));
        list.count = static_cast<std::uint32_t>(std::min<std::size_t>(n, slots.size()));
        for (std::uint32_t i = 0; i < list.count; ++i) list.pads[i].slot = slots[i];
        return list;
    }
    glyphs glyphs_of(const char* name, std::uint32_t kind, std::uint32_t slot) const {
        glyphs g; std::size_t n = 0;
        detail::check(ponko_ctx_action_glyphs(raw_, name, kind, slot, g.items.data(), g.items.size(), &n));
        g.count = std::min<std::size_t>(n, g.items.size());
        return g;
    }
    template<class T> std::uint32_t id() const { return world_view::id_of<T>(ponko_ctx_world(raw_), *owner_); }
    template<class T> std::optional<std::uint32_t> find_id() const { return world_view::find_id<T>(ponko_ctx_world(raw_), *owner_); }
    /// Runs `fn` right after the system returns (see detail::identity::later).
    void after_system(std::function<void(ctx&)> fn) {
        std::lock_guard<std::mutex> guard(owner_->later_lock);
        owner_->later.push_back(std::move(fn));
    }
    /// Calls the hook from ctx::set_contact_hook3d. Nothing may unwind into
    /// the engine: an exception is kept for the game to rethrow.
    static void contact_trampoline(PonkoContactPair3D* raw, void* user) noexcept {
        auto& owner = *static_cast<detail::identity*>(user);
        if (owner.hook_error.set.load() || !owner.contact_hook) return;
        try { contact_pair3d pair(raw); owner.contact_hook(pair); }
        catch (...) { owner.hook_error.capture(); }
    }
    /// Calls the rebuild from ctx::set_terrain_rebuild. Nothing may unwind
    /// into the engine: an exception is kept for the game to rethrow.
    static PonkoCollisionMeshData* terrain_trampoline(const PonkoLoadedWorld* raw, std::uint64_t tile, void* user) noexcept {
        auto& owner = *static_cast<detail::identity*>(user);
        if (owner.hook_error.set.load() || !owner.terrain_rebuild) return nullptr;
        try { return owner.terrain_rebuild(raw, tile); }
        catch (...) { owner.hook_error.capture(); return nullptr; }
    }
    friend class game;
    /// Replays what after_system queued, now that no query holds the world.
    void run_after_system() {
        for (;;) {
            std::vector<std::function<void(ctx&)>> todo;
            { std::lock_guard<std::mutex> guard(owner_->later_lock); todo.swap(owner_->later); }
            if (todo.empty()) return;
            for (auto& fn : todo) fn(*this);
        }
    }
public:
    /// Wraps a C context; the game makes these for its systems.
    ctx(PonkoCtx* raw, std::shared_ptr<detail::identity> owner) : raw_(raw), owner_(std::move(owner)) {}
    /// The C context, for the C API.
    PonkoCtx* raw() const noexcept { return raw_; }
    /// The game's world (Rust `ctx.world`): entities and components.
    world_view world() const { return world_view(ponko_ctx_world(raw_), owner_); }

    // Clock (safe from query_par callbacks).
    /// Seconds this system covers: the fixed step in sim systems, the frame time otherwise.
    float dt() const noexcept { return ponko_ctx_dt(raw_); }
    /// Simulation ticks completed so far.
    std::uint64_t sim_tick() const noexcept { return ponko_ctx_sim_tick(raw_); }
    /// Simulated seconds so far (exact: ticks × fixed step).
    double sim_time() const noexcept { return ponko_ctx_sim_time(raw_); }
    /// Wall-clock seconds since the game started.
    double time() const noexcept { return ponko_ctx_time(raw_); }
    /// Frames drawn so far.
    std::uint64_t frame() const noexcept { return ponko_ctx_frame(raw_); }
    /// Frames per second, averaged.
    float fps() const noexcept { return ponko_ctx_fps(raw_); }
    /// fps, frame-time percentiles, CPU and GPU time over the last few seconds.
    ponko::frame_stats frame_stats() const { ponko::frame_stats s{}; detail::check(ponko_ctx_frame_stats(raw_, &s)); return s; }
    /// What the renderer uploaded to the GPU over the last drawn frame (all zero headless).
    ponko::gpu_uploads gpu_uploads() const { ponko::gpu_uploads u{}; detail::check(ponko_ctx_gpu_uploads(raw_, &u)); return u; }
    /// Shows or hides the on-screen performance overlay.
    void set_perf_hud(bool on) { detail::check(ponko_ctx_set_perf_hud(raw_, on)); }
    /// Debug drawing: lines, shapes and labels in 2D and 3D (Rust ctx.debug).
    ponko::debug_draw debug() noexcept { return ponko::debug_draw(raw_); }
    /// Turns on scope timing (PONKO_PROFILE_SCOPE) and allocation counting;
    /// the overlay and PONKO_BENCH do it by themselves. Off, a scope costs one atomic load.
    void set_profiling(bool on) { detail::check(ponko_ctx_set_profiling(raw_, on)); }
    /// Chooses how often the GPU passes are timed (Rust `perf.set_gpu_timing`), apart from profiling:
    /// gpu_timing_auto() (the default: one frame in 16, every frame while profiling is on),
    /// gpu_timing_every(n) or gpu_timing_off(). The overlay, traces, benchmark runs and automatic quality
    /// still get every frame's times while they are on.
    void set_gpu_timing(const ponko::gpu_timing& timing) {
        detail::check(ponko_ctx_set_gpu_timing(raw_, static_cast<std::uint32_t>(timing.kind), timing.every));
    }
    /// What set_gpu_timing chose (Rust `perf.gpu_timing()`), not what the engine did for the frame.
    ponko::gpu_timing gpu_timing() const {
        std::uint32_t kind = 0, every = 0;
        detail::check(ponko_ctx_gpu_timing(raw_, &kind, &every));
        return {static_cast<gpu_timing_kind>(kind), every};
    }
    /// Why the latest frame has no usable GPU time (Rust `perf.gpu_untimed()`): gpu_untimed::none when it has one.
    ponko::gpu_untimed gpu_untimed() const {
        std::uint32_t why = 0;
        detail::check(ponko_ctx_gpu_untimed(raw_, &why));
        return static_cast<ponko::gpu_untimed>(why);
    }
    /// Where time and allocations went over the last frames (needs set_profiling(true) for a while).
    ponko::perf_report perf_report() const {
        return ponko::perf_report(detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_perf_report(raw_, b, c, n); }));
    }
    /// A snapshot of the world and the schedule as JSON (Rust `Ctx::ecs_diagnostics`, the `ecs.diagnostics`
    /// control method): entities, memory, archetypes, components, and every system with its phase, declared
    /// access, stage and timings.
    std::string ecs_diagnostics() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_ecs_diagnostics(raw_, b, c, n); });
    }
    /// Records the next `frames` frames as a Chrome/Perfetto trace at `path`.
    void record_trace(const char* path, std::uint32_t frames) { detail::check(ponko_ctx_record_trace(raw_, path, frames)); }
    /// How the window is shown now (Rust `ctx.window.mode()`).
    ponko::window_mode window_mode() const {
        auto d = detail::window_desc<PonkoWindowModeEx>(PONKO_WINDOW_MODE_EX_VERSION, ponko_window_mode_ex_defaults);
        detail::check(ponko_ctx_window_mode_ex(raw_, &d));
        ponko::window_mode m;
        m.kind = static_cast<window_mode_kind>(d.kind);
        if (d.monitor != PONKO_WINDOW_NO_MONITOR) m.monitor = d.monitor;
        m.video = detail::window_desc<PonkoWindowVideoModeEx>(PONKO_WINDOW_VIDEO_MODE_EX_VERSION, ponko_window_video_mode_ex_defaults);
        m.video.width = d.width;
        m.video.height = d.height;
        m.video.refresh_millihertz = d.refresh_millihertz;
        m.video.bit_depth = d.bit_depth;
        return m;
    }
    /// Switches to windowed, borderless or an advertised exclusive mode (Rust `set_mode`). The web
    /// supports windowed and borderless on the current monitor, from a user gesture.
    void window_set_mode(const ponko::window_mode& mode) {
        auto d = detail::window_desc<PonkoWindowModeEx>(PONKO_WINDOW_MODE_EX_VERSION, ponko_window_mode_ex_defaults);
        d.kind = static_cast<std::uint32_t>(mode.kind);
        d.monitor = mode.monitor.value_or(PONKO_WINDOW_NO_MONITOR);
        d.width = mode.video.width;
        d.height = mode.video.height;
        d.refresh_millihertz = mode.video.refresh_millihertz;
        d.bit_depth = mode.video.bit_depth;
        detail::check(ponko_ctx_window_set_mode_ex(raw_, &d));
    }
    /// The requested size, limits, position and desktop flags (Rust `ctx.window.settings()`).
    ponko::window_properties window_properties() const {
        auto d = detail::window_desc<PonkoWindowPropertiesEx>(PONKO_WINDOW_PROPERTIES_EX_VERSION, ponko_window_properties_ex_defaults);
        detail::check(ponko_ctx_window_properties_ex(raw_, &d));
        return d;
    }
    /// The requested title.
    std::string window_title() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_window_title(raw_, b, c, n); });
    }
    /// Sets the title (the web leaves the document title alone).
    void window_set_title(const char* title) { detail::check(ponko_ctx_window_set_title(raw_, title)); }
    /// Requests a logical client size.
    void window_set_size(vec2 size) { detail::check(ponko_ctx_window_set_size(raw_, size)); }
    /// Moves the window to a physical desktop position. Not on the web.
    void window_set_position(std::int32_t x, std::int32_t y) { detail::check(ponko_ctx_window_set_position(raw_, x, y)); }
    /// Sets logical resize limits; unset removes one. Not on the web.
    void window_set_resize_limits(std::optional<vec2> min = {}, std::optional<vec2> max = {}) {
        detail::check(ponko_ctx_window_set_resize_limits(raw_, min ? &*min : nullptr, max ? &*max : nullptr));
    }
    /// Keeps the window above others. Not on the web.
    void window_set_always_on_top(bool value) { detail::check(ponko_ctx_window_set_always_on_top(raw_, value)); }
    /// Shows or hides the title bar and borders. Not on the web.
    void window_set_decorations(bool value) { detail::check(ponko_ctx_window_set_decorations(raw_, value)); }
    /// Sets the window icon; no image restores the default. Not on the web.
    void window_set_icon(const window_image* image = nullptr) {
        if (!image) {
            detail::check(ponko_ctx_window_set_icon_ex(raw_, nullptr));
            return;
        }
        auto d = detail::window_image_desc(*image);
        detail::check(ponko_ctx_window_set_icon_ex(raw_, &d));
    }
    /// Shows a system cursor.
    void window_set_cursor(cursor_icon icon) { detail::check(ponko_ctx_window_set_cursor_icon(raw_, static_cast<std::uint32_t>(icon))); }
    /// Shows a custom cursor image with its hotspot.
    void window_set_cursor(const window_image& image) {
        auto d = detail::window_image_desc(image);
        detail::check(ponko_ctx_window_set_cursor_image_ex(raw_, &d));
    }
    /// Shows or hides the cursor, independently of capture.
    void window_set_cursor_visible(bool value) { detail::check(ponko_ctx_window_set_cursor_visible(raw_, value)); }
    /// Releases, confines or locks the cursor.
    void window_set_cursor_grab(cursor_grab value) { detail::check(ponko_ctx_window_set_cursor_grab(raw_, static_cast<std::uint32_t>(value))); }
    /// How many monitors are connected (0 headless). Indices hold until monitors change.
    std::size_t window_monitor_count() const {
        std::size_t n = 0;
        detail::check(ponko_ctx_window_monitor_count(raw_, &n));
        return n;
    }
    /// A connected monitor's origin, size, DPI scale and refresh. Throws if it is gone.
    window_monitor window_monitor_info(std::uint32_t index) const {
        auto d = detail::window_desc<PonkoWindowMonitorEx>(PONKO_WINDOW_MONITOR_EX_VERSION, ponko_window_monitor_ex_defaults);
        detail::check(ponko_ctx_window_monitor_ex(raw_, index, &d));
        return d;
    }
    /// A connected monitor's name, empty if the OS gives none.
    std::string window_monitor_name(std::uint32_t index) const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_window_monitor_name(raw_, index, b, c, n); });
    }
    /// The exclusive fullscreen modes a connected monitor advertises.
    std::vector<window_video_mode> window_video_modes(std::uint32_t monitor) const {
        std::size_t n = 0;
        detail::check(ponko_ctx_window_video_mode_count(raw_, monitor, &n));
        std::vector<window_video_mode> modes;
        modes.reserve(n);
        for (std::size_t i = 0; i < n; ++i) {
            auto d = detail::window_desc<PonkoWindowVideoModeEx>(PONKO_WINDOW_VIDEO_MODE_EX_VERSION, ponko_window_video_mode_ex_defaults);
            detail::check(ponko_ctx_window_video_mode_ex(raw_, monitor, static_cast<std::uint32_t>(i), &d));
            modes.push_back(d);
        }
        return modes;
    }
    /// What the OS reports now: physical size, DPI scale, focus, minimized, current monitor.
    ponko::window_state window_state() const {
        auto d = detail::window_desc<PonkoWindowStateEx>(PONKO_WINDOW_STATE_EX_VERSION, ponko_window_state_ex_defaults);
        detail::check(ponko_ctx_window_state_ex(raw_, &d));
        return d;
    }
    /// This frame's window events (reading them does not consume them).
    std::vector<window_event> window_events() const {
        std::size_t n = 0;
        detail::check(ponko_ctx_window_event_count(raw_, &n));
        std::vector<window_event> events;
        events.reserve(n);
        for (std::size_t i = 0; i < n; ++i) {
            auto d = detail::window_desc<PonkoWindowEventEx>(PONKO_WINDOW_EVENT_EX_VERSION, ponko_window_event_ex_defaults);
            auto path = detail::read_text([&](char* b, std::size_t c, std::size_t* len) { return ponko_ctx_window_event_ex(raw_, i, &d, b, c, len); });
            events.push_back({static_cast<window_event_kind>(d.kind), d.value, d.scale_factor, std::move(path)});
        }
        return events;
    }
    /// Whether the OS asked to close the window and no system canceled it yet.
    bool window_close_requested() const {
        bool value = false;
        detail::check(ponko_ctx_window_close_requested(raw_, &value));
        return value;
    }
    /// Cancels the OS close request (an "unsaved changes?" prompt); quit() still quits.
    void window_cancel_close() { detail::check(ponko_ctx_window_cancel_close(raw_)); }
    /// Saves the requested window settings to player storage (`window.json`).
    void window_save_settings() { detail::check(ponko_ctx_window_save_settings(raw_)); }
    /// Restores saved window settings; a monitor or mode that is gone falls back to windowed.
    void window_restore_settings() { detail::check(ponko_ctx_window_restore_settings(raw_)); }
    /// The window's safe area (Rust `window.safe_area()`): the part a phone's notch, home bar and rounded
    /// corners leave, in the same logical pixels as window_size(); the whole window where nothing covers it.
    /// The engine's touch controls keep to it, and UI roots made with ui::set_inside_safe_area are laid out in it.
    rect window_safe_area() const { rect r{}; detail::check(ponko_ctx_window_safe_area(raw_, &r)); return r; }
    /// How far in from each edge the platform covers the window (Rust `window.safe_insets()`): what
    /// window_safe_area() leaves out.
    safe_insets window_safe_insets() const { safe_insets i{}; detail::check(ponko_ctx_window_safe_insets(raw_, &i)); return i; }
    /// Feeds the safe area insets a phone would report (Rust `window.feed_safe_insets()`), as a test does; a
    /// negative or not finite inset counts as 0. The UI lays out with them from the next frame.
    void window_feed_safe_insets(safe_insets insets) { detail::check(ponko_ctx_window_feed_safe_insets(raw_, insets)); }
    /// Copies UTF-8 text to the clipboard.
    void clipboard_write(const char* value) { detail::check(ponko_ctx_clipboard_write(raw_, value)); }
    /// The clipboard's text; unset headless, with no text, or while a browser read is pending.
    std::optional<std::string> clipboard_read() const {
        bool available = false;
        auto text = detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_clipboard_read(raw_, b, c, n, &available); });
        if (!available) return std::nullopt;
        return text;
    }
    /// The window's size in logical window pixels.
    vec2 window_size() const noexcept { return ponko_ctx_window_size(raw_); }
    /// This tick and system's random streams (Rust `ctx.random()`): the same numbers on every run with the
    /// same seed.
    ponko::random random() const noexcept { return ponko::random(ponko_ctx_random_bits(raw_)); }

    // Simulation controls.
    /// Ends the game after this frame (or the headless run after this tick).
    void quit() { detail::check(ponko_ctx_quit(raw_)); }
    /// Whether a system asked to quit this tick or frame (Rust `ctx.quit_requested()`): what a headless test checks.
    bool quit_requested() const { bool q = false; detail::check(ponko_ctx_quit_requested(raw_, &q)); return q; }

    // Lockstep networking (game::lockstep_host / lockstep_join); offline: one player.
    /// This player's number in a lockstep session (0 is the host; 0 offline).
    std::uint32_t player() const { std::uint32_t p = 0, n = 0; detail::check(ponko_ctx_net_player(raw_, &p, &n)); return p; }
    /// How many players the session has (1 offline).
    std::uint32_t players() const { std::uint32_t p = 0, n = 0; detail::check(ponko_ctx_net_player(raw_, &p, &n)); return n; }
    /// Where the lockstep session is (Rust `NetState`): offline, waiting, playing, ...
    PonkoNetState net_state() const { std::uint32_t s = 0; detail::check(ponko_ctx_net_state(raw_, &s)); return static_cast<PonkoNetState>(s); }
    /// Whether the players' worlds stopped matching (details on stderr).
    bool desynced() const {
        bool found = false; std::uint64_t tick = 0; std::uint32_t who = 0;
        detail::check(ponko_ctx_net_desync(raw_, &found, &tick, &who));
        return found;
    }
    /// This player's input for the next tick it can go into (a plain struct
    /// of at most 1024 bytes). The last call before that tick wins.
    template<class T> void set_input(const T& input) {
        static_assert(detail::pod<T> && sizeof(T) <= 1024, "inputs are plain structs of at most 1024 bytes");
        detail::check(ponko_ctx_set_input(raw_, &input, sizeof(T)));
    }
    /// `player`'s input for this tick, if they sent one.
    template<class T> std::optional<T> input(std::uint32_t player) const {
        static_assert(detail::pod<T>, "inputs are plain structs");
        const void* data = nullptr;
        std::size_t size = 0;
        detail::check(ponko_ctx_input(raw_, player, &data, &size));
        if (size != sizeof(T)) return std::nullopt;
        T value;
        std::memcpy(&value, data, sizeof(T));
        return value;
    }
    // Rollback for the whole game (Rust GameRollback): see ponko_ctx_rollback_start.
    /// Starts a rollback session in setup or a frame system: every simulation tick runs through it from
    /// the next one on, and a late input that differs from the prediction loads the state before it and
    /// runs the simulation systems again. Start `config` from ponko_rollback_config_default(players, player).
    void rollback_start(const PonkoRollbackConfig& config) { detail::check(ponko_ctx_rollback_start(raw_, &config)); }
    /// Stops the session; ticks run as before. Returns false if none ran.
    bool rollback_stop() {
        const PonkoStatus status = ponko_ctx_rollback_stop(raw_);
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// This player's input for the next tick that runs (a plain struct of at most
    /// PONKO_ROLLBACK_INPUT_MAX bytes). It stays until set again.
    template<class T> void rollback_set_input(const T& input) {
        static_assert(detail::pod<T> && sizeof(T) <= PONKO_ROLLBACK_INPUT_MAX, "rollback inputs are plain structs of at most 32 bytes");
        detail::check(ponko_ctx_rollback_set_input(raw_, &input, sizeof(T)));
    }
    /// In a simulation system: `player`'s input for this tick (zeros before their first one).
    template<class T> T rollback_input(std::uint32_t player) const {
        static_assert(detail::pod<T> && sizeof(T) <= PONKO_ROLLBACK_INPUT_MAX, "rollback inputs are plain structs of at most 32 bytes");
        T value{};
        detail::check(ponko_ctx_rollback_input(raw_, player, &value, sizeof(T)));
        return value;
    }
    /// The packet to send to every other peer this frame, valid until the next rollback call.
    std::pair<const std::uint8_t*, std::size_t> rollback_packet() {
        const std::uint8_t* data = nullptr;
        std::size_t size = 0;
        detail::check(ponko_ctx_rollback_packet(raw_, &data, &size));
        return {data, size};
    }
    /// Takes a packet from another peer; false if it isn't one of this session's.
    bool rollback_receive(const void* data, std::size_t size) {
        bool accepted = false;
        detail::check(ponko_ctx_rollback_receive(raw_, data, size, &accepted));
        return accepted;
    }
    /// Whether this tick runs again after a rollback: skip one-shot effects (sounds are dropped already).
    bool rollback_resimulating() const { bool again = false; detail::check(ponko_ctx_rollback_resimulating(raw_, &again)); return again; }
    /// The next tick the session runs, counted from its start.
    std::uint64_t rollback_frame() const {
        std::uint64_t frame = 0, confirmed = 0; bool has = false;
        detail::check(ponko_ctx_rollback_frames(raw_, &frame, &has, &confirmed));
        return frame;
    }
    /// The last tick every player's input is in for; none before the first.
    std::optional<std::uint64_t> rollback_confirmed_frame() const {
        std::uint64_t frame = 0, confirmed = 0; bool has = false;
        detail::check(ponko_ctx_rollback_frames(raw_, &frame, &has, &confirmed));
        if (!has) return std::nullopt;
        return confirmed;
    }
    /// What the session did so far: ticks, rollbacks, ticks run again, stalls, bytes.
    PonkoRollbackStats rollback_stats() const { PonkoRollbackStats s{}; detail::check(ponko_ctx_rollback_stats(raw_, &s)); return s; }
    /// Whether two peers' worlds stopped matching (compared every config.checksum_every ticks).
    bool rollback_desynced() const {
        bool found = false; std::uint64_t frame = 0; std::uint32_t player = 0;
        detail::check(ponko_ctx_rollback_desync(raw_, &found, &frame, &player));
        return found;
    }
    // Declared replication (game::messages, then ctx::replication in setup).
    /// Declares what replicates, the same on the server and every client,
    /// in setup: components with their fields, a budget in bytes per second
    /// per client (0: 64 KB/s) and optional interest rings (null: every
    /// client gets every entity).
    void replication(const PonkoReplicatedComponent* components, std::size_t count, std::uint32_t budget = 0,
                     const PonkoInterestConfig* interest = nullptr) {
        detail::check(ponko_ctx_replication(raw_, components, count, budget, interest));
    }
    /// A component of C++ type `T` (a plain struct) for ctx::replication:
    /// its `name` on the wire and its `fields`, which must stay alive until
    /// replication() returns.
    template<class T> PonkoReplicatedComponent replicated(const char* name, const std::vector<PonkoReplicatedField>& fields) {
        static_assert(detail::pod<T>, "replicated components are plain structs");
        return PonkoReplicatedComponent{name, id<T>(), fields.data(), fields.size()};
    }
    /// Server: marks `e` as replicated (owner, priority, global), or changes how.
    void replicate(entity e, const PonkoReplicated& how = ponko_replicated_default()) {
        detail::check(ponko_ctx_replicate(raw_, e, &how));
    }
    /// This side (server or client), whether it joined, its peer id, the newest server tick.
    PonkoReplicationStatus replication_status() const {
        PonkoReplicationStatus s{}; detail::check(ponko_ctx_replication_status(raw_, &s)); return s;
    }
    /// The net id of `e` (an original once sent, or a replica), the same on every side.
    std::optional<std::uint32_t> net_id(entity e) const {
        std::uint32_t id = 0; const PonkoStatus status = ponko_ctx_net_id(raw_, e, &id);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return id;
    }
    /// The entity of net id `id` on this side, if there is one.
    std::optional<entity> net_entity(std::uint32_t id) const {
        entity e = 0; const PonkoStatus status = ponko_ctx_net_entity(raw_, id, &e);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return e;
    }
    /// Client: what replica `e` is (its id, owner, epoch, whether it is this client's).
    std::optional<PonkoReplica> replica(entity e) const {
        PonkoReplica r{}; const PonkoStatus status = ponko_ctx_replica(raw_, e, &r);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return r;
    }
    /// Server: the clients that joined, in id order.
    std::vector<std::uint64_t> replication_clients() const {
        std::size_t n = 0; detail::check(ponko_ctx_replication_clients(raw_, nullptr, 0, &n));
        std::vector<std::uint64_t> peers(n); detail::check(ponko_ctx_replication_clients(raw_, peers.data(), peers.size(), &n));
        peers.resize(std::min(n, peers.size())); return peers;
    }
    /// Server: where `peer` looks from, for interest; nullopt goes back to its own entities' positions.
    void set_viewer(std::uint64_t peer, std::optional<vec3> at) {
        detail::check(ponko_ctx_replication_set_viewer(raw_, peer, at ? &*at : nullptr));
    }
    /// Server: what it sent `peer`, if that client is there.
    std::optional<PonkoReplicationPeerStats> replication_peer_stats(std::uint64_t peer) const {
        PonkoReplicationPeerStats s{}; const PonkoStatus status = ponko_ctx_replication_peer_stats(raw_, peer, &s);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return s;
    }
    /// Server: how it resends to `peer` (early, on the timeout, the timeout now), if that client is there.
    std::optional<PonkoReplicationResendStats> replication_resend_stats(std::uint64_t peer) const {
        PonkoReplicationResendStats s{}; const PonkoStatus status = ponko_ctx_replication_resend_stats(raw_, peer, &s);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return s;
    }
    /// Client: what it received and sent.
    PonkoReplicationClientStats replication_client_stats() const {
        PonkoReplicationClientStats s{}; detail::check(ponko_ctx_replication_client_stats(raw_, &s)); return s;
    }
    /// Declares the game's input, a plain struct `I` of which `fields`
    /// travel, in setup after replication() (ponko_ctx_replication_input).
    /// A game has one input type. Throws ponko::error for a second one.
    template<class I> void replication_input(const char* name, const std::vector<PonkoReplicatedField>& fields) {
        static_assert(detail::pod<I>, "the input is a plain struct");
        detail::check(ponko_ctx_replication_input(raw_, name, sizeof(I), fields.data(), fields.size()));
    }
    /// Predicts the component replicated as `component` (a plain struct
    /// `S`) on the client that owns it: `step(S&, const I&)` moves it for
    /// one input, the same on the server, and must not throw or touch the
    /// ctx (ponko_ctx_replication_predicted). In setup, after
    /// replication_input<I>.
    template<class S, class I, class F> void replication_predicted(const char* component, F step) {
        static_assert(detail::pod<S> && detail::pod<I>, "predicted state and input are plain structs");
        auto* h = new F(std::move(step));
        auto call = [](void* state, std::size_t state_size, const void* input, std::size_t input_size, void* user) noexcept {
            if (state_size != sizeof(S) || input_size != sizeof(I)) return;
            S s; I i;
            std::memcpy(static_cast<void*>(&s), state, sizeof(S));
            std::memcpy(static_cast<void*>(&i), input, sizeof(I));
            (*static_cast<F*>(user))(s, static_cast<const I&>(i));
            std::memcpy(state, static_cast<const void*>(&s), sizeof(S));
        };
        auto free = [](void* user) noexcept { delete static_cast<F*>(user); };
        const PonkoStatus status = ponko_ctx_replication_predicted(raw_, component, call, h, free);
        if (status != PONKO_OK) delete h;
        detail::check(status);
    }
    /// Shows the component replicated as `component` between the values
    /// received, on clients: its floats move smoothly `config.delay` in the
    /// past, its other fields are set when they arrive
    /// (ponko_ctx_replication_interpolated). nullopt: the defaults with no
    /// extrapolation. In setup.
    void replication_interpolated(const char* component, std::optional<snapshot_config> config = std::nullopt) {
        detail::check(ponko_ctx_replication_interpolated(raw_, component, config ? &*config : nullptr));
    }
    /// Declares an event kind called `name`, in setup, and returns its kind
    /// for replication_send and net_events (ponko_ctx_replication_event).
    std::uint16_t replication_event(const char* name) {
        std::uint16_t kind = 0; detail::check(ponko_ctx_replication_event(raw_, name, &kind)); return kind;
    }
    /// For interest, an entity is where the `dims` (2 or 3) floats at byte
    /// `offset` of its `T` say, instead of its transform
    /// (ponko_ctx_replication_position). In setup.
    template<class T> void replication_position(std::size_t offset, std::uint32_t dims = 3) {
        static_assert(detail::pod<T>, "the position's component is a plain struct");
        detail::check(ponko_ctx_replication_position(raw_, id<T>(), offset, dims));
    }
    /// This side's input for this tick (ponko_ctx_set_replication_input).
    template<class I> void set_replication_input(const I& input) {
        static_assert(detail::pod<I>, "the input is a plain struct");
        detail::check(ponko_ctx_set_replication_input(raw_, &input, sizeof(I)));
    }
    /// Server: the input of `peer` applied last, if it sent one
    /// (ponko_ctx_replication_input_of).
    template<class I> std::optional<I> replication_input_of(std::uint64_t peer) const {
        static_assert(detail::pod<I>, "the input is a plain struct");
        I input{};
        const PonkoStatus status = ponko_ctx_replication_input_of(raw_, peer, &input, sizeof(I));
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return input;
    }
    /// Sends `size` bytes as an event of `kind` (replication_event) to the
    /// server, one client (`peer`) or every client, reliable and in order
    /// (ponko_ctx_replication_send).
    void replication_send(PonkoEventTarget target, std::uint16_t kind, const void* data, std::size_t size, std::uint64_t peer = 0) {
        detail::check(ponko_ctx_replication_send(raw_, static_cast<std::uint32_t>(target), peer, kind, data, size));
    }
    /// Sends a plain struct as an event of `kind`. Arrays and pointers (a
    /// string literal, a buffer) take the bytes-and-size form above.
    template<class T, std::enable_if_t<!std::is_array_v<T> && !std::is_pointer_v<T>, int> = 0>
    void replication_send(PonkoEventTarget target, std::uint16_t kind, const T& value, std::uint64_t peer = 0) {
        static_assert(detail::pod<T>, "an event is a plain struct (or bytes)");
        replication_send(target, kind, &value, sizeof(T), peer);
    }
    /// The events that arrived since this system last read them, in order,
    /// valid until the system returns (ponko_ctx_net_events). Read once
    /// per system; works inside a query, not query_par.
    struct net_events_view {
        /// The first event.
        const PonkoNetEvent* data = nullptr;
        /// How many.
        std::size_t size = 0;
        /// For range-for.
        const PonkoNetEvent* begin() const { return data; }
        /// For range-for.
        const PonkoNetEvent* end() const { return data + size; }
    };
    /// See net_events_view.
    net_events_view net_events() {
        net_events_view v; detail::check(ponko_ctx_net_events(raw_, &v.data, &v.size)); return v;
    }
    /// Draws the world with baked lighting (ponko::baked_lighting::raw()), or without it (nullptr):
    /// lightmapped models take their lightmap, the others the probe volume's light.
    void set_baked_lighting(const PonkoBakedLighting* baked) { detail::check(ponko_ctx_set_baked_lighting(raw_, baked)); }
    /// The Steam lobby of this session, or 0 (not created yet, or not Steam).
    std::uint64_t steam_lobby() const { std::uint64_t l = 0; detail::check(ponko_ctx_steam_lobby(raw_, &l)); return l; }
    /// Opens the Steam overlay to invite friends to this session's lobby.
    void steam_invite() { detail::check(ponko_ctx_steam_invite(raw_)); }
    /// Brings Steam Cloud and the stored files in step for `files` (names, or
    /// folders ending in '/'): the newest copy of each wins. Returns {downloaded,
    /// uploaded}; {0, 0} without Steam or with its cloud off (see steam::start).
    std::pair<std::uint32_t, std::uint32_t> steam_sync_cloud(std::initializer_list<const char*> files) {
        std::uint32_t down = 0, up = 0;
        detail::check(ponko_ctx_steam_sync_cloud(raw_, files.begin(), files.size(), &down, &up));
        return {down, up};
    }
    /// Deletes the stored file `name` and its Steam Cloud copy.
    void steam_delete_cloud_file(const char* name) { detail::check(ponko_ctx_steam_delete_cloud_file(raw_, name)); }
    /// Pauses the simulation; frame systems keep running.
    void pause() { detail::check(ponko_ctx_pause(raw_)); }
    /// Resumes a paused simulation.
    void resume() { detail::check(ponko_ctx_resume(raw_)); }
    /// Runs one simulation tick while paused.
    void step() { detail::check(ponko_ctx_step(raw_)); }
    /// Sets the simulation speed multiplier: 0.25 is slow motion, 2 fast forward, 0 a freeze.
    /// Results don't depend on it. Throws ponko::error for a negative or non-finite speed.
    void set_sim_speed(float speed) { detail::check(ponko_ctx_set_sim_speed(raw_, speed)); }
    /// The simulation speed multiplier.
    float sim_speed() const { float s = 0; detail::check(ponko_ctx_sim_speed(raw_, &s)); return s; }
    /// Whether the simulation is paused.
    bool is_paused() const { bool p = false; detail::check(ponko_ctx_is_paused(raw_, &p)); return p; }

    // Input.
    /// Install a copy of bindings. Rebinding itself generates no button edges.
    void set_actions(const action_map& map) { detail::check(ponko_ctx_set_actions(raw_, map.raw())); }
    /// A copy of the installed bindings.
    action_map actions() const { action_map map; detail::check(ponko_ctx_actions(raw_, map.raw())); return map; }
    /// Adds or replaces one installed action, leaving the others alone (Rust `Input::set_action`): the name
    /// keeps its id, held sources fire no pressed edge. Throws ponko::error for an invalid binding.
    void set_action(const char* name, const action& a) {
        detail::check(ponko_ctx_set_action(raw_, name, a.kind, a.bindings.data(), a.bindings.size()));
    }
    /// Everything that went down this frame (keys, mouse buttons, gamepad buttons, and gamepad axes
    /// past `axis_threshold`, 0 for the default of PONKO_PRESS_THRESHOLD), in a fixed order: keys,
    /// mouse, gamepad buttons, axes, touch gestures. What a rebinding screen binds, instead of polling
    /// every key.
    /// Throws ponko::error for a negative threshold.
    std::vector<pressed_input> pressed_this_frame(float axis_threshold = 0.0f) const {
        std::size_t count = 0;
        detail::check(ponko_ctx_pressed_this_frame(raw_, axis_threshold, nullptr, 0, &count));
        std::vector<pressed_input> list(count);
        if (count == 0) return list;
        static_assert(sizeof(pressed_input) == sizeof(PonkoActionBinding), "pressed_input is its C description");
        std::size_t filled = 0;
        detail::check(ponko_ctx_pressed_this_frame(raw_, axis_threshold, &list.data()->raw, count, &filled));
        list.resize(filled < count ? filled : count);
        return list;
    }
    /// Feeds manual input as if a device sent it (tests and replays): the same state as live input.
    void feed_input(input_event e) {
        switch (e.kind) {
        case 6: detail::check(ponko_ctx_feed_mouse_move(raw_, e.point)); break;
        case 7: detail::check(ponko_ctx_feed_mouse_wheel(raw_, e.point)); break;
        default: detail::check(ponko_ctx_feed_input(raw_, e.kind, e.slot, e.input, e.value));
        }
    }
    /// Whether named action `name` is held. Unknown actions and actions of another kind read false.
    bool action_down(const char* name) const { bool value = false; detail::check(ponko_ctx_action_down(raw_, name, &value)); return value; }
    /// Whether named action `name` went down this tick. Unknown actions read false.
    bool action_pressed(const char* name) const { bool value = false; detail::check(ponko_ctx_action_pressed(raw_, name, &value)); return value; }
    /// Whether named action `name` went up this tick. Unknown actions read false.
    bool action_released(const char* name) const { bool value = false; detail::check(ponko_ctx_action_released(raw_, name, &value)); return value; }

    // Touch (Rust `InputEvent::Touch`, `input.touches`, `gesture_events`, `set_touch_controls`, ...).
    /// Feeds a finger's event as the window does: `id` names the finger, `at` is in window pixels.
    void feed_touch(std::uint64_t id, touch_phase phase, vec2 at) { detail::check(ponko_ctx_feed_touch(raw_, id, static_cast<std::uint32_t>(phase), at)); }
    /// Feeds a trackpad gesture (Rust `InputEvent::Trackpad`): gesture::pinch (`amount` how much it
    /// spread), gesture::rotate (radians) or gesture::double_tap, at the mouse pointer; no touches.
    void feed_trackpad(gesture g, touch_phase phase, float amount) {
        detail::check(ponko_ctx_feed_trackpad(raw_, static_cast<std::uint32_t>(g), static_cast<std::uint32_t>(phase), amount));
    }
    /// The fingers on the screen, oldest first.
    std::vector<touch> touches() const {
        std::size_t count = 0;
        detail::check(ponko_ctx_touches(raw_, nullptr, 0, &count));
        std::vector<touch> list(count);
        if (count == 0) return list;
        std::size_t filled = 0;
        detail::check(ponko_ctx_touches(raw_, list.data(), count, &filled));
        list.resize(filled < count ? filled : count);
        return list;
    }
    /// The gestures that happened this frame, in order.
    std::vector<gesture_event> gesture_events() const {
        std::size_t count = 0;
        detail::check(ponko_ctx_gesture_events(raw_, nullptr, 0, &count));
        std::vector<gesture_event> list(count);
        if (count == 0) return list;
        std::size_t filled = 0;
        detail::check(ponko_ctx_gesture_events(raw_, list.data(), count, &filled));
        list.resize(filled < count ? filled : count);
        return list;
    }
    /// How far a one-finger drag moved this frame, in pixels.
    vec2 drag_delta() const { vec2 v{}; detail::check(ponko_ctx_gesture_deltas(raw_, &v, nullptr, nullptr)); return v; }
    /// How much a pinch spread this frame, as a factor (1: no pinch).
    float pinch_scale() const { float v = 1.0f; detail::check(ponko_ctx_gesture_deltas(raw_, nullptr, &v, nullptr)); return v; }
    /// How far two fingers turned this frame, in radians, clockwise positive.
    float rotation_delta() const { float v = 0.0f; detail::check(ponko_ctx_gesture_deltas(raw_, nullptr, nullptr, &v)); return v; }
    /// Whether this is a touch device: the platform has a touch screen, or a finger was seen.
    bool touch_device() const { bool b = false; detail::check(ponko_ctx_touch_device(raw_, &b)); return b; }
    /// Says whether this is a touch device (on-screen controls shown automatically follow it).
    void set_touch_device(bool touch_screen) { detail::check(ponko_ctx_set_touch_device(raw_, touch_screen)); }
    /// The gesture thresholds.
    ponko::gesture_settings gesture_settings() const { ponko::gesture_settings s{}; detail::check(ponko_ctx_gesture_settings(raw_, &s)); return s; }
    /// Sets the gesture thresholds; read them first and change what you need.
    void set_gesture_settings(const ponko::gesture_settings& s) { detail::check(ponko_ctx_set_gesture_settings(raw_, &s)); }
    /// Puts on-screen controls on the screen, replacing any.
    void set_touch_controls(const touch_controls& controls) {
        std::vector<PonkoTouchControl> list;
        std::vector<PonkoTouchControlStyle> styles;
        std::vector<PonkoTouchControlPlace> places;
        const PonkoTouchControlsDesc d = controls.desc(list, styles, places);
        detail::check(ponko_ctx_set_touch_controls_ex(raw_, &d));
    }
    /// Takes the on-screen controls away.
    void remove_touch_controls() { detail::check(ponko_ctx_remove_touch_controls(raw_)); }
    /// Changes when the on-screen controls show.
    void set_touch_controls_show(touch_show when) { detail::check(ponko_ctx_set_touch_controls_show(raw_, static_cast<std::uint32_t>(when))); }
    /// Shows or hides the control for `action` (a hidden one takes no touches); false when there is none.
    bool set_touch_control_shown(const char* action, bool shown) {
        PonkoStatus status = ponko_ctx_set_touch_control_shown(raw_, action, shown);
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// The same by an id from action_id(), with no name lookup: the form for a system that runs every frame.
    bool set_touch_control_shown(ponko::action_id action, bool shown) {
        PonkoStatus status = ponko_ctx_set_touch_control_shown_id(raw_, action.bits, shown);
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// Where the control for `action` is, in window pixels (what a test touches), if there is one.
    std::optional<vec2> touch_control_center(const char* action) const {
        vec2 at{};
        PonkoStatus status = ponko_ctx_touch_control_center(raw_, action, &at);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return at;
    }
    /// The same by an id from action_id().
    std::optional<vec2> touch_control_center(ponko::action_id action) const {
        vec2 at{};
        PonkoStatus status = ponko_ctx_touch_control_center_id(raw_, action.bits, &at);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return at;
    }
    /// Turns the player's arranging of the on-screen controls on or off (Rust
    /// `Input::edit_touch_controls`): while on, they show, press nothing, and a drag moves one into
    /// the player's layout. Save it when the player is done (save_touch_layout).
    void edit_touch_controls(bool on) { detail::check(ponko_ctx_edit_touch_controls(raw_, on)); }
    /// Opens the engine's ready-made screen where the player arranges the on-screen controls (Rust
    /// `Input::arrange_touch_controls`): edit mode at once, the screen (`touch.arrange.*` UI nodes) at the next
    /// UI pass; its Save writes the layout to storage, its Cancel puts the old one back.
    void arrange_touch_controls() { detail::check(ponko_ctx_arrange_touch_controls(raw_)); }
    /// Whether the player is arranging the on-screen controls.
    bool editing_touch_controls() const { bool b = false; detail::check(ponko_ctx_editing_touch_controls(raw_, &b)); return b; }
    /// Every control's size in the player's layout, times the game's.
    float touch_layout_size() const { float v = 1; detail::check(ponko_ctx_touch_layout(raw_, &v, nullptr)); return v; }
    /// Every control's opacity in the player's layout, 0 to 1.
    float touch_layout_opacity() const { float v = 1; detail::check(ponko_ctx_touch_layout(raw_, nullptr, &v)); return v; }
    /// Sets every control's size and opacity in the player's layout (a settings screen's sliders);
    /// each control's own changes stay.
    void set_touch_layout(float size, float opacity) { detail::check(ponko_ctx_set_touch_layout(raw_, size, opacity)); }
    /// What the player changed about the control driving `action`.
    touch_adjust touch_layout_control(const char* action) const {
        touch_adjust a{}; detail::check(ponko_ctx_touch_layout_control(raw_, action, &a)); return a;
    }
    /// Sets what the player changed about the control driving `action`.
    void set_touch_layout_control(const char* action, const touch_adjust& adjust) {
        detail::check(ponko_ctx_set_touch_layout_control(raw_, action, &adjust));
    }
    /// Forgets the player's changes to the control driving `action`: it follows the game again.
    void forget_touch_layout_control(const char* action) { detail::check(ponko_ctx_set_touch_layout_control(raw_, action, nullptr)); }
    /// Puts every control back where the game puts it, at size and opacity 1.
    void reset_touch_layout() { detail::check(ponko_ctx_reset_touch_layout(raw_)); }
    /// The player's layout as JSON, for a settings file of the game's own.
    std::string touch_layout_json() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_touch_layout_json(raw_, b, c, n); });
    }
    /// Replaces the player's layout with one from touch_layout_json's text; throws ponko::error if
    /// it isn't one (nothing changes).
    void set_touch_layout_json(const char* json) { detail::check(ponko_ctx_set_touch_layout_json(raw_, json)); }
    /// Saves the player's layout in the game's storage (`touch_controls.json`).
    void save_touch_layout() { detail::check(ponko_ctx_save_touch_layout(raw_)); }
    /// Reads back the saved layout, at setup; no file changes nothing.
    void restore_touch_layout() { detail::check(ponko_ctx_restore_touch_layout(raw_)); }
    /// Scalar axis action `name`, -1 to 1. Unknown actions read 0.
    float action_axis(const char* name) const { float value = 0; detail::check(ponko_ctx_action_axis(raw_, name, &value)); return value; }
    /// Vector axis action `name`, each component -1 to 1. Unknown actions read zero.
    vec2 action_axis2(const char* name) const { vec2 value{}; detail::check(ponko_ctx_action_axis2(raw_, name, &value)); return value; }
    /// Resolves an action name once (Rust's `ctx.input.action_id`); the
    /// queries then take the id and skip the name lookup. Resolve in setup,
    /// after set_actions. An unknown name throws ponko::error (kind
    /// PONKO_ERROR_NOT_FOUND, naming the known actions), where Rust logs it.
    ponko::action_id action_id(const char* name) const {
        ponko::action_id id; detail::check(ponko_ctx_action_id(raw_, name, &id.bits)); return id;
    }
    /// Resolves several names at once: `auto [jump, fire] = ctx.action_ids({"jump", "fire"});`.
    template<std::size_t N> std::array<ponko::action_id, N> action_ids(const char* const (&names)[N]) const {
        std::array<std::uint32_t, N> bits{};
        detail::check(ponko_ctx_action_ids(raw_, names, N, bits.data()));
        std::array<ponko::action_id, N> ids{};
        for (std::size_t i = 0; i < N; ++i) ids[i].bits = bits[i];
        return ids;
    }
    /// The same queries by an id from action_id(); action_id::none() reads false and zero.
    ///@{
    bool action_down(ponko::action_id a) const { bool value = false; detail::check(ponko_ctx_action_down_id(raw_, a.bits, &value)); return value; }
    bool action_pressed(ponko::action_id a) const { bool value = false; detail::check(ponko_ctx_action_pressed_id(raw_, a.bits, &value)); return value; }
    bool action_released(ponko::action_id a) const { bool value = false; detail::check(ponko_ctx_action_released_id(raw_, a.bits, &value)); return value; }
    float action_axis(ponko::action_id a) const { float value = 0; detail::check(ponko_ctx_action_axis_id(raw_, a.bits, &value)); return value; }
    vec2 action_axis2(ponko::action_id a) const { vec2 value{}; detail::check(ponko_ctx_action_axis2_id(raw_, a.bits, &value)); return value; }
    ///@}
    /// Starts a copy of `s` now, replacing a running scenario (Rust's
    /// `Input::start_scenario`); from a system, its tick 0 is the next tick.
    void start_scenario(const ponko::input_scenario& s) { detail::check(ponko_ctx_start_scenario(raw_, s.raw())); }
    /// Ends the running scenario, releasing what it holds; whether one was running.
    bool cancel_scenario() { bool was = false; detail::check(ponko_ctx_cancel_scenario(raw_, &was)); return was; }
    /// Starts recording the input (Rust `Input::start_recording`), from
    /// the next simulation tick; what is held now is recorded at its tick 0.
    void start_input_recording() { detail::check(ponko_ctx_start_input_recording(raw_)); }
    /// Stops the recording: a scenario that replays the input on the same
    /// ticks, or nothing when none was running.
    std::optional<ponko::input_scenario> stop_input_recording() {
        PonkoInputScenario* raw = nullptr;
        detail::check(ponko_ctx_stop_input_recording(raw_, &raw));
        if (!raw) return std::nullopt;
        return ponko::input_scenario(raw);
    }
    /// Whether key `k` is held.
    bool key_down(key k) const { bool b = false; detail::check(ponko_ctx_key_down(raw_, k, &b)); return b; }
    /// Whether key `k` went down this frame.
    bool key_pressed(key k) const { bool b = false; detail::check(ponko_ctx_key_pressed(raw_, k, &b)); return b; }
    /// Whether key `k` went up this frame.
    bool key_released(key k) const { bool b = false; detail::check(ponko_ctx_key_released(raw_, k, &b)); return b; }
    /// Whether mouse button `m` is held.
    bool mouse_down(mouse m) const { bool b = false; detail::check(ponko_ctx_mouse_down(raw_, m, &b)); return b; }
    /// Whether mouse button `m` went down this frame.
    bool mouse_pressed(mouse m) const { bool b = false; detail::check(ponko_ctx_mouse_pressed(raw_, m, &b)); return b; }
    /// Whether mouse button `m` went up this frame.
    bool mouse_released(mouse m) const { bool b = false; detail::check(ponko_ctx_mouse_released(raw_, m, &b)); return b; }
    /// The mouse position in window pixels.
    vec2 mouse_pos() const { vec2 v{}; detail::check(ponko_ctx_mouse_pos(raw_, &v)); return v; }
    /// The mouse position in world units, through the 2D camera.
    vec2 mouse_world() const { vec2 v{}; detail::check(ponko_ctx_mouse_world(raw_, &v)); return v; }
    /// Scroll this frame, in lines; positive y is away from the user.
    vec2 mouse_wheel() const { vec2 v{}; detail::check(ponko_ctx_mouse_wheel(raw_, &v)); return v; }
    /// How far the mouse moved this frame in raw device units (+y down),
    /// even while the cursor is locked: for mouse look.
    vec2 mouse_motion() const { vec2 v{}; detail::check(ponko_ctx_mouse_motion(raw_, &v)); return v; }
    /// Hides the cursor and holds it in the window (or lets it go), after this frame.
    void set_cursor_locked(bool locked) { detail::check(ponko_ctx_set_cursor_locked(raw_, locked)); }
    /// Whether the game asked for the cursor to be locked.
    bool cursor_locked() const { bool b = false; detail::check(ponko_ctx_cursor_locked(raw_, &b)); return b; }

    // Key labels on the player's keyboard layout (Rust `input.key_labels()`): what is printed on the
    // key the player presses, for prompts. They follow the system's layout while the window is open.
    /// What key `k` is labelled on the player's keyboard: "Z" for PONKO_KEY_W on a French one,
    /// "Space" for keys whose name doesn't depend on the layout.
    std::string key_label(key k) const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ctx_key_label(raw_, k, b, c, l); });
    }
    /// A glyph's label on the player's layout (ponko::glyph_label gives the US one).
    std::string layout_glyph_label(glyph g) const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ctx_glyph_label(raw_, g, b, c, l); });
    }
    /// Whether the key labels changed this frame (a layout switch, a key's label learned from a
    /// press, or set_key_labels): redraw text prompts.
    bool key_labels_changed() const { bool b = false; detail::check(ponko_ctx_key_labels_changed(raw_, &b)); return b; }
    /// Replaces the key labels with a US keyboard relabelled by `labels`: a "keyboard layout"
    /// option, or a test. Keys pressed later still correct them.
    void set_key_labels(const layout_key* labels, std::size_t count) {
        detail::check(ponko_ctx_set_key_labels(raw_, labels, count));
    }
    /// set_key_labels from a list: `ctx.set_key_labels({{PONKO_KEY_W, 'z'}, {PONKO_KEY_A, 'q'}})`.
    void set_key_labels(std::initializer_list<layout_key> labels) { set_key_labels(labels.begin(), labels.size()); }

    // IME composition (Rust `input.feed_ime`, `preedit`, `set_ime_allowed`, ...). The retained UI's text
    // inputs do all of this themselves; these are for a game's own text widgets, custom hosts and tests.
    /// The text typed this frame: committed Unicode from the keyboard and from IME commits.
    std::string typed_text() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ctx_typed_text(raw_, b, c, l); });
    }
    /// Records an IME step as the window system would; a commit's text is typed.
    void feed_ime(const ime_event& e) { detail::check(ponko_ctx_feed_ime(raw_, &e)); }
    /// The text being composed, not yet committed; empty when nothing is.
    std::string preedit() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ctx_preedit(raw_, b, c, l); });
    }
    /// The caret in the preedit as a byte range (start == end: a plain caret), if the IME shows one.
    std::optional<std::pair<std::size_t, std::size_t>> preedit_cursor() const {
        bool has = false; std::size_t a = 0, b = 0;
        detail::check(ponko_ctx_preedit_cursor(raw_, &has, &a, &b));
        return has ? std::optional<std::pair<std::size_t, std::size_t>>({a, b}) : std::nullopt;
    }
    /// Whether a composition went on this frame: Enter, Escape, Backspace and the arrows belong to the IME then.
    bool composing() const { bool b = false; detail::check(ponko_ctx_composing(raw_, &b)); return b; }
    /// Lets the window system compose text with an IME (while your text widget has the focus), after this frame.
    void set_ime_allowed(bool allowed) { detail::check(ponko_ctx_set_ime_allowed(raw_, allowed)); }
    /// Whether IME is allowed.
    bool ime_allowed() const { bool b = false; detail::check(ponko_ctx_ime_allowed(raw_, &b)); return b; }
    /// Where your text widget's caret is, in window points, so the candidate window opens next to it.
    void set_ime_cursor_area(ponko::rect area) { detail::check(ponko_ctx_set_ime_cursor_area(raw_, area)); }
    /// The caret area set last, if any.
    std::optional<ponko::rect> ime_cursor_area() const { bool has = false; ponko::rect r{}; detail::check(ponko_ctx_ime_cursor_area(raw_, &has, &r)); return has ? std::optional<ponko::rect>(r) : std::nullopt; }
    /// Drops the composition in progress (your text widget lost the focus mid-composition).
    void cancel_composition() { detail::check(ponko_ctx_cancel_composition(raw_)); }
    /// The connected gamepads: `for (ponko::gamepad pad : ctx.gamepads())`.
    gamepad_list gamepads() const {
        gamepad_list list;
        detail::check(ponko_ctx_gamepad_count(raw_, &list.count));
        list.count = std::min<std::uint32_t>(list.count, PONKO_MAX_GAMEPADS);
        for (std::uint32_t i = 0; i < list.count; ++i) detail::check(ponko_ctx_gamepad_at(raw_, i, &list.pads[i].slot));
        return list;
    }
    /// Whether button `b` is held on gamepad `p`.
    bool gamepad_down(gamepad p, gamepad_button b) const { bool v = false; detail::check(ponko_ctx_gamepad_down(raw_, p.slot, b, &v)); return v; }
    /// Whether button `b` went down on gamepad `p` this frame.
    bool gamepad_pressed(gamepad p, gamepad_button b) const { bool v = false; detail::check(ponko_ctx_gamepad_pressed(raw_, p.slot, b, &v)); return v; }
    /// Whether button `b` went up on gamepad `p` this frame.
    bool gamepad_released(gamepad p, gamepad_button b) const { bool v = false; detail::check(ponko_ctx_gamepad_released(raw_, p.slot, b, &v)); return v; }
    /// Sticks -1..1 (y down), triggers 0..1.
    float gamepad_axis(gamepad p, gamepad_axis a) const { float v = 0; detail::check(ponko_ctx_gamepad_axis(raw_, p.slot, a, &v)); return v; }
    /// A stick as a vector, y down: add it to a position to move that way.
    vec2 gamepad_stick(gamepad p, stick s) const {
        bool left = s == stick::left;
        return {gamepad_axis(p, left ? PONKO_GAMEPAD_LEFT_STICK_X : PONKO_GAMEPAD_RIGHT_STICK_X),
                gamepad_axis(p, left ? PONKO_GAMEPAD_LEFT_STICK_Y : PONKO_GAMEPAD_RIGHT_STICK_Y)};
    }
    /// Starts `r` on gamepad `p`, replacing what it played; stops after `r.seconds` of real time.
    void rumble(gamepad p, ponko::rumble r) {
        detail::check(ponko_ctx_gamepad_rumble(raw_, p.slot, r.strong, r.weak, r.left_trigger, r.right_trigger, r.seconds));
    }
    /// Stops gamepad `p`'s rumble now.
    void stop_rumble(gamepad p) { detail::check(ponko_ctx_gamepad_stop_rumble(raw_, p.slot)); }
    /// The motor levels gamepad `p` is asked to play now.
    motor_levels rumble_levels(gamepad p) const { motor_levels m{}; detail::check(ponko_ctx_gamepad_rumble_levels(raw_, p.slot, &m)); return m; }
    /// What gamepad `p` is (family, ids, capabilities); empty for a slot with no described pad.
    std::optional<ponko::gamepad_info> gamepad_info(gamepad p) const {
        ponko::gamepad_info i{}; bool connected = false;
        detail::check(ponko_ctx_gamepad_info(raw_, p.slot, &i, &connected));
        return connected ? std::optional<ponko::gamepad_info>(i) : std::nullopt;
    }
    /// The name the system gives gamepad `p` ("" for none).
    std::string gamepad_name(gamepad p) const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_gamepad_name(raw_, p.slot, b, c, n); });
    }
    /// Overrides gamepad `p`'s prompt family until it disconnects (a "button style" option).
    void set_gamepad_family(gamepad p, controller_family f) { detail::check(ponko_ctx_set_gamepad_family(raw_, p.slot, static_cast<std::uint32_t>(f))); }
    /// The latest gyro and accelerometer reading of gamepad `p`, if it sent one.
    std::optional<ponko::gamepad_motion> gamepad_motion(gamepad p) const {
        ponko::gamepad_motion m; bool has = false;
        detail::check(ponko_ctx_gamepad_motion(raw_, p.slot, &m.gyro, &m.accel, &has));
        return has ? std::optional<ponko::gamepad_motion>(m) : std::nullopt;
    }
    /// Feeds a motion reading of gamepad `p` (tests and replays).
    void feed_gamepad_motion(gamepad p, ponko::gamepad_motion m) { detail::check(ponko_ctx_feed_gamepad_motion(raw_, p.slot, m.gyro, m.accel)); }
    /// The device used last: prompts follow it.
    input_device last_device() const { input_device d; detail::check(ponko_ctx_last_device(raw_, &d.kind, &d.slot, nullptr)); return d; }
    /// Whether last_device changed this frame.
    bool last_device_changed() const { input_device d; bool changed = false; detail::check(ponko_ctx_last_device(raw_, &d.kind, &d.slot, &changed)); return changed; }
    /// The gamepads that connected this frame.
    gamepad_list gamepads_connected() const { return gamepads_changed(false); }
    /// The gamepads that disconnected this frame.
    gamepad_list gamepads_disconnected() const { return gamepads_changed(true); }
    /// Gives player `player` the device `d`, taking it from any other player.
    void assign_player(std::uint32_t player, input_device d) { detail::check(ponko_ctx_assign_player(raw_, player, d.kind, d.slot)); }
    /// Takes player `player`'s device away.
    void unassign_player(std::uint32_t player) { detail::check(ponko_ctx_unassign_player(raw_, player)); }
    /// The device player `player` holds, if any.
    std::optional<input_device> player_device(std::uint32_t player) const {
        input_device d; bool connected = false, has = false;
        detail::check(ponko_ctx_player_device(raw_, player, &d.kind, &d.slot, &connected, &has));
        return has ? std::optional<input_device>(d) : std::nullopt;
    }
    /// Whether player `player`'s device is there now (a pad while plugged in).
    bool player_connected(std::uint32_t player) const {
        input_device d; bool connected = false, has = false;
        detail::check(ponko_ctx_player_device(raw_, player, &d.kind, &d.slot, &connected, &has));
        return connected;
    }
    /// The lowest player number with no device, if any.
    std::optional<std::uint32_t> free_player() const {
        std::uint32_t p = 0; bool has = false; detail::check(ponko_ctx_free_player(raw_, &p, &has));
        return has ? std::optional<std::uint32_t>(p) : std::nullopt;
    }
    /// A pad no player holds that pressed `b` this frame: "press Start to join".
    std::optional<gamepad> join_pressed(gamepad_button b) const {
        std::uint32_t slot = 0; bool has = false; detail::check(ponko_ctx_join_pressed(raw_, b, &slot, &has));
        return has ? std::optional<gamepad>(gamepad{slot}) : std::nullopt;
    }
    /// The glyphs that show action `name`'s binding on the device used last.
    glyphs action_glyphs(const char* name) const { return glyphs_of(name, UINT32_MAX, 0); }
    /// The glyphs that show action `name`'s binding on device `d` (a player's own prompts).
    glyphs action_glyphs_for(const char* name, input_device d) const { return glyphs_of(name, d.kind, d.slot); }
    /// The atlas as a texture, uploaded on the first call.
    texture glyph_texture(glyph_atlas& atlas) { texture t{}; detail::check(ponko_glyph_atlas_texture(atlas.raw(), raw_, &t)); return t; }
    /// Draws a prompt row in window pixels: `g` as `height`-pixel squares from `pos`, then `label`.
    /// Returns the row's width.
    float draw_prompt(glyph_atlas& atlas, vec2 pos, float height, const glyphs& g, const char* label, color c = {1, 1, 1, 1}) {
        float width = 0;
        detail::check(ponko_glyph_atlas_draw_prompt(atlas.raw(), raw_, pos, height, g.items.data(), g.count, label, c, &width));
        return width;
    }

    // Textures and drawing.
    /// Whether `path` (as loaded) was reloaded at the start of this frame.
    bool was_reloaded(const char* path) const { bool b = false; detail::check(ponko_ctx_was_reloaded(raw_, path, &b)); return b; }
    /// Loads a JSON file from assets/ into resource `T` (described with
    /// world().reflect_resource; fields merge, and it's added if missing).
    /// With hot reload on, edits to the file show up live.
    template<class T> void load_json(const char* path) { try_load_json<T>(path).value(); }
    /// load_json, saying why not instead of throwing (Rust `try_load_resource`'s native form:
    /// a C++ type is reflected by name, so the file's fields merge into the resource).
    template<class T> result<void> try_load_json(const char* path) {
        return detail::attempt(ponko_ctx_load_resource_json(raw_, path, detail::type_name<T>().c_str()));
    }
    /// Loads a JSON file from assets/ into reflected component `T` of `e`
    /// (fields merge); with hot reload on, edits to the file show up live.
    template<class T> void load_json(const char* path, entity e) { try_load_json<T>(path, e).value(); }
    /// load_json for a component, saying why not instead of throwing.
    template<class T> result<void> try_load_json(const char* path, entity e) {
        return detail::attempt(ponko_ctx_load_json(raw_, path, e, id<T>()));
    }
    // Scenes and prefabs: worlds and entities kept as JSON under assets/.
    /// Replaces the world with the scene in assets/`path`; ids are kept.
    void load_scene(const char* path) { detail::check(ponko_ctx_load_scene(raw_, path)); }
    /// load_scene, named for a lockstep game: call it from a sim system or
    /// setup, which every peer runs. Both throw from a frame system in a session.
    void load_scene_lockstep(const char* path) { detail::check(ponko_ctx_load_scene_lockstep(raw_, path)); }
    /// Saves the world as a scene in assets/`path`.
    void save_scene(const char* path) { detail::check(ponko_ctx_save_scene(raw_, path)); }
    /// Spawns the prefab in assets/`path` with its position at `at`.
    entity spawn_prefab(const char* path, vec2 at) { entity e = 0; detail::check(ponko_ctx_spawn_prefab(raw_, path, at, &e)); return e; }
    // The player's files (Rust's ctx.storage): saves and settings in the
    // per-user data folder (browser storage on the web), written safely.
    // Needs game::app_id; every call throws ponko::error without one.
    /// Writes `size` bytes as the stored file `name`: the old file stays
    /// whole until the new one is complete, then becomes its backup.
    void storage_write(const char* name, const void* bytes, std::size_t size) {
        detail::check(ponko_ctx_storage_write(raw_, name, static_cast<const std::uint8_t*>(bytes), size));
    }
    /// storage_write for text.
    void storage_write(const char* name, const std::string& text) { storage_write(name, text.data(), text.size()); }
    /// The stored file `name`, or nothing if there is no such file.
    std::optional<std::vector<std::uint8_t>> storage_read(const char* name) {
        return read_stored([&](std::uint8_t* b, std::size_t c, std::size_t* n) { return ponko_ctx_storage_read(raw_, name, b, c, n); });
    }
    /// The stored file `name` as it was before its last write, or nothing.
    std::optional<std::vector<std::uint8_t>> storage_read_backup(const char* name) {
        return read_stored([&](std::uint8_t* b, std::size_t c, std::size_t* n) { return ponko_ctx_storage_read_backup(raw_, name, b, c, n); });
    }
    /// The stored file `name` as text, or nothing if there is no such file.
    std::optional<std::string> storage_read_text(const char* name) {
        auto bytes = storage_read(name);
        if (!bytes) return std::nullopt;
        return std::string(bytes->begin(), bytes->end());
    }
    /// Whether the stored file `name` exists.
    bool storage_exists(const char* name) { bool e = false; detail::check(ponko_ctx_storage_exists(raw_, name, &e)); return e; }
    /// Every stored file's name, sorted (backups aren't listed).
    std::vector<std::string> storage_list() {
        const std::string text = detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_storage_list(raw_, b, c, n); });
        std::vector<std::string> names;
        std::size_t start = 0;
        while (start < text.size()) {
            const std::size_t end = std::min(text.find('\n', start), text.size());
            names.emplace_back(text, start, end - start);
            start = end + 1;
        }
        return names;
    }
    /// Deletes the stored file `name` and its backup (a missing file is fine).
    void storage_delete(const char* name) { detail::check(ponko_ctx_storage_delete(raw_, name)); }
    /// The folder the files are in; empty on the web and without an app id.
    std::string storage_dir() {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_storage_dir(raw_, b, c, n); });
    }
    /// The crash reports in the game's storage, newest first (Rust `ponko::crash::saved_reports`):
    /// read one with storage_read_text, delete it with storage_delete.
    std::vector<std::string> saved_crash_reports() {
        const std::string text = detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_saved_crash_reports(raw_, b, c, n); });
        std::vector<std::string> names;
        std::size_t start = 0;
        while (start < text.size()) {
            const std::size_t end = std::min(text.find('\n', start), text.size());
            names.emplace_back(text, start, end - start);
            start = end + 1;
        }
        return names;
    }
    /// Saves the world's reflected components and resources in save slot `slot`.
    void save_world(const char* slot) { detail::check(ponko_ctx_save_world(raw_, slot)); }
    /// Replaces the world with save slot `slot` (as load_scene does).
    void load_world(const char* slot) { detail::check(ponko_ctx_load_world(raw_, slot)); }
    /// Loads every old save (`*.json`) in the folder `folder` through the
    /// game's migrations, saves and loads each again, and returns how many
    /// loaded (Rust `Ctx::check_old_saves`). Throws naming every file that failed.
    std::size_t check_old_saves(const char* folder) { std::size_t n = 0; detail::check(ponko_ctx_check_old_saves(raw_, folder, &n)); return n; }
    /// The save slots save_world wrote, sorted (Rust `Storage::world_slots`).
    std::vector<std::string> storage_world_slots() {
        const std::string text = detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_storage_world_slots(raw_, b, c, n); });
        std::vector<std::string> slots;
        std::size_t start = 0;
        while (start < text.size()) {
            const std::size_t end = std::min(text.find('\n', start), text.size());
            slots.emplace_back(text, start, end - start);
            start = end + 1;
        }
        return slots;
    }
    /// Deletes save slot `slot` and its backup (a missing slot is fine).
    void delete_world(const char* slot) { detail::check(ponko_ctx_delete_world(raw_, slot)); }
    /// Whether stored files outlive the run: true for a folder; on the web when the page's database opened.
    bool storage_is_persistent() { bool p = false; detail::check(ponko_ctx_storage_is_persistent(raw_, &p)); return p; }
    /// Whether writes are still on their way to the disk (on the web); always false on the desktop.
    bool storage_is_saving() { bool s = false; detail::check(ponko_ctx_storage_is_saving(raw_, &s)); return s; }
private:
    /// Reads bytes a C function writes into a buffer (asks for the size, then
    /// fills it; asks again if it grew); nothing for PONKO_NOT_FOUND.
    template<class F> static std::optional<std::vector<std::uint8_t>> read_stored(F&& fill) {
        std::vector<std::uint8_t> bytes;
        for (;;) {
            std::size_t len = 0;
            const auto status = fill(bytes.empty() ? nullptr : bytes.data(), bytes.size(), &len);
            if (status == PONKO_NOT_FOUND) return std::nullopt;
            detail::check(status);
            if (len <= bytes.size()) { bytes.resize(len); return bytes; }
            bytes.resize(len);
        }
    }
public:
    /// Repositions a 3D prefab while preserving saved rotation and scale.
    entity spawn_prefab3d(const char* path, vec3 at) { entity e = 0; detail::check(ponko_ctx_spawn_prefab3d(raw_, path, at, &e)); return e; }
    /// Places a prefab with a complete transform. Separate names preserve old brace-init calls.
    entity spawn_prefab_transform3d(const char* path, transform3d at) { entity e = 0; detail::check(ponko_ctx_spawn_prefab_transform3d(raw_, path, at, &e)); return e; }
    /// Spawns the prefab in assets/`path` with a complete 2D transform (position, rotation and scale).
    entity spawn_prefab_transform2d(const char* path, transform2d at) { entity e = 0; detail::check(ponko_ctx_spawn_prefab_transform2d(raw_, path, at, &e)); return e; }
    /// Saves `e` as a prefab in assets/`path` (without its position).
    void save_prefab(entity e, const char* path) { detail::check(ponko_ctx_save_prefab(raw_, e, path)); }
    // Prefabs with children: the parts spawned with a prefab's root.
    /// The parts spawn_prefab made with prefab root `root` (empty for one entity).
    std::vector<entity> prefab_parts(entity root) {
        std::size_t n = 0;
        detail::check(ponko_ctx_prefab_parts(raw_, root, nullptr, 0, &n));
        std::vector<entity> parts(n);
        if (n != 0) detail::check(ponko_ctx_prefab_parts(raw_, root, parts.data(), parts.size(), &n));
        parts.resize(n < parts.size() ? n : parts.size());
        return parts;
    }
    /// Despawns prefab root `root` with its parts; returns how many entities went.
    std::size_t despawn_prefab(entity root) { std::size_t n = 0; detail::check(ponko_ctx_despawn_prefab(raw_, root, &n)); return n; }
    /// Loads a texture; throws ponko::error (kind, path) if it can't.
    texture load_texture(const char* path) { return try_load_texture(path).value(); }
    /// Loads a texture, or says why not, without throwing (Rust's `try_load_texture`).
    result<texture> try_load_texture(const char* path) { texture t{}; const PonkoStatus status = ponko_ctx_try_load_texture(raw_, path, &t); return detail::attempt(status, t); }
    /// Changes the image limits from now on, for the whole process (see `game::image_limits`). They stay
    /// until set again; pictures already loaded stay as they are. Throws if either is 0.
    void set_image_limits(std::uint32_t max_side, std::uint64_t max_bytes) { detail::check(ponko_ctx_set_image_limits(raw_, max_side, max_bytes)); }
    /// The image limits in force: the longest side in pixels, and the most decoded bytes (RGBA8).
    std::pair<std::uint32_t, std::uint64_t> image_limits() const {
        std::uint32_t side = 0; std::uint64_t bytes = 0;
        detail::check(ponko_ctx_image_limits(raw_, &side, &bytes));
        return {side, bytes};
    }
    /// Gives asset source `source_id` image limits of its own (Rust's `set_source_image_limits`): pictures
    /// read from it (a mod pack, a download cache, a folder) are decoded under these, everything else under
    /// the game's. A refused picture is kind PONKO_ERROR_LIMIT and its message names the limits and the source.
    /// The source may be added later (the mounted mods' id is "mods"). Throws if either limit is 0 or the id is empty.
    void set_source_image_limits(const char* source_id, std::uint32_t max_side, std::uint64_t max_bytes) { detail::check(ponko_ctx_set_source_image_limits(raw_, source_id, max_side, max_bytes)); }
    /// Takes back a source's own image limits: its pictures follow the game's again.
    void clear_source_image_limits(const char* source_id) { detail::check(ponko_ctx_clear_source_image_limits(raw_, source_id)); }
    /// The image limits source `source_id` has of its own, or nothing if its pictures follow the game's.
    /// Read only; the control endpoint's `assets.image_limits` lists every source that has limits.
    std::optional<std::pair<std::uint32_t, std::uint64_t>> source_image_limits(const char* source_id) const {
        std::uint32_t side = 0; std::uint64_t bytes = 0; bool has = false;
        detail::check(ponko_ctx_source_image_limits(raw_, source_id, &side, &bytes, &has));
        if (!has) return std::nullopt;
        return std::make_pair(side, bytes);
    }
    /// Decodes one picture from `len` bytes (a PNG, JPEG, KTX2 or any other format this build reads) into a
    /// texture under limits of its own, not the game's (Rust's `try_decode_texture`): for a picture you
    /// don't trust. `name` is for messages. A picture past the limits is kind PONKO_ERROR_LIMIT, bytes
    /// that aren't a picture PONKO_ERROR_DECODE.
    result<texture> try_decode_texture(const std::uint8_t* bytes, std::size_t len, const char* name, std::uint32_t max_side, std::uint64_t max_bytes) {
        texture t{};
        const PonkoStatus status = ponko_ctx_try_decode_texture(raw_, bytes, len, name, max_side, max_bytes, &t);
        return detail::attempt(status, t);
    }
    /// `try_decode_texture` of a byte vector.
    result<texture> try_decode_texture(const std::vector<std::uint8_t>& bytes, const char* name, std::uint32_t max_side, std::uint64_t max_bytes) {
        return try_decode_texture(bytes.data(), bytes.size(), name, max_side, max_bytes);
    }

    // Asset sources: where load_* calls find bytes besides assets/.
    /// Adds a copy of `source`; it's asked before assets/. Add sources in
    /// setup, before loading what they hold. Ids are unique per game.
    void add_asset_source(const memory_source& source) { detail::check(ponko_ctx_add_memory_source(raw_, source.raw())); }
    /// Adds a copy of `source` that worker threads may read (Rust's
    /// `add_background_source`): asset groups and streaming read it off the game thread.
    void add_background_asset_source(const memory_source& source) {
        detail::check(ponko_ctx_add_background_memory_source(raw_, source.raw()));
    }

    // Mods: packs players add, mounted in a load order (Rust's ponko::mods).
    /// Checks the mod packs at `paths`, then mounts them in that order moved
    /// as their manifests' `requires` and `after` need: assets load from the
    /// mods first, the last mod with a name winning. A bad mod is refused
    /// whole and nothing is mounted; the ponko::error names the mod, the file
    /// (its path()) and the reason. Mount in setup, after reflecting components.
    void mount_mods(const std::vector<std::string>& paths) {
        std::vector<const char*> raw;
        raw.reserve(paths.size());
        for (const auto& p : paths) raw.push_back(p.c_str());
        detail::check(ponko_ctx_mount_mods(raw_, raw.data(), raw.size()));
    }
    /// One mod pack held in memory for mount_mods_from_memory: a name for
    /// errors and the pack's bytes (what `ponko pack` writes).
    struct mod_bytes {
        std::string name;
        std::vector<std::uint8_t> bytes;
    };
    /// As mount_mods, with packs held in memory (downloaded, built in): the
    /// bytes are copied, so `mods` may go right after.
    void mount_mods_from_memory(const std::vector<mod_bytes>& mods) {
        std::vector<PonkoModBytes> raw;
        raw.reserve(mods.size());
        for (const auto& m : mods) raw.push_back(PonkoModBytes{m.name.c_str(), m.bytes.data(), m.bytes.size()});
        detail::check(ponko_ctx_mount_mods_from_memory(raw_, raw.data(), raw.size()));
    }
    /// The mounted mods' ids, in load order (empty before mount_mods).
    std::vector<std::string> mod_ids() {
        std::size_t n = 0;
        detail::check(ponko_ctx_mod_count(raw_, &n));
        std::vector<std::string> ids(n);
        for (std::size_t i = 0; i < n; ++i)
            ids[i] = detail::read_text([&](char* b, std::size_t c, std::size_t* len) { return ponko_ctx_mod_id(raw_, i, b, c, len); });
        return ids;
    }
    /// The id of the mod asset `name` loads from, or nothing if it is the game's own.
    std::optional<std::string> mod_provider(const char* name) {
        std::size_t n = 0;
        const PonkoStatus status = ponko_ctx_mod_provider(raw_, name, nullptr, 0, &n);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return detail::read_text([&](char* b, std::size_t c, std::size_t* len) { return ponko_ctx_mod_provider(raw_, name, b, c, len); });
    }
    /// The mods' files whose names start with `prefix`, sorted, each once:
    /// what mods added under a folder ("enemies/").
    std::vector<std::string> mod_files_under(const char* prefix) {
        std::vector<std::string> files;
        for (std::size_t i = 0;; ++i) {
            std::size_t n = 0;
            const PonkoStatus status = ponko_ctx_mod_file_under(raw_, prefix, i, nullptr, 0, &n);
            if (status == PONKO_NOT_FOUND) return files;
            detail::check(status);
            files.push_back(detail::read_text([&](char* b, std::size_t c, std::size_t* len) {
                return ponko_ctx_mod_file_under(raw_, prefix, i, b, c, len);
            }));
        }
    }
    /// A hash of the mounted mods, their order and data (0 without mods):
    /// lockstep peers and replays compare it.
    std::uint64_t mods_fingerprint() { std::uint64_t f = 0; detail::check(ponko_ctx_mods_fingerprint(raw_, &f)); return f; }
    /// A function a code mod exports (needs the library built with `mods-wasm`): look it up once,
    /// call it with call_mod. Throws ponko::error: not found when no mod has code or no such function,
    /// unsupported when code mods aren't compiled in.
    PonkoModFn mod_function(const char* mod_id, const char* name) {
        PonkoModFn f{};
        detail::check(ponko_ctx_mod_function(raw_, mod_id, name, &f));
        return f;
    }
    /// Calls a mod function with numbers (mod_i32, mod_i64, mod_f32, mod_f64), with one call's fuel; its result,
    /// or nothing for a function without one. A call that runs out of fuel or memory or traps throws
    /// ponko::error naming the mod; the mod stays callable.
    std::optional<PonkoModValue> call_mod(PonkoModFn f, std::initializer_list<PonkoModValue> args = {}) {
        PonkoModValue result{};
        bool has = false;
        detail::check(ponko_ctx_mod_call(raw_, f, args.begin(), args.size(), &result, &has));
        if (!has) return std::nullopt;
        return result;
    }
    /// Lets the mods' get_field and set_field reach `e`'s reflected components (hand them `e` as an i64).
    void mod_expose(entity e) { detail::check(ponko_ctx_mod_expose_entity(raw_, e)); }
    /// Takes `e` back from the mods; whether it was exposed.
    bool mod_hide(entity e) { bool was = false; detail::check(ponko_ctx_mod_hide_entity(raw_, e, &was)); return was; }
    /// Lets the mods spawn the prefab at `path` with spawn_prefab.
    void mod_expose_prefab(const char* path) { detail::check(ponko_ctx_mod_expose_prefab(raw_, path)); }
    /// Seeds every mod's random: same seed, same sequence.
    void mod_seed_random(std::uint64_t seed) { detail::check(ponko_ctx_mod_seed_random(raw_, seed)); }
    /// Adds a source whose reads are `read(name)` (Rust's `AssetSource`):
    /// the bytes, or nothing if this source doesn't have `name` (the next
    /// source is asked). An exception from `read` is a read error. It runs
    /// inside the load call that needs it and must not call the engine.
    template<class F> void add_asset_source(const char* id, F read) {
        struct holder { F read; std::vector<std::uint8_t> last; };
        auto h = std::make_unique<holder>(holder{std::move(read), {}});
        auto on_read = [](void* user, const char* name, const std::uint8_t** data, std::size_t* len) noexcept -> std::uint32_t {
            auto& self = *static_cast<holder*>(user);
            try {
                std::optional<std::vector<std::uint8_t>> bytes = self.read(name);
                if (!bytes) return PONKO_NOT_FOUND;
                self.last = std::move(*bytes);
                *data = self.last.data(); *len = self.last.size();
                return PONKO_OK;
            } catch (...) { return PONKO_PANIC; }
        };
        auto on_free = [](void* user) noexcept { delete static_cast<holder*>(user); };
        detail::check(ponko_ctx_add_asset_source(raw_, id, on_read, nullptr, on_free, h.get()));
        h.release();
    }
    /// Reads asset `name` (from the sources, then assets/) and returns
    /// `decode(data, len)`: your own format, with no registration (Rust's
    /// `assets.load_with`). Nothing is cached. Throws ponko::error if the
    /// asset can't be read, and what `decode` throws.
    template<class F> auto load_asset_with(const char* name, F decode) -> decltype(decode(std::declval<const std::uint8_t*>(), std::size_t{})) {
        using R = decltype(decode(std::declval<const std::uint8_t*>(), std::size_t{}));
        static_assert(!std::is_void<R>::value, "the decoder returns the decoded value");
        struct state { F* decode; std::optional<R> value; std::exception_ptr error; } st{&decode, std::nullopt, nullptr};
        auto trampoline = [](void* user, const std::uint8_t* data, std::size_t len) noexcept -> std::uint32_t {
            auto& s = *static_cast<state*>(user);
            try { s.value.emplace((*s.decode)(data, len)); return PONKO_OK; }
            catch (...) { s.error = std::current_exception(); return PONKO_INVALID_ARGUMENT; }
        };
        const auto status = ponko_ctx_load_asset_with(raw_, name, trampoline, &st);
        if (st.error) std::rethrow_exception(st.error);
        detail::check(status);
        return std::move(*st.value);
    }
    /// A texture `w` x `h` pixels from `len` = w * h * 4 RGBA bytes (Rust `Graphics::create_texture`). Throws
    /// ponko::error if `len` doesn't match.
    texture create_texture(std::uint32_t w, std::uint32_t h, const std::uint8_t* rgba, std::size_t len) {
        texture t{}; detail::check(ponko_ctx_create_texture(raw_, w, h, rgba, len, &t)); return t;
    }
    /// Replaces texture `t`'s pixels (same size, width * height * 4 RGBA bytes). Throws ponko::error if `len`
    /// doesn't match, `t` was freed, or `t` is a render target's texture (its camera draws it).
    void update_texture(texture t, const std::uint8_t* rgba, std::size_t len) { detail::check(ponko_ctx_update_texture(raw_, t, rgba, len)); }
    /// The background color of this frame.
    void clear(color c) { detail::check(ponko_ctx_clear(raw_, c)); }
    /// Draws texture `t` centered at `pos` (world units) this frame.
    void draw(texture t, vec2 pos) { detail::check(ponko_ctx_draw(raw_, t, pos)); }
    /// Draws texture `t` at `pos` with options (start from draw_defaults()).
    void draw(texture t, vec2 pos, const draw_params& params) { detail::check(ponko_ctx_draw_ex(raw_, t, pos, &params)); }
    // Sprite blend modes and sprite materials.
    /// Draws texture `t` at `pos` with options, blended by `blend` and drawn with `material` (an id of
    /// PONKO_HANDLE_NONE: the built-in look).
    void draw(texture t, vec2 pos, const draw_params& params, sprite_blend blend,
              sprite_material material = sprite_material{PONKO_HANDLE_NONE}) {
        detail::check(ponko_ctx_draw_styled(raw_, t, pos, &params, static_cast<std::uint32_t>(blend), material));
    }
    /// Loads a sprite shader (WGSL) from assets/; with hot reload on, saving the file updates the game and an
    /// edit that doesn't compile keeps the last good one. Throws ponko::error if it can't be read or doesn't
    /// compile (the message says where; shader_status() lists it).
    ponko::sprite_shader load_sprite_shader(const char* path) {
        ponko::sprite_shader x{}; detail::check(ponko_ctx_load_sprite_shader(raw_, path, &x)); return x;
    }
    /// load_sprite_shader that returns the error instead of throwing.
    result<ponko::sprite_shader> try_load_sprite_shader(const char* path) {
        ponko::sprite_shader x{}; const PonkoStatus status = ponko_ctx_load_sprite_shader(raw_, path, &x); return detail::attempt(status, x);
    }
    /// Makes a sprite shader from WGSL source; `name` is for error messages.
    ponko::sprite_shader create_sprite_shader(const char* name, const char* source) {
        ponko::sprite_shader x{}; detail::check(ponko_ctx_create_sprite_shader(raw_, name, source, &x)); return x;
    }
    /// create_sprite_shader that returns the error instead of throwing.
    result<ponko::sprite_shader> try_create_sprite_shader(const char* name, const char* source) {
        ponko::sprite_shader x{}; const PonkoStatus status = ponko_ctx_create_sprite_shader(raw_, name, source, &x); return detail::attempt(status, x);
    }
    /// Makes a sprite material. Throws ponko::error for a shader this game didn't make.
    ponko::sprite_material create_sprite_material(const sprite_material_desc& d) {
        ponko::sprite_material m{}; detail::check(ponko_ctx_create_sprite_material(raw_, &d, &m)); return m;
    }
    /// Changes what a sprite material is made of; cheap enough every frame. Nothing for a freed one.
    void update_sprite_material(ponko::sprite_material m, const sprite_material_desc& d) {
        detail::check(ponko_ctx_update_sprite_material(raw_, m, &d));
    }
    /// What a sprite material is made of now; nothing once it's freed.
    std::optional<sprite_material_desc> sprite_material_desc_of(ponko::sprite_material m) {
        sprite_material_desc d{}; bool found = false;
        detail::check(ponko_ctx_sprite_material_desc(raw_, m, &d, &found));
        if (!found) return std::nullopt;
        return d;
    }
    /// Draws UTF-8 text with its top-left at `pos` in window pixels, `size` pixels tall, in the built-in
    /// font.
    void draw_text(const char* text, vec2 pos, float size, color c) { detail::check(ponko_ctx_draw_text(raw_, text, pos, size, c)); }
    /// The size in pixels draw_text would cover with the built-in font.
    vec2 measure_text(const char* text, float size) { vec2 v{}; detail::check(ponko_ctx_measure_text(raw_, text, size, &v)); return v; }
    /// The 2D camera.
    ponko::camera camera() const { ponko::camera c{}; detail::check(ponko_ctx_camera(raw_, &c)); return c; }
    /// Moves or zooms the 2D camera. Throws ponko::error unless the zoom is positive.
    void set_camera(const ponko::camera& c) { detail::check(ponko_ctx_set_camera(raw_, &c)); }
    /// Marks entity `e` as the one the kept camera rigs follow (Rust `CameraTarget`), or takes the
    /// mark off: the engine moves each rig kept with its keep() to it every frame. Throws ponko::error
    /// for a dead entity.
    void set_camera_target(entity e, bool on = true) { detail::check(ponko_ctx_set_camera_target(raw_, e, on)); }
    /// The entity the kept rigs follow (the first marked one), if any.
    std::optional<entity> camera_target() const {
        entity e = 0; bool found = false;
        detail::check(ponko_ctx_camera_target(raw_, &e, &found));
        if (!found) return std::nullopt;
        return e;
    }
    /// The authenticated control endpoint's loopback port, when enabled.
    std::optional<std::uint16_t> control_port() const {
        std::uint16_t port = 0;
        auto status = ponko_ctx_control_port(raw_, &port);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return port;
    }
    /// The engine's default lens and render layer for held items.
    static PonkoViewModel view_model_defaults() {
        PonkoViewModel model{};
        model.struct_size = sizeof(model);
        model.version = PONKO_VIEW_MODEL_VERSION;
        detail::check(ponko_view_model_defaults(&model));
        return model;
    }
    /// Configures the held-item pass. No value disables the pass.
    void set_view_model(std::optional<PonkoViewModel> model) {
        detail::check(ponko_ctx_set_view_model(raw_, model ? &*model : nullptr));
    }
    /// The held-item pass configuration when enabled.
    std::optional<PonkoViewModel> view_model() const {
        PonkoViewModel model{};
        model.struct_size = sizeof(model);
        model.version = PONKO_VIEW_MODEL_VERSION;
        const auto status = ponko_ctx_view_model(raw_, &model);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return model;
    }
    /// Sets the camera-space offset for an entity. No value removes the held-item mark.
    void set_held_item(entity e, std::optional<transform3d> offset) {
        detail::check(ponko_ctx_set_held_item(raw_, e, offset ? &*offset : nullptr));
    }
    /// An entity's held-item offset, if present.
    std::optional<transform3d> held_item(entity e) const {
        transform3d offset{};
        const auto status = ponko_ctx_held_item(raw_, e, &offset);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return offset;
    }
    /// Multiplies the 2D zoom by `factor`, keeping the world point under window pixel `screen` where it is
    /// (Rust `Camera::zoom_at`): zooming at the pointer. Throws ponko::error unless `factor` is positive.
    void camera_zoom_at(vec2 screen, float factor) { detail::check(ponko_ctx_camera_zoom_at(raw_, screen, factor)); }

    // 3D: meshes, materials, the camera and the sun.
    /// Puts a material on the GPU (roughness and metallic 0 to 1).
    material create_material(const material_desc& d) { material m{}; detail::check(ponko_ctx_create_material(raw_, &d, &m)); return m; }
    /// Changes a material: every model drawn with it changes too.
    void update_material(material m, const material_desc& d) { detail::check(ponko_ctx_update_material(raw_, m, &d)); }
    /// A material with a stencil state (see the guide's "Stencil").
    material create_material(const material_desc& d, const stencil& s) { material m{}; detail::check(ponko_ctx_create_material_with_stencil(raw_, &d, &s, &m)); return m; }
    /// Changes a material and its stencil state; update_material without one takes it off.
    void update_material(material m, const material_desc& d, const stencil& s) { detail::check(ponko_ctx_update_material_with_stencil(raw_, m, &d, &s)); }
    /// The materials loaded from glTF files so far that the files call
    /// `name`, with what each was made from: to give them a shader of your
    /// own with update_material.
    std::vector<std::pair<material, material_desc>> loaded_materials(const char* name) {
        std::size_t n = 0;
        detail::check(ponko_ctx_loaded_materials(raw_, name, nullptr, nullptr, 0, &n));
        std::vector<material> ms(n);
        std::vector<material_desc> ds(n);
        detail::check(ponko_ctx_loaded_materials(raw_, name, ms.data(), ds.data(), n, &n));
        std::vector<std::pair<material, material_desc>> out;
        for (std::size_t i = 0; i < ms.size() && i < n; i++) out.emplace_back(ms[i], ds[i]);
        return out;
    }
    /// Loads a material shader (WGSL) from assets/: it may define
    /// `fn vertex(v: Vertex) -> Vertex` and `fn surface(s: Surface) -> Surface`.
    /// Hot reloaded; throws if it doesn't compile (the message says where).
    /// What happened to each of the game's shaders the last time it was
    /// compiled (oldest first): whether an edit took effect, and where an
    /// error is in its own WGSL.
    std::vector<ponko::shader_status> shader_status() const {
        std::size_t count = 0;
        detail::check(ponko_ctx_shader_status_count(raw_, &count));
        std::vector<ponko::shader_status> list(count);
        for (std::size_t i = 0; i < count; ++i) {
            detail::check(ponko_ctx_shader_status(raw_, i, &list[i].info));
            list[i].shader = detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_shader_status_name(raw_, i, b, c, n); });
            list[i].errors = detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_shader_status_text(raw_, i, b, c, n); });
        }
        return list;
    }
    /// shader_status() as JSON, as the `shaders.status` control method returns it.
    std::string shader_status_json() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_shader_status_json(raw_, b, c, n); });
    }
    /// What this running game can do, as JSON: the control endpoint's
    /// `capabilities` answer (features, mode, subsystems, methods, and every
    /// reflected type with its described schema). Reading it changes nothing.
    std::string capabilities_json() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_capabilities_json(raw_, b, c, n); });
    }
    /// Loads a material shader (WGSL) from assets/; with hot reload on, saving the file updates the game.
    /// Throws ponko::error if it can't be read or doesn't compile (the message says where).
    ponko::shader load_shader(const char* path) { ponko::shader x{}; detail::check(ponko_ctx_load_shader(raw_, path, &x)); return x; }
    /// load_shader that returns the error instead of throwing.
    result<ponko::shader> try_load_shader(const char* path) { ponko::shader x{}; const PonkoStatus status = ponko_ctx_load_shader(raw_, path, &x); return detail::attempt(status, x); }
    /// Makes a material shader from WGSL source; `name` is for error messages.
    ponko::shader create_shader(const char* name, const char* source) {
        ponko::shader x{}; detail::check(ponko_ctx_create_shader(raw_, name, source, &x)); return x;
    }
    /// Loads a screen effect (WGSL defining `fn effect(p: Pixel) -> vec3<f32>`)
    /// from assets/, hot reloaded; the same file again gives the same effect.
    ponko::effect load_effect(const char* path) { ponko::effect e{}; detail::check(ponko_ctx_load_effect(raw_, path, &e)); return e; }
    /// load_effect that returns the error instead of throwing.
    result<ponko::effect> try_load_effect(const char* path) { ponko::effect e{}; const PonkoStatus status = ponko_ctx_load_effect(raw_, path, &e); return detail::attempt(status, e); }
    /// A screen effect from WGSL source; `name` is for error messages.
    ponko::effect create_effect(const char* name, const char* source) {
        ponko::effect e{}; detail::check(ponko_ctx_create_effect(raw_, name, source, &e)); return e;
    }
    /// The screen effects run over the lit 3D frame each frame, in order.
    std::vector<screen_effect> effects() const {
        std::size_t n = 0;
        detail::check(ponko_ctx_effects(raw_, nullptr, 0, &n));
        std::vector<screen_effect> list(n);
        detail::check(ponko_ctx_effects(raw_, list.data(), list.size(), &n));
        return list;
    }
    /// Replaces the screen effects run each frame (empty: none).
    void set_effects(const std::vector<screen_effect>& list) { detail::check(ponko_ctx_set_effects(raw_, list.data(), list.size())); }
    /// Zeroed or initialized GPU vec4 storage, updated without CPU particle work.
    instance_batch create_instance_batch(mesh m, const std::vector<std::array<float,16>>& matrices,
        instance_batch_options options = {32.0f, std::numeric_limits<float>::infinity(), true, colors::white}) {
        instance_batch b{};
        detail::check(ponko_ctx_create_instance_batch(raw_,m,reinterpret_cast<const float (*)[16]>(matrices.data()),matrices.size(),options,&b));
        return b;
    }
    /// Replaces a batch's instance transforms (column-major 4x4 matrices) and rebuilds its culling cells.
    void update_instance_batch(instance_batch b, const std::vector<std::array<float,16>>& matrices) {
        detail::check(ponko_ctx_update_instance_batch(raw_,b,reinterpret_cast<const float (*)[16]>(matrices.data()),matrices.size()));
    }
    /// A batch of several LOD `levels` (Rust `InstanceBatch::with_levels`): each instance draws the level
    /// whose distance band holds it. `colors` is empty or one tint per instance. Throws ponko::error for
    /// invalid levels or a count mismatch.
    instance_batch create_instance_batch_levels(const std::vector<instance_level>& levels,
        const std::vector<std::array<float,16>>& matrices, const std::vector<color>& colors = {},
        instance_batch_options options = {32.0f, std::numeric_limits<float>::infinity(), true, colors::white}) {
        instance_batch b{};
        detail::check(ponko_ctx_create_instance_batch_levels(raw_, levels.data(), levels.size(),
            reinterpret_cast<const float (*)[16]>(matrices.data()), matrices.size(), colors.data(), colors.size(), options, &b));
        return b;
    }
    /// Replaces a batch's transforms and colors; an empty `colors` (or a zero color) goes back to the batch
    /// tint.
    void update_instance_batch_colors(instance_batch b, const std::vector<std::array<float,16>>& matrices,
        const std::vector<color>& colors = {}) {
        detail::check(ponko_ctx_update_instance_batch_colors(raw_, b,
            reinterpret_cast<const float (*)[16]>(matrices.data()), matrices.size(), colors.data(), colors.size()));
    }
    /// 0 = automatic, 1 = prefer GPU, 2 = prefer CPU; device limits still apply.
    void set_instance_batch_culling(instance_batch b, std::uint32_t culling) {
        detail::check(ponko_ctx_set_instance_batch_culling(raw_, b, culling));
    }
    /// Frees a batch; freeing it again does nothing.
    void free_instance_batch(instance_batch b) { detail::check(ponko_ctx_free_instance_batch(raw_,b)); }
    /// Bakes `clips` on `skeleton`'s skeleton at `fps` frames a second (Rust `CrowdAnimation::bake`): clip `k`
    /// of the list is index `k`.
    crowd_animation bake_crowd_animation(entity skeleton, const std::vector<crowd_clip>& clips, float fps = 30) {
        PonkoCrowdAnimation* out = nullptr;
        detail::check(ponko_ctx_bake_crowd_animation(raw_, skeleton, clips.data(), clips.size(), fps, &out));
        return crowd_animation(out);
    }
    /// A crowd of `people` drawn by `levels` (Rust `Graphics::create_crowd`). Throws ponko::error for a freed or
    /// unskinned mesh, a bad band or influence count, or a character the engine rejects.
    crowd create_crowd(const crowd_animation& animation, const std::vector<crowd_level>& levels,
        const std::vector<crowd_instance>& people, crowd_options options = {true}) {
        crowd c{};
        detail::check(ponko_ctx_create_crowd(raw_, animation.raw(), levels.data(), levels.size(), people.data(),
            people.size(), options, &c));
        return c;
    }
    /// The same with crossfades between the levels (Rust `CrowdLevel::with_fade`): `fades[k]` is how far
    /// before level `k`'s far distance it dithers into the next, 0 to switch at once; one per level. Throws
    /// ponko::error for a fade that isn't finite or is wider than its level's band, and as create_crowd does.
    crowd create_crowd(const crowd_animation& animation, const std::vector<crowd_level>& levels,
        const std::vector<float>& fades, const std::vector<crowd_instance>& people, crowd_options options = {true}) {
        if (fades.size() != levels.size()) throw std::invalid_argument("create_crowd: give one fade per level");
        PonkoCrowdDesc d = ponko_crowd_desc_defaults();
        d.animation = animation.raw();
        d.levels = levels.data();
        d.level_count = levels.size();
        d.fades = fades.data();
        d.instances = people.data();
        d.count = people.size();
        d.cast_shadows = options.cast_shadows;
        crowd c{};
        detail::check(ponko_ctx_create_crowd_ex(raw_, &d, &c));
        return c;
    }
    /// Replaces a crowd's characters (Rust `Graphics::update_crowd`), as often as every frame. Throws
    /// ponko::error for a freed crowd or a rejected character; the crowd then keeps its characters.
    void update_crowd(crowd c, const std::vector<crowd_instance>& people) {
        detail::check(ponko_ctx_update_crowd(raw_, c, people.data(), people.size()));
    }
    /// How many characters a crowd has; std::nullopt for a freed one.
    std::optional<std::size_t> crowd_len(crowd c) const {
        std::size_t n = 0;
        const PonkoStatus status = ponko_ctx_crowd_len(raw_, c, &n);
        if (status == PONKO_INVALID_ARGUMENT) return std::nullopt;
        detail::check(status);
        return n;
    }
    /// Frees a crowd; freeing it again does nothing.
    void free_crowd(crowd c) { detail::check(ponko_ctx_free_crowd(raw_, c)); }
    /// Adds a cell of instances to batch `b` and uploads it now, leaving the batch's other instances as they
    /// are (Rust `Graphics::add_instance_cell`). `colors` is empty (the batch tint) or one per matrix.
    /// Throws ponko::error for a freed batch or a count mismatch.
    instance_cell add_instance_cell(instance_batch b, const std::vector<std::array<float,16>>& matrices,
        const std::vector<color>& colors = {}) {
        instance_cell cell{};
        detail::check(ponko_ctx_add_instance_cell(raw_, b, reinterpret_cast<const float (*)[16]>(matrices.data()),
            matrices.size(), colors.data(), colors.size(), &cell));
        return cell;
    }
    /// Replaces a cell's instances and uploads them now; the batch's other cells stay as they are. Throws
    /// ponko::error for a freed cell.
    void replace_instance_cell(instance_cell cell, const std::vector<std::array<float,16>>& matrices,
        const std::vector<color>& colors = {}) {
        detail::check(ponko_ctx_replace_instance_cell(raw_, cell,
            reinterpret_cast<const float (*)[16]>(matrices.data()), matrices.size(), colors.data(), colors.size()));
    }
    /// How many instances a cell draws. Throws ponko::error for a freed cell.
    std::size_t instance_cell_len(instance_cell cell) {
        std::size_t n = 0; detail::check(ponko_ctx_instance_cell_len(raw_, cell, &n)); return n;
    }
    /// Frees a cell: only its instances stop drawing. Freeing it again does nothing.
    void free_instance_cell(instance_cell cell) { detail::check(ponko_ctx_free_instance_cell(raw_, cell)); }
    /// A GPU buffer of `len` four-float elements, zeroed or filled from `data` (empty, or exactly `len`
    /// elements). Throws ponko::error otherwise.
    gpu_buffer create_gpu_buffer(uint32_t len, const std::vector<std::array<float, 4>>& data = {}) {
        gpu_buffer b{}; detail::check(ponko_ctx_create_gpu_buffer(raw_, len,
            reinterpret_cast<const float (*)[4]>(data.data()), data.size(), &b)); return b;
    }
    /// Writes `data` into a GPU buffer from element `offset`. Throws ponko::error for a range past the end.
    void write_gpu_buffer(gpu_buffer b, uint32_t offset, const std::vector<std::array<float, 4>>& data) {
        detail::check(ponko_ctx_write_gpu_buffer(raw_, b, offset, reinterpret_cast<const float (*)[4]>(data.data()), data.size()));
    }
    /// Frees a GPU buffer; its compute passes are removed and meshes using
    /// it draw nothing. The next buffer made takes its id: drop the handle.
    void free_gpu_buffer(gpu_buffer b) { detail::check(ponko_ctx_free_gpu_buffer(raw_, b)); }
    /// Draws `count` instances of a mesh from a GPU buffer, each at its
    /// entity's transform and tint (`items[v.instance_index]` in the shader).
    void set_mesh_gpu_buffer(mesh m, gpu_buffer b, uint32_t count) {
        detail::check(ponko_ctx_set_mesh_gpu_buffer(raw_, m, b, count));
    }
    /// Grows a mesh's culling bounds by the sphere `center`, `radius`: where
    /// its buffer-driven instances can be.
    void set_mesh_gpu_bounds(mesh m, vec3 center, float radius) {
        detail::check(ponko_ctx_set_mesh_gpu_bounds(raw_, m, center, radius));
    }
    /// Loads a compute pass (WGSL defining `fn compute(c: Cell) -> vec4<f32>`)
    /// from assets/, hot reloaded; the same file again gives the same one.
    ponko::compute load_compute(const char* path) { ponko::compute c{}; detail::check(ponko_ctx_load_compute(raw_, path, &c)); return c; }
    /// load_compute that returns the error instead of throwing.
    result<ponko::compute> try_load_compute(const char* path) { ponko::compute c{}; const PonkoStatus status = ponko_ctx_load_compute(raw_, path, &c); return detail::attempt(status, c); }
    /// A compute pass from WGSL source; `name` is for error messages.
    ponko::compute create_compute(const char* name, const char* source) {
        ponko::compute c{}; detail::check(ponko_ctx_create_compute(raw_, name, source, &c)); return c;
    }
    /// The compute passes run each frame, in order, before anything is drawn.
    std::vector<compute_pass> compute_passes() const {
        std::size_t n = 0;
        detail::check(ponko_ctx_compute_passes(raw_, nullptr, 0, &n));
        std::vector<compute_pass> list(n);
        detail::check(ponko_ctx_compute_passes(raw_, list.data(), list.size(), &n));
        return list;
    }
    /// Replaces the compute passes run each frame (empty: none).
    void set_compute_passes(const std::vector<compute_pass>& list) {
        detail::check(ponko_ctx_set_compute_passes(raw_, list.data(), list.size()));
    }
    /// The scene-stage compute passes (Rust `ctx.scene3d.passes.scene_compute`): after the surfaces, before
    /// the effects, reading this frame's lit image and depth (`scene_color`, `scene_position`, `scene_distance`).
    std::vector<compute_pass> scene_compute_passes() const {
        std::size_t n = 0;
        detail::check(ponko_ctx_scene_compute_passes(raw_, nullptr, 0, &n));
        std::vector<compute_pass> list(n);
        detail::check(ponko_ctx_scene_compute_passes(raw_, list.data(), list.size(), &n));
        return list;
    }
    /// Replaces the scene-stage compute passes (empty: none).
    void set_scene_compute_passes(const std::vector<compute_pass>& list) {
        detail::check(ponko_ctx_set_scene_compute_passes(raw_, list.data(), list.size()));
    }
    /// The target effects (Rust `ctx.scene3d.passes.target_effects`): effects over the lit frame that write
    /// render targets instead of the frame, before the screen effects.
    std::vector<target_effect> target_effects() const {
        std::size_t n = 0;
        detail::check(ponko_ctx_target_effects(raw_, nullptr, 0, &n));
        std::vector<target_effect> list(n);
        detail::check(ponko_ctx_target_effects(raw_, list.data(), list.size(), &n));
        return list;
    }
    /// Replaces the target effects (empty: none). Throws ponko::error for a target that isn't a render target.
    void set_target_effects(const std::vector<target_effect>& list) {
        detail::check(ponko_ctx_set_target_effects(raw_, list.data(), list.size()));
    }
    /// A render target cameras draw the 3D world into (linear HDR light);
    /// materials use it like any texture.
    ponko::render_target create_render_target(std::uint32_t w, std::uint32_t h) {
        ponko::render_target t{}; detail::check(ponko_ctx_create_render_target(raw_, w, h, &t)); return t;
    }
    /// `t` as the render target it is: the `target` of a target_camera,
    /// compute_pass or target_effect read back from the engine is a plain
    /// texture. Throws ponko::error (PONKO_ERROR_INVALID) if `t` isn't a live
    /// render target of this game.
    ponko::render_target render_target_of(texture t) const {
        bool live = false;
        detail::check(ponko_ctx_handle_is_live(raw_, PONKO_HANDLE_RENDER_TARGET, t.id, &live));
        if (!live) throw ponko::error("that texture isn't a render target of this game", PONKO_ERROR_INVALID);
        ponko::render_target r{};
        static_cast<texture&>(r) = t;
        return r;
    }
    /// The target camera seeing `c`'s view mirrored in the level plane at
    /// height `level`, clipped to what's above it. Read its target at the
    /// screen position flipped upside down.
    target_camera reflection_camera(const camera3d& c, ponko::render_target target, float level) {
        target_camera t{}; detail::check(ponko_ctx_reflection_camera(raw_, &c, target, level, &t)); return t;
    }
    /// The cameras drawing into render targets each frame, before the main view.
    std::vector<target_camera> target_cameras() const {
        std::size_t n = 0;
        detail::check(ponko_ctx_target_cameras(raw_, nullptr, 0, &n));
        std::vector<target_camera> list(n);
        detail::check(ponko_ctx_target_cameras(raw_, list.data(), list.size(), &n));
        return list;
    }
    /// Replaces the target cameras drawn each frame (empty: none).
    void set_target_cameras(const std::vector<target_camera>& list) {
        detail::check(ponko_ctx_set_target_cameras(raw_, list.data(), list.size()));
    }
    /// The target camera `index` (counting from 0, up to target_cameras().size()) in its versioned form
    /// (target_camera_ex), with whether it draws particles and debug drawing. Throws for an index past the end.
    ponko::target_camera_ex target_camera_ex(std::size_t index) const {
        ponko::target_camera_ex t{};
        t.struct_size = sizeof(ponko::target_camera_ex);
        t.version = PONKO_TARGET_CAMERA_EX_VERSION;
        detail::check(ponko_ctx_target_camera_ex(raw_, index, &t));
        return t;
    }
    /// Replaces the target cameras drawn each frame from their versioned forms (empty: none): as
    /// set_target_cameras, each with its `particles` and `debug_draw`.
    void set_target_cameras_ex(const std::vector<ponko::target_camera_ex>& list) {
        std::vector<const PonkoTargetCameraEx*> pointers;
        pointers.reserve(list.size());
        for (const ponko::target_camera_ex& t : list) pointers.push_back(&t);
        detail::check(ponko_ctx_set_target_cameras_ex(raw_, pointers.data(), pointers.size()));
    }
    /// A plain white material, made once and shared.
    material default_material() { material m{}; detail::check(ponko_ctx_default_material(raw_, &m)); return m; }
    /// Puts triangles on the GPU as a mesh of one material; draw it with
    /// entities that have a transform3d and a model_of(it).
    mesh create_mesh(const mesh_data& data, material m) { mesh x{}; detail::check(ponko_ctx_create_mesh(raw_, data.raw(), m, &x)); return x; }
    /// A mesh with no material of its own (Rust `Ctx::create_plain_mesh`):
    /// create_mesh with the default material, for meshes whose look comes from
    /// each model (set_model_material).
    mesh create_plain_mesh(const mesh_data& data) { mesh x{}; detail::check(ponko_ctx_create_plain_mesh(raw_, data.raw(), &x)); return x; }
    /// Loads a glTF model (.glb, or .gltf with its files) from assets/ as
    /// one mesh with its materials; the same file again gives the same mesh.
    mesh load_mesh(const char* path) { return try_load_mesh(path).value(); }
    /// Loads a glTF model's first mesh, or says why not, without throwing (Rust's `try_load_mesh`).
    result<mesh> try_load_mesh(const char* path) { mesh x{}; const PonkoStatus status = ponko_ctx_try_load_mesh(raw_, path, &x); return detail::attempt(status, x); }
    /// A texture array of `width` x `height` images, one after another in
    /// `rgba` (4 bytes a pixel).
    texture_array create_texture_array(std::uint32_t width, std::uint32_t height, const std::vector<std::uint8_t>& rgba, texture_kind kind) {
        texture_array a{};
        detail::check(ponko_ctx_create_texture_array(raw_, width, height, rgba.data(), rgba.size(), std::uint32_t(kind), &a));
        return a;
    }
    /// Loads same-size images from assets/ into a texture array, one layer each.
    texture_array load_texture_array(const std::vector<const char*>& paths, texture_kind kind) {
        texture_array a{};
        detail::check(ponko_ctx_load_texture_array(raw_, paths.data(), paths.size(), std::uint32_t(kind), &a));
        return a;
    }
    /// The texture array decals take their images from (Rust `set_decal_textures`).
    /// Make it with texture_kind::data; decals decode colors from sRGB themselves.
    void set_decal_textures(texture_array a) { detail::check(ponko_ctx_set_decal_textures(raw_, a)); }
    /// Back to flat-colored decals.
    void clear_decal_textures() { detail::check(ponko_ctx_clear_decal_textures(raw_)); }
    /// Whether this GPU draws projected decals (false headless and on WebGL2).
    bool supports_decals() { bool yes = false; detail::check(ponko_ctx_supports_decals(raw_, &yes)); return yes; }
    /// Build the 3D pipeline variants frames would build when first drawn
    /// (crossfades, buffer-driven instances); call at the end of loading.
    void warm_render_pipelines() { detail::check(ponko_ctx_warm_render_pipelines(raw_)); }
    /// A material for terrain: ground layers blended by a weight map.
    material create_terrain_material(const terrain_material_desc& desc) {
        material m{}; detail::check(ponko_ctx_create_terrain_material(raw_, &desc, &m)); return m;
    }
    /// A terrain tile `size` wide from n x n heights (row after row along
    /// +Z, NaN for a hole) drawn with `m`; spawn it with a transform3d at
    /// the tile's corner instead of a model.
    terrain_tile create_terrain_tile(const std::vector<float>& heights, float size, material m) {
        terrain_tile t{}; detail::check(ponko_ctx_create_terrain_tile(raw_, heights.data(), heights.size(), size, m, &t)); return t;
    }
    /// Puts a tile built (on any thread) as terrain_tile_data on the GPU.
    terrain_tile upload_terrain_tile(const terrain_tile_data& data, material m) {
        terrain_tile t{}; detail::check(ponko_ctx_upload_terrain_tile(raw_, data.raw(), m, &t)); return t;
    }
    /// Frees every detail level of a tile's GPU memory; despawn its entity too.
    void free_terrain_tile(terrain_tile t) { detail::check(ponko_ctx_free_terrain_tile(raw_, t)); }
    /// Frees a mesh's GPU memory; models still showing it draw nothing.
    void free_mesh(mesh m) { detail::check(ponko_ctx_free_mesh(raw_, m)); }
    /// Captures the scene around reflection probe `e` on the next rendered frame
    /// (only when asked: bake again to refresh). Throws once four environments exist.
    void bake_reflection_probe(entity e) { detail::check(ponko_ctx_bake_reflection_probe(raw_, e)); }
    /// Loads a linear HDR/EXR 2:1 equirectangular image as probe `e`'s environment.
    void load_reflection_probe(entity e, const std::string& name) { detail::check(ponko_ctx_load_reflection_probe(raw_, e, name.c_str())); }
    /// Whether models showing `m` cast shadows (on at first); off suits grass and pebbles.
    void set_mesh_shadows(mesh m, bool casts) { detail::check(ponko_ctx_set_mesh_shadows(raw_, m, casts)); }
    /// Draws `e`'s model with `m` instead of the materials its mesh was made
    /// with, every part (Rust `ModelMaterial`): one uploaded mesh in several
    /// materials. For plain, skinned and morphing models, LOD groups and
    /// terrain tiles.
    void set_model_material(entity e, material m) { detail::check(ponko_ctx_set_model_material(raw_, e, m, true)); }
    /// `e`'s model draws with its mesh's own materials again.
    void clear_model_material(entity e) { detail::check(ponko_ctx_set_model_material(raw_, e, material{}, false)); }
    /// The material set with set_model_material, if any.
    std::optional<material> model_material(entity e) const {
        material m{}; bool has = false;
        detail::check(ponko_ctx_model_material(raw_, e, &m, &has));
        return has ? std::optional<material>(m) : std::nullopt;
    }
    /// Draws the listed parts of `e`'s model with other materials (Rust
    /// `ModelMaterials`): `{part, material}` pairs, parts numbered from 0 in
    /// the order the mesh was made with. Parts not listed keep the material of
    /// set_model_material, or the mesh's own. An empty list takes it off.
    void set_model_part_materials(entity e, const std::vector<std::pair<std::uint32_t, material>>& parts) {
        std::vector<std::uint32_t> numbers;
        std::vector<material> materials;
        for (const auto& [part, m] : parts) { numbers.push_back(part); materials.push_back(m); }
        detail::check(ponko_ctx_set_model_part_materials(raw_, e, numbers.data(), materials.data(), parts.size()));
    }
    /// The list set with set_model_part_materials, by part number (empty: none).
    std::vector<std::pair<std::uint32_t, material>> model_part_materials(entity e) const {
        std::size_t n = 0;
        detail::check(ponko_ctx_model_part_materials(raw_, e, nullptr, nullptr, 0, &n));
        std::vector<std::uint32_t> numbers(n);
        std::vector<material> materials(n);
        detail::check(ponko_ctx_model_part_materials(raw_, e, numbers.data(), materials.data(), n, &n));
        std::vector<std::pair<std::uint32_t, material>> parts;
        for (std::size_t i = 0; i < n && i < numbers.size(); ++i) parts.emplace_back(numbers[i], materials[i]);
        return parts;
    }
    /// Draws `m` into the shadow maps for `e` instead of its model (Rust `ShadowMesh`);
    /// without a model, `e` casts only that shadow.
    void set_shadow_mesh(entity e, mesh m) { detail::check(ponko_ctx_set_shadow_mesh(raw_, e, m, true)); }
    /// `e` casts no shadow (a model, or a terrain tile's ground).
    void set_no_shadow(entity e) { detail::check(ponko_ctx_set_shadow_mesh(raw_, e, mesh{}, false)); }
    /// Gives `e` levels of detail (Rust `LodGroup`), most detailed first, for an
    /// object `size` tall, its distance measured to `center` in its own space.
    void set_lod_group(entity e, float size, const std::vector<lod_level>& levels, vec3 center = {0, 0, 0}) {
        std::vector<PonkoLodLevel> raw;
        raw.reserve(levels.size());
        for (const lod_level& l : levels) {
            raw.push_back(PonkoLodLevel{l.screen_height, l.fade_width, l.renderers.data(), l.renderers.size(),
                                         l.custom_shadow, l.shadow_casters.data(), l.shadow_casters.size()});
        }
        detail::check(ponko_ctx_set_lod_group(raw_, e, size, center, raw.data(), raw.size()));
    }
    /// The level of `e`'s LOD group the 3D camera picks now.
    lod_pick lod_pick_of(entity e) const { lod_pick p{}; detail::check(ponko_ctx_lod_pick(raw_, e, &p)); return p; }
    /// The global LOD settings.
    lod_settings lod() const { lod_settings s{}; detail::check(ponko_ctx_lod_settings(raw_, &s)); return s; }
    /// bias 0 or more: 2 keeps detail twice as far, 0.5 drops it at half the distance.
    void set_lod(const lod_settings& s) { detail::check(ponko_ctx_set_lod_settings(raw_, s)); }
    /// Holds each level of `e`'s LOD group until it is `fraction` (0 to 1) past the level's range (Rust
    /// `LodHysteresis`), so a camera resting near a threshold doesn't switch every frame. 0: none.
    void set_lod_hysteresis(entity e, float fraction) { detail::check(ponko_ctx_set_lod_hysteresis(raw_, e, fraction)); }
    /// `e`'s LOD hysteresis; 0 when it has none.
    float lod_hysteresis(entity e) const { float f = 0; detail::check(ponko_ctx_lod_hysteresis(raw_, e, &f)); return f; }
    /// Puts a skeleton of `joints`, at rest, on `e` (Rust `Skeleton::new`).
    void set_skeleton(entity e, const std::vector<joint>& joints) {
        std::vector<PonkoJoint> raw;
        raw.reserve(joints.size());
        for (const joint& j : joints) raw.push_back(PonkoJoint{j.name.c_str(), j.parent, j.rest});
        detail::check(ponko_ctx_set_skeleton(raw_, e, raw.data(), raw.size()));
    }
    /// Puts the skeleton of a glTF file with skins on `e`, at rest (Rust `Graphics::load_skeleton`).
    void load_skeleton(entity e, const char* path) { detail::check(ponko_ctx_load_skeleton(raw_, e, path)); }
    /// Maps the humanoid bones of `e`'s skeleton by joint name (Rust `HumanoidMap::detect`). Check
    /// humanoid_map::missing and fix the rest with humanoid_map::set.
    humanoid_map detect_humanoid_map(entity e) {
        PonkoHumanoidMap* m = nullptr;
        detail::check(ponko_ctx_detect_humanoid_map(raw_, e, &m));
        return humanoid_map(m);
    }
    /// Prepares retargeting from `source`'s skeleton to `target`'s, each with its map (Rust `Retargeter::new`).
    /// Throws (naming the bone) if a required bone isn't mapped or a map names a joint the skeleton lacks.
    ponko::retargeter create_retargeter(entity source, const humanoid_map& source_map, entity target,
                                        const humanoid_map& target_map) {
        PonkoRetargeter* r = nullptr;
        detail::check(ponko_ctx_create_retargeter(raw_, source, source_map.raw(), target, target_map.raw(), &r));
        return ponko::retargeter(r);
    }
    /// Poses `target`'s mapped joints like `source`'s skeleton is posed now (Rust `Retargeter::retarget_pose`):
    /// every frame, to follow a live skeleton.
    void retarget_pose(ponko::retargeter& r, entity source, entity target) {
        detail::check(ponko_ctx_retarget_pose(raw_, r.raw(), source, target));
    }
    /// A mesh that bends with the skeleton on `skeleton`: `data`'s joints
    /// (mesh_data::set_skin) index its joints; modelled with it at rest.
    mesh create_skinned_mesh(const mesh_data& data, material m, entity skeleton) {
        mesh x{}; detail::check(ponko_ctx_create_skinned_mesh(raw_, data.raw(), m, skeleton, &x)); return x;
    }
    /// How many joints the skeleton on `e` has. Throws ponko::error if `e` has none.
    std::size_t joint_count(entity e) const { std::size_t n = 0; detail::check(ponko_ctx_joint_count(raw_, e, &n)); return n; }
    /// The first joint of `e`'s skeleton called `name`.
    std::optional<std::size_t> find_joint(entity e, const char* name) const {
        bool found = false; std::size_t j = 0;
        detail::check(ponko_ctx_find_joint(raw_, e, name, &found, &j));
        return found ? std::optional<std::size_t>(j) : std::nullopt;
    }
    /// Where joint `j` of `e`'s skeleton sits relative to its parent this frame.
    transform3d joint_pose(entity e, std::size_t j) const { transform3d t{}; detail::check(ponko_ctx_joint_pose(raw_, e, j, &t)); return t; }
    /// Moves joint `j` of `e`'s skeleton: where it sits relative to its parent from now on (Rust
    /// `skeleton.pose[j] = pose`).
    void set_joint_pose(entity e, std::size_t j, const transform3d& pose) { detail::check(ponko_ctx_set_joint_pose(raw_, e, j, &pose)); }
    /// Puts every joint of `e`'s skeleton back at rest.
    void reset_pose(entity e) { detail::check(ponko_ctx_reset_pose(raw_, e)); }
    /// Joint `j`'s matrix in `e`'s own space (column after column), from its pose now.
    std::array<float, 16> joint_matrix(entity e, std::size_t j) const {
        std::array<float, 16> m{}; detail::check(ponko_ctx_joint_matrix(raw_, e, j, m.data())); return m;
    }
    /// Writes manually sampled skeleton morph channels to models.
    void write_animated_morphs() { detail::check(ponko_ctx_write_animated_morphs(raw_)); }
    /// The index of mesh `m`'s morph target called `name` (Rust
    /// `Graphics::morph_target`), for set_morph_weight and morph_weight.
    std::optional<std::size_t> morph_target(mesh m, const char* name) const {
        std::size_t result = 0;
        const PonkoStatus status = ponko_ctx_morph_target(raw_, m, name, &result);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return result;
    }
    /// Sets how much of morph target `t` `e`'s model shows: 0 none, 1 all
    /// of it (Rust `MorphWeights::set`); outside that range exaggerates.
    /// Gives `e` morph weights if it had none, so it stops showing its
    /// mesh's default weights.
    void set_morph_weight(entity e, std::size_t t, float weight) {
        detail::check(ponko_ctx_set_morph_weight(raw_, e, t, weight));
    }
    /// How much of morph target `t` `e`'s model shows, or nothing when `e`
    /// has no morph weights: then it shows its mesh's default weights.
    std::optional<float> morph_weight(entity e, std::size_t t) const {
        float result = 0;
        const PonkoStatus status = ponko_ctx_morph_weight(raw_, e, t, &result);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return result;
    }
    /// Takes `e`'s morph weights away, so its model shows its mesh's
    /// default weights again (Rust `MorphWeights` removed).
    void clear_morph_weights(entity e) { detail::check(ponko_ctx_clear_morph_weights(raw_, e)); }
    /// The clip called `name` of a glTF file with skins (Rust `Graphics::load_clip`).
    ponko::clip load_clip(const char* path, const char* name) {
        PonkoClip* c = nullptr; detail::check(ponko_ctx_load_clip(raw_, path, name, &c)); return ponko::clip(c);
    }
    /// Poses `e`'s skeleton with `c` at `time`, blending from its pose now
    /// by `weight` (1: the clip's pose).
    void sample_clip(const ponko::clip& c, entity e, float time, float weight = 1) {
        detail::check(ponko_ctx_sample_clip(raw_, c.raw(), e, time, weight));
    }
    /// Bends `e`'s chain upper -> middle -> end so `end` reaches `target` (world), the middle
    /// bending toward `pole`. Out of reach: a straight chain pointing at it. Use in a late system.
    void two_bone_ik(entity e, std::size_t upper, std::size_t middle, std::size_t end, vec3 target, vec3 pole, float weight = 1) {
        detail::check(ponko_ctx_two_bone_ik(raw_, e, upper, middle, end, target, pole, weight));
    }
    /// Turns joint `j` of `e` so its local `axis` points at `target` (world), at most `max_angle` radians.
    void look_at_joint(entity e, std::size_t j, vec3 axis, vec3 target, float max_angle = 3.1415927f, float weight = 1) {
        detail::check(ponko_ctx_look_at_joint(raw_, e, j, axis, target, max_angle, weight));
    }
    /// Draws `e`'s bones this frame, over the scene.
    void draw_skeleton(entity e, color c) { detail::check(ponko_ctx_draw_skeleton(raw_, e, c)); }
    /// Bakes `sources` into this context's navmesh (agents replan). Throws with the reason on bad geometry.
    void bake_navmesh(const nav_sources& sources, const nav_bake_settings& settings = nav_bake_defaults()) {
        detail::check(ponko_ctx_bake_navmesh(raw_, sources.raw(), &settings));
    }
    /// The nearest walkable point within `max_distance`, or std::nullopt.
    std::optional<vec3> navmesh_sample_position(vec3 point, float max_distance) {
        vec3 p{}; bool found = false;
        detail::check(ponko_ctx_navmesh_sample_position(raw_, point, max_distance, &p, &found));
        if (!found) return std::nullopt;
        return p;
    }
    /// A path's corners from `from` to `to`, start and end included; empty when there is none.
    std::vector<vec3> navmesh_find_path(vec3 from, vec3 to) {
        std::vector<vec3> corners(32);
        std::size_t len = 0;
        detail::check(ponko_ctx_navmesh_find_path(raw_, from, to, corners.data(), corners.size(), &len));
        if (len > corners.size()) {
            corners.resize(len);
            detail::check(ponko_ctx_navmesh_find_path(raw_, from, to, corners.data(), corners.size(), &len));
        }
        corners.resize(len);
        return corners;
    }
    /// Makes `e` (with a transform3d) an agent; moves only with game::navmesh3d().
    /// Opt into sampled crowd avoidance; weight controls separation steering.
    void set_nav_avoidance(entity e, bool enabled = true, float weight = 2, float neighbour_distance = 3.6f) {
        detail::check(ponko_ctx_set_nav_avoidance(raw_, e, enabled, weight, neighbour_distance));
    }
    /// Makes `e` (with a transform3d) a navmesh agent walking at `speed` m/s, speeding up by `acceleration`
    /// m/s², `radius` m wide (at most the bake's agent radius). Keeps its destination if it already was one.
    /// Needs game::navmesh3d to move.
    void set_nav_agent(entity e, float speed = 3, float radius = 0.3f, float acceleration = 10) {
        detail::check(ponko_ctx_set_nav_agent(raw_, e, speed, radius, acceleration));
    }
    /// Spawns a body of water (Rust water::Water::spawn); see water_desc. Needs game::water() to update.
    entity spawn_water(const PonkoWaterDesc& desc) {
        entity e = 0;
        detail::check(ponko_ctx_spawn_water(raw_, &desc, &e));
        return e;
    }
    /// Despawns a body of water and frees its textures and passes; false if `water` isn't one.
    bool despawn_water(entity water) {
        auto status = ponko_ctx_despawn_water(raw_, water);
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// The time `water` is drawn at this frame (seconds), for water_sample and water_height: the
    /// simulation's clock for waves from a spectrum (it stops while paused), the frame's otherwise.
    float water_drawn_time(entity water) const {
        float t = 0;
        detail::check(ponko_ctx_water_drawn_time(raw_, water, &t));
        return t;
    }
    /// The water's surface at ground point `at` (x, z) and time `t` (seconds): height, normal, velocity.
    PonkoWaterSample water_sample(entity water, vec2 at, float t) {
        PonkoWaterSample s{};
        detail::check(ponko_ctx_water_sample(raw_, water, at, t, &s));
        return s;
    }
    /// The water's height at ground point `at` (x, z) and time `t`, the level included.
    float water_height(entity water, vec2 at, float t) { return water_sample(water, at, t).height; }
    /// Starts a ring of contact ripples on the water at `at` (x, z); `strength` about metres high.
    void water_touch(entity water, vec2 at, float radius, float strength) {
        detail::check(ponko_ctx_water_touch(raw_, water, at, radius, strength));
    }
    /// How many contact ripples wait on `water` for its next update: the rings water_touch and floating
    /// bodies crossing the surface started (each update takes three; a body whose ripple switch is off
    /// starts none). Read it in a system after physics to see a body's ripples by their effect.
    std::uint32_t water_waiting_contacts(entity water) {
        std::uint32_t n = 0;
        detail::check(ponko_ctx_water_waiting_contacts(raw_, water, &n));
        return n;
    }
    /// How many contact ripples have started on `water` since it was made. The update doesn't drain it,
    /// so a test reads it once after the physics step instead of keeping the most of water_waiting_contacts.
    std::uint64_t water_started_contacts(entity water) {
        std::uint64_t n = 0;
        detail::check(ponko_ctx_water_started_contacts(raw_, water, &n));
        return n;
    }
    /// Makes 3D body `e` float as a box of `size` (needs physics3d and game::water()).
    void set_buoyancy_box(entity e, vec3 size, float drag = PONKO_BUOYANCY_DRAG) {
        detail::check(ponko_ctx_set_buoyancy_box(raw_, e, size, drag));
    }
    /// Makes 3D body `e` float as a ball of `radius`.
    void set_buoyancy_sphere(entity e, float radius, float drag = PONKO_BUOYANCY_DRAG) {
        detail::check(ponko_ctx_set_buoyancy_sphere(raw_, e, radius, drag));
    }
    /// Makes 3D body `e` float as `desc` says (see buoyancy_desc): its points, the water's density,
    /// drag and the ripple switch. Replaces any buoyancy it had.
    void set_buoyancy(entity e, const PonkoBuoyancyDesc& desc) {
        detail::check(ponko_ctx_set_buoyancy(raw_, e, &desc));
    }
    /// How floating body `e` floats, if it does: change a field and give it back to set_buoyancy.
    std::optional<PonkoBuoyancyDesc> buoyancy(entity e) const {
        PonkoBuoyancyDesc d{};
        d.struct_size = sizeof d;
        d.version = PONKO_BUOYANCY_DESC_VERSION;
        auto status = ponko_ctx_buoyancy(raw_, e, &d);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return d;
    }
    /// Stops `e` floating; false if it had no buoyancy.
    bool remove_buoyancy(entity e) {
        auto status = ponko_ctx_remove_buoyancy(raw_, e);
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// How much of floating body `e` was under water at the last physics step (0 to 1), if it floats.
    std::optional<float> buoyancy_submerged(entity e) {
        float share = 0;
        auto status = ponko_ctx_buoyancy_submerged(raw_, e, &share);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return share;
    }
    /// Sends `e`'s agent to `destination`; std::nullopt stops it.
    void set_nav_destination(entity e, std::optional<vec3> destination) {
        detail::check(ponko_ctx_set_nav_destination(raw_, e, destination ? &*destination : nullptr));
    }
    /// Makes `e` a carving box obstacle: cached tile layers are rebuilt to cut a real hole.
    /// Simulation consumes at most one layer per tick.
    void set_nav_carve_obstacle(entity e, vec3 center, vec3 half_extents) {
        detail::check(ponko_ctx_set_nav_carve_obstacle(raw_, e, center, half_extents));
    }
    /// Runs up to `budget` extra tile updates after simulation has gathered obstacle
    /// components; returns the steps taken and whether updates are still pending.
    std::pair<std::size_t, bool> navmesh_update_carving(std::size_t budget = 1) {
        std::size_t steps = 0; bool pending = false;
        detail::check(ponko_ctx_navmesh_update_carving(raw_, budget, &steps, &pending));
        return {steps, pending};
    }
    /// Makes `e` a box obstacle agents path around.
    void set_nav_obstacle(entity e, vec3 center, vec3 half_extents) {
        detail::check(ponko_ctx_set_nav_obstacle(raw_, e, center, half_extents));
    }
    /// Takes `e`'s agent and obstacle off (whichever it has).
    void remove_nav(entity e) { detail::check(ponko_ctx_remove_nav(raw_, e)); }
    /// What the navmesh's crowd has done since it was baked (Rust `NavMesh::crowd_stats`), for tests that
    /// count the crowd's cost instead of timing it. Throws ponko::error with no navmesh.
    nav_crowd_stats navmesh_crowd_stats() const {
        nav_crowd_stats s{};
        s.struct_size = sizeof s;
        s.version = PONKO_NAV_CROWD_STATS_VERSION;
        detail::check(ponko_ctx_navmesh_crowd_stats(raw_, &s));
        return s;
    }
    /// Gives `e` (which needs a 3D transform) a new particle emitter, replacing any old one.
    void set_particle_emitter3d(entity e, const particle_settings& settings) {
        PonkoParticleSettings3DEx c = settings.raw_ex();
        detail::check(ponko_ctx_set_particle_emitter3d_ex(raw_, e, &c));
    }
    /// Emits `count` particles from `e`'s emitter on the next frame.
    void particle_burst(entity e, std::uint32_t count) { detail::check(ponko_ctx_particle_burst(raw_, e, count)); }
    /// How many of `e`'s particles are alive.
    std::uint32_t particle_live_count(entity e) {
        std::uint32_t n = 0;
        detail::check(ponko_ctx_particle_live_count(raw_, e, &n));
        return n;
    }
    /// Removes `e`'s emitter and its particles.
    void remove_particle_emitter3d(entity e) { detail::check(ponko_ctx_remove_particle_emitter3d(raw_, e)); }
    /// Gives `e` (which needs a 2D transform) a new 2D particle emitter, replacing any old one.
    void set_particle_emitter2d(entity e, const particle_settings2d& settings) {
        PonkoParticleSettings2D c = settings.raw();
        detail::check(ponko_ctx_set_particle_emitter2d(raw_, e, &c));
    }
    /// Emits `count` particles from `e`'s 2D emitter on the next frame.
    void particle_burst2d(entity e, std::uint32_t count) { detail::check(ponko_ctx_particle_burst2d(raw_, e, count)); }
    /// How many of `e`'s 2D particles are alive.
    std::uint32_t particle_live_count2d(entity e) {
        std::uint32_t n = 0;
        detail::check(ponko_ctx_particle_live_count2d(raw_, e, &n));
        return n;
    }
    /// Advances `e`'s 2D emitter by `dt` seconds by hand (headless runs skip the frame phase that does it).
    void particle_update2d(entity e, float dt) { detail::check(ponko_ctx_particle_update2d(raw_, e, dt)); }
    /// Gives `e` (which needs a 2D transform) a `size` box occluder centred on it, replacing any old one:
    /// it casts shadows from 2D lights with shadows on. It turns and scales with the transform.
    void set_occluder2d_rect(entity e, vec2 size) { detail::check(ponko_ctx_set_occluder2d_rect(raw_, e, size)); }
    /// Gives `e` a round occluder (16 sides) of `radius`, replacing any old one.
    void set_occluder2d_circle(entity e, float radius) { detail::check(ponko_ctx_set_occluder2d_circle(raw_, e, radius)); }
    /// Gives `e` a closed polygon occluder (`points` relative to its transform, either winding),
    /// replacing any old one.
    void set_occluder2d_polygon(entity e, const std::vector<vec2>& points) {
        detail::check(ponko_ctx_set_occluder2d_polygon(raw_, e, points.data(), points.size()));
    }
    /// Gives `e` a thin wall from `a` to `b` as its occluder (shadows on both sides), replacing any old one.
    void set_occluder2d_line(entity e, vec2 a, vec2 b) { detail::check(ponko_ctx_set_occluder2d_line(raw_, e, a, b)); }
    /// Gives tile map `map` an occluder outlining its cells that show one of `solid` tiles (Rust
    /// `TileMap::occluder`), replacing any old one. Call it again after the walls change.
    void set_tilemap_occluder(entity map, const std::vector<std::uint16_t>& solid) {
        detail::check(ponko_ctx_set_tilemap_occluder(raw_, map, solid.data(), solid.size()));
    }
    /// Takes `e`'s occluder off (fine if it has none).
    void remove_occluder2d(entity e) { detail::check(ponko_ctx_remove_occluder2d(raw_, e)); }
    /// How many edges `e`'s occluder has (0 without one).
    std::size_t occluder2d_edge_count(entity e) {
        std::size_t n = 0;
        detail::check(ponko_ctx_occluder2d_edge_count(raw_, e, &n));
        return n;
    }
    /// Sets the 2D ambient light (the light everything gets with no light near), or turns it off with
    /// std::nullopt. Setting one turns 2D lighting on: unlit places show `ambient` times their color.
    void set_ambient_light2d(std::optional<color> ambient) {
        detail::check(ponko_ctx_set_ambient_light2d(raw_, ambient.has_value(), ambient.value_or(color{0, 0, 0, 1})));
    }
    /// The 2D ambient light, if one is set.
    std::optional<color> ambient_light2d() {
        bool on = false;
        color c{0, 0, 0, 1};
        detail::check(ponko_ctx_ambient_light2d(raw_, &on, &c));
        if (!on) return std::nullopt;
        return c;
    }
    /// Gives sprites and tiles drawn from `t` the normal map `normals` (tangent space, green up, the same
    /// size as `t`), or takes it away with std::nullopt. 2D lights then shade their bumps.
    void set_normal_map(texture t, std::optional<texture> normals) {
        detail::check(ponko_ctx_set_normal_map(raw_, t, normals.has_value(), normals.value_or(t)));
    }
    /// `t`'s normal map, if it has one.
    std::optional<texture> normal_map(texture t) {
        bool has = false;
        texture n = t;
        detail::check(ponko_ctx_normal_map(raw_, t, &has, &n));
        if (!has) return std::nullopt;
        return n;
    }
    /// Removes `e`'s 2D emitter and its particles.
    void remove_particle_emitter2d(entity e) { detail::check(ponko_ctx_remove_particle_emitter2d(raw_, e)); }
    /// Gives `e` (which needs a 3D transform) a new ribbon trail, replacing any old one.
    void set_trail3d(entity e, const trail3d& trail) {
        PonkoTrail3D c = trail.raw();
        detail::check(ponko_ctx_set_trail3d(raw_, e, &c));
    }
    /// How many points `e`'s trail keeps now (trails only record in visual frames).
    std::uint32_t trail3d_point_count(entity e) {
        std::uint32_t n = 0;
        detail::check(ponko_ctx_trail3d_point_count(raw_, e, &n));
        return n;
    }
    /// Forgets every point of `e`'s trail: after a teleport, so no ribbon spans the jump.
    void clear_trail3d(entity e) { detail::check(ponko_ctx_clear_trail3d(raw_, e)); }
    /// Starts or stops `e`'s trail adding points; while off, the ribbon fades out.
    void set_trail3d_emitting(entity e, bool emitting) { detail::check(ponko_ctx_set_trail3d_emitting(raw_, e, emitting)); }
    /// Removes `e`'s trail and its points.
    void remove_trail3d(entity e) { detail::check(ponko_ctx_remove_trail3d(raw_, e)); }
    /// Gives `e` (which needs a 3D transform) a line, replacing any old one.
    void set_line3d(entity e, const line3d& line) {
        PonkoLine3D c = line.raw();
        detail::check(ponko_ctx_set_line3d(raw_, e, &c));
    }
    /// Removes `e`'s line.
    void remove_line3d(entity e) { detail::check(ponko_ctx_remove_line3d(raw_, e)); }
    /// A glTF file's mesh on the CPU (Rust `Graphics::load_mesh_data`), all its
    /// parts in one, without making anything on the GPU: to cut decals from.
    /// Loads the file each call; keep the result.
    mesh_data load_mesh_data(const char* path) {
        PonkoMeshData* d = nullptr;
        detail::check(ponko_ctx_load_mesh_data(raw_, path, &d));
        return mesh_data(d);
    }
    /// Gives `e` (with a skeleton) an animator playing `g`; the engine advances it every frame.
    void set_animator(entity e, const animation_graph& g) { detail::check(ponko_ctx_set_animator(raw_, e, g.raw())); }
    /// Sets float parameter `parameter` of `e`'s animator.
    void animator_set_float(entity e, std::size_t parameter, float value) { detail::check(ponko_ctx_animator_set_float(raw_, e, parameter, value)); }
    /// Sets bool parameter `parameter` of `e`'s animator.
    void animator_set_bool(entity e, std::size_t parameter, bool value) { detail::check(ponko_ctx_animator_set_bool(raw_, e, parameter, value)); }
    /// Sets (or clears) trigger parameter `parameter` of `e`'s animator; the transition it fires clears it.
    void animator_set_trigger(entity e, std::size_t parameter, bool value = true) { detail::check(ponko_ctx_animator_set_trigger(raw_, e, parameter, value)); }
    /// The state `e`'s animator is in (or fading to).
    std::size_t animator_state(entity e) { std::size_t s = 0; detail::check(ponko_ctx_animator_state(raw_, e, &s, nullptr)); return s; }
    /// How far through its state it is: 1 is one full play.
    float animator_normalized_time(entity e) { float t = 0; detail::check(ponko_ctx_animator_state(raw_, e, nullptr, &t)); return t; }
    /// Unwrapped normalized time in double precision (1 is one play).
    double animator_normalized_time_f64(entity e) {
        double t = 0;
        detail::check(ponko_ctx_animator_normalized_time_f64(raw_, e, &t));
        return t;
    }
    /// Crossfades `e`'s animator to `state` over `duration` seconds (0 jumps), starting it at normalized time
    /// `offset`, on its next update.
    void animator_crossfade(entity e, std::size_t state, float duration, float offset = 0) {
        detail::check(ponko_ctx_animator_crossfade(raw_, e, state, duration, offset));
    }
    /// Jumps to `state` at normalized time `time` on the next update.
    void animator_play(entity e, std::size_t state, float time = 0) { animator_crossfade(e, state, 0, time); }
    /// Adds a layer playing `g` over `e`'s base animation; returns its index.
    /// `mask_joint` limits it to that joint and its children ("" for all joints).
    std::size_t animator_add_layer(entity e, const animation_graph& g, layer_blend mode = layer_blend::override_pose,
                                   const std::string& mask_joint = "") {
        std::size_t index = 0;
        detail::check(ponko_ctx_animator_add_layer(raw_, e, g.raw(), static_cast<std::uint32_t>(mode),
                                                    mask_joint.empty() ? nullptr : mask_joint.c_str(), &index));
        return index;
    }
    /// Sets how strongly layer `layer` of `e`'s animator shows, 0 to 1.
    void animator_set_layer_weight(entity e, std::size_t layer, float weight) {
        detail::check(ponko_ctx_animator_set_layer_weight(raw_, e, layer, weight));
    }
    /// Sets float parameter `parameter` of layer `layer`'s own graph.
    void animator_layer_set_float(entity e, std::size_t layer, std::size_t parameter, float value) {
        detail::check(ponko_ctx_animator_layer_set_float(raw_, e, layer, parameter, value));
    }
    /// Sets bool parameter `parameter` of layer `layer`'s own graph.
    void animator_layer_set_bool(entity e, std::size_t layer, std::size_t parameter, bool value) {
        detail::check(ponko_ctx_animator_layer_set_bool(raw_, e, layer, parameter, value));
    }
    /// Sets (or clears) trigger parameter `parameter` of layer `layer`'s own graph.
    void animator_layer_set_trigger(entity e, std::size_t layer, std::size_t parameter, bool value = true) {
        detail::check(ponko_ctx_animator_layer_set_trigger(raw_, e, layer, parameter, value));
    }
    /// Crossfades layer `layer` to `state` of its own graph (like animator_crossfade).
    void animator_layer_crossfade(entity e, std::size_t layer, std::size_t state, float duration, float offset = 0) {
        detail::check(ponko_ctx_animator_layer_crossfade(raw_, e, layer, state, duration, offset));
    }
    /// What `e`'s animator does with joint `root_joint`'s horizontal movement and turning.
    void animator_set_root_motion(entity e, root_motion mode, std::size_t root_joint = 0) {
        detail::check(ponko_ctx_animator_set_root_motion(raw_, e, static_cast<std::uint32_t>(mode), root_joint));
    }
    /// The movement (entity axes, before scale) and turn taken out of the pose in the latest update.
    std::pair<vec3, quat> animator_root_motion(entity e) {
        vec3 p{};
        quat r{0, 0, 0, 1};
        detail::check(ponko_ctx_animator_root_motion(raw_, e, &p, &r));
        return {p, r};
    }
    /// Turns the engine's automatic update of `e`'s animator off (`true`) or back on; while off, pose it with
    /// animator_update.
    void animator_set_manual(entity e, bool manual) { detail::check(ponko_ctx_animator_set_manual(raw_, e, manual)); }
    /// Advances `e`'s animator by `dt` and poses its skeleton now.
    void animator_update(entity e, float dt) { detail::check(ponko_ctx_animator_update(raw_, e, dt)); }
    /// The oldest unread event of `e`'s animator, or std::nullopt.
    std::optional<std::string> animator_next_event(entity e) {
        char name[128];
        bool found = false;
        detail::check(ponko_ctx_animator_next_event(raw_, e, name, sizeof name, &found));
        if (!found) return std::nullopt;
        return std::string(name);
    }
    // ---- 2D skeletons (Rust `ponko::skeleton2d`) ----
    /// Loads skeleton file `path` (Ponko's format, or a Spine JSON export with the `.atlas` beside it) and its
    /// images once; later loads and set_skeleton2d reuse it. Returns the Spine features it uses that aren't
    /// read, one warning each (they are also printed).
    std::vector<std::string> load_skeleton2d(const char* path) {
        std::size_t count = 0;
        detail::check(ponko_ctx_load_skeleton2d(raw_, path, &count));
        std::vector<std::string> warnings;
        for (std::size_t i = 0; i < count; ++i) {
            warnings.push_back(detail::read_text([&](char* b, std::size_t c, std::size_t* n) {
                return ponko_ctx_skeleton2d_warning(raw_, path, i, b, c, n);
            }));
        }
        return warnings;
    }
    /// Puts a skeleton playing file `path` (loaded once) on `e`, replacing one it had; give `e` a transform2d to
    /// place it. The engine advances and draws it.
    void set_skeleton2d(entity e, const char* path) { detail::check(ponko_ctx_set_skeleton2d(raw_, e, path)); }
    /// Takes `e`'s skeleton off (nothing happens if it has none).
    void remove_skeleton2d(entity e) { detail::check(ponko_ctx_remove_skeleton2d(raw_, e)); }
    /// Plays clip `clip` on `e`'s skeleton from its start, looping or once (then holding its last pose).
    void skeleton2d_play(entity e, const char* clip, bool looping = true) {
        detail::check(ponko_ctx_skeleton2d_play(raw_, e, clip, looping));
    }
    /// Fades `e`'s skeleton from what plays now to clip `clip` (looping) over `duration` seconds.
    void skeleton2d_crossfade(entity e, const char* clip, float duration) {
        detail::check(ponko_ctx_skeleton2d_crossfade(raw_, e, clip, duration));
    }
    /// The index of the clip named `name` of `e`'s skeleton (Rust's `Skeleton::clip_id`): resolve it once and
    /// play it with skeleton2d_play_index. Throws for a name no clip has; the message lists the clips.
    std::size_t skeleton2d_clip_index(entity e, const char* name) {
        std::size_t i = 0;
        detail::check(ponko_ctx_skeleton2d_clip_index(raw_, e, name, &i));
        return i;
    }
    /// skeleton2d_play by an index from skeleton2d_clip_index, with no name lookup.
    void skeleton2d_play_index(entity e, std::size_t clip, bool looping = true) {
        detail::check(ponko_ctx_skeleton2d_play_index(raw_, e, clip, looping));
    }
    /// skeleton2d_crossfade by an index from skeleton2d_clip_index, with no name lookup.
    void skeleton2d_crossfade_index(entity e, std::size_t clip, float duration) {
        detail::check(ponko_ctx_skeleton2d_crossfade_index(raw_, e, clip, duration));
    }
    /// Switches the state machine of `e`'s skeleton to state `state`, starting at normalized time `offset`,
    /// fading over `duration` seconds (0 jumps). Throws for a state not in it or a bad duration or offset.
    void skeleton2d_crossfade_state(entity e, std::size_t state, float duration, float offset = 0) {
        detail::check(ponko_ctx_skeleton2d_crossfade_state(raw_, e, state, duration, offset));
    }
    /// The state machine's state `e`'s skeleton is in, or std::nullopt while a clip plays by itself.
    std::optional<std::size_t> skeleton2d_state(entity e) {
        std::size_t s = 0;
        detail::check(ponko_ctx_skeleton2d_state(raw_, e, &s, nullptr, nullptr));
        if (s == SIZE_MAX) return std::nullopt;
        return s;
    }
    /// How far `e`'s skeleton is through what it plays (normalized: 1 is one full play).
    float skeleton2d_normalized_time(entity e) {
        float t = 0;
        detail::check(ponko_ctx_skeleton2d_state(raw_, e, nullptr, &t, nullptr));
        return t;
    }
    /// Unwrapped normalized time in double precision (1 is one play).
    double skeleton2d_normalized_time_f64(entity e) {
        double t = 0;
        detail::check(ponko_ctx_skeleton2d_normalized_time_f64(raw_, e, &t));
        return t;
    }
    /// Whether what `e`'s skeleton played once reached its end.
    bool skeleton2d_finished(entity e) {
        bool done = false;
        detail::check(ponko_ctx_skeleton2d_state(raw_, e, nullptr, nullptr, &done));
        return done;
    }
    /// The index of the state machine's state named `name` (throws if there is none; the message lists the states).
    std::size_t skeleton2d_state_index(entity e, const char* name) {
        std::size_t i = 0;
        detail::check(ponko_ctx_skeleton2d_state_index(raw_, e, name, &i));
        return i;
    }
    /// The index of the state machine's parameter named `name` (throws if there is none; the message lists the
    /// parameters).
    std::size_t skeleton2d_parameter_index(entity e, const char* name) {
        std::size_t i = 0;
        detail::check(ponko_ctx_skeleton2d_parameter_index(raw_, e, name, &i));
        return i;
    }
    /// Sets float parameter `parameter` of `e`'s skeleton; throws (changing nothing) for one that isn't a float
    /// or a value that isn't finite.
    void skeleton2d_set_float(entity e, std::size_t parameter, float value) {
        detail::check(ponko_ctx_skeleton2d_set_float(raw_, e, parameter, value));
    }
    /// Sets bool parameter `parameter` of `e`'s skeleton; throws for one that isn't a bool.
    void skeleton2d_set_bool(entity e, std::size_t parameter, bool value) {
        detail::check(ponko_ctx_skeleton2d_set_bool(raw_, e, parameter, value));
    }
    /// Sets (or clears) trigger `parameter` of `e`'s skeleton; the transition it fires clears it.
    void skeleton2d_set_trigger(entity e, std::size_t parameter, bool value = true) {
        detail::check(ponko_ctx_skeleton2d_set_trigger(raw_, e, parameter, value));
    }
    /// The oldest event `e`'s skeleton fired and nobody took yet, or std::nullopt.
    std::optional<std::string> skeleton2d_next_event(entity e) {
        char name[128];
        bool found = false;
        detail::check(ponko_ctx_skeleton2d_next_event(raw_, e, name, sizeof name, &found));
        if (!found) return std::nullopt;
        return std::string(name);
    }
    /// The index of the IK constraint named `name` (throws if there is none).
    std::size_t skeleton2d_ik_index(entity e, const char* name) {
        std::size_t i = 0;
        detail::check(ponko_ctx_skeleton2d_ik_index(raw_, e, name, &i));
        return i;
    }
    /// Points IK constraint `ik` of `e`'s skeleton at `target` (in the skeleton's own space) instead of its
    /// target bone.
    void skeleton2d_set_ik_target(entity e, std::size_t ik, vec2 target) {
        detail::check(ponko_ctx_skeleton2d_set_ik_target(raw_, e, ik, target));
    }
    /// Hands IK constraint `ik` of `e`'s skeleton back to its target bone.
    void skeleton2d_clear_ik_target(entity e, std::size_t ik) {
        detail::check(ponko_ctx_skeleton2d_clear_ik_target(raw_, e, ik));
    }
    /// How much IK constraint `ik` of `e`'s skeleton bends its bones, 0 to 1.
    void skeleton2d_set_ik_mix(entity e, std::size_t ik, float mix) {
        detail::check(ponko_ctx_skeleton2d_set_ik_mix(raw_, e, ik, mix));
    }
    /// Turns bone `bone` of `e`'s skeleton so its +x axis points at `target` (the skeleton's own space),
    /// `mix` of the way (0 to 1), and moves the bones below it. A one-off: call it in a late system, after the
    /// update.
    void skeleton2d_look_at(entity e, std::size_t bone, vec2 target, float mix = 1) {
        detail::check(ponko_ctx_skeleton2d_look_at(raw_, e, bone, target, mix));
    }
    /// Bends bone `upper` of `e`'s skeleton and its child `lower` so the tip of `lower` reaches `target` (the
    /// skeleton's own space), `mix` of the way. `bend_positive`: the joint turns clockwise on screen. A one-off,
    /// as skeleton2d_look_at; throws if `lower`'s parent isn't `upper`.
    void skeleton2d_two_bone_ik(entity e, std::size_t upper, std::size_t lower, vec2 target,
                                bool bend_positive = true, float mix = 1) {
        detail::check(ponko_ctx_skeleton2d_two_bone_ik(raw_, e, upper, lower, target, bend_positive, mix));
    }
    /// The index of the bone named `name` (throws if there is none).
    std::size_t skeleton2d_bone_index(entity e, const char* name) {
        std::size_t i = 0;
        detail::check(ponko_ctx_skeleton2d_bone_index(raw_, e, name, &i));
        return i;
    }
    /// Where bone `bone` of `e`'s skeleton is, in the skeleton's own space as last posed.
    vec2 skeleton2d_bone_position(entity e, std::size_t bone) {
        vec2 p{};
        detail::check(ponko_ctx_skeleton2d_bone(raw_, e, bone, &p, nullptr));
        return p;
    }
    /// Which way bone `bone` of `e`'s skeleton points (radians, clockwise on screen), as last posed.
    float skeleton2d_bone_rotation(entity e, std::size_t bone) {
        float r = 0;
        detail::check(ponko_ctx_skeleton2d_bone(raw_, e, bone, nullptr, &r));
        return r;
    }
    /// The index of the slot named `name` (throws if there is none).
    std::size_t skeleton2d_slot_index(entity e, const char* name) {
        std::size_t i = 0;
        detail::check(ponko_ctx_skeleton2d_slot_index(raw_, e, name, &i));
        return i;
    }
    /// The attachment slot `slot` of `e`'s skeleton shows ("" for nothing).
    std::string skeleton2d_attachment(entity e, std::size_t slot) {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) {
            return ponko_ctx_skeleton2d_attachment(raw_, e, slot, b, c, n);
        });
    }
    /// Makes slot `slot` of `e`'s skeleton show attachment `name` (nullptr: nothing) whatever its clips key,
    /// until skeleton2d_reset_attachment.
    void skeleton2d_set_attachment(entity e, std::size_t slot, const char* name) {
        detail::check(ponko_ctx_skeleton2d_set_attachment(raw_, e, slot, name));
    }
    /// Lets slot `slot` of `e`'s skeleton show what its clips and rest pose say again.
    void skeleton2d_reset_attachment(entity e, std::size_t slot) {
        detail::check(ponko_ctx_skeleton2d_reset_attachment(raw_, e, slot));
    }
    /// Shows skin `name` on `e`'s skeleton (throws if there is none).
    void skeleton2d_set_skin(entity e, const char* name) { detail::check(ponko_ctx_skeleton2d_set_skin(raw_, e, name)); }
    /// How `e`'s skeleton draws and runs: `z` (draw order among sprites), `tint` and `speed` (1 as recorded,
    /// 0 pauses).
    void skeleton2d_set_look(entity e, float z, color tint, float speed = 1) {
        detail::check(ponko_ctx_skeleton2d_set_look(raw_, e, z, tint, speed));
    }
    /// Turns the engine's automatic update of `e`'s skeleton off (`true`) or back on; while off, advance it
    /// with skeleton2d_update.
    void skeleton2d_set_manual(entity e, bool manual) { detail::check(ponko_ctx_skeleton2d_set_manual(raw_, e, manual)); }
    /// Advances `e`'s skeleton by `dt` seconds and poses it now.
    void skeleton2d_update(entity e, float dt) { detail::check(ponko_ctx_skeleton2d_update(raw_, e, dt)); }
    /// Poses `e`'s skinned model with `skeleton`'s skeleton and draws it
    /// there (Rust `SkinOf`), binding joints by name.
    void set_skin_of(entity e, entity skeleton) { detail::check(ponko_ctx_set_skin_of(raw_, e, skeleton)); }
    /// Keeps `e`'s transform on joint `j` of `skeleton`, moved by `offset`
    /// in the joint's space (Rust `AttachToJoint`).
    void attach_to_joint(entity e, entity skeleton, std::size_t j, const transform3d& offset) {
        detail::check(ponko_ctx_attach_to_joint(raw_, e, skeleton, j, &offset));
    }
    /// Whether models showing mesh `m` cast shadows.
    bool mesh_shadows(mesh m) const { bool casts = true; detail::check(ponko_ctx_mesh_shadows(raw_, m, &casts)); return casts; }
    /// Whether this game made `m` and hasn't freed it.
    bool is_mesh_live(mesh m) const { bool live = false; detail::check(ponko_ctx_is_mesh_live(raw_, m, &live)); return live; }
    /// Whether handle `h` (a texture, render_target, sound, font, mesh, material, shader,
    /// compute, effect, gpu_buffer, instance_batch or texture_array) names
    /// something this game made and hasn't freed (Rust `Ctx::is_live`).
    template<class H> bool is_live(H h) const {
        bool live = false;
        detail::check(ponko_ctx_handle_is_live(raw_, detail::handle_kind<H>::value, h.id, &live));
        return live;
    }
    /// Frees handle `h` (Rust `Ctx::free`): its id is never reused (except
    /// for meshes and GPU buffers, whose next one made takes it), and
    /// whatever still uses it draws the default (nothing for
    /// a mesh, white for a texture, the default material, the built-in font).
    template<class H> void free(H h) {
        detail::check(ponko_ctx_handle_free(raw_, detail::handle_kind<H>::value, h.id));
    }
    /// How many handles of each kind are live now (Rust `Ctx::live_counts`).
    ponko::live_counts live_counts() const {
        ponko::live_counts c{}; detail::check(ponko_ctx_live_counts(raw_, &c)); return c;
    }
    /// Live handles of `c` and their estimated bytes (Rust `Ctx::asset_inventory`).
    ponko::asset_count asset_count(asset_class c) const {
        ponko::asset_count n{}; detail::check(ponko_ctx_asset_count(raw_, static_cast<std::uint32_t>(c), &n)); return n;
    }
    /// Every live handle of `c`, oldest first, with its bytes, age and owner
    /// (Rust `Graphics::asset_handles`, `Audio::asset_handles`).
    std::vector<ponko::asset_record> asset_handles(asset_class c) const {
        auto n = asset_handles_into(c, nullptr, 0);
        std::vector<ponko::asset_record> out(n);
        if (n) n = asset_handles_into(c, out.data(), out.size());
        out.resize(std::min(n, out.size()));
        return out;
    }
    /// Copies the oldest handles into caller storage and returns the total count.
    std::size_t asset_handles_into(asset_class c, ponko::asset_record* records, std::size_t capacity) const {
        std::size_t n = 0;
        detail::check(ponko_ctx_asset_handles(raw_, static_cast<std::uint32_t>(c), records, capacity, &n));
        return n;
    }
    /// Copies mesh costs, largest total triangle count first, and returns the total count.
    /// `name_bytes` receives the required name storage for the copied records.
    /// Read a name only if its offset and length plus the NUL fit `name_capacity`.
    std::size_t mesh_costs_into(mesh_cost* records, std::size_t capacity, char* names,
                               std::size_t name_capacity, std::size_t& name_bytes) const {
        std::size_t n = 0;
        detail::check(ponko_ctx_mesh_costs(raw_, records, capacity, names, name_capacity, &n, &name_bytes));
        return n;
    }
    /// Copies streamed assets ordered by kind and name, returning the total count.
    /// `name_bytes` receives the required name storage for the copied records.
    /// Read a name only if its offset and length plus the NUL fit `name_capacity`.
    std::size_t streamed_assets_into(streamed_asset* records, std::size_t capacity, char* names,
                                    std::size_t name_capacity, std::size_t& name_bytes) const {
        std::size_t n = 0;
        detail::check(ponko_ctx_streamed_assets(raw_, records, capacity, names, name_capacity, &n, &name_bytes));
        return n;
    }
    /// The asset name that loaded handle `id` of `c`; empty if code or the engine made it.
    std::string asset_owner_name(asset_class c, std::uint32_t id) const {
        const auto k = static_cast<std::uint32_t>(c);
        std::size_t len = 0;
        detail::check(ponko_ctx_asset_owner(raw_, k, id, nullptr, 0, &len));
        std::string text(len + 1, '\0');
        detail::check(ponko_ctx_asset_owner(raw_, k, id, text.data(), text.size(), &len));
        text.resize(len);
        return text;
    }
    /// Frees everything loading glTF file `path` made: mesh, materials and
    /// unshared textures (Rust `Graphics::unload_mesh`). False if it wasn't loaded.
    bool unload_mesh(const char* path) {
        bool done = false; detail::check(ponko_ctx_unload_mesh(raw_, path, &done)); return done;
    }
    /// Sets `kind`'s residency budget in bytes; std::nullopt for no limit
    /// (Rust `Ctx::set_residency_budget`).
    void set_residency_budget(residency_kind kind, std::optional<std::uint64_t> bytes) {
        detail::check(ponko_ctx_set_residency_budget(raw_, static_cast<std::uint32_t>(kind), bytes.has_value(), bytes.value_or(0)));
    }
    /// `kind`'s budget and counters (Rust `Graphics::residency`).
    ponko::residency_stats residency(residency_kind kind) const {
        ponko::residency_stats r{}; detail::check(ponko_ctx_residency(raw_, static_cast<std::uint32_t>(kind), &r)); return r;
    }
    /// Whether the asset of `kind` called `name` is resident.
    bool is_resident(residency_kind kind, const char* name) const {
        bool r = false; detail::check(ponko_ctx_is_resident(raw_, static_cast<std::uint32_t>(kind), name, &r)); return r;
    }
    /// Texture `path` at full resolution under the texture budget, loaded
    /// now if it isn't; with `pin`, never evicted until unpinned (Rust
    /// `Ctx::resident_texture`, `pin_resident_texture`).
    texture resident_texture(const char* path, bool pin = false) {
        texture t{}; detail::check(ponko_ctx_resident_texture(raw_, path, pin, &t)); return t;
    }
    /// Mesh `path` under the mesh budget (Rust `Ctx::resident_mesh`, `pin_resident_mesh`).
    mesh resident_mesh(const char* path, bool pin = false) {
        mesh m{}; detail::check(ponko_ctx_resident_mesh(raw_, path, pin, &m)); return m;
    }
    /// Lets a pinned asset be evicted again.
    void unpin_resident(residency_kind kind, const char* path) {
        detail::check(ponko_ctx_unpin_resident(raw_, static_cast<std::uint32_t>(kind), path));
    }
    /// Texture `path` streamed in without waiting (Rust `Ctx::stream_texture`):
    /// std::nullopt until its stand-in is uploaded.
    std::optional<texture> stream_texture(const char* path) {
        texture t{}; bool ready = false;
        detail::check(ponko_ctx_stream_texture(raw_, path, &t, &ready));
        if (!ready) return std::nullopt;
        return t;
    }
    /// Mesh `path` streamed in without waiting (Rust `Ctx::stream_mesh`).
    std::optional<mesh> stream_mesh(const char* path) {
        mesh m{}; bool ready = false;
        detail::check(ponko_ctx_stream_mesh(raw_, path, &m, &ready));
        if (!ready) return std::nullopt;
        return m;
    }
    /// Where the asset of `kind` called `name` is (Rust `Graphics::asset_state`).
    ponko::asset_state asset_state(residency_kind kind, const char* name) const {
        std::uint32_t s = 0;
        detail::check(ponko_ctx_asset_state(raw_, static_cast<std::uint32_t>(kind), name, &s));
        return static_cast<ponko::asset_state>(s);
    }
    /// Why loading it failed; empty if it didn't (Rust `Graphics::asset_error`).
    std::string asset_error(residency_kind kind, const char* name) const {
        std::size_t len = 0;
        detail::check(ponko_ctx_asset_error(raw_, static_cast<std::uint32_t>(kind), name, nullptr, 0, &len));
        std::string text(len + 1, '\0');
        detail::check(ponko_ctx_asset_error(raw_, static_cast<std::uint32_t>(kind), name, text.data(), text.size(), &len));
        text.resize(len);
        return text;
    }
    /// Bytes the upload step may send each frame; std::nullopt for no limit.
    void set_upload_budget(std::optional<std::uint64_t> bytes) {
        detail::check(ponko_ctx_set_upload_budget(raw_, bytes.has_value(), bytes.value_or(0)));
    }
    /// Asset states change in the same frames every run (Rust `Graphics::set_deterministic_uploads`).
    void set_deterministic_uploads(bool on) { detail::check(ponko_ctx_set_deterministic_uploads(raw_, on)); }
    /// The upload queue's budget and counters.
    ponko::upload_stats upload_stats() const {
        ponko::upload_stats u{}; detail::check(ponko_ctx_upload_stats(raw_, &u)); return u;
    }
    /// The upload queue's counters with the GPU's own upload times (Rust `UploadStats::gpu_created_ms`,
    /// `gpu_upload_ms`): the has_gpu_* flags say whether a time arrived yet.
    ponko::upload_stats_ex upload_stats_ex() const {
        ponko::upload_stats_ex u{};
        u.struct_size = sizeof u;
        u.version = PONKO_UPLOAD_STATS_EX_VERSION;
        detail::check(ponko_ctx_upload_stats_ex(raw_, &u));
        return u;
    }
    /// Waits until everything streamed so far is loaded and every asset group is done (tests and tools).
    void finish_uploads() { detail::check(ponko_ctx_finish_uploads(raw_)); }
    // Asset groups (Rust `Assets::start_group` and the `group_*` calls).
    /// Starts loading `group` in the background (it is taken) and returns its id.
    std::uint32_t start_asset_group(asset_group group) {
        std::uint32_t id = 0; detail::check(ponko_ctx_start_asset_group(raw_, group.release(), &id)); return id;
    }
    /// The id of the last group started with `name`, or 0 if none.
    std::uint32_t find_asset_group(const char* name) const {
        std::uint32_t id = 0; detail::check(ponko_ctx_find_asset_group(raw_, name, &id)); return id;
    }
    /// Where group `id` is.
    ponko::group_state asset_group_state(std::uint32_t id) const {
        std::uint32_t s = 0; detail::check(ponko_ctx_asset_group_state(raw_, id, &s)); return static_cast<ponko::group_state>(s);
    }
    /// How far group `id` got, for a progress bar (`fraction` is 0 to 1).
    ponko::group_progress asset_group_progress(std::uint32_t id) const {
        ponko::group_progress p{}; detail::check(ponko_ctx_asset_group_progress(raw_, id, &p)); return p;
    }
    /// Why group `id` failed (`path()` names the asset); std::nullopt if it didn't.
    std::optional<ponko::error> asset_group_error(std::uint32_t id) const {
        if (ponko_ctx_asset_group_error(raw_, id) == PONKO_OK) return std::nullopt;
        return detail::last_error();
    }
    /// Stops loading group `id`; what loaded stays loaded. A failed group is cancelled too, dropping
    /// what it kept for a restart.
    void cancel_asset_group(std::uint32_t id) { detail::check(ponko_ctx_cancel_asset_group(raw_, id)); }
    /// Loads the rest of a failed or cancelled group `id`: only what had not loaded is read again.
    void restart_asset_group(std::uint32_t id) { detail::check(ponko_ctx_restart_asset_group(raw_, id)); }
    /// Takes the bytes of asset `path` of group `id` (added with `asset_group::bytes`) and returns
    /// `decode(data, len)`. Throws ponko::error if they aren't there (not loaded, or taken), and what
    /// `decode` throws.
    template<class F> auto take_asset_group_bytes(std::uint32_t id, const char* path, F decode)
        -> decltype(decode(std::declval<const std::uint8_t*>(), std::size_t{})) {
        using R = decltype(decode(std::declval<const std::uint8_t*>(), std::size_t{}));
        static_assert(!std::is_void<R>::value, "the decoder returns the decoded value");
        struct state { F* decode; std::optional<R> value; std::exception_ptr error; } st{&decode, std::nullopt, nullptr};
        auto trampoline = [](void* user, const std::uint8_t* data, std::size_t len) noexcept -> std::uint32_t {
            auto& s = *static_cast<state*>(user);
            try { s.value.emplace((*s.decode)(data, len)); return PONKO_OK; }
            catch (...) { s.error = std::current_exception(); return PONKO_INVALID_ARGUMENT; }
        };
        const auto status = ponko_ctx_take_asset_group_bytes(raw_, id, path, trampoline, &st);
        if (st.error) std::rethrow_exception(st.error);
        detail::check(status);
        return std::move(*st.value);
    }
    /// Takes the finished loader of asset `path` of group `id` (added with `asset_group::sliced`),
    /// once; `L` is the loader's type. Throws ponko::error if it isn't there (not finished, or taken).
    template<class L> std::unique_ptr<L> take_asset_group_sliced(std::uint32_t id, const char* path) {
        void* state = nullptr;
        detail::check(ponko_ctx_take_asset_group_sliced(raw_, id, path, &state));
        return std::unique_ptr<L>(static_cast<L*>(state));
    }
    /// Takes a finished value registered with asset_group::decoded, once.
    /// T is the return type of the decoder. Throws if the value is unavailable.
    template<class T> std::unique_ptr<T> take_asset_group_decoded(std::uint32_t id, const char* path) {
        void* state = nullptr;
        detail::check(ponko_ctx_take_asset_group_decoded(raw_, id, path, &state));
        return std::unique_ptr<T>(static_cast<T*>(state));
    }
    /// How long the game thread may spend on asset groups each frame, in milliseconds (default 4).
    void set_asset_group_frame_budget(float milliseconds) {
        detail::check(ponko_ctx_set_asset_group_frame_budget(raw_, milliseconds));
    }
    /// What the game thread's asset group step cost.
    ponko::group_stats asset_group_stats() const {
        ponko::group_stats s{}; detail::check(ponko_ctx_asset_group_stats(raw_, &s)); return s;
    }
    /// Streams the levels of `e`'s LOD group in from `files`, one per level
    /// (Rust `StreamedLod`).
    void set_streamed_lod(entity e, const std::vector<const char*>& files) {
        detail::check(ponko_ctx_set_streamed_lod(raw_, e, files.data(), files.size()));
    }
    /// How many 3D models were drawn last frame.
    std::size_t models_drawn() const { std::size_t n = 0; detail::check(ponko_ctx_models_drawn(raw_, &n)); return n; }
    /// The 3D camera.
    ponko::camera3d camera3d() const { ponko::camera3d c{}; detail::check(ponko_ctx_camera3d(raw_, &c)); return c; }
    /// fov_y between 0 and pi, 0 < near < far.
    void set_camera3d(const ponko::camera3d& c) { detail::check(ponko_ctx_set_camera3d(raw_, &c)); }
    /// The 3D camera in its versioned form (camera3d_ex): everything camera3d() has, plus occlusion_culling
    /// and particles.
    ponko::camera3d_ex camera3d_ex() const {
        ponko::camera3d_ex c{};
        c.struct_size = sizeof(ponko::camera3d_ex);
        c.version = PONKO_CAMERA3D_EX_VERSION;
        detail::check(ponko_ctx_camera3d_ex(raw_, &c));
        return c;
    }
    /// Changes the 3D camera from its versioned form: the checks of set_camera3d, plus occlusion culling and
    /// whether particles, trails and lines are drawn. Nothing changes when it throws.
    void set_camera3d(const ponko::camera3d_ex& c) { detail::check(ponko_ctx_set_camera3d_ex(raw_, &c)); }
    /// Turns the 3D camera to look at `target`, +Y up.
    void camera3d_look_at(vec3 target) { detail::check(ponko_ctx_camera3d_look_at(raw_, target)); }
    /// Skip models hidden behind what was drawn a frame or two before (off by default).
    void set_occlusion_culling(bool on) { detail::check(ponko_ctx_set_occlusion_culling(raw_, on)); }
    /// Whether occlusion culling is on (see set_occlusion_culling).
    bool occlusion_culling() const { bool on = false; detail::check(ponko_ctx_occlusion_culling(raw_, &on)); return on; }
    /// The ray from the 3D camera through a window point (as the mouse
    /// position): its start and unit direction.
    std::pair<vec3, vec3> screen_ray(vec2 screen) const {
        vec3 origin{}, direction{}; detail::check(ponko_ctx_screen_ray(raw_, screen, &origin, &direction)); return {origin, direction};
    }
    /// The window point where 2D world point `world` appears through the 2D camera.
    vec2 world_to_screen(vec2 world) const { vec2 p{}; detail::check(ponko_ctx_world_to_screen(raw_, world, &p)); return p; }
    /// The 2D world point under window point `screen` (mouse_world() for the mouse).
    vec2 screen_to_world(vec2 screen) const { vec2 p{}; detail::check(ponko_ctx_screen_to_world(raw_, screen, &p)); return p; }
    /// The window point where 3D world point `world` appears; std::nullopt behind the camera.
    std::optional<vec2> world_to_screen3d(vec3 world) const {
        bool visible = false; vec2 p{}; detail::check(ponko_ctx_world_to_screen3d(raw_, world, &visible, &p));
        if (!visible) return std::nullopt;
        return p;
    }
    /// The 3D world point under window point `screen`, `depth` units in front of the camera.
    vec3 screen_to_world3d(vec2 screen, float depth) const {
        vec3 p{}; detail::check(ponko_ctx_screen_to_world3d(raw_, screen, depth, &p)); return p;
    }
    /// Where the ray under window point `screen` meets the plane y = `height`; std::nullopt when it
    /// points away from it.
    std::optional<vec3> screen_to_ground(vec2 screen, float height = 0.0f) const {
        bool found = false; vec3 p{}; detail::check(ponko_ctx_screen_to_ground(raw_, screen, height, &found, &p));
        if (!found) return std::nullopt;
        return p;
    }
    /// The main 2D camera as a view: its position, zoom, rectangle and layers.
    view2d camera_view() const { view2d v; detail::check(ponko_ctx_camera_view(raw_, &v)); return v; }
    /// Sets the main 2D camera's position, zoom, rectangle and layers (its clear is the game's).
    void set_camera_view(const view2d& v) { detail::check(ponko_ctx_set_camera_view(raw_, &v)); }
    /// The 2D views (Rust `ctx.views2d`), drawn after the main camera in order.
    std::vector<view2d> views2d() const {
        std::size_t n = 0; detail::check(ponko_ctx_view2d_count(raw_, &n));
        std::vector<view2d> out(n);
        for (std::size_t i = 0; i < n; ++i) detail::check(ponko_ctx_view2d(raw_, i, &out[i]));
        return out;
    }
    /// Sets 2D view `index`; `index` equal to the count adds one at the end.
    void set_view2d(std::size_t index, const view2d& v) { detail::check(ponko_ctx_set_view2d(raw_, index, &v)); }
    /// Adds a 2D view at the end.
    void add_view2d(const view2d& v) {
        std::size_t n = 0; detail::check(ponko_ctx_view2d_count(raw_, &n)); set_view2d(n, v);
    }
    /// Removes 2D view `index`; the ones after it move down one.
    void remove_view2d(std::size_t index) { detail::check(ponko_ctx_remove_view2d(raw_, index)); }
    /// Which 2D camera draws window point `screen`: a view's index, or -1 for the main camera.
    int camera_at(vec2 screen) const { std::int32_t at = -1; detail::check(ponko_ctx_camera_at(raw_, screen, &at)); return at; }
    /// The 2D world point under window point `screen`, through the camera that draws there.
    vec2 screen_to_world_at(vec2 screen) const { vec2 p{}; detail::check(ponko_ctx_screen_to_world_at(raw_, screen, &p)); return p; }
    /// Puts what this frame draws in the world from now on on render layers `layers` (each frame starts on
    /// render_layers_default).
    void set_draw_layers(std::uint32_t layers) { detail::check(ponko_ctx_set_draw_layers(raw_, layers)); }
    /// The main 3D camera as a view: the camera, its rectangle and layers.
    view3d camera3d_view() const { view3d v; detail::check(ponko_ctx_camera3d_view(raw_, &v)); return v; }
    /// Sets the main 3D camera, its rectangle and layers (its clear and post are the game's).
    void set_camera3d_view(const view3d& v) { detail::check(ponko_ctx_set_camera3d_view(raw_, &v)); }
    /// The 3D views (Rust `ctx.scene3d.views`), drawn after the main camera in order.
    std::vector<view3d> views3d() const {
        std::size_t n = 0; detail::check(ponko_ctx_view3d_count(raw_, &n));
        std::vector<view3d> out(n);
        for (std::size_t i = 0; i < n; ++i) detail::check(ponko_ctx_view3d(raw_, i, &out[i]));
        return out;
    }
    /// Sets 3D view `index`; `index` equal to the count adds one at the end.
    void set_view3d(std::size_t index, const view3d& v) { detail::check(ponko_ctx_set_view3d(raw_, index, &v)); }
    /// Adds a 3D view at the end.
    void add_view3d(const view3d& v) {
        std::size_t n = 0; detail::check(ponko_ctx_view3d_count(raw_, &n)); set_view3d(n, v);
    }
    /// Removes 3D view `index`; the ones after it move down one.
    void remove_view3d(std::size_t index) { detail::check(ponko_ctx_remove_view3d(raw_, index)); }
    /// Which 3D camera draws window point `screen`: a view's index, or -1 for the main camera.
    int camera3d_at(vec2 screen) const { std::int32_t at = -1; detail::check(ponko_ctx_camera3d_at(raw_, screen, &at)); return at; }
    /// Where the ray under window point `screen`, through the 3D camera that draws there, meets the plane
    /// y = `height`; std::nullopt when it points away from it.
    std::optional<vec3> screen_to_ground_at(vec2 screen, float height = 0.0f) const {
        bool found = false; vec3 p{}; detail::check(ponko_ctx_screen_to_ground_at(raw_, screen, height, &found, &p));
        if (!found) return std::nullopt;
        return p;
    }
    /// The sun, sky light, shadows and global illumination.
    ponko::lighting lighting() const { ponko::lighting l{}; detail::check(ponko_ctx_lighting(raw_, &l)); return l; }
    /// Intensities and the shadow distance can't be negative; shadow_cascades is 1 to 4,
    /// shadow_refresh 1 to 8, shadow_filter 1 to 4; shadow_resolution is 256 to 4096, light_shadow_resolution
    /// 128 to 2048, light_shadow_views 0 to 24 and light_shadow_atlas a power of two
    /// from 1024 to 8192. With gi_enabled, gi_extent and gi_spacing must be above
    /// zero, gi_probes_per_frame 1 to 16 and gi_intensity not negative.
    void set_lighting(const ponko::lighting& l) { detail::check(ponko_ctx_set_lighting(raw_, &l)); }
    /// The lighting in its versioned form (lighting_ex): everything lighting() has, plus shadow_sun_size and
    /// shadow_sun_step.
    ponko::lighting_ex lighting_ex() const {
        ponko::lighting_ex l = lighting_ex_header();
        detail::check(ponko_ctx_lighting_ex(raw_, &l));
        return l;
    }
    /// Changes the lighting from its versioned form: the checks of set_lighting, plus shadow_sun_size 0 to 10
    /// degrees and shadow_sun_step 0 or more. Nothing changes when it throws.
    void set_lighting(const ponko::lighting_ex& l) { detail::check(ponko_ctx_set_lighting_ex(raw_, &l)); }
    /// Puts a material on the GPU from its versioned description (material_desc_ex).
    material create_material(const material_desc_ex& d) { material m{}; detail::check(ponko_ctx_create_material_ex(raw_, &d, &m)); return m; }
    /// Changes a material from its versioned description.
    void update_material(material m, const material_desc_ex& d) { detail::check(ponko_ctx_update_material_ex(raw_, m, &d)); }
    /// Bakes `sources` into the navmesh with versioned settings (nav_bake_settings_ex).
    void bake_navmesh(const nav_sources& sources, const nav_bake_settings_ex& settings) {
        detail::check(ponko_ctx_bake_navmesh_ex(raw_, sources.raw(), &settings));
    }
    /// The sky behind the 3D world and the fog in front of it.
    ponko::sky sky() const { ponko::sky s{}; detail::check(ponko_ctx_sky(raw_, &s)); return s; }
    /// Haze, stars, fog density, fog_start, fog_end and fog_sun_glow can't be negative;
    /// sun_size and fog_falloff must be above 0; fog_sun_focus is 0 to under 1;
    /// light_shafts is 0 to 1.
    void set_sky(const ponko::sky& s) { detail::check(ponko_ctx_set_sky(raw_, &s)); }
    /// Exposure, bloom, color grading and tone mapping of the 3D frame.
    ponko::post post() const { ponko::post p{}; detail::check(ponko_ctx_post(raw_, &p)); return p; }
    /// GTAO ao_half_resolution uses bilateral upsampling; ao_temporal accumulates
    /// camera-reprojected history independently of TAA. Contact shadows stay full resolution.
    /// Bloom, its threshold, contrast and saturation can't be negative; render_scale is
    /// 0.25 to 2 and sharpen 0 to 1; contact_shadows can't be negative; ssr is 0 to 1 and
    /// ssr_max_distance above zero.
    void set_post(const ponko::post& p) { detail::check(ponko_ctx_set_post(raw_, &p)); }
    /// The post-processing in its versioned form (post_ex): everything post() has, plus split toning, depth
    /// of field and the sample count.
    ponko::post_ex post_ex() const {
        ponko::post_ex p = post_ex_header();
        detail::check(ponko_ctx_post_ex(raw_, &p));
        return p;
    }
    /// Changes the post-processing from its versioned form: the checks of set_post, plus split toning tints
    /// finite and its balance -1 to 1, dof_focus_distance and dof_focus_range not negative, dof_blur 0 to
    /// 0.05 and msaa 2 or 4. Nothing changes when it throws.
    void set_post(const ponko::post_ex& p) { detail::check(ponko_ctx_set_post_ex(raw_, &p)); }
    /// The HDR output settings (Rust `ctx.gfx.hdr_output()`).
    ponko::hdr_output hdr_output() const {
        ponko::hdr_output h{};
        h.struct_size = sizeof(h);
        h.version = PONKO_HDR_OUTPUT_VERSION;
        detail::check(ponko_ctx_hdr_output(raw_, &h));
        return h;
    }
    /// Changes the HDR output settings from the next frame: `enabled` switches the window between HDR
    /// and SDR while the game runs (no restart); paper white, peak and HDR10 metadata follow.
    /// Throws, changing nothing, for a NaN or negative peak_nits or metadata value.
    void set_hdr_output(const ponko::hdr_output& h) { detail::check(ponko_ctx_set_hdr_output(raw_, &h)); }
    /// What the window does with the HDR settings: active, encoding, reason, paper white and peak as used,
    /// and the HDR10 metadata sent.
    ponko::hdr_status hdr_status() const {
        ponko::hdr_status s{};
        s.struct_size = sizeof(s);
        s.version = PONKO_HDR_STATUS_VERSION;
        detail::check(ponko_ctx_hdr_status(raw_, &s));
        return s;
    }
    /// The current graphics quality, gathered from the post and lighting settings.
    ponko::graphics_quality graphics_quality() const {
        ponko::graphics_quality q{}; detail::check(ponko_ctx_graphics_quality(raw_, &q)); return q;
    }
    /// Switches the 3D picture to `q` without a restart: the next frame draws at the new
    /// resolution scale and shadow map sizes. Throws, changing nothing, for a field
    /// outside its range.
    void set_graphics_quality(const ponko::graphics_quality& q) { detail::check(ponko_ctx_set_graphics_quality(raw_, &q)); }
    /// The GPU report as JSON (Rust `ctx.gpu_capabilities()`): adapter, limits,
    /// tier, forced_tier and one {feature, available, fallback, reason} entry per
    /// feature. Empty before a GPU opens (headless, before the window).
    std::optional<std::string> gpu_capabilities() const {
        std::size_t len = 0;
        PonkoStatus status = ponko_ctx_gpu_capabilities(raw_, nullptr, 0, &len);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_gpu_capabilities(raw_, b, c, n); });
    }
    /// The GPU's tier; empty before a GPU opens.
    std::optional<ponko::gpu_tier> gpu_tier() const {
        uint32_t t = 0;
        PonkoStatus status = ponko_ctx_gpu_tier(raw_, &t);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return ponko::gpu_tier(t);
    }
    /// Whether the GPU has feature `f` (false before a GPU opens); the fallback
    /// and the reason are in gpu_capabilities().
    bool gpu_has(ponko::gpu_feature f) const {
        bool on = false;
        PonkoStatus status = ponko_ctx_gpu_feature(raw_, uint32_t(f), &on);
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status); return on;
    }
    /// The preset for the open GPU, fitted to what it has (Rust
    /// `SceneSettings::set_recommended_quality`); empty before a GPU opens.
    std::optional<ponko::graphics_quality> recommended_graphics_quality() const {
        ponko::graphics_quality q{};
        PonkoStatus status = ponko_ctx_recommended_graphics_quality(raw_, &q);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return q;
    }
    /// Makes temporal antialiasing and temporal AO forget earlier frames: call it
    /// after a camera cut or an object teleport. (Resizes and big camera jumps
    /// reset by themselves.)
    void reset_temporal_history() { detail::check(ponko_ctx_reset_temporal_history(raw_)); }
    /// Gives `e`'s model no motion on the next drawn frame, keeping the rest of
    /// the temporal history: call it after teleporting one object, or when its
    /// animation snaps to another pose with no blend.
    void reset_motion(entity e) { detail::check(ponko_ctx_reset_motion(raw_, e)); }
    /// Makes GPU buffer `b` keep a copy of the elements the last frame drew
    /// (`keep`), so TAA and temporal AO see each element's own motion; it
    /// doubles the buffer's memory. Throws ponko::error for a freed buffer.
    void set_gpu_buffer_motion(gpu_buffer b, bool keep) {
        detail::check(ponko_ctx_set_gpu_buffer_motion(raw_, b, keep));
    }
    /// Saves this frame as a PNG at `path` once it is drawn. Does nothing in headless runs.
    void save_screenshot(const char* path) { detail::check(ponko_ctx_save_screenshot(raw_, path)); }
    /// texture_filter::nearest keeps pixel art square.
    void set_texture_filter(texture t, texture_filter f) {
        detail::check(ponko_ctx_set_texture_filter(raw_, t, f == texture_filter::nearest));
    }

    // Shapes (world space).
    /// Fills rectangle `r` in world space this frame.
    void draw_rect(rect r, color c) { detail::check(ponko_ctx_draw_rect(raw_, r, c)); }
    /// Draws a line `width` world units thick from `a` to `b` this frame.
    void draw_line(vec2 a, vec2 b, float width, color c) { detail::check(ponko_ctx_draw_line(raw_, a, b, width, c)); }
    /// Fills a circle in world space this frame.
    void draw_circle(vec2 center, float radius, color c) { detail::check(ponko_ctx_draw_circle(raw_, center, radius, c)); }


    /// Adds or replaces owned SDF text on an entity with a Transform3D.
    void set_text3d(entity e, const ponko::text3d& t) {
        PonkoText3D v{t.font, t.size, t.color, t.outline, t.outline_width,
                       t.softness, static_cast<uint32_t>(t.align), t.billboard};
        detail::check(ponko_ctx_set_text3d(raw_, e, t.text.c_str(), &v));
    }
    /// No value when the live entity has no text.
    std::optional<ponko::text3d> text3d(entity e) const {
        PonkoText3D v{};
        auto status = ponko_ctx_text3d(raw_, e, &v);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        std::size_t len = 0;
        detail::check(ponko_ctx_text3d_text(raw_, e, nullptr, 0, &len));
        std::string text(len + 1, '\0');
        detail::check(ponko_ctx_text3d_text(raw_, e, &text[0], text.size(), &len));
        text.resize(len);
        ponko::text3d t(std::move(text));
        t.font = v.font; t.size = v.size; t.color = v.color; t.outline = v.outline;
        t.outline_width = v.outline_width; t.softness = v.softness;
        t.align = static_cast<text_align3d>(v.align); t.billboard = v.billboard;
        return t;
    }
    /// Removes `e`'s world text; a live entity without text is unchanged.
    void remove_text3d(entity e) { detail::check(ponko_ctx_remove_text3d(raw_, e)); }

    // Fonts and text (screen space).
    /// Loads a font (TTF or OTF); throws ponko::error (kind, path) if it can't.
    font load_font(const char* path) { return try_load_font(path).value(); }
    /// Loads a font, or says why not, without throwing (Rust's `try_load_font`).
    result<font> try_load_font(const char* path) { font f{}; const PonkoStatus status = ponko_ctx_try_load_font(raw_, path, &f); return detail::attempt(status, f); }
    /// Draws text in window pixels with a font, size, color and z (start from text_defaults()). Throws
    /// ponko::error for an unknown or freed font.
    void draw_text(const char* text, vec2 pos, const text_params& params) { detail::check(ponko_ctx_draw_text_ex(raw_, text, pos, &params)); }
    /// The size in pixels draw_text would cover with font `f` at `size`. Throws ponko::error for an unknown
    /// or freed font.
    vec2 measure_text(const char* text, font f, float size) { vec2 v{}; detail::check(ponko_ctx_measure_text_ex(raw_, text, f, size, &v)); return v; }
    /// Sets the fonts tried, in order, for characters `f` has no glyph for (Rust `set_font_fallbacks`); an
    /// empty list turns fallback off. Throws ponko::error for a font this game didn't load.
    void set_font_fallbacks(font f, const std::vector<font>& fallbacks) { detail::check(ponko_ctx_set_font_fallbacks(raw_, f, fallbacks.data(), fallbacks.size())); }
    /// `f`'s fallback chain, with the system fonts found for it (Rust `font_fallbacks`).
    std::vector<font> font_fallbacks(font f) const {
        std::size_t n = 0;
        detail::check(ponko_ctx_font_fallbacks(raw_, f, nullptr, 0, &n));
        std::vector<font> out(n);
        if (n > 0) detail::check(ponko_ctx_font_fallbacks(raw_, f, out.data(), out.size(), &n));
        return out;
    }
    /// Sets which strings are shaped (Rust `set_text_shaping`); needs the library's `text-shaping` feature,
    /// on by default.
    void set_text_shaping(ponko::text_shaping mode) { detail::check(ponko_ctx_set_text_shaping(raw_, static_cast<std::uint32_t>(mode))); }
    /// Which strings are shaped; text_shaping::automatic unless set.
    ponko::text_shaping text_shaping() const { std::uint32_t m = 0; detail::check(ponko_ctx_text_shaping(raw_, &m)); return static_cast<ponko::text_shaping>(m); }
    /// Loads a font installed on the computer by family name ("Arial"); throws ponko::error (kind not_found,
    /// or unsupported without the library's `system-fonts` feature) if it can't.
    font load_system_font(const char* family) { return try_load_system_font(family).value(); }
    /// Loads a system font, or says why not, without throwing (Rust `try_load_system_font`).
    result<font> try_load_system_font(const char* family) { font f{}; const PonkoStatus status = ponko_ctx_try_load_system_font(raw_, family, &f); return detail::attempt(status, f); }
    /// With `on`, characters no loaded font has are looked up among the computer's fonts (Rust
    /// `set_system_font_fallback`). Returns whether this build can search them.
    bool set_system_font_fallback(bool on) { bool available = false; detail::check(ponko_ctx_set_system_font_fallback(raw_, on, &available)); return available; }

    // Localization (Rust `ponko::locale`): string tables, the current language, keyed text.
    /// Loads a string table file from the assets (Rust `assets.load_strings`); the first is the
    /// reference language and the current one. Throws ponko::error if it can't.
    void load_strings(const char* path) { try_load_strings(path).value(); }
    /// load_strings, saying why not instead of throwing (Rust `assets.try_load_strings`).
    result<void> try_load_strings(const char* path) { return detail::attempt(ponko_ctx_load_strings(raw_, path)); }
    /// Adds a string table from its JSON text, or replaces the table of the same language.
    void insert_strings_json(const char* json) { detail::check(ponko_ctx_strings_insert_json(raw_, json)); }
    /// Switches to `language` (a code with a table, or "en-XA", the pseudo-locale); false for an
    /// unknown code, which keeps the current language. Keyed UI text follows on the next frame.
    bool set_language(const char* language) { bool known = false; detail::check(ponko_ctx_strings_set_language(raw_, language, &known)); return known; }
    /// The current language's code.
    std::string language() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ctx_strings_language(raw_, b, c, l); });
    }
    /// The languages with a table, the reference language first: for a language menu.
    std::vector<string_language> languages() const {
        std::size_t n = 0;
        detail::check(ponko_ctx_strings_language_count(raw_, &n));
        std::vector<string_language> out;
        for (std::size_t i = 0; i < n; ++i) {
            std::size_t ignored = 0;
            std::string code = detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ctx_strings_language_at(raw_, i, b, c, l, nullptr, 0, &ignored); });
            std::string name = detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ctx_strings_language_at(raw_, i, nullptr, 0, &ignored, b, c, l); });
            out.push_back(string_language{std::move(code), std::move(name)});
        }
        return out;
    }
    /// Whether the current language is written right to left.
    bool language_is_rtl() const { bool b = false; detail::check(ponko_ctx_strings_is_rtl(raw_, &b)); return b; }
    /// Sets the font `language`'s keyed UI text is drawn with (Rust `strings.set_font`).
    void set_language_font(const char* language, font f) { detail::check(ponko_ctx_strings_set_font(raw_, language, f)); }
    /// The current language's font, if it has one: draw its text with it.
    std::optional<font> language_font() const { bool has = false; font f{}; detail::check(ponko_ctx_strings_font(raw_, &has, &f)); return has ? std::optional<font>(f) : std::nullopt; }
    /// `key`'s text in the current language with `args` in its placeholders (Rust `strings.format`).
    std::string format(const char* key, const std::vector<string_arg>& args = {}) const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ctx_strings_format(raw_, key, args.data(), args.size(), b, c, l); });
    }
    /// Every language's missing keys, plural forms and arguments, one per line; empty when complete.
    std::string missing_strings() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ctx_strings_missing_report(raw_, b, c, l); });
    }

    // UI widgets (screen space), styled by ui_style().
    /// The immediate-mode UI style.
    ponko::ui_style ui_style() const { ponko::ui_style s{}; detail::check(ponko_ctx_ui_style(raw_, &s)); return s; }
    /// Changes the UI style for widgets drawn after this call.
    void set_ui_style(const ponko::ui_style& s) { detail::check(ponko_ctx_set_ui_style(raw_, &s)); }
    /// The font of the immediate labels and buttons (Rust `ctx.ui.immediate.font`): the theme's unless set.
    font ui_immediate_font() const { font f{}; detail::check(ponko_ctx_ui_immediate_font(raw_, &f)); return f; }
    /// Sets the immediate widgets' font; it stays through a later theme change.
    void set_ui_immediate_font(font f) { detail::check(ponko_ctx_set_ui_immediate_font(raw_, f)); }
    /// The thickness of an immediate button's border in UI units (Rust `ctx.ui.immediate.border_width`).
    float ui_immediate_border_width() const { float w = 0; detail::check(ponko_ctx_ui_immediate_border_width(raw_, &w)); return w; }
    /// Sets the immediate buttons' border thickness, in UI units (0: none).
    void set_ui_immediate_border_width(float width) { detail::check(ponko_ctx_set_ui_immediate_border_width(raw_, width)); }
    /// The scale the immediate widgets are drawn at (Rust `UiImmediate::scale`): the screen's UI scale.
    float ui_immediate_scale() const { float sc = 0; detail::check(ponko_ctx_ui_immediate_scale(raw_, &sc)); return sc; }
    /// Whether the mouse is over UI drawn this frame or the last, so clicks on a panel don't also act on the
    /// world behind it.
    bool mouse_over_ui() const { bool b = false; detail::check(ponko_ctx_mouse_over_ui(raw_, &b)); return b; }
    /// Draws a panel background in window pixels this frame.
    void ui_panel(rect r) { detail::check(ponko_ctx_ui_panel(raw_, r)); }
    /// Draws text in the UI style at `pos` (window pixels); returns its size.
    vec2 ui_label(vec2 pos, const char* text) { vec2 v{}; detail::check(ponko_ctx_ui_label(raw_, pos, text, &v)); return v; }
    /// Draws a button in window pixels; true on the frame it was clicked.
    bool ui_button(rect r, const char* label) { bool b = false; detail::check(ponko_ctx_ui_button(raw_, r, label, &b)); return b; }
    /// Draws a checkbox in window pixels; a click flips `on`. True on the frame it changed.
    bool ui_toggle(rect r, const char* label, bool& on) { bool b = false; detail::check(ponko_ctx_ui_toggle(raw_, r, label, &on, &b)); return b; }
    /// Draws a slider for `value` (0 to 1) in window pixels; pressing and dragging sets it. True when it changed.
    bool ui_slider(rect r, float& value) { bool b = false; detail::check(ponko_ctx_ui_slider(raw_, r, &value, &b)); return b; }
    /// Draws a progress bar filled to `value` (0 to 1) in window pixels.
    void ui_progress_bar(rect r, float value) { detail::check(ponko_ctx_ui_progress_bar(raw_, r, value)); }
    /// The retained UI tree (Rust `ctx.ui_retained`): panels, buttons, text inputs and more, with real layout.
    ui::tree ui_retained() const { return ui::tree(raw_); }

    // World-space UI panels (Rust `UiPanel3D`): retained UI drawn into a render target shown on a quad.
    /// Gives `e` (which needs a `transform3d`) a UI panel `width` x `height` pixels and `size` meters
    /// in the world, plus the model that shows it: a glowing, alpha-blended quad facing +Z. Build its
    /// UI with `panel_ui(e)`. Replaces `e`'s model and any panel it had. Returns the panel's render target.
    ponko::render_target create_ui_panel(entity e, std::uint32_t width, std::uint32_t height, vec2 size) {
        ponko::render_target t{}; detail::check(ponko_ctx_create_ui_panel(raw_, e, width, height, size, &t)); return t;
    }
    /// The UI of `e`'s panel (Rust `UiPanel3D::ui`): the same calls as `ui_retained()`, in the panel's pixels.
    ui::tree panel_ui(entity e) const { return ui::tree(raw_, e); }
    /// Removes `e`'s panel; its model stays.
    void remove_ui_panel(entity e) { detail::check(ponko_ctx_remove_ui_panel(raw_, e)); }
    /// Whether the mouse and keyboard can use `e`'s panel (default true).
    void set_ui_panel_interactive(entity e, bool on) { detail::check(ponko_ctx_set_ui_panel_interactive(raw_, e, on)); }
    /// Where the mouse (or the pointer ray) is on `e`'s panel this frame, in its pixels; empty when it isn't over it.
    std::optional<vec2> ui_panel_pointer(entity e) const {
        bool over = false; vec2 at{};
        detail::check(ponko_ctx_ui_panel_pointer(raw_, e, &over, &at));
        return over ? std::optional<vec2>(at) : std::nullopt;
    }
    /// Panels use the ray from `origin` along `direction` instead of the camera's ray through the mouse:
    /// for a locked cursor use the camera's forward ray, for VR a controller's. `clear_ui_pointer_ray` goes back to the mouse.
    void set_ui_pointer_ray(vec3 origin, vec3 direction) { detail::check(ponko_ctx_set_ui_pointer_ray(raw_, origin, direction)); }
    /// Goes back to the camera's ray through the mouse for world-space panels.
    void clear_ui_pointer_ray() { detail::check(ponko_ctx_clear_ui_pointer_ray(raw_)); }

    // Sound (silent in headless runs).
    /// Loads a sound into memory (OGG, MP3, FLAC, WAV, AIFF); throws ponko::error (kind, path) if it can't.
    sound load_sound(const char* path) { return try_load_sound(path).value(); }
    /// Loads a sound, or says why not, without throwing (Rust's `try_load`).
    result<sound> try_load_sound(const char* path) { sound s{}; const PonkoStatus status = ponko_ctx_try_load_sound(raw_, path, &s); return detail::attempt(status, s); }
    /// A mono sound from `len` samples in -1..1 at `sample_rate` per second. Throws ponko::error for an empty
    /// buffer or a zero rate.
    sound create_sound(std::uint32_t sample_rate, const float* samples, std::size_t len) {
        sound s{}; detail::check(ponko_ctx_create_sound(raw_, sample_rate, samples, len, &s)); return s;
    }
    /// The sounds started since this tick (or frame) began, oldest first (Rust `ctx.audio.started()`).
    /// Headless runs hear nothing but still list them, so a test can check what was played.
    std::vector<sound_started> sounds_started() const {
        std::size_t n = 0;
        detail::check(ponko_ctx_sounds_started(raw_, &n));
        std::vector<sound_started> list(n);
        for (std::size_t i = 0; i < n; ++i) {
            list[i].struct_size = sizeof(sound_started);
            list[i].version = PONKO_SOUND_STARTED_VERSION;
            detail::check(ponko_ctx_sound_started(raw_, i, &list[i]));
        }
        return list;
    }
    /// Plays `s` once at full volume on the sfx bus. Silent in headless runs.
    playing play_sound(sound s) { playing p{}; detail::check(ponko_ctx_play_sound(raw_, s, nullptr, &p)); return p; }
    /// Plays `s` with `params`. Silent in headless runs.
    playing play_sound(sound s, const sound_params& params) { playing p{}; detail::check(ponko_ctx_play_sound(raw_, s, &params, &p)); return p; }
    /// Stops a playback; stopping one that already ended does nothing.
    void stop_sound(playing p) { detail::check(ponko_ctx_stop_sound(raw_, p)); }
    /// A long sound (music) streamed from disk as it plays.
    sound load_stream(const char* path) { return try_load_stream(path).value(); }
    /// Opens a sound for streaming, or says why not, without throwing (Rust's `try_load_stream`).
    result<sound> try_load_stream(const char* path) { sound s{}; const PonkoStatus status = ponko_ctx_try_load_stream(raw_, path, &s); return detail::attempt(status, s); }
    /// The bus named `name`, made the first time; keep the handle.
    PonkoAudioBus audio_bus(const char* name) { PonkoAudioBus b{}; detail::check(ponko_ctx_audio_bus(raw_, name, &b)); return b; }
    /// The bus named `name` that already exists, without making one (Rust's `Audio::find_bus`); a name no bus
    /// has fails, and the error lists the buses. `.value()` throws it.
    result<PonkoAudioBus> find_audio_bus(const char* name) {
        PonkoAudioBus b{}; const PonkoStatus status = ponko_ctx_audio_find_bus(raw_, name, &b); return detail::attempt(status, b);
    }
    /// Sets bus `b`'s volume, 0 (silent) to 1 (as recorded) or more.
    void set_audio_bus_volume(PonkoAudioBus b, float volume) { detail::check(ponko_ctx_set_audio_bus_volume(raw_, b, volume)); }
    /// Mutes or unmutes bus `b`, keeping its volume.
    void mute_audio_bus(PonkoAudioBus b, bool muted) { detail::check(ponko_ctx_mute_audio_bus(raw_, b, muted)); }
    /// The volume set on bus `b`, before snapshots, parents and mutes.
    float audio_bus_volume(PonkoAudioBus b) { float v = 1; detail::check(ponko_ctx_audio_bus_volume(raw_, b, &v)); return v; }
    /// Whether bus `b` is muted.
    bool audio_bus_muted(PonkoAudioBus b) { bool m = false; detail::check(ponko_ctx_audio_bus_muted(raw_, b, &m)); return m; }
    /// The gain bus `b` is heard at now: snapshots, parents and mutes included, ducking not.
    float audio_bus_gain(PonkoAudioBus b) { float g = 1; detail::check(ponko_ctx_audio_bus_gain(raw_, b, &g)); return g; }
    /// Puts bus `b` under `parent` (empty: straight to the output). Throws on a loop.
    void set_audio_bus_parent(PonkoAudioBus b, std::optional<PonkoAudioBus> parent) {
        detail::check(ponko_ctx_set_audio_bus_parent(raw_, b, parent ? &*parent : nullptr));
    }
    /// The bus `b` is under, if any.
    std::optional<PonkoAudioBus> audio_bus_parent(PonkoAudioBus b) {
        PonkoAudioBus p{}; bool has = false;
        detail::check(ponko_ctx_audio_bus_parent(raw_, b, &p, &has));
        return has ? std::optional<PonkoAudioBus>(p) : std::nullopt;
    }
    /// Replaces bus `b`'s effect chain; an empty list clears it.
    void set_audio_bus_effects(PonkoAudioBus b, const std::vector<bus_effect>& effects) {
        detail::check(ponko_ctx_set_audio_bus_effects(raw_, b, effects.data(), effects.size()));
    }
    /// Bus `b`'s effect chain as set (clamped), before snapshots.
    std::vector<bus_effect> audio_bus_effects(PonkoAudioBus b) {
        std::size_t n = 0; detail::check(ponko_ctx_audio_bus_effects(raw_, b, nullptr, 0, &n));
        std::vector<bus_effect> list(n);
        if (n) detail::check(ponko_ctx_audio_bus_effects(raw_, b, list.data(), n, &n));
        return list;
    }
    /// Replaces the buses that duck bus `b` (at most four); an empty list turns ducking off.
    void set_audio_bus_ducking(PonkoAudioBus b, const std::vector<ducking>& keys) {
        detail::check(ponko_ctx_set_audio_bus_ducking(raw_, b, keys.data(), keys.size()));
    }
    /// What ducks bus `b`, as set (clamped).
    std::vector<ducking> audio_bus_ducking(PonkoAudioBus b) {
        std::size_t n = 0; detail::check(ponko_ctx_audio_bus_ducking(raw_, b, nullptr, 0, &n));
        std::vector<ducking> list(n);
        if (n) detail::check(ponko_ctx_audio_bus_ducking(raw_, b, list.data(), n, &n));
        return list;
    }
    /// Adds snapshot `name` (or replaces the one of that name).
    void add_audio_snapshot(const char* name, const std::vector<bus_snapshot>& buses) {
        std::vector<PonkoBusSnapshot> raw;
        raw.reserve(buses.size());
        for (const bus_snapshot& b : buses) {
            raw.push_back({b.bus, b.volume.has_value(), b.volume.value_or(1.0f), b.effects.data(), b.effects.size()});
        }
        detail::check(ponko_ctx_add_audio_snapshot(raw_, name, raw.data(), raw.size()));
    }
    /// Starts blending to snapshot `name` over `fade` seconds; false for an unknown name.
    bool start_audio_snapshot(const char* name, float fade) {
        const PonkoStatus status = ponko_ctx_start_audio_snapshot(raw_, name, fade);
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// Blends snapshot `name` back out over `fade` seconds.
    void stop_audio_snapshot(const char* name, float fade) { detail::check(ponko_ctx_stop_audio_snapshot(raw_, name, fade)); }
    /// How much of snapshot `name` is heard, 0 to 1.
    float audio_snapshot_weight(const char* name) {
        float w = 0; detail::check(ponko_ctx_audio_snapshot_weight(raw_, name, &w)); return w;
    }
    /// Resolves a snapshot's name once (Rust's `Audio::snapshot_id`), for the snapshot calls below, which then
    /// skip the lookup. A name no snapshot has fails, and the error lists the snapshots; `.value()` throws it.
    result<ponko::audio_snapshot_id> audio_snapshot_id(const char* name) {
        ponko::audio_snapshot_id id; const PonkoStatus status = ponko_ctx_audio_snapshot_id(raw_, name, &id.bits);
        return detail::attempt(status, id);
    }
    /// start_audio_snapshot by an id from audio_snapshot_id; false for an id that names no snapshot.
    bool start_audio_snapshot(ponko::audio_snapshot_id id, float fade) {
        const PonkoStatus status = ponko_ctx_start_audio_snapshot_id(raw_, id.bits, fade);
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// stop_audio_snapshot by an id from audio_snapshot_id.
    void stop_audio_snapshot(ponko::audio_snapshot_id id, float fade) { detail::check(ponko_ctx_stop_audio_snapshot_id(raw_, id.bits, fade)); }
    /// audio_snapshot_weight by an id from audio_snapshot_id.
    float audio_snapshot_weight(ponko::audio_snapshot_id id) {
        float w = 0; detail::check(ponko_ctx_audio_snapshot_weight_id(raw_, id.bits, &w)); return w;
    }
    /// Loads the mixer (buses, effects, ducking, snapshots) from a JSON file in the assets; it hot reloads.
    void load_audio_mixer(const char* path) { detail::check(ponko_ctx_load_audio_mixer(raw_, path)); }
    /// Applies a mixer definition held in a string, in the mixer file's format.
    void apply_audio_mixer_json(const std::string& json) { detail::check(ponko_ctx_apply_audio_mixer_json(raw_, json.c_str())); }
    /// The mixer state to save with the settings (bus volumes, mutes, snapshots on), as JSON.
    std::string audio_mixer_state() {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_audio_mixer_state(raw_, b, c, n); });
    }
    /// Restores a state from audio_mixer_state(); load the mixer file first.
    void restore_audio_mixer_state(const std::string& json) {
        detail::check(ponko_ctx_restore_audio_mixer_state(raw_, json.c_str()));
    }
    /// Plays `s` on bus `b` with `params` and `effects` (low-pass, reverb send).
    playing play_sound_on(sound s, PonkoAudioBus b, const sound_params& params = sound_defaults(),
                          const audio_effects& effects = audio_effects_defaults()) {
        playing p{}; detail::check(ponko_ctx_play_sound_on(raw_, s, &params, b, &effects, &p)); return p;
    }
    /// Changes a playing voice's low-pass and reverb, smoothly.
    void set_sound_effects(playing p, const audio_effects& effects) {
        detail::check(ponko_ctx_set_sound_effects(raw_, p, &effects));
    }
    /// Plays a sound from a point; it follows the listener every frame.
    playing play_sound_at(sound s, vec3 position, const audio_settings3d& settings = audio_settings3d_defaults()) {
        playing p{}; detail::check(ponko_ctx_play_sound_at(raw_, s, position, &settings, &p)); return p;
    }
    /// Makes `e` (with a 3D transform) play `s` from where it is, following it.
    void set_audio_source3d(entity e, sound s, const audio_settings3d& settings = audio_settings3d_defaults()) {
        detail::check(ponko_ctx_set_audio_source3d(raw_, e, s, &settings));
    }
    /// play_sound_at with a keyed distance curve instead of the settings' rolloff (Rust
    /// `AudioRolloff::Keyed`): keys sorted by distance, 0 at min_distance, 1 at max_distance. Each distinct
    /// curve is kept for the rest of the program: make curves once.
    playing play_sound_at(sound s, vec3 position, const audio_settings3d& settings, const std::vector<audio_key>& curve) {
        playing p{}; detail::check(ponko_ctx_play_sound_at_keyed(raw_, s, position, &settings, curve.data(), curve.size(), &p)); return p;
    }
    /// set_audio_source3d with a keyed distance curve, as for play_sound_at.
    void set_audio_source3d(entity e, sound s, const audio_settings3d& settings, const std::vector<audio_key>& curve) {
        detail::check(ponko_ctx_set_audio_source3d_keyed(raw_, e, s, &settings, curve.data(), curve.size()));
    }
    /// Whether sound can play yet (Rust `Audio::unlocked`): on the web false until the first click or key;
    /// always true on the desktop.
    bool audio_unlocked() const { bool u = false; detail::check(ponko_ctx_audio_unlocked(raw_, &u)); return u; }
    /// Stops `e`'s positional sound and takes it off; nothing if it has none.
    void remove_audio_source3d(entity e) { detail::check(ponko_ctx_remove_audio_source3d(raw_, e)); }
    /// Puts the ears somewhere other than the camera; std::nullopt follows the camera again.
    void set_audio_listener(std::optional<audio_listener3d> l) {
        detail::check(ponko_ctx_set_audio_listener(raw_, l ? &*l : nullptr));
    }
    /// Where the ears are: the listener set above, or else the camera.
    audio_listener3d audio_listener() { audio_listener3d l{}; detail::check(ponko_ctx_audio_listener(raw_, &l)); return l; }

    // Fades, loop points and voice limits.
    /// Fades playback `p` to `volume` (1: as played) over `seconds`, 0 at once (Rust `Audio::set_volume`).
    void set_sound_volume(playing p, float volume, float seconds = 0) {
        detail::check(ponko_ctx_set_sound_volume(raw_, p, volume, seconds));
    }
    /// Changes playback `p`'s speed, and with it the pitch (1: as played), over `seconds`.
    void set_sound_speed(playing p, float speed, float seconds = 0) {
        detail::check(ponko_ctx_set_sound_speed(raw_, p, speed, seconds));
    }
    /// Fades playback `p` out over `seconds`, then stops it.
    void fade_out_sound(playing p, float seconds) { detail::check(ponko_ctx_fade_out_sound(raw_, p, seconds)); }
    /// How important playback `p` is against the voice limits: 0-255, higher first, 128 unless set.
    void set_sound_priority(playing p, std::uint8_t priority) { detail::check(ponko_ctx_set_sound_priority(raw_, p, priority)); }
    /// Whether playback `p` is virtual: followed but not heard (over the voice limits, or inaudible).
    bool sound_is_virtual(playing p) { bool v = false; detail::check(ponko_ctx_sound_is_virtual(raw_, p, &v)); return v; }
    /// How many sounds are heard and how many are virtual, music not counted.
    ponko::voice_count audio_voice_count() {
        ponko::voice_count c; detail::check(ponko_ctx_audio_voice_count(raw_, &c.real, &c.virtualized)); return c;
    }
    /// How many sounds are heard at once at most, 1-256 (64 unless set).
    void set_audio_voice_limit(std::uint32_t limit) { detail::check(ponko_ctx_set_audio_voice_limit(raw_, limit)); }
    /// The limit set_audio_voice_limit set.
    std::uint32_t audio_voice_limit() { std::uint32_t l = 0; detail::check(ponko_ctx_audio_voice_limit(raw_, &l)); return l; }
    /// How many sounds playing on bus `b` itself are heard at once at most; std::nullopt: only the overall limit.
    void set_audio_bus_voice_limit(PonkoAudioBus b, std::optional<std::uint32_t> limit) {
        detail::check(ponko_ctx_set_audio_bus_voice_limit(raw_, b, limit.value_or(0)));
    }
    /// The limit set_audio_bus_voice_limit set for bus `b`.
    std::optional<std::uint32_t> audio_bus_voice_limit(PonkoAudioBus b) {
        std::uint32_t l = 0; detail::check(ponko_ctx_audio_bus_voice_limit(raw_, b, &l));
        return l ? std::optional<std::uint32_t>(l) : std::nullopt;
    }
    /// Loop points of `s` in seconds: a looping playback plays to `end` (std::nullopt: the end of the sound),
    /// then goes back to `start`, so an intro plays once. For playbacks started from now on.
    void set_sound_loop(sound s, float start, std::optional<float> end = std::nullopt) {
        detail::check(ponko_ctx_set_sound_loop(raw_, s, start, end.value_or(-1.0f)));
    }
    /// The loop points set_sound_loop set: (0, std::nullopt), the whole sound, unless set.
    std::pair<float, std::optional<float>> sound_loop(sound s) {
        float start = 0, end = -1; detail::check(ponko_ctx_sound_loop(raw_, s, &start, &end));
        return {start, end < 0 ? std::nullopt : std::optional<float>(end)};
    }
    /// The priority `e`'s positional sound starts at; set it right after set_audio_source3d.
    void set_audio_source3d_priority(entity e, std::uint8_t priority) {
        detail::check(ponko_ctx_set_audio_source3d_priority(raw_, e, priority));
    }
    /// Pauses bus `b` and the buses under it, or resumes it: its sounds hold where they are.
    void pause_audio_bus(PonkoAudioBus b, bool paused) { detail::check(ponko_ctx_pause_audio_bus(raw_, b, paused)); }
    /// Whether bus `b` is paused: by pause_audio_bus, by a game state (game::pause_audio_in) or a bus above it.
    bool audio_bus_paused(PonkoAudioBus b) { bool p = false; detail::check(ponko_ctx_audio_bus_paused(raw_, b, &p)); return p; }

    // Music (Rust `Audio::play_music`): layered songs that stay in sync, changes on the beat.
    /// Plays a song of `layers` (each a sound and the volume it starts at) at `bpm` with `beats_per_bar`, the
    /// old song fading out over `fade` seconds; it starts at `at` (0 bpm: no grid, at once).
    void play_music(const std::vector<std::pair<sound, float>>& layers, float bpm = 0, std::uint32_t beats_per_bar = 4,
                    float fade = 0, quantize at = PONKO_QUANTIZE_NOW) {
        std::vector<sound> sounds; std::vector<float> volumes;
        for (const auto& [s, v] : layers) { sounds.push_back(s); volumes.push_back(v); }
        detail::check(ponko_ctx_play_music(raw_, sounds.data(), volumes.data(), sounds.size(), bpm, beats_per_bar, fade, at));
    }
    /// Stops the music, fading out over `fade` seconds from `at`.
    void stop_music(float fade = 0, quantize at = PONKO_QUANTIZE_NOW) { detail::check(ponko_ctx_stop_music(raw_, fade, at)); }
    /// Fades layer `layer` of the song to `volume` over `fade` seconds from `at`: bring the drums in on the bar.
    void set_music_layer(std::size_t layer, float volume, float fade = 0, quantize at = PONKO_QUANTIZE_NOW) {
        detail::check(ponko_ctx_set_music_layer(raw_, layer, volume, fade, at));
    }
    /// Plays `s` once over the music at `at`, on the music's bus.
    playing play_stinger(sound s, quantize at = PONKO_QUANTIZE_BEAT) {
        playing p{}; detail::check(ponko_ctx_play_stinger(raw_, s, at, &p)); return p;
    }
    /// The music's speed (and tempo, and pitch), 1 as written, over `fade` seconds.
    void set_music_speed(float speed, float fade = 0) { detail::check(ponko_ctx_set_music_speed(raw_, speed, fade)); }
    /// The bus music plays on (`music`, made the first time, unless set).
    void set_music_bus(PonkoAudioBus b) { detail::check(ponko_ctx_set_music_bus(raw_, b)); }
    /// The bus music plays on.
    PonkoAudioBus music_bus() { PonkoAudioBus b{}; detail::check(ponko_ctx_music_bus(raw_, &b)); return b; }
    /// Beats since the playing song started; std::nullopt when none plays.
    std::optional<double> music_beat() {
        double beat = 0; bool playing = false; detail::check(ponko_ctx_music_beat(raw_, &beat, &playing));
        return playing ? std::optional<double>(beat) : std::nullopt;
    }
    /// The volume layer `layer` of the playing song is set to.
    float music_layer_volume(std::size_t layer) {
        float v = 0; detail::check(ponko_ctx_music_layer_volume(raw_, layer, &v)); return v;
    }

    // Output devices and the offline mix.
    /// The output devices as JSON, `[{"id": ..., "name": ..., "default": ...}]`; empty in headless runs.
    std::string audio_output_devices() {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_audio_output_devices(raw_, b, c, n); });
    }
    /// Plays on the device with this id or name from now on (nullptr: the system's default); false if none has it.
    bool set_audio_output_device(const char* device) {
        const PonkoStatus status = ponko_ctx_set_audio_output_device(raw_, device);
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// The id of the device sound plays on now; empty for none.
    std::string audio_output_device() {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_audio_output_device(raw_, b, c, n); });
    }
    /// What happened to the output this frame as JSON: `[{"lost": name}, {"switched": name}]`.
    std::string audio_device_events() {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* n) { return ponko_ctx_audio_device_events(raw_, b, c, n); });
    }
    /// Mixes into memory instead of a device from now on, at `sample_rate`; works headless, for tests.
    void open_audio_offline(std::uint32_t sample_rate) { detail::check(ponko_ctx_open_audio_offline(raw_, sample_rate)); }
    /// The next `seconds` of the offline mix, interleaved left/right.
    std::vector<float> render_audio_offline(float seconds) {
        std::size_t n = 0;
        detail::check(ponko_ctx_render_audio_offline(raw_, seconds, nullptr, 0, &n));
        std::vector<float> mix(n);
        if (n) detail::check(ponko_ctx_render_audio_offline(raw_, seconds, mix.data(), n, &n));
        return mix;
    }

    /// Spreads work over `every` ticks (safe from query_par callbacks).
    ponko::turns turns(std::uint32_t every) const { ponko::turns t; detail::check(ponko_ctx_turns(raw_, every, &t.raw)); return t; }
    /// Textures reloaded from changed files at the start of this frame (hot
    /// reload). They keep their handles; redo anything computed from them.
    std::vector<texture> reloaded_textures() const {
        const PonkoTexture* list = nullptr; std::size_t n = 0;
        detail::check(ponko_ctx_reloaded_textures(raw_, &list, &n));
        return std::vector<texture>(list, list + n);
    }

    // Commands: applied after the system; usable inside world().each (not each_par).
    /// Despawns `e` after this system (stale entities are ignored).
    void despawn_later(entity e) { detail::check(ponko_ctx_despawn_later(raw_, e)); }
    /// Despawns `e` and everything under it after this system (Rust
    /// `ctx.commands.despawn_recursive`): the subtree as it is then.
    void despawn_recursive_later(entity e) { detail::check(ponko_ctx_despawn_recursive_later(raw_, e)); }
    // (A type used for the first time inside a query can't be registered
    // yet; such commands and events are sent right after the system returns.)
    /// Spawns an entity with `components` after this system (Rust
    /// `ctx.commands.spawn`) and returns it now: later commands of this
    /// system can use it; it is alive once the system's commands apply.
    template<class... Ts> entity spawn_later(const Ts&... components) {
        static_assert(!std::disjunction<std::is_same<Ts, polygon>...>::value, "spawn polygon colliders with world.spawn");
        const std::array<std::optional<std::uint32_t>, sizeof...(Ts)> ids{{find_id<Ts>()...}};
        entity e = 0;
        if (!std::all_of(ids.begin(), ids.end(), [](const auto& id) { return id.has_value(); })) {
            // Spawned empty now, so the id exists; the components follow
            // once their types are registered, in the same apply.
            detail::check(ponko_ctx_spawn_later_entity(raw_, nullptr, 0, &e));
            after_system([=](ctx& c) { (c.set_later(e, components), ...); });
            return e;
        }
        std::size_t i = 0;
        std::array<PonkoValue, sizeof...(Ts)> args{{PonkoValue{*ids[i++], &components, sizeof(Ts)}...}};
        detail::check(ponko_ctx_spawn_later_entity(raw_, args.data(), args.size(), &e));
        return e;
    }
    /// Adds or replaces `e`'s `T` after this system; a stale `e` is ignored.
    template<class T> void set_later(entity e, const T& component) {
        static_assert(!std::is_same<T, polygon>::value, "set polygon colliders with world.set");
        const auto id = find_id<T>();
        if (!id) { after_system([=](ctx& c) { c.set_later(e, component); }); return; }
        PonkoValue v{*id, &component, sizeof(T)};
        detail::check(ponko_ctx_set_later(raw_, e, &v));
    }
    /// Removes `e`'s `T` after this system; a stale `e` is ignored.
    template<class T> void remove_later(entity e) {
        const auto id = find_id<T>();
        if (!id) { after_system([=](ctx& c) { c.remove_later<T>(e); }); return; }
        detail::check(ponko_ctx_remove_later(raw_, e, *id));
    }

    // Events: messages between systems, any plain struct. Usable inside
    // world().each (not each_par).
    /// Sends `event` (any plain struct, at most 16-byte aligned) to the systems that read `T`s; it lives for
    /// two ticks or frames (Rust `ctx.events.send`).
    template<class T> void send(const T& event) {
        static_assert(alignof(T) <= 16, "events can be at most 16-byte aligned");
        const auto id = find_id<T>();
        if (!id) { after_system([=](ctx& c) { c.send(event); }); return; }
        detail::check(ponko_ctx_send_event(raw_, *id, &event, sizeof(T)));
    }
    /// The events of type `T` this system hasn't read yet, oldest first;
    /// valid until the next send of this type or the end of the system.
    template<class T> events_view<T> read() {
        static_assert(alignof(T) <= 16, "events can be at most 16-byte aligned");
        const void* data = nullptr;
        std::size_t count = 0;
        const auto id = find_id<T>();
        if (!id) return events_view<T>(nullptr, 0);  // never used, so never sent
        detail::check(ponko_ctx_read_events(raw_, *id, sizeof(T), &data, &count));
        return events_view<T>(static_cast<const T*>(data), count);
    }

    // Game states (game::state). Changes apply at the end of the tick that
    // asks (between ticks for frame systems); the last request wins.
    /// The game's current state, as the game's enum `S`.
    template<class S> S state() {
        std::uint32_t s = 0;
        detail::check(ponko_ctx_state(raw_, &s));
        return static_cast<S>(s);
    }
    /// Ticks run with the current state on top, this one included; every
    /// change resets it, a pause pushed over it doesn't advance it.
    std::uint64_t state_ticks() {
        std::uint64_t ticks = 0;
        detail::check(ponko_ctx_state_ticks(raw_, &ticks));
        return ticks;
    }
    /// Whether `s` is the current state or one it was pushed over.
    template<class S> bool in_state_stack(S s) {
        bool found = false;
        detail::check(ponko_ctx_state_in_stack(raw_, static_cast<std::uint32_t>(s), &found));
        return found;
    }
    /// Asks to leave every state on the stack and enter `s`.
    template<class S> void set_state(S s) { detail::check(ponko_ctx_set_state(raw_, static_cast<std::uint32_t>(s))); }
    /// Asks to enter `s` over the current state, which stays without being left.
    template<class S> void push_state(S s) { detail::check(ponko_ctx_push_state(raw_, static_cast<std::uint32_t>(s))); }
    /// Asks to leave the current state for the one it was pushed over.
    void pop_state() { detail::check(ponko_ctx_pop_state(raw_)); }

    // Physics (game::physics), as of the last physics step. Changing physics is not allowed inside query
    // callbacks; the casts, overlaps, closest_point and penetrations are (not in each_par).
    /// Changes gravity for every dynamic 2D body, in pixels per second².
    void set_gravity(vec2 gravity) { detail::check(ponko_ctx_set_gravity(raw_, gravity)); }
    /// Holds the 2D physics step (`true`) or lets it run again; every other system keeps
    /// running. While held, bodies stay put and no collision events are sent.
    void set_physics_paused(bool paused) { detail::check(ponko_ctx_set_physics_paused(raw_, paused)); }
    /// Whether `set_physics_paused` holds the 2D physics step.
    bool is_physics_paused() {
        bool paused = false;
        detail::check(ponko_ctx_is_physics_paused(raw_, &paused));
        return paused;
    }
    /// The first collider a ray from `origin` towards `direction` hits within `max_distance`.
    std::optional<ray_hit> cast_ray(vec2 origin, vec2 direction, float max_distance) {
        bool found = false;
        ray_hit hit{};
        detail::check(ponko_ctx_cast_ray(raw_, origin, direction, max_distance, &found, &hit));
        if (!found) return std::nullopt;
        return hit;
    }
    /// Common explicit-filter spelling; the old cast_ray call is unchanged.
    std::optional<ray_hit> cast_ray_filtered(vec2 origin, vec2 direction, float max_distance, filter2d filter) {
        return cast_ray(origin, direction, max_distance, filter);
    }
    /// The closest collider a ray hits within `max_distance`, seen through `filter` (Rust
    /// `d2::Physics::cast_ray`).
    std::optional<ray_hit> cast_ray(vec2 origin, vec2 direction, float max_distance, filter2d filter) {
        bool found = false; ray_hit hit{};
        detail::check(ponko_ctx_cast_ray2d(raw_, origin, direction, max_distance, filter.raw, filter.raw_groups,
                                           &found, &hit));
        if (!found) return std::nullopt;
        return hit;
    }
    /// Every collider a ray hits within `max_distance`, nearest first (equal distances by entity id), into
    /// `out` (reusing its capacity).
    void cast_ray_all(vec2 origin, vec2 direction, float max_distance, std::vector<ray_hit>& out, filter2d filter = {}) {
        fill_list(out, [&](ray_hit* data, std::size_t cap, std::size_t* n) {
            return ponko_ctx_cast_ray2d_all(raw_, origin, direction, max_distance, filter.raw, filter.raw_groups,
                                            data, cap, n);
        });
    }
    /// Sweeps a circle: the first collider it touches within `max_distance`.
    std::optional<ray_hit> cast_circle(vec2 origin, float radius, vec2 direction, float max_distance,
                                       filter2d filter = {}) {
        bool found = false; ray_hit hit{};
        detail::check(ponko_ctx_cast_circle2d(raw_, origin, radius, direction, max_distance, filter.raw,
                                              filter.raw_groups, &found, &hit));
        if (!found) return std::nullopt;
        return hit;
    }
    /// Sweeps a rectangle of `size` turned by `rotation` radians: the first collider it touches.
    std::optional<ray_hit> cast_box(vec2 origin, vec2 size, float rotation, vec2 direction, float max_distance,
                                    filter2d filter = {}) {
        bool found = false; ray_hit hit{};
        detail::check(ponko_ctx_cast_box2d(raw_, origin, size, rotation, direction, max_distance, filter.raw,
                                           filter.raw_groups, &found, &hit));
        if (!found) return std::nullopt;
        return hit;
    }
    /// Sweeps an upright capsule centered on `origin`: the first collider it touches.
    std::optional<ray_hit> cast_capsule(vec2 origin, float height, float radius, vec2 direction, float max_distance,
                                        filter2d filter = {}) {
        bool found = false; ray_hit hit{};
        detail::check(ponko_ctx_cast_capsule2d(raw_, origin, height, radius, direction, max_distance, filter.raw,
                                               filter.raw_groups, &found, &hit));
        if (!found) return std::nullopt;
        return hit;
    }
    /// Entities whose 2D collider contains `point`, into `out`.
    void overlap_point(vec2 point, std::vector<PonkoEntity>& out, filter2d filter = {}) {
        fill_list(out, [&](PonkoEntity* data, std::size_t cap, std::size_t* n) {
            return ponko_ctx_overlap_point2d(raw_, point, filter.raw, filter.raw_groups, data, cap, n);
        });
    }
    /// Entities whose 2D collider overlaps the circle, into `out`.
    void overlap_circle(vec2 center, float radius, std::vector<PonkoEntity>& out, filter2d filter = {}) {
        fill_list(out, [&](PonkoEntity* data, std::size_t cap, std::size_t* n) {
            return ponko_ctx_overlap_circle2d(raw_, center, radius, filter.raw, filter.raw_groups, data, cap, n);
        });
    }
    /// Entities whose 2D collider overlaps the rectangle of `size` turned by `rotation` radians, into `out`.
    void overlap_box(vec2 center, vec2 size, float rotation, std::vector<PonkoEntity>& out, filter2d filter = {}) {
        fill_list(out, [&](PonkoEntity* data, std::size_t cap, std::size_t* n) {
            return ponko_ctx_overlap_box2d(raw_, center, size, rotation, filter.raw, filter.raw_groups, data, cap, n);
        });
    }
    /// Entities whose 2D collider overlaps the upright capsule centered on `center`, into `out`.
    void overlap_capsule(vec2 center, float height, float radius, std::vector<PonkoEntity>& out,
                         filter2d filter = {}) {
        fill_list(out, [&](PonkoEntity* data, std::size_t cap, std::size_t* n) {
            return ponko_ctx_overlap_capsule2d(raw_, center, height, radius, filter.raw, filter.raw_groups, data,
                                               cap, n);
        });
    }
    /// The nearest 2D collider to `point` within `max_distance` and its nearest point; a point inside a
    /// collider is its own nearest point.
    std::optional<point_hit2d> closest_point(vec2 point, float max_distance, filter2d filter = {}) {
        bool found = false; point_hit2d hit{};
        detail::check(ponko_ctx_closest_point2d(raw_, point, max_distance, filter.raw, filter.raw_groups, &found,
                                                &hit));
        if (!found) return std::nullopt;
        return hit;
    }
    /// Every 2D collider that `shape` (a circle, rect or capsule collider), placed at `position` and turned
    /// by `rotation` radians, overlaps: how deep and which way out, deepest first, into `out`.
    void penetrations(const collider& shape, vec2 position, float rotation, std::vector<penetration2d>& out,
                      filter2d filter = {}) {
        fill_list(out, [&](penetration2d* data, std::size_t cap, std::size_t* n) {
            return ponko_ctx_penetrations2d(raw_, shape, position, rotation, filter.raw, filter.raw_groups, data,
                                            cap, n);
        });
    }
    /// 3D form of cast_ray_filtered: the first collider hit, or nothing (same as cast_ray3d).
    std::optional<hit3d> cast_ray_filtered(vec3 origin, vec3 direction, float max_distance, filter3d filter) {
        return cast_ray3d(origin, direction, max_distance, filter);
    }
    /// A circle in 2D and a sphere in 3D, with matching argument order.
    void overlap_ball(vec2 center, float radius, filter2d filter, std::vector<PonkoEntity>& out) {
        overlap_circle(center, radius, out, filter);
    }
    /// Entities whose 3D collider touches a sphere, into `out` (same as overlap_sphere3d).
    void overlap_ball(vec3 center, float radius, filter3d filter, std::vector<PonkoEntity>& out) {
        overlap_sphere3d(center, radius, out, filter);
    }
    /// Entities whose collider contains `point`, into `out`.
    void entities_at(vec2 point, std::vector<PonkoEntity>& out) {
        std::size_t n = 0;
        out.resize(out.capacity());
        detail::check(ponko_ctx_physics_at(raw_, point, out.data(), out.size(), &n));
        if (n > out.size()) { out.resize(n); detail::check(ponko_ctx_physics_at(raw_, point, out.data(), out.size(), &n)); }
        out.resize(n);
    }
    /// Entities whose collider overlaps the circle, into `out`.
    void entities_in_circle(vec2 center, float radius, std::vector<PonkoEntity>& out) {
        std::size_t n = 0;
        out.resize(out.capacity());
        detail::check(ponko_ctx_physics_in_circle(raw_, center, radius, out.data(), out.size(), &n));
        if (n > out.size()) { out.resize(n); detail::check(ponko_ctx_physics_in_circle(raw_, center, radius, out.data(), out.size(), &n)); }
        out.resize(n);
    }
    /// Pushes a dynamic body once (mass × pixels/s); false if `e` isn't a body.
    bool apply_impulse(entity e, vec2 impulse) {
        auto status = ponko_ctx_apply_impulse(raw_, e, impulse);
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// A body's mass (density × area), if `e` is a body.
    std::optional<float> mass(entity e) {
        float m = 0;
        auto status = ponko_ctx_body_mass(raw_, e, &m);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return m;
    }
    /// Pairs that started touching since this system last asked (sensor
    /// overlaps too), oldest first; valid until the system returns.
    events_view<collision> collisions_started() { return collisions(true); }
    /// Pairs that stopped touching since this system last asked.
    events_view<collision> collisions_stopped() { return collisions(false); }
private:
    events_view<collision> collisions(bool started) {
        const collision* data = nullptr;
        std::size_t count = 0;
        detail::check(ponko_ctx_collisions(raw_, started, &data, &count));
        return events_view<collision>(data, count);
    }
public:

    // 3D physics (game::physics3d), as of the last physics step. Changing physics is not allowed inside
    // query callbacks; the casts, overlaps, closest_point3d and penetrations3d are (not in each_par).
    /// Changes 3D gravity, in m/s².
    void set_gravity3d(vec3 gravity) { detail::check(ponko_ctx_set_gravity3d(raw_, gravity)); }
    /// Holds the 3D physics step (`true`) or lets it run again; every other system keeps
    /// running. While held, bodies and characters stay put, keeping their velocity, and no
    /// collision events are sent. The 3D counterpart of `set_physics_paused`.
    void set_physics3d_paused(bool paused) { detail::check(ponko_ctx_set_physics3d_paused(raw_, paused)); }
    /// Whether `set_physics3d_paused` holds the 3D physics step.
    bool is_physics3d_paused() {
        bool paused = false;
        detail::check(ponko_ctx_is_physics3d_paused(raw_, &paused));
        return paused;
    }
    /// A heightfield from the same n x n heights and `size` as
    /// create_terrain_tile (NaN for holes): give mesh_collider(it) to a
    /// fixed body at the tile's transform3d (its corner).
    collision_mesh heightfield_collider(const std::vector<float>& heights, float size) {
        collision_mesh m{}; detail::check(ponko_ctx_physics3d_heightfield(raw_, heights.data(), heights.size(), size, &m)); return m;
    }
    /// A convex hull around `points`.
    collision_mesh convex_hull_collider(const std::vector<vec3>& points) {
        collision_mesh m{}; detail::check(ponko_ctx_physics3d_convex_hull(raw_, points.data(), points.size(), &m)); return m;
    }
    /// A triangle mesh: three indices into `vertices` per triangle.
    collision_mesh trimesh_collider(const std::vector<vec3>& vertices, const std::vector<std::uint32_t>& indices) {
        collision_mesh m{};
        detail::check(ponko_ctx_physics3d_trimesh(raw_, vertices.data(), vertices.size(), indices.data(), indices.size(), &m));
        return m;
    }
    /// Makes a glTF file (under assets/) solid in one call (Rust
    /// `Ctx::load_collision_mesh`): its triangles, scaled by `scale`, or a
    /// convex hull around them with `convex`, for mesh_collider(it) on a
    /// fixed body. What a scene's mesh collider makes; each file and options
    /// is read once. Throws ponko::error if it can't.
    collision_mesh load_collision_mesh(const char* path, vec3 scale = {1.0f, 1.0f, 1.0f}, bool convex = false) {
        return try_load_collision_mesh(path, scale, convex).value();
    }
    /// load_collision_mesh, saying why not instead of throwing (Rust `try_load_collision_mesh_with`).
    result<collision_mesh> try_load_collision_mesh(const char* path, vec3 scale = {1.0f, 1.0f, 1.0f}, bool convex = false) {
        collision_mesh m{}; const PonkoStatus status = ponko_ctx_load_collision_mesh(raw_, path, scale, convex, &m); return detail::attempt(status, m);
    }
    /// Lets go of a collision mesh; colliders using it keep it until removed.
    void free_collision_mesh(collision_mesh m) { detail::check(ponko_ctx_physics3d_free_mesh(raw_, m)); }
    /// A collision mesh from data built (on any thread) as collision_mesh_data.
    collision_mesh add_collision_mesh(const collision_mesh_data& data) {
        collision_mesh m{}; detail::check(ponko_ctx_physics3d_add_mesh(raw_, data.raw(), &m)); return m;
    }
    /// Collider streaming (Rust `Physics3D::stream_colliders`): with a
    /// number, the colliders of fixed bodies and terrain colliders are only
    /// in the physics world near entities with a collider_focus, at most
    /// that many more each step, nearest first; std::nullopt turns it off.
    void stream_colliders(std::optional<std::size_t> max_new) {
        detail::check(ponko_ctx_physics3d_stream_colliders(raw_, max_new.has_value(), max_new.value_or(0)));
    }
    /// Whether `e`'s collider is in the physics world now (false for a
    /// static collider that streaming left out).
    bool is_collider_active(entity e) const {
        bool active = false; detail::check(ponko_ctx_physics3d_is_collider_active(raw_, e, &active)); return active;
    }
    /// How many joints the 3D physics world holds after its last step.
    std::size_t joint_count() const { std::size_t n = 0; detail::check(ponko_ctx_physics3d_joint_count(raw_, &n)); return n; }
    /// Sets the contact hook (Rust `Physics3D::set_contact_hook`): each step,
    /// before the solver uses them, `hook` is called for the contacts of every
    /// body with `hook_modifier3d()` (or a modifier with `.hook = true`). It
    /// can ignore the pair or remove points, change friction and bounciness,
    /// or set a surface velocity; changes last for that step. It runs inside
    /// the physics step: it sees only the pair (capture the rest), must not
    /// use this ctx or its world, and the `contact_pair3d&` is valid only
    /// during the call. An exception it throws stops the game after the step
    /// and comes out of `run`/`run_headless`. Replaces an earlier hook.
    void set_contact_hook3d(std::function<void(contact_pair3d&)> hook) {
        if (!hook) { clear_contact_hook3d(); return; }
        owner_->contact_hook = std::move(hook);
        detail::check(ponko_ctx_set_contact_hook3d(raw_, contact_trampoline, owner_.get()));
    }
    /// Removes the contact hook.
    void clear_contact_hook3d() {
        detail::check(ponko_ctx_set_contact_hook3d(raw_, nullptr, nullptr));
        owner_->contact_hook = nullptr;
    }
    /// How terrain tiles are made solid again after a load (Rust
    /// `Physics3D::set_terrain_rebuild`): a saved world keeps each
    /// terrain_collider's surface but not its heightfield, so after
    /// load_world or load_scene `rebuild(world, tile)` is called for each
    /// tile whose collider has no mesh, and the heightfield it returns goes
    /// in the tile's collider before the next step. The tile's saved
    /// components (`world.get<transform3d>(tile)`, or one of your own) say
    /// which heights it had; std::nullopt leaves it without ground. Set it
    /// once, in setup; it stays through loads and replaces an earlier one.
    /// It runs inside the load: it must not use this ctx. An exception it
    /// throws leaves that tile without ground, stops the game and comes out
    /// of `run`/`run_headless`.
    void set_terrain_rebuild(std::function<std::optional<collision_mesh_data>(const loaded_world&, entity)> rebuild) {
        if (!rebuild) { clear_terrain_rebuild(); return; }
        detail::identity* owner = owner_.get();
        owner_->terrain_rebuild = [owner, rebuild = std::move(rebuild)](const PonkoLoadedWorld* raw, std::uint64_t tile) {
            std::optional<collision_mesh_data> made = rebuild(loaded_world(raw, owner), tile);
            return made ? made->raw_.release() : nullptr;
        };
        detail::check(ponko_ctx_physics3d_set_terrain_rebuild(raw_, terrain_trampoline, owner));
    }
    /// Stops making terrain tiles solid again after a load.
    void clear_terrain_rebuild() {
        detail::check(ponko_ctx_physics3d_clear_terrain_rebuild(raw_));
        owner_->terrain_rebuild = nullptr;
    }
    /// The closest collider a ray hits within `max_distance`.
    std::optional<hit3d> cast_ray3d(vec3 origin, vec3 direction, float max_distance, filter3d filter = {}) {
        bool found = false; hit3d hit{};
        detail::check(ponko_ctx_cast_ray3d(raw_, origin, direction, max_distance, filter.raw, &found, &hit));
        if (!found) return std::nullopt;
        return hit;
    }
    /// Every collider a ray hits within `max_distance`, nearest first, into `out`.
    void cast_ray3d_all(vec3 origin, vec3 direction, float max_distance, std::vector<hit3d>& out, filter3d filter = {}) {
        std::size_t n = 0;
        out.resize(out.capacity());
        detail::check(ponko_ctx_cast_ray3d_all(raw_, origin, direction, max_distance, filter.raw, out.data(), out.size(), &n));
        if (n > out.size()) {
            out.resize(n);
            detail::check(ponko_ctx_cast_ray3d_all(raw_, origin, direction, max_distance, filter.raw, out.data(), out.size(), &n));
        }
        out.resize(n);
    }
    /// Sweeps a ball: the first collider it touches within `max_distance`.
    std::optional<hit3d> cast_sphere3d(vec3 origin, float radius, vec3 direction, float max_distance, filter3d filter = {}) {
        bool found = false; hit3d hit{};
        detail::check(ponko_ctx_cast_sphere3d(raw_, origin, radius, direction, max_distance, filter.raw, &found, &hit));
        if (!found) return std::nullopt;
        return hit;
    }
    /// Sweeps an upright capsule centered on `origin`.
    std::optional<hit3d> cast_capsule3d(vec3 origin, float height, float radius, vec3 direction, float max_distance,
                                        filter3d filter = {}) {
        bool found = false; hit3d hit{};
        detail::check(ponko_ctx_cast_capsule3d(raw_, origin, height, radius, direction, max_distance, filter.raw, &found, &hit));
        if (!found) return std::nullopt;
        return hit;
    }
    /// Entities whose collider overlaps the ball, into `out`.
    void overlap_sphere3d(vec3 center, float radius, std::vector<PonkoEntity>& out, filter3d filter = {}) {
        overlap3d(out, [&](PonkoEntity* data, std::size_t cap, std::size_t* n) {
            return ponko_ctx_overlap_sphere3d(raw_, center, radius, filter.raw, data, cap, n);
        });
    }
    /// Entities whose collider overlaps the box of `size` turned by `rotation`.
    void overlap_box3d(vec3 center, vec3 size, quat rotation, std::vector<PonkoEntity>& out, filter3d filter = {}) {
        overlap3d(out, [&](PonkoEntity* data, std::size_t cap, std::size_t* n) {
            return ponko_ctx_overlap_box3d(raw_, center, size, rotation, filter.raw, data, cap, n);
        });
    }
    /// Entities whose collider overlaps the upright capsule.
    void overlap_capsule3d(vec3 center, float height, float radius, std::vector<PonkoEntity>& out, filter3d filter = {}) {
        overlap3d(out, [&](PonkoEntity* data, std::size_t cap, std::size_t* n) {
            return ponko_ctx_overlap_capsule3d(raw_, center, height, radius, filter.raw, data, cap, n);
        });
    }
    /// Sweeps a box of `size` turned by `rotation`: the first collider it touches within `max_distance`
    /// (Rust `d3::Physics::cast_box`).
    std::optional<hit3d> cast_box3d(vec3 origin, vec3 size, quat rotation, vec3 direction, float max_distance,
                                    filter3d filter = {}) {
        bool found = false; hit3d hit{};
        detail::check(ponko_ctx_cast_box3d(raw_, origin, size, rotation, direction, max_distance, filter.raw, &found,
                                           &hit));
        if (!found) return std::nullopt;
        return hit;
    }
    /// Entities whose 3D collider contains `point`, into `out`.
    void overlap_point3d(vec3 point, std::vector<PonkoEntity>& out, filter3d filter = {}) {
        overlap3d(out, [&](PonkoEntity* data, std::size_t cap, std::size_t* n) {
            return ponko_ctx_overlap_point3d(raw_, point, filter.raw, data, cap, n);
        });
    }
    /// The nearest 3D collider to `point` within `max_distance` and its nearest point; a point inside a
    /// collider is its own nearest point (Rust `d3::Physics::closest_point`).
    std::optional<point_hit3d> closest_point3d(vec3 point, float max_distance, filter3d filter = {}) {
        bool found = false; point_hit3d hit{};
        detail::check(ponko_ctx_closest_point3d(raw_, point, max_distance, filter.raw, &found, &hit));
        if (!found) return std::nullopt;
        return hit;
    }
    /// Every 3D collider that `shape` (a sphere, box, capsule or mesh collider), placed at `position` and
    /// turned by `rotation`, overlaps: how deep and which way out, deepest first, into `out` (Rust
    /// `d3::Physics::penetrations`).
    void penetrations3d(const collider3d& shape, vec3 position, quat rotation, std::vector<penetration3d>& out,
                        filter3d filter = {}) {
        fill_list(out, [&](penetration3d* data, std::size_t cap, std::size_t* n) {
            return ponko_ctx_penetrations3d(raw_, shape, position, rotation, filter.raw, data, cap, n);
        });
    }
    /// Pushes a dynamic 3D body once (kg × m/s); false if `e` isn't one.
    bool apply_impulse3d(entity e, vec3 impulse) {
        auto status = ponko_ctx_apply_impulse3d(raw_, e, impulse);
        if (status == PONKO_NOT_FOUND) return false;
        detail::check(status);
        return true;
    }
    /// A 3D body's mass in kg, if `e` is one.
    std::optional<float> mass3d(entity e) {
        float m = 0;
        auto status = ponko_ctx_body_mass3d(raw_, e, &m);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return m;
    }
    /// A 3D body's center of mass in the world, if `e` is one.
    std::optional<vec3> center_of_mass3d(entity e) {
        vec3 c{};
        auto status = ponko_ctx_body_center_of_mass3d(raw_, e, &c);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return c;
    }
    /// Hands `e`'s skeleton over to physics (Rust `Ctx::start_ragdoll`): a
    /// body per bone, jointed to the nearest chosen ancestor. The bodies are
    /// linked at once, so you can push them straight away. Throws on an
    /// unknown or repeated joint, a radius that isn't positive, or no skeleton.
    void start_ragdoll(entity e, const std::vector<ragdoll_bone>& bones) {
        std::vector<PonkoRagdollBone> raw;
        raw.reserve(bones.size());
        for (const ragdoll_bone& b : bones) raw.push_back(b.raw());
        detail::check(ponko_ctx_start_ragdoll(raw_, e, raw.data(), raw.size()));
    }
    /// Removes `e`'s ragdoll bodies and blends its skeleton back to the
    /// animated pose over `seconds` (0 snaps). Call it before despawning `e`.
    void stop_ragdoll(entity e, float seconds = 0.0f) { detail::check(ponko_ctx_stop_ragdoll(raw_, e, seconds)); }
    /// The body standing in for joint `joint` (see find_joint) of a ragdolled `e`.
    std::optional<entity> ragdoll_body(entity e, std::size_t joint) const {
        entity body = 0;
        auto status = ponko_ctx_ragdoll_body(raw_, e, joint, &body);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return body;
    }
    /// Whether `e` is ragdolled (or still blending back from it).
    bool ragdolled(entity e) const { bool r = false, b = false; detail::check(ponko_ctx_ragdoll_state(raw_, e, &r, &b)); return r; }
    /// Whether `e` is blending back to its animation after stop_ragdoll.
    bool ragdoll_blending(entity e) const { bool r = false, b = false; detail::check(ponko_ctx_ragdoll_state(raw_, e, &r, &b)); return b; }
private:
    /// Calls `call` into `out`'s capacity, then again with room for all if it was too small.
    template<class T, class F> static void fill_list(std::vector<T>& out, F call) {
        std::size_t n = 0;
        out.resize(out.capacity());
        detail::check(call(out.data(), out.size(), &n));
        if (n > out.size()) { out.resize(n); detail::check(call(out.data(), out.size(), &n)); }
        out.resize(n);
    }
    template<class F> static void overlap3d(std::vector<PonkoEntity>& out, F call) {
        std::size_t n = 0;
        out.resize(out.capacity());
        detail::check(call(out.data(), out.size(), &n));
        if (n > out.size()) { out.resize(n); detail::check(call(out.data(), out.size(), &n)); }
        out.resize(n);
    }
public:

    // The built-in spatial grid (game::spatial_grid); safe from query_par callbacks.
    /// Entities of the built-in grid (game::spatial_grid) within `radius` of `center`, into `out` (reusing
    /// its capacity). Safe inside query callbacks.
    void spatial_radius(vec2 center, float radius, std::vector<grid_hit>& out) const {
        std::size_t n = 0;
        out.resize(out.capacity());
        detail::check(ponko_ctx_spatial_radius(raw_, center, radius, out.data(), out.size(), &n));
        if (n > out.size()) { out.resize(n); detail::check(ponko_ctx_spatial_radius(raw_, center, radius, out.data(), out.size(), &n)); }
        out.resize(n);
    }
    /// Entities of the built-in grid in the rectangle from `min` to `max`, into `out`.
    void spatial_rect(vec2 min, vec2 max, std::vector<grid_hit>& out) const {
        std::size_t n = 0;
        out.resize(out.capacity());
        detail::check(ponko_ctx_spatial_rect(raw_, min, max, out.data(), out.size(), &n));
        if (n > out.size()) { out.resize(n); detail::check(ponko_ctx_spatial_rect(raw_, min, max, out.data(), out.size(), &n)); }
        out.resize(n);
    }
    /// An inline-query view of the built-in grid, valid during this call.
    grid_view<grid_hit> spatial_view() const { PonkoGridView v{}; detail::check(ponko_ctx_spatial_view(raw_, &v)); return grid_view<grid_hit>(v); }
    /// The nearest entity of the built-in grid within `max_radius` other than `exclude`, or nothing.
    std::optional<grid_hit> spatial_nearest(vec2 center, float max_radius, entity exclude = UINT64_MAX) const {
        grid_hit hit{};
        auto status = ponko_ctx_spatial_nearest(raw_, center, max_radius, exclude, &hit);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status);
        return hit;
    }

    // The sky kit and the day-night driver (gl#122).
    /// The sky kit's settings: `enabled` false (and the defaults) while the plain atmosphere is drawn.
    ponko::sky_kit sky_kit() const {
        ponko::sky_kit k{};
        k.struct_size = sizeof k;
        k.version = PONKO_SKY_KIT_VERSION;
        detail::check(ponko_ctx_sky_kit(raw_, &k));
        return k;
    }
    /// Turns the sky kit on with `k`'s settings, or off when `k.enabled` is false. Throws, changing
    /// nothing, on a non-finite number or an unknown quality.
    void set_sky_kit(const ponko::sky_kit& k) { detail::check(ponko_ctx_set_sky_kit(raw_, &k)); }
    /// Sets the sun, the moon's phase (turning the sky kit on if it was off) and the night light to `d`'s
    /// time.
    void apply_day_night(const ponko::day_night& d) { detail::check(ponko_ctx_apply_day_night(raw_, &d)); }
    /// Advances `d` by this frame's time, then applies it: the whole cycle, once a frame.
    void update_day_night(ponko::day_night& d) { detail::check(ponko_ctx_update_day_night(raw_, &d)); }

    /// A view of the 3D navmesh for reads inside loops (see ponko::nav_view). Take it before the loop.
    /// Throws if no navmesh was baked.
    ponko::nav_view nav_view() const {
        PonkoNavView v{};
        detail::check(ponko_ctx_nav_view(raw_, &v));
        return ponko::nav_view(v);
    }
    // Joint-track clips and the versioned live counts (gl#122).
    /// A clip called `name` from joint tracks, for the skeleton of `skeleton` (Rust `AnimationClip::try_new`):
    /// tracks of one joint merge; a joint outside the skeleton or a channel given twice throws.
    ponko::clip create_clip(entity skeleton, const char* name, const std::vector<joint_track>& tracks) {
        PonkoClip* c = nullptr;
        detail::check(ponko_ctx_create_clip(raw_, skeleton, name, tracks.data(), tracks.size(), &c));
        return ponko::clip(c);
    }
    /// How many handles of each kind are live now, crowds and the other newer kinds included.
    ponko::live_counts_ex live_counts_ex() const {
        ponko::live_counts_ex c{};
        c.struct_size = sizeof c;
        c.version = PONKO_LIVE_COUNTS_EX_VERSION;
        detail::check(ponko_ctx_live_counts_ex(raw_, &c));
        return c;
    }
};

// Camera rigs (Rust ponko::camera, d2::FollowCamera, d2::PixelPerfect, d3::OrbitCamera,
// d3::FirstPersonCamera, d3::ThirdPersonCamera). Each is its C descriptor with the header and the
// defaults filled in: keep one, change its settings, and update it once a frame. Its state travels in
// it, so it can live anywhere (a member, a component) and copies freely.

/// Moves `current` toward `target` like a critically damped spring: eases in, settles without
/// overshoot. Keep `velocity` between calls (start at 0). Exact, so the same at every frame rate.
inline float smooth_damp(float current, float target, float& velocity, float smooth_time, float dt) {
    return ponko_smooth_damp(current, target, &velocity, smooth_time, dt);
}
/// smooth_damp for 2D points.
inline vec2 smooth_damp(vec2 current, vec2 target, vec2& velocity, float smooth_time, float dt) {
    return ponko_smooth_damp_vec2(current, target, &velocity, smooth_time, dt);
}
/// smooth_damp for 3D points.
inline vec3 smooth_damp(vec3 current, vec3 target, vec3& velocity, float smooth_time, float dt) {
    return ponko_smooth_damp_vec3(current, target, &velocity, smooth_time, dt);
}
/// Moves `current` toward `target`, half the remaining way every `half_life` seconds.
inline float damp(float current, float target, float half_life, float dt) { return ponko_damp(current, target, half_life, dt); }
/// damp for 2D points.
inline vec2 damp(vec2 current, vec2 target, float half_life, float dt) { return ponko_damp_vec2(current, target, half_life, dt); }
/// damp for 3D points.
inline vec3 damp(vec3 current, vec3 target, float half_life, float dt) { return ponko_damp_vec3(current, target, half_life, dt); }
/// damp for angles in radians, turning the short way round.
inline float damp_angle(float current, float target, float half_life, float dt) {
    return ponko_damp_angle(current, target, half_life, dt);
}

/// Trauma-based camera shake (Rust ponko::camera::CameraShake): add_trauma when something hits,
/// update once a frame, then apply2d or apply3d after the camera is placed for the frame.
struct camera_shake : PonkoCameraShake {
    /// No trauma, the default limits.
    camera_shake() : PonkoCameraShake{} {
        struct_size = sizeof(PonkoCameraShake);
        version = PONKO_CAMERA_SHAKE_VERSION;
        detail::check(ponko_camera_shake_defaults(this));
    }
    /// Adds trauma, kept within 0 to 1.
    void add_trauma(float amount) { detail::check(ponko_camera_shake_add_trauma(this, amount)); }
    /// Advances the shake and wears trauma off.
    void update(float dt) { detail::check(ponko_camera_shake_update(this, dt)); }
    /// Moves the 2D camera by the shake.
    void apply2d(ctx& c) const { detail::check(ponko_camera_shake_apply2d(this, c.raw())); }
    /// Turns the 3D camera by the shake.
    void apply3d(ctx& c) const { detail::check(ponko_camera_shake_apply3d(this, c.raw())); }
};

/// A sea's waves from a wind-driven spectrum (Rust water::WaveSpectrum): thousands of waves worked
/// out each frame with an FFT. Change tile, resolution, choppiness or seed, then water_desc::with_spectrum.
struct wave_spectrum : PonkoWaveSpectrum {
    /// A sea under `wind` (m/s along x and z) with `height` metres of significant wave height, on a
    /// 256 m square of 128 cells.
    wave_spectrum(vec2 wind, float height) : PonkoWaveSpectrum(ponko_wave_spectrum(wind, height)) {}
};

/// Everything about one body of water (Rust water::WaterDesc): start from lake, river or sea,
/// change any field, then ctx::spawn_water.
struct water_desc : PonkoWaterDesc {
    /// A still lake over `size` metres around `center` (x, z) at height `level`.
    static water_desc lake(vec2 center, vec2 size, float level) {
        water_desc d;
        detail::check(ponko_water_desc_lake(&d, center, size, level));
        return d;
    }
    /// A river flowing at `flow` m/s (x, z); set flow_map for a current that bends.
    static water_desc river(vec2 center, vec2 size, float level, vec2 flow) {
        water_desc d;
        detail::check(ponko_water_desc_river(&d, center, size, level, flow));
        return d;
    }
    /// An open sea to the horizon: Gerstner waves around `wind`, about `height` metres trough to crest.
    static water_desc sea(float level, vec2 wind, float height) {
        water_desc d;
        detail::check(ponko_water_desc_sea(&d, level, wind, height));
        return d;
    }
    /// The waves from `spectrum` instead of the Gerstner waves (Rust `Waves::spectrum`).
    water_desc& with_spectrum(const wave_spectrum& spectrum) {
        has_spectrum = true;
        this->spectrum = spectrum;
        return *this;
    }

private:
    water_desc() : PonkoWaterDesc{} {
        struct_size = sizeof(PonkoWaterDesc);
        version = PONKO_WATER_DESC_VERSION;
    }
};

/// How a 3D body floats on water (Rust water::Buoyancy): start from box, sphere or from_points (fresh
/// water, drag PONKO_BUOYANCY_DRAG, ripples on), change any field (water_density, drag, ripples, the
/// points), then ctx::set_buoyancy.
struct buoyancy_desc : PonkoBuoyancyDesc {
    /// A box of `size` (full width, height and depth) centred on the body: eight points.
    static buoyancy_desc box(vec3 size) {
        buoyancy_desc d;
        detail::check(ponko_buoyancy_desc_box(&d, size));
        return d;
    }
    /// A ball of `radius`: seven points.
    static buoyancy_desc sphere(float radius) {
        buoyancy_desc d;
        detail::check(ponko_buoyancy_desc_sphere(&d, radius));
        return d;
    }
    /// Sample `points` (body space, 1 to PONKO_BUOYANCY_MAX_POINTS) sharing `volume` m³, each standing for
    /// a column `height` metres tall.
    static buoyancy_desc from_points(const std::vector<vec3>& points, float volume, float height) {
        buoyancy_desc d;
        detail::check(ponko_buoyancy_desc_points(&d, points.data(), static_cast<std::uint32_t>(points.size()),
                                                 volume, height));
        return d;
    }

private:
    buoyancy_desc() : PonkoBuoyancyDesc{} {
        struct_size = sizeof(PonkoBuoyancyDesc);
        version = PONKO_BUOYANCY_DESC_VERSION;
    }
};

/// A 2D camera following a target (Rust d2::FollowCamera): dead zone, look-ahead, offset, bounds.
struct follow_camera2d : PonkoFollowCamera2D {
    /// The defaults: no dead zone, look-ahead or bounds.
    follow_camera2d() : PonkoFollowCamera2D{} {
        struct_size = sizeof(PonkoFollowCamera2D);
        version = PONKO_FOLLOW_CAMERA2D_VERSION;
        detail::check(ponko_follow_camera2d_defaults(this));
    }
    /// Never show outside `r`.
    void set_bounds(rect r) { has_bounds = true; bounds = r; }
    /// Moves the camera toward `target` (once a frame); the first call puts it there.
    void update(ctx& c, vec2 target, float dt) { detail::check(ponko_follow_camera2d_update(this, c.raw(), target, dt)); }
    /// Puts the camera on `target` at once.
    void snap(ctx& c, vec2 target) { detail::check(ponko_follow_camera2d_snap(this, c.raw(), target)); }
    /// Keeps a copy of this rig in the world for the engine to update every frame toward the entity
    /// marked with ctx::set_camera_target (Rust: the rig as a resource), replacing the one kept before.
    void keep(ctx& c) const { detail::check(ponko_ctx_set_follow_camera2d_rig(c.raw(), this)); }
    /// The rig the engine keeps, as it last updated it, if one is kept.
    static std::optional<follow_camera2d> kept(ctx& c) {
        follow_camera2d rig; bool is_kept = false;
        detail::check(ponko_ctx_follow_camera2d_rig(c.raw(), &rig, &is_kept));
        if (!is_kept) return std::nullopt;
        return rig;
    }
    /// Stops keeping the rig: the engine leaves the camera alone.
    static void remove_kept(ctx& c) { detail::check(ponko_ctx_set_follow_camera2d_rig(c.raw(), nullptr)); }
};

/// Pixel-perfect 2D (Rust d2::PixelPerfect): a whole-number zoom and a camera on the art pixel grid.
struct pixel_perfect : PonkoPixelPerfect {
    /// Pixel art drawn for `width` x `height` art pixels.
    pixel_perfect(float width = 320.0f, float height = 180.0f) : PonkoPixelPerfect{} {
        struct_size = sizeof(PonkoPixelPerfect);
        version = PONKO_PIXEL_PERFECT_VERSION;
        detail::check(ponko_pixel_perfect_defaults(this));
        resolution = {width, height};
    }
    /// Sets the zoom for the window and puts the camera on the grid (after it is placed).
    void apply(ctx& c) const { detail::check(ponko_pixel_perfect_apply(this, c.raw())); }
    /// Where exactly the resolution shows in the window, in window pixels.
    rect screen_rect(ctx& c) const { rect r{}; detail::check(ponko_pixel_perfect_screen_rect(this, c.raw(), &r)); return r; }
};

/// Where a video is (ponko_video_state): its texture, size, length, time, volume, sound and bus.
using video_state = PonkoVideoState;
/// A video played into a texture, its sound through the mixer (Rust `video::Video`, C library
/// feature `video`): a WebM file with AV1 pictures and Vorbis sound. It starts paused on its first
/// frame: play(), then update() once a frame and draw state().texture. A handle: free it with
/// free(ctx), which also frees its texture and sound.
class video {
    PonkoVideo* raw_ = nullptr;
    explicit video(PonkoVideo* raw) : raw_(raw) {}
public:
    /// No video.
    video() = default;
    /// Loads one from an asset source or `assets/` (ponko_ctx_load_video). Throws ponko::error:
    /// not found, another codec or a build without the feature, or a damaged file.
    static video load(ctx& c, const char* path) {
        PonkoVideo* v = nullptr; detail::check(ponko_ctx_load_video(c.raw(), path, &v)); return video(v);
    }
    /// Makes one from a file's bytes, copied (ponko_ctx_video_from_bytes).
    static video from_bytes(ctx& c, const void* data, std::size_t size) {
        PonkoVideo* v = nullptr; detail::check(ponko_ctx_video_from_bytes(c.raw(), data, size, &v)); return video(v);
    }
    /// Whether this holds a video.
    explicit operator bool() const noexcept { return raw_ != nullptr; }
    /// The C handle.
    PonkoVideo* raw() const noexcept { return raw_; }
    /// Where it is: texture, size, length, time, volume, playing, finished, looping, sound, bus.
    video_state state() const {
        video_state s{}; s.struct_size = sizeof(s); s.version = PONKO_VIDEO_STATE_VERSION;
        detail::check(ponko_video_state(raw_, &s)); return s;
    }
    /// The frame shown as RGBA pixels, valid until the video changes (ponko_video_frame_rgba).
    std::pair<const std::uint8_t*, std::size_t> frame_rgba() const {
        const std::uint8_t* p = nullptr; std::size_t n = 0;
        detail::check(ponko_video_frame_rgba(raw_, &p, &n)); return {p, n};
    }
    /// Starts over at its end, or stops there (off by default).
    void set_looping(bool looping) { detail::check(ponko_video_set_looping(raw_, looping)); }
    /// Plays from where it is; from the start if it had finished.
    void play(ctx& c) { detail::check(ponko_ctx_video_play(c.raw(), raw_)); }
    /// Pauses: the frame stays, the sound stops.
    void pause(ctx& c) { detail::check(ponko_ctx_video_pause(c.raw(), raw_)); }
    /// Jumps to `seconds`, playing or paused.
    void seek(ctx& c, float seconds) { detail::check(ponko_ctx_video_seek(c.raw(), raw_, seconds)); }
    /// Moves it on by this frame's time and shows that frame: once a frame, from a frame system.
    void update(ctx& c) { detail::check(ponko_ctx_video_update(c.raw(), raw_)); }
    /// update() by `seconds` instead of the frame's time.
    void advance(ctx& c, float seconds) { detail::check(ponko_ctx_video_advance(c.raw(), raw_, seconds)); }
    /// Its sound's volume, 0 silent to 1 as recorded.
    void set_volume(ctx& c, float volume) { detail::check(ponko_ctx_video_set_volume(c.raw(), raw_, volume)); }
    /// Plays its sound on `bus` from now on.
    void set_bus(ctx& c, audio_bus bus) { detail::check(ponko_ctx_video_set_bus(c.raw(), raw_, bus)); }
    /// Frees it, its texture and its sound; this handle holds no video afterwards.
    void free(ctx& c) { detail::check(ponko_ctx_free_video(c.raw(), raw_)); raw_ = nullptr; }
};

/// How a light bake bakes (Rust `LightBakeSettings`), starting from the defaults.
struct light_bake_settings : PonkoLightBakeSettings {
    /// The defaults: 4 texels a unit, 128 samples, 3 bounces, pages up to 1024, directional,
    /// denoised, probes every 2 units, seed 0.
    light_bake_settings() : PonkoLightBakeSettings{} {
        struct_size = sizeof(PonkoLightBakeSettings);
        version = PONKO_LIGHT_BAKE_SETTINGS_VERSION;
        detail::check(ponko_light_bake_settings_defaults(this));
    }
};
/// One model to bake (Rust `BakeModel`): `mesh` (with lightmap coordinates if it is lightmapped)
/// where `at` puts it, white, not glowing, one-sided, lightmapped.
struct bake_model : PonkoBakeModel {
    /// `mesh` must outlive the light_bake_scene::add call.
    bake_model(const mesh_data& m, const transform3d& at) : PonkoBakeModel{} {
        struct_size = sizeof(PonkoBakeModel);
        version = PONKO_BAKE_MODEL_VERSION;
        detail::check(ponko_bake_model_defaults(this));
        mesh = m.raw();
        transform = at;
    }
};
/// What a bake made and how it went (pages, GPU bytes, texels, probes, rays, seconds).
using baked_lighting_info = PonkoBakedLightingInfo;
/// Where a model's lightmap is on a page.
using lightmap_rect = PonkoLightmapRect;
/// The result of a light bake (Rust `BakedLighting`): lightmap pages and the probe volume. Owns
/// it; draw with it through ctx::set_baked_lighting(raw()), keep it with to_bytes().
class baked_lighting {
    PonkoBakedLighting* raw_ = nullptr;
public:
    /// Takes a C handle (from ponko_bake_lighting or _from_bytes).
    explicit baked_lighting(PonkoBakedLighting* raw) : raw_(raw) {}
    /// Not copyable: it owns its C handle.
    baked_lighting(const baked_lighting&) = delete;
    /// Not copyable: it owns its C handle.
    baked_lighting& operator=(const baked_lighting&) = delete;
    /// Moves the result.
    baked_lighting(baked_lighting&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    /// Moves the result.
    baked_lighting& operator=(baked_lighting&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    /// Frees it.
    ~baked_lighting() { ponko_baked_lighting_free(raw_); }
    /// Loads one saved with to_bytes() from an asset source or `assets/`
    /// (ponko_ctx_load_baked_lighting). Throws ponko::error: not found, or not baked lighting.
    static baked_lighting load(ctx& c, const char* path) {
        PonkoBakedLighting* b = nullptr;
        detail::check(ponko_ctx_load_baked_lighting(c.raw(), path, &b));
        return baked_lighting(b);
    }
    /// Reads one back from to_bytes()'s bytes. Throws ponko::error for bytes that aren't one.
    static baked_lighting from_bytes(const std::vector<std::uint8_t>& bytes) {
        PonkoBakedLighting* b = nullptr;
        detail::check(ponko_baked_lighting_from_bytes(bytes.data(), bytes.size(), &b));
        return baked_lighting(b);
    }
    /// The bytes for a file.
    std::vector<std::uint8_t> to_bytes() const {
        std::size_t n = 0; detail::check(ponko_baked_lighting_to_bytes(raw_, nullptr, 0, &n));
        std::vector<std::uint8_t> bytes(n);
        detail::check(ponko_baked_lighting_to_bytes(raw_, bytes.data(), bytes.size(), &n));
        return bytes;
    }
    /// What it holds and how its bake went.
    baked_lighting_info info() const {
        baked_lighting_info i{}; i.struct_size = sizeof(i); i.version = PONKO_BAKED_LIGHTING_INFO_VERSION;
        detail::check(ponko_baked_lighting_info(raw_, &i)); return i;
    }
    /// The component that draws model `index` of the bake with its lightmap, if it has one.
    std::optional<lightmapped> lightmap(std::size_t index) const {
        lightmapped l{}; const PonkoStatus status = ponko_baked_lighting_lightmap(raw_, index, nullptr, &l);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return l;
    }
    /// Where model `index`'s lightmap is, if it has one.
    std::optional<lightmap_rect> rect(std::size_t index) const {
        lightmap_rect r{}; const PonkoStatus status = ponko_baked_lighting_lightmap(raw_, index, &r, nullptr);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return r;
    }
    /// The baked light at a texel of a page (irradiance over pi) and whether it was baked.
    std::optional<std::pair<vec3, bool>> texel(std::uint32_t page, std::uint32_t x, std::uint32_t y) const {
        vec3 l{}; bool baked = false;
        const PonkoStatus status = ponko_baked_lighting_texel(raw_, page, x, y, &l, &baked);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return std::make_pair(l, baked);
    }
    /// The probe volume's light for a surface at `position` facing `normal`, inside the volume.
    std::optional<vec3> probe_light(vec3 position, vec3 normal) const {
        vec3 l{}; const PonkoStatus status = ponko_baked_lighting_probe_light(raw_, position, normal, &l);
        if (status == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(status); return l;
    }
    /// The C handle (for ctx::set_baked_lighting).
    const PonkoBakedLighting* raw() const noexcept { return raw_; }
};
/// The models and lamps to bake (Rust `LightBakeScene`), lit by the game's sun and sky as they
/// are when it is made. Holds copies, not the ctx: bake() may run on a thread of your own.
class light_bake_scene {
    PonkoLightBakeScene* raw_ = nullptr;
public:
    /// An empty scene lit as the game is now.
    explicit light_bake_scene(ctx& c) { detail::check(ponko_ctx_light_bake_scene_new(c.raw(), &raw_)); }
    /// Not copyable: it owns its C handle.
    light_bake_scene(const light_bake_scene&) = delete;
    /// Not copyable: it owns its C handle.
    light_bake_scene& operator=(const light_bake_scene&) = delete;
    /// Frees it.
    ~light_bake_scene() { ponko_light_bake_scene_free(raw_); }
    /// Adds a model; returns its index, which baked_lighting::lightmap takes.
    std::size_t add(const bake_model& m) {
        std::size_t i = 0; detail::check(ponko_light_bake_scene_add(raw_, &m, &i)); return i;
    }
    /// Adds a point light at `position`.
    void add_point_light(vec3 position, const point_light3d& light) {
        detail::check(ponko_light_bake_scene_add_point_light(raw_, position, &light));
    }
    /// Adds a spot light at `position` shining along `forward`.
    void add_spot_light(vec3 position, vec3 forward, const spot_light3d& light) {
        detail::check(ponko_light_bake_scene_add_spot_light(raw_, position, forward, &light));
    }
    /// Adds every point and spot light of the game's world where it is now.
    void add_lights(ctx& c) { detail::check(ponko_ctx_light_bake_scene_add_lights(c.raw(), raw_)); }
    /// Bakes (on every core; seconds to minutes). Throws ponko::error for settings out of range.
    baked_lighting bake(const light_bake_settings& settings = light_bake_settings()) const {
        PonkoBakedLighting* b = nullptr; detail::check(ponko_bake_lighting(raw_, &settings, &b)); return baked_lighting(b);
    }
};

/// A 3D camera circling a point (Rust d3::OrbitCamera): set target, yaw, pitch and distance (or use
/// mouse_controls), then update once a frame.
struct orbit_camera : PonkoOrbitCamera {
    /// Circling the origin at 10 units, looking down a little.
    orbit_camera() : PonkoOrbitCamera{} {
        struct_size = sizeof(PonkoOrbitCamera);
        version = PONKO_ORBIT_CAMERA_VERSION;
        detail::check(ponko_orbit_camera_defaults(this));
    }
    /// Left drag turns, right or middle drag pans, the wheel zooms.
    void mouse_controls(ctx& c) { detail::check(ponko_orbit_camera_mouse_controls(this, c.raw())); }
    /// Eases toward the goal and places the 3D camera.
    void update(ctx& c, float dt) { detail::check(ponko_orbit_camera_update(this, c.raw(), dt)); }
    /// Keeps a copy of this rig in the world for the engine to update every frame toward the entity
    /// marked with ctx::set_camera_target (Rust: the rig as a resource), replacing the one kept before.
    void keep(ctx& c) const { detail::check(ponko_ctx_set_orbit_camera_rig(c.raw(), this)); }
    /// The rig the engine keeps, as it last updated it, if one is kept.
    static std::optional<orbit_camera> kept(ctx& c) {
        orbit_camera rig; bool is_kept = false;
        detail::check(ponko_ctx_orbit_camera_rig(c.raw(), &rig, &is_kept));
        if (!is_kept) return std::nullopt;
        return rig;
    }
    /// Stops keeping the rig: the engine leaves the camera alone.
    static void remove_kept(ctx& c) { detail::check(ponko_ctx_set_orbit_camera_rig(c.raw(), nullptr)); }
};

/// A first-person camera (Rust d3::FirstPersonCamera): look with mouse motion, update at the eye.
struct first_person_camera : PonkoFirstPersonCamera {
    /// Looking along -Z.
    first_person_camera() : PonkoFirstPersonCamera{} {
        struct_size = sizeof(PonkoFirstPersonCamera);
        version = PONKO_FIRST_PERSON_CAMERA_VERSION;
        detail::check(ponko_first_person_camera_defaults(this));
    }
    /// Turns by mouse motion (ctx::mouse_motion) times the sensitivity.
    void look(vec2 delta) { detail::check(ponko_first_person_camera_look(this, delta)); }
    /// Places the 3D camera at `eye`, turned to the view.
    void update(ctx& c, vec3 eye, float dt) { detail::check(ponko_first_person_camera_update(this, c.raw(), eye, dt)); }
    /// Level forward, for walking.
    vec3 flat_forward() const { vec3 v{}; detail::check(ponko_first_person_camera_flat_forward(this, &v)); return v; }
    /// Level right, for strafing.
    vec3 flat_right() const { vec3 v{}; detail::check(ponko_first_person_camera_flat_right(this, &v)); return v; }
    /// Keeps a copy of this rig in the world for the engine to update every frame toward the entity
    /// marked with ctx::set_camera_target (Rust: the rig as a resource), replacing the one kept before.
    void keep(ctx& c) const { detail::check(ponko_ctx_set_first_person_camera_rig(c.raw(), this)); }
    /// The rig the engine keeps, as it last updated it, if one is kept.
    static std::optional<first_person_camera> kept(ctx& c) {
        first_person_camera rig; bool is_kept = false;
        detail::check(ponko_ctx_first_person_camera_rig(c.raw(), &rig, &is_kept));
        if (!is_kept) return std::nullopt;
        return rig;
    }
    /// Stops keeping the rig: the engine leaves the camera alone.
    static void remove_kept(ctx& c) { detail::check(ponko_ctx_set_first_person_camera_rig(c.raw(), nullptr)); }
};

/// A third-person camera behind a character (Rust d3::ThirdPersonCamera): follows it, and with
/// update_physics comes in front of walls.
struct third_person_camera : PonkoThirdPersonCamera {
    /// 5 units behind a pivot 1.5 above the target.
    third_person_camera() : PonkoThirdPersonCamera{} {
        struct_size = sizeof(PonkoThirdPersonCamera);
        version = PONKO_THIRD_PERSON_CAMERA_VERSION;
        detail::check(ponko_third_person_camera_defaults(this));
    }
    /// Turns by mouse motion times the sensitivity.
    void look(vec2 delta) { detail::check(ponko_third_person_camera_look(this, delta)); }
    /// Follows `target` and places the 3D camera, without checking for walls.
    void update(ctx& c, vec3 target, float dt) { detail::check(ponko_third_person_camera_update(this, c.raw(), target, dt)); }
    /// update, kept out of the 3D physics colliders (exclude the character in `filter`).
    void update_physics(ctx& c, vec3 target, const query_filter_ex& filter, float dt) {
        detail::check(ponko_third_person_camera_update_physics(this, c.raw(), target, &filter, dt));
    }
    /// Level forward (the way the camera faces), for moving the character.
    vec3 flat_forward() const { vec3 v{}; detail::check(ponko_third_person_camera_flat_forward(this, &v)); return v; }
    /// Level right, for strafing.
    vec3 flat_right() const { vec3 v{}; detail::check(ponko_third_person_camera_flat_right(this, &v)); return v; }
    /// Keeps a copy of this rig in the world for the engine to update every frame toward the entity
    /// marked with ctx::set_camera_target (Rust: the rig as a resource), replacing the one kept before.
    void keep(ctx& c) const { detail::check(ponko_ctx_set_third_person_camera_rig(c.raw(), this)); }
    /// The rig the engine keeps, as it last updated it, if one is kept.
    static std::optional<third_person_camera> kept(ctx& c) {
        third_person_camera rig; bool is_kept = false;
        detail::check(ponko_ctx_third_person_camera_rig(c.raw(), &rig, &is_kept));
        if (!is_kept) return std::nullopt;
        return rig;
    }
    /// Stops keeping the rig: the engine leaves the camera alone.
    static void remove_kept(ctx& c) { detail::check(ponko_ctx_set_third_person_camera_rig(c.raw(), nullptr)); }
};

/// Low-detail ground for the whole world beyond the loaded terrain tiles
/// (Rust `FarTerrain`): a few large meshes from a coarse grid of heights,
/// cut away a cell at a time where the cell's own tile is shown, with
/// skirts so there's no crack.
class far_terrain {
    std::unique_ptr<PonkoFarTerrain, void (*)(PonkoFarTerrain*)> raw_;
    static PonkoFarTerrain* make(const std::vector<float>& heights, std::size_t columns, vec2 corner, float spacing,
                                  float cell_size, material m) {
        PonkoFarTerrain* f = ponko_far_terrain_new(heights.data(), heights.size(), columns, corner, spacing, cell_size, m);
        if (!f) throw std::invalid_argument(ponko_last_error());
        return f;
    }
public:
    /// `heights`, `columns` per row (row after row along +Z, NaN for a
    /// hole), the first at `corner` (x, z) and the rest `spacing` apart,
    /// drawn with `m`; cut a cell at a time for tiles `cell_size` wide (a
    /// whole number of spacings; `corner` on the grid of spacings).
    far_terrain(const std::vector<float>& heights, std::size_t columns, vec2 corner, float spacing, float cell_size, material m)
        : raw_(make(heights, columns, corner, spacing, cell_size, m), ponko_far_terrain_free) {}
    /// Cuts the far ground away over `c`, whose own tile is now shown.
    void cut(cell c) { detail::check(ponko_far_terrain_cut(raw_.get(), c, true)); }
    /// Fills `c` with far ground again, when its tile goes.
    void uncut(cell c) { detail::check(ponko_far_terrain_cut(raw_.get(), c, false)); }
    /// Whether cell `c` is cut away.
    bool is_cut(cell c) const {
        bool cut = false; detail::check(ponko_far_terrain_is_cut(raw_.get(), c, &cut, nullptr)); return cut;
    }
    /// How many meshes (and entities) the far ground has in the world.
    std::size_t mesh_count() const {
        bool cut = false; std::size_t n = 0;
        detail::check(ponko_far_terrain_is_cut(raw_.get(), cell{0, 0}, &cut, &n)); return n;
    }
    /// Builds the meshes that changed since the last update (all of them
    /// the first time) and puts them in the world.
    void update(ctx& c) { detail::check(ponko_far_terrain_update(raw_.get(), c.raw())); }
    /// Despawns its entities and frees its meshes; the next update builds them again.
    void clear(ctx& c) { detail::check(ponko_far_terrain_clear(raw_.get(), c.raw())); }
};

namespace detail {
/// Whether plugin type `P` has a `setup(ctx&)` member.
template<class P, class = void> struct has_setup : std::false_type {};
template<class P>
struct has_setup<P, std::void_t<decltype(std::declval<P&>().setup(std::declval<ctx&>()))>> : std::true_type {};
} // namespace detail

/// The game builder (Rust `Game`). Add systems, then run() or run_headless().
class game {
    struct shared_state {
        std::shared_ptr<detail::identity> owner = std::make_shared<detail::identity>();
        detail::first_error error;
    };
    struct holder {
        std::function<void(ctx&)> fn;
        shared_state* st;
    };
    PonkoGame* raw_;
    std::unique_ptr<shared_state> state_ = std::make_unique<shared_state>();
    std::vector<std::unique_ptr<holder>> systems_;
    struct condition_holder {
        std::function<bool(ctx&)> fn;
        shared_state* st;
    };
    std::vector<std::unique_ptr<condition_holder>> conditions_;
    struct screen_holder {
        std::function<ui::id(ctx&)> fn;
        shared_state* st;
    };
    std::vector<std::unique_ptr<screen_holder>> screens_;
    struct group_holder {
        std::function<void(ctx&, asset_group&)> fn;
        shared_state* st;
    };
    std::vector<std::unique_ptr<group_holder>> groups_;
    struct control_holder {
        std::function<std::string(ctx&, std::string_view)> fn;
        shared_state* st;
    };
    std::vector<std::unique_ptr<control_holder>> control_methods_;
    std::unique_ptr<std::function<void(const crash_report&)>> crash_hook_;

    static void control_trampoline(PonkoCtx* raw, const char* params, PonkoControlReply* reply, void* user) noexcept {
        auto& h = *static_cast<control_holder*>(user);
        try {
            ctx c(raw, h.st->owner);
            const auto answer = h.fn(c, params);
            c.run_after_system();
            detail::check(ponko_control_reply_json(reply, answer.c_str()));
        } catch (const std::exception& e) {
            (void)ponko_control_reply_error(reply, e.what());
        } catch (...) {
            (void)ponko_control_reply_error(reply, "native control method threw an exception");
        }
    }

    // A loading state's group builder: adds nothing once the game has failed.
    static void group_trampoline(PonkoCtx* raw, PonkoAssetGroup* group, void* user) noexcept {
        auto& h = *static_cast<group_holder*>(user);
        if (h.st->error.set.load()) return;
        try { ctx c(raw, h.st->owner); asset_group g = asset_group::borrowed(group); h.fn(c, g); }
        catch (...) { h.st->error.capture(); (void)ponko_ctx_quit(raw); }
    }

    // A state screen's builder: an empty screen once the game has failed.
    static std::uint64_t screen_trampoline(PonkoCtx* raw, void* user) noexcept {
        auto& h = *static_cast<screen_holder*>(user);
        if (h.st->error.set.load()) return PONKO_UI_NONE;
        try {
            ctx c(raw, h.st->owner); const ui::id root = h.fn(c); c.run_after_system();
            if (h.st->owner->hook_error.set.load()) std::rethrow_exception(h.st->owner->hook_error.error);
            return root;
        }
        catch (...) { h.st->error.capture(); (void)ponko_ctx_quit(raw); return PONKO_UI_NONE; }
    }

    // A run condition: false (the system doesn't run) once the game has failed.
    static bool condition_trampoline(PonkoCtx* raw, void* user) noexcept {
        auto& h = *static_cast<condition_holder*>(user);
        if (h.st->error.set.load()) return false;
        try { ctx c(raw, h.st->owner); return h.fn(c); }
        catch (...) { h.st->error.capture(); (void)ponko_ctx_quit(raw); return false; }
    }
    condition_holder* keep_condition(std::function<bool(ctx&)> fn) {
        conditions_.push_back(std::make_unique<condition_holder>(condition_holder{std::move(fn), state_.get()}));
        return conditions_.back().get();
    }

    static void trampoline(PonkoCtx* raw, void* user) noexcept {
        auto& h = *static_cast<holder*>(user);
        if (h.st->error.set.load()) return;
        try {
            ctx c(raw, h.st->owner); h.fn(c); c.run_after_system();
            if (h.st->owner->hook_error.set.load()) std::rethrow_exception(h.st->owner->hook_error.error);
        }
        catch (...) { h.st->error.capture(); (void)ponko_ctx_quit(raw); }
    }
    holder* keep(std::function<void(ctx&)> fn) {
        systems_.push_back(std::make_unique<holder>(holder{std::move(fn), state_.get()}));
        return systems_.back().get();
    }
    PonkoGame* take() {
        if (!raw_) throw std::logic_error("game was already run");
        PonkoGame* g = raw_; raw_ = nullptr; return g;
    }
    game& check(PonkoStatus s) { if (!raw_) throw std::logic_error("game was already run"); detail::check(s); return *this; }
public:
    /// A game with a window `title` and size in pixels (ignored when headless), with the built-in systems
    /// (Rust `Game::new`). Throws ponko::error if the library's ABI version differs from this header's,
    /// std::runtime_error for bad arguments.
    game(const char* title, std::uint32_t width, std::uint32_t height) : raw_(nullptr) {
        detail::check_abi_version();
        raw_ = ponko_game_new(title, width, height);
        if (!raw_) throw std::runtime_error(ponko_last_error());
    }
    ~game() { ponko_game_free(raw_); }
    /// Not copyable: run() and run_headless() use the game up.
    ///@{
    game(const game&) = delete;
    game& operator=(const game&) = delete;
    ///@}

    /// Simulation ticks per second (default 60). Throws ponko::error for 0.
    game& fixed_hz(std::uint32_t hz) { return check(ponko_game_fixed_hz(raw_, hz)); }
    /// Seed of ctx::random (runs with the same seed and input repeat exactly).
    game& seed(std::uint64_t seed) { return check(ponko_game_seed(raw_, seed)); }
    /// Names the game for ctx::storage_*: its files go in the folder `app_id`
    /// in the player's data folder. Throws ponko::error for a bad id.
    game& app_id(const char* app_id) { return check(ponko_game_app_id(raw_, app_id)); }
    /// Keeps ctx::storage_* files in `dir` itself (tests, portable installs).
    game& data_dir(const char* dir) { return check(ponko_game_data_dir(raw_, dir)); }
    /// Crash reports (Rust `Game::crash_reports`): whether a panic, a fatal engine error, a crash in
    /// native code (finished at the next start) or ponko::crash_report writes a report into the game's
    /// storage (`crash-reports/`, needs app_id or data_dir), with the game's version (`game_version`,
    /// copied; null: not set), and the hook to run with each. The hook runs inside the crash handling,
    /// on the thread that crashed: keep it quick; what it throws is dropped.
    game& crash_reports(crash_mode mode, const char* game_version = nullptr,
                        std::function<void(const crash_report&)> on_crash = nullptr) {
        PonkoCrashReports r{};
        r.struct_size = sizeof r;
        r.version = PONKO_CRASH_REPORTS_VERSION;
        r.mode = static_cast<std::uint32_t>(mode);
        r.game_version = game_version;
        std::unique_ptr<std::function<void(const crash_report&)>> hook;
        if (on_crash) {
            hook = std::make_unique<std::function<void(const crash_report&)>>(std::move(on_crash));
            r.on_crash = [](const PonkoCrashReport* report, void* user) noexcept {
                try { (*static_cast<std::function<void(const crash_report&)>*>(user))(*report); } catch (...) {}
            };
            r.user = hook.get();
        }
        check(ponko_game_crash_reports(raw_, &r));
        crash_hook_ = std::move(hook);
        return *this;
    }
    /// Opens the first window with the player's saved settings (Rust `Game::restore_window_settings`):
    /// `window.json` from ctx::window_save_settings, before the window exists. Needs app_id or data_dir.
    game& restore_window_settings(bool on = true) { return check(ponko_game_restore_window_settings(raw_, on)); }
    /// Wait for the display between frames (default on).
    game& vsync(bool on) { return check(ponko_game_vsync(raw_, on)); }
    /// HDR display output (Rust `Game::hdr_output`): with `h.enabled` the window opens in an HDR format
    /// and color space where the display system offers one; ctx::set_hdr_output switches it later.
    /// Off by default, and then nothing changes.
    game& hdr_output(const ponko::hdr_output& h) { return check(ponko_game_hdr_output(raw_, &h)); }
    /// Samples per pixel of the 3D frame: 4 (default) or 2.
    game& msaa(std::uint32_t samples) { return check(ponko_game_msaa(raw_, samples)); }
    /// Fill the screen instead of opening a window.
    game& fullscreen(bool on) { return check(ponko_game_fullscreen(raw_, on)); }
    /// Turns on the built-in spatial grid with cells `cell_size` wide, rebuilt from every transform2d at the
    /// start of each tick (ctx::spatial_*).
    game& spatial_grid(float cell_size) { return check(ponko_game_spatial_grid(raw_, cell_size)); }
    /// Opens the control endpoint for tools and agents (ponko-mcp); also on with PONKO_CONTROL=1 in a debug
    /// build of the library.
    game& control_endpoint(bool on) { return check(ponko_game_control_endpoint(raw_, on)); }
    /// Registers a control method. The callback receives wire JSON and returns a JSON value.
    /// An exception becomes the control client's error reply.
    game& control_method(const char* name, const char* about,
                         std::function<std::string(ctx&, std::string_view)> method) {
        control_methods_.push_back(std::make_unique<control_holder>(control_holder{std::move(method), state_.get()}));
        const auto status = ponko_game_control_method(raw_, name, about, control_trampoline, control_methods_.back().get());
        if (status != PONKO_OK) control_methods_.pop_back();
        return check(status);
    }
    /// Registers a named debug toggle and its initial value.
    game& debug_toggle(const char* name, bool on) { return check(ponko_game_debug_toggle(raw_, name, on)); }
    /// Reload textures when their files change, under the same handles (on by
    /// default in debug builds of the engine; PONKO_HOT_RELOAD=1/0 overrides).
    game& hot_reload(bool on) { return check(ponko_game_hot_reload(raw_, on)); }
    /// How big a picture the image decoders accept, for the whole process (Rust's `Game::image_limits`):
    /// the longest side in pixels and the most bytes a decoded picture may take as RGBA8 (defaults 16384
    /// and 512 MiB). Every decoder follows them; a picture past them fails with kind PONKO_ERROR_LIMIT.
    /// Lower them for pictures from players or the network, raise them for large source art.
    game& image_limits(std::uint32_t max_side, std::uint64_t max_bytes) { return check(ponko_game_image_limits(raw_, max_side, max_bytes)); }
    /// The runtime inspector, opened with the backquote key (on by default in
    /// debug builds of the engine; PONKO_INSPECTOR=1/0/open overrides).
    game& inspector(bool on) { return check(ponko_game_inspector(raw_, on)); }
    /// The built-in camera controls: right/middle drag pans, the wheel zooms.
    game& camera_controls() { return check(ponko_game_camera_controls(raw_)); }
    /// Adds the built-in system that moves attached entities next to the
    /// entity they follow (and despawns them when it's gone). Add it after
    /// the sim systems that move things.
    game& follow_attachments() { return check(ponko_game_follow_attachments(raw_)); }
    /// Turns on 2D physics with `gravity` in pixels/s² (y is down): entities
    /// with a rigid_body, a collider and a transform2d are simulated. The step
    /// runs in phase::physics wherever this is called: add systems that feed
    /// it with add(phase::before_physics, ...); sim_system runs after it.
    game& physics(vec2 gravity) { return check(ponko_game_physics(raw_, gravity)); }
    /// Turns on 3D physics with `gravity` in m/s² (+Y up): entities with a
    /// rigid_body3d, a collider3d and a transform3d, and characters, are
    /// simulated at 50 steps a second (unless fixed_hz chose a rate), and
    /// drawn blended between steps. The step runs in phase::physics wherever
    /// this is called; feed it from add(phase::before_physics, ...).
    game& physics3d(vec3 gravity) { return check(ponko_game_physics3d(raw_, gravity)); }
    /// Hosts a lockstep session for `players` (2-8; you are player 0) on
    /// `addr` ("0.0.0.0:7777"; port 0 picks one, written to `*port`). Input
    /// is scheduled `input_delay` ticks ahead; worlds are compared every
    /// `checksum_every` ticks. Ticks run once every player's input is in.
    game& lockstep_host(const char* addr, std::uint32_t players, std::uint32_t input_delay = 3,
                        std::uint32_t checksum_every = 60, std::uint16_t* port = nullptr) {
        return check(ponko_game_lockstep_host(raw_, addr, players, input_delay, checksum_every, port));
    }
    /// Joins the lockstep session hosted at `addr` ("192.168.1.20:7777").
    game& lockstep_join(const char* addr) { return check(ponko_game_lockstep_join(raw_, addr)); }
    /// Gives the game a message endpoint: the engine polls it every frame
    /// and declared replication (ctx::replication) runs over it. A hosting
    /// endpoint makes this game the server, a joining one a client.
    game& messages(net_messages&& endpoint) { return check(ponko_game_messages(raw_, endpoint.release())); }
    /// Hosts a lockstep session over Steam (ponko_ffi feature `steam`):
    /// starts Steam for `app_id` (480: Valve's test app) and opens a
    /// friends-only lobby; friends join through invites (ctx::steam_invite).
    game& lockstep_steam_host(std::uint32_t app_id, std::uint32_t players, std::uint32_t input_delay = 3,
                              std::uint32_t checksum_every = 60) {
        return check(ponko_game_lockstep_steam_host(raw_, app_id, players, input_delay, checksum_every));
    }
    /// Joins a Steam lobby (from steam_lobby_from_args or steam_wait_for_invite).
    game& lockstep_steam_join(std::uint32_t app_id, std::uint64_t lobby) {
        return check(ponko_game_lockstep_steam_join(raw_, app_id, lobby));
    }
    /// Moves navmesh agents every simulation tick, at this point in the order.
    game& navmesh3d() { return check(ponko_game_navmesh3d(raw_)); }
    /// The water kit's systems (Rust water::WaterPlugin): updates water and floats buoyant bodies.
    game& water() { return check(ponko_game_water(raw_)); }
    /// A system on the fixed simulation tick; `every` > 1 runs it every n ticks.
    template<class F> game& sim_system(F fn, const char* name = nullptr, std::uint32_t every = 1) {
        return check(ponko_game_sim_system(raw_, name, every, trampoline, keep(std::move(fn))));
    }
    /// A system in an explicit phase; e.g. phase::before_physics for logic the
    /// current tick's physics step must see. Fixed-tick phases take `every` > 1.
    template<class F> game& add(phase p, F fn, const char* name = nullptr, std::uint32_t every = 1) {
        return check(ponko_game_add_system(raw_, static_cast<std::uint32_t>(p), name, every, trampoline,
                                            keep(std::move(fn))));
    }
    /// A system in an explicit phase with a declaration of what it reads and
    /// writes (see ponko::access). Debug builds stop the game if it uses
    /// anything undeclared. It still runs alone and in order, also with
    /// parallel_systems() on: a C++ system takes the whole ctx.
    template<class F> game& add(phase p, const access& a, F fn, const char* name = nullptr, std::uint32_t every = 1) {
        return check(ponko_game_add_system_with_access(raw_, static_cast<std::uint32_t>(p), name, every, a.raw(),
                                                        trampoline, keep(std::move(fn))));
    }
    /// One line per pair of declared systems in the same phase: whether
    /// they could run together, or what they conflict on.
    std::string access_report() {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_game_access_report(raw_, b, c, l); });
    }
    /// Turns the parallel system scheduler on or off (off by default).
    /// It runs Rust systems that declared what they use at the same time
    /// when they don't conflict. Systems added from C++ take the whole
    /// ctx, so they always run one at a time, in registration order.
    game& parallel_systems(bool on = true) { return check(ponko_game_parallel_systems(raw_, on)); }
    /// The stages each phase's systems run in, which systems share a
    /// stage, and why each stage starts where it does.
    std::string schedule_plan() {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_game_schedule_plan(raw_, b, c, l); });
    }
    // Game states (Rust's ponko::state): one state machine per game, its
    // states an enum of the game's (any type that casts to uint32_t).
    // The state only changes between simulation ticks.
    /// Gives the game a state machine starting in `initial`; its on_enter
    /// work runs at the start of the first tick.
    template<class S> game& state(S initial) {
        return check(ponko_game_state(raw_, static_cast<std::uint32_t>(initial)));
    }
    /// Runs `fn` each time the game enters `s`, at the start of a tick.
    template<class S, class F> game& on_enter(S s, F fn) {
        return check(ponko_game_on_enter(raw_, static_cast<std::uint32_t>(s), trampoline, keep(std::move(fn))));
    }
    /// Runs `fn` each time the game leaves `s`, before it changes.
    template<class S, class F> game& on_exit(S s, F fn) {
        return check(ponko_game_on_exit(raw_, static_cast<std::uint32_t>(s), trampoline, keep(std::move(fn))));
    }
    /// Pauses the `buses` named (made if missing) while state `s` is on the stack (Rust
    /// `Game::pause_audio_in`): a pause menu pushed over play pauses the world's sound. Call state() first.
    template<class S> game& pause_audio_in(S s, const std::vector<const char*>& buses) {
        return check(ponko_game_pause_audio_in_state(raw_, static_cast<std::uint32_t>(s), buses.data(), buses.size()));
    }
    /// Makes `loading` a loading screen (Rust `Game::loading_state`): entering it starts a group called
    /// `name` that `build(ctx, group)` fills; once it is ready the game moves to `then`. Leaving `loading`
    /// first cancels it; a failed group keeps the game in `loading`.
    template<class S, class F> game& loading_state(S loading, const char* name, F build, S then) {
        groups_.push_back(std::make_unique<group_holder>(group_holder{std::move(build), state_.get()}));
        return check(ponko_game_loading_state(raw_, static_cast<std::uint32_t>(loading), name, group_trampoline,
                                              groups_.back().get(), static_cast<std::uint32_t>(then)));
    }
    /// A UI screen that is up exactly while the state is `s` (Rust `Game::state_screen`): `build(ctx)` makes
    /// a root on entering `s` and returns it; it is pushed in `mode`, and popped and removed on leaving.
    /// Closing the screen (Escape, or a button with `ui::tree::set_closes_screen`) pops `s` when it was
    /// pushed over another state; the only state has nothing to go back to, so its screen comes back.
    template<class S, class F> game& state_screen(S s, ui::screen_mode mode, F build) {
        screens_.push_back(std::make_unique<screen_holder>(screen_holder{std::move(build), state_.get()}));
        return check(ponko_game_state_screen(raw_, static_cast<std::uint32_t>(s), static_cast<std::uint32_t>(mode),
                                             screen_trampoline, screens_.back().get()));
    }
    /// add(), for a system that runs only while the state is `s` (not
    /// while another state is pushed over it).
    template<class S, class F>
    game& add_in_state(phase p, S s, F fn, const char* name = nullptr, std::uint32_t every = 1) {
        return check(ponko_game_add_system_in_state(raw_, static_cast<std::uint32_t>(p), name, every,
                                                    static_cast<std::uint32_t>(s), trampoline, keep(std::move(fn))));
    }
    /// add_in_state(), also running while other states are pushed over `s`
    /// (drawing the level under a pause menu).
    template<class S, class F>
    game& add_in_stack(phase p, S s, F fn, const char* name = nullptr, std::uint32_t every = 1) {
        return check(ponko_game_add_system_in_stack(raw_, static_cast<std::uint32_t>(p), name, every,
                                                    static_cast<std::uint32_t>(s), trampoline, keep(std::move(fn))));
    }
    /// add(), for a system that runs only when `condition(ctx)` returns
    /// true, checked right before it would run (Rust's `.run_if(|ctx| ...)`).
    /// Needs no state machine. The condition should only read.
    template<class C, class F>
    game& add_if(phase p, C condition, F fn, const char* name = nullptr, std::uint32_t every = 1) {
        return check(ponko_game_add_system_if(raw_, static_cast<std::uint32_t>(p), name, every,
                                              condition_trampoline, keep_condition(std::move(condition)),
                                              trampoline, keep(std::move(fn))));
    }
    /// A system once per frame (input, camera, UI). Not run headless.
    template<class F> game& system(F fn, const char* name = nullptr) {
        return check(ponko_game_system(raw_, name, trampoline, keep(std::move(fn))));
    }
    /// A system that runs every frame after animation posed the skeletons, before drawing (IK, look-at).
    template<class F> game& late_system(F fn, const char* name = nullptr) {
        return check(ponko_game_late_system(raw_, name, trampoline, keep(std::move(fn))));
    }
    /// Like add(), and returns the system's id for remove(). Two systems
    /// made from the same function get different ids.
    template<class F> system_id add_system(phase p, F fn, const char* name = nullptr, std::uint32_t every = 1) {
        system_id id;
        check(ponko_game_add_system_id(raw_, static_cast<std::uint32_t>(p), name, every, trampoline,
                                        keep(std::move(fn)), &id.bits));
        return id;
    }
    /// Removes systems by id: one from add_system, or a built-in such as
    /// systems::draw_sprites (every instance of it). An id that names
    /// nothing is not an error.
    game& remove(system_id id) { return check(ponko_game_remove(raw_, id.bits)); }
    /// Adds built-in system `id` (systems::draw_tilemaps, ...) to phase `p` (Rust: a function of
    /// `ponko::systems` with `Game::add`): remove() it first to move it, or add one the engine never adds by
    /// itself (systems::follow_attachments). Physics and navigation steps come with their declarations.
    game& add_builtin(phase p, system_id id) { return check(ponko_game_add_builtin(raw_, static_cast<std::uint32_t>(p), id.bits)); }
    /// Removes systems by name, e.g. "ponko::ecs::draw_sprites". Kept for
    /// existing code: prefer remove(systems::draw_sprites).
    game& remove_system(const char* name) { return check(ponko_game_remove_system(raw_, name)); }
    /// A startup callback, `setup(ctx&)`: runs once after the engine started
    /// and before any system (also with zero ticks). Callbacks and plugin
    /// setups run in the order they were added, before the one passed to
    /// run(); commands are applied after each.
    template<class S> game& setup(S fn) { return check(ponko_game_setup(raw_, trampoline, keep(std::move(fn)))); }
    /// Adds a module of the game (Rust `Plugin`): any object with
    /// `void build(ponko::game&)`, which adds its systems and settings now,
    /// and optionally `void setup(ponko::ctx&)`, queued like setup(). Each
    /// system names its phase, so the order plugins are added in only
    /// matters between systems of the same phase.
    template<class P> game& plugin(P p) {
        if (!raw_) throw std::logic_error("game was already run");
        auto shared = std::make_shared<P>(std::move(p));
        shared->build(*this);
        if constexpr (detail::has_setup<P>::value) setup([shared](ctx& c) { shared->setup(c); });
        return *this;
    }

    /// Opens the window and runs until quit. `setup(ctx&)` runs once first.
    template<class S> void run(S setup) {
        holder* h = keep(std::move(setup));
        auto status = ponko_game_run(take(), trampoline, h);
        state_->error.rethrow();
        state_->owner->hook_error.rethrow();
        detail::check(status);
    }
    /// Opens the window and runs until quit, with no setup callback.
    void run() { run([](ctx&) {}); }
    /// Runs up to `ticks` ticks with no window or GPU and returns the world.
    template<class S> ponko::world run_headless(std::uint64_t ticks, S setup) {
        holder* h = keep(std::move(setup));
        PonkoWorld* result = nullptr;
        auto status = ponko_game_run_headless(take(), ticks, trampoline, h, &result);
        ponko::world owned(result, state_->owner);
        state_->error.rethrow();
        state_->owner->hook_error.rethrow();
        detail::check(status);
        return owned;
    }
    /// Runs up to `ticks` ticks with no window or GPU and returns the world, with no setup callback.
    ponko::world run_headless(std::uint64_t ticks) { return run_headless(ticks, [](ctx&) {}); }

    /// Runs a copy of `s` with the game from its first tick (Rust's
    /// `Game::input_scenario`); the report ends up in the world.
    game& input_scenario(const ponko::input_scenario& s) { return check(ponko_game_input_scenario(raw_, s.raw())); }
    /// Headless runs run whole frames (the UI, frame, late and draw systems)
    /// in a virtual window of the game's size (Rust's `Game::headless_frames`).
    game& headless_frames(bool on) { return check(ponko_game_headless_frames(raw_, on)); }
    /// What run_headless_scenario returns: `auto [world, report] = ...`.
    struct scenario_run {
        ponko::world world;
        ponko::scenario_report report;
    };
    /// Runs `s` headlessly for its ticks and returns the world and the
    /// scenario's report (Rust's `run_headless_scenario`): the bounded
    /// runner for tests and CI. A failed check ends the run at that tick.
    template<class S> scenario_run run_headless_scenario(const ponko::input_scenario& s, S setup) {
        const std::uint64_t ticks = s.ticks();
        this->input_scenario(s);
        ponko::world w = run_headless(ticks, std::move(setup));
        ponko::scenario_report pending{};
        pending.status = PONKO_SCENARIO_PENDING;
        pending.duration = ticks;
        pending.steps = s.step_count();
        auto report = w.scenario_report();
        return scenario_run{std::move(w), report ? *report : pending};
    }
    /// run_headless_scenario with no setup callback.
    scenario_run run_headless_scenario(const ponko::input_scenario& s) { return run_headless_scenario(s, [](ctx&) {}); }

    /// What profile_headless returns.
    struct profiled {
        ponko::world world;
        ponko::perf_report report;
    };
    /// Like run_headless, but timed: the first `warmup` ticks aren't measured,
    /// the next `ticks` are. The report says where the time went (systems,
    /// PONKO_PROFILE_SCOPE scopes), how many allocations each tick made, the
    /// slowest ticks and what to try. For tests and CI: see perf_report::check.
    template<class S> profiled profile_headless(std::uint64_t warmup, std::uint64_t ticks, S setup) {
        holder* h = keep(std::move(setup));
        PonkoWorld* result = nullptr;
        auto status = ponko_game_profile_headless(take(), warmup, ticks, trampoline, h, &result);
        ponko::world owned(result, state_->owner);
        state_->error.rethrow();
        state_->owner->hook_error.rethrow();
        detail::check(status);
        return profiled{std::move(owned), ponko::perf_report(detail::read_text([](char* b, std::size_t c, std::size_t* n) { return ponko_last_perf_report(b, c, n); }))};
    }
};

/// The lobby of the Steam invite the game was launched with, or 0.
inline std::uint64_t steam_lobby_from_args() {
    std::uint64_t l = 0;
    detail::check(ponko_steam_lobby_from_args(&l));
    return l;
}
/// Waits up to `timeout_ms` for the player to accept a friend's Steam
/// invite; the lobby, or 0. Starts Steam for `app_id`.
inline std::uint64_t steam_wait_for_invite(std::uint32_t app_id, std::uint32_t timeout_ms) {
    std::uint64_t l = 0;
    detail::check(ponko_steam_wait_for_invite(app_id, timeout_ms, &l));
    return l;
}

/// Steam's store-side services (the library built with `steam`): achievements,
/// stats, rich presence, the overlay's state and the Deck's text input; cloud
/// saves are ctx::steam_sync_cloud. One Steam per process. Before start(), when
/// Steam isn't running, and without the feature, every call does nothing and
/// returns false, so one build runs from any store.
namespace steam {
/// What the text input accepts (Rust's SteamTextMode).
enum class text_mode : std::uint32_t { single_line = 0, multi_line = 1, password = 2, email = 3, numeric = 4 };
/// Starts the services for `app_id` (480: Valve's test app); true when Steam runs.
inline bool start(std::uint32_t app_id) { return ponko_steam_start(app_id); }
/// Whether the services reach a running Steam.
inline bool running() { return ponko_steam_running(); }
/// Runs Steam's callbacks: the overlay's state and the text input. Once a frame.
inline void update() { ponko_steam_update(); }
/// Unlocks the achievement `name`; false when Steam is absent or refuses the name.
inline bool unlock_achievement(const char* name) { return ponko_steam_unlock_achievement(name); }
/// Locks the achievement `name` again.
inline bool clear_achievement(const char* name) { return ponko_steam_clear_achievement(name); }
/// Whether the player has the achievement; nothing when Steam is absent or doesn't know it.
inline std::optional<bool> achievement(const char* name) {
    const std::int32_t a = ponko_steam_achievement(name);
    if (a < 0) return std::nullopt;
    return a == 1;
}
/// Sets an integer stat; kept after store_stats().
inline bool set_stat(const char* name, std::int32_t value) { return ponko_steam_set_stat_i32(name, value); }
/// Sets a float stat; kept after store_stats().
inline bool set_stat(const char* name, float value) { return ponko_steam_set_stat_f32(name, value); }
/// An integer stat; nothing when Steam is absent or doesn't know it.
inline std::optional<std::int32_t> stat_i32(const char* name) {
    std::int32_t v = 0;
    if (!ponko_steam_stat_i32(name, &v)) return std::nullopt;
    return v;
}
/// A float stat; nothing when Steam is absent or doesn't know it.
inline std::optional<float> stat_f32(const char* name) {
    float v = 0;
    if (!ponko_steam_stat_f32(name, &v)) return std::nullopt;
    return v;
}
/// Sends the changed achievements and stats to Steam's servers.
inline bool store_stats() { return ponko_steam_store_stats(); }
/// Sets the rich presence `key` (an empty value removes it).
inline bool set_rich_presence(const char* key, const char* value) { return ponko_steam_set_rich_presence(key, value); }
/// Removes every rich presence key.
inline void clear_rich_presence() { ponko_steam_clear_rich_presence(); }
/// Whether the overlay is open, as of the last update(): pause while it is.
inline bool overlay_open() { return ponko_steam_overlay_open(); }
/// Whether the game runs on a Steam Deck.
inline bool is_steam_deck() { return ponko_steam_is_steam_deck(); }
/// Whether Steam runs in Big Picture mode.
inline bool is_big_picture() { return ponko_steam_is_big_picture(); }
/// Opens the full-screen gamepad text input (Big Picture, Deck); false when it didn't open.
inline bool show_text_input(text_mode mode, const char* description, const char* existing, std::uint32_t max_chars) {
    return ponko_steam_show_text_input(static_cast<std::uint32_t>(mode), description, existing, max_chars);
}
/// How the text input closed, once: {PONKO_STEAM_TEXT_*, the submitted text}.
inline std::pair<std::uint32_t, std::string> take_text_input() {
    std::uint32_t state = PONKO_STEAM_TEXT_NONE;
    std::size_t n = 0;
    if (ponko_steam_take_text_input(&state, nullptr, 0, &n) == PONKO_OK) return {state, std::string()};
    std::vector<char> b(n + 1);
    detail::check(ponko_steam_take_text_input(&state, b.data(), b.size(), &n));
    return {state, std::string(b.data(), n)};
}
/// Opens the Deck's floating keyboard beside the text field at x, y (pixels) of width by height.
inline bool show_floating_keyboard(text_mode mode, std::int32_t x, std::int32_t y, std::int32_t width, std::int32_t height) {
    return ponko_steam_show_floating_keyboard(static_cast<std::uint32_t>(mode), x, y, width, height);
}
/// Whether Steam Cloud keeps this player's files.
inline bool cloud_enabled() { return ponko_steam_cloud_enabled(); }
} // namespace steam

// ---- glTF files as scenes (from Blender and other editors) ----------------------
/// Spawns the glTF file at `path` as a scene placed at `at`: a root entity
/// and an entity per node, attached to its parent, with its model, light and
/// what its custom properties ask for (components by name, colliders, spawn
/// points, prefabs, levels of detail, layers). `without_saved_objects` leaves
/// the prefab nodes out (a world save holds them). Returns the root. With hot
/// reload on, exporting the file again updates it in place.
inline entity spawn_gltf(ctx& c, const char* path, transform3d at, bool without_saved_objects = false) {
    entity root = 0;
    detail::check(ponko_ctx_spawn_gltf(c.raw(), path, at, without_saved_objects, &root));
    return root;
}
/// Spawns the tile of terrain file `path` (`"format": "ponko-terrain"`, as
/// the editor's terrain window saves it) with its corner at `at`: an entity
/// with a transform3d, a terrain_tile and the TerrainFile that saves it in a
/// scene. Throws (not found, or what's wrong with the file) and spawns
/// nothing if the file doesn't load.
inline entity spawn_terrain_from_file(ctx& c, const char* path, vec3 at) {
    entity e = 0;
    detail::check(ponko_ctx_terrain_tile_spawn_from_file(c.raw(), path, at, &e));
    return e;
}
/// Despawns a scene spawned with spawn_gltf; false if `root` is not one.
inline bool despawn_gltf(ctx& c, entity root) {
    const PonkoStatus s = ponko_ctx_despawn_gltf(c.raw(), root);
    if (s == PONKO_NOT_FOUND) return false;
    detail::check(s);
    return true;
}
/// The entity of the node at `path` ("House/Door") or named `path` in the
/// scene of `root`; empty if there is none.
inline std::optional<entity> gltf_node(ctx& c, entity root, const char* path) {
    entity e = 0;
    const PonkoStatus s = ponko_gltf_node_entity(ponko_ctx_world(c.raw()), root, path, &e);
    if (s == PONKO_NOT_FOUND) return std::nullopt;
    detail::check(s);
    return e;
}
/// Every custom property of the node `e` was spawned from, as a JSON object.
inline std::string gltf_node_extras(ctx& c, entity e) {
    return detail::read_text([&](char* b, std::size_t n, std::size_t* len) { return ponko_gltf_node_extras(ponko_ctx_world(c.raw()), e, b, n, len); });
}
/// The name of the spawn point `e` (a node with ponko:spawn); throws if it is not one.
inline std::string spawn_point_name(ctx& c, entity e) {
    return detail::read_text([&](char* b, std::size_t n, std::size_t* len) { return ponko_spawn_point_name(ponko_ctx_world(c.raw()), e, b, n, len); });
}
/// Reads the glTF file at `path` and checks its custom properties without
/// spawning anything, as JSON: {"nodes": [{"name", "path", "parent",
/// "transform", "world", "extras", "mesh", "light"}], "spawn_points":
/// [{"name", "transform"}]}. Throws with the file and node on an error.
inline std::string gltf_scene_json(ctx& c, const char* path) {
    return detail::read_text([&](char* b, std::size_t n, std::size_t* len) { return ponko_gltf_scene_load(c.raw(), path, b, n, len); });
}
/// The JSON Schema of the custom properties the game reads (the ponko:
/// keys and every reflected component), as `ponko schema` writes it.
inline std::string gltf_properties_schema(ctx& c) {
    return detail::read_text([&](char* b, std::size_t n, std::size_t* len) { return ponko_gltf_properties_schema(ponko_ctx_world(c.raw()), b, n, len); });
}

/// Loads an Aseprite JSON Array export and selects a timeline frame. Use
/// a named layer for split exports. The context owns the atlas texture.
inline sprite aseprite_frame(ctx& c, const char* path, std::uint32_t frame,
                             bool split_layers = false, const char* layer = nullptr) {
    sprite s{};
    detail::check(ponko_ctx_aseprite_frame(c.raw(), path, split_layers, frame, layer, &s));
    return s;
}
/// Starts an Aseprite tag on an entity, adding its sprite if needed. Empty
/// tag plays the entire timeline. Playback follows asset hot reload.
inline void play_aseprite(ctx& c, entity e, const char* path, const char* tag = "",
                          bool split_layers = false, const char* layer = nullptr) {
    detail::check(ponko_ctx_play_aseprite(c.raw(), e, path, split_layers, tag, layer));
}

// ---- Levels from level editors (Tiled, LDtk) and tilemap extras ----------------
/// How spawn_level spawns a level; the defaults spawn everything.
struct level_options {
    /// Object classes spawned as prefab instances: {class, prefab file}.
    std::vector<std::pair<std::string, std::string>> prefabs;
    /// More solid things by name: layers, LDtk IntGrid values, tile classes or
    /// tags (layers named "collision" or "collisions" are solid anyway).
    std::vector<std::string> solid;
    /// For an LDtk project: the level (identifier or iid); empty for the first.
    std::string level;
    /// Spawn no colliders.
    bool no_colliders = false;
    /// The z of the objects' layer; layers below get one less each, above one more.
    float z = 0.0f;
    /// Spawn none of the objects a world save holds (the prefab objects): for
    /// spawning the level again after load_world, which brings them back.
    bool without_saved_objects = false;
};
/// A spawned level object's data: where it is, its shape, its tile.
using level_object = PonkoLevelObject;

/// Spawns the level at `path` (Tiled .tmx / .tmj, LDtk .ldtk) with its
/// top-left at `at`: layers as tilemaps, objects as entities or prefabs,
/// colliders with 2D physics. Returns the level's root entity. With hot
/// reload on, editing the files spawns it again in place.
inline entity spawn_level(ctx& c, const char* path, vec2 at, const level_options& options = {}) {
    std::vector<const char*> classes, paths, solid;
    for (const auto& p : options.prefabs) { classes.push_back(p.first.c_str()); paths.push_back(p.second.c_str()); }
    for (const auto& s : options.solid) solid.push_back(s.c_str());
    PonkoLevelOptions o{classes.data(), paths.data(), classes.size(), solid.data(), solid.size(),
                        options.level.empty() ? nullptr : options.level.c_str(), options.no_colliders, options.z};
    entity root = 0;
    detail::check(options.without_saved_objects
                      ? ponko_ctx_spawn_level_without_saved_objects(c.raw(), path, at, &o, &root)
                      : ponko_ctx_spawn_level(c.raw(), path, at, &o, &root));
    return root;
}
/// Spawns a selected LDtk level at its authored position relative to
/// `world_origin`. Pairs map LDtk entity classes to reflected component names.
inline entity spawn_ldtk_level(ctx& c, const char* path, vec2 world_origin,
                               const level_options& options = {},
                               const std::vector<std::pair<std::string, std::string>>& reflection = {}) {
    std::vector<const char*> prefab_classes, prefab_paths, solid, classes, components;
    for (const auto& p : options.prefabs) {
        prefab_classes.push_back(p.first.c_str());
        prefab_paths.push_back(p.second.c_str());
    }
    for (const auto& s : options.solid) solid.push_back(s.c_str());
    for (const auto& m : reflection) {
        classes.push_back(m.first.c_str());
        components.push_back(m.second.c_str());
    }
    const PonkoLevelOptions level{prefab_classes.data(), prefab_paths.data(), prefab_classes.size(),
                                  solid.data(), solid.size(), options.level.empty() ? nullptr : options.level.c_str(),
                                  options.no_colliders, options.z};
    const PonkoLdtkOptions ldtk{&level, true, options.without_saved_objects,
                               classes.data(), components.data(), classes.size()};
    entity root = 0;
    detail::check(ponko_ctx_spawn_ldtk_level(c.raw(), path, world_origin, &ldtk, &root));
    return root;
}
/// The name, stable IDs and placement of one level in an LDtk project.
struct ldtk_level_info {
    std::string world; ///< World name, empty for a legacy project.
    std::string world_iid; ///< Stable world instance ID.
    std::string identifier; ///< Level name.
    std::string iid; ///< Stable level instance ID.
    vec2 position; ///< Top-left position in world pixels.
    std::uint32_t width; ///< Width in pixels.
    std::uint32_t height; ///< Height in pixels.
    std::int32_t depth; ///< Authored world depth.
};

/// Reads an LDtk project's level placements in world and level order.
inline std::vector<ldtk_level_info> ldtk_levels(ctx& c, const char* path) {
    struct collector {
        std::vector<ldtk_level_info> levels;
        std::exception_ptr error;
    } state;
    detail::check(ponko_ctx_visit_ldtk_levels(c.raw(), path, [](void* user, const PonkoLdtkLevelInfo* level) {
        auto& data = *static_cast<collector*>(user);
        if (data.error) return;
        try {
            const auto str = [](PonkoLdtkText text) { return std::string(text.bytes, text.len); };
            data.levels.push_back({str(level->world), str(level->world_iid), str(level->identifier),
                                   str(level->iid), level->position, level->width, level->height, level->depth});
        } catch (...) {
            data.error = std::current_exception();
        }
    }, &state));
    if (state.error) std::rethrow_exception(state.error);
    return std::move(state.levels);
}

/// Despawns a level and everything it spawned; false if `root` was gone.
inline bool despawn_level(ctx& c, entity root) {
    const PonkoStatus s = ponko_ctx_despawn_level(c.raw(), root);
    if (s == PONKO_NOT_FOUND) return false;
    detail::check(s);
    return true;
}
/// Every entity a level spawned (layers, objects, colliders), root excluded.
inline std::vector<entity> level_entities(ctx& c, entity root) {
    PonkoWorld* w = ponko_ctx_world(c.raw());
    std::size_t n = 0;
    detail::check(ponko_level_entities(w, root, nullptr, 0, &n));
    std::vector<entity> out(n);
    detail::check(ponko_level_entities(w, root, out.data(), out.size(), &n));
    out.resize(std::min(n, out.size()));
    return out;
}
/// The level object on `e`; throws if `e` is not one.
inline level_object level_object_of(ctx& c, entity e) {
    level_object o{};
    detail::check(ponko_level_object_get(ponko_ctx_world(c.raw()), e, &o));
    return o;
}
/// A level object's name, class and layer as the editor shows them.
///@{
inline std::string level_object_name(ctx& c, entity e) {
    return detail::read_text([&](char* b, std::size_t n, std::size_t* len) { return ponko_level_object_name(ponko_ctx_world(c.raw()), e, b, n, len); });
}
inline std::string level_object_class(ctx& c, entity e) {
    return detail::read_text([&](char* b, std::size_t n, std::size_t* len) { return ponko_level_object_class(ponko_ctx_world(c.raw()), e, b, n, len); });
}
inline std::string level_object_layer(ctx& c, entity e) {
    return detail::read_text([&](char* b, std::size_t n, std::size_t* len) { return ponko_level_object_layer(ponko_ctx_world(c.raw()), e, b, n, len); });
}
///@}
/// A custom property of a level object, layer or level root, as JSON
/// ("5", "\"gold\"", "true"); throws if there is none.
inline std::string level_property_json(ctx& c, entity e, const char* name) {
    return detail::read_text([&](char* b, std::size_t n, std::size_t* len) { return ponko_level_property_json(ponko_ctx_world(c.raw()), e, name, b, n, len); });
}
/// How fast a tilemap follows the camera: (1, 1) moves with the world, (0, 0)
/// stays on screen, in between scrolls slower (far layers). `origin` is the
/// camera center where the map sits at its own position.
inline void tilemap_set_parallax(ctx& c, entity map, vec2 parallax, vec2 origin = {0, 0}) {
    detail::check(ponko_tilemap_set_parallax(ponko_ctx_world(c.raw()), map, parallax, origin));
}
/// Animates every cell showing `tile`: the frames in turn, each for its seconds.
/// An empty list stops the animation.
inline void tilemap_animate(ctx& c, entity map, std::uint16_t tile, const std::vector<std::pair<std::uint16_t, float>>& frames) {
    std::vector<std::uint16_t> f;
    std::vector<float> s;
    for (const auto& p : frames) { f.push_back(p.first); s.push_back(p.second); }
    detail::check(ponko_tilemap_animate(ponko_ctx_world(c.raw()), map, tile, f.data(), s.data(), f.size()));
}
/// Sets a cell's frame and its PONKO_TILE_FLIP_* bits.
inline void tilemap_set_flipped(ctx& c, entity map, std::uint32_t x, std::uint32_t y, std::uint16_t frame, std::uint8_t flip) {
    detail::check(ponko_tilemap_set_flipped(ponko_ctx_world(c.raw()), map, x, y, frame, flip));
}
/// A cell's PONKO_TILE_FLIP_* bits.
inline std::uint8_t tilemap_flip(ctx& c, entity map, std::uint32_t x, std::uint32_t y) {
    std::uint8_t f = 0;
    detail::check(ponko_tilemap_flip(ponko_ctx_world(c.raw()), map, x, y, &f));
    return f;
}
/// An autotile rule: 4 or 8 neighbours, the frame for each mask (16 frames
/// for 4 neighbours; 47 "blob" frames for 8), and whether the map's outside
/// counts as filled.
struct autotile_rule {
    /// 4 or 8.
    std::uint32_t neighbours = 4;
    /// 16 frames (4 neighbours, mask N=1 E=2 S=4 W=8) or 47 (8 neighbours, blob order).
    std::vector<std::uint16_t> frames;
    /// Cells beyond the map's edge count as filled.
    bool outside_filled = true;
};
/// Applies the rule over the whole map: each cell `filled` marks (width ×
/// height, row by row) gets the frame for its filled neighbours; other cells
/// are left alone.
inline void tilemap_autotile(ctx& c, entity map, const autotile_rule& rule, const std::vector<bool>& filled) {
    std::unique_ptr<bool[]> cells(new bool[filled.size()]);
    std::copy(filled.begin(), filled.end(), cells.get());
    detail::check(ponko_tilemap_autotile(ponko_ctx_world(c.raw()), map, rule.neighbours, rule.frames.data(), rule.frames.size(),
                                         rule.outside_filled, cells.get()));
}
/// Fills or clears one cell and fixes it and its neighbours to the rule.
inline void tilemap_paint_autotile(ctx& c, entity map, const autotile_rule& rule, std::uint32_t x, std::uint32_t y, bool on) {
    detail::check(ponko_tilemap_paint_autotile(ponko_ctx_world(c.raw()), map, rule.neighbours, rule.frames.data(), rule.frames.size(),
                                               rule.outside_filled, x, y, on));
}
/// The cells showing one of `solid` tiles, merged into few rectangles, in
/// pixels from the map's top-left.
inline std::vector<rect> tilemap_solid_rects(ctx& c, entity map, const std::vector<std::uint16_t>& solid) {
    PonkoWorld* w = ponko_ctx_world(c.raw());
    std::size_t n = 0;
    detail::check(ponko_tilemap_solid_rects(w, map, solid.data(), solid.size(), nullptr, 0, &n));
    std::vector<rect> out(n);
    detail::check(ponko_tilemap_solid_rects(w, map, solid.data(), solid.size(), out.data(), out.size(), &n));
    out.resize(std::min(n, out.size()));
    return out;
}
/// Spawns fixed bodies for the cells showing one of `solid` tiles, merged
/// into few rectangles; returns how many. Needs 2D physics.
inline std::size_t tilemap_spawn_colliders(ctx& c, entity map, const std::vector<std::uint16_t>& solid) {
    std::size_t n = 0;
    detail::check(ponko_tilemap_spawn_colliders(ponko_ctx_world(c.raw()), map, solid.data(), solid.size(), &n));
    return n;
}
/// The AI kit (Rust `ponko::ai`): behaviour trees compiled from JSON, and
/// brains with their blackboards that a native game keeps in its own
/// storage, thinking when it calls brain::think. The format of a tree is in
/// the guide's "Game AI".
namespace ai {
/// The id of key `name` (Rust `Key::named(name).id()`): blackboard entries,
/// actions and parameters go by id. Make each once and keep it.
inline std::uint64_t key(const char* name) {
    std::uint64_t k = 0;
    detail::check(ponko_ai_key(name, &k));
    return k;
}
/// The name of a key id.
inline std::string key_name(std::uint64_t key) {
    return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_ai_key_name(key, b, c, l); });
}
/// What an action did (Rust `Status`).
enum class status { running = PONKO_AI_RUNNING, success = PONKO_AI_SUCCESS, failure = PONKO_AI_FAILURE };

/// A compiled behaviour tree, shared by any number of brains (also from
/// several threads). Move-only; the destructor frees it.
class behavior_tree {
    PonkoBehaviorTree* raw_;
public:
    /// Compiles `json`; throws ponko::error naming the node and the problem.
    explicit behavior_tree(const std::string& json) : raw_(ponko_behavior_tree_new(json.c_str())) {
        if (!raw_) throw detail::last_error();
    }
    /// Compiles `json`, its `subtree` nodes running the trees in `subtrees`
    /// by name (they may name each other; the new tree keeps what it needs).
    /// Throws ponko::error for a bad tree or a cycle, naming the trees on it.
    behavior_tree(const std::string& json, const std::vector<std::pair<std::string, const behavior_tree*>>& subtrees)
        : raw_(nullptr) {
        std::vector<const char*> names;
        std::vector<const PonkoBehaviorTree*> trees;
        for (const auto& [name, tree] : subtrees) {
            names.push_back(name.c_str());
            trees.push_back(tree ? tree->raw() : nullptr);
        }
        raw_ = ponko_behavior_tree_new_linked(json.c_str(), names.data(), trees.data(), names.size());
        if (!raw_) throw detail::last_error();
    }
    ~behavior_tree() { ponko_behavior_tree_free(raw_); }
    /// Movable: takes `o`'s handle (a compiled tree).
    ///@{
    behavior_tree(behavior_tree&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    behavior_tree& operator=(behavior_tree&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    ///@}
    /// Not copyable: it owns a compiled tree.
    ///@{
    behavior_tree(const behavior_tree&) = delete;
    behavior_tree& operator=(const behavior_tree&) = delete;
    ///@}
    /// The C tree.
    const PonkoBehaviorTree* raw() const noexcept { return raw_; }
    /// How many nodes it has.
    std::size_t size() const { std::size_t n = 0; detail::check(ponko_behavior_tree_len(raw_, &n)); return n; }
};

/// One agent's mind and blackboard (Rust `Brain` and `Blackboard`). Use
/// from one thread at a time. Move-only; the destructor frees it.
class brain {
    PonkoBrain* raw_;
    struct adopt {};
    brain(adopt, PonkoBrain* raw) noexcept : raw_(raw) {}
public:
    /// A brain that thinks every `period` ticks (see due()).
    explicit brain(std::uint32_t period = 1) : raw_(ponko_brain_new(period)) {}
    ~brain() { ponko_brain_free(raw_); }
    /// Movable: takes `o`'s handle (a brain and its blackboard).
    ///@{
    brain(brain&& o) noexcept : raw_(o.raw_) { o.raw_ = nullptr; }
    brain& operator=(brain&& o) noexcept { std::swap(raw_, o.raw_); return *this; }
    ///@}
    /// Not copyable: it owns a brain and its blackboard.
    ///@{
    brain(const brain&) = delete;
    brain& operator=(const brain&) = delete;
    ///@}
    /// Thinks once with `tree` at sim tick `tick` of `dt` seconds.
    void think(const behavior_tree& tree, std::uint64_t tick, float dt) {
        detail::check(ponko_brain_think(raw_, tree.raw(), tick, dt));
    }
    /// Whether it should think at `tick`, for agent number `index` (its period, staggered; woken; new).
    bool due(std::uint32_t index, std::uint64_t tick) const {
        bool d = false;
        detail::check(ponko_brain_due(raw_, index, tick, &d));
        return d;
    }
    /// Thinks every `period` ticks.
    void set_period(std::uint32_t period) { detail::check(ponko_brain_set_period(raw_, period)); }
    /// The key id of its action, 0 for none; with several at once, the first.
    std::uint64_t action() const { std::uint64_t k = 0; detail::check(ponko_brain_action(raw_, &k)); return k; }
    /// The key id of its `index`th action in tree order, 0 past the last:
    /// a `parallel` node runs several at once (move and aim).
    std::uint64_t action_at(std::size_t index) const {
        std::uint64_t k = 0;
        detail::check(ponko_brain_action_at(raw_, index, &k));
        return k;
    }
    /// Whether it is doing `action_key` (any of its actions).
    bool doing(std::uint64_t action_key) const {
        for (std::size_t i = 0;; ++i) {
            const std::uint64_t k = action_at(i);
            if (k == 0) return false;
            if (k == action_key) return true;
        }
    }
    /// Whether running action `action_key` started at the last think;
    /// false if it isn't doing it.
    bool started_action(std::uint64_t action_key) const {
        bool s = false;
        const auto st = ponko_brain_action_started(raw_, action_key, &s);
        if (st == PONKO_NOT_FOUND) return false;
        detail::check(st);
        return s;
    }
    /// The number parameter `key` of running action `action_key`, if it has one.
    std::optional<float> action_param(std::uint64_t action_key, std::uint64_t key) const {
        float v = 0;
        const auto st = ponko_brain_action_param_num(raw_, action_key, key, &v);
        if (st == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(st);
        return v;
    }
    /// Running action `action_key` is done and worked; its other actions
    /// carry on. Nothing if it isn't doing it.
    void succeed_action(std::uint64_t action_key) {
        const auto st = ponko_brain_finish_action(raw_, action_key, PONKO_AI_SUCCESS);
        if (st != PONKO_NOT_FOUND) detail::check(st);
    }
    /// Running action `action_key` is done and failed; its other actions
    /// carry on. Nothing if it isn't doing it.
    void fail_action(std::uint64_t action_key) {
        const auto st = ponko_brain_finish_action(raw_, action_key, PONKO_AI_FAILURE);
        if (st != PONKO_NOT_FOUND) detail::check(st);
    }
    /// Whether its action started at the last think.
    bool started() const { bool s = false; detail::check(ponko_brain_started(raw_, &s)); return s; }
    /// The current action's number parameter `key`, if it has one.
    std::optional<float> param(std::uint64_t key) const {
        float v = 0;
        const auto st = ponko_brain_param_num(raw_, key, &v);
        if (st == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(st);
        return v;
    }
    /// The action is done and worked; the tree goes on at the next think.
    void succeed() { detail::check(ponko_brain_finish(raw_, PONKO_AI_SUCCESS)); }
    /// The action is done and failed; the tree goes on at the next think.
    void fail() { detail::check(ponko_brain_finish(raw_, PONKO_AI_FAILURE)); }
    /// What the game reported for the current action.
    ai::status status() const {
        PonkoAiStatus s = PONKO_AI_RUNNING;
        detail::check(ponko_brain_status(raw_, &s));
        return static_cast<ai::status>(s);
    }
    /// Thinks at the next chance, whatever the period.
    void wake() { detail::check(ponko_brain_wake(raw_)); }
    /// Hears signal `key` (from ai::key): a `wait_for` node waiting for it
    /// succeeds at the next think, which is due at once.
    void signal(std::uint64_t key) { detail::check(ponko_brain_signal(raw_, key)); }
    /// The seed its `random` and `chance` nodes draw from: give each agent its own.
    void set_seed(std::uint64_t seed) { detail::check(ponko_brain_set_seed(raw_, seed)); }
    /// The brain for a save: its memory (running nodes, waits, cooldowns) and blackboard.
    std::string save_json() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_brain_save_json(raw_, b, c, l); });
    }
    /// A brain read back from save_json(): it carries on where it was with
    /// the same tree. Throws ponko::error if `json` isn't a saved brain.
    static brain load_json(const std::string& json) {
        PonkoBrain* raw = ponko_brain_load_json(json.c_str());
        if (!raw) throw detail::last_error();
        return brain(adopt{}, raw);
    }
    /// Sets a blackboard entry. A key id must come from ai::key.
    ///@{
    void set_num(std::uint64_t key, float v) { detail::check(ponko_brain_set_num(raw_, key, v)); }
    void set_flag(std::uint64_t key, bool v) { detail::check(ponko_brain_set_flag(raw_, key, v)); }
    void set_vec2(std::uint64_t key, vec2 v) { detail::check(ponko_brain_set_vec2(raw_, key, v)); }
    void set_vec3(std::uint64_t key, vec3 v) { detail::check(ponko_brain_set_vec3(raw_, key, v)); }
    void set_entity(std::uint64_t key, entity e) { detail::check(ponko_brain_set_entity(raw_, key, e)); }
    ///@}
    /// A blackboard number, if it holds one.
    std::optional<float> num(std::uint64_t key) const {
        float v = 0;
        const auto st = ponko_brain_num(raw_, key, &v);
        if (st == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(st);
        return v;
    }
    /// A blackboard flag, if it holds one.
    std::optional<bool> flag(std::uint64_t key) const {
        bool v = false;
        const auto st = ponko_brain_flag(raw_, key, &v);
        if (st == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(st);
        return v;
    }
    /// A blackboard entity, if it holds one.
    std::optional<entity> get_entity(std::uint64_t key) const {
        entity v = 0;
        const auto st = ponko_brain_entity(raw_, key, &v);
        if (st == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(st);
        return v;
    }
    /// A blackboard 2D point, if it holds one.
    std::optional<vec2> get_vec2(std::uint64_t key) const {
        vec2 v{};
        const auto st = ponko_brain_vec2(raw_, key, &v);
        if (st == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(st);
        return v;
    }
    /// A blackboard 3D point, if it holds one.
    std::optional<vec3> get_vec3(std::uint64_t key) const {
        vec3 v{};
        const auto st = ponko_brain_vec3(raw_, key, &v);
        if (st == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(st);
        return v;
    }
    /// The blackboard as JSON.
    std::string blackboard_json() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_brain_blackboard_json(raw_, b, c, l); });
    }
    /// The tree, one node a line, marked with what each did at the last think.
    std::string trace() const {
        return detail::read_text([&](char* b, std::size_t c, std::size_t* l) { return ponko_brain_trace(raw_, b, c, l); });
    }
    /// The C brain.
    PonkoBrain* raw() noexcept { return raw_; }
};

/// The score of `x` (0..1) on a response curve; `param` is the power,
/// steepness or threshold of the last three curves.
inline float curve(PonkoAiCurve c, float x, float param = 0) { return ponko_ai_curve(c, param, x); }
/// Combines scores into one (the product, made up for the number of factors).
inline float combine(const std::vector<float>& scores) { return ponko_ai_combine(scores.data(), scores.size()); }

// World brains: Brain and Blackboard components on entities, run by
// ai::think(ctx) from a sim system, woken by signals (Rust `ai::think`,
// `Signal`). Senses, think, act: the same three steps as in Rust.

/// The `target` of a signal for every brain that waits for it.
inline constexpr entity every_brain = PONKO_AI_EVERY_BRAIN;
/// Loads a tree from a JSON file in `assets/` into the world, under its
/// path, with the files its subtrees name; hot reload swaps it while the
/// game runs. Throws ponko::error for a missing file or a bad tree.
inline void load_tree(ctx& c, const char* path) { detail::check(ponko_ctx_ai_load_tree(c.raw(), path)); }
/// Adds a compiled tree to the world under `name` (or replaces it).
inline void add_tree(ctx& c, const char* name, const behavior_tree& tree) {
    detail::check(ponko_ctx_ai_add_tree(c.raw(), name, tree.raw()));
}
/// Gives `e` a brain running the world's tree `tree`, thinking every
/// `period` ticks, and an empty blackboard if it has none.
inline void add_brain(ctx& c, entity e, const char* tree, std::uint32_t period = 1) {
    detail::check(ponko_ctx_ai_add_brain(c.raw(), e, tree, period));
}
/// Runs every due world brain (Rust `ai::think`): call it from a sim
/// system after the senses and signals, before the actions.
inline void think(ctx& c) { detail::check(ponko_ctx_ai_think(c.raw())); }
/// Sends signal `key` to the brain of `target`, or to every brain with
/// every_brain: a `wait_for` node waiting for it succeeds at once,
/// whatever the brain's period. Send it before think().
inline void signal(ctx& c, entity target, std::uint64_t key) {
    detail::check(ponko_ctx_ai_signal(c.raw(), target, key));
}
/// signal() carrying a number, which `"store": "key"` writes to the blackboard.
inline void signal_num(ctx& c, entity target, std::uint64_t key, float value) {
    detail::check(ponko_ctx_ai_signal_num(c.raw(), target, key, value));
}
/// signal() carrying an entity (who shouted).
inline void signal_entity(ctx& c, entity target, std::uint64_t key, entity value) {
    detail::check(ponko_ctx_ai_signal_entity(c.raw(), target, key, value));
}
/// signal() carrying a 2D point.
inline void signal_vec2(ctx& c, entity target, std::uint64_t key, vec2 value) {
    detail::check(ponko_ctx_ai_signal_vec2(c.raw(), target, key, value));
}
/// signal() carrying a 3D point.
inline void signal_vec3(ctx& c, entity target, std::uint64_t key, vec3 value) {
    detail::check(ponko_ctx_ai_signal_vec3(c.raw(), target, key, value));
}
/// The key id of the `index`th action of the brain of `e`, 0 past the last.
inline std::uint64_t action(ctx& c, entity e, std::size_t index = 0) {
    std::uint64_t k = 0;
    detail::check(ponko_ctx_ai_action(c.raw(), e, index, &k));
    return k;
}
/// Whether the brain of `e` is doing `action_key`.
inline bool doing(ctx& c, entity e, std::uint64_t action_key) {
    for (std::size_t i = 0;; ++i) {
        const std::uint64_t k = action(c, e, i);
        if (k == 0) return false;
        if (k == action_key) return true;
    }
}
/// Whether action `action_key` (0: the first) of the brain of `e` started
/// at this tick's think; false if it isn't doing it.
inline bool started(ctx& c, entity e, std::uint64_t action_key = 0) {
    bool s = false;
    const auto st = ponko_ctx_ai_started(c.raw(), e, action_key, &s);
    if (st == PONKO_NOT_FOUND) return false;
    detail::check(st);
    return s;
}
/// Reports action `action_key` (0: the first) of the brain of `e` done
/// (success or failure); its other actions carry on. Throws if it isn't
/// doing it.
inline void finish(ctx& c, entity e, std::uint64_t action_key, status result) {
    detail::check(ponko_ctx_ai_finish(c.raw(), e, action_key, static_cast<PonkoAiStatus>(result)));
}
/// The number parameter `key` of action `action_key` (0: the first) of
/// the brain of `e`, if it has one.
inline std::optional<float> param(ctx& c, entity e, std::uint64_t action_key, std::uint64_t key) {
    float v = 0;
    const auto st = ponko_ctx_ai_param_num(c.raw(), e, action_key, key, &v);
    if (st == PONKO_NOT_FOUND) return std::nullopt;
    detail::check(st);
    return v;
}
/// Blackboard writes for the brain of `e` (its senses).
///@{
inline void set_num(ctx& c, entity e, std::uint64_t key, float v) { detail::check(ponko_ctx_ai_set_num(c.raw(), e, key, v)); }
inline void set_flag(ctx& c, entity e, std::uint64_t key, bool v) { detail::check(ponko_ctx_ai_set_flag(c.raw(), e, key, v)); }
inline void set_vec2(ctx& c, entity e, std::uint64_t key, vec2 v) { detail::check(ponko_ctx_ai_set_vec2(c.raw(), e, key, v)); }
inline void set_vec3(ctx& c, entity e, std::uint64_t key, vec3 v) { detail::check(ponko_ctx_ai_set_vec3(c.raw(), e, key, v)); }
inline void set_entity(ctx& c, entity e, std::uint64_t key, entity v) {
    detail::check(ponko_ctx_ai_set_entity(c.raw(), e, key, v));
}
///@}
/// A number on the blackboard of `e`, if it holds one.
inline std::optional<float> num(ctx& c, entity e, std::uint64_t key) {
    float v = 0;
    const auto st = ponko_ctx_ai_num(c.raw(), e, key, &v);
    if (st == PONKO_NOT_FOUND) return std::nullopt;
    detail::check(st);
    return v;
}
/// A flag on the blackboard of `e`, if it holds one.
inline std::optional<bool> flag(ctx& c, entity e, std::uint64_t key) {
    bool v = false;
    const auto st = ponko_ctx_ai_flag(c.raw(), e, key, &v);
    if (st == PONKO_NOT_FOUND) return std::nullopt;
    detail::check(st);
    return v;
}
/// An entity on the blackboard of `e`, if it holds one.
inline std::optional<entity> get_entity(ctx& c, entity e, std::uint64_t key) {
    entity v = 0;
    const auto st = ponko_ctx_ai_entity(c.raw(), e, key, &v);
    if (st == PONKO_NOT_FOUND) return std::nullopt;
    detail::check(st);
    return v;
}
/// A 2D point on the blackboard of `e`, if it holds one.
inline std::optional<vec2> get_vec2(ctx& c, entity e, std::uint64_t key) {
    vec2 v{};
    const auto st = ponko_ctx_ai_vec2(c.raw(), e, key, &v);
    if (st == PONKO_NOT_FOUND) return std::nullopt;
    detail::check(st);
    return v;
}
/// A 3D point on the blackboard of `e`, if it holds one.
inline std::optional<vec3> get_vec3(ctx& c, entity e, std::uint64_t key) {
    vec3 v{};
    const auto st = ponko_ctx_ai_vec3(c.raw(), e, key, &v);
    if (st == PONKO_NOT_FOUND) return std::nullopt;
    detail::check(st);
    return v;
}

// World brains inside loops: world_view::each and each_par take ai::brain_row
// and ai::blackboard_row parameters (by value) beside the components, so a
// system reads and writes them where it visits the entity (C
// `ponko_chunk_ai_brain`, Rust `Query<(&mut Brain, &mut Blackboard, ...)>`):
//
//     c.world().each([&](ponko::entity e, ai::brain_row brain, ai::blackboard_row board, const position& p) {
//         board.set_vec2(at, p.value);
//         if (brain.doing(chase)) ...
//     });
//
// ai::query hands over whole chunks with their rows; ai::each is the older
// spelling of the typed each.

/// The world brain of one query row (C `PonkoAiBrain`): what to do. Valid
/// during the query callback.
class brain_row {
    PonkoAiBrain* raw_;
public:
    /// Wraps a C row; ai::query makes these for you.
    explicit brain_row(PonkoAiBrain* raw) noexcept : raw_(raw) {}
    /// The brain in row `row` of a chunk's brain column `column`; throws std::out_of_range past the end.
    static brain_row at(const PonkoChunk* chunk, std::size_t column, std::size_t row) {
        PonkoAiBrain* b = ponko_chunk_ai_brain(chunk, column, row);
        if (!b) throw std::out_of_range("brain row");
        return brain_row(b);
    }
    /// The key id of its `index`th action in tree order, 0 past the last.
    std::uint64_t action(std::size_t index = 0) const {
        std::uint64_t k = 0;
        detail::check(ponko_ai_brain_action(raw_, index, &k));
        return k;
    }
    /// Whether it is doing `action_key`.
    bool doing(std::uint64_t action_key) const {
        for (std::size_t i = 0;; ++i) {
            const std::uint64_t k = action(i);
            if (k == 0) return false;
            if (k == action_key) return true;
        }
    }
    /// Whether action `action_key` (0: the first) started at this tick's
    /// think; false if it isn't doing it.
    bool started(std::uint64_t action_key = 0) const {
        bool s = false;
        const auto st = ponko_ai_brain_started(raw_, action_key, &s);
        if (st == PONKO_NOT_FOUND) return false;
        detail::check(st);
        return s;
    }
    /// The number parameter `key` of action `action_key` (0: the first), if it has one.
    std::optional<float> param(std::uint64_t action_key, std::uint64_t key) const {
        float v = 0;
        const auto st = ponko_ai_brain_param_num(raw_, action_key, key, &v);
        if (st == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(st);
        return v;
    }
    /// Reports action `action_key` (0: the first) done (success or
    /// failure); its other actions carry on. Throws if it isn't doing it.
    void finish(std::uint64_t action_key, status result) {
        detail::check(ponko_ai_brain_finish(raw_, action_key, static_cast<PonkoAiStatus>(result)));
    }
    /// Thinks at the next ai::think, whatever its period: something it should react to happened (a
    /// noise, a hit). Fine from ai::query_par and each_par, each row on its chunk's thread.
    void wake() { detail::check(ponko_ai_brain_wake(raw_)); }
    /// Hears signal `key` (an ai::key) now: a `wait_for` node waiting for it succeeds at its next think,
    /// which comes at the next ai::think. Unlike ai::signal it reaches this brain at once, without an
    /// event, so it works from the parallel loops too.
    void signal(std::uint64_t key) { detail::check(ponko_ai_brain_signal(raw_, key)); }
    /// signal() carrying a number, which a `wait_for` node with `"store": "key"` writes to the blackboard.
    void signal_num(std::uint64_t key, float value) { detail::check(ponko_ai_brain_signal_num(raw_, key, value)); }
    /// signal() carrying an entity (who shouted, who shot).
    void signal_entity(std::uint64_t key, entity value) { detail::check(ponko_ai_brain_signal_entity(raw_, key, value)); }
    /// signal() carrying a 2D point (where the noise was).
    void signal_vec2(std::uint64_t key, vec2 value) { detail::check(ponko_ai_brain_signal_vec2(raw_, key, value)); }
    /// signal() carrying a 3D point.
    void signal_vec3(std::uint64_t key, vec3 value) { detail::check(ponko_ai_brain_signal_vec3(raw_, key, value)); }
    /// The C row.
    PonkoAiBrain* raw() const noexcept { return raw_; }
};

/// The blackboard of one query row (C `PonkoAiBlackboard`): what the brain
/// knows. Valid during the query callback.
class blackboard_row {
    PonkoAiBlackboard* raw_;
    template<class T, class Get> std::optional<T> read(std::uint64_t key, Get get) const {
        T v{};
        const auto st = get(raw_, key, &v);
        if (st == PONKO_NOT_FOUND) return std::nullopt;
        detail::check(st);
        return v;
    }
public:
    /// Wraps a C row; ai::query makes these for you.
    explicit blackboard_row(PonkoAiBlackboard* raw) noexcept : raw_(raw) {}
    /// The blackboard in row `row` of a chunk's blackboard column `column`; throws std::out_of_range past the
    /// end.
    static blackboard_row at(const PonkoChunk* chunk, std::size_t column, std::size_t row) {
        PonkoAiBlackboard* b = ponko_chunk_ai_blackboard(chunk, column, row);
        if (!b) throw std::out_of_range("blackboard row");
        return blackboard_row(b);
    }
    /// Sets an entry. A key id must come from ai::key.
    ///@{
    void set_num(std::uint64_t key, float v) { detail::check(ponko_ai_blackboard_set_num(raw_, key, v)); }
    void set_flag(std::uint64_t key, bool v) { detail::check(ponko_ai_blackboard_set_flag(raw_, key, v)); }
    void set_vec2(std::uint64_t key, vec2 v) { detail::check(ponko_ai_blackboard_set_vec2(raw_, key, v)); }
    void set_vec3(std::uint64_t key, vec3 v) { detail::check(ponko_ai_blackboard_set_vec3(raw_, key, v)); }
    void set_entity(std::uint64_t key, entity v) { detail::check(ponko_ai_blackboard_set_entity(raw_, key, v)); }
    ///@}
    /// An entry, if it holds a value of that kind.
    ///@{
    std::optional<float> num(std::uint64_t key) const { return read<float>(key, ponko_ai_blackboard_num); }
    std::optional<bool> flag(std::uint64_t key) const { return read<bool>(key, ponko_ai_blackboard_flag); }
    std::optional<entity> get_entity(std::uint64_t key) const { return read<entity>(key, ponko_ai_blackboard_entity); }
    std::optional<vec2> get_vec2(std::uint64_t key) const { return read<vec2>(key, ponko_ai_blackboard_vec2); }
    std::optional<vec3> get_vec3(std::uint64_t key) const { return read<vec3>(key, ponko_ai_blackboard_vec3); }
    ///@}
    /// The C row.
    PonkoAiBlackboard* raw() const noexcept { return raw_; }
};

/// The brains and blackboards of one ai::query chunk, row by row.
class rows {
    const PonkoChunk* raw_;
    std::size_t column_;
public:
    /// The chunk's rows, whose brain column is `column` (the blackboard's the next).
    rows(const PonkoChunk* raw, std::size_t column) noexcept : raw_(raw), column_(column) {}
    /// The brain of row `i`.
    brain_row brain(std::size_t i) const { return brain_row::at(raw_, column_, i); }
    /// The blackboard of row `i`.
    blackboard_row blackboard(std::size_t i) const { return blackboard_row::at(raw_, column_ + 1, i); }
};

} // namespace ai
namespace detail {
/// ai::query and ai::query_par: a query of `Ts` with the brain and blackboard columns last.
template<class... Ts, class F> void brain_query(ctx& c, bool parallel, F& fn) {
    static_assert(sizeof...(Ts) <= 30, "ai::query takes at most 30 components besides the brain and blackboard");
    world_view w = c.world();
    PonkoWorld* world = ponko_ctx_world(c.raw());
    std::array<std::uint32_t, sizeof...(Ts) + 2> ids{{w.component_id<bare<Ts>>()..., 0, 0}};
    check(ponko_ai_brain_component(world, &ids[sizeof...(Ts)]));
    check(ponko_ai_blackboard_component(world, &ids[sizeof...(Ts) + 1]));
    PonkoQueryOptions options = ponko_query_options_default();
    options.writes = 0;
    options.parallel = parallel;
    std::size_t i = 0;
    ((options.writes |= std::uint32_t(!std::is_const<Ts>::value) << i++), ...);
    options.writes |= std::uint32_t(3) << sizeof...(Ts);
    struct context { F* fn; first_error error; } cx{&fn, {}};
    auto trampoline = [](const PonkoChunk* raw, void* user) noexcept {
        auto& cx = *static_cast<context*>(user);
        if (cx.error.set.load()) return;
        try { (*cx.fn)(chunk<Ts...>(raw), ai::rows(raw, sizeof...(Ts))); } catch (...) { cx.error.capture(); }
    };
    const PonkoStatus status = ponko_query_with(world, ids.data(), ids.size(), &options, trampoline, &cx);
    cx.error.rethrow();
    check(status);
}
}  // namespace detail
namespace ai {

/// Calls `fn(chunk<Ts...>, ai::rows)` for every archetype of entities with
/// a world brain and all of `Ts`: the columns of `Ts` as in
/// world_view::query, and each row's brain and blackboard.
///
///     ai::query<const position>(c, [&](auto chunk, ai::rows brains) {
///         auto [pos] = chunk.columns();
///         for (std::size_t i = 0; i < chunk.size(); ++i)
///             brains.blackboard(i).set_vec2(at, pos[i].value);
///     });
template<class... Ts, class F> void query(ctx& c, F&& fn) { detail::brain_query<Ts...>(c, false, fn); }
/// ai::query on all CPU cores at once: `fn` must be thread-safe, and
/// touch only its own chunk's rows.
template<class... Ts, class F> void query_par(ctx& c, F&& fn) { detail::brain_query<Ts...>(c, true, fn); }
/// Calls `fn(entity, ai::brain_row, ai::blackboard_row, Ts&...)` for every
/// entity with a world brain and all of `Ts`. It is world_view::each with the
/// rows as parameters, the one typed loop over brains: write that directly
/// (`c.world().each([&](entity e, ai::brain_row b, ai::blackboard_row bb, T& t) {...})`),
/// which also takes the rows in any order, filters and each_par.
template<class... Ts, class F> void each(ctx& c, F&& fn) {
    c.world().each([&fn](entity e, brain_row brain, blackboard_row board, Ts&... ts) { fn(e, brain, board, ts...); });
}
} // namespace ai
} // namespace ponko

#define PONKO_PP_CAT2(a, b) a##b
#define PONKO_PP_CAT(a, b) PONKO_PP_CAT2(a, b)
/// Times the rest of the block as a scope called `name` (a string literal).
/// It shows up next to the systems in the overlay, in PONKO_BENCH reports and
/// in ctx::perf_report(). Scopes nest, work on any thread (times add up), and
/// cost one atomic load while profiling is off.
///
///     void ai(ponko::ctx& ctx) {
///         PONKO_PROFILE_SCOPE("ai");
///         { PONKO_PROFILE_SCOPE("ai/pathing"); /* ... */ }
///     }
#define PONKO_PROFILE_SCOPE(name) \
    static const PonkoProfileScope* const PONKO_PP_CAT(ponko_scope_, __LINE__) = ponko_profile_scope(name); \
    ::ponko::profile_scope_guard PONKO_PP_CAT(ponko_scope_guard_, __LINE__)(PONKO_PP_CAT(ponko_scope_, __LINE__))

namespace ponko::detail {
// PONKO_COUNT_CPP_ALLOCATIONS's operator new and delete: each block keeps its size in front of it (one
// alignment of operator new, so what new returns stays aligned), so delete can report the free too
// and net_alloc_bytes sees what the program gives back. That alignment is the larger of max_align_t's
// and __STDCPP_DEFAULT_NEW_ALIGNMENT__: on Apple silicon max_align_t is 8 bytes while new promises 16.
inline constexpr std::size_t counted_header =
#ifdef __STDCPP_DEFAULT_NEW_ALIGNMENT__
    alignof(std::max_align_t) > __STDCPP_DEFAULT_NEW_ALIGNMENT__ ? alignof(std::max_align_t) : __STDCPP_DEFAULT_NEW_ALIGNMENT__;
#else
    alignof(std::max_align_t);
#endif
inline void* counted_new(std::size_t n) {
    if (n > std::numeric_limits<std::size_t>::max() - counted_header) throw std::bad_alloc();
    void* block = std::malloc(n + counted_header);
    if (!block) throw std::bad_alloc();
    std::memcpy(block, &n, sizeof n);
    ponko_note_allocation(n);
    return static_cast<unsigned char*>(block) + counted_header;
}
inline void counted_delete(void* p) noexcept {
    if (!p) return;
    unsigned char* block = static_cast<unsigned char*>(p) - counted_header;
    std::size_t n = 0;
    std::memcpy(&n, block, sizeof n);
    ponko_note_deallocation(n);
    std::free(block);
}
} // namespace ponko::detail

/// Put this once, at global scope in one .cpp file, to count C++ heap
/// allocations (operator new) per frame and per system in the reports, and
/// their frees in net_alloc_bytes. It replaces the global operator new and
/// delete (each block keeps its size in a small header); counting happens
/// only while profiling is on, so the cost otherwise is one atomic load per
/// `new` and per `delete`.
#define PONKO_COUNT_CPP_ALLOCATIONS \
    void* operator new(std::size_t n) { return ::ponko::detail::counted_new(n); } \
    void* operator new[](std::size_t n) { return ::ponko::detail::counted_new(n); } \
    void operator delete(void* p) noexcept { ::ponko::detail::counted_delete(p); } \
    void operator delete[](void* p) noexcept { ::ponko::detail::counted_delete(p); } \
    void operator delete(void* p, std::size_t) noexcept { ::ponko::detail::counted_delete(p); } \
    void operator delete[](void* p, std::size_t) noexcept { ::ponko::detail::counted_delete(p); }
#endif
