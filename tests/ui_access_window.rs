//! The accessibility adapter on the engine's own window, for a person to run.
//!
//! `Game::run` makes its window hidden, attaches the adapter to it
//! (`UiAccessAdapter::for_window`: `for_ns_view` on the content view on
//! macOS, `for_hwnd` on Windows) and then shows it. This check runs that
//! path with the guide's sample menu, in a window placed far off the screen
//! (restored window settings put it at -30000, -30000, so nothing appears
//! where the system allows that), and has the system's accessibility API
//! find the Play button through the window and press it, as a screen reader
//! would: on macOS a system of the game asks the content view
//! (`NSAccessibility`, on the main thread, where AppKit wants it); on
//! Windows a UI Automation client on a thread of its own finds the window
//! by its title. The check passes once the game's status line says the run
//! started.
//!
//! It still makes a real window, takes the keyboard focus and runs the event
//! loop for a second or two, so it only runs when asked, as an ignored test
//! would:
//!
//! ```text
//! cargo test --features accessibility --test ui_access_window -- --ignored
//! ```
//!
//! The adapters without the engine's window are checked on every run:
//! `ui_access_macos` (an `NSView` with no window) and `ui_access_platforms`
//! (a Win32 window that is never shown, and AT-SPI). On Linux the engine's
//! window uses AT-SPI, which needs no window handle; this file does nothing
//! there, and nothing without the `accessibility` feature.
//!
//! AppKit and winit's event loop must be used from the main thread, so this
//! is a program without the test harness, like `ui_access_macos`.

#[cfg(not(all(feature = "accessibility", any(target_os = "macos", target_os = "windows"))))]
fn main() {}

#[cfg(all(feature = "accessibility", any(target_os = "macos", target_os = "windows")))]
fn main() {
    window::run();
}

/// The guide's sample menu, `examples/ui_menu.rs`: its nodes and what it
/// does with an event, without its `main`.
#[cfg(all(feature = "accessibility", any(target_os = "macos", target_os = "windows")))]
#[allow(dead_code)]
#[path = "../examples/ui_menu.rs"]
mod sample;

#[cfg(all(feature = "accessibility", any(target_os = "macos", target_os = "windows")))]
mod window {
    use super::{platform, sample};
    use ponko::math::IVec2;
    use ponko::prelude::*;
    use std::cell::Cell;

    /// The window's title: the Windows client finds the window by it.
    pub const TITLE: &str = "Ponko window adapter check";
    /// Far off every screen.
    const OFFSCREEN: IVec2 = IVec2::new(-30_000, -30_000);
    /// About ten seconds at 60 frames a second: then the check gives up.
    const FRAME_LIMIT: u32 = 600;
    /// What the status line says once Play was pressed.
    const STARTED: &str = "Started a run for Ann";

    /// The engine's path: `Game::run` gives its window to the system's
    /// accessibility API before showing it, and the system presses a button
    /// of the game's UI through that window.
    fn the_engine_window_answers_the_system_through_its_adapter_offscreen() {
        let dir = std::env::temp_dir().join(format!("ponko-access-window-{}", std::process::id()));
        // A first, headless run saves the window far off the screen; the
        // real run restores that, so its window is made there.
        Game::new(TITLE, 520, 460).data_dir(&dir).run_headless(0, |ctx| {
            ctx.window.set_position(OFFSCREEN).unwrap();
            ctx.window.save_settings(&mut ctx.storage).unwrap();
        });
        thread_local! {
            static FRAME: Cell<u32> = const { Cell::new(0) };
            static DONE: Cell<bool> = const { Cell::new(false) };
        }
        Game::new(TITLE, 520, 460)
            .data_dir(&dir)
            .restore_window_settings(true)
            .setup(|ctx: &mut Ctx| {
                let menu = sample::build(&mut ctx.ui);
                ctx.world.insert_resource(menu);
                platform::start();
            })
            .system(|ctx: &mut Ctx| {
                let menu = ctx.world.remove_resource::<sample::Menu>().unwrap();
                while let Some(event) = ctx.ui.next_event() {
                    sample::react(&mut ctx.ui, &menu, event);
                }
                let started = ctx.ui.text(menu.status) == Some(STARTED);
                ctx.world.insert_resource(menu);
                let frame = FRAME.get() + 1;
                FRAME.set(frame);
                if started {
                    DONE.set(true);
                    ctx.quit();
                } else if frame > FRAME_LIMIT {
                    ctx.quit();
                } else {
                    platform::frame(frame);
                }
            })
            .run();
        let _ = std::fs::remove_dir_all(&dir);
        platform::finish();
        assert!(
            DONE.get(),
            "the system did not press Play through the window's adapter within {FRAME_LIMIT} frames"
        );
    }

