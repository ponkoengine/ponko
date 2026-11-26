//! Native registration and typed arguments for the developer console.

use std::ffi::{c_char, c_void};
use std::{ptr, slice, str};

use ponko::console::{ConsoleArg, ConsoleType, ConsoleValue};
use ponko::error::{Error, ErrorKind};

use crate::ctx::{PonkoCtx, ctx_access, with_ctx};
use crate::game::{PonkoGame, build, name, ok};
use crate::{Failure, PonkoStatus, boundary, error, invalid};

/// Developer console argument and variable types.
#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PonkoConsoleType {
    /// UTF-8 text.
    PONKO_CONSOLE_TEXT,
    /// Signed 64-bit integer.
    PONKO_CONSOLE_INTEGER,
    /// Finite 64-bit float.
    PONKO_CONSOLE_FLOAT,
    /// Boolean.
    PONKO_CONSOLE_BOOL,
}

fn kind(raw: u32) -> Result<ConsoleType, Failure> {
    use PonkoConsoleType::*;
    match raw {
        x if x == PONKO_CONSOLE_TEXT as u32 => Ok(ConsoleType::Text),
        x if x == PONKO_CONSOLE_INTEGER as u32 => Ok(ConsoleType::Integer),
        x if x == PONKO_CONSOLE_FLOAT as u32 => Ok(ConsoleType::Float),
        x if x == PONKO_CONSOLE_BOOL as u32 => Ok(ConsoleType::Bool),
        _ => Err(invalid("unknown console value type")),
    }
}

/// One named, required positional argument.
#[repr(C)]
pub struct PonkoConsoleArg {
    /// NUL-terminated UTF-8, copied on registration.
    pub name: *const c_char,
    /// A PonkoConsoleType value.
    pub kind: u32,
}

/// One typed console value. Only the field for its kind is used.
#[repr(C)]
#[derive(Default)]
pub struct PonkoConsoleValue {
    /// A PonkoConsoleType value.
    pub kind: u32,
    /// Signed integer.
    pub integer: i64,
    /// Finite floating-point number.
    pub number: f64,
    /// Boolean.
    pub flag: bool,
    /// UTF-8 bytes for text, with no required NUL terminator.
    pub text: *const c_char,
    /// Number of bytes in text.
    pub text_len: usize,
}

impl PonkoConsoleValue {
    fn of(value: &ConsoleValue) -> Result<Self, Error> {
        use PonkoConsoleType::*;
        let mut out = Self::default();
        out.kind = match value {
            ConsoleValue::Text(s) => {
                out.text = s.as_ptr().cast();
                out.text_len = s.len();
                PONKO_CONSOLE_TEXT
            }
            ConsoleValue::Integer(i) => {
                out.integer = *i;
                PONKO_CONSOLE_INTEGER
            }
            ConsoleValue::Float(x) => {
                out.number = *x;
                PONKO_CONSOLE_FLOAT
            }
            ConsoleValue::Bool(on) => {
                out.flag = *on;
                PONKO_CONSOLE_BOOL
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::Unsupported,
                    "console value type is unavailable in C",
                ));
            }
        } as u32;
        Ok(out)
    }
}

unsafe fn view<'a, T>(data: *const T, len: usize) -> Result<&'a [T], Failure> {
    if len == 0 {
        return Ok(&[]);
    }
    if data.is_null() || len.saturating_mul(size_of::<T>()) > isize::MAX as usize {
        return Err(invalid("null or oversized console input"));
    }
    // SAFETY: the caller provides a valid readable slice for this call.
    Ok(unsafe { slice::from_raw_parts(data, len) })
}

unsafe fn value_of(value: &PonkoConsoleValue) -> Result<ConsoleValue, Failure> {
    Ok(match kind(value.kind)? {
        ConsoleType::Text => {
            // SAFETY: the caller provides readable UTF-8 bytes for text.
            let bytes = unsafe { view(value.text.cast(), value.text_len) }?;
            ConsoleValue::Text(
                str::from_utf8(bytes)
                    .map_err(|_| invalid("console text is not UTF-8"))?
                    .to_owned(),
            )
        }
        ConsoleType::Integer => ConsoleValue::Integer(value.integer),
        ConsoleType::Float => ConsoleValue::Float(value.number),
        ConsoleType::Bool => ConsoleValue::Bool(value.flag),
        _ => return Err(invalid("unknown console value type")),
    })
}

/// A reply slot borrowed for one command callback.
pub struct PonkoConsoleReply(Option<Result<String, Error>>);

/// Runs a registered command. The context, arguments, their text and the
/// reply are borrowed only for the duration of the callback.
pub type PonkoConsoleFn = Option<
    unsafe extern "C" fn(
        ctx: *mut PonkoCtx,
        args: *const PonkoConsoleValue,
        count: usize,
        reply: *mut PonkoConsoleReply,
        user: *mut c_void,
    ),
>;

/// Receives a variable's value and any text it holds, both borrowed only
/// for the duration of the callback.
pub type PonkoConsoleValueFn = Option<unsafe extern "C" fn(value: *const PonkoConsoleValue, user: *mut c_void)>;

/// Enable or disable the developer console, including its control methods.
/// # Safety
/// `game` is null or a live game.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_developer_console(game: *mut PonkoGame, on: bool) -> PonkoStatus {
    // SAFETY: forwarded game contract.
    unsafe { build(game, |game| ok(game.developer_console(on))) }
}

