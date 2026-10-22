# Editor expansion verification — 2026-10-22

All seven workflows in `docs/feature-plan.md` are implemented. Their supported
formats and restrictions are listed in `docs/editor-expansion.md`.

## Checks

- Native workspace and all targets: strict Clippy with zero warnings.
- Web target: `rysmo_app`, `wasm32-unknown-unknown`, `web-release` profile, strict
  Clippy with zero warnings.
- Eight headless library suites pass. A full run passed 629 tests; affected model
  and I/O packages were rerun after subsequent fixes. Latest model suite: 281 tests.
  Latest I/O suite: 78 tests. The combined current count is 634 library tests;
  with standalone recovery checks, 639 Rust tests pass.
- Five standalone recovery scheduling tests pass.
- Nineteen JavaScript tests pass, including binary picker bytes, cancellation,
  size limits and failed reads without replacing the model's file handle.
- Four web packaging tests pass, including missing JavaScript exports, direct
  WASM imports, asset corruption, source archive contents and cache invalidation.
- Local HTTP server cache invalidation test passes.
- Formatting and whitespace checks pass.

The regression coverage includes concave OBJ polygons, negative indices, unit/axis
conversion, DXF layers/bulges/block transforms, GLB roundtrips, interleaved and
sparse accessors, cyclic/expanding hierarchies, shared normal image export,
collection placement with one Undo, parameter regeneration across shared copies,
photo-camera projection/scale/roll recovery, image replacement and Undo, nested
solid subtraction, invalid-operand atomicity and a 1200-triangle solid operand.

## Drawing review

`cargo run -p rysmo_io --example drawing_demo -- /tmp/rysmo-drawing-demo`
generates a reproducible SVG/PDF fixture. Poppler reports one A4 landscape page
(841.89 × 595.276 points). The rendered page was inspected for linework, dimension
arrows, label gaps, margins and footer placement. Extracted text retains `m²`,
`Café / přízemí`, both dimension labels and the 1:50 scale. A unit test verifies
that one model metre becomes exactly 20 mm at 1:50. The PDF embeds its Unicode font.

## Remaining verification boundary

Interactive browser automation is unavailable because the browser access-policy
check is blocked. No browser screenshot, frame-rate measurement or interactive
button-by-button verification is claimed. Native GUI tests are type-checked by
Clippy; the full GPU-linked application test binary is not executed here.

Release linking and package validation are the final step; their result will be
added after the build. Nothing has been published externally.
