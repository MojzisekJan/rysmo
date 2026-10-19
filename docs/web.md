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
initializer). When the download is done it says "Starting..."; the app removes the
loading screen itself after its first frames are drawn.

## Files and storage

The browser half is `crates/rysmo_app/src/web_files.js`, modelled on Heron Brush's;
the Rust half is `web.rs`.

- **Autosave.** At the interval selected in File > Settings > Application, pending changes
  to the open document go into IndexedDB
  (database `rysmo`, store `state`, key `current`, with its name and whether it has
  unsaved changes). The next visit opens it again, with its title and unsaved mark, and
  says "Restored ... from the last visit". `?demo` in the address opens the demo
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

Studio environment lighting uses small bundled procedural cubemaps rather than
external HDR assets. Texture display copies use mipmaps and optional anisotropic
filtering, falling back to trilinear where the GPU does not support anisotropy.
Original image bytes remain available for saving/export. Display textures and
scaled 3D targets are capped at 4096 pixels on web.

The 3D resolution slider supports 50–150%. A separate render target is composited
behind the native-resolution egui interface whenever the scale differs from 100%.
Picking and annotation layout keep their original canvas coordinates. At 100%,
the direct window rendering path returns and the offscreen image is reduced to 1×1.
Existing SSAO/TAA platform restrictions remain.

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
