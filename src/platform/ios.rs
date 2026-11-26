//! iOS: the window's scene, the general pasteboard (the clipboard), the
//! on-screen keyboard's frame, and the audio session's category. winit
//! gives the rest: touches,
//! the safe area (as the window's inner frame), the keyboard itself
//! (`set_ime_allowed`), the app's lifecycle and memory warnings.

#[cfg(feature = "window")]
thread_local! {
    /// The game's UIWindow, until a scene takes it.
    static GAME_WINDOW: std::cell::RefCell<Option<objc2_06::rc::Retained<objc2_ui_kit::UIWindow>>> =
        const { std::cell::RefCell::new(None) };
}

/// Puts the game's window in the app's window scene. Since iOS 27's SDK
/// an app must use UIKit's scene life cycle or it isn't launched; the
/// package's Info.plist declares one scene (`UIApplicationSceneManifest`),
/// and winit 0.30 makes its window without one, so it is moved into the
/// scene: at once if the scene is connected, else when it connects.
#[cfg(feature = "window")]
pub(crate) fn adopt_scene(window: &winit::window::Window) {
    use block2::RcBlock;
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    use objc2_ui_kit::{UIApplication, UIView, UIWindowScene};
    use std::ptr::NonNull;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Some(mtm) = objc2_06::MainThreadMarker::new() else {
        return;
    };
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let RawWindowHandle::UiKit(handle) = handle.as_raw() else {
        return;
    };
    // SAFETY: winit's UiKit handle is the window's own view, alive as
    // long as the window, which the caller holds; used on the main thread.
    let view: &UIView = unsafe { handle.ui_view.cast::<UIView>().as_ref() };
    let Some(ui_window) = view.window() else {
        return;
    };
    let scene = UIApplication::sharedApplication(mtm)
        .connectedScenes()
        .anyObject()
        .and_then(|scene| scene.downcast::<UIWindowScene>().ok());
    if let Some(scene) = scene {
        place_in_scene(&ui_window, &scene);
        return;
    }
    GAME_WINDOW.with(|w| *w.borrow_mut() = Some(ui_window));
    let connects = RcBlock::new(|note: NonNull<NSNotification>| {
        // SAFETY: the center passes a live notification for the call.
        let note = unsafe { note.as_ref() };
        let Some(scene) = note.object().and_then(|o| o.downcast::<UIWindowScene>().ok()) else {
            return;
        };
        if let Some(window) = GAME_WINDOW.with(|w| w.borrow_mut().take()) {
            place_in_scene(&window, &scene);
        }
    });
    // SAFETY: a UIKit constant name, no object filter, no queue (the block
    // runs on UIKit's main thread, which posts it); the observer is kept
    // for the run, as the app has one scene for its whole life.
    unsafe {
        let observer = NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(objc2_ui_kit::UISceneWillConnectNotification),
            None,
            None,
            &connects,
        );
        std::mem::forget(observer);
    }
}

#[cfg(feature = "window")]
fn place_in_scene(window: &objc2_ui_kit::UIWindow, scene: &objc2_ui_kit::UIWindowScene) {
    if window.windowScene().is_none() {
        window.setWindowScene(Some(scene));
    }
    // Made before there was a scene, winit's window has a default size:
    // it fills the scene's screen (the view's layout then reports it).
    window.setFrame(scene.screen().bounds());
    window.makeKeyAndVisible();
    make_insets_probe(scene);
    turn_for_check(scene);
}

#[cfg(feature = "window")]
thread_local! {
    /// A window made in the game's scene, behind the game's window, that
    /// UIKit gives the scene's safe area: see [`safe_insets`].
    static INSETS_PROBE: std::cell::RefCell<Option<objc2_06::rc::Retained<objc2_ui_kit::UIWindow>>> =
        const { std::cell::RefCell::new(None) };
}

/// winit 0.30 makes its UIWindow with a frame (`initWithFrame:`), and
/// UIKit keeps a window made that way at its upright safe area for good,
/// even once it is in a scene and turned with it. A window made in the
/// scene (`initWithWindowScene:`) follows each turn, so an empty one is
/// kept under the game's window, where it is never seen and takes no
/// touches: its safe area is the game's. It needs a view controller of its
/// own to turn with the scene.
#[cfg(feature = "window")]
fn make_insets_probe(scene: &objc2_ui_kit::UIWindowScene) {
    use objc2_06::MainThreadOnly;
    use objc2_ui_kit::{UIViewController, UIWindow, UIWindowLevelNormal};
    let mtm = scene.mtm();
    let probe = UIWindow::initWithWindowScene(UIWindow::alloc(mtm), scene);
    probe.setRootViewController(Some(&UIViewController::new(mtm)));
    // SAFETY: a UIKit constant, read on the main thread.
    probe.setWindowLevel(unsafe { UIWindowLevelNormal } - 1.0);
    probe.setUserInteractionEnabled(false);
    // A hidden window doesn't turn.
    probe.setHidden(false);
    INSETS_PROBE.with(|p| *p.borrow_mut() = Some(probe));
}

