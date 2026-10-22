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
`docs/editor-expansion.md`. Release packaging is the final verification step.
