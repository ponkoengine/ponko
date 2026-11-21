#include "ponko.h"
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define CHECK(x) do { if (!(x)) { fprintf(stderr, "failed: %s\n", #x); abort(); } } while (0)
typedef struct { int x; } Position;
typedef struct { PonkoWorld* world; size_t seen; uint64_t entity; } Context;
static void visit(const PonkoChunk* chunk, void* user) {
    Context* c = user;
    CHECK(chunk->column_count == 1);
    for (size_t i = 0; i < chunk->count; ++i) {
        CHECK(ponko_entity_bits(chunk->entities[i]) == c->entity);
        ((Position*)chunk->columns[0])[i].x += 2;
        ++c->seen;
    }
    size_t len;
    CHECK(ponko_world_len(c->world, &len) == PONKO_BUSY);
    CHECK(ponko_world_free(c->world) == PONKO_BUSY);
}
typedef struct { size_t ticks; float dt; PonkoStatus busy_quit; } Run;
static void setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoGpuBuffer buffer;
    const float items[2][4] = {{1, 2, 3, 4}, {0, 0, 0, 0}};
    CHECK(ponko_ctx_create_gpu_buffer(ctx, 2, items, 2, &buffer) == PONKO_OK);
    CHECK(ponko_ctx_write_gpu_buffer(ctx, buffer, 1, items, 1) == PONKO_OK);
    CHECK(ponko_ctx_write_gpu_buffer(ctx, buffer, 2, items, 1) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_set_gpu_buffer_motion(ctx, buffer, true) == PONKO_OK);
    CHECK(ponko_ctx_create_gpu_buffer(ctx, 0, NULL, 0, &buffer) == PONKO_INVALID_ARGUMENT);
    PonkoMaterial batch_material;
    PonkoMaterialDesc batch_desc = ponko_material_desc((PonkoColor){1,1,1,1});
    CHECK(ponko_ctx_create_material(ctx, &batch_desc, &batch_material) == PONKO_OK);
    PonkoMeshData* batch_data = ponko_mesh_data_cube(1.0f);
    PonkoMesh batch_mesh;
    CHECK(ponko_ctx_create_mesh(ctx, batch_data, batch_material, &batch_mesh) == PONKO_OK);
    ponko_mesh_data_free(batch_data);
    const float matrices[1][16] = {{1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1}};
    PonkoInstanceBatch batch;
    CHECK(ponko_ctx_create_instance_batch(ctx,batch_mesh,matrices,1,(PonkoInstanceBatchOptions){32,100,true,{1,1,1,1}},&batch) == PONKO_OK);
    CHECK(ponko_ctx_update_instance_batch(ctx,batch,matrices,1) == PONKO_OK);
    for (uint32_t mode=0; mode<3; ++mode) CHECK(ponko_ctx_set_instance_batch_culling(ctx,batch,mode) == PONKO_OK);
    CHECK(ponko_ctx_set_instance_batch_culling(ctx,batch,3) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_free_instance_batch(ctx,batch) == PONKO_OK);
    CHECK(ponko_ctx_set_instance_batch_culling(ctx,batch,0) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_update_instance_batch(ctx,batch,matrices,1) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_free_gpu_buffer(ctx, buffer) == PONKO_OK);
    CHECK(ponko_ctx_free_gpu_buffer(ctx, buffer) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_write_gpu_buffer(ctx, buffer, 0, items, 1) == PONKO_INVALID_ARGUMENT);
    /* One lifecycle for every handle kind: a kind and an id. */
    bool live = true;
    CHECK(ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_GPU_BUFFER, buffer.id, &live) == PONKO_OK && !live);
    CHECK(ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_MESH, batch_mesh.id, &live) == PONKO_OK && live);
    PonkoLiveCounts counts;
    CHECK(ponko_ctx_live_counts(ctx, &counts) == PONKO_OK && counts.gpu_buffers == 0);
    size_t meshes = counts.meshes;
    CHECK(ponko_ctx_handle_free(ctx, PONKO_HANDLE_MESH, batch_mesh.id) == PONKO_OK);
    CHECK(ponko_ctx_handle_free(ctx, PONKO_HANDLE_MESH, batch_mesh.id) == PONKO_OK);
    CHECK(ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_MESH, batch_mesh.id, &live) == PONKO_OK && !live);
    CHECK(ponko_ctx_live_counts(ctx, &counts) == PONKO_OK && counts.meshes == meshes - 1);
    CHECK(ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_TEXTURE, PONKO_HANDLE_NONE, &live) == PONKO_OK && !live);
    CHECK(ponko_ctx_handle_free(ctx, PONKO_HANDLE_MATERIAL, PONKO_HANDLE_NONE) == PONKO_OK);
    CHECK(ponko_ctx_handle_free(ctx, 99, 0) == PONKO_INVALID_ARGUMENT);
    /* Asset accounting: counts and bytes by class, each handle's age and owner. */
    PonkoAssetCount textures_before, textures_after;
    CHECK(ponko_ctx_asset_count(ctx, PONKO_ASSET_CLASS_TEXTURE, &textures_before) == PONKO_OK);
    const uint8_t pixels[2 * 2 * 4] = {0};
    PonkoTexture made;
    CHECK(ponko_ctx_create_texture(ctx, 2, 2, pixels, sizeof pixels, &made) == PONKO_OK);
    CHECK(ponko_ctx_asset_count(ctx, PONKO_ASSET_CLASS_TEXTURE, &textures_after) == PONKO_OK);
    CHECK(textures_after.live == textures_before.live + 1 && textures_after.bytes == textures_before.bytes + 16);
    size_t record_count = 0;
    CHECK(ponko_ctx_asset_handles(ctx, PONKO_ASSET_CLASS_TEXTURE, NULL, 0, &record_count) == PONKO_OK);
    CHECK(record_count == textures_after.live);
    PonkoAssetRecord records[64];
    CHECK(record_count <= 64);
    CHECK(ponko_ctx_asset_handles(ctx, PONKO_ASSET_CLASS_TEXTURE, records, 64, &record_count) == PONKO_OK);
    bool found_made = false;
    for (size_t i = 0; i < record_count; i++) {
        if (records[i].id == made.id) {
            found_made = records[i].owner == PONKO_ASSET_OWNER_CODE && records[i].bytes == 16;
        }
    }
    CHECK(found_made);
    size_t owner_len = 99;
    CHECK(ponko_ctx_asset_owner(ctx, PONKO_ASSET_CLASS_TEXTURE, made.id, NULL, 0, &owner_len) == PONKO_OK && owner_len == 0);
    CHECK(ponko_ctx_asset_count(ctx, 99, &textures_after) == PONKO_INVALID_ARGUMENT);
    /* C keeps plain ids and checks the kind at run time: a texture is not a render target. */
    PonkoTexture target;
    CHECK(ponko_ctx_create_render_target(ctx, 4, 4, &target) == PONKO_OK);
    CHECK(ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_RENDER_TARGET, target.id, &live) == PONKO_OK && live);
    CHECK(ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_RENDER_TARGET, made.id, &live) == PONKO_OK && !live);
    CHECK(ponko_ctx_handle_free(ctx, PONKO_HANDLE_RENDER_TARGET, made.id) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_handle_free(ctx, PONKO_HANDLE_RENDER_TARGET, target.id) == PONKO_OK);
    CHECK(ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_TEXTURE, target.id, &live) == PONKO_OK && !live);
    CHECK(ponko_ctx_handle_free(ctx, PONKO_HANDLE_TEXTURE, made.id) == PONKO_OK);
    PonkoMesh house;
    CHECK(ponko_ctx_load_mesh(ctx, "models/house.glb", &house) == PONKO_OK);
    char owner[64];
    CHECK(ponko_ctx_asset_owner(ctx, PONKO_ASSET_CLASS_MESH, house.id, owner, sizeof owner, &owner_len) == PONKO_OK);
    CHECK(owner_len == strlen("models/house.glb") && strcmp(owner, "models/house.glb") == 0);
    bool unloaded = false;
    CHECK(ponko_ctx_unload_mesh(ctx, "models/house.glb", &unloaded) == PONKO_OK && unloaded);
    CHECK(ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_MESH, house.id, &live) == PONKO_OK && !live);
    CHECK(ponko_ctx_unload_mesh(ctx, "models/house.glb", &unloaded) == PONKO_OK && !unloaded);
    bool culling = true;
    CHECK(ponko_ctx_occlusion_culling(ctx, &culling) == PONKO_OK && !culling);
    CHECK(ponko_ctx_set_occlusion_culling(ctx, true) == PONKO_OK);
    CHECK(ponko_ctx_occlusion_culling(ctx, &culling) == PONKO_OK && culling);
    CHECK(ponko_ctx_set_occlusion_culling(ctx, false) == PONKO_OK);
    CHECK(ponko_ctx_occlusion_culling(NULL, &culling) == PONKO_INVALID_ARGUMENT);
    PonkoLighting light;
    CHECK(ponko_ctx_lighting(ctx, &light) == PONKO_OK && !light.has_equator);
    light.has_equator = true;
    light.equator_color = (PonkoColor){0, 1, 0, 1};
    CHECK(ponko_ctx_set_lighting(ctx, &light) == PONKO_OK);
    PonkoLighting lit;
    CHECK(ponko_ctx_lighting(ctx, &lit) == PONKO_OK && lit.has_equator && lit.equator_color.g == 1);
    light.has_equator = false;
    CHECK(ponko_ctx_set_lighting(ctx, &light) == PONKO_OK);
    /* Shader status: what compiled, and where an error is in the game's WGSL. */
    size_t shaders_before = 0;
    CHECK(ponko_ctx_shader_status_count(ctx, &shaders_before) == PONKO_OK);
    PonkoShader broken_shader;
    CHECK(ponko_ctx_create_shader(ctx, "broken.wgsl", "// tint\nfn surface(s: Surface) -> Surface {\n    return 1.0;\n}", &broken_shader) == PONKO_INVALID_ARGUMENT);
    size_t shaders = 0;
    CHECK(ponko_ctx_shader_status_count(ctx, &shaders) == PONKO_OK && shaders == shaders_before + 1);
    PonkoShaderStatus shader_status;
    CHECK(ponko_ctx_shader_status(ctx, shaders - 1, &shader_status) == PONKO_OK);
    CHECK(shader_status.kind == PONKO_SHADER_MATERIAL && shader_status.outcome == PONKO_SHADER_FAILED && !shader_status.ok);
    CHECK(shader_status.line == 3 && shader_status.column == 12 && !shader_status.in_engine_code);
    char shader_text[1024];
    size_t shader_len = 0;
    CHECK(ponko_ctx_shader_status_name(ctx, shaders - 1, shader_text, sizeof shader_text, &shader_len) == PONKO_OK);
    CHECK(strcmp(shader_text, "broken.wgsl") == 0 && shader_len == 11);
    CHECK(ponko_ctx_shader_status_text(ctx, shaders - 1, shader_text, sizeof shader_text, &shader_len) == PONKO_OK);
    CHECK(strncmp(shader_text, "broken.wgsl:3:12: error: ", 25) == 0);
    CHECK(ponko_ctx_shader_status_json(ctx, NULL, 0, &shader_len) == PONKO_OK && shader_len > 0);
    /* A schema described from C is what the capabilities answer reports. */
    PonkoWorld* described_world = ponko_ctx_world(ctx);
    uint32_t described = 0;
    CHECK(ponko_component_register(described_world, "DescribedInC", sizeof(float), _Alignof(float), &described) == PONKO_OK);
    PonkoField described_hp = {"hp", PONKO_FIELD_F32, 0};
    CHECK(ponko_component_reflect(described_world, described, &described_hp, 1) == PONKO_OK);
    CHECK(ponko_world_describe_component(described_world, "DescribedInC",
        "{\"Object\":[[\"hp\",\"Number\"],[\"team\",{\"Enum\":[\"MadeInC\"]}]]}") == PONKO_OK);
    size_t caps_len = 0;
    CHECK(ponko_ctx_capabilities_json(ctx, NULL, 0, &caps_len) == PONKO_OK && caps_len > 0);
    char* caps = malloc(caps_len + 1);
    CHECK(caps != NULL);
    CHECK(ponko_ctx_capabilities_json(ctx, caps, caps_len + 1, &caps_len) == PONKO_OK);
    CHECK(strstr(caps, "\"DescribedInC\"") != NULL && strstr(caps, "\"MadeInC\"") != NULL);
    free(caps);
    CHECK(ponko_ctx_capabilities_json(NULL, NULL, 0, &caps_len) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_shader_status(ctx, shaders, &shader_status) == PONKO_INVALID_ARGUMENT);
    /* Global illumination: off by default, its settings checked when on. */
    CHECK(!light.gi_enabled && light.gi_spacing == 2 && light.gi_probes_per_frame == 4);
    light.gi_enabled = true;
    light.gi_probes_per_frame = 0;
    CHECK(ponko_ctx_set_lighting(ctx, &light) == PONKO_INVALID_ARGUMENT);
    light.gi_probes_per_frame = 8;
    CHECK(ponko_ctx_set_lighting(ctx, &light) == PONKO_OK);
    CHECK(ponko_ctx_lighting(ctx, &lit) == PONKO_OK && lit.gi_enabled && lit.gi_probes_per_frame == 8);
    light.gi_enabled = false;
    CHECK(ponko_ctx_set_lighting(ctx, &light) == PONKO_OK);
    PonkoPost post;
    CHECK(ponko_ctx_post(ctx, &post) == PONKO_OK);
    CHECK(!post.ao_enabled && post.anti_aliasing == 0 && post.ao_radius == 1);
    post.ao_enabled = true; post.ao_half_resolution = true; post.ao_temporal = true; post.ao_radius = 2; post.ao_intensity = 1.5f;
    post.ao_slices = 4; post.ao_steps = 8; post.anti_aliasing = 1;
    CHECK(ponko_ctx_set_post(ctx, &post) == PONKO_OK);
    PonkoPost back;
    CHECK(ponko_ctx_post(ctx, &back) == PONKO_OK);
    CHECK(back.ao_half_resolution && back.ao_temporal && back.ao_enabled && back.ao_radius == 2 && back.anti_aliasing == 1);
    post.anti_aliasing = 4;
    CHECK(ponko_ctx_set_post(ctx, &post) == PONKO_INVALID_ARGUMENT);
    CHECK(back.render_scale == 1 && back.sharpen == 0);
    post.anti_aliasing = 1;
    post.render_scale = 0.1f;
    CHECK(ponko_ctx_set_post(ctx, &post) == PONKO_INVALID_ARGUMENT);
    post.render_scale = 0.5f; post.sharpen = 0.3f;
    CHECK(ponko_ctx_set_post(ctx, &post) == PONKO_OK);
    CHECK(ponko_ctx_post(ctx, &back) == PONKO_OK && back.render_scale == 0.5f && back.sharpen == 0.3f);
    PonkoGraphicsQuality quality, now;
    uint32_t level = 99;
    CHECK(ponko_graphics_quality_preset(4, &quality) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_graphics_quality_preset(0, &quality) == PONKO_OK);
    CHECK(quality.shadow_cascades == 2 && quality.light_shadow_views == 0 && quality.render_scale < 1);
    CHECK(quality.light_shadow_atlas == 1024);
    CHECK(ponko_graphics_quality_level(&quality, &level) == PONKO_OK && level == 0);
    CHECK(ponko_ctx_set_graphics_quality(ctx, &quality) == PONKO_OK);
    CHECK(ponko_ctx_graphics_quality(ctx, &now) == PONKO_OK && now.shadow_resolution == 1024);
    CHECK(ponko_ctx_post(ctx, &back) == PONKO_OK && back.render_scale == quality.render_scale);
    quality.shadow_resolution = 100;
    CHECK(ponko_ctx_set_graphics_quality(ctx, &quality) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_graphics_quality_preset(3, &quality) == PONKO_OK);
    CHECK(ponko_ctx_set_graphics_quality(ctx, &quality) == PONKO_OK);
    CHECK(ponko_ctx_graphics_quality(ctx, &now) == PONKO_OK && now.shadow_resolution == 4096 && now.ao_steps == 8 && now.shadow_refresh == 1 && now.shadow_filter == 4 && now.ssr == 1 && now.gi);
    CHECK(now.light_shadow_atlas == 8192 && now.light_shadow_views == 24);
    quality.light_shadow_atlas = 3000; /* not a power of two */
    CHECK(ponko_ctx_set_graphics_quality(ctx, &quality) == PONKO_INVALID_ARGUMENT);
    /* GPU tiers: reports and presets need no GPU; a headless ctx has none. */
    {
        size_t len = 0;
        bool on = true;
        uint32_t tier = 99;
        char small[8];
        PonkoGraphicsQuality web;
        CHECK(ponko_ctx_gpu_capabilities(ctx, NULL, 0, &len) == PONKO_NOT_FOUND);
        CHECK(ponko_ctx_gpu_tier(ctx, &tier) == PONKO_NOT_FOUND && tier == 99);
        CHECK(ponko_ctx_gpu_feature(ctx, PONKO_GPU_FEATURE_DECALS, &on) == PONKO_NOT_FOUND);
        CHECK(ponko_ctx_recommended_graphics_quality(ctx, &web) == PONKO_NOT_FOUND);
        CHECK(ponko_gpu_tier_report(PONKO_GPU_TIER_BASELINE, NULL, 0, &len) == PONKO_OK && len > 100);
        CHECK(ponko_gpu_tier_report(PONKO_GPU_TIER_BASELINE, small, sizeof small, &len) == PONKO_OK && small[0] == 0);
        CHECK(ponko_gpu_tier_report(3, NULL, 0, &len) == PONKO_INVALID_ARGUMENT);
        CHECK(ponko_gpu_tier_feature(PONKO_GPU_TIER_WEBGL2, PONKO_GPU_FEATURE_TEMPORAL_ANTIALIASING, &on) == PONKO_OK && !on);
        CHECK(ponko_gpu_tier_feature(PONKO_GPU_TIER_BASELINE, PONKO_GPU_FEATURE_TEMPORAL_ANTIALIASING, &on) == PONKO_OK && on);
        CHECK(ponko_gpu_tier_feature(PONKO_GPU_TIER_BASELINE, PONKO_GPU_FEATURE_DECALS, &on) == PONKO_OK && !on);
        CHECK(ponko_gpu_tier_feature(PONKO_GPU_TIER_DESKTOP, PONKO_GPU_FEATURE_GLOBAL_ILLUMINATION, &on) == PONKO_OK && on);
        CHECK(ponko_gpu_tier_feature(PONKO_GPU_TIER_DESKTOP, 13, &on) == PONKO_INVALID_ARGUMENT);
        CHECK(ponko_graphics_quality_for_tier(PONKO_GPU_TIER_WEBGL2, &web) == PONKO_OK);
        CHECK(web.anti_aliasing == 1 && !web.ambient_occlusion && web.ssr == 0 && !web.gi && web.shadow_cascades == 2);
        CHECK(ponko_graphics_quality_for_tier(PONKO_GPU_TIER_DESKTOP, &web) == PONKO_OK);
        CHECK(ponko_graphics_quality_level(&web, &level) == PONKO_OK && level == 2);
        CHECK(ponko_graphics_quality_for_tier(3, &web) == PONKO_INVALID_ARGUMENT);
    }
    post.anti_aliasing = 3; /* temporal */
    post.taa_sharpness = 2;
    CHECK(ponko_ctx_set_post(ctx, &post) == PONKO_INVALID_ARGUMENT);
    post.taa_sharpness = 0.5f;
    post.contact_shadows = -1;
    CHECK(ponko_ctx_set_post(ctx, &post) == PONKO_INVALID_ARGUMENT);
    post.contact_shadows = 0.3f;
    CHECK(ponko_ctx_set_post(ctx, &post) == PONKO_OK);
    CHECK(post.ssr == 0 && post.ssr_max_distance == 30); /* screen-space reflections: off */
    post.ssr = 2;
    CHECK(ponko_ctx_set_post(ctx, &post) == PONKO_INVALID_ARGUMENT);
    post.ssr = 0.5f;
    post.ssr_max_distance = 0;
    CHECK(ponko_ctx_set_post(ctx, &post) == PONKO_INVALID_ARGUMENT);
    post.ssr_max_distance = 20;
    CHECK(ponko_ctx_set_post(ctx, &post) == PONKO_OK);
    post.ao_enabled = true; post.ao_temporal = true;
    CHECK(ponko_ctx_set_post(ctx, &post) == PONKO_OK);
    CHECK(ponko_ctx_reset_temporal_history(ctx) == PONKO_OK);
    CHECK(ponko_ctx_post(ctx, &back) == PONKO_OK && back.anti_aliasing == 3 && back.taa_sharpness == 0.5f && back.contact_shadows == 0.3f && back.ssr == 0.5f && back.ssr_max_distance == 20);
    uint64_t e;
    CHECK(ponko_spawn(ponko_ctx_world(ctx), NULL, 0, &e) == PONKO_OK);
    CHECK(ponko_ctx_reset_motion(ctx, e) == PONKO_OK);
    CHECK(ponko_world_free(ponko_ctx_world(ctx)) == PONKO_BUSY);
    PonkoText3D t = ponko_text3d_default(), got;
    t.billboard = true; t.align = 2;
    CHECK(ponko_ctx_set_text3d(ctx, e, "Hello\nworld", &t) == PONKO_OK);
    CHECK(ponko_ctx_text3d(ctx, e, &got) == PONKO_OK && got.billboard && got.align == 2);
    char text[32]; size_t len;
    CHECK(ponko_ctx_text3d_text(ctx, e, text, sizeof(text), &len) == PONKO_OK);
    CHECK(len == 11 && strcmp(text, "Hello\nworld") == 0);
    t.font.id = UINT32_MAX;
    CHECK(ponko_ctx_set_text3d(ctx, e, "bad", &t) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_remove_text3d(ctx, e) == PONKO_OK);
    CHECK(ponko_ctx_text3d(ctx, e, &got) == PONKO_NOT_FOUND);

    CHECK(ponko_ctx_set_nav_avoidance(ctx, e, true, 2, 3.6f) == PONKO_NOT_FOUND);
    CHECK(ponko_ctx_set_nav_agent(ctx, e, 3, 0.3f, 10) == PONKO_OK);
    PonkoVec3 center = {0, 1, 0}, half = {1, 1, 1};
    CHECK(!ponko_nav_bake_settings_default().carving && ponko_nav_bake_settings_default().tile_size == 32);
    CHECK(ponko_ctx_set_nav_carve_obstacle(ctx, e, center, half) == PONKO_OK);
    size_t steps = 0; bool pending = false;
    CHECK(ponko_ctx_navmesh_update_carving(ctx, 0, &steps, &pending) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_set_nav_avoidance(ctx, e, true, 2, 3.6f) == PONKO_OK);
    CHECK(ponko_ctx_set_nav_avoidance(ctx, e, false, 0, 1) == PONKO_OK);
    CHECK(ponko_ctx_set_nav_avoidance(ctx, e, true, -1, 3) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_set_nav_avoidance(ctx, e, true, 2, 0) == PONKO_INVALID_ARGUMENT);
}
static void tick(PonkoCtx* ctx, void* user) {
    Run* r = user;
    size_t len;
    CHECK(ponko_world_len(ponko_ctx_world(ctx), &len) == PONKO_OK && len == 1);
    r->dt = ponko_ctx_dt(ctx);
    if (++r->ticks == 3) CHECK(ponko_ctx_quit(ctx) == PONKO_OK);
}
/* A box car on four raycast wheels: spawned and driven through the C API,
   the vehicle as a query column. */
typedef struct { uint32_t transform, body, collider, vehicle; uint64_t car; } Cars;
static void ids(PonkoWorld* w, Cars* c) {
    CHECK(ponko_transform3d_component(w, &c->transform) == PONKO_OK);
    CHECK(ponko_rigid_body3d_component(w, &c->body) == PONKO_OK);
    CHECK(ponko_collider3d_component(w, &c->collider) == PONKO_OK);
    CHECK(ponko_vehicle3d_component(w, &c->vehicle) == PONKO_OK);
}
static void car_setup(PonkoCtx* ctx, void* user) {
    Cars* c = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    ids(w, c);
    PonkoTransform3D floor_at = ponko_transform3d_at((PonkoVec3){0, -0.5f, 0});
    PonkoRigidBody3D fixed = ponko_rigid_body3d(PONKO_BODY_FIXED);
    PonkoCollider3D floor = ponko_collider3d_box((PonkoVec3){200, 1, 200});
    PonkoValue ground[] = {{c->transform, &floor_at, sizeof floor_at}, {c->body, &fixed, sizeof fixed},
                            {c->collider, &floor, sizeof floor}};
    uint64_t e; CHECK(ponko_spawn(w, ground, 3, &e) == PONKO_OK);
    PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){0, 0.8f, 0});
    PonkoRigidBody3D chassis = ponko_rigid_body3d(PONKO_BODY_DYNAMIC);
    chassis.mass = 1200;
    chassis.has_center_of_mass = true;
    chassis.center_of_mass = (PonkoVec3){0, -0.3f, 0};
    PonkoCollider3D box = ponko_collider3d_box((PonkoVec3){1.8f, 0.6f, 4.2f});
    PonkoVehicle3D car = ponko_vehicle3d();
    const float x[] = {-0.8f, 0.8f, -0.8f, 0.8f}, z[] = {-1.3f, -1.3f, 1.3f, 1.3f};
    for (int i = 0; i < 4; ++i)
        CHECK(ponko_vehicle3d_add_wheel(&car, ponko_wheel3d((PonkoVec3){x[i], -0.3f, z[i]}, 0.35f)) == PONKO_OK);
    PonkoValue values[] = {{c->transform, &at, sizeof at}, {c->body, &chassis, sizeof chassis},
                            {c->collider, &box, sizeof box}, {c->vehicle, &car, sizeof car}};
    CHECK(ponko_spawn(w, values, 4, &c->car) == PONKO_OK);
}
static void throttle(const PonkoChunk* chunk, void* user) {
    (void)user;
    PonkoVehicle3D* cars = chunk->columns[0];
    for (size_t i = 0; i < chunk->count; ++i) {
        cars[i].wheels[2].motor_torque = 500;
        cars[i].wheels[3].motor_torque = 500;
    }
}
static void drive(PonkoCtx* ctx, void* user) {
    Cars* c = user;
    if (ponko_ctx_sim_tick(ctx) > 25) CHECK(ponko_query(ponko_ctx_world(ctx), &c->vehicle, 1, throttle, NULL) == PONKO_OK);
}
static void vehicles(void) {
    Cars c = {0};
    PonkoGame* game = ponko_game_new("c vehicles", 1, 1); CHECK(game);
    CHECK(ponko_game_sim_system(game, "drive", 1, drive, &c) == PONKO_OK);
    CHECK(ponko_game_physics3d(game, (PonkoVec3){0, -9.81f, 0}) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 150, car_setup, &c, &w) == PONKO_OK);
    PonkoVehicle3D car; PonkoTransform3D t;
    CHECK(ponko_get(w, c.car, c.vehicle, &car, sizeof car) == PONKO_OK);
    CHECK(ponko_get(w, c.car, c.transform, &t, sizeof t) == PONKO_OK);
    CHECK(car.wheel_count == 4 && car.wheels[0].grounded && car.wheels[3].angular_velocity > 0);
    CHECK(t.position.z < -3 && t.position.y > 0.7f && t.position.y < 0.9f);
    PonkoTransform3D wheel = ponko_wheel3d_pose(&car.wheels[0], t);
    CHECK(wheel.position.y > 0.3f && wheel.position.y < 0.4f);
    CHECK(ponko_friction_curve_evaluate(ponko_friction_curve_sideways(), 0.2f) == 1);
    CHECK(ponko_world_free(w) == PONKO_OK);
}
/* The 3D casts, overlaps and distance queries inside ponko_query (a ground
   check for each entity a loop visits), refused in ponko_query_par; what
   changes physics stays refused in both. */
typedef struct { PonkoCtx* ctx; uint32_t marker; uint64_t wall; PonkoStatus statuses[16]; size_t calls, found_wall; } Loop3;
static void probe3d(const PonkoChunk* chunk, void* user) {
    Loop3* l = user;
    for (size_t i = 0; i < chunk->count; ++i) {
        const PonkoQueryFilter3D solid = ponko_query_filter3d();
        const PonkoVec3 origin = {0, 0, 0}, right = {1, 0, 0}, at_wall = {10, 0, 0};
        bool found = false;
        PonkoHit3D hit;
        PonkoPointHit3D near;
        PonkoEntity entities[4];
        PonkoPenetration3D pushes[4];
        size_t n = 0;
        PonkoStatus* s = l->statuses;
        s[0] = ponko_ctx_cast_ray3d(l->ctx, origin, right, 50, solid, &found, &hit);
        if (s[0] == PONKO_OK) { ++l->calls; if (found && ponko_entity_bits(hit.entity) == l->wall) ++l->found_wall; }
        s[1] = ponko_ctx_cast_sphere3d(l->ctx, origin, 0.5f, right, 50, solid, &found, &hit);
        s[2] = ponko_ctx_cast_capsule3d(l->ctx, origin, 1, 0.5f, right, 50, solid, &found, &hit);
        s[3] = ponko_ctx_overlap_sphere3d(l->ctx, at_wall, 1, solid, entities, 4, &n);
        s[4] = ponko_ctx_closest_point3d(l->ctx, origin, 50, solid, &found, &near);
        s[5] = ponko_ctx_penetrations3d(l->ctx, ponko_collider3d_sphere(1), (PonkoVec3){8.4f, 0, 0},
                                        ponko_quat_identity(), solid, pushes, 4, &n);
        s[6] = ponko_ctx_set_gravity3d(l->ctx, (PonkoVec3){0, -9.81f, 0});
    }
}
static void probe3d_system(PonkoCtx* ctx, void* user) {
    Loop3* l = user;
    if (ponko_ctx_sim_tick(ctx) != 1) return;
    l->ctx = ctx;
    PonkoWorld* w = ponko_ctx_world(ctx);
    CHECK(ponko_query(w, &l->marker, 1, probe3d, l) == PONKO_OK);
    CHECK(l->calls == 2 && l->found_wall == 2);
    for (int i = 0; i < 6; ++i) CHECK(l->statuses[i] == PONKO_OK);
    CHECK(l->statuses[6] == PONKO_BUSY);
    for (int i = 0; i < 7; ++i) l->statuses[i] = PONKO_OK;
    CHECK(ponko_query_par(w, &l->marker, 1, probe3d, l) == PONKO_OK);
    for (int i = 0; i < 7; ++i) CHECK(l->statuses[i] == PONKO_BUSY);
}
static void probe3d_setup(PonkoCtx* ctx, void* user) {
    Loop3* l = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    uint32_t transform, body, collider;
    CHECK(ponko_transform3d_component(w, &transform) == PONKO_OK);
    CHECK(ponko_rigid_body3d_component(w, &body) == PONKO_OK);
    CHECK(ponko_collider3d_component(w, &collider) == PONKO_OK);
    CHECK(ponko_component_register(w, "Visited", sizeof(uint32_t), _Alignof(uint32_t), &l->marker) == PONKO_OK);
    PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){10, 0, 0});
    PonkoRigidBody3D fixed = ponko_rigid_body3d(PONKO_BODY_FIXED);
    PonkoCollider3D shape = ponko_collider3d_box((PonkoVec3){2, 20, 20});
    PonkoValue wall[] = {{transform, &at, sizeof at}, {body, &fixed, sizeof fixed}, {collider, &shape, sizeof shape}};
    CHECK(ponko_spawn(w, wall, 3, &l->wall) == PONKO_OK);
    uint32_t one = 1;
    PonkoValue marked[] = {{l->marker, &one, sizeof one}};
    uint64_t e;
    for (int i = 0; i < 2; ++i) CHECK(ponko_spawn(w, marked, 1, &e) == PONKO_OK);
}
static void physics_queries3d_in_loops(void) {
    Loop3 l = {0};
    PonkoGame* game = ponko_game_new("c 3d queries in loops", 1, 1); CHECK(game);
    CHECK(ponko_game_physics3d(game, (PonkoVec3){0, 0, 0}) == PONKO_OK);
    CHECK(ponko_game_sim_system(game, "probe", 1, probe3d_system, &l) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 2, probe3d_setup, &l, &w) == PONKO_OK);
    ponko_world_free(w);
    CHECK(l.calls == 2 && l.found_wall == 2); /* only the serial loop's calls worked */
}
/* Phases: a BEFORE_PHYSICS system feeds this tick's step; a sim system,
   even one added before physics3d, runs after the step and sees its result. */
