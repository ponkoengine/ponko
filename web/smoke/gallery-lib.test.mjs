// The gallery check's decisions, without a browser: `node --test web/smoke`.
import assert from "node:assert/strict";
import { join } from "node:path";
import { test } from "node:test";
import {
  checkWebgpuMessage,
  expectedBackend,
  expectedPage,
  parseGalleryArgs,
  selectExamples,
  webglErrorOf,
} from "./gallery-lib.mjs";

const withWebgpu = { webgpu: { canvas: true }, webgl2: { renderer: "x" } };
const withoutWebgpu = { webgpu: null, webgl2: { renderer: "x" } };
const needs = { example: "compute", webgpu: "required" };
const either = { example: "battle", webgpu: "optional" };

test("a demo that needs WebGPU must show its message where the page has none", () => {
  assert.equal(expectedPage(needs, "webgl2", withWebgpu), "webgpu-message", "WebGPU hidden by --backend webgl2");
  assert.equal(expectedPage(needs, "auto", withoutWebgpu), "webgpu-message", "no adapter in the browser");
  assert.equal(expectedPage(needs, "auto", withWebgpu), "run");
  assert.equal(expectedPage(needs, "webgpu", withWebgpu), "run");
});

test("any other demo must run, on WebGPU where the page has it and WebGL2 otherwise", () => {
  for (const backend of ["auto", "webgpu", "webgl2"]) assert.equal(expectedPage(either, backend, withoutWebgpu), "run");
  assert.equal(expectedBackend("webgl2", withWebgpu), "webgl2");
  assert.equal(expectedBackend("auto", withWebgpu), "webgpu");
  assert.equal(expectedBackend("auto", withoutWebgpu), "webgl2");
});

test("the WebGPU message must say the demo needs WebGPU", () => {
  assert.equal(checkWebgpuMessage("This demo needs WebGPU, and this browser has no WebGPU."), null);
  assert.match(checkWebgpuMessage("Loading…"), /doesn't say/);
  assert.match(checkWebgpuMessage(null), /no message/);
});

test("arguments: defaults, an output folder per browser and backend, and bad values refused", () => {
  const args = parseGalleryArgs([]);
  assert.equal(args.dir, join("target", "web", "gallery"));
  assert.equal(args.backend, "auto");
  assert.equal(args.out, join("target", "web-smoke", "gallery-chromium-auto"));
  const custom = parseGalleryArgs(["--only", "battle,3d", "--backend", "webgl2", "--chromium-mode", "new", "--browser-arg", "--x"]);
  assert.deepEqual(custom.only, ["battle", "3d"]);
  assert.equal(custom.out, join("target", "web-smoke", "gallery-chromium-new-webgl2"));
  assert.deepEqual(custom.browserArgs, ["--x"]);
  assert.throws(() => parseGalleryArgs(["--backend", "vulkan"]), /--backend must be one of/);
  assert.throws(() => parseGalleryArgs(["--timeout", "0"]), /--timeout must be a positive/);
  assert.throws(() => parseGalleryArgs(["--fixture", "x"]), /unknown argument --fixture/);
  assert.throws(() => parseGalleryArgs(["--dir"]), /--dir needs a value/);
});

test("--only picks examples of the built gallery, in its order, and refuses others", () => {
  const gallery = { examples: [{ example: "a" }, { example: "b" }, { example: "c" }] };
  assert.deepEqual(selectExamples(gallery, null).map((e) => e.example), ["a", "b", "c"]);
  assert.deepEqual(selectExamples(gallery, ["c", "a"]).map((e) => e.example), ["a", "c"]);
  assert.throws(() => selectExamples(gallery, ["d"]), /not in this gallery: d/);
});

test("a WebGL error the browser logs fails the page; its performance notes don't", () => {
  assert.equal(
    webglErrorOf("[.WebGL-0x10400adc800] GL_INVALID_OPERATION: glDrawElementsInstanced: Mismatch between texture format and sampler type (signed/unsigned/float/shadow)."),
    "GL_INVALID_OPERATION: glDrawElementsInstanced: Mismatch between texture format and sampler type (signed/unsigned/float/shadow).",
  );
  assert.equal(webglErrorOf("[.WebGL-0x1] GL_OUT_OF_MEMORY: glTexStorage2D: out of memory"), "GL_OUT_OF_MEMORY: glTexStorage2D: out of memory");
  assert.equal(webglErrorOf("[.WebGL-0x1]GL Driver Message (OpenGL, Performance, GL_CLOSE_PATH_NV, High): GPU stall due to ReadPixels"), null);
  assert.equal(webglErrorOf("ponko: falling back to WebGL2: this browser has no WebGPU"), null);
  assert.equal(webglErrorOf(undefined), null);
});

