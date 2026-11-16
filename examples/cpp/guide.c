// The guide's C snippets (book/src/*.md include them by anchor), compiled
// and run headlessly by ctest (c_guide), so a C example in the guide can't
// fall behind the header.
#include "ponko.h"
#include <stdio.h>
#include <stdlib.h>

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "failed: %s (line %d)\n", #x, __LINE__); abort(); } } while (0)

typedef struct { float value; } Health;
typedef struct { uint8_t unused; } Marker;

static uint32_t player_id, enemy_id, dead_id, health_id;
static int hits;

static void damage(const PonkoChunk* chunk, void* user) {
    (void)user;
    Health* health = (Health*)chunk->columns[0];
    for (size_t i = 0; i < chunk->count; ++i) health[i].value -= 1.0f;
    hits += (int)chunk->count;
}

static void abi_check(void) {
    // ANCHOR: abi_check
    if (ponko_abi_version() != PONKO_ABI_VERSION) { /* wrong library: stop */ }
    // ANCHOR_END: abi_check
    CHECK(ponko_abi_version() == PONKO_ABI_VERSION);
}

static void filters(void) {
    PonkoWorld* world = ponko_world_new();
    CHECK(ponko_component_register(world, "Player", 1, 1, &player_id) == PONKO_OK);
    CHECK(ponko_component_register(world, "Enemy", 1, 1, &enemy_id) == PONKO_OK);
    CHECK(ponko_component_register(world, "Dead", 1, 1, &dead_id) == PONKO_OK);
    CHECK(ponko_component_register(world, "Health", sizeof(Health), _Alignof(Health), &health_id) == PONKO_OK);
    Marker m = {0};
    Health h = {10};
    uint64_t e = 0;
    PonkoValue player[] = {{player_id, &m, 1}, {health_id, &h, sizeof h}};
    PonkoValue enemy[] = {{enemy_id, &m, 1}, {health_id, &h, sizeof h}};
    PonkoValue corpse[] = {{enemy_id, &m, 1}, {dead_id, &m, 1}, {health_id, &h, sizeof h}};
    PonkoValue bystander[] = {{health_id, &h, sizeof h}};
    CHECK(ponko_spawn(world, player, 2, &e) == PONKO_OK);
    CHECK(ponko_spawn(world, enemy, 2, &e) == PONKO_OK);
    CHECK(ponko_spawn(world, corpse, 3, &e) == PONKO_OK);
    CHECK(ponko_spawn(world, bystander, 1, &e) == PONKO_OK);

    // ANCHOR: ecs_filters
    // Living: (Player OR Enemy) AND NOT Dead.
    PonkoFilter* player_filter = ponko_filter_with(player_id);
    PonkoFilter* enemy_filter = ponko_filter_with(enemy_id);
    PonkoFilter* alive = ponko_filter_without(dead_id);
    const PonkoFilter* fighters[] = {player_filter, enemy_filter};
    PonkoFilter* fighter = ponko_filter_any(fighters, 2);
    const PonkoFilter* living_parts[] = {fighter, alive};
    PonkoFilter* living = ponko_filter_all(living_parts, 2);
    ponko_query_filtered(world, &health_id, 1, living, NULL, damage, NULL);
    // ... then ponko_filter_free() each of them.
    // ANCHOR_END: ecs_filters
    CHECK(ponko_filter_check(world, living) == PONKO_OK);
    CHECK(hits == 2);
    ponko_filter_free(living);
    ponko_filter_free(fighter);
    ponko_filter_free(alive);
    ponko_filter_free(enemy_filter);
    ponko_filter_free(player_filter);
    CHECK(ponko_world_free(world) == PONKO_OK);
}

static void handles(PonkoCtx* ctx, void* user) {
    (void)user;
    PonkoMeshData* cube = ponko_mesh_data_cube(1.0f);
    PonkoMaterial material;
    CHECK(ponko_ctx_default_material(ctx, &material) == PONKO_OK);
    PonkoMesh mesh;
    CHECK(ponko_ctx_create_mesh(ctx, cube, material, &mesh) == PONKO_OK);
    ponko_mesh_data_free(cube);
    PonkoTexture texture;
    const uint8_t pixel[4] = {255, 255, 255, 255};
    CHECK(ponko_ctx_create_texture(ctx, 1, 1, pixel, 4, &texture) == PONKO_OK);
    uint32_t id = texture.id;

    // ANCHOR: handles
    bool live = false;
    ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_MESH, mesh.id, &live);
    ponko_ctx_handle_free(ctx, PONKO_HANDLE_MESH, mesh.id);
    PonkoLiveCounts counts;
    ponko_ctx_live_counts(ctx, &counts);
    // ANCHOR_END: handles
    CHECK(live);
    CHECK(ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_MESH, mesh.id, &live) == PONKO_OK && !live);

    // ANCHOR: asset_inventory
    PonkoAssetCount textures;
    ponko_ctx_asset_count(ctx, PONKO_ASSET_CLASS_TEXTURE, &textures);
    size_t n = 0;
    ponko_ctx_asset_handles(ctx, PONKO_ASSET_CLASS_TEXTURE, NULL, 0, &n); /* how many */
    /* ...then again with an array of n PonkoAssetRecord: id, owner, bytes, made_frame, age_frames */
    char name[256];
    size_t len = 0;
    ponko_ctx_asset_owner(ctx, PONKO_ASSET_CLASS_TEXTURE, id, name, sizeof name, &len);
    bool unloaded = false;
    ponko_ctx_unload_mesh(ctx, "models/house.glb", &unloaded);
    // ANCHOR_END: asset_inventory
    CHECK(n >= 1 && textures.live >= 1 && !unloaded);
}