typedef struct { uint32_t transform, velocity; uint64_t ball; float seen_before, seen_after; } Kick;
static void kick_setup(PonkoCtx* ctx, void* user) {
    Kick* k = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    uint32_t body, collider;
    CHECK(ponko_transform3d_component(w, &k->transform) == PONKO_OK);
    CHECK(ponko_velocity3d_component(w, &k->velocity) == PONKO_OK);
    CHECK(ponko_rigid_body3d_component(w, &body) == PONKO_OK);
    CHECK(ponko_collider3d_component(w, &collider) == PONKO_OK);
    PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){0, 0, 0});
    PonkoRigidBody3D rb = ponko_rigid_body3d(PONKO_BODY_DYNAMIC);
    PonkoCollider3D sphere = ponko_collider3d_sphere(0.5f);
    PonkoVelocity3D still = {{0, 0, 0}, {0, 0, 0}};
    PonkoValue values[] = {{k->transform, &at, sizeof at}, {body, &rb, sizeof rb},
                            {collider, &sphere, sizeof sphere}, {k->velocity, &still, sizeof still}};
    CHECK(ponko_spawn(w, values, 4, &k->ball) == PONKO_OK);
}
static void kick(PonkoCtx* ctx, void* user) {
    Kick* k = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    PonkoTransform3D t; CHECK(ponko_get(w, k->ball, k->transform, &t, sizeof t) == PONKO_OK);
    k->seen_before = t.position.x;
    PonkoVelocity3D v = {{5, 0, 0}, {0, 0, 0}};
    CHECK(ponko_set(w, k->ball, k->velocity, &v, sizeof v) == PONKO_OK);
}
static void watch(PonkoCtx* ctx, void* user) {
    Kick* k = user;
    PonkoTransform3D t; CHECK(ponko_get(ponko_ctx_world(ctx), k->ball, k->transform, &t, sizeof t) == PONKO_OK);
    k->seen_after = t.position.x;
}
static void phases(void) {
    Kick k = {0};
    PonkoGame* game = ponko_game_new("c phases", 1, 1); CHECK(game);
    CHECK(ponko_game_fixed_hz(game, 50) == PONKO_OK);
    CHECK(ponko_game_sim_system(game, "watch", 1, watch, &k) == PONKO_OK);
    CHECK(ponko_game_add_system(game, PONKO_PHASE_BEFORE_PHYSICS, "kick", 1, kick, &k) == PONKO_OK);
    CHECK(ponko_game_add_system(game, 99, "bad", 1, kick, &k) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_game_add_system(game, PONKO_PHASE_FRAME, "bad", 2, kick, &k) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_game_physics3d(game, (PonkoVec3){0, 0, 0}) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 1, kick_setup, &k, &w) == PONKO_OK);
    CHECK(k.seen_before == 0);
    CHECK(fabsf(k.seen_after - 5.0f / 50.0f) < 1e-3f);
    CHECK(ponko_world_free(w) == PONKO_OK);
}
/* System ids, setup callbacks and plugins: each callback logs a letter. */
typedef struct { char text[32]; int n; } Order;
static void put(Order* o, char c) { if (o->n < 31) o->text[o->n++] = c; }
static void mark_a(PonkoCtx* ctx, void* user) { (void)ctx; put(user, 'a'); }
static void mark_b(PonkoCtx* ctx, void* user) { (void)ctx; put(user, 'b'); }
static void mark_s(PonkoCtx* ctx, void* user) { (void)ctx; put(user, 's'); }
static void mark_t(PonkoCtx* ctx, void* user) { (void)ctx; put(user, 't'); }
static void mark_r(PonkoCtx* ctx, void* user) { (void)ctx; put(user, 'r'); }
static void build_area(PonkoGame* game, void* user) {
    CHECK(ponko_game_add_system(game, PONKO_PHASE_BEFORE_PHYSICS, "area", 1, mark_b, user) == PONKO_OK);
}
static void plugins_setups_and_system_ids(void) {
    Order o = {0};
    PonkoGame* game = ponko_game_new("c plugins", 1, 1); CHECK(game);
    uint32_t first = 0, second = 0;
    CHECK(ponko_game_add_system_id(game, PONKO_PHASE_AFTER_PHYSICS, "same", 1, mark_a, &o, &first) == PONKO_OK);
    CHECK(ponko_game_add_system_id(game, PONKO_PHASE_AFTER_PHYSICS, "same", 1, mark_a, &o, &second) == PONKO_OK);
    CHECK(first != 0 && second != 0 && first != second);
    CHECK(ponko_game_remove(game, first) == PONKO_OK);
    CHECK(ponko_game_remove(game, first) == PONKO_OK);   /* twice is fine */
    CHECK(ponko_game_remove(game, PONKO_SYSTEM_DRAW_SPRITES) == PONKO_OK);
    CHECK(ponko_game_remove(game, PONKO_SYSTEM_UPDATE_AUDIO3D) == PONKO_OK);
    CHECK(ponko_game_remove_system(game, "ponko::ecs::draw_sprites") == PONKO_NOT_FOUND);
    CHECK(ponko_game_add_system_id(game, PONKO_PHASE_FRAME, "bad", 2, mark_a, &o, &first) == PONKO_INVALID_ARGUMENT);
    CHECK(first == 0);
    /* Setups and plugins start in the order they were added, before the run setup. */
    CHECK(ponko_game_setup(game, mark_s, &o) == PONKO_OK);
    CHECK(ponko_game_plugin(game, build_area, mark_t, &o) == PONKO_OK);
    CHECK(ponko_game_setup(game, NULL, &o) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_game_plugin(game, NULL, NULL, &o) == PONKO_INVALID_ARGUMENT);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 2, mark_r, &o, &w) == PONKO_OK);
    CHECK(strcmp(o.text, "strbaba") == 0);
    CHECK(ponko_world_free(w) == PONKO_OK);
}
/* The retained UI: a column with a button, laid out and clicked from C. */
static void ui_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoUiLayout column = ponko_ui_layout_default();
    column.flex_direction = PONKO_UI_FLEX_COLUMN;
    column.width.kind = PONKO_UI_LENGTH_POINTS; column.width.value = 300;
    column.height.kind = PONKO_UI_LENGTH_POINTS; column.height.value = 200;
    uint64_t root, button, label;
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &column, &root) == PONKO_OK);
    PonkoUiLayout fixed = ponko_ui_layout_default();
    fixed.width.kind = PONKO_UI_LENGTH_POINTS; fixed.width.value = 120;
    fixed.height.kind = PONKO_UI_LENGTH_POINTS; fixed.height.value = 40;
    CHECK(ponko_ctx_ui_add_label(ctx, PONKO_UI_SCREEN, root, "Shop", &fixed, &label) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_button(ctx, PONKO_UI_SCREEN, root, "Buy", &fixed, &button) == PONKO_OK);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, root, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_OK);
    PonkoVec2 window = {800, 600};
    CHECK(ponko_ctx_ui_frame(ctx, PONKO_UI_SCREEN, window, NULL) == PONKO_OK);
    PonkoRect r;
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, button, &r) == PONKO_OK && r.min.y == 40 && r.max.x == 120);
    char text[8]; size_t len;
    CHECK(ponko_ctx_ui_text(ctx, PONKO_UI_SCREEN, button, text, sizeof text, &len) == PONKO_OK && len == 3 && text[0] == 'B');
    PonkoUiInput in = ponko_ui_input_default();
    in.pointer.x = 10; in.pointer.y = 50; in.pressed = true; in.held = true;
    CHECK(ponko_ctx_ui_frame(ctx, PONKO_UI_SCREEN, window, &in) == PONKO_OK);
    in.pressed = false; in.held = false; in.released = true;
    CHECK(ponko_ctx_ui_frame(ctx, PONKO_UI_SCREEN, window, &in) == PONKO_OK);
    PonkoUiEvent event;
    CHECK(ponko_ctx_ui_next_event(ctx, PONKO_UI_SCREEN, &event) == PONKO_OK);
    CHECK(event.kind == PONKO_UI_EVENT_CLICKED && event.id == button);
    CHECK(ponko_ctx_ui_next_event(ctx, PONKO_UI_SCREEN, &event) == PONKO_NOT_FOUND);
    /* The original flat track arrays remain valid; templates add repeat fragments. */
    PonkoUiTrack repeated_track = {PONKO_UI_TRACK_FR, 1};
    PonkoUiTemplateTrack repeated = {0};
    repeated.repetition = 1; repeated.count = 2;
    repeated.tracks = &repeated_track; repeated.track_count = 1;
    PonkoUiTrack rows[] = {{PONKO_UI_TRACK_POINTS, 40}, {PONKO_UI_TRACK_POINTS, 160}};
    PonkoUiLayout named = ponko_ui_layout_default();
    named.display = PONKO_UI_DISPLAY_GRID;
    named.width = (PonkoUiLength){PONKO_UI_LENGTH_POINTS, 300};
    named.height = (PonkoUiLength){PONKO_UI_LENGTH_POINTS, 200};
    named.grid_template_columns = &repeated; named.grid_template_column_count = 1;
    named.grid_rows = rows; named.grid_row_count = 2;
    uint64_t grid, main;
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &named, &grid) == PONKO_OK);
    const char* areas[] = {"header header", "side main"};
    CHECK(ponko_ctx_ui_set_grid_template_areas(ctx, PONKO_UI_SCREEN, grid, areas, 2) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, grid, NULL, &main) == PONKO_OK);
    CHECK(ponko_ctx_ui_set_grid_area(ctx, PONKO_UI_SCREEN, main, "main") == PONKO_OK);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, grid, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_OK);
    CHECK(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, (PonkoVec2){800, 600}) == PONKO_OK);
    PonkoRect main_rect;
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, main, &main_rect) == PONKO_OK);
    CHECK(main_rect.min.x == 150 && main_rect.min.y == 40 && main_rect.max.x == 300 && main_rect.max.y == 200);
    const char* invalid_areas[] = {"x x", "x y"};
    CHECK(ponko_ctx_ui_set_grid_template_areas(ctx, PONKO_UI_SCREEN, grid, invalid_areas, 2) == PONKO_INVALID_ARGUMENT);
    const char* names[] = {"start", "finish"};
    CHECK(ponko_ctx_ui_set_grid_line_names(ctx, PONKO_UI_SCREEN, grid, 0, names, 2) == PONKO_OK);
    CHECK(ponko_ctx_ui_set_grid_named_line(ctx, PONKO_UI_SCREEN, main, 0, false, "start", 1) == PONKO_OK);
    CHECK(ponko_ctx_ui_set_grid_named_line(ctx, PONKO_UI_SCREEN, main, 0, true, "finish", 1) == PONKO_OK);
    CHECK(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, (PonkoVec2){800, 600}) == PONKO_OK);
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, main, &main_rect) == PONKO_OK);
    CHECK(main_rect.min.x == 0 && main_rect.max.x == 300);
    column.display = 99; /* not a PonkoUiDisplay */
    uint64_t bad;
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, root, &column, &bad) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_ui_remove(ctx, PONKO_UI_SCREEN, root) == PONKO_OK);
    CHECK(ponko_ctx_ui_remove(ctx, PONKO_UI_SCREEN, button) == PONKO_NOT_FOUND);
}
/* A world-space UI panel: its UI takes the same calls, with the entity as the target. */
static void panel_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    uint32_t transform; uint64_t entity;
    CHECK(ponko_transform3d_component(w, &transform) == PONKO_OK);
    PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){0, 1.5f, -2});
    PonkoValue value = {transform, &at, sizeof at};
    CHECK(ponko_spawn(w, &value, 1, &entity) == PONKO_OK);
    PonkoTexture texture;
    CHECK(ponko_ctx_create_ui_panel(ctx, entity, 600, 400, (PonkoVec2){1.2f, 0.8f}, &texture) == PONKO_OK);
    CHECK(texture.width == 600 && texture.height == 400);
    PonkoUiLayout fixed = ponko_ui_layout_default();
    fixed.width.kind = PONKO_UI_LENGTH_POINTS; fixed.width.value = 200;
    fixed.height.kind = PONKO_UI_LENGTH_POINTS; fixed.height.value = 60;
    uint64_t button, top;
    CHECK(ponko_ctx_ui_add_button(ctx, entity, PONKO_UI_NONE, "Buy", &fixed, &button) == PONKO_OK);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, entity, button, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_OK);
    CHECK(ponko_ctx_ui_top_screen(ctx, PONKO_UI_SCREEN, &top) == PONKO_OK && top == PONKO_UI_NONE);
    CHECK(ponko_ctx_ui_top_screen(ctx, entity, &top) == PONKO_OK && top == button);
    PonkoVec2 pixels = {600, 400};
    CHECK(ponko_ctx_ui_layout(ctx, entity, pixels) == PONKO_OK);
    PonkoRect r;
    CHECK(ponko_ctx_ui_rect(ctx, entity, button, &r) == PONKO_OK && r.max.x == 200 && r.max.y == 60);
    bool over; PonkoVec2 pixel;
    CHECK(ponko_ctx_ui_panel_pointer(ctx, entity, &over, &pixel) == PONKO_OK && !over);
    CHECK(ponko_ctx_set_ui_panel_interactive(ctx, entity, false) == PONKO_OK);
    CHECK(ponko_ctx_set_ui_pointer_ray(ctx, (PonkoVec3){0, 1.5f, 0}, (PonkoVec3){0, 0, -1}) == PONKO_OK);
    CHECK(ponko_ctx_set_ui_pointer_ray(ctx, (PonkoVec3){0, 0, 0}, (PonkoVec3){0, 0, 0}) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_clear_ui_pointer_ray(ctx) == PONKO_OK);
    CHECK(ponko_ctx_remove_ui_panel(ctx, entity) == PONKO_OK);
    CHECK(ponko_ctx_ui_top_screen(ctx, entity, &top) == PONKO_NOT_FOUND);
}
static void ui_panels(void) {
    PonkoGame* game = ponko_game_new("c panels", 1, 1); CHECK(game);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 0, panel_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
}
/* Layout presets, the theme, titles, names and take_click. */
static void ui_presets_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoUiLayout fill = ponko_ui_layout_fill();
    fill.padding.top = fill.padding.left = (PonkoUiLength){PONKO_UI_LENGTH_POINTS, 10};
    PonkoUiLayout row = ponko_ui_layout_row(40), fixed = ponko_ui_layout_fixed(100, 30);
    PonkoUiLayout column = ponko_ui_layout_column(120);
    PonkoUiLayout map = ponko_ui_layout_anchored(0.75f, 0, 0.25f, 0.25f);
    PonkoUiLayout dialog_layout = ponko_ui_layout_centered(200, 100);
    uint64_t root, title, buy, side, corner, dialog, found, clicked;
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &fill, &root) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_title(ctx, PONKO_UI_SCREEN, root, "Shop", &row, &title) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_button(ctx, PONKO_UI_SCREEN, root, "Buy", &fixed, &buy) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, root, &column, &side) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, root, &map, &corner) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &dialog_layout, &dialog) == PONKO_OK);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, root, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_OK);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, dialog, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_OK);
    CHECK(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, (PonkoVec2){800, 600}) == PONKO_OK);
    PonkoRect r;
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, title, &r) == PONKO_OK && r.min.y == 10 && r.max.y == 50);
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, buy, &r) == PONKO_OK && r.min.x == 10 && r.max.x == 110);
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, corner, &r) == PONKO_OK && r.min.x == 600 && r.max.y == 150);
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, dialog, &r) == PONKO_OK && r.min.x == 300 && r.min.y == 250);
    CHECK(ponko_ctx_ui_pop_screen(ctx, PONKO_UI_SCREEN, &found) == PONKO_OK && found == dialog);
    /* A modal screen owns the pointer; an unknown mode is refused; the old bool form still works. */
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, dialog, PONKO_UI_SCREEN_MODE_MODAL) == PONKO_OK);
    bool wants = false;
    CHECK(ponko_ctx_ui_wants_pointer(ctx, PONKO_UI_SCREEN, &wants) == PONKO_OK && wants);
    CHECK(ponko_ctx_ui_pop_screen(ctx, PONKO_UI_SCREEN, &found) == PONKO_OK && found == dialog);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, dialog, 7) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_ui_push_screen(ctx, PONKO_UI_SCREEN, dialog, true) == PONKO_OK);
    CHECK(ponko_ctx_ui_pop_screen(ctx, PONKO_UI_SCREEN, &found) == PONKO_OK && found == dialog);
    /* The theme: read, changed, followed by titles and the immediate widgets. */
    PonkoUiTheme theme = ponko_ui_theme_default();
    CHECK(ponko_ctx_ui_theme(ctx, PONKO_UI_SCREEN, &theme) == PONKO_OK && theme.title_size > theme.text_size);
    theme.title = (PonkoColor){1, 1, 0, 1};
    theme.title_size = 30;
    theme.accent = (PonkoColor){0, 1, 0, 1};
    CHECK(ponko_ctx_ui_set_theme(ctx, PONKO_UI_SCREEN, &theme) == PONKO_OK);
    PonkoUiPaint paint;
    CHECK(ponko_ctx_ui_paint(ctx, PONKO_UI_SCREEN, title, &paint) == PONKO_OK);
    CHECK(paint.font_size == 30 && paint.color.g == 1 && paint.color.b == 0);
    CHECK(ponko_ctx_ui_paint(ctx, PONKO_UI_SCREEN, buy, &paint) == PONKO_OK && paint.border.g == 1 && paint.border.r == 0);
    PonkoUiStyle style;
    CHECK(ponko_ctx_ui_style(ctx, &style) == PONKO_OK && style.accent.g == 1 && style.accent.r == 0);
    theme.text_size = -1;
    CHECK(ponko_ctx_ui_set_theme(ctx, PONKO_UI_SCREEN, &theme) == PONKO_INVALID_ARGUMENT);
    /* Names instead of kept ids. */
    CHECK(ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, buy, "shop.buy") == PONKO_OK);
    CHECK(ponko_ctx_ui_find(ctx, PONKO_UI_SCREEN, "shop.buy", &found) == PONKO_OK && found == buy);
    CHECK(ponko_ctx_ui_find(ctx, PONKO_UI_SCREEN, "shop.sell", &found) == PONKO_OK && found == PONKO_UI_NONE);
    char name[16]; size_t len;
    CHECK(ponko_ctx_ui_name(ctx, PONKO_UI_SCREEN, buy, name, sizeof name, &len) == PONKO_OK && len == 8 && strcmp(name, "shop.buy") == 0);
    /* take_click: the last click's node, once. */
    PonkoVec2 window = {800, 600};
    PonkoUiInput in = ponko_ui_input_default();
    in.pointer = (PonkoVec2){50, 65}; in.pressed = true; in.held = true;
    CHECK(ponko_ctx_ui_frame(ctx, PONKO_UI_SCREEN, window, &in) == PONKO_OK);
    in.pressed = false; in.held = false; in.released = true;
    CHECK(ponko_ctx_ui_frame(ctx, PONKO_UI_SCREEN, window, &in) == PONKO_OK);
    CHECK(ponko_ctx_ui_take_click(ctx, PONKO_UI_SCREEN, &clicked) == PONKO_OK && clicked == buy);
    CHECK(ponko_ctx_ui_take_click(ctx, PONKO_UI_SCREEN, &clicked) == PONKO_OK && clicked == PONKO_UI_NONE);
    CHECK(ponko_ctx_ui_remove(ctx, PONKO_UI_SCREEN, root) == PONKO_OK);
    CHECK(ponko_ctx_ui_find(ctx, PONKO_UI_SCREEN, "shop.buy", &found) == PONKO_OK && found == PONKO_UI_NONE);
}
/* A screen root's auto margins take the window's free space, as a child's do. */
static void ui_auto_margins_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoUiLength automatic = {PONKO_UI_LENGTH_AUTO, 0};
    PonkoUiLayout across = ponko_ui_layout_fixed(200, 100);
    across.margin.left = across.margin.right = automatic;
    across.margin.top = (PonkoUiLength){PONKO_UI_LENGTH_POINTS, 40};
    PonkoUiLayout middle = ponko_ui_layout_fixed(200, 100);
    middle.margin.left = middle.margin.right = middle.margin.top = middle.margin.bottom = automatic;
    PonkoUiLayout pushed = ponko_ui_layout_fixed(200, 100);
    pushed.margin.left = automatic;
    uint64_t a, m, p;
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &across, &a) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &middle, &m) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &pushed, &p) == PONKO_OK);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, a, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_OK);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, m, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_OK);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, p, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_OK);
    CHECK(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, (PonkoVec2){800, 600}) == PONKO_OK);
    PonkoRect r;
    /* Centered across, 40 points down. */
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, a, &r) == PONKO_OK && r.min.x == 300 && r.min.y == 40 && r.max.x == 500);
    /* Auto on all four sides: the middle of the window. */
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, m, &r) == PONKO_OK && r.min.x == 300 && r.min.y == 250);
    /* Only the left margin auto: pushed to the right edge. */
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, p, &r) == PONKO_OK && r.min.x == 600 && r.max.x == 800 && r.min.y == 0);
    /* A bigger window moves them with it. */
    CHECK(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, (PonkoVec2){1000, 800}) == PONKO_OK);
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, m, &r) == PONKO_OK && r.min.x == 400 && r.min.y == 350);
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, p, &r) == PONKO_OK && r.min.x == 800);
}
/* What a screen reader reads of a UI, and what it can ask of it. */
static void ui_access_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    const uint64_t s = PONKO_UI_SCREEN;
    const PonkoVec2 window = {800, 600};
    PonkoUiLayout fill = ponko_ui_layout_fill(), row = ponko_ui_layout_row(30);
    uint64_t root, play, volume, name;
    CHECK(ponko_ctx_ui_add_panel(ctx, s, PONKO_UI_NONE, &fill, &root) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_button(ctx, s, root, "Play", &row, &play) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_slider(ctx, s, root, 0.5f, &row, &volume) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_text_input(ctx, s, root, "Ann", &row, &name) == PONKO_OK);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, s, root, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_OK);
    /* Names, a description and a live region. */
    CHECK(ponko_ctx_ui_set_accessible_name(ctx, s, volume, "Volume") == PONKO_OK);
    CHECK(ponko_ctx_ui_set_accessible_description(ctx, s, play, "Starts a run") == PONKO_OK);
    CHECK(ponko_ctx_ui_set_live_region(ctx, s, play, PONKO_UI_LIVE_POLITE) == PONKO_OK);
    char text[512]; size_t len = 0;
    CHECK(ponko_ctx_ui_accessible_name(ctx, s, volume, text, sizeof text, &len) == PONKO_OK && strcmp(text, "Volume") == 0);
    CHECK(ponko_ctx_ui_accessible_description(ctx, s, play, text, sizeof text, &len) == PONKO_OK && len == 12);
    PonkoUiLive live = PONKO_UI_LIVE_OFF;
    CHECK(ponko_ctx_ui_live_region(ctx, s, play, &live) == PONKO_OK && live == PONKO_UI_LIVE_POLITE);
    CHECK(ponko_ctx_ui_frame(ctx, s, window, NULL) == PONKO_OK);
    CHECK(ponko_ctx_ui_access_text(ctx, s, text, sizeof text, &len) == PONKO_OK);
    CHECK(strstr(text, "group\n  button \"Play\" description=\"Starts a run\" live=polite focusable\n") == text);

    /* The snapshot: every node at first, then only what changed. */
    PonkoUiAccess* access = ponko_ui_access_new(); CHECK(access);
    PonkoUiAccessChanges changes;
    CHECK(ponko_ui_access_update(access, ctx, s, &changes) == PONKO_OK && changes.any && changes.changed_count == 4);
    size_t count = 0;
    CHECK(ponko_ui_access_count(access, &count) == PONKO_OK && count == 4);
    PonkoUiAccessNode node;
    CHECK(ponko_ui_access_node(access, 0, &node) == PONKO_OK && node.id == root && node.role == PONKO_UI_ROLE_GROUP && node.child_count == 3);
    uint64_t child = 0;
    CHECK(ponko_ui_access_child(access, root, 1, &child) == PONKO_OK && child == volume);
    CHECK(ponko_ui_access_find(access, volume, &node) == PONKO_OK && node.role == PONKO_UI_ROLE_SLIDER);
    CHECK(node.value_kind == PONKO_UI_ACCESS_VALUE_RANGE && node.value == 0.5f && node.focusable && !node.focused);
    CHECK(ponko_ui_access_find(access, name, &node) == PONKO_OK && node.has_selection && node.selection_caret == 3);
    CHECK(ponko_ui_access_name(access, volume, text, sizeof text, &len) == PONKO_OK && strcmp(text, "Volume") == 0);
    CHECK(ponko_ui_access_description(access, play, text, sizeof text, &len) == PONKO_OK && len == 12);
    CHECK(ponko_ui_access_value_text(access, name, text, sizeof text, &len) == PONKO_OK && strcmp(text, "Ann") == 0);
    uint64_t focus = 0;
    CHECK(ponko_ui_access_focus(access, &focus) == PONKO_OK && focus == PONKO_UI_NONE);
    CHECK(ponko_ui_access_update(access, ctx, s, &changes) == PONKO_OK && !changes.any);

    /* A request does what the key does: the event, the value, the focus. */
    CHECK(ponko_ctx_ui_access_action(ctx, s, PONKO_UI_ACCESS_ACTION_ACTIVATE, play, 0, NULL) == PONKO_OK);
    CHECK(ponko_ctx_ui_access_action(ctx, s, PONKO_UI_ACCESS_ACTION_INCREMENT, volume, 0, NULL) == PONKO_OK);
    CHECK(ponko_ctx_ui_access_action(ctx, s, PONKO_UI_ACCESS_ACTION_SET_TEXT, name, 0, "Bea") == PONKO_OK);
    CHECK(ponko_ctx_ui_frame(ctx, s, window, NULL) == PONKO_OK);
    PonkoUiEvent event;
    CHECK(ponko_ctx_ui_next_event(ctx, s, &event) == PONKO_OK && event.kind == PONKO_UI_EVENT_CLICKED && event.id == play);
    CHECK(ponko_ui_access_update(access, ctx, s, &changes) == PONKO_OK && changes.any && changes.focus_changed);
    CHECK(changes.changed_count == 3 && changes.removed_count == 0);
    uint64_t changed = 0;
    CHECK(ponko_ui_access_changed(access, 0, &changed) == PONKO_OK && changed == play);
    CHECK(ponko_ui_access_focus(access, &focus) == PONKO_OK && focus == play);
    CHECK(ponko_ui_access_find(access, volume, &node) == PONKO_OK && node.value > 0.54f && node.value < 0.56f);
    CHECK(ponko_ui_access_value_text(access, name, text, sizeof text, &len) == PONKO_OK && strcmp(text, "Bea") == 0);

    /* A removed node is reported and no longer found. */
    CHECK(ponko_ctx_ui_remove(ctx, s, volume) == PONKO_OK);
    CHECK(ponko_ctx_ui_frame(ctx, s, window, NULL) == PONKO_OK);
    CHECK(ponko_ui_access_update(access, ctx, s, &changes) == PONKO_OK && changes.removed_count == 1);
    uint64_t removed = 0;
    CHECK(ponko_ui_access_removed(access, 0, &removed) == PONKO_OK && removed == volume);
    CHECK(ponko_ui_access_find(access, volume, &node) == PONKO_NOT_FOUND);
    CHECK(ponko_ui_access_reset(access) == PONKO_OK);
    CHECK(ponko_ui_access_update(access, ctx, s, &changes) == PONKO_OK && changes.changed_count == 3);
    CHECK(ponko_ctx_ui_set_live_region(ctx, s, play, 9) == PONKO_INVALID_ARGUMENT);
    ponko_ui_access_free(access);
}
static void retained_ui(void) {
    PonkoGame* game = ponko_game_new("c ui", 1, 1); CHECK(game);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 0, ui_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    game = ponko_game_new("c ui presets", 1, 1); CHECK(game);
    CHECK(ponko_game_run_headless(game, 0, ui_presets_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    game = ponko_game_new("c ui auto margins", 1, 1); CHECK(game);
    CHECK(ponko_game_run_headless(game, 0, ui_auto_margins_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    game = ponko_game_new("c ui access", 1, 1); CHECK(game);
    CHECK(ponko_game_run_headless(game, 0, ui_access_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
}
/* Ribbon and line defaults, and a decal cut from a floor. */
static void ribbons_and_decals(void) {
    PonkoTrail3D trail = ponko_trail3d_new(0.25f, 0.05f);
    CHECK(trail.lifetime == 0.25f && trail.width == 0.05f && trail.max_points == 64 && trail.emitting && !trail.has_texture);
    PonkoLine3D line = ponko_line3d_new(0.1f, (PonkoColor){1, 0, 0, 1});
    CHECK(line.width == 0.1f && line.point_count == 0 && line.points == NULL && line.texture_length == 0);
    PonkoMeshData* floor = ponko_mesh_data_plane(4);
    float down[16] = {0.5f, 0, 0, 0, 0, 0, -0.2f, 0, 0, 0.5f, 0, 0, 1, 0, 0, 1};
    PonkoMeshData* mark = ponko_mesh_data_decal(floor, down, 0.002f);
    CHECK(mark);
    PonkoMeshView view;
    CHECK(ponko_mesh_data_view(mark, &view) == PONKO_OK && view.index_count >= 3);
    ponko_mesh_data_free(mark);
    CHECK(ponko_mesh_data_decal(floor, NULL, 0.002f) == NULL);
    ponko_mesh_data_free(floor);
}
static void far_ground(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoMaterial ground; CHECK(ponko_ctx_default_material(ctx, &ground) == PONKO_OK);
    static float heights[33 * 33];
    for (int i = 0; i < 33 * 33; ++i) heights[i] = 1.0f;
    PonkoVec2 corner = {-256, -256};
    PonkoFarTerrain* far = ponko_far_terrain_new(heights, 33 * 33, 33, corner, 16, 64, ground); CHECK(far);
    CHECK(ponko_far_terrain_update(far, ctx) == PONKO_OK);
    PonkoCell cell = {0, 0}; bool cut = true; size_t meshes = 0;
    CHECK(ponko_far_terrain_is_cut(far, cell, &cut, &meshes) == PONKO_OK && !cut && meshes == 1);
    CHECK(ponko_far_terrain_cut(far, cell, true) == PONKO_OK);
    CHECK(ponko_far_terrain_update(far, ctx) == PONKO_OK);
    CHECK(ponko_far_terrain_is_cut(far, cell, &cut, NULL) == PONKO_OK && cut);
    CHECK(ponko_far_terrain_clear(far, ctx) == PONKO_OK);
    ponko_far_terrain_free(far);
    PonkoCollisionMeshData* data = ponko_collision_mesh_data_heightfield(heights, 33 * 33, 64); CHECK(data);
    PonkoCollisionMesh mesh;
    CHECK(ponko_ctx_physics3d_add_mesh(ctx, data, &mesh) == PONKO_OK);
    ponko_collision_mesh_data_free(data);
    PonkoTerrainCollider solid = ponko_terrain_collider(mesh);
    CHECK(solid.friction == .5f && solid.mesh.id == mesh.id);
    PonkoColliderFocus focus = ponko_collider_focus(100, 150);
    CHECK(focus.radius == 100 && focus.keep_radius == 150);
    CHECK(ponko_ctx_physics3d_stream_colliders(ctx, true, 4) == PONKO_OK);
    bool active = true;
    CHECK(ponko_ctx_physics3d_is_collider_active(ctx, 0, &active) == PONKO_OK && !active);
}

static void streamed_worlds(void) {
    PonkoGame* game = ponko_game_new("far", 1, 1); CHECK(game);
    CHECK(ponko_game_physics3d(game, (PonkoVec3){0, -9.81f, 0}) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 0, far_ground, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    float flat[4] = {0, 0, 0, 0};
    PonkoMaterial none = {0};
    CHECK(!ponko_far_terrain_new(flat, 4, 2, (PonkoVec2){0, 0}, 1, 1.5f, none));
    CHECK(!ponko_collision_mesh_data_heightfield(flat, 3, 1));
}

/* A terrain tile saved, then loaded: the rebuild makes its ground again
   from its saved corner. */
typedef struct { uint32_t transform, focus; int calls; bool solid; } TileRebuild;
static PonkoCollisionMeshData* tile_ground(const PonkoLoadedWorld* world, uint64_t tile, void* user) {
    TileRebuild* r = user;
    PonkoTransform3D at;
    CHECK(ponko_loaded_world_get(world, tile, r->transform, &at, sizeof at) == PONKO_OK);
    PonkoColliderFocus focus;
    CHECK(ponko_loaded_world_get(world, tile, r->focus, &focus, sizeof focus) == PONKO_NOT_FOUND);
    r->calls++;
    if (at.position.x != -10) return NULL;
    float heights[9] = {2, 2, 2, 2, 2, 2, 2, 2, 2};
    return ponko_collision_mesh_data_heightfield(heights, 9, 20);
}
static void tile_save_load(PonkoCtx* ctx, void* user) {
    TileRebuild* r = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    uint32_t terrain;
    CHECK(ponko_transform3d_component(w, &r->transform) == PONKO_OK);
    CHECK(ponko_collider_focus_component(w, &r->focus) == PONKO_OK);
    CHECK(ponko_terrain_collider_component(w, &terrain) == PONKO_OK);
    float heights[9] = {2, 2, 2, 2, 2, 2, 2, 2, 2};
    PonkoCollisionMeshData* data = ponko_collision_mesh_data_heightfield(heights, 9, 20); CHECK(data);
    PonkoCollisionMesh mesh;
    CHECK(ponko_ctx_physics3d_add_mesh(ctx, data, &mesh) == PONKO_OK);
    ponko_collision_mesh_data_free(data);
    PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){-10, 0, -10});
    PonkoTerrainCollider solid = ponko_terrain_collider(mesh);
    PonkoValue tile[2] = {{r->transform, &at, sizeof at}, {terrain, &solid, sizeof solid}};
    uint64_t e;
    CHECK(ponko_spawn(w, tile, 2, &e) == PONKO_OK);
    CHECK(ponko_ctx_save_world(ctx, "tiles") == PONKO_OK);
    CHECK(ponko_ctx_physics3d_set_terrain_rebuild(ctx, NULL, r) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_physics3d_set_terrain_rebuild(ctx, tile_ground, r) == PONKO_OK);
    CHECK(ponko_ctx_load_world(ctx, "tiles") == PONKO_OK);
    CHECK(r->calls == 1);
    CHECK(ponko_ctx_physics3d_clear_terrain_rebuild(ctx) == PONKO_OK);
    CHECK(ponko_ctx_load_world(ctx, "tiles") == PONKO_OK);
    CHECK(r->calls == 1);
    CHECK(ponko_ctx_physics3d_set_terrain_rebuild(ctx, tile_ground, r) == PONKO_OK);
    CHECK(ponko_ctx_load_world(ctx, "tiles") == PONKO_OK);
    CHECK(r->calls == 2);
    CHECK(ponko_ctx_delete_world(ctx, "tiles") == PONKO_OK);
}
static void tile_probe(PonkoCtx* ctx, void* user) {
    TileRebuild* r = user;
    if (ponko_ctx_sim_tick(ctx) != 1) return;
    bool found = false; PonkoHit3D hit;
    CHECK(ponko_ctx_cast_ray3d(ctx, (PonkoVec3){0, 10, 0}, (PonkoVec3){0, -1, 0}, 20, ponko_query_filter3d(), &found, &hit) == PONKO_OK);
    r->solid = found && fabsf(hit.point.y - 2) < 0.01f;
}

static void terrain_rebuild(void) {
    TileRebuild r = {0};
    PonkoGame* g = ponko_game_new("tiles", 1, 1); CHECK(g);
    CHECK(ponko_game_data_dir(g, "ponko-c-terrain") == PONKO_OK);
    CHECK(ponko_game_physics3d(g, (PonkoVec3){0, 0, 0}) == PONKO_OK);
    CHECK(ponko_game_sim_system(g, "probe", 1, tile_probe, &r) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(g, 3, tile_save_load, &r, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    CHECK(r.calls == 2 && r.solid);
}

static void recolor(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    uint32_t transform, model;
    CHECK(ponko_transform3d_component(w, &transform) == PONKO_OK && ponko_model_component(w, &model) == PONKO_OK);
    PonkoMaterialDesc look = ponko_material_desc((PonkoColor){1, 0.8f, 0.2f, 1});
    PonkoMaterial gold, wood;
    CHECK(ponko_ctx_create_material(ctx, &look, &gold) == PONKO_OK);
    CHECK(ponko_ctx_create_material(ctx, &look, &wood) == PONKO_OK);
    PonkoMeshData* cube = ponko_mesh_data_cube(1);
    PonkoMesh mesh;
    CHECK(ponko_ctx_create_plain_mesh(ctx, cube, &mesh) == PONKO_OK);
    CHECK(ponko_ctx_create_plain_mesh(ctx, NULL, &mesh) == PONKO_INVALID_ARGUMENT);
    ponko_mesh_data_free(cube);
    PonkoTransform3D t = ponko_transform3d_at((PonkoVec3){0, 0, 0});
    PonkoModel m = ponko_model(mesh);
    PonkoValue parts[] = {{transform, &t, sizeof t}, {model, &m, sizeof m}};
    uint64_t e;
    CHECK(ponko_spawn(w, parts, 2, &e) == PONKO_OK);

    PonkoMaterial found; bool has = true;
    CHECK(ponko_ctx_model_material(ctx, e, &found, &has) == PONKO_OK && !has);
    CHECK(ponko_ctx_set_model_material(ctx, e, gold, true) == PONKO_OK);
    CHECK(ponko_ctx_model_material(ctx, e, &found, &has) == PONKO_OK && has && found.id == gold.id);
    CHECK(ponko_ctx_set_model_material(ctx, e, (PonkoMaterial){99}, true) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_set_model_material(ctx, e, gold, false) == PONKO_OK);
    CHECK(ponko_ctx_model_material(ctx, e, &found, &has) == PONKO_OK && !has);

    uint32_t numbers[] = {3, 0, 3};
    PonkoMaterial materials[] = {gold, wood, wood};
    CHECK(ponko_ctx_set_model_part_materials(ctx, e, numbers, materials, 3) == PONKO_OK);
    uint32_t got_parts[2]; PonkoMaterial got[2]; size_t n = 0;
    CHECK(ponko_ctx_model_part_materials(ctx, e, NULL, NULL, 0, &n) == PONKO_OK && n == 2);
    CHECK(ponko_ctx_model_part_materials(ctx, e, got_parts, got, 2, &n) == PONKO_OK && n == 2);
    CHECK(got_parts[0] == 0 && got_parts[1] == 3 && got[0].id == wood.id && got[1].id == wood.id);
    CHECK(ponko_ctx_set_model_part_materials(ctx, e, numbers, NULL, 2) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_set_model_part_materials(ctx, e, NULL, NULL, 0) == PONKO_OK);
    CHECK(ponko_ctx_model_part_materials(ctx, e, NULL, NULL, 0, &n) == PONKO_OK && n == 0);
    // Left on for the frames after setup.
    CHECK(ponko_ctx_set_model_material(ctx, e, gold, true) == PONKO_OK);
    CHECK(ponko_ctx_set_model_part_materials(ctx, e, numbers, materials, 1) == PONKO_OK);
}
static void model_material_overrides(void) {
    PonkoGame* game = ponko_game_new("overrides", 64, 64); CHECK(game);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 2, recolor, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    CHECK(ponko_ctx_set_model_material(NULL, 0, (PonkoMaterial){0}, true) == PONKO_INVALID_ARGUMENT);
}

static void lod_helpers(void) {
    float fov = 60 * 3.14159265f / 180;
    float h = ponko_lod_screen_height_at(2, 10, fov);
    CHECK(h > 0.172f && h < 0.174f);
    float d = ponko_lod_distance_at(2, h, fov);
    CHECK(d > 9.99f && d < 10.01f);
    PonkoLodRenderer r = ponko_lod_renderer((PonkoMesh){3});
    CHECK(r.mesh.id == 3 && r.color.r == 1 && r.offset.scale.y == 1);
    PonkoLodLevel level = {0.1f, 0, &r, 1, false, NULL, 0};
    CHECK(ponko_ctx_set_lod_group(NULL, 0, 1, (PonkoVec3){0, 0, 0}, &level, 1) == PONKO_INVALID_ARGUMENT);
}
static void streaming_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    CHECK(ponko_ctx_set_deterministic_uploads(ctx, true) == PONKO_OK);
    CHECK(ponko_ctx_set_upload_budget(ctx, true, 1 << 20) == PONKO_OK);
    PonkoTexture t = {0};
    bool ready = true;
    CHECK(ponko_ctx_stream_texture(ctx, "crab.png", &t, &ready) == PONKO_OK && !ready);
    uint32_t state = 0;
    CHECK(ponko_ctx_asset_state(ctx, PONKO_RESIDENCY_TEXTURE, "crab.png", &state) == PONKO_OK);
    CHECK(state == PONKO_ASSET_LOADING);
    PonkoMesh m = {0};
    CHECK(ponko_ctx_stream_mesh(ctx, "models/house.glb", &m, &ready) == PONKO_OK && !ready);
    CHECK(ponko_ctx_finish_uploads(ctx) == PONKO_OK);
    CHECK(ponko_ctx_stream_texture(ctx, "crab.png", &t, &ready) == PONKO_OK && ready && t.width > 0);
    CHECK(ponko_ctx_asset_state(ctx, PONKO_RESIDENCY_TEXTURE, "crab.png", &state) == PONKO_OK);
    CHECK(state == PONKO_ASSET_RESIDENT);
    CHECK(ponko_ctx_stream_mesh(ctx, "models/house.glb", &m, &ready) == PONKO_OK && ready);
    PonkoUploadStats uploads;
    CHECK(ponko_ctx_upload_stats(ctx, &uploads) == PONKO_OK);
    CHECK(uploads.has_budget && uploads.budget == 1 << 20 && uploads.pending == 0 && uploads.total_uploads >= 2);
    /* The versioned form: the same counters, plus the GPU's own times (none headless). */
    PonkoUploadStatsEx uploads_ex = {.struct_size = sizeof uploads_ex, .version = PONKO_UPLOAD_STATS_EX_VERSION};
    CHECK(ponko_ctx_upload_stats_ex(ctx, &uploads_ex) == PONKO_OK);
    CHECK(uploads_ex.stats.total_uploads == uploads.total_uploads && uploads_ex.stats.failures == uploads.failures);
    CHECK(!uploads_ex.gpu_upload_timing && !uploads_ex.has_gpu_created_ms && uploads_ex.gpu_created_ms == 0);
    uploads_ex.version = 0;
    CHECK(ponko_ctx_upload_stats_ex(ctx, &uploads_ex) == PONKO_INVALID_ARGUMENT);
    /* Budgets: the mesh is pinned, so a zero budget can't evict it. */
    PonkoMesh pinned = {0};
    CHECK(ponko_ctx_resident_mesh(ctx, "models/house.glb", true, &pinned) == PONKO_OK && pinned.id == m.id);
    CHECK(ponko_ctx_set_residency_budget(ctx, PONKO_RESIDENCY_MESH, true, 0) == PONKO_OK);
    PonkoResidencyStats stats;
    CHECK(ponko_ctx_residency(ctx, PONKO_RESIDENCY_MESH, &stats) == PONKO_OK);
    CHECK(stats.has_budget && stats.resident == 1 && stats.pinned == 1 && stats.over_budget_bytes > 0);
    bool resident = false;
    CHECK(ponko_ctx_is_resident(ctx, PONKO_RESIDENCY_MESH, "models/house.glb", &resident) == PONKO_OK && resident);
    CHECK(ponko_ctx_unpin_resident(ctx, PONKO_RESIDENCY_MESH, "models/house.glb") == PONKO_OK);
    PonkoTexture crab = {0};
    CHECK(ponko_ctx_resident_texture(ctx, "crab.png", false, &crab) == PONKO_OK && crab.id == t.id);
    /* A missing file fails with a message. */
    CHECK(ponko_ctx_stream_texture(ctx, "nope.png", &t, &ready) == PONKO_OK && !ready);
    CHECK(ponko_ctx_finish_uploads(ctx) == PONKO_OK);
    CHECK(ponko_ctx_asset_state(ctx, PONKO_RESIDENCY_TEXTURE, "nope.png", &state) == PONKO_OK);
    CHECK(state == PONKO_ASSET_FAILED);
    char message[256];
    size_t len = 0;
    CHECK(ponko_ctx_asset_error(ctx, PONKO_RESIDENCY_TEXTURE, "nope.png", message, sizeof message, &len) == PONKO_OK);
    CHECK(len > 0 && strstr(message, "nope.png"));
    CHECK(ponko_ctx_asset_state(ctx, 7, "nope.png", &state) == PONKO_INVALID_ARGUMENT);
    PonkoStreamedAsset assets[8] = {0};
    char names[128] = {0};
    size_t listed = 0, name_bytes = 0;
    CHECK(ponko_ctx_streamed_assets(ctx, assets, 8, names, sizeof names, &listed, &name_bytes) == PONKO_OK);
    CHECK(listed >= 1 && listed <= 8 && name_bytes <= sizeof names);
    bool found_crab = false;
    for (size_t i = 0; i < listed; ++i) {
        if (strcmp(names + assets[i].name_offset, "crab.png") == 0) {
            found_crab = true;
            CHECK(assets[i].has_handle && assets[i].id == crab.id && assets[i].state == PONKO_ASSET_RESIDENT);
        }
    }
    CHECK(found_crab);
    CHECK(ponko_ctx_mesh_costs(ctx, NULL, 0, NULL, 0, &listed, &name_bytes) == PONKO_OK);
    CHECK(listed == 0 && name_bytes == 0);
    CHECK(ponko_ctx_resident_texture(ctx, "nope.png", false, &crab) == PONKO_INVALID_ARGUMENT);
}

static void asset_streaming(void) {
    PonkoGame* game = ponko_game_new("streaming", 1, 1); CHECK(game);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 0, streaming_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
}

static void steam_messages_validation(void) {
    PonkoNetMessages* endpoint = NULL;
    PonkoSteamLobby* lobby = NULL;
    CHECK(ponko_net_messages_steam_host(480, 0, &endpoint) == PONKO_INVALID_ARGUMENT && !endpoint);
    CHECK(ponko_net_messages_steam_join(480, 0, &endpoint) == PONKO_INVALID_ARGUMENT && !endpoint);
    CHECK(ponko_steam_lobby_create(480, 3, 2, &lobby) == PONKO_INVALID_ARGUMENT && !lobby);
    CHECK(ponko_steam_lobby_join(480, 0, &lobby) == PONKO_INVALID_ARGUMENT && !lobby);
    CHECK(ponko_net_messages_steam_host(480, 2, &endpoint) != PONKO_OK && !endpoint);
    CHECK(ponko_steam_lobby_create(480, 0, 2, &lobby) != PONKO_OK && !lobby);
    CHECK(ponko_steam_lobby_poll(NULL) != PONKO_OK);
    CHECK(ponko_steam_lobby_leave(NULL) != PONKO_OK);
    CHECK(ponko_steam_lobby_set_metadata(NULL, "version", "1") != PONKO_OK);
    ponko_steam_lobby_free(NULL);
}

/* A lobby client with no server behind it: requests wait for the server's
   answer, relaying needs a room, bad arguments are errors. The full flow
   runs against a real server in crates/ponko_ffi/tests/lobby.rs. */
static void lobby_client(void) {
    PonkoLobby* lobby = NULL;
    CHECK(ponko_lobby_join("not an address", &lobby) == PONKO_INVALID_ARGUMENT && !lobby);
    CHECK(ponko_lobby_join("127.0.0.1:9", &lobby) == PONKO_OK && lobby);
    CHECK(ponko_lobby_poll(lobby) == PONKO_OK);
    CHECK(ponko_lobby_create_room(lobby, 4) == PONKO_OK);
    CHECK(ponko_lobby_join_room(lobby, "KQ7X2M") == PONKO_OK);
    CHECK(ponko_lobby_leave_room(lobby) == PONKO_OK);
    CHECK(ponko_lobby_queue(lobby, "duel", 2, 1500) == PONKO_OK);
    CHECK(ponko_lobby_cancel_queue(lobby) == PONKO_OK);
    CHECK(ponko_lobby_set_meta(lobby, "mode=duel") == PONKO_OK);
    CHECK(ponko_lobby_relay(lobby, 0, PONKO_LANE_RELIABLE, (const uint8_t*)"x", 1) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_lobby_disconnect(lobby) == PONKO_INVALID_ARGUMENT); /* never connected */
    PonkoLobbyEvent event;
    bool found = true;
    CHECK(ponko_lobby_next_event(lobby, &event, &found) == PONKO_OK && !found);
    PonkoLobbyState state;
    CHECK(ponko_lobby_state(lobby, &state) == PONKO_OK);
    CHECK(!state.connected && state.me == 0 && !state.in_room && state.code[0] == 0);
    size_t count = 7;
    CHECK(ponko_lobby_members(lobby, NULL, 0, &count) == PONKO_OK && count == 0);
    char meta[8];
    size_t len = 7;
    CHECK(ponko_lobby_room_meta(lobby, meta, sizeof meta, &len) == PONKO_OK && len == 0 && meta[0] == 0);
    ponko_lobby_free(lobby);
    ponko_lobby_free(NULL);
}

/* WebRTC endpoints signaled through a lobby client: argument checks, and a
   non-WebRTC endpoint has no lobby member behind its peers. The library is
   built either without the `webrtc` feature (the calls then fail and say so)
   or with it (CMake option PONKO_WEBRTC, which defines PONKO_WEBRTC: a lobby
   client that is not in a room still gets an endpoint, which waits for its
   signaling). The full flow runs in crates/ponko_ffi/tests/rtc.rs. */
static void rtc_endpoints(void) {
    PonkoNetMessages* endpoint = (PonkoNetMessages*)1;
    CHECK(ponko_net_messages_rtc_host(NULL, NULL, NULL, 4, &endpoint) != PONKO_OK && !endpoint);
    PonkoLobby* lobby = NULL;
    CHECK(ponko_lobby_join("127.0.0.1:9", &lobby) == PONKO_OK);
    CHECK(ponko_net_messages_rtc_join(lobby, 0, NULL, NULL, &endpoint) != PONKO_OK && !endpoint);
#ifdef PONKO_WEBRTC
    CHECK(ponko_net_messages_rtc_host(lobby, NULL, "127.0.0.1:0", 4, &endpoint) == PONKO_OK && endpoint);
    uint32_t hosted = 7;
    CHECK(ponko_net_messages_rtc_peer(endpoint, 1, &hosted) == PONKO_OK && hosted == 0);
    ponko_net_messages_free(endpoint);
    endpoint = NULL;
    CHECK(ponko_net_messages_rtc_join(lobby, 1, "", "127.0.0.1:0", &endpoint) == PONKO_OK && endpoint);
    ponko_net_messages_free(endpoint);
    endpoint = (PonkoNetMessages*)1;
    CHECK(ponko_net_messages_rtc_host(lobby, NULL, "not an address", 4, &endpoint) != PONKO_OK && !endpoint);
    CHECK(strstr(ponko_last_error(), "bind is not an address") != NULL);
#else
    CHECK(ponko_net_messages_rtc_host(lobby, NULL, NULL, 4, &endpoint) != PONKO_OK && !endpoint);
    CHECK(strstr(ponko_last_error(), "without the `webrtc` feature") != NULL);
#endif
    ponko_lobby_free(lobby);
    PonkoNetMessages* pair[2] = {NULL, NULL};
    CHECK(ponko_net_messages_loopback(2, pair) == PONKO_OK);
    uint32_t member = 7;
    CHECK(ponko_net_messages_rtc_peer(pair[0], 1, &member) == PONKO_OK && member == 0);
    CHECK(ponko_net_messages_rtc_peer(NULL, 1, &member) != PONKO_OK);
    ponko_net_messages_free(pair[0]);
    ponko_net_messages_free(pair[1]);
}

static void morph_clip_api(void) {
    PonkoClip* clip = NULL;
    CHECK(ponko_clip_new("face", &clip) == PONKO_OK);
    float times[] = {0, 2}, values[] = {0, 1};
    CHECK(ponko_clip_set_morph_track(clip, "smile", times, values, 2, 1) == PONKO_OK);
    CHECK(ponko_clip_duration(clip) == 2);
    float cubic[] = {0, 0, 1, 1, 1, 0};
    CHECK(ponko_clip_set_morph_track(clip, "smile", times, cubic, 2, 2) == PONKO_OK);
    CHECK(ponko_clip_set_morph_track(clip, "smile", times, values, 2, 9) == PONKO_INVALID_ARGUMENT);
    ponko_clip_free(clip);
    PonkoMeshData* mesh = ponko_mesh_data_plane(1);
    PonkoVec3 deltas[4] = {{0, 1, 0}, {0, 1, 0}, {0, 1, 0}, {0, 1, 0}};
    CHECK(ponko_mesh_data_add_morph_target(mesh, "full", deltas, NULL, 4) == PONKO_OK);
    CHECK(ponko_mesh_data_add_morph_target(mesh, "half", deltas, NULL, 4) == PONKO_OK);
    CHECK(ponko_mesh_data_add_morph_frame(mesh, 0, 1, 0.5f) == PONKO_OK);
    CHECK(ponko_mesh_data_add_morph_frame(mesh, 0, 0, 1) == PONKO_OK);
    CHECK(ponko_mesh_data_add_morph_frame(mesh, 0, 0, 1) == PONKO_INVALID_ARGUMENT);
    ponko_mesh_data_free(mesh);
}
/* Replication helpers: sequence numbers, authority, snapshots, interest. */
static void replication(void) {
    CHECK(ponko_seq_newer(1, 0) && ponko_seq_newer(0, 65535) && !ponko_seq_newer(7, 7));
    CHECK(!ponko_seq_newer(65535, 0) && ponko_seq_diff(1, 65535) == 2 && ponko_seq_diff(65535, 1) == -2);
    PonkoUpdateStamp old = {0, 60000}, next = {1, 0};
    CHECK(ponko_stamp_newer(next, old) && !ponko_stamp_newer(old, next) && !ponko_stamp_newer(old, old));

    PonkoAuthority a = ponko_authority_new(0);
    uint16_t epoch = 99;
    bool changed = false;
    PonkoAuthorityCheck check;
    CHECK(ponko_authority_check(&a, 0, 0, &check) == PONKO_OK && check == PONKO_AUTHORITY_ACCEPT);
    CHECK(ponko_authority_handoff(&a, 1, &epoch) == PONKO_OK && epoch == 1 && a.owner == 1);
    CHECK(ponko_authority_check(&a, 0, 0, &check) == PONKO_OK && check == PONKO_AUTHORITY_STALE);
    CHECK(ponko_authority_check(&a, 0, 1, &check) == PONKO_OK && check == PONKO_AUTHORITY_WRONG_OWNER);
    CHECK(ponko_authority_check(&a, 1, 2, &check) == PONKO_OK && check == PONKO_AUTHORITY_AHEAD);
    CHECK(ponko_authority_adopt(&a, 2, 1, &changed) == PONKO_OK && !changed && a.owner == 1); /* race: lower wins */
    CHECK(ponko_authority_adopt(&a, 3, 2, &changed) == PONKO_OK && changed && a.owner == 3 && a.epoch == 2);
    CHECK(ponko_authority_adopt(NULL, 1, 1, &changed) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_authority_check(&a, 1, 1, NULL) == PONKO_INVALID_ARGUMENT);

    PonkoLatestWins* filter = ponko_latest_wins_new();
    bool ok = false;
    PonkoUpdateStamp s1 = {0, 1}, s3 = {0, 3};
    CHECK(ponko_latest_wins_accept(filter, 9, s3, &ok) == PONKO_OK && ok);
    CHECK(ponko_latest_wins_accept(filter, 9, s1, &ok) == PONKO_OK && !ok);
    CHECK(ponko_latest_wins_accept(filter, 9, s3, &ok) == PONKO_OK && !ok);
    CHECK(ponko_latest_wins_accept(filter, 10, s1, &ok) == PONKO_OK && ok && ponko_latest_wins_len(filter) == 2);
    CHECK(ponko_latest_wins_forget(filter, 9) == PONKO_OK);
    CHECK(ponko_latest_wins_accept(filter, 9, s1, &ok) == PONKO_OK && ok);
    CHECK(ponko_latest_wins_clear(filter) == PONKO_OK && ponko_latest_wins_len(filter) == 0);
    CHECK(ponko_latest_wins_accept(NULL, 1, s1, &ok) == PONKO_INVALID_ARGUMENT);
    ponko_latest_wins_free(filter);
    ponko_latest_wins_free(NULL);

    /* A sender 5000 s ahead walks along x at 20 units/s, 20 snapshots a second. */
    PonkoSnapshotStream* stream = NULL;
    CHECK(ponko_snapshot_stream_new(NULL, &stream) == PONKO_OK && stream);
    PonkoPose pose = {0}, shown = {0};
    pose.rotation.w = 1;
    pose.has_velocity = true;
    pose.velocity.x = 20;
    PonkoSnapshotState state;
    PonkoSnapshotPush result;
    bool found = true;
    CHECK(ponko_snapshot_stream_sample(stream, 0, &shown, &state, &found) == PONKO_OK && !found);
    for (int n = 0; n < 20; n++) {
        pose.position.x = (float)n;
        CHECK(ponko_snapshot_stream_push(stream, 5000 + n * 0.05, n * 0.05 + 0.03, &pose, &result) == PONKO_OK);
        CHECK(result == PONKO_SNAPSHOT_ADDED);
    }
    CHECK(ponko_snapshot_stream_push(stream, 5000, 1.0, &pose, &result) == PONKO_OK && result == PONKO_SNAPSHOT_DUPLICATE);
    CHECK(ponko_snapshot_stream_push(stream, NAN, 1.0, &pose, &result) == PONKO_OK && result == PONKO_SNAPSHOT_INVALID);
    CHECK(ponko_snapshot_stream_len(stream) == 20);
    CHECK(ponko_snapshot_stream_sample(stream, 1.0, &shown, &state, &found) == PONKO_OK && found);
    CHECK(state == PONKO_SNAPSHOT_INTERPOLATED && shown.position.x > 17.3f && shown.position.x < 17.5f && shown.has_velocity);
    /* Packets stop: predicted for 250 ms, then held where the prediction ended. */
    CHECK(ponko_snapshot_stream_sample(stream, 1.2, &shown, &state, &found) == PONKO_OK && state == PONKO_SNAPSHOT_EXTRAPOLATED);
    CHECK(ponko_snapshot_stream_sample(stream, 5.0, &shown, &state, &found) == PONKO_OK && state == PONKO_SNAPSHOT_HELD);
    CHECK(shown.position.x > 23.9f && shown.position.x < 24.1f); /* 19 + 0.25 s * 20 */
    double offset = 0;
    CHECK(ponko_snapshot_stream_clock_offset(stream, &offset, &found) == PONKO_OK && found && offset < -4999.9 && offset > -5000.0);
    CHECK(ponko_snapshot_stream_set_delay(stream, 0.0) == PONKO_OK);
    CHECK(ponko_snapshot_stream_set_delay(stream, -1.0) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_snapshot_stream_clear(stream) == PONKO_OK && ponko_snapshot_stream_len(stream) == 0);
    CHECK(ponko_snapshot_stream_sample(stream, 1.0, &shown, NULL, &found) == PONKO_OK && !found);
    CHECK(ponko_snapshot_stream_reset(stream) == PONKO_OK);
    CHECK(ponko_snapshot_stream_clock_offset(stream, &offset, &found) == PONKO_OK && !found);
    CHECK(ponko_snapshot_stream_push(stream, 1, 1, NULL, &result) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_snapshot_stream_sample(NULL, 1.0, &shown, &state, &found) == PONKO_INVALID_ARGUMENT);
    ponko_snapshot_stream_free(stream);
    ponko_snapshot_stream_free(NULL);
    PonkoSnapshotConfig bad = ponko_snapshot_config_default();
    bad.capacity = 1;
    CHECK(ponko_snapshot_stream_new(&bad, &stream) == PONKO_INVALID_ARGUMENT);
    bad = ponko_snapshot_config_default();
    bad.delay = -1;
    CHECK(ponko_snapshot_stream_new(&bad, &stream) == PONKO_INVALID_ARGUMENT);

    PonkoInterest* interest = NULL;
    PonkoInterestConfig config = ponko_interest_config_default();
    CHECK(config.tier_count == 2 && config.tiers[0].radius == 40 && config.tiers[1].every == 4);
    CHECK(ponko_interest_new(&config, &interest) == PONKO_OK);
    PonkoInterestViewer viewer = {1, {0, 0, 0}};
    PonkoInterestEntity world[3] = {{10, {10, 0, 0}}, {20, {100, 0, 0}}, {30, {900, 0, 0}}};
    CHECK(ponko_interest_update(interest, 0, &viewer, 1, world, 3) == PONKO_OK);
    PonkoInterestEntry entries[4];
    size_t count = 0;
    CHECK(ponko_interest_entries(interest, 1, entries, 4, &count) == PONKO_OK && count == 2);
    CHECK(entries[0].entity == 10 && entries[0].tier == 0 && entries[0].send);
    CHECK(entries[1].entity == 20 && entries[1].tier == 1 && entries[1].send && entries[1].distance == 100);
    PonkoInterestChange changes[4];
    CHECK(ponko_interest_changes(interest, changes, 4, &count) == PONKO_OK && count == 2);
    CHECK(changes[0].kind == PONKO_INTEREST_ENTERED && changes[0].peer == 1 && changes[0].entity == 10);
    /* The mid entity wobbles across the edge of the near ring (40): hysteresis keeps it in its ring. */
    for (uint64_t tick = 1; tick < 20; tick++) {
        world[1].position.x = 40 + (tick % 2 ? 3.0f : -3.0f);
        CHECK(ponko_interest_update(interest, tick, &viewer, 1, world, 2) == PONKO_OK);
        CHECK(ponko_interest_entries(interest, 1, entries, 4, &count) == PONKO_OK && count == 2 && entries[1].tier == 1);
        CHECK(ponko_interest_changes(interest, changes, 4, &count) == PONKO_OK && count == 0);
    }
    world[1].position.x = 500;
    CHECK(ponko_interest_update(interest, 20, &viewer, 1, world, 2) == PONKO_OK);
    CHECK(ponko_interest_changes(interest, changes, 4, &count) == PONKO_OK && count == 1);
    CHECK(changes[0].kind == PONKO_INTEREST_LEFT && changes[0].entity == 20);
    CHECK(ponko_interest_entries(interest, 99, NULL, 0, &count) == PONKO_OK && count == 0);
    CHECK(ponko_interest_remove_peer(interest, 1) == PONKO_OK);
    CHECK(ponko_interest_update(interest, 21, NULL, 0, NULL, 0) == PONKO_OK);
    CHECK(ponko_interest_update(interest, 21, NULL, 1, NULL, 0) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_interest_entries(NULL, 1, NULL, 0, &count) == PONKO_INVALID_ARGUMENT);
    ponko_interest_free(interest);
    interest = NULL;
    ponko_interest_free(NULL);
    config.tiers[1].radius = 10; /* not increasing */
    CHECK(ponko_interest_new(&config, &interest) == PONKO_INVALID_ARGUMENT && !interest);
    config = ponko_interest_config_default();
    config.tier_count = 0;
    CHECK(ponko_interest_new(&config, &interest) == PONKO_INVALID_ARGUMENT);
    config.tier_count = 9;
    CHECK(ponko_interest_new(&config, &interest) == PONKO_INVALID_ARGUMENT);
    config = ponko_interest_config_default();
    config.hysteresis = 40;
    CHECK(ponko_interest_new(&config, &interest) == PONKO_INVALID_ARGUMENT);
}

typedef struct { size_t rows; uint64_t last; } Rows;
static void count_rows(const PonkoChunk* chunk, void* user) {
    Rows* r = user;
    r->rows += chunk->count;
    if (chunk->count) r->last = ponko_entity_bits(chunk->entities[chunk->count - 1]);
}
static void bump(const PonkoChunk* chunk, void* user) {
    (void)user;
    for (size_t i = 0; i < chunk->count; ++i) ((Position*)chunk->columns[0])[i].x += 1;
}
static size_t rows_of(PonkoWorld* w, uint32_t id, PonkoQueryOptions options, uint64_t* last) {
    Rows rows = {0, 0};
    CHECK(ponko_query_with(w, &id, 1, &options, count_rows, &rows) == PONKO_OK);
    if (last) *last = rows.last;
    return rows.rows;
}
static void change_detection(void) {
    PonkoWorld* w = ponko_world_new(); CHECK(w);
    uint32_t p, other;
    CHECK(ponko_component_register(w, "Pos", sizeof(Position), _Alignof(Position), &p) == PONKO_OK);
    CHECK(ponko_component_register(w, "Other", sizeof(Position), _Alignof(Position), &other) == PONKO_OK);
    CHECK(ponko_component_track_changes(w, p) == PONKO_OK);
    CHECK(ponko_component_track_changes(w, 99) == PONKO_INVALID_ARGUMENT);
    Position value = {1}; PonkoValue pos = {p, &value, sizeof(value)};
    uint64_t a, b, c, seen, tick, last;
    CHECK(ponko_spawn(w, &pos, 1, &a) == PONKO_OK && ponko_spawn(w, &pos, 1, &b) == PONKO_OK);
    CHECK(ponko_world_advance_change_tick(w, &seen) == PONKO_OK && seen == 1);
    CHECK(ponko_world_change_tick(w, &tick) == PONKO_OK && tick == 2);

    // The defaults are ponko_query: every column may be written.
    PonkoQueryOptions read = ponko_query_options_default();
    CHECK(read.writes == UINT32_MAX && !read.changed && !read.added && !read.any && !read.parallel);
    CHECK(read.since == PONKO_SINCE_LAST_RUN);
    read.writes = 0;
    PonkoQueryOptions changed = read; changed.changed = 1; changed.since = seen;
    PonkoQueryOptions added = read; added.added = 1; added.since = seen;
    // A read-only query changes nothing.
    CHECK(rows_of(w, p, read, NULL) == 2 && rows_of(w, p, changed, NULL) == 0);
    // set and mark_changed do.
    value.x = 5; CHECK(ponko_set(w, b, p, &value, sizeof(value)) == PONKO_OK);
    CHECK(rows_of(w, p, changed, &last) == 1 && last == b && rows_of(w, p, added, NULL) == 0);
    CHECK(ponko_mark_changed(w, a, p) == PONKO_OK && rows_of(w, p, changed, NULL) == 2);
    CHECK(ponko_mark_changed(w, a, other) == PONKO_NOT_FOUND);
    CHECK(ponko_mark_changed(w, a, 99) == PONKO_INVALID_ARGUMENT);
    // The plain query marks every row it visits; a new entity is added.
    CHECK(ponko_world_advance_change_tick(w, &seen) == PONKO_OK);
    changed.since = added.since = seen;
    CHECK(rows_of(w, p, changed, NULL) == 0);
    CHECK(ponko_query(w, &p, 1, bump, NULL) == PONKO_OK && rows_of(w, p, changed, NULL) == 2);
    CHECK(ponko_spawn(w, &pos, 1, &c) == PONKO_OK);
    CHECK(rows_of(w, p, added, &last) == 1 && last == c && rows_of(w, p, changed, NULL) == 3);
    // Writing through a filtered query, on all cores.
    PonkoQueryOptions write_new = added; write_new.writes = 1; write_new.parallel = true;
    CHECK(ponko_query_with(w, &p, 1, &write_new, bump, NULL) == PONKO_OK);
    Position out;
    CHECK(ponko_get(w, c, p, &out, sizeof(out)) == PONKO_OK && out.x == 6);
    CHECK(ponko_get(w, a, p, &out, sizeof(out)) == PONKO_OK && out.x == 2);
    // PONKO_SINCE_LAST_RUN reads the world's own tick (a game sets it per system).
    changed.since = PONKO_SINCE_LAST_RUN;
    CHECK(rows_of(w, p, changed, NULL) == 3);
    CHECK(ponko_world_advance_change_tick(w, &seen) == PONKO_OK);
    CHECK(ponko_world_set_changes_since(w, seen) == PONKO_OK);
    CHECK(ponko_world_changes_since(w, &tick) == PONKO_OK && tick == seen);
    CHECK(rows_of(w, p, changed, NULL) == 0);
    // Removals: a removed component and a despawned entity.
    uint64_t gone[2] = {0, 0}; size_t n = 9;
    CHECK(ponko_removed(w, p, seen, NULL, 0, &n) == PONKO_OK && n == 0);
    CHECK(ponko_remove(w, a, p) == PONKO_OK && ponko_despawn(w, b) == PONKO_OK);
    CHECK(ponko_removed(w, p, PONKO_SINCE_LAST_RUN, gone, 1, &n) == PONKO_OK && n == 2 && gone[0] == a && gone[1] == 0);
    CHECK(ponko_removed(w, p, seen, gone, 2, &n) == PONKO_OK && n == 2 && gone[1] == b);
    CHECK(ponko_removed(w, other, 0, gone, 2, &n) == PONKO_INVALID_ARGUMENT);   // not tracked
    CHECK(ponko_removed(w, p, 0, NULL, 2, &n) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_query_with(w, &p, 1, &read, NULL, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_query_with(w, &p, 1, NULL, count_rows, &(Rows){0, 0}) == PONKO_OK);
    CHECK(ponko_world_change_tick(w, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_world_free(w) == PONKO_OK);
}

/* Query filters: with, without, OR over arms, AND groups inside an OR. One
 * entity per combination of four markers; bit i of an entity's label says
 * whether it has marker i, so every filter has a plain C predicate to compare
 * against. */
enum { PLAYER_BIT = 1, ENEMY_BIT = 2, DEAD_BIT = 4, GHOST_BIT = 8 };
typedef struct { uint32_t mask; size_t rows; } Labels;
static void collect_labels(const PonkoChunk* chunk, void* user) {
    Labels* l = user;
    const uint32_t* label = chunk->columns[0];
    for (size_t i = 0; i < chunk->count; ++i) { l->mask |= 1u << label[i]; ++l->rows; }
}
static uint32_t filtered_labels(PonkoWorld* w, uint32_t label, const PonkoFilter* filter, PonkoStatus* status) {
    Labels l = {0, 0};
    PonkoQueryOptions options = ponko_query_options_default();
    options.writes = 0;
    *status = ponko_query_filtered(w, &label, 1, filter, &options, collect_labels, &l);
    return l.mask;
}
/* A parallel callback fills row i of the query into slot i: no two threads share one. */
static void fill_slots(const PonkoChunk* chunk, void* user) {
    uint32_t* slots = user;
    const uint32_t* label = chunk->columns[0];
    for (size_t i = 0; i < chunk->count; ++i) slots[chunk->first + i] = label[i];
}
static uint32_t labels_where(int (*predicate)(unsigned)) {
    uint32_t mask = 0;
    for (unsigned bits = 0; bits < 16; ++bits) if (predicate(bits)) mask |= 1u << bits;
    return mask;
}
static int is_player(unsigned b) { return b & PLAYER_BIT; }
static int not_dead(unsigned b) { return !(b & DEAD_BIT); }
static int player_alive(unsigned b) { return (b & PLAYER_BIT) && !(b & DEAD_BIT); }
static int player_or_enemy(unsigned b) { return b & (PLAYER_BIT | ENEMY_BIT); }
static int living_fighter(unsigned b) { return player_or_enemy(b) && !(b & DEAD_BIT); }
static int both_or_alive(unsigned b) { return ((b & PLAYER_BIT) && (b & ENEMY_BIT)) || !(b & DEAD_BIT); }
static int nested(unsigned b) { return ((b & PLAYER_BIT) && ((b & DEAD_BIT) || ((b & GHOST_BIT) && !(b & ENEMY_BIT)))) || (b & ENEMY_BIT); }
static int anything(unsigned b) { (void)b; return 1; }
static int nothing(unsigned b) { (void)b; return 0; }
static void check_filter(PonkoWorld* w, uint32_t label, const PonkoFilter* filter, int (*predicate)(unsigned)) {
    PonkoStatus status;
    CHECK(ponko_filter_check(w, filter) == PONKO_OK);
    uint32_t want = labels_where(predicate);
    CHECK(filtered_labels(w, label, filter, &status) == want && status == PONKO_OK);
    /* The same rows on all cores. */
    uint32_t slots[16], rows = 0;
    for (int i = 0; i < 16; ++i) slots[i] = UINT32_MAX;
    PonkoQueryOptions par = ponko_query_options_default();
    par.writes = 0; par.parallel = true;
    CHECK(ponko_query_filtered(w, &label, 1, filter, &par, fill_slots, slots) == PONKO_OK);
    uint32_t seen = 0;
    for (int i = 0; i < 16; ++i) if (slots[i] != UINT32_MAX) { seen |= 1u << slots[i]; ++rows; }
    uint32_t count = 0;
    for (uint32_t m = want; m; m &= m - 1) ++count;
    CHECK(seen == want && rows == count);
}
static void query_filters(void) {
    PonkoWorld* w = ponko_world_new(); CHECK(w);
    const char* names[] = {"Player", "Enemy", "Dead", "Ghost"};
    uint32_t ids[4], label;
    for (int i = 0; i < 4; ++i) CHECK(ponko_component_register(w, names[i], sizeof(uint32_t), _Alignof(uint32_t), &ids[i]) == PONKO_OK);
    CHECK(ponko_component_register(w, "Label", sizeof(uint32_t), _Alignof(uint32_t), &label) == PONKO_OK);
    for (uint32_t bits = 0; bits < 16; ++bits) {
        PonkoValue values[5] = {{label, &bits, sizeof bits}};
        size_t n = 1;
        for (int i = 0; i < 4; ++i) if (bits >> i & 1) values[n++] = (PonkoValue){ids[i], &bits, sizeof bits};
        uint64_t e;
        CHECK(ponko_spawn(w, values, n, &e) == PONKO_OK);
    }
    PonkoFilter* player = ponko_filter_with(ids[0]);
    PonkoFilter* enemy = ponko_filter_with(ids[1]);
    PonkoFilter* dead = ponko_filter_with(ids[2]);
    PonkoFilter* ghost = ponko_filter_with(ids[3]);
    PonkoFilter* alive = ponko_filter_without(ids[2]);
    PonkoFilter* not_enemy = ponko_filter_without(ids[1]);
    CHECK(player && enemy && dead && ghost && alive && not_enemy);

    check_filter(w, label, player, is_player);
    check_filter(w, label, alive, not_dead);
    const PonkoFilter* player_alive_parts[] = {player, alive};
    PonkoFilter* player_alive_filter = ponko_filter_all(player_alive_parts, 2);
    check_filter(w, label, player_alive_filter, player_alive);
    const PonkoFilter* fighter_parts[] = {player, enemy};
    PonkoFilter* fighter = ponko_filter_any(fighter_parts, 2);
    check_filter(w, label, fighter, player_or_enemy);
    /* (Player OR Enemy) AND NOT Dead: two filters at once. */
    const PonkoFilter* living_parts[] = {fighter, alive};
    PonkoFilter* living = ponko_filter_all(living_parts, 2);
    check_filter(w, label, living, living_fighter);
    /* AND group inside an OR: (Player AND Enemy) OR NOT Dead. */
    const PonkoFilter* both_parts[] = {player, enemy};
    PonkoFilter* both = ponko_filter_all(both_parts, 2);
    const PonkoFilter* either_parts[] = {both, alive};
    PonkoFilter* either = ponko_filter_any(either_parts, 2);
    check_filter(w, label, either, both_or_alive);
    /* Nesting: (Player AND (Dead OR (Ghost AND NOT Enemy))) OR Enemy. */
    const PonkoFilter* ghost_parts[] = {ghost, not_enemy};
    PonkoFilter* ghost_alone = ponko_filter_all(ghost_parts, 2);
    const PonkoFilter* dead_or_parts[] = {dead, ghost_alone};
    PonkoFilter* dead_or_ghost = ponko_filter_any(dead_or_parts, 2);
    const PonkoFilter* player_arm_parts[] = {player, dead_or_ghost};
    PonkoFilter* player_arm = ponko_filter_all(player_arm_parts, 2);
    const PonkoFilter* nested_parts[] = {player_arm, enemy};
    PonkoFilter* nested_filter = ponko_filter_any(nested_parts, 2);
    check_filter(w, label, nested_filter, nested);
    /* Freeing the parts of a filter doesn't change it. */
    ponko_filter_free(player_arm); ponko_filter_free(dead_or_ghost); ponko_filter_free(ghost_alone);
    check_filter(w, label, nested_filter, nested);
    /* always matches everything, never nothing; null is no filter. */
    PonkoFilter* always = ponko_filter_always();
    PonkoFilter* never = ponko_filter_never();
    check_filter(w, label, always, anything);
    check_filter(w, label, never, nothing);
    PonkoStatus status;
    CHECK(filtered_labels(w, label, NULL, &status) == 0xFFFFu && status == PONKO_OK);
    CHECK(ponko_query_filtered(w, &label, 1, living, NULL, NULL, NULL) == PONKO_INVALID_ARGUMENT);
    /* A filter is checked in the world that runs it. */
    PonkoFilter* unknown = ponko_filter_with(9999);
    CHECK(unknown && ponko_filter_check(w, unknown) == PONKO_INVALID_ARGUMENT);
    CHECK(strstr(ponko_last_error(), "not registered") != NULL);
    CHECK(filtered_labels(w, label, unknown, &status) == 0 && status == PONKO_INVALID_ARGUMENT);
    /* Building errors are null: nothing to combine, a null part. */
    CHECK(ponko_filter_all(NULL, 0) == NULL && ponko_filter_any(NULL, 0) == NULL);
    const PonkoFilter* with_null[] = {player, NULL};
    CHECK(ponko_filter_any(with_null, 2) == NULL);
    /* A repeat is one slot and a contradiction matches nothing, as for a Rust tuple. */
    const PonkoFilter* contradiction[] = {player, ponko_filter_without(ids[0])};
    PonkoFilter* contradictory = ponko_filter_all(contradiction, 2);
    CHECK(contradictory);
    check_filter(w, label, contradictory, nothing);
    const PonkoFilter* repeat[] = {player, player};
    PonkoFilter* repeated = ponko_filter_all(repeat, 2);
    CHECK(repeated);
    check_filter(w, label, repeated, is_player);
    ponko_filter_free(contradictory); ponko_filter_free(repeated);
    ponko_filter_free((PonkoFilter*)contradiction[1]);
    /* Limits are errors too: 9 with components (8 are fine) and 5 `any` clauses (4 are). */
    PonkoFilter* many[9];
    for (int i = 0; i < 9; ++i) {
        char name[16]; uint32_t id;
        snprintf(name, sizeof name, "Extra%d", i);
        CHECK(ponko_component_register(w, name, 4, 4, &id) == PONKO_OK);
        many[i] = ponko_filter_with(id);
    }
    const PonkoFilter* nine_parts[9];
    for (int i = 0; i < 9; ++i) nine_parts[i] = many[i];
    PonkoFilter* eight = ponko_filter_all(nine_parts, 8);
    PonkoFilter* nine = ponko_filter_all(nine_parts, 9);
    CHECK(eight && nine && ponko_filter_check(w, eight) == PONKO_OK && ponko_filter_check(w, nine) == PONKO_INVALID_ARGUMENT);
    CHECK(strstr(ponko_last_error(), "at most 8 `with`") != NULL);
    PonkoFilter* clauses[5];
    for (int i = 0; i < 5; ++i) {
        const PonkoFilter* one[] = {many[i]};
        clauses[i] = ponko_filter_any(one, 1);
    }
    const PonkoFilter* clause_parts[5] = {clauses[0], clauses[1], clauses[2], clauses[3], clauses[4]};
    PonkoFilter* four_clauses = ponko_filter_all(clause_parts, 4);
    PonkoFilter* five_clauses = ponko_filter_all(clause_parts, 5);
    CHECK(four_clauses && five_clauses);
    CHECK(ponko_filter_check(w, four_clauses) == PONKO_OK && ponko_filter_check(w, five_clauses) == PONKO_INVALID_ARGUMENT);
    CHECK(strstr(ponko_last_error(), "at most 4 `Or`") != NULL);
    ponko_filter_free(four_clauses); ponko_filter_free(five_clauses);
    for (int i = 0; i < 5; ++i) ponko_filter_free(clauses[i]);
    ponko_filter_free(eight); ponko_filter_free(nine);
    for (int i = 0; i < 9; ++i) ponko_filter_free(many[i]);
    ponko_filter_free(unknown); ponko_filter_free(never); ponko_filter_free(always);
    ponko_filter_free(nested_filter); ponko_filter_free(either); ponko_filter_free(both);
    ponko_filter_free(living); ponko_filter_free(fighter); ponko_filter_free(player_alive_filter);
    ponko_filter_free(not_enemy); ponko_filter_free(alive); ponko_filter_free(ghost);
    ponko_filter_free(dead); ponko_filter_free(enemy); ponko_filter_free(player);
    ponko_filter_free(NULL);
    CHECK(ponko_world_free(w) == PONKO_OK);
}

/* Wide queries: a C query takes 32 columns (Rust tuples hold 12; nest them for more), also with a filter. */
static void count_wide(const PonkoChunk* chunk, void* user) {
    CHECK(chunk->column_count == 32);
    for (size_t column = 0; column < 32; ++column)
        for (size_t i = 0; i < chunk->count; ++i) CHECK(((const uint32_t*)chunk->columns[column])[i] == column);
    *(size_t*)user += chunk->count;
}
static void wide_queries(void) {
    PonkoWorld* w = ponko_world_new(); CHECK(w);
    uint32_t ids[33], values[33];
    PonkoValue spawn[33];
    for (uint32_t i = 0; i < 33; ++i) {
        char name[16];
        snprintf(name, sizeof name, "Wide%u", i);
        CHECK(ponko_component_register(w, name, sizeof(uint32_t), _Alignof(uint32_t), &ids[i]) == PONKO_OK);
        values[i] = i;
        spawn[i] = (PonkoValue){ids[i], &values[i], sizeof(uint32_t)};
    }
    uint64_t e;
    CHECK(ponko_spawn(w, spawn, 32, &e) == PONKO_OK && ponko_spawn(w, spawn, 33, &e) == PONKO_OK);
    size_t rows = 0;
    CHECK(ponko_query(w, ids, 32, count_wide, &rows) == PONKO_OK && rows == 2);
    CHECK(ponko_query(w, ids, 33, count_wide, &rows) == PONKO_INVALID_ARGUMENT);
    /* The 33rd component is a filter, not a column. */
    PonkoFilter* last = ponko_filter_with(ids[32]);
    rows = 0;
    CHECK(ponko_query_filtered(w, ids, 32, last, NULL, count_wide, &rows) == PONKO_OK && rows == 1);
    CHECK(ponko_query_filtered(w, ids, 33, last, NULL, count_wide, &rows) == PONKO_INVALID_ARGUMENT);
    ponko_filter_free(last);
    CHECK(ponko_world_free(w) == PONKO_OK);
}

/* The archetype index: 128 archetypes (one per subset of seven markers), so
 * the default policy applies. The per-query hint, the world's off switch and
 * the automatic policy all visit the same rows. */
static uint32_t cache_slots[256];
static uint32_t cached_rows(PonkoWorld* w, uint32_t label, const PonkoFilter* filter, uint32_t cache, bool parallel) {
    Rows rows = {0, 0};
    PonkoQueryOptions options = ponko_query_options_default();
    options.writes = 0; options.parallel = parallel;
    /* A parallel callback may not share a counter: count on one thread, and
     * only check that the parallel query runs. */
    if (parallel) { CHECK(ponko_query_cached(w, &label, 1, filter, &options, cache, fill_slots, cache_slots) == PONKO_OK); return 0; }
    CHECK(ponko_query_cached(w, &label, 1, filter, &options, cache, count_rows, &rows) == PONKO_OK);
    return (uint32_t)rows.rows;
}
static void query_cache(void) {
    PonkoWorld* w = ponko_world_new(); CHECK(w);
    uint32_t markers[7], label;
    char name[16];
    for (int i = 0; i < 7; ++i) {
        snprintf(name, sizeof name, "Marker%d", i);
        CHECK(ponko_component_register(w, name, sizeof(uint32_t), _Alignof(uint32_t), &markers[i]) == PONKO_OK);
    }
    CHECK(ponko_component_register(w, "Label", sizeof(uint32_t), _Alignof(uint32_t), &label) == PONKO_OK);
    for (uint32_t bits = 0; bits < 128; ++bits) {
        PonkoValue values[8] = {{label, &bits, sizeof bits}};
        size_t n = 1;
        for (int i = 0; i < 7; ++i) if (bits >> i & 1) values[n++] = (PonkoValue){markers[i], &bits, sizeof bits};
        uint64_t e;
        CHECK(ponko_spawn(w, values, n, &e) == PONKO_OK);
    }
    /* Marker 0 and marker 1, without marker 2: 16 of the 128. */
    PonkoFilter* a = ponko_filter_with(markers[0]);
    PonkoFilter* b = ponko_filter_with(markers[1]);
    PonkoFilter* c = ponko_filter_without(markers[2]);
    const PonkoFilter* parts[] = {a, b, c};
    PonkoFilter* filter = ponko_filter_all(parts, 3);
    CHECK(a && b && c && filter);

    PonkoQueryCacheStats stats;
    CHECK(ponko_world_query_cache_stats(w, &stats) == PONKO_OK && stats.signatures == 0 && stats.hits == 0);
    /* Never: the index is not touched. */
    CHECK(cached_rows(w, label, filter, PONKO_QUERY_CACHE_NEVER, false) == 16);
    CHECK(ponko_world_query_cache_stats(w, &stats) == PONKO_OK && stats.signatures == 0);
    /* Automatic, through the ordinary query function: indexed from its second run. */
    Rows rows = {0, 0};
    for (int i = 0; i < 3; ++i) CHECK(ponko_query_filtered(w, &label, 1, filter, NULL, count_rows, &rows) == PONKO_OK);
    CHECK(rows.rows == 3 * 16);
    CHECK(ponko_world_query_cache_stats(w, &stats) == PONKO_OK);
    CHECK(stats.signatures == 1 && stats.indexed == 1 && stats.indices == 16 && stats.hits == 2 && stats.bytes > 0);
    /* Always: a query the policy would scan (it matches every archetype) is indexed at once. */
    CHECK(cached_rows(w, label, NULL, PONKO_QUERY_CACHE_AUTO, false) == 128);
    CHECK(cached_rows(w, label, NULL, PONKO_QUERY_CACHE_AUTO, false) == 128);
    CHECK(ponko_world_query_cache_stats(w, &stats) == PONKO_OK && stats.signatures == 2 && stats.indexed == 1);
    CHECK(cached_rows(w, label, NULL, PONKO_QUERY_CACHE_ALWAYS, false) == 128);
    CHECK(ponko_world_query_cache_stats(w, &stats) == PONKO_OK && stats.indexed == 2 && stats.indices == 16 + 128);
    /* The same rows, in the same order, on all cores. */
    for (int i = 0; i < 128; ++i) cache_slots[i] = UINT32_MAX;
    cached_rows(w, label, filter, PONKO_QUERY_CACHE_ALWAYS, true);
    uint32_t expect = 0, filled = 0;
    for (uint32_t bits = 0; bits < 128; ++bits) {
        if ((bits & 7) != 3) continue;
        ++filled;
    }
    for (int i = 0; i < 128; ++i) if (cache_slots[i] != UINT32_MAX) { CHECK((cache_slots[i] & 7) == 3); ++expect; }
    CHECK(expect == filled && filled == 16);
    /* A new archetype is listed before the next query. */
    uint32_t extra, value = 3;
    CHECK(ponko_component_register(w, "Extra", sizeof(uint32_t), _Alignof(uint32_t), &extra) == PONKO_OK);
    PonkoValue grown[4] = {{label, &value, sizeof value}, {markers[0], &value, sizeof value}, {markers[1], &value, sizeof value}, {extra, &value, sizeof value}};
    uint64_t e;
    CHECK(ponko_spawn(w, grown, 4, &e) == PONKO_OK);
    CHECK(cached_rows(w, label, filter, PONKO_QUERY_CACHE_AUTO, false) == 17);
    CHECK(cached_rows(w, label, filter, PONKO_QUERY_CACHE_NEVER, false) == 17);

    /* The policy: read, change, and turn the index off for the world. */
    PonkoQueryCachePolicy policy, off = ponko_query_cache_policy_off(), standard = ponko_query_cache_policy_default();
    CHECK(ponko_world_query_cache_policy(w, &policy) == PONKO_OK);
    CHECK(policy.min_archetypes == standard.min_archetypes && policy.promote_after == standard.promote_after);
    CHECK(off.min_archetypes == SIZE_MAX && standard.min_archetypes < 128);
    CHECK(ponko_world_set_query_cache_policy(w, &off) == PONKO_OK);
    CHECK(ponko_world_query_cache_policy(w, &policy) == PONKO_OK && policy.min_archetypes == SIZE_MAX);
    CHECK(ponko_world_query_cache_stats(w, &stats) == PONKO_OK && stats.signatures == 0 && stats.bytes == 0);
    uint64_t hits = stats.hits;
    CHECK(cached_rows(w, label, filter, PONKO_QUERY_CACHE_ALWAYS, false) == 17);
    CHECK(cached_rows(w, label, filter, PONKO_QUERY_CACHE_AUTO, false) == 17);
    CHECK(ponko_world_query_cache_stats(w, &stats) == PONKO_OK && stats.signatures == 0 && stats.hits == hits);
    standard.promote_after = 1;
    CHECK(ponko_world_set_query_cache_policy(w, &standard) == PONKO_OK);
    CHECK(cached_rows(w, label, filter, PONKO_QUERY_CACHE_AUTO, false) == 17);
    CHECK(ponko_world_query_cache_stats(w, &stats) == PONKO_OK && stats.indexed == 1 && stats.hits == hits + 1);

    /* Bad arguments. */
    standard.reserved[0] = 1;
    CHECK(ponko_world_set_query_cache_policy(w, &standard) == PONKO_INVALID_ARGUMENT);
    standard.reserved[0] = 0;
    standard.max_match_percent = 101;
    CHECK(ponko_world_set_query_cache_policy(w, &standard) == PONKO_OK);
    CHECK(ponko_world_query_cache_policy(w, &policy) == PONKO_OK && policy.max_match_percent == 100);
    CHECK(ponko_world_set_query_cache_policy(w, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_world_set_query_cache_policy(NULL, &off) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_world_query_cache_policy(w, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_world_query_cache_stats(w, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_world_query_cache_stats(NULL, &stats) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_query_cached(w, &label, 1, filter, NULL, 3, count_rows, &rows) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_query_cached(w, &label, 1, filter, NULL, PONKO_QUERY_CACHE_ALWAYS, NULL, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_query_cached(NULL, &label, 1, filter, NULL, PONKO_QUERY_CACHE_ALWAYS, count_rows, &rows) == PONKO_INVALID_ARGUMENT);

    ponko_filter_free(a); ponko_filter_free(b); ponko_filter_free(c); ponko_filter_free(filter);
    CHECK(ponko_world_free(w) == PONKO_OK);
}

/* Headless: no renderer, so nothing is uploaded to the GPU. */
static void no_uploads(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoGpuUploads u = {7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7, 7};
    CHECK(ponko_ctx_gpu_uploads(ctx, &u) == PONKO_OK);
    CHECK(u.bytes == 0 && u.instances == 0 && u.uniforms == 0 && u.buffers_created == 0 && u.reallocations == 0);
    CHECK(ponko_ctx_gpu_uploads(ctx, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_gpu_uploads(NULL, &u) == PONKO_INVALID_ARGUMENT);
}

static void profiling_c(void) {
    const PonkoProfileScope* scope = ponko_profile_scope("c scope");
    CHECK(scope && scope == ponko_profile_scope("c scope"));
    CHECK(ponko_profile_scope(NULL) == NULL);
    ponko_profile_end(scope, ponko_profile_begin(scope));
    ponko_note_allocation(16);
    PonkoGame* game = ponko_game_new("c perf", 1, 1); CHECK(game);
    PonkoWorld* world = NULL;
    CHECK(ponko_game_profile_headless(game, 2, 10, NULL, NULL, &world) == PONKO_OK && world);
    CHECK(ponko_world_free(world) == PONKO_OK);
    size_t len = 0;
    CHECK(ponko_last_perf_report(NULL, 0, &len) == PONKO_OK && len > 10);
    char* json = malloc(len + 1);
    CHECK(ponko_last_perf_report(json, len + 1, &len) == PONKO_OK && json[0] == '{');
    size_t problems = 1;
    CHECK(ponko_perf_check(json, "{\"frame_p99_ms\": 100000}", NULL, 0, &problems) == PONKO_OK && problems == 0);
    CHECK(ponko_perf_check(json, "{\"systems\": {\"missing\": 1}}", NULL, 0, &problems) == PONKO_OK && problems > 0);
    CHECK(ponko_perf_report_text("nope", NULL, 0, &problems) == PONKO_INVALID_ARGUMENT);
    free(json);
    PonkoGame* quiet = ponko_game_new("c uploads", 1, 1); CHECK(quiet);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(quiet, 0, no_uploads, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
}

/* A two-joint skin with one clip, "rise": the arm goes up 2 over 1 second. */
static const char RISE_GLTF[] =
    "{\"asset\": {\"version\": \"2.0\"}, \"scene\": 0, \"scenes\": [{\"nodes\": [0]}],"
    "\"nodes\": [{\"name\": \"root\", \"children\": [1]}, {\"name\": \"arm\", \"mesh\": 0}],"
    "\"meshes\": [{\"primitives\": [{\"attributes\": {\"POSITION\": 2}}]}],"
    "\"skins\": [{\"joints\": [0, 1]}],"
    "\"animations\": [{\"name\": \"rise\", \"samplers\": [{\"input\": 0, \"output\": 1}],"
    "\"channels\": [{\"sampler\": 0, \"target\": {\"node\": 1, \"path\": \"translation\"}}]}],"
    "\"buffers\": [{\"byteLength\": 68, \"uri\": \"data:application/octet-stream;base64,"
    "AAAAAAAAgD8AAAAAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAAAAAAIA/AAAAAAAAAAAAAAAAAACAPwAAAAA=\"}],"
    "\"bufferViews\": [{\"buffer\": 0, \"byteLength\": 8}, {\"buffer\": 0, \"byteOffset\": 8, \"byteLength\": 24},"
    "{\"buffer\": 0, \"byteOffset\": 32, \"byteLength\": 36}],"
    "\"accessors\": [{\"bufferView\": 0, \"componentType\": 5126, \"count\": 2, \"type\": \"SCALAR\", \"min\": [0], \"max\": [1]},"
    "{\"bufferView\": 1, \"componentType\": 5126, \"count\": 2, \"type\": \"VEC3\"},"
    "{\"bufferView\": 2, \"componentType\": 5126, \"count\": 3, \"type\": \"VEC3\", \"min\": [0, 0, 0], \"max\": [1, 1, 0]}]}";

static const char* temp_dir(void) {
    const char* dir = getenv("TMPDIR");
    if (!dir) dir = getenv("TEMP");
    if (!dir) dir = getenv("TMP");
    return dir ? dir : "/tmp";
}

typedef struct { char path[1024]; } AnimatorRun;

/* A graph with a float parameter, a resting state and a "rise" state with an
   event half way, then an animator on a skeleton, run with manual updates. */
static void animator_setup(PonkoCtx* ctx, void* user) {
    const AnimatorRun* run = user;
    PonkoClip* rise = NULL;
    CHECK(ponko_ctx_load_clip(ctx, run->path, "rise", &rise) == PONKO_OK && rise);
    CHECK(fabsf(ponko_clip_duration(rise) - 1.0f) < 1e-5f);
    PonkoClip* missing = NULL;
    CHECK(ponko_ctx_load_clip(ctx, run->path, "no such clip", &missing) != PONKO_OK && !missing);

    PonkoAnimationGraphBuilder* b = ponko_animation_graph_builder_new();
    size_t speed = 99, still = 99, up = 99, go = 99;
    CHECK(ponko_animation_graph_add_float(b, 0.0f, &speed) == PONKO_OK && speed == 0);
    size_t looping = 99;
    PonkoMotion* motion = NULL;
    CHECK(ponko_motion_clip(rise, &motion) == PONKO_OK);
    CHECK(ponko_animation_graph_add_state(b, motion, 0.0f, -1, true, &still) == PONKO_OK && still == 0);
    CHECK(ponko_motion_clip(rise, &motion) == PONKO_OK);
    CHECK(ponko_animation_graph_add_state(b, motion, 1.0f, -1, false, &up) == PONKO_OK && up == 1);
    CHECK(ponko_motion_clip(rise, &motion) == PONKO_OK);
    CHECK(ponko_animation_graph_add_state(b, motion, 1.0f, -1, true, &looping) == PONKO_OK);
    CHECK(ponko_animation_graph_add_event(b, up, 0.5f, "half") == PONKO_OK);
    CHECK(ponko_animation_graph_add_event(b, 9, 0.5f, "nowhere") == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_animation_graph_add_transition(b, (int64_t)still, up, -1.0f, 0.0f, 0.0f, &go) == PONKO_OK);
    CHECK(ponko_animation_graph_add_condition(b, go, PONKO_CONDITION_GREATER, speed, 0.5f) == PONKO_OK);
    PonkoAnimationGraph* graph = NULL;
    CHECK(ponko_animation_graph_build(b, still, &graph) == PONKO_OK && graph);
    /* A graph without states does not build; the builder is used up either way. */
    PonkoAnimationGraphBuilder* empty = ponko_animation_graph_builder_new();
    PonkoAnimationGraph* none = NULL;
    CHECK(ponko_animation_graph_build(empty, 3, &none) != PONKO_OK && !none);

    PonkoWorld* world = ponko_ctx_world(ctx);
    uint32_t transform_id;
    CHECK(ponko_transform3d_component(world, &transform_id) == PONKO_OK);
    PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){0, 0, 0});
    PonkoValue part = {transform_id, &at, sizeof at};
    uint64_t e;
    CHECK(ponko_spawn(world, &part, 1, &e) == PONKO_OK);
    CHECK(ponko_ctx_set_animator(ctx, e, graph) != PONKO_OK); /* no skeleton yet */
    CHECK(ponko_ctx_load_skeleton(ctx, e, run->path) == PONKO_OK);
    CHECK(ponko_ctx_set_animator(ctx, e, graph) == PONKO_OK);
    CHECK(ponko_ctx_animator_set_manual(ctx, e, true) == PONKO_OK);

    size_t state = 99; float time = -1;
    CHECK(ponko_ctx_animator_update(ctx, e, 0.1f) == PONKO_OK);
    CHECK(ponko_ctx_animator_state(ctx, e, &state, &time) == PONKO_OK && state == still);
    CHECK(ponko_ctx_animator_set_float(ctx, e, speed, 1.0f) == PONKO_OK);
    /* A wrong kind or index, or bad values, are invalid arguments. */
    CHECK(ponko_ctx_animator_set_bool(ctx, e, speed, true) == PONKO_INVALID_ARGUMENT); /* it is a float */
    CHECK(ponko_ctx_animator_set_float(ctx, e, 9, 1.0f) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_animator_set_float(ctx, e, speed, NAN) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_animator_set_trigger(ctx, e, speed, true) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_animator_update(ctx, e, 0.0f) == PONKO_OK); /* takes the transition */
    CHECK(ponko_ctx_animator_state(ctx, e, &state, &time) == PONKO_OK && state == up);
    CHECK(ponko_ctx_animator_update(ctx, e, 0.75f) == PONKO_OK);
    CHECK(ponko_ctx_animator_state(ctx, e, &state, &time) == PONKO_OK && fabsf(time - 0.75f) < 1e-4f);
    PonkoTransform3D arm;
    CHECK(ponko_ctx_joint_pose(ctx, e, 1, &arm) == PONKO_OK && fabsf(arm.position.y - 1.5f) < 1e-4f);

    char name[64]; bool found = false;
    CHECK(ponko_ctx_animator_next_event(ctx, e, name, sizeof name, &found) == PONKO_OK && found);
    CHECK(strcmp(name, "half") == 0);
    CHECK(ponko_ctx_animator_next_event(ctx, e, name, sizeof name, &found) == PONKO_OK && !found);
    CHECK(ponko_ctx_animator_crossfade(ctx, e, still, 0.0f, 0.0f) == PONKO_OK);
    CHECK(ponko_ctx_animator_update(ctx, e, 0.0f) == PONKO_OK);
    CHECK(ponko_ctx_animator_state(ctx, e, &state, &time) == PONKO_OK && state == still);
    CHECK(ponko_ctx_animator_crossfade(ctx, e, 7, 0.0f, 0.0f) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_animator_crossfade(ctx, e, still, -1.0f, 0.0f) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_animator_crossfade(ctx, e, still, 0.0f, NAN) == PONKO_INVALID_ARGUMENT);

    /* At ten hours, a sub-millisecond step is invisible through the float ABI. */
    CHECK(ponko_ctx_animator_set_float(ctx, e, speed, 0.0f) == PONKO_OK);
    CHECK(ponko_ctx_animator_crossfade(ctx, e, looping, 0.0f, 36000.0f) == PONKO_OK);
    CHECK(ponko_ctx_animator_update(ctx, e, 0.0f) == PONKO_OK);
    double precise = -1;
    CHECK(ponko_ctx_animator_normalized_time_f64(ctx, e, &precise) == PONKO_OK && precise == 36000.0);
    CHECK(ponko_ctx_animator_normalized_time_f64(ctx, e, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_animator_normalized_time_f64(ctx, UINT64_MAX, &precise) != PONKO_OK && precise == 36000.0);

    CHECK(ponko_ctx_animator_update(ctx, e, 0.0001f) == PONKO_OK);
    CHECK(ponko_ctx_animator_normalized_time_f64(ctx, e, &precise) == PONKO_OK);
    CHECK(fabs(precise - 36000.0001) < 1e-8);
    CHECK(ponko_ctx_animator_state(ctx, e, NULL, &time) == PONKO_OK && time == 36000.0f);

    ponko_animation_graph_free(graph);
    ponko_clip_free(rise);
}

static void animation_graphs(void) {
    AnimatorRun run;
    snprintf(run.path, sizeof run.path, "%s/ponko-c-animator.gltf", temp_dir());
    FILE* file = fopen(run.path, "wb");
    CHECK(file);
    CHECK(fputs(RISE_GLTF, file) >= 0);
    CHECK(fclose(file) == 0);
    PonkoGame* game = ponko_game_new("animator", 1, 1);
    PonkoWorld* world = NULL;
    CHECK(ponko_game_run_headless(game, 0, animator_setup, &run, &world) == PONKO_OK);
    CHECK(ponko_world_free(world) == PONKO_OK);
    remove(run.path);
}

/* Crowds: a clip baked on a two-joint skeleton, a skinned post as the
   level, characters made, counted, replaced, rejected and freed. */
static void crowd_setup(PonkoCtx* ctx, void* user) {
    const AnimatorRun* run = user;
    PonkoClip* rise = NULL;
    CHECK(ponko_ctx_load_clip(ctx, run->path, "rise", &rise) == PONKO_OK && rise);
    PonkoWorld* world = ponko_ctx_world(ctx);
    uint32_t transform_id;
    CHECK(ponko_transform3d_component(world, &transform_id) == PONKO_OK);
    PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){0, 0, 0});
    PonkoValue part = {transform_id, &at, sizeof at};
    uint64_t e;
    CHECK(ponko_spawn(world, &part, 1, &e) == PONKO_OK);
    PonkoJoint joints[] = {{"root", -1, ponko_transform3d_at((PonkoVec3){0, 0, 0})},
                           {"arm", 0, ponko_transform3d_at((PonkoVec3){0, 1, 0})}};
    CHECK(ponko_ctx_set_skeleton(ctx, e, joints, 2) == PONKO_OK);

    /* A post whose upper corners follow the arm. */
    PonkoMeshData* post = ponko_mesh_data_cube(0.2f);
    PonkoMeshView view;
    CHECK(ponko_mesh_data_view(post, &view) == PONKO_OK);
    uint16_t* skin_joints = calloc(view.vertex_count * 4, sizeof *skin_joints);
    float* weights = calloc(view.vertex_count * 4, sizeof *weights);
    CHECK(skin_joints && weights);
    for (size_t i = 0; i < view.vertex_count; ++i) {
        skin_joints[i * 4] = view.positions[i].y > 0 ? 1 : 0;
        weights[i * 4] = 1;
    }
    CHECK(ponko_mesh_data_set_skin(post, skin_joints, weights, view.vertex_count) == PONKO_OK);
    PonkoMaterial material;
    CHECK(ponko_ctx_default_material(ctx, &material) == PONKO_OK);
    PonkoMesh mesh, plain;
    CHECK(ponko_ctx_create_skinned_mesh(ctx, post, material, e, &mesh) == PONKO_OK);
    PonkoMeshData* cube = ponko_mesh_data_cube(1);
    CHECK(ponko_ctx_create_mesh(ctx, cube, material, &plain) == PONKO_OK);

    /* The bake: its clips by name; bad clips and rates are rejected. */
    PonkoCrowdClip clips[] = {{rise, true}};
    PonkoCrowdAnimation* animation = NULL;
    CHECK(ponko_ctx_bake_crowd_animation(ctx, e, clips, 1, 0, &animation) == PONKO_INVALID_ARGUMENT);
    PonkoCrowdClip none[] = {{NULL, true}};
    CHECK(ponko_ctx_bake_crowd_animation(ctx, e, none, 1, 30, &animation) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_bake_crowd_animation(ctx, e, clips, 1, 30, &animation) == PONKO_OK && animation);
    uint16_t index = 9;
    CHECK(ponko_crowd_animation_clip(animation, "rise", &index) == PONKO_OK && index == 0);
    CHECK(ponko_crowd_animation_clip(animation, "walk", &index) == PONKO_NOT_FOUND);

    /* Three characters, the second rising half way. */
    PonkoCrowdInstance people[3];
    for (int i = 0; i < 3; ++i) people[i] = ponko_crowd_instance_at((PonkoVec3){(float)i, 0, 0});
    CHECK(people[0].speed == 1 && people[0].transform[12] == 0 && people[2].transform[12] == 2);
    people[1].time = 0.5f;
    PonkoCrowdLevel level = {mesh, 0, INFINITY, true, true, 4};
    PonkoCrowdOptions options = {true};
    PonkoCrowd crowd;
    CHECK(ponko_ctx_create_crowd(ctx, animation, &level, 1, people, 3, options, &crowd) == PONKO_OK);
    size_t count = 0;
    CHECK(ponko_ctx_crowd_len(ctx, crowd, &count) == PONKO_OK && count == 3);
    /* Two levels that crossfade over the near one's last 5 m (PonkoCrowdDesc); a fade wider than its
       band is refused. */
    {
        PonkoCrowdLevel near_far[2] = {{mesh, 0, 30, true, true, 4}, {mesh, 30, INFINITY, true, true, 1}};
        float fades[2] = {5, 0};
        PonkoCrowdDesc desc = ponko_crowd_desc_defaults();
        CHECK(desc.struct_size == sizeof desc && desc.cast_shadows && desc.fades == NULL);
        desc.animation = animation;
        desc.levels = near_far; desc.level_count = 2; desc.fades = fades;
        desc.instances = people; desc.count = 3;
        PonkoCrowd faded;
        CHECK(ponko_ctx_create_crowd_ex(ctx, &desc, &faded) == PONKO_OK);
        CHECK(ponko_ctx_crowd_len(ctx, faded, &count) == PONKO_OK && count == 3);
        CHECK(ponko_ctx_free_crowd(ctx, faded) == PONKO_OK);
        fades[0] = 31;
        CHECK(ponko_ctx_create_crowd_ex(ctx, &desc, &faded) == PONKO_INVALID_ARGUMENT);
        desc.version = 0;
        CHECK(ponko_ctx_create_crowd_ex(ctx, &desc, &faded) == PONKO_INVALID_ARGUMENT);
    }
    /* A level that isn't skinned or moves no corner, a clip the bake hasn't. */
    PonkoCrowdLevel bad = {plain, 0, INFINITY, true, true, 4};
    PonkoCrowd other;
    CHECK(ponko_ctx_create_crowd(ctx, animation, &bad, 1, people, 3, options, &other) == PONKO_INVALID_ARGUMENT);
    bad.mesh = mesh; bad.influences = 0;
    CHECK(ponko_ctx_create_crowd(ctx, animation, &bad, 1, people, 3, options, &other) == PONKO_INVALID_ARGUMENT);
    people[2].clip = 1;
    CHECK(ponko_ctx_update_crowd(ctx, crowd, people, 3) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_crowd_len(ctx, crowd, &count) == PONKO_OK && count == 3); /* kept */
    people[2].clip = 0;
    people[2].transform[0] = NAN;
    CHECK(ponko_ctx_update_crowd(ctx, crowd, people, 3) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_update_crowd(ctx, crowd, people, 2) == PONKO_OK);
    CHECK(ponko_ctx_crowd_len(ctx, crowd, &count) == PONKO_OK && count == 2);
    /* Freed: no length, no updates; freeing again is harmless. */
    CHECK(ponko_ctx_free_crowd(ctx, crowd) == PONKO_OK);
    CHECK(ponko_ctx_crowd_len(ctx, crowd, &count) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_update_crowd(ctx, crowd, people, 2) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_free_crowd(ctx, crowd) == PONKO_OK);
    ponko_crowd_animation_free(animation);
    ponko_crowd_animation_free(NULL);
    ponko_mesh_data_free(cube);
    ponko_mesh_data_free(post);
    free(skin_joints);
    free(weights);
    ponko_clip_free(rise);
}

static void crowds(void) {
    AnimatorRun run;
    snprintf(run.path, sizeof run.path, "%s/ponko-c-crowd.gltf", temp_dir());
    FILE* file = fopen(run.path, "wb");
    CHECK(file);
    CHECK(fputs(RISE_GLTF, file) >= 0);
    CHECK(fclose(file) == 0);
    PonkoGame* game = ponko_game_new("crowds", 1, 1);
    PonkoWorld* world = NULL;
    CHECK(ponko_game_run_headless(game, 0, crowd_setup, &run, &world) == PONKO_OK);
    CHECK(ponko_world_free(world) == PONKO_OK);
    remove(run.path);
}

/* 2D skeletons: a file in memory (its image, crab.png, from assets/), on an
 * entity, driven by clip, state, checked parameters, IK, events and slots. */
static const char SKELETON2D[] =
    "{ \"format\": \"ponko-skeleton2d\", \"version\": 1,"
    "  \"bones\": [{ \"name\": \"root\" }, { \"name\": \"upper\", \"parent\": \"root\", \"length\": 10 },"
    "    { \"name\": \"lower\", \"parent\": \"upper\", \"x\": 10, \"length\": 10 },"
    "    { \"name\": \"goal\", \"parent\": \"root\", \"x\": 15, \"y\": 5 }],"
    "  \"slots\": [{ \"name\": \"body\", \"bone\": \"root\", \"attachment\": \"body\" }],"
    "  \"skins\": [{ \"name\": \"default\", \"attachments\": ["
    "    { \"slot\": \"body\", \"name\": \"body\", \"image\": \"crab.png\", \"size\": [8, 8] },"
    "    { \"slot\": \"body\", \"name\": \"fist\", \"image\": \"crab.png\", \"size\": [4, 4] }] }],"
    "  \"ik\": [{ \"name\": \"reach\", \"bones\": [\"upper\", \"lower\"], \"target\": \"goal\", \"mix\": 0 }],"
    "  \"clips\": [{ \"name\": \"idle\", \"duration\": 1, \"events\": [{ \"time\": 0.5, \"name\": \"blink\" }] },"
    "    { \"name\": \"wave\", \"bones\": [{ \"bone\": \"root\","
    "      \"translate\": [{ \"time\": 0, \"value\": [0, 0] }, { \"time\": 1, \"value\": [10, 0] }] }] }],"
    "  \"graph\": { \"parameters\": [{ \"name\": \"speed\", \"float\": 0 }, { \"name\": \"jump\", \"trigger\": false }],"
    "    \"states\": [{ \"name\": \"idle\", \"clip\": \"idle\" }, { \"name\": \"wave\", \"clip\": \"wave\", \"loop\": false }],"
    "    \"transitions\": [{ \"from\": \"idle\", \"to\": \"wave\", \"when\": [{ \"trigger\": \"jump\" }] }] } }";

static void skeleton2d_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoMemorySource* memory = ponko_memory_source_new("skeletons");
    CHECK(ponko_memory_source_insert(memory, "rig.json", (const uint8_t*)SKELETON2D, strlen(SKELETON2D)) == PONKO_OK);
    CHECK(ponko_ctx_add_memory_source(ctx, memory) == PONKO_OK);
    ponko_memory_source_free(memory);
    size_t warnings = 99;
    CHECK(ponko_ctx_load_skeleton2d(ctx, "rig.json", &warnings) == PONKO_OK && warnings == 0);
    CHECK(ponko_ctx_load_skeleton2d(ctx, "missing.json", NULL) == PONKO_NOT_FOUND);

    uint64_t e;
    CHECK(ponko_spawn(ponko_ctx_world(ctx), NULL, 0, &e) == PONKO_OK);
    CHECK(ponko_ctx_set_skeleton2d(ctx, e, "rig.json") == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_set_manual(ctx, e, true) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_update(ctx, e, 0.0f) == PONKO_OK);

    /* The state machine: names to indices once, then checked calls. */
    size_t wave = 99, speed = 99, jump = 99, state = 99;
    float time = -1; bool finished = true;
    CHECK(ponko_ctx_skeleton2d_state_index(ctx, e, "wave", &wave) == PONKO_OK && wave == 1);
    CHECK(ponko_ctx_skeleton2d_parameter_index(ctx, e, "speed", &speed) == PONKO_OK && speed == 0);
    CHECK(ponko_ctx_skeleton2d_parameter_index(ctx, e, "jump", &jump) == PONKO_OK && jump == 1);
    CHECK(ponko_ctx_skeleton2d_set_float(ctx, e, speed, 2.0f) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_set_float(ctx, e, speed, NAN) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_skeleton2d_set_bool(ctx, e, speed, true) == PONKO_INVALID_ARGUMENT); /* a float */
    CHECK(ponko_ctx_skeleton2d_set_trigger(ctx, e, jump, true) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_update(ctx, e, 0.5f) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_state(ctx, e, &state, &time, &finished) == PONKO_OK);
    CHECK(state == wave && fabsf(time - 0.5f) < 1e-4f && !finished);
    PonkoVec2 at; float rotation;
    CHECK(ponko_ctx_skeleton2d_bone(ctx, e, 0, &at, &rotation) == PONKO_OK && fabsf(at.x - 5.0f) < 1e-4f);
    CHECK(ponko_ctx_skeleton2d_crossfade_state(ctx, e, 7, 0.0f, 0.0f) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_skeleton2d_crossfade_state(ctx, e, 0, 0.0f, 0.0f) == PONKO_OK);

    CHECK(ponko_ctx_skeleton2d_crossfade_state(ctx, e, 0, 0.0f, 36000.0f) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_update(ctx, e, 0.0f) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_update(ctx, e, 0.0001f) == PONKO_OK);
    double precise = -1;
    CHECK(ponko_ctx_skeleton2d_normalized_time_f64(ctx, e, &precise) == PONKO_OK);
    CHECK(fabs(precise - 36000.0001) < 1e-8);
    CHECK(ponko_ctx_skeleton2d_state(ctx, e, NULL, &time, NULL) == PONKO_OK && time == 36000.0f);
    CHECK(ponko_ctx_skeleton2d_normalized_time_f64(ctx, e, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_skeleton2d_normalized_time_f64(ctx, UINT64_MAX, &precise) != PONKO_OK);

    /* Clips by name, and their events. */
    CHECK(ponko_ctx_skeleton2d_play(ctx, e, "idle", true) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_play(ctx, e, "fly", true) == PONKO_NOT_FOUND);
    CHECK(ponko_ctx_skeleton2d_update(ctx, e, 0.6f) == PONKO_OK);
    char name[32]; bool found = false;
    CHECK(ponko_ctx_skeleton2d_next_event(ctx, e, name, sizeof name, &found) == PONKO_OK && found);
    CHECK(strcmp(name, "blink") == 0);
    CHECK(ponko_ctx_skeleton2d_next_event(ctx, e, name, sizeof name, &found) == PONKO_OK && !found);

    /* IK: straight down, the chain's whole length away. */
    size_t reach = 99, upper = 99;
    CHECK(ponko_ctx_skeleton2d_ik_index(ctx, e, "reach", &reach) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_bone_index(ctx, e, "upper", &upper) == PONKO_OK && upper == 1);
    CHECK(ponko_ctx_skeleton2d_set_ik_target(ctx, e, reach, (PonkoVec2){0, 20}) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_set_ik_mix(ctx, e, reach, 1.0f) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_update(ctx, e, 0.0f) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_bone(ctx, e, upper, NULL, &rotation) == PONKO_OK);
    CHECK(fabsf(rotation - 1.5707964f) < 1e-4f);
    CHECK(ponko_ctx_skeleton2d_clear_ik_target(ctx, e, reach) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_set_ik_mix(ctx, e, 5, 1.0f) == PONKO_INVALID_ARGUMENT);

    /* One-off posing after the update: look-at, then two-bone IK. */
    size_t lower = 99;
    CHECK(ponko_ctx_skeleton2d_bone_index(ctx, e, "lower", &lower) == PONKO_OK && lower == 2);
    CHECK(ponko_ctx_skeleton2d_set_ik_mix(ctx, e, reach, 0.0f) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_update(ctx, e, 0.0f) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_look_at(ctx, e, upper, (PonkoVec2){0, 10}, 1.0f) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_bone(ctx, e, upper, NULL, &rotation) == PONKO_OK);
    CHECK(fabsf(rotation - 1.5707964f) < 1e-4f);
    CHECK(ponko_ctx_skeleton2d_two_bone_ik(ctx, e, upper, lower, (PonkoVec2){10, 10}, true, 1.0f) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_bone(ctx, e, upper, NULL, &rotation) == PONKO_OK && fabsf(rotation) < 1e-4f);
    CHECK(ponko_ctx_skeleton2d_bone(ctx, e, lower, &at, &rotation) == PONKO_OK);
    CHECK(fabsf(rotation - 1.5707964f) < 1e-4f && fabsf(at.x - 10.0f) < 1e-4f && fabsf(at.y) < 1e-4f);
    CHECK(ponko_ctx_skeleton2d_two_bone_ik(ctx, e, lower, upper, (PonkoVec2){10, 10}, true, 1.0f) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_skeleton2d_look_at(ctx, e, 99, (PonkoVec2){0, 10}, 1.0f) == PONKO_INVALID_ARGUMENT);

    /* Slots and skins. */
    size_t body = 99, len = 0;
    CHECK(ponko_ctx_skeleton2d_slot_index(ctx, e, "body", &body) == PONKO_OK && body == 0);
    CHECK(ponko_ctx_skeleton2d_set_attachment(ctx, e, body, "fist") == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_update(ctx, e, 0.0f) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_attachment(ctx, e, body, name, sizeof name, &len) == PONKO_OK);
    CHECK(strcmp(name, "fist") == 0 && len == 4);
    CHECK(ponko_ctx_skeleton2d_reset_attachment(ctx, e, body) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_set_skin(ctx, e, "gold") == PONKO_NOT_FOUND);
    CHECK(ponko_ctx_skeleton2d_set_look(ctx, e, 1.0f, (PonkoColor){1, 1, 1, 1}, 1.0f) == PONKO_OK);
    CHECK(ponko_ctx_remove_skeleton2d(ctx, e) == PONKO_OK);
    CHECK(ponko_ctx_skeleton2d_update(ctx, e, 0.0f) == PONKO_NOT_FOUND);
}

static void skeletons_2d(void) {
    PonkoGame* game = ponko_game_new("skeletons", 1, 1);
    CHECK(ponko_game_hot_reload(game, false) == PONKO_OK);
    PonkoWorld* world = NULL;
    CHECK(ponko_game_run_headless(game, 0, skeleton2d_setup, NULL, &world) == PONKO_OK);
    CHECK(ponko_world_free(world) == PONKO_OK);
}

/* Declared access: a C system says what it reads and writes. */
typedef struct { uint32_t pos; uint64_t entity; size_t hits; } Declared;
static void declared_mover(PonkoCtx* ctx, void* user) {
    Declared* d = user;
    Position at;
    CHECK(ponko_get(ponko_ctx_world(ctx), d->entity, d->pos, &at, sizeof(at)) == PONKO_OK);
    at.x += 1;
    CHECK(ponko_set(ponko_ctx_world(ctx), d->entity, d->pos, &at, sizeof(at)) == PONKO_OK);
    CHECK(ponko_ctx_send_event(ctx, d->pos, &at, sizeof(at)) == PONKO_OK);
}
static void declared_reader(PonkoCtx* ctx, void* user) {
    Declared* d = user;
    const void* data; size_t count;
    CHECK(ponko_ctx_read_events(ctx, d->pos, sizeof(Position), &data, &count) == PONKO_OK);
    d->hits += count;
}
static void declared_setup(PonkoCtx* ctx, void* user) {
    Declared* d = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    CHECK(ponko_component_register(w, "DeclPos", sizeof(Position), _Alignof(Position), &d->pos) == PONKO_OK);
    Position start = {0}; PonkoValue v = {d->pos, &start, sizeof(start)};
    CHECK(ponko_spawn(w, &v, 1, &d->entity) == PONKO_OK);
}
static void declared_access(void) {
    PonkoAccess* mover = ponko_access_new(); CHECK(mover);
    CHECK(ponko_access_write_component(mover, "DeclPos") == PONKO_OK);
    CHECK(ponko_access_send_events(mover, "DeclPos") == PONKO_OK);
    CHECK(ponko_access_service(mover, PONKO_SERVICE_TIME, false) == PONKO_OK);
    CHECK(ponko_access_service(mover, 99, false) == PONKO_INVALID_ARGUMENT);
    PonkoAccess* reader = ponko_access_new(); CHECK(reader);
    CHECK(ponko_access_read_events(reader, "DeclPos") == PONKO_OK);
    size_t n = 9;
    CHECK(ponko_access_conflicts(reader, mover, true, &n) == PONKO_OK && n == 0);
    CHECK(ponko_access_conflicts(mover, reader, true, &n) == PONKO_OK && n == 1);
    CHECK(ponko_access_conflicts(reader, mover, false, &n) == PONKO_OK && n == 1);
    PonkoAccess* spawner = ponko_access_new(); CHECK(spawner);
    CHECK(ponko_access_commands(spawner) == PONKO_OK);
    CHECK(ponko_access_conflicts(spawner, mover, true, &n) == PONKO_OK && n == 1);
    CHECK(ponko_access_conflicts(mover, spawner, true, &n) == PONKO_OK && n == 0);
    ponko_access_free(spawner);
    Declared d = {0, 0, 0};
    PonkoGame* g = ponko_game_new("declared", 1, 1); CHECK(g);
    CHECK(ponko_game_add_system_with_access(g, PONKO_PHASE_AFTER_PHYSICS, "mover", 1, mover, declared_mover, &d) == PONKO_OK);
    CHECK(ponko_game_add_system_with_access(g, PONKO_PHASE_AFTER_PHYSICS, "reader", 1, reader, declared_reader, &d) == PONKO_OK);
    CHECK(ponko_game_add_system_with_access(g, PONKO_PHASE_AFTER_PHYSICS, "bad", 0, reader, declared_reader, &d) == PONKO_INVALID_ARGUMENT);
    ponko_access_free(mover);
    ponko_access_free(reader);
    size_t len = 0;
    CHECK(ponko_game_access_report(g, NULL, 0, &len) == PONKO_OK && len > 0);
    char* text = malloc(len + 1);
    CHECK(ponko_game_access_report(g, text, len + 1, &len) == PONKO_OK);
    CHECK(strcmp(text, "mover + reader: conflict on events `DeclPos`\n") == 0);
    free(text);
    /* The scheduler may be on: C systems still run one at a time, and the plan says why. */
    CHECK(ponko_game_parallel_systems(g, true) == PONKO_OK);
    CHECK(ponko_game_schedule_plan(g, NULL, 0, &len) == PONKO_OK && len > 0);
    text = malloc(len + 1);
    CHECK(ponko_game_schedule_plan(g, text, len + 1, &len) == PONKO_OK);
    CHECK(strstr(text, "  mover (alone: takes `&mut Ctx`)\n") != NULL);
    CHECK(strstr(text, "  reader (alone: takes `&mut Ctx`)\n") != NULL);
    free(text);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(g, 3, declared_setup, &d, &w) == PONKO_OK && w);
    Position at;
    CHECK(ponko_get(w, d.entity, d.pos, &at, sizeof(at)) == PONKO_OK && at.x == 3);
    CHECK(d.hits == 3);
    CHECK(ponko_world_free(w) == PONKO_OK);
}

/* Barriers: a system's commands and events reach the next system, and
 * nothing is applied while the recording system (or its query) still runs. */
typedef struct {
    PonkoCtx* ctx; uint32_t mark;
    size_t in_callback, after_recording, next_sees, pings, runs;
} Barrier;
static void barrier_visit(const PonkoChunk* chunk, void* user) {
    Barrier* b = user;
    Position p = {1}; PonkoValue v = {b->mark, &p, sizeof(p)};
    for (size_t i = 0; i < chunk->count; ++i) CHECK(ponko_ctx_spawn_later(b->ctx, &v, 1) == PONKO_OK);
    /* A callback can record commands but can't look at the world while the
     * query walks it, so it never sees a half-applied change. */
    size_t len;
    CHECK(ponko_world_len(ponko_ctx_world(b->ctx), &len) == PONKO_BUSY);
    ++b->in_callback;
}
static void barrier_recorder(PonkoCtx* ctx, void* user) {
    Barrier* b = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    size_t before; CHECK(ponko_world_len(w, &before) == PONKO_OK);
    b->ctx = ctx;
    b->in_callback = 0;
    CHECK(ponko_query(w, &b->mark, 1, barrier_visit, b) == PONKO_OK);
    CHECK(b->in_callback > 0);
    /* The commands recorded in the query are still pending after it. */
    size_t after_query; CHECK(ponko_world_len(w, &after_query) == PONKO_OK && after_query == before);
    Position p = {2}; PonkoValue v = {b->mark, &p, sizeof(p)};
    CHECK(ponko_ctx_spawn_later(ctx, &v, 1) == PONKO_OK);
    CHECK(ponko_ctx_send_event(ctx, b->mark, &p, sizeof(p)) == PONKO_OK);
    /* And still pending when the system is about to return. */
    CHECK(ponko_world_len(w, &b->after_recording) == PONKO_OK);
    CHECK(b->after_recording == before);
}
static void barrier_next(PonkoCtx* ctx, void* user) {
    Barrier* b = user;
    CHECK(ponko_world_len(ponko_ctx_world(ctx), &b->next_sees) == PONKO_OK);
    const void* data; size_t count;
    CHECK(ponko_ctx_read_events(ctx, b->mark, sizeof(Position), &data, &count) == PONKO_OK);
    b->pings += count;
    ++b->runs;
    /* Both commands of the recorder are in the world, together with its event:
     * one entity before the first tick, doubled plus one by every recorder run. */
    size_t expected = 1;
    for (size_t i = 0; i < b->runs; ++i) expected = expected * 2 + 1;
    CHECK(b->next_sees == expected);
    CHECK(count == 1);
}
static void barrier_setup(PonkoCtx* ctx, void* user) {
    Barrier* b = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    CHECK(ponko_component_register(w, "BarrierMark", sizeof(Position), _Alignof(Position), &b->mark) == PONKO_OK);
    Position p = {0}; PonkoValue v = {b->mark, &p, sizeof(p)};
    /* A command from setup applies before the first tick. */
    CHECK(ponko_ctx_spawn_later(ctx, &v, 1) == PONKO_OK);
}
static void command_barriers(void) {
    /* The same with the scheduler off and on: C systems always run alone. */
    for (int parallel = 0; parallel < 2; ++parallel) {
        Barrier b = {0};
        PonkoGame* g = ponko_game_new("barriers", 1, 1); CHECK(g);
        CHECK(ponko_game_parallel_systems(g, parallel != 0) == PONKO_OK);
        CHECK(ponko_game_add_system(g, PONKO_PHASE_AFTER_PHYSICS, "recorder", 1, barrier_recorder, &b) == PONKO_OK);
        CHECK(ponko_game_add_system(g, PONKO_PHASE_AFTER_PHYSICS, "next", 1, barrier_next, &b) == PONKO_OK);
        PonkoWorld* w = NULL;
        CHECK(ponko_game_run_headless(g, 3, barrier_setup, &b, &w) == PONKO_OK && w);
        CHECK(b.runs == 3 && b.pings == 3);
        size_t len; CHECK(ponko_world_len(w, &len) == PONKO_OK && len == 15);
        CHECK(ponko_world_free(w) == PONKO_OK);
    }
}

/* ponko_ctx_spawn_later_entity: the entity is known while recording, and a
 * spawn that fails leaves it dead. */
typedef struct { uint32_t mark; int done; uint64_t good, bad; } SpawnIds;
static void spawn_ids_system(PonkoCtx* ctx, void* user) {
    SpawnIds* s = user;
    if (s->done++) return;
    Position p = {5}; PonkoValue v = {s->mark, &p, sizeof(p)};
    CHECK(ponko_ctx_spawn_later_entity(ctx, &v, 1, &s->good) == PONKO_OK);
    bool alive = true;
    CHECK(ponko_is_alive(ponko_ctx_world(ctx), s->good, &alive) == PONKO_OK && !alive);
    /* A later command of the system uses it. */
    Position q = {6}; PonkoValue w = {s->mark, &q, sizeof(q)};
    CHECK(ponko_ctx_set_later(ctx, s->good, &w) == PONKO_OK);
    /* Wrong size: checked when applied, so the spawn fails then. */
    PonkoValue wrong = {s->mark, &p, 1};
    CHECK(ponko_ctx_spawn_later_entity(ctx, &wrong, 1, &s->bad) == PONKO_OK);
    CHECK(s->bad != s->good);
    /* NULL out_entity is allowed. */
    CHECK(ponko_ctx_spawn_later_entity(ctx, &v, 1, NULL) == PONKO_OK);
}
static void spawn_ids_setup(PonkoCtx* ctx, void* user) {
    SpawnIds* s = user;
    CHECK(ponko_component_register(ponko_ctx_world(ctx), "SpawnIdsMark", sizeof(Position), _Alignof(Position), &s->mark) == PONKO_OK);
}
static void command_spawn_ids(void) {
    SpawnIds s = {0};
    PonkoGame* g = ponko_game_new("spawn ids", 1, 1); CHECK(g);
    CHECK(ponko_game_add_system(g, PONKO_PHASE_AFTER_PHYSICS, "spawner", 1, spawn_ids_system, &s) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(g, 2, spawn_ids_setup, &s, &w) == PONKO_OK && w);
    bool alive = false;
    CHECK(ponko_is_alive(w, s.good, &alive) == PONKO_OK && alive);
    Position got = {0};
    CHECK(ponko_get(w, s.good, s.mark, &got, sizeof(got)) == PONKO_OK && got.x == 6);
    CHECK(ponko_is_alive(w, s.bad, &alive) == PONKO_OK && !alive);
    size_t len; CHECK(ponko_world_len(w, &len) == PONKO_OK && len == 2);
    CHECK(ponko_world_free(w) == PONKO_OK);
}

/* Action ids, asset sources, the try_ loaders and named grid spans (#262). */
typedef struct { const char* name; const uint8_t* bytes; size_t len; int frees; } Pak;
static uint32_t pak_read(void* user, const char* name, const uint8_t** data, size_t* len) {
    Pak* pak = user;
    if (strcmp(name, pak->name) != 0) return PONKO_NOT_FOUND;
    *data = pak->bytes; *len = pak->len;
    return PONKO_OK;
}
static void pak_free(void* user) { ((Pak*)user)->frees++; }
typedef struct { size_t len; uint8_t first; } Decoded;
static uint32_t decode_bytes(void* user, const uint8_t* data, size_t len) {
    Decoded* d = user;
    d->len = len; d->first = len ? data[0] : 0;
    return len ? PONKO_OK : PONKO_INVALID_ARGUMENT;
}
static void native_setup(PonkoCtx* ctx, void* user) {
    Pak* pak = user;
    /* Action ids: resolved once, then queried without the name. */
    PonkoActionMap* map = ponko_action_map_new();
    PonkoActionBinding space = {PONKO_ACTION_KEY, 0, PONKO_KEY_SPACE, 0, 0, 0, 0};
    CHECK(ponko_action_map_set(map, "jump", 0, &space, 1) == PONKO_OK);
    CHECK(ponko_ctx_set_actions(ctx, map) == PONKO_OK);
    ponko_action_map_free(map);
    uint32_t jump = PONKO_ACTION_NONE, ids[2];
    const char* names[2] = {"jump", "fly"};
    CHECK(ponko_ctx_action_id(ctx, "jump", &jump) == PONKO_OK && jump != PONKO_ACTION_NONE);
    CHECK(ponko_ctx_action_ids(ctx, names, 2, ids) == PONKO_NOT_FOUND);
    CHECK(ids[0] == jump && ids[1] == PONKO_ACTION_NONE && ponko_last_error_kind() == PONKO_ERROR_NOT_FOUND);
    CHECK(ponko_ctx_feed_input(ctx, 0, 0, PONKO_KEY_SPACE, 1) == PONKO_OK);
    bool down = false, pressed = false, released = true;
    float axis = 1;
    PonkoVec2 axis2 = {1, 1};
    CHECK(ponko_ctx_action_down_id(ctx, jump, &down) == PONKO_OK && down);
    CHECK(ponko_ctx_action_pressed_id(ctx, jump, &pressed) == PONKO_OK && pressed);
    CHECK(ponko_ctx_action_released_id(ctx, jump, &released) == PONKO_OK && !released);
    CHECK(ponko_ctx_action_axis_id(ctx, jump, &axis) == PONKO_OK && axis == 0);
    CHECK(ponko_ctx_action_axis2_id(ctx, PONKO_ACTION_NONE, &axis2) == PONKO_OK && axis2.x == 0);

    /* Pressed this frame: what a rebinding screen reads, ready for ponko_action_map_set. */
    CHECK(ponko_ctx_feed_input(ctx, 0, 0, PONKO_KEY_J, 1) == PONKO_OK);
    CHECK(ponko_ctx_feed_input(ctx, 3, 0, PONKO_GAMEPAD_LEFT_TRIGGER_AXIS, 0.9f) == PONKO_OK);
    PonkoActionBinding fresh[4];
    size_t fresh_count = 0;
    CHECK(ponko_ctx_pressed_this_frame(ctx, 0.0f, fresh, 4, &fresh_count) == PONKO_OK);
    /* Space went down above too: keys in key order, then the axis. */
    CHECK(fresh_count == 3 && fresh[0].kind == PONKO_ACTION_KEY && fresh[0].first == PONKO_KEY_J);
    CHECK(fresh[1].kind == PONKO_ACTION_KEY && fresh[1].first == PONKO_KEY_SPACE);
    CHECK(fresh[2].kind == PONKO_ACTION_GAMEPAD_AXIS && fresh[2].first == PONKO_GAMEPAD_LEFT_TRIGGER_AXIS);
    CHECK(fresh[2].deadzone > 0.0f && fresh[2].deadzone < PONKO_PRESS_THRESHOLD);
    CHECK(ponko_ctx_pressed_this_frame(ctx, -1.0f, fresh, 4, &fresh_count) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_feed_input(ctx, 0, 0, PONKO_KEY_J, 0) == PONKO_OK);
    CHECK(ponko_ctx_feed_input(ctx, 3, 0, PONKO_GAMEPAD_LEFT_TRIGGER_AXIS, 0) == PONKO_OK);

    /* The try_ loaders: kind and path, never a caught panic. */
    PonkoTexture texture;
    PonkoFont font;
    PonkoSound sound;
    PonkoMesh mesh;
    CHECK(ponko_ctx_try_load_texture(ctx, "no/such.png", &texture) == PONKO_NOT_FOUND);
    CHECK(ponko_last_error_kind() == PONKO_ERROR_NOT_FOUND && strcmp(ponko_last_error_path(), "no/such.png") == 0);
    CHECK(strcmp(ponko_error_kind_name(PONKO_ERROR_NOT_FOUND), "not found") == 0);
    CHECK(ponko_ctx_try_load_font(ctx, "no/such.ttf", &font) == PONKO_NOT_FOUND);
    CHECK(ponko_ctx_try_load_sound(ctx, "no/such.ogg", &sound) == PONKO_NOT_FOUND);
    CHECK(ponko_ctx_try_load_stream(ctx, "no/such.ogg", &sound) == PONKO_NOT_FOUND);
    CHECK(ponko_ctx_try_load_mesh(ctx, "no/such.glb", &mesh) == PONKO_NOT_FOUND);

    /* Asset sources: memory and callbacks, then your own decoder. */
    PonkoMemorySource* memory = ponko_memory_source_new("embedded");
    CHECK(memory && ponko_memory_source_insert(memory, "level.txt", (const uint8_t*)"##.", 3) == PONKO_OK);
    CHECK(ponko_ctx_add_memory_source(ctx, memory) == PONKO_OK);
    ponko_memory_source_free(memory);
    CHECK(ponko_ctx_add_asset_source(ctx, "pak", pak_read, NULL, pak_free, pak) == PONKO_OK);
    Decoded decoded = {0, 0};
    CHECK(ponko_ctx_load_asset_with(ctx, "level.txt", decode_bytes, &decoded) == PONKO_OK);
    CHECK(decoded.len == 3 && decoded.first == '#');
    CHECK(ponko_ctx_load_asset_with(ctx, pak->name, decode_bytes, &decoded) == PONKO_OK && decoded.len == pak->len);
    CHECK(ponko_ctx_load_asset_with(ctx, "missing.bin", decode_bytes, &decoded) == PONKO_NOT_FOUND);
    CHECK(strcmp(ponko_last_error_path(), "missing.bin") == 0);

    /* A named span: from line 1 to the second line called "edge". */
    PonkoUiTemplateTrack columns[4];
    for (int i = 0; i < 4; ++i) columns[i] = (PonkoUiTemplateTrack){{PONKO_UI_TRACK_POINTS, 100}, 0, 0, NULL, 0, NULL, 0};
    PonkoUiLayout grid = ponko_ui_layout_default();
    grid.display = PONKO_UI_DISPLAY_GRID;
    grid.grid_template_columns = columns;
    grid.grid_template_column_count = 4;
    uint64_t root, item;
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &grid, &root) == PONKO_OK);
    const char* lines[5] = {"", "", "edge", "", "edge"};
    CHECK(ponko_ctx_ui_set_grid_line_names(ctx, PONKO_UI_SCREEN, root, 0, lines, 5) == PONKO_OK);
    PonkoUiLayout cell = ponko_ui_layout_default();
    cell.grid_column_start = (PonkoUiGridLine){PONKO_UI_GRID_LINE, 1};
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, root, &cell, &item) == PONKO_OK);
    CHECK(ponko_ctx_ui_set_grid_named_span(ctx, PONKO_UI_SCREEN, item, 0, true, "edge", 2) == PONKO_OK);
    CHECK(ponko_ctx_ui_set_grid_named_span(ctx, PONKO_UI_SCREEN, item, 0, true, "edge", 0) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, root, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_OK);
    CHECK(ponko_ctx_ui_layout(ctx, PONKO_UI_SCREEN, (PonkoVec2){800, 600}) == PONKO_OK);
    PonkoRect r;
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, item, &r) == PONKO_OK && r.min.x == 0 && r.max.x == 400);
}
typedef struct { uint32_t points; } Points;
static void add_points(PonkoWorld* world, void* value, void* user) {
    Points* points = value;
    Points probe;
    CHECK(ponko_resource_get(world, "Points", &probe, sizeof probe) == PONKO_NOT_FOUND); /* lent out */
    points->points += *(uint32_t*)user;
}
static void scenario_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoActionMap* map = ponko_action_map_new();
    PonkoActionBinding space = {PONKO_ACTION_KEY, 0, PONKO_KEY_SPACE, 0, 0, 0, 0};
    CHECK(ponko_action_map_set(map, "jump", 0, &space, 1) == PONKO_OK);
    CHECK(ponko_ctx_set_actions(ctx, map) == PONKO_OK);
    ponko_action_map_free(map);
    /* Count checks name a reflected component. */
    PonkoWorld* w = ponko_ctx_world(ctx);
    PonkoField field = {"points", PONKO_FIELD_U32, 0};
    uint32_t coin;
    CHECK(ponko_component_register(w, "Coin", 4, 4, &coin) == PONKO_OK);
    CHECK(ponko_component_reflect(w, coin, &field, 1) == PONKO_OK);
}
static void native_coverage(void) {
    static const uint8_t blob[] = {1, 2, 3, 4};
    Pak pak = {"blob.bin", blob, sizeof blob, 0};
    PonkoGame* game = ponko_game_new("c natives", 1, 1);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 0, native_setup, &pak, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    CHECK(pak.frees == 1);

    /* resource_scope: the resource and the world, lent together. */
    w = ponko_world_new();
    Points points = {1};
    uint32_t add = 41;
    CHECK(ponko_resource_set(w, "Points", &points, sizeof points, _Alignof(Points)) == PONKO_OK);
    CHECK(ponko_resource_scope(w, "Points", sizeof points, add_points, &add) == PONKO_OK);
    CHECK(ponko_resource_get(w, "Points", &points, sizeof points) == PONKO_OK && points.points == 42);
    CHECK(ponko_resource_scope(w, "Nothing", 4, add_points, &add) == PONKO_NOT_FOUND);
    CHECK(ponko_world_free(w) == PONKO_OK);

    /* Input scenarios: Space held on ticks 2-4, checked through the action. */
    PonkoInputScenario* s = ponko_input_scenario_new();
    CHECK(ponko_input_scenario_at(s, 2, 0, 0, PONKO_KEY_SPACE, 1) == PONKO_OK);
    CHECK(ponko_input_scenario_at(s, 5, 0, 0, PONKO_KEY_SPACE, 0) == PONKO_OK);
    CHECK(ponko_input_scenario_expect_action(s, 2, "jump", 1, 1, -1) == PONKO_OK);
    CHECK(ponko_input_scenario_expect_action(s, 5, "jump", -1, 0, 1) == PONKO_OK);
    CHECK(ponko_input_scenario_expect_count(s, 5, "Coin", 0) == PONKO_OK);
    CHECK(ponko_input_scenario_expect_resource(s, 5, "Score", "/points", "3") == PONKO_OK);
    CHECK(ponko_input_scenario_named(s, "jump") == PONKO_OK);
    CHECK(ponko_input_scenario_duration(s, 6) == PONKO_OK);
    CHECK(ponko_input_scenario_release_when_done(s) == PONKO_OK);
    uint64_t ticks = 0;
    size_t steps = 0, len = 0;
    CHECK(ponko_input_scenario_info(s, &ticks, &steps) == PONKO_OK && ticks == 6 && steps == 6);
    CHECK(ponko_input_scenario_to_json(s, NULL, 0, &len) == PONKO_OK && len > 0);
    char* json = malloc(len + 1);
    CHECK(ponko_input_scenario_to_json(s, json, len + 1, &len) == PONKO_OK);
    PonkoInputScenario* copy = NULL;
    CHECK(ponko_input_scenario_from_json(json, &copy) == PONKO_OK && copy);
    free(json);
    CHECK(ponko_input_scenario_from_json("{", &copy) == PONKO_INVALID_ARGUMENT && ponko_last_error_kind() == PONKO_ERROR_JSON);
    game = ponko_game_new("c scenario", 1, 1);
    CHECK(ponko_game_input_scenario(game, copy) == PONKO_OK);
    ponko_input_scenario_free(copy);
    ponko_input_scenario_free(s);
    CHECK(ponko_game_run_headless(game, ticks, scenario_setup, NULL, &w) == PONKO_OK);
    PonkoScenarioReport report;
    CHECK(ponko_world_scenario_report(w, &report) == PONKO_OK);
    /* Steps 0-3 held; the Score resource was never added, so step 5 fails on tick 5 (the last). */
    CHECK(report.status == PONKO_SCENARIO_FAILED && report.failed && report.failure_step == 5);
    CHECK(report.inputs_applied == 2 && report.checks_passed == 3);
    CHECK(ponko_world_scenario_report_json(w, NULL, 0, &len) == PONKO_OK && len > 0);
    CHECK(ponko_world_free(w) == PONKO_OK);
    bool was_running = true;
    CHECK(ponko_ctx_cancel_scenario(NULL, &was_running) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_start_scenario(NULL, NULL) == PONKO_INVALID_ARGUMENT);
}

/* Headless frames: a form typed into and clicked by node name. */
static void form_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoUiLayout fill = ponko_ui_layout_fill(), box = ponko_ui_layout_fixed(200, 30);
    uint64_t root = 0, name = 0, ok = 0;
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &fill, &root) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_text_input(ctx, PONKO_UI_SCREEN, root, "", &box, &name) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_button(ctx, PONKO_UI_SCREEN, root, "OK", &box, &ok) == PONKO_OK);
    CHECK(ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, root, "form") == PONKO_OK);
    CHECK(ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, name, "form.name") == PONKO_OK);
    CHECK(ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, ok, "form.ok") == PONKO_OK);
    bool shown = true;
    CHECK(ponko_ctx_ui_is_shown(ctx, PONKO_UI_SCREEN, ok, &shown) == PONKO_OK && !shown);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, root, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_OK);
    CHECK(ponko_ctx_ui_is_shown(ctx, PONKO_UI_SCREEN, ok, &shown) == PONKO_OK && shown);
    CHECK(ponko_ctx_ui_is_shown(ctx, PONKO_UI_SCREEN, 9999, &shown) == PONKO_NOT_FOUND);
}
/* Frame system: OK closes the form; `user` counts the closes. */
static void form_frame(PonkoCtx* ctx, void* user) {
    uint64_t ok = PONKO_UI_NONE, clicked = PONKO_UI_NONE, root = PONKO_UI_NONE;
    CHECK(ponko_ctx_ui_find(ctx, PONKO_UI_SCREEN, "form.ok", &ok) == PONKO_OK);
    CHECK(ponko_ctx_ui_take_click(ctx, PONKO_UI_SCREEN, &clicked) == PONKO_OK);
    if (clicked != PONKO_UI_NONE && clicked == ok) {
        CHECK(ponko_ctx_ui_pop_screen(ctx, PONKO_UI_SCREEN, &root) == PONKO_OK);
        ++*(int*)user;
    }
}
static void headless_frames_ui(void) {
    PonkoInputScenario* s = ponko_input_scenario_new();
    CHECK(ponko_input_scenario_type_text(s, 2, "form.name", "Ada") == PONKO_OK);
    CHECK(ponko_input_scenario_expect_ui(s, 2, "form.name", 1, 1, 1, "Ada") == PONKO_OK);
    CHECK(ponko_input_scenario_click(s, 4, "form.ok") == PONKO_OK);
    CHECK(ponko_input_scenario_expect_ui(s, 4, "form", -1, 0, -1, NULL) == PONKO_OK);
    CHECK(ponko_input_scenario_expect_ui(s, 4, "form.cancel", 0, -1, -1, NULL) == PONKO_OK);
    CHECK(ponko_input_scenario_expect_ui(s, 4, "form", 2, -1, -1, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_input_scenario_duration(s, 6) == PONKO_OK);
    int closed = 0;
    PonkoGame* game = ponko_game_new("c frames", 640, 480);
    CHECK(ponko_game_headless_frames(game, true) == PONKO_OK);
    CHECK(ponko_game_system(game, "form", form_frame, &closed) == PONKO_OK);
    CHECK(ponko_game_input_scenario(game, s) == PONKO_OK);
    ponko_input_scenario_free(s);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 6, form_setup, NULL, &w) == PONKO_OK);
    PonkoScenarioReport report;
    CHECK(ponko_world_scenario_report(w, &report) == PONKO_OK);
    CHECK(report.status == PONKO_SCENARIO_PASSED && report.inputs_applied == 2 && report.checks_passed == 3);
    CHECK(closed == 1);
    CHECK(ponko_world_free(w) == PONKO_OK);
    CHECK(ponko_game_headless_frames(NULL, true) == PONKO_INVALID_ARGUMENT);
}

