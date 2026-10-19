# The .rysmo file format

A `.rysmo` file is UTF-8 JSON laid out with one element per line. People, scripts and
AI agents can read and change it with ordinary text tools, and a diff of two versions
shows exactly which elements changed.

The writer uses version 17. The reader also accepts versions 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15 and 16; the example
below remains a valid version 2 file.

## Example

A 1 m by 0.5 m rectangle on the ground, painted, and a 0.2 m cube made into a component
"Block" with two instances, one of them on the tag "Furniture" and turned a quarter
turn. A guide line runs along red at y = -0.5 m. The test suite loads this example, so
it stays valid.

```json
{
"format": "rysmo",
"version": 2,
"length_unit": "m",
"display_units": "mm",
"materials": [
  {"id":10,"name":"Oak","color":[0.8,0.6,0.4,1.0]}
],
"tags": [
  {"id":11,"name":"Furniture","visible":true}
],
"definitions": [
  {"id":20,"name":"Block"}
],
"vertices": [
  {"id":1,"p":[0.0,0.0,0.0]},
  {"id":2,"p":[1.0,0.0,0.0]},
  {"id":3,"p":[1.0,0.5,0.0]},
  {"id":4,"p":[0.0,0.5,0.0]},
  {"id":21,"in":20,"p":[0.0,0.0,0.0]},
  {"id":22,"in":20,"p":[0.2,0.0,0.0]},
  {"id":23,"in":20,"p":[0.2,0.2,0.0]},
  {"id":24,"in":20,"p":[0.0,0.2,0.0]},
  {"id":25,"in":20,"p":[0.0,0.0,0.2]},
  {"id":26,"in":20,"p":[0.2,0.0,0.2]},
  {"id":27,"in":20,"p":[0.2,0.2,0.2]},
  {"id":28,"in":20,"p":[0.0,0.2,0.2]}
],
"edges": [
  {"id":5,"v":[1,2]},
  {"id":6,"v":[2,3]},
  {"id":7,"v":[3,4]},
  {"id":8,"v":[4,1]},
  {"id":30,"in":20,"v":[21,22]},
  {"id":31,"in":20,"v":[22,23]},
  {"id":32,"in":20,"v":[23,24]},
  {"id":33,"in":20,"v":[24,21]},
  {"id":34,"in":20,"v":[25,26]},
  {"id":35,"in":20,"v":[26,27]},
  {"id":36,"in":20,"v":[27,28]},
  {"id":37,"in":20,"v":[28,25]},
  {"id":38,"in":20,"v":[21,25]},
  {"id":39,"in":20,"v":[22,26]},
  {"id":40,"in":20,"v":[23,27],"soft":true,"smooth":true},
  {"id":41,"in":20,"v":[24,28]}
],
"faces": [
  {"id":9,"outer":[1,4,3,2],"material":10},
  {"id":50,"in":20,"outer":[21,24,23,22]},
  {"id":51,"in":20,"outer":[25,26,27,28]},
  {"id":52,"in":20,"outer":[21,22,26,25]},
  {"id":53,"in":20,"outer":[22,23,27,26]},
  {"id":54,"in":20,"outer":[23,24,28,27]},
  {"id":55,"in":20,"outer":[24,21,25,28]}
],
"instances": [
  {"id":60,"def":20,"o":[2.0,0.0,0.0],"x":[1.0,0.0,0.0],"y":[0.0,1.0,0.0],"z":[0.0,0.0,1.0]},
  {"id":61,"def":20,"o":[3.0,0.0,0.0],"x":[0.0,1.0,0.0],"y":[-1.0,0.0,0.0],"z":[0.0,0.0,1.0],"tag":11,"material":10}
],
"guides": [
  {"id":70,"line":{"origin":[0.0,-0.5,0.0],"dir":[1.0,0.0,0.0]}}
]
}
```

## Top-level keys

