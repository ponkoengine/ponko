//! Steam's store-side services from C (Rust's `ponko::steam::Steam`):
//! achievements and stats, cloud saves kept in step with `ctx.storage`,
//! rich presence, the overlay's state, and the text input of Steam Deck and
//! Big Picture.
//!
//! One Steam per process, started by `ponko_steam_start`; it shares the
//! Steam client of the lockstep and lobby calls. Before the start, when
//! Steam isn't running, and in a library built without the `steam`
//! feature, every call does nothing and returns false (said once in the
//! log), so one build runs from Steam and from any other store.
// Without `steam` the arguments go unread: every call does nothing.
#![cfg_attr(not(feature = "steam"), allow(unused_variables))]

use std::ffi::c_char;

use crate::ctx::{PonkoCtx, ctx_access, out, text};
use crate::{PonkoStatus, boundary, engine_error, invalid, write_text};

/// The text input is still open, or nothing was shown.
pub const PONKO_STEAM_TEXT_NONE: u32 = 0;
/// The player confirmed the text.
pub const PONKO_STEAM_TEXT_SUBMITTED: u32 = 1;
/// The player closed the input without confirming.
pub const PONKO_STEAM_TEXT_CANCELLED: u32 = 2;

#[cfg(feature = "steam")]
mod imp {
    use ponko::steam::{Steam, SteamText, SteamTextMode};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    struct Process {
        steam: Option<Steam>,
        /// A submitted text that didn't fit the caller's buffer yet.
        text: Option<SteamText>,
    }
    static PROCESS: Mutex<Process> = Mutex::new(Process {
        steam: None,
        text: None,
    });

    fn process() -> MutexGuard<'static, Process> {
        PROCESS.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The process's Steam: absent until `start`.
    pub(super) fn steam() -> Steam {
        process().steam.get_or_insert_with(Steam::absent).clone()
    }

    pub(super) fn start(app_id: u32) -> bool {
        let steam = match crate::steam::client::get(app_id) {
            Ok(client) => Steam::with_client(&client),
            // A started Steam for another app stays; anything else is absent.
            Err(_) => match process().steam.clone() {
                Some(s) if s.is_running() => return false,
                _ => Steam::start(app_id),
            },
        };
        let running = steam.is_running();
        process().steam = Some(steam);
        running
    }

    pub(super) fn mode(m: u32) -> Option<SteamTextMode> {
        Some(match m {
            0 => SteamTextMode::SingleLine,
            1 => SteamTextMode::MultiLine,
            2 => SteamTextMode::Password,
            3 => SteamTextMode::Email,
            4 => SteamTextMode::Numeric,
            _ => return None,
        })
    }

    /// The closed text input, kept until it fits `capacity`.
    pub(super) fn take_text(fits: impl FnOnce(&str) -> bool) -> (u32, Option<String>) {
        let steam = steam();
        let mut p = process();
        if p.text.is_none() {
            p.text = steam.take_text_input();
        }
        match p.text.take() {
            None => (super::PONKO_STEAM_TEXT_NONE, None),
            Some(SteamText::Cancelled) => (super::PONKO_STEAM_TEXT_CANCELLED, None),
            Some(SteamText::Submitted(t)) => {
                if !fits(&t) {
                    p.text = Some(SteamText::Submitted(t.clone()));
                }
                (super::PONKO_STEAM_TEXT_SUBMITTED, Some(t))
            }
        }
    }
}

#[cfg(not(feature = "steam"))]
fn without_steam() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static SAID: AtomicBool = AtomicBool::new(false);
    if !SAID.swap(true, Ordering::Relaxed) {
        eprintln!("ponko: ponko_ffi was built without the `steam` feature: the Steam services do nothing");
    }
}

/// Runs `f` on the process's Steam; `default` without the feature.
macro_rules! with_steam {
    ($default:expr, |$s:ident| $body:expr) => {{
        #[cfg(feature = "steam")]
        {
            let $s = imp::steam();
            $body
        }
        #[cfg(not(feature = "steam"))]
        {
            without_steam();
            $default
        }
    }};
}

