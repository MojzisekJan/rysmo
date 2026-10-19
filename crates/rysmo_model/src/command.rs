//! Commands: every change to a document, as plain data.
//!
//! Geometry commands work in the definition being edited (the editor's context), in
//! that definition's coordinates. Tag, material and unit commands are document-wide.

use std::collections::{BTreeMap, BTreeSet};

use rysmo_math::{DMat4, DQuat, DVec3, EPS, Plane};
use rysmo_mesh::{
    DefinitionId, EdgeId, Element, FaceId, GuideId, InstanceId, MaterialId, Mesh, MeshError, TagId, Template,
};
use serde::{Deserialize, Serialize};

use crate::Units;
use std::sync::Arc;

use crate::document::{Definition, Document, Guide, Instance, Material, Tag};
use crate::{Image, Style, Texture};
use rysmo_mesh::{ImageId, TexturePlacement};

/// A change to the document. Points and distances are in metres, in the coordinates of
/// the context being edited (Z up). The JSON form is `{"op": "draw_lines", ...}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    SolidBoolean {
        left: InstanceId,
        right: InstanceId,
        operation: crate::solids::BooleanOp,
        keep_inputs: bool,
    },
    Paste {
        clipboard: Box<crate::clipboard::Clipboard>,
        transform: DMat4,
        reuse_definitions: bool,
    },
    CopyWithText {
        elements: Vec<Element>,
        offset: DVec3,
        copies: u32,
        pivot: DVec3,
        view: rysmo_math::View,
    },
    MoveWithText {
        elements: Vec<Element>,
        offset: DVec3,
        pivot: DVec3,
        view: rysmo_math::View,
    },
    ApplyTextDefaults {
        ids: Vec<rysmo_mesh::TextId>,
        camera: rysmo_math::OrbitCamera,
        viewport: rysmo_math::DVec2,
        transform: DMat4,
    },
    ApplyTextAppearance {
        ids: Vec<rysmo_mesh::TextId>,
    },
    Smoove {
        vertices: Vec<rysmo_mesh::VertexId>,
        centres: Vec<DVec3>,
        radius: f64,
        normal: DVec3,
        offset: DVec3,
    },
    /// Draped edges are created in the parent context, outside the terrain instance.
    DrapeOnFace {
        sources: Vec<Element>,
        target: FaceId,
    },
    DrapeSources {
        sources: Vec<Element>,
        target: InstanceId,
    },
    DrapeEdges {
        edges: Vec<EdgeId>,
        target: InstanceId,
    },
    StampOnFace {
        sources: Vec<Element>,
        target: FaceId,
        offset: f64,
        height: f64,
    },
    /// Stamps a closed terrain instance from selected faces/instances in its parent.
    StampTerrainInstance {
        sources: Vec<Element>,
        target: InstanceId,
        offset: f64,
        height: f64,
    },
    /// Stamp the active triangular mesh. Coordinates use its drawing axes.
    StampTerrainRings {
        outlines: Vec<Vec<rysmo_math::DVec2>>,
        offset: f64,
        height: f64,
    },
    StampTerrainOutline {
        outline: Vec<rysmo_math::DVec2>,
        offset: f64,
        height: f64,
    },
    AddTerrainDetail {
        face: FaceId,
        point: DVec3,
        offset: DVec3,
    },
    FlipTerrainEdge {
        edge: EdgeId,
    },
    TerrainFromContours {
        edges: Vec<EdgeId>,
    },
    CreateTerrainGrid {
        origin: DVec3,
        u: DVec3,
        v: DVec3,
        spacing: f64,
    },
    CreateText3d {
        content: String,
        settings: crate::text3d::Text3dSettings,
        origin: DVec3,
        x: DVec3,
        y: DVec3,
    },
    AddText {
        text: crate::text::Text,
    },
    SetText {
        id: rysmo_mesh::TextId,
        text: crate::text::Text,
    },
    EraseTexts {
        ids: Vec<rysmo_mesh::TextId>,
    },
    AddDimension {
        dimension: crate::dimensions::Dimension,
    },
    SetDimension {
        id: rysmo_mesh::DimensionId,
        dimension: crate::dimensions::Dimension,
    },
    ApplyDimensionDefaults {
        ids: Vec<rysmo_mesh::DimensionId>,
    },
    EraseDimensions {
        ids: Vec<rysmo_mesh::DimensionId>,
    },
    /// Lines, as drawn with the Line tool. `facing` is the preferred direction for new
    /// faces that nothing else decides (the tools pass the direction to the camera).
    DrawLines {
        lines: Vec<[DVec3; 2]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        facing: Option<DVec3>,
    },
    /// A rectangle from `corner` along the side vectors `u` and `v`.
    DrawRectangle {
        corner: DVec3,
        u: DVec3,
        v: DVec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        facing: Option<DVec3>,
    },
    /// A circle approximated by `segments` edges. `start` is the direction of the
    /// first vertex from the centre; it is made perpendicular to `normal`.
    DrawCircle {
        centre: DVec3,
        normal: DVec3,
        radius: f64,
        segments: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<DVec3>,
    },
    /// A regular polygon with faceted extrusion semantics.
    DrawPolygon {
        centre: DVec3,
        normal: DVec3,
        radius: f64,
        segments: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<DVec3>,
    },
    DrawFreehand {
        points: Vec<DVec3>,
        normal: DVec3,
    },
    SetCurveSegments {
        curve: rysmo_mesh::CurveId,
        segments: u32,
    },
    SetCurveRadius {
        curve: rysmo_mesh::CurveId,
        radius: f64,
    },
    /// A 2-point arc from `start` to `end` in the plane with `normal`, bulging by
    /// `bulge` (the sagitta) to the left of the chord seen from the normal.
    DrawArc {
        start: DVec3,
        end: DVec3,
        normal: DVec3,
        bulge: f64,
        segments: u32,
    },
    FilletEdges {
        edges: [EdgeId; 2],
        radius: f64,
        segments: u32,
    },
    DrawThreePointArc {
        start: DVec3,
        through: DVec3,
        end: DVec3,
        normal: DVec3,
        segments: u32,
    },
    /// A centre/radius/signed-sweep arc, optionally closed by two radii as a sector.
    DrawCentreArc {
        centre: DVec3,
        normal: DVec3,
        start: DVec3,
        radius: f64,
        sweep: f64,
        segments: u32,
        #[serde(default)]
        pie: bool,
    },
    /// Push/Pull: moves a face along its normal by `distance`, building sides.
    PushPull {
        face: FaceId,
        distance: f64,
        #[serde(default)]
        keep_base: bool,
    },
    Move {
        elements: Vec<Element>,
        offset: DVec3,
    },
    /// Copies elements `copies` times, each `offset` further.
    Copy {
        elements: Vec<Element>,
        offset: DVec3,
        copies: u32,
    },
    /// Rotates elements by `angle` degrees about the axis through `centre`
    /// (right-handed). With `copies` above zero, makes that many rotated copies instead,
    /// each `angle` further on.
    Rotate {
        elements: Vec<Element>,
        centre: DVec3,
        axis: DVec3,
        angle: f64,
        #[serde(default)]
        copies: u32,
    },
    /// Scales elements about `origin` by `factors` along the context's axes.
    Scale {
        elements: Vec<Element>,
        origin: DVec3,
        factors: DVec3,
    },
    ScaleInAxes {
        elements: Vec<Element>,
        origin: DVec3,
        factors: DVec3,
        x: DVec3,
        y: DVec3,
    },
    /// Offsets a face's outline; positive distances go inwards.
    OffsetFace {
        face: FaceId,
        distance: f64,
    },
    /// Uniformly resizes the active model/context about its origin.
    SetAxes {
        origin: DVec3,
        x: DVec3,
        y: DVec3,
    },
    ResizeContext {
        factor: f64,
    },
    /// Offsets a chain of edges to the left of its direction of travel, seen from
    /// `normal`.
    OffsetEdges {
        edges: Vec<EdgeId>,
        normal: DVec3,
        distance: f64,
    },
    /// Follow Me: sweeps `face` along the chain of edges `path`.
    FollowMe {
        face: FaceId,
        path: Vec<EdgeId>,
    },
    Erase {
        elements: Vec<Element>,
    },
    ReverseFaces {
        faces: Vec<FaceId>,
    },
    /// Sets edge flags; missing ones stay as they are.
    SetEdges {
        edges: Vec<EdgeId>,
        #[serde(default)]
        soft: Option<bool>,
        #[serde(default)]
        smooth: Option<bool>,
        #[serde(default)]
        hidden: Option<bool>,
    },
    /// Soften Edges: edges whose faces meet at less than `angle` degrees become soft
    /// and smooth, the others hard.
    SoftenEdges {
        edges: Vec<EdgeId>,
        angle: f64,
    },
    /// Makes a group of the elements (which may include instances and guides).
    MakeGroup {
        elements: Vec<Element>,
    },
    /// Makes a component of the elements.
    MakeComponent {
        elements: Vec<Element>,
        #[serde(default)]
        name: Option<String>,
    },
    /// Replaces an instance by its geometry.
    Explode {
        instance: InstanceId,
    },
    /// Gives an instance its own copy of its definition.
    MakeUnique {
        instance: InstanceId,
    },
    SetGeometryHidden {
        elements: Vec<Element>,
        hidden: bool,
    },
    SetInstancesHidden {
        instances: Vec<InstanceId>,
        hidden: bool,
    },
    SetComponentProperties {
        definition: DefinitionId,
        name: String,
        settings: crate::components::ComponentSettings,
    },
    DeleteDefinition {
        definition: DefinitionId,
    },
    RenameInstance {
        instance: InstanceId,
        name: String,
    },
    /// Places another instance of a definition: its origin goes to `origin`, turned by
    /// `rotation` degrees about the blue axis and scaled by `scale`.
    UnglueInstances {
        instances: Vec<InstanceId>,
    },
    PlaceGluedInstance {
        definition: DefinitionId,
        face: FaceId,
        origin: DVec3,
        rotation: f64,
        scale: f64,
    },
    PlaceInstance {
        definition: DefinitionId,
        origin: DVec3,
        #[serde(default)]
        rotation: f64,
        #[serde(default = "one")]
        scale: f64,
    },
    AddGuideLine {
        origin: DVec3,
        dir: DVec3,
    },
    AddGuidePoint {
        point: DVec3,
    },
    AddGuideSegment {
        start: DVec3,
        point: DVec3,
    },
    /// Erases all guides of the context.
    DeleteGuides,
    AddMaterial {
        name: String,
        color: [f32; 4],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        texture: Option<Texture>,
    },
    /// Imports an image and its material as one undoable operation.
    AddTexturedMaterial {
        name: String,
        color: [f32; 4],
        image_name: String,
        mime: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
        size: [f64; 2],
    },
    /// Adds an image for textured materials. `data` is the encoded file (PNG or JPEG),
    /// base64 in JSON.
    AddImage {
        name: String,
        mime: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
    /// Gives a material a texture, or takes it away (`None`).
    SetMaterialTexture {
        material: MaterialId,
        texture: Option<Texture>,
    },
    /// Makes a material give off light, in cd/m² (0: none).
    SetMaterialGlow {
        material: MaterialId,
        glow: f32,
    },
    /// Places the texture on faces (`None`: the default placement).
    SetTexturePlacement {
        faces: Vec<FaceId>,
        placement: Option<TexturePlacement>,
    },
    /// Sets how the model is drawn.
    CreateSectionSlice {
        id: u64,
    },
    AddSection {
        plane: crate::sections::SectionPlane,
    },
    SetSection {
        id: u64,
        plane: Option<crate::sections::SectionPlane>,
    },
    ActivateSection {
        id: Option<u64>,
    },
    IntersectFaces {
        elements: Vec<Element>,
        with_model: bool,
        context: Vec<InstanceId>,
    },
    AddScene {
        scene: crate::scenes::Scene,
    },
    SetScene {
        id: u64,
        scene: Option<crate::scenes::Scene>,
    },
    RestoreScene {
        id: u64,
    },
    ReorderScene {
        id: u64,
        index: usize,
    },
    SetStyle {
        style: Style,
    },
    SetMaterial {
        material: MaterialId,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        color: Option<[f32; 4]>,
    },
    Paint {
        faces: Vec<FaceId>,
        material: Option<MaterialId>,
        #[serde(default)]
        back: bool,
    },
    /// Paints a mixed selection of faces and instances in one history step.
    PaintElements {
        elements: Vec<Element>,
        material: Option<MaterialId>,
        #[serde(default)]
        back: bool,
    },
    PaintWrappedElements {
        elements: Vec<Element>,
        material: Option<MaterialId>,
        back: bool,
        source: FaceId,
        placement: TexturePlacement,
    },
    /// Transfers sampled texture placement with the material, atomically.
    PaintPositionedElements {
        elements: Vec<Element>,
        material: Option<MaterialId>,
        #[serde(default)]
        back: bool,
        placement: TexturePlacement,
    },
    /// Paints groups and components: their faces without a material take this one.
    PaintInstances {
        instances: Vec<InstanceId>,
        material: Option<MaterialId>,
    },
    AddTag {
        name: String,
    },
    SetTag {
        tag: TagId,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        visible: Option<bool>,
    },
    /// Deletes a tag; what was on it becomes untagged.
    DeleteTag {
        tag: TagId,
    },
    /// Puts instances on a tag (`None`: untagged).
    AssignTag {
        instances: Vec<InstanceId>,
        tag: Option<TagId>,
    },
    SetMeasurementSettings {
        settings: crate::MeasurementSettings,
    },
    SetLengthPrecision {
        precision: Option<u8>,
    },
    SetUnits {
        units: Units,
    },
}

/// What a command did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// Whether the document changed (and an undo step was recorded).
    pub changed: bool,
    pub dimensions: Vec<rysmo_mesh::DimensionId>,
    pub texts: Vec<rysmo_mesh::TextId>,
    /// Elements the command created, for selecting them afterwards.
    pub created: Vec<Element>,
    /// A material the command created.
    pub material: Option<MaterialId>,
    /// A tag the command created.
    pub tag: Option<TagId>,
    /// An image the command added.
    pub image: Option<ImageId>,
}

