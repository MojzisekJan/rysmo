# Equal edge divisions verification, 2026-10-27

Divide Edges is available in Model → Faces & Edges, the context menu and command
search. The English guide and current-operation help describe the preview,
segment count, cancellation, limits and curve exclusions.

Validation:

- Mesh library: 73 tests pass, including duplicate selection, face/edge appearance
  retention, bounded invalid batches and tolerance rejection.
- Model library: 289 tests pass, including closed-solid volume, nested-context
  Undo/Redo, and unchanged document/revision/history after rejecting a curve.
- I/O library: 82 tests pass, including native round-trip of subdivided boundaries.
- Strict native workspace all-target Clippy passes without warnings. Application
  tests cover cancellation, stale drafts, selection and tool changes; these tests
  were type-checked, not executed in the GPU-linked application test binary.
- Strict `rysmo_app` WASM `web-release` Clippy passes without warnings.
- Formatting and diff whitespace checks pass.

No interactive GUI verification or release linking was performed for this change.
Existing web artifacts predate the feature. No upload or publication was performed.
