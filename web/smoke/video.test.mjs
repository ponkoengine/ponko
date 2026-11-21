// The video runner's own checks (video-lib.mjs), without a browser.
import { test } from "node:test";
import assert from "node:assert/strict";
import { UsageError } from "./lib.mjs";
import { MAX_LAG, MAX_SEEK_UPDATES, SITE, buildArgs, checkVideo, parseArgs, videoDone } from "./video-lib.mjs";

const good = () => ({
  phase: "done",
  shown: Array.from({ length: 20 }, (_, i) => i),
  max_lag: 1,
  wrong_frame: 0,
  wrong_quarters: 0,
  seek_13: 3,
  seek_4: 2,
  finished: true,
  error: null,
});

test("a clean run passes", () => {
  assert.deepEqual(checkVideo(good()), []);
  assert.ok(videoDone({ video: good() }));
  assert.ok(!videoDone({ video: { ...good(), phase: "playing" } }));
  assert.ok(!videoDone(null));
});

test("every way the fixture can go wrong is named", () => {
  const cases = [
    [{ error: "the browser's decoder stopped" }, /video failed/],
    [{ shown: [0, 1, 2, 19] }, /only 4 of the clip's 20 frames/],
    [{ shown: [0, 2, 1, ...Array.from({ length: 17 }, (_, i) => i + 3)] }, /out of order/],
    [{ shown: Array.from({ length: 19 }, (_, i) => i) }, /last frame shown was 18/],
    [{ wrong_frame: 2 }, /another frame than frame_time names/],
    [{ wrong_quarters: 1 }, /quarters wrong/],
    [{ max_lag: MAX_LAG + 1 }, /frames behind its clock/],
    [{ finished: false }, /never finished/],
    [{ seek_13: null }, /frame 13 while paused never showed/],
    [{ seek_4: MAX_SEEK_UPDATES + 1 }, /frame 4 took/],
  ];
  for (const [change, message] of cases) {
    const problems = checkVideo({ ...good(), ...change });
    assert.equal(problems.length, 1, `${JSON.stringify(change)}: ${problems}`);
    assert.match(problems[0], message);
  }
  assert.deepEqual(checkVideo(undefined), ["the fixture published no video state"]);
});

test("arguments", () => {
  assert.deepEqual(parseArgs([]), { build: false, dir: SITE, browser: "chromium", out: "target/web-smoke/video", timeout: 60 });
  const args = parseArgs(["--build", "--dir", "site", "--browser", "firefox", "--timeout", "5", "--out", "o"]);
  assert.deepEqual(args, { build: true, dir: "site", browser: "firefox", out: "o", timeout: 5 });
  assert.throws(() => parseArgs(["--browser", "lynx"]), UsageError);
  assert.throws(() => parseArgs(["--timeout", "0"]), UsageError);
  assert.throws(() => parseArgs(["--dir"]), UsageError);
  assert.throws(() => parseArgs(["--loud"]), UsageError);
  assert.deepEqual(buildArgs("x"), ["web/build.sh", "--features", "video", "web_video", "x"]);
});
