#!/usr/bin/env node
// First-load cost of web builds: how many requests a page makes, how many
// bytes arrive and how long until its assets are in and its first frame is
// drawn, on an emulated network, in headless Chromium (no window).
//
//   node web/smoke/first-load.mjs --dir target/web-loose/ember-hop-0.1.0-web \
//        --dir target/web-pack/ember-hop-0.1.0-web [--runs 5] \
//        [--latency 40] [--mbps 20] [--json out.json]
//
// Each folder is served over HTTP/1.1 by a server that answers range
// requests (as static hosts and CDNs do), with the browser cache off. Each
// run opens the page fresh and waits for the game's first published frame
// (`<html data-smoke>`, which the reference games and the web fixtures
// set every frame), then reads the page's resource timings. It prints the
// median of the runs per folder:
//
//   requests   every request the page made before its first frame
//   assets     the requests for asset files (assets/..., *.pack)
//   kB         bytes received (bodies) before the first frame
//   assets in  ms from navigation start until the last asset response ended
//   asset phase ms from the first asset request until the last asset
//              response ended (the part assets cost, apart from the program)
//   first frame ms until the first frame was published
//
// The latency (ms, per round trip) and bandwidth (Mbit/s down) are
// Chromium's network emulation; --latency 0 --mbps 0 turns it off.

import { createServer } from "node:http";
import { readFile, writeFile } from "node:fs/promises";
import { extname, join, normalize, resolve, sep } from "node:path";
import { chromium } from "playwright-core";
import { launchConfig } from "./lib.mjs";

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript",
  ".wasm": "application/wasm",
  ".json": "application/json",
  ".png": "image/png",
};

function parse(argv) {
  const args = { dirs: [], runs: 5, latency: 40, mbps: 20, json: null, timeout: 60000 };
  for (let i = 0; i < argv.length; i++) {
    const value = () => {
      const v = argv[++i];
      if (v === undefined) throw new Error(`${argv[i - 1]} needs a value`);
      return v;
    };
    switch (argv[i]) {
      case "--dir": args.dirs.push(resolve(value())); break;
      case "--runs": args.runs = Number(value()); break;
      case "--latency": args.latency = Number(value()); break;
      case "--mbps": args.mbps = Number(value()); break;
      case "--json": args.json = value(); break;
      case "--timeout": args.timeout = Number(value()); break;
      default: throw new Error(`unknown argument ${argv[i]}`);
    }
  }
  if (!args.dirs.length) throw new Error("give at least one --dir");
  return args;
}

/// The `Range: bytes=a-b` of a request, clamped to the file, or null.
export function range(header, size) {
  const m = /^bytes=(\d*)-(\d*)$/.exec(header ?? "");
  if (!m || (m[1] === "" && m[2] === "")) return null;
  let start, end;
  if (m[1] === "") {
    start = Math.max(0, size - Number(m[2]));
    end = size - 1;
  } else {
    start = Number(m[1]);
    end = m[2] === "" ? size - 1 : Math.min(Number(m[2]), size - 1);
  }
  return start <= end && start < size ? { start, end } : "unsatisfiable";
}

function serve(root) {
  const server = createServer(async (req, res) => {
    const path = decodeURIComponent(new URL(req.url, "http://x").pathname);
    const file = normalize(join(root, path.endsWith("/") ? path + "index.html" : path));
    if (file !== root && !file.startsWith(root + sep)) {
      res.writeHead(403).end();
      return;
    }
    let body;
    try {
      body = await readFile(file);
    } catch {
      res.writeHead(404).end("not found");
      return;
    }
    const headers = {
      "content-type": MIME[extname(file)] ?? "application/octet-stream",
      "accept-ranges": "bytes",
      "cache-control": "no-store",
    };
    const r = range(req.headers.range, body.length);
    if (r === "unsatisfiable") {
      res.writeHead(416, { "content-range": `bytes */${body.length}` }).end();
    } else if (r) {
      res
        .writeHead(206, { ...headers, "content-range": `bytes ${r.start}-${r.end}/${body.length}` })
        .end(body.subarray(r.start, r.end + 1));
    } else {
      res.writeHead(200, headers).end(body);
    }
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => ok(server)));
}

