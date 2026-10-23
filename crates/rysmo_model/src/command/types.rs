//! Command data, outcomes and errors; serialized layout is independent of execution.

use crate::{Style, Texture, Units};
use rysmo_math::{DMat4, DVec3};
use rysmo_mesh::{
    DefinitionId, EdgeId, Element, FaceId, ImageId, InstanceId, MaterialId, MeshError, TagId, TexturePlacement,
};
use serde::{Deserialize, Serialize};

/// A change to the document. Points and distances are in metres, in the coordinates of
/// the context being edited (Z up). The JSON form is `{"op": "draw_lines", ...}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    ImportPhoto {
        name: String,
        mime: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
        size: [u32; 2],
    },
    SetPhotoMatch {
        setup: Option<crate::photo::Setup>,
    },
    AddNormalMap {
        material: MaterialId,
        name: String,
        mime: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
    /// Lights the studio with an equirectangular panorama (Radiance HDR, PNG or JPEG).
    SetEnvironment {
        name: String,
        mime: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
    SetMaterialPbr {
        material: MaterialId,
        pbr: crate::Pbr,
    },
    SetParametric {
        definition: DefinitionId,
        spec: crate::parametric::Spec,
    },
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
pub(super) fn invalid<T>(msg: impl Into<String>) -> Result<T, CommandError> {
    Err(CommandError::Invalid(msg.into()))
}
