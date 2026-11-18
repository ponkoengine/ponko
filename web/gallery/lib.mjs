// The parts of the web gallery that need no build and no browser: reading
// examples.json, and writing the index page and one page per example.
// build.mjs builds the examples and calls these; lib.test.mjs tests them
// (`node --test web/gallery`). See README.md.

export const REPO = "https://gitlab.com/ponkoengine/ponko";
// Where the release workflow publishes the gallery (GitHub Pages of the
// repository); the README, the guide and the release notes link here.
export const GALLERY_URL = "https://ponkoengine.github.io/ponko/";
export const KINDS = { "2d": "2D", "3d": "3D" };
export const WEBGPU = ["optional", "required"];

const KEYS = ["example", "title", "kind", "features", "webgpu", "shows", "controls"];
const REQUIRED = ["example", "title", "kind", "webgpu", "shows", "controls"];

export class ManifestError extends Error {}

/**
 * Parses examples.json: an array with one object per example, in the order
 * of the index page. Every field but `features` is required; unknown fields,
 * duplicates and bad values are refused with the example's name.
 */
export function parseManifest(text) {
  let entries;
  try {
    entries = JSON.parse(text);
  } catch (e) {
    throw new ManifestError(`examples.json is not JSON: ${e.message}`);
  }
  if (!Array.isArray(entries)) throw new ManifestError("examples.json must be an array of examples");
  const seen = new Set();
  return entries.map((entry, i) => {
    const name = entry?.example ?? `entry ${i + 1}`;
    const fail = (message) => {
      throw new ManifestError(`examples.json: ${name}: ${message}`);
    };
    if (typeof entry !== "object" || entry === null || Array.isArray(entry)) fail("must be an object");
    for (const key of Object.keys(entry)) {
      if (!KEYS.includes(key)) fail(`unknown field "${key}" (known: ${KEYS.join(", ")})`);
    }
    for (const key of REQUIRED) {
      if (typeof entry[key] !== "string" || !entry[key].trim()) fail(`needs a "${key}" (text)`);
    }
    if (!/^[a-z0-9_]+$/.test(entry.example)) fail("the example name may only hold a-z, 0-9 and _");
    if (seen.has(entry.example)) fail("is listed twice");
    seen.add(entry.example);
    if (!(entry.kind in KINDS)) fail(`"kind" must be one of ${Object.keys(KINDS).join(", ")}`);
    if (!WEBGPU.includes(entry.webgpu)) fail(`"webgpu" must be one of ${WEBGPU.join(", ")}`);
    const features = entry.features ?? [];
    if (!Array.isArray(features) || !features.every((f) => typeof f === "string" && /^[\w-]+$/.test(f))) {
      fail('"features" must be a list of Cargo feature names');
    }
    return { ...entry, features };
  });
}

/**
 * The `required-features` of every `[[example]]` in a Cargo.toml, by name
 * (examples without the key are absent). Enough TOML for the root manifest.
 */
export function requiredFeatures(cargoToml) {
  const found = {};
  for (const block of cargoToml.split(/^\[\[example\]\]\s*$/m).slice(1)) {
    const body = block.split(/^\[/m)[0];
    const name = /^name\s*=\s*"([^"]+)"/m.exec(body)?.[1];
    const features = /^required-features\s*=\s*\[([^\]]*)\]/m.exec(body)?.[1];
    if (name && features !== undefined) {
      found[name] = [...features.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    }
  }
  return found;
}

/** The version a ref names (`v0.2.0` -> `0.2.0`), or null for a branch or commit. */
export function versionOf(ref) {
  const match = /^v(\d+\.\d+\.\d+(?:-[\w.]+)?)$/.exec(ref);
  return match ? match[1] : null;
}

/** The example's source on GitLab at `ref`: a file (`examples/x.rs`) or a folder (`examples/x/`). */
export function sourceUrl(path, ref, repo = REPO) {
  const folder = path.endsWith("/");
  return `${repo}/-/${folder ? "tree" : "blob"}/${encodeURIComponent(ref)}/${folder ? path.slice(0, -1) : path}`;
}

/** The command that runs the example on the desktop. */
export function runCommand(entry) {
  const features = entry.features.length ? ` --features ${entry.features.join(",")}` : "";
  return `cargo run --release --example ${entry.example}${features}`;
}

export function escapeHtml(text) {
  return String(text).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
}

