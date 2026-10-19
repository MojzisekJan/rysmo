//! Connected triangular terrain extraction for tools targeting raw model geometry.
use rysmo_mesh::{FaceId, Mesh};
use std::collections::{BTreeMap, BTreeSet};
/// Collects edge-connected triangles from a seed face without changing the source.
/// Non-triangular neighbouring geometry forms a boundary, not part of the TIN.
pub fn connected(mesh: &Mesh, seed: FaceId) -> Result<Mesh, String> {
    let triangle = |id| mesh.face(id).is_some_and(|face| face.outer.len() == 3 && face.holes.is_empty());
    if !triangle(seed) {
        return Err("Choose a triangular terrain face.".into());
    }
    let mut adjacency = BTreeMap::new();
    for (id, face) in mesh.faces() {
        if face.outer.len() != 3 || !face.holes.is_empty() {
            continue;
        }
        for edge in mesh.face_edges(id) {
            adjacency.entry(edge).or_insert_with(Vec::new).push(id);
        }
    }
    let mut selected = BTreeSet::new();
    let mut pending = vec![seed];
    while let Some(face) = pending.pop() {
        if !selected.insert(face) {
            continue;
        }
        for edge in mesh.face_edges(face) {
            if let Some(faces) = adjacency.get(&edge) {
                pending.extend(faces.iter().copied().filter(|id| !selected.contains(id)));
            }
        }
    }
    let faces = selected.iter().map(|id| (*id, mesh.face(*id).unwrap().clone())).collect::<BTreeMap<_, _>>();
    let vertex_ids = faces.values().flat_map(|face| face.outer.iter().copied()).collect::<BTreeSet<_>>();
    let vertices = vertex_ids.into_iter().map(|id| (id, mesh.vertex(id).unwrap().clone())).collect();
    let edge_ids = selected.iter().flat_map(|id| mesh.face_edges(*id)).collect::<BTreeSet<_>>();
    let edges = edge_ids
        .into_iter()
        .map(|id| {
            let mut edge = mesh.edge(id).unwrap().clone();
            edge.curve = None;
            (id, edge)
        })
        .collect();
    Mesh::from_parts(vertices, edges, faces).map_err(|e| e.to_string())
}
/// Replace a terrain region while preserving unrelated geometry and curve identities.
/// Shared boundaries may only remain unchanged; otherwise the operation is rejected.
pub fn replace(source: &Mesh, region: &Mesh, replacement: &Mesh) -> Result<Mesh, String> {
    let mut vertices = source.vertices().map(|(id, value)| (id, value.clone())).collect::<BTreeMap<_, _>>();
    let mut edges = source.edges().map(|(id, value)| (id, value.clone())).collect::<BTreeMap<_, _>>();
    let mut faces = source.faces().map(|(id, value)| (id, value.clone())).collect::<BTreeMap<_, _>>();
    let mut curves = source.curves().map(|(id, value)| (id, value.clone())).collect::<BTreeMap<_, _>>();
    for (id, _) in region.faces() {
        faces.remove(&id);
    }
    let external_edges =
        source.edges().filter(|(id, _)| region.edge(*id).is_none()).map(|(_, edge)| edge.clone()).collect::<Vec<_>>();
    for (id, _) in region.edges() {
        let shared = faces.keys().any(|face| source.face_edges(*face).contains(&id));
        if shared && replacement.edge(id) != region.edge(id) {
            return Err("Stamp would split a boundary shared with other geometry. Group the terrain first.".into());
        }
        edges.remove(&id);
    }
    for (id, _) in region.vertices() {
        let shared = external_edges.iter().any(|edge| edge.v.contains(&id))
            || faces.values().any(|face| face.outer.contains(&id) || face.holes.iter().any(|ring| ring.contains(&id)));
        if shared && replacement.vertex(id) != source.vertex(id) {
            return Err("Stamp would move a vertex shared with other geometry. Group the terrain first.".into());
        }
        if !shared {
            vertices.remove(&id);
        }
    }
    vertices.extend(replacement.vertices().map(|(id, value)| (id, value.clone())));
    edges.extend(replacement.edges().map(|(id, value)| (id, value.clone())));
    faces.extend(replacement.faces().map(|(id, value)| (id, value.clone())));
    curves.retain(|id, _| edges.values().any(|edge| edge.curve == Some(*id)));
    curves.extend(replacement.curves().map(|(id, value)| (id, value.clone())));
    Mesh::from_parts_with_curves(vertices, edges, faces, curves).map_err(|e| e.to_string())
}