/// Choose the physical key (a PonkoKey value) that opens the console.
/// # Safety
/// `game` is null or a live game.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_console_key(game: *mut PonkoGame, key: u32) -> PonkoStatus {
    // SAFETY: forwarded game contract.
    unsafe {
        build(game, |game| match crate::keys::key_from_c(key) {
            Some(key) => ok(game.console_key(key)),
            None => (game, Err(invalid("unknown console key"))),
        })
    }
}

/// Registers a command. Its callback receives validated typed values
/// and may reply with ponko_console_reply_text. With no reply, output is empty.
///
/// Ownership: name, help and args (including each name) are copied.
/// The callback and `user` remain the caller's and must stay valid
/// until the game ends or is freed.
///
/// # Safety
/// `game` is null or live; name, help and argument names are NUL-terminated
/// UTF-8. `args` is readable for `count` entries (null if count is zero).
/// The callback must obey the borrowed lifetime of its arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_console_command(
    game: *mut PonkoGame,
    command: *const c_char,
    help: *const c_char,
    args: *const PonkoConsoleArg,
    count: usize,
    callback: PonkoConsoleFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded builder inputs and callback contract.
    unsafe {
        build(game, |game| {
            let registration = || {
                let callback = callback.ok_or_else(|| invalid("console callback is null"))?;
                let args = view(args, count)?
                    .iter()
                    .map(|arg| Ok(ConsoleArg::new(name(arg.name)?, kind(arg.kind)?)))
                    .collect::<Result<Vec<_>, Failure>>()?;
                Ok((name(command)?, name(help)?, args, callback))
            };
            let (command, help, args, callback) = match registration() {
                Ok(values) => values,
                Err(e) => return (game, Err(e)),
            };
            ok(game.console_command(command, help, &args, move |ctx, args| {
                let values = args.iter().map(PonkoConsoleValue::of).collect::<Result<Vec<_>, _>>()?;
                let data = if values.is_empty() {
                    ptr::null()
                } else {
                    values.as_ptr()
                };
                let mut reply = PonkoConsoleReply(None);
                with_ctx(ctx, |ctx| callback(ctx, data, values.len(), &mut reply, user));
                reply.0.unwrap_or_else(|| Ok(String::new()))
            }))
        })
    }
}

/// Copies text into a console command's reply. Set failed to make it an
/// error reply. An empty text is allowed with a null pointer and zero length.
///
/// # Safety
/// `reply` is live within the command callback, or null. `text` is
/// readable for `len` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_console_reply_text(
    reply: *mut PonkoConsoleReply,
    text: *const c_char,
    len: usize,
    failed: bool,
) -> PonkoStatus {
    // SAFETY: forwarded reply and buffer contracts.
    boundary(|| unsafe {
        let reply = reply.as_mut().ok_or_else(|| invalid("console reply is null"))?;
        let text = str::from_utf8(view(text.cast(), len)?).map_err(|_| invalid("console reply is not UTF-8"))?;
        reply.0 = Some(if failed {
            Err(Error::new(ErrorKind::Invalid, text))
        } else {
            Ok(text.to_owned())
        });
        Ok(())
    })
}

/// Registers a variable. Its type is fixed by its initial value, which
/// is copied (including any text).
///
/// # Safety
/// `game` is null or live; variable and help are NUL-terminated UTF-8.
/// `initial` is null or a readable value, including its text buffer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_game_console_variable(
    game: *mut PonkoGame,
    variable: *const c_char,
    help: *const c_char,
    initial: *const PonkoConsoleValue,
) -> PonkoStatus {
    // SAFETY: forwarded builder and value contracts.
    unsafe {
        build(game, |game| {
            let registration = || {
                let initial = initial
                    .as_ref()
                    .ok_or_else(|| invalid("console variable value is null"))?;
                Ok((name(variable)?, name(help)?, value_of(initial)?))
            };
            match registration() {
                Ok((variable, help, initial)) => ok(game.console_variable(variable, help, initial)),
                Err(e) => (game, Err(e)),
            }
        })
    }
}

/// Reads a registered variable through a callback, or returns PONKO_NOT_FOUND.
///
/// Ownership: `user` remains the caller's and must stay valid during this call.
/// The callback receives a copy, after the context borrow has ended.
///
/// # Safety
/// `ctx` is null or live; variable is NUL-terminated UTF-8. The callback
/// must obey the borrowed lifetime of the value.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ponko_ctx_console_value(
    ctx: *mut PonkoCtx,
    variable: *const c_char,
    callback: PonkoConsoleValueFn,
    user: *mut c_void,
) -> PonkoStatus {
    // SAFETY: forwarded context, name and callback contracts.
    boundary(|| unsafe {
        let callback = callback.ok_or_else(|| invalid("console value callback is null"))?;
        let variable = name(variable)?;
        let value = ctx_access(ctx, |ctx| {
            ctx.console_value(variable)
                .cloned()
                .ok_or_else(|| error(PonkoStatus::PONKO_NOT_FOUND, "unknown console variable"))
        })?;
        let raw = PonkoConsoleValue::of(&value).map_err(crate::engine_error)?;
        callback(&raw, user);
        Ok(())
    })
}