/* The window's safe area: fed by hand and by a scenario step, and a HUD root laid out inside it. */
typedef struct SafeSeen {
    PonkoRect area[6], hud[6], backdrop[6];
    int frames;
} SafeSeen;
static void safe_area_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoUiLayout fill = ponko_ui_layout_fill();
    uint64_t backdrop = 0, hud = 0;
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &fill, &backdrop) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &fill, &hud) == PONKO_OK);
    CHECK(ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, backdrop, "backdrop") == PONKO_OK);
    CHECK(ponko_ctx_ui_set_name(ctx, PONKO_UI_SCREEN, hud, "hud") == PONKO_OK);
    bool on = true;
    CHECK(ponko_ctx_ui_inside_safe_area(ctx, PONKO_UI_SCREEN, hud, &on) == PONKO_OK && !on);
    CHECK(ponko_ctx_ui_set_inside_safe_area(ctx, PONKO_UI_SCREEN, hud, true) == PONKO_OK);
    CHECK(ponko_ctx_ui_inside_safe_area(ctx, PONKO_UI_SCREEN, hud, &on) == PONKO_OK && on);
    CHECK(ponko_ctx_ui_set_inside_safe_area(ctx, PONKO_UI_SCREEN, 9999, true) == PONKO_NOT_FOUND);
    CHECK(ponko_ctx_ui_show_hud(ctx, PONKO_UI_SCREEN, backdrop) == PONKO_OK);
    CHECK(ponko_ctx_ui_show_hud(ctx, PONKO_UI_SCREEN, hud) == PONKO_OK);
}
static void safe_area_frame(PonkoCtx* ctx, void* user) {
    SafeSeen* seen = user;
    uint64_t hud = PONKO_UI_NONE, backdrop = PONKO_UI_NONE;
    CHECK(seen->frames < 6);
    CHECK(ponko_ctx_window_safe_area(ctx, &seen->area[seen->frames]) == PONKO_OK);
    CHECK(ponko_ctx_ui_find(ctx, PONKO_UI_SCREEN, "hud", &hud) == PONKO_OK);
    CHECK(ponko_ctx_ui_find(ctx, PONKO_UI_SCREEN, "backdrop", &backdrop) == PONKO_OK);
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, hud, &seen->hud[seen->frames]) == PONKO_OK);
    CHECK(ponko_ctx_ui_rect(ctx, PONKO_UI_SCREEN, backdrop, &seen->backdrop[seen->frames]) == PONKO_OK);
    ++seen->frames;
}
static bool rect_is(PonkoRect r, float x0, float y0, float x1, float y1) {
    return r.min.x == x0 && r.min.y == y0 && r.max.x == x1 && r.max.y == y1;
}
static void safe_area_by_hand(PonkoCtx* ctx, void* user);
static void safe_area_c(void) {
    PonkoSafeInsets phone = {0, 0, 21, 47}, turned = {0, 47, 21, 0}, got;
    PonkoInputScenario* s = ponko_input_scenario_new();
    CHECK(ponko_input_scenario_safe_insets(s, 1, phone) == PONKO_OK);
    CHECK(ponko_input_scenario_safe_insets(s, 3, turned) == PONKO_OK);
    CHECK(ponko_input_scenario_safe_insets(NULL, 3, turned) == PONKO_INVALID_ARGUMENT);
    SafeSeen seen = {0};
    PonkoGame* game = ponko_game_new("c notch", 844, 390);
    CHECK(ponko_game_headless_frames(game, true) == PONKO_OK);
    CHECK(ponko_game_system(game, "frame", safe_area_frame, &seen) == PONKO_OK);
    CHECK(ponko_game_input_scenario(game, s) == PONKO_OK);
    ponko_input_scenario_free(s);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 5, safe_area_setup, NULL, &w) == PONKO_OK);
    PonkoScenarioReport report;
    CHECK(ponko_world_scenario_report(w, &report) == PONKO_OK);
    CHECK(report.status == PONKO_SCENARIO_PASSED && report.inputs_applied == 2);
    CHECK(ponko_world_free(w) == PONKO_OK);
    CHECK(seen.frames == 5);
    CHECK(rect_is(seen.area[0], 0, 0, 844, 390));
    for (int i = 1; i < 3; ++i) CHECK(rect_is(seen.area[i], 47, 0, 844, 369));
    for (int i = 3; i < 5; ++i) CHECK(rect_is(seen.area[i], 0, 0, 797, 369));
    for (int i = 0; i < 5; ++i) {
        CHECK(rect_is(seen.hud[i], seen.area[i].min.x, seen.area[i].min.y, seen.area[i].max.x, seen.area[i].max.y));
        CHECK(rect_is(seen.backdrop[i], 0, 0, 844, 390)); /* reaches under the notch */
    }

    /* By hand: fed, read back, and a negative inset counts as 0. */
    game = ponko_game_new("c notch by hand", 844, 390);
    CHECK(ponko_game_run_headless(game, 0, safe_area_by_hand, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    CHECK(ponko_ctx_window_safe_area(NULL, &seen.area[0]) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_window_safe_insets(NULL, &got) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_window_feed_safe_insets(NULL, phone) == PONKO_INVALID_ARGUMENT);
}
static void safe_area_by_hand(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoRect area;
    PonkoSafeInsets got, phone = {-5, 0, 21, 47};
    CHECK(ponko_ctx_window_safe_area(ctx, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_window_safe_area(ctx, &area) == PONKO_OK && rect_is(area, 0, 0, 844, 390));
    CHECK(ponko_ctx_window_feed_safe_insets(ctx, phone) == PONKO_OK);
    CHECK(ponko_ctx_window_safe_insets(ctx, &got) == PONKO_OK);
    CHECK(got.top == 0 && got.right == 0 && got.bottom == 21 && got.left == 47);
    CHECK(ponko_ctx_window_safe_area(ctx, &area) == PONKO_OK && rect_is(area, 47, 0, 844, 369));
}

/* A menu that opens on its Play button, a HUD that Escape leaves up, and a
   glTF file made solid in one call. */
static void focus_and_hud_setup(PonkoCtx* ctx, void* user) {
    const char* path = user;
    const uint64_t s = PONKO_UI_SCREEN;
    PonkoUiLayout column = ponko_ui_layout_default();
    column.flex_direction = PONKO_UI_FLEX_COLUMN;
    column.width = (PonkoUiLength){PONKO_UI_LENGTH_POINTS, 300};
    column.height = (PonkoUiLength){PONKO_UI_LENGTH_POINTS, 200};
    PonkoUiLayout row = ponko_ui_layout_default();
    row.width = (PonkoUiLength){PONKO_UI_LENGTH_POINTS, 120};
    row.height = (PonkoUiLength){PONKO_UI_LENGTH_POINTS, 40};
    uint64_t hud, count, menu, play, quit;
    CHECK(ponko_ctx_ui_add_panel(ctx, s, PONKO_UI_NONE, &column, &hud) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_label(ctx, s, hud, "0 / 5", &row, &count) == PONKO_OK);
    CHECK(ponko_ctx_ui_show_hud(ctx, s, hud) == PONKO_OK);
    bool flag = false;
    CHECK(ponko_ctx_ui_is_hud(ctx, s, hud, &flag) == PONKO_OK && flag);
    CHECK(ponko_ctx_ui_wants_keyboard(ctx, &flag) == PONKO_OK && !flag);
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, s, hud, PONKO_UI_SCREEN_MODE_MODELESS) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_ui_show_hud(ctx, s, count) == PONKO_INVALID_ARGUMENT);

    CHECK(ponko_ctx_ui_add_panel(ctx, s, PONKO_UI_NONE, &column, &menu) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_button(ctx, s, menu, "Quit", &row, &quit) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_button(ctx, s, menu, "Play", &row, &play) == PONKO_OK);
    CHECK(ponko_ctx_ui_set_initial_focus(ctx, s, play, true) == PONKO_OK);
    CHECK(ponko_ctx_ui_set_focus(ctx, s, play) == PONKO_INVALID_ARGUMENT); /* not on a screen yet */
    CHECK(ponko_ctx_ui_push_screen_with_mode(ctx, s, menu, PONKO_UI_SCREEN_MODE_MODAL) == PONKO_OK);
    uint64_t focus = 0;
    CHECK(ponko_ctx_ui_focus(ctx, s, &focus) == PONKO_OK && focus == play);
    CHECK(ponko_ctx_ui_set_focus(ctx, s, count) == PONKO_INVALID_ARGUMENT); /* a label on a HUD */
    CHECK(ponko_ctx_ui_set_focus(ctx, s, 9999) == PONKO_NOT_FOUND);
    PonkoVec2 window = {800, 600};
    PonkoUiInput enter = ponko_ui_input_default();
    enter.confirm = true;
    CHECK(ponko_ctx_ui_frame(ctx, s, window, &enter) == PONKO_OK);
    PonkoUiEvent event;
    CHECK(ponko_ctx_ui_next_event(ctx, s, &event) == PONKO_OK);
    CHECK(event.kind == PONKO_UI_EVENT_CLICKED && event.id == play);
    CHECK(ponko_ctx_ui_set_focus(ctx, s, quit) == PONKO_OK);
    CHECK(ponko_ctx_ui_clear_focus(ctx, s) == PONKO_OK);
    CHECK(ponko_ctx_ui_focus(ctx, s, &focus) == PONKO_OK && focus == PONKO_UI_NONE);

    /* A still screen costs no layout: the count stays over its frames. */
    PonkoUiInput still = ponko_ui_input_default();
    CHECK(ponko_ctx_ui_frame(ctx, s, window, &still) == PONKO_OK);
    uint64_t laid_out = 0, passes = 0;
    CHECK(ponko_ctx_ui_layout_passes(ctx, s, &laid_out) == PONKO_OK && laid_out >= 1);
    for (int i = 0; i < 10; ++i) {
        CHECK(ponko_ctx_ui_set_text(ctx, s, count, "0 / 5") == PONKO_OK); /* the same text */
        CHECK(ponko_ctx_ui_frame(ctx, s, window, &still) == PONKO_OK);
    }
    CHECK(ponko_ctx_ui_layout_passes(ctx, s, &passes) == PONKO_OK && passes == laid_out);
    CHECK(ponko_ctx_ui_set_text(ctx, s, count, "1 / 5") == PONKO_OK);
    CHECK(ponko_ctx_ui_frame(ctx, s, window, &still) == PONKO_OK);
    CHECK(ponko_ctx_ui_layout_passes(ctx, s, &passes) == PONKO_OK && passes == laid_out + 1);
    CHECK(ponko_ctx_ui_layout_passes(ctx, s, NULL) == PONKO_INVALID_ARGUMENT);

    /* Escape closes the menu; the HUD stays. */
    PonkoUiInput escape = ponko_ui_input_default();
    escape.cancel = true;
    CHECK(ponko_ctx_ui_frame(ctx, s, window, &escape) == PONKO_OK);
    CHECK(ponko_ctx_ui_is_shown(ctx, s, count, &flag) == PONKO_OK && flag);
    CHECK(ponko_ctx_ui_is_shown(ctx, s, play, &flag) == PONKO_OK && !flag);
    CHECK(ponko_ctx_ui_hide_hud(ctx, s, hud) == PONKO_OK);
    CHECK(ponko_ctx_ui_is_shown(ctx, s, count, &flag) == PONKO_OK && !flag);

    /* A 4 x 4 m floor at y = 0, solid in one call, scaled or as a hull. */
    PonkoCollisionMesh floor, again, wide;
    PonkoVec3 one = {1, 1, 1}, twice = {2, 1, 2};
    CHECK(ponko_ctx_load_collision_mesh(ctx, path, one, false, &floor) == PONKO_OK);
    CHECK(ponko_ctx_load_collision_mesh(ctx, path, one, false, &again) == PONKO_OK && again.id == floor.id);
    CHECK(ponko_ctx_load_collision_mesh(ctx, path, twice, false, &wide) == PONKO_OK && wide.id != floor.id);
    CHECK(ponko_ctx_load_collision_mesh(ctx, path, one, true, &wide) == PONKO_INVALID_ARGUMENT); /* a flat hull */
    CHECK(ponko_ctx_load_collision_mesh(ctx, "no/such.glb", one, false, &wide) == PONKO_NOT_FOUND);
    CHECK(ponko_ctx_load_collision_mesh(ctx, path, one, false, NULL) == PONKO_INVALID_ARGUMENT);
    /* One mesh at any size: a collider scaled where it is used. */
    PonkoCollider3D mirrored = ponko_collider3d_scaled_mesh(floor, (PonkoVec3){-1, 1, 2});
    CHECK(mirrored.shape == PONKO_SHAPE3D_SCALED_MESH && mirrored.mesh.id == floor.id);
    CHECK(mirrored.size.x == -1 && mirrored.size.z == 2 && mirrored.friction == 0.5f);
}
static void focus_huds_and_collision_files(void) {
    char path[1024];
    snprintf(path, sizeof path, "%s/ponko-c-floor.gltf", temp_dir());
    FILE* f = fopen(path, "w");
    CHECK(f);
    fputs("{\"asset\": {\"version\": \"2.0\"}, \"scene\": 0, \"scenes\": [{\"nodes\": [0]}], \"nodes\": [{\"mesh\": 0}],"
          " \"meshes\": [{\"primitives\": [{\"attributes\": {\"POSITION\": 0}, \"indices\": 1}]}],"
          " \"buffers\": [{\"byteLength\": 60, \"uri\": \"data:application/octet-stream;base64,"
          "AAAAwAAAAAAAAABAAAAAQAAAAAAAAABAAAAAQAAAAAAAAADAAAAAwAAAAAAAAADAAAABAAIAAAACAAMA\"}],"
          " \"bufferViews\": [{\"buffer\": 0, \"byteLength\": 48}, {\"buffer\": 0, \"byteOffset\": 48, \"byteLength\": 12}],"
          " \"accessors\": [{\"bufferView\": 0, \"componentType\": 5126, \"count\": 4, \"type\": \"VEC3\","
          " \"min\": [-2, 0, -2], \"max\": [2, 0, 2]}, {\"bufferView\": 1, \"componentType\": 5123, \"count\": 6, \"type\": \"SCALAR\"}]}",
          f);
    fclose(f);
    PonkoGame* game = ponko_game_new("c focus and huds", 1, 1); CHECK(game);
    CHECK(ponko_game_physics3d(game, (PonkoVec3){0, -9.81f, 0}) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 0, focus_and_hud_setup, path, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    remove(path);
}
_Static_assert(sizeof(PonkoDebugStyle) == 16 && offsetof(PonkoDebugStyle, screen) == 13,
               "ABI 2 debug style layout");
_Static_assert(sizeof(PonkoDebugStyleEx) == 28 && offsetof(PonkoDebugStyleEx, space) == 24,
               "Version 1 debug style descriptor layout");

static void debug_draw_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoColor red = {1, 0, 0, 1};
    CHECK(ponko_ctx_debug_set_enabled(ctx, true) == PONKO_OK);
    CHECK(ponko_ctx_debug_clear(ctx) == PONKO_OK);
    PonkoDebugStyle style = ponko_debug_style_default();
    style.seconds = 2; style.on_top = true;
    CHECK(ponko_ctx_debug_line3d(ctx, (PonkoVec3){0, 0, 0}, (PonkoVec3){0, 1, 0}, red, &style) == PONKO_OK);
    CHECK(ponko_ctx_debug_box2d(ctx, (PonkoVec2){0, 0}, (PonkoVec2){4, 4}, red, NULL) == PONKO_OK);
    CHECK(ponko_ctx_debug_text2d(ctx, (PonkoVec2){8, 8}, "hi", red, NULL) == PONKO_OK);
    PonkoDebugStyleEx pixel_style = {.struct_size = sizeof pixel_style, .version = PONKO_DEBUG_STYLE_EX_VERSION};
    CHECK(ponko_debug_style_ex_defaults(&pixel_style) == PONKO_OK);
    pixel_style.space = PONKO_DRAW_SPACE_SCREEN;
    CHECK(ponko_ctx_debug_line2d_ex(ctx, (PonkoVec2){1, 1}, (PonkoVec2){2, 2}, red, &pixel_style) == PONKO_OK);
    CHECK(ponko_ctx_debug_show(ctx, PONKO_DEBUG_VIEW_NAVMESH, true) == PONKO_OK);
    CHECK(ponko_ctx_debug_show(ctx, 77, true) == PONKO_INVALID_ARGUMENT);
    PonkoDebugStats stats = {0};
    CHECK(ponko_ctx_debug_stats(ctx, &stats) == PONKO_OK);
    CHECK(stats.lines_3d == 1 && stats.lines_2d == 5 && stats.labels == 1);
    bool on = false;
    CHECK(ponko_ctx_debug_showing(ctx, PONKO_DEBUG_VIEW_NAVMESH, &on) == PONKO_OK && on);
}
static void debug_drawing(void) {
    PonkoGame* game = ponko_game_new("c debug drawing", 1, 1); CHECK(game);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 0, debug_draw_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
}
/* Versioned descriptors, retargeting and prediction (C ABI that can grow). */
typedef struct { int32_t x, vx; } Walker;
static bool walker_step(void* user, void* state, size_t state_size, const void* input, size_t input_size) {
    CHECK(state_size == sizeof(Walker) && input_size == sizeof(int32_t));
    Walker* w = state;
    w->vx += *(const int32_t*)input;
    w->x += w->vx;
    if (user) ++*(int*)user;
    return true;
}
static void humanoid(PonkoCtx* ctx, uint64_t e, const char* const names[15], float size) {
    static const int parents[15] = {-1, 0, 1, 1, 3, 4, 1, 6, 7, 0, 9, 10, 0, 12, 13};
    static const float at[15][3] = {{0, 1, 0}, {0, .1f, 0}, {0, .5f, 0}, {.2f, .4f, 0}, {.3f, 0, 0}, {.3f, 0, 0},
        {-.2f, .4f, 0}, {-.3f, 0, 0}, {-.3f, 0, 0}, {.1f, -.05f, 0}, {0, -.45f, 0}, {0, -.45f, 0},
        {-.1f, -.05f, 0}, {0, -.45f, 0}, {0, -.45f, 0}};
    PonkoJoint joints[15];
    for (int i = 0; i < 15; ++i) {
        joints[i].name = names[i];
        joints[i].parent = parents[i];
        joints[i].rest = ponko_transform3d_at((PonkoVec3){at[i][0] * size, at[i][1] * size, at[i][2] * size});
    }
    CHECK(ponko_ctx_set_skeleton(ctx, e, joints, 15) == PONKO_OK);
}
static void descriptors_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    /* Lighting: the sun's size for contact-hardening shadows. */
    PonkoLightingEx l = {.struct_size = sizeof l, .version = PONKO_LIGHTING_EX_VERSION};
    CHECK(ponko_ctx_lighting_ex(ctx, &l) == PONKO_OK);
    l.lighting.shadow_filter = 4;
    l.shadow_sun_size = 1.5f;
    CHECK(ponko_ctx_set_lighting_ex(ctx, &l) == PONKO_OK);
    PonkoLightingEx back = {.struct_size = sizeof back, .version = PONKO_LIGHTING_EX_VERSION};
    CHECK(ponko_ctx_lighting_ex(ctx, &back) == PONKO_OK && back.shadow_sun_size == 1.5f);
    l.shadow_sun_size = 20;
    CHECK(ponko_ctx_set_lighting_ex(ctx, &l) == PONKO_INVALID_ARGUMENT);
    PonkoLightingEx unset = {0};
    CHECK(ponko_ctx_lighting_ex(ctx, &unset) == PONKO_INVALID_ARGUMENT);
    /* A material with a stencil in one description. */
    PonkoMaterialDescEx d = {.struct_size = sizeof d, .version = PONKO_MATERIAL_DESC_EX_VERSION};
    CHECK(ponko_material_desc_ex_defaults(&d) == PONKO_OK && d.desc.roughness == .6f);
    d.has_stencil = true;
    d.stencil = ponko_stencil_mark(1);
    PonkoMaterial m;
    CHECK(ponko_ctx_create_material_ex(ctx, &d, &m) == PONKO_OK);
    CHECK(ponko_ctx_update_material_ex(ctx, m, &d) == PONKO_OK);
    /* Retargeting between two humanoids, the second half the size. */
    static const char* const a[15] = {"Hips", "Spine", "Head", "LeftArm", "LeftForeArm", "LeftHand", "RightArm",
        "RightForeArm", "RightHand", "LeftUpLeg", "LeftLeg", "LeftFoot", "RightUpLeg", "RightLeg", "RightFoot"};
    static const char* const b[15] = {"pelvis", "spine_01", "head", "upperarm_l", "lowerarm_l", "hand_l", "upperarm_r",
        "lowerarm_r", "hand_r", "thigh_l", "calf_l", "foot_l", "thigh_r", "calf_r", "foot_r"};
    PonkoWorld* w = ponko_ctx_world(ctx);
    uint64_t source, target;
    CHECK(ponko_spawn(w, NULL, 0, &source) == PONKO_OK && ponko_spawn(w, NULL, 0, &target) == PONKO_OK);
    humanoid(ctx, source, a, 1);
    humanoid(ctx, target, b, .5f);
    PonkoHumanoidMap *source_map = NULL, *target_map = NULL;
    CHECK(ponko_ctx_detect_humanoid_map(ctx, source, &source_map) == PONKO_OK);
    CHECK(ponko_ctx_detect_humanoid_map(ctx, target, &target_map) == PONKO_OK);
    size_t missing = 1;
    CHECK(ponko_humanoid_map_missing(target_map, NULL, 0, &missing) == PONKO_OK && missing == 0);
    int32_t joint = -1;
    CHECK(ponko_humanoid_map_get(target_map, PONKO_HUMAN_BONE_RIGHT_FOOT, &joint) == PONKO_OK && joint == 14);
    PonkoRetargeter* r = NULL;
    CHECK(ponko_ctx_create_retargeter(ctx, source, source_map, target, target_map, &r) == PONKO_OK);
    ponko_humanoid_map_free(source_map);
    ponko_humanoid_map_free(target_map);
    float scale = 0;
    CHECK(ponko_retargeter_scale(r, &scale) == PONKO_OK && fabsf(scale - .5f) < 1e-4f);
    CHECK(ponko_ctx_retarget_pose(ctx, r, source, target) == PONKO_OK);
    PonkoClip* idle = NULL;
    PonkoClip* idle_b = NULL;
    CHECK(ponko_clip_new("idle", &idle) == PONKO_OK);
    CHECK(ponko_retarget_clip(r, idle, &idle_b) == PONKO_OK && idle_b);
    ponko_clip_free(idle_b);
    ponko_clip_free(idle);
    ponko_retargeter_free(r);
    CHECK(strcmp(ponko_human_bone_name(PONKO_HUMAN_BONE_LEFT_UPPER_LEG), "left upper leg") == 0);
}
static void descriptors_retargeting_and_prediction(void) {
    /* A header that wasn't set, or one from a newer ponko.h, is refused. */
    PonkoQueryFilterEx f = {.struct_size = sizeof f, .version = PONKO_QUERY_FILTER_EX_VERSION};
    CHECK(ponko_query_filter_ex_defaults(&f) == PONKO_OK && f.groups.memberships == UINT32_MAX && !f.sensors);
    PonkoNavBakeSettingsEx nav = {.struct_size = sizeof nav, .version = PONKO_NAV_BAKE_SETTINGS_EX_VERSION + 1};
    CHECK(ponko_nav_bake_settings_ex_defaults(&nav) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_last_error_kind() == PONKO_ERROR_INVALID);
    PonkoColliderEx c = {.struct_size = sizeof c, .version = PONKO_COLLIDER_EX_VERSION};
    CHECK(ponko_collider_ex_from(ponko_collider_circle(2), &c) == PONKO_OK && c.collider.radius == 2);
    CHECK(c.groups.filter == UINT32_MAX && c.point_count == 0);

    PonkoGame* game = ponko_game_new("descriptors", 64, 64);
    PonkoWorld* world = NULL;
    CHECK(ponko_game_run_headless(game, 0, descriptors_setup, NULL, &world) == PONKO_OK);
    CHECK(ponko_world_free(world) == PONKO_OK);

    /* Prediction: three ticks ahead, then the server's state for the first. */
    Walker start = {0, 0};
    PonkoPrediction* p = NULL;
    CHECK(ponko_prediction_new(&start, sizeof start, sizeof(int32_t), 0, &p) == PONKO_OK);
    const int32_t inputs[3] = {1, 2, 3};
    uint16_t seq = 99;
    for (int i = 0; i < 3; ++i) {
        CHECK(ponko_prediction_predict(p, &inputs[i], walker_step, NULL, &seq) == PONKO_OK && seq == i);
    }
    Walker server = {11, 1}; /* the server pushed the walker 10 further */
    PonkoReconciled result;
    Walker before;
    int replayed = 0;
    CHECK(ponko_prediction_reconcile(p, 0, &server, walker_step, &replayed, &result, &before) == PONKO_OK);
    CHECK(result.outcome == PONKO_RECONCILE_CORRECTED && result.replayed == 2 && replayed == 2);
    CHECK(before.x == 10 && before.vx == 6);
    Walker now;
    CHECK(ponko_prediction_state(p, &now) == PONKO_OK && now.x == 20 && now.vx == 6);
    uint16_t seqs[4];
    int32_t unacked[4];
    size_t n = 0;
    CHECK(ponko_prediction_unacked(p, seqs, unacked, 4, &n) == PONKO_OK && n == 2);
    CHECK(seqs[0] == 1 && seqs[1] == 2 && unacked[0] == 2 && unacked[1] == 3);
    CHECK(ponko_prediction_predict(p, &inputs[0], NULL, NULL, NULL) == PONKO_INVALID_ARGUMENT);
    ponko_prediction_free(p);

    /* The server's queue puts inputs back in order. */
    PonkoInputQueue* q = NULL;
    CHECK(ponko_input_queue_new(sizeof(int32_t), 0, &q) == PONKO_OK);
    uint32_t push;
    CHECK(ponko_input_queue_push(q, 1, &inputs[1], &push) == PONKO_OK && push == PONKO_INPUT_ADDED);
    CHECK(ponko_input_queue_push(q, 0, &inputs[0], &push) == PONKO_OK && push == PONKO_INPUT_ADDED);
    CHECK(ponko_input_queue_push(q, 0, &inputs[0], &push) == PONKO_OK && push == PONKO_INPUT_DUPLICATE);
    bool found;
    int32_t input;
    CHECK(ponko_input_queue_pop(q, &found, &seq, &input) == PONKO_OK && found && seq == 0 && input == 1);
    CHECK(ponko_input_queue_pop(q, &found, &seq, &input) == PONKO_OK && found && seq == 1 && input == 2);
    CHECK(ponko_input_queue_pop(q, &found, &seq, &input) == PONKO_OK && !found);
    bool has;
    CHECK(ponko_input_queue_ack(q, &has, &seq) == PONKO_OK && has && seq == 1);
    ponko_input_queue_free(q);

    /* A status this machine can't run: its own value, next to the old ones. */
    CHECK(PONKO_UNSUPPORTED == 5 && PONKO_PANIC == 4);
}

