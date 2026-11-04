# Contributing Rust code

Rysmo requires Rust 1.97.1 or newer, matching Bevy 0.20. It uses Cargo's conventional module layout, and the checked-in
`rustfmt.toml`. Keep `Cargo.lock` in changes that update dependencies.

Use the [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html)
as a review reference, with the following project choices:

- Prefer direct, readable code and borrows when ownership does not require a copy.
- Return errors for failed user operations. Match optional values instead of
  checking and unwrapping them. Use a contextual `expect` only for a documented
  invariant, including required build-script inputs. Tests may use `unwrap`.
- Keep persistence errors visible. A failed write must not advance the saved state;
  retry transient failures without writing on every frame. Cover these paths with
  tests that can inject a storage failure.
- Use named modes for distinct actions. Mark export result types `must_use` so
  silently discarding a completed export produces a compiler warning.
- Keep core geometry/model/I/O code independent of Bevy. Bevy's prelude is allowed
  in viewport and ECS modules; avoid wildcard imports elsewhere.
- Avoid unrelated formatting or API rewrites while fixing behaviour. Split large
  modules by responsibility when working on them, preserving Undo transactions.

Chromium/Fuchsia-specific build, thread and test rules do not apply to this Cargo
application. The mobile adapter allows only the exported entry-point symbol
attribute; core crates forbid unsafe code. Public APIs shared only by the workspace do not need speculative
abstractions or blanket `non_exhaustive` annotations. Shared immutable image bytes
use `Arc` intentionally; the undo history currently uses document snapshots.

Before submitting changes, install the `rustfmt` and `clippy` components and the
`wasm32-unknown-unknown` target, then run the shared checks:

```sh
scripts/check-rust.sh
# With dependencies already cached:
scripts/check-rust.sh --offline
```

The script checks formatting, runs workspace Clippy with `-D warnings`, executes
all eight core library suites, MCP stdio/TCP integration tests and application
unit tests, and checks the
browser app with `web-release`. The application suite uses the `portable` profile
to limit debug symbols and disk use. It links renderer dependencies but does not
open a window or create a GPU device; no desktop session is required. Browser
recovery scheduling is included in that suite. Clippy also checks examples.
Local browser tests run the actual WASM editor in Chromium with software WebGL2.
Native interactive GUI verification remains a separate check on a suitable host.

Checks are run locally. Do not add CI workflows or hosted test automation.
Local verification uses
Rust 1.98.1 on Linux, plus JavaScript file/storage tests and Python web packaging
and server tests. The declared minimum remains 1.97.1; the local results do not
claim to test that exact minimum.

Keep Clippy clean instead of suppressing warnings globally. `dbg!`,
`clone_on_copy`, `result_unit_err` and `type_complexity` are denied in workspace
lint settings; the local check script also rejects other warnings, including `todo!` and
`unimplemented!`. Native file loading and annotation command handlers additionally
deny `unwrap`/`expect`; use fallible lookups for those input paths. Preserve
English GUI text and the Rysmo name. Keep private credentials and generated output
out of Git. Do not change persistence formats or file extensions without an
explicit compatibility plan.

Command data, labels, drawing helpers, transforms, groups, annotations and settings
live in separate modules under `rysmo_model/src/command/`. Keep serialized command
fields and public re-exports stable when moving code. Shared command finalization
belongs in `execute.rs`; preserve its glue refresh, opening validation and cleanup
order, along with the `Editor::apply` transaction boundary.

Use named records for related fields and operation arguments when positional
values are easy to confuse. `Template` stores named face, edge and curve records;
annotation reparenting and attached label placement use named request contexts.
Keep simple geometric pairs such as line endpoints as pairs.

## Local coverage and browser tests

Install `cargo-llvm-cov` 0.9.1 and the `llvm-tools-preview` Rust component, then run:

```sh
cargo install cargo-llvm-cov --version 0.9.1 --locked
rustup component add llvm-tools-preview
scripts/coverage.sh --offline
python3 scripts/coverage-summary.py target/coverage/all/summary.json
```

`--core` measures the eight headless libraries and MCP integration tests only. The
full default also runs the application suite. Reports are written under
`target/coverage/all/` or `target/coverage/core/` as HTML, JSON and LCOV. Instrumented
builds use their own `coverage` profile and target directory, keeping ordinary
build artifacts reusable. Each run clears old execution counters before collecting
results. Do not run two coverage commands concurrently. Stable-toolchain coverage
reports lines, regions and functions; it does not measure branches. Standalone test
files, examples and vendored sources are excluded; inline test modules remain in
the measured source. Native coverage does not cover browser-only Rust or GPU code.
The summary command prints per-crate coverage and writes a local Markdown summary.
Reports stay under `target/`; no hosted service or automatic run is configured.
There is no arbitrary pass percentage until a baseline has been reviewed.

For real browser tests, use Node 24 and pnpm 11.25.0:

```sh
pnpm --dir tests/web install --frozen-lockfile --ignore-scripts
pnpm --dir tests/web exec playwright install --with-deps chromium
scripts/build-web.sh
pnpm --dir tests/web test
```

The tests serve the built editor on a temporary loopback port and operate its real
canvas through Playwright. They check WebGL2 startup, native model file input,
rectangle dimensions, Undo/Redo, download/reopen and dirty IndexedDB recovery.
Screenshots and downloaded models are saved to `target/web-tests/` on the local
machine. Each scenario has a fresh browser process and context. The opt-in `?diagnostics`
panel provides read-only model/tool/history counts; tests do not inject commands
into the editor. An existing build can be selected with `RYSMO_WEB_DIST`. Local
runtimes may set `RYSMO_PLAYWRIGHT_MODULE` to a Playwright ESM module path and
`RYSMO_CHROMIUM_EXECUTABLE` to an installed compatible Chromium executable.
