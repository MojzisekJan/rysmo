# Architecture

Rysmo is a stack of crates. Everything that decides what the model is and how it
changes lives in plain Rust crates with no UI dependency and is tested without a window.
Only the top crate knows about Bevy and egui.

```
rysmo_app      Bevy + bevy_egui: window, rendering, input, panels, file dialogs
  rysmo_tools  tools as state machines, VCB parser, touch gestures
    rysmo_infer  inference engine
    rysmo_io     .rysmo, OBJ, glTF
      rysmo_model  Document, Command, Editor (undo, selection)
        rysmo_mesh   edge-and-face mesh, sticky geometry, push/pull, move, copy
          rysmo_math   tolerances, planes, rays, polygons, orbit camera
```

Bevy API changes between releases therefore touch only `rysmo_app`.

## The mesh

The model uses edges and faces: an edge joins two vertices and may border any
number of faces (none for a stray line, three or more where surfaces meet); a face is
flat, with an outer loop and holes. A half-edge structure cannot hold stray edges or an
edge shared by three faces, so `rysmo_mesh` stores:

- vertices with positions (`f64`, metres, Z up);
- edges as vertex pairs, with an index from the pair to the edge;
- faces as an outer loop and hole loops of vertex ids, counter-clockwise seen from the
  front, plus front and back materials.

Adjacency (the faces along an edge, the edges at a vertex) is looked up when needed.
That is linear in the model size, which is fine for the first milestone's models; a
cached adjacency index is a later optimisation.

All element kinds draw ids from one counter, so an id is unique in the document and
stable across edits and files.

### Sticky geometry

Two routines implement sticky geometry.

`insert_segment` adds a line: it welds the ends to vertices within the tolerance
(1 µm), splits existing edges where the new line ends on them or crosses them, splits the
new line at every vertex on it, and reuses edges it overlaps. Face loops get the new
split vertices threaded in.