/// The scene's safe area as `[top, right, bottom, left]` in points, for
/// the way the phone is turned now; `None` until the game's window is in
/// its scene. Cheap: read once a frame.
#[cfg(feature = "window")]
pub(crate) fn safe_insets() -> Option<[f32; 4]> {
    INSETS_PROBE.with(|p| {
        let probe = p.borrow();
        let i = probe.as_ref()?.safeAreaInsets();
        Some([i.top, i.right, i.bottom, i.left].map(|v| v as f32))
    })
}

/// For the simulator check (`scripts/ios-simulator.sh`), which can't turn
/// the simulator: `PONKO_IOS_ORIENTATION=landscape` (or `portrait`) asks
/// the scene to turn that way as the game starts.
#[cfg(feature = "window")]
fn turn_for_check(scene: &objc2_ui_kit::UIWindowScene) {
    use objc2_ui_kit::{UIInterfaceOrientationMask, UIWindowSceneGeometryPreferencesIOS};
    let mask = match std::env::var("PONKO_IOS_ORIENTATION").as_deref() {
        Ok("landscape") => UIInterfaceOrientationMask::Landscape,
        Ok("portrait") => UIInterfaceOrientationMask::Portrait,
        _ => return,
    };
    use objc2_06::AnyThread;
    let preferences = UIWindowSceneGeometryPreferencesIOS::initWithInterfaceOrientations(
        UIWindowSceneGeometryPreferencesIOS::alloc(),
        mask,
    );
    let failed = block2::RcBlock::new(|error: std::ptr::NonNull<objc2_foundation::NSError>| {
        // SAFETY: UIKit passes a live error for the call.
        eprintln!(
            "ponko: PONKO_IOS_ORIENTATION: {}",
            unsafe { error.as_ref() }.localizedDescription()
        );
    });
    scene.requestGeometryUpdateWithPreferences_errorHandler(&preferences, Some(&failed));
}

/// The general pasteboard's text, if it holds text.
#[cfg(feature = "window")]
pub(crate) fn pasteboard_text() -> Option<String> {
    let board = objc2_ui_kit::UIPasteboard::generalPasteboard();
    // SAFETY: `string` takes no arguments and returns an owned (retained)
    // string or nil; the pasteboard is safe to use from any thread.
    unsafe { board.string() }.map(|text| text.to_string())
}

/// Puts `text` on the general pasteboard.
#[cfg(feature = "window")]
pub(crate) fn set_pasteboard_text(text: &str) {
    let board = objc2_ui_kit::UIPasteboard::generalPasteboard();
    let text = objc2_foundation::NSString::from_str(text);
    // SAFETY: `setString:` copies the string it is given (a valid NSString).
    unsafe { board.setString(Some(&text)) };
}

#[cfg(feature = "window")]
thread_local! {
    /// The keyboard's frame when it last moved, in screen points: its top
    /// and height; `None` until it first shows, (top, 0) once hidden.
    static KEYBOARD: std::cell::Cell<Option<(f64, f64)>> = const { std::cell::Cell::new(None) };
}

/// Starts following the on-screen keyboard's frame (once; on the main
/// thread, where UIKit posts its notifications).
#[cfg(feature = "window")]
pub(crate) fn watch_keyboard() {
    use block2::RcBlock;
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    use std::ptr::NonNull;
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let center = NSNotificationCenter::defaultCenter();
        let moved = RcBlock::new(|note: NonNull<NSNotification>| {
            // SAFETY: the center passes a live notification for the call.
            let note = unsafe { note.as_ref() };
            if let Some(frame) = keyboard_end_frame(note) {
                KEYBOARD.with(|k| k.set(Some(frame)));
            }
        });
        let hidden = RcBlock::new(|_: NonNull<NSNotification>| {
            KEYBOARD.with(|k| k.set(k.get().map(|(top, _)| (top, 0.0))));
        });
        // SAFETY: the names are UIKit's own constants; no object filter and
        // no queue (the block runs on the posting thread, UIKit's main
        // one). The returned observers are kept for the whole run: the
        // app never stops following the keyboard.
        unsafe {
            let changed = center.addObserverForName_object_queue_usingBlock(
                Some(objc2_ui_kit::UIKeyboardWillChangeFrameNotification),
                None,
                None,
                &moved,
            );
            let hides = center.addObserverForName_object_queue_usingBlock(
                Some(objc2_ui_kit::UIKeyboardWillHideNotification),
                None,
                None,
                &hidden,
            );
            std::mem::forget((changed, hides));
        }
    });
}

