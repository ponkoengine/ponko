// Coin Dash in C++: the reference game of examples/coin_dash (Rust), written
// against ponko.hpp with the same rules, screens, controls, settings, saving
// and sounds. Shared by main.cpp (the program) and scenarios.cpp (its tests).
//
// Three layers, each a plugin and testable on its own, as in the Rust game:
//
// 1. PlayerInput: read_input (frame system) turns keys, gamepad and touch
//    into an Intent resource.
// 2. Rules: rules (sim system) is the whole game. It reads only the Intent and
//    the world, so a test can replace step 1 with a script.
// 3. Drawing: the engine's built-in sprite system draws the entities; hud
//    adds the text. Neither changes the game.
//
// SettingsScreen is a fourth plugin beside them (the retained UI's settings
// screen with rebinding), and CoinDash adds all four.
//
// The game moves through the engine's game states: title -> playing <->
// paused (pushed over it), playing -> result -> playing again (or back to the
// title), and title <-> settings. Every resource is a plain struct, so tools
// see the same state as the systems; Round and Progress are reflected.
//
// Where the C++ port differs from the Rust one, and why:
// - The chosen keys and buttons are saved in their own file, controls.json, in
//   the engine's action-map format (action_map::to_json), next to
//   settings.json. Rust keeps them inside settings.json, by Rust enum names.
// - Keys are shown by the engine's key labels (ctx::key_label, "Esc" for
//   Escape), which follow the keyboard layout; Rust prints its enum names.
// - Its files go in their own folder (APP_ID "coin-dash-cpp"), so the two
//   builds never read each other's settings.
// - The C library is built with text shaping, so Arabic is always offered.
#pragma once
#include "ponko.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <map>
#include <optional>
#include <string>
#include <vector>

namespace coin_dash {

using ponko::vec2;
namespace ui = ponko::ui;

/// Size of the playfield, in world units (the window starts the same size).
constexpr vec2 ARENA{800.0f, 600.0f};
/// Coins on the field at the start of a round.
constexpr std::uint32_t COINS = 8;
/// Seconds to collect them all.
constexpr float ROUND_SECONDS = 20.0f;
/// Player speed, units per second.
constexpr float SPEED = 240.0f;
constexpr float PLAYER_RADIUS = 14.0f;
constexpr float COIN_RADIUS = 10.0f;
/// Random seed for coin layouts: the same seed and inputs always give the same game.
constexpr std::uint64_t SEED = 2024;
/// The folder in the player's data folder where the files are kept.
constexpr const char* APP_ID = "coin-dash-cpp";
/// The coin's file, relative to assets/.
constexpr const char* COIN_ASSET = "coin_dash/coin.png";
/// The string tables, English (the reference) first.
constexpr std::array<const char*, 2> LANGUAGES{"coin_dash/lang/en.json", "coin_dash/lang/ar.json"};

/// The player. Position is the entity's transform2d.
struct Player {};
/// A coin to collect.
struct Coin {};

/// The pictures: a procedural disc (the player) and the coin, the one file the game loads.
struct Art {
    ponko::texture disc;
    ponko::texture coin;
};

/// The sound effects, made from sine tones at startup (no sound files).
struct Sounds {
    ponko::sound pickup, win, lose, select;
};

/// Which screen the game is on: the game's state.
enum class Screen : std::uint32_t { title, settings, playing, paused, result_won, result_lost };

inline bool is_result(Screen s) { return s == Screen::result_won || s == Screen::result_lost; }

inline const char* screen_name(Screen s) {
    switch (s) {
    case Screen::title: return "Title";
    case Screen::settings: return "Settings";
    case Screen::playing: return "Playing";
    case Screen::paused: return "Paused";
    case Screen::result_won: return "Result(Won)";
    case Screen::result_lost: return "Result(Lost)";
    }
    return "?";
}

/// Round bookkeeping. One resource, so tests and tools can read it directly.
struct Round {
    std::uint32_t collected = 0;
    float time_left = ROUND_SECONDS;
    /// 0 before the first round, then 1 for the first round, +1 for every new one.
    std::uint32_t number = 0;
    /// The finished round beat a saved best (set when it ends).
    bool new_best = false;
};

/// What the player asked for since the last simulation tick. The sim reads
/// this, never the keyboard, so a test can write it directly. The buttons are
/// latched until the sim takes them, so a tap between two ticks isn't lost.
struct Intent {
    /// Movement, each axis in -1..1; y is positive down.
    vec2 direction{0, 0};
    bool confirm = false, restart = false, pause = false, back = false, settings = false;
    /// Leave the settings (its Back button or the Escape key).
    bool close_settings = false;
};

/// The action names, resolved once at startup: queries by id are an array index.
struct Act {
    ponko::action_id movement, confirm, restart, pause, back, settings;
};

// ---- Small helpers ------------------------------------------------------------

inline ponko::color hex(std::uint32_t rgb) {
    return ponko::rgb(float((rgb >> 16) & 0xff) / 255.0f, float((rgb >> 8) & 0xff) / 255.0f, float(rgb & 0xff) / 255.0f);
}
inline float distance(vec2 a, vec2 b) { return ponko::length(a - b); }
inline vec2 clamp(vec2 v, vec2 lo, vec2 hi) { return {std::clamp(v.x, lo.x, hi.x), std::clamp(v.y, lo.y, hi.y)}; }
inline vec2 clamp_length_max(vec2 v, float max) {
    const float l = ponko::length(v);
    return l > max ? v * (max / l) : v;
}
/// A copy of the world's resource T.
template<class T> T res(ponko::ctx& c) { return c.world().resource<T>(); }
/// Replaces the world's resource T.
template<class T> void put(ponko::ctx& c, const T& value) { c.world().set_resource(value); }
/// Changes the world's resource T with `fn(T&)`.
template<class T, class F> void edit(ponko::ctx& c, F fn) {
    T value = res<T>(c);
    fn(value);
    put(c, value);
}
inline Screen screen(ponko::ctx& c) { return c.state<Screen>(); }

// ---- A small JSON reader and writer for the two saved files ----------------
//
// Both files are flat objects of numbers, strings, booleans and nulls. Values
// of other shapes (an older file's nested data) are checked and skipped.

namespace json {
struct Value {
    enum Kind { null, boolean, number, string, nested } kind = null;
    bool flag = false;
    double number_value = 0;
    std::string text;
};
using Object = std::map<std::string, Value>;

class Reader {
    const std::string& s_;
    std::size_t at_ = 0;
    void space() { while (at_ < s_.size() && std::strchr(" \t\r\n", s_[at_])) ++at_; }
    bool eat(char c) { space(); if (at_ < s_.size() && s_[at_] == c) { ++at_; return true; } return false; }
    bool word(const char* w) {
        const std::size_t n = std::strlen(w);
        if (s_.compare(at_, n, w) != 0) return false;
        at_ += n;
        return true;
    }
    std::optional<std::string> string() {
        if (!eat('"')) return std::nullopt;
        std::string out;
        while (at_ < s_.size()) {
            const char c = s_[at_++];
            if (c == '"') return out;
            if (static_cast<unsigned char>(c) < 0x20) return std::nullopt;
            if (c != '\\') { out += c; continue; }
            if (at_ >= s_.size()) return std::nullopt;
            const char e = s_[at_++];
            switch (e) {
            case '"': case '\\': case '/': out += e; break;
            case 'n': out += '\n'; break;
            case 't': out += '\t'; break;
            case 'r': out += '\r'; break;
            case 'b': out += '\b'; break;
            case 'f': out += '\f'; break;
            case 'u': {
                if (at_ + 4 > s_.size()) return std::nullopt;
                const unsigned long code = std::strtoul(s_.substr(at_, 4).c_str(), nullptr, 16);
                at_ += 4;
                // Enough for the codes the game writes (ASCII); others become '?'.
                out += code < 0x80 ? char(code) : '?';
                break;
            }
            default: return std::nullopt;
            }
        }
        return std::nullopt;
    }
    std::optional<Value> value(int depth) {
        if (depth > 32) return std::nullopt;
        space();
        if (at_ >= s_.size()) return std::nullopt;
        Value v;
        const char c = s_[at_];
        if (c == '"') {
            auto text = string();
            if (!text) return std::nullopt;
            v.kind = Value::string;
            v.text = std::move(*text);
        } else if (word("null")) {
            v.kind = Value::null;
        } else if (word("true") || word("false")) {
            v.kind = Value::boolean;
            v.flag = s_[at_ - 1] == 'e' && s_.compare(at_ - 4, 4, "true") == 0;
        } else if (c == '{' || c == '[') {
            const char close = c == '{' ? '}' : ']';
            ++at_;
            v.kind = Value::nested;
            if (eat(close)) return v;
            do {
                if (c == '{' && (!string() || !eat(':'))) return std::nullopt;
                if (!value(depth + 1)) return std::nullopt;
            } while (eat(','));
            if (!eat(close)) return std::nullopt;
        } else {
            const char* begin = s_.c_str() + at_;
            char* end = nullptr;
            v.kind = Value::number;
            v.number_value = std::strtod(begin, &end);
            if (end == begin || !std::isfinite(v.number_value)) return std::nullopt;
            at_ += std::size_t(end - begin);
        }
        return v;
    }

public:
    explicit Reader(const std::string& s) : s_(s) {}
    /// The fields of the one object `s` holds, or nothing if it isn't one.
    std::optional<Object> object() {
        Object out;
        if (!eat('{')) return std::nullopt;
        if (!eat('}')) {
            do {
                auto key = string();
                if (!key || !eat(':')) return std::nullopt;
                auto v = value(0);
                if (!v) return std::nullopt;
                out[*key] = std::move(*v);
            } while (eat(','));
            if (!eat('}')) return std::nullopt;
        }
        space();
        if (at_ != s_.size()) return std::nullopt;
        return out;
    }
};

/// A float the shortest way that reads back as the same float.
inline std::string number(float f) {
    char buffer[32];
    for (int digits = 1; digits <= 9; ++digits) {
        std::snprintf(buffer, sizeof buffer, "%.*g", digits, double(f));
        if (std::strtof(buffer, nullptr) == f) break;
    }
    std::string out = buffer;
    if (out.find_first_of(".e") == std::string::npos) out += ".0";
    return out;
}
inline std::string quoted(const std::string& s) {
    std::string out = "\"";
    for (char c : s) {
        if (c == '"' || c == '\\') { out += '\\'; out += c; }
        else if (c == '\n') out += "\\n";
        else out += c;
    }
    return out + "\"";
}

/// Reads field `name` into `out` when it is there; false when it has another type.
inline bool read(const Object& o, const char* name, double& out, bool allow_null = false, bool* is_null = nullptr) {
    auto it = o.find(name);
    if (it == o.end()) return true;
    if (allow_null && it->second.kind == Value::null) { if (is_null) *is_null = true; return true; }
    if (it->second.kind != Value::number) return false;
    out = it->second.number_value;
    if (is_null) *is_null = false;
    return true;
}
inline bool read_text(const Object& o, const char* name, std::string& out) {
    auto it = o.find(name);
    if (it == o.end()) return true;
    if (it->second.kind != Value::string) return false;
    out = it->second.text;
    return true;
}
}  // namespace json

// ---- Saved files --------------------------------------------------------------

/// How a saved file was found at startup.
enum class LoadStatus : std::uint32_t {
    /// There was no file (first run), or saving is off (no app id).
    fresh,
    /// The file was read.
    loaded,
    /// The file was damaged or from another version: the game started from
    /// zero and kept the old file next to it as `<name>.bad`.
    recovered,
};

inline const char* load_name(LoadStatus s) {
    switch (s) {
    case LoadStatus::fresh: return "Fresh";
    case LoadStatus::loaded: return "Loaded";
    case LoadStatus::recovered: return "Recovered";
    }
    return "?";
}

/// The bytes of stored file `name`: nothing for a first run (no file, or no
/// storage), or `recovered` set when it couldn't be read.
inline std::optional<std::string> read_stored(ponko::ctx& c, const char* name, LoadStatus& status) {
    status = LoadStatus::fresh;
    if (!c.storage_is_available()) return std::nullopt;
    try {
        return c.storage_read_text(name);
    } catch (const ponko::error&) {
        status = LoadStatus::recovered;
        return std::nullopt;
    }
}

/// Keeps a damaged file's bytes as `<name>.bad` for inspection (best effort).
inline void keep_bad(ponko::ctx& c, const char* name, const std::string& bytes) {
    try {
        c.storage_write((std::string(name) + ".bad").c_str(), bytes);
    } catch (const ponko::error&) {
    }
}

/// Layout version of the progress file; another version is recovered from.
constexpr std::uint32_t SAVE_VERSION = 1;
/// The progress file's name in the game's storage.
constexpr const char* PROGRESS_FILE = "progress.json";

/// How a round ended.
enum class Outcome { won, lost };

/// What survives between runs: the best results over all rounds.
struct Progress {
    std::uint32_t version = 0;
    /// Fastest win, in seconds into the round (when has_best_time).
    float best_time = 0;
    bool has_best_time = false;
    /// Most coins in one round, won or not (the high score).
    std::uint32_t best_coins = 0;
    std::uint32_t rounds_played = 0;
    std::uint32_t rounds_won = 0;

