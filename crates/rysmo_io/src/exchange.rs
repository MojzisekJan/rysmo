//! Shared evaluated, visible world geometry for DAE and DXF exchange.
use rysmo_math::DVec3;
use rysmo_mesh::{MaterialId, triangulate_face};
use rysmo_model::Document;
use std::collections::BTreeSet;

pub(crate) struct Triangle {
    pub points: [DVec3; 3],
    pub normal: DVec3,
    pub uv: [[f64; 2]; 3],
    pub material: Option<MaterialId>,
    pub boundary: [bool; 3],
}
pub(crate) struct Geometry {
    pub name: String,
    pub triangles: Vec<Triangle>,
    pub lines: Vec<[DVec3; 2]>,
}

pub(crate) fn collect(doc: &Document) -> Vec<Geometry> {
    let mut result = Vec::new();
    for occurrence in doc.occurrences(&[]) {
        let Some(definition) = doc.def(occurrence.definition) else { continue };
        let mesh = doc.effective_mesh(occurrence.definition);
        let transform = occurrence.transform;
        let normal_transform = transform.inverse().transpose();
        let mirrored = transform.determinant() < 0.;
        let mut geometry = Geometry {
            name: if occurrence.definition.is_none() { "Model".into() } else { definition.name.clone() },
            triangles: Vec::new(),
            lines: Vec::new(),
        };
        let mut face_edges = BTreeSet::new();
        let pair = |a, b| if a < b { (a, b) } else { (b, a) };
        for (id, face) in mesh.faces().filter(|(_, face)| !face.hidden) {
            let Some(normal) = mesh.face_normal(id) else { continue };
            let boundaries: BTreeSet<_> = face.loop_edges().into_iter().map(|(a, b)| pair(a, b)).collect();
            face_edges.extend(boundaries.iter().copied());
            let material = face.material.or(occurrence.material).filter(|id| doc.materials.contains_key(id));
            let texture =
                material.and_then(|id| doc.materials[&id].texture).filter(|t| doc.images.contains_key(&t.image));
            let placement = face.uv.unwrap_or_default();
            for mut triangle in triangulate_face(&mesh, id) {
                if mirrored {
                    triangle.swap(1, 2);
                }
                geometry.triangles.push(Triangle {
                    points: triangle.map(|v| transform.transform_point3(mesh.pos(v))),
                    normal: normal_transform.transform_vector3(normal).normalize(),
                    uv: triangle.map(|v| {
                        texture.map(|t| rysmo_model::uv_at(mesh.pos(v), normal, t.size, &placement)).unwrap_or([0., 0.])
                    }),
                    material,
                    boundary: std::array::from_fn(|i| boundaries.contains(&pair(triangle[i], triangle[(i + 1) % 3]))),
                });
            }
        }
        for (_, edge) in mesh.edges().filter(|(_, e)| e.visible() && !e.procedural) {
            if !face_edges.contains(&pair(edge.v[0], edge.v[1])) {
                geometry.lines.push(edge.v.map(|id| transform.transform_point3(mesh.pos(id))));
            }
        }
        if !geometry.triangles.is_empty() || !geometry.lines.is_empty() {
            result.push(geometry);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::{DMat4, DQuat};
    use rysmo_mesh::{DefinitionId, InstanceId};
    use rysmo_model::{Command, Editor, Instance};

    #[test]
    fn holes_stay_open_and_internal_diagonals_are_not_boundaries() {
        let mut editor = Editor::new();
        for (corner, size) in [(DVec3::ZERO, 4.), (DVec3::new(1., 1., 0.), 1.)] {
            editor
                .apply(Command::DrawRectangle { corner, u: DVec3::X * size, v: DVec3::Y * size, facing: None })
                .unwrap();
        }
        let mut doc = editor.doc().clone();
        let inner = doc.root.mesh.faces().find(|(id, _)| doc.root.mesh.face_area(*id) < 2.).unwrap().0;
        doc.root.mesh.remove_face(inner);
        let geometry = collect(&doc);
        let triangles = &geometry[0].triangles;
        let area: f64 =
            triangles.iter().map(|t| (t.points[1] - t.points[0]).cross(t.points[2] - t.points[0]).length() * 0.5).sum();
        assert!((area - 15.).abs() < 1e-9);
        assert_eq!(triangles.iter().flat_map(|t| t.boundary).filter(|b| *b).count(), 8);
        assert!(geometry[0].lines.is_empty());
    }

    #[test]
    fn mirrored_scaled_instances_keep_front_winding_and_hidden_instances_are_omitted() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let mut doc = editor.doc().clone();
        let definition = std::mem::take(&mut doc.root);
        doc.definitions.insert(DefinitionId(500), definition);
        let transform = DMat4::from_scale_rotation_translation(
            DVec3::new(-2., 3., 4.),
            DQuat::from_rotation_z(0.3),
            DVec3::new(10., -2., 5.),
        );
        let instance = Instance {
            definition: DefinitionId(500),
            transform,
            name: "Part".into(),
            tag: None,
            material: None,
            hidden: false,
            glued_to: None,
        };
        doc.root.instances.insert(InstanceId(501), instance.clone());
        doc.root.instances.insert(InstanceId(502), Instance { hidden: true, ..instance });
        let geometries = collect(&doc);
        assert_eq!(geometries.len(), 1);
        for triangle in &geometries[0].triangles {
            let n =
                (triangle.points[1] - triangle.points[0]).cross(triangle.points[2] - triangle.points[0]).normalize();
            assert!(n.dot(triangle.normal) > 0.99999);
            assert!(triangle.points.iter().all(|p| (p.z - 5.).abs() < 1e-9));
        }
    }

    #[test]
    fn loose_lines_are_exported_and_hidden_faces_are_omitted() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::splat(5.), DVec3::splat(6.)]], facing: None }).unwrap();
        assert_eq!(collect(editor.doc())[0].lines.len(), 1);
        let mut doc = editor.doc().clone();
        let id = doc.root.mesh.faces().next().unwrap().0;
        doc.root.mesh.face_mut(id).unwrap().hidden = true;
        assert!(collect(&doc)[0].triangles.is_empty());
    }
}