static void retained_ui(PonkoCtx* ctx, void* user) {
    (void)user;
    // ANCHOR: ui_c
    PonkoUiLayout column = ponko_ui_layout_default();
    column.flex_direction = PONKO_UI_FLEX_COLUMN;
    column.width = (PonkoUiLength){PONKO_UI_LENGTH_PERCENT, 1.0f};
    column.padding.top = column.padding.right = column.padding.bottom = column.padding.left =
        (PonkoUiLength){PONKO_UI_LENGTH_POINTS, 24.0f};

    uint64_t root, buy;
    ponko_ctx_ui_add_panel(ctx, PONKO_UI_SCREEN, PONKO_UI_NONE, &column, &root);
    ponko_ctx_ui_add_button(ctx, PONKO_UI_SCREEN, root, "Buy", NULL, &buy); /* NULL layout: default */
    ponko_ctx_ui_push_screen_with_mode(ctx, PONKO_UI_SCREEN, root, PONKO_UI_SCREEN_MODE_MODELESS);

    PonkoUiEvent event;
    while (ponko_ctx_ui_next_event(ctx, PONKO_UI_SCREEN, &event) == PONKO_OK)
        if (event.kind == PONKO_UI_EVENT_CLICKED && event.id == buy) { /* ... */ }
    // ANCHOR_END: ui_c
    uint64_t top = PONKO_UI_NONE;
    CHECK(ponko_ctx_ui_top_screen(ctx, PONKO_UI_SCREEN, &top) == PONKO_OK && top == root);

    PonkoTransform3D at = ponko_transform3d_at((PonkoVec3){0, 1, -2});
    uint32_t transform3d = 0;
    CHECK(ponko_transform3d_component(ponko_ctx_world(ctx), &transform3d) == PONKO_OK);
    PonkoValue placed = {transform3d, &at, sizeof at};
    uint64_t terminal = 0;
    CHECK(ponko_spawn(ponko_ctx_world(ctx), &placed, 1, &terminal) == PONKO_OK);
    // ANCHOR: ui_panel_c
    PonkoTexture texture;
    ponko_ctx_create_ui_panel(ctx, terminal, 600, 400, (PonkoVec2){1.2f, 0.8f}, &texture);
    uint64_t button;
    ponko_ctx_ui_add_button(ctx, terminal, PONKO_UI_NONE, "Buy", NULL, &button);
    ponko_ctx_ui_push_screen_with_mode(ctx, terminal, button, PONKO_UI_SCREEN_MODE_MODELESS);
    // ANCHOR_END: ui_panel_c
    CHECK(ponko_ctx_ui_top_screen(ctx, terminal, &top) == PONKO_OK && top == button);
}

static void versioned_descriptors(PonkoCtx* ctx) {
    // ANCHOR: versioned_descriptor
    PonkoLightingEx light = {.struct_size = sizeof light, .version = PONKO_LIGHTING_EX_VERSION};
    CHECK(ponko_ctx_lighting_ex(ctx, &light) == PONKO_OK);  // the current lighting, all fields
    light.lighting.shadow_filter = 4;                        // contact-hardening shadows
    light.shadow_sun_size = 1.5f;                            // a field ABI 2's PonkoLighting lacks
    CHECK(ponko_ctx_set_lighting_ex(ctx, &light) == PONKO_OK);

    PonkoMaterialDescEx glass = {.struct_size = sizeof glass, .version = PONKO_MATERIAL_DESC_EX_VERSION};
    CHECK(ponko_material_desc_ex_defaults(&glass) == PONKO_OK);  // white, roughness 0.6, no stencil
    glass.desc.color = (PonkoColor){.6f, .8f, 1, .5f};
    glass.has_stencil = true;
    glass.stencil = ponko_stencil_mark(1);
    PonkoMaterial material;
    CHECK(ponko_ctx_create_material_ex(ctx, &glass, &material) == PONKO_OK);
    // ANCHOR_END: versioned_descriptor

    // ANCHOR: versioned_post
    PonkoPostEx post = {.struct_size = sizeof post, .version = PONKO_POST_EX_VERSION};
    CHECK(ponko_ctx_post_ex(ctx, &post) == PONKO_OK);
    post.split_toning_shadows = (PonkoColor){.35f, .55f, .6f, 1};      // teal shadows
    post.split_toning_highlights = (PonkoColor){.7f, .5f, .35f, 1};    // orange highlights
    post.dof_enabled = true;                                           // blur what is out of focus
    post.dof_focus_distance = 4;
    CHECK(ponko_ctx_set_post_ex(ctx, &post) == PONKO_OK);

    PonkoParticleSettings3DEx smoke = {.struct_size = sizeof smoke, .version = PONKO_PARTICLE_SETTINGS3D_EX_VERSION};
    CHECK(ponko_particle_settings3d_ex_defaults(&smoke) == PONKO_OK);
    smoke.settings.rate = 20;
    smoke.softness = 1.5f;                                             // fade where it meets the ground
    // ponko_ctx_set_particle_emitter3d_ex(ctx, entity, &smoke) gives an entity the emitter
    // ANCHOR_END: versioned_post
}

static void setup(PonkoCtx* ctx, void* user) {
    handles(ctx, user);
    retained_ui(ctx, user);
    versioned_descriptors(ctx);
}

int main(void) {
    abi_check();
    filters();
    PonkoGame* game = ponko_game_new("c guide", 1, 1);
    CHECK(game);
    PonkoWorld* world = NULL;
    CHECK(ponko_game_run_headless(game, 1, setup, NULL, &world) == PONKO_OK);
    CHECK(ponko_world_free(world) == PONKO_OK);
    puts("C guide snippets ok");
    return 0;
}