| Key | Required | Meaning |
| --- | --- | --- |
| `format` | yes | Always `"rysmo"`. |
| `version` | yes | Format version. A reader refuses files newer than it knows. |
| `length_unit` | no | Unit of all coordinates: `"m"` (default, what Rysmo writes) or `"mm"`. |
| `display_units` | no | Units the user sees: `"mm"` (default), `"cm"`, `"m"`, `"in"` or `"ft"`. Display only; does not scale anything. |
| `camera` | no | Where the view was: orbit `target` point, `distance`, `yaw` and `pitch` in radians, vertical field of view `fov_y` in radians, `parallel` (default false) for parallel projection, and `eye_base_z` (default 0) for the world elevation used by the Eye Height field. Camera target, distance and eye reference use `length_unit`. |
| `materials` | no | Paint colours and optional texture image/size. |
| `axes` | no | Root drawing frame: `origin`, unit `x` and unit `y` vectors; blue is their cross product. |
| `curves` | no | Curve entities with analytic parameters; edges reference them by ID. |
| `images` | no | Embedded PNG/JPEG image bytes, base64 encoded. |
| `style` | no | Face and edge display, environment, lighting and render settings. |
| `tags` | no | Tags (layers). |
| `definitions` | no | Group and component definitions. |
| `vertices`, `edges`, `faces` | no | The geometry, of the model and of the definitions. |
| `instances` | no | Placed groups and components. |
| `guides` | no | Construction guides from Tape Measure and Protractor. |
| `dimensions` | no | Linear/radial dimension entities, placement and geometry associations. |

Unknown keys are ignored, so later versions can add sections that older readers skip.

## Coordinates

Right-handed, Z up: red axis X, green axis Y, blue axis Z. Lengths are
in `length_unit` (metres in files Rysmo writes). Rysmo rounds coordinates to a
nanometre when writing, so arithmetic noise like `0.30000000000000004` does not appear.

The modelling tolerance is one micrometre: two points closer than that are the same
point.

## Ids

Every element has an `id`, a positive integer. All kinds share one sequence: no vertex,
edge, face, material, tag, definition, instance or guide has the same id as another
element, wherever it lives. Ids are stable: an element keeps its id across edits, saves
and loads, as long as it exists. An edit that splits a face keeps the old id on one of
the parts. New elements get ids above every id in the file.

When you add elements by hand, pick any unused id.

## Where an element lives: `in`

The model's own geometry has no `in` key. Geometry, instances and guides that belong to
a definition carry `"in": <definition id>`. A definition's geometry is in the
definition's own coordinates; its instances place it in their parent.

## Elements

**Vertex.** `{"id": 1, "p": [x, y, z]}`.

**Edge.** `{"id": 5, "v": [a, b]}`: a straight line between vertices `a` and `b` of the
same mesh. The two vertices differ, and no two edges join the same pair. An edge may
belong to no face (a stray line), one, two, or more faces. Optional flags, `false` when
missing:

- `soft`: not drawn (a fold inside a curved surface);
- `smooth`: the shading blends across it;
- `hidden`: neither drawn nor picked.

**Face.** A flat polygon:

```
{"id": 9, "outer": [1, 4, 3, 2], "holes": [[11, 12, 13, 14]], "material": 10, "back_material": 15}
```

- `outer` lists the outline's vertices in order. Consecutive vertices, and the last and
  the first, must be joined by an edge.
- The order sets which side is the front: the outline runs counter-clockwise seen from
  the front. The front is the side the face's normal points to, the side `material`
  paints. In the example the rectangle's outline runs clockwise seen from above, so the
  face points down, matching the default ground-plane orientation.
- `holes` (optional) lists hole outlines, each running the other way round (clockwise
  seen from the front), each step an edge as well.
- `material` and `back_material` (optional) are material ids for the front and back.
  Without them the front is the default warm white and the back the default blue-grey
  (or the material of the enclosing instance, see below).
- All vertices of a face lie in one plane (within ten micrometres).

**Material.** `{"id": 10, "name": "Oak", "color": [r, g, b, a]}`, colour components 0 to
1, sRGB; `a` is the opacity (1 opaque, 0.35 glass-like).

**Tag.** `{"id": 11, "name": "Furniture", "visible": true}`. Instances on a hidden tag
are not drawn and cannot be picked. Tags apply to instances.

**Definition.** `{"id": 20, "name": "Block", "group": true}`: a group (`group: true`) or
a component (`group` missing or false). Its geometry is the vertices, edges and faces
with `"in": 20`; it can also contain instances and guides with `"in": 20`. A group is
meant to have one instance; a component definition is shared by all its instances, so
editing it changes all of them.

