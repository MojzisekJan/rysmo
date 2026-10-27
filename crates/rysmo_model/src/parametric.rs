//! Regenerable architectural components. Dimensions are local metres, with Z up.
use crate::{Definition, Document, Instance};
use rysmo_math::{DMat4, DVec3};
use rysmo_mesh::{DefinitionId, Edge, EdgeId, Face, FaceId, InstanceId, Mesh, Vertex, VertexId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Spec {
    Window { width: f64, height: f64, depth: f64, frame: f64, columns: u32 },
    Door { width: f64, height: f64, depth: f64, frame: f64, opening: f64 },
    Stairs { width: f64, rise: f64, tread: f64, steps: u32 },
}
impl Default for Spec {
    fn default() -> Self {
        Self::Window { width: 1.2, height: 1.4, depth: 0.12, frame: 0.06, columns: 2 }
    }
}
impl Spec {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Window { .. } => "Window",
            Self::Door { .. } => "Door",
            Self::Stairs { .. } => "Stairs",
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let positive = |v: f64| v.is_finite() && (0.001..=1000.).contains(&v);
        let valid = match *self {
            Self::Window { width, height, depth, frame, columns } => {
                [width, height, depth, frame].into_iter().all(positive)
                    && (1..=16).contains(&columns)
                    && width > (columns + 1) as f64 * frame + 0.002
                    && height > 2. * frame + 0.002
            }
            Self::Door { width, height, depth, frame, opening } => {
                [width, height, depth, frame].into_iter().all(positive)
                    && width > 2. * frame + 0.004
                    && height > frame + 0.004
                    && opening.is_finite()
                    && (0. ..=170.).contains(&opening)
            }
            Self::Stairs { width, rise, tread, steps } => {
                [width, rise, tread].into_iter().all(positive)
                    && (1..=128).contains(&steps)
                    && rise * steps as f64 <= 1000.
                    && tread * steps as f64 <= 1000.
            }
        };
        if valid { Ok(()) } else { Err("Invalid component dimensions, frame thickness, opening or count.".into()) }
    }
    /// Rescale dimensional parameters when loading a document in other source units.
    pub fn scaled(&self, scale: f64) -> Self {
        match *self {
            Self::Window { width, height, depth, frame, columns } => Self::Window {
                width: width * scale,
                height: height * scale,
                depth: depth * scale,
                frame: frame * scale,
                columns,
            },
            Self::Door { width, height, depth, frame, opening } => Self::Door {
                width: width * scale,
                height: height * scale,
                depth: depth * scale,
                frame: frame * scale,
                opening,
            },
            Self::Stairs { width, rise, tread, steps } => {
                Self::Stairs { width: width * scale, rise: rise * scale, tread: tread * scale, steps }
            }
        }
    }
    pub fn mesh(&self) -> Result<Mesh, String> {
        self.validate()?;
        let mut parts = Vec::new();
        let point = |x: f64, z: f64| DVec3::new(x, 0., z);
        match *self {
            Self::Window { width: w, height: h, depth: d, frame: f, columns } => {
                parts.push(extrude(
                    &[point(0., 0.), point(w, 0.), point(w, h), point(0., h)],
                    &[vec![point(f, f), point(f, h - f), point(w - f, h - f), point(w - f, f)]],
                    DVec3::NEG_Y,
                    d,
                )?);
                let pane = (w - (columns + 1) as f64 * f) / columns as f64;
                for i in 1..columns {
                    let x = f + i as f64 * pane + (i - 1) as f64 * f;
                    // Small clearances avoid coincident shells while retaining the frame outline.
                    parts.push(box_mesh(DVec3::new(x, -d, f + 0.0002), DVec3::new(f, d, h - 2. * f - 0.0004))?);
                }
                // Panes are thin geometry; assign glass through ordinary material editing.
                for i in 0..columns {
                    let x = f + i as f64 * (pane + f);
                    parts.push(box_mesh(
                        DVec3::new(x + 0.0002, -d * 0.5, f + 0.0002),
                        DVec3::new(pane - 0.0004, 0.004, h - 2. * f - 0.0004),
                    )?);
                }
            }
            Self::Door { width: w, height: h, depth: d, frame: f, opening } => {
                let outline = [
                    point(0., 0.),
                    point(f, 0.),
                    point(f, h - f),
                    point(w - f, h - f),
                    point(w - f, 0.),
                    point(w, 0.),
                    point(w, h),
                    point(0., h),
                ];
                parts.push(extrude(&outline, &[], DVec3::NEG_Y, d)?);
                let mut panel = box_mesh(
                    DVec3::new(f + 0.001, -d * 0.55, 0.002),
                    DVec3::new(w - 2. * f - 0.002, d * 0.45, h - f - 0.004),
                )?;
                let pivot = DVec3::new(f + 0.001, 0., 0.);
                let t = DMat4::from_translation(pivot)
                    * DMat4::from_rotation_z(opening.to_radians())
                    * DMat4::from_translation(-pivot);
                panel.map_all(|p| t.transform_point3(p));
                parts.push(panel);
            }
            Self::Stairs { width, rise, tread, steps } => {
                let mut profile = vec![
                    DVec3::ZERO,
                    DVec3::new(0., tread * steps as f64, 0.),
                    DVec3::new(0., tread * steps as f64, rise * steps as f64),
                ];
                for i in (0..steps).rev() {
                    profile.push(DVec3::new(0., tread * i as f64, rise * (i + 1) as f64));
                    if i > 0 {
                        profile.push(DVec3::new(0., tread * i as f64, rise * i as f64));
                    }
                }
                parts.push(extrude(&profile, &[], DVec3::X, width)?);
            }
        }
        let mut next = 0;
        let mut vertices = BTreeMap::new();
        let mut edges = BTreeMap::new();
        let mut faces = BTreeMap::new();
        for part in parts {
            let part = part
                .with_fresh_ids(|| {
                    next += 1;
                    next
                })
                .map_err(|e| e.to_string())?;
            vertices.extend(part.vertices().map(|(id, v)| (id, v.clone())));
            edges.extend(part.edges().map(|(id, e)| (id, e.clone())));
            faces.extend(part.faces().map(|(id, f)| (id, f.clone())));
        }
        Mesh::from_parts(vertices, edges, faces).map_err(|e| e.to_string())
    }
    /// Compare generated topology and positions while allowing face paint and edge display changes.
    pub fn matches_geometry(&self, mesh: &Mesh) -> Result<bool, String> {
        let expected = self.mesh()?;
        if expected.vertex_count() != mesh.vertex_count()
            || expected.edge_count() != mesh.edge_count()
            || expected.face_count() != mesh.face_count()
        {
            return Ok(false);
        }
        let shape = |mesh: &Mesh| {
            let point = |v: VertexId| mesh.pos(v).to_array().map(|n| (n / (rysmo_math::EPS * 0.1)).round() as i64);
            let mut positions: Vec<_> = mesh.vertices().map(|(id, _)| point(id)).collect();
            positions.sort();
            let mut edges: Vec<_> = mesh
                .edges()
                .map(|(_, e)| {
                    let mut points = e.v.map(point);
                    points.sort();
                    points
                })
                .collect();
            edges.sort();
            let mut faces: Vec<_> = mesh
                .faces()
                .map(|(_, f)| {
                    f.loops()
                        .map(|ring| {
                            let mut points: Vec<_> = ring.iter().map(|id| point(*id)).collect();
                            if let Some((index, _)) = points.iter().enumerate().min_by_key(|(_, p)| **p) {
                                points.rotate_left(index);
                            }
                            points
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            faces.sort();
            (positions, edges, faces)
        };
        Ok(shape(&expected) == shape(mesh))
    }

    pub fn document(&self) -> Result<Document, String> {
        let mut document = Document::new();
        let mesh = self.mesh()?.with_fresh_ids(|| document.reserve_id()).map_err(|e| e.to_string())?;
        let definition = DefinitionId(document.reserve_id());
        document.definitions.insert(
            definition,
            Definition { name: self.name().into(), mesh, parametric: Some(self.clone()), ..Default::default() }.into(),
        );
        let id = InstanceId(document.reserve_id());
        document.root_mut().instances.insert(
            id,
            Instance {
                definition,
                transform: DMat4::IDENTITY,
                name: self.name().into(),
                tag: None,
                material: None,
                hidden: false,
                glued_to: None,
                attributes: Default::default(),
            },
        );
        Ok(document)
    }
}
fn box_mesh(origin: DVec3, size: DVec3) -> Result<Mesh, String> {
    let p = [
        origin,
        origin + DVec3::X * size.x,
        origin + DVec3::X * size.x + DVec3::Y * size.y,
        origin + DVec3::Y * size.y,
    ];
    extrude(&p, &[], DVec3::Z, size.z)
}
fn extrude(outer: &[DVec3], holes: &[Vec<DVec3>], normal: DVec3, depth: f64) -> Result<Mesh, String> {
    let mut next = 0u64;
    let mut vertices = BTreeMap::new();
    let mut edges = BTreeMap::new();
    let mut boundary = |points: &[DVec3]| {
        let ids: Vec<_> = points
            .iter()
            .map(|p| {
                next += 1;
                let id = VertexId(next);
                vertices.insert(id, Vertex { pos: *p });
                id
            })
            .collect();
        for i in 0..ids.len() {
            next += 1;
            edges.insert(EdgeId(next), Edge::new(ids[i], ids[(i + 1) % ids.len()]));
        }
        ids
    };
    let mut face = Face {
        outer: boundary(outer),
        holes: holes.iter().map(|p| boundary(p)).collect(),
        material: None,
        back_material: None,
        uv: None,
        hidden: false,
    };
    let plane = rysmo_math::Plane::from_polygon(outer).ok_or("Invalid profile")?;
    if plane.normal.dot(normal) < 0. {
        face.outer.reverse();
        for h in &mut face.holes {
            h.reverse();
        }
    }
    next += 1;
    let fid = FaceId(next);
    let mut mesh = Mesh::from_parts(vertices, edges, BTreeMap::from([(fid, face)])).map_err(|e| e.to_string())?;
    mesh.push_pull(fid, depth, true).map_err(|e| e.to_string())?;
    Ok(mesh)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_have_expected_bounds_and_closed_stairs_volume() {
        for spec in [
            Spec::default(),
            Spec::Door { width: 0.9, height: 2.1, depth: 0.12, frame: 0.06, opening: 90. },
            Spec::Stairs { width: 1., rise: 0.18, tread: 0.28, steps: 12 },
        ] {
            let doc = spec.document().unwrap();
            assert!(doc.definition_bounds(None).is_some());
            let instance = doc.root.instances.values().next().unwrap();
            if matches!(spec, Spec::Stairs { .. }) {
                let volume = doc.instance_volume(instance, DMat4::IDENTITY).unwrap();
                assert!((volume - 0.18 * 0.28 * 78.).abs() < 1e-8);
            }
        }
    }
    #[test]
    fn invalid_parameters_reject_before_generating() {
        assert!(Spec::Window { width: 1., height: 1., depth: 0.1, frame: 0.2, columns: 16 }.mesh().is_err());
        assert!(Spec::Stairs { width: 1., rise: f64::NAN, tread: 0.2, steps: 10 }.mesh().is_err());
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;
    use crate::{Command, Editor, clipboard::Clipboard};
    #[test]
    fn regeneration_updates_shared_instances_and_undo_restores_parameters_and_geometry() {
        let spec = Spec::Stairs { width: 1., rise: 0.18, tread: 0.28, steps: 8 };
        let mut editor = Editor::new();
        let placed = editor
            .apply(Command::Paste {
                clipboard: Box::new(Clipboard {
                    document: spec.document().unwrap(),
                    source_transform: DMat4::IDENTITY,
                }),
                transform: DMat4::IDENTITY,
                reuse_definitions: false,
            })
            .unwrap();
        let crate::Element::Instance(id) = placed.created[0] else { panic!() };
        let definition = editor.doc().root.instances[&id].definition;
        editor
            .apply(Command::Copy { elements: vec![crate::Element::Instance(id)], offset: DVec3::X * 3., copies: 1 })
            .unwrap();
        assert_eq!(editor.doc().instance_count(definition), 2);
        let before = editor.doc().clone();
        let depth = editor.undo_depth();
        editor
            .apply(Command::SetParametric {
                definition,
                spec: Spec::Stairs { width: 1.5, rise: 0.2, tread: 0.3, steps: 10 },
            })
            .unwrap();
        assert_eq!(editor.doc().instance_count(definition), 2);
        assert_eq!(editor.undo_depth(), depth + 1);
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
        assert!(editor.redo());
        let after = editor.doc().clone();
        assert!(
            editor
                .apply(Command::SetParametric {
                    definition,
                    spec: Spec::Stairs { width: -1., rise: 0.2, tread: 0.3, steps: 10 }
                })
                .is_err()
        );
        assert_eq!(editor.doc(), &after);
    }
}
