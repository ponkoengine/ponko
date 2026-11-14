//! Localization (#411): the C side of Rust's `ponko::locale` (string tables,
//! arguments, plural rules, the current language, fonts per language, the
//! missing-key report) and `Ui::set_text_key`.
use std::ffi::c_char;

use ponko::locale::{Arg, PluralCategory, StringTable, Strings};
use ponko::prelude::Ctx;

use crate::ctx::{ctx_access, out, text};
use crate::media::{PonkoFont, font_from_c};
use crate::ui_retained::{existing, with_ui};
use crate::{Failure, PonkoCtx, PonkoStatus, boundary, error, invalid, invalid_owned, write_text};

/// What a `PonkoStringArg` holds.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoStringArgKind {
    /// `text`, inserted as it is.
    PONKO_STRING_ARG_TEXT = 0,
    /// `integer`, a whole number.
    PONKO_STRING_ARG_INT = 1,
    /// `number`, with as many decimals as the placeholder asks for.
    PONKO_STRING_ARG_NUMBER = 2,
}

/// One argument of `ponko_ctx_strings_format` and `ponko_ctx_ui_set_text_key`
/// (Rust's `(name, Arg)`): the placeholder `{name}` it fills, and its value.
/// An argument named `count` picks a plural form.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PonkoStringArg {
    /// The placeholder's name, NUL-terminated.
    pub name: *const c_char,
    /// Which of the values below is used.
    pub kind: PonkoStringArgKind,
    /// For `PONKO_STRING_ARG_TEXT`: NUL-terminated (null is empty).
    pub text: *const c_char,
    /// For `PONKO_STRING_ARG_INT`.
    pub integer: i64,
    /// For `PONKO_STRING_ARG_NUMBER`.
    pub number: f64,
}

/// The form of a word that goes with a number (Rust's `PluralCategory`).
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(non_camel_case_types)]
pub enum PonkoPluralCategory {
    /// Arabic's 0.
    PONKO_PLURAL_ZERO = 0,
    /// The singular.
    PONKO_PLURAL_ONE = 1,
    /// The dual.
    PONKO_PLURAL_TWO = 2,
    /// A few (Russian 2-4).
    PONKO_PLURAL_FEW = 3,
    /// Many (Russian 5-20).
    PONKO_PLURAL_MANY = 4,
    /// Everything else.
    PONKO_PLURAL_OTHER = 5,
}

impl From<PluralCategory> for PonkoPluralCategory {
    fn from(c: PluralCategory) -> Self {
        match c {
            PluralCategory::Zero => PonkoPluralCategory::PONKO_PLURAL_ZERO,
            PluralCategory::One => PonkoPluralCategory::PONKO_PLURAL_ONE,
            PluralCategory::Two => PonkoPluralCategory::PONKO_PLURAL_TWO,
            PluralCategory::Few => PonkoPluralCategory::PONKO_PLURAL_FEW,
            PluralCategory::Many => PonkoPluralCategory::PONKO_PLURAL_MANY,
            PluralCategory::Other => PonkoPluralCategory::PONKO_PLURAL_OTHER,
        }
    }
}

/// Reads `count` arguments.
///
/// # Safety
/// `args` readable for `count` arguments (or null with 0), each with valid strings.
unsafe fn args_from_c<'a>(args: *const PonkoStringArg, count: usize) -> Result<Vec<(&'a str, Arg)>, Failure> {
    if count == 0 {
        return Ok(Vec::new());
    }
    if args.is_null() {
        return Err(invalid("args is null"));
    }
    // SAFETY: caller contract.
    let list = unsafe { std::slice::from_raw_parts(args, count) };
    list.iter()
        .map(|a| {
            // SAFETY: caller contract for the strings.
            let name = unsafe { text(a.name)? };
            let value = match a.kind {
                PonkoStringArgKind::PONKO_STRING_ARG_TEXT => {
                    // SAFETY: caller contract.
                    Arg::from(unsafe { crate::ui_retained::optional_text(a.text)? })
                }
                PonkoStringArgKind::PONKO_STRING_ARG_INT => Arg::Int(a.integer),
                PonkoStringArgKind::PONKO_STRING_ARG_NUMBER => Arg::Number(a.number),
            };
            Ok((name, value))
        })
        .collect()
}

/// The game's `Strings`, or `PONKO_NOT_FOUND` before a table was loaded.
fn strings(c: &mut Ctx) -> Result<&mut Strings, Failure> {
    c.world.get_resource_mut::<Strings>().ok_or_else(|| {
        error(
            PonkoStatus::PONKO_NOT_FOUND,
            "no string tables: load one with ponko_ctx_load_strings",
        )
    })
}

