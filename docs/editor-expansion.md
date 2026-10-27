# Additional editor workflows

All GUI labels are English. The current file version is 20; versions 1 through 19
still load. See [editable workflows](editable-workflows.md) for live operations,
component connections, section hatching, document tabs, sheets and custom attributes.

## Model and underlay import

File → Import accepts OBJ, embedded GLB 2.0, ASCII DXF and native models. Preview
controls specify source units, OBJ up axis and DXF curve detail. Placement creates
one component through the normal paste preview. Escape cancels; one Undo removes
its placement. Import does not change the current project's Save destination.

OBJ retains polygon geometry, linework and UVs. Flat polygons stay single faces; bent
polygons and polygons whose UVs are not one affine map are triangulated with hidden seams. External MTL/image companions and
authored vertex normals are not loaded. GLB accepts embedded PNG/JPEG images,
material colours (including texture tint), scalar roughness/metallic and normal maps; static geometry is
retained for animated/skinned models. Required unsupported extensions are rejected.
Optional glTF extensions are reported and ignored; required extensions are rejected.
Metallic/roughness channel images are diagnosed and their scalar factors retained.
Authored vertex normals and non-repeat sampler modes are not retained; surface
normals are reconstructed.
DXF supports linework, lightweight/legacy polylines with bulges, arcs, circles,
3DFACE/SOLID/TRACE and nested block inserts. Source layers become visibility tags.
Closed polylines remain underlay linework. Unsupported entities are reported.

File imports are capped at 64 MiB. Geometry expansion, image pixel counts,
hierarchy depth and total decoded vertices are bounded independently. Embedded
GLB images are limited to 16 million pixels each and 32 million pixels in total.
Repeated normal maps are shared in GLB exports.

## Portable components

File → Component library supports selection capture, names, descriptions, search,
isometric wire previews and placement in another project. `.ryslib` collections
can be saved and merged on desktop and in the browser. The library survives New
and Open during the session. Save a collection to retain it after closing the app.
Collections contain up to 256 assets and 32 MiB of native model source.

## Parametric components

Model → Parametric components creates windows, doors and stairs in local metres.
Windows expose dimensions, frame thickness and column count. Doors add an opening
angle; stairs expose width, rise, tread and step count. Panes are editable ordinary
geometry; assign glass through materials. These components do not automatically
cut openings in surrounding walls.

Select an instance, open Edit selected component, adjust its parameters and Update.
Shared instances regenerate together in one Undo step; Make Unique isolates a
copy. Regeneration preserves front/back paint, UVs and hidden state on matching
generated parts. Instance paint and transforms remain. Components with manual geometry changes or added annotations
are rejected rather than overwriting those additions.

## Scaled drawings

File → Export drawing supports orthographic top/front/right/isometric SVG and PDF,
A4/A3 orientation, physical margins and scale denominator. Active visibility and
sections are respected. Hidden-line removal has a bounded work budget; wireframe
output can be chosen for larger models. Attached text and dimensions can be
included; screen-fixed notes stay outside the drawing. PDF embeds a Unicode font.
Print at 100% and disable printer fit-to-page to preserve scale. A model that does
not fit at the requested scale produces an error instead of silently rescaling.

## Photo matching

Camera → Photo matching loads a photograph with two red and two green perspective
lines, an orange origin marker and a blue reference endpoint. Match the line pairs
to two perpendicular ground-plane directions. Set the measured distance between
origin and reference in metres; Reverse X/Y resolves direction ambiguity. Apply
calibrates the camera and composites the model over the photograph. Direction choices
that would place the camera below the ground are rejected; toggle Reverse X or Y. Orbit, pan or
zoom detaches the photo; Apply restores its matched view. Guides and the original
image are saved in the project and participate in Undo.

Calibration assumes centred principal point, square pixels, no skew and negligible
lens distortion. Cropped, strongly distorted and near-orthographic photos can fail
calibration. The reference segment must lie on the model's ground plane.

## Material surfaces

The Materials inspector exposes roughness, metallic and PNG/JPEG normal maps with
strength and a DirectX Y-flip option. Sun/Studio lighting shows these effects; Flat
lighting remains unlit. Normal images use linear sampling and generated tangents.
Optional texture tint preserves GLB base colour factors without altering existing
materials. Native files and portable components retain the settings. GLB exports scalar
surface factors and embedded normal maps; a DirectX map is converted to the glTF
normal convention. OBJ, DXF and COLLADA retain their existing export capabilities.

## Nested solid operations

