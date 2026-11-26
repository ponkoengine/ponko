// The C++ Coin Dash played headlessly, through the engine's input scenarios:
// the same scenarios the Rust game's tests run (tests/coin_dash_menus.rs and
// the key-press session of tests/coin_dash.rs), read from the files they
// check against (examples/coin_dash/scenarios), with the same assertions.
// No window and no GPU.
//
//   ./ponko_cpp_coin_dash_scenarios <folder of the scenario files>
#include "game.hpp"

#include <filesystem>
#include <fstream>
#include <iostream>
#include <sstream>

using namespace coin_dash;

namespace {

int failures = 0;

#define CHECK(cond)                                                                       \
    do {                                                                                  \
        if (!(cond)) {                                                                    \
            ++failures;                                                                   \
            std::cerr << __FILE__ << ":" << __LINE__ << ": check failed: " #cond "\n";    \
        }                                                                                 \
    } while (0)

std::string read_file(const std::filesystem::path& path) {
    std::ifstream in(path, std::ios::binary);
    if (!in) throw std::runtime_error("cannot read " + path.string());
    std::stringstream text;
    text << in.rdbuf();
    return text.str();
}

/// What the player sees after one frame: the screen, and whether the settings
/// screen (retained UI) is up.
struct Shown {
    Screen screen;
    bool settings_ui;
    bool operator==(const Shown& o) const { return screen == o.screen && settings_ui == o.settings_ui; }
};

// tests/coin_dash_menus.rs: the menus lead into play and back, with the UI
// checked at each step by the scenario itself (ui checks), and the screens
// logged by a frame system for the steps a scenario can't name.
void menus(const std::filesystem::path& scenarios) {
    // ANCHOR: scenario_file
    const auto scenario = ponko::input_scenario::from_json(read_file(scenarios / "menus.json"));
    std::vector<Shown> seen;
    ponko::game g("coin dash menus", std::uint32_t(ARENA.x), std::uint32_t(ARENA.y));
    g.seed(SEED).headless_frames(true).plugin(CoinDash{});
    g.system([&seen](ponko::ctx& c) {
        auto t = c.ui_retained();
        const auto settings = t.find("settings");
        const std::size_t tick = std::size_t(c.sim_tick() - 1);
        if (seen.size() <= tick) seen.resize(tick + 1, Shown{Screen::title, false});
        seen[tick] = Shown{screen(c), settings && t.is_shown(*settings)};
    });
    auto [world, report] = g.run_headless_scenario(scenario);
    if (!report.passed()) std::cerr << "menus: " << world.scenario_report_json() << "\n";
    CHECK(report.passed());
    // ANCHOR_END: scenario_file

    auto at = [&](std::size_t tick) { return tick < seen.size() ? seen[tick] : Shown{Screen::title, false}; };
    CHECK((at(5) == Shown{Screen::title, false}));
    CHECK((at(11) == Shown{Screen::settings, true}));
    CHECK((at(26) == Shown{Screen::title, false}));
    CHECK((at(31) == Shown{Screen::playing, false}));
    CHECK((at(41) == Shown{Screen::paused, false}));
    CHECK((at(46) == Shown{Screen::title, false}));
    CHECK((at(51) == Shown{Screen::settings, true}));
    CHECK((at(56) == Shown{Screen::title, false}));
    // The screens in order, each once: nothing in between.
    std::vector<Screen> order;
    for (const auto& s : seen)
        if (order.empty() || order.back() != s.screen) order.push_back(s.screen);
    CHECK((order == std::vector<Screen>{Screen::title, Screen::settings, Screen::title, Screen::playing, Screen::paused,
                                        Screen::title, Screen::settings, Screen::title}));
    // The click on "+" changed the setting, and a round was started.
    CHECK(world.resource<Settings>().ui_scale == 1.25f);
    CHECK(world.resource<Round>().number == 1);
}

// tests/coin_dash.rs, a_whole_session_can_be_played_from_key_presses: a full
// session from raw key presses only. The real read_input runs on every tick
// (as a sim system, since a headless run has no frames) with only the Rules
// plugin; the "player's hands" steer towards the coins while a round runs.
void session(const std::filesystem::path& scenarios, const std::filesystem::path& data) {
    std::filesystem::remove_all(data);
    std::filesystem::create_directories(data);
    // Enter starts; Esc pauses; P resumes; the player then chases to a win;
    // Space plays again; R restarts mid-round; Esc, Q returns to the title.
    const auto scenario = ponko::input_scenario::from_json(read_file(scenarios / "session.json"));
    std::vector<std::pair<std::uint64_t, Screen>> seen;
    ponko::game g("coin dash keys", std::uint32_t(ARENA.x), std::uint32_t(ARENA.y));
    g.seed(SEED).data_dir(data.string().c_str()).input_scenario(scenario);
    g.sim_system(read_input).sim_system([&seen](ponko::ctx& c) {
        // Movement only while a round runs: written where read_input leaves it.
        if (screen(c) == Screen::playing) chase_nearest_coin(c);
        seen.emplace_back(c.sim_tick(), screen(c));
    });
    g.plugin(Rules{});
    auto world = g.run_headless(1800, [](ponko::ctx& c) {
        PlayerInput{}.setup(c);
        SettingsScreen{}.setup(c);
        Drawing{}.setup(c);
    });

    auto at = [&](std::uint64_t tick) {
        for (const auto& [t, s] : seen)
            if (t == tick) return s;
        return Screen::settings;   // never seen: fails every check below
    };
    CHECK(at(4) == Screen::title);
    CHECK(at(10) == Screen::playing);       // Enter started round 1
    CHECK(at(40) == Screen::paused);        // Escape paused
    CHECK(at(70) == Screen::playing);       // P resumed
    CHECK(at(1400) == Screen::result_won);  // round 1 won by chasing
    CHECK(at(1510) == Screen::playing);     // Space played again
    CHECK(at(1650) == Screen::playing);     // R restarted round 2 as round 3
    CHECK(at(1705) == Screen::paused);
    CHECK(world.state<Screen>() == Screen::title);   // Q went back to the title
    CHECK(world.resource<Round>().number == 3);

    // The win was saved; the abandoned rounds were not counted.
    const Progress kept = world.resource<Progress>();
    ponko::game reader("coin dash keys", 1, 1);
    reader.data_dir(data.string().c_str()).run_headless(0, [&](ponko::ctx& c) {
        const auto [saved, status] = Progress::load(c);
        CHECK(status == LoadStatus::loaded);
        CHECK(saved.rounds_played == 1 && saved.rounds_won == 1 && saved.best_coins == COINS);
        CHECK(saved.has_best_time && saved.best_time == kept.best_time && saved.rounds_played == kept.rounds_played);
    });
    std::filesystem::remove_all(data);
}

// tests/coin_dash.rs, collecting_every_coin_wins_the_round, with the scenario
// checking the reflected resources itself at the end of the run.
void win(const std::filesystem::path& data) {
    std::filesystem::remove_all(data);
    const std::uint64_t ticks = std::uint64_t(ROUND_SECONDS) * 60;
    // ANCHOR: resource_checks
    ponko::input_scenario scenario;
    scenario.duration(ticks)
        .expect(ticks - 1, ponko::scenario_check::resource("coin_dash::Round", "/collected", std::to_string(COINS)))
        .expect(ticks - 1, ponko::scenario_check::resource("coin_dash::Round", "/number", "1"))
        .expect(ticks - 1, ponko::scenario_check::resource("coin_dash::Progress", "/rounds_won", "1"))
        .named("coin dash win");
    ponko::game g("coin dash win", std::uint32_t(ARENA.x), std::uint32_t(ARENA.y));
    g.seed(SEED).data_dir(data.string().c_str()).sim_system(chase_nearest_coin).plugin(CoinDash{});
    // Skips the title screen: round 1 runs from the first tick.
    auto [world, report] = g.run_headless_scenario(scenario, [](ponko::ctx& c) { c.set_state(Screen::playing); });
    if (!report.passed()) std::cerr << "win: " << world.scenario_report_json() << "\n";
    CHECK(report.passed());
    CHECK(world.state<Screen>() == Screen::result_won);
    // ANCHOR_END: resource_checks
    const Round round = world.resource<Round>();
    CHECK(round.collected == COINS && round.number == 1 && round.time_left > 0);
    std::size_t coins = 0;
    world.each<ponko::with<Coin>>([&](const ponko::transform2d&) { ++coins; });
    CHECK(coins == 0);
    // The win was written, and the next run starts from it.
    CHECK(std::filesystem::exists(data / PROGRESS_FILE));
    ponko::game next("coin dash win", 1, 1);
    next.data_dir(data.string().c_str()).plugin(Rules{}).run_headless(0, [&](ponko::ctx& c) {
        CHECK(res<SaveFile>(c).load == LoadStatus::loaded && res<Progress>(c).rounds_won == 1);
    });
    std::filesystem::remove_all(data);
}

// A damaged progress file is replaced by fresh progress and kept as a backup;
// a damaged settings file means the defaults; both say so on the title screen.
void damaged_files(const std::filesystem::path& data) {
    std::filesystem::remove_all(data);
    std::filesystem::create_directories(data);
    std::ofstream(data / PROGRESS_FILE) << "{\"version\": 1, \"best_coins\": 99}";
    std::ofstream(data / SETTINGS_FILE) << "not json";
    ponko::game g("coin dash damaged", std::uint32_t(ARENA.x), std::uint32_t(ARENA.y));
    g.data_dir(data.string().c_str()).plugin(CoinDash{}).run_headless(0, [](ponko::ctx& c) {
        CHECK(res<SaveFile>(c).load == LoadStatus::recovered);
        CHECK(res<SettingsFile>(c).load == LoadStatus::recovered);
        CHECK(res<Progress>(c).rounds_played == 0 && res<Settings>(c).ui_scale == 1.0f);
        const auto lines = screen_lines(c);
        bool told = false;
        for (const auto& l : lines) told |= l.text == "Saved progress was unreadable and was reset";
        CHECK(told);
    });
    CHECK(std::filesystem::exists(data / "progress.json.bad"));
    CHECK(std::filesystem::exists(data / "settings.json.bad"));
    std::filesystem::remove_all(data);
}

}  // namespace

int main(int argc, char** argv) {
    if (argc != 2) {
        std::cerr << "usage: ponko_cpp_coin_dash_scenarios <folder of the scenario files>\n";
        return 2;
    }
    const std::filesystem::path scenarios = argv[1];
    const auto temp = std::filesystem::temp_directory_path() / "ponko-cpp-coin-dash";
    try {
        menus(scenarios);
        session(scenarios, temp / "session");
        win(temp / "win");
        damaged_files(temp / "damaged");
    } catch (const std::exception& e) {
        std::cerr << "coin_dash scenarios: " << e.what() << "\n";
        return 1;
    }
    std::filesystem::remove_all(temp);
    if (failures) {
        std::cerr << failures << " check(s) failed\n";
        return 1;
    }
    std::cout << "coin_dash scenarios: every check passed\n";
    return 0;
}
