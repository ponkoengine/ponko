//! Android: the app's entry point, its output in logcat, its folders, the
//! asset pack inside the APK, and what the NDK has no C API for, through
//! JNI: the window's insets (safe area and on-screen keyboard), the
//! clipboard, audio focus and the phone's call state, and leaving the app
//! on Back. winit (NativeActivity, through android-activity) gives the
//! rest: the window, touches, keys, the keyboard itself
//! (`set_ime_allowed`), the surface's life cycle and memory warnings.
//!
//! # How a game starts
//!
//! A package is the game's program linked as a shared library (`ponko
//! package --target android` does it). Android's NativeActivity loads it,
//! android-activity starts [`android_main`] on a thread of its own, and
//! that calls the program's own C `main`, which Rust made for its `fn main`:
//! the game's code is the same as on a desktop.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::{Mutex, OnceLock};

use jni::objects::JObject;
use jni::{JValue, jni_sig, jni_str};
use winit::platform::android::activity::AndroidApp;

static APP: OnceLock<AndroidApp> = OnceLock::new();

/// The running app, from [`android_main`].
pub(crate) fn app() -> Option<&'static AndroidApp> {
    APP.get()
}

/// Called by android-activity on its own thread once the activity exists:
/// sets up the process and runs the game's `main`. The process ends with
/// it, so a later start of the app is a fresh process (winit's event loop
/// can be made once per process).
#[unsafe(no_mangle)]
pub fn android_main(app: AndroidApp) {
    send_output_to_logcat();
    if APP.set(app.clone()).is_err() {
        eprintln!("ponko: the activity started a second time in one process; ending it");
        std::process::exit(0);
    }
    use_app_folders(&app);
    if debuggable() {
        environment_from_intent();
    }
    let code = run_main();
    std::process::exit(code);
}

/// Runs the C `main` of the shared library this code is in (the game's
/// program, linked as one): Rust's runtime starts as in any program, and
/// calls the game's `fn main`.
fn run_main() -> c_int {
    // SAFETY: `dladdr` reads the loader's records for an address in this
    // library; `dlopen` with RTLD_NOLOAD only looks the loaded library up
    // (it is loaded: this code runs from it); `dlsym` returns its `main`,
    // which rustc made with C's signature `int main(int, char **)`. argv
    // is a null-terminated list of NUL-terminated strings that lives
    // through the call.
    unsafe {
        let mut info: libc::Dl_info = std::mem::zeroed();
        if libc::dladdr(android_main as *const c_void, &mut info) == 0 || info.dli_fname.is_null() {
            eprintln!("ponko: can't find the game's library");
            return 1;
        }
        let library = libc::dlopen(info.dli_fname, libc::RTLD_NOW | libc::RTLD_NOLOAD);
        let main = if library.is_null() {
            std::ptr::null_mut()
        } else {
            libc::dlsym(library, c"main".as_ptr())
        };
        if main.is_null() {
            eprintln!(
                "ponko: {} has no `main`: build the game's program as a shared library with `ponko package --target android`",
                CStr::from_ptr(info.dli_fname).to_string_lossy()
            );
            return 1;
        }
        let main: extern "C" fn(c_int, *const *const c_char) -> c_int = std::mem::transmute(main);
        let argv = [c"game".as_ptr(), std::ptr::null()];
        main(1, argv.as_ptr())
    }
}

/// Standard output and error go to logcat (tag `ponko`), a line per
/// message: an app's own go nowhere. `adb logcat -s ponko` shows them.
fn send_output_to_logcat() {
    use std::io::BufRead;
    use std::os::fd::FromRawFd;
    let mut fds = [0 as c_int; 2];
    // SAFETY: `pipe` fills two new descriptors; `dup2` points 1 and 2 at
    // the write end (the original is closed after), and the read end is
    // owned by the File from then on.
    let read = unsafe {
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return;
        }
        libc::dup2(fds[1], 1);
        libc::dup2(fds[1], 2);
        libc::close(fds[1]);
        std::fs::File::from_raw_fd(fds[0])
    };
    let spawned = std::thread::Builder::new().name("ponko-logcat".into()).spawn(move || {
        for line in std::io::BufReader::new(read).split(b'\n') {
            let Ok(line) = line else { break };
            let line: Vec<u8> = line.into_iter().map(|b| if b == 0 { b'?' } else { b }).collect();
            let Ok(text) = std::ffi::CString::new(line) else {
                continue;
            };
            // SAFETY: both are NUL-terminated strings that outlive the call.
            unsafe {
                ndk_sys::__android_log_write(
                    ndk_sys::android_LogPriority::ANDROID_LOG_INFO.0 as c_int,
                    c"ponko".as_ptr(),
                    text.as_ptr(),
                );
            }
        }
    });
    drop(spawned);
}

