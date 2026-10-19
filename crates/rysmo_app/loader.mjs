// Trunk initializer: shows download progress on the loading screen. The app removes
// the screen itself once its first frames are drawn (crates/rysmo_app/src/web.rs).
//
// Trunk's own total can be the size before wasm-opt shrank the file, so the real size is
// asked of the server (the Content-Length of the preloaded wasm) when it can be.
const loading = () => document.getElementById("loading");
const status = (text) => { const l = loading(); if (l) l.querySelector(".status").textContent = text; };
const mb = (n) => (n / 1048576).toFixed(1);
let realTotal = 0;

export default function () {
  return {
    onStart: () => {
      status("Downloading...");
      realTotal = Number(document.querySelector('meta[name="rysmo-wasm-bytes"]')?.content) || 0;
      if (realTotal > 0) return;
      const wasm = document.querySelector('link[rel="preload"][href$=".wasm"]');
      if (wasm) {
        fetch(wasm.href, { method: "HEAD" })
          .then((r) => { realTotal = Number(r.headers.get("content-length")) || 0; })
          .catch(() => {});
      }
    },
    onProgress: ({ current }) => {
      const l = loading();
      if (!l) return;
      if (realTotal) {
        l.classList.remove("waiting");
        l.querySelector(".fill").style.width = `${Math.min(100, (current / realTotal) * 100)}%`;
        status(`Downloading ${mb(Math.min(current, realTotal))} of ${mb(realTotal)} MB`);
      } else {
        status(`Downloading ${mb(current)} MB`);
      }
    },
    onComplete: () => status("Starting..."),
    onSuccess: () => status("Starting..."),
    onFailure: (error) => {
      const l = loading();
      if (!l) return;
      l.className = "failed";
      status(`Rysmo could not start: ${error}`);
    },
  };
}
