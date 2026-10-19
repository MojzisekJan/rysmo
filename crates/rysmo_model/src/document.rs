//! The document: the model's own geometry, group and component definitions, their
//! instances, tags, materials and construction guides.
//!
//! Document structure: a definition holds geometry (a mesh), guides and instances of
//! other definitions. The model itself is the root definition. A group is a definition
//! with exactly one instance, made unique again if a copy of it is edited; a component
//! definition can have many instances, and editing one edits them all.

use std::collections::{BTreeMap, BTreeSet};

use rysmo_math::{DMat4, DVec3, OrbitCamera};
use rysmo_mesh::{DefinitionId, DimensionId, GuideId, InstanceId, MaterialId, Mesh, TagId};
use serde::{Deserialize, Serialize};

use crate::Units;

/// A paint colour. `color` is sRGB with alpha (opacity), each 0 to 1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    pub color: [f32; 4],
    /// An image laid over the colour, if the material is textured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<crate::Texture>,
    /// Light the faces give off, in cd/m²; 0 for none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub glow: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// A visibility tag (layer). Instances on a hidden tag are not drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    pub visible: bool,
}

/// A construction guide made by the Tape Measure: an infinite line or a point.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Guide {
    Line {
        origin: DVec3,
        dir: DVec3,
        #[serde(default)]
        hidden: bool,
    },
    Point {
        p: DVec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<DVec3>,
        #[serde(default)]
        hidden: bool,
    },
}

impl Guide {
    pub fn hidden(&self) -> bool {
        match self {
            Self::Line { hidden, .. } | Self::Point { hidden, .. } => *hidden,
        }
    }
    pub fn set_hidden(&mut self, value: bool) {
        match self {
            Self::Line { hidden, .. } | Self::Point { hidden, .. } => *hidden = value,
        }
    }
}

/// An instance of a definition, placed by a transform from the definition's
/// coordinates to its parent's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Instance {
    pub definition: DefinitionId,
    pub transform: DMat4,
    pub name: String,
    pub tag: Option<TagId>,
    /// Paints the faces inside that have no material of their own.
    pub material: Option<MaterialId>,
    pub hidden: bool,
    #[serde(default)]
    pub glued_to: Option<crate::FaceId>,
}

/// Geometry, guides and child instances: the model itself, a group or a component.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Definition {
    pub name: String,
    /// A group rather than a component.
    pub group: bool,
    pub mesh: Mesh,
    pub instances: BTreeMap<InstanceId, Instance>,
    pub guides: BTreeMap<GuideId, Guide>,
    pub dimensions: BTreeMap<DimensionId, crate::dimensions::Dimension>,
    pub texts: BTreeMap<rysmo_mesh::TextId, crate::text::Text>,
    pub axes: rysmo_math::DrawingAxes,
    pub sections: crate::sections::Sections,
    pub component: crate::components::ComponentSettings,
}

/// Everything that is saved in a file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// The model's own geometry and its top-level groups and components.
    pub root: Definition,
    pub definitions: BTreeMap<DefinitionId, Definition>,
    pub materials: BTreeMap<MaterialId, Material>,
    pub images: BTreeMap<rysmo_mesh::ImageId, crate::Image>,
    pub tags: BTreeMap<TagId, Tag>,
    pub style: crate::Style,
    pub units: Units,
    #[serde(default)]
    pub length_precision: Option<u8>,
    #[serde(default)]
    pub measurement: crate::MeasurementSettings,
    /// The camera at the time of saving, so a file opens where it was left.
    pub camera: Option<OrbitCamera>,
    pub scenes: Vec<crate::scenes::Scene>,
    /// The next free id; every element kind in every mesh draws from it.
    pub(crate) next_id: u64,
}

impl Default for Document {
    fn default() -> Self {
        Document {
            root: Definition { name: "Model".into(), mesh: Mesh::new(), ..Default::default() },
            definitions: BTreeMap::new(),
            materials: BTreeMap::new(),
            images: BTreeMap::new(),
            tags: BTreeMap::new(),
            style: crate::Style::default(),
            units: Units::default(),
            length_precision: None,
            measurement: Default::default(),
            camera: None,
            scenes: Vec::new(),
            next_id: 1,
        }
    }
}

