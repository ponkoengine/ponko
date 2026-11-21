;; A hostile mod: one export spins forever, one grows memory forever.
(module
  (memory (export "memory") 1)
  (func (export "spin") (loop $l (br $l)))
  (func (export "grow") (result i32)
    (loop $l
      (br_if $l (i32.ne (memory.grow (i32.const 16)) (i32.const -1))))
    (memory.size)))
