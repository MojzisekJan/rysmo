//! The native `.rysmo` format: JSON, one element per line.
//!
//! The layout is meant to be read and edited by people and by programs (an AI agent
//! included) with plain text tools: every vertex, edge, face, material, tag,
//! definition, instance and guide is one line with a stable id, so a diff shows exactly
//! which elements changed. See `docs/file-format.md`.

use std::collections::{BTreeMap, BTreeSet};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use rysmo_math::{DMat4, DVec3, DVec4, OrbitCamera};
use rysmo_mesh::ImageId;
use rysmo_mesh::{
    DefinitionId, DimensionId, Edge, EdgeId, Face, FaceId, GuideId, InstanceId, MaterialId, Mesh, TagId, Vertex,
    VertexId,
};
use rysmo_model::{Definition, Document, Guide, Image, Instance, Material, Style, Tag, Texture, Units};
use serde::{Deserialize, Serialize};

use crate::IoError;

pub const FORMAT: &str = "rysmo";
/// Version 2 added definitions, instances, tags, guides and edge flags; version 3 added
/// images, textured materials, texture placement on faces and the style.
/// Version 4 added curves; version 5 added dimension entities; version 6 adds text annotations.
/// Version 7 supports displaced attached text without leaders.
/// Version 8 adds context-specific section planes and section display settings.
/// Version 9 adds saved scenes.
/// Version 10 adds component insertion properties and instance visibility.
/// Version 11 adds independently painted edge materials.
/// Version 12 adds projective four-pin texture positioning.
/// Version 13 adds decimal display units and configurable length precision.
/// Version 14 adds measurement settings, hidden faces and glued component openings.
/// Version 15 adds measured guide origins and fractional/architectural units.
/// Version 16 adds independent volume units and precision.
/// Version 17 adds studio lighting, exposure, texture quality and render scaling.
pub const VERSION: u32 = 17;

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Serialize, Deserialize)]
struct MaterialLine {
    id: MaterialId,
    name: String,
    color: [f32; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    texture: Option<Texture>,
    #[serde(default, skip_serializing_if = "is_zero")]
    glow: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

#[derive(Serialize, Deserialize)]
struct ImageLine {
    id: ImageId,
    name: String,
    mime: String,
    /// The encoded file, base64.
    data: String,
}

#[derive(Serialize, Deserialize)]
struct TagLine {
    id: TagId,
    name: String,
    #[serde(default = "yes")]
    visible: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
struct DefinitionLine {
    id: DefinitionId,
    name: String,
    #[serde(default, skip_serializing_if = "is_false")]
    group: bool,
    #[serde(default)]
    axes: rysmo_math::DrawingAxes,
    #[serde(default)]
    component: rysmo_model::components::ComponentSettings,
}

#[derive(Serialize, Deserialize)]
struct SectionLine {
    id: u64,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    within: Option<DefinitionId>,
    #[serde(flatten)]
    plane: rysmo_model::sections::SectionPlane,
    #[serde(default, skip_serializing_if = "is_false")]
    active: bool,
}

/// Which definition an element belongs to; absent means the model itself.
type In = Option<DefinitionId>;

#[derive(Serialize, Deserialize)]
struct VertexLine {
    id: VertexId,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    within: In,
    p: [f64; 3],
}

#[derive(Serialize, Deserialize)]
struct EdgeLine {
    id: EdgeId,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    within: In,
    #[serde(flatten)]
    edge: Edge,
}

#[derive(Serialize, Deserialize)]
struct CurveLine {
    id: rysmo_mesh::CurveId,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    within: In,
    #[serde(flatten)]
    curve: rysmo_mesh::Curve,
}

#[derive(Serialize, Deserialize)]
struct FaceLine {
    id: FaceId,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    within: In,
    #[serde(flatten)]
    face: Face,
}

#[derive(Serialize, Deserialize)]
struct InstanceLine {
    id: InstanceId,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    within: In,
    def: DefinitionId,
    /// Where the definition's origin and axes go in the parent's coordinates.
    o: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    z: [f64; 3],
    #[serde(default, skip_serializing_if = "String::is_empty")]
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tag: Option<TagId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material: Option<MaterialId>,
    #[serde(default, skip_serializing_if = "is_false")]
    hidden: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    glued_to: Option<FaceId>,
}

#[derive(Serialize, Deserialize)]
struct GuideLine {
    id: GuideId,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    within: In,
    #[serde(flatten)]
    guide: Guide,
}

#[derive(Serialize, Deserialize)]
struct TextLine {
    id: rysmo_mesh::TextId,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    within: In,
    #[serde(flatten)]
    text: rysmo_model::text::Text,
}

#[derive(Serialize, Deserialize)]
struct DimensionLine {
    id: DimensionId,
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    within: In,
    #[serde(flatten)]
    dimension: rysmo_model::dimensions::Dimension,
}

#[derive(Deserialize)]
struct FileIn {
    format: String,
    version: u32,
    #[serde(default = "metres")]
    length_unit: String,
    #[serde(default)]
    display_units: Units,
    #[serde(default)]
    length_precision: Option<u8>,
    #[serde(default)]
    measurement: rysmo_model::MeasurementSettings,
    #[serde(default)]
    camera: Option<OrbitCamera>,
    #[serde(default)]
    axes: Option<rysmo_math::DrawingAxes>,
    #[serde(default)]
    section_planes: Vec<SectionLine>,
    #[serde(default)]
    scenes: Vec<rysmo_model::scenes::Scene>,
    #[serde(default)]
    style: Option<Style>,
    #[serde(default)]
    images: Vec<ImageLine>,
    #[serde(default)]
    materials: Vec<MaterialLine>,
    #[serde(default)]
    tags: Vec<TagLine>,
    #[serde(default)]
    definitions: Vec<DefinitionLine>,
    #[serde(default)]
    vertices: Vec<VertexLine>,
    #[serde(default)]
    edges: Vec<EdgeLine>,
    #[serde(default)]
    faces: Vec<FaceLine>,
    #[serde(default)]
    curves: Vec<CurveLine>,
    #[serde(default)]
    instances: Vec<InstanceLine>,
    #[serde(default)]
    guides: Vec<GuideLine>,
    #[serde(default)]
    dimensions: Vec<DimensionLine>,
    #[serde(default)]
    texts: Vec<TextLine>,
}

fn metres() -> String {
    "m".into()
}

/// Rounds to a nanometre so that arithmetic noise (0.30000000000000004) does not reach
/// the file. This is far below the modelling tolerance of a micrometre.
fn tidy(x: f64) -> f64 {
    let r = (x * 1e9).round() / 1e9;
    if r == 0.0 { 0.0 } else { r }
}

fn tidy3(v: DVec3) -> [f64; 3] {
    [tidy(v.x), tidy(v.y), tidy(v.z)]
}

fn section<T: Serialize>(out: &mut String, name: &str, items: impl Iterator<Item = T>, last: bool) {
    out.push_str(&format!("\"{name}\": ["));
    let mut first = true;
    for item in items {
        out.push_str(if first { "\n  " } else { ",\n  " });
        first = false;
        out.push_str(&serde_json::to_string(&item).expect("plain data serialises"));
    }
    out.push_str(if first { "]" } else { "\n]" });
    out.push_str(if last { "\n" } else { ",\n" });
}

/// Every definition with its id: the model first (`None`), then the others.
fn all_defs(doc: &Document) -> Vec<(In, &Definition)> {
    std::iter::once((None, &doc.root)).chain(doc.definitions.iter().map(|(id, d)| (Some(*id), d))).collect()
}

fn tidy_guide(g: &Guide) -> Guide {
    match g {
        Guide::Line { origin, dir, hidden } => Guide::Line {
            origin: DVec3::from_array(tidy3(*origin)),
            dir: DVec3::from_array(tidy3(*dir)),
            hidden: *hidden,
        },
        Guide::Point { p, origin, hidden } => Guide::Point {
            p: DVec3::from_array(tidy3(*p)),
            origin: origin.map(|p| DVec3::from_array(tidy3(p))),
            hidden: *hidden,
        },
    }
}

/// Writes a document as `.rysmo` text.
pub fn save(doc: &Document) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!("\"format\": \"{FORMAT}\",\n\"version\": {VERSION},\n"));
    out.push_str("\"length_unit\": \"m\",\n");
    out.push_str(&format!("\"display_units\": {},\n", serde_json::to_string(&doc.units).expect("display units")));
    out.push_str(&format!(
        "\"measurement\": {},\n",
        serde_json::to_string(&doc.measurement).expect("measurement settings")
    ));
    out.push_str(&format!(
        "\"length_precision\": {},\n",
        serde_json::to_string(&doc.length_precision).expect("precision")
    ));
    if let Some(c) = &doc.camera {
        let c = OrbitCamera {
            target: DVec3::from_array(tidy3(c.target)),
            distance: tidy(c.distance),
            yaw: tidy(c.yaw),
            pitch: tidy(c.pitch),
            fov_y: tidy(c.fov_y),
            parallel: c.parallel,
            eye_base_z: tidy(c.eye_base_z),
        };
        out.push_str(&format!("\"camera\": {},\n", serde_json::to_string(&c).expect("camera serialises")));
    }
    if doc.root.axes != Default::default() {
        out.push_str(&format!("\"axes\": {},\n", serde_json::to_string(&doc.root.axes).expect("axes serialises")));
    }
    if doc.style != Style::default() {
        out.push_str(&format!("\"style\": {},\n", serde_json::to_string(&doc.style).expect("style serialises")));
    }
    section(&mut out, "scenes", doc.scenes.iter(), false);
    section(
        &mut out,
        "images",
        doc.images.iter().map(|(id, i)| ImageLine {
            id: *id,
            name: i.name.clone(),
            mime: i.mime.clone(),
            data: STANDARD.encode(i.bytes.as_slice()),
        }),
        false,
    );
    section(
        &mut out,
        "materials",
        doc.materials.iter().map(|(id, m)| MaterialLine {
            id: *id,
            name: m.name.clone(),
            color: m.color,
            texture: m.texture,
            glow: m.glow,
        }),
        false,
    );
    section(
        &mut out,
        "tags",
        doc.tags.iter().map(|(id, t)| TagLine { id: *id, name: t.name.clone(), visible: t.visible }),
        false,
    );
    section(
        &mut out,
        "definitions",
        doc.definitions.iter().map(|(id, d)| DefinitionLine {
            id: *id,
            name: d.name.clone(),
            group: d.group,
            axes: d.axes,
            component: d.component.clone(),
        }),
        false,
    );
    section(
        &mut out,
        "section_planes",
        all_defs(doc).iter().flat_map(|(owner, definition)| {
            definition.sections.planes.iter().map(move |(id, plane)| SectionLine {
                id: *id,
                within: *owner,
                plane: plane.clone(),
                active: definition.sections.active == Some(*id),
            })
        }),
        false,
    );
    let defs = all_defs(doc);
    section(
        &mut out,
        "vertices",
        defs.iter()
            .flat_map(|(w, d)| d.mesh.vertices().map(move |(id, v)| VertexLine { id, within: *w, p: tidy3(v.pos) })),
        false,
    );
    section(
        &mut out,
        "edges",
        defs.iter().flat_map(|(w, d)| d.mesh.edges().map(move |(id, e)| EdgeLine { id, within: *w, edge: e.clone() })),
        false,
    );
    section(
        &mut out,
        "curves",
        defs.iter().flat_map(|(w, d)| d.mesh.curves().map(move |(id, c)| CurveLine { id, within: *w, curve: *c })),
        false,
    );
    section(
        &mut out,
        "faces",
        defs.iter().flat_map(|(w, d)| d.mesh.faces().map(move |(id, f)| FaceLine { id, within: *w, face: f.clone() })),
        false,
    );
    section(
        &mut out,
        "instances",
        defs.iter().flat_map(|(w, d)| {
            d.instances.iter().map(move |(id, i)| {
                let t = i.transform;
                InstanceLine {
                    id: *id,
                    within: *w,
                    def: i.definition,
                    o: tidy3(t.w_axis.truncate()),
                    x: tidy3(t.x_axis.truncate()),
                    y: tidy3(t.y_axis.truncate()),
                    z: tidy3(t.z_axis.truncate()),
                    name: i.name.clone(),
                    tag: i.tag,
                    material: i.material,
                    hidden: i.hidden,
                    glued_to: i.glued_to,
                }
            })
        }),
        false,
    );
    section(
        &mut out,
        "guides",
        defs.iter().flat_map(|(w, d)| {
            d.guides.iter().map(move |(id, g)| GuideLine { id: *id, within: *w, guide: tidy_guide(g) })
        }),
        false,
    );
    section(
        &mut out,
        "dimensions",
        defs.iter().flat_map(|(within, d)| {
            d.dimensions.iter().map(move |(id, dimension)| DimensionLine {
                id: *id,
                within: *within,
                dimension: dimension.clone(),
            })
        }),
        false,
    );
    section(
        &mut out,
        "texts",
        defs.iter().flat_map(|(within, d)| {
            d.texts.iter().map(move |(id, text)| TextLine { id: *id, within: *within, text: text.clone() })
        }),
        true,
    );
    out.push_str("}\n");
    out
}

/// Resolve an input record's owner without relying on an earlier validation pass.
fn definition_mut(doc: &mut Document, owner: Option<DefinitionId>) -> Result<&mut Definition, IoError> {
    doc.def_mut(owner).ok_or_else(|| {
        IoError::Format(match owner {
            Some(id) => format!("definition {id} is not defined"),
            None => "the model definition is missing".into(),
        })
    })
}

/// Reads `.rysmo` text up to [`VERSION`]. Everything is checked; a file with broken
/// loops, missing references or a component that contains itself is refused with a
/// message saying what is wrong.
pub fn load(text: &str) -> Result<Document, IoError> {
    let f: FileIn = serde_json::from_str(text)?;
    if f.format != FORMAT {
        return Err(IoError::Format(format!("\"format\" is \"{}\"", f.format)));
    }
    if f.version > VERSION {
        return Err(IoError::Version(f.version));
    }
    let scale = match f.length_unit.as_str() {
        "m" => 1.0,
        "mm" => 0.001,
        other => return Err(IoError::Format(format!("unknown length_unit \"{other}\""))),
    };
    let mut ids = BTreeSet::new();
    let mut unique = |id: u64| {
        if ids.insert(id) { Ok(()) } else { Err(IoError::Format(format!("id {id} is used twice"))) }
    };
    let mut images = BTreeMap::new();
    for i in f.images {
        unique(i.id.0)?;
        let bytes = STANDARD.decode(i.data.as_bytes()).map_err(|e| IoError::Format(format!("image {}: {e}", i.id)))?;
        images.insert(i.id, Image { name: i.name, mime: i.mime, bytes: std::sync::Arc::new(bytes) });
    }
    let mut materials = BTreeMap::new();
    for m in f.materials {
        unique(m.id.0)?;
        if let Some(t) = &m.texture
            && !images.contains_key(&t.image)
        {
            return Err(IoError::Format(format!("material {} uses image {}, which is not defined", m.id, t.image)));
        }
        if !m.glow.is_finite() || m.glow < 0.0 {
            return Err(IoError::Format(format!("material {} has an invalid glow", m.id)));
        }
        materials.insert(m.id, Material { name: m.name, color: m.color, texture: m.texture, glow: m.glow });
    }
    let mut tags = BTreeMap::new();
    for t in f.tags {
        unique(t.id.0)?;
        tags.insert(t.id, Tag { name: t.name, visible: t.visible });
    }
    let mut def_lines: BTreeMap<DefinitionId, DefinitionLine> = BTreeMap::new();
    for d in f.definitions {
        unique(d.id.0)?;
        def_lines.insert(d.id, d);
    }
    let known = |w: In| -> Result<(), IoError> {
        match w {
            Some(d) if !def_lines.contains_key(&d) => Err(IoError::Format(format!("definition {d} is not defined"))),
            _ => Ok(()),
        }
    };
    type Parts = (
        BTreeMap<VertexId, Vertex>,
        BTreeMap<EdgeId, Edge>,
        BTreeMap<FaceId, Face>,
        BTreeMap<rysmo_mesh::CurveId, rysmo_mesh::Curve>,
    );
    let mut parts: BTreeMap<In, Parts> = BTreeMap::new();
    for v in f.vertices {
        unique(v.id.0)?;
        known(v.within)?;
        parts.entry(v.within).or_default().0.insert(v.id, Vertex { pos: DVec3::from_array(v.p) * scale });
    }
    for e in f.edges {
        if let Some(material) = e.edge.material {
            if !materials.contains_key(&material) {
                return Err(IoError::Format(format!("edge {} uses undefined material {material}", e.id)));
            }
        }
        unique(e.id.0)?;
        known(e.within)?;
        parts.entry(e.within).or_default().1.insert(e.id, e.edge);
    }
    for fl in f.faces {
        if fl.face.uv.is_some_and(|p| !rysmo_model::valid_placement(&p)) {
            return Err(IoError::Format(format!("face {} has invalid texture placement", fl.id)));
        }
        unique(fl.id.0)?;
        known(fl.within)?;
        for m in [fl.face.material, fl.face.back_material].into_iter().flatten() {
            if !materials.contains_key(&m) {
                return Err(IoError::Format(format!("face {} uses material {m}, which is not defined", fl.id)));
            }
        }
        parts.entry(fl.within).or_default().2.insert(fl.id, fl.face);
    }
    for mut curve in f.curves {
        unique(curve.id.0)?;
        known(curve.within)?;
        curve.curve.centre *= scale;
        curve.curve.radius *= scale;
        parts.entry(curve.within).or_default().3.insert(curve.id, curve.curve);
    }
    let mut meshes: BTreeMap<In, Mesh> = BTreeMap::new();
    for (w, (vs, es, fs, curves)) in parts {
        meshes.insert(w, Mesh::from_parts_with_curves(vs, es, fs, curves)?);
    }
    let mut doc = Document::new();
    doc.materials = materials;
    doc.images = images;
    doc.style = f.style.unwrap_or_default();
    if !doc.style.valid_sections()
        || !doc.style.valid_render()
        || !doc.style.dimensions.validate()
        || !doc.style.dimension_defaults.validate()
        || !doc.style.text_defaults.validate()
    {
        return Err(IoError::Format("invalid display settings".into()));
    }
    let mut axes = f.axes.unwrap_or_default();
    axes.origin *= scale;
    if !axes.valid() {
        return Err(IoError::Format("invalid drawing axes".into()));
    }
    doc.root.axes = axes;
    doc.tags = tags;
    doc.units = f.display_units;
    if f.length_precision.is_some_and(|p| p > 6) {
        return Err(IoError::Format("invalid length precision".into()));
    }
    doc.length_precision = f.length_precision;
    if !f.measurement.valid() {
        return Err(IoError::Format("invalid measurement settings".into()));
    }
    doc.measurement = f.measurement;
    doc.camera = f.camera.map(|mut camera| {
        camera.target *= scale;
        camera.distance *= scale;
        camera.eye_base_z *= scale;
        camera
    });
    doc.root.mesh = meshes.remove(&None).unwrap_or_default();
    for (id, d) in def_lines.iter() {
        let mesh = meshes.remove(&Some(*id)).unwrap_or_default();
        let mut axes = d.axes;
        axes.origin *= scale;
        if !axes.valid() {
            return Err(IoError::Format("invalid definition axes".into()));
        }
        doc.definitions.insert(
            *id,
            Definition {
                name: d.name.clone(),
                group: d.group,
                mesh,
                axes,
                component: {
                    let mut component = d.component.clone();
                    component.insertion_axes.origin *= scale;
                    if !component.insertion_axes.valid() {
                        return Err(IoError::Format("invalid component insertion axes".into()));
                    }
                    component
                },
                ..Default::default()
            },
        );
    }
    for mut section in f.section_planes {
        unique(section.id)?;
        known(section.within)?;
        if section.id == 0 || !section.plane.valid() {
            return Err(IoError::Format("invalid section plane".into()));
        }
        section.plane.origin *= scale;
        if !section.plane.valid() {
            return Err(IoError::Format("invalid scaled section plane".into()));
        }
        let sections = &mut definition_mut(&mut doc, section.within)?.sections;
        if section.active && sections.active.is_some() {
            return Err(IoError::Format("multiple active section planes in one context".into()));
        }
        if section.active {
            sections.active = Some(section.id);
        }
        sections.planes.insert(section.id, section.plane);
    }
    for i in f.instances {
        unique(i.id.0)?;
        known(i.within)?;
        if !doc.definitions.contains_key(&i.def) {
            return Err(IoError::Format(format!("instance {} uses definition {}, which is not defined", i.id, i.def)));
        }
        if let Some(t) = i.tag
            && !doc.tags.contains_key(&t)
        {
            return Err(IoError::Format(format!("instance {} uses tag {t}, which is not defined", i.id)));
        }
        if let Some(m) = i.material
            && !doc.materials.contains_key(&m)
        {
            return Err(IoError::Format(format!("instance {} uses material {m}, which is not defined", i.id)));
        }
        let axis = |a: [f64; 3]| DVec4::new(a[0], a[1], a[2], 0.0);
        let o = DVec3::from_array(i.o) * scale;
        let transform = DMat4::from_cols(axis(i.x), axis(i.y), axis(i.z), DVec4::new(o.x, o.y, o.z, 1.0));
        let inst = Instance {
            glued_to: i.glued_to,
            definition: i.def,
            transform,
            name: i.name,
            tag: i.tag,
            material: i.material,
            hidden: i.hidden,
        };
        definition_mut(&mut doc, i.within)?.instances.insert(i.id, inst);
    }
    for g in f.guides {
        unique(g.id.0)?;
        known(g.within)?;
        let guide = match g.guide {
            Guide::Line { origin, dir, hidden } => {
                if dir.length() < 1e-12 {
                    return Err(IoError::Format(format!("guide {} has no direction", g.id)));
                }
                Guide::Line { origin: origin * scale, dir: dir.normalize(), hidden }
            }
            Guide::Point { p, origin, hidden } => {
                Guide::Point { p: p * scale, origin: origin.map(|p| p * scale), hidden }
            }
        };
        definition_mut(&mut doc, g.within)?.guides.insert(g.id, guide);
    }
    for entry in f.texts {
        unique(entry.id.0)?;
        known(entry.within)?;
        let text = entry.text.scaled(scale);
        if !text.validate() {
            return Err(IoError::Format(format!("text {} has invalid placement or font", entry.id)));
        }
        definition_mut(&mut doc, entry.within)?.texts.insert(entry.id, text);
    }
    for entry in f.dimensions {
        unique(entry.id.0)?;
        known(entry.within)?;
        let dimension = entry.dimension.scaled(scale);
        if !dimension.validate() {
            return Err(IoError::Format(format!("dimension {} has invalid geometry", entry.id)));
        }
        // Missing geometry is a supported non-associated dimension, with retained values.
        definition_mut(&mut doc, entry.within)?.dimensions.insert(entry.id, dimension);
    }
    for id in doc.definitions.keys() {
        let d = &doc.definitions[id];
        if d.instances.values().any(|i| doc.contains_definition(i.definition, *id)) {
            return Err(IoError::Format(format!("definition {id} contains itself")));
        }
    }
    for scene in f.scenes {
        unique(scene.id)?;
        let scene = scene.scaled(scale);
        if !scene.valid() {
            return Err(IoError::Format("invalid scene".into()));
        }
        doc.scenes.push(scene);
    }
    for definition in std::iter::once(&doc.root).chain(doc.definitions.values()) {
        if definition.component.cut_opening {
            if definition.component.glue == rysmo_model::components::GlueMode::None {
                return Err(IoError::Format("Cut Opening requires glue behaviour".into()));
            }
            rysmo_model::components::opening_outline(definition).map_err(|e| IoError::Format(e.to_string()))?;
        }
        for instance in definition.instances.values() {
            if instance.glued_to.is_some_and(|face| definition.mesh.face(face).is_none()) {
                return Err(IoError::Format("a glued component references a missing host face".into()));
            }
        }
    }
    rysmo_model::components::validate_openings(&doc).map_err(|e| IoError::Format(e.to_string()))?;
    doc.fix_next_id();
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use rysmo_model::{Command, Editor};

    use super::*;

    #[test]
    fn component_properties_and_instance_visibility_roundtrip() {
        use rysmo_model::components::ComponentSettings;
        let mut e = Editor::new();
        let elements = e
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        let out = e.apply(Command::MakeComponent { elements, name: Some("Window".into()) }).unwrap();
        let rysmo_mesh::Element::Instance(instance) = out.created[0] else { panic!() };
        let definition = e.doc().root.instances[&instance].definition;
        e.apply(Command::SetComponentProperties {
            definition,
            name: "Window".into(),
            settings: ComponentSettings {
                glue: Default::default(),
                cut_opening: false,
                description: "Insert at centre".into(),
                insertion_axes: rysmo_math::DrawingAxes::new(DVec3::new(0.5, 0.5, 0.), DVec3::Y, -DVec3::X).unwrap(),
            },
        })
        .unwrap();
        e.apply(Command::SetInstancesHidden { instances: vec![instance], hidden: true }).unwrap();
        let text = save(e.doc());
        let loaded = load(&text).unwrap();
        assert_eq!(loaded.root.instances, e.doc().root.instances);
        assert_eq!(loaded.definitions[&definition].component, e.doc().definitions[&definition].component);
        assert_eq!(loaded.occurrences(&[]).len(), 1);
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["length_unit"] = serde_json::json!("mm");
        let converted = load(&value.to_string()).unwrap();
        assert_eq!(converted.definitions[&definition].component.insertion_axes.origin, DVec3::new(0.0005, 0.0005, 0.));
        value["definitions"][0]["component"]["insertion_axes"]["x"] = serde_json::json!([0, 0, 0]);
        assert!(load(&value.to_string()).is_err());
    }

    #[test]
    fn removed_scene_references_are_not_reused_after_loading() {
        use rysmo_model::scenes::Scene;
        let mut e = Editor::new();
        let elements = e
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        e.apply(Command::MakeComponent { elements, name: Some("Part".into()) }).unwrap();
        let mut scene = Scene::capture(e.doc(), OrbitCamera::default(), "Old view".into());
        scene.contexts[0].hidden_instances.insert(InstanceId(900));
        scene.contexts[0].hidden_edges.insert(EdgeId(901));
        scene.contexts[0].section = Some(902);
        e.apply(Command::AddScene { scene }).unwrap();
        let loaded = load(&save(e.doc())).unwrap();
        assert!(loaded.next_id() > 902);
    }

    #[test]
    fn scenes_roundtrip_units_and_invalid_camera() {
        use rysmo_model::scenes::Scene;
        let mut editor = Editor::new();
        let camera = OrbitCamera { target: DVec3::new(3., 2., 1.), distance: 8., parallel: true, ..Default::default() };
        let scene = Scene::capture(editor.doc(), camera, "Elevation".into());
        editor.apply(Command::AddScene { scene }).unwrap();
        let saved = save(editor.doc());
        let loaded = load(&saved).unwrap();
        assert_eq!(loaded.scenes, editor.doc().scenes);
        assert!(loaded.next_id() > loaded.scenes[0].id);
        let mut value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        value["length_unit"] = serde_json::json!("mm");
        let scaled = load(&value.to_string()).unwrap();
        assert_eq!(scaled.scenes[0].camera.target, camera.target * 0.001);
        assert_eq!(scaled.scenes[0].camera.distance, 0.008);
        value["scenes"][0]["camera"]["distance"] = serde_json::json!(-1);
        assert!(load(&value.to_string()).is_err());
    }

    fn sample() -> Document {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle {
            corner: DVec3::ZERO,
            u: DVec3::new(0.3, 0.0, 0.0),
            v: DVec3::new(0.0, 0.1, 0.0),
            facing: None,
        })
        .unwrap();
        e.apply(Command::DrawRectangle {
            corner: DVec3::new(0.1, 0.02, 0.0),
            u: DVec3::new(0.1, 0.0, 0.0),
            v: DVec3::new(0.0, 0.05, 0.0),
            facing: None,
        })
        .unwrap();
        let m = e
            .apply(Command::AddMaterial { name: "Oak \"light\"".into(), color: [0.8, 0.6, 0.4, 1.0], texture: None })
            .unwrap()
            .material;
        let f = e.mesh().faces().next().unwrap().0;
        e.apply(Command::Paint { faces: vec![f], material: m, back: false }).unwrap();
        e.apply(Command::SetUnits { units: Units::Metres }).unwrap();
        let mut doc = e.doc().clone();
        doc.camera = Some(OrbitCamera::default());
        doc
    }