/* Game states: a title, play, and a pause pushed over play. */
enum { ST_TITLE, ST_PLAYING, ST_PAUSED };
typedef struct { char what; uint32_t state; uint64_t tick; } StateEntry;
typedef struct {
    StateEntry log[32];
    int len;
    uint32_t seen[16];  /* the state the last sim system saw, per tick */
    int played;         /* runs of the system that runs only while playing */
    int checked, even;  /* condition callback calls, and runs it allowed */
} StateRun;

static void state_note(PonkoCtx* ctx, StateRun* r, char what) {
    uint32_t s; CHECK(ponko_ctx_state(ctx, &s) == PONKO_OK);
    CHECK(r->len < 32);
    r->log[r->len++] = (StateEntry){what, s, ponko_ctx_sim_tick(ctx)};
}
static void state_entered(PonkoCtx* ctx, void* user) { state_note(ctx, user, 'e'); }
static void state_left(PonkoCtx* ctx, void* user) { state_note(ctx, user, 'x'); }

static void state_menus(PonkoCtx* ctx, void* user) {
    (void)user;
    uint32_t s; uint64_t ticks; bool in;
    switch (ponko_ctx_sim_tick(ctx)) {
    case 1:
        CHECK(ponko_ctx_set_state(ctx, ST_PLAYING) == PONKO_OK);
        /* Asked for, not applied: the rest of this tick is still on the title. */
        CHECK(ponko_ctx_state(ctx, &s) == PONKO_OK && s == ST_TITLE);
        break;
    case 4: CHECK(ponko_ctx_push_state(ctx, ST_PAUSED) == PONKO_OK); break;
    case 6:
        CHECK(ponko_ctx_state_in_stack(ctx, ST_PLAYING, &in) == PONKO_OK && in);
        CHECK(ponko_ctx_state_ticks(ctx, &ticks) == PONKO_OK && ticks == 2); /* ticks 5 and 6 */
        CHECK(ponko_ctx_save_world(ctx, "states") == PONKO_OK);
        break;
    case 7: CHECK(ponko_ctx_pop_state(ctx) == PONKO_OK); break;
    case 9: CHECK(ponko_ctx_set_state(ctx, ST_TITLE) == PONKO_OK); break;
    case 11:
        /* The save brings back the pause over play, at once: a load isn't a change. */
        CHECK(ponko_ctx_load_world(ctx, "states") == PONKO_OK);
        CHECK(ponko_ctx_state(ctx, &s) == PONKO_OK && s == ST_PAUSED);
        CHECK(ponko_ctx_state_in_stack(ctx, ST_PLAYING, &in) == PONKO_OK && in);
        CHECK(ponko_ctx_state_ticks(ctx, &ticks) == PONKO_OK && ticks == 2);
        CHECK(ponko_ctx_delete_world(ctx, "states") == PONKO_OK);
        break;
    case 12: CHECK(ponko_ctx_pop_state(ctx) == PONKO_OK); break;
    case 13: /* Back to play: its count from before the pause goes on (ticks 2-4, then 13). */
        CHECK(ponko_ctx_state_ticks(ctx, &ticks) == PONKO_OK && ticks == 4);
        break;
    default: break;
    }
}

