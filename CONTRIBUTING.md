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
all eight headless library suites, and checks the browser app with `web-release`.
Clippy also type-checks native application tests and examples. Running the full
GPU-linked application tests or checking GUI behaviour requires a suitable host;
these are additional checks for changes to the application.

GitHub Actions runs the same script with Rust 1.98.1 on Linux, plus the JavaScript
file/storage tests and Python web packaging/server tests. The declared minimum
remains 1.97.1; this workflow does not claim to test that exact minimum.

Keep Clippy clean instead of suppressing warnings globally. `dbg!`,
`clone_on_copy`, `result_unit_err` and `type_complexity` are denied in workspace
lint settings; CI also rejects other warnings, including `todo!` and
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