/// One place in the instance tree where a definition's geometry is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Occurrence {
    /// `None` for the root model.
    pub definition: Option<DefinitionId>,
    pub path: Vec<InstanceId>,
    /// Definition coordinates to world coordinates.
    pub transform: DMat4,
    /// Inside the context being edited (drawn normally); otherwise faded.
    pub in_context: bool,
    /// The child of the active context this occurrence belongs to, if any: what a
    /// click on it selects.
    pub top: Option<InstanceId>,
    /// The material inherited from enclosing instances, for faces without one.
    pub material: Option<MaterialId>,
}

impl Document {
    pub fn format_length(&self, metres: f64) -> String {
        let mut text = crate::format_length_with_precision(metres, self.units, self.length_precision);
        if self.units == Units::Architectural && self.measurement.force_zero_inches && text.ends_with('\'') {
            text.push_str(" 0\"");
        }
        if self.measurement.show_unit_markers || self.units.is_fractional() {
            text
        } else {
            text.trim_end_matches(self.units.suffix()).trim_end().into()
        }
    }

    pub fn format_area(&self, square_metres: f64) -> String {
        let units = self.measurement.area_units.unwrap_or(match self.units {
            Units::Millimetres | Units::Centimetres | Units::Metres => Units::Metres,
            _ => Units::Feet,
        });
        format!(
            "{:.*} {}²",
            self.measurement.area_precision.min(6) as usize,
            square_metres * units.per_metre().powi(2),
            units.suffix()
        )
    }