static void state_watch(PonkoCtx* ctx, void* user) {
    StateRun* r = user;
    uint64_t t = ponko_ctx_sim_tick(ctx);
    if (t < 16) CHECK(ponko_ctx_state(ctx, &r->seen[t]) == PONKO_OK);
}
static void state_play(PonkoCtx* ctx, void* user) { (void)ctx; ((StateRun*)user)->played++; }
static bool state_even(PonkoCtx* ctx, void* user) {
    ((StateRun*)user)->checked++;
    return ponko_ctx_sim_tick(ctx) % 2 == 0;
}
static void state_counted(PonkoCtx* ctx, void* user) { (void)ctx; ((StateRun*)user)->even++; }

static void states(void) {
    StateRun r = {0};
    PonkoGame* g = ponko_game_new("states", 1, 1); CHECK(g);
    CHECK(ponko_game_data_dir(g, "ponko-c-states") == PONKO_OK);
    CHECK(ponko_game_state(g, ST_TITLE) == PONKO_OK);
    for (uint32_t s = ST_TITLE; s <= ST_PAUSED; s++) {
        CHECK(ponko_game_on_enter(g, s, state_entered, &r) == PONKO_OK);
        CHECK(ponko_game_on_exit(g, s, state_left, &r) == PONKO_OK);
    }
    CHECK(ponko_game_sim_system(g, "menus", 1, state_menus, &r) == PONKO_OK);
    CHECK(ponko_game_add_system_in_state(g, PONKO_PHASE_SIM_LAST, "play", 1, ST_PLAYING, state_play, &r) == PONKO_OK);
    CHECK(ponko_game_add_system(g, PONKO_PHASE_SIM_LAST, "watch", 1, state_watch, &r) == PONKO_OK);
    CHECK(ponko_game_add_system_if(g, PONKO_PHASE_SIM_LAST, "even", 1, state_even, &r, state_counted, &r) == PONKO_OK);
    CHECK(ponko_game_add_system_if(g, PONKO_PHASE_SIM_LAST, NULL, 1, NULL, NULL, state_counted, &r) == PONKO_INVALID_ARGUMENT);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(g, 15, NULL, NULL, &w) == PONKO_OK);
    ponko_world_free(w);

    /* Every change applied at the end of the tick that asked for it, with
       its on-exit and on-enter work then; push and pop leave the state
       below alone; the load ran none. */
    const StateEntry want[] = {
        {'e', ST_TITLE, 0},
        {'x', ST_TITLE, 1}, {'e', ST_PLAYING, 1},
        {'e', ST_PAUSED, 4},
        {'x', ST_PAUSED, 7},
        {'x', ST_PLAYING, 9}, {'e', ST_TITLE, 9},
        {'x', ST_PAUSED, 12},
    };
    CHECK(r.len == (int)(sizeof want / sizeof want[0]));
    for (int i = 0; i < r.len; i++) {
        if (r.log[i].what != want[i].what || r.log[i].state != want[i].state || r.log[i].tick != want[i].tick) {
            fprintf(stderr, "state log %d: %c %u %llu\n", i, r.log[i].what, r.log[i].state, (unsigned long long)r.log[i].tick);
            abort();
        }
    }
    const uint32_t seen[15] = {ST_TITLE, ST_TITLE, ST_PLAYING, ST_PLAYING, ST_PLAYING, ST_PAUSED, ST_PAUSED, ST_PAUSED,
                               ST_PLAYING, ST_PLAYING, ST_TITLE, ST_PAUSED, ST_PAUSED, ST_PLAYING, ST_PLAYING};
    for (int t = 0; t < 15; t++) CHECK(r.seen[t] == seen[t]);
    CHECK(r.played == 7); /* ticks 2-4, 8-9, 13-14 */
    CHECK(r.checked == 15 && r.even == 8);
}