use winit::platform::android::activity::ndk_sys;

/// The app's own folders: `HOME` is its internal files folder (where
/// [`Storage`](crate::storage::Storage) keeps a game's files, private to
/// the app and backed up with it) and `TMPDIR` its cache folder.
fn use_app_folders(app: &AndroidApp) {
    let Some(files) = app.internal_data_path() else {
        return;
    };
    let cache = files.parent().map(|p| p.join("cache"));
    // SAFETY: set before the game's `main` runs, while no other thread of
    // ours reads the environment (the logcat thread doesn't).
    unsafe {
        std::env::set_var("HOME", &files);
        if let Some(cache) = cache.filter(|c| std::fs::create_dir_all(c).is_ok()) {
            std::env::set_var("TMPDIR", cache);
        }
    }
}

/// Runs `f` with the JNI environment and the activity, in a local frame
/// (its local references are freed after). A Java exception becomes an
/// error, which is logged once per place; `None` then.
fn with_activity<R>(
    what: &str,
    f: impl for<'local> FnOnce(&mut jni::Env<'local>, &JObject<'local>) -> jni::errors::Result<R>,
) -> Option<R> {
    let app = app()?;
    // SAFETY: android-activity's pointers: the process's Java VM, and a
    // global reference to the activity it keeps for the app's life.
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) };
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<R> {
        env.with_local_frame(32, |env| {
            // SAFETY: the activity's global reference (see above), valid
            // in any frame; this JObject never deletes it.
            let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr().cast()) };
            f(env, &activity)
        })
    });
    match result {
        Ok(value) => Some(value),
        Err(e) => {
            eprintln!("ponko: Android {what} failed: {e}");
            None
        }
    }
}

/// `context.getSystemService(name)`.
fn system_service<'local>(
    env: &mut jni::Env<'local>,
    activity: &JObject<'local>,
    name: &str,
) -> jni::errors::Result<JObject<'local>> {
    let name = env.new_string(name)?;
    env.call_method(
        activity,
        jni_str!("getSystemService"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
        &[JValue::Object(&name)],
    )?
    .l()
}

/// Whether the app is debuggable (`android:debuggable`, as a package
/// signed with the debug key is).
fn debuggable() -> bool {
    with_activity("debuggable check", |env, activity| {
        let info = env
            .call_method(
                activity,
                jni_str!("getApplicationInfo"),
                jni_sig!("()Landroid/content/pm/ApplicationInfo;"),
                &[],
            )?
            .l()?;
        let flags = env.get_field(&info, jni_str!("flags"), jni_sig!("I"))?.i()?;
        // ApplicationInfo.FLAG_DEBUGGABLE
        Ok(flags & 2 != 0)
    })
    .unwrap_or(false)
}

/// A debuggable app's `PONKO_*`, `WGPU_*` and `RUST_*` intent extras become environment
/// variables, so checks set what a desktop sets in the environment:
/// `adb shell am start -n <id>/android.app.NativeActivity -e PONKO_SMOKE 120`.
/// Never in a release package: another app could start it with them.
fn environment_from_intent() {
    let pairs = with_activity("intent extras", |env, activity| {
        let mut pairs = Vec::new();
        let intent = env
            .call_method(
                activity,
                jni_str!("getIntent"),
                jni_sig!("()Landroid/content/Intent;"),
                &[],
            )?
            .l()?;
        if intent.is_null() {
            return Ok(pairs);
        }
        let extras = env
            .call_method(&intent, jni_str!("getExtras"), jni_sig!("()Landroid/os/Bundle;"), &[])?
            .l()?;
        if extras.is_null() {
            return Ok(pairs);
        }
        let keys = env
            .call_method(&extras, jni_str!("keySet"), jni_sig!("()Ljava/util/Set;"), &[])?
            .l()?;
        let keys = env
            .call_method(&keys, jni_str!("toArray"), jni_sig!("()[Ljava/lang/Object;"), &[])?
            .l()?;
        let keys = env.cast_local::<jni::objects::JObjectArray>(keys)?;
        for i in 0..keys.len(env)? {
            let key = keys.get_element(env, i)?;
            let key = env.cast_local::<jni::objects::JString>(key)?;
            let name = key.try_to_string(env)?;
            if !["PONKO_", "WGPU_", "RUST_"].iter().any(|p| name.starts_with(p)) {
                continue;
            }
            let value = env
                .call_method(
                    &extras,
                    jni_str!("get"),
                    jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
                    &[JValue::Object(&key)],
                )?
                .l()?;
            if value.is_null() {
                continue;
            }
            let text = env
                .call_method(&value, jni_str!("toString"), jni_sig!("()Ljava/lang/String;"), &[])?
                .l()?;
            let text = env.cast_local::<jni::objects::JString>(text)?;
            pairs.push((name, text.try_to_string(env)?));
        }
        Ok(pairs)
    })
    .unwrap_or_default();
    for (name, value) in pairs {
        // SAFETY: as in `use_app_folders`: before the game's `main`.
        unsafe { std::env::set_var(name, value) };
    }
}

