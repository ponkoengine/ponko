// Cross-origin isolation without server headers (the guide's "Threads on
// the web", "Hosts that can't send the headers"). One file, two roles:
//
// - On the page (`<script src="ponko-isolate.js"></script>` in <head>,
//   before the game's script): if the page isn't cross-origin isolated, it
//   registers this same file as a service worker and, once the worker is
//   active, reloads the page once. The reloaded page comes through the
//   worker, with the headers, so the game's threads can start.
// - As the service worker: it adds `Cross-Origin-Opener-Policy:
//   same-origin` and `Cross-Origin-Embedder-Policy: require-corp` (or
//   `credentialless`, see below) to every response from the page's own
//   origin, as a server would. Responses from other origins pass through
//   unchanged: under `require-corp` they need their own
//   `Cross-Origin-Resource-Policy` or CORS headers, as with real headers.
//
// It must be its own file, served from the game's origin beside index.html
// (a service worker controls the folder it is in and those below it), over
// HTTPS or from localhost. `ponko package --web --threads --isolation-worker`
// and `web/build.sh --threads --isolation-worker` add it.
//
// `<script src="ponko-isolate.js" data-coep="credentialless"></script>`
// sends `credentialless` instead of `require-corp`: other origins' files
// then load without cookies instead of needing to opt in. Browsers without
// it (Safari) then don't isolate the page, and the game runs on one thread.
//
// Where it can't work (no service workers, as in some private windows; not
// a secure context; a page inside another site's frame), it does nothing
// and the game runs on the page's thread alone, saying why in the console.
// It reloads at most once per tab session, so it can't loop.

/* global clients */
"use strict";

if (typeof window === "undefined") {
  // The service worker.
  const coep = new URL(self.location.href).searchParams.get("coep") === "credentialless" ? "credentialless" : "require-corp";
  self.addEventListener("install", () => self.skipWaiting());
  self.addEventListener("activate", (event) => event.waitUntil(clients.claim()));
  self.addEventListener("fetch", (event) => {
    const request = event.request;
    // Other origins' files: untouched (and a fetch the browser may answer
    // only from its cache is left to it).
    if (new URL(request.url).origin !== self.location.origin) return;
    if (request.cache === "only-if-cached" && request.mode !== "same-origin") return;
    event.respondWith(
      fetch(request).then((response) => {
        // Opaque or error responses can't take headers.
        if (response.status === 0) return response;
        const headers = new Headers(response.headers);
        headers.set("Cross-Origin-Opener-Policy", "same-origin");
        headers.set("Cross-Origin-Embedder-Policy", coep);
        return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
      }),
    );
  });
} else {
  // The page.
  (() => {
    const KEY = "ponko-isolate-reloaded";
    const storage = (() => {
      try {
        return window.sessionStorage;
      } catch {
        return null;
      }
    })();
    if (window.crossOriginIsolated) {
      storage?.removeItem(KEY);
      return;
    }
    const why = !window.isSecureContext
      ? "the page isn't a secure context (HTTPS or localhost)"
      : !("serviceWorker" in navigator)
        ? "this browser (or window) has no service workers"
        : window.top !== window
          ? "the page is inside a frame: the page around it must be isolated too"
          : storage == null
            ? "it can't remember that it reloaded once (no session storage)"
            : storage.getItem(KEY)
              ? "it reloaded once already and the page still isn't isolated"
              : null;
    if (why) {
      console.warn(`ponko-isolate.js: not isolating the page: ${why}`);
      return;
    }
    const script = document.currentScript;
    const url = new URL(script?.src ?? "ponko-isolate.js", window.location.href);
    if (script?.dataset.coep === "credentialless") url.searchParams.set("coep", "credentialless");
    navigator.serviceWorker
      .register(url.href)
      .then(() => navigator.serviceWorker.ready)
      .then(() => {
        // The worker is active: a reload comes through it, with the
        // headers. (This page loaded without them: on the first visit, or
        // after a reload that skipped the worker, such as Shift+Reload.)
        storage.setItem(KEY, "1");
        console.info("ponko-isolate.js: reloading once, through the service worker, for cross-origin isolation");
        window.location.reload();
      })
      .catch((e) => console.warn(`ponko-isolate.js: not isolating the page: ${e.message ?? e}`));
  })();
}
