//! Offline English guide for the tools and workflows actually available in this editor.
use crate::{actions::Action, icons::Icon, state::App3d};
use bevy_egui::egui;
use rysmo_tools::ToolKind;

#[derive(Default)]
pub struct HelpState {
    pub open: bool,
    query: String,
    selected: usize,
}
struct Topic {
    title: &'static str,
    category: &'static str,
    icon: Option<Icon>,
    body: &'static str,
}
static TOPICS: &[Topic] = &[
    Topic {
        title: "Import and component collections",
        category: "Files",
        icon: None,
        body: "File > Import previews OBJ, embedded GLB and ASCII DXF before placement. Set source units and OBJ up axis, then Place in model. Click to place; Escape cancels and one Undo removes placement. Unsupported records are reported. OBJ companions are not loaded. DXF closed polylines remain underlay linework. File > Component library captures selection with a name and description, shows wire previews, searches and places reusable assets. Save or merge .ryslib collections to transfer components between projects. The library stays in the session across New/Open; save it before exiting.",
    },
    Topic {
        title: "Parametric components",
        category: "Modelling",
        icon: None,
        body: "Model > Parametric components creates windows, doors and stairs in local metres. Windows have frame and column controls, doors have an opening angle, and stairs have rise, tread and step count. Panes are ordinary geometry: assign materials as needed. Automatic wall cutting is not applied. Select a component, choose Edit selected component and Update to regenerate shared instances in one Undo. Make Unique isolates a copy. Regeneration replaces face paint; instance paint and transforms remain. Manual geometry changes and added annotations are protected by refusing regeneration.",
    },
    Topic {
        title: "Scaled vector drawings",
        category: "Files",
        icon: None,
        body: "File > Export drawing exports top/front/right/isometric SVG and PDF with A4/A3 page controls, margins and scale. Active visibility and sections are respected. Include dimensions and attached text as needed. Screen-fixed text stays outside the drawing. Print at 100%, without fit-to-page, to retain physical scale. If geometry does not fit, choose a larger page or scale denominator. Hidden-line removal has a work budget; disable it for larger wireframe drawings.",
    },
    Topic {
        title: "Photo matching",
        category: "Navigation",
        icon: None,
        body: "Camera > Photo matching loads a PNG/JPEG photograph. Align two red lines with parallel X edges and two green lines with perpendicular Y edges. Orange sets the model origin; blue sets the other endpoint of a known ground-plane distance. Set that length in metres and use Reverse X/Y to resolve direction; Apply rejects directions that would put the camera below the ground. Apply calibrates the camera and displays geometry over the photo. Orbit, pan or zoom detaches the background; Apply restores it. Original photo and guides are saved. Calibration assumes image-centred principal point, square pixels and negligible distortion. Cropped or nearly orthographic photos can fail.",
    },
    Topic {
        title: "Material surface response",
        category: "Materials",
        icon: None,
        body: "The Materials inspector exposes Roughness, Metallic and normal maps. Sun or Studio lighting displays these effects; Flat is unlit. Load a PNG/JPEG normal map, adjust strength and enable Flip normal Y for DirectX maps. Normal maps share face UV coordinates and colour texture size; without a colour image, one UV unit is one metre. Native files, clipboard and portable collections retain these settings. GLB exports the scalar factors and embedded normal image; other formats retain their existing material capabilities.",
    },
    Topic {
        title: r###"Getting started"###,
        category: r###"Start here"###,
        icon: None,
        body: r###"Edit contains clipboard, grouping, visibility and selection commands. Model contains Flip, Faces & Edges, Terrain and Construction operations.
Choose a drawing tool from the left toolbar. Navigation, construction, annotations and terrain tools are above the canvas. Hover a button to see its name and shortcut.
Draw a closed planar outline, then use Push/Pull to give it depth. Group separate objects before drawing nearby geometry so their edges do not merge.
The bottom status line explains the current step. Type exact values into Measurements without clicking the box, then press Enter. Escape cancels the current operation; Space returns to Select.
Open this guide from Help > Editor Help. F1 or Help for Current Tool opens the active tool topic. Search covers both topic titles and their contents."###,
    },
    Topic {
        title: r###"Command search"###,
        category: r###"Start here"###,
        icon: None,
        body: r###"Open Help > Search Commands, or press Shift+S while Measurements is empty. Type part of a tool, action, window or shortcut name. Search also recognises terms such as renderer, units, guides and glTF.
Use Up/Down to choose a result and Enter to run it, or click a result. Unavailable actions stay disabled until the selection or history supports them.
Escape closes search and preserves the unfinished tool and Measurements text. Choosing a command uses the same operation and file-replacement safeguards as the menus. Ctrl+Shift+S still opens Save As."###,
    },
    Topic {
        title: r###"Measurements and inference"###,
        category: r###"Start here"###,
        icon: None,
        body: r###"Model geometry is stored in metres. File > Settings > Model units chooses displayed units and length precision. Display rounding does not round geometry. Fractional and architectural output use reduced inch fractions.
Numbers without suffixes use project units. Explicit suffixes override them: 125cm, 24in, 2ft or 5' 3 1/2". A leading ~ marks an approximate display value.
Rectangles accept width,depth, or width;depth with decimal commas. Copy arrays use 5x or /5. Radial curves accept segment counts such as 24s. The status line and Measurements label identify the value expected at each step.
Inference markers locate endpoints, midpoints, faces, axes, intersections and guides. Shift holds an inference direction. Arrow locks follow the tool and current drawing axes. Exact typed input overrides a visual snap."###,
    },
    Topic {
        title: r###"Keyboard shortcuts"###,
        category: r###"Start here"###,
        icon: None,
        body: r###"Space Select; E Eraser; L Line; A Arc; R Rectangle; C Circle; P Push/Pull; F Offset; M Move; Q Rotate; S Scale; T Tape Measure; B Paint Bucket; O Orbit; H Pan; Z Zoom.
G creates a component. Shift+Z frames the model. Ctrl+Shift+W starts Zoom Window. F1 opens help for the current tool.
Ctrl+C Copy; Ctrl+X Cut; Ctrl+V Paste; Ctrl+Shift+V Paste in Place; Ctrl+D Duplicate. Delete erases the selection. Ctrl+Z Undo; Ctrl+Y or Ctrl+Shift+Z Redo.
Ctrl+S saves, Ctrl+O opens, Ctrl+N starts a new model. While editing text, typing and text shortcuts belong to the text field rather than the modelling tools. Browser or operating-system shortcuts may be reserved outside the canvas."###,
    },
    Topic {
        title: r###"Groups and components"###,
        category: r###"Model organisation"###,
        icon: None,
        body: r###"Select entities and use Make Group or Make Component from Edit > Groups & Components or the context menu's Groups & Components submenu. Double-click an instance to edit it. Escape or Close Group leaves its context. Other contexts are faded.
Components share a definition: editing one changes all its instances. Make Unique separates the selected instance. Copied groups become independent when edited. Explode returns an instance's contents to its parent context.
Window > Components lists in-model definitions. Filter by name or description, inspect a thumbnail, place an instance and adjust its origin, rotation or scale. Insertion axes are independent of drawing axes.
Unused component definitions remain available until explicitly deleted. External component collections are not available yet. In Window > Components, edit a definition and choose Glue to to align new instances to a face. Horizontal/Vertical/Sloped use the editing context's blue axis.
Cut opening uses a closed outline in the insertion plane to cut one host face. The outline must fit wholly inside the face and cannot overlap another opening. Moving or deleting the component updates the opening; Unglue detaches it and restores the host. The host face must be in the current editing context. Explode converts an opening to editable boundary geometry."###,
    },
    Topic {
        title: r###"Copy, paste and history"###,
        category: r###"Model organisation"###,
        icon: None,
        body: r###"Copy captures selected objects without changing the model. Cut removes them in one Undo step. Paste shows a placement preview; click or press Enter to commit, or Escape to cancel.
Paste in Place uses original world coordinates, even in another editing context. Duplicate starts a separate copy preview without replacing clipboard contents. Materials, textures, curves and holes travel with copied geometry.
Model > Flip opens a mirror preview. Choose Red, Green or Blue (or use the arrow keys), click or drag a coloured plane, drag its centre handle or adjust Plane offset in metres, and confirm Flip Selection or enable Create mirrored copy. The plane starts at the selection centre. The selection-axes option uses a single instance’s orientation; turn it off for the current drawing axes. Escape or Cancel leaves the model unchanged. End-on views require numeric offset input. Pick model face previews a magenta plane on visible geometry, including transformed groups/components. Click to lock that plane, or drag to offset it. Escape while picking returns to the previous plane; arrows return to axis planes. The final Flip confirmation commits one Undo step.
Components retain shared definitions when pasted within the same model; group snapshots are independent. The object clipboard survives New/Open in this editor session. It does not transfer objects between separate tabs or applications.
Undo and Redo track committed model changes. Camera > Previous view has a separate view history. Drafts normally do not enter model history until completed."###,
    },
    Topic {
        title: "Solid Tools",
        category: "Modelling",
        icon: None,
        body: "Select exactly two groups or components and open Model > Faces & Edges > Solid Tools. Union combines both volumes. Subtract removes B from A; use Swap A and B to reverse the cut. Intersect keeps only their shared volume. Trim removes B from A and always retains cutter B. Split creates A minus B, B minus A and the shared part as separate groups, omitting empty pieces. Outer Shell combines the solids and removes enclosed internal shells; through-holes remain.
Results are new groups in the current editing context. Keep original objects retains both operands. Without that option, operations replace the operands in one Undo step; Trim always retains B. Other instances of shared component definitions stay unchanged. An empty Subtract or Intersect result may remove both operands.
Operands may contain multiple closed shells, cavities and nested solid islands. Their orientation must match the nesting. Nested solid groups/components are supported, including inherited paint and transforms; overlaps within a hierarchy are unioned. Guides, text, dimensions and section planes inside operands are not supported. Unhide operands and their faces first. Stray/open edges, non-manifold contacts and self-intersections are rejected. Complexity limits protect interactive performance: at most 4096 input triangles and 16384 vertices per operand, 4096 occurrences and 32 hierarchy levels, plus bounded clipping, stitching and validation work.
Rotated, mirrored and nonuniformly scaled instances are supported. Face materials and explicit texture placement are carried into cut fragments. Analytic curves become polygonal boundaries; coplanar subdivision seams are softened. Results may contain disjoint shells or enclosed cavities. Those results can be used in subsequent operations, and Properties reports their combined volume with cavities subtracted. A Split operation commits all pieces in one Undo step. Trim retains B even when its result is empty. Invalid or over-complex results leave the model unchanged.",
    },
    Topic {
        title: r###"Inspector and Outliner"###,
        category: r###"Model organisation"###,
        icon: None,
        body: r###"The floating Inspector has Model and Properties tabs. Drag its title to move it and its border to resize it. Window > Inspector reopens a closed panel.
Model lists nested groups/components. Search, expand/collapse, sort by name, rename or show/hide instances. Opening an Outliner item enters its editing context.
Properties shows controls for the selection: lengths, face areas, instance bounds, materials, edge flags, curve parameters, text, dimensions and texture placement. Only applicable controls appear.
If a tool cannot see an object, check the active editing context, instance visibility and its tag."###,
    },
    Topic {
        title: r###"Tags and visibility"###,
        category: r###"Model organisation"###,
        icon: None,
        body: r###"Window > Tags creates and removes tags, switches visibility and assigns a tag to selected groups/components. Tags apply to instances, rather than raw faces or edges.
Use the Model inspector or Edit/context menu to hide or show instances. Eraser with Shift hides edges; Unhide All Edges restores them in the current context.
Soft edges join facets into a surface. Smooth controls shading. Properties exposes separate flags and an angle-based softening control. Edit > Visibility > Hide Selected Geometry hides faces, edges and instances. Unhide Selected Geometry restores just the selection; Unhide All Geometry restores everything in the current context. Window > Hidden Geometry reveals hidden faces translucently and hidden/soft edges as dashed lines so you can pick individual facets.
A hidden tag can prevent an otherwise visible instance from appearing. Show the tag explicitly when needed."###,
    },
    Topic {
        title: r###"Materials and texture position"###,
        category: r###"Materials"###,
        icon: None,
        body: r###"Window > Materials offers in-model materials, a bundled library, colour/opacity and PNG/JPEG texture loading. Texture width and height use model units. Properties assigns front, back and independent edge materials.
Select a textured face and choose Texture Position in Properties or the context menu. Drag its face-plane pins or the texture itself. Fixed pins move, rotate/scale, shear and distort; free pins deform individual corners. Click a pin to relocate its anchor. Shift keeps scale while rotating.
Done or Enter finishes the session as one Undo step. Cancel or Escape restores its starting placement. Reset clears custom positioning. Camera navigation remains available.
Alt+Paint Bucket transfers texture placement and projection. Painting connected soft faces wraps coordinates across their shared edges. Closed surfaces can have one seam. By Material edge display uses edge assignments independently from face materials."###,
    },
    Topic {
        title: r###"Views, scenes and sections"###,
        category: r###"Camera"###,
        icon: None,
        body: r###"Camera offers standard views, perspective, parallel projection, field of view and Previous view. Shift+Z frames the visible model. Wheel bursts and navigation sessions are grouped in view history.
Window > Scenes saves views and selected model properties. Choose which camera, visibility, tags, sections, style, shadows and axes properties to capture or update. Scene tabs recall stored states. Scene animation is not included.
Section Plane places a cutting plane on geometry. Window > Section Planes controls its name, transform, direction and active cut. Select, Move, Rotate or copy the visible plane symbol; double-click activates its cut.
Cuts are non-destructive. Appearance settings control plane symbols, section edges, fill colours and line width."###,
    },
    Topic {
        title: r###"Settings and rendering"###,
        category: r###"Settings and files"###,
        icon: None,
        body: r###"File > Settings groups Application, Controls, Workspace, Appearance, Model units and Annotations. Application, Controls and Workspace are saved on this device, outside model Undo. Appearance, units and annotation defaults are saved with the project and participate in Undo/Redo.
Appearance groups rendering into Lighting, Image quality and Performance. Lighting offers Flat, Sun and Studio. Studio provides Neutral/Warm procedural environments with intensity and rotation; exposure compensation brightens or darkens Sun/Studio views. Studio has no sun shadows. Image quality includes anti-aliasing, tone mapping, ambient occlusion and trilinear or 2x/4x/8x/16x anisotropic texture filtering. Performance offers 50-150% 3D resolution and texture display limits. GUI text and picking remain at native resolution. Mipmaps improve distant textures; original image bytes are retained. Display images and 3D targets are capped at 4096 px on web and 8192 px on native. Presets reset resolution and texture quality as well as lighting. Appearance also controls face/edge modes, profiles, sky, ground and section display. Lower quality and shadow resolution can help large models. Browser rendering exposes only supported effects.
Editor chooses units, length precision, touch layout and Inspector visibility. Units include millimetres, centimetres, metres, decimal inches, engineering feet, architectural feet/inches and fractional inches. Fractional precision ranges from one inch to 1/64 inch. Architectural output can force zero inches. Decimal unit markers can be disabled. Area units and decimal precision are independent of length units; Automatic uses m² for metric projects and ft² for imperial projects. Volume units and decimal precision are also independent; Automatic uses m³ or ft³. Properties reports the volume of a single closed, consistently oriented group/component shell, including enclosing scale and mirrors. Hover Volume unavailable for the reason when topology cannot be measured. Nested groups remain unavailable in the volume readout; compound shells are validated within a work budget. Length and angle snapping are optional, with adjustable increments and angle precision. Typed values and exact geometry inferences take priority over length snapping.
Annotations controls text/dimension defaults, visibility and updates to selected entities. Changing defaults does not necessarily restyle existing labels until you apply an update."###,
    },
    Topic {
        title: r###"Files and browser use"###,
        category: r###"Settings and files"###,
        icon: None,
        body: r###"File > New starts a model. Open and Save use the native .rysmo document, which preserves editable geometry, groups, materials, annotations and project settings. New/Open and closing a modified model offer Save, Discard and Cancel. Cancelling or failing a save keeps the current document open.
In a browser, opening and importing use file pickers. Save reuses a granted file handle where supported; otherwise it downloads a file. Keep downloaded models somewhere you can find them again. Recent files and browser auto-save help restore a session. Set Enable auto-save and the interval in minutes under File > Settings > Application in the browser version. Auto-save keeps a browser recovery copy; use Save for a model file.
File > Export provides OBJ, glTF/GLB, COLLADA/DAE and DXF. COLLADA includes colours, UVs and companion texture images; textured browser exports download one ZIP to extract together. DXF exports Z-up 3D faces and loose lines in metres, with face colours and a layer per occurrence. It does not export textures, annotations or editable CAD solids. Native .rysmo remains the format for continuing work with editor-specific data.
PNG/JPEG texture import is available on desktop and web. Very detailed curves and large textures increase memory use and download/rendering costs."###,
    },
    Topic {
        title: r###"Touch and troubleshooting"###,
        category: r###"Start here"###,
        icon: None,
        body: r###"File > Settings > Controls enables the touch layout. A one-finger drag orbits with Select or Orbit and drives many other tools; use on-screen modifiers when needed. Two fingers pan and pinch zooms.
If geometry disappears, use Zoom Extents, check tags and hidden instances, and leave any unintended editing context. Check whether a section cut is hiding the area.
If a numeric value fails, check the Measurements label, units, separator and current stage. Escape cancels an unfinished operation. Undo restores committed changes.
This guide describes the implemented editor. Use native saves regularly while working."###,
    },
    Topic {
        title: r###"Select"###,
        category: r###"Selection"###,
        icon: Some(Icon::Tool(ToolKind::Select)),
        body: r###"Click an entity to select it. Drag left to right to select entities fully inside the box; drag right to left to include anything the box crosses.
Ctrl adds, Shift toggles, and Ctrl+Shift removes. Double-click a face to include its boundary; triple-click selects connected geometry. Double-click a group or component to edit it.
Press Space to return to Select. Selection is limited to the current editing context. Use the Model inspector to find nested or hidden instances."###,
    },
    Topic {
        title: r###"Eraser"###,
        category: r###"Selection"###,
        icon: Some(Icon::Tool(ToolKind::Eraser)),
        body: r###"Click or drag over edges, guides or instances to erase them. Removing an edge between coplanar faces can merge the faces.
Hold Ctrl to soften and smooth edges, Shift to hide them, or Ctrl+Shift to remove softening. Hidden edges can be restored from the Edit or context menu.
Use Delete to erase the current selection, and Undo to restore an accidental deletion."###,
    },
    Topic {
        title: r###"Line"###,
        category: r###"Drawing"###,
        icon: Some(Icon::Tool(ToolKind::Line)),
        body: r###"Click the start and end, or press, drag and release. Continue clicking to draw connected edges. Closing a planar loop creates a face.
Type a length and press Enter while choosing the endpoint. Inference markers help you find endpoints, midpoints, intersections and drawing axes. Shift holds the current inference direction; arrow keys lock axes.
Escape cancels the current segment. Use separate groups to keep new edges from merging with other geometry."###,
    },
    Topic {
        title: r###"Freehand"###,
        category: r###"Drawing"###,
        icon: Some(Icon::Tool(ToolKind::Freehand)),
        body: r###"Press and drag to draw a planar curve, then release to finish. Return near the starting point to close a loop and create a face.
The curve belongs to one drawing plane. Escape cancels the stroke; Undo removes the completed stroke in one step.
Freehand is intended for planar outlines. Decorative freehand strokes through three-dimensional space are not supported."###,
    },
    Topic {
        title: "Fillet",
        category: "Drawing",
        icon: Some(Icon::Action(Action::Fillet)),
        body: "Select exactly two adjacent straight edges, then choose Model > Faces & Edges > Fillet or search for Fillet with Shift+S. Set a positive radius, including units such as 100mm or 1in, and 2-512 segments. A cyan preview shows the replacement arc and shortened sides. Apply trims the corner and creates an analytic arc in one Undo step. Escape or Cancel leaves the model unchanged.
Wire corners and convex outlines of one face without holes are supported. Both edges must share an unbranched corner. Curved, hidden, solid and intersecting geometry is rejected. A radius that reaches either adjacent endpoint is too large. Face facing, front/back materials and texture placement are retained. Fillet does not round a solid's 3D edges.",
    },
    Topic {
        title: r###"Arc"###,
        category: r###"Drawing"###,
        icon: Some(Icon::Tool(ToolKind::Arc)),
        body: r###"Click the start, the end of the chord, and then a point that sets the bulge. The preview indicates a half circle when it snaps.
Measurements accepts chord length, bulge, a radius followed by r such as 500mmr, or segment count such as 12s, depending on the stage.
Select an intact arc to inspect its radius and segments in Properties. Segment changes can be rejected when attachments or intersections would make the replacement ambiguous.
At an unconnected endpoint of a straight edge or analytic arc, move the bulge until the preview turns cyan and reads Tangent. Tangency can be acquired at either end of the new arc. Hold Shift after acquisition to retain that tangent while moving the pointer; release Shift to resume free bulge movement. For exact-radius corner trimming, use Model > Faces & Edges > Fillet."###,
    },
    Topic {
        title: r###"Three Point Arc"###,
        category: r###"Drawing"###,
        icon: Some(Icon::Tool(ToolKind::ThreePointArc)),
        body: r###"Click the start, a point that the arc must pass through, and the endpoint. Before the first click, arrow keys choose the drawing plane. Later, arrows and Shift hold a direction from the last point. Collinear or repeated points cannot define an arc.
Enter a distance to place the second point precisely along the pointer direction. At the final step, first show a valid circle with the pointer, then enter signed total degrees to extend or shorten that circle while retaining the through point. The requested sweep must still pass through the second point.
Enter 24s to set segments. After creation, signed degrees, 500mmr, 24s or 20c correct the same Undo step. The through point is a real curve vertex; segments are distributed on both sides of it. Later Properties resampling may redistribute free vertices and rejects attached vertices it cannot preserve. Radius remains editable as for other arcs.
Escape and changing tools cancel drafts. Model or context changes block stale creation and corrections."###,
    },
    Topic {
        title: r###"Center Arc"###,
        category: r###"Drawing"###,
        icon: Some(Icon::Tool(ToolKind::CenterArc)),
        body: r###"Click the centre, a radius/start point and then the arc endpoint. Move around the centre to draw beyond 180 degrees; clockwise sweeps are negative. Before choosing the radius, arrows set the plane. Shift holds a direction or the final angle.
Enter a radius in project units or with a unit suffix, then enter signed degrees such as 90, -45deg or 270°. Enter 24s for segments. During the angle step, 500mmr changes the radius without committing. Geometry points take priority over optional snapping.
After creation, enter a new angle, radius with r, or segment count to amend the same Undo step. Enter 20c to derive the current arc segment count from a 20-segment circle, rounded up; this converts the current angle once. Curve identity, radius and segments remain editable in Properties, subject to attachment checks. Escape cancels the draft; model/context changes invalidate drafts and corrections. Use Circle for a complete turn."###,
    },
    Topic {
        title: r###"Pie"###,
        category: r###"Drawing"###,
        icon: Some(Icon::Tool(ToolKind::Pie)),
        body: r###"Click a centre, radius/start point and endpoint to draw a sector closed by two radii. Pie shares Center Arc's plane, Shift, radius, signed-angle and segment controls.
The arc retains editable curve identity; the two radius edges remain ordinary edges. Creation and immediate numeric corrections occupy one Undo step. Overlapping geometry can split or join the face, as with other drawing tools. Use Circle for a full turn."###,
    },
    Topic {
        title: r###"Rectangle"###,
        category: r###"Drawing"###,
        icon: Some(Icon::Tool(ToolKind::Rectangle)),
        body: r###"Click one corner, move diagonally, then click the opposite corner. The first face under the pointer supplies the drawing plane; empty space uses the current drawing axes.
Type width,depth and press Enter, for example 2000,1000 in a millimetre project. Use a semicolon when values contain decimal commas.
Square and golden-section proportions snap near the pointer. A dashed diagonal marks a square while drawing. Exact numeric dimensions and geometry inferences take priority.
The arrow keys lock the drawing plane: Right is perpendicular to red, Left to green, Up is horizontal; Down unlocks. Press one before the first click, or after it to turn the plane about that corner. Typing new dimensions right after drawing corrects the rectangle."###,
    },
    Topic {
        title: r###"Rotated Rectangle"###,
        category: r###"Drawing"###,
        icon: Some(Icon::Tool(ToolKind::RotatedRectangle)),
        body: r###"Click a first corner, a first-edge endpoint and then a width point. The third point sets both width and tilt around the first edge. Only the final step changes the model.
Before the first click, Right/Left/Up select a plane normal to the drawing red/green/blue axis. During the first edge, arrow keys lock its direction and Alt-click locks the current drawing plane without advancing. Down clears constraints. Shift holds the current edge direction or width angle.
Enter a first-edge length, then a width; or enter length,width at the width stage. Explicit unit suffixes and decimal-comma values separated with a semicolon are supported. Enter an angle such as 30deg to hold a tilt relative to the initial plane, then enter the width. Alt-click sets a new angle baseline without committing.
Immediately after drawing, enter length,width to correct both dimensions in the same Undo step. Escape or changing tools discards the draft. Edits or an editing-context change invalidate a draft or correction."###,
    },
    Topic {
        title: r###"Circle"###,
        category: r###"Drawing"###,
        icon: Some(Icon::Tool(ToolKind::Circle)),
        body: r###"Click the centre, then click to set the radius. The face under the centre supplies the plane.
Type a radius and press Enter. Enter a segment count such as 24s to control smoothness. Immediate numeric correction is available after creation until another operation intervenes.
The arrow keys lock the plane as for Rectangle: Right perpendicular to red, Left to green, Up horizontal, Down unlocks. Polygon and Arc use the same keys.
Select a circle to edit its radius or segment count in Properties. More segments increase geometry and rendering cost."###,
    },
    Topic {
        title: r###"Polygon"###,
        category: r###"Drawing"###,
        icon: Some(Icon::Tool(ToolKind::Polygon)),
        body: r###"Click a centre and then a radius point to draw a regular polygon. Enter a radius or a side count such as 6s through Measurements.
Polygon sides remain sharp when extruded, unlike smooth circle facets. Radius and side count can be edited for intact planar polygons in Properties.
Split curves, attached intermediate vertices or new intersections can prevent segment replacement. Undo restores the original geometry."###,
    },
    Topic {
        title: r###"Push/Pull"###,
        category: r###"Editing"###,
        icon: Some(Icon::Tool(ToolKind::PushPull)),
        body: r###"Click a face, move perpendicular to it and click to finish. Type a distance for an exact extrusion; double-click another face to repeat the last distance.
Ctrl keeps the starting face as a new base. Pulling circles creates softened side edges; polygon sides stay sharp. Pushing through suitable geometry can cut a hole.
Push/Pull acts on faces in the active context. Open a group first to edit its faces."###,
    },
    Topic {
        title: r###"Offset"###,
        category: r###"Editing"###,
        icon: Some(Icon::Tool(ToolKind::Offset)),
        body: r###"Click a face to offset its outline, or preselect a coplanar edge chain. Move to choose the side and distance, then click to finish.
Type a distance for an exact offset. Double-click repeats the last distance. Face offsets include inner hole boundaries.
Collapsed or self-intersecting results are rejected. An invalid value leaves the original geometry intact."###,
    },
    Topic {
        title: r###"Follow Me"###,
        category: r###"Editing"###,
        icon: Some(Icon::Tool(ToolKind::FollowMe)),
        body: r###"Select connected path edges, choose Follow Me, then click the profile face to sweep it along the path. Open and closed paths are supported.
You can also start from a profile and trace connected edges interactively, or press, drag and release along the path. A preselected face can supply its boundary as a path; Alt supports automatic surface paths.
The preview shows the proposed sweep. Escape cancels. Keep the profile and path in the same editing context."###,
    },
    Topic {
        title: r###"Move"###,
        category: r###"Editing"###,
        icon: Some(Icon::Tool(ToolKind::Move)),
        body: r###"Select geometry, click a reference point, then click its destination. With no selection, Move can pick an entity under the pointer. Type a distance for an exact move.
Ctrl toggles copy mode. After a copy, enter 5x for five copies at the same spacing, or /5 to divide the original span into five intervals. Shift and arrow locks constrain direction.
Glued components drag within their host plane. Use Unglue for free movement. Immediate correction updates the same Undo step. It stops being available after unrelated edits, selection changes or a context change."###,
    },
    Topic {
        title: r###"Rotate"###,
        category: r###"Editing"###,
        icon: Some(Icon::Tool(ToolKind::Rotate)),
        body: r###"Choose a rotation plane, click the centre, click a reference direction, then click the target angle. Arrow keys choose a plane; the protractor uses the angle increment in File > Settings > Model units.
Type an angle in degrees and press Enter. Ctrl toggles copies. After a copied rotation, 5x repeats the spacing and /5 divides the angle.
Selected groups and components show coloured handles at the bounding box face centres. Click a handle to fix the pivot and rotation plane, then choose the reference direction and angle. Arrow locks override the handle plane. Handles can be picked through other geometry.
Rotate supports selected geometry, groups, annotations and section symbols. Escape cancels the current operation."###,
    },
    Topic {
        title: r###"Scale"###,
        category: r###"Editing"###,
        icon: Some(Icon::Tool(ToolKind::Scale)),
        body: r###"Select entities and drag a scaling grip. Grips follow the current drawing axes and can be picked through geometry. The nearest grip on screen wins; overlapping grips prefer the one nearest the camera. Corner grips start with uniform scaling; Shift toggles uniformity.
Ctrl changes between an opposite grip pivot and the centre. Measurements accepts factors such as 2 or 2,3, or explicit lengths such as 1.5m. Negative factors mirror the selection.
Geometry changes size, while annotation font sizes remain display settings. Immediate numeric correction shares the original Undo step."###,
    },
    Topic {
        title: r###"Tape Measure"###,
        category: r###"Construction"###,
        icon: Some(Icon::Tool(ToolKind::TapeMeasure)),
        body: r###"Click two points to measure their distance. Starting from an edge creates a parallel guide line; starting from a point creates a guide point. Press and release Ctrl to toggle measure-only mode. Measuring from an edge then reports the full point-to-point distance.
Type a positive or negative distance to place a guide precisely. Immediately typing another distance corrects the guide in the same Undo step. After clicking two measured points, typing a new length requests proportional resizing of the model or active context. Review the confirmation before accepting.
Guide points retain a finite dashed segment from the measured start. The segment and endpoint can be selected or erased. Hide Selected Geometry also hides guides; Unhide All Geometry restores them. Window > Guides toggles all construction guides. Delete Guides clears guides in the current context. Guides are not solid model geometry."###,
    },
    Topic {
        title: r###"Protractor"###,
        category: r###"Construction"###,
        icon: Some(Icon::Tool(ToolKind::Protractor)),
        body: r###"Choose the angle plane, click its centre and reference direction, then set the measured angle. Arrow keys choose a plane.
Measurements accepts an exact angle in degrees. Ctrl creates an angled construction guide. Angle snapping and display precision are configured in File > Settings > Model units.
Use Rotate to transform geometry; Protractor measures angles and creates guides."###,
    },
    Topic {
        title: r###"Axes"###,
        category: r###"Construction"###,
        icon: Some(Icon::Tool(ToolKind::Axes)),
        body: r###"Click the new origin, a point along red, then a point toward green. This sets the drawing frame for the current context.
Drawing tools, inference locks and Scale grips use these axes. Moving the axes does not move model geometry.
Window > Reset drawing axes restores the default frame. Groups can have their own local drawing axes."###,
    },
    Topic {
        title: r###"Paint Bucket"###,
        category: r###"Materials"###,
        icon: Some(Icon::Tool(ToolKind::PaintBucket)),
        body: r###"Choose a material in Window > Materials and click a visible face side, edge, group or component. With a selection, painting applies to the selected entities.
Alt samples material and texture placement. Ctrl fills adjacent matching faces, Shift replaces matching materials in the context, and Ctrl+Shift replaces matching materials within connected geometry.
Paint connected surfaces treats soft facets as one surface. Turn it off to paint a single facet. Back-face painting is separate from front-face and edge materials."###,
    },
    Topic {
        title: r###"Orbit"###,
        category: r###"Camera"###,
        icon: Some(Icon::Tool(ToolKind::Orbit)),
        body: r###"Drag to rotate the view around a picked visible surface, or around the current camera target when no surface is picked.
Middle-button dragging temporarily orbits while another tool is active. Wheel zoom remains available.
Touch: one-finger dragging orbits with Select or Orbit; other tools may use that gesture for their own operation."###,
    },
    Topic {
        title: r###"Pan"###,
        category: r###"Camera"###,
        icon: Some(Icon::Tool(ToolKind::Pan)),
        body: r###"Drag to slide the camera sideways and vertically without changing its viewing direction.
Shift+middle-button drag temporarily pans while another tool is active. On touch screens, use two fingers.
Pan changes only the view. Model geometry is unaffected."###,
    },
    Topic {
        title: r###"Zoom"###,
        category: r###"Camera"###,
        icon: Some(Icon::Tool(ToolKind::Zoom)),
        body: r###"Drag with Zoom or use the mouse wheel. Wheel zoom moves toward the pointer. Touch supports pinch zoom.
In perspective, Shift-drag adjusts field of view. Measurements accepts an angle or a focal length in millimetres.
Use Camera > Previous view to return to an earlier view, or Zoom Extents to find your model."###,
    },
    Topic {
        title: r###"Zoom Window"###,
        category: r###"Camera"###,
        icon: Some(Icon::Tool(ToolKind::ZoomWindow)),
        body: r###"Drag a rectangle over the part of the model you want to see. Release to frame that area.
Use Ctrl+Shift+W to activate the tool. Escape cancels an unfinished rectangle.
The tool works in perspective and parallel projection and records the previous camera view."###,
    },
    Topic {
        title: r###"Position Camera"###,
        category: r###"Camera"###,
        icon: Some(Icon::Tool(ToolKind::PositionCamera)),
        body: r###"Click a point to place the camera. Drag while placing to aim its direction. Geometry inference helps locate the position.
Measurements sets eye height. Follow with Look Around to turn from a fixed position, or Walk to move through the model.
Camera placement affects the view rather than creating a camera object in the document."###,
    },
    Topic {
        title: r###"Look Around"###,
        category: r###"Camera"###,
        icon: Some(Icon::Tool(ToolKind::LookAround)),
        body: r###"Drag to look left, right, up or down while keeping the eye position fixed.
Measurements sets eye height. Use Position Camera first to place the eye accurately.
Use Orbit to turn around an object instead, or Walk to change position."###,
    },
    Topic {
        title: r###"Walk"###,
        category: r###"Camera"###,
        icon: Some(Icon::Tool(ToolKind::Walk)),
        body: r###"Press and drag from the walking crosshair to move through the model. Distance from the crosshair controls speed.
Ctrl runs, Shift changes to vertical motion, and Alt bypasses collision. The middle button temporarily looks around. Eye height is available through Measurements.
Ground following and body collision help navigate spaces, but imperfect or open geometry may affect movement."###,
    },
    Topic {
        title: r###"Dimensions"###,
        category: r###"Annotations"###,
        icon: Some(Icon::Tool(ToolKind::Dimension)),
        body: r###"Click an edge, circle or arc for a quick dimension, or choose two points and then place the dimension line. Ctrl starts a point instead of using an entity shortcut.
Measurements sets the physical offset. Dimensions can follow referenced geometry; Properties edits text, endpoints, font and alignment. Use <> inside custom text to retain the automatic value.
File > Settings > Annotations controls defaults and visibility. Deleted geometry can leave a dimension unassociated; review its highlighted state and anchors."###,
    },
    Topic {
        title: r###"Text"###,
        category: r###"Annotations"###,
        icon: Some(Icon::Tool(ToolKind::Text)),
        body: r###"Click empty space for screen text, or click geometry and place an attached leader. Double-click a face for an area label; double-click an existing text label to edit it.
Enter adds a line. Enter twice, Ctrl+Enter, Create or a canvas click completes the draft. Escape cancels.
Properties edits content and appearance. File > Settings > Annotations controls defaults. Screen text stays in the viewport; attached labels belong to the model."###,
    },
    Topic {
        title: r###"Make Component"###,
        category: r###"Model organisation"###,
        icon: Some(Icon::Action(Action::MakeComponent)),
        body: r###"Select geometry and use Make Component, or press G. The new instance contains that geometry and uses a reusable definition.
Copies of a component share edits. Make Unique separates a copy. The Components window provides in-model browsing, staged insertion and insertion-axis editing.
Use Make Group when you want a separate object without shared component editing."###,
    },
    Topic {
        title: r###"3D Text"###,
        category: r###"Annotations"###,
        icon: Some(Icon::Action(Action::Text3d)),
        body: r###"Enter text in the 3D Text dialog, choose Sans/Serif/Mono, bold, alignment, height and optional fill/extrusion depth. Lengths accept project units or explicit suffixes.
Place builds a geometry preview. Move it to the target, click to commit, or Escape to cancel. The result is editable model geometry in an instance, rather than an annotation.
Complex text and fine outlines create more geometry. Font options are the supplied families, rather than a system font browser."###,
    },
    Topic {
        title: r###"Section Plane"###,
        category: r###"Camera"###,
        icon: Some(Icon::Action(Action::SectionPlane)),
        body: r###"Choose Section Plane and click a face to place a cutting plane. Manage planes in Window > Section Planes.
Select, Move, Rotate or copy the visible plane symbol. Double-click a symbol activates its cut; reverse its direction in the panel. Planes belong to their editing context.
Section cuts change visibility without deleting geometry. File > Settings > Appearance controls section display."###,
    },
    Topic {
        title: r###"From Scratch"###,
        category: r###"Terrain"###,
        icon: Some(Icon::Action(Action::TerrainGrid)),
        body: r###"Set Grid Spacing in Measurements before the first click. Click the origin, a point defining length, and a perpendicular width point.
Measurements accepts exact dimensions. The result is a grouped triangulated grid with softened diagonals. Enter the group before editing its vertices.
Escape cancels the preview. A finished grid is one Undo step. Keep spacing large enough to avoid unnecessary triangles."###,
    },
    Topic {
        title: r###"From Contours"###,
        category: r###"Terrain"###,
        icon: Some(Icon::Action(Action::FromContours)),
        body: r###"Select survey contour edges and run From Contours to triangulate them into terrain. Contour elevation supplies the surface height.
Inspect the result for gaps or unexpected triangulation where contours are sparse, duplicate or ambiguous.
The operation creates terrain geometry; keep a copy of source contours when you need them for later reference."###,
    },
    Topic {
        title: r###"Smoove"###,
        category: r###"Terrain"###,
        icon: Some(Icon::Action(Action::Smoove)),
        body: r###"Enter or explode the terrain group first. Set a radius in Measurements, click a triangle near the desired vertex, move along blue and click to finish.
Type an offset after choosing a centre, or enter a radius with r, for example 2mr. Preselection can restrict eligible terrain vertices.
The deformation falls off away from the centre. Double-click repeats the last displacement. Escape cancels the current preview."###,
    },
    Topic {
        title: r###"Stamp"###,
        category: r###"Terrain"###,
        icon: Some(Icon::Action(Action::Stamp)),
        body: r###"Start Stamp, choose the footprint and target terrain following the status line, then choose the transition offset and elevation.
Measurements supplies exact values at each stage. The tool flattens terrain under the footprint and builds a transition to the surrounding surface.
Review the preview before committing. Invalid or ambiguous terrain intersections are rejected."###,
    },
    Topic {
        title: r###"Drape"###,
        category: r###"Terrain"###,
        icon: Some(Icon::Action(Action::Drape)),
        body: r###"Choose the source outline and target terrain as requested by the status line. Drape projects the outline onto the surface.
The preview shows where projected edges will divide terrain triangles. Click to finish or Escape to cancel.
Source and target must be accessible in the current context. Drape adds outlines rather than flattening terrain."###,
    },
    Topic {
        title: r###"Add Detail"###,
        category: r###"Terrain"###,
        icon: Some(Icon::Action(Action::AddDetail)),
        body: r###"Enter the terrain context and choose Add Detail. Pick a terrain location to subdivide its triangles and add a vertex.
Move the new detail and use Measurements for a precise displacement when requested by the tool.
Use this locally where extra resolution is needed. Refining the entire terrain unnecessarily increases model size."###,
    },
    Topic {
        title: r###"Flip Edge"###,
        category: r###"Terrain"###,
        icon: Some(Icon::Action(Action::FlipEdge)),
        body: r###"Enter the terrain group and choose Flip Edge. Point at an eligible internal diagonal shared by two triangles.
The preview shows the alternative diagonal. Click to commit or Escape to cancel.
Boundary edges or configurations that would create invalid triangles cannot be flipped."###,
    },
    Topic {
        title: r###"Zoom Extents"###,
        category: r###"Camera"###,
        icon: None,
        body: r###"Use Zoom Extents or Shift+Z to frame the model. This is useful after navigation or when opening unfamiliar geometry.
Hidden or tag-invisible geometry does not help explain what you can currently see. If objects remain missing, inspect visibility and section settings.
Camera > Previous view restores an earlier view."###,
    },
    Topic {
        title: r###"Intersect Faces"###,
        category: r###"Editing"###,
        icon: None,
        body: r###"Select faces and use Intersect Faces with Model or Intersect Faces with Selection from Edit or the context menu.
The first considers surrounding model geometry, while the second limits intersections to the selected set. Resulting edges are placed in the active editing context, including transformed group intersections.
Intersection creates cutting edges; remove unwanted pieces afterwards. It is not a general solid Boolean operation."###,
    },
];

