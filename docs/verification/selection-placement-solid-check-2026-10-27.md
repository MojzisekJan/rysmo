# Selection, surface placement and solid inspection, 2026-10-27

Implemented Select Similar, Paste on Face and Solid Check with explicit repairs.
Their workflows, limits and compatibility boundaries are documented in
`docs/editor-expansion.md` and in the English application guide.

## Validation

`scripts/check-rust.sh --offline` passes:

- 675 tests across all eight headless library suites: infer 27, I/O 86, library 2,
  math 22, MCP 5, mesh 79, model 304 and tools 150.
- Five standalone recovery tests. The combined Rust count is 680.
- Strict native workspace all-target Clippy, formatting and the web-release WASM
  target check.

Strict web-release WASM Clippy also passes without warnings. All 19 JavaScript
file/recovery tests pass. Diff whitespace checks pass.

New executed coverage includes typed material/definition selection, hidden tags,
relative/absolute length tolerances, world lengths in nonuniformly scaled editing
contexts, read-only queries, arbitrary/mirrored surface frames, antiparallel normals,
rotation and offset, actual Paste Undo and native persistence, diagnostic categories,
non-manifold vertices, selective loose-edge removal, hollow shells with nested
islands, consistent outward/cavity orientation, failed-repair atomicity, no-op
history and persisted repairs. Independent shell fixtures assert their positions
and extents so sticky drawing or ground-face winding cannot change the test geometry.

Application tests for dialog cancellation/staleness, picked surface locking and
nested diagnostic paths are type-checked by Clippy. The GPU-linked native app test
binary was not executed and no interactive GUI verification is claimed.

## Behaviour boundaries

Select Similar inspects the current context and compares assigned material rather
than inherited appearance. Results are cached while query parameters and document
state remain unchanged. Selection does not create a geometry Undo step.

Surface placement uses a component's insertion frame or the clipboard frame. Its
world-space rigid rotation preserves size and shape under nonuniform/mirrored
editing transforms. Placement does not glue or cut the target face. The preview
is bound to document revision, project generation and editing context.

Solid Check inspects each chosen hierarchy mesh separately, not a Boolean union.
Bounded topology diagnostics address edges and non-manifold vertices; detailed
validation can report intersections without identifying one selectable edge.
The overlay draws at most 10000 markers and says when truncated. Counts and
problem selection remain complete. Repairs are explicit and atomic; they do not
fill holes or merge ambiguous joins. Closed-shell orientation uses validated
containment depth so cavities and inner islands keep the correct orientation.
The existing strict CSG validator continues to require correct shell orientation.

No dependency or persistence-version change was needed. No release linking,
new web package, push or external publication was performed in this change.
