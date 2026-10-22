//! Non-destructive section planes and clipping in occurrence world coordinates.
use crate::{Document, InstanceId};
use rysmo_math::{DMat4, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SectionPlane {
    pub name: String,
    pub origin: DVec3,
    pub normal: DVec3,
}
impl SectionPlane {
    pub fn valid(&self) -> bool {
        self.origin.is_finite() && self.normal.is_finite() && (self.normal.length() - 1.).abs() < 1e-8
    }
    pub fn distance(&self, p: DVec3) -> f64 {
        (p - self.origin).dot(self.normal)
    }
    pub fn transformed(&self, transform: DMat4) -> Self {
        Self {
            name: self.name.clone(),
            origin: transform.transform_point3(self.origin),
            normal: transform.inverse().transpose().transform_vector3(self.normal).normalize(),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sections {
    pub planes: BTreeMap<u64, SectionPlane>,
    pub active: Option<u64>,
}
impl Sections {
    pub fn valid(&self) -> bool {
        self.planes.iter().all(|(id, plane)| *id > 0 && plane.valid())
            && self.active.is_none_or(|id| self.planes.contains_key(&id))
    }
}
/// Finite symbol for an infinite cut plane, shared by rendering, picking and tools.
pub fn glyph(plane: &SectionPlane, bounds: Option<(DVec3, DVec3)>) -> Vec<[DVec3; 2]> {
    let radius = bounds.map(|(lo, hi)| (hi - lo).length().max(1.) * 0.6).unwrap_or(1.);
    let (u, v) = rysmo_math::Plane::new(plane.normal, plane.origin).basis();
    let corners = [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)].map(|(a, b)| plane.origin + (u * a + v * b) * radius);
    let mut lines = (0..4).map(|i| [corners[i], corners[(i + 1) % 4]]).collect::<Vec<_>>();
    lines.push([plane.origin, plane.origin + plane.normal * radius * 0.25]);
    lines
}
impl Document {
    pub fn section_planes(&self, path: &[InstanceId]) -> Vec<SectionPlane> {
        if !self.style.section_cuts {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut definition = &self.root;
        let mut transform = DMat4::IDENTITY;
        for step in 0..=path.len() {
            if let Some(plane) = definition.sections.active.and_then(|id| definition.sections.planes.get(&id)) {
                result.push(plane.transformed(transform));
            }
            if step == path.len() {
                break;
            }
            let Some(instance) = definition.instances.get(&path[step]) else { break };
            transform *= instance.transform;
            let Some(next) = self.def(Some(instance.definition)) else { break };
            definition = next;
        }
        result
    }
}
/// Keep the half-space opposite the plane normal, including the boundary.
pub fn clip_segment(mut line: [DVec3; 2], planes: &[SectionPlane]) -> Option<[DVec3; 2]> {
    for plane in planes {
        let [a, b] = line.map(|p| plane.distance(p));
        if a > 1e-9 && b > 1e-9 {
            return None;
        }
        if (a > 1e-9) != (b > 1e-9) {
            let intersection = line[0].lerp(line[1], a / (a - b));
            line[if a > 1e-9 { 0 } else { 1 }] = intersection;
        }
    }
    Some(line)
}
/// Segment where a triangle crosses a plane; coplanar triangles are not cuts.
pub fn triangle_slice(triangle: [DVec3; 3], plane: &SectionPlane) -> Option<[DVec3; 2]> {
    let distances = triangle.map(|p| plane.distance(p));
    if distances.iter().all(|d| d.abs() <= 1e-9) {
        return None;
    }
    let mut points: Vec<DVec3> = Vec::new();
    for i in 0..3 {
        let j = (i + 1) % 3;
        let mut push = |point: DVec3| {
            if points.iter().all(|p| p.distance(point) > 1e-9) {
                points.push(point);
            }
        };
        if distances[i].abs() <= 1e-9 {
            push(triangle[i]);
        }
        if distances[i] * distances[j] < 0. {
            push(triangle[i].lerp(triangle[j], distances[i] / (distances[i] - distances[j])));
        }
    }
    (points.len() == 2).then(|| [points[0], points[1]])
}
pub fn slice(
    doc: &Document,
    owner: Option<crate::DefinitionId>,
    plane: &SectionPlane,
) -> Result<Vec<[DVec3; 2]>, String> {
    fn collect(
        doc: &Document,
        owner: Option<crate::DefinitionId>,
        transform: DMat4,
        plane: &SectionPlane,
        depth: usize,
        lines: &mut Vec<[DVec3; 2]>,
    ) -> Result<(), String> {
        if depth > 128 {
            return Err("Section source nesting is too deep or cyclic.".into());
        }
        let definition = doc.def(owner).ok_or("Section source definition is missing.")?;
        let mesh = doc.effective_mesh(owner);
        for (id, _) in mesh.faces().filter(|(_, face)| !face.hidden) {
            for triangle in rysmo_mesh::triangulate_face(&mesh, id) {
                if let Some(line) = triangle_slice(triangle.map(|v| transform.transform_point3(mesh.pos(v))), plane) {
                    lines.push(line);
                }
            }
        }
        for instance in definition.instances.values() {
            if !instance.hidden && doc.tag_visible(instance.tag) {
                collect(doc, Some(instance.definition), transform * instance.transform, plane, depth + 1, lines)?;
            }
        }
        Ok(())
    }
    let mut lines = Vec::new();
    collect(doc, owner, DMat4::IDENTITY, plane, 0, &mut lines)?;
    Ok(lines)
}
/// Clipped polygon with barycentric weights for interpolating normals and UVs.
pub fn clip_triangle(triangle: [DVec3; 3], planes: &[SectionPlane]) -> Vec<(DVec3, [f64; 3])> {
    let mut polygon = triangle
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let mut weights = [0.; 3];
            weights[i] = 1.;
            (p, weights)
        })
        .collect::<Vec<_>>();
    for plane in planes {
        let mut output = Vec::new();
        for i in 0..polygon.len() {
            let (a, wa) = polygon[i];
            let (b, wb) = polygon[(i + 1) % polygon.len()];
            let (da, db) = (plane.distance(a), plane.distance(b));
            if da <= 1e-9 {
                output.push((a, wa));
            }
            if (da > 1e-9) != (db > 1e-9) {
                let t = da / (da - db);
                output.push((a.lerp(b, t), std::array::from_fn(|j| wa[j] + (wb[j] - wa[j]) * t)));
            }
        }
        output.dedup_by(|a, b| a.0.distance(b.0) <= 1e-12);
        if output.len() > 1 && output[0].0.distance(output.last().unwrap().0) <= 1e-12 {
            output.pop();
        }
        polygon = output;
    }
    polygon
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slice_is_an_independent_wire_group_and_undo_restores_everything() {
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::ZERO,
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        editor.apply(Command::PushPull { face, distance: -2., keep_base: false }).unwrap();
        editor
            .apply(Command::AddSection {
                plane: SectionPlane { name: "Middle".into(), origin: DVec3::ONE, normal: DVec3::X },
            })
            .unwrap();
        let id = editor.active().sections.active.unwrap();
        let before = editor.doc().clone();
        let out = editor.apply(Command::CreateSectionSlice { id }).unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        let group = &editor.doc().definitions[&editor.doc().root.instances[&instance].definition];
        assert!(group.group);
        assert_eq!(group.mesh.faces().count(), 0);
        assert!(group.mesh.edges().count() >= 4);
        assert_eq!(group.mesh.bounds(), Some((DVec3::new(1., 0., 0.), DVec3::new(1., 2., 2.))));
        assert_eq!(editor.doc().root.mesh, before.root.mesh);
        let after = editor.doc().clone();
        editor.undo();
        assert_eq!(editor.doc(), &before);
        editor.redo();
        assert_eq!(editor.doc(), &after);
    }
    #[test]
    fn unique_and_explode_preserve_planes_with_fresh_ids() {
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(2., 3., 4.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                spacing: 1.,
            })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        editor
            .apply(Command::AddSection {
                plane: SectionPlane { name: "Child".into(), origin: DVec3::ONE, normal: DVec3::Y },
            })
            .unwrap();
        let original = editor.active().sections.active.unwrap();
        editor.exit_all();
        editor.apply(Command::MakeUnique { instance }).unwrap();
        let definition = editor.doc().root.instances[&instance].definition;
        let copied = editor.doc().definitions[&definition].sections.active.unwrap();
        assert_ne!(original, copied);
        editor.apply(Command::Explode { instance }).unwrap();
        let id = editor.active().sections.active.unwrap();
        assert_ne!(id, copied);
        assert_eq!(editor.active().sections.planes[&id].origin, DVec3::new(3., 4., 5.));
    }
    #[test]
    fn section_history_and_nested_contexts_are_independent() {
        use crate::{Command, Editor, Element};
        let plane = |normal| SectionPlane { name: "Cut".into(), origin: DVec3::splat(0.5), normal };
        let mut editor = Editor::new();
        editor.apply(Command::AddSection { plane: plane(DVec3::X) }).unwrap();
        let root_id = editor.active().sections.active.unwrap();
        let before = editor.doc().clone();
        editor.apply(Command::SetSection { id: root_id, plane: Some(plane(-DVec3::X)) }).unwrap();
        editor.undo();
        assert_eq!(editor.doc(), &before);
        assert!(editor.apply(Command::ActivateSection { id: Some(u64::MAX) }).is_err());
        assert_eq!(editor.doc(), &before);
        let out = editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(2., 3., 4.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                spacing: 1.,
            })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        editor.apply(Command::AddSection { plane: plane(DVec3::Y) }).unwrap();
        let child_id = editor.active().sections.active.unwrap();
        let cuts = editor.doc().section_planes(&[instance]);
        assert_eq!(cuts.len(), 2);
        assert_eq!(cuts[1].origin, DVec3::new(2.5, 3.5, 4.5));
        assert_eq!(editor.doc().root.sections.active, Some(root_id));
        editor.apply(Command::ResizeContext { factor: 2. }).unwrap();
        assert_eq!(editor.active().sections.planes[&child_id].origin, DVec3::ONE);
        editor.undo();
        editor.apply(Command::ActivateSection { id: None }).unwrap();
        assert_eq!(editor.doc().section_planes(&[instance]).len(), 1);
        editor.undo();
        assert_eq!(editor.doc().section_planes(&[instance]).len(), 2);
    }
    #[test]
    fn clipping_preserves_attribute_weights_and_handles_multiple_cuts() {
        let plane = SectionPlane { name: String::new(), origin: DVec3::ZERO, normal: DVec3::X };
        assert!(clip_segment([DVec3::X, DVec3::X * 2.], std::slice::from_ref(&plane)).is_none());
        assert_eq!(clip_segment([-DVec3::X, DVec3::X], std::slice::from_ref(&plane)), Some([-DVec3::X, DVec3::ZERO]));
        let tri = [-DVec3::X, DVec3::X, DVec3::Y];
        let result = clip_triangle(tri, &[plane]);
        assert!(result.len() >= 3);
        for (p, w) in result {
            assert!(p.x <= 1e-9);
            assert!((w.iter().sum::<f64>() - 1.).abs() < 1e-12);
            assert!((tri[0] * w[0] + tri[1] * w[1] + tri[2] * w[2]).distance(p) < 1e-12);
        }
    }
}