    #[test]
    fn all_input_record_owners_are_checked_before_insertion() {
        use rysmo_model::sections::SectionPlane;
        use rysmo_model::text::{Text, TextPlacement};
        let mut doc = load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        let definition = DefinitionId(doc.reserve_id());
        doc.definitions.insert(definition, Definition::default());
        let instance = InstanceId(doc.reserve_id());
        doc.root.instances.insert(
            instance,
            Instance {
                definition,
                transform: DMat4::IDENTITY,
                name: "Test".into(),
                tag: None,
                material: None,
                hidden: false,
                glued_to: None,
            },
        );
        let guide = GuideId(doc.reserve_id());
        doc.root.guides.insert(guide, Guide::Point { p: DVec3::ZERO, origin: None, hidden: false });
        let text = rysmo_mesh::TextId(doc.reserve_id());
        doc.root.texts.insert(
            text,
            Text {
                content: "Note".into(),
                placement: TextPlacement::Screen { position: rysmo_math::DVec2::splat(0.5) },
                font: Default::default(),
                endpoint: Default::default(),
            },
        );
        let section = doc.reserve_id();
        doc.root
            .sections
            .planes
            .insert(section, SectionPlane { name: "Section".into(), origin: DVec3::ZERO, normal: DVec3::Z });
        let saved = save(&doc);
        assert!(load(&saved).is_ok(), "The starting fixture must be a valid native document");
        let original: serde_json::Value = serde_json::from_str(&saved).unwrap();
        for collection in ["instances", "guides", "texts", "dimensions", "section_planes"] {
            let mut invalid = original.clone();
            assert!(!invalid[collection].as_array().unwrap().is_empty(), "Missing {collection} fixture");
            invalid[collection][0]["in"] = serde_json::json!(u64::MAX);
            match load(&invalid.to_string()) {
                Err(IoError::Format(message)) => assert!(message.contains("is not defined"), "{collection}: {message}"),
                other => panic!("{collection} must report the invalid owner, got {other:?}"),
            }
        }
    }