/* Camera rigs: each is a versioned descriptor holding its settings and state. */
static void camera_rigs_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoFollowCamera2D follow = {.struct_size = sizeof follow, .version = PONKO_FOLLOW_CAMERA2D_VERSION};
    CHECK(ponko_follow_camera2d_defaults(&follow) == PONKO_OK && follow.smooth_time > 0);
    follow.look_ahead = (PonkoVec2){32, 0};
    CHECK(ponko_follow_camera2d_update(&follow, ctx, (PonkoVec2){500, 300}, 1.0f / 60) == PONKO_OK);
    CHECK(follow.started);
    CHECK(ponko_follow_camera2d_snap(&follow, ctx, (PonkoVec2){600, 300}) == PONKO_OK);
    PonkoVec2 screen;
    CHECK(ponko_ctx_world_to_screen(ctx, (PonkoVec2){600, 300}, &screen) == PONKO_OK && screen.x == 400 && screen.y == 300);
    PonkoVec2 world;
    CHECK(ponko_ctx_screen_to_world(ctx, screen, &world) == PONKO_OK && world.x == 600);

    PonkoPixelPerfect pixels = {.struct_size = sizeof pixels, .version = PONKO_PIXEL_PERFECT_VERSION};
    CHECK(ponko_pixel_perfect_defaults(&pixels) == PONKO_OK);
    CHECK(ponko_pixel_perfect_apply(&pixels, ctx) == PONKO_OK);
    PonkoRect bars;
    CHECK(ponko_pixel_perfect_screen_rect(&pixels, ctx, &bars) == PONKO_OK && bars.max.x - bars.min.x == 640);

    PonkoCameraShake shake = {.struct_size = sizeof shake, .version = PONKO_CAMERA_SHAKE_VERSION};
    CHECK(ponko_camera_shake_defaults(&shake) == PONKO_OK);
    CHECK(ponko_camera_shake_add_trauma(&shake, 1) == PONKO_OK && ponko_camera_shake_update(&shake, 0.5f) == PONKO_OK);
    CHECK(shake.trauma == 0.5f);
    CHECK(ponko_camera_shake_apply2d(&shake, ctx) == PONKO_OK && ponko_camera_shake_apply3d(&shake, ctx) == PONKO_OK);

    PonkoOrbitCamera orbit = {.struct_size = sizeof orbit, .version = PONKO_ORBIT_CAMERA_VERSION};
    CHECK(ponko_orbit_camera_defaults(&orbit) == PONKO_OK);
    CHECK(ponko_orbit_camera_mouse_controls(&orbit, ctx) == PONKO_OK);
    CHECK(ponko_orbit_camera_update(&orbit, ctx, 1.0f / 60) == PONKO_OK);
    bool found = false;
    PonkoVec3 ground;
    CHECK(ponko_ctx_screen_to_ground(ctx, (PonkoVec2){400, 300}, 0, &found, &ground) == PONKO_OK && found);
    CHECK(fabsf(ground.x) < 1e-3f && fabsf(ground.z) < 1e-3f);
    bool visible = false;
    CHECK(ponko_ctx_world_to_screen3d(ctx, ground, &visible, &screen) == PONKO_OK && visible);
    PonkoVec3 ahead;
    CHECK(ponko_ctx_screen_to_world3d(ctx, (PonkoVec2){400, 300}, 2, &ahead) == PONKO_OK);

    PonkoFirstPersonCamera eyes = {.struct_size = sizeof eyes, .version = PONKO_FIRST_PERSON_CAMERA_VERSION};
    CHECK(ponko_first_person_camera_defaults(&eyes) == PONKO_OK);
    CHECK(ponko_first_person_camera_look(&eyes, (PonkoVec2){100, 0}) == PONKO_OK && eyes.yaw < 0);
    CHECK(ponko_first_person_camera_update(&eyes, ctx, (PonkoVec3){0, 1.7f, 0}, 1.0f / 60) == PONKO_OK);
    PonkoVec3 walk, strafe;
    CHECK(ponko_first_person_camera_flat_forward(&eyes, &walk) == PONKO_OK && walk.y == 0 && walk.x > 0);
    CHECK(ponko_first_person_camera_flat_right(&eyes, &strafe) == PONKO_OK && strafe.z > 0);

    PonkoThirdPersonCamera chase = {.struct_size = sizeof chase, .version = PONKO_THIRD_PERSON_CAMERA_VERSION};
    CHECK(ponko_third_person_camera_defaults(&chase) == PONKO_OK);
    CHECK(ponko_third_person_camera_look(&chase, (PonkoVec2){0, 10}) == PONKO_OK);
    CHECK(ponko_third_person_camera_update(&chase, ctx, (PonkoVec3){0, 0, 0}, 1.0f / 60) == PONKO_OK);
    CHECK(ponko_third_person_camera_flat_forward(&chase, &walk) == PONKO_OK && fabsf(walk.z + 1) < 1e-5f);
    CHECK(ponko_third_person_camera_flat_right(&chase, &strafe) == PONKO_OK && fabsf(strafe.x - 1) < 1e-5f);
    CHECK(ponko_third_person_camera_update_physics(&chase, ctx, (PonkoVec3){0, 0, 0}, NULL, 1.0f / 60) == PONKO_OK);
}
static void camera_rigs(void) {
    float velocity = 0;
    CHECK(ponko_smooth_damp(0, 10, &velocity, 0, 0.1f) == 10 && velocity == 0);
    PonkoVec2 v2 = {0, 0};
    CHECK(ponko_smooth_damp_vec2((PonkoVec2){0, 0}, (PonkoVec2){1, 1}, &v2, 0.2f, 10).x > 0.99f);
    PonkoVec3 v3 = {0, 0, 0};
    CHECK(ponko_smooth_damp_vec3((PonkoVec3){0, 0, 0}, (PonkoVec3){1, 1, 1}, &v3, 0.2f, 10).z > 0.99f);
    CHECK(fabsf(ponko_damp(0, 10, 0.5f, 0.5f) - 5) < 1e-4f);
    CHECK(fabsf(ponko_damp_vec2((PonkoVec2){0, 0}, (PonkoVec2){4, 0}, 1, 1).x - 2) < 1e-4f);
    CHECK(fabsf(ponko_damp_vec3((PonkoVec3){0, 0, 0}, (PonkoVec3){0, 0, 4}, 1, 1).z - 2) < 1e-4f);
    CHECK(ponko_damp_angle(3, -3, 0.1f, 10) > 3);
    PonkoGame* game = ponko_game_new("camera rigs", 800, 600);
    CHECK(ponko_game_physics3d(game, (PonkoVec3){0, -9.81f, 0}) == PONKO_OK);
    PonkoWorld* world = NULL;
    CHECK(ponko_game_run_headless(game, 0, camera_rigs_setup, NULL, &world) == PONKO_OK);
    CHECK(ponko_world_free(world) == PONKO_OK);
}