/** The badge text for an example's WebGPU needs. */
export function webgpuLabel(entry) {
  return entry.webgpu === "required" ? "Needs WebGPU" : "WebGPU or WebGL2";
}

function head(title, stylesheet) {
  return `<!doctype html>
<html lang="en"%ATTRS%>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>${escapeHtml(title)}</title>
  <link rel="icon" href="data:,">
  <link rel="stylesheet" href="${stylesheet}">
</head>`;
}

function label(site) {
  return site.version ? `Ponko ${site.version}` : `Ponko (${site.ref})`;
}

/**
 * One example's page: the demo in a box (the game draws into
 * `<canvas id="ponko">`), what it shows, its controls, its WebGPU needs and
 * its source at `site.ref`. `play.js` checks for WebGPU and starts the game.
 */
export function renderPage(entry, site) {
  const source = sourceUrl(entry.source, site.ref, site.repo);
  const attrs = ` data-example="${escapeHtml(entry.example)}" data-webgpu="${entry.webgpu}"`;
  const needs =
    entry.webgpu === "required"
      ? "Needs WebGPU: it uses GPU features that WebGL2 lacks. Without WebGPU the page says so instead of starting it."
      : "Runs with WebGPU, or with WebGL2 where the browser has no WebGPU (effects that need compute shaders are then off).";
  return `${head(`${entry.title} · ${label(site)} examples`, "../gallery.css").replace("%ATTRS%", attrs)}
<body class="example">
  <header>
    <a class="back" href="../">← All examples</a>
    <h1>${escapeHtml(entry.title)}</h1>
    <span class="badge kind">${KINDS[entry.kind]}</span>
    <span class="badge webgpu-${entry.webgpu}">${webgpuLabel(entry)}</span>
    <button id="fill" type="button" title="Fill the window with the game (again to go back)">Fill window</button>
  </header>
  <main>
    <div class="stage">
      <canvas id="ponko" tabindex="0"></canvas>
      <div id="status" role="status">Loading…</div>
    </div>
    <p id="backend-note" class="note" hidden></p>
    <section class="about">
      <p class="shows">${escapeHtml(entry.shows)}</p>
      <p><strong>Controls:</strong> ${escapeHtml(entry.controls)}</p>
      <p><strong>WebGPU:</strong> ${needs}</p>
      <p><a href="${escapeHtml(source)}">Source on GitHub</a> (${escapeHtml(site.ref)}). On the desktop:
        <code>${escapeHtml(runCommand(entry))}</code></p>
    </section>
  </main>
  <script type="module" src="../play.js"></script>
</body>
</html>
`;
}

/** The gallery's front page: every example, grouped by kind, in manifest order. */
export function renderIndex(entries, site) {
  const groups = Object.entries(KINDS)
    .map(([kind, name]) => {
      const cards = entries
        .filter((e) => e.kind === kind)
        .map(
          (e) => `      <li><a href="${e.example}/">
        <span class="title">${escapeHtml(e.title)}</span>
        <span class="badge webgpu-${e.webgpu}">${webgpuLabel(e)}</span>
        <span class="shows">${escapeHtml(e.shows)}</span>
      </a></li>`,
        )
        .join("\n");
      return cards ? `    <h2>${name}</h2>\n    <ul class="cards">\n${cards}\n    </ul>` : "";
    })
    .filter(Boolean)
    .join("\n");
  const tree = `${site.repo ?? REPO}/tree/${encodeURIComponent(site.ref)}/examples`;
  return `${head(`${label(site)} examples in the browser`, "gallery.css").replace("%ATTRS%", "")}
<body class="index">
  <header>
    <h1>${escapeHtml(label(site))}: the examples, in your browser</h1>
  </header>
  <main>
    <p>Each demo is an example from the engine's repository, compiled to
      WebAssembly. They draw with WebGPU where the browser has it and with
      WebGL2 where it doesn't; the few that need WebGPU say so. Every page
      links the example's <a href="${escapeHtml(tree)}">source</a> at ${escapeHtml(site.ref)}.</p>
${groups}
    <p class="links"><a href="${REPO}">Ponko on GitHub</a> ·
      <a href="https://ponko.dev/guide/">The guide</a> ·
      <a href="https://ponko.dev/guide/web.html">Building your game for the web</a></p>
  </main>
</body>
</html>
`;
}