    #[test]
    fn section_planes_roundtrip_and_reject_invalid_references() {
        use rysmo_model::sections::SectionPlane;
        let mut editor = Editor::new();
        editor
            .apply(Command::AddSection {
                plane: SectionPlane { name: "Root".into(), origin: DVec3::X, normal: DVec3::X },
            })
            .unwrap();
        let output = editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(2., 3., 4.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                spacing: 1.,
            })
            .unwrap();
        let rysmo_model::Element::Instance(instance) = output.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        editor
            .apply(Command::AddSection {
                plane: SectionPlane { name: "Child".into(), origin: DVec3::Y, normal: DVec3::Y },
            })
            .unwrap();
        let restored = load(&save(editor.doc())).unwrap();
        assert_eq!(restored.root.sections, editor.doc().root.sections);
        assert_eq!(restored.definitions, editor.doc().definitions);
        assert_eq!(restored.section_planes(&[instance]).len(), 2);
        let mut value: serde_json::Value = serde_json::from_str(&save(editor.doc())).unwrap();
        value["section_planes"][1].as_object_mut().unwrap().remove("in");
        assert!(load(&value.to_string()).is_err());
        value["section_planes"][1]["active"] = serde_json::json!(false);
        value["section_planes"][1]["normal"] = serde_json::json!([0, 0, 0]);
        assert!(load(&value.to_string()).is_err());
    }
    #[test]
    fn round_trip_keeps_everything() {
        let doc = sample();
        let text = save(&doc);
        let back = load(&text).unwrap();
        assert_eq!(back.units, doc.units);
        assert_eq!(back.materials, doc.materials);
        assert_eq!(back.root.mesh.face_count(), 2);
        let ids = |d: &Document| d.root.mesh.faces().map(|(id, f)| (id, f.clone())).collect::<Vec<_>>();
        assert_eq!(ids(&back), ids(&doc));
        for (id, v) in doc.root.mesh.vertices() {
            assert!(back.root.mesh.vertex(id).unwrap().pos.distance(v.pos) < 1e-9);
        }
        // Saving again gives the same text.
        assert_eq!(save(&back), text);
        // New ids do not collide with the material's.
        let mut d = back.clone();
        let fresh = d.reserve_id();
        assert!(back.materials.keys().all(|k| k.0 < fresh));
    }

    #[test]
    fn one_element_per_line() {
        let text = save(&sample());
        assert!(text.contains("\n  {\"id\":1,\"p\":[0.0,0.0,0.0]}"));
        assert!(text.lines().any(|l| l.starts_with("  {\"id\":") && l.contains("\"outer\":")));
        // 0.1 + 0.2 style noise is rounded away.
        assert!(!text.contains("0000000000"));
    }

    #[test]
    fn hand_written_file_in_millimetres_loads() {
        let text = r#"{
"format": "rysmo", "version": 1, "length_unit": "mm",
"vertices": [
  {"id": 1, "p": [0, 0, 0]}, {"id": 2, "p": [1000, 0, 0]}, {"id": 3, "p": [1000, 500, 0]}
],
"edges": [{"id": 4, "v": [1, 2]}, {"id": 5, "v": [2, 3]}, {"id": 6, "v": [3, 1]}],
"faces": [{"id": 7, "outer": [1, 3, 2]}]
}"#;
        let doc = load(text).unwrap();
        assert_eq!(doc.root.mesh.face_count(), 1);
        assert!((doc.root.mesh.pos(VertexId(2)).x - 1.0).abs() < 1e-12);
        assert!(doc.root.mesh.face_normal(FaceId(7)).unwrap().z < -0.99);
    }

    #[test]
    fn broken_files_are_refused() {
        assert!(matches!(load(r#"{"format":"other","version":1}"#), Err(IoError::Format(_))));
        assert!(matches!(load(r#"{"format":"rysmo","version":99}"#), Err(IoError::Version(99))));
        let missing_edge = r#"{"format":"rysmo","version":1,
"vertices":[{"id":1,"p":[0,0,0]},{"id":2,"p":[1,0,0]},{"id":3,"p":[1,1,0]}],
"edges":[{"id":4,"v":[1,2]},{"id":5,"v":[2,3]}],
"faces":[{"id":7,"outer":[1,2,3]}]}"#;
        assert!(matches!(load(missing_edge), Err(IoError::Mesh(_))));
        let dup = r#"{"format":"rysmo","version":1,"vertices":[{"id":1,"p":[0,0,0]},{"id":1,"p":[1,0,0]}]}"#;
        assert!(matches!(load(dup), Err(IoError::Format(_))));
    }

    /// A model with a component used twice (one nested in a group), a tag, a guide of
    /// each kind, soft edges and a painted instance.
    fn structured() -> Document {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let f = e.mesh().faces().next().unwrap().0;
        e.apply(Command::PushPull { face: f, distance: -1.0, keep_base: false }).unwrap();
        let edges: Vec<EdgeId> = e.mesh().edges().map(|(id, _)| id).collect();
        e.apply(Command::SetEdges { edges: edges[..3].to_vec(), soft: Some(true), smooth: Some(true), hidden: None })
            .unwrap();
        let all: Vec<rysmo_mesh::Element> = e
            .mesh()
            .edges()
            .map(|(id, _)| rysmo_mesh::Element::Edge(id))
            .chain(e.mesh().faces().map(|(id, _)| rysmo_mesh::Element::Face(id)))
            .collect();
        let c = e.apply(Command::MakeComponent { elements: all, name: Some("Cube".into()) }).unwrap().created[0];
        let copy =
            e.apply(Command::Copy { elements: vec![c], offset: DVec3::new(2.0, 0.0, 0.0), copies: 1 }).unwrap().created
                [0];
        e.apply(Command::Rotate { elements: vec![copy], centre: DVec3::ZERO, axis: DVec3::Z, angle: 30.0, copies: 0 })
            .unwrap();
        let g = e.apply(Command::MakeGroup { elements: vec![copy] }).unwrap().created[0];
        let t = e.apply(Command::AddTag { name: "Boxes".into() }).unwrap().tag;
        let rysmo_mesh::Element::Instance(gi) = g else { panic!() };
        e.apply(Command::AssignTag { instances: vec![gi], tag: t }).unwrap();
        let m = e
            .apply(Command::AddMaterial { name: "Blue".into(), color: [0.2, 0.3, 0.9, 0.5], texture: None })
            .unwrap()
            .material;
        e.apply(Command::PaintInstances { instances: vec![gi], material: m }).unwrap();
        e.apply(Command::AddGuideLine { origin: DVec3::new(0.0, -1.0, 0.0), dir: DVec3::X }).unwrap();
        e.apply(Command::AddGuidePoint { point: DVec3::new(0.5, 0.5, 2.0) }).unwrap();
        e.doc().clone()
    }

    #[test]
    fn groups_components_tags_and_guides_survive_a_round_trip() {
        let doc = structured();
        assert_eq!(doc.definitions.len(), 2);
        let text = save(&doc);
        let back = load(&text).unwrap();
        assert_eq!(back.definitions.len(), 2);
        assert_eq!(back.tags, doc.tags);
        assert_eq!(back.materials, doc.materials);
        assert_eq!(back.root.guides.len(), 2);
        for (id, d) in &doc.definitions {
            let b = &back.definitions[id];
            assert_eq!((b.name.as_str(), b.group), (d.name.as_str(), d.group));
            assert_eq!(b.mesh.face_count(), d.mesh.face_count());
            assert_eq!(b.instances.keys().collect::<Vec<_>>(), d.instances.keys().collect::<Vec<_>>());
            for (iid, inst) in &d.instances {
                let bi = &b.instances[iid];
                assert_eq!((bi.definition, bi.tag, bi.material), (inst.definition, inst.tag, inst.material));
                assert!(bi.transform.abs_diff_eq(inst.transform, 1e-9));
            }
            let soft = |m: &Mesh| m.edges().filter(|(_, e)| e.soft && e.smooth).count();
            assert_eq!(soft(&b.mesh), soft(&d.mesh));
        }
        assert_eq!(back.root.instances.len(), doc.root.instances.len());
        assert_eq!(save(&back), text);
        assert!(text.contains(&format!("\"version\": {VERSION}")));
        assert!(text.lines().any(|l| l.contains("\"def\":") && l.contains("\"x\":")));
        // The loaded document hands out ids above everything in it.
        assert!(back.next_id() >= doc.next_id());
    }

    #[test]
    fn self_containing_component_is_refused() {
        let text = r#"{"format":"rysmo","version":2,
"definitions":[{"id":1,"name":"A"}],
"instances":[{"id":2,"in":1,"def":1,"o":[0,0,0],"x":[1,0,0],"y":[0,1,0],"z":[0,0,1]}]}"#;
        assert!(matches!(load(text), Err(IoError::Format(_))));
        let missing = r#"{"format":"rysmo","version":2,
"instances":[{"id":2,"def":9,"o":[0,0,0],"x":[1,0,0],"y":[0,1,0],"z":[0,0,1]}]}"#;
        assert!(matches!(load(missing), Err(IoError::Format(_))));
    }

    #[test]
    fn the_documented_example_loads() {
        let doc_text = include_str!("../../../docs/file-format.md");
        let start = doc_text.find("```json\n").expect("the doc has a JSON example") + 8;
        let end = start + doc_text[start..].find("\n```").expect("the example ends");
        let doc = load(&doc_text[start..end]).unwrap();
        assert_eq!(doc.root.mesh.face_count(), 1);
        assert_eq!(doc.definitions.len(), 1);
        let block = &doc.definitions[&rysmo_mesh::DefinitionId(20)];
        assert_eq!(block.mesh.face_count(), 6);
        assert_eq!(doc.root.instances.len(), 2);
        assert_eq!(doc.occurrences(&[]).len(), 3);
        assert_eq!(doc.root.guides.len(), 1);
        assert!(block.mesh.edges().any(|(_, e)| e.soft && e.smooth));
        // The turned instance maps the block's corner (0.2, 0, 0) to (3, 0.2, 0).
        let turned = &doc.root.instances[&rysmo_mesh::InstanceId(61)];
        assert!(
            (turned.transform.transform_point3(DVec3::new(0.2, 0.0, 0.0)) - DVec3::new(3.0, 0.2, 0.0)).length() < 1e-12
        );
        // The cube faces outwards.
        let faces: std::collections::BTreeSet<_> = block.mesh.faces().map(|(f, _)| f).collect();
        assert!(block.mesh.signed_volume(&faces) > 0.0);
    }

    #[test]
    fn images_textures_placement_and_style_survive_a_round_trip() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let f = e.mesh().faces().next().unwrap().0;
        let bytes: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        let img = e
            .apply(Command::AddImage { name: "wood.png".into(), mime: "image/png".into(), data: bytes.clone() })
            .unwrap()
            .image
            .unwrap();
        let tex = rysmo_model::Texture { image: img, size: [1.2, 0.3] };
        let m = e
            .apply(Command::AddMaterial { name: "Oak".into(), color: [0.8, 0.6, 0.4, 1.0], texture: Some(tex) })
            .unwrap()
            .material;
        e.apply(Command::Paint { faces: vec![f], material: m, back: true }).unwrap();
        e.apply(Command::SetMaterialGlow { material: m.unwrap(), glow: 1500.0 }).unwrap();
        let pl = rysmo_model::TexturePlacement {
            offset: [0.1, 0.2],
            rotation: 30.0,
            scale: [2.0, 1.0],
            pins: Some([[0., 0.], [1., 0.], [1.2, 1.], [0., 1.]]),
            projected_normal: Some([0., 0., 1.]),
        };
        e.apply(Command::SetTexturePlacement { faces: vec![f], placement: Some(pl) }).unwrap();
        let style = Style { faces: rysmo_model::FaceStyle::HiddenLine, profiles: true, ..Default::default() };
        e.apply(Command::SetStyle { style }).unwrap();
        let text = save(e.doc());
        assert!(
            text.contains(&format!("\"version\": {VERSION}"))
                && text.contains("\"images\": [")
                && text.contains("\"style\": {")
        );
        let back = load(&text).unwrap();
        assert_eq!(back.images[&img].bytes.as_slice(), bytes.as_slice());
        assert_eq!(back.materials, e.doc().materials);
        assert_eq!(back.materials[&m.unwrap()].glow, 1500.0);
        assert_eq!(back.style, style);
        assert_eq!(back.root.mesh.face(f).unwrap().uv, Some(pl));
        assert_eq!(save(&back), text);
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["faces"][0]["uv"]["pins"] = serde_json::json!([[0., 0.], [0., 0.], [0., 0.], [0., 0.]]);
        assert!(load(&value.to_string()).is_err());
        value["faces"][0]["uv"].as_object_mut().unwrap().remove("pins");
        value["faces"][0]["uv"]["projected_normal"] = serde_json::json!([0., 0., 0.]);
        assert!(load(&value.to_string()).is_err());
        value["faces"][0]["uv"].as_object_mut().unwrap().remove("projected_normal");
        value["version"] = serde_json::json!(11);
        let legacy = load(&value.to_string()).unwrap().root.mesh.face(f).unwrap().uv.unwrap();
        assert_eq!(legacy.pins, None);
        assert_eq!(legacy.projected_normal, None);
        let broken = text.replace(&format!("\"image\":{}", img), "\"image\":4242");
        assert!(load(&broken).is_err());
    }
    #[test]
    fn parallel_camera_round_trip_and_legacy_camera_default() {
        let mut doc = Document::new();
        let mut camera = OrbitCamera { parallel: true, ..Default::default() };
        camera.set_standard_view(rysmo_math::StandardView::Top);
        doc.camera = Some(camera);
        let text = save(&doc);
        let loaded = load(&text).unwrap().camera.unwrap();
        assert!(loaded.parallel);
        assert!((loaded.pitch - camera.pitch).abs() < 1e-8);
        let legacy = text.replace(",\"parallel\":true", "");
        assert!(!load(&legacy).unwrap().camera.unwrap().parallel);
    }
    #[test]
    fn eye_height_reference_round_trip_and_camera_units() {
        let mut doc = Document::new();
        let mut camera = OrbitCamera::default();
        camera.place_eye(DVec3::new(2.0, 3.0, 4.7), None);
        camera.set_eye_height(3.0, 1.7);
        doc.camera = Some(camera);
        let text = save(&doc);
        let loaded = load(&text).unwrap().camera.unwrap();
        assert_eq!(loaded.eye_base_z, 3.0);
        assert!((loaded.eye().z - loaded.eye_base_z - 1.7).abs() < 1e-8);
        let millimetres = text.replace("\"length_unit\": \"m\"", "\"length_unit\": \"mm\"");
        let scaled = load(&millimetres).unwrap().camera.unwrap();
        assert!((scaled.target - loaded.target * 0.001).length() < 1e-10);
        assert!((scaled.distance - loaded.distance * 0.001).abs() < 1e-10);
        assert_eq!(scaled.eye_base_z, 0.003);
        let legacy = text.replace(",\"eye_base_z\":3.0", "");
        assert_eq!(load(&legacy).unwrap().camera.unwrap().eye_base_z, 0.0);
    }
    #[test]
    fn curves_survive_save_group_and_load_with_valid_references() {
        let mut e = rysmo_model::Editor::new();
        e.apply(rysmo_model::Command::DrawPolygon {
            centre: DVec3::ZERO,
            normal: DVec3::Z,
            radius: 1.0,
            segments: 24,
            start: None,
        })
        .unwrap();
        let curve = e.mesh().curves().next().unwrap().0;
        e.apply(rysmo_model::Command::SetCurveSegments { curve, segments: 9 }).unwrap();
        let text = save(e.doc());
        let loaded = load(&text).unwrap();
        assert_eq!(loaded.root.mesh.curve(curve).unwrap().segments, 9);
        assert_eq!(loaded.root.mesh.edge_count(), 9);
        assert_eq!(loaded.root.mesh.curves().count(), 1);
        assert_eq!(save(&loaded), text);
        let id = loaded.root.mesh.curves().next().unwrap().0;
        assert!(loaded.root.mesh.edges().all(|(_, e)| e.curve == Some(id)));
        let broken = text.replace(&format!("\"curve\":{}", id.0), "\"curve\":999999");
        assert!(load(&broken).is_err());
        let all = e.mesh().faces().map(|(id, _)| rysmo_mesh::Element::Face(id)).collect();
        e.apply(rysmo_model::Command::MakeGroup { elements: all }).unwrap();
        let grouped = load(&save(e.doc())).unwrap();
        assert_eq!(grouped.definitions.values().map(|d| d.mesh.curves().count()).sum::<usize>(), 1);
    }
    #[test]
    fn custom_axes_round_trip_preserves_frame_and_invalid_axes_are_rejected() {
        let mut e = rysmo_model::Editor::new();
        e.apply(rysmo_model::Command::SetAxes {
            origin: DVec3::new(2.0, 3.0, 4.0),
            x: DVec3::new(1.0, 1.0, 0.0),
            y: DVec3::new(-1.0, 1.0, 0.0),
        })
        .unwrap();
        let text = save(e.doc());
        let loaded = load(&text).unwrap();
        assert_eq!(loaded.root.axes, e.active().axes);
        assert_eq!(save(&loaded), text);
        // Build a malformed frame structurally rather than depend on float formatting.
        let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["axes"]["x"] = serde_json::json!([0.0, 0.0, 0.0]);
        assert!(load(&json.to_string()).is_err());
    }
    #[test]
    fn dimensions_round_trip_contexts_units_and_invalid_entries() {
        use rysmo_model::dimensions::*;
        let dimension = Dimension {
            measure: DimensionMeasure::Linear {
                from: DimensionAnchor { point: DVec3::ZERO, reference: None },
                to: DimensionAnchor { point: DVec3::X * 2., reference: None },
                axis: None,
            },
            normal: DVec3::Z,
            offset: DVec3::Y,
            text: "Width <>".into(),
            alignment: DimensionTextAlignment::DimensionLine,
            font: DimensionFont { size: 28., monospace: true, color: [180, 20, 20] },
            text_position: DimensionTextPosition::OutsideEnd,
            endpoints: DimensionEndpoints::Slash,
        };
        let mut editor = Editor::new();
        let root = editor.apply(Command::AddDimension { dimension: dimension.clone() }).unwrap().dimensions[0];
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let elements = editor.mesh().edges().map(|(id, _)| rysmo_mesh::Element::Edge(id)).collect();
        let group = editor.apply(Command::MakeGroup { elements }).unwrap().created[0];
        let rysmo_mesh::Element::Instance(group) = group else {
            panic!("group");
        };
        editor.enter(group).unwrap();
        let owner = editor.active_definition();
        let child = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
        editor.exit();
        let text = save(editor.doc());
        let mut loaded = load(&text).unwrap();
        assert_eq!(loaded.root.dimensions, editor.doc().root.dimensions);
        assert_eq!(loaded.def(owner).unwrap().dimensions, editor.doc().def(owner).unwrap().dimensions);
        assert_eq!(save(&loaded), text);
        assert!(loaded.reserve_id() > root.0.max(child.0));
        let mm = text.replace("\"length_unit\": \"m\"", "\"length_unit\": \"mm\"");
        let loaded = load(&mm).unwrap();
        assert!((loaded.root.dimensions[&root].resolve(&loaded, None).length - 0.002).abs() < 1e-10);
        assert_eq!(loaded.root.dimensions[&root].offset, DVec3::Y * 0.001);
        let mut broken: serde_json::Value = serde_json::from_str(&text).unwrap();
        broken["dimensions"][0]["normal"] = serde_json::json!([0., 0., 0.]);
        assert!(load(&broken.to_string()).is_err());
        let mut duplicate: serde_json::Value = serde_json::from_str(&text).unwrap();
        duplicate["dimensions"][1]["id"] = serde_json::json!(root.0);
        assert!(load(&duplicate.to_string()).is_err());
    }
    #[test]
    fn legacy_version_four_without_dimensions_still_loads() {
        let doc = Document::new();
        let text = save(&doc).replace("\"version\": 5", "\"version\": 4");
        let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json.as_object_mut().unwrap().remove("dimensions");
        assert!(load(&json.to_string()).unwrap().root.dimensions.is_empty());
    }
    #[test]
    fn dimension_preview_fixture_has_four_live_layouts() {
        let doc = load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        assert_eq!(doc.root.dimensions.len(), 4);
        for dimension in doc.root.dimensions.values() {
            let layout = dimension.layout(&doc, None, DMat4::IDENTITY, 0.1).unwrap();
            assert!(layout.measurement.associated);
            assert!(layout.text_position.is_finite());
            assert!(!layout.segments.is_empty());
            assert!(!dimension.label(&layout.measurement, doc.units).contains("<>"));
        }
    }
}

#[cfg(test)]
mod dimension_visibility_io_tests {
    use super::*;
    #[test]
    fn visibility_round_trip_legacy_defaults_and_invalid_thresholds() {
        let mut doc = Document::new();
        doc.style.dimension_defaults.font.size = 28.;
        doc.style.dimension_defaults.endpoints = rysmo_model::dimensions::DimensionEndpoints::Slash;
        doc.style.dimensions.hide_small = true;
        doc.style.dimensions.minimum_pixels = 60.;
        doc.style.dimensions.hide_foreshortened = true;
        doc.style.dimensions.angle_degrees = 30.;
        doc.style.dimensions.highlight_non_associated = true;
        doc.style.dimensions.highlight_color = [180, 20, 20];
        let text = save(&doc);
        assert_eq!(load(&text).unwrap().style, doc.style);
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["style"]["dimensions"]["angle_degrees"] = serde_json::json!(91);
        assert!(load(&value.to_string()).is_err());
        value["style"]["dimensions"]["angle_degrees"] = serde_json::json!(30);
        value["style"]["dimension_defaults"]["font"]["size"] = serde_json::json!(0);
        assert!(load(&value.to_string()).is_err());
        value["style"].as_object_mut().unwrap().remove("dimension_defaults");
        assert_eq!(load(&value.to_string()).unwrap().style.dimension_defaults, Default::default());
        value["style"].as_object_mut().unwrap().remove("dimensions");
        assert_eq!(load(&value.to_string()).unwrap().style.dimensions, Default::default());
    }
}

#[cfg(test)]
mod grouped_dimension_io_tests {
    use super::*;
    #[test]
    fn grouped_dimension_retains_owner_ids_and_links_after_save_load() {
        let doc = load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        let mut editor = rysmo_model::Editor::with_document(doc);
        let id = rysmo_model::DimensionId(10);
        let face = editor.mesh().faces().next().unwrap().0;
        let out = editor
            .apply(rysmo_model::Command::MakeGroup {
                elements: vec![rysmo_model::Element::Face(face), rysmo_model::Element::Dimension(id)],
            })
            .unwrap();
        let rysmo_model::Element::Instance(instance) = out.created[0] else { panic!("group") };
        let owner = Some(editor.active().instances[&instance].definition);
        let text = save(editor.doc());
        let loaded = load(&text).unwrap();
        assert_eq!(save(&loaded), text);
        assert!(!loaded.root.dimensions.contains_key(&id));
        let dimension = &loaded.def(owner).unwrap().dimensions[&id];
        assert!(dimension.resolve(&loaded, owner).associated);
        assert_eq!(dimension.resolve(&loaded, owner).length, 2.);
        assert!(loaded.definition_bounds(owner).is_some());
    }
}

#[cfg(test)]
mod rotated_dimension_io_tests {
    use super::*;
    #[test]
    fn rotated_circle_and_dimension_round_trip_their_plane_and_live_association() {
        let doc = load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        let mut editor = rysmo_model::Editor::with_document(doc);
        let id = rysmo_model::DimensionId(46);
        let mut elements = editor
            .mesh()
            .curve_edges(rysmo_mesh::CurveId(45))
            .into_iter()
            .map(rysmo_model::Element::Edge)
            .collect::<Vec<_>>();
        elements.push(rysmo_model::Element::Dimension(id));
        editor
            .apply(rysmo_model::Command::Rotate {
                elements,
                centre: DVec3::ZERO,
                axis: DVec3::X,
                angle: 90.,
                copies: 0,
            })
            .unwrap();
        let text = save(editor.doc());
        let loaded = load(&text).unwrap();
        assert_eq!(save(&loaded), text);
        let dimension = &loaded.root.dimensions[&id];
        let measured = dimension.resolve(&loaded, None);
        assert!(measured.associated);
        assert!((measured.length - 0.5).abs() < 1e-8);
        assert!(dimension.normal.distance(-DVec3::Y) < 1e-8);
        assert!(dimension.validate());
    }
}

#[cfg(test)]
mod arc_fallback_io_tests {
    use super::*;
    #[test]
    fn detached_arc_sweep_round_trips_and_invalid_sweeps_are_rejected() {
        let doc = load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        let mut editor = rysmo_model::Editor::with_document(doc);
        let preview = editor
            .preview_dimension_rotate(
                &[rysmo_model::Element::Dimension(rysmo_model::DimensionId(86))],
                DVec3::ZERO,
                DVec3::Z,
                35.,
                false,
            )
            .unwrap();
        let id =
            editor.apply(rysmo_model::Command::AddDimension { dimension: preview[0].clone() }).unwrap().dimensions[0];
        let text = save(editor.doc());
        let loaded = load(&text).unwrap();
        assert_eq!(save(&loaded), text);
        let dimension = &loaded.root.dimensions[&id];
        let rysmo_model::dimensions::DimensionMeasure::Radial { reference, .. } = &dimension.measure else {
            panic!("radial")
        };
        assert!(reference.curve.is_none());
        assert_eq!(reference.arc_sweep, Some(-180.));
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let entry = value["dimensions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["id"] == serde_json::json!(id.0))
            .unwrap();
        entry["measure"]["reference"]["arc_sweep"] = serde_json::json!(361);
        assert!(load(&value.to_string()).is_err());
    }
}

#[cfg(test)]
mod scaled_radial_io_tests {
    use super::*;
    #[test]
    fn affine_radial_fallback_round_trips_scales_units_and_rejects_invalid_axes() {
        let doc = load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        let mut editor = rysmo_model::Editor::with_document(doc);
        let id = rysmo_model::DimensionId(46);
        let mut elements = editor
            .mesh()
            .curve_edges(rysmo_mesh::CurveId(45))
            .into_iter()
            .map(rysmo_model::Element::Edge)
            .collect::<Vec<_>>();
        elements.push(rysmo_model::Element::Dimension(id));
        editor
            .apply(rysmo_model::Command::Scale { elements, origin: DVec3::ZERO, factors: DVec3::new(-2., 1., 1.) })
            .unwrap();
        let text = save(editor.doc());
        let loaded = load(&text).unwrap();
        assert_eq!(save(&loaded), text);
        let dimension = &loaded.root.dimensions[&id];
        let rysmo_model::dimensions::DimensionMeasure::Radial { reference, .. } = &dimension.measure else {
            panic!("radial")
        };
        assert!(reference.radial_axes.is_some());
        assert!(!dimension.resolve(&loaded, None).associated);
        let layout = dimension.layout(&loaded, None, DMat4::IDENTITY, 0.1).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["length_unit"] = serde_json::json!("mm");
        let small = load(&value.to_string()).unwrap();
        let tiny = small.root.dimensions[&id].layout(&small, None, DMat4::IDENTITY, 0.0001).unwrap();
        assert!((tiny.measurement.length - layout.measurement.length * 0.001).abs() < 1e-8);
        let entry = value["dimensions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["id"] == serde_json::json!(id.0))
            .unwrap();
        entry["measure"]["reference"]["radial_axes"] = serde_json::json!([[0, 0, 0], [0, 1, 0]]);
        assert!(load(&value.to_string()).is_err());
    }
}

#[cfg(test)]
mod text_io_tests {
    use super::*;
    #[test]
    fn text_annotations_round_trip_and_reject_invalid_ids_owners_and_placements() {
        use rysmo_math::DVec2;
        use rysmo_model::text::*;
        let mut editor = rysmo_model::Editor::new();
        let screen = Text {
            content: "Podlaží\nČíslo 2".into(),
            placement: TextPlacement::Screen { position: DVec2::new(0.2, 0.3) },
            font: TextFont { size: 24., monospace: true, color: [90, 20, 30] },
            endpoint: TextEndpoint::None,
        };
        editor.apply(rysmo_model::Command::AddText { text: screen.clone() }).unwrap();
        editor.apply(rysmo_model::Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let elements = editor.mesh().edges().map(|(id, _)| rysmo_model::Element::Edge(id)).collect();
        let group = editor.apply(rysmo_model::Command::MakeGroup { elements }).unwrap().created[0];
        let rysmo_model::Element::Instance(group) = group else { panic!() };
        editor.enter(group).unwrap();
        let edge = editor.mesh().edges().next().unwrap().0;
        let attached = Text {
            content: "Hrana".into(),
            placement: TextPlacement::Attached {
                anchor: TextAnchor {
                    point: DVec3::X * 0.5,
                    reference: Some(TextReference::Point {
                        reference: rysmo_model::dimensions::PointReference {
                            path: vec![],
                            entity: rysmo_model::dimensions::PointEntity::Edge { edge, parameter: 0.5 },
                        },
                    }),
                },
                leader: TextLeader::Pushpin { offset: DVec3::Y },
            },
            font: Default::default(),
            endpoint: TextEndpoint::Dot,
        };
        editor.apply(rysmo_model::Command::AddText { text: attached }).unwrap();
        let owner = editor.active_definition();
        let saved = save(editor.doc());
        let restored = load(&saved).unwrap();
        assert_eq!(restored.root.texts, editor.doc().root.texts);
        assert_eq!(restored.def(owner).unwrap().texts, editor.active().texts);
        let mut value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        value["length_unit"] = serde_json::json!("mm");
        let millimetres = load(&value.to_string()).unwrap();
        assert_eq!(millimetres.root.texts.values().next().unwrap(), &screen);
        let text = millimetres.def(owner).unwrap().texts.values().next().unwrap();
        let TextPlacement::Attached { anchor, leader: TextLeader::Pushpin { offset } } = &text.placement else {
            panic!()
        };
        assert_eq!(anchor.point, DVec3::X * 0.0005);
        assert_eq!(*offset, DVec3::Y * 0.001);
        let mut invalid = value.clone();
        invalid["texts"][1]["id"] = invalid["texts"][0]["id"].clone();
        assert!(load(&invalid.to_string()).is_err());
        let mut invalid = value.clone();
        invalid["texts"][0]["in"] = serde_json::json!(u64::MAX);
        assert!(load(&invalid.to_string()).is_err());
        let mut invalid = value.clone();
        invalid["texts"][0]["font"]["size"] = serde_json::json!(0);
        assert!(load(&invalid.to_string()).is_err());
        value["version"] = serde_json::json!(5);
        value.as_object_mut().unwrap().remove("texts");
        assert!(load(&value.to_string()).unwrap().root.texts.is_empty());
    }
    #[test]
    fn edge_material_round_trips_validates_and_defaults_in_old_files() {
        let mut editor = rysmo_model::Editor::new();
        editor.apply(rysmo_model::Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let edge = editor.mesh().edges().next().unwrap().0;
        let material = editor
            .apply(rysmo_model::Command::AddMaterial { name: "Edge".into(), color: [1., 0., 0., 1.], texture: None })
            .unwrap()
            .material;
        editor
            .apply(rysmo_model::Command::PaintElements {
                elements: vec![rysmo_model::Element::Edge(edge)],
                material,
                back: false,
            })
            .unwrap();
        let saved = save(editor.doc());
        let reopened = load(&saved).unwrap();
        assert_eq!(reopened.root.mesh.edge(edge).unwrap().material, material);
        assert_eq!(reopened.materials, editor.doc().materials);
        assert_eq!(save(&reopened), saved);
        let mut value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        value["edges"][0]["material"] = serde_json::json!(u64::MAX);
        assert!(load(&value.to_string()).is_err());
        value["edges"][0].as_object_mut().unwrap().remove("material");
        value["version"] = serde_json::json!(10);
        assert_eq!(load(&value.to_string()).unwrap().root.mesh.edge(edge).unwrap().material, None);
    }
    #[test]
    fn renderer_settings_round_trip_validate_and_default_for_older_files() {
        let mut doc = Document::new();
        doc.style.sun_intensity = 2.;
        doc.style.ambient_intensity = 0.;
        doc.style.shadow_quality = rysmo_model::ShadowQuality::Low;
        let saved = save(&doc);
        assert_eq!(load(&saved).unwrap().style, doc.style);
        let mut value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        value["style"]["sun_intensity"] = serde_json::json!(-1);
        assert!(load(&value.to_string()).is_err());
        for key in ["sun_intensity", "ambient_intensity", "shadow_quality"] {
            value["style"].as_object_mut().unwrap().remove(key);
        }
        let style = load(&value.to_string()).unwrap().style;
        assert_eq!(style.sun_intensity, 1.);
        assert_eq!(style.ambient_intensity, 1.);
        assert_eq!(style.shadow_quality, rysmo_model::ShadowQuality::High);
    }
    #[test]
    fn text_defaults_round_trip_validate_and_load_legacy_style() {
        let mut doc = Document::new();
        doc.style.text_defaults.screen_font.size = 26.;
        doc.style.text_defaults.leader_font.monospace = true;
        doc.style.text_defaults.leader = rysmo_model::text::TextLeaderKind::Pushpin;
        let saved = save(&doc);
        assert_eq!(load(&saved).unwrap().style.text_defaults, doc.style.text_defaults);
        let mut value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        value["style"]["text_defaults"]["screen_font"]["size"] = serde_json::json!(0);
        assert!(load(&value.to_string()).is_err());
        value["style"].as_object_mut().unwrap().remove("text_defaults");
        assert_eq!(load(&value.to_string()).unwrap().style.text_defaults, Default::default());
    }
    #[test]
    fn displaced_no_leader_text_round_trips_and_legacy_none_defaults_to_zero() {
        use rysmo_model::text::*;
        let mut editor = rysmo_model::Editor::new();
        let text = Text {
            content: "Area".into(),
            placement: TextPlacement::Attached {
                anchor: TextAnchor { point: DVec3::ZERO, reference: None },
                leader: TextLeader::None { offset: DVec3::Y },
            },
            font: Default::default(),
            endpoint: Default::default(),
        };
        editor.apply(rysmo_model::Command::AddText { text }).unwrap();
        let saved = save(editor.doc());
        assert_eq!(load(&saved).unwrap().root.texts, editor.doc().root.texts);
        let mut value: serde_json::Value = serde_json::from_str(&saved).unwrap();
        value["version"] = serde_json::json!(6);
        value["texts"][0]["placement"]["leader"].as_object_mut().unwrap().remove("offset");
        let doc = load(&value.to_string()).unwrap();
        assert!(
            matches!(doc.root.texts.values().next().unwrap().placement,TextPlacement::Attached {leader:TextLeader::None {offset},..} if offset==DVec3::ZERO)
        );
    }
}

#[cfg(test)]
mod text3d_tests {
    use super::*;
    use rysmo_model::{Command, Editor, text3d::Text3dSettings};
    #[test]
    fn geometric_text_preserves_holes_extrusion_and_placement_on_save() {
        let mut editor = Editor::new();
        editor
            .apply(Command::CreateText3d {
                content: "Přízemí 8".into(),
                settings: Text3dSettings { height: 0.25, depth: 0.03, ..Default::default() },
                origin: DVec3::new(2., 3., 4.),
                x: DVec3::Y,
                y: DVec3::Z,
            })
            .unwrap();
        let original = editor.doc();
        let back = load(&save(original)).unwrap();
        assert_eq!(back.root.instances, original.root.instances);
        for (id, definition) in &original.definitions {
            let restored = &back.definitions[id];
            assert_eq!(restored.name, definition.name);
            assert_eq!(restored.mesh.edges().count(), definition.mesh.edges().count());
            assert_eq!(restored.mesh.faces().count(), definition.mesh.faces().count());
            assert!(restored.mesh.faces().any(|(_, f)| !f.holes.is_empty()));
            for (id, vertex) in definition.mesh.vertices() {
                assert!(restored.mesh.pos(id).distance(vertex.pos) < 1e-9);
            }
            restored.mesh.validate().unwrap();
        }
    }
}

#[cfg(test)]
mod terrain_tests {
    use super::*;
    use rysmo_model::{Command, Editor};
    #[test]
    fn smooved_terrain_roundtrip_preserves_positions_topology_and_can_be_edited() {
        use rysmo_model::Element;
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(2., 3., 4.),
                u: DVec3::X * 4.,
                v: DVec3::Y * 4.,
                spacing: 1.,
            })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        let vertices = editor.mesh().vertices().map(|(id, _)| id).collect::<Vec<_>>();
        editor
            .apply(Command::Smoove {
                vertices: vertices.clone(),
                centres: vec![DVec3::new(2., 2., 0.)],
                radius: 2.,
                normal: DVec3::Z,
                offset: DVec3::Z * 1.25,
            })
            .unwrap();
        let original = editor.doc();
        let restored = load(&save(original)).unwrap();
        assert_eq!(restored.root.instances, original.root.instances);
        let definition = original.root.instances[&instance].definition;
        let source = &original.definitions[&definition].mesh;
        let target = &restored.definitions[&definition].mesh;
        for (id, v) in source.vertices() {
            assert!(target.pos(id).distance(v.pos) < 1e-9);
        }
        for (id, f) in source.faces() {
            assert_eq!(target.face(id), Some(f));
        }
        for (id, e) in source.edges() {
            assert_eq!(target.edge(id), Some(e));
        }
        let mut reopened = Editor::new();
        reopened.replace_document(restored);
        reopened.enter(instance).unwrap();
        let before = reopened.doc().clone();
        reopened
            .apply(Command::Smoove {
                vertices,
                centres: vec![DVec3::new(2., 2., 1.25)],
                radius: 2.,
                normal: DVec3::Z,
                offset: -DVec3::Z * 0.5,
            })
            .unwrap();
        reopened.mesh().validate().unwrap();
        reopened.undo();
        assert_eq!(reopened.doc(), &before);
    }
    #[test]
    fn terrain_grid_roundtrip_preserves_triangles_flags_and_frame() {
        let mut editor = Editor::new();
        editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(4., 3., 2.),
                u: DVec3::X * 2.5,
                v: -DVec3::Y * 1.5,
                spacing: 1.,
            })
            .unwrap();
        let original = editor.doc();
        let back = load(&save(original)).unwrap();
        assert_eq!(back.root.instances, original.root.instances);
        for (id, definition) in &original.definitions {
            let restored = &back.definitions[id];
            assert_eq!(restored.mesh.faces().count(), 12);
            assert_eq!(restored.mesh.edges().filter(|(_, e)| e.soft && e.smooth).count(), 6);
            for (id, face) in definition.mesh.faces() {
                assert_eq!(restored.mesh.face(id), Some(face));
            }
            for (id, vertex) in definition.mesh.vertices() {
                assert!(restored.mesh.pos(id).distance(vertex.pos) < 1e-9);
            }
            restored.mesh.validate().unwrap();
        }
    }
}

#[cfg(test)]
mod contour_tests {
    use super::*;
    use rysmo_model::{Command, Editor, Element};
    #[test]
    fn generated_contour_terrain_and_curves_survive_save_and_smoove() {
        let mut editor = Editor::new();
        for (radius, height) in [(4., 0.), (2., 2.)] {
            editor
                .apply(Command::DrawCircle {
                    centre: DVec3::Z * height,
                    normal: DVec3::Z,
                    radius,
                    segments: 12,
                    start: Some(DVec3::X),
                })
                .unwrap();
        }
        let edges = editor.mesh().edges().map(|(id, _)| id).collect();
        let out = editor.apply(Command::TerrainFromContours { edges }).unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        let original = editor.doc();
        let back = load(&save(original)).unwrap();
        assert_eq!(back.root.instances, original.root.instances);
        for (id, definition) in &original.definitions {
            let restored = &back.definitions[id];
            assert_eq!(
                restored.mesh.curves().map(|(id, c)| (id, *c)).collect::<Vec<_>>(),
                definition.mesh.curves().map(|(id, c)| (id, *c)).collect::<Vec<_>>()
            );
            for (id, v) in definition.mesh.vertices() {
                assert!(restored.mesh.pos(id).distance(v.pos) < 1e-9);
            }
            for (id, e) in definition.mesh.edges() {
                assert_eq!(restored.mesh.edge(id), Some(e));
            }
            for (id, f) in definition.mesh.faces() {
                assert_eq!(restored.mesh.face(id), Some(f));
            }
        }
        let mut reopened = Editor::new();
        reopened.replace_document(back);
        reopened.enter(instance).unwrap();
        let vertices = reopened.mesh().vertices().map(|(id, _)| id).collect();
        let before = reopened.doc().clone();
        reopened
            .apply(Command::Smoove {
                vertices,
                centres: vec![DVec3::new(4., 4., 2.)],
                radius: 3.,
                normal: DVec3::Z,
                offset: DVec3::Z * 0.5,
            })
            .unwrap();
        reopened.mesh().validate().unwrap();
        reopened.undo();
        assert_eq!(reopened.doc(), &before);
    }
}

#[cfg(test)]
mod flip_tests {
    use super::*;
    use rysmo_model::{Command, Editor, Element};
    #[test]
    fn flipped_terrain_roundtrip_preserves_topology_and_can_flip_again() {
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::new(2., 3., 4.), u: DVec3::X, v: DVec3::Y, spacing: 1. })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        let diagonal = editor.mesh().edges().find(|(_, e)| e.soft).unwrap().0;
        let out = editor.apply(Command::FlipTerrainEdge { edge: diagonal }).unwrap();
        let Element::Edge(replacement) = out.created[0] else { panic!() };
        let original = editor.doc();
        let restored = load(&save(original)).unwrap();
        assert_eq!(restored.root.instances, original.root.instances);
        let definition = original.root.instances[&instance].definition;
        let source = &original.definitions[&definition].mesh;
        let target = &restored.definitions[&definition].mesh;
        for (id, f) in source.faces() {
            assert_eq!(target.face(id), Some(f));
        }
        for (id, e) in source.edges() {
            assert_eq!(target.edge(id), Some(e));
        }
        for (id, v) in source.vertices() {
            assert!(target.pos(id).distance(v.pos) < 1e-9);
        }
        let mut reopened = Editor::new();
        reopened.replace_document(restored);
        reopened.enter(instance).unwrap();
        let before = reopened.doc().clone();
        reopened.apply(Command::FlipTerrainEdge { edge: replacement }).unwrap();
        reopened.mesh().validate().unwrap();
        reopened.undo();
        assert_eq!(reopened.doc(), &before);
    }
}

