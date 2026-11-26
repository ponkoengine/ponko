//! naga's validation errors in the shader's own words.
//!
//! The validator reports what it found with IR handles (`Function [4]`,
//! `Expression [10]`, `Some([5])`): positions in the module's arenas that
//! mean nothing to the person or agent reading the error. This resolves them
//! against the module: types by their WGSL names (`vec3<f32>`, `SpritePixel`),
//! functions, variables and parameters by name, and expressions by their
//! source text (`p.color.rgb`). The errors people meet most get a message of
//! their own; any other keeps naga's text, with the handles taken out.

// The parent module's `naga`: the engine's (wgpu's) or `ponko cook`'s, which
// keeps a copy of this file (`crates/ponko_cli/src/cook/shaders/names.rs`).
use super::naga;

use naga::common::wgsl::TypeContext;
use naga::proc::TypeResolution;
use naga::valid::{
    CallError, Disalignment, EntryPointError, ExpressionError, FunctionError, GlobalVariableError, ValidationError,
};
use naga::{Handle, Module, TypeInner};

/// The longest expression quoted from the source; longer ones are cut.
const MAX_SNIPPET: usize = 48;

/// Names for the handles in one validation error of `module`, whose source
/// is `composed`.
pub(super) struct Names<'a> {
    module: &'a Module,
    composed: &'a str,
    /// The function the error is in.
    function: Option<&'a naga::Function>,
    /// The expression the error is about, if it is one whose type can't be
    /// worked out (an operation on mismatched types): no type in its label.
    failing: Option<Handle<naga::Expression>>,
}

impl<'a> Names<'a> {
    pub(super) fn new(module: &'a Module, composed: &'a str, error: &ValidationError) -> Self {
        let function = match error {
            ValidationError::Function { handle, .. } => Some(&module.functions[*handle]),
            ValidationError::EntryPoint { name, stage, .. } => module
                .entry_points
                .iter()
                .find(|ep| &ep.name == name && ep.stage == *stage)
                .map(|ep| &ep.function),
            _ => None,
        };
        let failing = match error {
            ValidationError::Function {
                source: FunctionError::Expression { handle, .. },
                ..
            }
            | ValidationError::EntryPoint {
                source: EntryPointError::Function(FunctionError::Expression { handle, .. }),
                ..
            } => Some(*handle),
            _ => None,
        };
        Self {
            module,
            composed,
            function,
            failing,
        }
    }

    /// The error's message: what is wrong, by the names in the source.
    pub(super) fn message(&self, error: &ValidationError) -> String {
        match error {
            ValidationError::Function { name, source, .. } => self.function_error(name, source),
            ValidationError::EntryPoint { name, stage, source } => {
                let stage = format!("{stage:?}").to_lowercase();
                match source {
                    EntryPointError::Function(source) => self.function_error(name, source),
                    EntryPointError::BindingCollision(global) => format!(
                        "`{}` uses the same @group/@binding as another resource of the @{stage} entry point `{name}`",
                        self.global_name(*global)
                    ),
                    other => format!("the @{stage} entry point `{name}`: {}", stripped_chain(other)),
                }
            }
            ValidationError::GlobalVariable { handle, name, source } => self.global_error(*handle, name, source),
            ValidationError::Type { name, source, .. } if !name.is_empty() => {
                format!("type `{name}`: {}", stripped_chain(source))
            }
            ValidationError::Constant { name, source, .. } => format!("constant `{name}`: {}", stripped_chain(source)),
            ValidationError::Override { name, source, .. } => format!("override `{name}`: {}", stripped_chain(source)),
            other => stripped_chain(other),
        }
    }

    /// A span label naga wrote as an IR handle (`naga::ir::Expression [10]`),
    /// in the source's words; `None` for a label that is already words.
    pub(super) fn label(&self, label: &str) -> Option<String> {
        let rest = label.strip_prefix("naga::ir::")?;
        let (kind, index) = rest.split_once(' ')?;
        let index: usize = index.strip_prefix('[')?.strip_suffix(']')?.parse().ok()?;
        Some(match kind {
            "Expression" => {
                let function = self.function?;
                let (handle, _) = function.expressions.iter().nth(index)?;
                if self.failing == Some(handle) {
                    return Some(String::new());
                }
                match (self.snippet(handle), self.expression_type(handle)) {
                    (Some(code), Some(ty)) => format!("`{code}` is `{ty}`"),
                    (None, Some(ty)) => format!("this is `{ty}`"),
                    _ => String::new(),
                }
            }
            "Function" => {
                let (_, function) = self.module.functions.iter().nth(index)?;
                format!("in `fn {}`", function.name.as_deref().unwrap_or("?"))
            }
            "GlobalVariable" => {
                let (_, global) = self.module.global_variables.iter().nth(index)?;
                format!("`{}`", global.name.as_deref().unwrap_or("?"))
            }
            "Type" => {
                let (handle, _) = self.module.types.iter().nth(index)?;
                format!("`{}`", self.type_name(handle))
            }
            "LocalVariable" => {
                let function = self.function?;
                let (_, local) = function.local_variables.iter().nth(index)?;
                format!("`{}`", local.name.as_deref().unwrap_or("?"))
            }
            _ => String::new(),
        })
    }