`rebuild_plane` recomputes the faces of one plane. It takes every edge in the plane,
walks the planar graph turning as far right as possible at each vertex, and so traces
every region: bounded regions counter-clockwise, the outside of each connected piece
clockwise. A clockwise outline inside a bounded region of another piece is a hole in it.
Dangling edges come out as back-and-forth steps and are dropped from the loops. Each
region becomes a face if it lies inside a face that was there before (it keeps that
face's material and facing; one part keeps the old id) or if a newly drawn edge bounds
it. A region whose loops equal an old face's leaves that face untouched.

Drawing lines calls `rebuild_plane` for every plane the new edges lie in with an
existing face or a neighbouring edge. Erasing the edge between two coplanar faces calls
it with no new edges, so the two regions become one face. Vertices left between two
collinear edges are healed away.

New faces with nothing to inherit from face consistently with the faces they share edges
with; failing that, down on the ground (so a pulled-up box ends up facing outwards), then towards the camera.

### Push/Pull

`push_pull` moves the face along its normal, joins the old and new outlines with side
quads, and then:

- leaves a copy behind if the face stood alone (no other face along its edges) or the
  user asked for it (Ctrl), so the result is a closed solid;
- merges side faces with coplanar neighbours, so pulling the top of a box stretches the
  box instead of stacking a second one on it;
- cuts a hole when the face lands inside an opposite-facing coplanar face (pushing a
  rectangle through a slab).

### Transforms, copies and auto-fold

`transform_vertices` applies any map to a set of vertices (Move, Rotate, Scale). A face
that would stop being flat is automatically folded: it is split
into triangles worked out before the move (so they are valid), the move is applied, and
triangles that ended up coplanar are merged back, which leaves the fewest folds. A face
that would collapse makes the whole operation fail with nothing changed.

Copies go through a `Template`, the edges and faces of a selection lifted out of a mesh,
which `stamp` draws back under a map with the sticky line routine and then gives the
new faces their facing (from the mapped normal, so mirror images stay right), materials
and edge flags. Copy arrays, radial arrays, explode and make unique all use it.
`extract` moves geometry out of a mesh into a new one, with fresh ids; Make Group and
Make Component use it.

### Edge flags, Offset and Follow Me

Edges carry `soft`, `smooth` and `hidden` flags. Soft and hidden edges are not drawn or
picked; smooth edges blend the shading of the faces on both sides
(`Mesh::smooth_normals`). Push/pull and Follow Me soften the new edges between faces
that meet at less than 25 degrees, so a pulled circle is a round cylinder.

Offset mitres a closed outline or an open chain in its plane and refuses distances that
would fold it over itself. Follow Me sweeps every loop of a face along a chain of
points, mitring each corner on the plane halfway between two segments; an open path
keeps the face as one cap and closes the other end, a closed path makes a ring. New
faces are oriented consistently from one seed and turned outwards by the sign of the
enclosed volume.

## Groups, components and the editing context

The document has a root definition (the model's own geometry) and a map of group and
component definitions. A definition holds a mesh, construction guides and instances of
other definitions; an instance is a definition id, a transform into its parent, a name,
a tag and an optional material for faces without one. Groups are definitions marked as
groups; when a copied group is opened, the editor first gives it its own definition
(Make Unique). Unused definitions are purged after every command.

All meshes draw ids from one document counter (`Document::with_mesh` syncs it), so ids
stay unique across the whole file.

The editor keeps a context path, the instances opened from the model down. Commands
work in the definition at the end of that path, in its own coordinates, and the tools
see a camera view folded through the path's transform (`View::local`), so drawing
inside a rotated component works the same as drawing in the model. Undoing past the
command that made a group closes the context.

`Document::occurrences` walks the instance tree (skipping hidden tags) and lists where
each definition is drawn, with its transform, whether it is inside the context, the
child of the context it belongs to (what a click selects) and the material it inherits.
The viewport, the exports and the inference engine all build on that list.

## Commands, the editor and undo

`rysmo_model::Command` is a serde enum with every change a user can make: draw lines,
rectangles, circles and arcs; push/pull, move, copy, rotate (with radial copies),
scale, offset, Follow Me; erase, reverse faces, edge styles; make group, make
component, explode, make unique; guides; materials, paint, tags; units. Each command is
plain data in the coordinates of the context being edited, with no reference to the
camera or the UI.

`rysmo_model::Editor` is a headless session: a `Document`, an undo history, a
selection, the editing context, and a revision counter that viewers watch to know when
to redraw. `Editor::apply(command)` is the only way the document changes:

1. it clones the document;
2. runs the command on the document;
3. on success, pushes the clone and the command's label on the undo stack and clears
   redo; on error, puts the clone back, so a failed command never leaves a half-done
   change.

Undo and redo swap whole documents. That is simple and cannot go wrong, and for models of
thousands of elements a clone is cheap; the history keeps 200 steps. When models grow,
the plan is to store per-command diffs of the elements that changed (the stable ids make
that straightforward) without changing the `Editor` API.

The selection is not part of the history. It is pruned when an undo or
a command removes selected elements.

## Inference and tools

`rysmo_infer::infer` takes the mesh being edited, the other visible meshes (groups,
components and the faded model around the context, each with a transform into the
context), the context's guides, the camera's `View`, the cursor, the operation's start
point, an optional lock and recently touched points. It returns what the cursor means:
the point, its kind (endpoint, midpoint, on edge, on face, axis, from point,
intersection, guide point, on guide, ground), the edge and face involved when they are
in the mesh being edited, the group the point is on otherwise, the plane to draw in, and
a guide line. Points and edges hidden behind faces are skipped; soft and hidden edges do
not snap. Kinds carry colours and tooltips. `rysmo_infer::pick` answers
what a click selects: an edge, face or guide of the context, a whole group or
component, or something outside the context (a click there closes it).

`rysmo_tools::ToolBox` holds the active tool and its state machine. The app feeds it
pointer events (move, down with a click count, up), keys (Esc, arrows, Shift) and VCB
text, with the camera view and the modifiers; the tools call `Editor::apply` and describe
what to draw as a `Preview` (lines, the inference, highlighted faces and edges, the
selection box). Because the tools only see the `Editor` and a `View`, the tests drive
them with a synthetic camera and projected points.

The core owns the camera (`rysmo_math::OrbitCamera`) and the app copies it to Bevy each
frame, so what the tools measure against is exactly what is drawn.

## The app

`rysmo_app` has one resource, `App3d`, with the editor, the toolbox, the camera and UI
state. Systems:

- `input`: window size, keyboard (shortcuts and the VCB), mouse read as an ordered
  `WindowEvent` stream (so a click is handled where it happened even when frames are
  slow), touch through `rysmo_tools::gesture`;
- `viewport`: the camera; two unlit face meshes (opaque, and blended for see-through
  materials) rebuilt from the occurrence list when the editor's revision or the
  highlights change, with everything outside the edited group faded; edges, guides,
  bounding boxes of selected groups and tool previews as gizmos;
- `ui`, `actions`, `panels`: menu bar, toolbar, status bar with the VCB, inference
  marker and tooltip, Scale's grips, the context menu, the Materials and Tags windows,
  touch modifier toggles.

Shading is done in vertex colours from a fixed sun direction, so the look does not
depend on lights, shadows or post-processing, and the same picture comes out on WebGL2
and GLES.

## Scripting and MCP

The MCP server (`rysmo_mcp`, see docs/mcp.md) is a second front end on the same
`Editor`, next to the UI. Its tools map onto `Command`s with JSON arguments; element
arguments are plain ids, since ids are unique in the document. Read-only tools return
`describe_model` data or the `.rysmo` text, so an agent sees the same ids the file
has.

It runs in three ways: the `rysmo-mcp` binary on stdio (no Bevy in it), the app
binary with `--mcp` (the same, no window), and the app with `--mcp-port`, where socket
threads pass lines to a Bevy system that runs them against the window's editor on the
main thread. Because commands are the only way to change a document, there is one code
path to test and every front end gets the same sticky behaviour and undo.

## Web build

The same `rysmo_app` crate builds for `wasm32-unknown-unknown` with trunk; see
docs/web.md. Bevy draws with WebGL2 there, and the renderer needs nothing WebGL2
lacks: an unlit material with vertex colours and gizmos, no compute shaders and no
storage buffers in vertex shaders (the problem Heron Brush's renderer has on WebGL2).
The core crates use no threads, files or clocks. Files open through a file input and
save as downloads (`web.rs`); the MCP server and its TCP port are desktop only.

## Android and iOS

Input is already written for touch (see the README). The Android build would follow
Heron Brush's NativeActivity route with Bevy's `android-native-activity` feature; iOS
through Bevy's iOS support. Storage and file pickers differ per platform and are the
main work.

## Curve entities

The mesh stores analytic curve definitions keyed by `CurveId`; sampled edges reference
that ID. The curve kind, centre, plane, start direction, radius, segment count and signed
sweep describe circles, polygons and arcs. Polylines retain grouping after deformation.
A separate faceted flag preserves polygon side edges even when analytic parameters
no longer apply. Curve IDs use the same document counter as geometry IDs.

Copies and group extraction remap curve identities. Uniform transforms update analytic
parameters; partial or nonuniform changes downgrade to polylines. Push/Pull and Follow
Me copy curve definitions onto section rings and use curve membership for smoothing.
The v4 native format stores curve definitions with their context and edge references;
older formats remain readable. Radius changes use the existing vertex transform and
undo system. Segment resampling and further curve editing workflows remain unfinished.

The staged `rysmo_model::dimensions` module defines dimension measurements and
geometry associations. Its anchors resolve vertices, edge parameters and analytic
curve centres relative to an owning definition and nested instance paths. Radial
measurements reject ellipse-producing instance transforms; missing/deformed
geometry uses a clearly non-associated fallback. Dimensions now belong to definitions and support create/edit/erase commands,
Undo and native persistence. Geometry commands refresh retained anchor values;
grouping, Make Unique and Explode remap their references. World-space layouts now drive the renderer for loaded linear and radial
dimensions, including transformed instance measurements and arc-sweep-constrained
radius tips. The Dimensions tool creates point, edge, circle and arc measurements.
Click/box selection and Entity Info share layout geometry and font measurements.
Text size caches use label, size and family keys to keep identical strings with
different styling independent. Entity Info edits text, linear text position,
endpoint style, size, proportional/monospace family and colour. Global defaults and broader grouping/copy audits remain open. Text can align
to the screen or dimension line; oriented screen rectangles drive its background,
rotation, precise point picking and box contact without selecting empty corners
of an axis-aligned bounding box.

Expert dimension visibility is stored in `Style.dimensions`. Layouts evaluate
projected measurement-string size and the angle between the string and camera
ray; rendering and point/box picking use the same result. Non-associated warning
colour yields to selection highlighting. The Styles panel edits these model-wide
settings through the validated, undoable SetStyle command.

Dimension creation snapshots `Style.dimension_defaults` into its draft. The Styles
panel exposes separate Select All and Update Selected controls for active-context
dimensions; ApplyDimensionDefaults validates all IDs before updating their styling
as one undo operation. Changes to the defaults alone preserve stored dimensions.
Nested/group selection semantics still require the broader editor audit.

Make Group/Component moves selected annotation IDs into the new definition and
remaps their geometry/child-instance links into its coordinates. Outside geometry
references detach with retained values. Definition bounds include dimension
string/extension geometry and label anchors, so annotation-only groups have
world bounds; screen font extents remain a separate selection concern.

Translated copies of dimensions retarget only links to geometry/instances selected
for copying. Other anchors remain attached to their original measurement while
the annotation offset moves; copied linked geometry cancels that extra offset
translation. Copy arrays return dimensions as generic created elements and work
with Move tool correction/Undo. Rotated copies transform annotation geometry and
retarget only copied associations; other links detach with rotated fallback values.

Editor.preview_dimension_move predicts selected annotation placement without
history changes. Annotation-only previews calculate offsets directly; mixed
selections execute the corresponding command in a temporary document and detach
the resulting anchor values. ToolBox carries these dimensions in Preview and the
app paints them through the common dimension renderer. Group-owned annotations
use the occurrence-based preview path described below.

Rigid rotation of selected dimensions maps anchors, placement normal, projected
measurement axis and offset. Links survive only when their geometry vertices/full
curve or first path instance participates in the rotation. Rotate and Ctrl+Rotate
arrays share these rules; font size remains a screen property. Selected dimensions support finite, invertible Scale transforms. Circular
associations lost under anisotropic scaling retain an affine radial frame, so
the arrow follows the transformed curve image while reporting non-association.

Selected-dimension rotation previews use Editor.preview_dimension_rotate and the
same renderer as saved dimensions. Annotation-only previews detach resolved values
and map them directly; mixed geometry selections run the command in a temporary
document. Radial fallback data retains an arc sweep, keeping arrow tips on the
correct sector after detaching. Radial mapped normals use the oriented transformed
frame, preserving mirrored arc placement through Explode.

Scale previews use Editor.preview_dimension_scale; annotation-only predictions
map resolved values directly, while mixed selections run the scale command in a
temporary document. Scale grip bounds include selected dimension lines and label
anchors. Font pixel size remains fixed. Radial fallback frames preserve the
original curve parameter and signed arc sector across anisotropic transforms.
Editor.preview_instance_dimensions supplies detached annotations plus affine
owner-to-context transforms for selected group/component subtrees. Move, Rotate
and Scale compose these transforms with the active editing context at paint time,
preserving nested and anisotropically scaled radial layouts. Group geometry
previews include visible occurrence edges as well as their bounding box. Screen-font
bounds used by selection are supplemented by measured annotation rectangles as
described below. Large-model preview performance remains open.

Uncommitted dimension labels may shift on screen to clear preview gizmo markers;
a short leader connects the shifted label. This does not change model placement
or saved-entity pick bounds. Saved labels keep their normal layout. The avoidance
uses the oriented text rectangle and preserves Outside Start/End direction.

Point inference exposes analytic curve centres as a separate Center kind with a
curve identity and placement plane. Dimension anchors record CurveCentre and a
relative instance path, matching the picked occurrence and location. Curves
outside the editing subtree can snap without attaching an invalid relative link.
Polyline records and curves with no visible member edges do not offer centres.

Rendered annotation rectangle hits enter ToolBox through AnnotationBoxHits for
the same pointer-up transaction as mesh and instance bounds. Active dimensions
are selected directly; visible dimensions in nested occurrence subtrees aggregate
any crossing/all enclosed per active child instance. Crossing accepts geometry
or annotation intersection; a window requires geometry and every visible
annotation inside. This uses camera-reprojected measured font sizes and the same
oriented label/endpoint layout as painting. Modifiers apply once after combining
hits, preventing Shift from toggling a group twice. Outside editing contexts and
hidden tagged occurrences do not contribute annotation box hits.

Text annotation data lives in model::text with separate viewport-fixed screen
placement and owner-local attachment. Point anchors reuse dimension point
references; face anchors retain three vertex IDs and barycentric weights so
geometry movement/deformation updates the attachment. Leaders are None,
ViewBased (logical pixels) and Pushpin (owner-local displacement). The layout
projects both leader types with the current occurrence transform/camera.
Editor commands refresh fallback attachment positions before and after edits,
including deletion, and Undo restores the previous document. Text IDs share the
global counter. Make Unique creates fresh IDs and remaps local geometry/child
references; Explode transfers annotations to the parent. This data/command layer
is implemented ahead of the interactive Text tool: creation
UI, text settings, explicit transformations and external-reference remapping
through grouping/uniqueness/explosion still require implementation and audit.

Text now has a selectable Element::Text ID. The overlay measures egui galleys and
caches by content/font size/family, then draws TextGeometry: the text rectangle,
two-segment leader and None/Dot/Open/Closed marker. The same screen geometry
drives pointer and box hits, including nested instance aggregates. Screen text
stays at its viewport fraction; attached text is reprojected from its owning
occurrence on each frame. Attached text behind the eye is omitted in perspective
and orthographic views. Text paints after dimensions and wins equal-distance
annotation hits. Select All/Erase/Undo and MCP element ID decoding include texts.
Standalone selected-text transformations and grouping currently reject the
operation explicitly and atomically until their full semantics are implemented.

Entity Info now edits a single selected Text. A draft buffer is isolated from the
document until Apply/Ctrl+Enter, and resets after selection/file replacement,
Escape/discard or window closure. Font size/family/colour and leader endpoint/type
changes use SetText with continuous parameter-drag undo coalescing. Content commits
do not coalesce into style drags. Double-click selection opens Entity Info.
ViewBased/Pushpin conversion preserves the current projected label position by
intersecting its view ray with the screen-facing plane through the attachment,
then converting to owning coordinates. Removing the leader places the label at
its geometry anchor; adding one starts with an 80/-40 pixel displacement.
Creation, global defaults and selected-text transformation semantics remain open.

Styles and Shadows exposes persisted TextDefaults with separate screen and
attached fonts, endpoints and new-text leader mode. Per-type selection stays in
the active editing context. ApplyTextAppearance validates the entire ID list
before updating the appropriate font and endpoint, retaining content and
placement. It detects no-op updates and participates in normal Undo. Applying
the default leader mode to existing selections is still separate pending work.

ApplyTextDefaults now extends appearance updates with leader conversion using
the supplied camera/viewport/context transform. It prepares all updates before
mutation, then commits one Undo step; screen placements remain unchanged.

The app text_tool controller stages TextCreation against a context/revision.
ToolBox supplies inference/point references while the app handles pixel placement
and egui content entry. Both saved and draft text use paint_text/TextGeometry.
Blank canvas clicks produce screen text; nearest in-context face hits produce
barycentric anchors and occurrence-scaled area text. Face double-click skips
leader placement. Commit uses AddText once; cancellation never modifies history.
Stale drafts are hidden/discarded before rendering or entry.

Text-tool annotation picking respects active-child instance proxies. Single-click
selects the proxy, while double-click enters owning contexts until the annotation
itself is editable, opening Entity Info without a duplicate draft. Scaled-context
creation is integration-tested for owner-local anchors and world-space face area.

Text creation derives initial content from owner-relative references: component
or explicit instance names take precedence for child occurrences, edge length
is measured through its world transform, and point coordinates use world/model
units. Explicit face double-click bypasses naming to show area. Content is still
a stored editable string; automatic geometry-driven text-value refresh needs an
additional audit/implementation.

No-leader text has an optional owner-local displacement (legacy zero) so moving
a label need not detach its geometry. Text.at_label_position preserves references
for every placement/leader mode, using the current label's view plane for spatial
displacements. Entity Info exposes pixel placement for screen/view-based labels
and unit-aware spatial offsets expressed in the active drawing basis. Native
version 7 persists displaced no-leader text. Normal Move-tool integration remains
pending; removal of a leader now retains its displacement instead of resetting
placement to the anchor.

MoveWithText serializes the local editing View and drag pivot, prepares all text
targets, runs geometry movement once, and places labels while retaining their
references. Spatial offsets preserve full 3D displacement; screen/view-based
placement is projected through the input view. Fully moved view-based anchors
retain their prior pixel offset. Tool Last::Move retains the view for precise
correction. preview_text_move predicts mixed selection through a temporary
document and returns resolved detached labels to the common renderer. View JSON
uses serde_json float_roundtrip so the snapshot remains numerically identical.

preview_instance_texts mirrors the occurrence-based dimension preview path,
returning detached owner-local texts paired with composed affine transforms.
Group/component Move, Rotate and Scale populate Preview.instanced_texts, which
the overlay paints in the current editing context through the common renderer.

CopyWithText prepares original text snapshots, copies the non-text selection per
array step, then retargets only references whose geometry/first instance was
copied. Each text gets a fresh ID. Label displacement compensates for any moved
anchor, retaining old associations for text-only copies. Tool Last::Copy stores
its input view and pivot for distance/count/division corrections. Copy preview
returns resolved detached snapshots without modifying the source document.

transform_items now handles selected attached texts through transformed_links: an
affine map updates placement and retains only participating geometry references.
copy_items similarly includes texts and remaps copied references; non-translation
copies detach references to uncopied geometry. Rotate previews use the same
operations on a temporary document. Font pixels/view-based offsets remain fixed
and viewport screen text ignores ordinary rotation.

Selected-text Scale uses transform_items and preview_text_transform. Grip bounds
include attached anchors and spatial label ends; view-based label positions are
projected onto the anchor plane using the local view covector. Font pixels and
view-based offsets remain unchanged, while spatial offsets follow affine scaling.
A valid Scale of only screen text is a no-op without a history entry.

make_definition transfers selected texts with their existing IDs, includes their
spatial placement in the new origin bounds and remaps their references into the
extracted mesh/children. Text::reparented updates external point/face references
through the inserted instance, including paths to regrouped children. Screen
positions and font pixels are unchanged; ungrouped geometry links on moved texts
fall back to their cached placement. Editor history makes the operation atomic.

Text::remap_instance_reference handles external annotations for MakeUnique and
Explode alongside dimensions. It maps the reference suffix relative to the
changed instance using Text::remapped, then restores the unchanged prefix and
retains/removes the instance as appropriate. Owner-local fallback positions and
leader offsets stay unchanged because the external annotation itself did not
move. Failed remapping detaches the reference and retains its cached position.

3D Text geometry is generated in model::text3d from bundled DejaVu TrueType
fonts via ttf-parser (Sans/Serif/Mono, regular/bold; redistribution license in
model/fonts/LICENSE.txt). Quadratic/cubic outlines become polygonal contours.
Contour containment determines filled regions and holes; extrusion uses ordinary
Mesh::push_pull. Height is measured using capital H, multiline alignment uses
advance widths. CreateText3d creates one editable group in an explicit placement
frame with fresh global IDs and one history entry. Dialog and interactive placement are implemented in text3d_tool. Pair kerning
uses the bundled fonts’ horizontal kern tables; glyph positions and multiline
alignment share the same advance calculation. Complex-script shaping remains
outside the current typography implementation.

App text3d_tool owns a revision/context-bound draft, settings strings and prepared
preview mesh. A toolbar/menu action opens it; Place validates model-unit values
and builds the mesh. Pointer hover uses ToolBox inference in the active context;
commit uses CreateText3d in the current drawing frame. Wireframe rendering maps
through the editing-context transform. Input routing prevents regular Move from
mutating history while the draft is active; exact distance input commits the
placement. Tool/file/Escape/context changes discard drafts. Session defaults are
separate from document geometry.

Terrain::grid constructs an indexed XY triangle mesh directly with shared
vertices/edges, soft/smooth diagonals and visible square boundaries. It keeps
exact requested dimensions with partial boundary cells and suppresses floating
point sliver cells near an exact spacing multiple. CreateTerrainGrid allocates
global IDs directly, places one Terrain group in a perpendicular local frame,
and is one Undo operation. Dense/invalid grids fail before allocation.
App terrain_tool holds a context/revision-bound three-stage draft. It uses the
active red/green plane, ToolBox inference and a ray-plane fallback; width is
constrained perpendicular to the baseline. Preview renders grid lines and length
ticks outside history. Measurements routes spacing/length/width inputs to this
controller. The model command keeps faces oriented toward the active drawing
normal when width is drawn in the negative direction.

ToolBox::draft_inference exposes the existing scene-aware inference query to
staged app tools without starting a modelling operation. From Scratch supplies
the chosen origin/baseline endpoint and a plane or line lock explicitly, so
axis/Shift constraints and perpendicular-width inference use the draft geometry
instead of idle Move state. The query still includes guides and other instances.

Editor::amend_last prepares a replacement document from the last history step’s
original baseline. It commits only after successful command execution, preserving
that baseline as one Undo step; failures leave revision/history/selection intact.
From Scratch stores a context/revision-bound LastGrid after confirmation, allowing
width or length,width corrections until another click/tool/file/model change.
The first point uses unrestricted scene inference, so elevated geometry/guides
can establish the grid height; later stages lock the plane at that height.

Terrain::smoove applies a cosine radial falloff in the plane perpendicular to
the supplied normal; multiple centres use maximum influence. The caller supplies
eligible terrain vertex IDs so adjacent unrelated geometry is excluded. Input
validation requires triangle faces at those vertices. Mesh::deform_vertices
updates positions/affected curve descriptors atomically without welding or
changing topology/IDs, and rejects invalid output geometry. Smoove is a model
command with no-op detection and normal history/annotation refresh. App smoove_tool provides the staged mouse/Measurements controller. Connected
triangle vertices form the eligible region; selected edge/face vertices provide
seed centres. Idle hover draws a brush circle, active preview clones/deforms the
mesh outside history and shows changed edges plus affected vertices. Vertical
movement uses line/ray proximity; Shift uses a view plane for tool-selected
centres. Commit uses Command::Smoove, retains session radius and stores the
last offset for double-click repetition. Context/revision guards cancel stale
drafts.

Smoove detects exact seed positions through a hash set before searching centres.
This avoids a quadratic centre search for a fully preselected terrain while
retaining identical falloff for surrounding vertices. A 40×40-cell regression
verifies uniform movement and unchanged triangle counts. Native-format tests
also preserve a deformed mesh’s positions/faces/flags and verify further edits
and Undo after reopening.

Smoove drafts retain the committed command for immediate offset/radius
correction until another click or invalidation. Corrections use Editor::amend_last
over the operation’s original baseline, then refresh repeat metadata. A no-op
replacement now restores that baseline and removes the superseded history step,
allowing a zero Smoove correction without a phantom Undo entry. Failure remains
atomic. Measurements displays the committed offset during the correction phase.

SmooveInfluence projects seed centres into the normal plane and builds a balanced
2D nearest-neighbour tree with duplicate projected points removed. Deformation
and brush highlighting use the same weight query; extreme non-finite projections
retain the exhaustive fallback. The app caches affected vertex IDs/seed markers
for a fixed draft radius, so rendering no longer searches all centres per vertex
per frame. Preselection initializes the highlight cache immediately. Regressions
compare indexed and exhaustive falloff for a partially selected 100×100-cell grid
and non-axis-aligned normals/duplicate centres.

model::contours triangulates 3D contour segments with Spade constrained Delaunay
triangulation. XY sites are normalized for numerical range; original 3D samples
remain unchanged. Equal-height intersections receive an interpolated split
vertex, and incompatible heights at duplicated/crossing/junction sites are
rejected. Returned constraint edges are checked against their source segment’s
height. Contours and convex-hull boundaries stay visible; other edges are
soft/smooth. The kernel fills the XY convex hull and allocates IDs from a supplied
start. TerrainFromContours reads selected edges in the active definition, converts
them through the current drawing frame for triangulation and creates a translated
terrain group with fresh IDs. The new definition retains the drawing-frame
orientation. Sources remain untouched. The app action accepts selected edges
and face boundaries, cancels staged tools, runs the command and selects the group.

TerrainFromContours now maps selected source curve members onto generated
constraint edges. Whole unsplit curves keep their translated analytic descriptor
under fresh curve IDs; partial/split membership becomes Polyline. The source
mesh stays unchanged. Native round-trip regression verifies contour-generated
terrain positions, faces, edges and curve descriptors/membership, then further
Smoove editing with exact Undo.

Mesh::flip_terrain_edge atomically replaces the shared diagonal of two triangles.
It checks opposite vertices, existing diagonal conflicts and projected convexity,
preserves face IDs/material/back-material/UV fields and transfers diagonal flags
to a fresh edge ID. Removed curve members leave remaining curves as Polyline
with an updated count; empty descriptors are pruned. FlipTerrainEdge wraps the
operation in normal document history and returns the new edge. Mesh::can_flip_terrain_edge reuses the geometric plan without cloning the mesh.
App flip_tool uses frontmost active-face picking, screen-space edge proximity
and this eligibility query, exposing soft diagonals while ignoring hidden edges.
Hover draws a magenta line; click applies the command, selects the new edge and
updates the mode revision. Context/revision guards and Escape/tool/file changes
cancel it. Native round-trip regression preserves the replacement topology and permits
continued flipping/Undo. Removed-edge text references retain cached fallback
positions as non-associated and recover through exact document Undo.

Mesh::add_terrain_detail inserts a fresh terrain vertex into a triangle or splits
an edge and all adjacent triangles. It preserves old face attributes, reuses one
face ID per original triangle and allocates the additional faces. New internal
edges are soft/smooth; split boundaries retain flags. Split curve membership
becomes Polyline with the updated segment count. An existing vertex is moved
without insertion; zero displacement is a no-op in that case. Geometry validation
and displacement are atomic. AddTerrainDetail wraps insertion plus movement in
one history step. App detail_tool stores a context/revision-bound face/point/offset draft and
uses the same insertion kernel on a cloned preview mesh. Vertex snaps take
precedence over edge snaps. Vertical motion uses line/ray proximity; Shift uses
a view plane. Commit creates one history operation and returns to point picking.
Measurements supplies exact offsets and the overlay shows changed edges/new
vertex. Further correction/annotation/persistence audits remain pending.

Add Detail drafts retain the committed command and whether it inserted a vertex,
permitting immediate offset corrections over the original history baseline.
Correcting a new insertion to zero retains its subdivision; correcting an
existing-vertex move to zero removes the superseded operation. New clicks and
normal draft invalidation end correction. Native-format regression preserves
subdivision topology/flags/placement and verifies another edit plus exact Undo.

Add Detail annotation regressions verify that existing-vertex text references
follow offset/correction and cached fallback updates, with exact Undo/Redo and
zero restoration. Removed split-edge references retain their last point as
non-associated and recover their live association through document Undo.
