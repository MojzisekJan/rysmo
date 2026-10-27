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

## Release package

The offline optimized web build completed successfully from code commit
`ea26b0c`. Cargo release compilation took 7 min 11 s; the subsequent optimizer and
packager completed successfully. The final archive is
`target/web/rysmo-web.zip` (approximately 13.6 MB, including corresponding source).

- Optimized WASM: 28,889,773 bytes.
- Gzip WASM: 10,366,262 bytes; +1.34% against the previous 10,228,712-byte build.
- Static package validator: all 18 asset hashes verified, two JavaScript modules
  linked, nine direct WASM→JavaScript imports resolved, WASM module compiled.
- ZIP CRCs, root `index.html` and clean source revision verified.
- The final documentation-only commit is included by repackaging the identical
  compiled application; no second release link is needed.

A fresh browser access attempt against the local server was also blocked by the
unavailable admin policy check. The temporary tab and server were closed. Static
validation does not exercise editor startup or WebGL. Nothing has been published
externally.

## Solid hierarchy preflight follow-up

Solid Tools now validates both operand hierarchies and geometry budgets before
computing their bounds. Bounds use only the validated leaf vertices; they no
longer expand the definition hierarchy separately. Triangle and vertex counting
stops at the first exceeded limit and uses checked or saturating arithmetic.

Three regression tests reject cyclic definitions, a compact shared hierarchy
with over 16 million expanded leaves, and repeated geometry above the triangle
budget. Each verifies that rejection leaves the document unchanged.

Validation: all 284 model library tests pass. Strict native workspace all-target
Clippy and strict web-release WASM Clippy pass without warnings. Formatting and
diff whitespace checks pass. The release ZIP described above predates this
follow-up and does not contain this fix.
