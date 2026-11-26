<!-- area: Rendering -->
### Changed
- A WGSL error the compiler's validator finds (material, sprite, sky and
  screen-effect shaders, compute passes) names types, functions, parameters
  and variables as the source writes them, instead of the compiler's
  internal handles (`Function [4]`, `Some([10])`, `naga::ir::Expression`):
  "`fragment` returns `vec3<f32>` but is declared to return `vec4<f32>`".
  A return type, a call's argument type or count, an operation on two types
  that don't go together (with the conversion or swizzle to write), an
  assignment, `select`, an index, an `if`/`switch` condition, a write to a
  read-only buffer, a uniform buffer's layout and a binding used twice get
  messages of their own; any other keeps the compiler's text without the
  handles. The caret's label gives the expression's type and the note the
  function. The line, column and caret are unchanged, and so is
  `ShaderDiagnostic`: this reaches `shader_status` (Rust, C and MCP), the
  hot-reload notice and the `create_*` panics and errors alike, and
  `ponko cook`'s shader check words its errors the same way.
