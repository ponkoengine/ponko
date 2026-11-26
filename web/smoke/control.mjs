#!/usr/bin/env node
// Verify the browser control transport through the existing MCP tools.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { setTimeout as delay } from "node:timers/promises";
import { chromium } from "playwright-core";
import { launchConfig, withDeadline } from "./lib.mjs";
import { canvasStats, serve } from "./smoke.mjs";

const [dir, mcp, output = "target/web-smoke/web-control.png"] = process.argv.slice(2);
if (!dir || !mcp) {
  console.error("usage: node web/smoke/control.mjs <built game dir> <ponko-mcp binary> [output PNG]");
  process.exit(3);
}

const scratch = await mkdtemp(join(tmpdir(), "ponko-web-control-"));
const env = { ...process.env, PONKO_INSTANCES_DIR: join(scratch, "instances"), TMPDIR: scratch, TEMP: scratch };
const children = [];

function start(args) {
  const child = spawn(resolve(mcp), args, { env, stdio: ["pipe", "pipe", "inherit"] });
  children.push(child);
  const lines = createInterface({ input: child.stdout })[Symbol.asyncIterator]();
  const failed = new Promise((_, reject) => {
    child.once("error", reject);
    child.stdin.on("error", reject);
  });
  failed.catch(() => {});
  const nextLine = async () => {
    const { value, done } = await withDeadline(Promise.race([failed, lines.next()]), 10_000, "ponko-mcp reply");
    assert(!done, "ponko-mcp closed stdout");
    return value;
  };
  return { child, nextLine };
}

let browser;
let server;
const timeout = setTimeout(() => {
  for (const child of children) child.kill();
  console.error("browser control smoke exceeded its 90-second deadline");
  process.exit(1);
}, 90_000);
try {
  server = await serve(resolve(dir));
  const url = `http://127.0.0.1:${server.address().port}/`;
  const bridge = start(["--web-bridge"]);
  const fragment = await bridge.nextLine();
  assert(fragment.startsWith("#ponko-control=ws://127.0.0.1:"), "bridge printed no local browser URL");

  const { child, nextLine } = start([]);
  let id = 0;
  async function tool(name, args = {}) {
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id: ++id, method: "tools/call", params: { name, arguments: args } }) + "\n");
    const reply = JSON.parse(await nextLine());
    assert.equal(reply.id, id);
    assert(reply.result, JSON.stringify(reply));
    assert(!reply.result.isError, reply.result.content[0].text);
    return reply.result.content;
  }
  const json = async (name, args) => JSON.parse((await tool(name, args))[0].text);

  const { options } = launchConfig("chromium", process.platform);
  browser = await chromium.launch(options);
  const context = await browser.newContext({ viewport: { width: 800, height: 600 } });
  await context.addInitScript(() => {
    Object.defineProperty(Navigator.prototype, "gpu", { get: () => undefined, configurable: true });
  });
  const errors = [];
  const page = await context.newPage();
  page.on("pageerror", (e) => errors.push(e.message));
  const waitForGame = () => page.waitForFunction(() => {
    const state = JSON.parse(document.documentElement.getAttribute("data-smoke") ?? "{}");
    return state.ticks >= 2;
  }, undefined, { timeout: 20_000 });
  await page.goto(url, { timeout: 20_000 });
  await waitForGame();
  assert.deepEqual(await json("list_instances"), [], "a feature-enabled build connected without the URL opt-in");

  await page.goto(url + fragment, { timeout: 20_000 });
  await page.reload({ timeout: 20_000 });
  await waitForGame();
  let instances = [];
  for (let attempts = 0; instances.length === 0 && attempts < 50; attempts++) {
    instances = await json("list_instances");
    if (instances.length === 0) await delay(20);
  }
  const [tab, ...others] = instances;
  assert.equal(tab?.transport, "web", "browser is registered as a control instance");
  assert.equal(others.length, 0);
  assert(!Object.hasOwn(tab, "token"), "MCP exposed the native control token");
  const instance = tab.id;
  const capabilities = await json("instance_capabilities", { instance });
  assert.equal(capabilities.subsystems.control.transport, "web");
  assert.equal(capabilities.subsystems.renderer.adapter.backend, "gl");
  assert.equal(capabilities.methods.find((m) => m.name === "world.load").available, false);
  const state = await json("world_summary", { instance });
  assert(Number.isInteger(state.entities), "MCP returned the world's state");
  const paused = await json("pause", { instance });
  assert.equal(paused.paused, true);
  const stepped = await json("run_ticks", { instance, ticks: 7, timeout_secs: 5 });
  assert.equal(stepped.ticks_run, 7);
  assert.equal(stepped.status.tick, paused.tick + 7);
  const fixtureTick = () => JSON.parse(document.documentElement.getAttribute("data-smoke")).ticks;
  const gameTick = await page.evaluate(fixtureTick);
  assert.equal(gameTick, stepped.status.tick, "the game's simulation system ran on all seven ticks");
  const shot = await tool("screenshot", { instance, max_size: 320 });
  const image = shot.find((item) => item.type === "image");
  assert.equal(image?.mimeType, "image/png");
  const png = Buffer.from(image.data, "base64");
  const pixels = await withDeadline(canvasStats(page, png), 5_000, "decode the MCP screenshot");
  assert.equal(Math.max(pixels.width, pixels.height), 320);
  assert(pixels.colors > 8 && pixels.foreground > 0.01, `blank screenshot: ${JSON.stringify(pixels)}`);
  assert.equal((await json("instance_info", { instance })).tick, paused.tick + 7);
  assert.deepEqual(errors, [], "the game's page reported errors");
  await mkdir(dirname(output), { recursive: true });
  await writeFile(output, png);
  await page.close();
  for (let attempts = 0; (await json("list_instances")).length > 0; attempts++) {
    assert(attempts < 50, "the closed tab is still registered");
    await delay(20);
  }
  console.log(`Chromium WebGL2 (${capabilities.subsystems.renderer.adapter.name}): inspected, stepped 7 ticks, captured ${pixels.width}x${pixels.height} PNG (${pixels.colors} colors)`);
} finally {
  clearTimeout(timeout);
  for (const child of children) child.kill();
  try {
    if (browser) await withDeadline(browser.close(), 5_000, "close Chromium");
  } finally {
    server?.closeAllConnections();
    server?.close();
    await rm(scratch, { recursive: true, force: true });
  }
}