/// A NUL-terminated string, or `None` for null or not UTF-8.
unsafe fn name<'a>(p: *const c_char) -> Option<&'a str> {
    // SAFETY: forwarded caller contract.
    unsafe { text(p) }.ok()
}

/// Starts Steam's services for `app_id` (480 is Valve's test app), sharing
/// the Steam client of the lockstep and lobby calls: true when Steam runs.
/// When it doesn't (or the library was built without `steam`), the services
/// stay absent: every call does nothing and returns false. Call it once, at
/// launch.
/// ```no_run
/// let running = ponko_ffi::ponko_steam_start(480);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_steam_start(app_id: u32) -> bool {
    #[cfg(feature = "steam")]
    {
        imp::start(app_id)
    }
    #[cfg(not(feature = "steam"))]
    {
        let _ = app_id;
        without_steam();
        false
    }
}

/// Whether the services reach a running Steam.
/// ```
/// assert!(!ponko_ffi::ponko_steam_running());
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_steam_running() -> bool {
    #[cfg(feature = "steam")]
    {
        imp::steam().is_running()
    }
    #[cfg(not(feature = "steam"))]
    {
        false
    }
}

/// Runs Steam's callbacks and takes in the overlay's state and a closed
/// text input. Call it once a frame.
/// ```
/// ponko_ffi::ponko_steam_update();
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_steam_update() {
    #[cfg(feature = "steam")]
    imp::steam().update();
}

/// Unlocks the achievement `name` (its API name on the Steamworks site).
/// False when Steam is absent or refuses the name (said once per name).
/// # Safety
/// `name` NUL-terminated.
/// ```
/// assert!(!unsafe { ponko_ffi::ponko_steam_unlock_achievement(c"FIRST_WIN".as_ptr()) });
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_unlock_achievement(achievement: *const c_char) -> bool {
    // SAFETY: forwarded caller contract.
    let Some(n) = (unsafe { name(achievement) }) else {
        return false;
    };
    with_steam!(false, |s| s.unlock_achievement(n))
}

/// Locks the achievement `name` again. False when Steam is absent or refuses.
/// # Safety
/// `name` NUL-terminated.
/// ```
/// assert!(!unsafe { ponko_ffi::ponko_steam_clear_achievement(c"FIRST_WIN".as_ptr()) });
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_clear_achievement(achievement: *const c_char) -> bool {
    // SAFETY: forwarded caller contract.
    let Some(n) = (unsafe { name(achievement) }) else {
        return false;
    };
    with_steam!(false, |s| s.clear_achievement(n))
}

/// Whether the player has the achievement `name`: 1 yes, 0 no, -1 when
/// Steam is absent or doesn't know the name.
/// # Safety
/// `name` NUL-terminated.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_steam_achievement(c"FIRST_WIN".as_ptr()) }, -1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_achievement(achievement: *const c_char) -> i32 {
    // SAFETY: forwarded caller contract.
    let Some(n) = (unsafe { name(achievement) }) else {
        return -1;
    };
    with_steam!(-1, |s| s.achievement(n).map_or(-1, i32::from))
}

/// Sets the integer stat `name`; Steam keeps it after
/// `ponko_steam_store_stats`. False when Steam is absent or refuses.
/// # Safety
/// `name` NUL-terminated.
/// ```
/// assert!(!unsafe { ponko_ffi::ponko_steam_set_stat_i32(c"wins".as_ptr(), 1) });
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_set_stat_i32(stat: *const c_char, value: i32) -> bool {
    // SAFETY: forwarded caller contract.
    let Some(n) = (unsafe { name(stat) }) else { return false };
    with_steam!(false, |s| s.set_stat_i32(n, value))
}

