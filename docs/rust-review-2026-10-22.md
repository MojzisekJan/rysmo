# Rust review, 2026-10-22

Reviewed the Rysmo source at GitHub commit `8d7230e`, using the
[Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html) as a
reference. This is a first source-quality pass, not proof that every code path is
correct or a dependency/security audit.

## Fixed

- Preferences previously copied attempted JSON into the saved-state cache even if
  storage rejected the write. An unchanged setting would then never retry. The
  store now records only successful writes, retries after five seconds, accepts
  changed settings immediately and clears stale errors when reverting to saved
  preferences. Closing the native app bypasses the retry delay for a final attempt.
  Three tests inject storage failures without depending on disk/browser state.
- The preferences serializer no longer substitutes empty text on failure.
- The unsaved-document dialog now uses named choices and pattern matching for its
  pending action, instead of integer choices and check-then-unwrap.
- Windows icon build failures now explain the missing Cargo input or failed file
  operation. This path still needs execution on a Windows build host.
- Completed OBJ/COLLADA exports and print-check results are marked `must_use`.
- The workspace rejects `dbg!` and warns on unfinished `todo!`/`unimplemented!` code.
- The declared Rust minimum was 1.90 and was not inherited by the member packages.
  All ten members now inherit 1.97.1, matching the highest minimum declared by the
  locked dependencies, including Bevy 0.20 and its shader tooling. Verification on
  this machine uses Rust 1.98.1; the exact minimum compiler is not installed here.

## Follow-up work

Large modules make changes harder to review. `rysmo_model/src/command.rs` is over
3,200 lines and `rysmo_tools/src/tools/mod.rs` is over 2,100. Extract cohesive command
families/tool state operations in small changes with existing Undo, cancellation
and stale-preview tests intact. Avoid a single repository-wide reorganisation.

Production code still has bare `unwrap()` calls. Review user-input, file and stale
selection paths first. Many calls enforce already-validated geometry invariants;
others belong to developer verification tools or tests. Do not replace all of them
with silent fallbacks. Use pattern matching or return an error where absence is
recoverable, and contextual `expect` for actual internal invariants.

The first pass identified `Document::resolve` returning `Result<_, ()>` and losing
context about failures. This is addressed in the follow-up below.

The model uses full document snapshots for Undo. That copy has a purpose; changing
it requires measurements and transaction tests. Shared image bytes intentionally
use `Arc`. Neither should be removed merely to satisfy a style preference.

Bevy preludes remain allowed in ECS/viewport modules. Standard Cargo layout,
`#[test]`, platform threads and async browser I/O remain appropriate. Google rules
specific to Chromium or Fuchsia are not adopted. Current application APIs are not
turned into generic public libraries or annotated wholesale as non-exhaustive.

The adopted rules and verification commands are in
[CONTRIBUTING.md](../CONTRIBUTING.md).

## Verification

- `cargo fmt --all --check` passed.
- `cargo clippy --workspace --all-targets --offline --locked` completed with no
  compiler errors and 71 distinct warnings, deduplicated across library/test
  targets by diagnostic, source file and line. These warnings remain visible;
  they have not been suppressed. The largest category is collapsible `if` blocks.
- The eight headless library suites passed all 604 tests: math, mesh, model,
  inference, tools, I/O, MCP and the material library.
- All three new preference-store tests passed in a standalone Rust harness built
  from the actual store implementation and its test functions. The full app test
  suite was type-checked by Clippy but its GPU-linked binary was not executed.
- All 10 JavaScript file/storage/loader tests and four Python packaging/server
  tests passed.
- `cargo check -p rysmo_app --target wasm32-unknown-unknown --profile web-release
  --offline --locked` passed. This checks compilation, without linking or publishing
  a new browser release.

No native GUI session, Windows resource build or mobile runtime was exercised in
this pass. The pre-existing web release package has not been rebuilt or published.

## Follow-up: context errors and module responsibilities

`Document::resolve` now returns a named `ContextError` with the failing instance,
path index and owning/referenced definition. It checks the target definition at
*every* step, including the final one. Previously a dangling final reference could
incorrectly return success. Intersection errors preserve those details for the
caller. Tests cover root/nested resolution, missing instances, dangling definitions
and the transactional behaviour of a rejected intersection command.

The `Editor` implementation moved from `lib.rs` into `editor.rs`, keeping
`rysmo_model::Editor` and `UNDO_LIMIT` available under their original public paths.
The root module now declares modules and re-exports APIs. Tests explicitly import
the matrix type they use rather than inheriting a private root-module import.
This is a structural change; Undo snapshots and document serialization are retained.

Terrain edge flipping now uses a named `FlipPlan` with two face IDs and two triangle
arrays. The type records the validated lengths, replacing a long tuple of vectors.
Recoverable failed lookups return `MeshError` before the candidate mesh is committed.
Existing tests cover convexity rejection, attributes, curve membership and atomic
failure. Three unnecessary `clone()` calls on copyable curve values were removed.

Native input insertion now resolves owners through a fallible helper instead of
checked `unwrap()`/`expect` calls for instances, guides, text, dimensions and sections.
A regression test checks all five record collections against missing owner IDs.
The native file layout and version are unchanged.

Follow-up verification passed on Rust 1.98.1:

- All eight headless library suites: **610 passed**, no failures, including six new
  tests for context errors, rejected command state and native input owners.
- Workspace Clippy, including all native targets: no compiler errors, **66 distinct
  warnings**, down from 71. Three `clone_on_copy`, one `result_unit_err` and one
  `type_complexity` finding were removed; no new warning category/count was added.
- Browser compilation with the `web-release` profile passed.
- Formatting and diff checks passed. No browser release was rebuilt or published.

The `Document::resolve` error type changes from `()` to `ContextError`. External
Rust callers matching `Err(())` must handle the named error; successful values and
method paths are unchanged. Workspace callers are checked by Clippy and the tests.
Other large modules and the remaining warnings are still follow-up work.