#[cfg(test)]
mod detail_tests {
    use super::*;
    use rysmo_model::{Command, Editor, Element};
    #[test]
    fn detailed_terrain_roundtrip_preserves_subdivision_and_can_be_edited() {
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::new(2., 3., 4.), u: DVec3::X, v: DVec3::Y, spacing: 1. })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        editor
            .apply(Command::AddTerrainDetail { face, point: DVec3::new(0.75, 0.25, 0.), offset: DVec3::Z * 0.5 })
            .unwrap();
        let original = editor.doc();
        let restored = load(&save(original)).unwrap();
        assert_eq!(restored.root.instances, original.root.instances);
        let definition = original.root.instances[&instance].definition;
        let source = &original.definitions[&definition].mesh;
        let target = &restored.definitions[&definition].mesh;
        assert_eq!(target.faces().count(), 4);
        assert_eq!(target.vertices().count(), 5);
        for (id, v) in source.vertices() {
            assert!(target.pos(id).distance(v.pos) < 1e-9);
        }
        for (id, e) in source.edges() {
            assert_eq!(target.edge(id), Some(e));
        }
        for (id, f) in source.faces() {
            assert_eq!(target.face(id), Some(f));
        }
        let mut reopened = Editor::new();
        reopened.replace_document(restored);
        reopened.enter(instance).unwrap();
        let before = reopened.doc().clone();
        let point = DVec3::new(0.75, 0.25, 0.5);
        let face = reopened.mesh().faces().find(|(id, _)| reopened.mesh().face_contains(*id, point)).unwrap().0;
        reopened.apply(Command::AddTerrainDetail { face, point, offset: DVec3::Z * 0.1 }).unwrap();
        reopened.mesh().validate().unwrap();
        reopened.undo();
        assert_eq!(reopened.doc(), &before);
    }
    #[test]
    fn raw_stamp_roundtrip_keeps_source_and_can_be_stamped_again() {
        let mut document = Document::default();
        document.with_mesh(None, |mesh| *mesh = rysmo_model::terrain::grid(6., 6., 1.).unwrap()).unwrap();
        let mut editor = Editor::new();
        editor.replace_document(document);
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(2., 2., 5.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let source = editor.mesh().faces().find(|(_, face)| face.outer.len() == 4).unwrap().0;
        let target = editor.mesh().faces().find(|(_, face)| face.outer.len() == 3).unwrap().0;
        editor
            .apply(Command::StampOnFace { sources: vec![Element::Face(source)], target, offset: 0.5, height: 2. })
            .unwrap();
        let restored = load(&save(editor.doc())).unwrap();
        assert_eq!(restored.root.mesh, editor.doc().root.mesh);
        let mut reopened = Editor::new();
        reopened.replace_document(restored);
        let before = reopened.doc().clone();
        let target = reopened.mesh().faces().find(|(_, face)| face.outer.len() == 3).unwrap().0;
        reopened
            .apply(Command::StampOnFace { sources: vec![Element::Face(source)], target, offset: 0.75, height: 2.5 })
            .unwrap();
        assert!(reopened.mesh().find_vertex(DVec3::new(3., 3., 2.5)).is_some());
        reopened.undo();
        assert_eq!(reopened.doc(), &before);
    }
    #[test]
    fn stamped_terrain_roundtrip_preserves_identity_and_continued_editing() {
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(2., 3., 4.),
                u: DVec3::X * 6.,
                v: DVec3::Y * 6.,
                spacing: 1.,
            })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(4., 5., 8.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let command = |height| Command::StampTerrainInstance {
            sources: vec![Element::Face(face)],
            target: instance,
            offset: 0.5,
            height,
        };
        editor.apply(command(6.)).unwrap();
        let original = editor.doc();
        let restored = load(&save(original)).unwrap();
        assert_eq!(restored.root.instances, original.root.instances);
        assert_eq!(restored.root.mesh, original.root.mesh);
        let definition = original.root.instances[&instance].definition;
        let source = &original.definitions[&definition].mesh;
        let target = &restored.definitions[&definition].mesh;
        for (id, vertex) in source.vertices() {
            assert!(target.pos(id).distance(vertex.pos) < 1e-9);
        }
        assert_eq!(target.vertices().count(), source.vertices().count());
        for (id, edge) in source.edges() {
            assert_eq!(target.edge(id), Some(edge));
        }
        for (id, face) in source.faces() {
            assert_eq!(target.face(id), Some(face));
        }
        target.validate().unwrap();
        let mut reopened = Editor::new();
        reopened.replace_document(restored);
        let before = reopened.doc().clone();
        reopened.apply(command(5.)).unwrap();
        let mesh = &reopened.doc().definitions[&definition].mesh;
        let height = rysmo_model::terrain_surface::TerrainSurface::new(mesh)
            .unwrap()
            .sample(rysmo_math::DVec2::new(3., 3.))
            .unwrap()
            .0;
        assert!((height - 1.).abs() < 1e-9);
        reopened.undo();
        assert_eq!(reopened.doc(), &before);
    }
    #[test]
    fn draped_parent_edges_roundtrip_and_allow_continued_editing() {
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(2., 3., 4.),
                u: DVec3::X * 4.,
                v: DVec3::Y * 4.,
                spacing: 1.,
            })
            .unwrap();
        let Element::Instance(target) = out.created[0] else { panic!() };
        editor
            .apply(Command::DrawLines {
                lines: vec![[DVec3::new(1., 4.5, 10.), DVec3::new(7., 4.5, 10.)]],
                facing: None,
            })
            .unwrap();
        let edges = editor.mesh().edges().map(|(id, _)| id).collect::<Vec<_>>();
        let terrain = editor.doc().definitions.clone();
        editor.apply(Command::DrapeEdges { edges: edges.clone(), target }).unwrap();
        assert_eq!(editor.doc().definitions, terrain);
        let original = editor.doc();
        let restored = load(&save(original)).unwrap();
        assert_eq!(restored.root.mesh, original.root.mesh);
        assert_eq!(restored.root.instances, original.root.instances);
        assert_eq!(restored.definitions, original.definitions);
        let mut reopened = Editor::new();
        reopened.replace_document(restored);
        reopened
            .apply(Command::DrawLines { lines: vec![[DVec3::new(4., 2., 12.), DVec3::new(4., 8., 12.)]], facing: None })
            .unwrap();
        let sources = reopened
            .mesh()
            .edges()
            .filter(|(_, edge)| edge.v.iter().all(|id| (reopened.mesh().pos(*id).z - 12.).abs() < 1e-9))
            .map(|(id, _)| id)
            .collect();
        let before = reopened.doc().clone();
        reopened.apply(Command::DrapeEdges { edges: sources, target }).unwrap();
        assert!(reopened.mesh().vertices().count() > before.root.mesh.vertices().count());
        assert_eq!(reopened.doc().definitions, before.definitions);
        reopened.undo();
        assert_eq!(reopened.doc(), &before);
    }
}