/// Bytes as base64 text in JSON.
mod b64 {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD.decode(text.as_bytes()).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CommandError {
    #[error(transparent)]
    Mesh(#[from] MeshError),
    #[error("{0}")]
    Invalid(String),
}

fn one() -> f64 {
    1.0
}

/// The brightest a material may glow, in cd/m².
pub const MAX_GLOW: f32 = 1_000_000.0;

fn invalid<T>(msg: impl Into<String>) -> Result<T, CommandError> {
    Err(CommandError::Invalid(msg.into()))
}

/// Points of a 2-point arc, start to end.
pub fn arc_points(start: DVec3, end: DVec3, normal: DVec3, bulge: f64, segments: u32) -> Option<Vec<DVec3>> {
    let chord = end - start;
    let len = chord.length();
    if !start.is_finite()
        || !end.is_finite()
        || !normal.is_finite()
        || !bulge.is_finite()
        || len < EPS
        || bulge.abs() < EPS
        || segments < 1
        || normal.length() < EPS
    {
        return None;
    }
    let n = normal.normalize();
    let chord_dir = chord / len;
    let left = n.cross(chord_dir).normalize();
    let mid = (start + end) * 0.5;
    let r = (len * len / 4.0 + bulge * bulge) / (2.0 * bulge.abs());
    let centre = mid + left * (bulge - bulge.signum() * r);
    let theta = 4.0 * (2.0 * bulge.abs() / len).atan();
    let e1 = (start - centre) / r;
    let bulge_point = mid + left * bulge;
    let sweep = |axis: DVec3| -> Vec<DVec3> {
        let e2 = axis.cross(e1);
        (0..=segments)
            .map(|k| {
                let phi = theta * k as f64 / segments as f64;
                centre + (e1 * phi.cos() + e2 * phi.sin()) * r
            })
            .collect()
    };
    // Sweep round the normal or against it, whichever passes the bulge point.
    let halfway = centre + (e1 * (theta * 0.5).cos() + n.cross(e1) * (theta * 0.5).sin()) * r;
    let mut pts = if halfway.distance(bulge_point) < 1e-6 * (1.0 + r) { sweep(n) } else { sweep(-n) };
    // Pin the ends exactly.
    pts[0] = start;
    let last = pts.len() - 1;
    pts[last] = end;
    Some(pts)
}

/// Samples a signed arc strictly smaller than a full turn. Distances are metres.
pub fn centre_arc_points(
    centre: DVec3,
    normal: DVec3,
    start: DVec3,
    radius: f64,
    sweep: f64,
    segments: u32,
) -> Option<Vec<DVec3>> {
    if !centre.is_finite()
        || !normal.is_finite()
        || !start.is_finite()
        || !radius.is_finite()
        || !(radius * radius).is_finite()
        || radius <= EPS
        || !sweep.is_finite()
        || sweep.abs() < 1e-6
        || sweep.abs() >= 360.
        || !(2..=4096).contains(&segments)
    {
        return None;
    }
    let n = normal.try_normalize()?;
    let u = start.try_normalize()?;
    if n.dot(u).abs() > 1e-6 {
        return None;
    }
    let v = n.cross(u);
    Some(
        (0..=segments)
            .map(|i| {
                let t = (sweep * i as f64 / segments as f64).to_radians();
                centre + (u * t.cos() + v * t.sin()) * radius
            })
            .collect(),
    )
}

pub(crate) fn validate_rotation(centre: DVec3, axis: DVec3, angle: f64) -> Result<(), CommandError> {
    if !centre.is_finite()
        || !axis.is_finite()
        || !axis.length().is_finite()
        || axis.length() < EPS
        || !angle.is_finite()
    {
        return invalid("rotation needs a finite centre, angle and usable axis");
    }
    Ok(())
}

pub(crate) fn rotation(centre: DVec3, axis: DVec3, degrees: f64) -> DMat4 {
    DMat4::from_translation(centre)
        * DMat4::from_quat(DQuat::from_axis_angle(axis.normalize(), degrees.to_radians()))
        * DMat4::from_translation(-centre)
}

fn split(elements: &[Element]) -> (Vec<Element>, Vec<InstanceId>, Vec<GuideId>, Vec<rysmo_mesh::DimensionId>) {
    let mut geo = Vec::new();
    let mut inst = Vec::new();
    let mut guides = Vec::new();
    let mut dimensions = Vec::new();
    for e in elements {
        match e {
            Element::Edge(_) | Element::Face(_) => geo.push(*e),
            Element::Instance(i) => inst.push(*i),
            Element::Guide(g) => guides.push(*g),
            Element::Dimension(id) => dimensions.push(*id),
            Element::Text(_) | Element::Section(_) => {}
        }
    }
    (geo, inst, guides, dimensions)
}

fn map_guide(g: &Guide, m: &DMat4) -> Guide {
    match g {
        Guide::Line { origin, dir, hidden } => Guide::Line {
            origin: m.transform_point3(*origin),
            dir: m.transform_vector3(*dir).normalize_or(*dir),
            hidden: *hidden,
        },
        Guide::Point { p, origin, hidden } => {
            Guide::Point { p: m.transform_point3(*p), origin: origin.map(|p| m.transform_point3(p)), hidden: *hidden }
        }
    }
}

impl Command {
    /// A short action name for the undo menu.
    pub fn label(&self) -> &'static str {
        match self {
            Command::SolidBoolean { operation, .. } => operation.name(),
            Command::ApplyTextDefaults { .. } => "Text Settings",
            Command::ApplyTextAppearance { .. } => "Text Appearance",
            Command::Smoove { .. } => "Smoove",
            Command::DrapeEdges { .. } | Command::DrapeSources { .. } | Command::DrapeOnFace { .. } => "Drape",
            Command::StampTerrainOutline { .. }
            | Command::StampTerrainRings { .. }
            | Command::StampTerrainInstance { .. }
            | Command::StampOnFace { .. } => "Stamp",
            Command::AddTerrainDetail { .. } => "Add Detail",
            Command::FlipTerrainEdge { .. } => "Flip Edge",
            Command::TerrainFromContours { .. } => "From Contours",
            Command::CreateTerrainGrid { .. } => "From Scratch",
            Command::CreateText3d { .. } => "3D Text",
            Command::AddText { .. } => "Text",
            Command::SetText { .. } => "Edit Text",
            Command::EraseTexts { .. } => "Erase Text",
            Command::AddDimension { .. } => "Dimension",
            Command::SetDimension { .. } => "Edit Dimension",
            Command::ApplyDimensionDefaults { .. } => "Update Dimensions",
            Command::EraseDimensions { .. } => "Erase Dimensions",
            Command::DrawLines { .. } => "Line",
            Command::DrawRectangle { .. } => "Rectangle",
            Command::DrawCircle { .. } => "Circle",
            Command::DrawPolygon { .. } => "Polygon",
            Command::DrawFreehand { .. } => "Freehand",
            Command::SetCurveRadius { .. } => "Curve Radius",
            Command::SetCurveSegments { .. } => "Curve Segments",
            Command::DrawArc { .. } => "Arc",
            Command::FilletEdges { .. } => "Fillet",
            Command::DrawThreePointArc { .. } => "Three Point Arc",
            Command::DrawCentreArc { pie: true, .. } => "Pie",
            Command::DrawCentreArc { .. } => "Center Arc",
            Command::PushPull { .. } => "Push/Pull",
            Command::Move { .. } | Command::MoveWithText { .. } => "Move",
            Command::Paste { .. } => "Paste",
            Command::Copy { .. } | Command::CopyWithText { .. } => "Copy",
            Command::Rotate { .. } => "Rotate",
            Command::Scale { .. } | Command::ScaleInAxes { .. } => "Scale",
            Command::SetAxes { .. } => "Drawing Axes",
            Command::ResizeContext { .. } => "Resize Model",
            Command::OffsetFace { .. } | Command::OffsetEdges { .. } => "Offset",
            Command::FollowMe { .. } => "Follow Me",
            Command::Erase { .. } => "Erase",
            Command::ReverseFaces { .. } => "Reverse Faces",
            Command::SetEdges { .. } => "Edge Style",
            Command::SoftenEdges { .. } => "Soften Edges",
            Command::MakeGroup { .. } => "Make Group",
            Command::MakeComponent { .. } => "Make Component",
            Command::Explode { .. } => "Explode",
            Command::MakeUnique { .. } => "Make Unique",
            Command::SetGeometryHidden { .. } => "Geometry visibility",
            Command::SetInstancesHidden { .. } => "Instance visibility",
            Command::SetComponentProperties { .. } => "Component properties",
            Command::DeleteDefinition { .. } => "Delete component",
            Command::RenameInstance { .. } => "Rename",
            Command::PlaceInstance { .. } | Command::PlaceGluedInstance { .. } => "Place Component",
            Command::UnglueInstances { .. } => "Unglue",
            Command::AddGuideLine { .. } | Command::AddGuidePoint { .. } | Command::AddGuideSegment { .. } => "Guide",
            Command::DeleteGuides => "Delete Guides",
            Command::AddMaterial { .. } | Command::AddTexturedMaterial { .. } => "New Material",
            Command::AddImage { .. } => "Add Image",
            Command::SetMaterialTexture { .. } => "Texture",
            Command::SetMaterialGlow { .. } => "Glow",
            Command::SetTexturePlacement { .. } => "Position Texture",
            Command::CreateSectionSlice { .. }
            | Command::AddSection { .. }
            | Command::SetSection { .. }
            | Command::ActivateSection { .. } => "Section",
            Command::IntersectFaces { .. } => "Intersect Faces",
            Command::AddScene { .. } => "Add scene",
            Command::SetScene { .. } => "Edit scene",
            Command::RestoreScene { .. } => "Restore scene",
            Command::ReorderScene { .. } => "Reorder scene",
            Command::SetStyle { .. } => "Style",
            Command::SetMaterial { .. } => "Edit Material",
            Command::Paint { .. }
            | Command::PaintInstances { .. }
            | Command::PaintElements { .. }
            | Command::PaintPositionedElements { .. }
            | Command::PaintWrappedElements { .. } => "Paint",
            Command::AddTag { .. } => "Add Tag",
            Command::SetTag { .. } => "Tag",
            Command::DeleteTag { .. } => "Delete Tag",
            Command::AssignTag { .. } => "Change Tag",
            Command::SetMeasurementSettings { .. } => "Measurement settings",
            Command::SetUnits { .. } | Command::SetLengthPrecision { .. } => "Units",
        }
    }

