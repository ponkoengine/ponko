//! Registered developer commands and variables, shared by the retained
//! console, headless games and control tools.
//!
//! **Maturity: tested**, from the guide chapter `control.md`. Game: none. Tests:
//! `tests/control.rs`, `tests/console.rs`. Platforms: Linux, macOS and Windows, headless (the
//! workspace tests in the list of checks). Graphics: not involved. Missing: a reference game that
//! uses it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fmt::{self, Write};
use std::rc::Rc;

use crate::ctx::Ctx;
use crate::error::{Error, ErrorKind};

mod overlay;
pub(crate) use overlay::Overlay;

const MAX_LINE_BYTES: usize = 16 * 1024;
const DISABLED: &str = "developer console is disabled; enable it with Game::developer_console(true)";

/// The type expected for a command argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConsoleType {
    /// UTF-8 text. Use quotes to keep spaces in an argument.
    Text,
    /// A signed 64-bit integer.
    Integer,
    /// A finite 64-bit floating-point number.
    Float,
    /// `true` or `false`.
    Bool,
}

impl fmt::Display for ConsoleType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Text => "text",
            Self::Integer => "integer",
            Self::Float => "float",
            Self::Bool => "bool",
        })
    }
}

impl ConsoleType {
    fn parse(self, token: String) -> Option<ConsoleValue> {
        Some(match self {
            Self::Text => ConsoleValue::Text(token),
            Self::Integer => ConsoleValue::Integer(token.parse().ok()?),
            Self::Float => ConsoleValue::Float(token.parse::<f64>().ok().filter(|v| v.is_finite())?),
            Self::Bool => ConsoleValue::Bool(token.parse().ok()?),
        })
    }
}

/// A required positional argument in a command's signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleArg {
    name: String,
    kind: ConsoleType,
}

impl ConsoleArg {
    /// Gives the argument a name for help and validation errors.
    pub fn new(name: impl Into<String>, kind: ConsoleType) -> Self {
        Self {
            name: name.into(),
            kind,
        }
    }
}

/// A validated argument or the value of a registered console variable.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ConsoleValue {
    /// UTF-8 text.
    Text(String),
    /// A signed 64-bit integer.
    Integer(i64),
    /// A finite 64-bit floating-point number.
    Float(f64),
    /// A boolean.
    Bool(bool),
}

impl ConsoleValue {
    fn kind(&self) -> ConsoleType {
        match self {
            Self::Text(_) => ConsoleType::Text,
            Self::Integer(_) => ConsoleType::Integer,
            Self::Float(_) => ConsoleType::Float,
            Self::Bool(_) => ConsoleType::Bool,
        }
    }
}

impl fmt::Display for ConsoleValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(v) => v.fmt(f),
            Self::Integer(v) => v.fmt(f),
            Self::Float(v) => v.fmt(f),
            Self::Bool(v) => v.fmt(f),
        }
    }
}

type Handler = Rc<RefCell<dyn FnMut(&mut Ctx, &[ConsoleValue]) -> Result<String, Error>>>;

enum EntryKind {
    Command { args: Vec<ConsoleArg>, run: Handler },
    Variable(ConsoleValue),
}

struct Entry {
    help: String,
    kind: EntryKind,
}

impl Entry {
    fn usage(&self, name: &str) -> String {
        let mut usage = name.to_owned();
        match &self.kind {
            EntryKind::Command { args, .. } => {
                for arg in args {
                    let _ = write!(usage, " <{}:{}>", arg.name, arg.kind);
                }
            }
            EntryKind::Variable(value) => {
                let _ = write!(usage, " [value:{}]", value.kind());
            }
        }
        usage
    }
}

/// A game's developer console at `ctx.console`.
///
/// Register entries with [`Game::console_command`](crate::game::Game::console_command)
/// and [`Game::console_variable`](crate::game::Game::console_variable).
#[derive(Default)]
pub struct Console {
    entries: BTreeMap<String, Entry>,
    pub(crate) enabled: Option<bool>,
}

