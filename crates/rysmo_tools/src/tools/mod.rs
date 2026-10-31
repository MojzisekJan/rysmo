//! Interactive tool behaviour: click-move-click (or
//! press-drag-release) drawing, Shift to lock an inference, arrow keys to lock an axis,
//! Ctrl to copy or to toggle a mode, Alt to sample, and the VCB for exact values, also
//! right after an operation to correct it.
//!
//! Every tool works with a single pointer: what a mouse shows while hovering, a finger
//! shows while dragging, and a tap is a click.

mod centre_arc;
mod dimension;
mod draw;
mod edit;
mod protractor;
mod rotated_rectangle;
mod select;
mod three_point_arc;

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use rysmo_infer::{Accel, Inference, Kind, Lock, MeshIndex, Other, Query, SnapGuide, axis_color, infer};
use rysmo_math::{Axis, DMat4, DVec2, DVec3, EPS, Plane, View};
use rysmo_mesh::{DefinitionId, EdgeId, Element, FaceId, GuideId, InstanceId, MaterialId};
use rysmo_model::{Command, Editor, Guide, Outcome};

use crate::vcb::{VcbInput, parse_vcb};

/// Screen-space annotations supplied by the renderer for a selection rectangle.
#[derive(Clone, Debug, Default)]
pub struct AnnotationBoxHits {
    pub elements: Vec<Element>,
    /// Per active child instance: any visible annotation crosses, all are enclosed.
    pub instances: std::collections::BTreeMap<rysmo_mesh::InstanceId, (bool, bool)>,
}

/// The tools, in toolbar order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToolKind {
    Select,
    Eraser,
    Line,
    Freehand,
    Arc,
    ThreePointArc,
    CenterArc,
    Pie,
    Rectangle,
    RotatedRectangle,
    Circle,
    Polygon,
    PushPull,
    Offset,
    FollowMe,
    Move,
    Rotate,
    Scale,
    TapeMeasure,
    Protractor,
    Axes,
    PaintBucket,
    Orbit,
    Pan,
    Zoom,
    ZoomWindow,
    PositionCamera,
    LookAround,
    Dimension,
    Text,
    Walk,
}

impl ToolKind {
    pub const ALL: [ToolKind; 31] = [
        ToolKind::Select,
        ToolKind::Eraser,
        ToolKind::Line,
        ToolKind::Freehand,
        ToolKind::Arc,
        ToolKind::ThreePointArc,
        ToolKind::CenterArc,
        ToolKind::Pie,
        ToolKind::Rectangle,
        ToolKind::RotatedRectangle,
        ToolKind::Circle,
        ToolKind::Polygon,
        ToolKind::PushPull,
        ToolKind::Offset,
        ToolKind::FollowMe,
        ToolKind::Move,
        ToolKind::Rotate,
        ToolKind::Scale,
        ToolKind::TapeMeasure,
        ToolKind::Protractor,
        ToolKind::Axes,
        ToolKind::PaintBucket,
        ToolKind::Orbit,
        ToolKind::Pan,
        ToolKind::Zoom,
        ToolKind::ZoomWindow,
        ToolKind::PositionCamera,
        ToolKind::LookAround,
        ToolKind::Dimension,
        ToolKind::Text,
        ToolKind::Walk,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ToolKind::Select => "Select",
            ToolKind::Eraser => "Eraser",
            ToolKind::Line => "Line",
            ToolKind::Freehand => "Freehand",
            ToolKind::Arc => "Arc",
            ToolKind::ThreePointArc => "Three Point Arc",
            ToolKind::CenterArc => "Center Arc",
            ToolKind::Pie => "Pie",
            ToolKind::Rectangle => "Rectangle",
            ToolKind::RotatedRectangle => "Rotated Rectangle",
            ToolKind::Circle => "Circle",
            ToolKind::Polygon => "Polygon",
            ToolKind::PushPull => "Push/Pull",
            ToolKind::Offset => "Offset",
            ToolKind::FollowMe => "Follow Me",
            ToolKind::Move => "Move",
            ToolKind::Rotate => "Rotate",
            ToolKind::Scale => "Scale",
            ToolKind::TapeMeasure => "Tape Measure",
            ToolKind::Protractor => "Protractor",
            ToolKind::Axes => "Axes",
            ToolKind::PaintBucket => "Paint Bucket",
            ToolKind::Orbit => "Orbit",
            ToolKind::Pan => "Pan",
            ToolKind::Zoom => "Zoom",
            ToolKind::ZoomWindow => "Zoom Window",
            ToolKind::PositionCamera => "Position Camera",
            ToolKind::LookAround => "Look Around",
            ToolKind::Dimension => "Dimensions",
            ToolKind::Text => "Text",
            ToolKind::Walk => "Walk",
        }
    }

    /// The default shortcut ("" for tools without a shortcut).
    pub fn shortcut(self) -> &'static str {
        match self {
            ToolKind::Select => "Space",
            ToolKind::Eraser => "E",
            ToolKind::Line => "L",
            ToolKind::Freehand => "",
            ToolKind::Arc => "A",
            ToolKind::ThreePointArc | ToolKind::CenterArc | ToolKind::Pie => "",
            ToolKind::Rectangle => "R",
            ToolKind::RotatedRectangle => "",
            ToolKind::Circle => "C",
            ToolKind::Polygon => "",
            ToolKind::PushPull => "P",
            ToolKind::Offset => "F",
            ToolKind::FollowMe => "",
            ToolKind::Move => "M",
            ToolKind::Rotate => "Q",
            ToolKind::Scale => "S",
            ToolKind::TapeMeasure => "T",
            ToolKind::Protractor => "",
            ToolKind::Axes => "",
            ToolKind::PaintBucket => "B",
            ToolKind::Orbit => "O",
            ToolKind::Pan => "H",
            ToolKind::Zoom => "Z",
            ToolKind::ZoomWindow => "Ctrl+Shift+W",
            ToolKind::PositionCamera | ToolKind::LookAround | ToolKind::Walk | ToolKind::Dimension | ToolKind::Text => {
                ""
            }
        }
    }

    /// Whether a one-finger drag drives the tool rather than orbiting the camera.
    pub fn takes_drags(self) -> bool {
        !matches!(self, ToolKind::Select | ToolKind::Orbit | ToolKind::LookAround | ToolKind::PaintBucket)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// Where the pointer is and how the camera sees the context being edited. The view is
/// in the context's coordinates (the app folds the context's transform into it).
pub struct Ctx<'a> {
    pub view: &'a View,
    pub cursor: DVec2,
    pub modifiers: Modifiers,
    /// Pick tolerance multiplier: 1 with a mouse, about 2.5 on a touch screen.
    pub scale: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pointer {
    Move,
    /// The primary button went down; `clicks` counts quick clicks in a row.
    Down {
        clicks: u8,
    },
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Escape,
    /// Arrow keys lock an axis: right red, left green, up blue, down unlocks.
    Right,
    Left,
    Up,
    Down,
    /// Shift went down (true) or up (false): lock the current inference while held.
    Shift(bool),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewLine {
    pub a: DVec3,
    pub b: DVec3,
    pub color: [f32; 3],
    pub dashed: bool,
    pub thick: bool,
}

/// What the viewport should draw on top of the model for the active tool, in the
/// coordinates of the context being edited.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preview {
    pub lines: Vec<PreviewLine>,
    pub dimension: Option<rysmo_model::dimensions::Dimension>,
    pub instanced_texts: Vec<(rysmo_model::text::Text, DMat4)>,
    pub moving_texts: Vec<rysmo_model::text::Text>,
    pub moving_dimensions: Vec<rysmo_model::dimensions::Dimension>,
    pub instanced_dimensions: Vec<(rysmo_model::dimensions::Dimension, DMat4)>,
    pub inference: Option<Inference>,
    /// Faces to highlight (the face Push/Pull would take, Move's pre-selection).
    pub faces: Vec<FaceId>,
    pub edges: Vec<EdgeId>,
    /// Groups and components to highlight.
    pub instances: Vec<rysmo_mesh::InstanceId>,
    /// Small square markers (Scale's grips).
    pub markers: Vec<(DVec3, [f32; 3])>,
    /// A tooltip that replaces the inference's ("Half Circle", an angle).
    pub label: Option<String>,
    /// The selection rectangle in pixels and whether it is a crossing selection.
    pub select_box: Option<(DVec2, DVec2, bool)>,
}

pub(crate) const BLACK: [f32; 3] = [0.0, 0.0, 0.0];
pub(crate) const MAGENTA: [f32; 3] = [0.85, 0.0, 0.85];
pub(crate) const GRIP: [f32; 3] = [0.9, 0.1, 0.1];

/// A Scale grip: which of the bounding box's 26 handles, as -1/0/1 per axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Grip(pub [i8; 3]);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum OffsetTarget {
    Face(FaceId),
    /// Selected edges and the normal of their plane.
    Chain(DVec3),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum State {
    Idle,
    Line {
        start: DVec3,
    },
    Following {
        face: FaceId,
        path: Vec<EdgeId>,
        automatic: bool,
        revision: u64,
        context: Vec<rysmo_mesh::InstanceId>,
        preview: Vec<(DVec3, DVec3)>,
    },
    Freehand {
        points: Vec<DVec3>,
        plane: Plane,
    },
    Rectangle {
        corner: DVec3,
        plane: Plane,
    },
    RotatedEdge {
        corner: DVec3,
        plane: Option<Plane>,
        revision: u64,
        context: Vec<rysmo_mesh::InstanceId>,
    },
    RotatedWidth {
        corner: DVec3,
        u: DVec3,
        baseline: DVec3,
        plane: Option<Plane>,
        angle_lock: Option<DVec3>,
        revision: u64,
        context: Vec<rysmo_mesh::InstanceId>,
    },
    Circle {
        centre: DVec3,
        plane: Plane,
    },
    ThreeMiddle {
        start: DVec3,
        plane: Plane,
        revision: u64,
        context: Vec<rysmo_mesh::InstanceId>,
    },
    ThreeEnd {
        start: DVec3,
        through: DVec3,
        plane: Plane,
        revision: u64,
        context: Vec<rysmo_mesh::InstanceId>,
    },
    CentreRadius {
        centre: DVec3,
        plane: Plane,
        revision: u64,
        context: Vec<rysmo_mesh::InstanceId>,
    },
    CentreSweep {
        centre: DVec3,
        plane: Plane,
        start: DVec3,
        radius: f64,
        sweep: f64,
        revision: u64,
        context: Vec<rysmo_mesh::InstanceId>,
    },
    ArcChord {
        start: DVec3,
        plane: Plane,
    },
    ArcBulge {
        start: DVec3,
        end: DVec3,
        plane: Plane,
        bulge: f64,
    },
    PushPull {
        face: FaceId,
        start: DVec3,
        normal: DVec3,
        distance: f64,
    },
    Move {
        elements: Vec<Element>,
        start: DVec3,
        offset: DVec3,
    },
    RotatePivot {
        elements: Vec<Element>,
        centre: DVec3,
        axis: DVec3,
    },
    Rotating {
        elements: Vec<Element>,
        centre: DVec3,
        axis: DVec3,
        from: DVec3,
        angle: f64,
    },
    Scaling {
        elements: Vec<Element>,
        grip: Grip,
        fixed: DVec3,
        handle: DVec3,
        factors: DVec3,
        basis: DMat4,
        extent: DVec3,
        centre: DVec3,
        opposite: DVec3,
        uniform: bool,
        drag_plane: Plane,
    },
    Offsetting {
        target: OffsetTarget,
        edges: Vec<EdgeId>,
        distance: f64,
    },
    AxesRed {
        origin: DVec3,
    },
    AxesGreen {
        origin: DVec3,
        red: DVec3,
    },
    AngleBase {
        centre: DVec3,
        plane: Plane,
    },
    AngleSweep {
        centre: DVec3,
        plane: Plane,
        from: DVec3,
        angle: f64,
    },
    Taping {
        start: DVec3,
        along: Option<DVec3>,
    },
    Erasing {
        marked: BTreeSet<Element>,
    },
    Selecting {
        from: DVec2,
    },
}

/// The last finished operation, which the VCB can still change.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Last {
    None,
    ThreePointArc {
        start: DVec3,
        through: DVec3,
        end: DVec3,
        normal: DVec3,
        segments: u32,
        revision: u64,
    },
    CentreArc {
        centre: DVec3,
        normal: DVec3,
        start: DVec3,
        radius: f64,
        sweep: f64,
        segments: u32,
        pie: bool,
        revision: u64,
    },
    /// A 2-point arc: segments or a new bulge typed right after it redraw it.
    Arc {
        start: DVec3,
        end: DVec3,
        normal: DVec3,
        bulge: f64,
        segments: u32,
        revision: u64,
    },
    RotatedRectangle {
        corner: DVec3,
        u: DVec3,
        v: DVec3,
        facing: Option<DVec3>,
        revision: u64,
    },
    TapeGuide {
        start: DVec3,
        along: Option<DVec3>,
        direction: DVec3,
        length: f64,
        revision: u64,
    },
    TapeMeasurement {
        length: f64,
        revision: u64,
    },
    Radial {
        centre: DVec3,
        plane: Plane,
        radius: f64,
        towards: DVec3,
        segments: u32,
        revision: u64,
    },
    PushPull {
        face: FaceId,
        distance: f64,
        keep_base: bool,
        free: bool,
    },
    Move {
        elements: Vec<Element>,
        offset: DVec3,
        text_view: Option<(View, DVec3)>,
    },
    Copy {
        elements: Vec<Element>,
        offset: DVec3,
        copies: u32,
        divide: bool,
        text_view: Option<(View, DVec3)>,
    },
    Rotate {
        elements: Vec<Element>,
        centre: DVec3,
        axis: DVec3,
        angle: f64,
        copies: u32,
        divide: bool,
    },
    Scale {
        elements: Vec<Element>,
        origin: DVec3,
        grip: Grip,
        factors: DVec3,
        basis: DMat4,
        extent: DVec3,
        revision: u64,
        uniform: bool,
    },
    Offset {
        target: OffsetTarget,
        edges: Vec<EdgeId>,
        distance: f64,
        revision: u64,
    },
}

