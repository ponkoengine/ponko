// The browser multiplayer runner's decisions (multiplayer.mjs), without a browser.

import { test } from "node:test";
import assert from "node:assert/strict";
import { checkSession, parseArgs, parseHosting } from "./multiplayer.mjs";

const peer = (name, player, extra = {}) => ({
  name,
  state: { player, players: 4, state: "Running", fingerprint: "00ff", steered: [10, 20, 30, 40], desync: null, error: null, ...extra },
});
const session = () => [peer("native host", 0), peer("native client", 1), peer("tab 1", 2), peer("tab 2", 3)];

test("four peers with one world pass", () => {
  assert.deepEqual(checkSession(session()), []);
});

test("a different world, desync or lost session fails and names the peer", () => {
  const peers = session();
  peers[2].state.fingerprint = "0000";
  assert.match(checkSession(peers).join("\n"), /tab 1's world \(0000\) differs/);
  const desynced = session();
  desynced[3].state.desync = "desync with player 0 at tick 59: Crab differ";
  assert.match(checkSession(desynced).join("\n"), /tab 2 saw a desync/);
  const lost = session();
  lost[1].state.error = "player 0 stopped answering";
  assert.match(checkSession(lost).join("\n"), /native client lost the session/);
});

test("a peer that never reached the fingerprint tick, or reported nothing, fails", () => {
  const peers = session();
  peers[3].state.fingerprint = null;
  assert.match(checkSession(peers).join("\n"), /tab 2 never reached the fingerprint tick/);
  const silent = session();
  silent[2].state = null;
  assert.match(checkSession(silent).join("\n"), /tab 1 reported nothing/);
});

test("an input that never crossed fails", () => {
  const peers = session().map((p) => ({ ...p, state: { ...p.state, steered: [10, 20, 0, 40] } }));
  assert.match(checkSession(peers).join("\n"), /player 2's input never reached the others/);
  const disagree = session();
  disagree[3].state.steered = [10, 20, 30, 39];
  assert.match(checkSession(disagree).join("\n"), /tab 2 saw steering/);
});

test("players must be numbered once each and the count must match", () => {
  const twice = session();
  twice[3].state.player = 2;
  assert.match(checkSession(twice).join("\n"), /each should have its own number/);
  const fewer = session();
  fewer[0].state.players = 3;
  assert.match(checkSession(fewer).join("\n"), /native host plays with 3 players/);
});

test("the host's first line gives both addresses", () => {
  assert.deepEqual(parseHosting("hosting 4 players on 127.0.0.1:5000, browsers on ws://127.0.0.1:5001"), {
    players: 4,
    udp: "127.0.0.1:5000",
    web: "ws://127.0.0.1:5001",
  });
  assert.equal(parseHosting("hosting 2 players on 0.0.0.0:7777"), null);
  assert.equal(parseHosting(undefined), null);
});

test("arguments", () => {
  assert.equal(parseArgs([]).ticks, 720);
  assert.equal(parseArgs(["--ticks", "900", "--browser", "firefox"]).browser, "firefox");
  assert.throws(() => parseArgs(["--ticks", "100"]), /at least 400/);
  assert.throws(() => parseArgs(["--browser", "lynx"]), /unknown browser/);
  assert.throws(() => parseArgs(["--dir"]), /needs a value/);
  assert.throws(() => parseArgs(["--what"]), /unknown argument/);
});