impl Console {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
            .unwrap_or(cfg!(debug_assertions) && !self.entries.is_empty())
    }

    pub(crate) fn unavailable(&self) -> Option<&'static str> {
        (!self.enabled()).then_some(DISABLED)
    }

    fn check_name(&self, name: &str) -> Result<(), Error> {
        if !valid_name(name) {
            return Err(invalid(format!(
                "console name {name:?} must contain only lowercase ASCII letters, digits, underscores or dots"
            )));
        }
        if name == "help" || self.entries.contains_key(name) {
            return Err(invalid(format!("console name `{name}` is already registered")));
        }
        Ok(())
    }

    pub(crate) fn command(
        &mut self,
        name: String,
        help: String,
        args: &[ConsoleArg],
        run: impl FnMut(&mut Ctx, &[ConsoleValue]) -> Result<String, Error> + 'static,
    ) -> Result<(), Error> {
        self.check_name(&name)?;
        for (i, arg) in args.iter().enumerate() {
            if !valid_name(&arg.name) || args[..i].iter().any(|a| a.name == arg.name) {
                return Err(invalid(format!(
                    "console command `{name}` has an invalid or duplicate argument name {:?}",
                    arg.name
                )));
            }
        }
        self.entries.insert(
            name,
            Entry {
                help,
                kind: EntryKind::Command {
                    args: args.to_vec(),
                    run: Rc::new(RefCell::new(run)),
                },
            },
        );
        Ok(())
    }

    pub(crate) fn variable(&mut self, name: String, help: String, value: ConsoleValue) -> Result<(), Error> {
        self.check_name(&name)?;
        if matches!(&value, ConsoleValue::Float(v) if !v.is_finite()) {
            return Err(invalid(format!("console variable `{name}` requires a finite default")));
        }
        self.entries.insert(
            name,
            Entry {
                help,
                kind: EntryKind::Variable(value),
            },
        );
        Ok(())
    }

    fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once("help").chain(self.entries.keys().map(String::as_str))
    }

    fn help(&self, args: &[String]) -> Result<String, Error> {
        const HELP: &str = "help [name] — List commands and variables, or show help for one.";
        match args {
            [] => {
                let mut text = HELP.to_owned();
                for (name, entry) in &self.entries {
                    let _ = write!(text, "\n{} — {}", entry.usage(name), entry.help);
                }
                Ok(text)
            }
            [name] if name == "help" => Ok(HELP.into()),
            [name] => {
                let entry = self.entries.get(name).ok_or_else(|| unknown(name))?;
                Ok(format!("{} — {}", entry.usage(name), entry.help))
            }
            _ => Err(invalid("usage: help [name]")),
        }
    }
}

fn valid_name(name: &str) -> bool {
    name.split('.').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    })
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new(ErrorKind::Invalid, message)
}

fn unknown(name: &str) -> Error {
    invalid(format!(
        "unknown console command or variable `{name}`; type `help` for the list"
    ))
}

fn parse_value(token: String, name: &str, kind: ConsoleType, usage: &str) -> Result<ConsoleValue, Error> {
    kind.parse(token)
        .ok_or_else(|| invalid(format!("`{name}` expects a {kind}; usage: {usage}")))
}

fn tokenize(line: &str) -> Result<Vec<String>, Error> {
    if line.len() > MAX_LINE_BYTES {
        return Err(Error::new(ErrorKind::Limit, "console command line exceeds 16 KiB"));
    }
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut started = false;
    let mut quote = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            token.push(
                chars
                    .next()
                    .ok_or_else(|| invalid("console line ends in an escape; add a character after the backslash"))?,
            );
        } else if let Some(open) = quote {
            if c == open {
                quote = None;
            } else {
                token.push(c);
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c.is_whitespace() {
            if started {
                tokens.push(std::mem::take(&mut token));
                started = false;
            }
            continue;
        } else {
            token.push(c);
        }
        started = true;
    }
    if let Some(open) = quote {
        return Err(invalid(format!("unclosed {open} quote in console line")));
    }
    if started {
        tokens.push(token);
    }
    Ok(tokens)
}

impl Console {
    /// Runs a console line on this thread and returns its text output.
    /// Use `help` to list entries, `help name` for one entry, and
    /// `name [value]` to read or set a variable. The console must be enabled.
    /// A syntax, argument or lookup error is reported before the handler runs.
    ///
    /// Pass the whole context so a registered command can use any game service.
    pub fn run(ctx: &mut Ctx, line: &str) -> Result<String, Error> {
        if let Some(reason) = ctx.console.unavailable() {
            return Err(invalid(reason));
        }
        let mut tokens = tokenize(line)?.into_iter();
        let name = tokens
            .next()
            .ok_or_else(|| invalid("enter a console command or type `help`"))?;
        if name == "help" {
            return ctx.console.help(tokens.as_slice());
        }
        let entry = ctx.console.entries.get_mut(&name).ok_or_else(|| unknown(&name))?;
        let usage = entry.usage(&name);
        match &mut entry.kind {
            EntryKind::Variable(value) => {
                if tokens.len() > 1 {
                    return Err(invalid(format!("too many arguments; usage: {usage}")));
                }
                if let Some(token) = tokens.next() {
                    *value = parse_value(token, "value", value.kind(), &usage)?;
                }
                Ok(value.to_string())
            }
            EntryKind::Command { args, run } => {
                if tokens.len() > args.len() {
                    return Err(invalid(format!("too many arguments; usage: {usage}")));
                }
                let values = args
                    .iter()
                    .map(|arg| {
                        let token = tokens
                            .next()
                            .ok_or_else(|| invalid(format!("missing argument `{}`; usage: {usage}", arg.name)))?;
                        parse_value(token, &arg.name, arg.kind, &usage)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let handler = Rc::clone(run);
                let mut handler = handler
                    .try_borrow_mut()
                    .map_err(|_| invalid(format!("console command `{name}` is already running")))?;
                handler(ctx, &values)
            }
        }
    }

    /// Reads a registered variable, even when the developer console is disabled.
    pub fn value(&self, name: &str) -> Option<&ConsoleValue> {
        match &self.entries.get(name)?.kind {
            EntryKind::Variable(value) => Some(value),
            EntryKind::Command { .. } => None,
        }
    }
}
