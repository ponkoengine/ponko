// The C++ halves of the cookbook (book/src/cookbook), next to the Rust recipes
// in tests/cookbook_*.rs. Everything here compiles in CI; main() runs each
// recipe headless for a few ticks and checks that it did its job.
#include "ponko.hpp"
#include <chrono>
#include <cmath>
#include <filesystem>
#include <cstdio>
#include <algorithm>
#include <cstdlib>
#include <optional>
#include <string>
#include <utility>
#include <vector>

using namespace ponko;

static int failures = 0;

static void check(bool ok, const char* what) {
    if (!ok) {
        std::fprintf(stderr, "cookbook: %s\n", what);
        ++failures;
    }
}

static bool near(float a, float b, float within) { return std::fabs(a - b) <= within; }

// A storage folder of the test's own, in the system's temp folder: empty when made, removed when the
// test ends, even if it ends by throwing. Nothing is ever written into the repository.
class temp_folder {
    std::filesystem::path path_;
public:
    explicit temp_folder(const char* name) {
        const auto unique = std::chrono::steady_clock::now().time_since_epoch().count();
        path_ = std::filesystem::temp_directory_path() / ("ponko-cookbook-" + std::to_string(unique) + "-" + name);
        std::error_code ignored;
        std::filesystem::remove_all(path_, ignored);
    }
    ~temp_folder() { std::error_code ignored; std::filesystem::remove_all(path_, ignored); }
    temp_folder(const temp_folder&) = delete;
    temp_folder& operator=(const temp_folder&) = delete;
    std::string str() const { return path_.string(); }
};

// ---- Moving things --------------------------------------------------------------
// ANCHOR: wasd
struct Player {};
// The `move` action, resolved once when it is bound: systems read it by id,
// without looking its name up every tick.
struct Move { ponko::action_id id; };

static void bind_move(ponko::ctx& ctx) {
    ponko::action_map bindings;
    bindings.set("move", ponko::action::axis2({
        ponko::action_binding::keys2(PONKO_KEY_A, PONKO_KEY_D, PONKO_KEY_W, PONKO_KEY_S),
        ponko::action_binding::keys2(PONKO_KEY_LEFT, PONKO_KEY_RIGHT, PONKO_KEY_UP, PONKO_KEY_DOWN)}));
    ctx.set_actions(bindings);
    ctx.world().set_resource(Move{ctx.action_id("move")});
}

// Diagonals must not be faster: a stick or two keys, at most length 1.
static void walk(ponko::ctx& ctx) {
    ponko::vec2 stick = ctx.action_axis2(ctx.world().resource<Move>().id);
    if (ponko::length(stick) > 1) stick = ponko::normalize_or(stick, {0, 0});
    ctx.world().each([&](ponko::transform2d& t, const Player&) { t.position += stick * 200.0f * ctx.dt(); });
}
// ANCHOR_END: wasd

static void wasd_moves_the_player() {
    auto world = ponko::game("wasd", 640, 480)
        .sim_system(walk, "walk")
        .sim_system([](ponko::ctx& ctx) {
            if (ctx.sim_tick() == 0) ctx.feed_input(ponko::input_event::key(PONKO_KEY_D, true));
            if (ctx.sim_tick() == 30) ctx.feed_input(ponko::input_event::key(PONKO_KEY_D, false));
        }, "keys")
        .run_headless(40, [](ponko::ctx& ctx) {
            bind_move(ctx);
            ctx.world().spawn(ponko::transform_at({100, 100}), Player{});
        });
    ponko::vec2 at{};
    world.each([&](const ponko::transform2d& t, const Player&) { at = t.position; });
    check(at.x > 190 && at.x < 215 && at.y == 100, "wasd: the player walks right while D is held, and stops");
}

// ANCHOR: gamepad
static void bind_move_with_a_stick(ponko::ctx& ctx) {
    ponko::action_map bindings;
    bindings.set("move", ponko::action::axis2({
        ponko::action_binding::keys2(PONKO_KEY_A, PONKO_KEY_D, PONKO_KEY_W, PONKO_KEY_S),
        ponko::action_binding::stick({0}, ponko::stick::left, 0.2f)})); // pad 0, 20% dead zone
    ctx.set_actions(bindings);
    ctx.world().set_resource(Move{ctx.action_id("move")});
}
// ANCHOR_END: gamepad

static void a_stick_and_keys_move_the_same_action() {
    ponko::game("gamepad", 640, 480).run_headless(0, [](ponko::ctx& ctx) {
        bind_move_with_a_stick(ctx);
        ctx.feed_input(ponko::input_event::key(PONKO_KEY_W, true));
        check(ctx.action_axis2("move").y == -1, "gamepad: W is up, and y points down");
    });
}

// ANCHOR: stay_on_screen
static void stay_on_screen(ponko::ctx& ctx) {
    const ponko::vec2 window = ctx.window_size();
    ctx.world().each([&](ponko::transform2d& t, const Player&) {
        t.position.x = std::fmin(std::fmax(t.position.x, 16.0f), window.x - 16.0f);
        t.position.y = std::fmin(std::fmax(t.position.y, 16.0f), window.y - 16.0f);
    });
}
// ANCHOR_END: stay_on_screen

static void the_player_cannot_leave_the_window() {
    auto world = ponko::game("bounds", 640, 480)
        .sim_system([](ponko::ctx& ctx) {
            ctx.world().each([](ponko::transform2d& t, const Player&) { t.position += ponko::vec2{50, -50}; });
        }, "run away")
        .sim_system(stay_on_screen, "stay")
        .run_headless(30, [](ponko::ctx& ctx) { ctx.world().spawn(ponko::transform_at({320, 240}), Player{}); });
    ponko::vec2 at{};
    world.each([&](const ponko::transform2d& t, const Player&) { at = t.position; });
    check(at.x == 624 && at.y == 16, "stay_on_screen: stopped in the corner");
}

// ANCHOR: click_to_move
struct Goal { ponko::vec2 at; bool active; };

static void click_to_move(ponko::ctx& ctx) {
    const bool clicked = ctx.mouse_pressed(PONKO_MOUSE_LEFT);
    const ponko::vec2 under_mouse = ctx.mouse_world();
    ctx.world().each([&](ponko::transform2d& t, Goal& goal, const Player&) {
        if (clicked) goal = {under_mouse, true};
        if (!goal.active) return;
        const ponko::vec2 to_goal = goal.at - t.position;
        const float step = 150.0f * ctx.dt();
        if (ponko::length(to_goal) <= step) { t.position = goal.at; goal.active = false; }
        else t.position += ponko::normalize_or(to_goal, {0, 0}) * step;
    });
}
// ANCHOR_END: click_to_move

static void the_player_walks_to_the_clicked_point() {
    auto world = ponko::game("click", 800, 600)
        .sim_system([](ponko::ctx& ctx) {
            if (ctx.sim_tick() == 2) ctx.feed_input(ponko::input_event::mouse(PONKO_MOUSE_LEFT, true));
            if (ctx.sim_tick() == 3) ctx.feed_input(ponko::input_event::mouse(PONKO_MOUSE_LEFT, false));
        }, "mouse")
        .sim_system(click_to_move, "click to move")
        .run_headless(100, [](ponko::ctx& ctx) {
            ctx.feed_input(ponko::input_event::mouse_move({300, 200})); // the camera is at the origin: pixels are world units
            ctx.world().spawn(ponko::transform_at({100, 100}), Goal{{0, 0}, false}, Player{});
        });
    ponko::vec2 at{};
    world.each([&](const ponko::transform2d& t, const Player&) { at = t.position; });
    check(ponko::length(at - ponko::vec2{300, 200}) < 1.0f, "click_to_move: arrived at the point under the pointer");
}

// ANCHOR: face_mouse
static void face_the_mouse(ponko::ctx& ctx) {
    const ponko::vec2 mouse = ctx.mouse_world();
    ctx.world().each([&](ponko::transform2d& t, const Player&) {
        const ponko::vec2 to_mouse = mouse - t.position;
        t.rotation = std::atan2(to_mouse.y, to_mouse.x); // 0 points right; y points down
    });
}
// ANCHOR_END: face_mouse

static void the_player_turns_toward_the_pointer() {
    auto world = ponko::game("face", 800, 600)
        .sim_system(face_the_mouse, "face")
        .run_headless(2, [](ponko::ctx& ctx) { ctx.world().spawn(ponko::transform_at({100, 0}), Player{}); });
    float angle = 9;
    world.each([&](const ponko::transform2d& t, const Player&) { angle = t.rotation; });
    check(near(std::fabs(angle), 3.14159265f, 1e-4f), "face_mouse: the pointer is to the left of the player");
}

// ---- Fighting and collecting ------------------------------------------------------
// ANCHOR: shoot
struct Gun { float cooldown; float ready_at; };
struct Bullet { ponko::vec2 velocity; float dies_at; };

static void shoot(ponko::ctx& ctx) {
    const double now = ctx.sim_time();
    const bool firing = ctx.key_down(PONKO_KEY_SPACE);
    ctx.world().each([&](const ponko::transform2d& t, Gun& gun) {
        if (!firing || now < gun.ready_at) return;
        gun.ready_at = static_cast<float>(now) + gun.cooldown;
        ctx.spawn_later(ponko::transform_at(t.position), Bullet{{400, 0}, static_cast<float>(now) + 1.0f});
    });
}
// ANCHOR_END: shoot

// ANCHOR: bullet_life
static void bullets_fly_and_expire(ponko::ctx& ctx) {
    const double now = ctx.sim_time();
    ctx.world().each([&](ponko::entity e, ponko::transform2d& t, const Bullet& bullet) {
        t.position += bullet.velocity * ctx.dt();
        if (now >= bullet.dies_at) ctx.despawn_later(e);
    });
}
// ANCHOR_END: bullet_life

static void a_held_trigger_fires_at_the_guns_rate_and_bullets_expire() {
    std::size_t most = 0;
    auto world = ponko::game("shoot", 640, 480)
        .sim_system(shoot, "shoot")
        .sim_system(bullets_fly_and_expire, "bullets")
        .sim_system([&](ponko::ctx& ctx) {
            if (ctx.sim_tick() == 0) ctx.feed_input(ponko::input_event::key(PONKO_KEY_SPACE, true));
            if (ctx.sim_tick() == 29) ctx.feed_input(ponko::input_event::key(PONKO_KEY_SPACE, false));
            most = std::max(most, ctx.world().size());
        }, "trigger")
        .run_headless(120, [](ponko::ctx& ctx) { ctx.world().spawn(ponko::transform_at({0, 0}), Gun{0.25f, 0}); });
    check(most >= 3 && most <= 6, "shoot: a few bullets in the air while firing");
    check(world.size() == 1, "shoot: only the gun is left once the bullets have expired");
}

// ANCHOR: pickup
struct Coin {};
struct Coins { int count; };

static void pick_up_coins(ponko::ctx& ctx) {
    ponko::vec2 player_at{};
    ctx.world().each([&](const ponko::transform2d& t, const Player&) { player_at = t.position; });
    int picked = 0;
    ctx.world().each([&](ponko::entity e, const ponko::transform2d& t, const Coin&) {
        if (ponko::length(t.position - player_at) < 24) { ctx.despawn_later(e); ++picked; }
    });
    if (picked > 0) {
        auto coins = ctx.world().resource<Coins>();
        coins.count += picked;
        ctx.world().set_resource(coins);
    }
}
// ANCHOR_END: pickup

