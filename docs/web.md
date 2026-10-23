# Web build

Rysmo runs in the browser as WebAssembly, drawing with WebGL2, which every current
browser has (Firefox and Chromium on Linux included, where WebGPU is often missing).

## Building

You need the `wasm32-unknown-unknown` target and [trunk](https://trunkrs.dev) 0.21:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk
cd crates/rysmo_app
trunk build --release true      # into crates/rysmo_app/dist (about 11 minutes the first time)
trunk serve --release true      # or build and serve on http://127.0.0.1:8080
```

Trunk fetches the `wasm-bindgen` command matching the `wasm-bindgen` crate in
`Cargo.lock`, and `wasm-opt`, into its cache. `index.html` asks for `wasm-opt -Oz`.

The page is `crates/rysmo_app/index.html` with `loader.mjs`, trunk's initializer. The
build is the same `rysmo_app` crate with `target_arch = "wasm32"`:

- Bevy gets the `web` and `webgl2` features instead of `multi_threaded`, `x11` and
  `wayland` (see the target sections in `crates/rysmo_app/Cargo.toml`).
- `.cargo/config.toml` sets `getrandom_backend="wasm_js"` for the wasm target, and the
  app turns on getrandom's `wasm_js` feature, so getrandom 0.3 reads
  `crypto.getRandomValues`.
- The window is the page's `<canvas id="rysmo">`, sized to the page.
- The MCP server, the TCP port and the native file dialogs are left out.

Add `?demo` to the address to open the demo model.

For a ZIP with source and build metadata, use `scripts/build-web.sh` from the
repository root.

## Loading screen

Before downloading anything the page checks that the browser has WebGL2 and says so if
not. While the wasm file downloads, a bar shows the megabytes so far (from trunk's
initializer). When the download is done it says "Starting...", then "Preparing the 3D
view..." while the renderer compiles its shaders; the app removes the loading screen itself
after its first frames are drawn. Firefox on macOS links the main material shader slowly,
so this step can take several seconds there.

## Files and storage

The browser half is `crates/rysmo_app/src/web_files.js`, modelled on Heron Brush's;
the Rust half is `web.rs`.

- **Autosave.** At the interval selected in File > Settings > Application, pending changes
  to the open document go into IndexedDB
  (database `rysmo`, store `state`, key `current`, with its name and whether it has
  unsaved changes). The next visit opens it again, with its title and unsaved mark, and
  says "Restored ... from the last visit". A snapshot counts as saved only after
  IndexedDB commits it. Failed writes show a message and retry after five seconds;
  the previous recovery copy is retained. A successful Save writes the recovery copy at
  once, so a reload cannot restore a state older than the saved file. If browser storage remains blocked, use
  Save to keep a model file. `?demo` in the address opens the demo
  instead.
- **Recent files.** Files opened or saved are listed under File > Open Recent (up to
  eight). IndexedDB keeps a copy of each one's text, so they reopen without asking the
  user to pick them again.
- **Open** (Ctrl+O). Where the browser has the File System Access API (Chromium), the
  open picker when running as a standalone page; the file's handle is kept, also in IndexedDB. Embedded pages and browsers without this API
  use a file input. A blocked native picker also falls back to the input. The file is read in the background and opened at the next
  frame.
- **Save** (Ctrl+S) writes the picked file in place when there is a handle (asking for
  permission once if the browser wants that); otherwise it shows the save picker where
  there is one, and downloads `<name>.rysmo` where there is not. **Save As** always
  picks a new file (or downloads).
- **Export OBJ** downloads the `.obj` and its `.mtl`; **Export glTF** downloads a `.glb`.

## Size

Historical baseline before the renderer additions. Current sizes are recorded by
`scripts/build-web.sh` in `target/web/build-report.json`. Trunk builds with the `web-release` profile
(`opt-level = "z"`, fat LTO, one codegen unit; see the workspace `Cargo.toml`) and runs
`wasm-opt -Oz`:

| File | Raw | gzip -9 | brotli 11 |
| --- | --- | --- | --- |
| `rysmo-*_bg.wasm` | 25.7 MB | 8.5 MB | 6.1 MB |
| `rysmo-*.js` | 115 kB | 16 kB | |

With the plain release profile the wasm was 49 MB after wasm-opt. Most of the size is
Bevy's renderer and its shaders; the core crates are a small part. Serve it compressed:
browsers download the brotli or gzip size.

Trunk's own progress total is the size before wasm-opt, so `loader.mjs` asks the server
for the real size (a HEAD request for the preloaded wasm) and shows that.

## Historical browser checks

The build was opened in headless Chromium 154 (the flatpak, `--headless=new
--enable-unsafe-swiftshader`), served by a local HTTP server, and driven through the
DevTools protocol:

- with the download slowed to 2 MB/s, the loading screen showed "Downloading 7.0 of
  24.5 MB" with its bar;
- the app started (Bevy reported WebGL2 through ANGLE on Vulkan and turned off the
  features WebGL2 lacks: compute, storage buffers, SSAO), removed the loading screen
  about 2 s after the page opened locally, and drew `?demo` the same as the desktop;
- keys and clicks sent through `Input.dispatchKeyEvent` and `Input.dispatchMouseEvent`
  drew a rectangle with a typed `2000,1000` and pulled it up with a typed `1000`;
- screenshots came back through `Page.captureScreenshot`.

Storage, checked the same way with a fresh browser profile each time
(`rysmo_storage.py` in the session's scratch directory):

- *Without the File System Access API* (the pickers deleted by an init script, as in
  Firefox): a box drawn with typed values was in IndexedDB two seconds later, was back
  after a reload with its unsaved mark ("Restored Untitled.rysmo from the last
  visit"), Ctrl+S downloaded `Untitled.rysmo` with its six faces, and Ctrl+O opened
  `example.rysmo` through the file input (intercepted with
  `Page.setInterceptFileChooserDialog` and filled with `DOM.setFileInputFiles`); the
  recent list then held both files.
- *With the API* (the pickers replaced by fakes that record writes, since headless
  Chromium cannot show the real dialogs): the first Ctrl+S asked the save picker once
  and wrote six faces; after a line split the top, the second Ctrl+S wrote seven faces
  to the same file without asking again; Ctrl+O went through the open picker.

Not checked: Firefox itself, a real (non-headless) browser window and its real file
dialogs, storing a real file handle across a reload (the fakes cannot be stored),
touch.

## Not done

- WebGPU: Bevy would need its `webgpu` feature, which replaces WebGL2 in the same
  build; serving two builds and choosing at load time is possible but not set up.
- Touch is the same code as on the desktop; it has not been tried on a phone or a tablet
  in the browser.
- Drag-and-drop of files onto the page. The editor's internal object clipboard is
  available through Ctrl/Cmd+C, X, V and D. System clipboard interchange with other
  modelling applications is not implemented.

## Materials and rendering controls

Materials can load PNG/JPEG images through a browser file input. This picker does not
change the model file handle or the recent-file list. Image bytes are embedded in the
model and included in autosave. The initial texture width is 1 metre, with height from
the image aspect ratio; Materials lets the user change both dimensions in model units.

Styles and Shadows exposes the render settings. Ambient occlusion and temporal
anti-aliasing are unavailable in the WebGL2 renderer and their controls are disabled
or omitted. Other settings still need visual verification in the browser.

Camera navigation also triggers auto-save at the configured interval, even when geometry
has not changed. This keeps the current view across reloads without adding camera
moves to geometry undo history.

## Unsaved changes

New, Open and Open Recent ask whether to Save, Discard or Cancel when the model is
modified. Save continues the requested action only after saving succeeds and the
model is clean. Cancelling the picker or failing to write leaves the model open.
The native window close button uses the same guard.

The browser registers `beforeunload` and updates its dirty flag every frame,
independently of the delayed IndexedDB autosave. Closing or refreshing a modified
model requests the browser's own confirmation. Browsers control its wording and
may suppress it without prior user interaction or during mobile process termination.


## Renderer quality controls

Studio environment lighting uses small bundled procedural cubemaps unless the
model carries an HDRI panorama. Panoramas are filtered into cube maps on the CPU
(background, GGX-prefiltered reflections and spherical-harmonic diffuse light), so
WebGL2 lights models the same way as the desktop renderer; on the web the panorama
is first reduced to 2048 pixels wide and the cube maps are half the desktop size. Texture display copies use mipmaps and optional anisotropic
filtering, falling back to trilinear where the GPU does not support anisotropy.
Original image bytes remain available for saving/export. Display textures and
scaled 3D targets are capped at 4096 pixels on web.

The 3D resolution slider supports 50–150%. A separate render target is composited
behind the native-resolution egui interface whenever the scale differs from 100%.
Picking and annotation layout keep their original canvas coordinates. At 100%,
the direct window rendering path returns and the offscreen image is reduced to 1×1.
Existing SSAO/TAA platform restrictions remain. Bloom around glowing materials is
desktop-only: in WebGL2 it stopped the view from presenting new frames. Glowing materials
and their lights still render in the browser.

## Auto-save preferences

File > Settings > Application provides **Enable auto-save** and **Save every**, in whole
minutes from 1 to 120. The default is one minute. The interval starts with the first
unsaved recovery change and continues during active modelling. Disabling auto-save,
changing the interval, or opening another document restarts the timer. Idle models
are not written repeatedly. Browser local storage retains the preference across
reloads when permitted; blocked storage leaves it usable for the current session.
These preferences belong to the application, not the model or undo history.

Auto-save writes a recovery snapshot to IndexedDB, including the camera, and does
not mark the model saved or overwrite a user-selected file. Use Save to write or
download `.rysmo`. This setting is currently browser-only; native recovery and
additional interchange exporters are not implemented.

The web build defaults to one Cargo job to limit peak memory during the WASM release link. Set `CARGO_BUILD_JOBS` explicitly to use more jobs on a larger build host. Native builds keep Cargo's normal defaults.


## Recovery regression checks, 2026-10-22

The asynchronous Save result identifies both the document generation and revision.
New/Open can reuse revision numbers, so a late result from another document must
not rename the current model or clear its unsaved flag. JavaScript keeps the file
handle captured when Save began and does not attach a late Save As handle to a
replacement document. Open results also carry the document version from before
opening the picker. If the model changed in the meantime, the result is rejected
instead of discarding those edits. File handles are staged under separate tokens
and accepted only after the editor parses and installs the model. Failed or
rejected Open/restore operations keep the previous handle.

Failed IndexedDB opens can be retried; aborted transactions
report failure even without an error event. Recovery tracks the dirty flag as well
as geometry and camera changes so saving a file updates the recovery metadata.

The JavaScript tests cover storage commits/aborts, retry after a blocked database
open, dirty-state restore, beforeunload, and saves spanning document replacement.
The shared Rust check script also runs the actual autosave module's five tests as
a standalone binary, without linking Bevy's GPU dependencies.

A visual browser check was attempted on this date, but the browser tool could not
verify its administrator-enforced access policy for the local HTTP URL. No browser
interaction, rendering result or performance measurement is claimed for this pass.
The local package was rebuilt later in this session; see the release-candidate checks below.


Verification for these fixes passed with 611 headless library tests, five actual
recovery-module tests and 18 JavaScript tests. Strict Clippy passed for native
workspace targets and the browser app; the `web-release` compilation check passed.
These are automated source/module checks, not a substitute for the blocked browser
interaction test described above.


## Release-candidate checks, 2026-10-22

The full `web-release` build completed with the recovery fixes from `1905d67`,
including linking, wasm-bindgen and `wasm-opt`. The optimized WASM is 28,462,234
bytes, about 27.1 MiB, or 9.8 MiB with gzip. The upload package is
`target/web/rysmo-itch.zip`, with `index.html` at its root, the GPL source archive
and build metadata. Its exact size and revision are in `target/web/build-report.json`.
No upload or publication was performed.

After building, run the optional static package check with Node.js:

```sh
node --experimental-vm-modules scripts/check-web-dist.mjs
```

It verifies every manifest asset's size and SHA-256, loader size metadata, local
JavaScript import fingerprints and named exports. It also compiles the actual
WASM module and checks its direct snippet imports against the packaged JavaScript
exports. It does not evaluate the editor, instantiate WebGL or simulate browser
interaction. Packaging tests use small fixtures to reject missing JavaScript
exports, missing direct WASM imports and corrupt files.

The candidate passed this check with two linked JavaScript modules and nine direct
WASM-to-JavaScript imports. ZIP CRC and archive/manifest hash checks also passed.
A browser access-policy failure still blocks visual, file-picker, iframe and
renderer FPS verification. A successful static check does not remove that gap.

## Repeatable native core baseline

Generate the benchmark models and run the headless example:

```sh
python3 scripts/create-web-benchmark.py
cargo run -p rysmo_io --example benchmark_core --offline --locked -- \
  target/web-test/grid-flat-100.rysmo target/web-test/grid-unique-100.rysmo
```

Both models have 1,200 cube instances and 7,200 faces across their occurrences.
The first shares one definition; the second stores 1,200 independent definitions.
Generated coordinates use the native format's nine-decimal precision, avoiding
normalization differences such as `3.9000000000000004` becoming `3.9` on save.
The benchmark requires an identical load/save round trip, then verifies the
original and moved documents after every Undo and Redo.

The recorded run used Rust 1.98.1, the normal dev profile with workspace code at
optimization level 1 and debug assertions enabled, on the host's reported
AMD Ryzen 5 40. Each result has 21 samples after three load/save warm-up cycles.
These are native CPU timings on a shared host, not browser timings. Load parses
already-read text and Save serializes to memory; disk and IndexedDB are excluded.
Clone includes destruction; edit timings exclude editor setup and equality checks.

| Median, milliseconds | Shared definition | Independent definitions |
| --- | ---: | ---: |
| Load and validate | 2.17 | 63.59 |
| Serialize | 1.38 | 19.42 |
| Clone and destroy | 0.34 | 12.62 |
| Move one instance | 0.35 | 8.29 |
| Undo | 0.015 | 0.010 |
| Redo | 0.003 | 0.009 |

Peak RSS for this benchmark process was 43,400 KiB, about 42.4 MiB; it is not the
editor's memory footprint. Full results including the 95th percentile are in
[core-performance-2026-10-22.json](verification/core-performance-2026-10-22.json).
The comparison identifies geometry storage and snapshot copying as useful next
profiling targets. It does not measure a speedup from a production code change.
