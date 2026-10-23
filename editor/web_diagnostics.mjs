// Optional browser-frame measurements. These report main-thread presentation cadence,
// not GPU timings or total process memory. Enable with ?diagnostics.
if (new URLSearchParams(location.search).has("diagnostics")) {
  const panel = document.createElement("output");
  panel.id = "web-diagnostics";
  panel.setAttribute("aria-label", "Browser performance");
  panel.style.cssText = "position:fixed;right:8px;bottom:44px;z-index:20;pointer-events:none;white-space:pre;font:12px monospace;color:#f1f4f8;background:#242a33e8;border:1px solid #66758a;border-radius:5px;padding:8px";
  document.body.append(panel);
  let last = 0, since = 0, frames = [], memory = null;
  // Trunk's public startup event exposes the initialized module exports.
  addEventListener("TrunkApplicationStarted", event => {
    if (event.detail?.wasm?.memory instanceof WebAssembly.Memory) memory = event.detail.wasm.memory;
  });
  document.addEventListener("visibilitychange", () => { last = 0; since = 0; frames = []; });
  function frame(now) {
    if (!document.hidden && !document.getElementById("loading")) {
      if (last) frames.push(now - last);
      if (!since) since = now;
      if (now - since >= 2000 && frames.length) {
        const sorted = [...frames].sort((a, b) => a - b);
        const canvas = document.getElementById("rysmo");
        const editorStale = panel.dataset.editorUpdated && Date.now() - Number(panel.dataset.editorUpdated) > 3500;
        panel.textContent = `Browser frames: ${(1000 * frames.length / frames.reduce((a,b)=>a+b,0)).toFixed(1)} FPS\nFrame p95: ${sorted[Math.min(sorted.length-1,Math.floor(sorted.length*0.95))].toFixed(1)} ms\nCanvas: ${canvas.width} × ${canvas.height}${memory ? `\nWASM memory: ${(memory.buffer.byteLength / 1048576).toFixed(1)} MiB allocated` : ""}${panel.dataset.editor ? `\n${panel.dataset.editor}${editorStale ? "\nEditor update report: stale" : ""}` : ""}`;
        frames = []; since = now;
      }
      last = now;
    } else { last = 0; since = 0; frames = []; }
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);
}