static void touching_a_coin_picks_it_up_once() {
    auto world = ponko::game("pickup", 640, 480)
        .sim_system(pick_up_coins, "pickup")
        .run_headless(5, [](ponko::ctx& ctx) {
            ctx.world().set_resource(Coins{0});
            ctx.world().spawn(ponko::transform_at({100, 100}), Player{});
            ctx.world().spawn(ponko::transform_at({110, 100}), Coin{});
            ctx.world().spawn(ponko::transform_at({90, 110}), Coin{});
            ctx.world().spawn(ponko::transform_at({400, 100}), Coin{});
        });
    check(world.resource<Coins>().count == 2, "pickup: two coins were close enough");
    std::size_t left = 0;
    world.each([&](const Coin&) { ++left; });
    check(left == 1, "pickup: the far coin is still there");
}

// ANCHOR: damage
struct Health { int hp; int max; };
struct Damage { ponko::entity target; int amount; };
struct Died { ponko::entity who; };

// Anything that hurts sends a Damage event; one system keeps the books.
static void apply_damage(ponko::ctx& ctx) {
    for (const Damage& hit : ctx.read<Damage>()) {
        auto health = ctx.world().try_get<Health>(hit.target);
        if (!health) continue; // already gone
        Health updated = *health;
        updated.hp = std::max(0, updated.hp - hit.amount);
        ctx.world().set(hit.target, updated);
        if (updated.hp == 0) ctx.send(Died{hit.target});
    }
}

static void remove_the_dead(ponko::ctx& ctx) {
    for (const Died& d : ctx.read<Died>()) ctx.despawn_later(d.who);
}
// ANCHOR_END: damage

static void damage_adds_up_and_the_last_hit_removes_the_victim() {
    ponko::entity victim = 0;
    auto world = ponko::game("damage", 640, 480)
        .sim_system([&](ponko::ctx& ctx) {
            if (ctx.sim_tick() < 3) ctx.send(Damage{victim, 4});
        }, "hits")
        .sim_system(apply_damage, "damage")
        .sim_system(remove_the_dead, "the dead")
        .run_headless(6, [&](ponko::ctx& ctx) { victim = ctx.world().spawn(Health{10, 10}); });
    check(world.size() == 0, "damage: 4 + 4 + 4 is more than 10 hit points");
}

// ANCHOR: chase
struct Enemy { float speed; };

// Walks every enemy straight at the player, and stops at arm's length.
static void chase_the_player(ponko::ctx& ctx) {
    ponko::vec2 player_at{};
    ctx.world().each([&](const ponko::transform2d& t, const Player&) { player_at = t.position; });
    ctx.world().each([&](ponko::transform2d& t, const Enemy& enemy) {
        const ponko::vec2 to_player = player_at - t.position;
        if (ponko::length(to_player) > 20)
            t.position += ponko::normalize_or(to_player, {0, 0}) * enemy.speed * ctx.dt();
    });
}
// ANCHOR_END: chase

static void an_enemy_closes_in_and_stops_at_arms_length() {
    auto world = ponko::game("chase", 640, 480)
        .sim_system(chase_the_player, "chase")
        .run_headless(200, [](ponko::ctx& ctx) {
            ctx.world().spawn(ponko::transform_at({400, 100}), Player{});
            ctx.world().spawn(ponko::transform_at({100, 100}), Enemy{120});
        });
    float distance = 0;
    world.each([&](const ponko::transform2d& t, const Enemy&) { distance = ponko::length(ponko::vec2{400, 100} - t.position); });
    check(distance > 15 && distance <= 20.1f, "chase: stopped near 20 pixels away");
}

// ANCHOR: loot
struct Drop { float chance; };

// When something dies, it may leave a coin. The luck comes from the game's seed.
static void drop_loot(ponko::ctx& ctx) {
    ponko::rng luck = ctx.random().stream(ctx.sim_tick());
    for (const Died& d : ctx.read<Died>()) {
        const auto drop = ctx.world().try_get<Drop>(d.who);
        const auto where = ctx.world().try_get<ponko::transform2d>(d.who);
        if (drop && where && luck.chance(drop->chance)) ctx.spawn_later(ponko::transform_at(where->position), Coin{});
    }
}
// ANCHOR_END: loot

static void a_sure_drop_always_leaves_a_coin_and_no_chance_never_does() {
    auto run = [](float chance) {
        auto world = ponko::game("loot", 640, 480)
            .sim_system([](ponko::ctx& ctx) {
                if (ctx.sim_tick() == 1) {
                    std::vector<ponko::entity> dead;
                    ctx.world().each([&](ponko::entity e, const Drop&) { dead.push_back(e); });
                    for (ponko::entity e : dead) ctx.send(Died{e});
                }
            }, "deaths")
            .sim_system(drop_loot, "loot")
            .run_headless(4, [chance](ponko::ctx& ctx) {
                for (int i = 0; i < 20; ++i) ctx.world().spawn(ponko::transform_at({float(i), 0}), Drop{chance});
            });
        std::size_t coins = 0;
        world.each([&](const Coin&) { ++coins; });
        return coins;
    };
    check(run(1.0f) == 20, "loot: chance 1 drops a coin every time");
    check(run(0.0f) == 0, "loot: chance 0 never does");
}

// ---- Time ---------------------------------------------------------------------------
// ANCHOR: fuse
struct Fuse { float explodes_at; };
struct Boom { ponko::vec2 at; };

static void burn_fuses(ponko::ctx& ctx) {
    const double now = ctx.sim_time();
    ctx.world().each([&](ponko::entity e, const ponko::transform2d& t, const Fuse& fuse) {
        if (now < fuse.explodes_at) return;
        ctx.send(Boom{t.position});
        ctx.despawn_later(e);
    });
}
// ANCHOR_END: fuse

static void a_fuse_goes_off_once_after_its_time() {
    int booms = 0;
    ponko::game("fuse", 640, 480)
        .sim_system(burn_fuses, "fuses")
        .sim_system([&](ponko::ctx& ctx) { booms += static_cast<int>(ctx.read<Boom>().size()); }, "count")
        .run_headless(100, [](ponko::ctx& ctx) {
            ctx.world().spawn(ponko::transform_at({5, 5}), Fuse{0.5f}); // half a second, 25 ticks
        });
    check(booms == 1, "fuse: one explosion");
}

// ANCHOR: repeat
static void spawn_a_coin(ponko::ctx& ctx) {
    ctx.spawn_later(ponko::transform_at({0, 0}), Coin{});
}

static void add_the_spawner(ponko::game& game) {
    // The last number is a period in ticks: this one runs every 50 ticks, once a second.
    game.sim_system(spawn_a_coin, "coins", 50);
}
// ANCHOR_END: repeat

static void a_system_with_a_period_runs_at_that_rate() {
    ponko::game game("repeat", 640, 480);
    add_the_spawner(game);
    auto world = game.run_headless(200);
    std::size_t coins = 0;
    world.each([&](const Coin&) { ++coins; });
    check(coins == 4, "repeat: four runs in 200 ticks");
}

// ANCHOR: slow_motion
static void slow_motion_while_held(ponko::ctx& ctx) {
    // A quarter speed runs a quarter as many ticks a second; each tick is as
    // long as ever, so the simulation gives the same results, only later.
    ctx.set_sim_speed(ctx.key_down(PONKO_KEY_TAB) ? 0.25f : 1.0f);
}
// ANCHOR_END: slow_motion

static void holding_tab_drops_the_speed_to_a_quarter_and_letting_go_restores_it() {
    std::vector<float> speeds;
    ponko::input_scenario keys;
    keys.at(2, ponko::input_event::key(PONKO_KEY_TAB, true)).at(4, ponko::input_event::key(PONKO_KEY_TAB, false));
    ponko::game("slow", 640, 480)
        .sim_system(slow_motion_while_held, "slow motion")
        .sim_system([&](ponko::ctx& ctx) { speeds.push_back(ctx.sim_speed()); }, "watch")
        .input_scenario(keys)
        .run_headless(6);
    check(speeds == std::vector<float>({1, 1, 0.25f, 0.25f, 1, 1}), "slow_motion: a quarter while Tab is held");
}

// ---- Cameras -------------------------------------------------------------------------
// ANCHOR: follow
static ponko::follow_camera2d make_follow_camera() {
    ponko::follow_camera2d follow; // keep it: it holds the camera's state
    follow.dead_zone = {32, 48};              // the player can move this much without scrolling
    follow.look_ahead = {48, 0};              // show more of where they are heading
    follow.set_bounds({{0, 0}, {4000, 720}}); // never show past the level
    return follow;
}

// A frame system, after the player moved.
static void follow_the_player(ponko::ctx& ctx, ponko::follow_camera2d& follow) {
    ponko::vec2 hero{};
    ctx.world().each([&](const ponko::transform2d& t, const Player&) { hero = t.position; });
    follow.update(ctx, hero, ctx.dt());
}
// ANCHOR_END: follow

static void the_camera_stops_at_the_level_edge() {
    ponko::game("follow", 800, 600).run_headless(1, [](ponko::ctx& ctx) {
        ponko::follow_camera2d follow = make_follow_camera();
        ctx.world().spawn(ponko::transform_at({10, 360}), Player{});
        for (int i = 0; i < 120; ++i) follow_the_player(ctx, follow);
        check(near(ctx.camera().position.x, 0, 1.0f), "follow: the level starts at 0, so the camera's left edge stops there");
    });
}

// ANCHOR: shake
struct Hit {};

static void shake_on_hits(ponko::ctx& ctx, ponko::camera_shake& shake) {
    const std::size_t hits = ctx.read<Hit>().size();
    if (hits > 0) shake.add_trauma(0.5f * static_cast<float>(hits)); // from 0 to 1, wears off by itself
    shake.update(ctx.dt());
    shake.apply2d(ctx); // after the camera was placed for this frame
}
// ANCHOR_END: shake

static void a_hit_moves_the_camera() {
    ponko::camera_shake shake;
    float worst = 0;
    ponko::game("shake", 800, 600)
        .sim_system([](ponko::ctx& ctx) {
            if (ctx.sim_tick() == 3) ctx.send(Hit{});
        }, "hit")
        .sim_system([&](ponko::ctx& ctx) {
            shake_on_hits(ctx, shake);
            worst = std::fmax(worst, std::fabs(ctx.camera().position.x) + std::fabs(ctx.camera().position.y));
        }, "shake")
        .run_headless(20);
    check(worst > 0.1f, "shake: the camera moved");
}

// ANCHOR: pixel_perfect
static void keep_pixels_square(ponko::ctx& ctx, const ponko::pixel_perfect& pixels) {
    pixels.apply(ctx); // after anything else that moves the camera: a whole zoom, whole pixels
}
// ANCHOR_END: pixel_perfect

static void pixel_art_zooms_by_whole_numbers() {
    ponko::game("pixels", 1280, 720).run_headless(1, [](ponko::ctx& ctx) {
        ponko::pixel_perfect pixels(320, 180); // art drawn for 320 x 180
        keep_pixels_square(ctx, pixels);
        check(ctx.camera().zoom == 4.0f, "pixel_perfect: exactly 4 window pixels per art pixel");
    });
}

// ANCHOR: zoom
// Zooms in (wheel up) or out around the pixel under the cursor, so that
// pixel stays under the cursor.
static void zoom_at_cursor(ponko::ctx& ctx, float wheel) {
    if (wheel != 0) ctx.camera_zoom_at(ctx.mouse_pos(), std::pow(1.1f, wheel)); // 10% per notch
}

static void zoom_with_the_wheel(ponko::ctx& ctx) { zoom_at_cursor(ctx, ctx.mouse_wheel().y); }
// ANCHOR_END: zoom

