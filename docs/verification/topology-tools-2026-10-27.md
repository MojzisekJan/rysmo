# Curve grouping and face orientation verification, 2026-10-27

Weld Edges, Explode Curve and Orient Faces are exposed through Model → Faces &
Edges, the context menu, command search and the English guide. Properties labels
non-analytic curves as polylines rather than implying they were deformed.

Validation:

- Mesh library: 79 tests pass. New coverage includes open/closed curve grouping,
  retained edge styles and geometry, complete-curve replacement, rejection of
  branches/disconnected selections/partial curves, hidden-face barriers, physical
  material-side preservation, non-manifold rejection and a non-orientable strip.
- Model library: 293 tests pass. New coverage includes nested editing, copied
  curve independence, one-step Undo/Redo, unchanged history/revision after invalid
  inputs or no-ops, analytic circle restoration and closed-solid volume/UVs.
- I/O library: 84 tests pass, including saved polyline/exploded metadata and
  consistently oriented solid geometry after native round-trip.
- Tools library: 150 tests pass, including whole-curve click selection before
  explosion and individual-edge selection afterwards.
- Strict native workspace all-target and strict WASM `web-release` Clippy pass
  without warnings. Menu/action tests are type-checked; the GPU-linked native
  application test binary was not executed.
- Formatting and diff whitespace checks pass.

The four executed suites contain 606 passing tests. No interactive GUI verification,
release linking or external publication was performed. Existing web packages
predate these operations. No persistence version change or dependency was needed.