**Instance.** A placed definition:

```
{"id": 61, "def": 20, "o": [3, 0, 0], "x": [0, 1, 0], "y": [-1, 0, 0], "z": [0, 0, 1], "tag": 11, "material": 10, "name": "Left"}
```

- `def`: the definition placed.
- `o`, `x`, `y`, `z`: where the definition's origin and its three axes go in the
  parent's coordinates. Identity is `o` = `[0,0,0]` with unit axes. Axes may be scaled
  (Scale on an instance) or mirrored.
- `in` (optional): the definition this instance sits in; missing means the model.
- `tag`, `material`, `name` (optional): the tag; a material for the faces inside that
  have none of their own; a name.

A definition must not contain itself, directly or deeper.

**Guide.** A guide line `{"id": 70, "line": {"origin": [x, y, z], "dir": [dx, dy, dz]}}`
(infinite, through `origin` along `dir`) or a guide point
`{"id": 71, "point": {"p": [x, y, z]}}`. Guides carry `in` like everything else.

## Checks when reading

Rysmo refuses a file, with a message naming the element, when an id is used twice, an
edge uses a missing vertex or joins a vertex to itself, two edges join the same pair, a
face step has no edge, a face is not flat or has fewer than three vertices, something
uses a material, tag or definition that is not defined, a guide line has no direction,
or a definition contains itself.

## Editing by hand or by program

Typical changes and what they need:

- **Move a point**: change its `p`. Faces through it must stay flat.
- **Add a stray line**: add two vertices (or reuse existing ones) and an edge.
- **Add a face**: add its edges first, then the face with its outline in the order that
  makes it face the way you want.
- **Paint**: add a material, set `material` on faces or on an instance.
- **Delete a face**: remove its line; its edges stay.
- **Place another copy of a component**: add an instance line with a new id and the
  same `def`.
- **Hide a tag**: set its `visible` to false.

Rysmo does not split or merge geometry when it reads a file. Lines that cross without
a shared vertex stay separate until someone edits them in the app. A program that
wants sticky behaviour should go through the editing commands instead (see
`docs/architecture.md`, "Scripting and MCP").

## Textures and styles, version 3

An image line contains `id`, `name`, `mime` and `data`. `data` is the base64-encoded
PNG or JPEG file. A textured material adds `texture: {"image": 80, "size": [1.0, 0.5]}`;
`image` refers to an image ID and `size` is its repeated width/height in metres in files
written by the application. The colour still carries opacity.

Faces may have `uv: {"offset": [0.1, 0.2], "rotation": 30.0, "scale": [2.0, 1.0]}`.
Offsets are along the canonical texture axes, in metres in written files; rotation
is in degrees; scales multiply the material's size. Missing placement means zero
offset/rotation and unit scale.

The optional `style` object stores the fields of `rysmo_model::Style`, with defaults
for omitted fields. It includes face modes, edges/profiles, edge colouring, background,
sky/ground, X-ray opacity, lighting, shadows and the sun, ambient occlusion, tone mapping
and anti-aliasing. Style enum values use snake_case. The default style is omitted.

## Curve entities, version 4

A curve entry stores `id`, optional `in`, `kind`, `centre`, `normal`, `start`, `radius`,
`segments`, signed `sweep` in degrees and `faceted` (default false). Kinds are `circle`, `polygon`, `arc` and
`polyline`. Edges may carry `curve: <id>`. Curve IDs share the document counter.
Coordinates/radii use `length_unit`; directions are dimensionless. Missing curve
sections in older files mean ungrouped edges.

Uniform transformations preserve analytic parameters. Nonuniform scaling or moving
part of a curve converts it to a polyline. The faceted flag preserves polygon edges even after deformation; circle/arc membership retains radial geometry. Polyline metadata groups
segments without offering a misleading editable radius. Radius editing is available
in Entity Info; later resampling and extrusion controls still require implementation.

## Dimension entities, version 5

