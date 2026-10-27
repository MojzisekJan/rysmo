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

## Replay fixture

`testdata/path-points-report.rysmo` contains one scaled Block component and a
connected two-edge polyline. Its definition has a 0.2 m cube scaled three times:
occurrence edge length 7.2 m, face area 2.16 m² and volume 0.216 m³. The open root
path adds 4 m of linework. Therefore the overall report has length 11.2 m and
area 2.16 m²; its total volume is unavailable because root linework has no closed
surface. The component volume remains available separately.

To replay the workflows in the web editor:

1. Open the fixture and select all. Search for Array Along Path with Shift+S.
   Review the preview, create copies, then Undo and Redo.
2. Undo the array and select only the polyline. Search for Edit Curve Points.
   Change one local coordinate, review the green preview and Apply point. Undo
   restores the original point.
3. Export Model report (CSV) from File > Export. Check the quantities above,
   definition summaries and the reason for the blank total volume.

The fixture was also loaded through the built native codec and its quantities
were checked against these values before interactive verification.

## Browser verification and web package

The current optimized web build started successfully in the in-app browser, with
actual 3D rendering. Interactive verification then used a temporary same-origin
iframe wrapper to exercise the embedded editor's file-input path. The
standalone page's native File System Access chooser could not be driven through
the browser filechooser API. The embedded chooser eventually loaded the fixture,
but its automation transfer was unexpectedly slow.

Executed in the embedded editor:

- Select all and launch Array Along Path through command search. Set four new
  occurrences, inspect the four green bounding boxes, create the array, then
  verify one Undo removes all copies and Redo restores them.
- Select the polyline, open Edit Curve Points and set its first point's Z to 1 m.
  Inspect the read-only preview, shrink the dialog and confirm its text/fields
  resize without overlap. Apply, close and Undo restore the original point.
  A second pending edit was discarded with Escape.
- Open Model Report through command search and trigger Export CSV. The application
  reported `Downloaded model-report.csv`, with no browser warning/error logs.
  The automation download event timed out, so downloaded bytes were not inspected.
  CSV contents and expected fixture quantities are covered by the native codec
  smoke check and executed I/O tests rather than a claimed browser-download check.

Visual evidence is saved as `path-array-preview-2026-10-27.jpg`,
`curve-point-preview-2026-10-27.jpg` and `model-report-dialog-2026-10-27.jpg` in this
directory. No FPS, browser memory or mobile-device benchmark is claimed.

`RYSMO_WEB_OFFLINE=true CARGO_BUILD_JOBS=1 bash scripts/build-web.sh` completed a
fresh optimized build after the dependency update. Cargo's cold release phase took
18m 21s, followed by wasm-bindgen/wasm-opt and packaging. The final compact exact
coordinate display was included before the application crate compiled; native
and WASM Clippy were rerun successfully afterward. No release code changed after
this build.

The ZIP is `target/web/rysmo-web.zip`. It includes corresponding source and
licences; the larger source archive is a separate downloadable asset and is not
loaded to start the editor. `scripts/check-web-dist.mjs` validates generated
modules, imports, assets, source metadata and archive consistency. The temporary
iframe wrapper is removed before final packaging. See `target/web/build-report.json`
for final package size and revision. Upload and publication are outside this stage.

Optimized WASM: 28439344 bytes. SHA-256: `aa7679bee6f5c32d98627b50ea717f4d13e664a1400fb0fc78050763ce0a87ef`.
