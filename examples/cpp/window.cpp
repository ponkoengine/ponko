// The window service from C++ and C, headless: every call goes to the inert backend, so no window
// or event loop is ever opened. Run by ctest (cpp_window_headless).
#ifdef NDEBUG
#undef NDEBUG
#endif
#include "ponko.hpp"
#include <cassert>
#include <cstdint>
#include <filesystem>
#include <string>

namespace {

// ANCHOR: window-options
// An options menu: fullscreen choice, cursor and clipboard.
void options_menu(ponko::ctx& ctx) {
    ctx.window_set_title("Play");
    ctx.window_set_size({960, 540});
    // Borderless on the current monitor; exclusive needs an advertised mode.
    ctx.window_set_mode(ponko::window_mode::borderless());
    if (ctx.window_monitor_count() > 0) {
        auto modes = ctx.window_video_modes(0);
        if (!modes.empty()) ctx.window_set_mode(ponko::window_mode::exclusive(0, modes.back()));
    }
    ctx.window_set_cursor(ponko::cursor_icon::pointer);
    ctx.window_set_cursor_grab(ponko::cursor_grab::confined);
    ctx.clipboard_write("seed 1234");
    if (auto text = ctx.clipboard_read()) { /* paste *text */ }
    ctx.window_save_settings(); // restored by window_restore_settings() at setup
}
// ANCHOR_END: window-options

// ANCHOR: window-events
// "Quit without saving?": keep running when the player closes the window.
void confirm_close(ponko::ctx& ctx, bool unsaved) {
    for (const auto& e : ctx.window_events()) {
        if (e.kind == ponko::window_event_kind::file_dropped) { /* open e.path */ }
    }
    if (ctx.window_close_requested() && unsaved) ctx.window_cancel_close();
}
// ANCHOR_END: window-events

void wrapper_calls(ponko::ctx& ctx) {
    options_menu(ctx);
    confirm_close(ctx, true);
    ctx.window_restore_settings();
    assert(ctx.window_title() == "Play");
    assert(ctx.window_mode().kind == ponko::window_mode_kind::windowed); // headless reports windowed
    assert(ctx.window_state().width == 0 && ctx.window_state().monitor == PONKO_WINDOW_NO_MONITOR);
    assert(!ctx.clipboard_read());
    ctx.window_set_mode(ponko::window_mode::windowed());
    ctx.window_set_cursor_visible(false);
    ctx.window_set_cursor_grab(ponko::cursor_grab::locked);
    ctx.window_set_cursor(ponko::cursor_icon::default_);
    ctx.window_set_position(-100, 20);
    ctx.window_set_resize_limits(ponko::vec2{320, 200}, ponko::vec2{1920, 1080});
    ctx.window_set_always_on_top(true);
    ctx.window_set_decorations(false);
    auto props = ctx.window_properties();
    assert(props.always_on_top && !props.decorations && props.has_position && props.position_x == -100);
    assert(props.has_min_size && props.min_size.x == 320);

    ponko::window_image image;
    image.width = 2;
    image.height = 2;
    image.rgba.assign(16, 255);
    image.hotspot_x = 1;
    image.hotspot_y = 1;
    ctx.window_set_icon(&image);
    ctx.window_set_icon();
    ctx.window_set_cursor(image);
    image.hotspot_x = 2; // outside the image
    try {
        ctx.window_set_cursor(image);
        assert(false);
    } catch (const ponko::error& e) {
        assert(e.kind() == PONKO_ERROR_INVALID);
    }
    try {
        ctx.window_set_size({0, 0});
        assert(false);
    } catch (const ponko::error& e) {
        assert(e.kind() == PONKO_ERROR_INVALID);
    }

    assert(!ctx.window_close_requested());
    assert(ctx.window_events().empty());
    try {
        (void)ctx.window_monitor_info(0);
        assert(false);
    } catch (const ponko::error& e) {
        assert(e.kind() == PONKO_ERROR_NOT_FOUND);
    }
    try {
        (void)ctx.window_monitor_name(0);
        assert(false);
    } catch (const ponko::error&) {
    }
    try {
        (void)ctx.window_video_modes(0);
        assert(false);
    } catch (const ponko::error&) {
    }
}

template<class T> T desc(std::uint32_t version) {
    T d{};
    d.struct_size = sizeof d;
    d.version = version;
    return d;
}

// Every C function directly, including descriptor validation.
void c_calls(PonkoCtx* raw) {
    auto m = desc<PonkoWindowModeEx>(PONKO_WINDOW_MODE_EX_VERSION);
    assert(ponko_window_mode_ex_defaults(&m) == PONKO_OK);
    assert(ponko_ctx_window_mode_ex(raw, &m) == PONKO_OK);
    assert(m.kind == PONKO_WINDOW_MODE_WINDOWED && m.monitor == PONKO_WINDOW_NO_MONITOR);
    assert(ponko_ctx_window_set_mode_ex(raw, &m) == PONKO_OK);
    m.kind = 9;
    assert(ponko_ctx_window_set_mode_ex(raw, &m) == PONKO_INVALID_ARGUMENT);
    m.kind = PONKO_WINDOW_MODE_EXCLUSIVE; // needs a monitor index
    assert(ponko_ctx_window_set_mode_ex(raw, &m) == PONKO_INVALID_ARGUMENT);
    m.kind = PONKO_WINDOW_MODE_WINDOWED;
    m.version = 0;
    assert(ponko_ctx_window_set_mode_ex(raw, &m) == PONKO_INVALID_ARGUMENT);
    m.version = 2;
    assert(ponko_ctx_window_set_mode_ex(raw, &m) == PONKO_INVALID_ARGUMENT);
    m.version = 1;
    m.struct_size = 8;
    assert(ponko_ctx_window_set_mode_ex(raw, &m) == PONKO_INVALID_ARGUMENT);
    assert(ponko_window_mode_ex_defaults(nullptr) == PONKO_INVALID_ARGUMENT);

    auto p = desc<PonkoWindowPropertiesEx>(PONKO_WINDOW_PROPERTIES_EX_VERSION);
    assert(ponko_window_properties_ex_defaults(&p) == PONKO_OK);
    assert(p.decorations && p.size.x == 800);
    assert(ponko_ctx_window_properties_ex(raw, &p) == PONKO_OK);
    assert(ponko_ctx_window_set_title(raw, "Native") == PONKO_OK);
    assert(ponko_ctx_window_set_title(raw, nullptr) == PONKO_INVALID_ARGUMENT);
    assert(ponko_ctx_window_set_size(raw, {800, 600}) == PONKO_OK);
    assert(ponko_ctx_window_set_position(raw, 0, 0) == PONKO_OK);
    assert(ponko_ctx_window_set_resize_limits(raw, nullptr, nullptr) == PONKO_OK);
    PonkoVec2 big{900, 900}, small{100, 100};
    assert(ponko_ctx_window_set_resize_limits(raw, &big, &small) == PONKO_INVALID_ARGUMENT);
    assert(ponko_ctx_window_set_always_on_top(raw, false) == PONKO_OK);
    assert(ponko_ctx_window_set_decorations(raw, true) == PONKO_OK);

    std::uint8_t pixels[16] = {};
    auto i = desc<PonkoWindowImageEx>(PONKO_WINDOW_IMAGE_EX_VERSION);
    assert(ponko_window_image_ex_defaults(&i) == PONKO_OK);
    i.width = 2;
    i.height = 2;
    i.rgba = pixels;
    i.rgba_len = sizeof pixels;
    assert(ponko_ctx_window_set_icon_ex(raw, &i) == PONKO_OK);
    assert(ponko_ctx_window_set_icon_ex(raw, nullptr) == PONKO_OK);
    assert(ponko_ctx_window_set_cursor_image_ex(raw, &i) == PONKO_OK);
    i.rgba_len = 3;
    assert(ponko_ctx_window_set_cursor_image_ex(raw, &i) == PONKO_INVALID_ARGUMENT);
    assert(ponko_ctx_window_set_cursor_icon(raw, PONKO_CURSOR_POINTER) == PONKO_OK);
    assert(ponko_ctx_window_set_cursor_icon(raw, PONKO_CURSOR_ALL_RESIZE) == PONKO_OK);
    assert(ponko_ctx_window_set_cursor_icon(raw, UINT32_MAX) == PONKO_INVALID_ARGUMENT);
    assert(ponko_ctx_window_set_cursor_visible(raw, true) == PONKO_OK);
    assert(ponko_ctx_window_set_cursor_grab(raw, PONKO_CURSOR_GRAB_NONE) == PONKO_OK);
    assert(ponko_ctx_window_set_cursor_grab(raw, 3) == PONKO_INVALID_ARGUMENT);

    char text[256] = {};
    std::size_t len = 0, count = 99;
    bool value = true;
    assert(ponko_ctx_window_title(raw, text, sizeof text, &len) == PONKO_OK);
    assert(std::string(text) == "Native" && len == 6);
    assert(ponko_ctx_window_close_requested(raw, &value) == PONKO_OK && !value);
    assert(ponko_ctx_window_close_requested(raw, nullptr) == PONKO_INVALID_ARGUMENT);
    assert(ponko_ctx_window_cancel_close(raw) == PONKO_OK);
    assert(ponko_ctx_window_save_settings(raw) == PONKO_OK);
    assert(ponko_ctx_window_restore_settings(raw) == PONKO_OK);

    assert(ponko_ctx_window_monitor_count(raw, &count) == PONKO_OK && count == 0);
    auto monitor = desc<PonkoWindowMonitorEx>(PONKO_WINDOW_MONITOR_EX_VERSION);
    assert(ponko_window_monitor_ex_defaults(&monitor) == PONKO_OK);
    assert(ponko_ctx_window_monitor_ex(raw, 0, &monitor) == PONKO_NOT_FOUND);
    assert(ponko_ctx_window_monitor_name(raw, 0, text, sizeof text, &len) == PONKO_NOT_FOUND);
    auto video = desc<PonkoWindowVideoModeEx>(PONKO_WINDOW_VIDEO_MODE_EX_VERSION);
    assert(ponko_window_video_mode_ex_defaults(&video) == PONKO_OK);
    assert(ponko_ctx_window_video_mode_count(raw, 0, &count) == PONKO_NOT_FOUND);
    assert(ponko_ctx_window_video_mode_ex(raw, 0, 0, &video) == PONKO_NOT_FOUND);

    auto st = desc<PonkoWindowStateEx>(PONKO_WINDOW_STATE_EX_VERSION);
    assert(ponko_window_state_ex_defaults(&st) == PONKO_OK);
    assert(ponko_ctx_window_state_ex(raw, &st) == PONKO_OK);
    assert(st.monitor == PONKO_WINDOW_NO_MONITOR && st.scale_factor == 1.0f);

    auto event = desc<PonkoWindowEventEx>(PONKO_WINDOW_EVENT_EX_VERSION);
    assert(ponko_window_event_ex_defaults(&event) == PONKO_OK);
    assert(ponko_ctx_window_event_count(raw, &count) == PONKO_OK && count == 0);
    assert(ponko_ctx_window_event_ex(raw, 0, &event, text, sizeof text, &len) == PONKO_INVALID_ARGUMENT);

    assert(ponko_ctx_clipboard_write(raw, "hello") == PONKO_OK);
    assert(ponko_ctx_clipboard_read(raw, text, sizeof text, &len, &value) == PONKO_OK);
    assert(!value && len == 0); // headless: no clipboard
    assert(ponko_ctx_clipboard_read(raw, text, sizeof text, nullptr, &value) == PONKO_INVALID_ARGUMENT);
}

} // namespace

int main() {
    ponko::game("window service", 800, 600).data_dir("window-settings").run_headless(0, [](ponko::ctx& ctx) {
        wrapper_calls(ctx);
        c_calls(ctx.raw());
    });
    std::filesystem::remove_all("window-settings");
}
