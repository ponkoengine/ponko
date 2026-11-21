// What web/smoke/video.mjs checks, kept apart from the browser so
// video.test.mjs can test it: the arguments, and the fixture's verdict.

import { BROWSERS, UsageError } from "./lib.mjs";

export const USAGE =
  "usage: node web/smoke/video.mjs [--build] [--dir <site>] [--browser chromium|firefox|webkit]\n" +
  "                                [--out <dir>] [--timeout <s>]";

/** Where `web/build.sh --features video web_video <dir>` puts the site by default. */
export const SITE = "target/web/web_video";

/** Frames the picture may be behind its clock while playing: the browser hands frames back later. */
export const MAX_LAG = 2;
/** Distinct frames of the clip's 20 that must show while it plays (a slow machine skips some). */
export const MIN_SHOWN = 12;
/** Updates a seek may take to show its frame. */
export const MAX_SEEK_UPDATES = 60;

export function parseArgs(argv) {
  const args = { build: false, dir: SITE, browser: "chromium", out: "target/web-smoke/video", timeout: 60 };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const value = () => {
      const v = argv[++i];
      if (v === undefined) throw new UsageError(`${flag} needs a value`);
      return v;
    };
    if (flag === "--build") args.build = true;
    else if (flag === "--dir") args.dir = value();
    else if (flag === "--out") args.out = value();
    else if (flag === "--browser") {
      args.browser = value();
      if (!BROWSERS.includes(args.browser)) throw new UsageError(`unknown browser ${args.browser}`);
    } else if (flag === "--timeout") {
      args.timeout = Number(value());
      if (!(args.timeout > 0)) throw new UsageError("--timeout needs a number of seconds above 0");
    } else throw new UsageError(`unknown argument ${flag}`);
  }
  return args;
}

/** The build command for the fixture. */
export function buildArgs(dir) {
  return ["web/build.sh", "--features", "video", "web_video", dir];
}

/** Whether the fixture is through (or stopped on an error). */
export function videoDone(state) {
  return state?.video?.phase === "done";
}

/**
 * The problems in the fixture's final `video` state, or none. Frames must
 * be the ones their time names, in order, with the quarters in place, at
 * most MAX_LAG frames behind the clock; the clip must reach its end; each
 * seek while paused must show its frame within MAX_SEEK_UPDATES updates.
 */
export function checkVideo(video) {
  if (!video) return ["the fixture published no video state"];
  const problems = [];
  if (video.error) problems.push(`the video failed: ${video.error}`);
  const shown = video.shown ?? [];
  if (shown.length < MIN_SHOWN) problems.push(`only ${shown.length} of the clip's 20 frames showed while it played: ${JSON.stringify(shown)}`);
  if (shown.some((f, i) => i > 0 && f <= shown[i - 1])) problems.push(`frames showed out of order: ${JSON.stringify(shown)}`);
  if (shown.length && shown[shown.length - 1] !== 19) problems.push(`the last frame shown was ${shown[shown.length - 1]}, not 19`);
  if (video.wrong_frame > 0) problems.push(`${video.wrong_frame} updates showed pixels of another frame than frame_time names`);
  if (video.wrong_quarters > 0) problems.push(`${video.wrong_quarters} updates showed the colored quarters wrong`);
  if (!(video.max_lag <= MAX_LAG)) problems.push(`the picture fell ${video.max_lag} frames behind its clock (at most ${MAX_LAG})`);
  if (!video.finished) problems.push("the clip never finished");
  for (const [key, frame] of [["seek_13", 13], ["seek_4", 4]]) {
    const updates = video[key];
    if (updates == null) problems.push(`seeking to frame ${frame} while paused never showed it`);
    else if (updates > MAX_SEEK_UPDATES) problems.push(`seeking to frame ${frame} took ${updates} updates to show (at most ${MAX_SEEK_UPDATES})`);
  }
  return problems;
}