    fn function_error(&self, function: &str, error: &FunctionError) -> String {
        match error {
            FunctionError::InvalidReturnType {
                expression,
                expected_ty,
            } => match (expression, expected_ty) {
                (Some(value), Some(expected)) => match self.expression_type(*value) {
                    Some(actual) => format!(
                        "`{function}` returns `{actual}` but is declared to return `{}`",
                        self.type_name(*expected)
                    ),
                    None => format!(
                        "`{function}` returns {} but is declared to return `{}`",
                        self.quoted(*value),
                        self.type_name(*expected)
                    ),
                },
                (None, Some(expected)) => format!(
                    "`{function}` is declared to return `{}` but returns without a value",
                    self.type_name(*expected)
                ),
                (Some(value), None) => {
                    let ty = self.expression_type(*value).unwrap_or_else(|| "?".into());
                    format!("`{function}` returns `{ty}` but declares no return type: add `-> {ty}` or return nothing")
                }
                (None, None) => format!("`{function}`: {}", stripped(&error.to_string())),
            },
            FunctionError::InvalidCall {
                function: callee,
                error,
            } => self.call_error(*callee, error),
            FunctionError::Expression { source, .. } => self
                .expression_error(source)
                .unwrap_or_else(|| format!("in `fn {function}`: {}", stripped_chain(source))),
            FunctionError::InvalidStoreTypes { pointer, value } => {
                let target = self.pointee_type(*pointer);
                let value_ty = self.expression_type(*value);
                match (target, value_ty) {
                    (Some(target), Some(value_ty)) => format!(
                        "can't assign `{value_ty}` to {}, which is `{target}`",
                        self.quoted(*pointer)
                    ),
                    _ => format!("the value assigned to {} has the wrong type", self.quoted(*pointer)),
                }
            }
            FunctionError::InvalidStorePointer(pointer) => {
                format!("{} can't be written to: it is read-only", self.quoted(*pointer))
            }
            FunctionError::InvalidIfType(condition) => self.condition("an `if` condition", "`bool`", *condition),
            FunctionError::InvalidSwitchType(selector) => {
                self.condition("a `switch` selector", "`i32` or `u32`", *selector)
            }
            other => format!("in `fn {function}`: {}", stripped_chain(other)),
        }
    }

    fn call_error(&self, callee: Handle<naga::Function>, error: &CallError) -> String {
        let function = &self.module.functions[callee];
        let name = function.name.as_deref().unwrap_or("?");
        match error {
            CallError::ArgumentCount { required, seen } => format!(
                "`{name}` takes {required} argument{}, but the call gives {seen}",
                if *required == 1 { "" } else { "s" }
            ),
            CallError::ArgumentType {
                index,
                required,
                seen_expression,
            } => {
                let parameter = function
                    .arguments
                    .get(*index)
                    .and_then(|a| a.name.as_deref())
                    .map_or(String::new(), |p| format!(" `{p}`"));
                let argument = format!("argument {} of `{name}`", index + 1);
                let code = self
                    .snippet(*seen_expression)
                    .map_or(String::new(), |c| format!(" (`{c}`)"));
                let required = self.type_name(*required);
                match self.expression_type(*seen_expression) {
                    Some(seen) => format!("{argument}{code} is `{seen}`, but its parameter{parameter} is `{required}`"),
                    None => format!("{argument}{code} doesn't match its parameter{parameter}, which is `{required}`"),
                }
            }
            CallError::Argument { index, source } => {
                let why = self.expression_error(source).unwrap_or_else(|| stripped_chain(source));
                format!("argument {} of `{name}`: {why}", index + 1)
            }
            other => format!("the call to `{name}`: {}", stripped_chain(other)),
        }
    }