static void zooming_keeps_the_world_point_under_the_cursor_where_it_was() {
    ponko::game("zoom", 800, 600).run_headless(0, [](ponko::ctx& ctx) {
        ctx.set_camera({{120, 80}, 1});
        ctx.feed_input(ponko::input_event::mouse_move({600, 150})); // the test plays the mouse
        const ponko::vec2 before = ctx.screen_to_world(ctx.mouse_pos());
        zoom_at_cursor(ctx, 3);
        const ponko::vec2 after = ctx.screen_to_world(ctx.mouse_pos());
        check(near(ctx.camera().zoom, std::pow(1.1f, 3.0f), 1e-4f), "zoom: three notches in");
        check(ponko::length(after - before) < 1e-3f, "zoom: the same world point under the cursor");
        zoom_at_cursor(ctx, 0);
        check(near(ctx.camera().zoom, std::pow(1.1f, 3.0f), 1e-4f), "zoom: no wheel, no zoom");
    });
}

static void two_notches_of_the_wheel_zoom_in_around_the_pointer() {
    std::vector<std::pair<float, float>> seen; // (zoom, world x under the pointer)
    ponko::input_scenario pointer;
    pointer.at(0, ponko::input_event::mouse_move({200, 450})).at(2, ponko::input_event::wheel({0, 2}));
    ponko::game("zoom", 800, 600)
        .sim_system(zoom_with_the_wheel, "zoom")
        .sim_system([&](ponko::ctx& ctx) { seen.emplace_back(ctx.camera().zoom, ctx.screen_to_world(ctx.mouse_pos()).x); }, "watch")
        .input_scenario(pointer)
        .run_headless(4);
    check(seen.size() == 4 && seen[1] == std::make_pair(1.0f, 200.0f), "zoom: no wheel yet");
    check(seen.size() == 4 && near(seen[2].first, 1.21f, 1e-4f), "zoom: two notches of 10%");
    check(seen.size() == 4 && near(seen[3].second, 200, 1e-3f) && seen[3].first == seen[2].first,
          "zoom: the same world point under the pointer, and the wheel turned in one frame only");
}

// ---- Bodies and collisions -------------------------------------------------------------
// ANCHOR: contact_damage
struct Hazard { int damage; };

// Physics reports each pair once, when they begin to touch.
static void hazards_hurt(ponko::ctx& ctx) {
    for (const ponko::collision& contact : ctx.collisions_started()) {
        const ponko::entity pair[2][2] = {{ponko::bits(contact.a), ponko::bits(contact.b)},
                                          {ponko::bits(contact.b), ponko::bits(contact.a)}};
        for (const auto& [hazard, victim] : pair) {
            const auto h = ctx.world().try_get<Hazard>(hazard);
            const auto health = ctx.world().try_get<Health>(victim);
            if (!h || !health) continue;
            ctx.world().set(victim, Health{health->hp - h->damage, health->max});
            ctx.despawn_later(hazard); // a spent hazard
        }
    }
}
// ANCHOR_END: contact_damage

static void a_hazard_that_touches_the_player_costs_one_hit() {
    auto world = ponko::game("hazard", 640, 480)
        .physics({0, 0})
        .sim_system(hazards_hurt, "hurt")
        .run_headless(60, [](ponko::ctx& ctx) {
            auto w = ctx.world();
            w.spawn(Player{}, Health{3, 3}, ponko::transform_at({200, 100}), ponko::dynamic_body(), ponko::circle_collider(10));
            ponko::velocity flying{};
            flying.linear = {300, 0};
            w.spawn(Hazard{2}, ponko::transform_at({100, 100}), ponko::dynamic_body(), ponko::circle_collider(5), flying);
        });
    int hp = 0;
    std::size_t hazards = 0;
    world.each([&](const Health& h, const Player&) { hp = h.hp; });
    world.each([&](const Hazard&) { ++hazards; });
    check(hp == 1 && hazards == 0, "contact_damage: 3 - 2 hit points, and the hazard is gone");
}

// ANCHOR: knockback
// Pushes `victim` away from `from`, as if hit hard enough to fly at `speed` pixels a second.
static void knock_back(ponko::ctx& ctx, ponko::entity victim, ponko::vec2 from, float speed) {
    const auto t = ctx.world().try_get<ponko::transform2d>(victim);
    if (!t) return;
    const ponko::vec2 away = ponko::normalize_or(t->position - from, {1, 0});
    if (auto mass = ctx.mass(victim)) ctx.apply_impulse(victim, away * *mass * speed); // impulse = mass x speed
}
// ANCHOR_END: knockback

static void knockback_sends_the_body_away_from_the_blow() {
    ponko::entity body = 0;
    auto world = ponko::game("knockback", 640, 480)
        .physics({0, 0})
        .sim_system([&](ponko::ctx& ctx) {
            if (ctx.sim_tick() == 1) knock_back(ctx, body, {50, 100}, 300);
        }, "blow")
        .run_headless(32, [&](ponko::ctx& ctx) {
            body = ctx.world().spawn(ponko::transform_at({100, 100}), ponko::dynamic_body(), ponko::rect_collider({20, 20}));
        });
    const auto at = world.get<ponko::transform2d>(body).position;
    check(near(at.x, 250, 6) && near(at.y, 100, 0.01f), "knockback: flew right at about 300 pixels a second");
}

// ANCHOR: line_of_sight
static bool can_see(ponko::ctx& ctx, ponko::entity looker, ponko::entity target) {
    const auto from = ctx.world().try_get<ponko::transform2d>(looker);
    const auto to = ctx.world().try_get<ponko::transform2d>(target);
    if (!from || !to) return false;
    const ponko::vec2 way = to->position - from->position;
    // The nearest thing along the line, not counting the looker itself.
    auto hit = ctx.cast_ray(from->position, way, ponko::length(way), ponko::filter2d().exclude(looker));
    return hit && ponko::bits(hit->entity) == target; // a wall in between would be hit first
}
// ANCHOR_END: line_of_sight

static void a_wall_blocks_the_view() {
    for (bool with_wall : {true, false}) {
        bool sees = false;
        ponko::entity guard = 0, thief = 0;
        ponko::game("sight", 640, 480)
            .physics({0, 0})
            .sim_system([&](ponko::ctx& ctx) {
                if (ctx.sim_tick() == 2) sees = can_see(ctx, guard, thief);
            }, "look")
            .run_headless(4, [&](ponko::ctx& ctx) {
                auto w = ctx.world();
                guard = w.spawn(ponko::transform_at({50, 100}), ponko::fixed_body(), ponko::circle_collider(8));
                thief = w.spawn(ponko::transform_at({350, 100}), ponko::fixed_body(), ponko::circle_collider(8));
                if (with_wall) w.spawn(ponko::transform_at({200, 100}), ponko::fixed_body(), ponko::rect_collider({10, 80}));
            });
        check(sees == !with_wall, "line_of_sight: a wall blocks the view, no wall does not");
    }
}

// ---- Levels and objects ------------------------------------------------------------------
// ANCHOR: load_level
struct Hero {};

// Builds the level from a Tiled map and turns its objects into game entities:
// the "Spawn" object is where the hero starts, each "Coin" object becomes a coin.
static ponko::entity load_level(ponko::ctx& ctx) {
    const ponko::entity level = ponko::spawn_level(ctx, "tutorial_2d/cove.tmx", {0, 0});
    for (ponko::entity e : ponko::level_entities(ctx, level)) {
        if (!ctx.world().try_get<ponko::transform2d>(e)) continue;
        try {
            const std::string kind = ponko::level_object_class(ctx, e);
            const ponko::vec2 at = ponko::level_object_of(ctx, e).position;
            if (kind == "Spawn") ctx.world().spawn(Hero{}, ponko::transform_at(at));
            if (kind == "Coin") ctx.world().spawn(Coin{}, ponko::transform_at(at));
        } catch (const ponko::error&) {} // a layer or a collider, not an object
    }
    return level; // keep it: ponko::despawn_level(ctx, level) removes it all again
}
// ANCHOR_END: load_level

static void the_level_places_the_hero_and_the_coins() {
    auto world = ponko::game("level", 800, 600).run_headless(1, [](ponko::ctx& ctx) { load_level(ctx); });
    std::size_t heroes = 0, coins = 0;
    ponko::vec2 hero_at{};
    world.each([&](const Hero&, const ponko::transform2d& t) { ++heroes; hero_at = t.position; });
    world.each([&](const Coin&) { ++coins; });
    check(heroes == 1 && hero_at.x == 40 && hero_at.y == 180, "load_level: the hero is at the Spawn object");
    check(coins == 12, "load_level: one coin for each Coin object");
}

// ANCHOR: attach
struct Sword {};

static ponko::entity give_a_sword(ponko::ctx& ctx, ponko::entity hero) {
    // The sword hangs 14 pixels to the hero's right, and follows wherever the hero goes.
    return ctx.world().spawn(Sword{}, ponko::transform_at({0, 0}), ponko::attach_to(hero, {14, 0}));
}

static void add_attachments(ponko::game& game) {
    game.follow_attachments(); // moves everything attached to where its parent is now: after the systems that move things
}
// ANCHOR_END: attach

static void the_sword_follows_the_hero() {
    ponko::entity sword = 0;
    ponko::game game("sword", 800, 600);
    game.sim_system([](ponko::ctx& ctx) {
        if (ctx.sim_tick() < 30) ctx.world().each([](ponko::transform2d& t, const Hero&) { t.position.x += 1; });
    }, "walk");
    add_attachments(game);
    auto world = game.run_headless(30, [&](ponko::ctx& ctx) {
        const auto hero = ctx.world().spawn(Hero{}, ponko::transform_at({100, 100}));
        sword = give_a_sword(ctx, hero);
    });
    const auto at = world.get<ponko::transform2d>(sword).position;
    check(at.x == 144 && at.y == 100, "attach: 14 pixels to the right of the hero");
}

// ---- Sound and saves ------------------------------------------------------------------------
// ANCHOR: save_game
struct Lives { int count; };

static void register_saved_components(ponko::ctx& ctx) {
    ctx.world().reflect<Lives>({ponko::field("count", &Lives::count)}); // only reflected components go in a save
}

// F5 saves, F9 loads. A slot is a name: "quick", "slot 1", "autosave"...
static void quick_save_and_load(ponko::ctx& ctx) {
    if (ctx.key_pressed(PONKO_KEY_F5)) ctx.save_world("quick");
    if (ctx.key_pressed(PONKO_KEY_F9)) ctx.load_world("quick");
}
// ANCHOR_END: save_game

static void f5_saves_and_f9_restores() {
    const temp_folder dir("saves");
    auto world = ponko::game("saves", 640, 480)
        .data_dir(dir.str().c_str())
        .sim_system([](ponko::ctx& ctx) {
            const auto tap = [&](ponko::key k, std::uint64_t tick) {
                if (ctx.sim_tick() == tick) ctx.feed_input(ponko::input_event::key(k, true));
                if (ctx.sim_tick() == tick + 1) ctx.feed_input(ponko::input_event::key(k, false));
            };
            tap(PONKO_KEY_F5, 3);
            tap(PONKO_KEY_F9, 8);
            if (ctx.sim_tick() == 5) ctx.world().each([](Lives& l) { l.count = 1; }); // hurt after saving
        }, "script")
        .sim_system(quick_save_and_load, "quick")
        .run_headless(12, [](ponko::ctx& ctx) {
            register_saved_components(ctx);
            ctx.world().spawn(Lives{7});
        });
    int lives = 0;
    world.each([&](const Lives& l) { lives = l.count; });
    check(lives == 7, "save_game: F9 brought the saved world back");
}

// ANCHOR: settings
// What the player chose: a plain file of their own. An absent file is a first run.
static float load_volume(ponko::ctx& ctx) {
    auto text = ctx.storage_read_text("volume.txt");
    return text ? std::stof(*text) : 0.8f;
}

// The write is safe: a crash can't leave half a file.
static void finish_round(ponko::ctx& ctx, float volume) {
    ctx.storage_write("volume.txt", std::to_string(volume));
}
// ANCHOR_END: settings