/// Restore complete source curves to an extracted region before deformation.
pub fn with_curves(source: &Mesh, region: Mesh) -> Result<Mesh, String> {
    let mut edges = region.edges().map(|(id, value)| (id, value.clone())).collect::<BTreeMap<_, _>>();
    let mut curves = BTreeMap::new();
    for (id, curve) in source.curves() {
        let members =
            source.edges().filter(|(_, edge)| edge.curve == Some(id)).map(|(edge, _)| edge).collect::<Vec<_>>();
        if members.iter().all(|edge| edges.contains_key(edge)) {
            for edge in members {
                edges.get_mut(&edge).unwrap().curve = Some(id);
            }
            curves.insert(id, curve.clone());
        } else if members.iter().any(|edge| edges.contains_key(edge)) {
            return Err("A terrain curve crosses into other geometry. Group the terrain first.".into());
        }
    }
    Mesh::from_parts_with_curves(
        region.vertices().map(|(id, v)| (id, v.clone())).collect(),
        edges,
        region.faces().map(|(id, f)| (id, f.clone())).collect(),
        curves,
    )
    .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    #[test]
    fn raw_stamp_preserves_sources_and_unrelated_geometry_and_history() {
        use crate::{Command, Document, Editor, Element};
        let mut document = Document::default();
        document.root.mesh = crate::terrain::grid(6., 6., 1.).unwrap();
        document.next_id = document.root.mesh.next_id();
        let mut editor = Editor::new();
        editor.replace_document(document);
        let target = editor.mesh().faces().next().unwrap().0;
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(2., 2., 5.),
                u: DVec3::X * 2.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let source = editor.mesh().faces().find(|(_, face)| face.outer.len() == 4).unwrap().0;
        editor
            .apply(Command::DrawLines { lines: vec![[DVec3::new(10., 0., 0.), DVec3::new(11., 0., 0.)]], facing: None })
            .unwrap();
        let before = editor.doc().clone();
        let command =
            |height| Command::StampOnFace { sources: vec![Element::Face(source)], target, offset: 0.5, height };
        editor.apply(command(2.)).unwrap();
        editor.mesh().validate().unwrap();
        assert_eq!(editor.mesh().face(source), before.root.mesh.face(source));
        for vertex in &before.root.mesh.face(source).unwrap().outer {
            assert_eq!(editor.mesh().vertex(*vertex), before.root.mesh.vertex(*vertex));
        }
        assert!(editor.mesh().find_vertex(DVec3::new(3., 3., 2.)).is_some());
        assert!(editor.mesh().find_vertex(DVec3::new(10., 0., 0.)).is_some());
        editor.amend_last(command(2.5)).unwrap();
        assert!(editor.mesh().find_vertex(DVec3::new(3., 3., 2.5)).is_some());
        let after = editor.doc().clone();
        editor.undo();
        assert_eq!(editor.doc(), &before);
        editor.redo();
        assert_eq!(editor.doc(), &after);
        assert!(
            editor
                .apply(Command::StampOnFace { sources: vec![Element::Face(target)], target, offset: 1., height: 3. })
                .is_err()
        );
        assert_eq!(editor.doc(), &after);
    }

    #[test]
    fn raw_stamp_uses_rotated_drawing_axes_and_preserves_other_islands() {
        use crate::{Command, Document, Editor, Element};
        use rysmo_math::{DMat4, DQuat, DrawingAxes};
        let rotation = DQuat::from_rotation_x(0.37) * DQuat::from_rotation_z(0.29);
        let origin = DVec3::new(3., -2., 4.);
        let transform = DMat4::from_rotation_translation(rotation, origin);
        let axes = DrawingAxes::new(origin, rotation * DVec3::X, rotation * DVec3::Y).unwrap();
        let mut document = Document::default();
        document.root.axes = axes;
        document.root.mesh = crate::terrain::grid(6., 6., 1.).unwrap();
        document.root.mesh.map_all(|point| transform.transform_point3(point));
        document.next_id = document.root.mesh.next_id();
        let mut editor = Editor::new();
        editor.replace_document(document);
        let target = editor.mesh().faces().next().unwrap().0;
        editor
            .apply(Command::DrawRectangle {
                corner: transform.transform_point3(DVec3::new(2., 2., 5.)),
                u: rotation * DVec3::X * 2.,
                v: rotation * DVec3::Y * 2.,
                facing: Some(axes.z()),
            })
            .unwrap();
        let source = editor.mesh().faces().find(|(_, face)| face.outer.len() == 4).unwrap().0;
        editor
            .apply(Command::DrawLines {
                lines: vec![
                    [DVec3::new(20., 0., 0.), DVec3::new(21., 0., 0.)],
                    [DVec3::new(21., 0., 0.), DVec3::new(20., 1., 0.)],
                    [DVec3::new(20., 1., 0.), DVec3::new(20., 0., 0.)],
                ],
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let island = editor
            .mesh()
            .faces()
            .find(|(_, face)| face.outer.iter().all(|id| editor.mesh().pos(*id).x >= 20.))
            .unwrap()
            .0;
        let before = editor.doc().clone();
        editor
            .apply(Command::StampOnFace { sources: vec![Element::Face(source)], target, offset: 0.5, height: 2. })
            .unwrap();
        assert!(editor.mesh().find_vertex(transform.transform_point3(DVec3::new(3., 3., 2.))).is_some());
        let unaffected = before.root.mesh.find_vertex(transform.transform_point3(DVec3::ZERO)).unwrap();
        assert_eq!(editor.mesh().vertex(unaffected), before.root.mesh.vertex(unaffected));
        assert_eq!(editor.mesh().face(island), before.root.mesh.face(island));
        for id in &before.root.mesh.face(island).unwrap().outer {
            assert_eq!(editor.mesh().vertex(*id), before.root.mesh.vertex(*id));
        }
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }

    #[test]
    fn replacement_rejects_moving_vertices_used_by_external_edges() {
        let mut source = crate::terrain::grid(2., 2., 1.).unwrap();
        source.add_lines(&[(DVec3::ZERO, DVec3::new(-1., 0., 0.))], None).unwrap();
        let target = source.faces().next().unwrap().0;
        let region = connected(&source, target).unwrap();
        let mut replacement = region.clone();
        replacement.map_all(|point| point + DVec3::Z);
        assert!(replace(&source, &region, &replacement).is_err());
        assert_eq!(replace(&source, &region, &region).unwrap(), source);
    }

    #[test]
    fn raw_terrain_drape_adds_surface_edges_and_undoes_atomically() {
        use crate::{Command, Document, Editor, Element};
        let mut document = Document::default();
        document.root.mesh = crate::terrain::grid(4., 4., 1.).unwrap();
        document.next_id = document.root.mesh.next_id();
        let mut editor = Editor::new();
        editor.replace_document(document);
        editor
            .apply(Command::DrawLines {
                lines: vec![[DVec3::new(-1., 1.5, 5.), DVec3::new(5., 1.5, 5.)]],
                facing: None,
            })
            .unwrap();
        let sources = editor
            .mesh()
            .edges()
            .filter(|(_, edge)| edge.v.iter().all(|id| editor.mesh().pos(*id).z == 5.))
            .map(|(id, _)| Element::Edge(id))
            .collect::<Vec<_>>();
        let target = editor.mesh().faces().next().unwrap().0;
        let before = editor.doc().clone();
        editor.apply(Command::DrapeOnFace { sources: sources.clone(), target }).unwrap();
        editor.mesh().validate().unwrap();
        assert!(editor.mesh().find_vertex(DVec3::new(1.5, 1.5, 0.)).is_some());
        for source in sources {
            let Element::Edge(id) = source else { panic!() };
            assert_eq!(editor.mesh().edge(id), before.root.mesh.edge(id));
        }
        let after = editor.doc().clone();
        assert!(editor.apply(Command::DrapeOnFace { sources: vec![Element::Face(target)], target }).is_err());
        assert_eq!(editor.doc(), &after);
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }

    #[test]
    fn extraction_keeps_ids_and_attributes_and_excludes_unrelated_geometry() {
        let mut mesh = crate::terrain::grid(4., 4., 1.).unwrap();
        let source = mesh.clone();
        let mut lines = Vec::new();
        let points =
            [DVec3::new(10., 10., 3.), DVec3::new(12., 10., 3.), DVec3::new(12., 12., 3.), DVec3::new(10., 12., 3.)];
        for i in 0..4 {
            lines.push((points[i], points[(i + 1) % 4]));
        }
        let triangle = [DVec3::new(16., 10., 3.), DVec3::new(18., 10., 3.), DVec3::new(17., 12., 3.)];
        for i in 0..3 {
            lines.push((triangle[i], triangle[(i + 1) % 3]));
        }
        mesh.add_lines(&lines, Some(DVec3::Z)).unwrap();
        let seed = source.faces().next().unwrap().0;
        let region = connected(&mesh, seed).unwrap();
        assert_eq!(region.faces().count(), source.faces().count());
        for (id, face) in source.faces() {
            assert_eq!(region.face(id), Some(face));
        }
        for (id, edge) in source.edges() {
            assert_eq!(region.edge(id), Some(edge));
        }
        for (id, vertex) in source.vertices() {
            assert_eq!(region.vertex(id), Some(vertex));
        }
        let quad = mesh.faces().find(|(_, face)| face.outer.len() == 4).unwrap().0;
        assert!(connected(&mesh, quad).is_err());
        assert!(connected(&mesh, FaceId(u64::MAX)).is_err());
        crate::terrain_surface::TerrainSurface::new(&region).unwrap();
    }
}
