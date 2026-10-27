# Path arrays, exact polyline points and CSV quantities, 2026-10-27

The stage adds three complete GUI workflows backed by shared headless model/I/O
code. The accepted scope is in `docs/development-stage-2026-10-27.md`; detailed
usage is in `docs/editor-expansion.md` and the English in-application guide.

## Implementation

Array Along Path retains the original component and creates new occurrences from
one connected open chain or closed loop. Count, spacing, start offset, reversal
and world tangent alignment are previewed without editing the document. Copies
share the definition and instance appearance; existing glue is cleared. Rigid
world alignment preserves the source's world shape under mirrored or nonuniform
editing contexts. Count arrays include open endpoints and omit a repeated closed
seam. Spacing arrays leave a possible shorter final interval. Creation is one
Undo step. Bounds and expansion limits prevent excessive previews/arrays.

Edit Curve Points uses ordered existing vertices and exact local coordinates.
Preview and Apply share one conservative deformation operation that keeps IDs,
curve grouping and attached data. It refuses merges, collapsed/bent faces,
protected boundaries, other attached curves and new unjoined crossings/overlaps.
Each Apply is one Undo step and keeps the window open. Both modelling previews
are tied to document revision, generation and editing context. Escape, a tool
change or an invalidated draft discards pending changes.

Model Report exports deterministic, quoted, spreadsheet-safe CSV with occurrence
paths, counts and world quantities. Its total includes each occurrence's own
geometry once, and excludes optional summary rows. Effective meshes and local
volumes are cached per definition; world scale and nonuniform transformations
still apply per occurrence. Face areas use transformed edge vectors so translation
does not subtract large world coordinates. Hidden/tag visibility filtering is
optional. Missing volumes have a stated reason. The exporter does not change the
model's Save destination or Undo history.

## Behaviour boundaries

Polyline editing does not convert analytic arc/circle descriptors automatically;
existing radius/segment controls remain available. Coordinates refer to the
current editing context, while array spacing refers to world distance.

CSV quantities are not a Boolean union. Section planes do not trim quantities,
and soft edges are included. Single-shell volume uses the existing closed,
consistently oriented topology check used by Properties. Solid Check provides
geometric intersection diagnostics. Compound-shell volume additionally requires
validated nesting. Excluding hidden geometry makes volume unavailable. Counts
and geometry work are bounded, and cyclic/deep hierarchies reject the report.

No dependency or native file-version change is required.

## Executed checks

`scripts/check-rust.sh --offline` passes with 691 headless library tests: infer 27,
I/O 91, library 2, math 22, MCP 5, mesh 79, model 315 and tools 150. Five standalone
recovery tests also pass, for 696 executed Rust tests. All 19 JavaScript
file/recovery tests pass. Strict native workspace all-target Clippy and strict
web-release WASM Clippy pass without warnings. Formatting and diff whitespace
checks pass.

New headless coverage includes open endpoints, measured spacing, closed seam
handling, reversal, nonuniform/mirrored world contexts, shared definitions,
one-step Undo/Redo, invalid count/spacing, branch/disconnected/cyclic path rejection,
point-edit read-only preview, stable IDs, failed edit atomicity, face bending,
new edge crossings, transformed cube quantities, summary/total accounting,
hidden-geometry volume availability, Unicode/CSV escaping, formula protection,
cyclic report rejection and native persistence of exact point edits.

Application tests for dialog cancellation and stale project generations are
type-checked. The GPU-linked application test binary was not executed.
