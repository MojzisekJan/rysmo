# Editable workflows

## Live operations

Edit > Groups & Components > Live Operations keeps inputs and parameters inside
an ordinary component. Choose one component plus a connected wire path for a
path array, one profile face plus separate connected path edges for a profile
sweep, or two solid occurrences for a solid operation. Array distances use local
component metres; scaling the live component also scales the array. Solid operands
have a stable stored order; Swap operands reverses it.

Create live component replaces the selected inputs in one Undo transaction.
Update parameters regenerates from the stored inputs. Edit source inputs opens
the editable input geometry; Finish inputs and regenerate saves those changes.
When editing nested source components, return to the live component context before
finishing its inputs. Cancel input changes restores the previous inputs and output. Convert visible
geometry to ordinary component removes the recipe and retains whatever is visible.
During input editing this retains the inputs. Otherwise it retains the output.

Regeneration replaces direct output edits. Keep lasting changes in the source
inputs. Shared copies regenerate together; Make Unique isolates one occurrence.
Invalid regeneration leaves the current model intact. Recipes persist in native
files and cross-document copy/paste. Input budgets include 200,000 geometry
elements, 4 procedural nesting levels, 4,096 definitions, 20,000 expanded
occurrences and 8 MiB of referenced image data per input document.

## Connection points and attributes

Edit > Groups & Components > Component Data & Connections edits named connection
frames in component coordinates. Each point has an origin and local X/Y axes;
Orthonormalize repairs a proposed axis pair before applying it. Coordinates use
local metres. Definition data affects every shared copy.

Select two occurrences and choose source and target point names. Attach makes
the origins coincide and the normals face each other while preserving the source's
world dimensions, including under scaled or mirrored parent contexts. This is a
one-time placement with Undo. It creates no persistent motion constraint.

Custom attributes support text, finite numbers and booleans, with optional unit
labels. Definition defaults and occurrence overrides are edited separately but
Apply data stores them in one transaction. Limits are 128 fields per map, 64
connection points, 128-byte names, 4,096-byte text values and 32-byte unit labels.

File > Model report adds sorted custom-field and unit columns. Group by attribute
creates summary rows. Numeric fields sum only when their units agree. Mixed types,
mixed units and overflow produce an unavailable-total marker. Summary rows are
excluded from the grand total. CSV text fields are escaped and formula-prefixed;
validated numeric values remain numeric, including negative values.

## Architectural dimensions

The Parametric components window can fit a window or door to a selected rectangular
planar face. Select one occurrence and one face, then Fit selected component.
Fitting makes the occurrence unique and changes its dimensions and placement in
one Undo step. It uses the face as a target; it does not cut a wall opening.

With the window open, select a parametric occurrence to expose dimension grips.
Drag a grip to preview its dimensions and release to regenerate once. Escape
cancels. Shared copies update together; Make Unique first to edit one copy.
Numeric dimension fields remain available for exact values. Edit selected component
loads the current selected dimensions into the parameter draft. Regeneration retains
front/back paint, UVs and hidden state on matching generated parts. New parts use
default paint unless the previous component had uniform paint. Ambiguous changes
to individually painted stair steps are rejected. Manual geometry changes remain
protected from replacement.

## Section hatching

Section Planes > Section hatching selects a model default, component override or
material setting. Set angle, world spacing and optional crosshatching, then Apply.
Component overrides take priority over inherited whole-occurrence material paint,
then the model default. Face-only paint does not assign a section hatch.

Hatching uses active section planes and even/odd closed contours, preserving holes.
Viewport, SVG and PDF use the same world-space segments. Open or ambiguous contours
report an error. Generation is limited to 4,096 scanlines per direction, 20,000
hatch segments and a bounded intersection budget; increase spacing if needed.

## Document tabs

New and Open retain the current model in another tab. Each document has independent
Undo/Redo, camera, selection, editing context and save destination. The clipboard,
component library and application preferences are shared. Plus creates a document;
the close button closes the active document and checks unsaved changes. Closing
the application checks every dirty document. Up to 16 documents can be open.

In the browser, each tab retains its own file handle. A delayed Save As remains
associated with its initiating document. Autosave stores a workspace with all
models, dirty flags, cameras and the active tab; switching/closing tabs also writes
a snapshot when autosave is enabled. Recovery restores models and cameras, not Undo
history. Browser storage can be denied or cleared; Save still produces model files.

## Drawing sheets

File > Drawing sheets stores named paper layouts with several viewports. Set paper
size, viewport position and size in millimetres, projection, scale 1:N and optional
scene visibility/sections. Add title-block text and paper notes, then Apply layouts
to save them in the model with Undo. The page preview displays viewport rectangles.

Export selected SVG writes one sheet. Export all sheets PDF writes a multi-page
vector document with embedded Unicode text. Geometry is regenerated from the current
model at export. A view that does not fit at the requested scale reports an error.
Print at 100% with printer fit-to-page disabled to retain physical scale.

Limits are 32 sheets, 128 total viewports, 16 viewports and 64 notes per sheet,
200,000 vector lines per sheet and 500,000 per PDF set. Notes use paper coordinates
and do not alter the model. Deleting a referenced scene requires choosing another
scene or Current model before exporting that viewport.
