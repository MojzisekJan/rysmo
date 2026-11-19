// Optional browser-frame measurements. These report main-thread presentation cadence,
// not GPU timings or total process memory. Enable with ?diagnostics.
if (new URLSearchParams(location.search).has("diagnostics")) {
  const panel = document.createElement("output");
  panel.id = "web-diagnostics";
  panel.setAttribute("aria-label", "Browser performance");
  panel.style.cssText = "position:fixed;right:8px;bottom:44px;z-index:20;pointer-events:none;white-space:pre;font:12px monospace;color:#f1f4f8;background:#242a33e8;border:1px solid #66758a;border-radius:5px;padding:8px";
  document.body.append(panel);
  // Export through an ordinary UI action so manual cursor checks can retain
  // phase timings without reaching into the application's private state.
  const controls = document.createElement("div");
  controls.style.cssText = "position:fixed;right:8px;bottom:8px;z-index:21;display:flex;gap:6px";
  const reset = document.createElement("button");
  reset.textContent = "Reset profile";
  const download = document.createElement("button");
  download.textContent = "Download profile";
  const view = document.createElement("button");
  view.textContent = "View profile";
  controls.append(reset, view, download);
  document.body.append(controls);
  let started = performance.now(), longTasks = [], droppedLongTasks = 0;
  if (typeof PerformanceObserver !== "undefined" && PerformanceObserver.supportedEntryTypes?.includes("longtask")) {
    new PerformanceObserver(list => {
      for (const entry of list.getEntries()) {
        if (entry.startTime < started) continue;
        if (longTasks.length >= 4096) { longTasks.shift(); droppedLongTasks++; }
        longTasks.push({ start: entry.startTime, duration: entry.duration });
      }
    }).observe({ type: "longtask" });
  }
  reset.addEventListener("click", () => {
    started = performance.now(); longTasks = []; droppedLongTasks = 0;
    const profile = window.rysmoPerformance;
    if (profile) { profile.samples = []; profile.milestones = []; profile.dropped = 0; }
    reset.textContent = "Profile reset";
  });
  function report() {
    const profile = window.rysmoPerformance;
    return {
      started, ended: performance.now(), longTasks, droppedLongTasks,
      longTasksSupported: typeof PerformanceObserver !== "undefined" && PerformanceObserver.supportedEntryTypes?.includes("longtask"),
      samples: profile?.samples ?? [], milestones: profile?.milestones ?? [], droppedSamples: profile?.dropped ?? 0,
      wasmBytes: memory?.buffer.byteLength ?? null, editor: panel.dataset.editor ?? null,
    };
  }
  view.addEventListener("click", () => {
    const dialog = document.createElement("dialog");
    dialog.style.cssText = "width:min(800px,85vw);padding:20px;border:1px solid #66758a;border-radius:8px";
    const title = document.createElement("h2");
    title.textContent = "Performance profile";
    const output = document.createElement("textarea");
    output.setAttribute("aria-label", "Performance profile JSON");
    output.readOnly = true;
    output.style.cssText = "box-sizing:border-box;width:100%;height:55vh;font:12px monospace";
    output.textContent = JSON.stringify(report(), null, 2);
    const close = document.createElement("button");
    close.textContent = "Close";
    close.style.cssText = "display:block;margin:12px 0 0 auto;padding:6px 16px";
    close.addEventListener("click", () => dialog.close());
    dialog.addEventListener("close", () => dialog.remove());
    dialog.append(title, output, close);
    document.body.append(dialog);
    dialog.showModal();
  });
  download.addEventListener("click", () => {
    const url = URL.createObjectURL(new Blob([JSON.stringify(report(), null, 2)], { type: "application/json" }));
    const link = document.createElement("a");
    link.href = url; link.download = "rysmo-profile.json";
    document.body.append(link); link.click(); link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  });
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