Solid Tools accept nested groups/components with inherited paint and transforms.
Overlapping closed children are unioned before applying the chosen operation.
Each leaf must be a valid closed solid. Hidden/glued children and annotations or
sections inside operands are rejected explicitly. Components can share definitions;
source definitions used by other instances remain unchanged.

Limits are 4096 estimated triangles / 16384 vertices per operand, 4096 occurrences,
32 hierarchy levels and a shared clipping budget. Result welding, stitching,
intersection validation and edge-style matching also have bounded budgets. Invalid
or excessive operations fail atomically and leave the document and Undo unchanged.

## Equal edge divisions

Select straight edges and choose Model → Faces & Edges → Divide Edges, or the
same command in the context menu or command search. Set 2–512 segments per edge.
Green points preview the new endpoints; a single selected edge also reports its
segment length in project units. Divide creates joined segments, retains adjacent
faces and their front/back paint and UV placement, and commits one Undo step.
Cancel, Escape or switching tools discards the preview. Changing the document,
project generation or editing context invalidates it.

The operation accepts at most 8192 resulting segments. Duplicate input edge IDs
are counted once. Missing edges, curve members, evaluated component-opening
boundaries and divisions below model tolerance are rejected without partial edits.
Use Properties segment settings for intact arcs and circles. Divided straight
edges remain separate edges rather than becoming an analytic curve.

## Curve grouping and face orientation

Model → Faces & Edges, the context menu and command search expose three operations:

- Weld Edges groups one connected open chain or closed loop into a selectable
  polyline. Select at least two edges, including every member of any existing
  curves being joined. Branches, gaps and disconnected selections are rejected;
  nearby endpoints are not merged. Positions, faces and edge styles stay unchanged.
  An already complete single curve is a no-op. Joining multiple curves retains
  their polygonal geometry but replaces their analytic descriptors with a polyline.
- Explode Curve removes grouping from every curve touched by the selected edges.
  Even a partial member selection explodes the whole curve. Individual segments
  remain selected, with their geometry, styles and faces retained. Undo restores
  the original curve descriptors, including circle and arc parameters.
- Orient Faces uses exactly one selected reference face and makes its visible
  connected neighbours consistent. The reference direction stays unchanged;
  Reverse Faces can change it first. This does not infer a solid's outside.
  Hidden faces act as barriers and disconnected surfaces are untouched. Reversed
  faces swap front/back paint to preserve physical sides; face IDs and UV placement
  remain unchanged. Non-manifold edges and contradictory orientation constraints
  reject the entire operation without partial changes.

Each operation is one Undo step in the active editing context, including groups
and shared component definitions. Make Unique first to isolate one component copy.
No-op welding/orientation does not create an Undo step. Component-opening
boundaries are protected. Curve grouping accepts at most 8192 input/member edges;
orientation is limited to 100000 context faces and 500000 boundary steps.

## Select Similar

Select one edge, face, group or component and open Edit → Selection → Select
Similar, the context menu or command search. The preview counts matches in the
current editing context and replaces the selection on confirmation, without a
model edit or Undo entry. Escape or Cancel discards it. Nested child geometry is
not traversed.

Assigned material compares material IDs on entities of the same kind. Faces use
front paint; inherited appearance is not substituted for an unpainted entity.
Default matches Default. Instance definition compares the actual shared definition,
including groups if they share one. Edge length uses world-space dimensions,
including nonuniform scale of the editing context. The larger relative/absolute
length tolerance applies, with model tolerance as the lower bound. Absolute input
uses project units and accepts explicit suffixes. Include hidden also includes
softened edges and hidden tags, without changing display settings. Queries are
bounded to 200000 entities; invalid tolerances are reported.

## Paste on Face

Copy an object and choose Edit → Paste on Face, the context menu or command
search. Align to model face is also available in ordinary Paste previews, so
imported models and component-library assets can use it. Move over a visible face
and click to lock the point. Adjust rotation, offset in project units or Reverse
face normal, then Place or Enter. Pick another face unlocks the point. Escape,
Cancel or changing tools leaves the model unchanged. Model/project/context
changes invalidate the preview.

A single component uses its insertion origin and local Z-normal. Other clipboard
geometry uses its clipboard origin and axes. A rigid world-space rotation retains
size and shape even inside a mirrored or nonuniformly scaled editing context.
Hidden and section-clipped surfaces are excluded by the normal model surface
picker. Alignment is a one-time placement, without persistent gluing or opening
cuts. Placement uses the existing transactional Paste operation and one Undo.

## Solid Check and repairs

