//! Transactional Boolean operations on two closed, leaf instances.
use crate::solid_bsp::{self, Polygon};
use crate::{CommandError, Definition, DefinitionId, Document, Element, Instance, InstanceId};
use rysmo_math::{DMat4, DVec3, EPS, closest_on_segment};
use rysmo_mesh::{Edge, EdgeId, FaceId, Mesh, Vertex, VertexId, triangulate_face};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BooleanOp {
    Union,
    Subtract,
    Intersect,
    Trim,
    Split,
    OuterShell,
}
impl BooleanOp {
    pub fn name(self) -> &'static str {
        match self {
            Self::Union => "Solid Union",
            Self::Subtract => "Solid Subtract",
            Self::Intersect => "Solid Intersect",
            Self::Trim => "Solid Trim",
            Self::Split => "Solid Split",
            Self::OuterShell => "Outer Shell",
        }
    }
}
fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::Invalid(message.into())
}
fn operand(
    doc: &Document,
    owner: Option<DefinitionId>,
    id: InstanceId,
    shift: DVec3,
) -> Result<Vec<Polygon>, CommandError> {
    let instance = doc
        .def(owner)
        .and_then(|d| d.instances.get(&id))
        .ok_or_else(|| invalid("Solid operand not found in the editing context"))?;
    let def = doc.definitions.get(&instance.definition).ok_or_else(|| invalid("Missing solid definition"))?;
    if instance.hidden || !doc.tag_visible(instance.tag) || instance.glued_to.is_some() {
        return Err(invalid("Solid operand is hidden or glued to a face"));
    }
    if !def.instances.is_empty()
        || !def.guides.is_empty()
        || !def.texts.is_empty()
        || !def.dimensions.is_empty()
        || !def.sections.planes.is_empty()
    {
        return Err(invalid("Solid operands must be leaf groups/components containing only geometry"));
    }
    if def.mesh.faces().any(|(_, f)| f.hidden) {
        return Err(invalid("Unhide all operand faces before a solid operation"));
    }
    let estimated_triangles = def
        .mesh
        .faces()
        .map(|(_, f)| (f.loops().map(|l| l.len()).sum::<usize>() + 2 * f.holes.len()).saturating_sub(2))
        .sum::<usize>();
    if def.mesh.vertex_count() > 4096 || estimated_triangles > 1024 {
        return Err(invalid("Solid operands are limited to 4096 vertices and 1024 triangles each"));
    }
    let signed = crate::solid_validation::validate(&def.mesh, false).map_err(invalid)?;
    let t = instance.transform;
    if !t.is_finite() || !t.inverse().is_finite() || t.determinant().abs() < 1e-12 {
        return Err(invalid("Invalid solid transform"));
    }
    let inverse = t.inverse();
    let mut out = Vec::new();
    for (id, face) in def.mesh.faces() {
        let plane = def.mesh.face_plane(id).unwrap();
        let mut appearance = face.clone();
        appearance.material = appearance.material.or(instance.material);
        appearance.back_material = appearance.back_material.or(instance.material);
        if signed < 0. {
            std::mem::swap(&mut appearance.material, &mut appearance.back_material);
        }
        let size = [appearance.material, appearance.back_material]
            .into_iter()
            .flatten()
            .find_map(|m| doc.materials.get(&m).and_then(|m| m.texture.map(|t| t.size)))
            .or(face.uv.map(|_| [1., 1.]));
        if let Some(size) = size {
            appearance.uv = Some(
                crate::texture::transformed(plane, size, face.uv.unwrap_or_default(), t)
                    .ok_or_else(|| invalid("Cannot preserve operand texture placement"))?,
            );
        }
        let expected = inverse.transpose().transform_vector3(plane.normal).normalize() * signed.signum();
        for triangle in triangulate_face(&def.mesh, id) {
            let mut p = triangle.map(|v| t.transform_point3(def.mesh.pos(v)) - shift).to_vec();
            if (p[1] - p[0]).cross(p[2] - p[0]).length() < EPS * EPS {
                continue;
            }
            if (p[1] - p[0]).cross(p[2] - p[0]).dot(expected) < 0. {
                p.reverse();
            }
            out.push(Polygon::new(p, appearance.clone()).ok_or_else(|| invalid("Degenerate transformed solid face"))?);
        }
    }
    if out.len() > 1024 {
        return Err(invalid("Solid operands are limited to 1024 triangles each"));
    }
    Ok(out)
}
fn assemble(polygons: Vec<Polygon>, shift: DVec3, styles: &[(DVec3, DVec3, Edge)]) -> Result<Mesh, CommandError> {
    let mut positions = Vec::<DVec3>::new();
    let mut loops = Vec::new();
    let mut appearances = Vec::new();
    let mut weld_budget = 4_000_000usize;
    for p in polygons {
        let mut outline = Vec::new();
        for point in p.points {
            weld_budget = weld_budget
                .checked_sub(positions.len())
                .ok_or_else(|| invalid("Solid welding exceeds its complexity limit"))?;
            let i = if let Some(i) = positions.iter().position(|p| p.distance(point) < EPS) {
                i
            } else {
                if positions.len() >= 8192 {
                    return Err(invalid("Solid result exceeds its vertex limit"));
                }
                positions.push(point);
                positions.len() - 1
            };
            if outline.last() != Some(&i) {
                outline.push(i);
            }
        }
        if outline.first() == outline.last() {
            outline.pop();
        }
        if outline.len() < 3 {
            return Err(invalid("Solid result contains a collapsed face"));
        }
        loops.push(outline);
        appearances.push(p.face);
    }
    if positions.len() > 8192 {
        return Err(invalid("Solid result exceeds its vertex limit"));
    }
    let mut budget = 4_000_000usize;
    // Thread all existing vertices into every edge. BSP splits can otherwise leave
    // T junctions between a clipped polygon and an unsplit neighbouring polygon.
    for outline in &mut loops {
        let mut stitched = Vec::new();
        for i in 0..outline.len() {
            let a = outline[i];
            let b = outline[(i + 1) % outline.len()];
            let p = positions[a];
            let q = positions[b];
            let mut members = Vec::new();
            for (j, v) in positions.iter().enumerate() {
                budget =
                    budget.checked_sub(1).ok_or_else(|| invalid("Solid stitching exceeds its complexity limit"))?;
                let (point, t) = closest_on_segment(p, q, *v);
                if point.distance(*v) < EPS && t < 1. - 1e-10 {
                    members.push((t, j));
                }
            }
            members.sort_by(|a, b| a.0.total_cmp(&b.0));
            stitched.extend(members.into_iter().map(|(_, i)| i));
        }
        stitched.dedup();
        *outline = stitched;
    }
    let vertices =
        positions.iter().enumerate().map(|(i, p)| (VertexId(i as u64 + 1), Vertex { pos: *p + shift })).collect();
    let mut next = positions.len() as u64 + 1;
    let mut edges = BTreeMap::new();
    let mut edge_keys = BTreeMap::new();
    let mut faces = BTreeMap::new();
    for (outline, mut face) in loops.into_iter().zip(appearances) {
        face.outer = outline.into_iter().map(|i| VertexId(i as u64 + 1)).collect();
        face.holes.clear();
        face.hidden = false;
        for i in 0..face.outer.len() {
            let a = face.outer[i];
            let b = face.outer[(i + 1) % face.outer.len()];
            let key = (a.min(b), a.max(b));
            edge_keys.entry(key).or_insert_with(|| {
                let id = EdgeId(next);
                next += 1;
                edges.insert(id, Edge::new(a, b));
                id
            });
        }
        faces.insert(FaceId(next), face);
        next += 1;
    }
    let mut mesh = Mesh::from_parts(vertices, edges, faces).map_err(CommandError::from)?;
    // Hide triangulation/split seams on one plane without changing material regions.
    let soft = mesh
        .edges()
        .filter_map(|(id, _)| {
            let faces = mesh.faces_of_edge(id);
            (faces.len() == 2 && mesh.face_normal(faces[0])?.dot(mesh.face_normal(faces[1])?) > 1. - 1e-8).then_some(id)
        })
        .collect::<Vec<_>>();
    mesh.set_edge_flags(&soft, Some(true), Some(false), None);
    let mut style_budget = 4_000_000usize;
    let mut matched = Vec::new();
    for (id, _) in mesh.edges() {
        let (p, q) = mesh.edge_points(id);
        for (a, b, style) in styles {
            style_budget = style_budget
                .checked_sub(1)
                .ok_or_else(|| invalid("Solid edge matching exceeds its complexity limit"))?;
            if closest_on_segment(*a, *b, p).0.distance(p) < EPS && closest_on_segment(*a, *b, q).0.distance(q) < EPS {
                matched.push((id, style.clone()));
                break;
            }
        }
    }
    for (id, style) in matched {
        mesh.set_edge_flags(&[id], Some(style.soft), Some(style.smooth), Some(style.hidden));
        mesh.paint_edge(id, style.material).map_err(CommandError::from)?;
    }
    Ok(mesh)
}
pub(crate) fn run(
    doc: &mut Document,
    owner: Option<DefinitionId>,
    left: InstanceId,
    right: InstanceId,
    operation: BooleanOp,
    keep_inputs: bool,
) -> Result<Vec<Element>, CommandError> {
    if left == right {
        return Err(invalid("Choose two different solid instances"));
    }
    let first = doc.def(owner).and_then(|d| d.instances.get(&left)).ok_or_else(|| invalid("Missing first solid"))?;
    let bounds = doc.definition_bounds(Some(first.definition)).ok_or_else(|| invalid("Empty first solid"))?;
    let shift = first.transform.transform_point3((bounds.0 + bounds.1) * 0.5);
    let a = operand(doc, owner, left, shift).map_err(|e| invalid(format!("Operand A: {e}")))?;
    let b = operand(doc, owner, right, shift).map_err(|e| invalid(format!("Operand B: {e}")))?;
    let mut styles = Vec::new();
    for id in [left, right] {
        let instance = &doc.def(owner).unwrap().instances[&id];
        let source = &doc.definitions[&instance.definition].mesh;
        for (edge, style) in source.edges() {
            let (p, q) = source.edge_points(edge);
            styles.push((
                instance.transform.transform_point3(p),
                instance.transform.transform_point3(q),
                style.clone(),
            ));
        }
    }
    let tasks = match operation {
        BooleanOp::Split => vec![
            (a.clone(), b.clone(), BooleanOp::Subtract, "Split A minus B"),
            (b.clone(), a.clone(), BooleanOp::Subtract, "Split B minus A"),
            (a, b, BooleanOp::Intersect, "Split intersection"),
        ],
        BooleanOp::Trim => vec![(a, b, BooleanOp::Subtract, operation.name())],
        BooleanOp::OuterShell => vec![(a, b, BooleanOp::Union, operation.name())],
        _ => vec![(a, b, operation, operation.name())],
    };
    let mut results = Vec::new();
    for (a, b, basic, name) in tasks {
        let polygons = solid_bsp::boolean(a, b, basic).map_err(invalid)?;
        if polygons.is_empty() {
            continue;
        }
        let mut mesh = assemble(polygons, shift, &styles)?;
        let analysis =
            crate::solid_validation::analyse(&mesh).map_err(|e| invalid(format!("Invalid solid result: {e}")))?;
        if analysis.volume <= 0. {
            return Err(invalid("Solid result has inconsistent shell orientation"));
        }
        if operation == BooleanOp::OuterShell && analysis.shells.iter().any(|s| s.depth > 0) {
            let keep = analysis
                .shells
                .iter()
                .filter(|s| s.depth == 0)
                .flat_map(|s| s.faces.iter().map(|id| Element::Face(*id)))
                .collect::<Vec<_>>();
            mesh = mesh.extract(&keep);
            crate::solid_validation::validate(&mesh, false).map_err(invalid)?;
        }
        results.push((name, mesh));
    }
    // Stage and validate every Split piece before changing either operand.
    if !keep_inputs {
        let elements = if operation == BooleanOp::Trim {
            vec![Element::Instance(left)]
        } else {
            vec![Element::Instance(left), Element::Instance(right)]
        };
        crate::Command::Erase { elements }.run(doc, owner)?;
    }
    let mut created = Vec::new();
    for (name, mesh) in results {
        let mesh = mesh.with_fresh_ids(|| doc.reserve_id()).map_err(CommandError::from)?;
        let definition = DefinitionId(doc.reserve_id());
        let id = InstanceId(doc.reserve_id());
        doc.definitions.insert(definition, Definition { name: name.into(), group: true, mesh, ..Default::default() });
        doc.def_mut(owner).unwrap().instances.insert(
            id,
            Instance {
                definition,
                transform: DMat4::IDENTITY,
                name: name.into(),
                tag: None,
                material: None,
                hidden: false,
                glued_to: None,
            },
        );
        created.push(Element::Instance(id));
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor};
    fn cube(editor: &mut Editor, corner: DVec3, size: DVec3) -> InstanceId {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner, u: DVec3::X * size.x, v: DVec3::Y * size.y, facing: Some(DVec3::Z) })
            .unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        e.apply(Command::PushPull { face, distance: size.z * e.mesh().face_normal(face).unwrap().z, keep_base: false })
            .unwrap();
        let mut doc = editor.doc().clone();
        let definition = DefinitionId(doc.reserve_id());
        let mesh = e.mesh().with_fresh_ids(|| doc.reserve_id()).unwrap();
        doc.definitions.insert(definition, Definition { mesh, group: true, ..Default::default() });
        let id = InstanceId(doc.reserve_id());
        doc.root.instances.insert(
            id,
            Instance {
                definition,
                transform: DMat4::IDENTITY,
                name: "Cube".into(),
                material: None,
                tag: None,
                hidden: false,
                glued_to: None,
            },
        );
        *editor = Editor::with_document(doc);
        id
    }
    fn volume(e: &Editor, id: InstanceId) -> f64 {
        let instance = &e.active().instances[&id];
        crate::solid_validation::validate(&e.doc().definitions[&instance.definition].mesh, false).unwrap()
    }
    #[test]
    fn overlapping_boxes_boolean_volumes_and_undo() {
        for (operation, expected) in [(BooleanOp::Union, 15.), (BooleanOp::Subtract, 7.), (BooleanOp::Intersect, 1.)] {
            let mut e = Editor::new();
            let a = cube(&mut e, DVec3::ZERO, DVec3::splat(2.));
            let b = cube(&mut e, DVec3::ONE, DVec3::splat(2.));
            let before = e.doc().clone();
            let out = e.apply(Command::SolidBoolean { left: a, right: b, operation, keep_inputs: false }).unwrap();
            let Element::Instance(result) = out.created[0] else { panic!() };
            assert!((volume(&e, result) - expected).abs() < 1e-7, "{operation:?}");
            assert_eq!(e.active().instances.len(), 1);
            assert_eq!(e.undo_label(), Some(operation.name()));
            e.undo();
            assert_eq!(e.doc(), &before);
            e.redo();
            assert!((volume(&e, result) - expected).abs() < 1e-7);
        }
    }
    #[test]
    fn coplanar_identical_disjoint_and_cavity_cases() {
        for (corner, size, volumes) in [
            (DVec3::new(1., 0., 0.), DVec3::splat(2.), [12., 4., 4.]),
            (DVec3::new(2., 0., 0.), DVec3::splat(2.), [16., 8., 0.]),
            (DVec3::new(4., 0., 0.), DVec3::splat(2.), [16., 8., 0.]),
            (DVec3::ZERO, DVec3::splat(2.), [8., 0., 8.]),
            (DVec3::splat(0.5), DVec3::ONE, [8., 7., 1.]),
        ] {
            for (operation, expected) in
                [BooleanOp::Union, BooleanOp::Subtract, BooleanOp::Intersect].into_iter().zip(volumes)
            {
                let mut e = Editor::new();
                let a = cube(&mut e, DVec3::ZERO, DVec3::splat(2.));
                let b = cube(&mut e, corner, size);
                let before = e.doc().clone();
                let out = e
                    .apply(Command::SolidBoolean { left: a, right: b, operation, keep_inputs: false })
                    .unwrap_or_else(|e| panic!("{corner:?} {operation:?}: {e}"));
                if expected == 0. {
                    assert!(out.created.is_empty());
                    assert!(e.active().instances.is_empty());
                } else {
                    let Element::Instance(id) = out.created[0] else { panic!() };
                    assert!(
                        (volume(&e, id) - expected).abs() < 1e-7,
                        "{corner:?} {operation:?} actual {} expected {expected}",
                        volume(&e, id)
                    );
                }
                e.undo();
                assert_eq!(e.doc(), &before);
            }
        }
    }
    #[test]
    fn mirrored_rotated_nonuniform_instances_and_shared_definitions_are_preserved() {
        let mut e = Editor::new();
        let a = cube(&mut e, DVec3::ZERO, DVec3::ONE);
        let b = cube(&mut e, DVec3::X * 0.5, DVec3::ONE);
        let mut doc = e.doc().clone();
        let transform = DMat4::from_translation(DVec3::new(3., 2., 1.))
            * DMat4::from_rotation_z(0.4)
            * DMat4::from_scale(DVec3::new(-2., 1., 1.));
        doc.root.instances.get_mut(&a).unwrap().transform = transform;
        doc.root.instances.get_mut(&b).unwrap().transform = transform;
        let definition = doc.root.instances[&a].definition;
        let shared = InstanceId(doc.reserve_id());
        doc.root.instances.insert(shared, doc.root.instances[&a].clone());
        let before = doc.definitions[&definition].clone();
        e = Editor::with_document(doc);
        let out = e
            .apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Union, keep_inputs: true })
            .unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        assert!((volume(&e, id) - 3.).abs() < 1e-7);
        assert_eq!(e.active().instances.len(), 4);
        assert_eq!(&e.doc().definitions[&definition], &before);
    }
    #[test]
    fn invalid_operand_is_atomic_and_point_touch_union_is_rejected() {
        let mut e = Editor::new();
        let a = cube(&mut e, DVec3::ZERO, DVec3::ONE);
        let b = cube(&mut e, DVec3::ONE, DVec3::ONE);
        let before = e.doc().clone();
        let revision = e.revision();
        let depth = e.undo_depth();
        assert!(
            e.apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Union, keep_inputs: false })
                .is_err()
        );
        assert_eq!(e.doc(), &before);
        assert_eq!(e.revision(), revision);
        assert_eq!(e.undo_depth(), depth);
        let mut doc = e.doc().clone();
        let def = doc.root.instances[&b].definition;
        let mesh = &mut doc.definitions.get_mut(&def).unwrap().mesh;
        let face = mesh.faces().next().unwrap().0;
        mesh.erase(&[Element::Face(face)]);
        e = Editor::with_document(doc);
        let before = e.doc().clone();
        assert!(
            e.apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Subtract, keep_inputs: false })
                .is_err()
        );
        assert_eq!(e.doc(), &before);
    }
    #[test]
    fn empty_intersection_with_retained_inputs_does_not_add_history() {
        let mut e = Editor::new();
        let a = cube(&mut e, DVec3::ZERO, DVec3::ONE);
        let b = cube(&mut e, DVec3::splat(4.), DVec3::ONE);
        let before = e.doc().clone();
        let out = e
            .apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Intersect, keep_inputs: true })
            .unwrap();
        assert!(!out.changed);
        assert_eq!(e.undo_depth(), 0);
        assert_eq!(e.doc(), &before);
    }
    #[test]
    fn polygonal_cylinder_cut_preserves_a_closed_curved_boundary() {
        let mut e = Editor::new();
        let b = cube(&mut e, DVec3::new(0., -2., -1.), DVec3::new(2., 4., 4.));
        e.apply(Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1., segments: 16, start: None })
            .unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        e.apply(Command::PushPull { face, distance: 2. * e.mesh().face_normal(face).unwrap().z, keep_base: false })
            .unwrap();
        let elements = e.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
        let a = e.apply(Command::MakeGroup { elements }).unwrap().created[0];
        let Element::Instance(a) = a else { panic!() };
        let out = e
            .apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Subtract, keep_inputs: false })
            .unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        let expected = 8. * (std::f64::consts::TAU / 16.).sin();
        assert!((volume(&e, id) - expected).abs() < 1e-6);
        let mesh = &e.doc().definitions[&e.active().instances[&id].definition].mesh;
        assert!(mesh.edges().any(|(_, edge)| edge.soft && edge.smooth));
    }
    #[test]
    fn self_intersecting_triangulated_shell_is_rejected() {
        let mut e = Editor::new();
        let a = cube(&mut e, DVec3::ZERO, DVec3::ONE);
        let b = cube(&mut e, DVec3::X * 4., DVec3::ONE);
        let mut doc = e.doc().clone();
        let definition = doc.root.instances[&a].definition;
        let source = &doc.definitions[&definition].mesh;
        let mut vertices = source.vertices().map(|(id, v)| (id, v.clone())).collect::<BTreeMap<_, _>>();
        let moved = *vertices.iter().min_by(|a, b| a.1.pos.element_sum().total_cmp(&b.1.pos.element_sum())).unwrap().0;
        vertices.get_mut(&moved).unwrap().pos = DVec3::new(2.3, 1.7, 0.4);
        let mut next = source.next_id();
        let mut edges = BTreeMap::new();
        let mut keys = std::collections::BTreeSet::new();
        let mut faces = BTreeMap::new();
        for (id, face) in source.faces() {
            for triangle in triangulate_face(source, id) {
                for i in 0..3 {
                    let a = triangle[i];
                    let b = triangle[(i + 1) % 3];
                    if keys.insert((a.min(b), a.max(b))) {
                        edges.insert(EdgeId(next), Edge::new(a, b));
                        next += 1;
                    }
                }
                faces
                    .insert(FaceId(next), rysmo_mesh::Face { outer: triangle.to_vec(), holes: vec![], ..face.clone() });
                next += 1;
            }
        }
        let mesh = Mesh::from_parts(vertices, edges, faces).unwrap();
        let error = crate::solid_validation::validate(&mesh, true).unwrap_err();
        assert!(error.contains("Self-intersecting"), "{error}");
        doc.definitions.get_mut(&definition).unwrap().mesh = mesh;
        e = Editor::with_document(doc);
        let before = e.doc().clone();
        assert!(
            e.apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Union, keep_inputs: false })
                .is_err()
        );
        assert_eq!(e.doc(), &before);
    }
    #[test]
    fn materials_explicit_uv_and_inward_shells_survive_boolean_processing() {
        let mut e = Editor::new();
        let a = cube(&mut e, DVec3::ZERO, DVec3::splat(2.));
        let b = cube(&mut e, DVec3::ONE, DVec3::splat(2.));
        let mut doc = e.doc().clone();
        let definition = doc.root.instances[&a].definition;
        let mesh = &mut doc.definitions.get_mut(&definition).unwrap().mesh;
        let ids = mesh.faces().map(|(id, _)| id).collect::<Vec<_>>();
        let uv = rysmo_mesh::TexturePlacement { offset: [0.2, 0.3], rotation: 30., ..Default::default() };
        for id in &ids {
            let face = mesh.face_mut(*id).unwrap();
            face.material = Some(crate::MaterialId(9999));
            face.uv = Some(uv);
        }
        mesh.reverse_faces(&ids);
        doc.materials.insert(
            crate::MaterialId(9999),
            crate::Material { name: "Test".into(), color: [1., 0., 0., 1.], texture: None },
        );
        e = Editor::with_document(doc);
        let out = e
            .apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Union, keep_inputs: false })
            .unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        assert!((volume(&e, id) - 15.).abs() < 1e-6);
        let mesh = &e.doc().definitions[&e.active().instances[&id].definition].mesh;
        let faces = mesh.faces().filter(|(_, f)| f.material == Some(crate::MaterialId(9999))).collect::<Vec<_>>();
        assert!(!faces.is_empty());
        assert!(faces.iter().all(|(_, f)| f.uv.is_some()));
        for (id, face) in faces {
            let normal = mesh.face_normal(id).unwrap();
            for vertex in &face.outer {
                let p = mesh.pos(*vertex);
                let expected = crate::uv_at(p, normal, [1., 1.], &uv);
                let actual = crate::uv_at(p, normal, [1., 1.], &face.uv.unwrap());
                assert!((expected[0] - actual[0]).abs() < 1e-7 && (expected[1] - actual[1]).abs() < 1e-7);
            }
        }
    }
    #[test]
    fn far_origin_and_reversed_operand_order() {
        let mut e = Editor::new();
        let origin = DVec3::splat(1e6);
        let a = cube(&mut e, origin, DVec3::splat(2.));
        let b = cube(&mut e, origin + DVec3::splat(0.5), DVec3::ONE);
        let before = e.doc().clone();
        let out = e
            .apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Subtract, keep_inputs: false })
            .unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        assert!((volume(&e, id) - 7.).abs() < 1e-7);
        e.undo();
        assert_eq!(e.doc(), &before);
        let out = e
            .apply(Command::SolidBoolean { left: b, right: a, operation: BooleanOp::Subtract, keep_inputs: false })
            .unwrap();
        assert!(out.created.is_empty());
    }
    fn result_id(out: crate::command::Outcome) -> InstanceId {
        let Element::Instance(id) = out.created[0] else { panic!() };
        id
    }
    #[test]
    fn trim_preserves_the_cutter_and_undo_restores_both_operands() {
        for keep_inputs in [false, true] {
            let mut e = Editor::new();
            let a = cube(&mut e, DVec3::ZERO, DVec3::splat(2.));
            let b = cube(&mut e, DVec3::ONE, DVec3::splat(2.));
            let before = e.doc().clone();
            let cutter = e.active().instances[&b].clone();
            let id = result_id(
                e.apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Trim, keep_inputs }).unwrap(),
            );
            assert!((volume(&e, id) - 7.).abs() < 1e-7);
            assert_eq!(&e.active().instances[&b], &cutter);
            assert_eq!(e.active().instances.contains_key(&a), keep_inputs);
            assert_eq!(e.undo_label(), Some("Solid Trim"));
            e.undo();
            assert_eq!(e.doc(), &before);
        }
        let mut e = Editor::new();
        let a = cube(&mut e, DVec3::ONE, DVec3::ONE);
        let b = cube(&mut e, DVec3::ZERO, DVec3::splat(3.));
        let out = e
            .apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Trim, keep_inputs: false })
            .unwrap();
        assert!(out.created.is_empty());
        assert_eq!(e.active().instances.keys().copied().collect::<Vec<_>>(), vec![b]);
    }
    #[test]
    fn split_creates_three_disjoint_pieces_in_one_history_step() {
        let mut e = Editor::new();
        let a = cube(&mut e, DVec3::ZERO, DVec3::splat(2.));
        let b = cube(&mut e, DVec3::ONE, DVec3::splat(2.));
        let before = e.doc().clone();
        let out = e
            .apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Split, keep_inputs: false })
            .unwrap();
        assert_eq!(out.created.len(), 3);
        assert_eq!(e.undo_depth(), 1);
        let mut actual = out
            .created
            .iter()
            .map(|e0| {
                let Element::Instance(id) = e0 else { panic!() };
                volume(&e, *id)
            })
            .collect::<Vec<_>>();
        actual.sort_by(f64::total_cmp);
        assert!(actual.iter().zip([1., 7., 7.]).all(|(a, b)| (*a - b).abs() < 1e-7));
        e.undo();
        assert_eq!(e.doc(), &before);
        e.redo();
        assert_eq!(e.active().instances.len(), 3);
    }
    #[test]
    fn cavities_and_nested_islands_can_be_used_in_followup_booleans_and_report_volume() {
        let mut e = Editor::new();
        let a = cube(&mut e, DVec3::ZERO, DVec3::splat(4.));
        let b = cube(&mut e, DVec3::ONE, DVec3::splat(2.));
        let hollow = result_id(
            e.apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Subtract, keep_inputs: false })
                .unwrap(),
        );
        assert!((e.doc().instance_volume(&e.active().instances[&hollow], DMat4::IDENTITY).unwrap() - 56.).abs() < 1e-7);
        let island = cube(&mut e, DVec3::splat(1.5), DVec3::ONE);
        let empty = e
            .apply(Command::SolidBoolean {
                left: hollow,
                right: island,
                operation: BooleanOp::Intersect,
                keep_inputs: true,
            })
            .unwrap();
        assert!(!empty.changed);
        assert!(empty.created.is_empty());
        let compound = result_id(
            e.apply(Command::SolidBoolean {
                left: hollow,
                right: island,
                operation: BooleanOp::Union,
                keep_inputs: false,
            })
            .unwrap(),
        );
        assert!((volume(&e, compound) - 57.).abs() < 1e-7);
        let mesh = &e.doc().definitions[&e.active().instances[&compound].definition].mesh;
        let mut depths =
            crate::solid_validation::analyse(mesh).unwrap().shells.iter().map(|s| s.depth).collect::<Vec<_>>();
        depths.sort();
        assert_eq!(depths, vec![0, 1, 2]);
        let cutter = cube(&mut e, DVec3::ZERO, DVec3::new(2., 4., 4.));
        let cut = result_id(
            e.apply(Command::SolidBoolean {
                left: compound,
                right: cutter,
                operation: BooleanOp::Subtract,
                keep_inputs: false,
            })
            .unwrap(),
        );
        assert!((volume(&e, cut) - 28.5).abs() < 1e-7);
    }
    #[test]
    fn outer_shell_removes_enclosed_cavities_and_keeps_separate_exteriors() {
        let mut e = Editor::new();
        let a = cube(&mut e, DVec3::ZERO, DVec3::splat(4.));
        let b = cube(&mut e, DVec3::ONE, DVec3::splat(2.));
        let hollow = result_id(
            e.apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Subtract, keep_inputs: false })
                .unwrap(),
        );
        let separate = cube(&mut e, DVec3::X * 6., DVec3::ONE);
        let before = e.doc().clone();
        let out = result_id(
            e.apply(Command::SolidBoolean {
                left: hollow,
                right: separate,
                operation: BooleanOp::OuterShell,
                keep_inputs: false,
            })
            .unwrap(),
        );
        assert!((volume(&e, out) - 65.).abs() < 1e-7);
        assert!((e.doc().instance_volume(&e.active().instances[&out], DMat4::IDENTITY).unwrap() - 65.).abs() < 1e-7);
        e.undo();
        assert_eq!(e.doc(), &before);
        e.redo();
        assert!((volume(&e, out) - 65.).abs() < 1e-7);
        let clip = cube(&mut e, DVec3::ZERO, DVec3::splat(5.));
        let intersect = result_id(
            e.apply(Command::SolidBoolean {
                left: out,
                right: clip,
                operation: BooleanOp::Intersect,
                keep_inputs: false,
            })
            .unwrap(),
        );
        assert!((volume(&e, intersect) - 64.).abs() < 1e-7);
    }
    #[test]
    fn wrong_orientation_in_a_cavity_is_rejected_atomically() {
        let mut e = Editor::new();
        let a = cube(&mut e, DVec3::ZERO, DVec3::splat(4.));
        let b = cube(&mut e, DVec3::ONE, DVec3::splat(2.));
        let hollow = result_id(
            e.apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Subtract, keep_inputs: false })
                .unwrap(),
        );
        let other = cube(&mut e, DVec3::X * 6., DVec3::ONE);
        let mut doc = e.doc().clone();
        let def = doc.root.instances[&hollow].definition;
        let mesh = &mut doc.definitions.get_mut(&def).unwrap().mesh;
        let cavity = crate::solid_validation::analyse(mesh).unwrap().shells.into_iter().find(|s| s.depth == 1).unwrap();
        mesh.reverse_faces(&cavity.faces.into_iter().collect::<Vec<_>>());
        let error = crate::solid_validation::validate(mesh, false).unwrap_err();
        assert!(error.contains("cavity nesting"));
        e = Editor::with_document(doc);
        let before = e.doc().clone();
        assert!(
            e.apply(Command::SolidBoolean {
                left: hollow,
                right: other,
                operation: BooleanOp::Union,
                keep_inputs: false
            })
            .is_err()
        );
        assert_eq!(e.doc(), &before);
    }
    #[test]
    fn split_omits_empty_pieces_for_identical_nested_and_disjoint_operands() {
        for (corner, size, count, total) in [
            (DVec3::ZERO, DVec3::splat(2.), 1, 8.),
            (DVec3::splat(0.5), DVec3::ONE, 2, 8.),
            (DVec3::X * 4., DVec3::splat(2.), 2, 16.),
        ] {
            let mut e = Editor::new();
            let a = cube(&mut e, DVec3::ZERO, DVec3::splat(2.));
            let b = cube(&mut e, corner, size);
            let out = e
                .apply(Command::SolidBoolean { left: a, right: b, operation: BooleanOp::Split, keep_inputs: true })
                .unwrap();
            assert_eq!(out.created.len(), count);
            let sum = out
                .created
                .iter()
                .map(|item| {
                    let Element::Instance(id) = item else { panic!() };
                    volume(&e, *id)
                })
                .sum::<f64>();
            assert!((sum - total).abs() < 1e-7);
            assert_eq!(e.active().instances.len(), count + 2);
        }
    }
}