/// Sets the float stat `name`, as `ponko_steam_set_stat_i32`.
/// # Safety
/// `name` NUL-terminated.
/// ```
/// assert!(!unsafe { ponko_ffi::ponko_steam_set_stat_f32(c"distance".as_ptr(), 1.5) });
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_set_stat_f32(stat: *const c_char, value: f32) -> bool {
    // SAFETY: forwarded caller contract.
    let Some(n) = (unsafe { name(stat) }) else { return false };
    with_steam!(false, |s| s.set_stat_f32(n, value))
}

/// Reads the integer stat `name` into `out_value`: false (and `out_value`
/// untouched) when Steam is absent or doesn't know it.
/// # Safety
/// `name` NUL-terminated; `out_value` writable.
/// ```
/// let mut v = 0;
/// assert!(!unsafe { ponko_ffi::ponko_steam_stat_i32(c"wins".as_ptr(), &mut v) });
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_stat_i32(stat: *const c_char, out_value: *mut i32) -> bool {
    // SAFETY: forwarded caller contract.
    let Some(n) = (unsafe { name(stat) }) else { return false };
    let value: Option<i32> = with_steam!(None, |s| s.stat_i32(n));
    // SAFETY: forwarded caller contract.
    value.is_some_and(|v| unsafe { out(out_value, v) }.is_ok())
}

/// Reads the float stat `name`, as `ponko_steam_stat_i32`.
/// # Safety
/// `name` NUL-terminated; `out_value` writable.
/// ```
/// let mut v = 0.0;
/// assert!(!unsafe { ponko_ffi::ponko_steam_stat_f32(c"distance".as_ptr(), &mut v) });
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_stat_f32(stat: *const c_char, out_value: *mut f32) -> bool {
    // SAFETY: forwarded caller contract.
    let Some(n) = (unsafe { name(stat) }) else { return false };
    let value: Option<f32> = with_steam!(None, |s| s.stat_f32(n));
    // SAFETY: forwarded caller contract.
    value.is_some_and(|v| unsafe { out(out_value, v) }.is_ok())
}

/// Sends the changed achievements and stats to Steam's servers. Call it at
/// moments that matter, not every frame. False when Steam is absent or refuses.
/// ```
/// assert!(!ponko_ffi::ponko_steam_store_stats());
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_steam_store_stats() -> bool {
    with_steam!(false, |s| s.store_stats())
}

/// Sets the rich presence `key` to `value` (what friends see; `"status"`,
/// or `"steam_display"` naming a localized string). An empty `value`
/// removes the key. False when Steam is absent or refuses.
/// # Safety
/// `key` and `value` NUL-terminated.
/// ```
/// assert!(!unsafe { ponko_ffi::ponko_steam_set_rich_presence(c"status".as_ptr(), c"Menu".as_ptr()) });
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_set_rich_presence(key: *const c_char, value: *const c_char) -> bool {
    // SAFETY: forwarded caller contract.
    let (Some(k), Some(v)) = (unsafe { name(key) }, unsafe { name(value) }) else {
        return false;
    };
    with_steam!(false, |s| s.set_rich_presence(k, v))
}

/// Removes every rich presence key.
/// ```
/// ponko_ffi::ponko_steam_clear_rich_presence();
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_steam_clear_rich_presence() {
    with_steam!((), |s| s.clear_rich_presence());
}

/// Whether the Steam overlay is open, as of the last `ponko_steam_update`:
/// pause the game while it is.
/// ```
/// assert!(!ponko_ffi::ponko_steam_overlay_open());
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_steam_overlay_open() -> bool {
    #[cfg(feature = "steam")]
    {
        imp::steam().overlay_open()
    }
    #[cfg(not(feature = "steam"))]
    {
        false
    }
}

/// Whether the game runs on a Steam Deck.
/// ```
/// assert!(!ponko_ffi::ponko_steam_is_steam_deck());
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_steam_is_steam_deck() -> bool {
    #[cfg(feature = "steam")]
    {
        imp::steam().is_steam_deck()
    }
    #[cfg(not(feature = "steam"))]
    {
        false
    }
}