#[cfg(test)]
mod entity_tests {
    use super::*;
    use crate::{Command, Editor, Element};
    fn add(e: &mut Editor) -> u64 {
        let out = e
            .apply(Command::AddSection {
                plane: SectionPlane { name: "Cut".into(), origin: DVec3::new(0.5, 0., 0.), normal: DVec3::X },
            })
            .unwrap();
        let Element::Section(id) = out.created[0] else { panic!() };
        id
    }
    #[test]
    fn section_entities_move_rotate_copy_erase_and_restore_exact_history() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y * 2., facing: None })
            .unwrap();
        let id = add(&mut e);
        e.set_selection([Element::Section(id)]);
        assert!(e.is_selected(Element::Section(id)));
        let mesh = e.mesh().clone();
        let before = e.doc().clone();
        e.apply(Command::Move { elements: vec![Element::Section(id)], offset: DVec3::X * 0.25 }).unwrap();
        assert_eq!(e.active().sections.planes[&id].origin, DVec3::new(0.75, 0., 0.));
        assert_eq!(e.mesh(), &mesh);
        e.undo();
        assert_eq!(e.doc(), &before);
        e.redo();
        e.apply(Command::Rotate {
            elements: vec![Element::Section(id)],
            centre: DVec3::ZERO,
            axis: DVec3::Y,
            angle: 90.,
            copies: 0,
        })
        .unwrap();
        assert!(e.active().sections.planes[&id].normal.distance(-DVec3::Z) < 1e-9);
        let out =
            e.apply(Command::Copy { elements: vec![Element::Section(id)], offset: DVec3::Z * 2., copies: 1 }).unwrap();
        let Element::Section(copy) = out.created[0] else { panic!() };
        assert_ne!(copy, id);
        assert_eq!(e.active().sections.active, Some(id));
        assert!(
            e.active().sections.planes[&copy].origin.distance(e.active().sections.planes[&id].origin + DVec3::Z * 2.)
                < 1e-9
        );
        let saved = e.doc().clone();
        e.apply(Command::Erase { elements: vec![Element::Section(id)] }).unwrap();
        assert_eq!(e.active().sections.active, None);
        e.undo();
        assert_eq!(e.doc(), &saved);
        assert_eq!(e.mesh(), &mesh);
    }
    #[test]
    fn grouping_a_selected_plane_preserves_world_cut_and_explode_restores_it() {
        let mut e = Editor::new();
        let mut elements = e
            .apply(Command::DrawRectangle { corner: DVec3::X * 4., u: DVec3::X * 2., v: DVec3::Y * 2., facing: None })
            .unwrap()
            .created;
        let id = add(&mut e);
        let world = e.active().sections.planes[&id].clone();
        elements.push(Element::Section(id));
        let before = e.doc().clone();
        let out = e.apply(Command::MakeGroup { elements }).unwrap();
        let Element::Instance(group) = out.created[0] else { panic!() };
        assert!(e.doc().root.sections.planes.is_empty());
        let planes = e.doc().section_planes(&[group]);
        assert_eq!(planes.len(), 1);
        assert_eq!(planes[0], world);
        e.apply(Command::Explode { instance: group }).unwrap();
        assert_eq!(e.doc().root.sections.planes.len(), 1);
        let restored = e.doc().root.sections.active.unwrap();
        assert_eq!(e.doc().root.sections.planes[&restored], world);
        e.undo();
        e.undo();
        assert_eq!(e.doc(), &before);
    }
    #[test]
    fn invalid_section_transform_is_atomic() {
        let mut e = Editor::new();
        let id = add(&mut e);
        let before = e.doc().clone();
        assert!(e.apply(Command::Move { elements: vec![Element::Section(999)], offset: DVec3::X }).is_err());
        assert_eq!(e.doc(), &before);
        assert!(
            e.apply(Command::Rotate {
                elements: vec![Element::Section(id)],
                centre: DVec3::ZERO,
                axis: DVec3::ZERO,
                angle: 90.,
                copies: 0
            })
            .is_err()
        );
        assert_eq!(e.doc(), &before);
    }
}
