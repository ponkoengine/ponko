;; The frost drip's crawl, as a mod would ship it: it calls two engine
;; functions (as the C API's ray casts) and returns the new x and heading.
(module
  (import "ponko" "wall_ahead" (func $wall (param f32 f32) (result i32)))
  (import "ponko" "floor_ahead" (func $floor (param f32 f32) (result i32)))
  (memory (export "memory") 1)
  (func (export "update") (param $x f32) (param $heading f32) (param $speed f32) (param $dt f32) (result f32 f32)
    (if (i32.or
          (call $wall (local.get $x) (local.get $heading))
          (i32.eqz (call $floor (local.get $x) (local.get $heading))))
      (then (local.set $heading (f32.neg (local.get $heading)))))
    (f32.add (local.get $x) (f32.mul (f32.mul (local.get $heading) (local.get $speed)) (local.get $dt)))
    (local.get $heading)))