/// The keyboard's frame (top, height) in screen points from a keyboard
/// notification's `UIKeyboardFrameEndUserInfoKey`.
#[cfg(feature = "window")]
fn keyboard_end_frame(note: &objc2_foundation::NSNotification) -> Option<(f64, f64)> {
    use objc2_06::runtime::AnyObject;
    use objc2_foundation::NSValue;
    use objc2_ui_kit::NSValueUIGeometryExtensions as _;
    let info = note.userInfo()?;
    // SAFETY: a UIKit constant, valid for the app's life.
    let key: &AnyObject = unsafe { objc2_ui_kit::UIKeyboardFrameEndUserInfoKey }.as_ref();
    let value = info.objectForKey(key)?.downcast::<NSValue>().ok()?;
    // SAFETY: UIKit documents this key's value as an NSValue of a CGRect.
    let rect = unsafe { value.CGRectValue() };
    Some((rect.origin.y, rect.size.height))
}

/// The keyboard's last frame (top, height) in screen points, if it has
/// shown since the app started.
#[cfg(feature = "window")]
pub(crate) fn keyboard_frame() -> Option<(f32, f32)> {
    KEYBOARD
        .with(|k| k.get())
        .map(|(top, height)| (top as f32, height as f32))
}

/// Sets the audio session's category: how the game's sound mixes with
/// other apps' and whether the ring/silent switch mutes it.
#[cfg(feature = "audio")]
pub(crate) fn set_audio_category(category: crate::audio::AudioSessionCategory) -> Result<(), String> {
    use crate::audio::AudioSessionCategory as C;
    use objc2_avf_audio::{
        AVAudioSession, AVAudioSessionCategoryAmbient, AVAudioSessionCategoryPlayback,
        AVAudioSessionCategorySoloAmbient,
    };
    // SAFETY: AVFoundation's constants, valid for the app's life (`None`
    // only on a system too old to have them).
    let name = unsafe {
        match category {
            C::SoloAmbient => AVAudioSessionCategorySoloAmbient,
            C::Ambient => AVAudioSessionCategoryAmbient,
            C::Playback => AVAudioSessionCategoryPlayback,
        }
    }
    .ok_or("this iOS has no such audio session category")?;
    // SAFETY: the shared session is a process-wide singleton, safe to use
    // from any thread; `setCategory:error:` takes a category constant.
    unsafe { AVAudioSession::sharedInstance().setCategory_error(name) }
        .map_err(|e| e.localizedDescription().to_string())
}

// These run on the iOS simulator, not on the Mac: `scripts/ios-simulator.sh`
// builds the library's tests for it and runs this module's in a simulated
// iPhone (`xcrun simctl spawn`). The pasteboard and the scene need an app:
// the same script checks them in the phone check.
#[cfg(all(test, feature = "window"))]
mod tests {
    use super::*;

    #[test]
    fn the_keyboard_frame_follows_its_notifications_and_hiding_keeps_only_its_top() {
        use objc2_06::rc::Retained;
        use objc2_06::runtime::AnyObject;
        use objc2_core_foundation::{CGPoint, CGRect, CGSize};
        use objc2_foundation::{NSDictionary, NSNotificationCenter, NSValue};
        use objc2_ui_kit::NSValueUIGeometryExtensions as _;
        watch_keyboard();
        watch_keyboard(); // a second call adds no second observer
        let rect = CGRect::new(CGPoint::new(0.0, 573.0), CGSize::new(402.0, 301.0));
        // SAFETY: boxes a plain CGRect.
        let frame = unsafe { NSValue::valueWithCGRect(rect) };
        // SAFETY: UIKit's constant key; a dictionary of NSString to NSValue
        // is one of AnyObject to AnyObject.
        let info: Retained<NSDictionary> = unsafe {
            let key: &objc2_foundation::NSString = objc2_ui_kit::UIKeyboardFrameEndUserInfoKey;
            let typed = NSDictionary::from_slices(&[key], &[&*frame]);
            Retained::cast_unchecked::<NSDictionary<AnyObject, AnyObject>>(typed)
        };
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: UIKit's notification names; the observers run on this
        // thread, during the post.
        unsafe {
            center.postNotificationName_object_userInfo(
                objc2_ui_kit::UIKeyboardWillChangeFrameNotification,
                None,
                Some(&info),
            );
        }
        assert_eq!(keyboard_frame(), Some((573.0, 301.0)));
        // SAFETY: as above.
        unsafe { center.postNotificationName_object(objc2_ui_kit::UIKeyboardWillHideNotification, None) };
        assert_eq!(keyboard_frame(), Some((573.0, 0.0)));
    }

    #[cfg(feature = "audio")]
    #[test]
    fn every_audio_session_category_is_accepted_by_the_system() {
        use crate::audio::AudioSessionCategory as C;
        for category in [C::Ambient, C::Playback, C::SoloAmbient] {
            assert_eq!(set_audio_category(category), Ok(()), "{category:?}");
        }
    }
}
