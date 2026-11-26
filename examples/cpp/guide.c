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

    // ANCHOR: render_target_kind
    PonkoTexture target; /* a render target is a PonkoTexture too */
    ponko_ctx_create_render_target(ctx, 64, 64, &target);
    bool is_target = false, image_is_target = true;
    ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_RENDER_TARGET, target.id, &is_target);        /* true */
    ponko_ctx_handle_is_live(ctx, PONKO_HANDLE_RENDER_TARGET, texture.id, &image_is_target); /* false */
    // ANCHOR_END: render_target_kind
    CHECK(is_target && !image_is_target);
    CHECK(ponko_ctx_handle_free(ctx, PONKO_HANDLE_RENDER_TARGET, texture.id) == PONKO_INVALID_ARGUMENT);
    CHECK(ponko_ctx_handle_free(ctx, PONKO_HANDLE_RENDER_TARGET, target.id) == PONKO_OK);

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

    // ANCHOR: ui_widgets_c
    const char* levels[] = {"Low", "Medium", "High"};
    PonkoUiLayout field = ponko_ui_layout_fixed(220, 32);
    uint64_t quality, list;
    ponko_ctx_ui_add_dropdown(ctx, PONKO_UI_SCREEN, root, levels, 3, &field, &quality);
    ponko_ctx_ui_set_tooltip(ctx, PONKO_UI_SCREEN, quality, "How detailed the world looks");
    /* A list keeps nodes only for the rows on screen: 10 000 items cost what 10 do. */
    PonkoUiLayout box = ponko_ui_layout_fixed(300, 240);
    ponko_ctx_ui_add_list(ctx, PONKO_UI_SCREEN, root, 1, 24.0f, &box, &list);
    ponko_ctx_ui_set_list_len(ctx, PONKO_UI_SCREEN, list, 10000);

    /* The extended event carries what the widget set's events need. */
    PonkoUiEventEx ev = {.struct_size = sizeof ev, .version = PONKO_UI_EVENT_EX_VERSION};
    while (ponko_ctx_ui_next_event_ex(ctx, PONKO_UI_SCREEN, &ev) == PONKO_OK) {
        uint64_t choice = PONKO_UI_NO_INDEX;
        if (ev.kind == PONKO_UI_EVENT_CHANGED && ev.id == quality)
            ponko_ctx_ui_selected(ctx, PONKO_UI_SCREEN, quality, &choice);
        else if (ev.kind == PONKO_UI_EVENT_CHOSEN) { /* context-menu entry ev.index of node ev.id */ }
    }
    /* A custom theme: the dark one with square corners. */
    PonkoUiThemeEx theme = {.struct_size = sizeof theme, .version = PONKO_UI_THEME_EX_VERSION};
    ponko_ui_theme_ex_dark(&theme);
    theme.radius = 0;
    ponko_ctx_ui_set_theme_ex(ctx, PONKO_UI_SCREEN, &theme);
    // ANCHOR_END: ui_widgets_c
    uint64_t chosen = PONKO_UI_NONE;
    CHECK(ponko_ctx_ui_selected(ctx, PONKO_UI_SCREEN, quality, &chosen) == PONKO_OK && chosen == 0);

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

    // ANCHOR: glass_c
    PonkoMaterialDescEx green_glass = {.struct_size = sizeof green_glass, .version = PONKO_MATERIAL_DESC_EX_VERSION};
    CHECK(ponko_material_desc_ex_defaults(&green_glass) == PONKO_OK);
    green_glass.desc.roughness = 0.12f;
    green_glass.layers.ior = 1.45f;
    green_glass.transmission.factor = 1.0f;
    green_glass.transmission.thickness = 0.15f;
    green_glass.transmission.attenuation_distance = 3.0f;
    green_glass.transmission.attenuation_color = (PonkoColor){0.7f, 0.9f, 0.8f, 1};
    PonkoMaterial glass_material;
    CHECK(ponko_ctx_create_material_ex(ctx, &green_glass, &glass_material) == PONKO_OK);
    // ANCHOR_END: glass_c

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

static void debug_map_camera(PonkoCtx* ctx) {
    // ANCHOR: debug_map_camera_c
    PonkoCamera3DEx main = {.struct_size = sizeof main, .version = PONKO_CAMERA3D_EX_VERSION};
    CHECK(ponko_ctx_camera3d_ex(ctx, &main) == PONKO_OK);
    main.debug_draw = PONKO_DEBUG_DRAW_OFF;          // the player's view stays clean
    CHECK(ponko_ctx_set_camera3d_ex(ctx, &main) == PONKO_OK);

    PonkoTexture map;                                // a corner of the screen, a tool
    CHECK(ponko_ctx_create_render_target(ctx, 256, 256, &map) == PONKO_OK);
    PonkoTargetCamera above = {.camera = main.camera, .target = map};
    above.camera.position = (PonkoVec3){0, 120, 0};
    above.camera.rotation = ponko_quat_from_axis_angle((PonkoVec3){1, 0, 0}, -1.5707964f);  // straight down
    PonkoTargetCameraEx map_camera = {.struct_size = sizeof map_camera, .version = PONKO_TARGET_CAMERA_EX_VERSION};
    CHECK(ponko_target_camera_ex_from(&above, &map_camera) == PONKO_OK);
    map_camera.debug_draw = PONKO_DEBUG_DRAW_ON;     // the map shows the paths and sight cones
    const PonkoTargetCameraEx* cameras[1] = {&map_camera};
    CHECK(ponko_ctx_set_target_cameras_ex(ctx, cameras, 1) == PONKO_OK);
    // ANCHOR_END: debug_map_camera_c
    PonkoTargetCameraEx read = {.struct_size = sizeof read, .version = PONKO_TARGET_CAMERA_EX_VERSION};
    CHECK(ponko_ctx_target_camera_ex(ctx, 0, &read) == PONKO_OK && read.debug_draw == PONKO_DEBUG_DRAW_ON);
    CHECK(read.target_camera.camera.position.y == 120 && read.target_camera.target.id == map.id);
    PonkoCamera3DEx window = {.struct_size = sizeof window, .version = PONKO_CAMERA3D_EX_VERSION};
    CHECK(ponko_ctx_camera3d_ex(ctx, &window) == PONKO_OK && window.debug_draw == PONKO_DEBUG_DRAW_OFF);
    CHECK(ponko_ctx_set_target_cameras_ex(ctx, NULL, 0) == PONKO_OK);
    window.debug_draw = PONKO_DEBUG_DRAW_DEFAULT;
    CHECK(ponko_ctx_set_camera3d_ex(ctx, &window) == PONKO_OK);
}