/// Whether Steam runs in Big Picture mode.
/// ```
/// assert!(!ponko_ffi::ponko_steam_is_big_picture());
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_steam_is_big_picture() -> bool {
    #[cfg(feature = "steam")]
    {
        imp::steam().is_big_picture()
    }
    #[cfg(not(feature = "steam"))]
    {
        false
    }
}

/// Opens Steam's full-screen gamepad text input (Big Picture and Steam
/// Deck only) with `description` above it and `existing` already typed,
/// for at most `max_chars` characters. `mode`: 0 one line, 1 several
/// lines, 2 password, 3 email, 4 digits. The result arrives through
/// `ponko_steam_take_text_input` after the player closes it. False when it
/// didn't open: show the game's own keyboard then.
/// # Safety
/// `description` and `existing` NUL-terminated.
/// ```
/// assert!(!unsafe { ponko_ffi::ponko_steam_show_text_input(0, c"Name".as_ptr(), c"".as_ptr(), 16) });
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_show_text_input(
    mode: u32,
    description: *const c_char,
    existing: *const c_char,
    max_chars: u32,
) -> bool {
    // SAFETY: forwarded caller contract.
    let (Some(d), Some(e)) = (unsafe { name(description) }, unsafe { name(existing) }) else {
        return false;
    };
    #[cfg(feature = "steam")]
    {
        imp::mode(mode).is_some_and(|m| imp::steam().show_text_input(m, d, e, max_chars))
    }
    #[cfg(not(feature = "steam"))]
    {
        let _ = (mode, d, e, max_chars);
        without_steam();
        false
    }
}

/// How the player closed the text input of `ponko_steam_show_text_input`:
/// `*out_state` is `PONKO_STEAM_TEXT_NONE` (still open, or nothing shown),
/// `PONKO_STEAM_TEXT_SUBMITTED` or `PONKO_STEAM_TEXT_CANCELLED`, once. A
/// submitted text is written to `buffer` with its length in `*out_len`;
/// when it doesn't fit `capacity` (with its NUL), the call fails, keeps the
/// text, and `*out_len` says how much room it needs.
/// # Safety
/// `out_state` and `out_len` writable; `buffer` writable for `capacity`
/// bytes, or null with capacity 0.
/// ```
/// let (mut state, mut len) = (9, 0);
/// let s = unsafe { ponko_ffi::ponko_steam_take_text_input(&mut state, std::ptr::null_mut(), 0, &mut len) };
/// assert_eq!((s as u32, state), (0, 0));
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_steam_take_text_input(
    out_state: *mut u32,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    boundary(|| {
        if out_state.is_null() || out_len.is_null() || (buffer.is_null() && capacity != 0) {
            return Err(invalid("output is null"));
        }
        #[cfg(feature = "steam")]
        let (state, submitted) = imp::take_text(|t| t.len() < capacity);
        #[cfg(not(feature = "steam"))]
        let (state, submitted) = (PONKO_STEAM_TEXT_NONE, None::<String>);
        // SAFETY: forwarded caller contract (checked non-null).
        unsafe {
            out(out_state, state)?;
            write_text(buffer, capacity, out_len, submitted.as_deref().unwrap_or(""))?;
        }
        match submitted {
            Some(t) if t.len() >= capacity => Err(invalid("the text needs a bigger buffer: *out_len + 1 bytes")),
            _ => Ok(()),
        }
    })
}