    /// The message for an expression error, `None` for one with no message
    /// of its own.
    fn expression_error(&self, error: &ExpressionError) -> Option<String> {
        Some(match error {
            ExpressionError::InvalidBinaryOperandTypes {
                op,
                lhs_expr,
                lhs_type,
                rhs_expr,
                rhs_type,
            } => {
                let op = naga::back::binary_operation_str(*op);
                let lhs = self.inner_name(lhs_type);
                let rhs = self.inner_name(rhs_type);
                let code = |e| self.snippet(e).map_or(String::new(), |c| format!(" (`{c}`)"));
                let mut message = format!(
                    "`{op}` can't combine `{lhs}`{} and `{rhs}`{}",
                    code(*lhs_expr),
                    code(*rhs_expr)
                );
                message.push_str(&binary_hint(lhs_type, rhs_type));
                message
            }
            ExpressionError::SelectValuesTypeMismatch { accept, reject } => format!(
                "`select(f, t, condition)` needs `f` and `t` of one type, but `f` is `{}` and `t` is `{}`",
                self.inner_name(reject),
                self.inner_name(accept)
            ),
            ExpressionError::SelectConditionNotABool { actual } => format!(
                "the condition of `select` must be `bool` (or a vector of `bool`), not `{}`",
                self.inner_name(actual)
            ),
            ExpressionError::InvalidIndexType(index) => self.condition("an index", "`i32` or `u32`", *index),
            ExpressionError::InvalidUnaryOperandType(op, operand) => {
                let op = match op {
                    naga::UnaryOperator::Negate => "-",
                    naga::UnaryOperator::LogicalNot => "!",
                    naga::UnaryOperator::BitwiseNot => "~",
                };
                match self.expression_type(*operand) {
                    Some(ty) => format!("`{op}` can't take `{ty}` ({})", self.quoted(*operand)),
                    None => format!("`{op}` can't take {}", self.quoted(*operand)),
                }
            }
            _ => return None,
        })
    }

    fn global_error(&self, global: Handle<naga::GlobalVariable>, name: &str, error: &GlobalVariableError) -> String {
        match error {
            GlobalVariableError::Alignment(space, ty, why) => {
                let space = format!("{space:?}").to_lowercase();
                let whole = self.module.global_variables[global].ty;
                let what = if whole == *ty {
                    format!("its type `{}`", self.type_name(*ty))
                } else {
                    format!("`{}` in its type `{}`", self.type_name(*ty), self.type_name(whole))
                };
                let mut message = format!(
                    "`{name}`: {what} breaks the {space} address space's layout rules: {}",
                    lowercase_first(&stripped(&why.to_string()))
                );
                if space == "uniform" && matches!(why, Disalignment::ArrayStride { .. }) {
                    message.push_str(
                        " (in a uniform buffer, array elements are 16 bytes apart: use `array<vec4<f32>, N>`, or a storage buffer)",
                    );
                }
                message
            }
            GlobalVariableError::InvalidBinding => {
                format!("`{name}` needs `@group(..) @binding(..)`, or its address space allows none")
            }
            other => format!("`{name}`: {}", stripped_chain(other)),
        }
    }

    /// "`what` must be `wanted`, but `code` is `type`".
    fn condition(&self, what: &str, wanted: &str, expression: Handle<naga::Expression>) -> String {
        match self.expression_type(expression) {
            Some(ty) => format!("{what} must be {wanted}, but {} is `{ty}`", self.quoted(expression)),
            None => format!("{what} must be {wanted}: {} isn't", self.quoted(expression)),
        }
    }

    /// The expression's source text in backticks, or "this expression".
    fn quoted(&self, expression: Handle<naga::Expression>) -> String {
        self.snippet(expression)
            .map_or_else(|| "this expression".into(), |c| format!("`{c}`"))
    }

    /// The source text of `expression` in the function the error is in:
    /// the name of a `let`, a variable or a parameter, else its code.
    fn snippet(&self, expression: Handle<naga::Expression>) -> Option<String> {
        let function = self.function?;
        if let Some(name) = function.named_expressions.get(&expression) {
            return Some(name.clone());
        }
        let named = match function.expressions[expression] {
            naga::Expression::LocalVariable(local) => function.local_variables[local].name.clone(),
            naga::Expression::GlobalVariable(global) => self.module.global_variables[global].name.clone(),
            naga::Expression::FunctionArgument(index) => {
                function.arguments.get(index as usize).and_then(|a| a.name.clone())
            }
            _ => None,
        };
        if named.is_some() {
            return named;
        }
        let mut range = function.expressions.get_span(expression).to_range()?;
        // A call's span is its name: take in its arguments too.
        range.end = call_end(self.composed, range.end).unwrap_or(range.end);
        let code = self.composed.get(range)?.trim();
        if code.is_empty() {
            return None;
        }
        let code = code.split_whitespace().collect::<Vec<_>>().join(" ");
        Some(if code.chars().count() > MAX_SNIPPET {
            let cut: String = code.chars().take(MAX_SNIPPET - 1).collect();
            format!("{cut}…")
        } else {
            code
        })
    }