    /// Whether the numbers could have come from this game.
    bool is_valid() const {
        return version == SAVE_VERSION && best_coins <= COINS && rounds_won <= rounds_played &&
               (!has_best_time || (std::isfinite(best_time) && best_time > 0 && best_time <= ROUND_SECONDS));
    }

    /// Counts a finished round; true when it set a new best time or high score.
    bool record(Outcome outcome, std::uint32_t collected, float elapsed) {
        version = SAVE_VERSION;
        rounds_played += 1;
        bool new_best = collected > best_coins;
        best_coins = std::max(best_coins, collected);
        if (outcome == Outcome::won) {
            rounds_won += 1;
            // A win can't take longer than the round or no time at all.
            elapsed = std::clamp(elapsed, 1.1920929e-7f, ROUND_SECONDS);
            if (!has_best_time || elapsed < best_time) {
                best_time = elapsed;
                has_best_time = true;
                new_best = true;
            }
        }
        return new_best;
    }

    std::string to_json() const {
        return "{\n  \"version\": " + std::to_string(SAVE_VERSION) +
               ",\n  \"best_time\": " + (has_best_time ? json::number(best_time) : std::string("null")) +
               ",\n  \"best_coins\": " + std::to_string(best_coins) +
               ",\n  \"rounds_played\": " + std::to_string(rounds_played) +
               ",\n  \"rounds_won\": " + std::to_string(rounds_won) + "\n}";
    }

    /// Reads a progress file; nothing if it doesn't parse. Missing fields keep
    /// their defaults (a missing version is 0, so such a file is invalid).
    static std::optional<Progress> from_json(const std::string& text) {
        auto o = json::Reader(text).object();
        if (!o) return std::nullopt;
        double version = 0, best_time = 0, best_coins = 0, played = 0, won = 0;
        bool no_time = true;
        if (!json::read(*o, "version", version) || !json::read(*o, "best_time", best_time, true, &no_time) ||
            !json::read(*o, "best_coins", best_coins) || !json::read(*o, "rounds_played", played) ||
            !json::read(*o, "rounds_won", won))
            return std::nullopt;
        for (double n : {version, best_coins, played, won})
            if (n < 0 || n > 4294967295.0 || n != std::floor(n)) return std::nullopt;
        Progress p;
        p.version = std::uint32_t(version);
        p.has_best_time = o->count("best_time") && !no_time;
        p.best_time = float(best_time);
        p.best_coins = std::uint32_t(best_coins);
        p.rounds_played = std::uint32_t(played);
        p.rounds_won = std::uint32_t(won);
        return p;
    }

    /// Reads PROGRESS_FILE. A missing file is a first run; a file that can't
    /// be read, doesn't parse or holds impossible numbers is recovered from:
    /// zeroed progress, and the old bytes copied to progress.json.bad.
    static std::pair<Progress, LoadStatus> load(ponko::ctx& c) {
        LoadStatus status;
        auto text = read_stored(c, PROGRESS_FILE, status);
        if (!text) return {Progress{}, status};
        auto p = from_json(*text);
        if (p && p->is_valid()) return {*p, LoadStatus::loaded};
        keep_bad(c, PROGRESS_FILE, *text);
        return {Progress{}, LoadStatus::recovered};
    }