fn matches(topic: &Topic, query: &str) -> bool {
    let haystack = format!("{} {} {}", topic.title, topic.category, topic.body).to_lowercase();
    query.split_whitespace().all(|word| haystack.contains(&word.to_lowercase()))
}

pub fn open_current(app: &mut App3d) {
    app.help.open = true;
    app.help.query.clear();
    let current = if app.paste_place.is_some() {
        "Copy, paste and history"
    } else if app.texture_session.is_some() {
        "Materials and texture position"
    } else if app.component_place.is_some() {
        "Groups and components"
    } else if app.section_placing.is_some() {
        "Section Plane"
    } else if app.drape.is_some() {
        "Drape"
    } else if app.stamp.is_some() {
        "Stamp"
    } else if app.detail.is_some() {
        "Add Detail"
    } else if app.flip.is_some() {
        "Flip Edge"
    } else if app.smoove.is_some() {
        "Smoove"
    } else if app.terrain.is_some() {
        "From Scratch"
    } else if app.text3d.is_some() {
        "3D Text"
    } else if app.text_creation.is_some() {
        "Text"
    } else {
        app.tools.active().name()
    };
    app.help.selected = TOPICS.iter().position(|topic| topic.title == current).unwrap_or(0);
}

/// Navigation rows reserve their complete text/padding area before painting any state.
fn topic_button(ui: &mut egui::Ui, title: &str, selected: bool, width: f32) -> egui::Response {
    let text = ui.painter().layout(
        title.to_owned(),
        egui::FontId::proportional(15.),
        egui::Color32::PLACEHOLDER,
        (width - 24.).max(24.),
    );
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, (text.size().y + 14.).max(32.)), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, title));
    let visuals = ui.style().interact_selectable(&response, selected);
    if selected || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 5., visuals.weak_bg_fill);
        let stroke = if response.has_focus() { egui::Stroke::new(1., crate::theme::ACCENT) } else { visuals.bg_stroke };
        ui.painter().rect_stroke(rect.shrink(0.5), 5., stroke, egui::StrokeKind::Inside);
    }
    ui.painter().galley(
        egui::pos2(rect.left() + 12., rect.center().y - text.size().y * 0.5),
        text,
        visuals.text_color(),
    );
    response
}

pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.help.open {
        return;
    }
    let mut open = true;
    let initial = (ctx.content_rect().size() - egui::vec2(40., 60.)).min(egui::vec2(900., 640.));
    let window = crate::theme::window("Rysmo Help")
        .open(&mut open)
        .default_size(initial)
        .min_size(egui::vec2(360., 260.))
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Search");
                ui.add(
                    crate::theme::singleline(&mut app.help.query)
                        .hint_text("Tool, shortcut or workflow")
                        .desired_width(260.),
                );
                if ui.button("Clear").clicked() {
                    app.help.query.clear();
                }
                if ui.button("Current tool (F1)").clicked() {
                    open_current(app);
                }
            });
            ui.separator();
            let found: Vec<_> = TOPICS
                .iter()
                .enumerate()
                .filter(|(_, topic)| matches(topic, &app.help.query))
                .map(|(i, _)| i)
                .collect();
            if !found.contains(&app.help.selected) {
                app.help.selected = found.first().copied().unwrap_or(0);
            }
            if found.is_empty() {
                ui.label("No matching topics. Try a tool name, a shortcut or a shorter search.");
                return;
            }
            let height = ui.available_height().max(160.);
            let width = ui.available_width();
            let navigation = if width < 580. { 145. } else { 205. };
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(navigation, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(navigation);
                        egui::ScrollArea::vertical()
                            .id_salt("help_topics")
                            .auto_shrink([false, false])
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                            .max_height(height)
                            .show(ui, |ui| {
                                ui.set_width(navigation - 18.);
                                for category in [
                                    "Start here",
                                    "Selection",
                                    "Drawing",
                                    "Editing",
                                    "Construction",
                                    "Materials",
                                    "Camera",
                                    "Annotations",
                                    "Model organisation",
                                    "Terrain",
                                    "Settings and files",
                                ] {
                                    if !found.iter().any(|&i| TOPICS[i].category == category) {
                                        continue;
                                    }
                                    ui.add_space(8.);
                                    ui.strong(category);
                                    for &i in found.iter().filter(|&&i| TOPICS[i].category == category) {
                                        if topic_button(ui, TOPICS[i].title, app.help.selected == i, navigation - 18.)
                                            .clicked()
                                        {
                                            app.help.selected = i;
                                        }
                                    }
                                }
                            });
                    },
                );
                ui.separator();
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(("help_article", app.help.selected))
                            .auto_shrink([false, false])
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                            .max_height(height)
                            .show(ui, |ui| {
                                let topic = &TOPICS[app.help.selected];
                                ui.horizontal_wrapped(|ui| {
                                    if let Some(icon) = topic.icon {
                                        ui.add_enabled_ui(false, |ui| {
                                            let _ = crate::icons::button(ui, icon, false, 30.);
                                        });
                                    }
                                    ui.heading(topic.title);
                                });
                                ui.weak(topic.category);
                                if let Some(Icon::Tool(tool)) = topic.icon
                                    && !tool.shortcut().is_empty()
                                {
                                    ui.label(format!("Shortcut: {}", tool.shortcut()));
                                }
                                ui.add_space(10.);
                                for paragraph in topic.body.split('\n') {
                                    ui.label(paragraph);
                                    ui.add_space(10.);
                                }
                            });
                    },
                );
            });
        });
    app.help.open = open;
    if let Some(window) = window {
        let r = window.response.rect;
        app.ui_rects.push((
            rysmo_math::DVec2::new(r.min.x as f64, r.min.y as f64),
            rysmo_math::DVec2::new(r.max.x as f64, r.max.y as f64),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_tool_and_terrain_action_has_help_and_search_finds_content() {
        for tool in ToolKind::ALL {
            assert_eq!(
                TOPICS.iter().filter(|topic| matches!(topic.icon, Some(Icon::Tool(t)) if t == tool)).count(),
                1,
                "{}",
                tool.name()
            );
        }
        for action in [
            Action::MakeComponent,
            Action::Text3d,
            Action::SectionPlane,
            Action::TerrainGrid,
            Action::FromContours,
            Action::Smoove,
            Action::Stamp,
            Action::Drape,
            Action::AddDetail,
            Action::FlipEdge,
            Action::Fillet,
        ] {
            assert!(TOPICS.iter().any(|topic| matches!(topic.icon, Some(Icon::Action(a)) if a == action)));
        }
        assert!(TOPICS.iter().any(|topic| matches(topic, "clipboard tabs")));
        assert!(TOPICS.iter().any(|topic| matches(topic, "scale negative")));
    }
    #[test]
    fn navigation_hover_selection_and_focus_preserve_rows_and_article_bounds() {
        for width in [127., 187.] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut baseline: Vec<egui::Rect> = Vec::new();
            for pass in 0..7 {
                let hovered = (2..=4).contains(&pass);
                let selected = pass == 3 || pass == 5;
                let pointer = if hovered { baseline[1].center() } else { egui::pos2(790., 590.) };
                let mut rects = Vec::new();
                let mut response_hovered = false;
                let mut response_focused = false;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 600.))),
                        events: vec![egui::Event::PointerMoved(pointer)],
                        ..Default::default()
                    },
                    |ui| {
                        ui.horizontal_top(|ui| {
                            ui.allocate_ui_with_layout(
                                egui::vec2(width, 450.),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    for title in ["Getting started", "Groups, components and insertion axes", "Line"] {
                                        let response = topic_button(ui, title, selected, width);
                                        if title.starts_with("Groups") {
                                            response_hovered = response.hovered();
                                            response_focused = response.has_focus();
                                            if pass == 4 {
                                                response.request_focus();
                                            }
                                        }
                                        rects.push(response.rect);
                                    }
                                },
                            );
                            rects.push(ui.separator().rect);
                            rects.push(ui.allocate_exact_size(egui::vec2(400., 450.), egui::Sense::hover()).0);
                        });
                    },
                );
                output.textures_delta.clear();
                if pass == 0 {
                    baseline = rects;
                } else {
                    assert_eq!(rects, baseline, "navigation width {width}, pass {pass}");
                }
                if hovered {
                    assert!(response_hovered);
                }
                if pass >= 5 {
                    assert!(response_focused);
                }
            }
        }
    }

    #[test]
    fn reading_every_topic_preserves_document_and_registers_window_bounds() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = App3d::new(rysmo_model::Editor::new());
        let before = app.editor.doc().clone();
        app.help.open = true;
        for i in 0..TOPICS.len() {
            app.help.selected = i;
            app.ui_rects.clear();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100., 780.))),
                    ..Default::default()
                },
                |ui| panel(ui.ctx(), &mut app),
            );
            output.textures_delta.clear();
            assert!(!app.ui_rects.is_empty());
            assert_eq!(app.editor.doc(), &before);
            assert_eq!(app.editor.undo_depth(), 0);
        }
        app.set_tool(ToolKind::Rotate);
        open_current(&mut app);
        assert_eq!(TOPICS[app.help.selected].title, "Rotate");
        assert!(app.help.query.is_empty());
        crate::terrain_tool::begin(&mut app);
        open_current(&mut app);
        assert_eq!(TOPICS[app.help.selected].title, "From Scratch");
        assert!(app.terrain.is_some());
        assert_eq!(app.editor.doc(), &before);
    }
}
