// The browser replication runner's decisions (replication.mjs), without a browser.

import { test } from "node:test";
import assert from "node:assert/strict";
import { MIN_MOVE, checkReplication, parseArgs, parseServing } from "./replication.mjs";

const state = (extra = {}) => ({
  fixture: "net_arena",
  ticks: 120,
  joined: true,
  error: null,
  replicas: 25,
  orbs: 24,
  moved: 24,
  farthest: 12.5,
  mine: true,
  ...extra,
});

test("a tab that joined, holds its player and saw orbs move passes", () => {
  assert.deepEqual(checkReplication(state()), []);
});

test("a tab that reported nothing, didn't join or was refused fails", () => {
  assert.match(checkReplication(null).join("\n"), /reported nothing/);
  assert.match(checkReplication(state({ joined: false })).join("\n"), /did not join/);
  const refused = checkReplication(state({ joined: false, error: "the server replicates different components" }));
  assert.match(refused.join("\n"), /replication failed: the server replicates different components/);
});

test("a tab without replicas or its own player fails", () => {
  assert.match(checkReplication(state({ orbs: 0, replicas: 1 })).join("\n"), /no replicated orbs \(1 replicas\)/);
  assert.match(checkReplication(state({ mine: false })).join("\n"), /no player of its own/);
});

test("orbs that stood still, or barely moved, fail", () => {
  assert.match(checkReplication(state({ moved: 0, farthest: 0 })).join("\n"), /no replicated orb moved/);
  assert.match(checkReplication(state({ moved: 3, farthest: MIN_MOVE - 0.5 })).join("\n"), /farthest 4\.50/);
  assert.deepEqual(checkReplication(state({ moved: 1, farthest: MIN_MOVE })), []);
});

test("the server's address line is found among its log lines", () => {
  const lines = ['{"level":"info","msg":"starting"}', "serving on 127.0.0.1:4100, browsers on ws://127.0.0.1:4101"];
  assert.deepEqual(parseServing(lines), { udp: "127.0.0.1:4100", web: "ws://127.0.0.1:4101" });
  assert.equal(parseServing(["nothing yet"]), null);
  assert.equal(parseServing(undefined), null);
});

test("arguments: defaults, values and mistakes", () => {
  const args = parseArgs([]);
  assert.equal(args.browser, "chromium");
  assert.equal(args.runTimeout, 120);
  assert.equal(parseArgs(["--browser", "firefox", "--run-timeout", "60"]).browser, "firefox");
  assert.throws(() => parseArgs(["--browser", "lynx"]), /unknown browser/);
  assert.throws(() => parseArgs(["--run-timeout", "5"]), /at least 30/);
  assert.throws(() => parseArgs(["--dir"]), /needs a value/);
  assert.throws(() => parseArgs(["--fast"]), /unknown argument/);
});
