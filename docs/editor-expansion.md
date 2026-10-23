# Additional editor workflows

All GUI labels are English. File version 18 adds editable component parameters,
material surface properties and photo calibration; versions 1–17 still load.
Older builds reject version 18 to prevent silently losing these properties.

## Model and underlay import

File → Import accepts OBJ, embedded GLB 2.0, ASCII DXF and native models. Preview
controls specify source units, OBJ up axis and DXF curve detail. Placement creates
one component through the normal paste preview. Escape cancels; one Undo removes
its placement. Import does not change the current project's Save destination.

OBJ retains polygon geometry, linework and UVs. External MTL/image companions and
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
copy. Regeneration replaces generated faces and face paint, while instance paint
and transforms remain. Components with manual geometry changes or added annotations
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