static void a_setting_is_kept_between_runs() {
    const temp_folder dir("settings");
    ponko::game("settings", 1, 1).data_dir(dir.str().c_str()).run_headless(0, [](ponko::ctx& ctx) {
        check(load_volume(ctx) == 0.8f, "settings: the default on a first run");
        finish_round(ctx, 0.25f);
    });
    ponko::game("settings", 1, 1).data_dir(dir.str().c_str()).run_headless(0, [](ponko::ctx& ctx) {
        check(near(load_volume(ctx), 0.25f, 1e-6f), "settings: the choice is still there on the next run");
    });
}

// ANCHOR: save_on_quit
// The menu's "Save and quit": saves first, and quits only after that worked.
static void save_and_quit(ponko::ctx& ctx) {
    if (!ctx.key_pressed(PONKO_KEY_Q)) return;
    try {
        ctx.save_world("autosave");
        ctx.quit();
    } catch (const ponko::error& e) {
        std::fprintf(stderr, "could not save, staying in the game: %s\n", e.what());
    }
}
// ANCHOR_END: save_on_quit

static void quitting_saves_first_and_stops_the_game() {
    std::vector<std::pair<std::uint64_t, bool>> seen; // (tick, quit asked)
    const temp_folder dir("quit");
    ponko::game("quit", 640, 480)
        .data_dir(dir.str().c_str())
        .sim_system([](ponko::ctx& ctx) {
            if (ctx.sim_tick() == 4) ctx.feed_input(ponko::input_event::key(PONKO_KEY_Q, true));
        }, "press Q")
        .sim_system(save_and_quit, "quit")
        .sim_system([&](ponko::ctx& ctx) { seen.emplace_back(ctx.sim_tick(), ctx.quit_requested()); }, "watch")
        .run_headless(50, [](ponko::ctx& ctx) {
            register_saved_components(ctx);
            ctx.world().spawn(Lives{3});
        });
    check(!seen.empty() && seen.back() == std::make_pair(std::uint64_t{4}, true),
          "save_on_quit: Q on tick 4 asked to quit, and no tick ran after it");
    check(std::filesystem::is_regular_file(std::filesystem::path(dir.str()) / "saves" / "autosave.json"),
          "save_on_quit: the save was written first");
}

// ---- More moving, fighting and timing ---------------------------------------------------
// ANCHOR: jump
struct Body { float vy; bool on_ground; };

static const float gravity = 1200.0f; // pixels per second squared; y points down
static const float floor_y = 400.0f;

static void jump_and_fall(ponko::ctx& ctx) {
    const float dt = ctx.dt();
    const bool jump = ctx.key_pressed(PONKO_KEY_SPACE);
    ctx.world().each([&](Body& body, ponko::transform2d& t) {
        if (jump && body.on_ground) body.vy = -500.0f; // up
        body.vy += gravity * dt;
        t.position.y += body.vy * dt;
        body.on_ground = t.position.y >= floor_y;
        if (body.on_ground) { t.position.y = floor_y; body.vy = 0; }
    });
}
// ANCHOR_END: jump

static void a_jump_leaves_the_floor_once_and_lands() {
    float highest = floor_y;
    auto world = ponko::game("jump", 640, 480)
        .sim_system([](ponko::ctx& ctx) {
            if (ctx.sim_tick() == 5) ctx.feed_input(ponko::input_event::key(PONKO_KEY_SPACE, true));
            if (ctx.sim_tick() == 6) ctx.feed_input(ponko::input_event::key(PONKO_KEY_SPACE, false));
        }, "keys")
        .sim_system(jump_and_fall, "jump")
        .sim_system([&](ponko::ctx& ctx) {
            ctx.world().each([&](const Body&, const ponko::transform2d& t) { highest = std::fmin(highest, t.position.y); });
        }, "watch")
        .run_headless(100, [](ponko::ctx& ctx) { ctx.world().spawn(Body{0, true}, ponko::transform_at({100, floor_y})); });
    float y = 0;
    world.each([&](const Body&, const ponko::transform2d& t) { y = t.position.y; });
    check(highest < floor_y - 50 && y == floor_y, "jump: went up by more than 50 pixels and landed again");
}

// ANCHOR: dash
struct Dash { double ready_at; }; // simulation time, in seconds

static void dash(ponko::ctx& ctx) {
    if (!ctx.key_pressed(PONKO_KEY_LEFT_SHIFT)) return;
    const double now = ctx.sim_time();
    const ponko::vec2 aim = ponko::normalize_or(ctx.action_axis2(ctx.world().resource<Move>().id), {0, 0});
    ctx.world().each([&](Dash& d, ponko::transform2d& t) {
        if (now < d.ready_at) return;
        t.position += aim * 120.0f;
        d.ready_at = now + 1.5; // a cooldown of a second and a half
    });
}
// ANCHOR_END: dash

static void a_dash_is_ready_again_after_its_cooldown() {
    auto world = ponko::game("dash", 640, 480)
        .sim_system([](ponko::ctx& ctx) {
            const std::uint64_t t = ctx.sim_tick();
            // Shift tapped at ticks 2, 10 (inside the cooldown) and 100 (after it).
            for (std::uint64_t at : {2, 10, 100}) {
                if (t == at) ctx.feed_input(ponko::input_event::key(PONKO_KEY_LEFT_SHIFT, true));
                if (t == at + 1) ctx.feed_input(ponko::input_event::key(PONKO_KEY_LEFT_SHIFT, false));
            }
            if (t == 0) ctx.feed_input(ponko::input_event::key(PONKO_KEY_D, true));
        }, "keys")
        .sim_system(dash, "dash")
        .run_headless(110, [](ponko::ctx& ctx) {
            bind_move(ctx);
            ctx.world().spawn(ponko::transform_at({0, 0}), Dash{0});
        });
    float x = 0;
    world.each([&](const Dash&, const ponko::transform2d& t) { x = t.position.x; });
    check(x == 240.0f, "dash: two dashes of 120 pixels; the one inside the cooldown did nothing");
}

// ANCHOR: rebind
struct Rebinding { bool open; }; // true while the "press a key for jump" prompt is open

static void rebind_jump(ponko::ctx& ctx) {
    if (!ctx.world().resource<Rebinding>().open) return;
    for (const ponko::pressed_input& pressed : ctx.pressed_this_frame()) { // the first thing pressed
        if (pressed.is_gamepad_axis()) continue; // an axis binds with action::axis
        auto bindings = ctx.actions();
        bindings.set("jump", ponko::action::button({pressed.binding()}));
        ctx.set_actions(bindings);
        ctx.world().set_resource(Rebinding{false});
        return;
    }
}
// ANCHOR_END: rebind

static void the_next_key_pressed_becomes_the_jump_key() {
    ponko::game("rebind", 640, 480).run_headless(0, [](ponko::ctx& ctx) {
        ponko::action_map bindings;
        bindings.set("jump", ponko::action::button({ponko::action_binding::key(PONKO_KEY_SPACE)}));
        ctx.set_actions(bindings);
        ctx.world().set_resource(Rebinding{true});
        ctx.feed_input(ponko::input_event::key(PONKO_KEY_J, true));
        rebind_jump(ctx);
        check(!ctx.world().resource<Rebinding>().open, "rebind: the prompt closed");
        ctx.feed_input(ponko::input_event::key(PONKO_KEY_J, false));
        ctx.feed_input(ponko::input_event::key(PONKO_KEY_SPACE, true));
        check(!ctx.action_pressed("jump"), "rebind: Space no longer jumps");
        ctx.feed_input(ponko::input_event::key(PONKO_KEY_SPACE, false));
        ctx.feed_input(ponko::input_event::key(PONKO_KEY_J, true));
        check(ctx.action_pressed("jump"), "rebind: J does");
    });
}

// ANCHOR: waves
struct Wave { std::uint32_t number; };

// When the field is clear, the next wave comes: one more enemy each time.
static void spawn_waves(ponko::ctx& ctx) {
    std::size_t enemies = 0;
    ctx.world().each([&](const Enemy&) { ++enemies; });
    if (enemies > 0) return;
    const std::uint32_t number = ctx.world().resource<Wave>().number + 1;
    ctx.world().set_resource(Wave{number});
    const ponko::vec2 centre = ctx.window_size() / 2.0f;
    ponko::rng random = ctx.random().stream(number); // the same waves in every run
    for (std::uint32_t i = 0; i < number + 2; ++i)
        ctx.spawn_later(ponko::transform_at(centre + random.unit_vec2() * 300.0f), Enemy{60});
}
// ANCHOR_END: waves

static void each_wave_has_one_more_enemy_than_the_last() {
    std::vector<std::size_t> sizes;
    ponko::game("waves", 640, 480)
        .sim_system(spawn_waves, "waves")
        .sim_system([&](ponko::ctx& ctx) {
            std::size_t n = 0;
            ctx.world().each([&](const Enemy&) { ++n; });
            if (!sizes.empty() && n == 0) return;
            if (sizes.size() < ctx.world().resource<Wave>().number) sizes.push_back(n);
            // Defeat everyone on tick 5 and 10.
            if (ctx.sim_tick() == 5 || ctx.sim_tick() == 10) {
                std::vector<ponko::entity> all;
                ctx.world().each([&](ponko::entity e, const Enemy&) { all.push_back(e); });
                for (ponko::entity e : all) ctx.despawn_later(e);
            }
        }, "defeat")
        .run_headless(14, [](ponko::ctx& ctx) { ctx.world().set_resource(Wave{0}); });
    check(sizes.size() == 3 && sizes[0] == 3 && sizes[1] == 4 && sizes[2] == 5, "waves: 3, then 4, then 5 enemies");
}

// ANCHOR: patrol
// What a guard is doing: the state is a field, and the AI is one switch on it.
struct Guard { bool chasing; ponko::vec2 a, b; bool going_to_b; };

static void guard_ai(ponko::ctx& ctx) {
    ponko::vec2 player{};
    ctx.world().each([&](const ponko::transform2d& t, const Player&) { player = t.position; });
    const float step = 60.0f * ctx.dt();
    ctx.world().each([&](Guard& g, ponko::transform2d& t) {
        const bool near_player = ponko::length(player - t.position) < 100;
        if (!g.chasing && near_player) g.chasing = true;
        else if (g.chasing && !near_player) g = {false, t.position, t.position + ponko::vec2{150, 0}, true};
        if (g.chasing) { t.position += ponko::normalize_or(player - t.position, {0, 0}) * step * 1.5f; return; }
        const ponko::vec2 goal = g.going_to_b ? g.b : g.a;
        if (ponko::length(goal - t.position) <= step) g.going_to_b = !g.going_to_b; // turn round at the end
        t.position += ponko::normalize_or(goal - t.position, {0, 0}) * step;
    });
}
// ANCHOR_END: patrol

static void a_guard_patrols_until_the_player_comes_near() {
    bool chased = false;
    float far_end = 0;
    ponko::game("patrol", 800, 600)
        .sim_system(guard_ai, "guard")
        .sim_system([&](ponko::ctx& ctx) {
            ctx.world().each([&](const Guard& g, const ponko::transform2d& t) {
                if (ctx.sim_tick() < 150) far_end = std::fmax(far_end, t.position.x);
                if (g.chasing) chased = true;
            });
            if (ctx.sim_tick() == 160) ctx.world().each([](ponko::transform2d& t, const Player&) { t.position = {200, 0}; });
        }, "watch")
        .run_headless(180, [](ponko::ctx& ctx) {
            ctx.world().spawn(ponko::transform_at({1000, 1000}), Player{});
            ctx.world().spawn(ponko::transform_at({100, 0}), Guard{false, {100, 0}, {250, 0}, true});
        });
    check(far_end > 240 && far_end < 251, "patrol: walked to the far end of its route");
    check(chased, "patrol: gave chase when the player came near");
}

// ANCHOR: tween
struct Slide { ponko::vec2 from, to; float seconds, elapsed; };

