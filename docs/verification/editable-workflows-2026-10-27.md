# Editable workflows, 2026-10-27

The stage adds stored live-operation inputs, named component connection frames,
typed definition/occurrence attributes, architectural fitting and material retention,
section hatching, document tabs and regenerated multi-viewport drawing sheets.
Controls, persistence boundaries and work limits are in `docs/editable-workflows.md`.

## Executed checks

`scripts/check-rust.sh --offline` passes 707 headless library tests: infer 27,
I/O 96, library 2, math 22, MCP 5, mesh 79, model 326 and tools 150. Five standalone
recovery tests also pass. The combined Rust count is 712. Strict native workspace
all-target Clippy, formatting and web-release WASM checks pass. Strict WASM Clippy
also passes without warnings. The final I/O suite was rerun after paper-text fit
checks; all 96 tests pass.

All 21 JavaScript file/recovery tests and four web-packaging tests pass. New browser
storage coverage includes separate file handles per document, Save As completing
after a tab switch, workspace recovery of all documents and active-tab identity,
and serialized workspace writes that preserve the latest snapshot.

New model/I/O coverage exercises world-rigid connection matching under mirrored
and nonuniform contexts, invalid metadata, architectural Make Unique/fit/Undo,
semantic part paint through pane-count changes, live array parameter changes,
source editing/Finish/Cancel, sweep and Boolean recipe evaluation, failed-edit
atomicity, detach/Undo, native persistence followed by regeneration, hatch holes,
cross patterns, density budgets, inherited object hatches, custom CSV values,
negative numeric cells, mixed-unit totals, paper validation and multi-page PDF.

The GUI regression tests compile under all-target Clippy; this does not claim
that the Bevy GUI test binary was linked and run.

## Vector output inspection

`cargo run -p rysmo_io --example editable_workflow_demo --offline --locked -- target/editable-workflow-demo`
produces a reusable native model, SVG and a two-page PDF. The PDF was rendered with
Poppler and both pages inspected. `pdfinfo` confirms two A4 landscape pages;
`pdftotext -layout` retains Czech text, accents, viewport scales and title blocks.
Both pages have aligned frames and readable notes without clipping or overlap.
The first contains top/front viewports; the second contains an isometric viewport.
Exports use the live assembly, active cut and shared hatch generation.