/// A file of the APK's `assets/` folder stored uncompressed: the APK's
/// descriptor, and where in it the file starts and how long it is.
pub(crate) fn apk_asset(name: &CStr) -> Option<(std::fs::File, u64, u64)> {
    let asset = app()?.asset_manager().open(name)?;
    let opened = asset.open_file_descriptor().ok()?;
    Some((std::fs::File::from(opened.fd), opened.offset as u64, opened.size as u64))
}

/// The `WindowInsets.Type` masks: system bars and the display cutout
/// (what the safe area leaves out), and the on-screen keyboard.
fn inset_types(env: &mut jni::Env<'_>) -> jni::errors::Result<(i32, i32)> {
    static TYPES: OnceLock<(i32, i32)> = OnceLock::new();
    if let Some(types) = TYPES.get() {
        return Ok(*types);
    }
    let class = jni_str!("android/view/WindowInsets$Type");
    let mask = |env: &mut jni::Env<'_>, name| -> jni::errors::Result<i32> {
        env.call_static_method(class, name, jni_sig!("()I"), &[])?.i()
    };
    let safe = mask(env, jni_str!("systemBars"))? | mask(env, jni_str!("displayCutout"))?;
    let keyboard = mask(env, jni_str!("ime"))?;
    Ok(*TYPES.get_or_init(|| (safe, keyboard)))
}

/// The window's insets in physical pixels: what system bars and the
/// display cutout cover from each edge (top, right, bottom, left), and how
/// far up the on-screen keyboard covers it (0 while hidden). `None` before
/// the window is laid out.
pub(crate) fn window_insets() -> Option<([i32; 4], i32)> {
    with_activity("window insets", |env, activity| {
        let window = env
            .call_method(
                activity,
                jni_str!("getWindow"),
                jni_sig!("()Landroid/view/Window;"),
                &[],
            )?
            .l()?;
        let decor = env
            .call_method(
                &window,
                jni_str!("getDecorView"),
                jni_sig!("()Landroid/view/View;"),
                &[],
            )?
            .l()?;
        let insets = env
            .call_method(
                &decor,
                jni_str!("getRootWindowInsets"),
                jni_sig!("()Landroid/view/WindowInsets;"),
                &[],
            )?
            .l()?;
        if insets.is_null() {
            return Ok(None);
        }
        let (safe, keyboard) = inset_types(env)?;
        let read = |env: &mut jni::Env<'_>, mask: i32| -> jni::errors::Result<[i32; 4]> {
            let edges = env
                .call_method(
                    &insets,
                    jni_str!("getInsets"),
                    jni_sig!("(I)Landroid/graphics/Insets;"),
                    &[JValue::Int(mask)],
                )?
                .l()?;
            let mut field = |name| env.get_field(&edges, name, jni_sig!("I")).and_then(|v| v.i());
            Ok([
                field(jni_str!("top"))?,
                field(jni_str!("right"))?,
                field(jni_str!("bottom"))?,
                field(jni_str!("left"))?,
            ])
        };
        let safe = read(env, safe)?;
        let keyboard = read(env, keyboard)?[2];
        Ok(Some((safe, keyboard)))
    })
    .flatten()
}

/// The clipboard's text, if it holds something that reads as text.
pub(crate) fn clipboard_text() -> Option<Option<String>> {
    with_activity("clipboard read", |env, activity| {
        let manager = system_service(env, activity, "clipboard")?;
        let clip = env
            .call_method(
                &manager,
                jni_str!("getPrimaryClip"),
                jni_sig!("()Landroid/content/ClipData;"),
                &[],
            )?
            .l()?;
        if clip.is_null()
            || env
                .call_method(&clip, jni_str!("getItemCount"), jni_sig!("()I"), &[])?
                .i()?
                == 0
        {
            return Ok(None);
        }
        let item = env
            .call_method(
                &clip,
                jni_str!("getItemAt"),
                jni_sig!("(I)Landroid/content/ClipData$Item;"),
                &[JValue::Int(0)],
            )?
            .l()?;
        let text = env
            .call_method(
                &item,
                jni_str!("coerceToText"),
                jni_sig!("(Landroid/content/Context;)Ljava/lang/CharSequence;"),
                &[JValue::Object(activity)],
            )?
            .l()?;
        if text.is_null() {
            return Ok(None);
        }
        let text = env
            .call_method(&text, jni_str!("toString"), jni_sig!("()Ljava/lang/String;"), &[])?
            .l()?;
        let text = env.cast_local::<jni::objects::JString>(text)?;
        Ok(Some(text.try_to_string(env)?))
    })
}