// 0 to 1, fast at first and gentle at the end. Swap in your favourite curve.
static float ease_out_cubic(float t) { return 1.0f - std::pow(1.0f - t, 3.0f); }

static void slide(ponko::ctx& ctx) {
    const float dt = ctx.dt();
    ctx.world().each([&](Slide& s, ponko::transform2d& t) {
        s.elapsed = std::fmin(s.elapsed + dt, s.seconds);
        const float progress = ease_out_cubic(s.elapsed / s.seconds);
        t.position = s.from + (s.to - s.from) * progress;
    });
}
// ANCHOR_END: tween

static void a_slide_arrives_exactly() {
    auto world = ponko::game("slide", 640, 480)
        .sim_system(slide, "slide")
        .run_headless(100, [](ponko::ctx& ctx) {
            ctx.world().spawn(ponko::transform_at({0, 0}), Slide{{0, 0}, {300, 100}, 1.0f, 0});
        });
    ponko::vec2 at{};
    world.each([&](const Slide&, const ponko::transform2d& t) { at = t.position; });
    check(at.x == 300 && at.y == 100, "tween: ended on the target");
}

// ANCHOR: day_night
static const double day_seconds = 120.0;

// How much light there is: 0 (midnight) to 1 (noon).
static float sunlight(double clock) {
    const double time_of_day = std::fmod(clock, day_seconds) / day_seconds; // 0 is midnight, 0.5 is noon
    return static_cast<float>(0.5 - 0.5 * std::cos(time_of_day * 6.283185307179586));
}

// The colour to tint the world with: deep blue at night, white at noon.
static ponko::color tint_at(double clock) {
    const float light = sunlight(clock);
    const auto mix = [&](float night, float day) { return night + (day - night) * light; };
    return ponko::rgb(mix(0.1f, 1.0f), mix(0.1f, 1.0f), mix(0.3f, 1.0f));
}
// ANCHOR_END: day_night

static void midnight_is_dark_and_noon_is_bright() {
    check(sunlight(0) < 0.001f && sunlight(60) > 0.999f && near(sunlight(120), 0, 0.001f), "day_night: dark, bright, dark");
    check(tint_at(60).r > 0.99f && tint_at(0).b < 0.31f, "day_night: white at noon, blue at midnight");
}

// ---- A few more bodies, shapes and sounds -----------------------------------------------------
// ANCHOR: click_body
struct Selected {};

// Click a body to select it (the topmost one if several overlap).
static void click_to_select(ponko::ctx& ctx) {
    if (!ctx.mouse_pressed(PONKO_MOUSE_LEFT)) return;
    std::vector<PonkoEntity> under;
    ctx.overlap_point(ctx.mouse_world(), under);
    std::vector<ponko::entity> old;
    ctx.world().each([&](ponko::entity e, const Selected&) { old.push_back(e); });
    for (ponko::entity e : old) ctx.remove_later<Selected>(e);
    if (!under.empty()) ctx.set_later(ponko::bits(under.front()), Selected{});
}
// ANCHOR_END: click_body

static void clicking_a_body_selects_it() {
    auto world = ponko::game("select", 640, 480)
        .physics({0, 0})
        .sim_system([](ponko::ctx& ctx) {
            if (ctx.sim_tick() == 3) ctx.feed_input(ponko::input_event::mouse(PONKO_MOUSE_LEFT, true));
            if (ctx.sim_tick() == 4) ctx.feed_input(ponko::input_event::mouse(PONKO_MOUSE_LEFT, false));
        }, "mouse")
        .sim_system(click_to_select, "select")
        .run_headless(8, [](ponko::ctx& ctx) {
            ctx.feed_input(ponko::input_event::mouse_move({105, 95})); // the camera is at the origin: pixels are world units
            ctx.world().spawn(ponko::transform_at({100, 100}), ponko::fixed_body(), ponko::circle_collider(20));
            ctx.world().spawn(ponko::transform_at({300, 100}), ponko::fixed_body(), ponko::circle_collider(20));
        });
    std::size_t selected = 0;
    ponko::vec2 at{};
    world.each([&](const Selected&, const ponko::transform2d& t) { ++selected; at = t.position; });
    check(selected == 1 && at.x == 100, "click_body: the body under the pointer is selected");
}

// ANCHOR: debug_draw
struct Watcher { bool sees; ponko::vec2 target; };

// Shapes last until this system runs again. They only show in debug builds
// of the engine, so there is nothing to remove for the release.
static void show_what_the_ai_thinks(ponko::ctx& ctx) {
    auto debug = ctx.debug();
    ctx.world().each([&](const ponko::transform2d& t, const Watcher& w) {
        debug.circle(t.position, 120.0f, ponko::colors::yellow); // how far it can see
        if (w.sees) debug.arrow(t.position, w.target, ponko::colors::red);
        else debug.text(t.position, "idle", ponko::colors::white);
    });
}
// ANCHOR_END: debug_draw

static void the_ai_draws_its_reach_its_target_and_its_mood() {
    ponko::game("debug", 800, 600).run_headless(0, [](ponko::ctx& ctx) {
        ctx.debug().set_enabled(true);
        ctx.world().spawn(ponko::transform_at({100, 100}), Watcher{true, {200, 100}});
        ctx.world().spawn(ponko::transform_at({400, 100}), Watcher{false, {0, 0}});
        show_what_the_ai_thinks(ctx);
        const auto drawn = ctx.debug().stats();
        check(drawn.labels == 1 && drawn.lines_2d > 64, "debug_draw: two circles, an arrow and one label");
    });
}

// ANCHOR: sound_on_event
struct Picked {}; // an event, sent when the player picks something up

struct CoinSound { ponko::sound sound; double quiet_until; }; // simulation time

// One sound per pickup, but never more than one every 50 ms: ten coins in
// the same tick would only be louder, not better.
static void coin_sound(ponko::ctx& ctx) {
    const double now = ctx.sim_time();
    if (ctx.read<Picked>().empty()) return;
    auto coin = ctx.world().resource<CoinSound>();
    if (now < coin.quiet_until) return;
    coin.quiet_until = now + 0.05;
    ctx.world().set_resource(coin);
    ctx.play_sound(coin.sound);
}
// ANCHOR_END: sound_on_event

static void ten_pickups_in_a_tick_make_one_sound() {
    std::vector<std::uint64_t> played;
    ponko::game("coin sound", 640, 480)
        .sim_system([](ponko::ctx& ctx) {
            if (ctx.sim_tick() == 1 || ctx.sim_tick() == 2 || ctx.sim_tick() == 20)
                for (int i = 0; i < 10; ++i) ctx.send(Picked{});
        }, "pickups")
        .sim_system(coin_sound, "sound")
        .sim_system([&](ponko::ctx& ctx) { for (std::size_t i = 0; i < ctx.sounds_started().size(); ++i) played.push_back(ctx.sim_tick()); }, "listen")
        .run_headless(30, [](ponko::ctx& ctx) {
            const std::vector<float> silence(1000, 0.0f);
            ctx.world().set_resource(CoinSound{ctx.create_sound(22050, silence.data(), silence.size()), 0});
        });
    check(played == std::vector<std::uint64_t>({1, 20}), "sound_on_event: tick 2 was inside the 50 ms, tick 20 was not");
}

// ---- The rest of the cookbook: dragging, bars, menus, the map, platformers, 3D, prefabs, footsteps -----
// ANCHOR: drag
struct Draggable { ponko::vec2 half_size; };
// The sprite being dragged and where, relative to its centre, it was grabbed.
struct Held { bool active; ponko::entity sprite; ponko::vec2 grab; };

static void drag(ponko::ctx& ctx) {
    const ponko::vec2 mouse = ctx.mouse_world();
    auto held = ctx.world().resource<Held>();
    if (ctx.mouse_pressed(PONKO_MOUSE_LEFT)) {
        ctx.world().each([&](ponko::entity e, const Draggable& d, const ponko::transform2d& t) {
            const ponko::vec2 off = mouse - t.position;
            if (std::fabs(off.x) <= d.half_size.x && std::fabs(off.y) <= d.half_size.y) held = {true, e, t.position - mouse};
        });
    }
    if (!ctx.mouse_down(PONKO_MOUSE_LEFT)) held.active = false; // let go
    if (held.active) {
        auto at = ctx.world().get<ponko::transform2d>(held.sprite);
        at.position = mouse + held.grab;
        ctx.world().set(held.sprite, at);
    }
    ctx.world().set_resource(held);
}
// ANCHOR_END: drag

static void a_sprite_follows_the_pointer_by_the_spot_it_was_grabbed_and_stays_when_dropped() {
    auto world = ponko::game("drag", 640, 480)
        .sim_system([](ponko::ctx& ctx) {
            // Puts the pointer over a world point (the camera is at the origin: pixels are world units).
            const auto point_at = [&](ponko::vec2 world_point) { ctx.feed_input(ponko::input_event::mouse_move(world_point)); };
            const std::uint64_t t = ctx.sim_tick();
            if (t == 0) point_at({100, 100});
            if (t == 3) ctx.feed_input(ponko::input_event::mouse(PONKO_MOUSE_LEFT, true));
            if (t == 5) point_at({200, 150});
            if (t == 8) ctx.feed_input(ponko::input_event::mouse(PONKO_MOUSE_LEFT, false));
            if (t == 10) point_at({300, 300});
        }, "pointer")
        .sim_system(drag, "drag")
        .run_headless(14, [](ponko::ctx& ctx) {
            ctx.world().set_resource(Held{false, 0, {0, 0}});
            ctx.world().spawn(ponko::transform_at({105, 100}), Draggable{{10, 10}});
        });
    ponko::vec2 at{};
    world.each([&](const Draggable&, const ponko::transform2d& t) { at = t.position; });
    check(at.x == 205 && at.y == 150, "drag: moved with the pointer, kept the grab offset, stayed where it was dropped");
}

// ANCHOR: health_bar
struct Bar { ponko::rect back, front; };

// The empty bar and the full part of it, above a unit at `at`.
static Bar bar_rects(ponko::vec2 at, const Health& health) {
    const float width = 40;
    const ponko::vec2 corner = at + ponko::vec2{-width / 2, -30};
    const float full = width * static_cast<float>(health.hp) / static_cast<float>(health.max);
    return {{corner, corner + ponko::vec2{width, 5}}, {corner, corner + ponko::vec2{full, 5}}};
}

// A frame system: bars are drawn in world units, so they follow the unit and the camera.
static void draw_health_bars(ponko::ctx& ctx) {
    ctx.world().each([&](const ponko::transform2d& t, const Health& health) {
        const Bar bar = bar_rects(t.position, health);
        ctx.draw_rect(bar.back, ponko::rgb(0.25f, 0.13f, 0.13f));
        ctx.draw_rect(bar.front, ponko::rgb(0.25f, 0.75f, 0.25f));
    });
}
// ANCHOR_END: health_bar

static void the_bar_is_as_full_as_the_health() {
    const Bar half = bar_rects({100, 100}, Health{5, 10});
    check(half.back.max.x - half.back.min.x == 40 && half.front.max.x - half.front.min.x == 20, "health_bar: half the health, half the bar");
    check(half.back.min.y == 70, "health_bar: above the unit");
    auto run = ponko::game("bars", 640, 480)
        .system(draw_health_bars, "bars")
        .headless_frames(true)
        .run_headless_scenario(ponko::input_scenario().duration(3), [](ponko::ctx& ctx) {
            ctx.world().spawn(ponko::transform_at({100, 100}), Health{5, 10});
        });
    check(run.report.passed(), "health_bar: the frame system ran");
}

// ---- Menus and screens
// ANCHOR: pause
enum class screen : std::uint32_t { playing, paused };