    pub fn format_volume(&self, cubic_metres: f64) -> String {
        let units = self.measurement.volume_units.unwrap_or(match self.units {
            Units::Millimetres | Units::Centimetres | Units::Metres => Units::Metres,
            _ => Units::Feet,
        });
        format!(
            "{:.*} {}³",
            self.measurement.volume_precision.min(6) as usize,
            cubic_metres * units.per_metre().powi(3),
            units.suffix()
        )
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn material_by_name(&self, name: &str) -> Option<MaterialId> {
        self.materials.iter().find(|(_, m)| m.name == name).map(|(id, _)| *id)
    }

    pub fn tag_by_name(&self, name: &str) -> Option<TagId> {
        self.tags.iter().find(|(_, t)| t.name == name).map(|(id, _)| *id)
    }

    /// The model's own mesh (the root definition's).
    pub fn mesh(&self) -> &Mesh {
        &self.root.mesh
    }

    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    pub fn reserve_id(&mut self) -> u64 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    /// Raises the id counter above every id in use (after loading).
    pub fn fix_next_id(&mut self) {
        let mut top = self.next_id.saturating_sub(1);
        let mut see = |x: u64| top = top.max(x);
        for d in std::iter::once(&self.root).chain(self.definitions.values()) {
            see(d.mesh.next_id().saturating_sub(1));
            d.sections.planes.keys().for_each(|id| see(*id));
            d.instances.keys().for_each(|k| see(k.0));
            d.guides.keys().for_each(|k| see(k.0));
            d.dimensions.keys().for_each(|k| see(k.0));
            d.texts.keys().for_each(|k| see(k.0));
        }
        self.definitions.keys().for_each(|k| see(k.0));
        self.materials.keys().for_each(|k| see(k.0));
        self.images.keys().for_each(|k| see(k.0));
        self.tags.keys().for_each(|k| see(k.0));
        for scene in &self.scenes {
            see(scene.id);
            scene.tags.keys().for_each(|id| see(id.0));
            for context in &scene.contexts {
                if let Some(id) = context.definition {
                    see(id.0);
                }
                if let Some(id) = context.section {
                    see(id);
                }
                context.hidden_edges.iter().for_each(|id| see(id.0));
                context.hidden_faces.iter().for_each(|id| see(id.0));
                context.hidden_instances.iter().for_each(|id| see(id.0));
            }
        }
        self.next_id = top + 1;
    }

    pub fn def(&self, id: Option<DefinitionId>) -> Option<&Definition> {
        match id {
            None => Some(&self.root),
            Some(d) => self.definitions.get(&d),
        }
    }

    pub fn def_mut(&mut self, id: Option<DefinitionId>) -> Option<&mut Definition> {
        match id {
            None => Some(&mut self.root),
            Some(d) => self.definitions.get_mut(&d),
        }
    }

    /// Runs `f` on a definition's mesh with the document's id counter, so ids stay
    /// unique across meshes.
    pub fn with_mesh<R>(&mut self, def: Option<DefinitionId>, f: impl FnOnce(&mut Mesh) -> R) -> Option<R> {
        let next = self.next_id;
        let d = self.def_mut(def)?;
        d.mesh.ensure_next_id(next);
        let r = f(&mut d.mesh);
        let after = d.mesh.next_id();
        self.next_id = self.next_id.max(after);
        Some(r)
    }

    /// The definition a context path ends in: `None` for the model itself. `Err` if the
    /// path does not lead anywhere.
    pub fn resolve(&self, path: &[InstanceId]) -> Result<Option<DefinitionId>, ()> {
        let mut def: Option<DefinitionId> = None;
        for i in path {
            let inst = self.def(def).ok_or(())?.instances.get(i).ok_or(())?;
            def = Some(inst.definition);
        }
        Ok(def)
    }

    /// Definition coordinates of the end of `path` to world coordinates.
    pub fn path_transform(&self, path: &[InstanceId]) -> DMat4 {
        let mut def: Option<DefinitionId> = None;
        let mut t = DMat4::IDENTITY;
        for i in path {
            let Some(inst) = self.def(def).and_then(|d| d.instances.get(i)) else { break };
            t *= inst.transform;
            def = Some(inst.definition);
        }
        t
    }

    pub fn tag_visible(&self, tag: Option<TagId>) -> bool {
        tag.and_then(|t| self.tags.get(&t)).is_none_or(|t| t.visible)
    }

    /// How many instances of a definition there are, anywhere.
    pub fn instance_count(&self, def: DefinitionId) -> usize {
        std::iter::once(&self.root)
            .chain(self.definitions.values())
            .map(|d| d.instances.values().filter(|i| i.definition == def).count())
            .sum()
    }

    /// Whether `def` contains (directly or deeper) an instance of `target`, or is it.
    pub fn contains_definition(&self, def: DefinitionId, target: DefinitionId) -> bool {
        let mut stack = vec![def];
        let mut seen = BTreeSet::new();
        while let Some(d) = stack.pop() {
            if d == target {
                return true;
            }
            if !seen.insert(d) {
                continue;
            }
            if let Some(defn) = self.definitions.get(&d) {
                stack.extend(defn.instances.values().map(|i| i.definition));
            }
        }
        false
    }

    /// Every place geometry is drawn, depth first, skipping hidden tags. `context` is
    /// the path being edited.
    pub fn occurrences(&self, context: &[InstanceId]) -> Vec<Occurrence> {
        let mut out = Vec::new();
        self.walk(None, DMat4::IDENTITY, &[], context, None, None, &mut out, 0);
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        &self,
        def: Option<DefinitionId>,
        t: DMat4,
        path: &[InstanceId],
        context: &[InstanceId],
        top: Option<InstanceId>,
        material: Option<MaterialId>,
        out: &mut Vec<Occurrence>,
        depth: usize,
    ) {
        let Some(d) = self.def(def) else { return };
        if depth > 64 {
            return;
        }
        let in_context = path.len() >= context.len() && path[..context.len()] == *context;
        out.push(Occurrence { definition: def, path: path.to_vec(), transform: t, in_context, top, material });
        for (id, inst) in &d.instances {
            if inst.hidden || !self.tag_visible(inst.tag) {
                continue;
            }
            let mut p = path.to_vec();
            p.push(*id);
            // The instance a click selects: the child of the active context on this path.
            let top = if p.len() == context.len() + 1 && p[..context.len()] == *context { Some(*id) } else { top };
            let material = inst.material.or(material);
            self.walk(Some(inst.definition), t * inst.transform, &p, context, top, material, out, depth + 1);
        }
    }

    /// The bounding box of a definition's geometry, in its own coordinates, including
    /// nested instances.
    pub fn definition_bounds(&self, def: Option<DefinitionId>) -> Option<(DVec3, DVec3)> {
        let d = self.def(def)?;
        let mut vertices = std::collections::BTreeSet::new();
        for (_, face) in d.mesh.faces().filter(|(_, f)| self.style.show_hidden_geometry || !f.hidden) {
            vertices.extend(face.loops().flatten().copied());
        }
        for (_, edge) in d.mesh.edges().filter(|(_, e)| self.style.show_hidden_geometry || !e.hidden) {
            vertices.extend(edge.v);
        }
        let mut acc: Option<(DVec3, DVec3)> = None;
        for vertex in vertices {
            let point = d.mesh.pos(vertex);
            acc = Some(acc.map_or((point, point), |(lo, hi)| (lo.min(point), hi.max(point))));
        }
        for dimension in d.dimensions.values() {
            if let Some(layout) = dimension.layout(self, def, DMat4::IDENTITY, 0.) {
                for point in layout.segments.iter().flatten().copied().chain(std::iter::once(layout.text_position)) {
                    acc = Some(match acc {
                        Some((lo, hi)) => (lo.min(point), hi.max(point)),
                        None => (point, point),
                    });
                }
            }
        }
        for text in d.texts.values() {
            if let crate::text::TextPlacement::Attached { anchor, leader } = &text.placement {
                let point = anchor.resolve(self, def).0;
                let end = if let crate::text::TextLeader::Pushpin { offset }
                | crate::text::TextLeader::None { offset } = leader
                {
                    point + *offset
                } else {
                    point
                };
                for point in [point, end] {
                    acc = Some(match acc {
                        Some((lo, hi)) => (lo.min(point), hi.max(point)),
                        None => (point, point),
                    });
                }
            }
        }
        for inst in d.instances.values().filter(|instance| !instance.hidden) {
            if let Some((lo, hi)) = self.definition_bounds(Some(inst.definition)) {
                for corner in corners(lo, hi) {
                    let p = inst.transform.transform_point3(corner);
                    acc = Some(match acc {
                        Some((a, b)) => (a.min(p), b.max(p)),
                        None => (p, p),
                    });
                }
            }
        }
        acc
    }
}

/// The eight corners of a box.
pub fn corners(lo: DVec3, hi: DVec3) -> [DVec3; 8] {
    [
        DVec3::new(lo.x, lo.y, lo.z),
        DVec3::new(hi.x, lo.y, lo.z),
        DVec3::new(hi.x, hi.y, lo.z),
        DVec3::new(lo.x, hi.y, lo.z),
        DVec3::new(lo.x, lo.y, hi.z),
        DVec3::new(hi.x, lo.y, hi.z),
        DVec3::new(hi.x, hi.y, hi.z),
        DVec3::new(lo.x, hi.y, hi.z),
    ]
}

#[cfg(test)]
mod hidden_geometry_tests {
    use crate::{Command, Editor, Element};
    use rysmo_math::{DVec3, OrbitCamera, Ray};
    #[test]
    fn hidden_faces_survive_splits_grouping_explode_scenes_and_history() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y * 2., facing: None })
            .unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        let mut elements: Vec<_> = e.mesh().face_edges(face).into_iter().map(Element::Edge).collect();
        elements.push(Element::Face(face));
        e.apply(Command::SetGeometryHidden { elements: elements.clone(), hidden: true }).unwrap();
        assert!(e.doc().definition_bounds(None).is_none());
        assert!(e.mesh().pick_face(&Ray::new(DVec3::new(1., 1., 2.), -DVec3::Z)).is_none());
        assert!(e.mesh().pick_face_with_hidden(&Ray::new(DVec3::new(1., 1., 2.), -DVec3::Z), true).is_some());
        let scene = crate::scenes::Scene::capture(e.doc(), OrbitCamera::default(), "Hidden".into());
        e.apply(Command::SetGeometryHidden { elements: elements.clone(), hidden: false }).unwrap();
        let mut document = e.doc().clone();
        scene.restore(&mut document);
        e = Editor::with_document(document);
        assert!(e.mesh().face(face).unwrap().hidden);
        let made = e.apply(Command::MakeGroup { elements }).unwrap();
        let Element::Instance(instance) = made.created[0] else { panic!() };
        let definition = e.doc().root.instances[&instance].definition;
        assert!(e.doc().definitions[&definition].mesh.faces().all(|(_, f)| f.hidden));
        e.apply(Command::Explode { instance }).unwrap();
        assert!(e.mesh().faces().all(|(_, f)| f.hidden));
        e.apply(Command::DrawLines { lines: vec![[DVec3::new(1., 0., 0.), DVec3::new(1., 2., 0.)]], facing: None })
            .unwrap();
        assert_eq!(e.mesh().face_count(), 2);
        assert!(e.mesh().faces().all(|(_, f)| f.hidden));
        let before = e.doc().clone();
        let remaining_face = e.mesh().faces().next().unwrap().0;
        assert!(
            e.apply(Command::SetGeometryHidden {
                elements: vec![Element::Face(remaining_face), Element::Face(crate::FaceId(999999))],
                hidden: false
            })
            .is_err()
        );
        assert_eq!(e.doc(), &before);
        e.undo();
        assert_eq!(e.mesh().face_count(), 1);
    }
}