Each `dimensions` entry has a globally unique `id`, optional definition `in`,
`measure`, `normal`, `offset` and `text`. Coordinates, offsets and fallback radii
use `length_unit`; directions and edge parameters are dimensionless.
`measure.kind` is `linear` or `radial`. Linear measurements store `from` and `to`
anchors and an optional projection `axis`. Radial measurements store a curve
reference and a `diameter` flag. Empty text uses the automatic formatted value;
`<>` inserts it into custom text.

Point anchors contain a retained `point` and optional `reference` with an instance
`path` relative to the owning definition. Point references identify a `vertex`,
an `edge` with its `parameter`, or a `curve_centre`. Radial references contain an
optional `curve` ID, `path`, and retained `centre`, `normal`, `start` and `radius`.
Missing/deformed referenced geometry is supported as a non-associated dimension.
The retained values are updated as live geometry changes, before deletion.
Invalid placement frames, non-finite values, duplicate IDs and missing owning
definitions are refused. This section supports persistence; the editor's
Dimensions interaction/rendering/settings are still being implemented.

## Changes

Version 5 adds dimensions and their geometry associations.

Version 4 adds curve entities and edge curve references.

Version 3 adds images, material textures, per-face UV placement and model styles.
The optional camera `parallel` flag defaults to false for older files. Rysmo writes
version 11 and accepts versions 1 through 11.


**Version 2** (Rysmo 0.2) added `tags`, `definitions`, `instances` and `guides`, the
`in` key, the edge flags `soft`, `smooth` and `hidden`, and opacity in material colours.
A version 1 file is a version 2 file without those, so Rysmo still reads both.

**Version 1** (Rysmo 0.1): materials, vertices, edges and faces of the model.

Drawing frames may also appear as `axes` on definition entries. Missing frames use
origin zero and red/green world axes. Origins use `length_unit`; directions must be
finite, unit length and orthogonal. The reader validates frames without renormalising
valid values, preserving exact save/load round trips.

Dimension entries optionally store `text_position` as `outside_start`, `centered`
or `outside_end`. Missing values use `centered` for older version-5 files. This
controls linear label placement; radial dimensions retain their leader placement.

Dimension `endpoints` optionally stores `none`, `slash`, `dot`, `closed_arrow` or
`open_arrow`. Missing values use `closed_arrow` for older files. The same setting
applies to both ends of a linear/diameter dimension or the single radius tip.

Dimension `font` stores `size` (1–512 logical screen pixels), `monospace` and
three-byte RGB `color`. Missing font fields use the default 14 px proportional
font and `[30,30,30]` colour. Font size remains independent of model scaling.
Invalid size values are refused when loading or applying commands.

Dimension `alignment` stores `screen` (default) or `dimension_line`. The latter
rotates the label along the projected measurement line while keeping it upright.
Measured text size remains unrotated in the font cache; drawing and picking derive
the oriented rectangle from the current camera, placement and alignment.

Model `style.dimensions` stores expert dimension visibility: `hide_foreshortened`,
`angle_degrees` (0–90), `hide_small`, `minimum_pixels` (0–4096),
`highlight_non_associated` and RGB `highlight_color`. Missing fields default to
disabled toggles, 20 degrees, 40 px and `[220,40,40]`. Thresholds are validated
on loading and style commands. Hidden dimensions are excluded from drawing and
point/box picking; selection highlighting takes precedence over warning colour.

`style.dimension_defaults` stores default `font`, `alignment` and `endpoints`
for newly created dimensions. Older files use existing default values. Changing
these settings does not restyle stored entities. ApplyDimensionDefaults updates
explicit dimension IDs in the current editing definition as one validated undo
step, preserving measurement, offset, text and text position.

Radial references optionally retain `arc_sweep` in signed degrees. A finite sweep
with magnitude greater than 1e-9 and no greater than 360 degrees (with round-off
tolerance) preserves the last arc sector when its geometry reference is detached
or erased. Missing fields preserve older-file compatibility. Length-unit
conversion scales the radius and placement, leaving this angular value unchanged.

Radial `radial_axes` optionally stores two owner-local length vectors: the affine
image of the circular frame. It preserves the last mapped ellipse/arc shape when
nonuniform scale makes a radius association invalid. These vectors scale with
length units; the radius/start/normal must agree with their first axis and oriented
plane. Zero, collinear, non-finite or inconsistent frames are rejected. Older
files omit the field and retain circular fallback behaviour.

## Text annotations, version 6

