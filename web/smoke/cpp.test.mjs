// The C++ runner's program: the guide's first game, taken from guide.cpp as the guide shows it.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { anchored, firstGameProgram } from "./cpp.mjs";

test("anchored takes the lines between a snippet's anchors", () => {
  const source = "a\n// ANCHOR: x\nkept 1\nkept 2\n// ANCHOR_END: x\nb";
  assert.equal(anchored(source, "x"), "kept 1\nkept 2");
  assert.throws(() => anchored(source, "y"), /no anchor y/);
});

test("the program is the guide's first game with a main that plays it", () => {
  const guide = readFileSync(new URL("../../examples/cpp/guide.cpp", import.meta.url), "utf8");
  const program = firstGameProgram(guide);
  assert.match(program, /#include "ponko\.hpp"/);
  assert.match(program, /static void my_game\(\)/);
  assert.match(program, /load_texture\("crab\.png"\)/);
  assert.match(program, /PONKO_KEY_RIGHT/);
  assert.match(program, /int main\(\) \{ my_game\(\); \}\n$/);
  assert.doesNotMatch(program, /ANCHOR/);
});