    /// The WGSL type of `expression`, if naga can work it out.
    fn expression_type(&self, expression: Handle<naga::Expression>) -> Option<String> {
        let function = self.function?;
        let resolution = self.resolve(function, expression)?;
        Some(self.module.to_ctx().type_resolution_to_string(&resolution))
    }

    /// The type a pointer `expression` points to.
    fn pointee_type(&self, pointer: Handle<naga::Expression>) -> Option<String> {
        let function = self.function?;
        let resolution = self.resolve(function, pointer)?;
        let inner = resolution.inner_with(&self.module.types);
        Some(match *inner {
            TypeInner::Pointer { base, .. } => self.type_name(base),
            TypeInner::ValuePointer {
                size: Some(size),
                scalar,
                ..
            } => self.inner_name(&TypeInner::Vector { size, scalar }),
            TypeInner::ValuePointer { size: None, scalar, .. } => self.inner_name(&TypeInner::Scalar(scalar)),
            _ => self.module.to_ctx().type_resolution_to_string(&resolution),
        })
    }

    fn resolve(&self, function: &naga::Function, expression: Handle<naga::Expression>) -> Option<TypeResolution> {
        let context =
            naga::proc::ResolveContext::with_locals(self.module, &function.local_variables, &function.arguments);
        let mut typifier = naga::front::Typifier::new();
        typifier.grow(expression, &function.expressions, &context).ok()?;
        Some(typifier[expression].clone())
    }

    fn type_name(&self, ty: Handle<naga::Type>) -> String {
        self.module.to_ctx().type_to_string(ty)
    }

    fn inner_name(&self, inner: &TypeInner) -> String {
        self.module
            .to_ctx()
            .type_resolution_to_string(&TypeResolution::Value(inner.clone()))
    }

    fn global_name(&self, global: Handle<naga::GlobalVariable>) -> String {
        self.module.global_variables[global]
            .name
            .clone()
            .unwrap_or_else(|| "?".into())
    }
}

/// Where the argument list that follows byte `at` of `code` ends (after its
/// `)`), if a `(` follows (spaces aside): `at` is the end of a call's name.
fn call_end(code: &str, at: usize) -> Option<usize> {
    let rest = code.get(at..)?;
    let open = rest.len() - rest.trim_start().len();
    if !rest[open..].starts_with('(') {
        return None;
    }
    let mut depth = 0usize;
    for (i, c) in rest[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at + open + i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// A hint for the usual mistakes behind a binary operation on mismatched
/// types: two number types, or two vector sizes.
fn binary_hint(lhs: &TypeInner, rhs: &TypeInner) -> String {
    let (a, b) = (lhs.scalar(), rhs.scalar());
    if let (Some(a), Some(b)) = (a, b)
        && a.kind != b.kind
    {
        return "; WGSL doesn't convert between number types by itself: convert one side, e.g. `f32(x)`".into();
    }
    match (lhs, rhs) {
        (TypeInner::Vector { size: a, .. }, TypeInner::Vector { size: b, .. }) if a != b => {
            "; the vectors' sizes differ: pick components with a swizzle (`.xy`, `.rgb`) or build one of the other size"
                .into()
        }
        _ => String::new(),
    }
}

/// `error` and its causes, from the outside in, with the IR handles taken out.
fn stripped_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    stripped(&message)
}

/// `text` without naga's IR handles (`[4]`, `Some([10])`), which name
/// positions in its arenas rather than anything in the source.
pub(super) fn stripped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(len) = handle_len(rest) {
            // Drop the space before the handle too: "Call to [4] is" -> "Call to is".
            if out.ends_with(' ') {
                out.pop();
            }
            rest = &rest[len..];
            continue;
        }
        let c = rest.chars().next().unwrap_or_default();
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// The length of the IR handle `rest` starts with: `[12]` or `Some([12])`.
fn handle_len(rest: &str) -> Option<usize> {
    let (prefix, suffix) = if rest.starts_with("Some([") {
        ("Some([", "])")
    } else {
        ("[", "]")
    };
    let digits = rest.strip_prefix(prefix)?;
    let count = digits.bytes().take_while(u8::is_ascii_digit).count();
    (count > 0 && digits[count..].starts_with(suffix)).then_some(prefix.len() + count + suffix.len())
}

fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_lowercase().chain(chars).collect())
}
