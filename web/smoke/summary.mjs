#!/usr/bin/env node
// Lists every smoke run under a folder (their report.json files) as a
// Markdown table:
//
//   node web/smoke/summary.mjs target/web-smoke
//
// Exits 1 when the folder holds no report, so a step that never ran can't
// pass unnoticed; each run's own exit code already decided whether it failed.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { summaryMarkdown, summaryRow } from "./lib.mjs";

function reportsUnder(dir) {
  const found = [];
  for (const name of readdirSync(dir).sort()) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) found.push(...reportsUnder(path));
    else if (name === "report.json") found.push(path);
  }
  return found;
}

const dir = process.argv[2];
if (!dir) {
  console.error("usage: summary.mjs <folder of smoke artifacts>");
  process.exit(3);
}
const rows = reportsUnder(dir).map((path) => summaryRow(JSON.parse(readFileSync(path, "utf8"))));
if (rows.length === 0) {
  console.error(`summary: no report.json under ${dir}`);
  process.exit(1);
}
const markdown = summaryMarkdown(rows);
console.log(markdown);