    /// Runs the command with `def` (the context being edited; `None` is the model).
    pub(crate) fn run(self, doc: &mut Document, def: Option<DefinitionId>) -> Result<Outcome, CommandError> {
        let glued_before = crate::components::capture_glue(doc);
        if doc.def(def).is_none() {
            return invalid("the context being edited no longer exists");
        }
        let text_transform = match &self {
            Command::Move { elements, .. } | Command::Copy { elements, .. } => {
                elements.iter().any(|e| matches!(e, Element::Text(_)))
            }
            _ => false,
        };
        if text_transform {
            return invalid("transforming selected text is not implemented yet");
        }
        let mut out = Outcome { changed: true, ..Default::default() };
        let faces_before: BTreeSet<FaceId> = doc.def(def).expect("checked").mesh.faces().map(|(f, _)| f).collect();
        let new_faces = |doc: &Document| -> Vec<Element> {
            doc.def(def)
                .expect("checked")
                .mesh
                .faces()
                .map(|(f, _)| f)
                .filter(|f| !faces_before.contains(f))
                .map(Element::Face)
                .collect()
        };
        let edges = |v: Vec<EdgeId>| v.into_iter().map(Element::Edge).collect::<Vec<_>>();
        let mesh_op = |doc: &mut Document, f: &mut dyn FnMut(&mut Mesh) -> Result<Vec<Element>, MeshError>| {
            doc.with_mesh(def, |m| f(m)).expect("checked").map_err(CommandError::from)
        };
        let polygon = matches!(&self, Command::DrawPolygon { .. });
        let scale_basis = if let Command::ScaleInAxes { x, y, .. } = &self {
            rysmo_math::DrawingAxes::new(DVec3::ZERO, *x, *y)
                .ok_or_else(|| CommandError::Invalid("invalid scale axes".into()))?
                .basis()
        } else {
            DMat4::IDENTITY
        };
        match self {
            Command::SolidBoolean { left, right, operation, keep_inputs } => {
                out.created = crate::solids::run(doc, def, left, right, operation, keep_inputs)?;
                out.changed = !keep_inputs || !out.created.is_empty();
            }
            Command::Paste { clipboard, transform, reuse_definitions } => {
                out.created = crate::clipboard::paste(doc, def, &clipboard, transform, reuse_definitions)?;
            }
            Command::Smoove { vertices, centres, radius, normal, offset } => {
                out.changed = doc
                    .with_mesh(def, |mesh| crate::terrain::smoove(mesh, &vertices, &centres, radius, normal, offset))
                    .expect("checked")
                    .map_err(CommandError::Invalid)?;
            }
            Command::DrapeOnFace { sources, target } => {
                if sources.contains(&Element::Face(target)) {
                    return invalid("terrain cannot also be the drape source");
                }
                let parent = doc.def(def).unwrap();
                let axes = parent.axes;
                let frame = axes.basis().inverse() * DMat4::from_translation(-axes.origin);
                let segments = crate::terrain_footprint::source_segments(doc, def, &sources, frame)
                    .map_err(CommandError::Invalid)?
                    .into_iter()
                    .filter(|line| {
                        rysmo_math::DVec2::new(line[0].x - line[1].x, line[0].y - line[1].y).length_squared() > 1e-20
                    })
                    .collect::<Vec<_>>();
                if segments.is_empty() {
                    return invalid("select source edges with horizontal extent");
                }
                let mut terrain =
                    crate::terrain_region::connected(&parent.mesh, target).map_err(CommandError::Invalid)?;
                terrain.map_all(|p| frame.transform_point3(p));
                let lines = crate::terrain_surface::TerrainSurface::drape_downward(&terrain, &segments)
                    .map_err(CommandError::Invalid)?;
                if lines.is_empty() {
                    out.changed = false;
                    return Ok(out);
                }
                let inverse = frame.inverse();
                return Command::DrawLines {
                    lines: lines.into_iter().map(|line| line.map(|p| inverse.transform_point3(p))).collect(),
                    facing: Some(axes.z()),
                }
                .run(doc, def);
            }
            Command::DrapeEdges { edges, target } => {
                return Command::DrapeSources { sources: edges.into_iter().map(Element::Edge).collect(), target }
                    .run(doc, def);
            }
            Command::DrapeSources { sources, target } => {
                if sources.contains(&Element::Instance(target)) {
                    return invalid("terrain cannot also be the drape source");
                }
                let parent = doc.def(def).unwrap();
                let instance = parent
                    .instances
                    .get(&target)
                    .ok_or_else(|| CommandError::Invalid("terrain instance is missing".into()))?;
                let axes = parent.axes;
                let frame = axes.basis().inverse() * DMat4::from_translation(-axes.origin);
                let segments = crate::terrain_footprint::source_segments(doc, def, &sources, frame)
                    .map_err(CommandError::Invalid)?
                    .into_iter()
                    .filter(|line| {
                        rysmo_math::DVec2::new(line[0].x - line[1].x, line[0].y - line[1].y).length_squared() > 1e-20
                    })
                    .collect::<Vec<_>>();
                if segments.is_empty() {
                    return invalid("select source edges with horizontal extent");
                }
                let definition = doc
                    .def(Some(instance.definition))
                    .ok_or_else(|| CommandError::Invalid("terrain definition is missing".into()))?;
                if !definition.instances.is_empty() {
                    return invalid("terrain must be a triangular mesh without child instances");
                }
                let projection = frame * instance.transform;
                if !projection.inverse().is_finite() {
                    return invalid("terrain transform is singular");
                }
                let mut terrain = definition.mesh.clone();
                terrain.map_all(|p| projection.transform_point3(p));
                let lines = crate::terrain_surface::TerrainSurface::drape_downward(&terrain, &segments)
                    .map_err(CommandError::Invalid)?;
                if lines.is_empty() {
                    out.changed = false;
                    return Ok(out);
                }
                let inverse = frame.inverse();
                let lines = lines.into_iter().map(|line| line.map(|p| inverse.transform_point3(p))).collect();
                return Command::DrawLines { lines, facing: Some(axes.z()) }.run(doc, def);
            }
            Command::StampOnFace { sources, target, offset, height } => {
                let parent = doc.def(def).unwrap();
                let region = crate::terrain_region::connected(&parent.mesh, target).map_err(CommandError::Invalid)?;
                if sources.iter().any(|source| matches!(source, Element::Face(face) if region.face(*face).is_some())) {
                    return invalid("terrain cannot also be the stamp source");
                }
                let region = crate::terrain_region::with_curves(&parent.mesh, region).map_err(CommandError::Invalid)?;
                let axes = parent.axes;
                let frame = axes.basis().inverse() * DMat4::from_translation(-axes.origin);
                let outlines = crate::terrain_footprint::source_boundaries(doc, def, &sources, frame)
                    .map_err(CommandError::Invalid)?;
                let mut projected = region.clone();
                projected.map_all(|p| frame.transform_point3(p));
                let mut stamped = crate::terrain_surface::TerrainSurface::stamp_rings(
                    &projected,
                    &outlines,
                    offset,
                    height,
                    doc.next_id,
                )
                .map_err(CommandError::Invalid)?;
                let unchanged = stamped
                    .vertices()
                    .filter(|(id, vertex)| projected.vertex(*id) == Some(*vertex))
                    .map(|(id, _)| id)
                    .collect::<BTreeSet<_>>();
                let inverse = frame.inverse();
                stamped.map_all(|p| inverse.transform_point3(p));
                stamped = Mesh::from_parts_with_curves(
                    stamped
                        .vertices()
                        .map(|(id, vertex)| {
                            (
                                id,
                                if unchanged.contains(&id) {
                                    region.vertex(id).unwrap().clone()
                                } else {
                                    vertex.clone()
                                },
                            )
                        })
                        .collect(),
                    stamped.edges().map(|(id, edge)| (id, edge.clone())).collect(),
                    stamped.faces().map(|(id, face)| (id, face.clone())).collect(),
                    stamped.curves().map(|(id, curve)| (id, *curve)).collect(),
                )?;
                let mesh =
                    crate::terrain_region::replace(&parent.mesh, &region, &stamped).map_err(CommandError::Invalid)?;
                doc.with_mesh(def, |target| *target = mesh).expect("checked");
                out.created = new_faces(doc);
            }
            Command::StampTerrainInstance { sources, target, offset, height } => {
                if sources.contains(&Element::Instance(target)) {
                    return invalid("terrain cannot also be the stamp source");
                }
                let parent = doc.def(def).unwrap();
                let instance = parent
                    .instances
                    .get(&target)
                    .ok_or_else(|| CommandError::Invalid("terrain instance is missing".into()))?
                    .clone();
                if !instance.transform.is_finite() || !instance.transform.inverse().is_finite() {
                    return invalid("terrain instance transform is singular or non-finite");
                }
                let axes = parent.axes;
                let frame = axes.basis().inverse() * DMat4::from_translation(-axes.origin);
                let outlines = crate::terrain_footprint::source_boundaries(doc, def, &sources, frame)
                    .map_err(CommandError::Invalid)?;
                let definition = doc
                    .def(Some(instance.definition))
                    .ok_or_else(|| CommandError::Invalid("terrain definition is missing".into()))?;
                if !definition.instances.is_empty() {
                    return invalid("terrain must contain a triangular mesh without nested instances");
                }
                let projection = frame * instance.transform;
                let mut mesh = definition.mesh.clone();
                mesh.map_all(|p| projection.transform_point3(p));
                let mut stamped =
                    crate::terrain_surface::TerrainSurface::stamp_rings(&mesh, &outlines, offset, height, doc.next_id)
                        .map_err(CommandError::Invalid)?;
                let inverse = projection.inverse();
                stamped.map_all(|p| inverse.transform_point3(p));
                doc.with_mesh(Some(instance.definition), |mesh| *mesh = stamped).expect("checked");
                out.created.push(Element::Instance(target));
            }
            Command::StampTerrainOutline { outline, offset, height } => {
                return Command::StampTerrainRings { outlines: vec![outline], offset, height }.run(doc, def);
            }
            Command::StampTerrainRings { outlines, offset, height } => {
                let source = doc.def(def).unwrap();
                let axes = source.axes;
                let basis = axes.basis();
                let inverse = basis.inverse();
                let mut projected = source.mesh.clone();
                projected.map_all(|p| inverse.transform_vector3(p - axes.origin));
                let mut stamped = crate::terrain_surface::TerrainSurface::stamp_rings(
                    &projected,
                    &outlines,
                    offset,
                    height,
                    doc.next_id,
                )
                .map_err(CommandError::Invalid)?;
                stamped.map_all(|p| axes.origin + basis.transform_vector3(p));
                doc.with_mesh(def, |mesh| *mesh = stamped).expect("checked");
                out.created = new_faces(doc);
            }
            Command::AddTerrainDetail { face, point, offset } => {
                let (_, changed) =
                    doc.with_mesh(def, |mesh| mesh.add_terrain_detail(face, point, offset)).expect("checked")?;
                out.changed = changed;
                out.created = new_faces(doc);
            }
            Command::FlipTerrainEdge { edge } => {
                let new_edge = doc.with_mesh(def, |mesh| mesh.flip_terrain_edge(edge)).expect("checked")?;
                out.created.push(Element::Edge(new_edge));
            }
            Command::TerrainFromContours { edges } => {
                let source = doc.def(def).unwrap();
                let axes = source.axes;
                let basis = axes.basis();
                let inverse = basis.inverse();
                let unique = edges.into_iter().collect::<BTreeSet<_>>();
                if unique.is_empty() {
                    return invalid("select contour edges first");
                }
                let lines = unique
                    .iter()
                    .map(|id| {
                        let edge = source
                            .mesh
                            .edge(*id)
                            .ok_or_else(|| CommandError::Invalid("contour edge is missing".into()))?;
                        Ok(edge.v.map(|vertex| inverse.transform_vector3(source.mesh.pos(vertex) - axes.origin)))
                    })
                    .collect::<Result<Vec<_>, CommandError>>()?;
                let mut mesh = crate::contours::triangulate(&lines, doc.next_id).map_err(CommandError::Invalid)?;
                mesh.map_all(|point| axes.origin + basis.transform_vector3(point));
                let origin = mesh.bounds().unwrap().0;
                mesh.map_all(|point| point - origin);
                for (curve_id, curve) in source.mesh.curves() {
                    let original = source.mesh.curve_edges(curve_id);
                    let selected = original.iter().filter(|id| unique.contains(id)).copied().collect::<Vec<_>>();
                    if selected.is_empty() {
                        continue;
                    }
                    let mut members = BTreeSet::new();
                    for edge in &selected {
                        let endpoints = source.mesh.edge(*edge).unwrap().v.map(|id| source.mesh.pos(id) - origin);
                        members.extend(mesh.edges_along(endpoints[0], endpoints[1]));
                    }
                    if members.is_empty() {
                        continue;
                    }
                    let mut descriptor = curve.mapped(&|point| point - origin);
                    if selected.len() != original.len() || members.len() != selected.len() {
                        descriptor.kind = rysmo_mesh::CurveKind::Polyline;
                        descriptor.segments = members.len() as u32;
                    }
                    mesh.register_curve(&members.into_iter().collect::<Vec<_>>(), descriptor)?;
                }
                doc.next_id = doc.next_id.max(mesh.next_id());
                let definition = DefinitionId(doc.reserve_id());
                doc.definitions.insert(
                    definition,
                    Definition {
                        name: "Terrain from Contours".into(),
                        group: true,
                        mesh,
                        axes: rysmo_math::DrawingAxes { origin: DVec3::ZERO, x: axes.x, y: axes.y },
                        ..Default::default()
                    },
                );
                let instance = InstanceId(doc.reserve_id());
                doc.def_mut(def).unwrap().instances.insert(
                    instance,
                    Instance {
                        glued_to: None,
                        definition,
                        transform: DMat4::from_translation(origin),
                        name: String::new(),
                        tag: None,
                        material: None,
                        hidden: false,
                    },
                );
                out.created.push(Element::Instance(instance));
            }
            Command::CreateTerrainGrid { origin, u, v, spacing } => {
                let axes = rysmo_math::DrawingAxes::new(origin, u, v)
                    .ok_or_else(|| CommandError::Invalid("invalid terrain plane".into()))?;
                if u.normalize().dot(v.normalize()).abs() > 1e-6 {
                    return invalid("terrain sides must be perpendicular");
                }
                let mut mesh = crate::terrain::grid_with_ids(u.length(), v.length(), spacing, doc.next_id)
                    .map_err(CommandError::Invalid)?;
                if axes.z().dot(doc.def(def).unwrap().axes.z()) < 0. {
                    let faces: Vec<_> = mesh.faces().map(|(id, _)| id).collect();
                    mesh.reverse_faces(&faces);
                }
                doc.next_id = doc.next_id.max(mesh.next_id());
                let definition = DefinitionId(doc.reserve_id());
                doc.definitions
                    .insert(definition, Definition { name: "Terrain".into(), group: true, mesh, ..Default::default() });
                let instance = InstanceId(doc.reserve_id());
                doc.def_mut(def).unwrap().instances.insert(
                    instance,
                    Instance {
                        glued_to: None,
                        definition,
                        transform: DMat4::from_translation(origin) * axes.basis(),
                        name: String::new(),
                        tag: None,
                        material: None,
                        hidden: false,
                    },
                );
                out.created.push(Element::Instance(instance));
            }
            Command::CreateText3d { content, settings, origin, x, y } => {
                let axes = rysmo_math::DrawingAxes::new(origin, x, y)
                    .ok_or_else(|| CommandError::Invalid("invalid 3D text placement axes".into()))?;
                let mut mesh = crate::text3d::build_text(&content, &settings).map_err(CommandError::Invalid)?;
                mesh.ensure_next_id(doc.next_id);
                let elements: Vec<_> = mesh
                    .edges()
                    .map(|(id, _)| Element::Edge(id))
                    .chain(mesh.faces().map(|(id, _)| Element::Face(id)))
                    .collect();
                let mesh = mesh.extract(&elements);
                doc.next_id = doc.next_id.max(mesh.next_id());
                let definition = DefinitionId(doc.reserve_id());
                doc.definitions.insert(
                    definition,
                    Definition {
                        name: content.lines().next().unwrap_or("3D Text").chars().take(80).collect(),
                        group: true,
                        mesh,
                        ..Default::default()
                    },
                );
                let instance = InstanceId(doc.reserve_id());
                doc.def_mut(def).unwrap().instances.insert(
                    instance,
                    Instance {
                        glued_to: None,
                        definition,
                        transform: DMat4::from_translation(origin) * axes.basis(),
                        name: String::new(),
                        tag: None,
                        material: None,
                        hidden: false,
                    },
                );
                out.created.push(Element::Instance(instance));
            }
            Command::ApplyTextDefaults { ids, camera, viewport, transform } => {
                let defaults = doc.style.text_defaults;
                if !defaults.validate()
                    || !viewport.is_finite()
                    || viewport.min_element() <= 0.
                    || !transform.is_finite()
                    || !transform.inverse().is_finite()
                {
                    return invalid("invalid text update view");
                }
                let view = camera.view(viewport);
                if !view.view.is_finite() || !view.proj.is_finite() {
                    return invalid("invalid text update camera");
                }
                let mut updates = BTreeMap::new();
                for id in ids {
                    let original = doc
                        .def(def)
                        .unwrap()
                        .texts
                        .get(&id)
                        .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))?;
                    let mut text = if matches!(original.placement, crate::text::TextPlacement::Attached { .. }) {
                        original.with_leader(doc, def, transform, &view, defaults.leader).ok_or_else(|| {
                            CommandError::Invalid("cannot convert a selected text leader in this view".into())
                        })?
                    } else {
                        original.clone()
                    };
                    defaults.apply_appearance(&mut text);
                    updates.insert(id, text);
                }
                out.changed = false;
                for (id, text) in updates {
                    let previous = doc.def_mut(def).unwrap().texts.get_mut(&id).unwrap();
                    out.changed |= *previous != text;
                    *previous = text;
                }
            }
            Command::ApplyTextAppearance { ids } => {
                let defaults = doc.style.text_defaults;
                let definition = doc.def_mut(def).unwrap();
                if ids.iter().any(|id| !definition.texts.contains_key(id)) {
                    return invalid("text not found in this context");
                }
                out.changed = false;
                for id in ids {
                    let text = definition.texts.get_mut(&id).unwrap();
                    let before = text.clone();
                    defaults.apply_appearance(text);
                    out.changed |= *text != before;
                }
            }
            Command::AddText { text } => {
                if !text.validate() {
                    return invalid("invalid text annotation");
                }
                let id = rysmo_mesh::TextId(doc.reserve_id());
                doc.def_mut(def).unwrap().texts.insert(id, text);
                out.texts.push(id);
                out.created.push(Element::Text(id));
            }
            Command::SetText { id, text } => {
                if !text.validate() {
                    return invalid("invalid text annotation");
                }
                let previous = doc
                    .def_mut(def)
                    .unwrap()
                    .texts
                    .get_mut(&id)
                    .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))?;
                out.changed = *previous != text;
                *previous = text;
            }
            Command::EraseTexts { ids } => {
                let d = doc.def_mut(def).unwrap();
                if ids.iter().any(|id| !d.texts.contains_key(id)) {
                    return invalid("text not found in this context");
                }
                out.changed = !ids.is_empty();
                for id in ids {
                    d.texts.remove(&id);
                }
            }
            Command::AddDimension { dimension } => {
                if !dimension.validate() {
                    return invalid("invalid dimension geometry");
                }
                let id = rysmo_mesh::DimensionId(doc.reserve_id());
                doc.def_mut(def).expect("checked").dimensions.insert(id, dimension);
                out.dimensions.push(id);
                out.created.push(Element::Dimension(id));
            }
            Command::SetDimension { id, dimension } => {
                if !dimension.validate() {
                    return invalid("invalid dimension geometry");
                }
                let previous = doc
                    .def_mut(def)
                    .expect("checked")
                    .dimensions
                    .get_mut(&id)
                    .ok_or_else(|| CommandError::Invalid(format!("no dimension {id} here")))?;
                out.changed = *previous != dimension;
                *previous = dimension;
            }
            Command::ApplyDimensionDefaults { ids } => {
                out.changed = false;
                let defaults = doc.style.dimension_defaults;
                let d = doc.def_mut(def).expect("checked");
                if ids.iter().any(|id| !d.dimensions.contains_key(id)) {
                    return invalid("dimension not found in this context");
                }
                for id in ids {
                    let dimension = d.dimensions.get_mut(&id).unwrap();
                    let previous = dimension.clone();
                    defaults.apply(dimension);
                    out.changed |= *dimension != previous;
                }
            }
            Command::EraseDimensions { ids } => {
                let d = doc.def_mut(def).expect("checked");
                if ids.iter().any(|id| !d.dimensions.contains_key(id)) {
                    return invalid("dimension not found in this context");
                }
                out.changed = !ids.is_empty();
                for id in ids {
                    d.dimensions.remove(&id);
                }
            }
            Command::SetCurveSegments { curve, segments } => {
                out.created = mesh_op(doc, &mut |mesh| mesh.set_curve_segments(curve, segments).map(edges))?;
            }
            Command::SetCurveRadius { curve, radius } => {
                mesh_op(doc, &mut |mesh| mesh.set_curve_radius(curve, radius).map(|_| Vec::new()))?;
            }
            Command::DrawFreehand { points, normal } => {
                if points.len() < 2
                    || !points.iter().all(|p| p.is_finite())
                    || !normal.is_finite()
                    || normal.length() < EPS
                {
                    return invalid("a curve needs at least two points");
                }
                let lines: Vec<_> = points.windows(2).map(|p| (p[0], p[1])).collect();
                out.created = mesh_op(doc, &mut |m| {
                    let members = m.add_lines(&lines, Some(normal))?;
                    m.register_curve(
                        &members,
                        rysmo_mesh::Curve {
                            kind: rysmo_mesh::CurveKind::Polyline,
                            faceted: false,
                            centre: points[0],
                            normal,
                            start: DVec3::X,
                            radius: 0.0,
                            segments: (points.len() - 1) as u32,
                            sweep: 0.0,
                        },
                    )?;
                    Ok(edges(members))
                })?;
                out.created.extend(new_faces(doc));
            }
            Command::DrawLines { lines, facing } => {
                let lines: Vec<_> = lines.into_iter().map(|[a, b]| (a, b)).collect();
                out.created = mesh_op(doc, &mut |m| m.add_lines(&lines, facing).map(edges))?;
                out.created.extend(new_faces(doc));
            }
            Command::DrawRectangle { corner, u, v, facing } => {
                if u.length() < EPS || v.length() < EPS || u.cross(v).length() < EPS * EPS {
                    return invalid("the rectangle has no area");
                }
                let p = [corner, corner + u, corner + u + v, corner + v];
                let lines: Vec<_> = (0..4).map(|i| (p[i], p[(i + 1) % 4])).collect();
                out.created = mesh_op(doc, &mut |m| m.add_lines(&lines, facing).map(edges))?;
                out.created.extend(new_faces(doc));
            }
            Command::DrawCircle { centre, normal, radius, segments, start }
            | Command::DrawPolygon { centre, normal, radius, segments, start } => {
                if !radius.is_finite()
                    || !centre.is_finite()
                    || !normal.is_finite()
                    || radius < EPS
                    || segments < 3
                    || normal.length() < EPS
                {
                    return invalid("a circle needs a radius and at least 3 segments");
                }
                let n = normal.normalize();
                let plane = Plane::new(n, centre);
                let u = start
                    .map(|s| s - n * s.dot(n))
                    .filter(|s| s.length() > EPS)
                    .map(|s| s.normalize())
                    .unwrap_or_else(|| plane.basis().0);
                let v = n.cross(u);
                let pts: Vec<DVec3> = (0..segments)
                    .map(|i| {
                        let a = i as f64 / segments as f64 * std::f64::consts::TAU;
                        centre + (u * a.cos() + v * a.sin()) * radius
                    })
                    .collect();
                let k = pts.len();
                let lines: Vec<_> = (0..k).map(|i| (pts[i], pts[(i + 1) % k])).collect();
                out.created = mesh_op(doc, &mut |m| {
                    let members = m.add_lines(&lines, Some(n))?;
                    m.register_curve(
                        &members,
                        rysmo_mesh::Curve {
                            faceted: polygon,
                            kind: if polygon { rysmo_mesh::CurveKind::Polygon } else { rysmo_mesh::CurveKind::Circle },
                            centre,
                            normal: n,
                            start: u,
                            radius,
                            segments,
                            sweep: 360.0,
                        },
                    )?;
                    Ok(edges(members))
                })?;
                out.created.extend(new_faces(doc));
            }
            Command::DrawArc { start, end, normal, bulge, segments } => {
                let pts = arc_points(start, end, normal, bulge, segments)
                    .ok_or_else(|| CommandError::Invalid("the arc needs a chord, a bulge and segments".into()))?;
                let lines: Vec<_> = pts.windows(2).map(|w| (w[0], w[1])).collect();
                let chord = end - start;
                let n = normal.normalize();
                let r = (chord.length_squared() * 0.25 + bulge * bulge) / (2.0 * bulge.abs());
                let centre = (start + end) * 0.5 + n.cross(chord.normalize()) * (bulge - bulge.signum() * r);
                let sweep = -bulge.signum() * 4.0 * (2.0 * bulge.abs() / chord.length()).atan().to_degrees();
                out.created = mesh_op(doc, &mut |m| {
                    let members = m.add_lines(&lines, Some(normal))?;
                    m.register_curve(
                        &members,
                        rysmo_mesh::Curve {
                            kind: rysmo_mesh::CurveKind::Arc,
                            faceted: false,
                            centre,
                            normal: n,
                            start: (start - centre).normalize(),
                            radius: r,
                            segments,
                            sweep,
                        },
                    )?;
                    Ok(edges(members))
                })?;
                out.created.extend(new_faces(doc));
            }
            Command::FilletEdges { edges, radius, segments } => {
                out.created = doc
                    .with_mesh(def, |m| m.fillet(edges, radius, segments))
                    .expect("checked")
                    .map_err(CommandError::Invalid)?;
            }
            Command::DrawThreePointArc { start, through, end, normal, segments } => {
                let (arc, points) =
                    crate::arcs::three_point_points(start, through, end, normal, segments).ok_or_else(|| {
                        CommandError::Invalid(
                            "three-point arc needs distinct, non-collinear coplanar points and 2-4096 segments".into(),
                        )
                    })?;
                let lines: Vec<_> = points.windows(2).map(|p| (p[0], p[1])).collect();
                out.created = mesh_op(doc, &mut |mesh| {
                    let members = mesh.add_lines(&lines, Some(arc.normal))?;
                    mesh.register_curve(
                        &members,
                        rysmo_mesh::Curve {
                            kind: rysmo_mesh::CurveKind::Arc,
                            faceted: false,
                            centre: arc.centre,
                            normal: arc.normal,
                            start: arc.start,
                            radius: arc.radius,
                            sweep: arc.sweep,
                            segments,
                        },
                    )?;
                    Ok(edges(members))
                })?;
                out.created.extend(new_faces(doc));
            }
            Command::DrawCentreArc { centre, normal, start, radius, sweep, segments, pie } => {
                let points=centre_arc_points(centre,normal,start,radius,sweep,segments)
                    .ok_or_else(||CommandError::Invalid("arc needs a finite centre/radius, perpendicular start, 2-4096 segments and a nonzero sweep below 360 degrees".into()))?;
                let lines: Vec<_> = points.windows(2).map(|p| (p[0], p[1])).collect();
                out.created = mesh_op(doc, &mut |mesh| {
                    let members = mesh.add_lines(&lines, Some(normal))?;
                    let mut all = members.clone();
                    if pie {
                        all.extend(
                            mesh.add_lines(&[(centre, points[0]), (*points.last().unwrap(), centre)], Some(normal))?,
                        );
                    }
                    mesh.register_curve(
                        &members,
                        rysmo_mesh::Curve {
                            kind: rysmo_mesh::CurveKind::Arc,
                            faceted: false,
                            centre,
                            normal: normal.normalize(),
                            start: start.normalize(),
                            radius,
                            segments,
                            sweep,
                        },
                    )?;
                    all.sort();
                    all.dedup();
                    Ok(edges(all))
                })?;
                out.created.extend(new_faces(doc));
            }
            Command::PushPull { face, distance, keep_base } => {
                out.created = mesh_op(doc, &mut |m| {
                    let r = m.push_pull(face, distance, keep_base)?;
                    let mut c: Vec<Element> = r.sides.into_iter().map(Element::Face).collect();
                    c.extend(r.face.map(Element::Face));
                    Ok(c)
                })?;
            }
            Command::CopyWithText { elements, offset, copies, pivot, view } => {
                if !offset.is_finite()
                    || !pivot.is_finite()
                    || !view.eye.is_finite()
                    || !view.size.is_finite()
                    || view.size.min_element() <= 0.
                    || !view.view.is_finite()
                    || !view.proj.is_finite()
                    || !view.view.inverse().is_finite()
                    || !view.proj.inverse().is_finite()
                    || copies == 0
                    || copies > 10000
                {
                    return invalid("invalid text copy count or view");
                }
                let source = doc.def(def).unwrap().mesh.clone();
                let originals = elements
                    .iter()
                    .filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None })
                    .map(|id| {
                        doc.def(def)
                            .unwrap()
                            .texts
                            .get(&id)
                            .cloned()
                            .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let geometry = elements.iter().copied().filter(|e| !matches!(e, Element::Text(_))).collect::<Vec<_>>();
                let old_instances = geometry
                    .iter()
                    .filter_map(|e| if let Element::Instance(id) = e { Some(*id) } else { None })
                    .collect::<Vec<_>>();
                if old_instances.iter().any(|id| !doc.def(def).unwrap().instances.contains_key(id)) {
                    return invalid("instance not found in this context");
                }
                for k in 1..=copies {
                    let delta = offset * k as f64;
                    let matrix = DMat4::from_translation(delta);
                    let made = if geometry.is_empty() {
                        Vec::new()
                    } else {
                        Command::Copy { elements: geometry.clone(), offset: delta, copies: 1 }.run(doc, def)?.created
                    };
                    let instances = old_instances
                        .iter()
                        .copied()
                        .zip(made.iter().filter_map(|e| if let Element::Instance(id) = e { Some(*id) } else { None }))
                        .collect::<BTreeMap<_, _>>();
                    out.created.extend(made);
                    let pixel_delta = view
                        .project(pivot + delta)
                        .zip(view.project(pivot))
                        .map(|(a, b)| a - b)
                        .ok_or_else(|| CommandError::Invalid("copy pivot is behind the camera".into()))?;
                    for original in &originals {
                        let mut text =
                            original.copied_links(&source, &doc.def(def).unwrap().mesh, matrix, &instances, &geometry);
                        match (&original.placement, &mut text.placement) {
                            (
                                crate::text::TextPlacement::Screen { position: old },
                                crate::text::TextPlacement::Screen { position },
                            ) => *position = *old + pixel_delta / view.size,
                            (
                                crate::text::TextPlacement::Attached { anchor: old, leader: old_leader },
                                crate::text::TextPlacement::Attached { anchor, leader },
                            ) => {
                                let old_point = old.point;
                                let point = anchor.resolve(doc, def).0;
                                match (old_leader, leader) {
                                    (
                                        crate::text::TextLeader::ViewBased { offset: old_offset },
                                        crate::text::TextLeader::ViewBased { offset: label_offset },
                                    ) => {
                                        if point.distance(old_point + delta) < EPS {
                                            *label_offset = *old_offset;
                                        } else {
                                            *label_offset = *old_offset + pixel_delta;
                                        }
                                    }
                                    (
                                        crate::text::TextLeader::Pushpin { offset: old_offset }
                                        | crate::text::TextLeader::None { offset: old_offset },
                                        crate::text::TextLeader::Pushpin { offset: label_offset }
                                        | crate::text::TextLeader::None { offset: label_offset },
                                    ) => *label_offset = old_point + *old_offset + delta - point,
                                    _ => unreachable!(),
                                }
                            }
                            _ => unreachable!(),
                        }
                        if !text.validate() {
                            return invalid("invalid copied text placement");
                        }
                        let id = rysmo_mesh::TextId(doc.reserve_id());
                        doc.def_mut(def).unwrap().texts.insert(id, text);
                        out.created.push(Element::Text(id));
                        out.texts.push(id);
                    }
                }
                out.changed = !out.created.is_empty();
            }
            Command::MoveWithText { elements, offset, pivot, view } => {
                if !offset.is_finite()
                    || !view.eye.is_finite()
                    || !pivot.is_finite()
                    || !view.size.is_finite()
                    || view.size.min_element() <= 0.
                    || !view.view.is_finite()
                    || !view.proj.is_finite()
                    || !view.view.inverse().is_finite()
                    || !view.proj.inverse().is_finite()
                {
                    return invalid("invalid text move view or offset");
                }
                if offset.length() < EPS {
                    if elements
                        .iter()
                        .any(|e| matches!(e,Element::Text(id) if !doc.def(def).unwrap().texts.contains_key(id)))
                    {
                        return invalid("text not found in this context");
                    }
                    out.changed = false;
                    return Ok(out);
                }
                let pixel_delta = view
                    .project(pivot + offset)
                    .zip(view.project(pivot))
                    .map(|(a, b)| a - b)
                    .ok_or_else(|| CommandError::Invalid("move pivot is behind the camera".into()))?;
                let mut targets = Vec::new();
                for element in &elements {
                    if let Element::Text(id) = element {
                        let text = doc
                            .def(def)
                            .unwrap()
                            .texts
                            .get(id)
                            .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))?
                            .clone();
                        let layout = text
                            .layout(doc, def, DMat4::IDENTITY, &view)
                            .ok_or_else(|| CommandError::Invalid("text cannot be moved in this view".into()))?;
                        let position = match &text.placement {
                            crate::text::TextPlacement::Screen { .. } => layout.position + pixel_delta,
                            crate::text::TextPlacement::Attached { anchor, leader } => {
                                let point = anchor.resolve(doc, def).0;
                                match leader {
                                    crate::text::TextLeader::ViewBased { .. } => layout.position + pixel_delta,
                                    crate::text::TextLeader::Pushpin { offset: label_offset }
                                    | crate::text::TextLeader::None { offset: label_offset } => {
                                        view.project(point + *label_offset + offset).ok_or_else(|| {
                                            CommandError::Invalid("moved text is behind the camera".into())
                                        })?
                                    }
                                }
                            }
                        };
                        targets.push((*id, text, position));
                    }
                }
                let geometry: Vec<_> = elements.iter().copied().filter(|e| !matches!(e, Element::Text(_))).collect();
                out.changed = if geometry.is_empty() {
                    false
                } else {
                    Command::Move { elements: geometry, offset }.run(doc, def)?.changed
                };
                for (id, mut original, mut position) in targets {
                    if let crate::text::TextPlacement::Attached {
                        anchor,
                        leader: crate::text::TextLeader::ViewBased { offset: label_offset },
                    } = &original.placement
                    {
                        let old = anchor.point;
                        let new = anchor.resolve(doc, def).0;
                        if new.distance(old + offset) < EPS && new.distance(old) > EPS {
                            position = view
                                .project(new)
                                .ok_or_else(|| CommandError::Invalid("moved attachment is behind the camera".into()))?
                                + *label_offset;
                        }
                    }
                    if let crate::text::TextPlacement::Attached {
                        anchor,
                        leader:
                            crate::text::TextLeader::Pushpin { offset: label_offset }
                            | crate::text::TextLeader::None { offset: label_offset },
                    } = &mut original.placement
                    {
                        *label_offset = anchor.point + *label_offset + offset - anchor.resolve(doc, def).0;
                    } else {
                        original = original
                            .at_label_position(doc, def, DMat4::IDENTITY, &view, position)
                            .ok_or_else(|| CommandError::Invalid("invalid moved text placement".into()))?;
                    }
                    if !original.validate() {
                        return invalid("invalid moved text placement");
                    }
                    let stored = doc.def_mut(def).unwrap().texts.get_mut(&id).unwrap();
                    out.changed |= *stored != original;
                    *stored = original;
                }
            }
            Command::Move { elements, offset } => {
                if offset.length() < EPS {
                    out.changed = false;
                } else {
                    let source_mesh = &doc.def(def).unwrap().mesh;
                    let moved_faces: Vec<_> = elements
                        .iter()
                        .filter_map(|element| {
                            let Element::Face(id) = element else { return None };
                            let face = source_mesh.face(*id)?;
                            let outer: Vec<_> = face.outer.iter().map(|v| source_mesh.pos(*v) + offset).collect();
                            let plane = rysmo_math::Plane::from_polygon(&outer)?;
                            let outer: Vec<_> = outer.into_iter().map(|p| plane.to_2d(p)).collect();
                            let holes: Vec<Vec<_>> = face
                                .holes
                                .iter()
                                .map(|hole| hole.iter().map(|v| plane.to_2d(source_mesh.pos(*v) + offset)).collect())
                                .collect();
                            Some((*id, plane, outer, holes))
                        })
                        .collect();
                    let moved_edges: Vec<_> = elements
                        .iter()
                        .filter_map(|e| match e {
                            Element::Edge(id) => doc.def(def).unwrap().mesh.edge(*id).map(|_| {
                                let (a, b) = doc.def(def).unwrap().mesh.edge_points(*id);
                                (*id, a + offset, b + offset)
                            }),
                            _ => None,
                        })
                        .collect();
                    // Apply placement before topology splits so every resulting
                    // region inherits the moved texture frame.
                    let mesh = &mut doc.def_mut(def).expect("checked").mesh;
                    for e in &elements {
                        if let Element::Face(f) = e
                            && let Some(n) = mesh.face_normal(*f)
                            && let Some(face) = mesh.face_mut(*f)
                        {
                            face.uv = Some(crate::texture::moved(face.uv.unwrap_or_default(), n, offset));
                        }
                    }
                    transform_items(doc, def, &elements, DMat4::from_translation(offset))?;
                    let mesh = &doc.def(def).expect("checked").mesh;
                    out.created = elements
                        .into_iter()
                        .flat_map(|element| {
                            if let Element::Edge(id) = element
                                && let Some((_, a, b)) = moved_edges.iter().find(|(old, _, _)| *old == id)
                            {
                                return mesh.edges_along(*a, *b).into_iter().map(Element::Edge).collect::<Vec<_>>();
                            }
                            if let Element::Face(id) = element
                                && let Some((_, plane, outer, holes)) = moved_faces.iter().find(|(old, ..)| *old == id)
                            {
                                return mesh
                                    .faces()
                                    .filter_map(|(id, _)| {
                                        if !mesh.face_plane(id)?.same_surface(plane) {
                                            return None;
                                        }
                                        let triangle = *rysmo_mesh::triangulate_face(mesh, id).first()?;
                                        let point =
                                            plane.to_2d(triangle.iter().map(|v| mesh.pos(*v)).sum::<DVec3>() / 3.);
                                        let inside = rysmo_math::point_in_polygon_2d(point, outer, 1e-9)
                                            != rysmo_math::PolygonSide::Outside
                                            && holes.iter().all(|hole| {
                                                rysmo_math::point_in_polygon_2d(point, hole, 1e-9)
                                                    == rysmo_math::PolygonSide::Outside
                                            });
                                        inside.then_some(Element::Face(id))
                                    })
                                    .collect::<Vec<_>>();
                            }
                            vec![element]
                        })
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect();
                }
            }
            Command::Copy { elements, offset, copies } => {
                if copies == 0 {
                    return invalid("zero copies");
                }
                if offset.length() < EPS {
                    return Err(MeshError::ZeroLength.into());
                }
                out.created = copy_items(doc, def, &elements, copies, |k| DMat4::from_translation(offset * k as f64))?;
            }
            Command::Rotate { elements, centre, axis, angle, copies } => {
                validate_rotation(centre, axis, angle)?;
                if !(angle * copies.max(1) as f64).is_finite() {
                    return invalid("rotation array angle exceeds the numeric range");
                }
                let screen_only=!elements.is_empty() && elements.iter().all(|e| matches!(e,Element::Text(id) if doc.def(def).unwrap().texts.get(id).is_some_and(|text| matches!(text.placement,crate::text::TextPlacement::Screen {..}))));
                if angle.abs() < 1e-9 || copies == 0 && screen_only {
                    out.changed = false;
                } else if copies == 0 {
                    transform_items(doc, def, &elements, rotation(centre, axis, angle))?;
                    out.created = elements;
                } else {
                    out.created =
                        copy_items(doc, def, &elements, copies, |k| rotation(centre, axis, angle * k as f64))?;
                }
            }
            Command::SetAxes { origin, x, y } => {
                let axes = rysmo_math::DrawingAxes::new(origin, x, y)
                    .ok_or_else(|| CommandError::Invalid("axes need finite, independent directions".into()))?;
                let d = doc.def_mut(def).expect("checked");
                out.changed = d.axes != axes;
                d.axes = axes;
            }
            Command::ResizeContext { factor } => {
                if !factor.is_finite() || factor <= 1e-9 {
                    return invalid("resize needs a finite positive factor");
                }
                if (factor - 1.0).abs() < 1e-12 {
                    out.changed = false;
                } else {
                    let d = doc.def(def).expect("checked");
                    let elements: Vec<_> = d
                        .mesh
                        .edges()
                        .map(|(id, _)| Element::Edge(id))
                        .chain(d.mesh.faces().map(|(id, _)| Element::Face(id)))
                        .chain(d.instances.keys().copied().map(Element::Instance))
                        .chain(d.guides.keys().copied().map(Element::Guide))
                        .collect();
                    transform_items(doc, def, &elements, DMat4::from_scale(DVec3::splat(factor)))?;
                    let d = doc.def_mut(def).expect("checked");
                    d.axes.origin *= factor;
                    for plane in d.sections.planes.values_mut() {
                        plane.origin *= factor;
                    }
                    for dimension in d.dimensions.values_mut() {
                        *dimension = dimension.clone().scaled(factor);
                    }
                    for text in d.texts.values_mut() {
                        *text = text.clone().scaled(factor);
                    }
                    let faces: Vec<_> = d.mesh.faces().map(|(id, _)| id).collect();
                    for id in faces {
                        let face = d.mesh.face_mut(id).unwrap();
                        let mut uv = face.uv.unwrap_or_default();
                        uv.offset = uv.offset.map(|v| v * factor);
                        uv.scale = uv.scale.map(|v| v * factor);
                        face.uv = Some(uv);
                    }
                    if def.is_none()
                        && let Some(camera) = &mut doc.camera
                    {
                        camera.target *= factor;
                        camera.distance *= factor;
                        camera.eye_base_z *= factor;
                    }
                }
            }
            Command::Scale { elements, origin, factors } | Command::ScaleInAxes { elements, origin, factors, .. } => {
                if !factors.is_finite()
                    || !origin.is_finite()
                    || factors.x.abs() < 1e-9
                    || factors.y.abs() < 1e-9
                    || factors.z.abs() < 1e-9
                {
                    return invalid("a scale factor of zero flattens the geometry");
                }
                let m = DMat4::from_translation(origin)
                    * scale_basis
                    * DMat4::from_scale(factors)
                    * scale_basis.inverse()
                    * DMat4::from_translation(-origin);
                if !elements.is_empty() && elements.iter().all(|e| matches!(e,Element::Text(id) if doc.def(def).unwrap().texts.get(id).is_some_and(|text| matches!(text.placement,crate::text::TextPlacement::Screen {..})))) {out.changed=false;return Ok(out);}
                transform_items(doc, def, &elements, m)?;
                // A mirror (an odd number of negative factors) turns faces inside out.
                if factors.x * factors.y * factors.z < 0.0 {
                    let faces: Vec<FaceId> = elements
                        .iter()
                        .filter_map(|e| if let Element::Face(f) = e { Some(*f) } else { None })
                        .collect();
                    doc.with_mesh(def, |m| m.reverse_faces(&faces));
                }
                out.created = elements;
            }
            Command::OffsetFace { face, distance } => {
                out.created = mesh_op(doc, &mut |m| m.offset_face(face, distance).map(edges))?;
                out.created.extend(new_faces(doc));
            }
            Command::OffsetEdges { edges: chain, normal, distance } => {
                out.created = mesh_op(doc, &mut |m| m.offset_chain(&chain, normal, distance).map(edges))?;
                out.created.extend(new_faces(doc));
            }
            Command::FollowMe { face, path } => {
                out.created = mesh_op(doc, &mut |m| {
                    let (chain, closed) = m.edge_chain(&path).ok_or(MeshError::NotAChain)?;
                    let pts = m.loop_points(&chain);
                    m.follow_me(face, &pts, closed).map(|fs| fs.into_iter().map(Element::Face).collect())
                })?;
            }
            Command::Erase { elements } => {
                let (geo, inst, guides, dimensions) = split(&elements);
                let texts: Vec<_> =
                    elements.iter().filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None }).collect();
                let d = doc.def(def).expect("checked");
                let sections: Vec<_> = elements
                    .iter()
                    .filter_map(|e| if let Element::Section(id) = e { Some(*id) } else { None })
                    .collect();
                let any = sections.iter().any(|id| d.sections.planes.contains_key(id))
                    || geo.iter().any(|e| d.mesh.contains(*e))
                    || inst.iter().any(|i| d.instances.contains_key(i))
                    || guides.iter().any(|g| d.guides.contains_key(g))
                    || dimensions.iter().any(|id| d.dimensions.contains_key(id))
                    || texts.iter().any(|id| d.texts.contains_key(id));
                if !any {
                    out.changed = false;
                } else {
                    doc.with_mesh(def, |m| m.erase(&geo));
                    let d = doc.def_mut(def).expect("checked");
                    for i in inst {
                        d.instances.remove(&i);
                    }
                    for g in guides {
                        d.guides.remove(&g);
                    }
                    for id in dimensions {
                        d.dimensions.remove(&id);
                    }
                    for id in texts {
                        d.texts.remove(&id);
                    }
                    for id in sections {
                        d.sections.planes.remove(&id);
                        if d.sections.active == Some(id) {
                            d.sections.active = None;
                        }
                    }
                }
            }
            Command::ReverseFaces { faces } => {
                doc.with_mesh(def, |m| m.reverse_faces(&faces));
            }
            Command::SetEdges { edges, soft, smooth, hidden } => {
                doc.with_mesh(def, |m| m.set_edge_flags(&edges, soft, smooth, hidden));
            }
            Command::SoftenEdges { edges, angle } => {
                doc.with_mesh(def, |m| {
                    m.set_edge_flags(&edges, Some(false), Some(false), None);
                    m.soften_by_angle(&edges, angle);
                });
            }
            Command::MakeGroup { elements } => {
                let i = make_definition(doc, def, &elements, true, None)?;
                out.created = vec![Element::Instance(i)];
            }
            Command::MakeComponent { elements, name } => {
                let i = make_definition(doc, def, &elements, false, name)?;
                out.created = vec![Element::Instance(i)];
            }
            Command::Explode { instance } => {
                out.created = explode(doc, def, instance)?;
            }
            Command::MakeUnique { instance } => {
                make_unique(doc, def, instance)?;
                out.created = vec![Element::Instance(instance)];
            }
            Command::SetGeometryHidden { elements, hidden } => {
                let owner = doc.def_mut(def).unwrap();
                out.changed = false;
                for element in elements {
                    match element {
                        Element::Face(id) => {
                            let face = owner
                                .mesh
                                .face_mut(id)
                                .ok_or_else(|| CommandError::Invalid("face is missing".into()))?;
                            out.changed |= face.hidden != hidden;
                            face.hidden = hidden;
                        }
                        Element::Edge(id) => {
                            let edge =
                                owner.mesh.edge(id).ok_or_else(|| CommandError::Invalid("edge is missing".into()))?;
                            out.changed |= edge.hidden != hidden;
                            owner.mesh.set_edge_flags(&[id], None, None, Some(hidden));
                        }
                        Element::Instance(id) => {
                            let instance = owner
                                .instances
                                .get_mut(&id)
                                .ok_or_else(|| CommandError::Invalid("instance is missing".into()))?;
                            out.changed |= instance.hidden != hidden;
                            instance.hidden = hidden;
                        }
                        Element::Guide(id) => {
                            let guide = owner
                                .guides
                                .get_mut(&id)
                                .ok_or_else(|| CommandError::Invalid("guide is missing".into()))?;
                            out.changed |= guide.hidden() != hidden;
                            guide.set_hidden(hidden);
                        }
                        _ => return invalid("Only geometry, guides and instances can be hidden."),
                    }
                }
            }
            Command::SetInstancesHidden { instances, hidden } => {
                let owner = doc.def_mut(def).unwrap();
                out.changed = false;
                for id in instances {
                    let instance = owner
                        .instances
                        .get_mut(&id)
                        .ok_or_else(|| CommandError::Invalid("instance is missing".into()))?;
                    out.changed |= instance.hidden != hidden;
                    instance.hidden = hidden;
                }
            }
            Command::SetComponentProperties { definition, name, settings } => {
                if settings.cut_opening {
                    if settings.glue == crate::components::GlueMode::None {
                        return invalid("Choose a glue orientation before enabling Cut Opening.");
                    }
                    let mut proposed = doc
                        .def(Some(definition))
                        .ok_or_else(|| CommandError::Invalid("component is missing".into()))?
                        .clone();
                    proposed.component = settings.clone();
                    crate::components::opening_outline(&proposed)?;
                }
                if name.trim().is_empty() || !settings.insertion_axes.valid() {
                    return invalid("invalid component properties");
                }
                let definition = doc
                    .definitions
                    .get_mut(&definition)
                    .ok_or_else(|| CommandError::Invalid("component is missing".into()))?;
                out.changed = definition.name != name || definition.component != settings;
                definition.name = name;
                definition.component = settings;
            }
            Command::DeleteDefinition { definition } => {
                if doc.instance_count(definition) > 0 {
                    return invalid("delete the component instances first");
                }
                out.changed = doc.definitions.remove(&definition).is_some();
            }
            Command::RenameInstance { instance, name } => {
                let inst = doc
                    .def_mut(def)
                    .expect("checked")
                    .instances
                    .get_mut(&instance)
                    .ok_or_else(|| CommandError::Invalid(format!("no instance {instance}")))?;
                inst.name = name;
            }
            Command::UnglueInstances { instances } => {
                out.changed = false;
                for id in instances {
                    let instance = doc
                        .def_mut(def)
                        .unwrap()
                        .instances
                        .get_mut(&id)
                        .ok_or_else(|| CommandError::Invalid("instance is missing".into()))?;
                    out.changed |= instance.glued_to.take().is_some();
                }
            }
            Command::PlaceGluedInstance { definition, face, origin, rotation, scale } => {
                if Some(definition) == def || def.is_some_and(|d| doc.contains_definition(definition, d)) {
                    return invalid("A component cannot contain itself.");
                }
                let transform =
                    crate::components::glued_transform(doc, def, definition, face, origin, rotation, scale)?;
                let id = InstanceId(doc.reserve_id());
                doc.def_mut(def).unwrap().instances.insert(
                    id,
                    Instance {
                        definition,
                        transform,
                        name: String::new(),
                        tag: None,
                        material: None,
                        hidden: false,
                        glued_to: Some(face),
                    },
                );
                out.created = vec![Element::Instance(id)];
            }
            Command::PlaceInstance { definition, origin, rotation, scale } => {
                if !doc.definitions.contains_key(&definition) {
                    return invalid(format!("no definition {definition}"));
                }
                if !origin.is_finite() || !rotation.is_finite() || !scale.is_finite() || scale.abs() < 1e-9 {
                    return invalid("the placement must be finite and the scale must not be zero");
                }
                if Some(definition) == def || def.is_some_and(|d| doc.contains_definition(definition, d)) {
                    return invalid("a component cannot contain itself");
                }
                let transform = DMat4::from_translation(origin)
                    * DMat4::from_rotation_z(rotation.to_radians())
                    * DMat4::from_scale(DVec3::splat(scale))
                    * doc.definitions[&definition].component.frame().inverse();
                let id = InstanceId(doc.reserve_id());
                doc.def_mut(def).expect("checked").instances.insert(
                    id,
                    Instance {
                        glued_to: None,
                        definition,
                        transform,
                        name: String::new(),
                        tag: None,
                        material: None,
                        hidden: false,
                    },
                );
                out.created = vec![Element::Instance(id)];
            }
            Command::AddGuideLine { origin, dir } => {
                if dir.length() < EPS {
                    return invalid("a guide line needs a direction");
                }
                let id = GuideId(doc.reserve_id());
                doc.def_mut(def)
                    .expect("checked")
                    .guides
                    .insert(id, Guide::Line { origin, dir: dir.normalize(), hidden: false });
                out.created = vec![Element::Guide(id)];
            }
            Command::AddGuidePoint { point } => {
                let id = GuideId(doc.reserve_id());
                doc.def_mut(def)
                    .expect("checked")
                    .guides
                    .insert(id, Guide::Point { p: point, origin: None, hidden: false });
                out.created = vec![Element::Guide(id)];
            }
            Command::AddGuideSegment { start, point } => {
                if !start.is_finite() || !point.is_finite() {
                    return invalid("Invalid guide coordinates");
                }
                let id = GuideId(doc.reserve_id());
                doc.def_mut(def)
                    .expect("checked")
                    .guides
                    .insert(id, Guide::Point { p: point, origin: Some(start), hidden: false });
                out.created = vec![Element::Guide(id)];
            }
            Command::DeleteGuides => {
                let d = doc.def_mut(def).expect("checked");
                out.changed = !d.guides.is_empty();
                d.guides.clear();
            }
            Command::AddMaterial { name, color, texture } => {
                check_texture(doc, texture.as_ref())?;
                let id = MaterialId(doc.reserve_id());
                doc.materials.insert(id, Material { name, color, texture, glow: 0.0 });
                out.material = Some(id);
            }
            Command::AddTexturedMaterial { name, color, image_name, mime, data, size } => {
                if data.is_empty() || !size.iter().all(|v| v.is_finite() && *v > 0.0) {
                    return invalid("a texture needs image data and a finite positive size");
                }
                let existing = doc
                    .images
                    .iter()
                    .find_map(|(id, image)| (image.mime == mime && image.bytes.as_ref() == &data).then_some(*id));
                let image = if let Some(image) = existing {
                    image
                } else {
                    let id = rysmo_mesh::ImageId(doc.reserve_id());
                    doc.images.insert(id, Image { name: image_name, mime, bytes: Arc::new(data) });
                    id
                };
                let material = MaterialId(doc.reserve_id());
                doc.materials
                    .insert(material, Material { name, color, texture: Some(Texture { image, size }), glow: 0.0 });
                out.image = Some(image);
                out.material = Some(material);
            }
            Command::AddImage { name, mime, data } => {
                if data.is_empty() {
                    return invalid("the image is empty");
                }
                let id = ImageId(doc.reserve_id());
                doc.images.insert(id, Image { name, mime, bytes: Arc::new(data) });
                out.image = Some(id);
            }
            Command::SetMaterialTexture { material, texture } => {
                check_texture(doc, texture.as_ref())?;
                let m = doc
                    .materials
                    .get_mut(&material)
                    .ok_or_else(|| CommandError::Invalid(format!("no material {material}")))?;
                m.texture = texture;
            }
            Command::SetMaterialGlow { material, glow } => {
                if !glow.is_finite() || !(0.0..=MAX_GLOW).contains(&glow) {
                    return invalid("a glow must be between 0 and 1000000 cd/m²");
                }
                let m = doc
                    .materials
                    .get_mut(&material)
                    .ok_or_else(|| CommandError::Invalid(format!("no material {material}")))?;
                m.glow = glow;
            }
            Command::SetTexturePlacement { faces, placement } => {
                if let Some(p) = placement
                    && !crate::texture::valid_placement(&p)
                {
                    return invalid("a texture placement needs finite values, nonzero scale and a convex pin layout");
                }
                let d = doc.def_mut(def).expect("checked");
                out.changed = false;
                for f in faces {
                    let face = d.mesh.face_mut(f).ok_or_else(|| CommandError::Invalid(format!("no face {f}")))?;
                    out.changed |= face.uv != placement;
                    face.uv = placement;
                }
            }
            Command::CreateSectionSlice { id } => {
                let plane = doc
                    .def(def)
                    .unwrap()
                    .sections
                    .planes
                    .get(&id)
                    .ok_or_else(|| CommandError::Invalid("section plane is missing".into()))?;
                let lines = crate::sections::slice(doc, def, plane)
                    .map_err(CommandError::Invalid)?
                    .into_iter()
                    .filter(|line| line[0].distance(line[1]) > EPS)
                    .collect::<Vec<_>>();
                if lines.is_empty() {
                    return invalid("section plane does not intersect geometry");
                }
                let mut mesh = Mesh::new();
                mesh.ensure_next_id(doc.next_id);
                mesh.add_lines(&lines.into_iter().map(|line| (line[0], line[1])).collect::<Vec<_>>(), None)?;
                doc.next_id = doc.next_id.max(mesh.next_id());
                let mesh = Mesh::from_parts(
                    mesh.vertices().map(|(id, v)| (id, v.clone())).collect(),
                    mesh.edges().map(|(id, e)| (id, e.clone())).collect(),
                    BTreeMap::new(),
                )?;
                let definition = DefinitionId(doc.reserve_id());
                doc.definitions.insert(
                    definition,
                    Definition { name: "Section Slice".into(), group: true, mesh, ..Default::default() },
                );
                let instance = InstanceId(doc.reserve_id());
                doc.def_mut(def).unwrap().instances.insert(
                    instance,
                    Instance {
                        glued_to: None,
                        definition,
                        transform: DMat4::IDENTITY,
                        name: "Section Slice".into(),
                        tag: None,
                        material: None,
                        hidden: false,
                    },
                );
                out.created.push(Element::Instance(instance));
            }
            Command::AddSection { plane } => {
                if !plane.valid() {
                    return invalid("invalid section plane");
                }
                let id = doc.reserve_id();
                let sections = &mut doc.def_mut(def).unwrap().sections;
                sections.planes.insert(id, plane);
                sections.active = Some(id);
                out.created.push(Element::Section(id));
            }
            Command::SetSection { id, plane } => {
                let sections = &mut doc.def_mut(def).unwrap().sections;
                if !sections.planes.contains_key(&id) {
                    return invalid("section plane is missing");
                }
                if let Some(plane) = plane {
                    if !plane.valid() {
                        return invalid("invalid section plane");
                    }
                    out.changed = sections.planes.get(&id) != Some(&plane);
                    sections.planes.insert(id, plane);
                } else {
                    sections.planes.remove(&id);
                    if sections.active == Some(id) {
                        sections.active = None;
                    }
                }
            }
            Command::ActivateSection { id } => {
                let sections = &mut doc.def_mut(def).unwrap().sections;
                if id.is_some_and(|id| !sections.planes.contains_key(&id)) {
                    return invalid("section plane is missing");
                }
                out.changed = sections.active != id;
                sections.active = id;
            }
            Command::IntersectFaces { elements, with_model, context } => {
                if doc.resolve(&context).map_err(|_| CommandError::Invalid("invalid intersection context".into()))?
                    != def
                {
                    return invalid("intersection context does not match the editor");
                }
                let lines =
                    crate::intersect::lines(doc, &context, &elements, with_model).map_err(CommandError::Invalid)?;
                let before = doc.def(def).unwrap().mesh.clone();
                if lines.is_empty() {
                    out.changed = false;
                } else {
                    out.created = mesh_op(doc, &mut |m| m.add_lines(&lines, None).map(edges))?;
                    out.created.extend(new_faces(doc));
                    out.changed = before != doc.def(def).unwrap().mesh;
                }
            }
            Command::AddScene { mut scene } => {
                scene.id = doc.reserve_id();
                if !scene.valid() {
                    return invalid("invalid scene");
                }
                doc.scenes.push(scene);
            }
            Command::SetScene { id, scene } => {
                let index = doc
                    .scenes
                    .iter()
                    .position(|s| s.id == id)
                    .ok_or_else(|| CommandError::Invalid("scene is missing".into()))?;
                if let Some(scene) = scene {
                    if scene.id != id || !scene.valid() {
                        return invalid("invalid scene");
                    }
                    out.changed = doc.scenes[index] != scene;
                    doc.scenes[index] = scene;
                } else {
                    doc.scenes.remove(index);
                }
            }
            Command::RestoreScene { id } => {
                let scene = doc
                    .scenes
                    .iter()
                    .find(|s| s.id == id)
                    .cloned()
                    .ok_or_else(|| CommandError::Invalid("scene is missing".into()))?;
                scene.restore(doc);
            }
            Command::ReorderScene { id, index } => {
                let old = doc
                    .scenes
                    .iter()
                    .position(|s| s.id == id)
                    .ok_or_else(|| CommandError::Invalid("scene is missing".into()))?;
                if index >= doc.scenes.len() {
                    return invalid("invalid scene order");
                }
                out.changed = old != index;
                let scene = doc.scenes.remove(old);
                doc.scenes.insert(index, scene);
            }
            Command::SetStyle { style } => {
                if !style.valid_sections()
                    || !style.valid_render()
                    || !style.dimensions.validate()
                    || !style.dimension_defaults.validate()
                    || !style.text_defaults.validate()
                {
                    return Err(CommandError::Invalid("invalid display settings".into()));
                }
                out.changed = doc.style != style;
                doc.style = style;
            }
            Command::SetMaterial { material, name, color } => {
                let m = doc
                    .materials
                    .get_mut(&material)
                    .ok_or_else(|| CommandError::Invalid(format!("no material {material}")))?;
                if let Some(n) = name {
                    m.name = n;
                }
                if let Some(c) = color {
                    m.color = c;
                }
            }
            Command::Paint { faces, material, back } => {
                check_material(doc, material)?;
                doc.with_mesh(def, |m| faces.iter().try_for_each(|f| m.paint(*f, material, back))).expect("checked")?;
            }
            Command::PaintWrappedElements { elements, material, back, source, placement } => {
                check_material(doc, material)?;
                let size = material
                    .and_then(|m| doc.materials.get(&m))
                    .and_then(|m| m.texture)
                    .ok_or_else(|| CommandError::Invalid("Wrapping requires a textured material.".into()))?
                    .size;
                let faces = elements
                    .iter()
                    .filter_map(|e| if let Element::Face(f) = e { Some(*f) } else { None })
                    .collect::<Vec<_>>();
                let placements =
                    crate::texture::wrapped_placements(&doc.def(def).unwrap().mesh, source, &faces, size, placement)
                        .map_err(CommandError::Invalid)?;
                let result = Command::PaintElements { elements, material, back }.run(doc, def)?;
                for (face, placement) in placements {
                    doc.def_mut(def).unwrap().mesh.face_mut(face).unwrap().uv = Some(placement);
                }
                return Ok(result);
            }
            Command::PaintPositionedElements { elements, material, back, placement } => {
                let faces =
                    elements.iter().filter_map(|e| if let Element::Face(f) = e { Some(*f) } else { None }).collect();
                let outcome = Command::PaintElements { elements, material, back }.run(doc, def)?;
                Command::SetTexturePlacement { faces, placement: Some(placement) }.run(doc, def)?;
                return Ok(outcome);
            }
            Command::PaintElements { elements, material, back } => {
                check_material(doc, material)?;
                let faces: Vec<_> =
                    elements.iter().filter_map(|e| if let Element::Face(f) = e { Some(*f) } else { None }).collect();
                let instances: Vec<_> = elements
                    .iter()
                    .filter_map(|e| if let Element::Instance(i) = e { Some(*i) } else { None })
                    .collect();
                let edges: Vec<_> =
                    elements.iter().filter_map(|e| if let Element::Edge(id) = e { Some(*id) } else { None }).collect();
                if faces.is_empty() && instances.is_empty() && edges.is_empty() {
                    return invalid("Select edges, faces, groups or components to paint.");
                }
                doc.with_mesh(def, |m| {
                    faces.iter().try_for_each(|f| m.paint(*f, material, back))?;
                    edges.iter().try_for_each(|e| m.paint_edge(*e, material))
                })
                .expect("checked")?;
                let d = doc.def_mut(def).expect("checked");
                for id in instances {
                    d.instances
                        .get_mut(&id)
                        .ok_or_else(|| CommandError::Invalid(format!("no instance {id}")))?
                        .material = material;
                }
            }
            Command::PaintInstances { instances, material } => {
                check_material(doc, material)?;
                let d = doc.def_mut(def).expect("checked");
                for i in instances {
                    d.instances
                        .get_mut(&i)
                        .ok_or_else(|| CommandError::Invalid(format!("no instance {i}")))?
                        .material = material;
                }
            }
            Command::AddTag { name } => {
                if name.trim().is_empty() {
                    return invalid("a tag needs a name");
                }
                let id = TagId(doc.reserve_id());
                doc.tags.insert(id, Tag { name, visible: true });
                out.tag = Some(id);
            }
            Command::SetTag { tag, name, visible } => {
                let t = doc.tags.get_mut(&tag).ok_or_else(|| CommandError::Invalid(format!("no tag {tag}")))?;
                if let Some(n) = name {
                    t.name = n;
                }
                if let Some(v) = visible {
                    t.visible = v;
                }
            }
            Command::DeleteTag { tag } => {
                if doc.tags.remove(&tag).is_none() {
                    return invalid(format!("no tag {tag}"));
                }
                for d in std::iter::once(&mut doc.root).chain(doc.definitions.values_mut()) {
                    for i in d.instances.values_mut() {
                        if i.tag == Some(tag) {
                            i.tag = None;
                        }
                    }
                }
            }
            Command::AssignTag { instances, tag } => {
                if let Some(t) = tag
                    && !doc.tags.contains_key(&t)
                {
                    return invalid(format!("no tag {t}"));
                }
                let d = doc.def_mut(def).expect("checked");
                for i in instances {
                    d.instances.get_mut(&i).ok_or_else(|| CommandError::Invalid(format!("no instance {i}")))?.tag = tag;
                }
            }
            Command::SetMeasurementSettings { settings } => {
                if !settings.valid() {
                    return invalid("Invalid measurement settings.");
                }
                out.changed = doc.measurement != settings;
                doc.measurement = settings;
            }
            Command::SetLengthPrecision { precision } => {
                if precision.is_some_and(|p| p > 6) {
                    return invalid("Length precision must be between 0 and 6 decimal places.");
                }
                out.changed = doc.length_precision != precision;
                doc.length_precision = precision;
            }
            Command::SetUnits { units } => {
                out.changed = doc.units != units;
                doc.units = units;
            }
        }
        crate::components::refresh_glue(doc, &glued_before);
        crate::components::validate_openings(doc)?;
        purge_unused(doc);
        Ok(out)
    }
}