/// The active tool and everything it remembers between events.
pub struct ToolBox {
    scenes: Mutex<SceneCache>,
    pub(crate) current_view: Option<View>,
    pub(crate) axes: rysmo_math::DrawingAxes,
    pub(crate) dimension_start: Option<rysmo_model::dimensions::DimensionAnchor>,
    pub(crate) dimension_draft: Option<rysmo_model::dimensions::Dimension>,
    active: ToolKind,
    pub(crate) state: State,
    pub(crate) inference: Option<Inference>,
    override_pick: Option<rysmo_infer::Pick>,
    annotation_box: Option<AnnotationBoxHits>,
    preview_copy: bool,
    /// Lock from an arrow key.
    axis_lock: Option<Axis>,
    /// Rectangle, Circle, Polygon and Arc: the arrow keys lock the drawing plane, which
    /// is perpendicular to this axis, instead of taking the face under the first click.
    pub(crate) plane_lock: Option<Axis>,
    /// Lock from holding Shift.
    shift_lock: Option<Lock>,
    glue_plane: Option<Plane>,
    /// Points the cursor touched recently, for "from point" inferences.
    hints: Vec<DVec3>,
    /// Where the button went down, to tell a click from a drag.
    pub(crate) press: Option<DVec2>,
    pub(crate) last: Last,
    correction_scope: Option<(u64, Vec<rysmo_mesh::InstanceId>, usize)>,
    pub(crate) pending_resize: Option<(f64, u64)>,
    pub(crate) tape_measure_only: bool,
    /// Last Push/Pull distance, for the double-click repeat.
    pub(crate) last_distance: Option<f64>,
    /// Last Offset distance, for the double-click repeat.
    pub(crate) last_offset: Option<f64>,
    pub(crate) circle_segments: u32,
    pub(crate) polygon_segments: u32,
    pub(crate) arc_segments: u32,
    /// The Paint Bucket's material (`None`: the default material).
    pub(crate) paint: Option<MaterialId>,
    pub(crate) paint_surface: bool,
    pub(crate) paint_placement: Option<(
        rysmo_model::TexturePlacement,
        Vec<rysmo_model::InstanceId>,
        FaceId,
        rysmo_math::DMat4,
        rysmo_math::Plane,
    )>,
    /// The cursor at the last event, in pixels.
    pub(crate) cursor: DVec2,
    /// World size of a pixel near the cursor at the last event, to size the protractor.
    pub(crate) px: f64,
    status: String,
    pub(crate) message: Option<String>,
}

impl Default for ToolBox {
    fn default() -> Self {
        ToolBox {
            scenes: Mutex::default(),
            active: ToolKind::Select,
            dimension_start: None,
            dimension_draft: None,
            current_view: None,
            axes: Default::default(),
            state: State::Idle,
            inference: None,
            override_pick: None,
            annotation_box: None,
            preview_copy: false,
            axis_lock: None,
            plane_lock: None,
            shift_lock: None,
            glue_plane: None,
            hints: Vec::new(),
            press: None,
            last: Last::None,
            correction_scope: None,
            pending_resize: None,
            tape_measure_only: false,
            last_distance: None,
            last_offset: None,
            circle_segments: 24,
            polygon_segments: 6,
            arc_segments: 12,
            paint: None,
            paint_surface: true,
            paint_placement: None,
            cursor: DVec2::ZERO,
            px: 0.01,
            status: String::new(),
            message: None,
        }
    }
}

/// Other geometry and the guides of the context, for inference and picking. Building
/// one walks every occurrence, so it stays until the document or context changes.
pub(crate) struct Scene {
    pub mesh: rysmo_mesh::Mesh,
    pub clips: Vec<Plane>,
    pub others: Vec<Other<'static>>,
    pub accel: Accel,
    pub guides: Vec<(GuideId, SnapGuide)>,
}

struct KnownScene {
    revision: u64,
    root: Arc<rysmo_model::Definition>,
    context: Vec<InstanceId>,
    scene: Arc<Scene>,
}

/// The current scene with what identifies it, and the trees of each definition's mesh.
/// Holding the definition keeps the pointer from being reused and any edit from
/// happening in place, so an equal pointer means the same geometry.
#[derive(Default)]
pub(crate) struct SceneCache {
    scene: Option<KnownScene>,
    indices: HashMap<Option<DefinitionId>, (Arc<rysmo_model::Definition>, Arc<MeshIndex>)>,
}

impl SceneCache {
    fn index(
        &mut self,
        doc: &rysmo_model::Document,
        owner: Option<DefinitionId>,
        mesh: &rysmo_mesh::Mesh,
        evaluated: bool,
    ) -> Arc<MeshIndex> {
        let definition = doc.def_arc(owner).expect("an occurrence's definition");
        let index = match self.indices.get(&owner) {
            Some((known, index)) if Arc::ptr_eq(known, definition) && !evaluated => return index.clone(),
            Some((_, index)) => Arc::new(index.update(mesh)),
            None => Arc::new(MeshIndex::new(mesh)),
        };
        self.indices.insert(owner, (definition.clone(), index.clone()));
        index
    }
}

pub(crate) fn scene(cache: &Mutex<SceneCache>, editor: &Editor) -> Arc<Scene> {
    let doc = editor.doc();
    let context = editor.context();
    let mut cache = cache.lock().expect("no panic while the cache is held");
    if let Some(known) = &cache.scene
        && known.revision == editor.content_revision()
        && Arc::ptr_eq(&known.root, &doc.root)
        && known.context == context
    {
        return known.scene.clone();
    }
    let to_local = editor.context_transform().inverse();
    let mut others = Vec::new();
    let mut placed = Vec::new();
    let mut evaluated: HashMap<Option<DefinitionId>, (rysmo_mesh::Mesh, Arc<MeshIndex>)> = HashMap::new();
    let mut active_seen = false;
    for occ in doc.occurrences(context) {
        // The first occurrence inside the context is the context itself.
        if occ.in_context && !active_seen {
            active_seen = true;
            continue;
        }
        if let Some(_def) = doc.def(occ.definition) {
            let (mesh, index) = evaluated.entry(occ.definition).or_insert_with(|| {
                let mesh = doc.effective_mesh(occ.definition);
                let index = cache.index(doc, occ.definition, &mesh, matches!(mesh, Cow::Owned(_)));
                (mesh.into_owned(), index)
            });
            let placement = to_local * occ.transform;
            placed.push((index.clone(), placement));
            others.push(Other {
                clips: doc
                    .section_planes(&occ.path)
                    .into_iter()
                    .map(|p| p.transformed(to_local))
                    .map(|p| Plane::new(p.normal, p.origin))
                    .collect(),
                mesh: Cow::Owned(mesh.clone()),
                to_local: placement,
                instance: occ.top,
            });
        }
    }
    let guides = editor
        .active()
        .guides
        .iter()
        .filter(|(_, g)| doc.style.show_guides && (!g.hidden() || doc.style.show_hidden_geometry))
        .map(|(id, g)| {
            let sg = match g {
                Guide::Line { origin, dir, .. } => SnapGuide::Line { origin: *origin, dir: *dir },
                Guide::Point { p, origin: Some(start), .. } => SnapGuide::Segment { start: *start, point: *p },
                Guide::Point { p, .. } => SnapGuide::Point(*p),
            };
            (*id, sg)
        })
        .collect();
    let active = doc.effective_mesh(editor.active_definition());
    let index = cache.index(doc, editor.active_definition(), &active, matches!(active, Cow::Owned(_)));
    let scene = Arc::new(Scene {
        mesh: active.into_owned(),
        others,
        accel: Accel::new(index, placed),
        guides,
        clips: doc
            .section_planes(context)
            .into_iter()
            .map(|p| p.transformed(to_local))
            .map(|p| Plane::new(p.normal, p.origin))
            .collect(),
    });
    cache.scene = Some(KnownScene {
        revision: editor.content_revision(),
        root: doc.root.clone(),
        context: context.to_vec(),
        scene: scene.clone(),
    });
    cache.indices.retain(|owner, _| doc.def(*owner).is_some());
    scene
}

