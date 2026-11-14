### Added
- Stencil state on 3D materials: `MaterialDesc::stencil` takes a `Stencil`
  (reference, read and write masks, and per face a `StencilCompare` with the
  fail, depth-fail and pass `StencilOp`s). `Stencil::mark`, `inside`,
  `outside` and `once` cover masks, portals, outlines and see-through
  surfaces that blend once where they overlap; `Stencil::order` sets the
  order solid stencil materials draw in. It works on WebGL2 too.
- The scene depth gets stencil bits only once a material has a stencil:
  scenes without one keep their plain depth target, passes and pipelines,
  and draw as before.
- From C, `ponko_ctx_create_material_with_stencil`,
  `ponko_ctx_update_material_with_stencil` and the `ponko_stencil_*`
  constructors with `PonkoStencil`; in C++, `ctx::create_material(desc,
  stencil)` and `ponko::stencil_once` and its siblings.
- The `stencil` example (also in the web gallery) and the guide's "Stencil"
  section.
