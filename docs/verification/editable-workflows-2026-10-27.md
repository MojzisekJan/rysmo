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

## Browser GUI verification

The optimized build was exercised in the in-app browser at a dedicated localhost
origin, using a local-only fixture page to prepare two recovery documents. No
fixture route or seeded storage is included in the distribution.

Verified actions:

- Restore an assembly and a cube with separate cameras and modified flags, switch
  between them and retain each model's visible geometry.
- Erase the cube, switch documents and confirm that Undo in the assembly does not
  undo the cube's edit; return to the cube and undo its own edit.
- Copy the cube through Edit > Copy, switch to the assembly, Paste in Place and
  remove that paste with one Undo. The automation clipboard intercepted keyboard
  paste, so this check used the editor's own menu commands.
- Change a live array from three to four new occurrences, regenerate, open source
  inputs and cancel source editing back to the generated result.
- Change the Cost attribute from 125 to 150, Apply data, Undo and see the form reload
  the stored value 125. Named connection axes and labels were visible in the model.
- Open the section-hatching controls and confirm loaded model spacing and scope.
- Open the stored drawing layouts, inspect the page preview and trigger all-sheet
  PDF export. The application reported the download; payload validation used the
  separately generated native example and Poppler inspection described above.
- Attempt to close a dirty document, see Save/Discard/Cancel and cancel without
  losing it. Save both models through the embedded download path, reload the page
  and restore both tabs, the active document and the changed five-window assembly.
- Move the parametric form beside the model, drag a width grip from 1.2 m to about
  2.185 m and undo that regeneration once. Load the selected component's dimensions
  to confirm 1.2 m. The repaired form has a visible header and scrollable content;
  grips are suppressed over forms rather than intercepting their fields.

Screenshots:

- [Recovered document workspace](document-workspace-recovery-2026-10-27.jpg)
- [Live operation controls](live-operation-edit-2026-10-27.jpg)
- [Drawing-sheet preview and export](drawing-sheets-panel-2026-10-27.jpg)
- [Parametric dimension grips](parametric-dimension-grips-2026-10-27.jpg)
- [Handles stay below an overlapping sheet editor](model-handle-occlusion-2026-10-27.jpg)

The browser log captured one unattributed MutationObserver TypeError during a
reload. Shipped JavaScript contains no MutationObserver constructor; a clean
browser-console result is therefore not claimed. The model actions and exports
continued to work. Real filesystem-picker permissions were tested through the
JavaScript regressions, not through a native browser permission dialog.

The final `21e5c1d` build also repeated the grip drag/Undo check and opened Drawing
Sheets above the selected parametric component. Handles disappeared under that
window, reappeared after it closed and remained usable. Final native and WASM
Clippy checks pass without warnings.

The final WASM was linked from `21e5c1d`; subsequent changes only complete the
English guides and add verification screenshots. Packaging is refreshed after
those documentation changes without relinking identical Rust source.