The optional `texts` array contains one entry per text annotation. Entries share
the document ID counter and use optional `in` for their owning definition.
`content` stores Unicode and newlines, `font` stores logical pixel size, monospace
choice and RGB colour, and `endpoint` is `none`, `dot`, `closed_arrow` or
`open_arrow`. `placement.kind` is `screen` or `attached`.

A screen placement stores `position` as a viewport fraction, independent of
camera/model transforms. Attached placement stores `anchor.point` in owner-local
metres and an optional point or barycentric face reference relative to its owner.
Face references store a path, face ID, three vertex IDs and three finite weights
summing to one. Missing/deleted geometry uses the last resolved fallback point.
The `leader.kind` is `none`, `view_based` (logical-pixel `offset`) or `pushpin`
(owner-local metre `offset`). File length-unit conversion scales fallback points
and pushpin offsets, leaving screen positions, view-based offsets and font sizes
unchanged. Fonts and placement are validated on load. Versions 1–5 may omit texts.

Text IDs are selectable with `{"kind":"text","id":…}` in element lists.
AddText returns the new element; Erase accepts text elements and Undo restores
them. This format support does not imply that every interactive Text workflow
has been implemented yet.

`style.text_defaults` stores `screen_font`, `leader_font`, `endpoint` and `leader`
(`none`, `view_based`, `pushpin`). Older styles may omit it. Both default font
sizes must be finite and between 1 and 512 logical pixels.

## Displaced text without leaders, version 7

A `none` text leader may carry an owner-local metre `offset`. Older entries
without it default to zero. The attachment remains unchanged while the label
uses anchor plus offset. Unit conversion, definition remapping and bounds handle
the displacement as model geometry. Version 7 protects this placement from being
resaved by an older writer that does not support displaced no-leader labels.

## Section planes, version 8

`section_planes` contains one record per plane, with a globally unique numeric
`id`, optional definition owner `in`, `name`, `origin`, a unit `normal`, and an
optional `active` flag. At most one plane may be active in each definition.
Coordinates use the owner's local model frame. The positive normal side is cut
away. Planes in ancestor contexts also affect nested occurrences.

```json
{"id": 420, "name": "Cross section", "origin": [2, 0, 0], "normal": [1, 0, 0], "active": true}
```

Styles store independent plane/cut/cut-edge visibility, active and inactive plane
colours, cut-edge colour and pixel width. Older files default to visible cuts
and planes and contain no plane entities. Loading rejects invalid normals,
duplicate IDs, unknown owners and multiple active planes in one context.

## Scenes, version 9

The ordered `scenes` array stores a stable document ID, name, description,
`include_in_animation`, seven `properties` switches, a camera/style snapshot,
tag visibility and per-definition hidden-edge IDs, drawing axes and active
section IDs. Restoring a scene changes only enabled properties. Shadow settings
are independent of drawing style. Deleted definitions, tags and section planes
are skipped on recall; they do not invalidate a saved view. Scene IDs share the
normal global ID namespace. Loading validates cameras, axes and display settings
and converts camera distances/targets and axes origins from file units.
Scene add/edit/delete/reorder/restore are ordinary undoable model commands.

## Component insertion and instance visibility, version 10

Definition records store `component.description` and `component.insertion_axes`
with an origin and red/green directions. This frame determines future insertion;
changing it preserves existing geometry and placements. Instance records add
optional `hidden: true`. Hidden instances are skipped by rendering/picking and
visible hierarchy bounds, while Outliner lists them for recovery. Scene context
snapshots add `hidden_instances` alongside hidden edges. Old files default to
visible instances and the standard insertion frame. Insertion origins are scaled
from file units and frames are validated. Unused component definitions remain in
the document until explicitly deleted; unused group definitions are purged.

Style renderer settings include `shadow_quality` (`low`, `medium`, `high`),
`sun_intensity` and `ambient_intensity`. Missing fields default to High and 1.0
for both multipliers, preserving older projects. Multipliers must be finite and
in 0..4. Scene style records store and validate the same fields. These additive
style fields do not require a version bump.

Version 11 adds an optional `material` ID to each edge record. Missing edge
materials mean Default and display black in By Material mode. Edge references
must resolve to a document material. Other face/instance material assignments
remain independent. Older files load with no explicit edge material.

