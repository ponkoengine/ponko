// The parts of the gallery check (gallery.mjs) that need no browser: its
// arguments and what each page must do. gallery-lib.test.mjs tests them.
import { join } from "node:path";
import { BACKENDS, BROWSERS, UsageError } from "./lib.mjs";

export const GALLERY_USAGE =
  "usage: gallery.mjs [--dir <built gallery>] [--only <example,...>] [--browser chromium|firefox|webkit]\n" +
  "                   [--backend auto|webgpu|webgl2] [--chromium-mode shell|new] [--browser-arg <flag>]...\n" +
  "                   [--timeout <startup seconds>] [--out <artifacts dir>] [--allow-unsupported]";

// Draw calls (WebGL2) or queue submissions (WebGPU) the page must make after
// its first frame, within PROGRESS_SECONDS: a coarse bound that only a page
// that stopped drawing misses (a game draws at least once per frame), with
// time for a heavy scene to compile its pipelines in its first frames.
export const MIN_PROGRESS_DRAWS = 10;
export const PROGRESS_SECONDS = 15;

export function parseGalleryArgs(argv) {
  const args = {
    dir: join("target", "web", "gallery"),
    only: null,
    browser: "chromium",
    backend: "auto",
    chromiumMode: "shell",
    browserArgs: [],
    timeout: 90,
    out: null,
    allowUnsupported: false,
  };
  const fail = (message) => {
    throw new UsageError(message);
  };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const value = () => argv[++i] ?? fail(`${flag} needs a value`);
    switch (flag) {
      case "--dir": args.dir = value(); break;
      case "--only": args.only = value().split(",").filter(Boolean); break;
      case "--browser": args.browser = value(); break;
      case "--backend": args.backend = value(); break;
      case "--chromium-mode": args.chromiumMode = value(); break;
      case "--browser-arg": args.browserArgs.push(value()); break;
      case "--timeout": args.timeout = Number(value()); break;
      case "--out": args.out = value(); break;
      case "--allow-unsupported": args.allowUnsupported = true; break;
      default: fail(`unknown argument ${flag}`);
    }
  }
  if (!BACKENDS.includes(args.backend)) fail(`--backend must be one of ${BACKENDS.join(", ")}`);
  if (!BROWSERS.includes(args.browser)) fail(`--browser must be one of ${BROWSERS.join(", ")}`);
  if (!["shell", "new"].includes(args.chromiumMode)) fail("--chromium-mode must be shell or new");
  if (!(args.timeout > 0)) fail("--timeout must be a positive number of seconds");
  const browserLabel = args.browser === "chromium" && args.chromiumMode === "new" ? "chromium-new" : args.browser;
  args.out ??= join("target", "web-smoke", `gallery-${browserLabel}-${args.backend}`);
  return args;
}

/**
 * What a demo's page must do in this run: "run" the game, or show its
 * "webgpu-message" (a demo that needs WebGPU, where the page has none: WebGPU
 * hidden by `--backend webgl2`, or no adapter in the browser). `caps` is the
 * runner's probe of the browser (probe() in smoke.mjs).
 */
export function expectedPage(entry, backend, caps) {
  if (entry.webgpu !== "required") return "run";
  if (backend === "webgl2" || !caps.webgpu) return "webgpu-message";
  return "run";
}

/** The backend the page's own check picks: WebGPU when the browser offers an adapter, else WebGL2. */
export function expectedBackend(backend, caps) {
  if (backend === "webgl2") return "webgl2";
  return caps.webgpu ? "webgpu" : "webgl2";
}

/**
 * Checks the message a page shows instead of a demo that needs WebGPU: it
 * must say WebGPU is needed. An error message, or null when it's right.
 */
export function checkWebgpuMessage(text) {
  if (!text) return "the page showed no message";
  if (!/needs WebGPU/.test(text)) return `the message doesn't say the demo needs WebGPU: "${text}"`;
  return null;
}

/** The examples of a built gallery (its gallery.json) to check, in order; `only` must name some of them. */
export function selectExamples(gallery, only) {
  const examples = gallery.examples ?? [];
  if (!only) return examples;
  const unknown = only.filter((name) => !examples.some((e) => e.example === name));
  if (unknown.length) throw new UsageError(`--only: not in this gallery: ${unknown.join(", ")}`);
  return examples.filter((e) => only.includes(e.example));
}

/**
 * A WebGL error the browser logged for the page (`GL_INVALID_OPERATION`,
 * `GL_INVALID_ENUM`, ..., `GL_OUT_OF_MEMORY`): the call was refused, so a
 * draw that logs one drew nothing, even when the canvas isn't quite blank.
 * The message without the context's address, or null for any other line
 * (the driver's performance notes, "GPU stall due to ReadPixels", are not errors).
 */
export function webglErrorOf(text) {
  const match = /\b(GL_(?:INVALID_[A-Z_]+|OUT_OF_MEMORY)\b.*)$/m.exec(text ?? "");
  return match ? match[1].trim() : null;
}
