// Coin Dash in C++: a small, complete game on ponko.hpp, the port of the
// Rust reference game examples/coin_dash. A title screen, settings (UI scale,
// volume, rebindable controls, language), rounds (collect every coin before
// the clock runs out), a pause, a result screen and a saved best time, in
// English and Arabic. The rules are in game.hpp, which scenarios.cpp drives
// without a window through the engine's input scenarios.
//
//   ./ponko_cpp_coin_dash                                   play (a window)
//   ./ponko_cpp_coin_dash --smoke headless [--report r.json]  no window, no GPU
//   ./ponko_cpp_coin_dash --smoke window [--report r.json]    a real window
//
// Move: arrow keys, WASD or the left stick. Enter / Space / South: start,
// resume, play again. R / West: restart the round. Esc / P / Start: pause
// (quit on the title screen). Q / East: back to the title. O / North: settings.
//
// The smoke modes play the game by themselves, from the title screen through
// the settings and a won round and back, and print (or write) a JSON report
// of what they saw; they exit with 0 when the walk-through finished. The
// best time, most coins and the settings are saved in the player's data
// folder (PONKO_DATA_DIR moves it).
#include "game.hpp"

#include <cstring>
#include <fstream>
#include <iostream>
#include <memory>

using namespace coin_dash;

namespace {

/// Ticks to stay on a screen before moving on, so each is shown for a few frames.
constexpr std::uint64_t DWELL = 20;
/// The walk-through gives up after this many ticks (ten rounds' worth of time).
constexpr std::uint64_t MAX_TICKS = 10 * std::uint64_t(ROUND_SECONDS) * 60;

enum class Step { open_settings, close_settings, start, play, leave, done };

/// The walk-through's progress and what it saw.
struct Smoke {
    std::string mode, report_path;
    Step step = Step::open_settings;
    std::uint64_t since = 0;
    std::vector<std::string> screens;
    std::string progress_at_start, settings_at_start, storage_dir;
    std::uint32_t asset_size[2] = {0, 0};
    std::uint64_t frames = 0, ticks = 0;
    bool finished = false;
};

void finish(ponko::ctx& c, Smoke& smoke, std::uint64_t tick, bool completed) {
    const Progress progress = res<Progress>(c);
    smoke.ticks = tick;
    smoke.finished = completed;
    std::string screens;
    for (const auto& s : smoke.screens) screens += (screens.empty() ? "" : ", ") + json::quoted(s);
    const std::string report =
        "{\n  \"mode\": " + json::quoted(smoke.mode) + ",\n  \"screens\": [" + screens +
        "],\n  \"rounds_played\": " + std::to_string(progress.rounds_played) +
        ",\n  \"rounds_won\": " + std::to_string(progress.rounds_won) +
        ",\n  \"best_time\": " + (progress.has_best_time ? json::number(progress.best_time) : std::string("null")) +
        ",\n  \"progress_at_start\": " + json::quoted(smoke.progress_at_start) +
        ",\n  \"storage_dir\": " + (smoke.storage_dir.empty() ? std::string("null") : json::quoted(smoke.storage_dir)) +
        ",\n  \"settings_at_start\": " + json::quoted(smoke.settings_at_start) +
        ",\n  \"asset\": " + json::quoted(COIN_ASSET) + ",\n  \"asset_size\": [" + std::to_string(smoke.asset_size[0]) +
        ", " + std::to_string(smoke.asset_size[1]) + "],\n  \"frames\": " + std::to_string(smoke.frames) +
        ",\n  \"ticks\": " + std::to_string(smoke.ticks) + ",\n  \"finished\": " + (completed ? "true" : "false") + "\n}\n";
    if (smoke.report_path.empty()) {
        std::cout << report;
    } else if (!(std::ofstream(smoke.report_path) << report)) {
        std::cerr << "coin_dash: could not write the report " << smoke.report_path << "\n";
    }
    c.quit();
}

/// Sim system (runs before rules): the player of the walk-through.
void autoplay(ponko::ctx& c, Smoke& smoke) {
    const std::uint64_t tick = c.sim_tick();
    const Screen on = screen(c);
    const std::string name = screen_name(on);
    if (smoke.screens.empty() || smoke.screens.back() != name) smoke.screens.push_back(name);
    const bool waited = tick - smoke.since >= DWELL;
    Step next = smoke.step;
    switch (smoke.step) {
    case Step::open_settings:
        if (on == Screen::title && waited) { edit<Intent>(c, [](Intent& i) { i.settings = true; }); next = Step::close_settings; }
        break;
    case Step::close_settings:
        if (on == Screen::settings && waited) { edit<Intent>(c, [](Intent& i) { i.close_settings = true; }); next = Step::start; }
        break;
    case Step::start:
        if (on == Screen::title && waited) { edit<Intent>(c, [](Intent& i) { i.confirm = true; }); next = Step::play; }
        break;
    case Step::play:
        if (on == Screen::playing) chase_nearest_coin(c);
        else if (is_result(on)) next = Step::leave;
        break;
    case Step::leave:
        if (waited) { edit<Intent>(c, [](Intent& i) { i.back = true; }); next = Step::done; }
        break;
    case Step::done:
        if (on == Screen::title) finish(c, smoke, tick, true);
        break;
    }
    if (tick >= MAX_TICKS) finish(c, smoke, tick, false);
    if (next != smoke.step) {
        smoke.step = next;
        smoke.since = tick;
    }
}

int smoke(const std::string& mode, const std::string& report_path) {
    auto state = std::make_shared<Smoke>();
    state->mode = mode;
    state->report_path = report_path;
    ponko::game g(TITLE, std::uint32_t(ARENA.x), std::uint32_t(ARENA.y));
    g.seed(SEED).app_id(APP_ID).sim_system([state](ponko::ctx& c) { autoplay(c, *state); }, "coin_dash::autoplay");
    g.plugin(CoinDash{}).system([state](ponko::ctx&) { state->frames += 1; }, "coin_dash::count_frames");
    auto setup = [state](ponko::ctx& c) {
        const Art art = res<Art>(c);
        state->asset_size[0] = art.coin.width;
        state->asset_size[1] = art.coin.height;
        state->progress_at_start = load_name(res<SaveFile>(c).load);
        state->settings_at_start = load_name(res<SettingsFile>(c).load);
        state->storage_dir = c.storage_dir();
    };
    if (mode == "window") {
        g.run(setup);
    } else {
        g.run_headless(MAX_TICKS + 1, setup);
    }
    if (!state->finished) {
        std::cerr << "coin_dash: the walk-through did not finish\n";
        return 1;
    }
    return 0;
}

int usage() {
    std::cerr << "usage: ponko_cpp_coin_dash [--smoke headless|window [--report <file>]]\n";
    return 2;
}

}  // namespace

int main(int argc, char** argv) {
    try {
        if (argc == 1) {
            ponko::game g(TITLE, std::uint32_t(ARENA.x), std::uint32_t(ARENA.y));
            build(g).run();
            return 0;
        }
        if (std::strcmp(argv[1], "--smoke") != 0 || argc < 3) return usage();
        const std::string mode = argv[2];
        if (mode != "headless" && mode != "window") return usage();
        std::string report;
        if (argc == 5 && std::strcmp(argv[3], "--report") == 0) report = argv[4];
        else if (argc != 3) return usage();
        return smoke(mode, report);
    } catch (const std::exception& e) {
        std::cerr << "coin_dash: " << e.what() << "\n";
        return 1;
    }
}