    pub fn run() {
        let checks: &[(&str, fn())] = &[(
            "the_engine_window_answers_the_system_through_its_adapter_offscreen",
            the_engine_window_answers_the_system_through_its_adapter_offscreen,
        )];
        // `cargo test -- --ignored` (or `--include-ignored`) asks for the
        // checks a person runs; a plain `cargo test` only lists them.
        let args: Vec<String> = std::env::args().skip(1).collect();
        let asked = args.iter().any(|a| a == "--ignored" || a == "--include-ignored");
        let filter = args.iter().find(|a| !a.starts_with('-'));
        println!("\nrunning {} checks on the main thread", checks.len());
        let mut ran = 0;
        for (name, check) in checks {
            if filter.is_some_and(|f| !name.contains(f.as_str())) {
                continue;
            }
            if !asked {
                println!("check {name} ... ignored, opens a window: run with -- --ignored");
                continue;
            }
            // A failed check panics: the program exits non-zero and cargo
            // reports the test as failed, with the name printed last.
            print!("check {name} ... ");
            check();
            println!("ok");
            ran += 1;
        }
        println!("\ncheck result: ok. {ran} passed\n");
    }
}

/// macOS: a system of the game asks the window's content view what
/// VoiceOver asks, on the main thread.
#[cfg(all(feature = "accessibility", target_os = "macos"))]
mod platform {
    use objc2::rc::Id;
    use objc2::runtime::{AnyObject, NSObject};
    use objc2::{class, msg_send, msg_send_id};
    use objc2_foundation::{NSArray, NSString};
    use std::cell::Cell;

    type Element = Id<AnyObject>;

    thread_local! {
        static PRESSED: Cell<bool> = const { Cell::new(false) };
    }

    /// The content view of the game's window.
    fn content_view() -> Element {
        // SAFETY: main thread (systems run on it); the game's window exists
        // while its systems run, and these messages return objects.
        unsafe {
            let app: Element = msg_send_id![class!(NSApplication), sharedApplication];
            let windows: Id<NSArray<NSObject>> = msg_send_id![&*app, windows];
            let window: Element = msg_send_id![&*windows, objectAtIndex: 0usize];
            msg_send_id![&*window, contentView]
        }
    }

    fn children(of: &AnyObject) -> Vec<Element> {
        // SAFETY: `accessibilityChildren` returns an array of objects (or nil).
        let array: Option<Id<NSArray<NSObject>>> = unsafe { msg_send_id![of, accessibilityChildren] };
        let Some(array) = array else { return Vec::new() };
        // SAFETY: plain `NSArray` messages, with indices below its count.
        let count: usize = unsafe { msg_send![&*array, count] };
        (0..count)
            .map(|i| unsafe { msg_send_id![&*array, objectAtIndex: i] })
            .collect()
    }

    fn text(of: &AnyObject, attribute: &str) -> String {
        // SAFETY: both messages return an `NSString` or nil.
        let value: Option<Id<NSString>> = unsafe {
            match attribute {
                "role" => msg_send_id![of, accessibilityRole],
                _ => msg_send_id![of, accessibilityTitle],
            }
        };
        value.map(|v| v.to_string()).unwrap_or_default()
    }