    /// Writes PROGRESS_FILE. The engine writes safely: a crash in the middle
    /// leaves the old file, not half a new one.
    void save(ponko::ctx& c) const { c.storage_write(PROGRESS_FILE, to_json()); }
};

/// What happened to the progress file. A resource.
struct SaveFile {
    LoadStatus load = LoadStatus::fresh;
    /// The last attempt to write failed (a read-only disk, say). The game goes on.
    bool save_failed = false;
};

// ---- Controls -------------------------------------------------------------------

/// Something the player can do, for which a key (and a pad button) can be chosen.
enum class Control : std::uint32_t { move_up, move_down, move_left, move_right, confirm, restart, pause, back, settings };
constexpr std::array<Control, 9> ALL_CONTROLS{Control::move_up, Control::move_down, Control::move_left,
                                              Control::move_right, Control::confirm, Control::restart,
                                              Control::pause, Control::back, Control::settings};

/// The string table key of a control's name on the settings screen.
inline const char* control_key(Control c) {
    static const char* const keys[] = {"control.move_up", "control.move_down", "control.move_left", "control.move_right",
                                       "control.confirm", "control.restart", "control.pause", "control.back",
                                       "control.settings"};
    return keys[std::size_t(c)];
}
/// The action a control's key and button feed: every movement key is one
/// source of "move", so movement has none of its own.
inline const char* control_action(Control c) {
    static const char* const actions[] = {nullptr, nullptr, nullptr, nullptr, "confirm", "restart", "pause", "back", "settings"};
    return actions[std::size_t(c)];
}
/// Movement is the keys here and the left stick on the pad; the stick is not rebindable.
inline bool is_movement(Control c) { return control_action(c) == nullptr; }

/// Keys that always work next to the chosen ones (arrows for movement, Space
/// to confirm, P to pause). They can't be given to another control.
constexpr std::array<ponko::key, 6> FIXED_KEYS{PONKO_KEY_LEFT, PONKO_KEY_RIGHT, PONKO_KEY_UP,
                                               PONKO_KEY_DOWN, PONKO_KEY_SPACE, PONKO_KEY_P};
inline bool is_fixed(ponko::key k) { return std::find(FIXED_KEYS.begin(), FIXED_KEYS.end(), k) != FIXED_KEYS.end(); }

/// The gamepad buttons the retained UI uses for navigation, which no control may take.
inline bool is_reserved_pad(ponko::gamepad_button b) {
    return b == PONKO_GAMEPAD_EAST || b == PONKO_GAMEPAD_DPAD_UP || b == PONKO_GAMEPAD_DPAD_DOWN ||
           b == PONKO_GAMEPAD_DPAD_LEFT || b == PONKO_GAMEPAD_DPAD_RIGHT;
}

/// No gamepad button.
constexpr std::int32_t NO_PAD = -1;

/// Why a key or button was not accepted.
enum class Refusal { none, reserved, fixed, in_use, no_button };
struct Refused {
    Refusal why = Refusal::none;
    /// The control that has it (in_use).
    Control other = Control::move_up;
};

/// The key, and gamepad button, chosen for each Control (in ALL_CONTROLS
/// order). The installed action map is the live record; from_bindings reads
/// this back from it.
struct Controls {
    ponko::key keys[9] = {PONKO_KEY_W, PONKO_KEY_S, PONKO_KEY_A, PONKO_KEY_D, PONKO_KEY_ENTER,
                          PONKO_KEY_R, PONKO_KEY_ESCAPE, PONKO_KEY_Q, PONKO_KEY_O};
    std::int32_t pads[9] = {NO_PAD, NO_PAD, NO_PAD, NO_PAD, PONKO_GAMEPAD_SOUTH,
                            PONKO_GAMEPAD_WEST, PONKO_GAMEPAD_START, PONKO_GAMEPAD_EAST, PONKO_GAMEPAD_NORTH};

    ponko::key key(Control c) const { return keys[std::size_t(c)]; }
    std::optional<ponko::gamepad_button> pad(Control c) const {
        const auto p = pads[std::size_t(c)];
        return p == NO_PAD ? std::nullopt : std::optional<ponko::gamepad_button>(ponko::gamepad_button(p));
    }

    /// Reads the chosen keys and buttons back from bindings made by
    /// bindings_for (the installed ones are ctx::actions()). A control the map
    /// doesn't bind keeps its default.
    static Controls from_bindings(const ponko::action_map& map) {
        Controls controls;
        if (auto move = map.get("move"); move && !move->bindings.empty() && move->bindings[0].kind == PONKO_ACTION_KEYS2) {
            const auto& k = move->bindings[0];
            controls.keys[std::size_t(Control::move_left)] = ponko::key(k.first);
            controls.keys[std::size_t(Control::move_right)] = ponko::key(k.second);
            controls.keys[std::size_t(Control::move_up)] = ponko::key(k.third);
            controls.keys[std::size_t(Control::move_down)] = ponko::key(k.fourth);
        }
        for (Control control : ALL_CONTROLS) {
            const char* name = control_action(control);
            if (!name) continue;
            auto action = map.get(name);
            if (!action) continue;
            // The chosen key comes first, then the chosen button (if any), then the fixed alternate key.
            const auto& sources = action->bindings;
            if (!sources.empty() && sources[0].kind == PONKO_ACTION_KEY)
                controls.keys[std::size_t(control)] = ponko::key(sources[0].first);
            controls.pads[std::size_t(control)] = NO_PAD;
            for (const auto& b : sources)
                if (b.kind == PONKO_ACTION_GAMEPAD_BUTTON) { controls.pads[std::size_t(control)] = std::int32_t(b.first); break; }
        }
        return controls;
    }

    /// Gives `control` the key, unless the key is not allowed. Nothing changes on refusal.
    Refused set_key(Control control, ponko::key k) {
        if (k == PONKO_KEY_ESCAPE || k == PONKO_KEY_TAB) return {Refusal::reserved};
        if (is_fixed(k)) return {Refusal::fixed};
        for (Control other : ALL_CONTROLS)
            if (other != control && key(other) == k) return {Refusal::in_use, other};
        keys[std::size_t(control)] = k;
        return {};
    }

    /// Gives `control` the gamepad button, unless it is not allowed.
    Refused set_pad(Control control, ponko::gamepad_button b) {
        if (is_movement(control)) return {Refusal::no_button};
        if (is_reserved_pad(b)) return {Refusal::reserved};
        for (Control other : ALL_CONTROLS)
            if (other != control && pad(other) == b) return {Refusal::in_use, other};
        pads[std::size_t(control)] = std::int32_t(b);
        return {};
    }

