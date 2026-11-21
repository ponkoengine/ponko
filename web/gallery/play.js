// Starts the demo on a gallery page (web/gallery/README.md). Before loading
// the game it checks what the browser offers: a demo whose page says
// `data-webgpu="required"` runs only with WebGPU and otherwise explains
// what's missing; any other demo also runs on WebGL2. Then it starts the game
// as web/index.html does. The page reports how it went in
// `<html data-gallery>`: "loading", "running", "unsupported" or "error"
// (web/smoke/gallery.mjs reads it).
const root = document.documentElement;
const status = document.getElementById("status");
const note = document.getElementById("backend-note");
const canvas = document.getElementById("ponko");
root.dataset.gallery = "loading";

function show(kind, text) {
  root.dataset.gallery = kind;
  status.className = kind;
  status.textContent = text;
  status.hidden = false;
}

// "Fill window" (and ?full in the address) lets the game take the whole window.
document.getElementById("fill").addEventListener("click", () => {
  root.classList.toggle("full");
  canvas.focus();
});
if (new URLSearchParams(location.search).has("full")) root.classList.add("full");

/** Why WebGPU can't be used here, or null when it can. */
async function webgpuProblem() {
  if (!navigator.gpu) return "this browser has no WebGPU";
  try {
    const adapter = await navigator.gpu.requestAdapter();
    return adapter ? null : "this browser has WebGPU but found no graphics adapter for it";
  } catch (e) {
    return `this browser's WebGPU failed: ${e}`;
  }
}

function hasWebgl2() {
  const gl = document.createElement("canvas").getContext("webgl2");
  gl?.getExtension("WEBGL_lose_context")?.loseContext();
  return Boolean(gl);
}

const ADVICE = "Try a current Chrome, Edge or Safari, or switch WebGPU on in your browser's settings.";

async function start() {
  const problem = await webgpuProblem();
  if (problem && root.dataset.webgpu === "required") {
    show("unsupported", `This demo needs WebGPU, and ${problem}. ${ADVICE}`);
    return;
  }
  if (problem && !hasWebgl2()) {
    show("unsupported", `This demo needs WebGPU or WebGL2, and this browser has neither (${problem}; no WebGL2). ${ADVICE}`);
    return;
  }
  if (problem) {
    note.textContent = `Drawing with WebGL2: ${problem}. Effects that need compute shaders are off.`;
    note.hidden = false;
  }
  root.dataset.backend = problem ? "webgl2" : "webgpu";

  window.addEventListener("error", (e) => {
    // The engine's report of a panic comes first; the trap after it says less.
    if (root.dataset.gallery !== "error") show("error", `The demo stopped: ${e.message} (see the browser console)`);
  });
  const { default: init, onPonkoStartup, onPonkoFailure } = await import(new URL("game.js", location.href));
  try {
    await init();
  } catch (e) {
    // winit leaves its startup function by throwing; that's not an error.
    if (!String(e).includes("Using exceptions for control flow")) {
      show("error", `Could not start the demo: ${e}`);
      throw e;
    }
  }
  // A panic, the WebGL2 context lost while playing, a stopped worker thread.
  onPonkoFailure?.((failure) => {
    if (root.dataset.gallery !== "error") show("error", failure.text ?? `The demo stopped: ${failure.message}`);
  });
  function started(error) {
    if (error) {
      show("error", `Could not start the demo: ${error.message}`);
    } else if (root.dataset.gallery === "loading") {
      root.dataset.gallery = "running";
      status.hidden = true;
      canvas.focus();
    }
  }
  if (onPonkoStartup) onPonkoStartup(started);
  else started(null);
}

start().catch((e) => {
  if (root.dataset.gallery !== "error") show("error", `Could not start the demo: ${e}`);
  throw e;
});