static void add_pause_menu(ponko::game& game) {
    // Up while paused. Resume, like Escape, closes it, and that leaves the state.
    game.state_screen(screen::paused, ponko::ui::screen_mode::modal, [](ponko::ctx& ctx) {
        namespace u = ponko::ui;
        auto ui = ctx.ui_retained();
        const auto menu = ui.add_panel(u::none, u::centered(300, 160));
        ui.add_title(menu, "Paused");
        const auto resume = ui.add_button(menu, "Resume");
        ui.set_initial_focus(resume);
        ui.set_closes_screen(resume);
        return menu;
    });
    game.system([](ponko::ctx& ctx) {
        if (ctx.key_pressed(PONKO_KEY_ESCAPE) && ctx.state<screen>() == screen::playing) ctx.push_state(screen::paused);
    }, "pause");
}
// ANCHOR_END: pause

static void escape_pauses_the_world_and_resume_or_escape_brings_it_back() {
    using ponko::input_event;
    const auto tap = [](ponko::input_scenario& sc, std::uint64_t tick, ponko::key k) -> ponko::input_scenario& {
        return sc.at(tick, input_event::key(k, true)).at(tick + 1, input_event::key(k, false));
    };
    std::vector<std::uint64_t> steps, menu_up;
    ponko::input_scenario scenario;
    tap(scenario, 10, PONKO_KEY_ESCAPE);
    tap(scenario, 20, PONKO_KEY_ENTER); // Resume has the focus
    tap(scenario, 30, PONKO_KEY_ESCAPE);
    tap(scenario, 40, PONKO_KEY_ESCAPE);
    scenario.duration(50);
    ponko::game game("pause", 800, 600);
    game.state(screen::playing)
        .add_in_state(ponko::phase::after_physics, screen::playing, [&](ponko::ctx& ctx) { steps.push_back(ctx.sim_tick()); }, "steps");
    add_pause_menu(game);
    game.system([&](ponko::ctx& ctx) { if (ctx.ui_retained().top_screen()) menu_up.push_back(ctx.sim_tick()); }, "menu up");
    auto run = game.headless_frames(true).run_headless_scenario(scenario, [](ponko::ctx&) {});
    check(run.report.passed(), "pause: the scenario ran");
    const auto count = [](const std::vector<std::uint64_t>& ticks, std::uint64_t from, std::uint64_t to) {
        int n = 0;
        for (std::uint64_t t : ticks) n += (t >= from && t < to);
        return n;
    };
    check(count(steps, 0, 10) == 10, "pause: playing");
    check(count(steps, 13, 20) == 0 && count(menu_up, 13, 20) > 0, "pause: paused, nothing ran, the menu is up");
    check(count(steps, 24, 30) == 6 && count(menu_up, 24, 30) == 0, "pause: Resume closed the menu and resumed");
    check(count(steps, 33, 40) == 0, "pause: paused again");
    check(count(steps, 44, 50) == 6 && count(menu_up, 44, 50) == 0, "pause: resumed with Escape");
}

// ANCHOR: title
enum class page : std::uint32_t { title, in_game };

static void build_title(ponko::ctx& ctx) {
    namespace u = ponko::ui;
    auto ui = ctx.ui_retained();
    const auto menu = ui.add_panel(u::none, u::centered(300, 200).gap(12));
    ui.add_title(menu, "Treasure Dash", u::row(48));
    const auto play = ui.add_button(menu, "Play", u::row(44));
    ui.set_name(play, "title.play");
    ui.set_initial_focus(play);
    ui.set_name(ui.add_button(menu, "Quit", u::row(44)), "title.quit");
    ui.push_modeless(menu);
}

// A frame system: what the buttons do. Clicks are events, read once.
static void title_buttons(ponko::ctx& ctx) {
    namespace u = ponko::ui;
    auto ui = ctx.ui_retained();
    for (const auto& e : ui.events()) {
        if (e.kind != u::event_kind::clicked) continue;
        // Names are looked up when something was clicked, not every frame.
        const auto play = ui.find("title.play"), quit = ui.find("title.quit");
        if (play && e.node == *play) {
            ui.pop_screen(); // close the title
            ctx.set_state(page::in_game);
        } else if (quit && e.node == *quit) {
            ctx.quit();
        }
    }
}
// ANCHOR_END: title

static void the_play_button_starts_the_game_and_the_quit_button_ends_the_run() {
    using ponko::scenario_check;
    page last = page::title;
    ponko::input_scenario play;
    play.expect(0, scenario_check::ui("title.play").shown(true).focused(true))
        .click(3, "title.play")
        .expect(4, scenario_check::ui("title.play").shown(false))
        .duration(8);
    auto run = ponko::game("title", 800, 600)
        .state(page::title)
        .system(title_buttons, "title buttons")
        .sim_system([&](ponko::ctx& ctx) { last = ctx.state<page>(); }, "watch")
        .headless_frames(true)
        .run_headless_scenario(play, build_title);
    check(run.report.passed() && last == page::in_game, "title: Play closed the title and changed the state");

    std::vector<std::pair<std::uint64_t, bool>> seen; // (tick, quit asked)
    ponko::input_scenario quit;
    quit.click(3, "title.quit").duration(50);
    auto run_quit = ponko::game("title", 800, 600)
        .state(page::title)
        .system(title_buttons, "title buttons")
        .system([&](ponko::ctx& ctx) { seen.emplace_back(ctx.sim_tick(), ctx.quit_requested()); }, "watch")
        .headless_frames(true)
        .run_headless_scenario(quit, build_title);
    check(!seen.empty() && seen.back() == std::make_pair(std::uint64_t{4}, true), "title: the click on tick 3 asked to quit");
    check(run_quit.report.ticks_run == 4, "title: and no tick ran after it");
}

// ANCHOR: game_over
enum class game_mode : std::uint32_t { playing, game_over };

struct Chances { std::uint32_t count; };

static void lose_a_life(ponko::ctx& ctx) {
    Chances chances = ctx.world().resource<Chances>();
    chances.count = chances.count > 0 ? chances.count - 1 : 0;
    ctx.world().set_resource(chances);
    if (chances.count == 0) ctx.set_state(game_mode::game_over); // applies at the end of the tick
}

// On the game over screen, Enter plays again.
static void play_again(ponko::ctx& ctx) {
    // Setting the state it is already in isn't a no-op: it leaves and enters
    // `playing`, so the on-enter work below runs again.
    if (ctx.key_pressed(PONKO_KEY_ENTER)) ctx.set_state(game_mode::playing);
}

static void add_rounds(ponko::game& game) {
    game.state(game_mode::playing)
        .on_enter(game_mode::playing, [](ponko::ctx& ctx) { ctx.world().set_resource(Chances{3}); })
        .add_in_state(ponko::phase::after_physics, game_mode::game_over, play_again, "play again");
}
// ANCHOR_END: game_over

static void losing_every_life_ends_the_round_and_enter_starts_a_new_one() {
    struct Seen { std::uint64_t tick; game_mode mode; std::uint32_t chances; };
    std::vector<Seen> seen;
    ponko::game game("rounds", 800, 600);
    add_rounds(game);
    ponko::input_scenario scenario;
    scenario.at(12, ponko::input_event::key(PONKO_KEY_ENTER, true)).at(13, ponko::input_event::key(PONKO_KEY_ENTER, false));
    game.sim_system([](ponko::ctx& ctx) { // the test loses a life on ticks 4, 5 and 6
            if (ctx.sim_tick() >= 4 && ctx.sim_tick() <= 6 && ctx.state<game_mode>() == game_mode::playing) lose_a_life(ctx);
        }, "hits")
        .sim_system([&](ponko::ctx& ctx) {
            seen.push_back({ctx.sim_tick(), ctx.state<game_mode>(), ctx.world().resource<Chances>().count});
        }, "record")
        .input_scenario(scenario)
        .run_headless(20);
    check(seen.size() == 20, "game_over: ran 20 ticks");
    check(seen[2].mode == game_mode::playing && seen[2].chances == 3, "game_over: three chances at the start");
    check(seen[8].mode == game_mode::game_over && seen[8].chances == 0, "game_over: out of chances");
    check(seen[19].mode == game_mode::playing && seen[19].chances == 3, "game_over: playing again with fresh chances");
}

// ANCHOR: hud
// The score label, kept when the HUD is built: the frame system uses it
// without looking its name up.
struct ScoreLabel { ponko::ui::id label; };

static void build_hud(ponko::ctx& ctx) {
    namespace u = ponko::ui;
    auto ui = ctx.ui_retained();
    const auto corner = ui.add_panel(u::none, u::fill().padding(16));
    const auto label = ui.add_label(corner, "Score: 0");
    ui.set_name(label, "hud.score");
    ui.show_hud(corner); // always shown, never takes clicks
    ctx.world().set_resource(ScoreLabel{label});
}

struct Score { std::uint32_t points; };

static void update_hud(ponko::ctx& ctx) {
    auto ui = ctx.ui_retained();
    const std::string text = "Score: " + std::to_string(ctx.world().resource<Score>().points);
    const auto label = ctx.world().resource<ScoreLabel>().label;
    if (ui.text(label) != text) ui.set_text(label, text.c_str());
}
// ANCHOR_END: hud

static void the_hud_label_follows_the_score() {
    using ponko::scenario_check;
    ponko::input_scenario scenario;
    scenario.expect(2, scenario_check::ui("hud.score").text("Score: 0"))
        .expect(8, scenario_check::ui("hud.score").text("Score: 30"))
        .duration(10);
    auto run = ponko::game("hud", 800, 600)
        .sim_system([](ponko::ctx& ctx) { if (ctx.sim_tick() == 5) ctx.world().set_resource(Score{30}); }, "points")
        .system(update_hud, "hud")
        .headless_frames(true)
        .run_headless_scenario(scenario, [](ponko::ctx& ctx) {
            ctx.world().set_resource(Score{0});
            build_hud(ctx);
        });
    check(run.report.passed(), "hud: the label showed the score");
}

// ANCHOR: volume_slider
struct MusicBus { ponko::audio_bus bus; };
struct VolumeSlider { ponko::ui::id slider; };

static void build_volume_slider(ponko::ctx& ctx) {
    namespace u = ponko::ui;
    auto ui = ctx.ui_retained();
    const auto panel = ui.add_panel(u::none, u::anchored(0, 0, 0.3f, 0.1f).padding(8));
    const auto slider = ui.add_slider(panel, 0.8f, u::row(24));
    ui.set_name(slider, "options.volume");
    ui.push_modeless(panel);
    ctx.world().set_resource(VolumeSlider{slider});
    // A named bus: everything played on it shares one volume.
    ctx.world().set_resource(MusicBus{ctx.audio_bus("music")});
}

// A frame system: the slider's value (0 to 1) is the bus's volume.
static void apply_volume(ponko::ctx& ctx) {
    auto ui = ctx.ui_retained();
    const auto slider = ctx.world().resource<VolumeSlider>().slider;
    ctx.set_audio_bus_volume(ctx.world().resource<MusicBus>().bus, ui.value(slider));
}
// ANCHOR_END: volume_slider

static void the_slider_sets_the_music_bus_volume() {
    std::vector<float> heard;
    auto run = ponko::game("volume", 800, 600)
        .system([](ponko::ctx& ctx) { // the test moves the slider on frame 4 (a player would drag it)
            if (ctx.frame() == 4) { auto ui = ctx.ui_retained(); ui.set_value(*ui.find("options.volume"), 0.25f); }
        }, "player")
        .system(apply_volume, "volume")
        .system([&](ponko::ctx& ctx) { heard.push_back(ctx.audio_bus_volume(ctx.world().resource<MusicBus>().bus)); }, "listen")
        .headless_frames(true)
        .run_headless_scenario(ponko::input_scenario().duration(8), build_volume_slider);
    check(run.report.passed() && heard.size() >= 8, "volume_slider: ran");
    check(heard.size() >= 8 && heard[2] == 0.8f && heard[7] == 0.25f, "volume_slider: the slider's value is the bus's volume");
}

