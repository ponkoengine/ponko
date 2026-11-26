//! iOS: the general pasteboard (the clipboard), the on-screen keyboard's
//! frame, and the audio session's category. winit gives the rest: touches,
//! the safe area (as the window's inner frame), the keyboard itself
//! (`set_ime_allowed`), the app's lifecycle and memory warnings.

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
    KEYBOARD.with(|k| k.get()).map(|(top, height)| (top as f32, height as f32))
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
    unsafe { AVAudioSession::sharedInstance().setCategory_error(name) }.map_err(|e| e.localizedDescription().to_string())
}