static void loopback_link(void) {
    // ANCHOR: loopback_link_c
    PonkoLoopbackLink link = {.struct_size = sizeof link, .version = PONKO_LOOPBACK_LINK_VERSION};
    CHECK(ponko_loopback_link_defaults(&link) == PONKO_OK);  // a perfect link
    link.loss = 0.05;                                        // 5% lost each way
    link.delay = 0.017;                                      // a frame of delay
    link.jitter = 0.05;                                      // plus up to 50 ms: packets overtake
    link.seed = 1;
    PonkoNetMessages* nets[3];
    CHECK(ponko_net_messages_loopback_link(3, &link, nets) == PONKO_OK);
    // Every frame, each endpoint moves on by the same step on its own clock:
    for (int i = 0; i < 3; ++i) CHECK(ponko_net_messages_poll_elapsed(nets[i], 1.0 / 60) == PONKO_OK);
    // ANCHOR_END: loopback_link_c
    for (int i = 0; i < 3; ++i) ponko_net_messages_free(nets[i]);
}

// ANCHOR: sliced_loader_c
// A level's walls, read a row at a time so a big file never stalls a frame.
typedef struct { char* text; size_t len, at, walls; } WallCount;

static uint32_t walls_start(void* user, const uint8_t* bytes, size_t len, void** out_state) {
    (void)user;
    WallCount* w = calloc(1, sizeof *w);
    if (!w || !(w->text = malloc(len ? len : 1))) { free(w); return PONKO_INVALID_ARGUMENT; }  // not PONKO_OK: the group fails
    for (size_t i = 0; i < len; ++i) w->text[i] = (char)bytes[i];  // copy: bytes lives only during this call
    w->len = len;
    *out_state = w;
    return PONKO_OK;
}
static uint32_t walls_step(void* user, void* state, bool* out_done) {
    (void)user;
    WallCount* w = state;
    for (; w->at < w->len && w->text[w->at] != '\n'; ++w->at) w->walls += w->text[w->at] == '#';
    if (w->at < w->len) ++w->at;  // one row a step
    *out_done = w->at == w->len;
    return PONKO_OK;
}
static void walls_free(void* user, void* state) {
    (void)user;
    WallCount* w = state;
    free(w->text);
    free(w);
}
// ANCHOR_END: sliced_loader_c

static void sliced_loader(PonkoCtx* ctx) {
    PonkoMemorySource* files = ponko_memory_source_new("levels");
    const char level[] = "#..#\n#..#\n####\n";
    CHECK(ponko_memory_source_insert(files, "walls.txt", (const uint8_t*)level, sizeof level - 1) == PONKO_OK);
    CHECK(ponko_ctx_add_background_memory_source(ctx, files) == PONKO_OK);
    ponko_memory_source_free(files);
    // ANCHOR: sliced_group_c
    PonkoSliceLoader loader = {.struct_size = sizeof loader, .version = PONKO_SLICE_LOADER_VERSION,
                               .start = walls_start, .step = walls_step, .free_state = walls_free};
    PonkoAssetGroup* group = ponko_asset_group_new("level");
    CHECK(ponko_asset_group_add_sliced(group, "walls.txt", &loader) == PONKO_OK);
    uint32_t id;
    CHECK(ponko_ctx_start_asset_group(ctx, group, &id) == PONKO_OK);  // the context owns the group now
    // ... later, once ponko_ctx_asset_group_state says PONKO_GROUP_READY:
    void* state = NULL;
    // ANCHOR_END: sliced_group_c
    CHECK(ponko_ctx_finish_uploads(ctx) == PONKO_OK);
    // ANCHOR: sliced_take_c
    if (ponko_ctx_take_asset_group_sliced(ctx, id, "walls.txt", &state) == PONKO_OK) {
        WallCount* walls = state;  // yours now: free it with your own function
        printf("%zu walls\n", walls->walls);
        walls_free(NULL, walls);
    }
    // ANCHOR_END: sliced_take_c
    CHECK(state != NULL);
}

static void setup(PonkoCtx* ctx, void* user) {
    handles(ctx, user);
    retained_ui(ctx, user);
    versioned_descriptors(ctx);
    sliced_loader(ctx);
    debug_map_camera(ctx);
}

int main(void) {
    abi_check();
    filters();
    loopback_link();
    PonkoGame* game = ponko_game_new("c guide", 1, 1);
    CHECK(game);
    PonkoWorld* world = NULL;
    CHECK(ponko_game_run_headless(game, 1, setup, NULL, &world) == PONKO_OK);
    CHECK(ponko_world_free(world) == PONKO_OK);
    puts("C guide snippets ok");
    return 0;
}