Version 12 adds optional texture placement `pins`, four corners of a tile in the
unwarped UV coordinate plane. Their order is (0,0), (1,0), (1,1), (0,1). A projective
mapping sends these displaced corners back to the image corners. Pins must form a
finite, convex, noncollapsed quadrilateral. Missing pins preserve the older numeric
positioning behavior. Optional `projected_normal` sets a shared projection direction
for faces painted from a sampled texture. Zero and nonfinite directions are rejected.

### Version 13 length display settings

`length_precision` is null for automatic per-unit precision, or an integer from 0
through 6 for decimal places. Display rounding never changes stored metre coordinates.
Versions 1–12 without this field retain automatic precision. Decimal centimetres,
inches and feet extend `display_units`. Other unit formats and snapping settings
are not stored by this increment.


### Version 14 measurement, visibility and glued components

`measurement` stores `show_unit_markers`, `length_snap` in metres or null,
`angle_precision` from 0 through 6, and `angle_snap` in degrees or null. Defaults
show unit markers, disable length snapping, show one decimal angle place and snap
to 15 degrees. Valid length increments range from 1e-9 through 1e12 metres;
angle increments range from 0.1 through 180 degrees. Invalid settings are rejected.

Faces have optional `hidden`, default false. Scene contexts add `hidden_faces`;
those IDs remain reserved after deletion. Style adds `show_hidden_geometry`.

Component records add `glue` as none/any/horizontal/vertical/sloped and
`cut_opening` as a boolean. Instance `glued_to` references a host face in the same
owner definition. Both fields default to disabled. Cutters use a closed outline
in the insertion plane and affect a single face. Host links and contours are
validated on loading. Openings are evaluated from component geometry for display,
picking, navigation and export. Their generated boundary IDs are not serialized.
Explode materializes an opening with reserved ordinary editable geometry IDs.

### Version 15 construction guides and fractional units

Point guides optionally store an `origin`, the start of their finite measured
segment. Both guide kinds store `hidden`, defaulting to false for legacy files.
Scene context visibility includes `hidden_guides`; `style.show_guides` controls
global display and inference. Guide transforms preserve both measured endpoints.
`display_units` additionally accepts `architectural` and `fractional`, with numeric
entry in inches unless a suffix is supplied. Their precision is the power-of-two
fraction denominator, 0 through 6 representing one inch through 1/64 inch.
`measurement.force_zero_inches` controls architectural whole-foot output. Legacy
measurement settings default it to false. Engineering uses existing decimal `ft`.
Area display is independently configured by `measurement.area_units` and
`measurement.area_precision`. A null unit uses m² in metric projects and ft² in
imperial projects; explicit area units must be decimal. Precision is 0 through 6,
with a legacy default of three places. Properties and newly created face-area text
share this formatter. Existing user-authored text remains unchanged.

### Version 16 volume display settings

`measurement.volume_units` independently selects decimal `mm`, `cm`, `m`, `in` or
`ft` display units. `null` selects automatic cubic metres or cubic feet according
to the length-unit family. `measurement.volume_precision` selects 0–6 decimal
places. Legacy files default to automatic units and three decimals. Fractional
volume units and out-of-range precision are rejected. These settings affect
presentation only and share the existing measurement-settings Undo command.


## Renderer quality, version 17

`style.lighting` additionally accepts `studio`. This enum extension requires version
17 so older readers reject a mode they cannot render. Older files still load using
Style defaults for omitted fields:

- `studio_environment`: `neutral` or `warm`; `studio_intensity`: 0..4, default 1;
  `studio_rotation`: -180..180 degrees, default 0.
- `exposure`: -4..4 stops, default 0; positive values brighten lit views.
- `texture_limit`: 0 (original within platform limits), 512, 1024, 2048, 4096 or
  8192 pixels, default 4096. `texture_anisotropy`: 1, 2, 4, 8 or 16, default 4.
- `render_scale`: 0.5..1.5, default 1. This scales only the 3D image.

Original image bytes stay unchanged. Scene style snapshots carry and validate the
same settings. Lighting-only scene restoration includes studio settings/exposure,
while texture and render resolution remain governed by full style restoration.