/* The 3D picture's versioned descriptors: post (split toning, depth of field, sample count), soft
   particles, and cameras with a switch for particles. */
static void render_descriptors_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoPostEx post = {.struct_size = sizeof post, .version = PONKO_POST_EX_VERSION};
    CHECK(ponko_ctx_post_ex(ctx, &post) == PONKO_OK);
    CHECK(!post.dof_enabled && post.msaa == 4 && post.split_toning_shadows.r == .5f);
    post.split_toning_shadows = (PonkoColor){.35f, .5f, .6f, 1};
    post.split_toning_highlights = (PonkoColor){.65f, .5f, .35f, 1};
    post.dof_enabled = true;
    post.dof_focus_distance = 4;
    post.post.exposure = .5f;
    CHECK(ponko_ctx_set_post_ex(ctx, &post) == PONKO_OK);
    PonkoPostEx back = {.struct_size = sizeof back, .version = PONKO_POST_EX_VERSION};
    CHECK(ponko_ctx_post_ex(ctx, &back) == PONKO_OK && back.dof_enabled && back.dof_focus_distance == 4);
    CHECK(back.split_toning_highlights.r == .65f && back.post.exposure == .5f);
    PonkoPost old;
    CHECK(ponko_ctx_post(ctx, &old) == PONKO_OK && old.exposure == .5f);
    post.dof_blur = 1; /* above 0.05 */
    CHECK(ponko_ctx_set_post_ex(ctx, &post) == PONKO_INVALID_ARGUMENT);
    PonkoPostEx unset = {0};
    CHECK(ponko_ctx_post_ex(ctx, &unset) == PONKO_INVALID_ARGUMENT);
    /* Version 2: thin objects in reflections. A version 1 caller leaves them as they are. */
    post.dof_blur = .01f;
    CHECK(!back.ssr_thin_objects);
    post.ssr_thin_objects = true;
    CHECK(ponko_ctx_set_post_ex(ctx, &post) == PONKO_OK);
    PonkoPostEx v1 = {.struct_size = sizeof v1, .version = 1};
    CHECK(ponko_ctx_post_ex(ctx, &v1) == PONKO_OK && !v1.ssr_thin_objects); /* not written */
    CHECK(ponko_ctx_set_post_ex(ctx, &v1) == PONKO_OK);
    CHECK(ponko_ctx_post_ex(ctx, &back) == PONKO_OK && back.ssr_thin_objects);
    post.ssr_thin_objects = false;
    CHECK(ponko_ctx_set_post_ex(ctx, &post) == PONKO_OK);
    /* Two looks blended, and volumes by where the camera is. */
    PonkoPostEx day = {.struct_size = sizeof day, .version = PONKO_POST_EX_VERSION};
    PonkoPostEx cave = day, blended = day;
    CHECK(ponko_post_ex_defaults(&day) == PONKO_OK && ponko_post_ex_defaults(&cave) == PONKO_OK);
    cave.post.exposure = 1;
    CHECK(ponko_post_ex_lerp(&day, &cave, .25f, &blended) == PONKO_OK && blended.post.exposure == .25f);
    PonkoPostVolume volume = {{-10, -1, -40}, {10, 5, -20}, 4, &cave};
    CHECK(ponko_post_blend_volumes(&day, &volume, 1, (PonkoVec3){0, 2, -30}, &blended) == PONKO_OK);
    CHECK(blended.post.exposure == 1);
    CHECK(ponko_post_blend_volumes(&day, &volume, 1, (PonkoVec3){0, 2, 20}, &blended) == PONKO_OK);
    CHECK(blended.post.exposure == 0);
    volume.blend_distance = -1;
    CHECK(ponko_post_blend_volumes(&day, &volume, 1, (PonkoVec3){0, 2, 20}, &blended) == PONKO_INVALID_ARGUMENT);
    /* Smoke that fades where it meets the ground. */
    PonkoWorld* w = ponko_ctx_world(ctx);
    uint32_t transform; uint64_t smoke;
    CHECK(ponko_transform3d_component(w, &transform) == PONKO_OK);
    PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){0, 1, 0});
    PonkoValue value = {transform, &at, sizeof at};
    CHECK(ponko_spawn(w, &value, 1, &smoke) == PONKO_OK);
    PonkoParticleSettings3DEx particles = {.struct_size = sizeof particles, .version = PONKO_PARTICLE_SETTINGS3D_EX_VERSION};
    CHECK(ponko_particle_settings3d_ex_defaults(&particles) == PONKO_OK && particles.softness == 0);
    particles.settings.rate = 40;
    particles.softness = 1.5f;
    CHECK(ponko_ctx_set_particle_emitter3d_ex(ctx, smoke, &particles) == PONKO_OK);
    particles.softness = -1;
    CHECK(ponko_ctx_set_particle_emitter3d_ex(ctx, smoke, &particles) == PONKO_INVALID_ARGUMENT);
    /* The camera, and a monitor that shows the room without the smoke. */
    PonkoCamera3DEx camera = {.struct_size = sizeof camera, .version = PONKO_CAMERA3D_EX_VERSION};
    CHECK(ponko_ctx_camera3d_ex(ctx, &camera) == PONKO_OK && camera.particles && !camera.occlusion_culling);
    camera.occlusion_culling = true;
    CHECK(ponko_ctx_set_camera3d_ex(ctx, &camera) == PONKO_OK);
    bool culling = false;
    CHECK(ponko_ctx_occlusion_culling(ctx, &culling) == PONKO_OK && culling);
    camera.camera.near = 0;
    CHECK(ponko_ctx_set_camera3d_ex(ctx, &camera) == PONKO_INVALID_ARGUMENT);
    PonkoCamera3D abi2;
    CHECK(ponko_ctx_camera3d(ctx, &abi2) == PONKO_OK && abi2.near > 0);
    PonkoCamera3DEx from = {.struct_size = sizeof from, .version = PONKO_CAMERA3D_EX_VERSION};
    CHECK(ponko_camera3d_ex_from(&abi2, &from) == PONKO_OK && from.particles && !from.occlusion_culling);
    PonkoTexture screen;
    CHECK(ponko_ctx_create_render_target(ctx, 64, 64, &screen) == PONKO_OK);
    PonkoTargetCamera monitor = {.camera = abi2, .target = screen};
    PonkoTargetCameraEx monitor_ex = {.struct_size = sizeof monitor_ex, .version = PONKO_TARGET_CAMERA_EX_VERSION};
    CHECK(ponko_target_camera_ex_from(&monitor, &monitor_ex) == PONKO_OK && monitor_ex.particles);
    CHECK(monitor_ex.debug_draw == PONKO_DEBUG_DRAW_DEFAULT);
    monitor_ex.particles = false;
    monitor_ex.debug_draw = PONKO_DEBUG_DRAW_ON;
    const PonkoTargetCameraEx* cameras[1] = {&monitor_ex};
    CHECK(ponko_ctx_set_target_cameras_ex(ctx, cameras, 1) == PONKO_OK);
    PonkoTargetCameraEx read = {.struct_size = sizeof read, .version = PONKO_TARGET_CAMERA_EX_VERSION};
    CHECK(ponko_ctx_target_camera_ex(ctx, 0, &read) == PONKO_OK && !read.particles && read.target_camera.target.id == screen.id);
    CHECK(read.debug_draw == PONKO_DEBUG_DRAW_ON);
    monitor_ex.debug_draw = 9;
    CHECK(ponko_ctx_set_target_cameras_ex(ctx, cameras, 1) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_target_camera_ex(ctx, 1, &read) == PONKO_INVALID_ARGUMENT);
}
static void render_descriptors(void) {
    PonkoGame* game = ponko_game_new("render descriptors", 64, 64);
    PonkoWorld* world = NULL;
    CHECK(ponko_game_run_headless(game, 0, render_descriptors_setup, NULL, &world) == PONKO_OK);
    CHECK(ponko_world_free(world) == PONKO_OK);
}

/* What a headless C game can play and observe: the pointer, the sounds it
 * started, a quit request, zoom_at, and a button that closes its screen. */
static void headless_play_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    CHECK(ponko_ctx_feed_mouse_move(ctx, (PonkoVec2){400, 300}) == PONKO_OK);
    CHECK(ponko_ctx_feed_mouse_wheel(ctx, (PonkoVec2){0, 2}) == PONKO_OK);
    CHECK(ponko_ctx_feed_mouse_move(ctx, (PonkoVec2){NAN, 0}) == PONKO_INVALID_ARGUMENT);
    PonkoVec2 at = {0, 0}, wheel = {0, 0};
    CHECK(ponko_ctx_mouse_pos(ctx, &at) == PONKO_OK && at.x == 400 && at.y == 300);
    CHECK(ponko_ctx_mouse_wheel(ctx, &wheel) == PONKO_OK && wheel.y == 2);

    float samples[100] = {0};
    PonkoSound beep = {0};
    CHECK(ponko_ctx_create_sound(ctx, 8000, samples, 100, &beep) == PONKO_OK);
    PonkoSoundParams params = ponko_sound_params_default();
    params.volume = 0.5f;
    PonkoPlaying playing = {0};
    CHECK(ponko_ctx_play_sound(ctx, beep, &params, &playing) == PONKO_OK && playing.id != 0);
    size_t count = 0;
    CHECK(ponko_ctx_sounds_started(ctx, &count) == PONKO_OK && count == 1);
    PonkoSoundStarted started = {.struct_size = sizeof started, .version = PONKO_SOUND_STARTED_VERSION};
    CHECK(ponko_ctx_sound_started(ctx, 0, &started) == PONKO_OK);
    CHECK(started.playing.id == playing.id && started.sound.id == beep.id && started.volume == 0.5f);
    CHECK(ponko_ctx_sound_started(ctx, 1, &started) == PONKO_NOT_FOUND);

    PonkoCamera camera = {{0, 0}, 1};
    CHECK(ponko_ctx_set_camera(ctx, &camera) == PONKO_OK);
    CHECK(ponko_ctx_camera_zoom_at(ctx, (PonkoVec2){800, 600}, 2) == PONKO_OK);
    CHECK(ponko_ctx_camera(ctx, &camera) == PONKO_OK && camera.zoom == 2);
    CHECK(ponko_ctx_camera_zoom_at(ctx, (PonkoVec2){0, 0}, 0) == PONKO_INVALID_ARGUMENT);

    bool quit = true;
    CHECK(ponko_ctx_quit_requested(ctx, &quit) == PONKO_OK && !quit);
    CHECK(ponko_ctx_quit(ctx) == PONKO_OK);
    CHECK(ponko_ctx_quit_requested(ctx, &quit) == PONKO_OK && quit);

    uint64_t menu = PONKO_UI_NONE, back = PONKO_UI_NONE;
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, NULL, &menu) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_button(ctx, PONKO_UI_SCREEN, menu, "Back", NULL, &back) == PONKO_OK);
    bool closes = true;
    CHECK(ponko_ctx_ui_closes_screen(ctx, PONKO_UI_SCREEN, back, &closes) == PONKO_OK && !closes);
    CHECK(ponko_ctx_ui_set_closes_screen(ctx, PONKO_UI_SCREEN, back, true) == PONKO_OK);
    CHECK(ponko_ctx_ui_closes_screen(ctx, PONKO_UI_SCREEN, back, &closes) == PONKO_OK && closes);
}

static uint64_t build_paused_screen(PonkoCtx* ctx, void* user) {
    uint64_t root = PONKO_UI_NONE, resume = PONKO_UI_NONE;
    CHECK(ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, NULL, &root) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_button(ctx, PONKO_UI_SCREEN, root, "Resume", NULL, &resume) == PONKO_OK);
    CHECK(ponko_ctx_ui_set_closes_screen(ctx, PONKO_UI_SCREEN, resume, true) == PONKO_OK);
    ++*(int*)user;
    return root;
}

static void pause_on_tick_one(PonkoCtx* ctx, void* user) {
    (void)user;
    if (ponko_ctx_sim_tick(ctx) == 1) CHECK(ponko_ctx_push_state(ctx, 1) == PONKO_OK);
    if (ponko_ctx_sim_tick(ctx) == 3) CHECK(ponko_ctx_pop_state(ctx) == PONKO_OK);
}

static void screen_up_by_tick(PonkoCtx* ctx, void* user) {
    uint64_t top = PONKO_UI_NONE;
    CHECK(ponko_ctx_ui_top_screen(ctx, PONKO_UI_SCREEN, &top) == PONKO_OK);
    const uint64_t tick = ponko_ctx_sim_tick(ctx);
    if (tick < 6) ((bool*)user)[tick] = top != PONKO_UI_NONE;
}

static void headless_play(void) {
    PonkoGame* game = ponko_game_new("headless play", 800, 600);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 1, headless_play_setup, NULL, &w) == PONKO_OK);
    ponko_world_free(w);

    int built = 0;
    bool up[6] = {false};
    game = ponko_game_new("state screen", 800, 600);
    CHECK(ponko_game_state(game, 0) == PONKO_OK);
    CHECK(ponko_game_headless_frames(game, true) == PONKO_OK);
    CHECK(ponko_game_state_screen(game, 1, PONKO_UI_SCREEN_MODE_MODAL, build_paused_screen, &built) == PONKO_OK);
    CHECK(ponko_game_sim_system(game, "pause", 1, pause_on_tick_one, NULL) == PONKO_OK);
    CHECK(ponko_game_system(game, "watch", screen_up_by_tick, up) == PONKO_OK);
    CHECK(ponko_game_run_headless(game, 6, NULL, NULL, &w) == PONKO_OK);
    ponko_world_free(w);
    CHECK(built == 1);
    CHECK(!up[0] && !up[1] && up[2] && up[3] && !up[4] && !up[5]);
}

/* World brains inside query loops: the brain and blackboard columns. */
typedef struct BrainLoop {
    uint64_t alert, chase, speed, at;
    uint32_t ids[2];
    int chasing, started;
    float seen_speed;
} BrainLoop;

static uint64_t ai_key_of(const char* name) {
    uint64_t k = 0;
    CHECK(ponko_ai_key(name, &k) == PONKO_OK);
    return k;
}

static void brain_senses_rows(const PonkoChunk* chunk, void* user) {
    const BrainLoop* l = user;
    for (size_t i = 0; i < chunk->count; ++i) {
        PonkoAiBlackboard* board = ponko_chunk_ai_blackboard(chunk, 1, i);
        CHECK(board != NULL);
        CHECK(ponko_ai_blackboard_set_flag(board, l->alert, true) == PONKO_OK);
        PonkoVec2 at = {(float)i, 2};
        CHECK(ponko_ai_blackboard_set_vec2(board, l->at, at) == PONKO_OK);
    }
    CHECK(ponko_chunk_ai_blackboard(chunk, 1, chunk->count) == NULL);
}

static void brain_act_rows(const PonkoChunk* chunk, void* user) {
    BrainLoop* l = user;
    for (size_t i = 0; i < chunk->count; ++i) {
        PonkoAiBrain* brain = ponko_chunk_ai_brain(chunk, 0, i);
        const PonkoAiBlackboard* board = ponko_chunk_ai_blackboard(chunk, 1, i);
        uint64_t action = 0;
        bool started = false, alert = false;
        PonkoVec2 at = {0, 0};
        CHECK(ponko_ai_brain_action(brain, 0, &action) == PONKO_OK);
        CHECK(ponko_ai_blackboard_flag(board, l->alert, &alert) == PONKO_OK && alert);
        CHECK(ponko_ai_blackboard_vec2(board, l->at, &at) == PONKO_OK && at.y == 2);
        if (action != l->chase) continue;
        ++l->chasing;
        CHECK(ponko_ai_brain_started(brain, 0, &started) == PONKO_OK);
        if (started) ++l->started;
        CHECK(ponko_ai_brain_param_num(brain, 0, l->speed, &l->seen_speed) == PONKO_OK);
        CHECK(ponko_ai_brain_finish(brain, l->chase, PONKO_AI_SUCCESS) == PONKO_OK);
    }
}

static void brain_senses(PonkoCtx* ctx, void* user) {
    BrainLoop* l = user;
    CHECK(ponko_query(ponko_ctx_world(ctx), l->ids, 2, brain_senses_rows, l) == PONKO_OK);
}

static void brain_think(PonkoCtx* ctx, void* user) {
    (void)user;
    CHECK(ponko_ctx_ai_think(ctx) == PONKO_OK);
}

static void brain_act(PonkoCtx* ctx, void* user) {
    BrainLoop* l = user;
    CHECK(ponko_query(ponko_ctx_world(ctx), l->ids, 2, brain_act_rows, l) == PONKO_OK);
}

static void brain_setup(PonkoCtx* ctx, void* user) {
    BrainLoop* l = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    CHECK(ponko_ai_brain_component(w, &l->ids[0]) == PONKO_OK);
    CHECK(ponko_ai_blackboard_component(w, &l->ids[1]) == PONKO_OK);
    PonkoBehaviorTree* tree = ponko_behavior_tree_new(
        "{\"root\": {\"sequence\": [{\"check\": \"alert\"}, {\"do\": \"chase\", \"speed\": 2.5}]}}");
    CHECK(tree != NULL);
    CHECK(ponko_ctx_ai_add_tree(ctx, "guard", tree) == PONKO_OK);
    ponko_behavior_tree_free(tree);
    for (int i = 0; i < 4; ++i) {
        uint64_t e = 0;
        CHECK(ponko_spawn(w, NULL, 0, &e) == PONKO_OK);
        CHECK(ponko_ctx_ai_add_brain(ctx, e, "guard", 1) == PONKO_OK);
        if (i == 0) {
            /* Entities and points on a blackboard read back by entity, too. */
            PonkoVec3 p = {1, 2, 3}, q = {0, 0, 0};
            uint64_t buddy = 0;
            CHECK(ponko_ctx_ai_set_entity(ctx, e, ai_key_of("buddy"), e) == PONKO_OK);
            CHECK(ponko_ctx_ai_set_vec3(ctx, e, ai_key_of("p3"), p) == PONKO_OK);
            CHECK(ponko_ctx_ai_entity(ctx, e, ai_key_of("buddy"), &buddy) == PONKO_OK && buddy == e);
            CHECK(ponko_ctx_ai_vec3(ctx, e, ai_key_of("p3"), &q) == PONKO_OK && q.z == 3);
            PonkoVec2 none;
            CHECK(ponko_ctx_ai_vec2(ctx, e, ai_key_of("p3"), &none) == PONKO_NOT_FOUND);
        }
    }
}

static void world_brains_in_loops(void) {
    BrainLoop l = {ai_key_of("alert"), ai_key_of("chase"), ai_key_of("speed"), ai_key_of("at"), {0, 0}, 0, 0, 0};
    PonkoGame* game = ponko_game_new("brain loops", 1, 1);
    CHECK(ponko_game_sim_system(game, "senses", 1, brain_senses, &l) == PONKO_OK);
    CHECK(ponko_game_sim_system(game, "think", 1, brain_think, &l) == PONKO_OK);
    CHECK(ponko_game_sim_system(game, "act", 1, brain_act, &l) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 4, brain_setup, &l, &w) == PONKO_OK);
    ponko_world_free(w);
    /* Each chase is reported done when seen: the tree succeeds at the next
       think and starts over at the one after, so they chase at ticks 0 and 2. */
    CHECK(l.chasing == 8 && l.started == 8 && l.seen_speed == 2.5f);

    /* A native brain's blackboard holds entities and points as well. */
    PonkoBrain* b = ponko_brain_new(1);
    PonkoVec2 p = {3, 4}, q = {0, 0};
    uint64_t e = 0;
    CHECK(ponko_brain_set_vec2(b, l.at, p) == PONKO_OK && ponko_brain_vec2(b, l.at, &q) == PONKO_OK && q.y == 4);
    CHECK(ponko_brain_set_entity(b, l.alert, 9) == PONKO_OK && ponko_brain_entity(b, l.alert, &e) == PONKO_OK && e == 9);
    PonkoVec3 r;
    CHECK(ponko_brain_vec3(b, l.at, &r) == PONKO_NOT_FOUND);
    ponko_brain_free(b);
}

/* Native parity, round 5 (gl#157): crash reports, touch looks and the player's layout, trackpad
   gestures, camera targets with kept rigs, the window restored before setup, spectrum seas and
   sliced loaders, from C. */
typedef struct { int reports; uint32_t kind; char message[64]; char version[16]; char file[96]; } CrashSeen;
static void crash_hook(const PonkoCrashReport* report, void* user) {
    CrashSeen* seen = user;
    seen->reports++;
    seen->kind = report->kind;
    snprintf(seen->message, sizeof seen->message, "%s", report->message);
    snprintf(seen->version, sizeof seen->version, "%s", report->game_version ? report->game_version : "");
    snprintf(seen->file, sizeof seen->file, "%s", report->file ? report->file : "");
}
static void crash_setup(PonkoCtx* ctx, void* user) {
    CrashSeen* seen = user;
    CHECK(ponko_crash_report("the save file is damaged") == PONKO_OK);
    CHECK(seen->reports == 1 && seen->kind == PONKO_CRASH_ERROR);
    CHECK(strcmp(seen->message, "the save file is damaged") == 0 && strcmp(seen->version, "1.4.2") == 0);
    CHECK(strncmp(seen->file, "crash-reports/", 14) == 0);
    char names[512]; size_t len = 0;
    CHECK(ponko_ctx_saved_crash_reports(ctx, names, sizeof names, &len) == PONKO_OK && len > 0);
    CHECK(strstr(names, seen->file) == names); /* newest first */
    char text[8192]; size_t text_len = 0;
    CHECK(ponko_ctx_storage_read(ctx, seen->file, (uint8_t*)text, sizeof text - 1, &text_len) == PONKO_OK);
    text[text_len < sizeof text ? text_len : sizeof text - 1] = 0;
    CHECK(strstr(text, "game version: 1.4.2") != NULL);
    CHECK(ponko_ctx_storage_delete(ctx, seen->file) == PONKO_OK);
}
static void crash_off_setup(PonkoCtx* ctx, void* user) {
    (void)ctx; (void)user;
    CHECK(ponko_crash_report("not written") == PONKO_NOT_FOUND);
}

static void touch_layout_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoTouchControl controls[] = {{PONKO_TOUCH_STICK, "move", NULL}, {PONKO_TOUCH_BUTTON, "jump", "A"}};
    PonkoTouchControlPlace places[] = {{"jump", {{0.75f, 1}, {0, -60}}}};
    PonkoTouchControlStyle styles[] = {{"jump", ponko_touch_style_default()}};
    styles[0].style.button = (PonkoColor){1, .2f, .2f, .4f};
    PonkoTouchControlsDesc desc = {.struct_size = sizeof desc, .version = PONKO_TOUCH_CONTROLS_DESC_VERSION};
    CHECK(ponko_touch_controls_desc_defaults(&desc) == PONKO_OK && desc.style.size == 1);
    desc.controls = controls; desc.count = 2; desc.show = PONKO_TOUCH_SHOW_ALWAYS;
    desc.style.size = 1.2f;
    desc.styles = styles; desc.style_count = 1;
    desc.places = places; desc.place_count = 1;
    CHECK(ponko_ctx_set_touch_controls_ex(ctx, &desc) == PONKO_OK);
    PonkoVec2 at;
    CHECK(ponko_ctx_touch_control_center(ctx, "jump", &at) == PONKO_OK && at.x == 720 && at.y == 480);
    desc.style.size = 0;
    CHECK(ponko_ctx_set_touch_controls_ex(ctx, &desc) == PONKO_INVALID_ARGUMENT);

    /* The player drags the jump button in edit mode: it moves, it isn't pressed. */
    bool editing = false;
    CHECK(ponko_ctx_edit_touch_controls(ctx, true) == PONKO_OK);
    CHECK(ponko_ctx_editing_touch_controls(ctx, &editing) == PONKO_OK && editing);
    PonkoVec2 to = {at.x - 300, at.y};
    CHECK(ponko_ctx_feed_touch(ctx, 1, PONKO_TOUCH_STARTED, at) == PONKO_OK);
    CHECK(ponko_ctx_feed_touch(ctx, 1, PONKO_TOUCH_MOVED, to) == PONKO_OK);
    CHECK(ponko_ctx_feed_touch(ctx, 1, PONKO_TOUCH_ENDED, to) == PONKO_OK);
    CHECK(ponko_ctx_edit_touch_controls(ctx, false) == PONKO_OK);
    PonkoVec2 now;
    CHECK(ponko_ctx_touch_control_center(ctx, "jump", &now) == PONKO_OK && now.x == to.x && now.y == to.y);
    PonkoTouchAdjust adjust;
    CHECK(ponko_ctx_touch_layout_control(ctx, "jump", &adjust) == PONKO_OK && adjust.placed);

    /* Sliders, JSON, reset. */
    float size = 0, opacity = 0;
    CHECK(ponko_ctx_set_touch_layout(ctx, 1.5f, .5f) == PONKO_OK);
    CHECK(ponko_ctx_touch_layout(ctx, &size, &opacity) == PONKO_OK && size == 1.5f && opacity == .5f);
    char json[1024]; size_t len = 0;
    CHECK(ponko_ctx_touch_layout_json(ctx, json, sizeof json, &len) == PONKO_OK && strstr(json, "jump"));
    CHECK(ponko_ctx_reset_touch_layout(ctx) == PONKO_OK);
    CHECK(ponko_ctx_touch_layout_control(ctx, "jump", &adjust) == PONKO_OK && !adjust.placed);
    CHECK(ponko_ctx_set_touch_layout_json(ctx, json) == PONKO_OK);
    CHECK(ponko_ctx_touch_layout_control(ctx, "jump", &adjust) == PONKO_OK && adjust.placed);
    CHECK(ponko_ctx_set_touch_layout_control(ctx, "jump", NULL) == PONKO_OK);
    adjust.placed = false; adjust.size = 1.25f;
    CHECK(ponko_ctx_set_touch_layout_control(ctx, "move", &adjust) == PONKO_OK);
    CHECK(ponko_ctx_touch_layout_control(ctx, "move", &adjust) == PONKO_OK && adjust.size == 1.25f);
    CHECK(ponko_ctx_save_touch_layout(ctx) == PONKO_OK);
    CHECK(ponko_ctx_restore_touch_layout(ctx) == PONKO_OK);
    /* The engine's own arrange screen: edit mode at once. */
    CHECK(ponko_ctx_arrange_touch_controls(ctx) == PONKO_OK);
    CHECK(ponko_ctx_editing_touch_controls(ctx, &editing) == PONKO_OK && editing);
}

/* A trackpad pinch: the same gesture as two fingers, and no touch device. */
static void trackpad_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    CHECK(ponko_ctx_feed_trackpad(ctx, PONKO_GESTURE_PINCH, PONKO_TOUCH_STARTED, .25f) == PONKO_OK);
    float scale = 0; bool touch = true;
    CHECK(ponko_ctx_gesture_deltas(ctx, NULL, &scale, NULL) == PONKO_OK && scale == 1.25f);
    CHECK(ponko_ctx_touch_device(ctx, &touch) == PONKO_OK && !touch);
    PonkoInputScenario* scenario = ponko_input_scenario_new();
    CHECK(ponko_input_scenario_trackpad(scenario, 2, PONKO_GESTURE_ROTATE, PONKO_TOUCH_MOVED, .5f) == PONKO_OK);
    ponko_input_scenario_free(scenario);
}

typedef struct { uint64_t hero; bool started; PonkoVec2 center; } TargetSeen;
static void target_setup(PonkoCtx* ctx, void* user) {
    TargetSeen* seen = user;
    PonkoFollowCamera2D follow = {.struct_size = sizeof follow, .version = PONKO_FOLLOW_CAMERA2D_VERSION};
    CHECK(ponko_follow_camera2d_defaults(&follow) == PONKO_OK);
    CHECK(ponko_ctx_set_follow_camera2d_rig(ctx, &follow) == PONKO_OK);
    PonkoWorld* w = ponko_ctx_world(ctx);
    uint32_t transform;
    CHECK(ponko_transform_component(w, &transform) == PONKO_OK);
    PonkoTransform2D at = {{500, 200}, 0, {1, 1}};
    PonkoValue value = {transform, &at, sizeof at};
    CHECK(ponko_spawn(w, &value, 1, &seen->hero) == PONKO_OK);
    CHECK(ponko_ctx_set_camera_target(ctx, seen->hero, true) == PONKO_OK);
}
static void target_look(PonkoCtx* ctx, void* user) {
    TargetSeen* seen = user;
    PonkoFollowCamera2D follow = {.struct_size = sizeof follow, .version = PONKO_FOLLOW_CAMERA2D_VERSION};
    bool kept = false, found = false; uint64_t target = 0;
    CHECK(ponko_ctx_follow_camera2d_rig(ctx, &follow, &kept) == PONKO_OK && kept);
    CHECK(ponko_ctx_camera_target(ctx, &target, &found) == PONKO_OK && found && target == seen->hero);
    seen->started = follow.started;
    seen->center = follow.center;
}

typedef struct { PonkoVec2 size; bool save; } WindowSeen;
static void window_setup(PonkoCtx* ctx, void* user) {
    WindowSeen* seen = user;
    PonkoWindowPropertiesEx props = {.struct_size = sizeof props, .version = PONKO_WINDOW_PROPERTIES_EX_VERSION};
    CHECK(ponko_ctx_window_properties_ex(ctx, &props) == PONKO_OK);
    seen->size = props.size;
    if (seen->save) {
        CHECK(ponko_ctx_window_set_size(ctx, (PonkoVec2){1024, 768}) == PONKO_OK);
        CHECK(ponko_ctx_window_save_settings(ctx) == PONKO_OK);
    }
}
static PonkoVec2 window_size_seen(bool restore, bool save) {
    WindowSeen seen = {{0, 0}, save};
    PonkoGame* game = ponko_game_new("restore", 800, 600);
    CHECK(ponko_game_data_dir(game, "ponko-c-batch5") == PONKO_OK);
    CHECK(ponko_game_restore_window_settings(game, restore) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 0, window_setup, &seen, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    return seen.size;
}

static void spectrum_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoWaterDesc sea = {.struct_size = sizeof sea, .version = PONKO_WATER_DESC_VERSION};
    CHECK(ponko_water_desc_sea(&sea, 0, (PonkoVec2){12, 4}, 2) == PONKO_OK && !sea.has_spectrum);
    sea.has_spectrum = true;
    sea.spectrum = ponko_wave_spectrum((PonkoVec2){12, 4}, 2);
    CHECK(sea.spectrum.tile == 256 && sea.spectrum.resolution == 128);
    uint64_t water;
    CHECK(ponko_ctx_spawn_water(ctx, &sea, &water) == PONKO_OK);
    float spread = 0;
    for (int i = 0; i < 4; ++i) {
        PonkoWaterSample s;
        CHECK(ponko_ctx_water_sample(ctx, water, (PonkoVec2){i * 17.0f, i * -9.0f}, i * .7f, &s) == PONKO_OK);
        spread += fabsf(s.height);
    }
    CHECK(spread > 0.001f); /* a 2 m sea is not flat */
    /* Waves from a spectrum are drawn on the simulation's clock. */
    float drawn = -1;
    CHECK(ponko_ctx_water_drawn_time(ctx, water, &drawn) == PONKO_OK && drawn == (float)ponko_ctx_sim_time(ctx));
    CHECK(ponko_ctx_water_drawn_time(NULL, water, &drawn) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_water_drawn_time(ctx, water, NULL) == PONKO_INVALID_ARGUMENT);
    uint64_t dry;
    CHECK(ponko_spawn(ponko_ctx_world(ctx), NULL, 0, &dry) == PONKO_OK);
    CHECK(ponko_ctx_water_drawn_time(ctx, dry, &drawn) == PONKO_NOT_FOUND);
    sea.spectrum.tile = 0;
    CHECK(ponko_ctx_spawn_water(ctx, &sea, &water) == PONKO_INVALID_ARGUMENT);
}

/* Buoyancy from a descriptor: sea water, no ripples, points of its own. */
static void buoyancy_desc_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    uint64_t raft;
    CHECK(ponko_spawn(ponko_ctx_world(ctx), NULL, 0, &raft) == PONKO_OK);
    PonkoBuoyancyDesc d = {.struct_size = sizeof d, .version = PONKO_BUOYANCY_DESC_VERSION};
    CHECK(ponko_buoyancy_desc_box(&d, (PonkoVec3){2, 0.4f, 2}) == PONKO_OK);
    CHECK(d.point_count == 8 && d.water_density == PONKO_FRESH_WATER_DENSITY && d.ripples);
    d.water_density = 1025;
    d.ripples = false;
    CHECK(ponko_ctx_set_buoyancy(ctx, raft, &d) == PONKO_OK);
    PonkoBuoyancyDesc read = {.struct_size = sizeof read, .version = PONKO_BUOYANCY_DESC_VERSION};
    CHECK(ponko_ctx_buoyancy(ctx, raft, &read) == PONKO_OK);
    CHECK(read.water_density == 1025 && !read.ripples && read.point_count == 8);
    CHECK(read.points[7].x == d.points[7].x && read.points[7].y == d.points[7].y);
    const PonkoVec3 hull[4] = {{-1, 0, 0}, {1, 0, 0}, {0, 0, -1}, {0, 0, 1}};
    CHECK(ponko_buoyancy_desc_points(&d, hull, 4, 0.6f, 0.3f) == PONKO_OK);
    CHECK(d.point_count == 4 && d.drag == PONKO_BUOYANCY_DRAG);
    CHECK(ponko_ctx_set_buoyancy(ctx, raft, &d) == PONKO_OK);
    d.point_count = PONKO_BUOYANCY_MAX_POINTS + 1;
    CHECK(ponko_ctx_set_buoyancy(ctx, raft, &d) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_remove_buoyancy(ctx, raft) == PONKO_OK);
    CHECK(ponko_ctx_buoyancy(ctx, raft, &read) == PONKO_NOT_FOUND);
}

