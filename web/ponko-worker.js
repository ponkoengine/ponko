// One worker thread of a Ponko web game built with threads (the guide's
// "Threads on the web"). The game starts these itself on a cross-origin
// isolated page: each gets the compiled module, the game's shared memory and
// a task, and this file only connects them. It sits next to game.js.
import { initSync, ponkoWorkerStart, ponkoWorkerFailed } from "./game.js";

self.onmessage = ({ data }) => {
  self.onmessage = null;
  initSync({ module: data.module, memory: data.memory });
  self.postMessage("ready");
  try {
    // Returns only when the game ends.
    ponkoWorkerStart(data.task);
  } catch (error) {
    // A panic aborts the worker: the game stops too, instead of waiting
    // for this thread forever.
    ponkoWorkerFailed();
    self.postMessage({ failed: String(error) });
    throw error;
  }
};
