// The gallery's manifest and pages, without a build or a browser:
// `node --test web/gallery`.
import assert from "node:assert/strict";
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  GALLERY_URL,
  ManifestError,
  REPO,
  escapeHtml,
  pagesUrl,
  parseManifest,
  parseProjectUrl,
  renderIndex,
  renderPage,
  requiredFeatures,
  runCommand,
  sourceUrl,
  versionOf,
} from "./lib.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..", "..");
const manifestText = readFileSync(join(HERE, "examples.json"), "utf8");

const entry = (fields = {}) => ({
  example: "battle",
  title: "Battle",
  kind: "2d",
  webgpu: "optional",
  shows: "Two armies.",
  controls: "Click.",
  ...fields,
});
const parse = (entries) => parseManifest(JSON.stringify(entries));

test("every listed example exists and asks for the features Cargo.toml requires", () => {
  const entries = parseManifest(manifestText);
  const required = requiredFeatures(readFileSync(join(ROOT, "Cargo.toml"), "utf8"));
  assert.ok(entries.length >= 20, `only ${entries.length} examples listed`);
  for (const e of entries) {
    const file = existsSync(join(ROOT, "examples", `${e.example}.rs`)) || existsSync(join(ROOT, "examples", e.example, "main.rs"));
    assert.ok(file, `examples/${e.example} does not exist`);
    for (const feature of required[e.example] ?? []) {
      assert.ok(e.features.includes(feature), `${e.example} needs the ${feature} feature`);
    }
  }
});

test("every example is in the gallery or named in the README with why it isn't", () => {
  const listed = new Set(parseManifest(manifestText).map((e) => e.example));
  const readme = readFileSync(join(HERE, "README.md"), "utf8");
  const start = readme.indexOf("### Which examples are in, and why the rest aren't");
  const why = readme.slice(start, readme.indexOf("\n## ", start));
  const examples = readdirSync(join(ROOT, "examples"), { withFileTypes: true })
    .filter((d) => (d.isFile() && d.name.endsWith(".rs")) || (d.isDirectory() && existsSync(join(ROOT, "examples", d.name, "main.rs"))))
    .map((d) => d.name.replace(/\.rs$/, ""));
  assert.ok(examples.length > listed.size);
  const unexplained = examples.filter((name) => !listed.has(name) && !why.includes(`\`${name}\``));
  assert.deepEqual(unexplained, [], "add each to examples.json, or name it in the README's \"Which examples are in\" with the reason");
});

test("the gallery has 2D examples, a 3D scene and a demo that needs WebGPU", () => {
  const entries = parseManifest(manifestText);
  assert.ok(entries.some((e) => e.kind === "2d"));
  assert.ok(entries.some((e) => e.kind === "3d" && e.webgpu === "optional"), "a 3D scene that runs on WebGL2 too");
  assert.ok(entries.some((e) => e.webgpu === "required"));
});

test("examples.json keeps one example per line, so adding one is a one-line change", () => {
  const lines = manifestText.trim().split("\n");
  assert.equal(lines[0], "[");
  assert.equal(lines.at(-1), "]");
  for (const line of lines.slice(1, -1)) {
    assert.match(line, /^ {2}\{"example": "[a-z0-9_]+".*\},?$/, `not one example on one line: ${line.slice(0, 60)}`);
  }
});

test("a manifest with a missing, unknown or bad field is refused with the example's name", () => {
  assert.equal(parse([entry()])[0].features.length, 0, "features default to none");
  assert.throws(() => parse([entry({ shows: "" })]), /battle: needs a "shows"/);
  assert.throws(() => parse([entry({ colour: "red" })]), /battle: unknown field "colour"/);
  assert.throws(() => parse([entry({ webgpu: "maybe" })]), /"webgpu" must be one of optional, required/);
  assert.throws(() => parse([entry({ kind: "4d" })]), /"kind" must be one of 2d, 3d/);
  assert.throws(() => parse([entry({ features: "3d" })]), /"features" must be a list/);
  assert.throws(() => parse([entry(), entry()]), /battle: is listed twice/);
  assert.throws(() => parse([entry({ example: "../x" })]), /may only hold/);
  assert.throws(() => parseManifest("{}"), ManifestError);
  assert.throws(() => parseManifest("[,]"), /not JSON/);
});

test("required features are read from every [[example]] of a Cargo.toml", () => {
  const toml = '[package]\nname = "x"\n\n[[example]]\nname = "a"\nrequired-features = ["3d", "physics"]\n\n[[example]]\nname = "b"\n\n[features]\nx = []\n\n[[example]]\nname = "c"\nrequired-features = ["navmesh"]\n';
  assert.deepEqual(requiredFeatures(toml), { a: ["3d", "physics"], c: ["navmesh"] });
});

test("source links point at the ref: a file as a blob, a folder as a tree", () => {
  assert.equal(sourceUrl("examples/battle.rs", "v0.2.0"), "https://github.com/ponkoengine/ponko/blob/v0.2.0/examples/battle.rs");
  assert.equal(sourceUrl("examples/coin_dash/", "main"), "https://github.com/ponkoengine/ponko/tree/main/examples/coin_dash");
  assert.equal(versionOf("v0.2.0"), "0.2.0");
  assert.equal(versionOf("v0.2.0-rc.1"), "0.2.0-rc.1");
  assert.equal(versionOf("main"), null);
});