// ANCHOR: slide_in
struct SlideIn { float seconds; }; // since the panel was asked to appear
struct SidePanel { ponko::ui::id panel; };

static void build_side_panel(ponko::ctx& ctx) {
    namespace u = ponko::ui;
    auto ui = ctx.ui_retained();
    const auto panel = ui.add_panel(u::none, u::fixed(240, 160).top(40).left(-240));
    ui.set_name(ui.add_label(panel, "Inventory"), "side.title");
    ui.set_name(panel, "side.panel");
    ui.show_hud(panel);
    ctx.world().set_resource(SidePanel{panel});
}

// A frame system: 0.4 seconds to slide in from off the left edge, easing out.
static void slide_in_panel(ponko::ctx& ctx) {
    namespace u = ponko::ui;
    SlideIn slide = ctx.world().resource<SlideIn>();
    slide.seconds += ctx.dt();
    ctx.world().set_resource(slide);
    const float t = std::fmin(slide.seconds / 0.4f, 1.0f);
    const float x = -240.0f * std::pow(1.0f - t, 3.0f); // from -240 to 0, slowing down
    auto ui = ctx.ui_retained();
    ui.set_layout(ctx.world().resource<SidePanel>().panel, u::fixed(240, 160).top(40).left(x));
}
// ANCHOR_END: slide_in

static void the_panel_slides_in_from_the_left_and_stops_at_the_edge() {
    std::vector<float> left;
    auto run = ponko::game("slide", 800, 600)
        .system(slide_in_panel, "slide")
        .system([&](ponko::ctx& ctx) {
            auto ui = ctx.ui_retained();
            ui.layout_for(ctx.window_size()); // the layout the next frame will start from
            left.push_back(ui.rect(*ui.find("side.panel")).min.x);
        }, "measure")
        .headless_frames(true)
        .run_headless_scenario(ponko::input_scenario().duration(60), [](ponko::ctx& ctx) {
            ctx.world().set_resource(SlideIn{0});
            build_side_panel(ctx);
        });
    check(run.report.passed() && left.size() >= 60, "slide_in: ran");
    bool never_back = true;
    for (std::size_t i = 1; i < left.size(); ++i) never_back &= left[i] >= left[i - 1];
    check(!left.empty() && left.front() < -200.0f && never_back && left.back() == 0.0f, "slide_in: from off screen to the edge, only ever to the right");
}

// ---- Cameras and maps
// ANCHOR: minimap
static const ponko::vec2 map_size{160, 90};     // pixels on screen
static const ponko::vec2 world_size{3200, 1800};

// Where a world position lands on the minimap, in window pixels. The map sits
// in the bottom right corner, 10 pixels from the edges.
static ponko::vec2 on_minimap(ponko::vec2 window, ponko::vec2 world_pos) {
    const ponko::vec2 corner = window - map_size - ponko::vec2{10, 10};
    return corner + ponko::vec2{world_pos.x / world_size.x * map_size.x, world_pos.y / world_size.y * map_size.y};
}

struct Marker { ponko::color color; };
struct Dot { ponko::texture texture; };

// A frame system. `screen` puts a sprite in window pixels, ignoring the camera.
static void draw_minimap(ponko::ctx& ctx) {
    const ponko::texture dot = ctx.world().resource<Dot>().texture; // a 1 x 1 white texture, made once in setup
    const ponko::vec2 window = ctx.window_size();
    auto screen = ponko::draw_defaults();
    screen.screen = true;
    screen.pivot = {0, 0}; // the pivot is the point of the sprite at the draw position: the top left, not the middle
    auto background = screen;
    background.scale = map_size;
    background.color = ponko::rgba(0, 0, 0, 0.5f);
    ctx.draw(dot, window - map_size - ponko::vec2{10, 10}, background);
    ctx.world().each([&](const ponko::transform2d& t, const Marker& m) {
        auto spot = screen;
        spot.scale = {3, 3};
        spot.color = m.color;
        ctx.draw(dot, on_minimap(window, t.position) - ponko::vec2{1.5f, 1.5f}, spot);
    });
}
// ANCHOR_END: minimap

static void units_land_on_the_minimap_in_proportion_and_the_map_draws_in_a_frame() {
    const ponko::vec2 window{1280, 720};
    const auto top_left = on_minimap(window, {0, 0}), bottom_right = on_minimap(window, world_size), middle = on_minimap(window, world_size / 2.0f);
    check(top_left.x == 1110 && top_left.y == 620, "minimap: the map's top left corner");
    check(bottom_right.x == 1270 && bottom_right.y == 710, "minimap: 10 pixels from the corner");
    check(middle.x == 1190 && middle.y == 665, "minimap: the middle of the world, the middle of the map");
    auto run = ponko::game("minimap", 1280, 720)
        .system(draw_minimap, "minimap")
        .headless_frames(true)
        .run_headless_scenario(ponko::input_scenario().duration(3), [](ponko::ctx& ctx) {
            const std::uint8_t white[4] = {255, 255, 255, 255};
            ctx.world().set_resource(Dot{ctx.create_texture(1, 1, white, 4)});
            ctx.world().spawn(ponko::transform_at({100, 100}), Marker{ponko::colors::red});
            ctx.world().spawn(ponko::transform_at({3000, 900}), Marker{ponko::colors::green});
        });
    check(run.report.passed(), "minimap: the frame system drew the map");
}

// ---- 2D physics
// ANCHOR: platformer
// Runs before the physics step, so the new speed is what the step uses.
static void platformer_controls(ponko::ctx& ctx) {
    const float run = ctx.action_axis2(ctx.world().resource<Move>().id).x; // `move`, resolved once
    const bool jump = ctx.key_pressed(PONKO_KEY_SPACE);
    ctx.world().each([&](ponko::entity me, const Player&, const ponko::transform2d& t, ponko::velocity& v) {
        v.linear.x = run * 200.0f; // gravity keeps handling y
        // Standing on something: a short ray down from the feet finds ground.
        const ponko::vec2 feet = t.position + ponko::vec2{0, 10};
        const bool grounded = ctx.cast_ray(feet, {0, 1}, 4, ponko::filter2d().exclude(me)).has_value();
        if (jump && grounded) v.linear.y = -450.0f; // y points down
    });
}
// ANCHOR_END: platformer

static void the_platformer_player_runs_jumps_from_the_ground_only_and_lands() {
    std::vector<float> heights;
    ponko::input_scenario keys; // fed before the simulation systems of its tick, so before the physics step
    keys.at(10, ponko::input_event::key(PONKO_KEY_D, true)).at(40, ponko::input_event::key(PONKO_KEY_D, false))
        .at(60, ponko::input_event::key(PONKO_KEY_SPACE, true)).at(61, ponko::input_event::key(PONKO_KEY_SPACE, false));
    auto world = ponko::game("platformer", 640, 480)
        .physics({0, 900})
        .input_scenario(keys)
        .add(ponko::phase::before_physics, platformer_controls, "platformer")
        .sim_system([&](ponko::ctx& ctx) {
            ctx.world().each([&](const Player&, const ponko::transform2d& t) { heights.push_back(t.position.y); });
        }, "watch")
        .run_headless(150, [](ponko::ctx& ctx) {
            bind_move(ctx);
            auto w = ctx.world();
            w.spawn(ponko::transform_at({300, 300}), ponko::fixed_body(), ponko::rect_collider({2000, 20})); // the floor, top at y = 290
            auto body = ponko::dynamic_body();
            body.lock_rotation = true;
            w.spawn(Player{}, ponko::transform_at({100, 270}), body, ponko::rect_collider({16, 20}), ponko::velocity{});
        });
    float x = 0;
    world.each([&](const Player&, const ponko::transform2d& t) { x = t.position.x; });
    check(near(x, 190, 8), "platformer: 30 ticks at 200 pixels a second");
    check(heights.size() == 150, "platformer: recorded every tick");
    if (heights.size() == 150) {
        float highest = heights[50];
        for (float h : heights) highest = std::fmin(highest, h);
        check(heights[50] - highest > 50, "platformer: the jump went more than 50 pixels up");
        check(near(heights[149], heights[50], 1.0f), "platformer: and it landed again");
    }
}

// ---- 3D
// ANCHOR: click_3d
struct Chosen {};

// What is under a point of the window: a ray from the camera through it,
// into the 3D physics world.
static std::optional<ponko::entity> pick_at(ponko::ctx& ctx, ponko::vec2 pointer) {
    const auto [origin, direction] = ctx.screen_ray(pointer);
    const auto hit = ctx.cast_ray3d(origin, direction, 100);
    if (!hit) return std::nullopt;
    return ponko::bits(hit->entity);
}

static void click_to_pick(ponko::ctx& ctx) {
    if (!ctx.mouse_pressed(PONKO_MOUSE_LEFT) || ctx.ui_retained().wants_pointer()) return;
    if (const auto clicked = pick_at(ctx, ctx.mouse_pos())) ctx.set_later(*clicked, Chosen{});
}
// ANCHOR_END: click_3d

static ponko::entity crate_at(ponko::ctx& ctx, ponko::vec3 at) {
    return ctx.world().spawn(ponko::transform3d_at(at), ponko::fixed_body3d(), ponko::box_collider({2, 2, 2}));
}

static void a_ray_through_the_pointer_finds_the_cube_under_it_and_nothing_in_the_sky() {
    ponko::entity left = 0, right = 0;
    std::optional<ponko::entity> found_left, found_right, found_sky;
    ponko::game("pick", 800, 600)
        .physics3d({0, 0, 0})
        .sim_system([&](ponko::ctx& ctx) {
            if (ctx.sim_tick() != 2) return;
            found_left = pick_at(ctx, *ctx.world_to_screen3d({-3, 0, 0}));
            found_right = pick_at(ctx, *ctx.world_to_screen3d({3, 0, 0}));
            found_sky = pick_at(ctx, {5, 5});
        }, "pick")
        .run_headless(4, [&](ponko::ctx& ctx) {
            auto camera = ctx.camera3d();
            camera.position = {0, 2, 10};
            ctx.set_camera3d(camera);
            ctx.camera3d_look_at({0, 0, 0});
            left = crate_at(ctx, {-3, 0, 0});
            right = crate_at(ctx, {3, 0, 0});
        });
    check(found_left == left && found_right == right && !found_sky, "click_3d: the ray finds each cube and nothing in the sky");
}

static void clicking_marks_what_the_pointer_is_over() {
    ponko::entity right = 0;
    auto world = ponko::game("click", 800, 600)
        .physics3d({0, 0, 0})
        .sim_system([](ponko::ctx& ctx) {
            if (ctx.sim_tick() == 3) ctx.feed_input(ponko::input_event::mouse(PONKO_MOUSE_LEFT, true));
            if (ctx.sim_tick() == 4) ctx.feed_input(ponko::input_event::mouse(PONKO_MOUSE_LEFT, false));
        }, "mouse")
        .sim_system(click_to_pick, "click")
        .run_headless(8, [&](ponko::ctx& ctx) {
            auto camera = ctx.camera3d();
            camera.position = {0, 0, 10};
            ctx.set_camera3d(camera);
            ctx.camera3d_look_at({0, 0, 0});
            crate_at(ctx, {-3, 0, 0});
            right = crate_at(ctx, {3, 0, 0});
            ctx.feed_input(ponko::input_event::mouse_move(*ctx.world_to_screen3d({3, 0, 0}))); // the pointer over the right one
        });
    std::vector<ponko::entity> chosen;
    world.each([&](ponko::entity e, const Chosen&) { chosen.push_back(e); });
    check(chosen == std::vector<ponko::entity>{right}, "click_3d: clicking marked the cube under the pointer, not the other");
}