/// Opens the Steam Deck's floating keyboard, placed so it doesn't cover the
/// text field at `x, y` (pixels from the window's top left) of `width` by
/// `height`; its keys arrive as ordinary typed text. `mode` as in
/// `ponko_steam_show_text_input`. False when it didn't open (not on a Deck).
/// ```
/// assert!(!ponko_ffi::ponko_steam_show_floating_keyboard(4, 0, 0, 200, 30));
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_steam_show_floating_keyboard(mode: u32, x: i32, y: i32, width: i32, height: i32) -> bool {
    #[cfg(feature = "steam")]
    {
        imp::mode(mode).is_some_and(|m| imp::steam().show_floating_keyboard(m, x, y, width, height))
    }
    #[cfg(not(feature = "steam"))]
    {
        let _ = (mode, x, y, width, height);
        without_steam();
        false
    }
}

/// Whether Steam Cloud keeps this player's files: Steam runs and neither
/// the player nor the app turned it off.
/// ```
/// assert!(!ponko_ffi::ponko_steam_cloud_enabled());
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn ponko_steam_cloud_enabled() -> bool {
    with_steam!(false, |s| s.cloud_enabled())
}

/// Brings Steam Cloud and `ctx.storage` in step for the `count` entries of
/// `files`: file names, and folders ending in `/` for every file below
/// them. A file on one side only is copied to the other; one that differs
/// is copied from the side written last. Call it at launch and after
/// saving. Without Steam, or with its cloud off, it does nothing (the
/// counts are 0). `out_downloaded` and `out_uploaded` (each may be null)
/// get how many files were copied each way.
/// # Safety
/// Live ctx; `files` holds `count` NUL-terminated strings (null with count
/// 0); the outputs null or writable.
/// ```
/// let files = [c"saves/".as_ptr()];
/// let s = unsafe { ponko_ffi::ponko_ctx_steam_sync_cloud(std::ptr::null_mut(), files.as_ptr(), 1, std::ptr::null_mut(), std::ptr::null_mut()) };
/// assert_eq!(s as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_steam_sync_cloud(
    ctx: *mut PonkoCtx,
    files: *const *const c_char,
    count: usize,
    out_downloaded: *mut u32,
    out_uploaded: *mut u32,
) -> PonkoStatus {
    boundary(|| {
        if files.is_null() && count != 0 {
            return Err(invalid("files is null"));
        }
        let mut names = Vec::with_capacity(count);
        for i in 0..count {
            // SAFETY: forwarded caller contract: `count` readable pointers.
            names.push(unsafe { text(*files.add(i)) }?);
        }
        // SAFETY: forwarded caller contract.
        let (down, up) = unsafe {
            ctx_access(ctx, |c| {
                #[cfg(feature = "steam")]
                {
                    let done = imp::steam().sync_cloud(&mut c.storage, &names).map_err(engine_error)?;
                    Ok((done.downloaded.len(), done.uploaded.len()))
                }
                #[cfg(not(feature = "steam"))]
                {
                    let _ = (c, &names);
                    without_steam();
                    Ok((0, 0))
                }
            })
        }?;
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        // SAFETY: forwarded caller contract.
        unsafe {
            if !out_downloaded.is_null() {
                out_downloaded.write(count(down));
            }
            if !out_uploaded.is_null() {
                out_uploaded.write(count(up));
            }
        }
        Ok(())
    })
}

/// Deletes the stored file `name` and its Steam Cloud copy, so the next
/// sync doesn't bring it back. Without Steam it deletes the local file only.
/// # Safety
/// Live ctx; `name` NUL-terminated.
/// ```
/// let s = unsafe { ponko_ffi::ponko_ctx_steam_delete_cloud_file(std::ptr::null_mut(), c"saves/a.json".as_ptr()) };
/// assert_eq!(s as u32, 1);
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_steam_delete_cloud_file(ctx: *mut PonkoCtx, file: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let file = text(file)?;
        ctx_access(ctx, |c| {
            #[cfg(feature = "steam")]
            {
                imp::steam()
                    .delete_cloud_file(&mut c.storage, file)
                    .map_err(engine_error)
            }
            #[cfg(not(feature = "steam"))]
            {
                c.storage.delete(file).map_err(engine_error)
            }
        })
    })
}