#[cfg(test)]
mod unit_settings_tests {
    use super::*;
    use rysmo_model::{Command, Editor};
    #[test]
    fn units_precision_roundtrip_and_legacy_defaults() {
        for units in [
            Units::Millimetres,
            Units::Centimetres,
            Units::Metres,
            Units::Inches,
            Units::Feet,
            Units::Architectural,
            Units::Fractional,
        ] {
            let mut editor = Editor::new();
            editor.apply(Command::SetUnits { units }).unwrap();
            editor.apply(Command::SetLengthPrecision { precision: Some(5) }).unwrap();
            let text = save(editor.doc());
            let back = load(&text).unwrap();
            assert_eq!(back.units, units);
            assert_eq!(back.length_precision, Some(5));
            assert_eq!(back.format_length(1.23456789), editor.doc().format_length(1.23456789));
        }
        let mut value: serde_json::Value = serde_json::from_str(&save(&Document::default())).unwrap();
        value["version"] = serde_json::json!(12);
        value.as_object_mut().unwrap().remove("length_precision");
        assert_eq!(load(&value.to_string()).unwrap().length_precision, None);
        for invalid in [serde_json::json!(7), serde_json::json!(-1), serde_json::json!(1.5)] {
            value["length_precision"] = invalid;
            assert!(load(&value.to_string()).is_err());
        }
    }
}