    /// Whether a settings file could have been written by this game: no key or
    /// button twice, no reserved or fixed key, no button on a movement.
    bool is_valid() const {
        std::vector<ponko::key> seen_keys;
        std::vector<std::int32_t> seen_pads;
        for (Control c : ALL_CONTROLS) {
            const ponko::key k = key(c);
            const bool key_ok = !is_fixed(k) && k != PONKO_KEY_TAB && (k != PONKO_KEY_ESCAPE || c == Control::pause) &&
                                std::find(seen_keys.begin(), seen_keys.end(), k) == seen_keys.end();
            seen_keys.push_back(k);
            bool pad_ok = true;
            if (auto b = pad(c)) {
                // East is Back's default, so it is allowed there and nowhere else.
                pad_ok = !is_movement(c) && (!is_reserved_pad(*b) || (*b == PONKO_GAMEPAD_EAST && c == Control::back)) &&
                         std::find(seen_pads.begin(), seen_pads.end(), std::int32_t(*b)) == seen_pads.end();
                seen_pads.push_back(std::int32_t(*b));
            }
            if (!key_ok || !pad_ok) return false;
        }
        return true;
    }
};

/// A key as the player reads it: the engine's label on their layout ("Enter",
/// "W", "1"), with Escape shortened to "Esc" as in the hints.
inline std::string key_name(ponko::ctx& c, ponko::key k) {
    std::string label = c.key_label(k);
    return label == "Escape" ? "Esc" : label;
}
/// A gamepad button as the settings screen shows it.
inline std::string pad_name(ponko::gamepad_button b) {
    static const char* const names[] = {"South", "East", "North", "West", "LeftBumper", "RightBumper",
                                        "LeftTrigger", "RightTrigger", "Select", "Start", "LeftStick",
                                        "RightStick", "DPadUp", "DPadDown", "DPadLeft", "DPadRight"};
    const auto i = std::size_t(b);
    return i < sizeof names / sizeof names[0] ? names[i] : "?";
}

/// The input bindings for `controls`: the chosen keys and buttons, the fixed
/// alternates, and the left stick.
inline ponko::action_map bindings_for(const Controls& controls) {
    using ponko::action_binding;
    const ponko::gamepad pad{0};
    ponko::action_map map;
    map.set("move", ponko::action::axis2({
                        action_binding::keys2(controls.key(Control::move_left), controls.key(Control::move_right),
                                              controls.key(Control::move_up), controls.key(Control::move_down)),
                        action_binding::keys2(PONKO_KEY_LEFT, PONKO_KEY_RIGHT, PONKO_KEY_UP, PONKO_KEY_DOWN),
                        action_binding::stick(pad, ponko::stick::left, 0.2f),
                    }));
    for (Control control : ALL_CONTROLS) {
        const char* name = control_action(control);
        if (!name) continue;
        ponko::action a{0, {}};
        a.bindings.push_back(action_binding::key(controls.key(control)).raw);
        if (auto b = controls.pad(control)) a.bindings.push_back(action_binding::button(pad, *b).raw);
        if (control == Control::confirm) a.bindings.push_back(action_binding::key(PONKO_KEY_SPACE).raw);
        if (control == Control::pause) a.bindings.push_back(action_binding::key(PONKO_KEY_P).raw);
        // On a touch screen a tap anywhere starts, resumes, plays again.
        if (control == Control::confirm) a.bindings.push_back(action_binding::gesture(ponko::gesture::tap).raw);
        map.set(name, a);
    }
    return map;
}

/// The default controls.
inline ponko::action_map bindings() { return bindings_for(Controls{}); }

/// The on-screen controls, shown on touch screens only: a stick to move,
/// pause in the corner while a round is on, settings on the title. A tap
/// anywhere else confirms (see bindings_for).
inline ponko::touch_controls touch_controls() {
    return ponko::touch_controls().stick("move").corner_button("pause", "II").corner_button("settings", "Set");
}

// ---- Settings -------------------------------------------------------------------

/// Layout version of the settings file; another version is recovered from.
constexpr std::uint32_t SETTINGS_VERSION = 1;
/// The settings file's name, beside the progress file.
constexpr const char* SETTINGS_FILE = "settings.json";
/// The chosen controls, in the engine's action-map format.
constexpr const char* CONTROLS_FILE = "controls.json";
/// The UI scales the settings offer.
constexpr std::array<float, 4> SCALES{0.75f, 1.0f, 1.25f, 1.5f};

/// What the player chose. A resource.
struct Settings {
    std::uint32_t version = SETTINGS_VERSION;
    /// One of SCALES.
    float ui_scale = 1.0f;
    /// Sound effects, 0..1.
    float volume = 0.8f;
    Controls controls;
    /// The language's code ("en", "ar"); one the game has no table for leaves it in English.
    char language[16] = "en";

    std::string language_code() const { return std::string(language, strnlen(language, sizeof language)); }
    void set_language(const std::string& code) {
        std::memset(language, 0, sizeof language);
        std::memcpy(language, code.data(), std::min(code.size(), sizeof language - 1));
    }

    bool is_valid() const {
        return version == SETTINGS_VERSION && std::find(SCALES.begin(), SCALES.end(), ui_scale) != SCALES.end() &&
               std::isfinite(volume) && volume >= 0 && volume <= 1 && controls.is_valid() && language[0] != 0;
    }

    std::string to_json() const {
        return "{\n  \"version\": " + std::to_string(version) + ",\n  \"ui_scale\": " + json::number(ui_scale) +
               ",\n  \"volume\": " + json::number(volume) + ",\n  \"language\": " + json::quoted(language_code()) + "\n}";
    }

    /// Reads settings.json over the defaults (missing fields keep them);
    /// nothing if it doesn't parse or a field has the wrong type.
    static std::optional<Settings> from_json(const std::string& text) {
        auto o = json::Reader(text).object();
        if (!o) return std::nullopt;
        Settings s;
        double version = s.version, scale = s.ui_scale, volume = s.volume;
        std::string language = s.language_code();
        if (!json::read(*o, "version", version) || !json::read(*o, "ui_scale", scale) ||
            !json::read(*o, "volume", volume) || !json::read_text(*o, "language", language))
            return std::nullopt;
        if (version < 0 || version > 4294967295.0 || version != std::floor(version) || language.size() >= sizeof s.language)
            return std::nullopt;
        s.version = std::uint32_t(version);
        s.ui_scale = float(scale);
        s.volume = float(volume);
        s.set_language(language);
        return s;
    }

    /// Reads settings.json and controls.json: like the progress file, a missing
    /// file is a first run and a damaged or impossible one is replaced by the
    /// defaults (and kept as .bad).
    static std::pair<Settings, LoadStatus> load(ponko::ctx& c) {
        Settings settings;
        LoadStatus settings_status;
        if (auto text = read_stored(c, SETTINGS_FILE, settings_status)) {
            auto read = from_json(*text);
            if (read && read->is_valid()) {
                settings = *read;
                settings_status = LoadStatus::loaded;
            } else {
                keep_bad(c, SETTINGS_FILE, *text);
                settings_status = LoadStatus::recovered;
            }
        }
        LoadStatus controls_status;
        if (auto text = read_stored(c, CONTROLS_FILE, controls_status)) {
            try {
                const Controls read = Controls::from_bindings(ponko::action_map::from_json(*text));
                Settings check = settings;
                check.controls = read;
                if (!check.controls.is_valid()) throw std::runtime_error("impossible controls");
                settings.controls = read;
                controls_status = LoadStatus::loaded;
            } catch (const std::exception&) {
                keep_bad(c, CONTROLS_FILE, *text);
                controls_status = LoadStatus::recovered;
            }
        }
        if (settings_status == LoadStatus::recovered || controls_status == LoadStatus::recovered)
            return {Settings{}, LoadStatus::recovered};
        const bool any = settings_status == LoadStatus::loaded || controls_status == LoadStatus::loaded;
        return {settings, any ? LoadStatus::loaded : LoadStatus::fresh};
    }