/// Puts `text` on the clipboard. Whether it worked.
pub(crate) fn set_clipboard_text(text: &str) -> bool {
    with_activity("clipboard write", |env, activity| {
        let manager = system_service(env, activity, "clipboard")?;
        let label = env.new_string("text")?;
        let text = env.new_string(text)?;
        let clip = env
            .call_static_method(
                jni_str!("android/content/ClipData"),
                jni_str!("newPlainText"),
                jni_sig!("(Ljava/lang/CharSequence;Ljava/lang/CharSequence;)Landroid/content/ClipData;"),
                &[JValue::Object(&label), JValue::Object(&text)],
            )?
            .l()?;
        env.call_method(
            &manager,
            jni_str!("setPrimaryClip"),
            jni_sig!("(Landroid/content/ClipData;)V"),
            &[JValue::Object(&clip)],
        )?;
        Ok(())
    })
    .is_some()
}

/// Leaves the app as Back does by default on Android 12 and later: its
/// task goes to the background, the activity stays (and is resumed when
/// the player comes back).
pub(crate) fn leave() {
    with_activity("leaving on Back", |env, activity| {
        env.call_method(
            activity,
            jni_str!("moveTaskToBack"),
            jni_sig!("(Z)Z"),
            &[JValue::Bool(true)],
        )?
        .z()
    });
}

/// The audio focus request the app holds, to give it back with.
static FOCUS: Mutex<Option<jni::refs::Global<JObject<'static>>>> = Mutex::new(None);

/// Asks for audio focus (`AUDIOFOCUS_GAIN` for a game's sound: other apps'
/// music pauses), or (`false`) gives it back. Whether the system granted it.
pub(crate) fn audio_focus(want: bool) -> bool {
    let mut held = FOCUS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if want == held.is_some() {
        return want;
    }
    with_activity("audio focus", |env, activity| {
        let manager = system_service(env, activity, "audio")?;
        if let Some(request) = held.take() {
            env.call_method(
                &manager,
                jni_str!("abandonAudioFocusRequest"),
                jni_sig!("(Landroid/media/AudioFocusRequest;)I"),
                &[JValue::Object(request.as_obj())],
            )?;
            return Ok(false);
        }
        let attributes = env.new_object(jni_str!("android/media/AudioAttributes$Builder"), jni_sig!("()V"), &[])?;
        // AudioAttributes.USAGE_GAME, CONTENT_TYPE_SONIFICATION
        let attributes = env
            .call_method(
                &attributes,
                jni_str!("setUsage"),
                jni_sig!("(I)Landroid/media/AudioAttributes$Builder;"),
                &[JValue::Int(14)],
            )?
            .l()?;
        let attributes = env
            .call_method(
                &attributes,
                jni_str!("setContentType"),
                jni_sig!("(I)Landroid/media/AudioAttributes$Builder;"),
                &[JValue::Int(4)],
            )?
            .l()?;
        let attributes = env
            .call_method(
                &attributes,
                jni_str!("build"),
                jni_sig!("()Landroid/media/AudioAttributes;"),
                &[],
            )?
            .l()?;
        // AudioManager.AUDIOFOCUS_GAIN
        let builder = env.new_object(
            jni_str!("android/media/AudioFocusRequest$Builder"),
            jni_sig!("(I)V"),
            &[JValue::Int(1)],
        )?;
        let builder = env
            .call_method(
                &builder,
                jni_str!("setAudioAttributes"),
                jni_sig!("(Landroid/media/AudioAttributes;)Landroid/media/AudioFocusRequest$Builder;"),
                &[JValue::Object(&attributes)],
            )?
            .l()?;
        let request = env
            .call_method(
                &builder,
                jni_str!("build"),
                jni_sig!("()Landroid/media/AudioFocusRequest;"),
                &[],
            )?
            .l()?;
        let granted = env
            .call_method(
                &manager,
                jni_str!("requestAudioFocus"),
                jni_sig!("(Landroid/media/AudioFocusRequest;)I"),
                &[JValue::Object(&request)],
            )?
            .i()?;
        // AUDIOFOCUS_REQUEST_GRANTED
        if granted == 1 {
            *held = Some(env.new_global_ref(&request)?);
        }
        Ok(granted == 1)
    })
    .unwrap_or(false)
}

/// Whether a phone call rings or is on (`AudioManager.getMode()` is not
/// `MODE_NORMAL`): the game's sound waits until it ends.
pub(crate) fn in_call() -> bool {
    with_activity("call state", |env, activity| {
        let manager = system_service(env, activity, "audio")?;
        Ok(env
            .call_method(&manager, jni_str!("getMode"), jni_sig!("()I"), &[])?
            .i()?
            != 0)
    })
    .unwrap_or(false)
}