fn check_texture(doc: &Document, texture: Option<&Texture>) -> Result<(), CommandError> {
    match texture {
        Some(t) if !doc.images.contains_key(&t.image) => invalid(format!("no image {}", t.image)),
        Some(t) if !t.size.iter().all(|v| v.is_finite() && *v > 0.0) => {
            invalid("a texture needs a finite positive size")
        }
        _ => Ok(()),
    }
}

fn check_material(doc: &Document, material: Option<MaterialId>) -> Result<(), CommandError> {
    match material {
        Some(m) if !doc.materials.contains_key(&m) => invalid(format!("no material {m}")),
        _ => Ok(()),
    }
}

/// Applies an affine map to geometry, instances and guides of a definition.
pub(crate) fn transform_items(
    doc: &mut Document,
    def: Option<DefinitionId>,
    elements: &[Element],
    m: DMat4,
) -> Result<(), CommandError> {
    let (geo, inst, guides, dimensions) = split(elements);
    let translation = m.x_axis.truncate().distance(DVec3::X) < 1e-10
        && m.y_axis.truncate().distance(DVec3::Y) < 1e-10
        && m.z_axis.truncate().distance(DVec3::Z) < 1e-10;
    if !m.is_finite() || !m.inverse().is_finite() {
        return invalid("transform needs a finite, invertible matrix");
    }
    let text_ids =
        elements.iter().filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None }).collect::<Vec<_>>();
    let original_texts = text_ids
        .iter()
        .map(|id| {
            doc.def(def)
                .unwrap()
                .texts
                .get(id)
                .cloned()
                .map(|text| (*id, text))
                .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let source_mesh = (!text_ids.is_empty() || !dimensions.is_empty() && !translation)
        .then(|| doc.def(def).expect("checked").mesh.clone());
    let vertices = source_mesh.as_ref().map(|mesh| mesh.vertices_of(&geo)).unwrap_or_default();
    let original_dimensions: Vec<_> = dimensions
        .iter()
        .map(|id| {
            let dimension = doc
                .def(def)
                .expect("checked")
                .dimensions
                .get(id)
                .cloned()
                .ok_or_else(|| CommandError::Invalid(format!("no dimension {id} here")))?;
            let measured = dimension.resolve(doc, def);
            let origin =
                if matches!(dimension.measure, crate::dimensions::DimensionMeasure::Radial { diameter: true, .. }) {
                    (measured.from + measured.to) * 0.5
                } else {
                    measured.from
                };
            Ok((*id, dimension, origin))
        })
        .collect::<Result<_, CommandError>>()?;
    if !geo.is_empty() {
        doc.with_mesh(def, |mesh| mesh.transform_elements(&geo, |p| m.transform_point3(p))).expect("checked")?;
    }
    let sections: Vec<_> =
        elements.iter().filter_map(|e| if let Element::Section(id) = e { Some(*id) } else { None }).collect();
    for id in sections {
        let plane = doc
            .def_mut(def)
            .unwrap()
            .sections
            .planes
            .get_mut(&id)
            .ok_or_else(|| CommandError::Invalid("section plane is missing".into()))?;
        *plane = plane.transformed(m);
    }
    let d = doc.def_mut(def).expect("checked");
    for i in inst.iter().copied() {
        if let Some(x) = d.instances.get_mut(&i) {
            x.transform = m * x.transform;
        }
    }
    for g in guides {
        if let Some(x) = d.guides.get_mut(&g) {
            *x = map_guide(x, &m);
        }
    }
    for (id, text) in original_texts {
        let text = text.transformed_links(source_mesh.as_ref().unwrap(), m, &vertices, &inst);
        if !text.validate() {
            return invalid("invalid transformed text");
        }
        doc.def_mut(def).unwrap().texts.insert(id, text);
    }
    for (id, mut dimension, original_origin) in original_dimensions {
        if !translation {
            dimension = dimension.transformed_links(source_mesh.as_ref().unwrap(), m, &vertices, &inst);
            if !dimension.validate() {
                return invalid("transformed dimension has invalid placement");
            }
            doc.def_mut(def).expect("checked").dimensions.insert(id, dimension);
            continue;
        }
        let measured = dimension.resolve(doc, def);
        let origin = if matches!(dimension.measure, crate::dimensions::DimensionMeasure::Radial { diameter: true, .. })
        {
            (measured.from + measured.to) * 0.5
        } else {
            measured.from
        };
        dimension.offset += m.transform_point3(original_origin) - origin;
        if !dimension.validate() {
            return invalid("dimension placement must stay in its plane");
        }
        doc.def_mut(def).expect("checked").dimensions.insert(id, dimension);
    }
    Ok(())
}

/// Copies geometry, instances and guides `copies` times, the k-th mapped by `m(k)`.
fn copy_items(
    doc: &mut Document,
    def: Option<DefinitionId>,
    elements: &[Element],
    copies: u32,
    m: impl Fn(usize) -> DMat4,
) -> Result<Vec<Element>, CommandError> {
    let (geo, inst, guides, dimensions) = split(elements);
    if dimensions.iter().any(|id| !doc.def(def).expect("checked").dimensions.contains_key(id)) {
        return invalid("dimension not found in this context");
    }
    let sections = elements
        .iter()
        .filter_map(|e| if let Element::Section(id) = e { Some(*id) } else { None })
        .map(|id| {
            doc.def(def)
                .unwrap()
                .sections
                .planes
                .get(&id)
                .cloned()
                .ok_or_else(|| CommandError::Invalid("section plane is missing".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let texts = elements
        .iter()
        .filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None })
        .map(|id| {
            doc.def(def)
                .unwrap()
                .texts
                .get(&id)
                .cloned()
                .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let source_mesh =
        (!dimensions.is_empty() || !texts.is_empty()).then(|| doc.def(def).expect("checked").mesh.clone());
    let originals = dimensions
        .iter()
        .map(|id| {
            let dimension = doc.def(def).expect("checked").dimensions[id].clone();
            let measured = dimension.resolve(doc, def);
            let origin =
                if matches!(dimension.measure, crate::dimensions::DimensionMeasure::Radial { diameter: true, .. }) {
                    (measured.from + measured.to) * 0.5
                } else {
                    measured.from
                };
            (dimension, origin)
        })
        .collect::<Vec<_>>();
    let copied_edges = geo
        .iter()
        .filter(|_| source_mesh.is_some())
        .flat_map(|element| match element {
            Element::Edge(id) => vec![*id],
            Element::Face(id) => source_mesh.as_ref().unwrap().face_edges(*id),
            _ => Vec::new(),
        })
        .collect::<BTreeSet<_>>();
    let mut out = Vec::new();
    if !geo.is_empty() {
        let made = doc
            .with_mesh(def, |mesh| mesh.copy_elements_with(&geo, copies as usize, |p, k| m(k).transform_point3(p)))
            .expect("checked")?;
        out.extend(made);
    }
    for k in 1..=copies as usize {
        let mk = m(k);
        let translation =
            [DVec3::X, DVec3::Y, DVec3::Z].iter().all(|axis| mk.transform_vector3(*axis).distance(*axis) < 1e-10);
        for plane in &sections {
            let id = doc.reserve_id();
            doc.def_mut(def).unwrap().sections.planes.insert(id, plane.transformed(mk));
            out.push(Element::Section(id));
        }
        let mut instance_map = BTreeMap::new();
        for i in &inst {
            let Some(x) = doc.def(def).expect("checked").instances.get(i).cloned() else { continue };
            let id = InstanceId(doc.reserve_id());
            doc.def_mut(def).expect("checked").instances.insert(id, Instance { transform: mk * x.transform, ..x });
            instance_map.insert(*i, id);
            out.push(Element::Instance(id));
        }
        for (original, old_origin) in &originals {
            let mut dimension = original.copied_links(
                source_mesh.as_ref().unwrap(),
                &doc.def(def).expect("checked").mesh,
                mk,
                &instance_map,
                &copied_edges,
            );
            let measured = dimension.resolve(doc, def);
            let new_origin =
                if matches!(dimension.measure, crate::dimensions::DimensionMeasure::Radial { diameter: true, .. }) {
                    (measured.from + measured.to) * 0.5
                } else {
                    measured.from
                };
            if translation {
                dimension.offset += mk.transform_point3(*old_origin) - new_origin;
            }
            if !dimension.validate() {
                return invalid("copied dimension placement must stay in its plane");
            }
            let id = rysmo_mesh::DimensionId(doc.reserve_id());
            doc.def_mut(def).expect("checked").dimensions.insert(id, dimension);
            out.push(Element::Dimension(id));
        }
        for original in &texts {
            let text = original.copied_links(
                source_mesh.as_ref().unwrap(),
                &doc.def(def).unwrap().mesh,
                mk,
                &instance_map,
                &geo,
            );
            if !text.validate() {
                return invalid("invalid copied text");
            }
            let id = rysmo_mesh::TextId(doc.reserve_id());
            doc.def_mut(def).unwrap().texts.insert(id, text);
            out.push(Element::Text(id));
        }
        for g in &guides {
            let Some(x) = doc.def(def).expect("checked").guides.get(g).copied() else { continue };
            let id = GuideId(doc.reserve_id());
            doc.def_mut(def).expect("checked").guides.insert(id, map_guide(&x, &mk));
            out.push(Element::Guide(id));
        }
    }
    Ok(out)
}

/// Moves elements into a new definition with one instance where they were.
fn make_definition(
    doc: &mut Document,
    def: Option<DefinitionId>,
    elements: &[Element],
    group: bool,
    name: Option<String>,
) -> Result<InstanceId, CommandError> {
    let (geo, inst, guides, dimensions) = split(elements);
    let sections: BTreeSet<_> =
        elements.iter().filter_map(|e| if let Element::Section(id) = e { Some(*id) } else { None }).collect();
    if sections.iter().any(|id| !doc.def(def).unwrap().sections.planes.contains_key(id)) {
        return invalid("section plane not found in this context");
    }
    let texts: BTreeSet<_> =
        elements.iter().filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None }).collect();
    if texts.iter().any(|id| !doc.def(def).unwrap().texts.contains_key(id)) {
        return invalid("text not found in this context");
    }
    if geo.is_empty()
        && inst.is_empty()
        && guides.is_empty()
        && dimensions.is_empty()
        && texts.is_empty()
        && sections.is_empty()
    {
        return invalid("select something first");
    }
    if dimensions.iter().any(|id| !doc.def(def).expect("checked").dimensions.contains_key(id)) {
        return invalid("dimension not found in this context");
    }
    let source_mesh = doc.def(def).expect("checked").mesh.clone();
    // Geometry out of the parent, with fresh ids.
    let mut mesh = doc.with_mesh(def, |m| m.extract(&geo)).expect("checked");
    doc.next_id = doc.next_id.max(mesh.next_id());
    let parent = doc.def_mut(def).expect("checked");
    let children: BTreeMap<InstanceId, Instance> =
        inst.iter().filter_map(|i| parent.instances.remove(i).map(|x| (*i, x))).collect();
    let moved_guides: BTreeMap<GuideId, Guide> =
        guides.iter().filter_map(|g| parent.guides.remove(g).map(|x| (*g, x))).collect();
    let moved_dimensions: BTreeMap<_, _> =
        dimensions.iter().map(|id| (*id, parent.dimensions.remove(id).expect("validated"))).collect();
    let moved_texts: BTreeMap<_, _> =
        texts.iter().map(|id| (*id, parent.texts.remove(id).expect("validated"))).collect();
    let moved_active = parent.sections.active.filter(|id| sections.contains(id));
    if moved_active.is_some() {
        parent.sections.active = None;
    }
    let moved_sections = sections
        .iter()
        .map(|id| (*id, parent.sections.planes.remove(id).expect("validated")))
        .collect::<BTreeMap<_, _>>();
    // Place the component axes at a corner of the new definition's bounding box.
    let mut tmp = Definition {
        mesh: mesh.clone(),
        instances: children.clone(),
        dimensions: moved_dimensions.clone(),
        texts: moved_texts.clone(),
        ..Default::default()
    };
    let id = DefinitionId(doc.reserve_id());
    doc.definitions.insert(id, std::mem::take(&mut tmp));
    let origin = doc
        .definition_bounds(Some(id))
        .map(|(lo, _)| lo)
        .or_else(|| moved_sections.values().next().map(|p| p.origin))
        .unwrap_or(DVec3::ZERO);
    mesh.map_all(|p| p - origin);
    let back = DMat4::from_translation(-origin);
    let children = children.into_iter().map(|(k, mut x)| {
        x.transform = back * x.transform;
        (k, x)
    });
    let guides = moved_guides.into_iter().map(|(k, g)| (k, map_guide(&g, &back)));
    let instance_map = inst.iter().map(|id| (*id, *id)).collect();
    let dimensions = moved_dimensions
        .into_iter()
        .map(|(id, dimension)| (id, dimension.remapped(&source_mesh, &mesh, back, &instance_map)))
        .collect();
    let texts = moved_texts
        .into_iter()
        .map(|(id, text)| (id, text.remapped(&source_mesh, &mesh, back, &instance_map)))
        .collect();
    let sections = crate::sections::Sections {
        planes: moved_sections.into_iter().map(|(id, plane)| (id, plane.transformed(back))).collect(),
        active: moved_active,
    };
    let count = doc.definitions.values().filter(|d| !d.group).count();
    let name = name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| if group { "Group".into() } else { format!("Component#{count}") });
    doc.definitions.insert(
        id,
        Definition {
            name,
            group,
            mesh,
            instances: children.collect(),
            guides: guides.collect(),
            sections,
            component: Default::default(),
            axes: Default::default(),
            dimensions,
            texts,
        },
    );
    let iid = InstanceId(doc.reserve_id());
    doc.def_mut(def).expect("checked").instances.insert(
        iid,
        Instance {
            glued_to: None,
            definition: id,
            transform: DMat4::from_translation(origin),
            name: String::new(),
            tag: None,
            material: None,
            hidden: false,
        },
    );
    let owners: Vec<_> = std::iter::once(None).chain(doc.definitions.keys().copied().map(Some)).collect();
    let document = &*doc;
    let target = &document.def(Some(id)).expect("created").mesh;
    let updates: Vec<_> = owners
        .into_iter()
        .flat_map(|owner| {
            document
                .def(owner)
                .unwrap()
                .dimensions
                .iter()
                .map(|(dimension_id, dimension)| {
                    (
                        owner,
                        *dimension_id,
                        dimension.reparented(document, owner, def, iid, &source_mesh, target, back, &inst),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    for (owner, id, dimension) in updates {
        doc.def_mut(owner).unwrap().dimensions.insert(id, dimension);
    }
    let owners: Vec<_> = std::iter::once(None).chain(doc.definitions.keys().copied().map(Some)).collect();
    let document = &*doc;
    let target = &document.def(Some(id)).unwrap().mesh;
    let updates: Vec<_> = owners
        .into_iter()
        .flat_map(|owner| {
            document
                .def(owner)
                .unwrap()
                .texts
                .iter()
                .map(|(text_id, text)| {
                    (owner, *text_id, text.reparented(document, owner, def, iid, &source_mesh, target, back, &inst))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    for (owner, id, text) in updates {
        doc.def_mut(owner).unwrap().texts.insert(id, text);
    }
    Ok(iid)
}

pub(crate) fn explode(
    doc: &mut Document,
    def: Option<DefinitionId>,
    instance: InstanceId,
) -> Result<Vec<Element>, CommandError> {
    crate::components::bake_opening(doc, def, instance)?;
    let inst = doc
        .def_mut(def)
        .expect("checked")
        .instances
        .remove(&instance)
        .ok_or_else(|| CommandError::Invalid(format!("no group or component {instance} here")))?;
    let mut source = doc
        .definitions
        .get(&inst.definition)
        .cloned()
        .ok_or_else(|| CommandError::Invalid("broken instance".into()))?;
    let t = inst.transform;
    let face_placements = source
        .mesh
        .faces()
        .filter_map(|(id, face)| {
            let size = [face.material.or(inst.material), face.back_material.or(inst.material)]
                .into_iter()
                .flatten()
                .find_map(|m| doc.materials.get(&m).and_then(|m| m.texture.map(|t| t.size)));
            let size = size.or(face.uv.map(|_| [1., 1.]))?;
            let plane = source.mesh.face_plane(id)?;
            Some((id, crate::texture::transformed(plane, size, face.uv.unwrap_or_default(), t)))
        })
        .collect::<Vec<_>>();
    for (id, placement) in face_placements {
        if let Some(placement) = placement {
            source.mesh.face_mut(id).unwrap().uv = Some(placement);
        }
    }
    let template = Template::whole(&source.mesh);
    let before: BTreeSet<FaceId> = doc.def(def).expect("checked").mesh.faces().map(|(f, _)| f).collect();
    let mut out = doc.with_mesh(def, |m| m.stamp(&template, |p| t.transform_point3(p))).expect("checked")?;
    // A mirrored instance turns the stamped faces inside out; stamp already fixes facing
    // from the mapped normals. The instance's paint goes to faces without their own.
    if let Some(mat) = inst.material {
        doc.with_mesh(def, |m| {
            let fresh: Vec<FaceId> = m.faces().map(|(f, _)| f).filter(|f| !before.contains(f)).collect();
            for f in fresh {
                if m.face(f).is_some_and(|x| x.material.is_none()) {
                    let _ = m.paint(f, Some(mat), false);
                }
            }
        });
    }
    let mut instance_map = BTreeMap::new();
    for (old, x) in &source.instances {
        let id = InstanceId(doc.reserve_id());
        instance_map.insert(*old, id);
        doc.def_mut(def).expect("checked").instances.insert(id, Instance { transform: t * x.transform, ..x.clone() });
        out.push(Element::Instance(id));
    }
    for g in source.guides.values() {
        let id = GuideId(doc.reserve_id());
        doc.def_mut(def).expect("checked").guides.insert(id, map_guide(g, &t));
        out.push(Element::Guide(id));
    }
    for (old, plane) in &source.sections.planes {
        let id = doc.reserve_id();
        let sections = &mut doc.def_mut(def).unwrap().sections;
        sections.planes.insert(id, plane.transformed(t));
        out.push(Element::Section(id));
        if sections.active.is_none() && source.sections.active == Some(*old) {
            sections.active = Some(id);
        }
    }
    let target_mesh = doc.def(def).expect("checked").mesh.clone();
    for definition in std::iter::once(&mut doc.root).chain(doc.definitions.values_mut()) {
        for dimension in definition.dimensions.values_mut() {
            dimension.remap_instance_reference(instance, &source.mesh, &target_mesh, &instance_map, t, true);
        }
        for text in definition.texts.values_mut() {
            text.remap_instance_reference(instance, &source.mesh, &target_mesh, &instance_map, t, true);
        }
    }
    for text in source.texts.values() {
        let text = text.remapped(&source.mesh, &doc.def(def).unwrap().mesh, t, &instance_map);
        let id = rysmo_mesh::TextId(doc.reserve_id());
        doc.def_mut(def).unwrap().texts.insert(id, text);
    }
    for dimension in source.dimensions.values() {
        let dimension = dimension.remapped(&source.mesh, &doc.def(def).expect("checked").mesh, t, &instance_map);
        if !dimension.validate() {
            return invalid("exploded dimension has degenerate placement");
        }
        let id = rysmo_mesh::DimensionId(doc.reserve_id());
        doc.def_mut(def).expect("checked").dimensions.insert(id, dimension);
    }
    Ok(out)
}

fn make_unique(doc: &mut Document, def: Option<DefinitionId>, instance: InstanceId) -> Result<(), CommandError> {
    let old = doc
        .def(def)
        .expect("checked")
        .instances
        .get(&instance)
        .map(|i| i.definition)
        .ok_or_else(|| CommandError::Invalid(format!("no group or component {instance} here")))?;
    let source = doc.definitions.get(&old).cloned().ok_or_else(|| CommandError::Invalid("broken instance".into()))?;
    // Fresh ids for everything inside, so ids stay unique in the document.
    let mut mesh = source.mesh.clone();
    mesh.ensure_next_id(doc.next_id);
    let all: Vec<Element> =
        mesh.edges().map(|(e, _)| Element::Edge(e)).chain(mesh.faces().map(|(f, _)| Element::Face(f))).collect();
    let fresh = mesh.extract(&all);
    doc.next_id = doc.next_id.max(fresh.next_id());
    let mut instance_map = BTreeMap::new();
    let instances: BTreeMap<InstanceId, Instance> = source
        .instances
        .iter()
        .map(|(old, x)| {
            let id = InstanceId(doc.reserve_id());
            instance_map.insert(*old, id);
            (id, x.clone())
        })
        .collect();
    let guides: BTreeMap<GuideId, Guide> = source.guides.values().map(|g| (GuideId(doc.reserve_id()), *g)).collect();
    let dimensions: BTreeMap<_, _> = source
        .dimensions
        .values()
        .map(|dimension| {
            let dimension = dimension.remapped(&source.mesh, &fresh, DMat4::IDENTITY, &instance_map);
            (rysmo_mesh::DimensionId(doc.reserve_id()), dimension)
        })
        .collect();
    let texts = source
        .texts
        .values()
        .map(|text| {
            (rysmo_mesh::TextId(doc.reserve_id()), text.remapped(&source.mesh, &fresh, DMat4::IDENTITY, &instance_map))
        })
        .collect();
    let mut sections = crate::sections::Sections::default();
    for (old, plane) in &source.sections.planes {
        let id = doc.reserve_id();
        sections.planes.insert(id, plane.clone());
        if source.sections.active == Some(*old) {
            sections.active = Some(id);
        }
    }
    let id = DefinitionId(doc.reserve_id());
    let name = if source.group { source.name.clone() } else { format!("{}#{}", source.name, doc.instance_count(old)) };
    for definition in std::iter::once(&mut doc.root).chain(doc.definitions.values_mut()) {
        for dimension in definition.dimensions.values_mut() {
            dimension.remap_instance_reference(instance, &source.mesh, &fresh, &instance_map, DMat4::IDENTITY, false);
        }
        for text in definition.texts.values_mut() {
            text.remap_instance_reference(instance, &source.mesh, &fresh, &instance_map, DMat4::IDENTITY, false);
        }
    }
    doc.definitions.insert(
        id,
        Definition {
            name,
            group: source.group,
            component: source.component.clone(),
            mesh: fresh,
            instances,
            guides,
            axes: source.axes,
            dimensions,
            texts,
            sections,
        },
    );
    doc.def_mut(def).expect("checked").instances.get_mut(&instance).expect("found above").definition = id;
    Ok(())
}

/// Removes definitions no instance uses any more (after erasing or exploding).
fn purge_unused(doc: &mut Document) {
    loop {
        let used: BTreeSet<DefinitionId> = std::iter::once(&doc.root)
            .chain(doc.definitions.values())
            .flat_map(|d| d.instances.values().map(|i| i.definition))
            .collect();
        let before = doc.definitions.len();
        doc.definitions.retain(|id, definition| !definition.group || used.contains(id));
        if doc.definitions.len() == before {
            break;
        }
    }
}

#[cfg(test)]
mod centre_arc_tests {
    use super::*;
    use crate::Editor;
    fn command(pie: bool, sweep: f64) -> Command {
        Command::DrawCentreArc {
            centre: DVec3::ZERO,
            normal: DVec3::Z,
            start: DVec3::X,
            radius: 2.,
            sweep,
            segments: 24,
            pie,
        }
    }
    #[test]
    fn major_arc_keeps_analytic_identity_and_supports_radius_and_segment_edits() {
        let mut e = Editor::new();
        e.apply(command(false, 270.)).unwrap();
        assert_eq!(e.mesh().face_count(), 0);
        let (id, curve) = e.mesh().curves().next().unwrap();
        assert_eq!(curve.sweep, 270.);
        assert_eq!(curve.radius, 2.);
        assert_eq!(curve.kind, rysmo_mesh::CurveKind::Arc);
        assert_eq!(e.mesh().curve_edges(id).len(), 24);
        assert!(e.mesh().find_vertex(DVec3::new(0., -2., 0.)).is_some());
        e.apply(Command::SetCurveSegments { curve: id, segments: 32 }).unwrap();
        e.apply(Command::SetCurveRadius { curve: id, radius: 3. }).unwrap();
        assert_eq!(e.mesh().curve(id).unwrap().radius, 3.);
        let doc = e.doc().clone();
        e.undo();
        e.redo();
        assert_eq!(e.doc(), &doc);
    }
    #[test]
    fn pie_radii_are_not_arc_members_and_face_remains_editable() {
        let mut e = Editor::new();
        e.apply(command(true, 90.)).unwrap();
        let (id, _) = e.mesh().curves().next().unwrap();
        assert_eq!(e.mesh().curve_edges(id).len(), 24);
        assert_eq!(e.mesh().edges().filter(|(_, e)| e.curve.is_none()).count(), 2);
        assert_eq!(e.mesh().face_count(), 1);
        let face = e.mesh().faces().next().unwrap().0;
        let expected = 24. * 2. * (90_f64.to_radians() / 24.).sin();
        assert!((e.mesh().face_area(face) - expected).abs() < 1e-8);
        e.apply(Command::SetCurveSegments { curve: id, segments: 12 }).unwrap();
        e.apply(Command::SetCurveRadius { curve: id, radius: 3. }).unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        let expected = 12. * 4.5 * (90_f64.to_radians() / 12.).sin();
        assert!((e.mesh().face_area(face) - expected).abs() < 1e-8);
        e.apply(Command::PushPull { face, distance: 1., keep_base: false }).unwrap();
        assert!(e.mesh().edges().all(|(id, _)| e.mesh().faces_of_edge(id).len() == 2));
    }
    #[test]
    fn clockwise_major_pie_is_one_face_and_keeps_signed_sweep() {
        let mut e = Editor::new();
        e.apply(command(true, -270.)).unwrap();
        assert_eq!(e.mesh().face_count(), 1);
        assert_eq!(e.mesh().curves().next().unwrap().1.sweep, -270.);
        let face = e.mesh().faces().next().unwrap().0;
        assert!((e.mesh().face_area(face) - 48. * (270_f64.to_radians() / 24.).sin()).abs() < 1e-8);
        let json = serde_json::to_string(&command(true, -270.)).unwrap();
        assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), command(true, -270.));
    }
    #[test]
    fn invalid_centre_arcs_are_transactional() {
        let mut e = Editor::new();
        let doc = e.doc().clone();
        for cmd in [
            command(false, 0.),
            command(true, 360.),
            command(false, f64::NAN),
            Command::DrawCentreArc {
                centre: DVec3::ZERO,
                normal: DVec3::Z,
                start: DVec3::Z,
                radius: 2.,
                sweep: 90.,
                segments: 24,
                pie: false,
            },
            Command::DrawCentreArc {
                centre: DVec3::ZERO,
                normal: DVec3::Z,
                start: DVec3::X,
                radius: 0.,
                sweep: 90.,
                segments: 24,
                pie: true,
            },
        ] {
            assert!(e.apply(cmd).is_err());
            assert_eq!(e.doc(), &doc);
            assert_eq!(e.undo_depth(), 0);
        }
    }
}
