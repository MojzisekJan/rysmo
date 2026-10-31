# Architecture

Rysmo is a stack of crates. Everything that decides what the model is and how it
changes lives in plain Rust crates with no UI dependency and is tested without a window.
Only the top crate knows about Bevy and egui.

```
rysmo_mobile   Android and iOS entry points
rysmo_app      Bevy + bevy_egui: window, rendering, input, panels, file dialogs
  rysmo_mcp    MCP server on the same editor
  rysmo_tools  tools as state machines, VCB parser, touch gestures
    rysmo_infer  inference engine
    rysmo_io     .rysmo, OBJ, glTF, COLLADA, DXF
      rysmo_model  Document, Command, Editor (undo, selection)
        rysmo_library  starter materials and generated textures
        rysmo_mesh     edge-and-face mesh, sticky geometry, push/pull, move, copy
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
thousands of elements a clone is cheap; the history keeps 200 steps.

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
- `render`: the sun and its shadows, ambient light, ambient occlusion, tone mapping
  and anti-aliasing from the model's style, with Bevy's own features;
- `ui`, `actions`, `panels`: menu bar, toolbar, status bar with the VCB, inference
  marker and tooltip, Scale's grips, the context menu, the Materials and Tags windows,
  touch modifier toggles.

Tools that need their own pointer handling and dialogs (text, 3D text, terrain, Smoove,
Add Detail, Flip Edge) keep a draft in the app tied to the editing context and the
document revision, and drop it when either changes.

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
docs/web.md. Bevy draws with WebGL2 there; ambient occlusion and temporal
anti-aliasing fall back to none and FXAA. The core crates use no threads, files or
clocks. Files open through a file input and
save as downloads (`web.rs`); the MCP server and its TCP port are desktop only.

## Android and iOS

`rysmo_mobile` is the entry point: a NativeActivity for Android and a C function an
iOS host app calls. Input is written for touch from the start.

## Curves

The mesh stores analytic curve definitions (circle, polygon, arc: centre, plane,
start direction, radius, segment count, sweep) keyed by `CurveId`, and sampled edges
point to their curve. Uniform transforms update the parameters; a partial or
nonuniform change turns the curve into a polyline that only keeps the grouping.
Push/Pull and Follow Me copy curves onto the new rings and smooth along them.

## Annotations

Dimensions and texts belong to a definition, like geometry. Their anchors refer to
vertices, points along edges, curve centres or points on faces (three vertices and
barycentric weights), through an instance path when the geometry is inside a group.
Commands refresh the cached anchor positions, so a dimension follows its geometry; when
the geometry is gone, the annotation keeps its last position and shows as no longer
attached.

Move, Rotate, Scale, copies, Make Group, Make Unique and Explode remap the anchors:
links to geometry that takes part in the operation are kept, others are detached.
Previews run the same command on a temporary document. Labels are laid out on screen
once per frame, and drawing, picking and box selection use that same layout.

## 3D text

`model::text3d` turns the bundled DejaVu fonts into outlines, finds holes by
containment and extrudes them with the ordinary Push/Pull, as one new group.

## Terrain

A terrain is a group with a triangle mesh: a grid (`Terrain::grid`) or a Delaunay
triangulation of contour lines (`model::contours`, with Spade). Smoove raises vertices
with a cosine falloff around picked centres, Flip Edge swaps the diagonal of two
triangles and Add Detail inserts a vertex. Each is one command and one undo step; a
typed correction straight after replaces that step (`Editor::amend_last`).

## Solids, sections and scenes

`model::solids` runs Union, Subtract, Intersect, Trim, Split and Outer Shell on two
closed groups or components. Both are checked first (`solid_validation`: every edge
between two consistently facing faces, no self-intersections), the result is built
with a BSP and becomes new groups in one undo step. Section planes clip the drawn
model without changing it, and scenes store a camera with selected view settings.

## Large models

Editing and drawing avoid visiting the whole model once per element.

- `rysmo_mesh::blocks::BlockMap` stores mesh elements and instances in small blocks
  behind `Arc`. An undo snapshot copies a list of blocks; an edit copies the blocks it
  writes to. Write through `get_mut` where only some entries change, since `values_mut`
  unshares every block.
- `rysmo_mesh::spatial` has box trees. Results that depend on id order are sorted, so a
  tree never changes an outcome.
- `Editor::content_revision` ignores selection changes. Anything built from the document
  waits for it; `Editor::revision` also covers the selection.
- Snapping keeps a scene per content revision and a mesh view per definition. A held
  `Arc<Definition>` keeps its pointer valid as an identity, because an edit then has to
  copy it. Meshes with evaluated openings depend on other definitions and are rebuilt.
- `Mesh::vertex_edges` is kept with `edge_index` by `link_edge` and `remove_edge_raw`,
  so an edge's neighbours need no search. Add edges only through those.
- A copied closed shell that lands clear of everything is placed directly, and a move
  of geometry attached to nothing else that lands clear only sets positions. Anything
  that touches takes the full path of intersecting and finding faces.
- The viewport keeps triangles per definition in chunks of a face id range and edge
  lists per definition, reusing what `Mesh::changed_since` leaves untouched. The screen
  is split into pieces, each with its own Bevy meshes, and a piece is rebuilt when its
  signature changes. Style and material changes redraw all pieces. Selected and hovered
  faces are a separate blended mesh, so a click does not rebuild the model.
- `Mesh::view` gives trees, faces at a vertex and edges of a curve without any edit having
  to report what it did. It holds a snapshot of the element maps, compares the mesh with
  it through `BlockMap::changed_since` and adds small trees for the difference, so the
  cost follows the change. Results are checked against the mesh. Take a new view after
  adding elements; the snapshot is replaced once changes reach an eighth of the mesh.
- Rebuilding a plane starts from the changed edges and takes what is connected to them
  plus whatever overlaps that, since anything else cannot share a region with them.

`crates/rysmo_tools/examples/perf.rs` times whole-model operations on scenes of boxes
and `crates/rysmo_io/examples/perf_scene.rs` writes such a scene to a file.