// ANCHOR: walk_3d
// A frame system, with the `move` action bound as in the first recipe: walk
// relative to the camera, which follows from behind and comes in front of walls.
static void walk_and_follow(ponko::ctx& ctx, ponko::third_person_camera& rig) {
    const float dt = ctx.dt();
    const ponko::vec2 stick = ctx.action_axis2(ctx.world().resource<Move>().id); // y is down: W is -1
    ctx.world().each([&](ponko::entity me, const Hero&, ponko::transform3d& t) {
        t.position += (rig.flat_forward() * -stick.y + rig.flat_right() * stick.x) * 5.0f * dt;
        auto not_me = ponko::query_filter_ex_defaults();
        not_me.exclude = true;
        not_me.exclude_entity = me; // the camera looks through the hero's own collider
        rig.update_physics(ctx, t.position, not_me, dt);
    });
}
// ANCHOR_END: walk_3d

static void the_hero_walks_where_the_camera_looks_and_the_camera_stays_behind() {
    ponko::third_person_camera rig;
    ponko::vec3 camera_at{};
    ponko::input_scenario scenario;
    scenario.at(0, ponko::input_event::key(PONKO_KEY_W, true))
        .at(60, ponko::input_event::key(PONKO_KEY_W, false))
        .duration(120);
    auto run = ponko::game("walk3d", 800, 600)
        .physics3d({0, 0, 0})
        .system([&](ponko::ctx& ctx) { walk_and_follow(ctx, rig); }, "walk")
        .system([&](ponko::ctx& ctx) { camera_at = ctx.camera3d().position; }, "camera")
        .headless_frames(true)
        .run_headless_scenario(scenario, [](ponko::ctx& ctx) {
            bind_move(ctx);
            ctx.world().spawn(Hero{}, ponko::transform3d_at({0, 0, 0}));
        });
    check(run.report.passed(), "walk_3d: the scenario ran");
    ponko::vec3 at{};
    run.world.each([&](const Hero&, const ponko::transform3d& t) { at = t.position; });
    // About a second at 5 units a second along the camera's flat forward: -Z by default.
    check(at.z > -7.0f && at.z < -4.5f && std::fabs(at.x) < 0.01f, "walk_3d: walked forward");
    check(camera_at.z > at.z + 2 && camera_at.y > 0.5f, "walk_3d: the camera is behind and above the hero");
}

// ---- Prefabs and footsteps
// ANCHOR: prefab
// What the prefab file `prefabs/crab.json` says about a crab.
struct Walker { float speed; float heading; };

// Three crabs from one file, each turned its own way.
static void spawn_crabs(ponko::ctx& ctx) {
    // Components in files must be reflected first, under the name the file uses.
    ctx.world().reflect<Walker>("Walker", {ponko::field("speed", &Walker::speed), ponko::field("heading", &Walker::heading)});
    ponko::rng random = ctx.random().stream(1);
    for (int i = 0; i < 3; ++i) {
        const ponko::entity crab = ctx.spawn_prefab("prefabs/crab.json", {100.0f + 80.0f * static_cast<float>(i), 200});
        auto walker = ctx.world().get<Walker>(crab);
        walker.heading = random.angle(); // this copy only
        ctx.world().set(crab, walker);
    }
}
// ANCHOR_END: prefab

static void three_crabs_from_one_prefab_file_share_a_speed_and_differ_in_heading() {
    auto world = ponko::game("prefab", 800, 600).run_headless(0, spawn_crabs);
    std::vector<float> speeds, headings, xs;
    world.each([&](const Walker& w, const ponko::transform2d& t) { speeds.push_back(w.speed); headings.push_back(w.heading); xs.push_back(t.position.x); });
    check(speeds.size() == 3, "prefab: three crabs");
    if (speeds.size() != 3) return;
    check(speeds[0] == 50 && speeds[1] == 50 && speeds[2] == 50, "prefab: the file's speed");
    check(headings[0] != headings[1] && headings[1] != headings[2] && headings[0] != headings[2], "prefab: each its own heading");
    std::sort(xs.begin(), xs.end());
    check(xs[0] == 100 && xs[1] == 180 && xs[2] == 260, "prefab: spaced out as asked");
}

// ANCHOR: footsteps
struct StepSound { ponko::sound sound; };
struct Stride {
    ponko::vec2 last; // where the last tick left it
    float walked;     // distance since the last step
};

static void footsteps(ponko::ctx& ctx) {
    const ponko::sound step = ctx.world().resource<StepSound>().sound;
    ponko::rng random = ctx.random().stream(ctx.sim_tick());
    ctx.world().each([&](Stride& stride, const ponko::transform2d& t) {
        stride.walked += ponko::length(t.position - stride.last);
        stride.last = t.position;
        if (stride.walked < 40) return;
        stride.walked -= 40;
        auto params = ponko::sound_defaults();
        params.volume = 0.6f;
        params.speed = random.range(0.9f, 1.1f); // a little different each time, so it doesn't sound like a machine
        ctx.play_sound(step, params);
    });
}

static void load_step_sound(ponko::ctx& ctx) {
    // A short thump made in code; a game would `ctx.load_sound("step.ogg")` instead.
    std::vector<float> thump(2000);
    for (std::size_t i = 0; i < thump.size(); ++i)
        thump[i] = std::sin(static_cast<float>(i) * 0.05f) * (1.0f - static_cast<float>(i) / 2000.0f);
    ctx.world().set_resource(StepSound{ctx.create_sound(22050, thump.data(), thump.size())});
}
// ANCHOR_END: footsteps

static void walking_two_hundred_pixels_takes_five_steps() {
    std::vector<std::pair<std::uint64_t, ponko::sound_started>> played; // headless runs hear nothing, but list it
    auto world = ponko::game("steps", 640, 480)
        .sim_system([](ponko::ctx& ctx) { // walks 4 pixels a tick for 50 ticks
            if (ctx.sim_tick() < 50) ctx.world().each([](ponko::transform2d& t, const Stride&) { t.position.x += 4; });
        }, "walk")
        .sim_system(footsteps, "footsteps")
        .sim_system([&](ponko::ctx& ctx) { for (const auto& s : ctx.sounds_started()) played.emplace_back(ctx.sim_tick(), s); }, "listen")
        .run_headless(60, [](ponko::ctx& ctx) {
            load_step_sound(ctx);
            ctx.world().spawn(ponko::transform_at({0, 0}), Stride{{0, 0}, 0});
        });
    std::vector<std::uint64_t> ticks;
    bool as_asked = true, varied = false;
    const ponko::sound step = world.resource<StepSound>().sound;
    for (const auto& [tick, s] : played) {
        ticks.push_back(tick);
        as_asked &= s.sound.id == step.id && s.volume == 0.6f && s.speed >= 0.9f && s.speed < 1.1f && !s.looping;
        varied |= s.speed != played.front().second.speed;
    }
    check(ticks == std::vector<std::uint64_t>({9, 19, 29, 39, 49}), "footsteps: 200 pixels, a step every 40");
    check(as_asked && varied, "footsteps: at 0.6 volume, each a little different");
}

// ANCHOR: developer_console
static ponko::game& add_console(ponko::game& game) {
    return game.console_variable("show_paths", "Show path debug lines", false)
        .console_command("echo", "Print a count", {{"count", PONKO_CONSOLE_INTEGER}},
            [](ponko::ctx&, const std::vector<ponko::console_value>& args) {
                return "Count: " + std::to_string(std::get<std::int64_t>(args.at(0)));
            });
}
// ANCHOR_END: developer_console

static void the_cookbook_console_runs_a_native_command() {
    const auto press = [](ponko::ctx& ctx, ponko::key key) {
        ctx.feed_input(ponko::input_event::key(key, true));
        ctx.feed_input(ponko::input_event::key(key, false));
    };
    ponko::game game("c++ console", 640, 480);
    add_console(game)
        .headless_frames(true)
        .developer_console(true)
        .console_key(PONKO_KEY_F8)
        .system([&](ponko::ctx& ctx) {
            if (ctx.sim_tick() == 1) {
                const auto paths = ctx.console_value("show_paths");
                check(paths && !std::get<bool>(*paths), "console: variable starts false");
                check(!ctx.console_value("missing"), "console: missing variable");
                ctx.ui_retained().insert_text("echo 42");
                press(ctx, PONKO_KEY_ENTER);
            } else if (ctx.sim_tick() == 2) {
                auto ui = ctx.ui_retained();
                const auto output = ui.find("ponko.console.output");
                check(output && ui.text(*output).find("Count: 42") != std::string::npos, "console: typed command returns output");
            }
        })
        .run_headless(2, [&](ponko::ctx& ctx) { press(ctx, PONKO_KEY_F8); });
}

int main() {
    wasd_moves_the_player();
    a_stick_and_keys_move_the_same_action();
    the_player_cannot_leave_the_window();
    the_player_walks_to_the_clicked_point();
    the_player_turns_toward_the_pointer();
    a_held_trigger_fires_at_the_guns_rate_and_bullets_expire();
    touching_a_coin_picks_it_up_once();
    damage_adds_up_and_the_last_hit_removes_the_victim();
    an_enemy_closes_in_and_stops_at_arms_length();
    a_sure_drop_always_leaves_a_coin_and_no_chance_never_does();
    a_jump_leaves_the_floor_once_and_lands();
    a_dash_is_ready_again_after_its_cooldown();
    the_next_key_pressed_becomes_the_jump_key();
    each_wave_has_one_more_enemy_than_the_last();
    a_guard_patrols_until_the_player_comes_near();
    a_slide_arrives_exactly();
    midnight_is_dark_and_noon_is_bright();
    clicking_a_body_selects_it();
    the_ai_draws_its_reach_its_target_and_its_mood();
    ten_pickups_in_a_tick_make_one_sound();
    a_fuse_goes_off_once_after_its_time();
    a_system_with_a_period_runs_at_that_rate();
    holding_tab_drops_the_speed_to_a_quarter_and_letting_go_restores_it();
    the_camera_stops_at_the_level_edge();
    a_hit_moves_the_camera();
    pixel_art_zooms_by_whole_numbers();
    zooming_keeps_the_world_point_under_the_cursor_where_it_was();
    two_notches_of_the_wheel_zoom_in_around_the_pointer();
    a_hazard_that_touches_the_player_costs_one_hit();
    knockback_sends_the_body_away_from_the_blow();
    a_wall_blocks_the_view();
    the_level_places_the_hero_and_the_coins();
    the_sword_follows_the_hero();
    f5_saves_and_f9_restores();
    a_setting_is_kept_between_runs();
    quitting_saves_first_and_stops_the_game();
    a_sprite_follows_the_pointer_by_the_spot_it_was_grabbed_and_stays_when_dropped();
    the_bar_is_as_full_as_the_health();
    escape_pauses_the_world_and_resume_or_escape_brings_it_back();
    the_play_button_starts_the_game_and_the_quit_button_ends_the_run();
    losing_every_life_ends_the_round_and_enter_starts_a_new_one();
    the_hud_label_follows_the_score();
    the_slider_sets_the_music_bus_volume();
    the_panel_slides_in_from_the_left_and_stops_at_the_edge();
    units_land_on_the_minimap_in_proportion_and_the_map_draws_in_a_frame();
    the_platformer_player_runs_jumps_from_the_ground_only_and_lands();
    a_ray_through_the_pointer_finds_the_cube_under_it_and_nothing_in_the_sky();
    clicking_marks_what_the_pointer_is_over();
    the_hero_walks_where_the_camera_looks_and_the_camera_stays_behind();
    three_crabs_from_one_prefab_file_share_a_speed_and_differ_in_heading();
    walking_two_hundred_pixels_takes_five_steps();
    the_cookbook_console_runs_a_native_command();
    if (failures != 0) return 1;
    std::puts("cookbook: all recipes ran");
    return 0;
}