/// Loads a string table file from the assets (Rust's `assets.load_strings`;
/// see the guide's "Localization" for the format). The first table is the
/// reference language and the current one; with hot reload on, an edited
/// file replaces its table.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_load_strings(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `path` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_load_strings(ctx: *mut PonkoCtx, path: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let path = text(path)?;
        ctx_access(ctx, |c| {
            c.assets.load_strings(&mut c.world, path).map_err(crate::engine_error)
        })
    })
}

/// Adds a string table from its JSON text, or replaces the table of the same
/// language (Rust's `strings.insert(StringTable::from_json(json)?)`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_strings_insert_json(std::ptr::null_mut(), std::ptr::null()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `json` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_strings_insert_json(ctx: *mut PonkoCtx, json: *const c_char) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let json = text(json)?;
        let table = StringTable::from_json(json).map_err(invalid_owned)?;
        ctx_access(ctx, |c| {
            if c.world.get_resource::<Strings>().is_none() {
                c.world.insert_resource(Strings::default());
            }
            c.world.resource_mut::<Strings>().insert(table);
            Ok(())
        })
    })
}

/// Switches to `language` (a code with a table, or `"en-XA"`, the
/// pseudo-locale) and writes whether it could to `out_known`; an unknown
/// code keeps the current language. Keyed UI text follows on the next frame.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_strings_set_language(std::ptr::null_mut(), std::ptr::null(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `language` NUL-terminated; `out_known` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_strings_set_language(
    ctx: *mut PonkoCtx,
    language: *const c_char,
    out_known: *mut bool,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let language = text(language)?;
        let known = ctx_access(ctx, |c| Ok(strings(c)?.set_language(language)))?;
        out(out_known, known)
    })
}

/// The current language's code (`"en-XA"` for the pseudo-locale), written
/// as `ponko_ctx_ui_text` writes text.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_strings_language(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` writable for `capacity` bytes (or null with
/// capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_strings_language(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let language = strings(c)?.language().to_string();
            write_text(buffer, capacity, out_len, &language)
        })
    })
}

/// How many languages have a table, to `out_count`; their codes come from
/// `ponko_ctx_strings_language_at` (Rust's `strings.languages()`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_strings_language_count(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out_count` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_strings_language_count(ctx: *mut PonkoCtx, out_count: *mut usize) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let n = ctx_access(ctx, |c| Ok(strings(c)?.languages().len()))?;
        out(out_count, n)
    })
}

/// The code of language number `index` (the reference language is 0), and
/// with `name_buffer` its name for a menu: each written as
/// `ponko_ctx_ui_text` writes text. `PONKO_NOT_FOUND` past the end.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_strings_language_at(std::ptr::null_mut(), 0, std::ptr::null_mut(), 0, &mut 0, std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; each buffer writable for its capacity (or null with
/// capacity 0); both lengths writable.
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn ponko_ctx_strings_language_at(
    ctx: *mut PonkoCtx,
    index: usize,
    code_buffer: *mut c_char,
    code_capacity: usize,
    out_code_len: *mut usize,
    name_buffer: *mut c_char,
    name_capacity: usize,
    out_name_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let strings = strings(c)?;
            let code = *strings
                .languages()
                .get(index)
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "no language at that index"))?;
            let table = strings.table(code).expect("listed");
            write_text(code_buffer, code_capacity, out_code_len, code)?;
            write_text(name_buffer, name_capacity, out_name_len, table.name())
        })
    })
}

/// Whether the current language is written right to left, to `out`.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_strings_is_rtl(std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_strings_is_rtl(ctx: *mut PonkoCtx, out_rtl: *mut bool) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let rtl = ctx_access(ctx, |c| Ok(strings(c)?.is_rtl()))?;
        out(out_rtl, rtl)
    })
}

/// Sets the font `language`'s keyed UI text is drawn with (Rust's
/// `strings.set_font`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_strings_set_font(std::ptr::null_mut(), std::ptr::null(), Default::default()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `language` NUL-terminated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_strings_set_font(
    ctx: *mut PonkoCtx,
    language: *const c_char,
    font: PonkoFont,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let language = text(language)?;
        ctx_access(ctx, |c| {
            let font = font_from_c(font);
            if !c.gfx.is_live(font) {
                return Err(invalid("unknown font"));
            }
            strings(c)?.set_font(language, font);
            Ok(())
        })
    })
}