impl ToolBox {
    pub fn new() -> Self {
        let mut t = Self::default();
        t.update_status();
        t
    }

    pub fn inferred_anchor(&self, editor: &Editor) -> Option<rysmo_model::dimensions::DimensionAnchor> {
        self.inference.map(|inf| dimension::anchor(editor, inf))
    }
    pub fn active(&self) -> ToolKind {
        self.active
    }

    pub fn set_tool(&mut self, kind: ToolKind) {
        self.active = kind;
        self.tape_measure_only = false;
        self.reset();
        self.last = Last::None;
    }

    pub fn pending_resize_factor(&self) -> Option<f64> {
        self.pending_resize.map(|(factor, _)| factor)
    }

    pub fn confirm_resize(&mut self, editor: &mut Editor, accept: bool) -> Result<Option<f64>, String> {
        let pending = self.pending_resize.take();
        if !accept {
            return Ok(None);
        }
        let Some((factor, revision)) = pending else { return Ok(None) };
        if revision != editor.revision() {
            return Err("The model changed. Measure again before resizing.".into());
        }
        editor.apply(Command::ResizeContext { factor }).map_err(|e| e.to_string())?;
        self.last = if let Last::TapeMeasurement { length, .. } = self.last {
            Last::TapeMeasurement { length: length * factor, revision: editor.revision() }
        } else {
            Last::None
        };
        self.message = Some("Model resized.".into());
        Ok(Some(factor))
    }

    /// Whether an operation is under way (Escape would cancel it).
    pub fn busy(&self) -> bool {
        self.state != State::Idle || self.dimension_start.is_some() || self.dimension_draft.is_some()
    }

    pub(crate) fn reset(&mut self) {
        self.pending_resize = None;
        self.dimension_start = None;
        self.dimension_draft = None;
        self.state = State::Idle;
        self.axis_lock = None;
        self.plane_lock = None;
        self.shift_lock = None;
        self.glue_plane = None;
        self.press = None;
        self.update_status();
    }

    /// The status bar text: what the tool wants next, or the last error.
    pub fn status(&self) -> &str {
        self.message.as_deref().unwrap_or(&self.status)
    }

    pub fn polygon_segments(&self) -> u32 {
        self.polygon_segments
    }

    pub(crate) fn radial_segments(&self) -> u32 {
        if self.active == ToolKind::Polygon { self.polygon_segments } else { self.circle_segments }
    }

    pub fn circle_segments(&self) -> u32 {
        self.circle_segments
    }

    /// Change curve tessellation without committing the active draft.
    pub fn set_curve_segments(&mut self, value: u32) {
        let value = value.clamp(3, 360);
        match self.active {
            ToolKind::Circle => self.circle_segments = value,
            ToolKind::Polygon => self.polygon_segments = value,
            _ => self.arc_segments = value,
        }
    }

    pub fn arc_segments(&self) -> u32 {
        self.arc_segments
    }

    /// The Paint Bucket's current material.
    pub fn paint(&self) -> Option<MaterialId> {
        self.paint
    }

    pub fn paint_surface(&self) -> bool {
        self.paint_surface
    }
    pub fn set_paint_surface(&mut self, value: bool) {
        self.paint_surface = value;
    }

    pub fn set_paint(&mut self, material: Option<MaterialId>) {
        self.paint = material;
        self.paint_placement = None;
    }

    pub(crate) fn update_status(&mut self) {
        self.status = match (&self.active, &self.state) {
            (ToolKind::Select, _) => {
                "Select objects. Shift to toggle, Ctrl to add, Shift+Ctrl to remove. Drag for a box. Double-click a group to edit it."
            }
            (ToolKind::Eraser, _) => "Click or drag over edges and objects to erase them. Ctrl softens, Shift hides, Ctrl+Shift unsoftens.",
            (ToolKind::Line, State::Idle) => "Select start point.",
            (ToolKind::Line, _) => "Select end point or enter a length. Shift locks the inference, arrows lock an axis.",
            (ToolKind::Axes, State::Idle) => "Select the drawing origin.",
            (ToolKind::Axes, State::AxesRed { .. }) => "Select a point along the red axis.",
            (ToolKind::Axes, _) => "Select the green direction. Blue is perpendicular; Escape cancels.",
            (ToolKind::Freehand, State::Idle) => "Press and drag to draw a planar curve; release to finish.",
            (ToolKind::Freehand, _) => "Drag to draw. Return to the start to close the curve. Escape cancels.",
            (ToolKind::Protractor, State::Idle) => "Select the angle vertex. Arrow keys lock the plane.",
            (ToolKind::Protractor, State::AngleBase { .. }) => "Select the start of the angle.",
            (ToolKind::Protractor, _) => "Select the angle or type degrees. Ctrl toggles an angled guide.",
            (ToolKind::ThreePointArc,State::Idle) => "Select arc start. Arrows choose the plane; type 24s for segments.",
            (ToolKind::ThreePointArc,State::ThreeMiddle {..}) => "Select a point on the arc, or enter its distance from the start.",
            (ToolKind::ThreePointArc,_) => "Select the endpoint. After a valid preview, type signed total degrees. Shift holds direction; Escape cancels.",
            (ToolKind::CenterArc | ToolKind::Pie, State::Idle) => "Select centre. Arrows choose the plane; type 24s for segments.",
            (ToolKind::CenterArc | ToolKind::Pie, State::CentreRadius {..}) => "Select radius and start direction, or type a radius. Shift holds direction.",
            (ToolKind::CenterArc | ToolKind::Pie, _) => "Select arc end or enter signed degrees. Shift holds angle; 500mmr adjusts radius; Escape cancels.",
            (ToolKind::Arc, State::Idle) => "Select start point. Type a number and s for segments.",
            (ToolKind::Arc, State::ArcChord { .. }) => "Select end point or enter the chord length.",
            (ToolKind::Arc, _) => "Select the bulge or enter it (or a radius ending in r).",
            (ToolKind::Rectangle | ToolKind::RotatedRectangle, State::Idle) if matches!(self.last,Last::RotatedRectangle {..}) => "Select first corner, or enter length,width to correct the rectangle.",
            (ToolKind::RotatedRectangle, State::Idle) => "Select first corner. Arrows choose a plane; Down clears it.",
            (ToolKind::RotatedRectangle, State::RotatedEdge { .. }) => "Select the first edge endpoint or enter its length. Shift locks direction.",
            (ToolKind::RotatedRectangle, _) => "Select width and tilt, or type width / length,width. Type 30deg for tilt; Alt-click sets its baseline.",
            (ToolKind::Rectangle, State::Idle) => "Select first corner.",
            (ToolKind::Rectangle, _) => "Select opposite corner or enter dimensions.",
            (ToolKind::Circle | ToolKind::Polygon, State::Idle) => "Select centre point. Type a number and s for sides.",
            (ToolKind::Circle | ToolKind::Polygon, _) => "Select a point on the edge or enter the radius.",
            (ToolKind::PushPull, State::Idle) => {
                "Pick a face. Double-click repeats the last distance. Ctrl keeps the starting face."
            }
            (ToolKind::PushPull, _) => "Pick the distance or enter a value.",
            (ToolKind::Offset, State::Idle) => "Pick a face, or select edges first and click. Double-click repeats.",
            (ToolKind::Offset, _) => "Pick the offset or enter a distance.",
            (ToolKind::FollowMe, State::Idle) => "Click a profile, then trace its path. Preselect edges or a path face for an automatic sweep.",
            (ToolKind::FollowMe, _) => "Trace connected path edges, then click or release to finish. Alt starts automatic surface-boundary mode. Escape cancels.",
            (ToolKind::Move, State::Idle) => "Pick the point to move from. Ctrl copies.",
            (ToolKind::Move, _) => "Pick the point to move to or enter a distance. Ctrl copies.",
            (ToolKind::Rotate, State::Idle) => "Pick the centre. Arrow keys lock the rotation plane. Ctrl copies.",
            (ToolKind::Rotate, State::RotatePivot { .. }) => "Pick the start of the angle.",
            (ToolKind::Rotate, _) => "Pick the angle or enter it in degrees. Ctrl copies.",
            (ToolKind::Scale, State::Idle) => "Pick a grip. Shift toggles uniform scaling; Ctrl scales about the centre. Select something first.",
            (ToolKind::Scale, _) => "Pick the size or enter a factor (2, or 2,3) or a length (2m). Shift toggles uniform; Ctrl toggles centre.",
            (ToolKind::TapeMeasure, State::Idle) => {
                "Pick the start. From an edge it makes a parallel guide line, from a point a guide point. Ctrl: measure only."
            }
            (ToolKind::TapeMeasure, _) => "Pick the end or enter a distance.",
            (ToolKind::PaintBucket, _) => "Click a face or selection to paint. Alt samples, Ctrl fills adjacent matches, Shift replaces all matches, Ctrl+Shift replaces connected matches.",
            (ToolKind::Orbit, _) => "Drag to orbit. Shift pans.",
            (ToolKind::Pan, _) => "Drag to pan.",
            (ToolKind::PositionCamera, _) => "Click to place the camera at eye height; drag to aim. Shift places directly on the surface.",
            (ToolKind::Walk, _) => "Hold and move from the crosshair to walk. Shift raises/lowers, Ctrl runs, Alt crosses walls. Middle button looks around.",
            (ToolKind::LookAround, _) => "Drag to look around. Type eye height in Measurements.",
            (ToolKind::ZoomWindow, _) => "Drag a rectangle to zoom into it. Escape cancels.",
            (ToolKind::Text,_)=>"Click geometry for a leader or empty space for screen text. Double-click a face for its area. Escape cancels.",
            (ToolKind::Dimension,_)=>if self.dimension_draft.is_some() {"Place the dimension line or type its offset. Escape cancels."} else if self.dimension_start.is_some() {"Select the second measured point."} else {"Select a point, edge, circle or arc to dimension."},
            (ToolKind::Zoom, _) => "Drag up or down to zoom. Shift+Z zooms to the whole model.",
        }
        .to_string();
    }