    /// The element of `role` named `name` under `of`, depth-first.
    fn find(of: &AnyObject, role: &str, name: &str) -> Option<Element> {
        for child in children(of) {
            if text(&child, "role") == role && text(&child, "title") == name {
                return Some(child);
            }
            if let Some(found) = find(&child, role, name) {
                return Some(found);
            }
        }
        None
    }

    pub fn start() {}

    /// The first question wakes the adapter; the tree comes with a later
    /// frame, and then Play is pressed through it, once.
    pub fn frame(frame: u32) {
        let view = content_view();
        if frame < 5 || PRESSED.get() {
            return;
        }
        if let Some(play) = find(&view, "AXButton", "Play") {
            // SAFETY: `accessibilityPerformPress` returns a BOOL.
            let pressed: bool = unsafe { msg_send![&*play, accessibilityPerformPress] };
            assert!(pressed, "Play takes a press");
            PRESSED.set(true);
        }
    }

    pub fn finish() {}
}

/// Windows: a UI Automation client on a thread of its own (a client must
/// not run on the window's thread, which has to keep answering), as
/// Narrator and NVDA read the window.
#[cfg(all(feature = "accessibility", target_os = "windows"))]
mod platform {
    use std::cell::RefCell;
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
    use windows::Win32::UI::Accessibility::*;
    use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
    use windows::core::{PCWSTR, w};

    const TIMEOUT: Duration = Duration::from_secs(10);
    const POLL: Duration = Duration::from_millis(20);

    thread_local! {
        static CLIENT: RefCell<Option<JoinHandle<()>>> = const { RefCell::new(None) };
    }

    /// Waits until `f` gives a value.
    fn eventually<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let start = Instant::now();
        loop {
            if let Some(value) = f() {
                return value;
            }
            assert!(start.elapsed() < TIMEOUT, "timed out waiting for {what}");
            std::thread::sleep(POLL);
        }
    }

    /// The first element under `root` (depth-first) of `control_type`
    /// named `name`.
    fn find(
        uia: &IUIAutomation,
        root: &IUIAutomationElement,
        control_type: UIA_CONTROLTYPE_ID,
        name: &str,
    ) -> Option<IUIAutomationElement> {
        // SAFETY: COM calls on interfaces this client owns.
        unsafe {
            let walker = uia.RawViewWalker().ok()?;
            let mut stack = vec![root.clone()];
            while let Some(element) = stack.pop() {
                if element.CurrentControlType().ok()? == control_type && element.CurrentName().ok()? == name {
                    return Some(element);
                }
                let mut child = walker.GetFirstChildElement(&element).ok();
                while let Some(c) = child {
                    child = walker.GetNextSiblingElement(&c).ok();
                    stack.push(c);
                }
            }
            None
        }
    }

    /// Finds the game's window by its title, then its Play button, and
    /// presses it.
    fn press_play() {
        // SAFETY: COM is set up for this thread (the multithreaded
        // apartment, which UI Automation clients may use) before the client
        // object is made; the rest are COM calls on interfaces it owns.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok().unwrap();
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).unwrap();
            // Keep in step with `window::TITLE`.
            let window = eventually("the game's window", || {
                FindWindowW(PCWSTR::null(), w!("Ponko window adapter check")).ok()
            });
            let root = uia.ElementFromHandle(window).unwrap();
            let play = eventually("the Play button", || find(&uia, &root, UIA_ButtonControlTypeId, "Play"));
            let invoke: IUIAutomationInvokePattern = play.GetCurrentPatternAs(UIA_InvokePatternId).unwrap();
            invoke.Invoke().unwrap();
        }
    }

    pub fn start() {
        let client = std::thread::spawn(press_play);
        CLIENT.with_borrow_mut(|c| *c = Some(client));
    }

    pub fn frame(_: u32) {}

    /// Joins the client, passing on its failure.
    pub fn finish() {
        if let Some(client) = CLIENT.with_borrow_mut(Option::take)
            && let Err(panic) = client.join()
        {
            std::panic::resume_unwind(panic);
        }
    }
}
