# Editable modelling and documentation stage

Implement all seven accepted areas as one autonomous stage. Keep GUI and project
text English, portable web/native model code, transactional edits, stable shared
component behaviour and backward-readable documents. Batch source changes before
expensive web release linking. Keep local Git author and committer Jan Mojzisek
<mojzisekjan@gmail.com>. External publication is outside this stage.

- [ ] Live operations: retain editable inputs and parameters for path arrays,
  sweeps and solid operations; regenerate atomically, expose editing/conversion
  in GUI, retain persistence and Undo/Redo, reject invalid/cyclic/over-budget work.
- [ ] Component connection points: named local frames, placement and orientation
  matching, multiple points per definition, editing UI, persistence and Undo.
- [ ] Parametric fitting: fit architectural components to planar targets, expose
  dimension controls and retain generated part materials when dimensions change.
- [ ] Section hatching: material/object settings, angle/spacing controls and
  matching viewport and SVG/PDF section output with bounded generation.
- [ ] Multiple documents: tabs with independent editor history, camera, file state
  and recovery identity; safe close/save, no cross-document draft or async writes.
- [ ] Drawing sheets: persisted pages with several scaled model viewports, title
  blocks and labels, regenerated drawings after model changes, SVG/PDF output.
- [ ] Attributes and reports: typed custom fields on component definitions,
  instance overrides, editable GUI, CSV custom columns/grouping and numeric totals.

Each area requires executable core regressions and appropriate native/WASM checks.
Verify GUI where browser tooling permits it; record actual limitations explicitly.
Finish coherent local commits, updated English guides and a current validated web
package after the related changes. Keep vendor comparison names out of the project.