/* A sliced loader: counts the '#' of a file, 64 bytes a step. */
typedef struct { uint8_t* bytes; size_t len, at, walls; } Walls;
typedef struct { int steps, freed_users; } SliceCounts;
static uint32_t walls_start(void* user, const uint8_t* bytes, size_t len, void** out_state) {
    (void)user;
    Walls* w = calloc(1, sizeof *w);
    w->bytes = malloc(len); memcpy(w->bytes, bytes, len); w->len = len;
    *out_state = w;
    return PONKO_OK;
}
static uint32_t walls_step(void* user, void* state, bool* out_done) {
    SliceCounts* counts = user; Walls* w = state;
    counts->steps++;
    size_t end = w->at + 64 < w->len ? w->at + 64 : w->len;
    for (; w->at < end; ++w->at) w->walls += w->bytes[w->at] == '#';
    *out_done = w->at == w->len;
    return PONKO_OK;
}
static void walls_free(void* user, void* state) { (void)user; Walls* w = state; free(w->bytes); free(w); }
static void walls_free_user(void* user) { ((SliceCounts*)user)->freed_users++; }
static void sliced_setup(PonkoCtx* ctx, void* user) {
    SliceCounts* counts = user;
    char level[200];
    for (int i = 0; i < 200; ++i) level[i] = i % 4 == 0 ? '#' : '.';
    PonkoMemorySource* files = ponko_memory_source_new("walls");
    CHECK(ponko_memory_source_insert(files, "walls.txt", (const uint8_t*)level, sizeof level) == PONKO_OK);
    CHECK(ponko_ctx_add_background_memory_source(ctx, files) == PONKO_OK);
    ponko_memory_source_free(files);
    PonkoSliceLoader loader = {.struct_size = sizeof loader, .version = PONKO_SLICE_LOADER_VERSION,
                               .start = walls_start, .step = walls_step, .free_state = walls_free,
                               .user = counts, .free_user = walls_free_user};
    PonkoAssetGroup* group = ponko_asset_group_new("level");
    CHECK(ponko_asset_group_add_sliced(group, "walls.txt", &loader) == PONKO_OK);
    uint32_t id;
    CHECK(ponko_ctx_start_asset_group(ctx, group, &id) == PONKO_OK);
    CHECK(ponko_ctx_finish_uploads(ctx) == PONKO_OK);
    uint32_t state;
    CHECK(ponko_ctx_asset_group_state(ctx, id, &state) == PONKO_OK && state == PONKO_GROUP_READY);
    CHECK(counts->steps == 4); /* 200 bytes, 64 a step */
    void* taken = NULL;
    CHECK(ponko_ctx_take_asset_group_sliced(ctx, id, "walls.txt", &taken) == PONKO_OK);
    CHECK(((Walls*)taken)->walls == 50);
    walls_free(NULL, taken);
    CHECK(ponko_ctx_take_asset_group_sliced(ctx, id, "walls.txt", &taken) == PONKO_NOT_FOUND);
}

/* The navmesh crowd's counts, read after the navigation step each tick. */
static void crowd_count_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoNavCrowdStats stats = {.struct_size = sizeof stats, .version = PONKO_NAV_CROWD_STATS_VERSION};
    CHECK(ponko_ctx_navmesh_crowd_stats(ctx, &stats) == PONKO_NOT_FOUND); /* nothing baked yet */
    PonkoNavSources* sources = ponko_nav_sources_new();
    PonkoMeshData* floor = ponko_mesh_data_plane(20);
    CHECK(ponko_nav_sources_add_mesh(sources, floor, ponko_transform3d_at((PonkoVec3){0, 0, 0})) == PONKO_OK);
    ponko_mesh_data_free(floor);
    CHECK(ponko_ctx_bake_navmesh(ctx, sources, NULL) == PONKO_OK);
    ponko_nav_sources_free(sources);
    PonkoWorld* w = ponko_ctx_world(ctx);
    uint32_t transform;
    CHECK(ponko_transform3d_component(w, &transform) == PONKO_OK);
    for (int i = 0; i < 3; ++i) {
        PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){-5, 0, (float)i - 1});
        PonkoValue part = {transform, &at, sizeof at};
        uint64_t e;
        CHECK(ponko_spawn(w, &part, 1, &e) == PONKO_OK);
        CHECK(ponko_ctx_set_nav_agent(ctx, e, 2, 0.25f, 8) == PONKO_OK);
        CHECK(ponko_ctx_set_nav_avoidance(ctx, e, true, 1, 3) == PONKO_OK);
        PonkoVec3 to = {5, 0, 0};
        CHECK(ponko_ctx_set_nav_destination(ctx, e, &to) == PONKO_OK);
    }
}
static void crowd_count_read(PonkoCtx* ctx, void* user) {
    PonkoNavCrowdStats* stats = user;
    CHECK(ponko_ctx_navmesh_crowd_stats(ctx, stats) == PONKO_OK);
}

/* Allocations on this thread, and a scope that counts the ones made inside it. */
static void alloc_count_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    CHECK(ponko_ctx_set_profiling(ctx, true) == PONKO_OK);
    const PonkoProfileScope* scope = ponko_profile_scope("c/alloc");
    uint64_t token = ponko_profile_begin(scope);
    CHECK(token != 0); /* profiling is on */
    PonkoAllocCounts before = ponko_thread_alloc_counts();
    ponko_note_allocation(64);
    ponko_note_allocation(32);
    PonkoAllocCounts after = ponko_thread_alloc_counts();
    ponko_profile_end(scope, token);
    CHECK(after.count - before.count == 2 && after.bytes - before.bytes == 96);
    /* Net bytes: what is given back leaves the figure, what is kept stays in it. */
    int64_t held = ponko_net_alloc_bytes();
    ponko_note_allocation(4096);
    int64_t holding = ponko_net_alloc_bytes();
    ponko_note_deallocation(4096);
    int64_t released = ponko_net_alloc_bytes();
    CHECK(holding - held >= 4096 && holding - released >= 4096);
    CHECK(ponko_ctx_set_profiling(ctx, false) == PONKO_OK);
}

/* The GPU timing choice: set and read back; a bad kind changes nothing. */
static void gpu_timing_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    uint32_t kind = 99, every = 99;
    CHECK(ponko_ctx_gpu_timing(ctx, &kind, &every) == PONKO_OK);
    CHECK(kind == PONKO_GPU_TIMING_AUTO && every == 0); /* the default */
    CHECK(ponko_ctx_set_gpu_timing(ctx, PONKO_GPU_TIMING_EVERY, 4) == PONKO_OK);
    CHECK(ponko_ctx_gpu_timing(ctx, &kind, &every) == PONKO_OK);
    CHECK(kind == PONKO_GPU_TIMING_EVERY && every == 4);
    CHECK(ponko_ctx_set_gpu_timing(ctx, PONKO_GPU_TIMING_OFF, 0) == PONKO_OK);
    CHECK(ponko_ctx_gpu_timing(ctx, &kind, &every) == PONKO_OK);
    CHECK(kind == PONKO_GPU_TIMING_OFF && every == 0);
    CHECK(ponko_ctx_set_gpu_timing(ctx, 7, 1) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_gpu_timing(ctx, &kind, &every) == PONKO_OK);
    CHECK(kind == PONKO_GPU_TIMING_OFF); /* unchanged */
    CHECK(ponko_ctx_gpu_timing(ctx, NULL, &every) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_set_gpu_timing(ctx, PONKO_GPU_TIMING_AUTO, 5) == PONKO_OK);
    CHECK(ponko_ctx_gpu_timing(ctx, &kind, &every) == PONKO_OK);
    CHECK(kind == PONKO_GPU_TIMING_AUTO && every == 0);
}

static void native_parity_round5(void) {
    /* Crash reports: on, with the game's version and a hook. */
    CrashSeen seen = {0};
    PonkoCrashReports reports = {.struct_size = sizeof reports, .version = PONKO_CRASH_REPORTS_VERSION};
    reports.mode = PONKO_CRASH_REPORTS_ON;
    reports.game_version = "1.4.2";
    reports.on_crash = crash_hook;
    reports.user = &seen;
    PonkoGame* game = ponko_game_new("Space Crab", 1, 1);
    CHECK(ponko_game_data_dir(game, "ponko-c-batch5") == PONKO_OK);
    CHECK(ponko_game_crash_reports(game, &reports) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 0, crash_setup, &seen, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    reports.mode = PONKO_CRASH_REPORTS_OFF;
    game = ponko_game_new("Space Crab", 1, 1);
    CHECK(ponko_game_crash_reports(game, &reports) == PONKO_OK);
    reports.mode = 7;
    CHECK(ponko_game_crash_reports(game, &reports) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_game_run_headless(game, 0, crash_off_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    CHECK(seen.reports == 1);

    game = ponko_game_new("touch", 960, 540);
    CHECK(ponko_game_data_dir(game, "ponko-c-batch5") == PONKO_OK);
    CHECK(ponko_game_run_headless(game, 0, touch_layout_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    game = ponko_game_new("trackpad", 960, 540);
    CHECK(ponko_game_run_headless(game, 0, trackpad_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);

    TargetSeen target = {0};
    game = ponko_game_new("target", 640, 360);
    CHECK(ponko_game_headless_frames(game, true) == PONKO_OK);
    CHECK(ponko_game_late_system(game, "look", target_look, &target) == PONKO_OK);
    CHECK(ponko_game_run_headless(game, 3, target_setup, &target, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    CHECK(target.started && fabsf(target.center.x - 500) < 1e-3f && fabsf(target.center.y - 200) < 1e-3f);

    PonkoVec2 first = window_size_seen(false, true);
    CHECK(first.x == 800 && first.y == 600);
    PonkoVec2 restored = window_size_seen(true, false);
    CHECK(restored.x == 1024 && restored.y == 768);

    game = ponko_game_new("sea", 1, 1);
    CHECK(ponko_game_run_headless(game, 0, spectrum_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);

    game = ponko_game_new("raft", 1, 1);
    CHECK(ponko_game_run_headless(game, 0, buoyancy_desc_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);

    PonkoNavCrowdStats crowd_stats = {.struct_size = sizeof crowd_stats, .version = PONKO_NAV_CROWD_STATS_VERSION};
    game = ponko_game_new("nav crowd", 1, 1);
    CHECK(ponko_game_fixed_hz(game, 60) == PONKO_OK && ponko_game_navmesh3d(game) == PONKO_OK);
    CHECK(ponko_game_sim_system(game, "read", 1, crowd_count_read, &crowd_stats) == PONKO_OK);
    CHECK(ponko_game_run_headless(game, 10, crowd_count_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    CHECK(crowd_stats.steps == 10 && crowd_stats.agent_steps == 30 && crowd_stats.work > 0);

    game = ponko_game_new("allocs", 1, 1);
    CHECK(ponko_game_run_headless(game, 0, alloc_count_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);

    game = ponko_game_new("gpu timing", 1, 1);
    CHECK(ponko_game_run_headless(game, 0, gpu_timing_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);

    SliceCounts counts = {0};
    game = ponko_game_new("walls", 1, 1);
    CHECK(ponko_game_run_headless(game, 0, sliced_setup, &counts, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    CHECK(counts.freed_users == 1);
}

/* gl#190: brain rows woken and signalled from ponko_query_par, and a lossy, reordering loopback link. */
typedef struct BrainStir {
    uint32_t ids[2];
    uint64_t alert, door, chase, idle, enter, guest;
    int checked, wrong;
} BrainStir;

static void stir_rows(const PonkoChunk* chunk, void* user) {
    const BrainStir* s = user;
    for (size_t i = 0; i < chunk->count; ++i) {
        PonkoAiBrain* brain = ponko_chunk_ai_brain(chunk, 0, i);
        PonkoAiBlackboard* board = ponko_chunk_ai_blackboard(chunk, 1, i);
        int even = chunk->entities[i].index % 2 == 0;
        bool guest = false;
        CHECK(ponko_ai_blackboard_flag(board, s->guest, &guest) == PONKO_OK);
        CHECK(ponko_ai_blackboard_set_flag(board, s->alert, true) == PONKO_OK);
        /* Even rows are woken (guards) or hear the door (guests); the others are left alone. */
        if (even && !guest) CHECK(ponko_ai_brain_wake(brain) == PONKO_OK);
        if (even && guest) CHECK(ponko_ai_brain_signal(brain, s->door) == PONKO_OK);
    }
}

static void stir_brains(PonkoCtx* ctx, void* user) {
    BrainStir* s = user;
    if (ponko_ctx_sim_tick(ctx) != 1) return;
    CHECK(ponko_query_par(ponko_ctx_world(ctx), s->ids, 2, stir_rows, s) == PONKO_OK);
}

static void check_stirred_rows(const PonkoChunk* chunk, void* user) {
    BrainStir* s = user;
    for (size_t i = 0; i < chunk->count; ++i) {
        uint64_t action = 1, other = 1;
        int even = chunk->entities[i].index % 2 == 0;
        CHECK(ponko_ai_brain_action(ponko_chunk_ai_brain(chunk, 0, i), 0, &action) == PONKO_OK);
        CHECK(ponko_ai_brain_action(ponko_chunk_ai_brain(chunk, 0, i), 1, &other) == PONKO_OK && other == 0);
        bool guest = false;
        CHECK(ponko_ai_blackboard_flag(ponko_chunk_ai_blackboard(chunk, 1, i), s->guest, &guest) == PONKO_OK);
        /* Guards: chase if woken, idle (not thought since tick 0) otherwise. Guests: enter if signalled. */
        uint64_t expected = guest ? (even ? s->enter : 0) : (even ? s->chase : s->idle);
        int ok = action == expected;
        if (!ok) ++s->wrong;
        ++s->checked;
    }
}

static void check_stirred(PonkoCtx* ctx, void* user) {
    BrainStir* s = user;
    if (ponko_ctx_sim_tick(ctx) != 1) return;
    CHECK(ponko_query(ponko_ctx_world(ctx), s->ids, 2, check_stirred_rows, s) == PONKO_OK);
}

static void stir_setup(PonkoCtx* ctx, void* user) {
    BrainStir* s = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    CHECK(ponko_ai_brain_component(w, &s->ids[0]) == PONKO_OK);
    CHECK(ponko_ai_blackboard_component(w, &s->ids[1]) == PONKO_OK);
    PonkoBehaviorTree* guard = ponko_behavior_tree_new(
        "{\"root\": {\"selector\": [{\"sequence\": [{\"check\": \"alert\"}, {\"do\": \"chase\"}]}, {\"do\": \"idle\"}]}}");
    PonkoBehaviorTree* guest = ponko_behavior_tree_new(
        "{\"root\": {\"sequence\": [{\"wait_for\": \"door\"}, {\"do\": \"enter\"}]}}");
    CHECK(guard && guest);
    CHECK(ponko_ctx_ai_add_tree(ctx, "guard", guard) == PONKO_OK && ponko_ctx_ai_add_tree(ctx, "guest", guest) == PONKO_OK);
    ponko_behavior_tree_free(guard);
    ponko_behavior_tree_free(guest);
    for (int i = 0; i < 2000; ++i) {
        uint64_t e = 0;
        CHECK(ponko_spawn(w, NULL, 0, &e) == PONKO_OK);
        /* Once in a million ticks: only waking or a signal makes them think again. */
        CHECK(ponko_ctx_ai_add_brain(ctx, e, i < 1000 ? "guard" : "guest", 1000000) == PONKO_OK);
        CHECK(ponko_ctx_ai_set_flag(ctx, e, s->guest, i >= 1000) == PONKO_OK);
    }
}

/* Steps a host and a client over `link` at 60 Hz on their own clocks. The host sends 0..99 on both lanes,
   one a frame once connected; returns the frames until every reliable one was in, in order. */
static int lossy_run(const PonkoLoopbackLink* link, int* unreliable_count, uint32_t* unreliable_sum) {
    PonkoNetMessages* e[2] = {NULL, NULL};
    CHECK(ponko_net_messages_loopback_link(2, link, e) == PONKO_OK);
    uint64_t peer = 0;
    int host_saw = 0, joined = 0, frame = 0;
    uint32_t sent = 0, next_reliable = 0;
    *unreliable_count = 0;
    *unreliable_sum = 0;
    for (; frame < 5000 && next_reliable < 100; ++frame) {
        for (int i = 0; i < 2; ++i) CHECK(ponko_net_messages_poll_elapsed(e[i], 1.0 / 60) == PONKO_OK);
        PonkoMessageEvent ev;
        bool found = true;
        while (ponko_net_messages_next_event(e[0], &ev, &found) == PONKO_OK && found)
            if (ev.kind == PONKO_MESSAGE_CONNECTED) { peer = ev.peer; host_saw = 1; }
        while (ponko_net_messages_next_event(e[1], &ev, &found) == PONKO_OK && found) {
            if (ev.kind == PONKO_MESSAGE_CONNECTED) joined = 1;
            if (ev.kind != PONKO_MESSAGE) continue;
            uint32_t n = 0;
            CHECK(ev.len == sizeof n);
            memcpy(&n, ev.bytes, sizeof n);
            if (ev.lane == PONKO_LANE_RELIABLE) {
                CHECK(n == next_reliable); /* every one, in order */
                ++next_reliable;
            } else {
                ++*unreliable_count;
                *unreliable_sum += n;
            }
        }
        if (host_saw && joined && sent < 100) {
            CHECK(ponko_net_messages_send(e[0], peer, PONKO_LANE_RELIABLE, (const uint8_t*)&sent, sizeof sent) == PONKO_OK);
            CHECK(ponko_net_messages_send(e[0], peer, PONKO_LANE_UNRELIABLE, (const uint8_t*)&sent, sizeof sent) == PONKO_OK);
            ++sent;
        }
    }
    CHECK(next_reliable == 100);
    ponko_net_messages_free(e[0]);
    ponko_net_messages_free(e[1]);
    return frame;
}

static void lossy_loopback_and_brain_rows(void) {
    BrainStir s = {{0, 0}, ai_key_of("alert"), ai_key_of("door"), ai_key_of("chase"), ai_key_of("idle"),
                   ai_key_of("enter"), ai_key_of("guest"), 0, 0};
    PonkoGame* game = ponko_game_new("brain rows", 1, 1);
    CHECK(ponko_game_sim_system(game, "stir", 1, stir_brains, &s) == PONKO_OK);
    CHECK(ponko_game_sim_system(game, "think", 1, brain_think, &s) == PONKO_OK);
    CHECK(ponko_game_sim_system(game, "check", 1, check_stirred, &s) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 2, stir_setup, &s, &w) == PONKO_OK);
    ponko_world_free(w);
    CHECK(s.checked == 2000 && s.wrong == 0);
    CHECK(ponko_ai_brain_wake(NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ai_brain_signal(NULL, s.door) == PONKO_INVALID_ARGUMENT);

    PonkoLoopbackLink link = {.struct_size = sizeof link, .version = PONKO_LOOPBACK_LINK_VERSION};
    CHECK(ponko_loopback_link_defaults(&link) == PONKO_OK && link.loss == 0 && link.jitter == 0);
    int perfect_count = 0, lossy_count = 0, again_count = 0;
    uint32_t perfect_sum = 0, lossy_sum = 0, again_sum = 0;
    int perfect_frames = lossy_run(&link, &perfect_count, &perfect_sum);
    CHECK(perfect_count == 100);
    link.loss = 0.2;
    link.delay = 0.03;
    link.jitter = 0.05;
    link.seed = 3;
    int lossy_frames = lossy_run(&link, &lossy_count, &lossy_sum);
    CHECK(lossy_count >= 50 && lossy_count < 100 && lossy_frames > perfect_frames);
    /* The same seed loses the same datagrams. */
    CHECK(lossy_run(&link, &again_count, &again_sum) == lossy_frames && again_count == lossy_count && again_sum == lossy_sum);
    link.loss = 1.5;
    PonkoNetMessages* e[2] = {NULL, NULL};
    CHECK(ponko_net_messages_loopback_link(2, &link, e) == PONKO_INVALID_ARGUMENT);
    link.loss = 0;
    link.version = 0;
    CHECK(ponko_net_messages_loopback_link(2, &link, e) == PONKO_INVALID_ARGUMENT);
}

/* gl#202: brain rows signalled with a value, a water body's waiting ripples, a UI's own safe area. */
typedef struct {
    uint32_t ids[2];
    uint64_t shout, heard, react, guards[5];
    int checked;
} Voices;

static void voice_rows(const PonkoChunk* chunk, void* user) {
    const Voices* v = user;
    for (size_t i = 0; i < chunk->count; ++i) {
        size_t at = 0;
        while (v->guards[at] != (uint64_t)chunk->entities[i].index) ++at;
        PonkoAiBrain* b = ponko_chunk_ai_brain(chunk, 0, i);
        switch (at) {
        case 0: CHECK(ponko_ai_brain_signal_num(b, v->shout, 7.5f) == PONKO_OK); break;
        case 1: CHECK(ponko_ai_brain_signal_entity(b, v->shout, 42) == PONKO_OK); break;
        case 2: CHECK(ponko_ai_brain_signal_vec2(b, v->shout, (PonkoVec2){1, -2}) == PONKO_OK); break;
        case 3: CHECK(ponko_ai_brain_signal_vec3(b, v->shout, (PonkoVec3){3, 4, 5}) == PONKO_OK); break;
        default: CHECK(ponko_ai_brain_signal(b, v->shout) == PONKO_OK); break; /* nothing to store */
        }
    }
}

static void voice_hear_rows(const PonkoChunk* chunk, void* user) {
    Voices* v = user;
    for (size_t i = 0; i < chunk->count; ++i) {
        size_t at = 0;
        while (v->guards[at] != (uint64_t)chunk->entities[i].index) ++at;
        PonkoAiBlackboard* board = ponko_chunk_ai_blackboard(chunk, 1, i);
        uint64_t action = 1, e = 0;
        float n = 0;
        PonkoVec2 p2 = {0};
        PonkoVec3 p3 = {0};
        CHECK(ponko_ai_brain_action(ponko_chunk_ai_brain(chunk, 0, i), 0, &action) == PONKO_OK && action == v->react);
        CHECK((ponko_ai_blackboard_num(board, v->heard, &n) == PONKO_OK) == (at == 0));
        CHECK((ponko_ai_blackboard_entity(board, v->heard, &e) == PONKO_OK) == (at == 1));
        CHECK((ponko_ai_blackboard_vec2(board, v->heard, &p2) == PONKO_OK) == (at == 2));
        CHECK((ponko_ai_blackboard_vec3(board, v->heard, &p3) == PONKO_OK) == (at == 3));
        if (at == 0) CHECK(n == 7.5f);
        if (at == 1) CHECK(e == 42);
        if (at == 2) CHECK(p2.x == 1 && p2.y == -2);
        if (at == 3) CHECK(p3.x == 3 && p3.y == 4 && p3.z == 5);
        ++v->checked;
    }
}

static void voice_system(PonkoCtx* ctx, void* user) {
    Voices* v = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    if (ponko_ctx_sim_tick(ctx) == 1) CHECK(ponko_query_par(w, v->ids, 2, voice_rows, v) == PONKO_OK);
    if (ponko_ctx_sim_tick(ctx) == 2) CHECK(ponko_query(w, v->ids, 2, voice_hear_rows, v) == PONKO_OK);
}

static void voice_setup(PonkoCtx* ctx, void* user) {
    Voices* v = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    CHECK(ponko_ai_brain_component(w, &v->ids[0]) == PONKO_OK);
    CHECK(ponko_ai_blackboard_component(w, &v->ids[1]) == PONKO_OK);
    PonkoBehaviorTree* tree = ponko_behavior_tree_new(
        "{\"root\": {\"sequence\": [{\"wait_for\": \"shout\", \"store\": \"heard\"}, {\"do\": \"react\"}]}}");
    CHECK(tree && ponko_ctx_ai_add_tree(ctx, "listener", tree) == PONKO_OK);
    ponko_behavior_tree_free(tree);
    for (int i = 0; i < 5; ++i) {
        uint64_t e = 0;
        CHECK(ponko_spawn(w, NULL, 0, &e) == PONKO_OK);
        CHECK(ponko_ctx_ai_add_brain(ctx, e, "listener", 1000000) == PONKO_OK);
        v->guards[i] = (uint64_t)(uint32_t)e; /* the entity's index (generation 0) */
    }
}

typedef struct { Cars cars; uint64_t lake; bool ripples; uint64_t started; } Splash;
static void splash_setup(PonkoCtx* ctx, void* user) {
    Splash* s = user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    ids(w, &s->cars);
    PonkoWaterDesc lake = {.struct_size = sizeof lake, .version = PONKO_WATER_DESC_VERSION};
    CHECK(ponko_water_desc_lake(&lake, (PonkoVec2){0, 0}, (PonkoVec2){20, 20}, 0) == PONKO_OK);
    CHECK(ponko_ctx_spawn_water(ctx, &lake, &s->lake) == PONKO_OK);
    PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){1, 1.5f, 0});
    PonkoRigidBody3D body = ponko_rigid_body3d(PONKO_BODY_DYNAMIC);
    PonkoCollider3D box = ponko_collider3d_box((PonkoVec3){0.8f, 0.8f, 0.8f});
    box.density = 500;
    PonkoValue values[] = {{s->cars.transform, &at, sizeof at}, {s->cars.body, &body, sizeof body},
                           {s->cars.collider, &box, sizeof box}};
    uint64_t e;
    CHECK(ponko_spawn(w, values, 3, &e) == PONKO_OK);
    PonkoBuoyancyDesc d = {.struct_size = sizeof d, .version = PONKO_BUOYANCY_DESC_VERSION};
    CHECK(ponko_buoyancy_desc_box(&d, (PonkoVec3){0.8f, 0.8f, 0.8f}) == PONKO_OK);
    d.ripples = s->ripples;
    CHECK(ponko_ctx_set_buoyancy(ctx, e, &d) == PONKO_OK);
    uint32_t waiting = UINT32_MAX;
    CHECK(ponko_ctx_water_waiting_contacts(ctx, s->lake, &waiting) == PONKO_OK && waiting == 0);
}
static void splash_watch(PonkoCtx* ctx, void* user) {
    Splash* s = user;
    uint64_t n = UINT64_MAX;
    CHECK(ponko_ctx_water_started_contacts(ctx, s->lake, &n) == PONKO_OK);
    s->started = n;
}
static uint64_t splashed(bool ripples) {
    Splash s = {0};
    s.ripples = ripples;
    PonkoGame* game = ponko_game_new("splash", 1, 1);
    CHECK(ponko_game_physics3d(game, (PonkoVec3){0, -9.81f, 0}) == PONKO_OK && ponko_game_water(game) == PONKO_OK);
    CHECK(ponko_game_sim_system(game, "watch", 1, splash_watch, &s) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 100, splash_setup, &s, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    return s.started;
}

static void panel_area_setup(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoWorld* w = ponko_ctx_world(ctx);
    uint32_t transform;
    CHECK(ponko_transform3d_component(w, &transform) == PONKO_OK);
    PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){0, 0, -3});
    PonkoValue value = {transform, &at, sizeof at};
    uint64_t panel;
    CHECK(ponko_spawn(w, &value, 1, &panel) == PONKO_OK);
    CHECK(ponko_ctx_create_ui_panel(ctx, panel, 200, 100, (PonkoVec2){2, 1}, NULL) == PONKO_OK);
    PonkoUiLayout fill = ponko_ui_layout_fill();
    uint64_t inside, outside;
    CHECK(ponko_ctx_ui_add_panel(ctx, panel, PONKO_UI_NONE, &fill, &inside) == PONKO_OK);
    CHECK(ponko_ctx_ui_add_panel(ctx, panel, PONKO_UI_NONE, &fill, &outside) == PONKO_OK);
    CHECK(ponko_ctx_ui_set_inside_safe_area(ctx, panel, inside, true) == PONKO_OK);
    CHECK(ponko_ctx_ui_show_hud(ctx, panel, inside) == PONKO_OK && ponko_ctx_ui_show_hud(ctx, panel, outside) == PONKO_OK);
    PonkoRect area = {{10, 5}, {190, 95}}, got, r;
    CHECK(ponko_ctx_ui_safe_area(ctx, panel, &got) == PONKO_NOT_FOUND); /* none yet */
    CHECK(ponko_ctx_ui_set_safe_area(ctx, panel, &area) == PONKO_OK);
    CHECK(ponko_ctx_ui_safe_area(ctx, panel, &got) == PONKO_OK && rect_is(got, 10, 5, 190, 95));
    CHECK(ponko_ctx_ui_layout(ctx, panel, (PonkoVec2){200, 100}) == PONKO_OK);
    CHECK(ponko_ctx_ui_rect(ctx, panel, inside, &r) == PONKO_OK && rect_is(r, 10, 5, 190, 95));
    CHECK(ponko_ctx_ui_rect(ctx, panel, outside, &r) == PONKO_OK && rect_is(r, 0, 0, 200, 100));
    CHECK(ponko_ctx_ui_safe_area(ctx, PONKO_UI_SCREEN, &got) == PONKO_NOT_FOUND); /* the screen's UI is its own */
    CHECK(ponko_ctx_ui_set_safe_area(ctx, panel, NULL) == PONKO_OK);
    CHECK(ponko_ctx_ui_safe_area(ctx, panel, &got) == PONKO_NOT_FOUND);
    CHECK(ponko_ctx_ui_layout(ctx, panel, (PonkoVec2){200, 100}) == PONKO_OK);
    CHECK(ponko_ctx_ui_rect(ctx, panel, inside, &r) == PONKO_OK && rect_is(r, 0, 0, 200, 100));
    CHECK(ponko_ctx_ui_safe_area(ctx, panel, NULL) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_ui_safe_area(NULL, panel, &got) == PONKO_INVALID_ARGUMENT);
}

static void brain_values_ripples_and_ui_area(void) {
    Voices v = {{0, 0}, ai_key_of("shout"), ai_key_of("heard"), ai_key_of("react"), {0}, 0};
    PonkoGame* game = ponko_game_new("voices", 1, 1);
    CHECK(ponko_game_sim_system(game, "voices", 1, voice_system, &v) == PONKO_OK);
    CHECK(ponko_game_sim_system(game, "think", 1, brain_think, &v) == PONKO_OK);
    PonkoWorld* w = NULL;
    CHECK(ponko_game_run_headless(game, 3, voice_setup, &v, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
    CHECK(v.checked == 5);
    CHECK(ponko_ai_brain_signal_num(NULL, v.shout, 1) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ai_brain_signal_entity(NULL, v.shout, 1) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ai_brain_signal_vec2(NULL, v.shout, (PonkoVec2){0, 0}) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ai_brain_signal_vec3(NULL, v.shout, (PonkoVec3){0, 0, 0}) == PONKO_INVALID_ARGUMENT);

    /* A body falling in starts ripples with its switch on, none with it off. */
    CHECK(splashed(true) > 0);
    CHECK(splashed(false) == 0);
    uint64_t none = 7;
    CHECK(ponko_ctx_water_started_contacts(NULL, 0, &none) == PONKO_INVALID_ARGUMENT);

    game = ponko_game_new("panel area", 1, 1);
    CHECK(ponko_game_run_headless(game, 0, panel_area_setup, NULL, &w) == PONKO_OK);
    CHECK(ponko_world_free(w) == PONKO_OK);
}

int main(void) {
    states();
    change_detection();
    declared_access();
    command_barriers();
    command_spawn_ids();
    steam_messages_validation();
    lobby_client();
    rtc_endpoints();
    morph_clip_api();
    replication();
    CHECK(ponko_abi_version() == 2);
    /* The macro in the header and the loaded library agree. */
    CHECK(ponko_abi_version() == PONKO_ABI_VERSION);
    PonkoWorld* w = ponko_world_new(); CHECK(w);
    uint32_t p; CHECK(ponko_component_register(w, "Pos", sizeof(Position), _Alignof(Position), &p) == PONKO_OK);
    Position initial = {7}; PonkoValue value = {p, &initial, sizeof(initial)};
    uint64_t e; CHECK(ponko_spawn(w, &value, 1, &e) == PONKO_OK);
    Context context = {w, 0, e}; CHECK(ponko_query(w, &p, 1, visit, &context) == PONKO_OK); CHECK(context.seen == 1);
    Position result; CHECK(ponko_get(w, e, p, &result, sizeof(result)) == PONKO_OK && result.x == 9);
    CHECK(ponko_get(w, e, p, &result, 0) == PONKO_INVALID_ARGUMENT);
    uint32_t duplicate[] = {p, p}; CHECK(ponko_query(w, duplicate, 2, visit, &context) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_despawn(w, e) == PONKO_OK); bool alive = true;
    CHECK(ponko_is_alive(w, e, &alive) == PONKO_OK && !alive);
    CHECK(ponko_despawn(w, e) == PONKO_NOT_FOUND);
    CHECK(ponko_world_free(w) == PONKO_OK);

    w = ponko_world_new();
    uint32_t probe_id;
    CHECK(ponko_reflection_probe3d_component(w, &probe_id) == PONKO_OK);
    PonkoReflectionProbe3D probe = ponko_reflection_probe3d_default();
    CHECK(probe.content == 0 && probe.box_projection && probe.size.x == 10);
    PonkoValue probe_value = {probe_id, &probe, sizeof(probe)};
    CHECK(ponko_spawn(w, &probe_value, 1, &e) == PONKO_OK);
    PonkoReflectionProbe3D probe_out;
    CHECK(ponko_get(w, e, probe_id, &probe_out, sizeof(probe_out)) == PONKO_OK && probe_out.blend_distance == 1);
    uint32_t decal_id, receives_id;
    CHECK(ponko_decal3d_component(w, &decal_id) == PONKO_OK);
    CHECK(ponko_receives_decals_component(w, &receives_id) == PONKO_OK);
    PonkoDecal3D decal = ponko_decal3d_default();
    CHECK(decal.opacity == 1 && decal.image == 0 && decal.normal_image == PONKO_DECAL_NO_IMAGE && decal.layers == 0xFFFF);
    decal.size.x = 2; decal.order = 4;
    PonkoReceivesDecals none = {0};
    PonkoValue decal_values[] = {{decal_id, &decal, sizeof(decal)}, {receives_id, &none, sizeof(none)}};
    CHECK(ponko_spawn(w, decal_values, 2, &e) == PONKO_OK);
    PonkoDecal3D decal_out; PonkoReceivesDecals none_out = {1};
    CHECK(ponko_get(w, e, decal_id, &decal_out, sizeof(decal_out)) == PONKO_OK && decal_out.size.x == 2 && decal_out.order == 4);
    CHECK(ponko_get(w, e, receives_id, &none_out, sizeof(none_out)) == PONKO_OK && none_out.layers == 0);
    bool shadow_only = true;
    CHECK(ponko_world_is_shadow_only(w, e, &shadow_only) == PONKO_OK && !shadow_only);
    CHECK(ponko_world_set_shadow_only(w, e, true) == PONKO_OK);
    CHECK(ponko_world_is_shadow_only(w, e, &shadow_only) == PONKO_OK && shadow_only);
    CHECK(ponko_world_set_shadow_only(w, e, false) == PONKO_OK);
    CHECK(ponko_world_is_shadow_only(w, e, &shadow_only) == PONKO_OK && !shadow_only);
    CHECK(ponko_world_set_shadow_only(w, 42, true) == PONKO_NOT_FOUND);
    uint32_t point_id, spot_id;
    CHECK(ponko_point_light3d_component(w, &point_id) == PONKO_OK);
    CHECK(ponko_spot_light3d_component(w, &spot_id) == PONKO_OK);
    PonkoPointLight3D point = {{1, .5f, .1f, 1}, 20, 12, true, 512};
    PonkoSpotLight3D spot = {{.1f, .5f, 1, 1}, 30, 18, .2f, .5f, true, 0};
    PonkoValue lights[] = {{point_id, &point, sizeof(point)}, {spot_id, &spot, sizeof(spot)}};
    CHECK(ponko_spawn(w, lights, 2, &e) == PONKO_OK);
    PonkoPointLight3D point_out;
    PonkoSpotLight3D spot_out;
    CHECK(ponko_get(w, e, point_id, &point_out, sizeof(point_out)) == PONKO_OK && point_out.intensity == 20);
    CHECK(point_out.shadow_resolution == 512 && point_out.shadows);
    CHECK(ponko_get(w, e, spot_id, &spot_out, sizeof(spot_out)) == PONKO_OK && spot_out.outer_angle == .5f);
    CHECK(ponko_world_free(w) == PONKO_OK);

    Run r = {0, 0, PONKO_OK};
    PonkoGame* game = ponko_game_new("c tests", 1, 1); CHECK(game);
    CHECK(ponko_game_fixed_hz(game, 50) == PONKO_OK);
    CHECK(ponko_game_fixed_hz(game, 0) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_game_sim_system(game, "tick", 1, tick, &r) == PONKO_OK);
    PonkoWorld* sim = NULL;
    CHECK(ponko_game_run_headless(game, 100, setup, NULL, &sim) == PONKO_OK);
    CHECK(r.ticks == 3 && r.dt > .019f && r.dt < .021f && sim);
    CHECK(ponko_world_free(sim) == PONKO_OK);
    PonkoDrawParams params = ponko_draw_params_default();
    CHECK(params.scale.x == 1 && params.pivot.x == .5f && !params.screen);
    vehicles();
    physics_queries3d_in_loops();
    phases();
    plugins_setups_and_system_ids();
    retained_ui();
    ui_panels();
    ribbons_and_decals();
    lod_helpers();
    streamed_worlds();
    terrain_rebuild();
    model_material_overrides();
    asset_streaming();
    profiling_c();
    animation_graphs();
    crowds();
    skeletons_2d();
    query_filters();
    query_cache();
    wide_queries();
    native_coverage();
    headless_frames_ui();
    safe_area_c();
    focus_huds_and_collision_files();
    debug_drawing();
    descriptors_retargeting_and_prediction();
    render_descriptors();
    camera_rigs();
    headless_play();
    world_brains_in_loops();
    native_parity_round5();
    lossy_loopback_and_brain_rows();
    brain_values_ripples_and_ui_area();
    return 0;
}