    void save(ponko::ctx& c) const {
        c.storage_write(SETTINGS_FILE, to_json());
        c.storage_write(CONTROLS_FILE, bindings_for(controls).to_json());
    }
};

/// What happened to the settings files. A resource.
struct SettingsFile {
    LoadStatus load = LoadStatus::fresh;
    bool save_failed = false;
};

/// The languages the settings offer, in the order the language button cycles
/// through them: every table the game loaded.
inline std::vector<ponko::string_language> languages(ponko::ctx& c) { return c.languages(); }

/// The language after `current`, wrapping around.
inline std::string next_language(ponko::ctx& c, const std::string& current) {
    const auto all = languages(c);
    if (all.empty()) return "en";
    std::size_t at = 0;
    for (std::size_t i = 0; i < all.size(); ++i)
        if (all[i].code == current) at = i + 1;
    return all[at % all.size()].code;
}

/// Switches the string tables to `settings`' language, or English if the game
/// has no table for it. Keyed UI text follows on the next frame.
inline void apply_language(ponko::ctx& c, const Settings& settings) {
    if (!c.set_language(settings.language)) c.set_language("en");
}

/// The next step of a UI scale: +1 larger, -1 smaller, stopping at the ends.
inline float step_scale(float current, int direction) {
    auto it = std::find(SCALES.begin(), SCALES.end(), current);
    const int index = it == SCALES.end() ? 1 : int(it - SCALES.begin());
    return SCALES[std::size_t(std::clamp(index + direction, 0, int(SCALES.size()) - 1))];
}

// ---- The settings screen --------------------------------------------------------

/// One control's row: its key button and its pad button (ui::none for movement).
struct Row {
    ui::id key = ui::none;
    ui::id pad = ui::none;
};

/// The settings screen's widgets and the rebinding prompt's state. A
/// resource, built the first time the screen opens.
struct SettingsUi {
    ui::id root, prompt, prompt_text, status, scale_label, scale_down, scale_up, volume, reset, back, language;
    Row rows[9];
    /// The strings revision the accessible names were written for.
    std::uint64_t revision;
    /// The screen is pushed on the UI's stack.
    bool open;
    /// A rebinding prompt is open: the next press of `capture_pad`'s device is the answer.
    bool capturing;
    Control capture_control;
    bool capture_pad;
    /// False in the frame the prompt opened: the press that opened it must not answer it.
    bool armed;
};

namespace detail {
/// Changes `node`'s paint with `fn(PonkoUiPaint&)`, starting from what it has.
template<class F> void restyle(ui::tree& t, ui::id node, F fn) {
    PonkoUiPaint p = t.resolved_paint(node);
    fn(p);
    t.set_paint(node, ui::paint(p));
}
inline ui::id keyed_label(ui::tree& t, ui::id parent, const char* key, const ui::layout& l) {
    const ui::id id = t.add_label(parent, "", l);
    t.set_text_key(id, key);
    return id;
}
}  // namespace detail

inline SettingsUi build_settings_ui(ponko::ctx& c, const Settings& settings) {
    auto t = c.ui_retained();
    SettingsUi s{};
    s.root = t.add_panel(ui::none, ui::fill().padding(16).gap(6));
    detail::restyle(t, s.root, [](PonkoUiPaint& p) { p.background = hex(0x14181f); });
    t.set_name(s.root, "settings");
    const ui::id title = detail::keyed_label(t, s.root, "settings.title", ui::row(40));
    detail::restyle(t, title, [](PonkoUiPaint& p) { p.font_size = 28; });

    ui::id strip = t.add_panel(s.root, ui::row(34).gap(8));
    detail::keyed_label(t, strip, "settings.scale", ui::fixed(150, 34));
    s.scale_down = t.add_button(strip, "-", ui::fixed(40, 34));
    s.scale_label = t.add_label(strip, "", ui::fixed(70, 34));
    t.set_name(s.scale_label, "settings.scale");
    s.scale_up = t.add_button(strip, "+", ui::fixed(40, 34));
    t.set_name(s.scale_up, "settings.scale_up");

    strip = t.add_panel(s.root, ui::row(34).gap(8));
    detail::keyed_label(t, strip, "settings.volume", ui::fixed(150, 34));
    s.volume = t.add_slider(strip, settings.volume, ui::fixed(240, 34));

    strip = t.add_panel(s.root, ui::row(34).gap(8));
    detail::keyed_label(t, strip, "settings.language", ui::fixed(150, 34));
    s.language = t.add_button(strip, "", ui::fixed(160, 34));
    t.set_name(s.language, "settings.language");

    const ui::id hint = detail::keyed_label(t, s.root, "settings.hint", ui::row(52));
    detail::restyle(t, hint, [](PonkoUiPaint& p) { p.font_size = 16; p.wrap = true; });
    const ui::id scroll = t.add_scroll(s.root, ui::fill());
    const ui::id list = t.add_panel(scroll, ui::layout().column().gap(4).width(ui::percent(0.95f)));
    for (Control control : ALL_CONTROLS) {
        strip = t.add_panel(list, ui::row(32).gap(8));
        detail::keyed_label(t, strip, control_key(control), ui::fixed(150, 32));
        Row& row = s.rows[std::size_t(control)];
        row.key = t.add_button(strip, "", ui::fixed(120, 32));
        if (is_movement(control)) detail::keyed_label(t, strip, "settings.left_stick", ui::fixed(120, 32));
        else row.pad = t.add_button(strip, "", ui::fixed(120, 32));
    }

    strip = t.add_panel(s.root, ui::row(36).gap(8));
    s.reset = t.add_button(strip, "", ui::fixed(190, 36));
    t.set_text_key(s.reset, "settings.reset");
    s.back = t.add_button(strip, "", ui::fixed(120, 36));
    t.set_text_key(s.back, "settings.back");
    t.set_name(s.back, "settings.back");
    s.status = t.add_label(s.root, "", ui::row(24));
    detail::restyle(t, s.status, [](PonkoUiPaint& p) { p.font_size = 16; });

    // The prompt: a modal screen with nothing to focus, so no key can press a
    // button behind it; Escape and East pop it (the engine does that).
    s.prompt = t.add_panel(ui::none, ui::centered(460, 110).padding(16).gap(8));
    detail::restyle(t, s.prompt, [](PonkoUiPaint& p) { p.background = hex(0x2b3240); });
    s.prompt_text = t.add_label(s.prompt, "", ui::fixed(420, 78));
    detail::restyle(t, s.prompt_text, [](PonkoUiPaint& p) { p.wrap = true; });
    // The screen fades in and out; the prompt grows into place.
    const auto fade = ui::screen_fade(0.15f);
    t.set_screen_transition(s.root, fade, ui::screen_fade(0.1f));
    auto zoom = ui::screen_zoom(0.9f, 0.12f);
    zoom.opacity = 0.0f;   // and fades (Rust `with_fade`)
    t.set_screen_transition(s.prompt, zoom, fade);
    return s;
}

/// Shows the current settings in the labels and buttons: the scale and volume
/// from `settings`, the keys and buttons read back from the installed
/// bindings, and the names a screen reader says, in the current language.
inline void refresh(ponko::ctx& c, SettingsUi& s, const Settings& settings) {
    auto t = c.ui_retained();
    s.revision = c.strings_revision();
    t.set_text(s.scale_label, (c.format_number(double(settings.ui_scale) * 100.0, 0) + "%").c_str());
    t.set_value(s.volume, settings.volume);
    t.set_accessible_name(s.scale_down, c.format("settings.smaller").c_str());
    t.set_accessible_name(s.scale_up, c.format("settings.larger").c_str());
    t.set_accessible_name(s.volume, c.format("settings.volume").c_str());
    std::string language = c.language();
    for (const auto& l : languages(c))
        if (l.code == language) language = l.name;
    t.set_text(s.language, language.c_str());
    t.set_accessible_name(s.language, (c.format("settings.language") + ": " + language).c_str());
    const Controls bound = Controls::from_bindings(c.actions());
    // A button's name is its text unless told otherwise; "Enter" alone says nothing.
    for (Control control : ALL_CONTROLS) {
        const Row& row = s.rows[std::size_t(control)];
        const std::string key = key_name(c, bound.key(control));
        const std::string label = c.format(control_key(control));
        t.set_text(row.key, key.c_str());
        t.set_accessible_name(row.key, c.format("settings.key_name", {ponko::arg("control", label.c_str()),
                                                                      ponko::arg("key", key.c_str())}).c_str());
        auto button = bound.pad(control);
        if (row.pad != ui::none && button) {
            const std::string name = pad_name(*button);
            t.set_text(row.pad, name.c_str());
            t.set_accessible_name(row.pad, c.format("settings.pad_name", {ponko::arg("control", label.c_str()),
                                                                          ponko::arg("button", name.c_str())}).c_str());
        }
    }
}

/// Installs the bindings and scale of `settings`.
inline void apply(ponko::ctx& c, const Settings& settings) {
    c.set_actions(bindings_for(settings.controls));
    c.ui_retained().set_scale(settings.ui_scale);
}

/// Writes the settings and returns to the title screen. Called by rules.
inline void leave_settings(ponko::ctx& c) {
    const Settings settings = res<Settings>(c);
    bool failed = false;
    if (c.storage_is_available()) {
        try {
            settings.save(c);
        } catch (const ponko::error&) {
            failed = true;
        }
    }
    edit<SettingsFile>(c, [&](SettingsFile& f) { f.save_failed = failed; });
    c.set_state(Screen::title);
}

/// The sentence a refused key or button gets, in the current language.
inline std::string refusal(ponko::ctx& c, Refused refused, const std::string& name) {
    switch (refused.why) {
    case Refusal::reserved: return c.format("refused.reserved", {ponko::arg("key", name.c_str())});
    case Refusal::fixed: return c.format("refused.fixed", {ponko::arg("key", name.c_str())});
    case Refusal::in_use: {
        const std::string other = c.format(control_key(refused.other));
        return c.format("refused.in_use", {ponko::arg("key", name.c_str()), ponko::arg("control", other.c_str())});
    }
    case Refusal::no_button: return c.format("refused.no_button");
    case Refusal::none: break;
    }
    return "";
}

inline void click(ponko::ctx& c, SettingsUi& s, ui::id id) {
    auto t = c.ui_retained();
    if (id == s.back) {
        edit<Intent>(c, [](Intent& i) { i.close_settings = true; });
    } else if (id == s.scale_down || id == s.scale_up) {
        Settings settings = res<Settings>(c);
        settings.ui_scale = step_scale(settings.ui_scale, id == s.scale_up ? 1 : -1);
        put(c, settings);
        t.set_scale(settings.ui_scale);
        refresh(c, s, settings);
    } else if (id == s.language) {
        Settings settings = res<Settings>(c);
        settings.set_language(next_language(c, settings.language_code()));
        put(c, settings);
        apply_language(c, settings);
        refresh(c, s, settings);
    } else if (id == s.reset) {
        // The language is not a control: Reset keeps it.
        Settings settings;
        settings.set_language(res<Settings>(c).language_code());
        apply(c, settings);
        refresh(c, s, settings);
        put(c, settings);
        t.set_text_key(s.status, "settings.restored");
    } else {
        for (Control control : ALL_CONTROLS) {
            const Row& row = s.rows[std::size_t(control)];
            if (id != row.key && id != row.pad) continue;
            const bool pad = id == row.pad;
            const std::string name = c.format(control_key(control));
            t.set_text_key(s.prompt_text, pad ? "settings.prompt_pad" : "settings.prompt_key",
                           {ponko::arg("control", name.c_str())});
            t.push_modal(s.prompt);
            s.capturing = true;
            s.capture_control = control;
            s.capture_pad = pad;
            s.armed = false;
            return;
        }
    }
}

/// While the prompt is open: the first new press of the device it asked for
/// is the answer. The engine reports every key and button that went down this
/// frame, so the screen polls none of them.
inline void capture_press(ponko::ctx& c, SettingsUi& s) {
    if (!s.armed) {
        s.armed = true;
        return;
    }
    std::optional<ponko::pressed_input> press;
    for (const auto& p : c.pressed_this_frame()) {
        if ((s.capture_pad && p.is_gamepad_button()) || (!s.capture_pad && p.is_key())) {
            press = p;
            break;
        }
    }
    if (!press) return;
    const std::string name = s.capture_pad ? pad_name(press->button()) : key_name(c, press->key());
    // What is bound now, read back from the installed map: no copy to drift.
    Controls controls = Controls::from_bindings(c.actions());
    const Refused result = s.capture_pad ? controls.set_pad(s.capture_control, press->button())
                                         : controls.set_key(s.capture_control, press->key());
    auto t = c.ui_retained();
    if (result.why == Refusal::none) {
        // Replace only the action this control feeds; its id stays valid.
        const char* action = control_action(s.capture_control);
        if (!action) action = "move";
        c.set_action(action, *bindings_for(controls).get(action));
        // The saved copy is read back from the map, too.
        Settings settings = res<Settings>(c);
        settings.controls = Controls::from_bindings(c.actions());
        refresh(c, s, settings);
        put(c, settings);
        const std::string control = c.format(control_key(s.capture_control));
        t.set_text_key(s.status, "settings.bound", {ponko::arg("control", control.c_str()), ponko::arg("key", name.c_str())});
        t.pop_screen();   // the prompt
        s.capturing = false;
    } else {
        // Stay in the prompt and say why: pressing Escape still cancels.
        t.set_text(s.prompt_text, refusal(c, result, name).c_str());
    }
}

/// Frame system: the settings screen. Runs after the engine has processed this
/// frame's UI input, so the focus has moved and the events are waiting. It
/// opens the screen when the game is on Screen::settings, turns the UI's
/// events into changes of the Settings (and Intent::close_settings for Back or
/// Escape), runs the rebinding prompt, and closes the screen again when the
/// game has left it.
inline void settings_ui(ponko::ctx& c) {
    const bool on_screen = screen(c) == Screen::settings;
    auto existing = c.world().try_resource<SettingsUi>();
    if (!existing) {
        if (!on_screen) return;
        existing = build_settings_ui(c, res<Settings>(c));
    }
    SettingsUi s = *existing;
    auto t = c.ui_retained();
    if (!on_screen) {
        if (s.open) {
            // Left through the Back button (or Intent): take the screens off the stack.
            while (t.top_screen()) t.pop_screen();
            s.open = false;
            s.capturing = false;
        }
        put(c, s);
        return;
    }
    if (!s.open) {
        refresh(c, s, res<Settings>(c));
        t.set_text(s.status, "");
        t.push_modeless(s.root);
        s.open = true;
    }
    // One at a time: the handlers change the tree between events.
    while (auto event = t.next_event()) {
        if (event->kind == ui::event_kind::clicked) {
            click(c, s, event->node);
        } else if (event->kind == ui::event_kind::changed && event->node == s.volume) {
            const float volume = std::clamp(t.value(s.volume), 0.0f, 1.0f);
            edit<Settings>(c, [&](Settings& settings) { settings.volume = volume; });
        } else if (event->kind == ui::event_kind::screen_popped && event->node == s.prompt) {
            s.capturing = false;
        } else if (event->kind == ui::event_kind::screen_popped && event->node == s.root) {
            // Escape (or East) on the settings screen: leave, as the Back button does.
            s.open = false;
            edit<Intent>(c, [](Intent& i) { i.close_settings = true; });
        }
    }
    if (s.capturing) capture_press(c, s);
    // A table reloaded while the screen is up: the spoken names follow.
    if (s.revision != c.strings_revision()) refresh(c, s, res<Settings>(c));
    put(c, s);
}

// ---- Rules ------------------------------------------------------------------------

/// Plays one of the sound effects at the saved volume.
inline void sfx(ponko::ctx& c, ponko::sound Sounds::*pick) {
    const Sounds sounds = res<Sounds>(c);
    const float volume = res<Settings>(c).volume;
    if (volume > 0) {
        auto params = ponko::sound_defaults();
        params.volume = volume;
        c.play_sound(sounds.*pick, params);
    }
}

/// Entering playing (from the title, the result screen, or a restart): clears
/// the field, then places the player in the middle and the coins at random
/// spots for the next round. The spots come from the context's seeded random
/// numbers, so a given seed and round number always give one layout.
inline void new_round(ponko::ctx& c) {
    auto w = c.world();
    const std::uint32_t number = w.resource<Round>().number + 1;
    w.clear();
    const Art art = w.resource<Art>();
    ponko::rng rng = c.random().stream(number);
    // The disc and the coin picture are 16 units in radius.
    w.spawn(ponko::transform_at(ARENA / 2.0f, 0, {PLAYER_RADIUS / 16, PLAYER_RADIUS / 16}),
            ponko::sprite_of(art.disc, hex(0x3a86ff)), Player{});
    for (std::uint32_t i = 0; i < COINS; ++i) {
        const vec2 at{rng.range(40.0f, ARENA.x - 40.0f), rng.range(40.0f, ARENA.y - 40.0f)};
        w.spawn(ponko::transform_at(at, 0, {COIN_RADIUS / 16, COIN_RADIUS / 16}), ponko::sprite_of(art.coin), Coin{});
    }
    w.set_resource(Round{0, ROUND_SECONDS, number, false});
}

/// Ends the round: records the result in the saved progress (and the file)
/// and shows the result screen.
inline void finish_round(ponko::ctx& c, Outcome outcome) {
    Round round = res<Round>(c);
    Progress progress = res<Progress>(c);
    round.new_best = progress.record(outcome, round.collected, ROUND_SECONDS - round.time_left);
    put(c, progress);
    put(c, round);
    bool failed = false;
    if (c.storage_is_available()) {
        try {
            progress.save(c);
        } catch (const ponko::error&) {
            failed = true;
        }
    }
    edit<SaveFile>(c, [&](SaveFile& f) { f.save_failed = failed; });
    sfx(c, outcome == Outcome::won ? &Sounds::win : &Sounds::lose);
    c.set_state(outcome == Outcome::won ? Screen::result_won : Screen::result_lost);
}

/// One tick of a running round: move, pick up coins, run the clock, end the
/// round when it is won or the time is up.
inline void play(ponko::ctx& c, vec2 direction) {
    auto w = c.world();
    // Move, clamped to the arena. A diagonal is not faster than a straight line.
    const vec2 step = clamp_length_max(direction, 1.0f) * (SPEED * c.dt());
    vec2 player{0, 0};
    w.each<ponko::with<Player>>([&](ponko::transform2d& t) {
        t.position = clamp(t.position + step, {0, 0}, ARENA);
        player = t.position;
    });
    // Pick up the coins the player touches.
    const float reach = PLAYER_RADIUS + COIN_RADIUS;
    std::vector<ponko::entity> touched;
    w.each<ponko::with<Coin>>([&](ponko::entity e, const ponko::transform2d& t) {
        if (distance(t.position, player) <= reach) touched.push_back(e);
    });
    for (auto e : touched) w.despawn(e);
    if (!touched.empty()) sfx(c, &Sounds::pickup);

    Round round = res<Round>(c);
    round.collected += std::uint32_t(touched.size());
    round.time_left = std::max(round.time_left - c.dt(), 0.0f);
    put(c, round);
    if (round.collected >= COINS) finish_round(c, Outcome::won);
    else if (round.time_left <= 0) finish_round(c, Outcome::lost);
}

/// Sim system: the whole game, one fixed step at a time. What a button does
/// depends on the screen (a new screen shows from the next tick on):
///
/// | Screen   | confirm    | restart    | pause  | back  | settings      |
/// |----------|------------|------------|--------|-------|---------------|
/// | title    | play       |            | quit   | quit  | open settings |
/// | playing  |            | new round  | pause  |       |               |
/// | paused   | resume     | new round  | resume | title |               |
/// | result   | play again | play again | title  | title |               |
/// | settings |            |            |        |       |               |
///
/// The settings screen is the retained UI's: only close_settings (its Back
/// button or Escape) leaves it, see settings_ui.
inline void rules(ponko::ctx& c) {
    const Intent in = res<Intent>(c);
    // Take the latched buttons; keep the held direction for the next tick.
    Intent kept;
    kept.direction = in.direction;
    put(c, kept);

    const Screen on = screen(c);
    switch (on) {
    case Screen::title:
        if (in.confirm) {
            sfx(c, &Sounds::select);
            c.set_state(Screen::playing);
        } else if (in.settings) {
            sfx(c, &Sounds::select);
            c.set_state(Screen::settings);
        } else if (in.pause || in.back) {
            c.quit();
        }
        break;
    case Screen::settings:
        if (in.close_settings) leave_settings(c);
        break;
    case Screen::playing:
        if (in.pause) c.push_state(Screen::paused);
        else if (in.restart) c.set_state(Screen::playing);
        else play(c, in.direction);
        break;
    case Screen::paused:
        if (in.back) c.set_state(Screen::title);
        else if (in.restart) c.set_state(Screen::playing);
        else if (in.pause || in.confirm) c.pop_state();
        break;
    case Screen::result_won:
    case Screen::result_lost:
        if (in.confirm || in.restart) c.set_state(Screen::playing);
        else if (in.pause || in.back) c.set_state(Screen::title);
        break;
    }
}

/// Walks straight to the nearest coin: the scripted player of the smoke run and the tests.
inline void chase_nearest_coin(ponko::ctx& c) {
    auto w = c.world();
    vec2 player{0, 0};
    w.each<ponko::with<Player>>([&](const ponko::transform2d& t) { player = t.position; });
    std::optional<vec2> nearest;
    w.each<ponko::with<Coin>>([&](const ponko::transform2d& t) {
        if (!nearest || distance(t.position, player) < distance(*nearest, player)) nearest = t.position;
    });
    edit<Intent>(c, [&](Intent& i) { i.direction = nearest ? ponko::normalize_or(*nearest - player, {0, 0}) : vec2{0, 0}; });
}

/// Frame system: named actions to Intent. Runs once per rendered frame.
inline void read_input(ponko::ctx& c) {
    const Screen on = screen(c);
    const Act act = res<Act>(c);
    c.set_touch_control_shown(act.pause, on == Screen::playing || on == Screen::paused);
    c.set_touch_control_shown(act.settings, on == Screen::title);
    Intent intent = res<Intent>(c);
    intent.direction = c.action_axis2(act.movement);
    intent.confirm |= c.action_pressed(act.confirm);
    intent.restart |= c.action_pressed(act.restart);
    intent.pause |= c.action_pressed(act.pause);
    intent.back |= c.action_pressed(act.back);
    intent.settings |= c.action_pressed(act.settings);
    put(c, intent);
}

// ---- Drawing ----------------------------------------------------------------------

/// A short sine tone gliding from `from` to `to` Hz, fading out: a sound
/// effect made in code, so the game needs no sound files.
inline ponko::sound tone(ponko::ctx& c, float from, float to, float seconds) {
    constexpr std::uint32_t RATE = 44100;
    const std::size_t count = std::size_t(float(RATE) * seconds);
    std::vector<float> samples(count);
    float phase = 0;
    for (std::size_t i = 0; i < count; ++i) {
        const float t = float(i) / float(count);
        phase += (from + (to - from) * t) * 6.28318530717958647692f / float(RATE);
        samples[i] = std::sin(phase) * (1.0f - t) * 0.25f;
    }
    return c.create_sound(RATE, samples.data(), samples.size());
}

/// A white 32x32 disc, generated here so the game needs no image files.
/// Sprites tint and scale it into the player.
inline ponko::texture disc(ponko::ctx& c) {
    constexpr std::uint32_t SIZE = 32;
    std::vector<std::uint8_t> rgba;
    rgba.reserve(SIZE * SIZE * 4);
    for (std::uint32_t y = 0; y < SIZE; ++y) {
        for (std::uint32_t x = 0; x < SIZE; ++x) {
            const float d = distance({float(x) + 0.5f, float(y) + 0.5f}, {SIZE / 2.0f, SIZE / 2.0f});
            const float alpha = std::clamp(SIZE / 2.0f - d, 0.0f, 1.0f);   // soft 1 px edge
            for (std::uint8_t v : {std::uint8_t(255), std::uint8_t(255), std::uint8_t(255), std::uint8_t(alpha * 255.0f)})
                rgba.push_back(v);
        }
    }
    return c.create_texture(SIZE, SIZE, rgba.data(), rgba.size());
}

/// One line of text on a screen.
struct Line {
    std::string text;
    float size;
    ponko::color color;
};

/// The text of the current screen, top to bottom, in the current language.
/// Changes nothing, so a test can check what the player would read.
inline std::vector<Line> screen_lines(ponko::ctx& c) {
    const ponko::color gold = hex(0xffbe0b), green = hex(0x8ac926), red = hex(0xff595e), white = ponko::colors::white;
    const Progress progress = res<Progress>(c);
    const SaveFile save = res<SaveFile>(c);
    const Settings settings = res<Settings>(c);
    auto key = [&](Control control) { return key_name(c, settings.controls.key(control)); };
    using ponko::arg;
    std::string best;
    if (progress.rounds_played == 0) {
        best = c.format("title.no_rounds");
    } else {
        const std::string time = progress.has_best_time
                                     ? c.format("progress.seconds", {arg("seconds", double(progress.best_time))})
                                     : c.format("progress.none");
        best = c.format("progress.best", {arg("time", time.c_str()), arg("coins", int(progress.best_coins)),
                                          arg("total", int(COINS)), arg("won", int(progress.rounds_won)),
                                          arg("played", int(progress.rounds_played))});
    }
    std::vector<Line> lines;
    const Screen on = screen(c);
    switch (on) {
    // The settings screen is the retained UI's; a round shows only the HUD.
    case Screen::playing:
    case Screen::settings:
        return {};
    case Screen::title: {
        lines.push_back({c.format("title.name"), 56, gold});
        lines.push_back({c.format("title.goal", {arg("count", int(COINS)), arg("seconds", int(ROUND_SECONDS))}), 20, white});
        lines.push_back({best, 20, white});
        if (save.load == LoadStatus::recovered) lines.push_back({c.format("title.progress_reset"), 20, red});
        if (res<SettingsFile>(c).load == LoadStatus::recovered) lines.push_back({c.format("title.settings_reset"), 20, red});
        const std::string confirm = key(Control::confirm), open = key(Control::settings), pause = key(Control::pause);
        lines.push_back({c.format("title.keys_quit", {arg("confirm", confirm.c_str()), arg("settings", open.c_str()),
                                                      arg("pause", pause.c_str())}),
                         20, white});
        break;
    }
    case Screen::paused: {
        const std::string pause = key(Control::pause), restart = key(Control::restart), back = key(Control::back);
        lines.push_back({c.format("paused.title"), 48, white});
        lines.push_back({c.format("paused.keys", {arg("pause", pause.c_str()), arg("restart", restart.c_str()),
                                                  arg("back", back.c_str())}),
                         20, white});
        break;
    }
    case Screen::result_won:
    case Screen::result_lost: {
        const Round round = res<Round>(c);
        if (on == Screen::result_won) {
            lines.push_back({c.format("result.won"), 48, green});
            lines.push_back({c.format("result.time", {arg("seconds", double(ROUND_SECONDS - round.time_left))}), 24, white});
        } else {
            lines.push_back({c.format("result.lost"), 48, red});
            lines.push_back({c.format("result.coins", {arg("count", int(round.collected)), arg("total", int(COINS))}), 24, white});
        }
        if (round.new_best) lines.push_back({c.format("result.new_best"), 24, gold});
        lines.push_back({best, 20, white});
        const std::string confirm = key(Control::confirm), back = key(Control::back);
        lines.push_back({c.format("result.keys", {arg("confirm", confirm.c_str()), arg("back", back.c_str())}), 20, white});
        break;
    }
    }
    if (save.save_failed) lines.push_back({c.format("save.failed"), 20, red});
    return lines;
}

/// How the lines of a screen come in: they fade in and rise into place, on the
/// UI clock, so they come in the same way on a paused round.
inline ui::transition menu_in() { return ui::make_transition(0.25f); }
/// How far the lines rise as they come in, at UI scale 1.
constexpr float MENU_RISE = 12.0f;

/// The lines of which screen are shown, since when on the UI clock, and how far
/// they have come in (0 when the screen has just changed, 1 once its lines are in).
struct MenuFade {
    bool has_screen = false;
    Screen screen = Screen::title;
    double since = 0;
    float shown = 0;
};

/// How far the current screen's lines have come in, 0 to 1.
inline float menu_fade(ponko::ctx& c) {
    auto t = c.ui_retained();
    const double now = t.clock();
    const Screen current = screen(c);
    MenuFade fade = res<MenuFade>(c);
    if (!fade.has_screen || fade.screen != current) {
        fade.has_screen = true;
        fade.screen = current;
        fade.since = now;
    }
    fade.shown = t.reduced_motion() ? 1.0f : ui::progress(menu_in(), float(now - fade.since));
    put(c, fade);
    return fade.shown;
}

/// Frame system: the text on top of the sprites, at the player's UI scale,
/// with the current language's font (the built-in one for English).
inline void hud(ponko::ctx& c) {
    const float shown = menu_fade(c);
    const float scale = res<Settings>(c).ui_scale;
    const ponko::font font = c.language_font().value_or(ponko::font{});
    auto params = [&](float size, ponko::color color) {
        auto p = ponko::text_defaults();
        p.font = font;
        p.size = size;
        p.color = color;
        return p;
    };
    if (c.in_state_stack(Screen::playing)) {
        const Round round = res<Round>(c);
        const std::string status = c.format("hud.status", {ponko::arg("round", int(round.number)),
                                                           ponko::arg("count", int(round.collected)),
                                                           ponko::arg("total", int(COINS)),
                                                           ponko::arg("seconds", double(round.time_left))});
        // Right to left, the status line starts at the right edge.
        const float width = c.measure_text(status.c_str(), font, 20 * scale).x;
        const float x = c.language_is_rtl() ? ARENA.x - 12 * scale - width : 12 * scale;
        c.draw_text(status.c_str(), {x, 8 * scale}, params(20 * scale, ponko::colors::white));
    }
    // The block of lines, centred on the arena.
    const auto lines = screen_lines(c);
    float height = 0;
    for (const auto& l : lines) height += l.size * scale * 1.5f;
    float y = (ARENA.y - height) / 2 + MENU_RISE * scale * (1 - shown);
    for (const auto& l : lines) {
        const float size = l.size * scale;
        const float width = c.measure_text(l.text.c_str(), font, size).x;
        ponko::color color = l.color;
        color.a *= shown;
        c.draw_text(l.text.c_str(), {(ARENA.x - width) / 2, y}, params(size, color));
        y += size * 1.5f;
    }
}

// ---- Plugins -------------------------------------------------------------------

/// Layer 1: keys, gamepad and touch to an Intent, once per rendered frame.
struct PlayerInput {
    void build(ponko::game& g) { g.add(ponko::phase::frame, read_input, "coin_dash::read_input"); }
    void setup(ponko::ctx& c) {
        // The default controls; the ids stay valid when the settings (or a rebinding) replace them.
        c.set_actions(bindings());
        put(c, Act{c.action_id("move"), c.action_id("confirm"), c.action_id("restart"), c.action_id("pause"),
                   c.action_id("back"), c.action_id("settings")});
        put(c, Intent{});
        c.set_touch_controls(touch_controls());
    }
};

/// The player's choices (UI scale, volume, controls, language): loaded at
/// startup, changed on the settings screen.
struct SettingsScreen {
    void build(ponko::game& g) { g.add(ponko::phase::frame, settings_ui, "coin_dash::settings_ui"); }
    void setup(ponko::ctx& c) {
        for (const char* table : LANGUAGES) c.load_strings(table);
        const auto [loaded, load] = Settings::load(c);
        c.set_actions(bindings_for(loaded.controls));
        auto t = c.ui_retained();
        t.set_scale(loaded.ui_scale);
        // The settings screen's controls ease between their states.
        ui::theme_ex theme = t.theme_ex();
        theme.transition = ui::make_transition(0.12f);
        theme.press_scale = 0.97f;
        t.set_theme(theme);
        apply_language(c, loaded);
        put(c, loaded);
        put(c, SettingsFile{load, false});
    }
};

// ANCHOR: rules_plugin
/// Layer 2: the rules, on the fixed tick, the screens and the resources they
/// own. The game starts on the title screen with the progress loaded.
struct Rules {
    void build(ponko::game& g) {
        g.state(Screen::title)
            .on_enter(Screen::title, [](ponko::ctx& c) { c.world().clear(); })
            .on_enter(Screen::playing, new_round)
            .add(ponko::phase::after_physics, rules, "coin_dash::rules");
    }
    void setup(ponko::ctx& c) {
        auto w = c.world();
        w.reflect_resource<Round>({ponko::field("collected", &Round::collected), ponko::field("time_left", &Round::time_left),
                                   ponko::field("number", &Round::number), ponko::field("new_best", &Round::new_best)});
        w.reflect_resource<Progress>({ponko::field("best_time", &Progress::best_time),
                                      ponko::field("has_best_time", &Progress::has_best_time),
                                      ponko::field("best_coins", &Progress::best_coins),
                                      ponko::field("rounds_played", &Progress::rounds_played),
                                      ponko::field("rounds_won", &Progress::rounds_won)});
        put(c, Round{});
        const auto [progress, load] = Progress::load(c);
        put(c, progress);
        put(c, SaveFile{load, false});
    }
};
// ANCHOR_END: rules_plugin

/// Layer 3: the pictures, the text and the sound effects. The sprites
/// themselves are drawn by the engine's built-in sprite system.
struct Drawing {
    void build(ponko::game& g) { g.add(ponko::phase::frame, hud, "coin_dash::hud"); }
    void setup(ponko::ctx& c) {
        put(c, MenuFade{});
        c.clear(hex(0x14181f));
        put(c, Art{disc(c), c.load_texture(COIN_ASSET)});
        put(c, Sounds{tone(c, 880, 1320, 0.10f), tone(c, 523, 1047, 0.45f), tone(c, 330, 140, 0.45f),
                      tone(c, 660, 660, 0.05f)});
    }
};

// ANCHOR: whole_game
/// The whole game: its layers, each a plugin of its own. Startup is the
/// plugins' setup, in this order.
struct CoinDash {
    void build(ponko::game& g) { g.plugin(PlayerInput{}).plugin(SettingsScreen{}).plugin(Rules{}).plugin(Drawing{}); }
};

/// The window's title.
constexpr const char* TITLE = "Coin Dash";

/// Makes `g` (a ponko::game(TITLE, ARENA)) the game, ready for run or
/// run_headless: CoinDash with progress and settings in the player's data
/// folder (PONKO_DATA_DIR moves it).
inline ponko::game& build(ponko::game& g) { return g.seed(SEED).app_id(APP_ID).plugin(CoinDash{}); }
// ANCHOR_END: whole_game

}  // namespace coin_dash