test("an example's page has the demo, what it shows, its controls, its WebGPU needs and its source", () => {
  const e = { ...entry({ features: ["physics"] }), source: "examples/battle.rs" };
  const html = renderPage(e, { ref: "v0.2.0", version: "0.2.0" });
  assert.match(html, /<canvas id="ponko"/);
  assert.match(html, /data-webgpu="optional"/);
  assert.match(html, /Two armies\./);
  assert.match(html, /Controls:<\/strong> Click\./);
  assert.match(html, /WebGPU or WebGL2/);
  assert.match(html, /href="https:\/\/github.com\/ponkoengine\/ponko\/blob\/v0.2.0\/examples\/battle.rs"/);
  assert.match(html, /cargo run --release --example battle --features physics/);
  assert.match(html, /<title>Battle · Ponko 0\.2\.0 examples<\/title>/);
  assert.match(html, /src="\.\.\/play\.js"/);
  const required = renderPage({ ...e, webgpu: "required" }, { ref: "main", version: null });
  assert.match(required, /data-webgpu="required"/);
  assert.match(required, /Needs WebGPU/);
  assert.match(required, /Ponko \(main\)/);
});

test("text from the manifest is escaped in the pages", () => {
  assert.equal(escapeHtml(`<a href="x">&'`), "&lt;a href=&quot;x&quot;&gt;&amp;&#39;");
  const html = renderPage({ ...parse([entry({ shows: "<script>alert(1)</script>" })])[0], source: "examples/battle.rs" }, { ref: "main" });
  assert.ok(!html.includes("<script>alert"));
});

test("the index lists every example under its kind, in manifest order", () => {
  const entries = [entry(), entry({ example: "3d", title: "3D scene", kind: "3d" }), entry({ example: "flow", title: "Flow" })];
  const html = renderIndex(entries, { ref: "v0.2.0", version: "0.2.0" });
  assert.ok(html.indexOf("<h2>2D</h2>") < html.indexOf('href="battle/"'));
  assert.ok(html.indexOf('href="battle/"') < html.indexOf('href="flow/"'));
  assert.ok(html.indexOf('href="flow/"') < html.indexOf("<h2>3D</h2>"));
  assert.ok(html.indexOf("<h2>3D</h2>") < html.indexOf('href="3d/"'));
  assert.match(html, /Ponko 0\.2\.0: the examples, in your browser/);
  assert.match(html, /tree\/v0\.2\.0\/examples/);
  assert.ok(!renderIndex([entry()], { ref: "main" }).includes("<h2>3D</h2>"), "an empty group is left out");
});

test("the run command names the example's features", () => {
  assert.equal(runCommand(entry({ features: [] })), "cargo run --release --example battle");
  assert.equal(runCommand(entry({ features: ["physics3d", "x"] })), "cargo run --release --example battle --features physics3d,x");
});

test("the README and the guide point to the gallery's chapter and the release notes link the gallery", () => {
  // The docs link the guide's web chapter, not the gallery's host (scripts/project-url.py keeps old hosts out).
  assert.ok(readFileSync(join(ROOT, "README.md"), "utf8").includes("https://ponko.dev/guide/web.html"), "README.md");
  assert.ok(readFileSync(join(ROOT, "book", "src", "web.md"), "utf8").includes("web/gallery/"), "book/src/web.md");
  // release-check.py derives the same address from PROJECT_URL (its self-test checks the rule).
  const releaseCheck = readFileSync(join(ROOT, "scripts", "release-check.py"), "utf8");
  assert.ok(releaseCheck.includes("pages_url(project_url())"), "scripts/release-check.py reads PROJECT_URL");
  assert.ok(!releaseCheck.includes(GALLERY_URL), "scripts/release-check.py holds no copy of the address");
});

test("the repository and the gallery's address come from PROJECT_URL", () => {
  assert.equal(REPO, readFileSync(join(ROOT, "PROJECT_URL"), "utf8").trim());
  assert.equal(GALLERY_URL, pagesUrl(REPO));
  assert.equal(parseProjectUrl("https://github.com/Some-Org/proj\n"), "https://github.com/Some-Org/proj");
  assert.equal(pagesUrl("https://github.com/Some-Org/proj"), "https://some-org.github.io/proj/", "the host is lower case, the repository is as written");
  for (const bad of ["", "http://github.com/o/r", "https://github.com", "https://github.com/o/r/", "github.com/o/r", "https://github.com/o r"]) {
    assert.throws(() => parseProjectUrl(bad), /PROJECT_URL must be one https URL/, JSON.stringify(bad));
  }
  assert.throws(() => pagesUrl("https://git.example.test/o/r"), /no GitHub Pages address/);
  assert.throws(() => pagesUrl("https://github.com/o/r/extra"), /no GitHub Pages address/);
});