#[cfg(test)]
mod editor_basics_roundtrip_tests {
    use super::*;
    use rysmo_model::{Command, Editor, Element, MeasurementSettings};
    #[test]
    fn measurement_visibility_and_scene_defaults_roundtrip_and_reject_invalid_settings() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        e.apply(Command::SetGeometryHidden { elements: vec![Element::Face(face)], hidden: true }).unwrap();
        let mut style = e.doc().style;
        style.show_hidden_geometry = true;
        e.apply(Command::SetStyle { style }).unwrap();
        let settings = MeasurementSettings {
            show_unit_markers: false,
            force_zero_inches: true,
            area_units: Some(Units::Feet),
            area_precision: 2,
            volume_units: Some(Units::Centimetres),
            volume_precision: 4,
            length_snap: Some(0.0125),
            angle_precision: 3,
            angle_snap: Some(22.5),
        };
        e.apply(Command::SetMeasurementSettings { settings }).unwrap();
        let scene = rysmo_model::scenes::Scene::capture(e.doc(), OrbitCamera::default(), "Hidden face".into());
        e.apply(Command::AddScene { scene }).unwrap();
        let text = save(e.doc());
        let doc = load(&text).unwrap();
        assert_eq!(doc.measurement, settings);
        let mut old: serde_json::Value = serde_json::from_str(&text).unwrap();
        old["version"] = serde_json::json!(15);
        old["measurement"].as_object_mut().unwrap().remove("volume_units");
        old["measurement"].as_object_mut().unwrap().remove("volume_precision");
        let old = load(&old.to_string()).unwrap();
        assert_eq!(old.measurement.volume_units, None);
        assert_eq!(old.measurement.volume_precision, 3);
        let mut bad: serde_json::Value = serde_json::from_str(&text).unwrap();
        bad["measurement"]["volume_precision"] = serde_json::json!(7);
        assert!(load(&bad.to_string()).is_err());

