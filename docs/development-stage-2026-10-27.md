# Modelling workflow stage, 2026-10-27

Continue this stage autonomously without requiring a new instruction between
features. Complete coherent changes and local commits; report actual blockers and
verification gaps. Batch changes before expensive release builds. Keep GUI English
and Git author/committer Jan Mojzisek <mojzisekjan@gmail.com>.

## Scope and completion checks

- [x] Component arrays along a connected edge path: open/closed paths, count or
  spacing, optional tangent alignment, reverse direction and a bounded preview.
  Preserve shared definitions and instance appearance. Cancel is read-only and
  confirmation is one Undo step. Reject branched/disconnected/degenerate paths.
- [x] Polyline point editing: select an existing grouped polyline, inspect ordered
  points and enter exact coordinates with a model-space preview. Keep groupings
  and material/UV behaviour, reject face-bending or invalid edits atomically,
  protect evaluated opening geometry and support one-step Undo/Redo. Analytic
  circle/arc editing continues through existing radius/segment controls.
- [x] CSV model report: component occurrence counts and geometry quantities with
  stated units, hierarchical transforms, visible-only option and explicit volume
  availability. Bounded traversal, stable ordering, CSV quoting and safe text
  cells. Browser and native exports use the same report implementation.
- [ ] Validate new and preceding workflows: headless regressions, strict native
  and WASM checks, persistence and cancellation. Attempt interactive browser
  verification when the available browser tools permit access; record blockers
  without bypassing them or claiming screenshots/FPS that were not measured.
- [ ] Build and validate one current optimized web package after related fixes.
  Check source revision, loader/assets, ZIP and corresponding source contents.
  Keep build outputs ignored and report the actual package location and size.
- [ ] Update the English guide, workflow documentation and a verification record;
  finish with clean, reviewable local commits and a clear remaining-work list.

External upload/publication is not part of this stage. Scene animation and
spatial decorative freehand remain excluded. Do not introduce speculative formats
or vendor integrations merely to expand scope.