Model → Faces & Edges → Solid Check inspects a selected group/component hierarchy,
or geometry in the active context. Choose the individual mesh to inspect; nested
meshes are not Boolean-unioned. Orange edges show open boundaries, red edges or
points show non-manifold joins, magenta shows orientation conflicts and blue shows
loose linework. The diagnostic also reports detailed intersection and shell-nesting
failures that cannot always be mapped to one edge. Hidden occurrences/faces are
reported separately. A geometrically valid mesh does not by itself prove that an
entire hierarchy satisfies Solid Tools' annotation/glue/visibility restrictions.

Enter this geometry follows the selected occurrence path into its editing context.
Select problem geometry selects edges in the chosen category, or incident faces
for a non-manifold vertex. Shared component edits affect all copies; Make Unique
first to isolate one. Reports are invalidated after edits, context or project
changes and can be closed with Escape.

Remove loose edges deletes only edges referenced by no face. Repair closed-shell
orientation requires closed, visible manifold geometry without stray edges. It
first makes each shell consistent, validates contacts/intersections and derives
nesting, then turns outer shells outward and cavity shells inward. An island
inside a cavity faces outward again. Physical material sides are retained. Each
repair is one Undo step; failed or unnecessary repairs leave document/history
unchanged. Holes and ambiguous topology are not automatically filled or merged.

Limits: 4096 hierarchy occurrences, 32 relative hierarchy levels, 100000 edges,
20000 faces and 100000 boundary steps per inspected mesh. Detailed diagnostic
intersection checks are limited to 4096 estimated triangles; larger meshes still
receive topology diagnostics with an explicit validation-limit message. Shell
repair is limited to 64 components and the existing solid-validator work budgets.

The diagnostic overlay draws at most 10000 markers and reports truncation. Counts
and problem selection remain complete. Select Similar caches unchanged queries
instead of scanning the context on each UI frame. Validation is recorded in
`docs/verification/selection-placement-solid-check-2026-10-27.md`.

## Component path arrays

Select one component occurrence together with connected edge linework. Edit >
Groups & Components > Array Along Path creates new occurrences along the path,
with a count or measured spacing, start offset, reverse direction and optional
local X-axis tangent alignment. The original occurrence stays in place. Count
means new occurrences, including the start, which can overlap the source. Open
count arrays span both endpoints. Closed arrays avoid a duplicate seam and wrap
an offset around the loop. Spacing arrays keep the requested distance and may
leave a shorter final interval. Distances include the editing context's world
scale. The preview shows at most 256 bounding boxes; confirmation creates the
complete array in one Undo step. Cancel and Escape do not change the model.

A path is one unbranched connected chain or loop, at most 2048 edges. An array is
limited to 1024 new occurrences, 100000 expanded occurrences, one million vertices
and 500000 faces. Groups, protected opening boundaries, disconnected paths and
invalid transforms are rejected. Definitions, instance paint and tags are shared;
existing glue attachments are not copied.

## Exact polyline points

Select a grouped polyline and open Model > Faces & Edges > Edit Curve Points.
Choose an ordered point and enter local X/Y/Z coordinates. Explicit unit suffixes
are accepted. The green model preview is read-only until Apply point. Each Apply
has one Undo step; the window stays open for another point. Vertex and curve IDs,
edge properties, face paint and UV data remain attached to the same entities.

This editor preserves topology. It refuses vertex merging, unjoined edge crossings,
overlaps, non-planar or collapsed faces, other attached curves and evaluated opening
boundaries. Limits are 2048 curve edges, 50000 context vertices and two million
edge intersection comparisons. Analytic arcs and circles use existing radius and
segment controls. Explode and Weld can convert them to an editable polyline.

## CSV quantities

File > Export > Model report (CSV) exports hierarchy paths, component occurrence
counts, own edge lengths in metres, face areas in square metres and available solid
volumes in cubic metres. Visible-only filtering is optional. Mirrored and nonuniform
parent transforms are included. Section planes do not trim quantities; soft edges
are included. Each assembly lists its own geometry, with children
in separate rows, so totals do not count nested geometry twice. Optional component
summaries are excluded from the total. The occurrence count excludes the root.

An unavailable volume is blank with a reason. Excluded hidden geometry makes volume
unavailable. Single-shell quantities use the closed, consistent topology check used
by Properties; use Solid Check for geometric self-intersection diagnostics. Compound
shells also require validated nesting. Quantities are not a Boolean union of
intersecting occurrences. Empty assemblies have no own volume.

CSV has deterministic ordering, CRLF records, quoted text and escaped spreadsheet
formula prefixes. Export does not change the model, its Save destination or Undo
history. Traversal is limited to 32 hierarchy levels, 100000 occurrences and two
million evaluated geometry elements/boundary steps; excessive or cyclic reports
fail without a partial download.