/// The current language's font to `out_font` and whether it has one to
/// `out_has` (Rust's `strings.font()`).
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_strings_font(std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; both outputs writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_strings_font(
    ctx: *mut PonkoCtx,
    out_has: *mut bool,
    out_font: *mut PonkoFont,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        if out_has.is_null() || out_font.is_null() {
            return Err(invalid("output is null"));
        }
        let font = ctx_access(ctx, |c| Ok(strings(c)?.font()))?;
        out(out_has, font.is_some())?;
        out(out_font, crate::media::font_to_c(font.unwrap_or_default()))
    })
}

/// `key`'s text in the current language with `count` arguments from `args`
/// put in its placeholders (Rust's `strings.format`), written as
/// `ponko_ctx_ui_text` writes text. A key no table has is written as itself.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_strings_format(std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), 0, std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `key` NUL-terminated; `args` readable for `count`
/// arguments (or null with 0); `buffer` writable for `capacity` bytes (or
/// null with capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_strings_format(
    ctx: *mut PonkoCtx,
    key: *const c_char,
    args: *const PonkoStringArg,
    count: usize,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let key = text(key)?;
        let args = args_from_c(args, count)?;
        ctx_access(ctx, |c| {
            let formatted = strings(c)?.format(key, &args);
            write_text(buffer, capacity, out_len, &formatted)
        })
    })
}

/// Every language's missing keys, plural forms and arguments, one per line
/// (Rust's `strings.missing_report()`; empty when the tables are
/// complete), written as `ponko_ctx_ui_text` writes text.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_strings_missing_report(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `buffer` writable for `capacity` bytes (or null with
/// capacity 0); `out_len` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_strings_missing_report(
    ctx: *mut PonkoCtx,
    buffer: *mut c_char,
    capacity: usize,
    out_len: *mut usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let report = strings(c)?.missing_report();
            write_text(buffer, capacity, out_len, &report)
        })
    })
}

/// The plural form `count` takes in `language` (Rust's
/// `ponko::locale::plural_category`), to `out`.
/// ```
/// let mut c = ponko_ffi::PonkoPluralCategory::PONKO_PLURAL_OTHER;
/// assert_eq!(unsafe { ponko_ffi::ponko_plural_category(c"en".as_ptr(), 1.0, &mut c) } as u32, 0);
/// assert_eq!(c, ponko_ffi::PonkoPluralCategory::PONKO_PLURAL_ONE);
/// ```
/// # Safety
/// `language` NUL-terminated; `out` writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_plural_category(
    language: *const c_char,
    count: f64,
    out_category: *mut PonkoPluralCategory,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let language = text(language)?;
        out(out_category, ponko::locale::plural_category(language, count).into())
    })
}

/// Makes a node's text the string table key `key` with `count` arguments
/// (Rust's `ui.set_text_key`): it shows the current language's text and
/// font from the next frame on, and follows when the language changes.
/// `ponko_ctx_ui_set_text` replaces the key with plain text.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_set_text_key(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN, 0, std::ptr::null(), std::ptr::null(), 0) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live; `key` NUL-terminated; `args` readable for `count`
/// arguments (or null with 0).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_set_text_key(
    ctx: *mut PonkoCtx,
    target: u64,
    id: u64,
    key: *const c_char,
    args: *const PonkoStringArg,
    count: usize,
) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        let key = text(key)?;
        let args = args_from_c(args, count)?;
        with_ui(ctx, target, |ui| {
            let id = existing(ui, id)?;
            ui.set_text_key(id, key, &args);
            Ok(())
        })
    })
}

/// Gives every node of `target`'s UI that holds a key its text and font in
/// the current language now (Rust's `ui.localize(strings)`). The engine does
/// it for the screen's UI before each frame; call it when you drive a UI by
/// hand (headless tests) or for a world-space panel's tree.
/// ```
/// assert_eq!(unsafe { ponko_ffi::ponko_ctx_ui_localize(std::ptr::null_mut(), ponko_ffi::PONKO_UI_SCREEN) } as u32, 1);
/// ```
/// # Safety
/// `ctx` null or live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_ui_localize(ctx: *mut PonkoCtx, target: u64) -> PonkoStatus {
    // SAFETY: forwarded caller contract.
    boundary(|| unsafe {
        ctx_access(ctx, |c| {
            let strings = c.world.remove_resource::<Strings>().ok_or_else(|| {
                error(
                    PonkoStatus::PONKO_NOT_FOUND,
                    "no string tables: load one with ponko_ctx_load_strings",
                )
            })?;
            let done = crate::ui_retained::ui_of(c, target).map(|ui| ui.localize(&strings));
            c.world.insert_resource(strings);
            done
        })
    })
}
