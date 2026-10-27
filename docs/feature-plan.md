# Editor expansion

This is the agreed implementation scope. All seven items are required. GUI text
stays English, operations retain Undo/Redo and cancellation, and old documents
remain readable. Browser and native paths must share model/I/O implementations.
Scene animation and spatial decorative freehand remain out of scope.

- [x] Model/underlay import: OBJ, embedded GLB and ASCII 2D DXF, units and axis
  controls, explicit diagnostics, transactional placement and bounded input work.
- [x] External component collections: reusable named assets, thumbnails, search,
  import/export of a portable collection and placement across projects.
- [x] Parametric windows, doors and stairs: dimension controls, editable parameters,
  regeneration of shared definitions and Undo, native persistence.
- [x] Drawing output: orthographic SVG/PDF at a stated scale, page controls,
  dimensions, section cuts and vector linework.
- [x] Photo matching: background photograph, two orthogonal sets of perspective
  guides, origin/scale controls, calibrated camera and saved project settings.
- [x] Material properties: roughness, metallic and normal maps in editing,
  rendering, persistence, clipboard and supported exports.
- [x] Solid operations: nested operand geometry, increased bounded complexity,
  diagnostic errors, validation and atomic edits preserving shared definitions.

Work is batched before expensive builds. Headless tests and native/web checks
precede release linking. A blocked browser access policy must not be treated as
successful visual verification. No external publication is implied.

The seven workflows are implemented. Tests and actual format limits are recorded
in `docs/verification/editor-expansion-2026-10-22.md` and
`docs/editor-expansion.md`. The optimized web package and its static module/asset checks pass; the browser
verification boundary is documented in the record.

## Additional topology tools

- [x] Equal straight-edge divisions with endpoint preview and transactional Undo.
- [x] Weld one connected edge chain/loop into a selectable polyline without moving
  geometry; protect incomplete existing curves and reject branches or gaps.
- [x] Explode complete curve groupings while retaining geometry and restoring
  analytic descriptors through Undo.
- [x] Orient visible connected faces from one reference, preserving physical paint
  sides and rejecting conflicting/non-manifold topology atomically.

Checks and verification boundaries are recorded in
`docs/verification/edge-divisions-2026-10-27.md` and
`docs/verification/topology-tools-2026-10-27.md`.

## Selection, surface placement and solid inspection

- [x] Select Similar by assigned material, world-space edge length or shared
  component definition, with preview count, tolerances and visibility control.
- [x] Paste on Face with picked insertion point, rotation/normal/offset preview,
  shared clipboard/import placement and one Undo step.
- [x] Addressable solid diagnostics across component occurrences, selection of
  problem geometry, loose-edge removal and validated closed-shell orientation
  repair that retains cavities and refuses ambiguous topology.

## Path arrays, polyline points and quantities

- [x] Component arrays along open or closed edge paths, measured count/spacing,
  tangent alignment, reverse direction, start offset and a bounded preview.
- [x] Exact point coordinates for grouped polylines, stable IDs, preview and
  topology-preserving edits that refuse ambiguous crossings or bent faces.
- [x] Portable CSV quantities with transformed geometry, occurrence paths/counts,
  visibility filters, definition summaries and spreadsheet-safe text.

The autonomous stage and acceptance checks are in
`docs/development-stage-2026-10-27.md`. Detailed behaviour and limits are in
`docs/editor-expansion.md` and the English application guide.