function median(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = sorted.length >> 1;
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

async function run(browser, url, args) {
  const context = await browser.newContext();
  const page = await context.newPage();
  const cdp = await context.newCDPSession(page);
  await cdp.send("Network.enable");
  await cdp.send("Network.setCacheDisabled", { cacheDisabled: true });
  if (args.latency > 0 || args.mbps > 0) {
    const bytesPerSecond = args.mbps > 0 ? (args.mbps * 1e6) / 8 : -1;
    await cdp.send("Network.emulateNetworkConditions", {
      offline: false,
      latency: args.latency,
      downloadThroughput: bytesPerSecond,
      uploadThroughput: bytesPerSecond,
    });
  }
  const errors = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(url);
  await page.waitForFunction(() => document.documentElement.hasAttribute("data-smoke"), null, {
    timeout: args.timeout,
    polling: 16,
  });
  const result = await page.evaluate(() => {
    const frame = performance.now();
    const resources = performance.getEntriesByType("resource").filter((r) => r.startTime <= frame);
    const nav = performance.getEntriesByType("navigation")[0];
    const isAsset = (r) => /\/assets\/|\.pack(\?|$)/.test(r.name);
    const assets = resources.filter(isAsset);
    return {
      requests: resources.length + 1,
      assets: assets.length,
      bytes: resources.reduce((n, r) => n + r.encodedBodySize, nav.encodedBodySize),
      assetsIn: assets.length ? Math.max(...assets.map((r) => r.responseEnd)) : 0,
      assetPhase: assets.length
        ? Math.max(...assets.map((r) => r.responseEnd)) - Math.min(...assets.map((r) => r.startTime))
        : 0,
      firstFrame: frame,
      urls: resources.map((r) => r.name.replace(location.origin, "")),
    };
  });
  await context.close();
  if (errors.length) throw new Error(`page errors: ${errors.join("; ")}`);
  return result;
}

async function main() {
  const args = parse(process.argv.slice(2));
  const { options } = launchConfig("chromium", process.platform);
  const browser = await chromium.launch(options);
  const report = [];
  try {
    for (const dir of args.dirs) {
      const server = await serve(dir);
      const url = `http://127.0.0.1:${server.address().port}/`;
      const runs = [];
      for (let i = 0; i < args.runs; i++) runs.push(await run(browser, url, args));
      server.close();
      const m = (key) => median(runs.map((r) => r[key]));
      const row = {
        dir,
        runs: runs.length,
        requests: m("requests"),
        assets: m("assets"),
        kB: Math.round(m("bytes") / 1000),
        assetsInMs: Math.round(m("assetsIn")),
        assetPhaseMs: Math.round(m("assetPhase")),
        firstFrameMs: Math.round(m("firstFrame")),
        firstFrameMsRange: [Math.round(Math.min(...runs.map((r) => r.firstFrame))), Math.round(Math.max(...runs.map((r) => r.firstFrame)))],
        urls: runs[0].urls,
      };
      report.push(row);
      console.log(
        `${dir}\n  ${row.requests} requests (${row.assets} for assets), ${row.kB} kB, ` +
          `assets in at ${row.assetsInMs} ms (asset phase ${row.assetPhaseMs} ms), first frame at ${row.firstFrameMs} ms ` +
          `(${row.firstFrameMsRange.join("-")} ms over ${row.runs} runs)`,
      );
    }
  } finally {
    await browser.close();
  }
  console.log(`network: ${args.latency ? `${args.latency} ms latency` : "no added latency"}, ${args.mbps ? `${args.mbps} Mbit/s` : "unthrottled"}`);
  if (args.json) await writeFile(args.json, JSON.stringify({ network: { latency: args.latency, mbps: args.mbps }, report }, null, 2));
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((e) => {
    console.error(`first-load: ${e.message}`);
    process.exit(1);
  });
}