    /// The VCB label.
    pub fn vcb_label(&self) -> &'static str {
        if self.active == ToolKind::Dimension {
            return "Offset";
        }
        if self.active == ToolKind::Scale {
            let mode = match &self.state {
                State::Scaling { grip, uniform, .. } => Some((*grip, *uniform)),
                State::Idle => {
                    if let Last::Scale { grip, uniform, .. } = self.last {
                        Some((grip, uniform))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some((grip, false)) = mode {
                return match (grip.0[0] != 0, grip.0[1] != 0, grip.0[2] != 0) {
                    (true, false, false) => "Red Scale",
                    (false, true, false) => "Green Scale",
                    (false, false, true) => "Blue Scale",
                    (true, true, false) => "Red, Green Scale",
                    (true, false, true) => "Red, Blue Scale",
                    (false, true, true) => "Green, Blue Scale",
                    _ => "Scale",
                };
            }
        }
        match (&self.active, &self.state) {
            (ToolKind::Line, _) | (ToolKind::TapeMeasure, _) => "Length",
            (ToolKind::Move, _) | (ToolKind::PushPull, _) | (ToolKind::Offset, _) => "Distance",
            (ToolKind::RotatedRectangle, State::RotatedEdge { .. }) => "Length",
            (ToolKind::RotatedRectangle, _) => "Width / Dimensions",
            (ToolKind::Rectangle, _) => "Dimensions",
            (ToolKind::Protractor, _) => "Angle",
            (ToolKind::Circle | ToolKind::Polygon, State::Circle { .. }) => "Radius",
            (ToolKind::Circle | ToolKind::Polygon, _) => "Sides",
            (ToolKind::ThreePointArc, State::ThreeMiddle { .. }) => "Length",
            (ToolKind::ThreePointArc, State::ThreeEnd { .. }) => "Angle",
            (ToolKind::ThreePointArc, _) => "Segments / Angle",
            (ToolKind::CenterArc | ToolKind::Pie, State::CentreRadius { .. }) => "Radius",
            (ToolKind::CenterArc | ToolKind::Pie, State::CentreSweep { .. }) => "Angle",
            (ToolKind::CenterArc | ToolKind::Pie, _) => "Segments / Angle",
            (ToolKind::Arc, State::ArcChord { .. }) => "Length",
            (ToolKind::Arc, State::ArcBulge { .. }) => "Bulge",
            (ToolKind::Arc, _) => "Segments",
            (ToolKind::Rotate, _) => "Angle",
            (ToolKind::Scale, _) => "Scale",
            _ => "Measurements",
        }
    }

    /// The value the VCB shows while nothing is typed.
    pub fn vcb_value(&self, editor: &Editor) -> String {
        if let Some(dimension) = &self.dimension_draft {
            return editor.doc().format_length(editor.context_transform().transform_vector3(dimension.offset).length());
        }
        let len = |m: f64| editor.doc().format_length(m);
        let p = self.inference.map(|i| i.point);
        if self.active == ToolKind::TapeMeasure
            && matches!(self.state, State::Idle)
            && let Last::TapeGuide { length, .. } = self.last
        {
            return editor.doc().format_length(length.abs());
        }
        if self.active == ToolKind::TapeMeasure
            && self.state == State::Idle
            && let Last::TapeMeasurement { length, .. } = self.last
        {
            return len(length);
        }
        if self.active == ToolKind::RotatedRectangle
            && self.state == State::Idle
            && let Last::RotatedRectangle { u, v, .. } = self.last
        {
            return format!("{}, {}", len(u.length()), len(v.length()));
        }
        if self.active == ToolKind::ThreePointArc {
            return self.three_value(editor);
        }
        match (&self.state, p) {
            (State::RotatedEdge { corner, .. }, Some(p)) => len(p.distance(*corner)),
            (State::RotatedWidth { corner, u, .. }, Some(p)) => {
                let v = rotated_rectangle::width_vector(*corner, *u, p);
                format!("{}, {}", len(u.length()), len(v.length()))
            }
            (State::Line { start }, Some(p)) => len(p.distance(*start)),
            (State::Rectangle { corner, plane }, Some(p)) => {
                let (u, v) = self.axes.rect_axes(plane);
                let d = p - *corner;
                format!("{}, {}", len(d.dot(u).abs()), len(d.dot(v).abs()))
            }
            (State::Circle { centre, .. }, Some(p)) => len(p.distance(*centre)),
            (State::CentreRadius { centre, .. }, Some(p)) => len(p.distance(*centre)),
            (State::CentreSweep { sweep, .. }, _) => editor.doc().measurement.format_angle(*sweep),
            (State::Idle, _) if matches!(self.active, ToolKind::CenterArc | ToolKind::Pie) => {
                if let Last::CentreArc { sweep, .. } = self.last {
                    editor.doc().measurement.format_angle(sweep)
                } else {
                    self.arc_segments.to_string()
                }
            }
            (State::ArcChord { start, .. }, Some(p)) => len(p.distance(*start)),
            (State::AngleSweep { angle, .. }, _) => editor.doc().measurement.format_angle(*angle),
            (State::ArcBulge { bulge, .. }, _) => len(bulge.abs()),
            (State::Idle, _) if matches!(self.active, ToolKind::Circle | ToolKind::Polygon) => {
                self.radial_segments().to_string()
            }
            (State::Idle, _) if self.active == ToolKind::Arc => self.arc_segments.to_string(),
            (State::PushPull { distance, .. }, _) => len(*distance),
            (State::Move { offset, .. }, _) => len(offset.length()),
            (State::Rotating { angle, .. }, _) => editor.doc().measurement.format_angle(*angle),
            (State::Scaling { factors, grip, .. }, _) => edit::format_factors(*factors, *grip),
            (State::Offsetting { distance, .. }, _) => len(distance.abs()),
            (State::Taping { start, along }, Some(p)) => {
                let d = p - *start;
                len(along
                    .filter(|_| !self.tape_measure_only)
                    .map(|dir| (d - dir * d.dot(dir)).length())
                    .unwrap_or(d.length()))
            }
            _ => String::new(),
        }
    }

    /// What to draw for the tool.
    pub fn preview(&self, editor: &Editor) -> Preview {
        let mut pv =
            Preview { inference: self.inference, dimension: self.dimension_draft.clone(), ..Default::default() };
        self.preview_state(editor, &mut pv);
        self.centre_preview(editor, &mut pv);
        self.three_preview(editor, &mut pv);
        if let State::Selecting { from } = &self.state
            && self.press.is_some()
            && from.distance(self.cursor) > 3.0
        {
            pv.select_box = Some((*from, self.cursor, self.cursor.x < from.x));
        }
        if let Some((a, b, kind)) = self.inference.and_then(|i| i.guide)
            && (a.distance(b) > EPS && !matches!(self.state, State::Idle) || matches!(kind, Kind::FromPoint(_)))
        {
            pv.lines.push(PreviewLine { a, b, color: kind.color(), dashed: true, thick: false });
        }
        pv
    }

    // ----- events -----

    /// Handles a pointer event at `ctx.cursor`.
    /// Supplies a hit from a rendered annotation while retaining normal pointer
    /// bookkeeping and the Select tool's modifier/context behaviour.
    pub fn pointer_with_pick(&mut self, editor: &mut Editor, ctx: &Ctx, event: Pointer, pick: rysmo_infer::Pick) {
        self.override_pick = Some(pick);
        self.pointer(editor, ctx, event);
        self.override_pick = None;
    }

    /// Applies measured annotation bounds in the same selection transaction as geometry.
    pub fn pointer_with_box_hits(&mut self, editor: &mut Editor, ctx: &Ctx, event: Pointer, hits: AnnotationBoxHits) {
        self.annotation_box = Some(hits);
        self.pointer(editor, ctx, event);
        self.annotation_box = None;
    }

    pub fn pointer(&mut self, editor: &mut Editor, ctx: &Ctx, ev: Pointer) {
        if self.three_stale(editor) || self.centre_stale(editor) || self.rotated_stale(editor) {
            return;
        }
        if let State::Following { revision, context, .. } = &self.state
            && (*revision != editor.revision() || context != editor.context())
        {
            self.reset();
            self.message = Some("The model changed; start a new Follow Me operation.".into());
            return;
        }
        self.current_view = Some(*ctx.view);
        self.tape_measure_only = ctx.modifiers.ctrl;
        self.axes = editor.active().axes;
        self.scale_modifiers(ctx);
        self.cursor = ctx.cursor;
        if matches!(ev, Pointer::Down { .. }) {
            self.message = None;
        }
        self.refresh_inference(editor, ctx);
        if matches!(self.state, State::Scaling { .. }) {
            self.scale_move();
        }
        if let Some(p) = self.anchor().or(self.inference.map(|i| i.point)) {
            self.px = ctx.view.pixel_size_at(p);
        }
        match ev {
            Pointer::Move => self.on_move(editor, ctx),
            Pointer::Down { clicks } => {
                self.press = Some(ctx.cursor);
                self.on_click(editor, ctx, clicks);
            }
            Pointer::Up => {
                let dragged = self.press.is_some_and(|p| p.distance(ctx.cursor) > 6.0 * ctx.scale);
                self.on_release(editor, ctx, dragged);
                self.press = None;
            }
        }
        self.update_status();
    }

    pub fn key(&mut self, editor: &mut Editor, ctx: &Ctx, key: Key) {
        self.current_view = Some(*ctx.view);
        self.tape_measure_only = ctx.modifiers.ctrl;
        self.axes = editor.active().axes;
        self.scale_modifiers(ctx);
        let anchor = self.anchor();
        match key {
            Key::Escape => {
                self.pending_resize = None;
                self.last = Last::None;
                if self.busy() {
                    self.reset();
                } else if self.active == ToolKind::Select || !editor.context().is_empty() {
                    // Escape with nothing under way closes the group being edited.
                    editor.exit();
                }
                self.message = None;
            }
            Key::Right | Key::Left | Key::Up => {
                let axis = match key {
                    Key::Right => Axis::X,
                    Key::Left => Axis::Y,
                    _ => Axis::Z,
                };
                let plane_tool =
                    matches!(self.active, ToolKind::Rectangle | ToolKind::Circle | ToolKind::Polygon | ToolKind::Arc);
                match &mut self.state {
                    State::Idle if plane_tool => {
                        self.plane_lock = if self.plane_lock == Some(axis) { None } else { Some(axis) };
                    }
                    State::Rectangle { corner: anchor, plane }
                    | State::Circle { centre: anchor, plane }
                    | State::ArcChord { start: anchor, plane }
                        if plane_tool =>
                    {
                        self.plane_lock = Some(axis);
                        *plane = Plane::new(self.axes.dir(axis), *anchor);
                    }
                    State::Idle | State::CentreRadius { .. }
                        if matches!(self.active, ToolKind::CenterArc | ToolKind::Pie) =>
                    {
                        self.axis_lock = if self.axis_lock == Some(axis) { None } else { Some(axis) };
                        if let State::CentreRadius { centre, plane, .. } = &mut self.state {
                            *plane = Plane::new(self.axis_lock.map_or(self.axes.z(), |a| self.axes.dir(a)), *centre);
                        }
                    }
                    State::Idle if self.active == ToolKind::ThreePointArc => {
                        self.axis_lock = if self.axis_lock == Some(axis) { None } else { Some(axis) };
                    }
                    State::Idle if self.active == ToolKind::RotatedRectangle => {
                        self.axis_lock = if self.axis_lock == Some(axis) { None } else { Some(axis) };
                    }
                    // Rotate: the arrows pick the protractor's plane.
                    State::Idle | State::RotatePivot { .. } if self.active == ToolKind::Rotate => {
                        self.axis_lock = if self.axis_lock == Some(axis) { None } else { Some(axis) };
                        if let State::RotatePivot { axis: a, .. } = &mut self.state {
                            *a = self.axes.dir(axis);
                        }
                    }
                    _ if self.active == ToolKind::Protractor => {
                        self.axis_lock = if self.axis_lock == Some(axis) { None } else { Some(axis) };
                        if let State::AngleBase { centre, plane } = &mut self.state {
                            *plane = Plane::new(self.axes.dir(axis), *centre);
                        }
                    }
                    State::CentreSweep { .. } => {}
                    State::RotatedWidth { angle_lock, .. } => {
                        *angle_lock = None;
                        self.shift_lock = None;
                        self.axis_lock = if self.axis_lock == Some(axis) { None } else { Some(axis) };
                    }
                    _ if anchor.is_some() => {
                        self.axis_lock = if self.axis_lock == Some(axis) { None } else { Some(axis) };
                    }
                    _ => {}
                }
            }
            Key::Down => {
                self.axis_lock = None;
                self.plane_lock = None;
                if let State::RotatedEdge { plane, .. } | State::RotatedWidth { plane, .. } = &mut self.state {
                    *plane = None;
                }
                if let State::RotatedWidth { angle_lock, .. } = &mut self.state {
                    *angle_lock = None;
                }
                self.shift_lock = None;
            }
            Key::Shift(true) => {
                if matches!(self.state, State::ThreeMiddle { .. } | State::ThreeEnd { .. }) {
                    self.shift_lock = self.anchor().and_then(|origin| {
                        self.inference.and_then(|i| (i.point - origin).try_normalize()).map(|dir| Lock::Line {
                            origin,
                            dir,
                            axis: None,
                        })
                    });
                } else if let State::CentreSweep { centre, .. } = self.state {
                    self.shift_lock = self
                        .inference
                        .and_then(|i| (i.point - centre).try_normalize())
                        .map(|dir| Lock::Line { origin: centre, dir, axis: None });
                } else if let State::RotatedWidth { corner, u, .. } = self.state {
                    self.shift_lock = self
                        .inference
                        .and_then(|i| rotated_rectangle::width_vector(corner, u, i.point).try_normalize())
                        .map(|dir| Lock::Line { origin: corner, dir, axis: None });
                } else if self.active == ToolKind::Scale {
                    self.shift_lock = None;
                } else if self.shift_lock.is_none() {
                    self.shift_lock = self.inference.and_then(|i| i.lock_for(anchor, editor.mesh()));
                }
            }
            Key::Shift(false) => self.shift_lock = None,
        }
        self.refresh_inference(editor, ctx);
        self.on_move(editor, ctx);
        self.update_status();
    }

    /// Release a transient modifier when a UI field owns keyboard input, without
    /// sampling the cursor or changing an unfinished tool draft.
    pub fn release_shift_lock(&mut self) {
        self.shift_lock = None;
    }

    /// Applies typed VCB text. Returns an error message for the status bar.
    pub fn vcb_enter(&mut self, editor: &mut Editor, ctx: &Ctx, text: &str) -> Result<(), String> {
        self.current_view = Some(*ctx.view);
        self.tape_measure_only = ctx.modifiers.ctrl;
        self.axes = editor.active().axes;
        self.scale_modifiers(ctx);
        if self.active == ToolKind::Dimension {
            return self.dimension_vcb(editor, text);
        }
        if self.active == ToolKind::ThreePointArc {
            return self.three_vcb(editor, ctx, text);
        }
        if matches!(self.active, ToolKind::CenterArc | ToolKind::Pie) {
            return self.centre_vcb(editor, ctx, text);
        }
        if self.active == ToolKind::RotatedRectangle {
            return self.rotated_vcb(editor, ctx, text);
        }
        let units = editor.doc().units;
        if self.active == ToolKind::TapeMeasure
            && matches!(self.state, State::Idle)
            && matches!(self.last, Last::TapeGuide { .. })
        {
            let length = rysmo_model::parse_length(text, units).map_err(|e| e.to_string())?;
            return self.amend_last(editor, VcbInput::Lengths(vec![length]));
        }
        if self.active == ToolKind::TapeMeasure
            && matches!(self.state, State::Idle)
            && let Last::TapeMeasurement { length, revision } = self.last
        {
            if revision != editor.revision() {
                self.last = Last::None;
                return Err("The model changed. Measure again before resizing.".into());
            }
            let desired = rysmo_model::parse_length(text, units).map_err(|e| e.to_string())?;
            if !desired.is_finite() || desired <= EPS || length <= EPS {
                return Err("Enter a positive measured length.".into());
            }
            self.pending_resize = Some((desired / length, revision));
            return Ok(());
        }
        if self.active == ToolKind::Protractor {
            return self.protractor_vcb(editor, ctx, text);
        }
        // Rotate and Scale read plain numbers, not lengths.
        if self.active == ToolKind::Rotate {
            return self.rotate_vcb(editor, ctx, text);
        }
        if self.active == ToolKind::Scale {
            return self.scale_vcb(editor, text);
        }
        let text = if matches!(self.active, ToolKind::Circle | ToolKind::Polygon)
            && (matches!(self.state, State::Circle { .. }) || matches!(self.last, Last::Radial { .. }))
        {
            text.trim().trim_end_matches(['r', 'R'])
        } else {
            text
        };
        let wanted = if matches!(self.state, State::Rectangle { .. })
            || (self.state == State::Idle && matches!(self.last, Last::RotatedRectangle { .. }))
        {
            2
        } else {
            1
        };
        // Arc: a trailing r means a radius.
        if let (State::ArcBulge { start, end, plane, .. }, Some(r)) =
            (self.state.clone(), text.trim().strip_suffix(['r', 'R']))
        {
            let r = rysmo_model::parse_length(r, units).map_err(|e| e.to_string())?;
            return self.arc_radius(editor, start, end, plane, r);
        }
        let input = parse_vcb(text, units, wanted)?;
        let first =
            |v: &[f64]| v.first().copied().filter(|x| x.is_finite()).ok_or_else(|| "Type a length.".to_string());
        if matches!(self.state, State::Idle) && matches!(self.last, Last::Radial { .. } | Last::Arc { .. }) {
            self.amend_last(editor, input)?;
            self.update_status();
            return Ok(());
        }
        match (self.state.clone(), input) {
            (State::Line { start }, VcbInput::Lengths(v)) => {
                let len = first(&v)?;
                let dir = self.current_dir(start).ok_or("Move the cursor to show the direction first.")?;
                self.finish_line(editor, ctx, start, start + dir * len);
            }
            (State::Rectangle { corner, plane }, VcbInput::Lengths(v)) => {
                let (u, v_axis) = self.axes.rect_axes(&plane);
                let d = self.inference.map(|i| i.point - corner).unwrap_or(u + v_axis);
                let sign = |x: f64| if x < 0.0 { -1.0 } else { 1.0 };
                let w = v.first().copied().filter(|x| x.is_finite()).unwrap_or(d.dot(u).abs());
                let h = v.get(1).copied().filter(|x| x.is_finite()).unwrap_or(d.dot(v_axis).abs());
                self.finish_rectangle(
                    editor,
                    ctx,
                    corner,
                    plane,
                    u * w * sign(d.dot(u)),
                    v_axis * h * sign(d.dot(v_axis)),
                );
            }
            (State::Circle { centre, plane }, VcbInput::Lengths(v)) => {
                let r = first(&v)?;
                if r <= EPS {
                    return Err("Enter a positive radius.".into());
                }
                let towards = self.inference.map(|i| i.point - centre).unwrap_or(DVec3::X);
                self.finish_circle(editor, centre, plane, r, towards);
            }
            (_, VcbInput::Segments(n)) if self.active == ToolKind::Circle => self.circle_segments = n,
            (_, VcbInput::Segments(n)) if self.active == ToolKind::Polygon => self.polygon_segments = n,
            (_, VcbInput::Segments(n)) if self.active == ToolKind::Arc => self.arc_segments = n.max(1),
            (State::Idle, VcbInput::Lengths(v))
                if matches!(self.active, ToolKind::Circle | ToolKind::Polygon | ToolKind::Arc) =>
            {
                let n = first(&v)? * editor.doc().units.per_metre();
                if n.fract().abs() > 1e-9 || n < 3.0 {
                    return Err("Enter the number of sides (at least 3).".into());
                }
                if self.active == ToolKind::Circle {
                    self.circle_segments = n as u32;
                } else if self.active == ToolKind::Polygon {
                    self.polygon_segments = n as u32;
                } else {
                    self.arc_segments = n as u32;
                }
            }
            (State::ArcChord { start, plane }, VcbInput::Lengths(v)) => {
                let len = first(&v)?;
                let dir = self.current_dir(start).ok_or("Move the cursor to show the direction first.")?;
                let end = plane.project(start + dir * len);
                self.state = State::ArcBulge { start, end, plane, bulge: 0.0 };
            }
            (State::ArcBulge { start, end, plane, bulge }, VcbInput::Lengths(v)) => {
                let b = first(&v)?;
                let b = if bulge < 0.0 { -b.abs() } else { b.abs() };
                self.finish_arc(editor, start, end, plane, b);
            }
            (State::PushPull { face, distance, .. }, VcbInput::Lengths(v)) => {
                let d = first(&v)?;
                let d = if distance < 0.0 { -d.abs() } else { d };
                self.finish_push_pull(editor, ctx, face, d);
            }
            (State::Move { elements, offset, start }, VcbInput::Lengths(v)) => {
                let len = first(&v)?;
                let dir = if offset.length() > EPS {
                    offset.normalize()
                } else {
                    self.current_dir(start).ok_or("Move the cursor to show the direction first.")?
                };
                self.finish_move(editor, ctx, elements, dir * len);
            }
            (State::Offsetting { target, edges, distance }, VcbInput::Lengths(v)) => {
                let d = first(&v)?;
                let d = if distance < 0.0 { -d.abs() } else { d.abs() };
                self.finish_offset(editor, target, edges, d);
            }
            (State::Taping { start, along }, VcbInput::Lengths(v)) => {
                let len = first(&v)?;
                self.tape_value(editor, ctx, start, along, len)?;
            }
            // Corrections of the operation just finished.
            (State::Idle, input) => self.amend_last(editor, input)?,
            _ => return Err("This value does not apply here.".into()),
        }
        self.update_status();
        Ok(())
    }

    // ----- internals -----

    pub(crate) fn anchor(&self) -> Option<DVec3> {
        if let Some(start) = &self.dimension_start {
            return Some(start.point);
        }
        if let Some(dimension) = &self.dimension_draft {
            return Some(dimension::origin(dimension));
        }
        match &self.state {
            State::ThreeMiddle { start, .. } => Some(*start),
            State::ThreeEnd { through, .. } => Some(*through),
            State::Line { start } | State::ArcChord { start, .. } | State::Taping { start, .. } => Some(*start),
            State::Rectangle { corner, .. }
            | State::RotatedEdge { corner, .. }
            | State::RotatedWidth { corner, .. } => Some(*corner),
            State::AxesRed { origin } | State::AxesGreen { origin, .. } => Some(*origin),
            State::CentreRadius { centre, .. }
            | State::CentreSweep { centre, .. }
            | State::AngleBase { centre, .. }
            | State::AngleSweep { centre, .. }
            | State::Circle { centre, .. }
            | State::RotatePivot { centre, .. }
            | State::Rotating { centre, .. } => Some(*centre),
            State::PushPull { start, .. } | State::Move { start, .. } => Some(*start),
            _ => None,
        }
    }

    fn lock(&self) -> Option<Lock> {
        if let Some(lock) = self.three_lock() {
            return Some(lock);
        }
        if self.active == ToolKind::RotatedRectangle
            && let Some(lock) = self.rotated_lock()
        {
            return Some(lock);
        }
        if let Some(dimension) = &self.dimension_draft {
            return Some(Lock::Plane(Plane::new(dimension.normal, dimension::origin(dimension))));
        }
        if let (Some(plane), State::Move { start, .. }) = (self.glue_plane, &self.state) {
            let direction = match self.shift_lock {
                Some(Lock::Line { dir, .. }) => Some(dir),
                _ => self.axis_lock.map(|axis| self.axes.dir(axis)),
            };
            if let Some(direction) = direction
                && let Some(dir) = (direction - plane.normal * direction.dot(plane.normal)).try_normalize()
            {
                return Some(Lock::Line { origin: *start, dir, axis: None });
            }
            return Some(Lock::Plane(plane));
        }
        if let Some(l) = self.shift_lock {
            return Some(l);
        }
        match &self.state {
            State::CentreRadius { plane, .. }
            | State::CentreSweep { plane, .. }
            | State::Freehand { plane, .. }
            | State::AngleBase { plane, .. }
            | State::AngleSweep { plane, .. } => Some(Lock::Plane(*plane)),
            State::PushPull { start, normal, .. } => Some(Lock::Line { origin: *start, dir: *normal, axis: None }),
            State::RotatePivot { centre, axis, .. } | State::Rotating { centre, axis, .. } => {
                Some(Lock::Plane(Plane::new(*axis, *centre)))
            }
            State::Scaling { fixed, handle, uniform, drag_plane, grip, .. } => {
                if !uniform && grip.0.iter().filter(|v| **v != 0).count() > 1 {
                    return Some(Lock::Plane(*drag_plane));
                }
                let d = *handle - *fixed;
                (d.length() > EPS).then(|| Lock::Line { origin: *fixed, dir: d.normalize(), axis: None })
            }
            State::Rectangle { .. } | State::Circle { .. } | State::ArcBulge { .. } => None,
            _ => {
                if self.active == ToolKind::Rotate {
                    return None;
                }
                let o = self.anchor()?;
                self.axis_lock.map(|a| Lock::Line { origin: o, dir: self.axes.dir(a), axis: Some(a) })
            }
        }
    }

    /// Inference for staged app tools without starting a modelling operation.
    pub fn draft_inference(
        &mut self,
        editor: &Editor,
        ctx: &Ctx,
        from: Option<DVec3>,
        lock: Option<Lock>,
    ) -> Inference {
        self.axes = editor.active().axes;
        self.current_view = Some(*ctx.view);
        self.tape_measure_only = ctx.modifiers.ctrl;
        let sc = scene(&self.scenes, editor);
        let result = infer(&Query {
            show_hidden: editor.doc().style.show_hidden_geometry,
            clips: sc.clips.clone(),
            axes: self.axes,
            mesh: &sc.mesh,
            view: ctx.view,
            cursor: ctx.cursor,
            from,
            lock,
            hints: &self.hints,
            scale: ctx.scale,
            others: &sc.others,
            accel: Some(&sc.accel),
            guides: &sc.guides,
        });
        let result = if let Some(origin) = from.filter(|_| {
            matches!(result.kind, Kind::Free | Kind::Ground | Kind::OnFace | Kind::OnAxis(_) | Kind::FromPoint(_))
        }) {
            let delta = result.point - origin;
            let point = delta
                .try_normalize()
                .map_or(result.point, |dir| origin + dir * editor.doc().measurement.snap_length(delta.length()));
            Inference { point, ..result }
        } else {
            result
        };
        self.inference = Some(result);
        result
    }

    pub(crate) fn refresh_inference(&mut self, editor: &Editor, ctx: &Ctx) {
        if matches!(
            self.active,
            ToolKind::Orbit
                | ToolKind::Pan
                | ToolKind::Zoom
                | ToolKind::ZoomWindow
                | ToolKind::LookAround
                | ToolKind::Walk
        ) {
            self.inference = None;
            return;
        }
        if !self.busy()
            && let Some(rysmo_infer::Pick::Element(Element::Section(id))) = self.override_pick
            && let Some(plane) = editor.active().sections.planes.get(&id)
        {
            let ray = ctx.view.ray(ctx.cursor);
            let point = rysmo_model::sections::glyph(plane, editor.doc().definition_bounds(editor.active_definition()))
                .into_iter()
                .filter_map(|[a, b]| {
                    let (sa, sb) = ctx.view.project_segment(a, b)?;
                    let distance = rysmo_math::distance_point_segment_2d(ctx.cursor, sa, sb);
                    let t = rysmo_math::closest_line_ray(a, b - a, &ray).map(|(t, _)| t.clamp(0., 1.)).unwrap_or(0.);
                    Some((distance, a + (b - a) * t))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, p)| p)
                .unwrap_or(plane.origin);
            self.inference = Some(Inference {
                point,
                kind: Kind::Free,
                edge: None,
                curve: None,
                face: None,
                plane: Some(Plane::new(plane.normal, plane.origin)),
                guide: None,
                instance: None,
                construction: None,
            });
            return;
        }
        if matches!(
            self.state,
            State::RotatedWidth { .. }
                | State::CentreRadius { .. }
                | State::CentreSweep { .. }
                | State::ThreeMiddle { .. }
                | State::ThreeEnd { .. }
        ) && let Some(Lock::Plane(plane)) = self.lock()
            && plane
                .intersect_ray(&ctx.view.ray(ctx.cursor))
                .or_else(|| plane.flipped().intersect_ray(&ctx.view.ray(ctx.cursor)))
                .is_none()
        {
            self.inference = None;
            return;
        }
        let sc = scene(&self.scenes, editor);
        let q = Query {
            show_hidden: editor.doc().style.show_hidden_geometry,
            clips: sc.clips.clone(),
            axes: editor.active().axes,
            mesh: &sc.mesh,
            view: ctx.view,
            cursor: ctx.cursor,
            from: self.anchor(),
            lock: self.lock(),
            hints: &self.hints,
            scale: ctx.scale,
            others: &sc.others,
            accel: Some(&sc.accel),
            guides: &sc.guides,
        };
        let mut inf = infer(&q);
        if self.shift_lock.is_some()
            && inf.kind == Kind::Free
            && let Some(kind @ (Kind::Parallel | Kind::Perpendicular | Kind::Tangent)) = self.inference.map(|i| i.kind)
        {
            inf.kind = kind;
            inf.guide = inf.guide.map(|(a, b, _)| (a, b, kind));
        }
        // Rectangles, circles and arcs stay in their plane: a snapped point is projected
        // into it, anything else is where the cursor ray meets the plane.
        let inf = match (&self.state, self.shift_lock) {
            (
                State::ThreeMiddle { plane, .. }
                | State::ThreeEnd { plane, .. }
                | State::CentreRadius { plane, .. }
                | State::CentreSweep { plane, .. }
                | State::Freehand { plane, .. }
                | State::Rectangle { plane, .. }
                | State::Circle { plane, .. }
                | State::AngleBase { plane, .. }
                | State::AngleSweep { plane, .. }
                | State::ArcChord { plane, .. }
                | State::ArcBulge { plane, .. },
                None,
            ) => {
                let snapped = matches!(
                    inf.kind,
                    Kind::Endpoint
                        | Kind::Midpoint
                        | Kind::OnEdge
                        | Kind::Origin
                        | Kind::Intersection
                        | Kind::GuidePoint
                        | Kind::OnGuide
                ) || matches!(
                    inf.kind,
                    Kind::OnAxis(_) | Kind::FromPoint(_) | Kind::Parallel | Kind::Perpendicular | Kind::Tangent
                );
                let ray = ctx.view.ray(ctx.cursor);
                let hit = plane.intersect_ray(&ray).or_else(|| plane.flipped().intersect_ray(&ray)).map(|t| ray.at(t));
                match (snapped, hit) {
                    (false, Some(p)) => {
                        let on_plane = inf.plane.is_some_and(|pl| pl.same_surface(plane));
                        Inference { point: p, kind: if on_plane { inf.kind } else { Kind::Free }, ..inf }
                    }
                    _ => Inference { point: plane.project(inf.point), ..inf },
                }
            }
            _ => inf,
        };
        if matches!(inf.kind, Kind::Endpoint | Kind::Midpoint)
            && !self.hints.iter().any(|h| h.distance(inf.point) < EPS)
        {
            self.hints.push(inf.point);
            if self.hints.len() > 3 {
                self.hints.remove(0);
            }
        }
        let inf = self.snap_length_inference(editor, inf);
        let inf = if self.active == ToolKind::RotatedRectangle { self.rotated_snap(editor, ctx, inf) } else { inf };
        let inf = if let State::Rectangle { corner, plane } = self.state {
            if self.shift_lock.is_none() && matches!(inf.kind, Kind::Free | Kind::Ground | Kind::OnFace) {
                Inference { point: self.snap_rectangle(ctx, corner, plane, inf.point), ..inf }
            } else {
                inf
            }
        } else {
            inf
        };
        self.inference = Some(inf);
    }

    fn snap_length_inference(&self, editor: &Editor, inf: Inference) -> Inference {
        let settings = editor.doc().measurement;
        if matches!(
            self.active,
            ToolKind::RotatedRectangle | ToolKind::CenterArc | ToolKind::Pie | ToolKind::ThreePointArc
        ) && inf.guide.is_some()
        {
            return inf;
        }
        if settings.length_snap.is_none()
            || !matches!(inf.kind, Kind::Free | Kind::Ground | Kind::OnFace | Kind::OnAxis(_) | Kind::FromPoint(_))
        {
            return inf;
        }
        let snap_vector = |d: DVec3| d.try_normalize().map_or(d, |dir| dir * settings.snap_length(d.length()));
        let point = match self.state {
            State::Line { start } | State::ArcChord { start, .. } | State::Move { start, .. } => {
                start + snap_vector(inf.point - start)
            }
            State::RotatedEdge { corner, .. } => corner + snap_vector(inf.point - corner),
            State::RotatedWidth { corner, u, .. } => {
                corner + snap_vector(rotated_rectangle::width_vector(corner, u, inf.point))
            }
            State::ThreeMiddle { start: centre, .. }
            | State::CentreRadius { centre, .. }
            | State::Circle { centre, .. } => centre + snap_vector(inf.point - centre),
            State::Rectangle { corner, plane } => {
                let (u, v) = self.axes.rect_axes(&plane);
                let d = inf.point - corner;
                corner + u * settings.snap_length(d.dot(u)) + v * settings.snap_length(d.dot(v))
            }
            State::Taping { start, along } => {
                let d = inf.point - start;
                let parallel = along.map_or(DVec3::ZERO, |dir| dir * d.dot(dir));
                start + parallel + snap_vector(d - parallel)
            }
            State::PushPull { start, normal, .. } if inf.face.is_none() => {
                let d = (inf.point - start).dot(normal);
                inf.point + normal * (settings.snap_length(d) - d)
            }
            _ => return inf,
        };
        Inference { point, ..inf }
    }

    pub(crate) fn current_dir(&self, from: DVec3) -> Option<DVec3> {
        let p = self.inference?.point;
        let d = p - from;
        (d.length() > EPS).then(|| d.normalize())
    }

    pub(crate) fn facing(ctx: &Ctx) -> DVec3 {
        -ctx.view.ray(ctx.view.offset + ctx.view.size * 0.5).dir
    }

    fn on_move(&mut self, editor: &mut Editor, ctx: &Ctx) {
        if self.active == ToolKind::Dimension {
            self.dimension_move(editor, ctx);
        }
        let Some(inf) = self.inference else { return };
        match self.state.clone() {
            State::PushPull { start, normal, .. } => {
                if let State::PushPull { distance, .. } = &mut self.state {
                    *distance = (inf.point - start).dot(normal);
                }
            }
            State::Move { start, .. } => {
                if let State::Move { offset, .. } = &mut self.state {
                    *offset = inf.point - start;
                }
            }
            State::Erasing { .. } => {
                if let Some(e) = self.pick(editor, ctx)
                    && let State::Erasing { marked } = &mut self.state
                {
                    marked.insert(e);
                }
            }
            State::Following { .. } => self.follow_me_move(editor, ctx),
            State::Freehand { .. } => self.freehand_move(ctx),
            State::CentreSweep { .. } => self.centre_move(editor, ctx),
            State::AngleSweep { .. } => self.protractor_move(editor),
            State::ArcBulge { .. } => self.arc_move(editor, ctx),
            State::Rotating { .. } => self.rotate_move(editor),
            State::Scaling { .. } => self.scale_move(),
            State::Offsetting { .. } => self.offset_move(editor, ctx),
            _ => {}
        }
    }

    fn on_release(&mut self, editor: &mut Editor, ctx: &Ctx, dragged: bool) {
        match self.state.clone() {
            State::Freehand { .. } => self.finish_freehand(editor, ctx),
            State::Selecting { from } => {
                if dragged {
                    self.box_select(editor, ctx, from, ctx.cursor);
                }
                self.state = State::Idle;
            }
            State::Erasing { marked } => {
                self.finish_erase(editor, ctx, marked);
                self.state = State::Idle;
            }
            // Press-drag-release: the release is the next click.
            _ if dragged && self.busy() => self.on_click(editor, ctx, 1),
            _ => {}
        }
    }

    fn on_click(&mut self, editor: &mut Editor, ctx: &Ctx, clicks: u8) {
        let Some(inf) = self.inference else {
            if self.active == ToolKind::Select {
                self.select_click(editor, ctx, clicks);
            }
            return;
        };
        match self.active {
            ToolKind::Text => {}
            ToolKind::Dimension => self.dimension_click(editor, ctx, inf),
            ToolKind::Protractor => self.protractor_click(editor, ctx, inf),
            ToolKind::Select => self.select_click(editor, ctx, clicks),
            ToolKind::Eraser => {
                let mut marked = BTreeSet::new();
                marked.extend(self.pick(editor, ctx));
                self.state = State::Erasing { marked };
            }
            ToolKind::ThreePointArc => self.three_click(editor, ctx, inf),
            ToolKind::CenterArc | ToolKind::Pie => self.centre_click(editor, ctx, inf),
            ToolKind::Axes
            | ToolKind::Freehand
            | ToolKind::Line
            | ToolKind::Rectangle
            | ToolKind::RotatedRectangle
            | ToolKind::Circle
            | ToolKind::Polygon
            | ToolKind::Arc => self.draw_click(editor, ctx, inf),
            ToolKind::PushPull => self.push_pull_click(editor, ctx, inf, clicks),
            ToolKind::Move => self.move_click(editor, ctx, inf),
            ToolKind::Rotate => self.rotate_click(editor, ctx, inf),
            ToolKind::Scale => self.scale_click(editor, ctx),
            ToolKind::Offset => self.offset_click(editor, ctx, clicks),
            ToolKind::FollowMe => self.follow_me_click(editor, ctx),
            ToolKind::TapeMeasure => self.tape_click(editor, ctx, inf),
            ToolKind::PaintBucket => self.paint_click(editor, ctx),
            ToolKind::Orbit
            | ToolKind::Pan
            | ToolKind::Zoom
            | ToolKind::ZoomWindow
            | ToolKind::PositionCamera
            | ToolKind::LookAround
            | ToolKind::Walk => {}
        }
    }

    /// What a click at the cursor selects in the context.
    pub(crate) fn pick(&self, editor: &Editor, ctx: &Ctx) -> Option<Element> {
        match self.pick_full(editor, ctx) {
            rysmo_infer::Pick::Element(e) => Some(e),
            _ => None,
        }
    }

    /// Picks visible geometry using the editor context and standard selection tolerances.
    pub fn pick_full(&self, editor: &Editor, ctx: &Ctx) -> rysmo_infer::Pick {
        if let Some(pick) = self.override_pick {
            return pick;
        }
        let sc = scene(&self.scenes, editor);
        let q = Query {
            show_hidden: editor.doc().style.show_hidden_geometry,
            clips: sc.clips.clone(),
            axes: editor.active().axes,
            mesh: &sc.mesh,
            view: ctx.view,
            cursor: ctx.cursor,
            from: None,
            lock: None,
            hints: &[],
            scale: ctx.scale,
            others: &sc.others,
            accel: Some(&sc.accel),
            guides: &sc.guides,
        };
        let pick = rysmo_infer::pick(&q);
        match pick {
            rysmo_infer::Pick::Element(Element::Edge(id)) if editor.mesh().edge(id).is_none() => {
                rysmo_infer::Pick::Nothing
            }
            _ => pick,
        }
    }

    pub(crate) fn run(&mut self, editor: &mut Editor, cmd: Command) -> Option<Outcome> {
        match editor.apply(cmd) {
            Ok(o) => {
                self.correction_scope =
                    o.changed.then(|| (editor.revision(), editor.context().to_vec(), editor.undo_depth()));
                Some(o)
            }
            Err(e) => {
                self.message = Some(format!("{e}."));
                None
            }
        }
    }

    pub(crate) fn select_result(&mut self, editor: &mut Editor, elements: Vec<Element>) {
        editor.set_selection(elements);
        if let Some(scope) = &mut self.correction_scope {
            scope.0 = editor.revision();
        }
    }

    /// Typing after an operation corrects it: a new distance, angle, factor, or an array.
    fn amend_last(&mut self, editor: &mut Editor, input: VcbInput) -> Result<(), String> {
        let last = self.last.clone();
        match (last, input) {
            (Last::Arc { start, end, normal, bulge, segments, revision }, input) => {
                if editor.revision() != revision {
                    self.last = Last::None;
                    return Err("The model changed. Draw a new arc to edit it.".into());
                }
                let (bulge, segments) = match input {
                    VcbInput::Segments(n) => (bulge, n.max(1)),
                    VcbInput::Lengths(v) => {
                        let b = v.first().copied().filter(|b| b.is_finite() && b.abs() > EPS);
                        (b.ok_or("Enter a nonzero bulge.")?.abs() * bulge.signum(), segments)
                    }
                    _ => return Err("Enter a bulge or a segment count such as 12s.".into()),
                };
                self.correct_last(editor, Command::DrawArc { start, end, normal, bulge, segments })?;
                self.arc_segments = segments;
                self.last = Last::Arc { start, end, normal, bulge, segments, revision: editor.revision() };
            }
            (Last::RotatedRectangle { corner, u, v, facing, revision }, VcbInput::Lengths(values)) => {
                if revision != editor.revision() {
                    return Err("The model changed; draw a new rectangle.".into());
                }
                let [length, width] = values[..] else {
                    return Err("Enter length,width to correct both dimensions.".into());
                };
                if !(length.is_finite() && width.is_finite()) || length.abs() <= EPS || width.abs() <= EPS {
                    return Err("Enter nonzero, finite rectangle dimensions.".into());
                }
                let (u, v) = (u.normalize() * length, v.normalize() * width);
                self.correct_last(editor, Command::DrawRectangle { corner, u, v, facing })?;
                self.last = Last::RotatedRectangle { corner, u, v, facing, revision: editor.revision() };
            }
            (Last::TapeGuide { start, along, direction, revision, .. }, VcbInput::Lengths(v)) => {
                if revision != editor.revision() {
                    return Err("The model changed; create a new guide.".into());
                }
                let length = v.first().copied().ok_or("Enter a distance.")?;
                if !length.is_finite() || length.abs() <= EPS {
                    return Err("Enter a nonzero distance.".into());
                }
                let point = start + direction * length;
                let command = match along {
                    Some(dir) => Command::AddGuideLine { origin: point, dir },
                    None => Command::AddGuideSegment { start, point },
                };
                self.correct_last(editor, command)?;
                self.last = Last::TapeGuide { start, along, direction, length, revision: editor.revision() };
                self.message = Some(format!("Length {}", editor.doc().format_length(length.abs())));
            }
            (Last::Radial { centre, plane, radius, towards, segments, revision }, input) => {
                if editor.revision() != revision {
                    self.last = Last::None;
                    return Err("The model changed. Draw a new circle or polygon to edit its dimensions.".into());
                }
                let (radius, segments) = match input {
                    VcbInput::Lengths(values) => (
                        values
                            .first()
                            .copied()
                            .filter(|v| v.is_finite() && *v > EPS)
                            .ok_or("Enter a positive radius.")?,
                        segments,
                    ),
                    VcbInput::Segments(n) => (radius, n),
                    _ => return Err("Enter a radius or a side count ending in s.".into()),
                };
                self.correct_last(editor, self.radial_command(centre, plane, radius, towards, segments))?;
                if self.active == ToolKind::Polygon {
                    self.polygon_segments = segments;
                } else {
                    self.circle_segments = segments;
                }
                self.last = Last::Radial { centre, plane, radius, towards, segments, revision: editor.revision() };
            }
            (Last::PushPull { face, distance, keep_base, free }, VcbInput::Lengths(v)) => {
                let d = v.first().copied().filter(|x| x.is_finite()).ok_or("Type a distance.")?;
                let d = if distance < 0.0 { -d.abs() } else { d };
                self.correct_last(editor, Command::PushPull { face, distance: d, keep_base })?;
                self.last = Last::PushPull { face, distance: d, keep_base, free };
                self.last_distance = Some(if free { d.abs() } else { d });
            }
            (Last::Move { elements, offset, text_view }, VcbInput::Lengths(v)) => {
                let len = v.first().copied().filter(|x| x.is_finite()).ok_or("Type a distance.")?;
                let off = offset.normalize() * len;
                let command = if let Some((view, pivot)) = text_view {
                    Command::MoveWithText { elements: elements.clone(), offset: off, pivot, view }
                } else {
                    Command::Move { elements: elements.clone(), offset: off }
                };
                let out = self.correct_last(editor, command)?;
                self.select_result(editor, out.created);
                self.last = Last::Move { elements, offset: off, text_view };
            }
            (Last::Copy { elements, offset, copies, divide, text_view }, input) => {
                let (offset, copies, divide) = match input {
                    VcbInput::Lengths(v) => {
                        let len = v.first().copied().filter(|x| x.is_finite()).ok_or("Type a distance.")?;
                        (offset.normalize() * len, copies, divide)
                    }
                    VcbInput::Copies(n) => (offset, n, false),
                    VcbInput::Divide(n) => (offset, n, true),
                    VcbInput::Segments(_) => return Err("This value does not apply here.".into()),
                };
                let step = if divide { offset / copies as f64 } else { offset };
                let command = if let Some((view, pivot)) = text_view {
                    Command::CopyWithText { elements: elements.clone(), offset: step, copies, pivot, view }
                } else {
                    Command::Copy { elements: elements.clone(), offset: step, copies }
                };
                let out = self.correct_last(editor, command)?;
                self.select_result(editor, out.created);
                self.last = Last::Copy { elements, offset, copies, divide, text_view };
            }
            (Last::Rotate { elements, centre, axis, angle, copies, .. }, VcbInput::Copies(n)) if copies > 0 => {
                let out = self.correct_last(
                    editor,
                    Command::Rotate { elements: elements.clone(), centre, axis, angle, copies: n },
                )?;
                self.select_result(editor, out.created);
                self.last = Last::Rotate { elements, centre, axis, angle, copies: n, divide: false };
            }
            (Last::Rotate { elements, centre, axis, angle, copies, .. }, VcbInput::Divide(n)) if copies > 0 => {
                let step = angle / n as f64;
                let out = self.correct_last(
                    editor,
                    Command::Rotate { elements: elements.clone(), centre, axis, angle: step, copies: n },
                )?;
                self.select_result(editor, out.created);
                self.last = Last::Rotate { elements, centre, axis, angle, copies: n, divide: true };
            }
            (Last::Offset { target, edges, distance, revision }, VcbInput::Lengths(v)) => {
                if revision != editor.revision() {
                    return Err("The model changed; start a new Offset.".into());
                }
                let d = v.first().copied().filter(|x| x.is_finite()).ok_or("Type a distance.")?;
                let d = if distance < 0.0 { -d.abs() } else { d.abs() };
                let cmd = match target {
                    OffsetTarget::Face(face) => Command::OffsetFace { face, distance: d },
                    OffsetTarget::Chain(normal) => Command::OffsetEdges { edges: edges.clone(), normal, distance: d },
                };
                editor.amend_last(cmd).map_err(|e| e.to_string())?;
                self.last = Last::Offset { target, edges, distance: d, revision: editor.revision() };
                self.last_offset = Some(d);
            }
            _ => return Err("There is nothing to apply this value to.".into()),
        }
        Ok(())
    }

    pub(crate) fn correct_last(&mut self, editor: &mut Editor, cmd: Command) -> Result<Outcome, String> {
        if self.correction_scope.as_ref().is_none_or(|(revision, context, depth)| {
            *revision != editor.revision() || context != editor.context() || *depth != editor.undo_depth()
        }) {
            return Err(
                "The model or editing context changed. Start a new operation before correcting its value.".into()
            );
        }
        let depth = editor.undo_depth();
        match editor.amend_last(cmd) {
            Ok(out) => {
                self.correction_scope =
                    (editor.undo_depth() == depth).then(|| (editor.revision(), editor.context().to_vec(), depth));
                Ok(out)
            }
            Err(e) => {
                self.message = Some(format!("{e}."));
                Err(e.to_string())
            }
        }
    }
}

/// Edges of elements, for previews: geometry edges, and the bounding boxes of groups
/// and components, in the context's coordinates.
pub(crate) fn element_lines(editor: &Editor, elements: &[Element]) -> Vec<(DVec3, DVec3)> {
    let mesh = editor.mesh();
    let mut edges: BTreeSet<EdgeId> = BTreeSet::new();
    let mut out = Vec::new();
    for e in elements {
        match e {
            Element::Edge(id) if mesh.edge(*id).is_some() => {
                edges.insert(*id);
            }
            Element::Face(id) if mesh.face(*id).is_some() => edges.extend(mesh.face_edges(*id)),
            Element::Section(id) => {
                if editor.doc().style.section_planes
                    && let Some(plane) = editor.active().sections.planes.get(id)
                {
                    out.extend(
                        rysmo_model::sections::glyph(plane, editor.doc().definition_bounds(editor.active_definition()))
                            .into_iter()
                            .map(|[a, b]| (a, b)),
                    );
                }
            }
            Element::Instance(i) => {
                if let Some(inst) = editor.active().instances.get(i)
                    && let Some((lo, hi)) = editor.doc().definition_bounds(Some(inst.definition))
                {
                    out.extend(box_lines(lo, hi, &inst.transform));
                    let inverse = editor.context_transform().inverse();
                    for occurrence in editor
                        .doc()
                        .occurrences(editor.context())
                        .into_iter()
                        .filter(|o| o.in_context && o.path.get(editor.context().len()) == Some(i))
                    {
                        let transform = inverse * occurrence.transform;
                        if let Some(definition) = editor.doc().def(occurrence.definition) {
                            for (edge, member) in definition.mesh.edges().filter(|(_, e)| e.visible()) {
                                let _ = member;
                                let (a, b) = definition.mesh.edge_points(edge);
                                out.push((transform.transform_point3(a), transform.transform_point3(b)));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for e in edges {
        out.push(mesh.edge_points(e));
    }
    out
}

/// The twelve edges of a box under a transform.
pub fn box_lines(lo: DVec3, hi: DVec3, t: &DMat4) -> Vec<(DVec3, DVec3)> {
    let c = rysmo_model::corners(lo, hi).map(|p| t.transform_point3(p));
    let idx = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)];
    idx.iter().map(|(a, b)| (c[*a], c[*b])).collect()
}

/// The bounding box of elements in the context's coordinates.
pub(crate) fn element_bounds_in_axes(
    editor: &Editor,
    elements: &[Element],
    basis: DMat4,
    view: Option<&View>,
) -> Option<(DVec3, DVec3)> {
    let inverse = basis.inverse();
    let mut points: Vec<_> = element_lines(editor, elements)
        .into_iter()
        .flat_map(|(a, b)| [a, b])
        .map(|p| inverse.transform_point3(p))
        .collect();
    for element in elements {
        if let Element::Dimension(id) = element
            && let Some(dimension) = editor.active().dimensions.get(id)
            && let Some(layout) = dimension.layout(editor.doc(), editor.active_definition(), DMat4::IDENTITY, 0.)
        {
            points.extend(
                layout
                    .segments
                    .iter()
                    .flatten()
                    .copied()
                    .chain(std::iter::once(layout.text_position))
                    .map(|p| inverse.transform_point3(p)),
            );
        }
    }
    for element in elements {
        if let Element::Text(id) = element
            && let Some(text) = editor.active().texts.get(id)
            && let rysmo_model::text::TextPlacement::Attached { anchor, leader } = &text.placement
        {
            let point = anchor.resolve(editor.doc(), editor.active_definition()).0;
            points.push(inverse.transform_point3(point));
            match leader {
                rysmo_model::text::TextLeader::Pushpin { offset } | rysmo_model::text::TextLeader::None { offset } => {
                    points.push(inverse.transform_point3(point + *offset))
                }
                rysmo_model::text::TextLeader::ViewBased { .. } => {
                    if let Some(view) = view
                        && let Some(layout) =
                            text.layout(editor.doc(), editor.active_definition(), DMat4::IDENTITY, view)
                    {
                        let ray = view.ray(layout.position);
                        let plane = Plane::new(view.view.transpose().transform_vector3(DVec3::Z), point);
                        if let Some(depth) = plane.intersect_ray(&ray) {
                            points.push(inverse.transform_point3(ray.at(depth)));
                        }
                    }
                }
            }
        }
    }
    let first = *points.first()?;
    Some(points.into_iter().fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
}

/// The element under the cursor for tools that act on what is hovered when nothing
/// is selected (Move, Rotate, Scale).
pub(crate) fn hovered(inf: &Inference) -> Option<Element> {
    if let Some(i) = inf.instance {
        return Some(Element::Instance(i));
    }
    match (inf.edge, inf.face) {
        (Some(e), _) if matches!(inf.kind, Kind::OnEdge | Kind::Midpoint) => Some(Element::Edge(e)),
        (_, Some(f)) => Some(Element::Face(f)),
        (Some(e), None) => Some(Element::Edge(e)),
        _ => None,
    }
}

pub(crate) fn line_color(inf: Option<&Inference>) -> [f32; 3] {
    match inf.map(|i| i.kind) {
        Some(Kind::OnAxis(a)) => axis_color(a),
        Some(kind @ (Kind::Parallel | Kind::Perpendicular | Kind::Tangent)) => kind.color(),
        _ => BLACK,
    }
}