        assert!(doc.style.show_hidden_geometry);
        assert!(doc.root.mesh.face(face).unwrap().hidden);
        assert!(doc.scenes[0].contexts[0].hidden_faces.contains(&face));
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["measurement"]["length_snap"] = serde_json::json!(-1);
        assert!(load(&value.to_string()).is_err());
        let mut legacy: serde_json::Value = serde_json::from_str(&save(&Document::default())).unwrap();
        legacy["version"] = serde_json::json!(13);
        legacy.as_object_mut().unwrap().remove("measurement");
        assert_eq!(load(&legacy.to_string()).unwrap().measurement, MeasurementSettings::default());
    }
    #[test]
    fn glued_opening_persists_and_export_uses_the_cut_surface() {
        use rysmo_model::components::GlueMode;
        let mut e = Editor::new();
        let geometry = e
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        let made = e.apply(Command::MakeComponent { elements: geometry, name: Some("Window".into()) }).unwrap();
        let Element::Instance(original) = made.created[0] else { panic!() };
        let definition = e.doc().root.instances[&original].definition;
        e.apply(Command::Erase { elements: vec![Element::Instance(original)] }).unwrap();
        let mut settings = e.doc().definitions[&definition].component.clone();
        settings.glue = GlueMode::Any;
        settings.cut_opening = true;
        e.apply(Command::SetComponentProperties { definition, name: "Window".into(), settings }).unwrap();
        e.apply(Command::DrawRectangle {
            corner: DVec3::new(2., 2., 0.),
            u: DVec3::X * 10.,
            v: DVec3::Y * 10.,
            facing: Some(DVec3::Z),
        })
        .unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        let before_export = crate::obj::export(e.doc(), "model.mtl").obj;
        let made = e
            .apply(Command::PlaceGluedInstance {
                definition,
                face,
                origin: DVec3::new(5., 5., 0.),
                rotation: 0.,
                scale: 1.,
            })
            .unwrap();
        let Element::Instance(instance) = made.created[0] else { panic!() };
        let doc = load(&save(e.doc())).unwrap();
        assert_eq!(doc.root.instances[&instance].glued_to, Some(face));
        assert!((doc.effective_mesh(None).face_area(face) - 99.).abs() < 1e-8);
        let exported = crate::obj::export(&doc, "model.mtl").obj;
        assert!(
            exported.lines().filter(|line| line.starts_with("f ")).count()
                > before_export.lines().filter(|line| line.starts_with("f ")).count()
        );
        assert!(!crate::gltf::export(&doc).is_empty());
        let mut malformed: serde_json::Value = serde_json::from_str(&save(e.doc())).unwrap();
        malformed["instances"][0]["glued_to"] = serde_json::json!(999999);
        assert!(load(&malformed.to_string()).is_err());
    }
}

