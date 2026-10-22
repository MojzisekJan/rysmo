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

Before submitting changes, run:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked
cargo test --workspace --locked
cargo check -p rysmo_app --target wasm32-unknown-unknown --locked
```

Inspect Clippy findings, including existing warnings. Warnings are not all errors
or confirmed bugs; the current workspace does not yet pass `-D warnings`.
`dbg!` is denied; `todo!` and `unimplemented!` produce review warnings. Preserve
English GUI text and the Rysmo name. Keep private credentials and generated output
out of Git. Do not change persistence formats or file extensions without an
explicit compatibility plan.