#[cfg(test)]
mod measured_guide_tests {
    use super::*;
    use rysmo_model::{Command, Editor};
    #[test]
    fn guide_origin_roundtrip_and_transform_preserve_measurement_segment() {
        let mut e = Editor::new();
        let start = DVec3::new(0.25, 0.5, 0.);
        let point = DVec3::new(1., 0.5, 0.);
        let out = e.apply(Command::AddGuideSegment { start, point }).unwrap();
        let before = save(e.doc());
        assert_eq!(save(&load(&before).unwrap()), before);
        e.apply(Command::Move { elements: out.created, offset: DVec3::Y }).unwrap();
        assert!(
            matches!(e.active().guides.values().next(), Some(Guide::Point { p, origin: Some(origin), .. }) if *p == point + DVec3::Y && *origin == start + DVec3::Y)
        );
        e.undo();
        assert_eq!(save(e.doc()), before);
    }
}

#[cfg(test)]
mod centre_arc_roundtrip_tests {
    use super::*;
    use rysmo_model::{Command, Editor};
    #[test]
    fn major_pie_face_and_curve_identity_roundtrip_and_export() {
        let mut e = Editor::new();
        e.apply(Command::DrawCentreArc {
            centre: DVec3::new(1., 2., 3.),
            normal: DVec3::Z,
            start: DVec3::X,
            radius: 2.,
            sweep: -270.,
            segments: 24,
            pie: true,
        })
        .unwrap();
        let loaded = load(&save(e.doc())).unwrap();
        // The native writer rounds coordinates to nine decimal places.
        assert_eq!(loaded.root.mesh.faces().collect::<Vec<_>>(), e.mesh().faces().collect::<Vec<_>>());
        assert_eq!(loaded.root.mesh.edges().collect::<Vec<_>>(), e.mesh().edges().collect::<Vec<_>>());
        assert_eq!(loaded.root.mesh.vertex_count(), e.mesh().vertex_count());
        for (id, v) in e.mesh().vertices() {
            assert!(loaded.root.mesh.pos(id).distance(v.pos) < 1e-8);
        }

        let (id, curve) = loaded.root.mesh.curves().next().unwrap();
        assert_eq!(curve.sweep, -270.);
        assert_eq!(curve.radius, 2.);
        assert_eq!(loaded.root.mesh.curve_edges(id).len(), 24);
        let exported = crate::obj::export(&loaded, "sector.mtl");
        assert!(exported.obj.lines().any(|line| line.starts_with("f ")));
        let mut reopened = Editor::with_document(loaded);
        reopened.apply(Command::SetCurveSegments { curve: id, segments: 12 }).unwrap();
        reopened.apply(Command::SetCurveRadius { curve: id, radius: 2.5 }).unwrap();
        assert_eq!(reopened.mesh().face_count(), 1);
        assert_eq!(reopened.mesh().curve(id).unwrap().radius, 2.5);
    }
}

#[cfg(test)]
mod three_point_arc_roundtrip_tests {
    use super::*;
    use rysmo_model::{Command, Editor};
    #[test]
    fn through_vertex_and_analytic_identity_survive_save_load_and_further_edits() {
        let point = DVec3::new(0.37_f64.cos(), 0.37_f64.sin(), 0.);
        let mut e = Editor::new();
        e.apply(Command::DrawThreePointArc {
            start: DVec3::X,
            through: point,
            end: -DVec3::X,
            normal: DVec3::Z,
            segments: 12,
        })
        .unwrap();
        let pivot = e.mesh().find_vertex(point).unwrap();
        let loaded = load(&save(e.doc())).unwrap();
        assert!(loaded.root.mesh.pos(pivot).distance(point) < 1e-8);
        let (id, c) = loaded.root.mesh.curves().next().unwrap();
        assert_eq!(c.kind, rysmo_mesh::CurveKind::Arc);
        assert!((c.sweep - 180.).abs() < 1e-7);
        assert_eq!(loaded.root.mesh.curve_edges(id).len(), 12);
        assert!(
            loaded.root.mesh.curve_edges(id).iter().any(|id| loaded.root.mesh.edge(*id).unwrap().v.contains(&pivot))
        );
        let mut reopened = Editor::with_document(loaded);
        reopened.apply(Command::SetCurveRadius { curve: id, radius: 2. }).unwrap();
        reopened.apply(Command::SetCurveSegments { curve: id, segments: 24 }).unwrap();
        assert_eq!(reopened.mesh().curve(id).unwrap().segments, 24);
        assert!((reopened.mesh().curve(id).unwrap().radius - 2.).abs() < 1e-8);
    }
}

#[cfg(test)]
mod renderer_settings_tests {
    #[test]
    fn extended_render_style_round_trips_and_version_16_defaults_load() {
        let mut doc = rysmo_model::Document::new();
        doc.style = rysmo_model::Style {
            lighting: rysmo_model::Lighting::Studio,
            studio_environment: rysmo_model::StudioEnvironment::Warm,
            studio_rotation: 35.,
            exposure: 1.,
            render_scale: 1.5,
            texture_limit: 1024,
            texture_anisotropy: 8,
            ..Default::default()
        };
        let saved = crate::save(&doc);
        assert_eq!(crate::load(&saved).unwrap().style, doc.style);
        let mut old: serde_json::Value = serde_json::from_str(&saved).unwrap();
        old["version"] = serde_json::json!(16);
        old["style"]["lighting"] = serde_json::json!("sun");
        for key in [
            "studio_environment",
            "studio_intensity",
            "studio_rotation",
            "exposure",
            "render_scale",
            "texture_limit",
            "texture_anisotropy",
        ] {
            old["style"].as_object_mut().unwrap().remove(key);
        }
        let loaded = crate::load(&serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!(loaded.style.render_scale, 1.);
        assert_eq!(loaded.style.exposure, 0.);
        assert_eq!(loaded.style.texture_anisotropy, 4);
    }
}
