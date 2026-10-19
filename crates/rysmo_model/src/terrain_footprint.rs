//! Projected boundary loops of a connected or disjoint selection of faces.
use rysmo_math::{DMat4, DVec2, polygon_area_2d};
use rysmo_mesh::{FaceId, Mesh, VertexId};
use std::collections::{BTreeMap, BTreeSet};

/// Extracts face-set boundaries in the supplied projection frame. Internal shared
/// edges disappear; outer loops are counter-clockwise and holes clockwise.
/// Non-manifold selections and degenerate projected faces are rejected.
pub fn face_boundaries(mesh: &Mesh, faces: &[FaceId], projection: DMat4) -> Result<Vec<Vec<DVec2>>, String> {
    if faces.is_empty() || !projection.is_finite() {
        return Err("Select finite stamp faces.".into());
    }
    let mut counts = BTreeMap::new();
    let mut points = BTreeMap::new();
    for id in faces.iter().copied().collect::<BTreeSet<_>>() {
        let face = mesh.face(id).ok_or("Stamp face is missing.")?;
        for ring in std::iter::once(&face.outer).chain(face.holes.iter()) {
            let mut projected = Vec::new();
            for vertex in ring {
                let p = projection.transform_point3(mesh.pos(*vertex));
                if !p.is_finite() {
                    return Err("Stamp projection is not finite.".into());
                }
                let xy = DVec2::new(p.x, p.y);
                points.insert(*vertex, xy);
                projected.push(xy);
            }
            if polygon_area_2d(&projected).abs() < 1e-14 {
                return Err("Stamp face collapses in projection.".into());
            }
        }
        for (a, b) in face.loop_edges() {
            let key = if a < b { (a, b) } else { (b, a) };
            let count = counts.entry(key).or_insert(0);
            *count += 1;
            if *count > 2 {
                return Err("Stamp faces form a non-manifold boundary.".into());
            }
        }
    }
    let mut adjacency: BTreeMap<VertexId, BTreeSet<VertexId>> = BTreeMap::new();
    for ((a, b), count) in counts {
        if count != 1 {
            continue;
        }
        if points[&a].distance_squared(points[&b]) < 1e-20 {
            return Err("Stamp boundary collapses in projection.".into());
        }
        adjacency.entry(a).or_default().insert(b);
        adjacency.entry(b).or_default().insert(a);
    }
    if adjacency.is_empty() || adjacency.values().any(|neighbours| neighbours.len() != 2) {
        return Err("Stamp selection must have closed, non-branching boundaries.".into());
    }
    let mut remaining = adjacency.keys().copied().collect::<BTreeSet<_>>();
    let mut rings = Vec::new();
    while let Some(start) = remaining.pop_first() {
        let mut ring = vec![points[&start]];
        let mut previous = start;
        let mut current = *adjacency[&start].first().unwrap();
        while current != start {
            if !remaining.remove(&current) {
                return Err("Stamp boundary branches.".into());
            }
            ring.push(points[&current]);
            let next = *adjacency[&current].iter().find(|id| **id != previous).unwrap();
            previous = current;
            current = next;
        }
        if ring.len() < 3 || polygon_area_2d(&ring).abs() < 1e-14 {
            return Err("Stamp boundary has no area.".into());
        }
        let points3 = ring.iter().map(|p| rysmo_math::DVec3::new(p.x, p.y, 0.)).collect::<Vec<_>>();
        rysmo_mesh::offset_polyline(&points3, rysmo_math::DVec3::Z, 0., true)
            .ok_or("Stamp boundary intersects itself.")?;
        rings.push(ring);
    }
    let inside = |point: DVec2, ring: &[DVec2]| {
        let mut result = false;
        for i in 0..ring.len() {
            let a = ring[i];
            let b = ring[(i + 1) % ring.len()];
            if (a.y > point.y) != (b.y > point.y) && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x {
                result = !result;
            }
        }
        result
    };
    let depths = rings
        .iter()
        .enumerate()
        .map(|(i, ring)| rings.iter().enumerate().filter(|(j, other)| *j != i && inside(ring[0], other)).count())
        .collect::<Vec<_>>();
    for (ring, depth) in rings.iter_mut().zip(depths) {
        if (polygon_area_2d(ring) > 0.) != (depth % 2 == 0) {
            ring.reverse();
        }
    }
    Ok(rings)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disjoint_faces_and_projection_frame_are_supported() {
        let mesh = crate::terrain::grid(4., 4., 1.).unwrap();
        let first = mesh.faces().next().unwrap().0;
        let last = mesh.faces().last().unwrap().0;
        let projection = DMat4::from_scale(rysmo_math::DVec3::new(2., 3., 1.));
        let rings = face_boundaries(&mesh, &[first, last, first], projection).unwrap();
        assert_eq!(rings.len(), 2);
        assert!(rings.iter().all(|r| (polygon_area_2d(r) - 3.).abs() < 1e-9));
        assert!(face_boundaries(&mesh, &[FaceId(u64::MAX)], DMat4::IDENTITY).is_err());
        assert!(face_boundaries(&mesh, &[first], DMat4::from_scale(rysmo_math::DVec3::new(0., 1., 1.))).is_err());
    }

    #[test]
    fn boundaries_remove_internal_edges_and_retain_holes_and_islands() {
        let mesh = crate::terrain::grid(4., 4., 1.).unwrap();
        let selected = mesh
            .faces()
            .filter(|(id, _)| {
                let face = mesh.face(*id).unwrap();
                let p = face.outer.iter().map(|id| mesh.pos(*id)).sum::<rysmo_math::DVec3>() / face.outer.len() as f64;
                !(p.x > 1. && p.x < 3. && p.y > 1. && p.y < 3.)
            })
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        let rings = face_boundaries(&mesh, &selected, DMat4::IDENTITY).unwrap();
        assert_eq!(rings.len(), 2);
        let mut areas = rings.iter().map(|r| polygon_area_2d(r)).collect::<Vec<_>>();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas, vec![-4., 16.]);
        assert!(face_boundaries(&mesh, &[], DMat4::IDENTITY).is_err());
    }
}

/// Unions projected polygons, each represented by an outer ring followed by holes.
/// Degenerate rings are ignored so vertical walls do not enlarge a solid's footprint.
pub fn projected_union(polygons: &[Vec<Vec<DVec2>>]) -> Result<Vec<Vec<DVec2>>, String> {
    use spade::{ConstrainedDelaunayTriangulation, Point2, Triangulation};
    let points = polygons.iter().flatten().flatten().copied().collect::<Vec<_>>();
    if points.is_empty() || points.iter().any(|p| !p.is_finite()) {
        return Err("Source has no finite projected faces.".into());
    }
    let lo = points.iter().fold(DVec2::splat(f64::INFINITY), |a, p| a.min(*p));
    let hi = points.iter().fold(DVec2::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
    let scale = (hi - lo).max_element();
    if !scale.is_finite() || scale <= 0. {
        return Err("Source has no projected area.".into());
    }
    let polygons = polygons
        .iter()
        .filter_map(|polygon| {
            let rings = polygon
                .iter()
                .map(|ring| ring.iter().map(|p| (*p - lo) / scale).collect::<Vec<_>>())
                .collect::<Vec<_>>();
            if rings.first().is_some_and(|ring| ring.len() >= 3 && polygon_area_2d(ring).abs() > 1e-14) {
                Some(rings)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
    for polygon in &polygons {
        for ring in polygon {
            if ring.len() < 3 || polygon_area_2d(ring).abs() < 1e-14 {
                continue;
            }
            let handles = ring
                .iter()
                .map(|p| cdt.insert(Point2::new(p.x, p.y)).map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            for i in 0..handles.len() {
                if handles[i] != handles[(i + 1) % handles.len()] {
                    cdt.add_constraint_and_split(handles[i], handles[(i + 1) % handles.len()], |p| p);
                }
            }
        }
    }
    let contains = |point: DVec2, ring: &[DVec2]| {
        let mut inside = false;
        for i in 0..ring.len() {
            let a = ring[i];
            let b = ring[(i + 1) % ring.len()];
            if (a.y > point.y) != (b.y > point.y) && point.x < (b.x - a.x) * (point.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
        }
        inside
    };
    let filled = cdt
        .inner_faces()
        .filter_map(|face| {
            let centre = face.vertices().iter().fold(DVec2::ZERO, |a, v| {
                let p = v.position();
                a + DVec2::new(p.x, p.y)
            }) / 3.;
            polygons
                .iter()
                .any(|polygon| contains(centre, &polygon[0]) && !polygon[1..].iter().any(|hole| contains(centre, hole)))
                .then_some(face.fix().index())
        })
        .collect::<BTreeSet<_>>();
    let mut next = BTreeMap::new();
    for edge in cdt.undirected_edges() {
        let edge = edge.as_directed();
        let left = edge.face().as_inner().is_some_and(|f| filled.contains(&f.fix().index()));
        let right = edge.rev().face().as_inner().is_some_and(|f| filled.contains(&f.fix().index()));
        if left == right {
            continue;
        }
        let edge = if left { edge } else { edge.rev() };
        let a = edge.from().fix().index();
        let b = edge.to().fix().index();
        if next.insert(a, b).is_some() {
            return Err("Projected source has touching boundary branches.".into());
        }
    }
    let mut result = Vec::new();
    while let Some((&start, _)) = next.first_key_value() {
        let mut current = start;
        let mut ring = Vec::new();
        loop {
            let point = cdt.vertex(spade::handles::FixedVertexHandle::from_index(current)).position();
            ring.push(lo + DVec2::new(point.x, point.y) * scale);
            current = next.remove(&current).ok_or("Projected boundary is open.")?;
            if current == start {
                break;
            }
        }
        result.push(ring);
    }
    if result.is_empty() {
        return Err("Source has no projected area.".into());
    }
    Ok(result)
}

#[cfg(test)]
mod union_tests {
    use super::*;
    fn rectangle(x: f64, y: f64, width: f64, height: f64) -> Vec<DVec2> {
        vec![DVec2::new(x, y), DVec2::new(x + width, y), DVec2::new(x + width, y + height), DVec2::new(x, y + height)]
    }
    #[test]
    fn union_removes_overlaps_duplicate_caps_and_vertical_walls() {
        let a = rectangle(0., 0., 3., 2.);
        let mut reversed = a.clone();
        reversed.reverse();
        let b = rectangle(2., 1., 3., 2.);
        let wall = vec![DVec2::ZERO, DVec2::X, DVec2::X, DVec2::ZERO];
        let loops = projected_union(&[vec![a], vec![reversed], vec![b], vec![wall]]).unwrap();
        assert_eq!(loops.len(), 1);
        assert!((polygon_area_2d(&loops[0]) - 11.).abs() < 1e-9);
    }
    #[test]
    fn union_retains_courtyard_and_disjoint_regions() {
        let loops = projected_union(&[
            vec![rectangle(0., 0., 6., 6.), rectangle(2., 2., 2., 2.)],
            vec![rectangle(8., 0., 2., 2.)],
        ])
        .unwrap();
        let mut areas = loops.iter().map(|r| polygon_area_2d(r)).collect::<Vec<_>>();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas, vec![-4., 4., 36.]);
        assert!(projected_union(&[]).is_err());
        assert!(projected_union(&[vec![vec![DVec2::splat(f64::NAN)]]]).is_err());
    }
}

/// Projects selected faces and closed instances from the owner's coordinates.
/// Child-instance transforms are composed recursively; source geometry is read-only.
pub fn source_boundaries(
    doc: &crate::Document,
    owner: Option<rysmo_mesh::DefinitionId>,
    elements: &[rysmo_mesh::Element],
    projection: DMat4,
) -> Result<Vec<Vec<DVec2>>, String> {
    source_footprint(doc, owner, elements, projection).map(|(rings, _)| rings)
}

/// Returns projected union boundaries and the lowest source elevation in that frame.
pub fn source_footprint(
    doc: &crate::Document,
    owner: Option<rysmo_mesh::DefinitionId>,
    elements: &[rysmo_mesh::Element],
    projection: DMat4,
) -> Result<(Vec<Vec<DVec2>>, f64), String> {
    use rysmo_mesh::{DefinitionId, Element};
    fn append_faces(
        definition: &crate::document::Definition,
        faces: impl IntoIterator<Item = FaceId>,
        projection: DMat4,
        polygons: &mut Vec<Vec<Vec<DVec2>>>,
        elevation: &mut f64,
    ) -> Result<(), String> {
        for id in faces {
            let face = definition.mesh.face(id).ok_or("Source face is missing.")?;
            let polygon = std::iter::once(&face.outer)
                .chain(face.holes.iter())
                .map(|ring| {
                    ring.iter()
                        .map(|id| {
                            let point = projection.transform_point3(definition.mesh.pos(*id));
                            if !point.is_finite() {
                                return Err("Source projection is not finite.");
                            }
                            *elevation = elevation.min(point.z);
                            Ok(DVec2::new(point.x, point.y))
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(str::to_owned)?;
            polygons.push(polygon);
        }
        Ok(())
    }
    fn append_definition(
        doc: &crate::Document,
        id: DefinitionId,
        projection: DMat4,
        stack: &mut BTreeSet<DefinitionId>,
        polygons: &mut Vec<Vec<Vec<DVec2>>>,
        elevation: &mut f64,
    ) -> Result<(), String> {
        if stack.len() >= 128 || !stack.insert(id) {
            return Err("Source component hierarchy is cyclic or too deep.".into());
        }
        let definition = doc.def(Some(id)).ok_or("Source definition is missing.")?;
        append_faces(definition, definition.mesh.faces().map(|(id, _)| id), projection, polygons, elevation)?;
        for instance in definition.instances.values() {
            append_definition(doc, instance.definition, projection * instance.transform, stack, polygons, elevation)?;
        }
        stack.remove(&id);
        Ok(())
    }
    if !projection.is_finite() {
        return Err("Source projection is not finite.".into());
    }
    let definition = doc.def(owner).ok_or("Source context is missing.")?;
    let mut polygons = Vec::new();
    let mut elevation = f64::INFINITY;
    let mut stack = BTreeSet::new();
    for element in elements.iter().copied().collect::<BTreeSet<_>>() {
        match element {
            Element::Face(id) => append_faces(definition, [id], projection, &mut polygons, &mut elevation)?,
            Element::Instance(id) => {
                let instance = definition.instances.get(&id).ok_or("Source instance is missing.")?;
                append_definition(
                    doc,
                    instance.definition,
                    projection * instance.transform,
                    &mut stack,
                    &mut polygons,
                    &mut elevation,
                )?;
            }
            _ => {}
        }
    }
    Ok((projected_union(&polygons)?, elevation))
}

#[cfg(test)]
mod source_tests {
    use super::*;
    use crate::document::{Definition, Instance};
    use rysmo_math::DVec3;
    use rysmo_mesh::{DefinitionId, Element, InstanceId};
    fn instance(definition: DefinitionId, transform: DMat4) -> Instance {
        Instance {
            glued_to: None,
            definition,
            transform,
            name: String::new(),
            tag: None,
            material: None,
            hidden: false,
        }
    }
    #[test]
    fn nested_instances_compose_transforms_and_leave_source_unchanged() {
        let mut doc = crate::Document::default();
        let child = DefinitionId(1000);
        let parent = DefinitionId(1001);
        doc.definitions
            .insert(child, Definition { mesh: crate::terrain::grid(2., 1., 1.).unwrap(), ..Default::default() });
        let mut container = Definition::default();
        container.instances.insert(InstanceId(1002), instance(child, DMat4::from_translation(DVec3::new(3., 4., 5.))));
        doc.definitions.insert(parent, container);
        doc.root.instances.insert(InstanceId(1003), instance(parent, DMat4::from_scale(DVec3::new(-2., 3., 1.))));
        doc.root.instances.insert(InstanceId(1004), instance(parent, DMat4::from_translation(DVec3::new(20., 0., 0.))));
        let before = doc.clone();
        let rings = source_boundaries(
            &doc,
            None,
            &[Element::Instance(InstanceId(1003)), Element::Instance(InstanceId(1004))],
            DMat4::IDENTITY,
        )
        .unwrap();
        let mut areas = rings.iter().map(|ring| polygon_area_2d(ring)).collect::<Vec<_>>();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas, vec![2., 12.]);
        let lo = rings.iter().flatten().fold(DVec2::splat(f64::INFINITY), |a, p| a.min(*p));
        assert_eq!(lo, DVec2::new(-10., 4.));
        let (_, elevation) = source_footprint(
            &doc,
            None,
            &[Element::Instance(InstanceId(1003)), Element::Instance(InstanceId(1004))],
            DMat4::from_translation(DVec3::Z * 10.),
        )
        .unwrap();
        assert_eq!(elevation, 15.);
        let segments = source_segments(
            &doc,
            None,
            &[Element::Instance(InstanceId(1003)), Element::Instance(InstanceId(1004))],
            DMat4::from_translation(DVec3::Z * 10.),
        )
        .unwrap();
        assert_eq!(segments.len(), 2 * doc.definitions[&child].mesh.edges().count());
        assert!(segments.iter().flatten().all(|p| p.z == 15.));
        assert!(segments.iter().flatten().any(|p| p.x == -10.));

        assert_eq!(doc, before);
        assert!(source_boundaries(&doc, None, &[Element::Instance(InstanceId(99999))], DMat4::IDENTITY).is_err());
        doc.definitions.get_mut(&child).unwrap().instances.insert(InstanceId(1005), instance(parent, DMat4::IDENTITY));
        assert!(source_boundaries(&doc, None, &[Element::Instance(InstanceId(1003))], DMat4::IDENTITY).is_err());
    }
}

/// Collects source edge segments, composing nested instance transforms into a frame.
pub fn source_segments(
    doc: &crate::Document,
    owner: Option<rysmo_mesh::DefinitionId>,
    elements: &[rysmo_mesh::Element],
    projection: DMat4,
) -> Result<Vec<[rysmo_math::DVec3; 2]>, String> {
    use rysmo_mesh::{DefinitionId, EdgeId, Element};
    fn append_edges(
        definition: &crate::document::Definition,
        edges: impl IntoIterator<Item = EdgeId>,
        projection: DMat4,
        lines: &mut Vec<[rysmo_math::DVec3; 2]>,
    ) -> Result<(), String> {
        for id in edges {
            let edge = definition.mesh.edge(id).ok_or("Source edge is missing.")?;
            let line = edge.v.map(|id| projection.transform_point3(definition.mesh.pos(id)));
            if line.iter().any(|p| !p.is_finite()) {
                return Err("Source projection is not finite.".into());
            }
            lines.push(line);
        }
        Ok(())
    }
    fn append_definition(
        doc: &crate::Document,
        id: DefinitionId,
        projection: DMat4,
        stack: &mut BTreeSet<DefinitionId>,
        lines: &mut Vec<[rysmo_math::DVec3; 2]>,
    ) -> Result<(), String> {
        if stack.len() >= 128 || !stack.insert(id) {
            return Err("Source hierarchy is cyclic or too deep.".into());
        }
        let definition = doc.def(Some(id)).ok_or("Source definition is missing.")?;
        append_edges(definition, definition.mesh.edges().map(|(id, _)| id), projection, lines)?;
        for instance in definition.instances.values() {
            append_definition(doc, instance.definition, projection * instance.transform, stack, lines)?;
        }
        stack.remove(&id);
        Ok(())
    }
    if !projection.is_finite() {
        return Err("Source projection is not finite.".into());
    }
    let definition = doc.def(owner).ok_or("Source context is missing.")?;
    let mut edges = BTreeSet::new();
    let mut face_edge_counts = BTreeMap::new();
    let mut lines = Vec::new();
    let mut stack = BTreeSet::new();
    for element in elements.iter().copied().collect::<BTreeSet<_>>() {
        match element {
            Element::Edge(id) => {
                edges.insert(id);
            }
            Element::Face(id) => {
                if definition.mesh.face(id).is_none() {
                    return Err("Source face is missing.".into());
                }
                for edge in definition.mesh.face_edges(id) {
                    *face_edge_counts.entry(edge).or_insert(0usize) += 1;
                }
            }
            Element::Instance(id) => {
                let instance = definition.instances.get(&id).ok_or("Source instance is missing.")?;
                append_definition(doc, instance.definition, projection * instance.transform, &mut stack, &mut lines)?;
            }
            _ => {}
        }
    }
    edges.extend(face_edge_counts.into_iter().filter_map(|(edge, count)| (count == 1).then_some(edge)));
    append_edges(definition, edges, projection, &mut lines)?;
    Ok(lines)
}

#[cfg(test)]
mod drape_face_source_tests {
    use super::*;
    #[test]
    fn draped_face_series_creates_only_its_outline_on_terrain() {
        use crate::{Command, Editor};
        use rysmo_math::DVec3;
        use rysmo_mesh::Element;
        let mut document = crate::Document::default();
        document.root.mesh = crate::terrain::grid(2., 2., 2.).unwrap();
        document.root.mesh.map_all(|p| p + DVec3::new(1., 1., 4.));
        document.next_id = document.root.mesh.next_id();
        let mut editor = Editor::new();
        editor.replace_document(document);
        let sources = editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
        let out = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X * 4., v: DVec3::Y * 4., spacing: 1. })
            .unwrap();
        let Element::Instance(target) = out.created[0] else { panic!() };
        let before = editor.doc().clone();
        editor.apply(Command::DrapeSources { sources, target }).unwrap();
        let projected = editor
            .mesh()
            .edges()
            .filter(|(_, edge)| edge.v.iter().all(|id| editor.mesh().pos(*id).z == 0.))
            .collect::<Vec<_>>();
        assert!(!projected.is_empty());
        assert!(projected.iter().all(|(_, edge)| {
            let [a, b] = edge.v.map(|id| editor.mesh().pos(id));
            a.x == b.x || a.y == b.y
        }));
        assert_eq!(editor.doc().definitions, before.definitions);
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }

    #[test]
    fn selected_face_set_excludes_internal_edges_but_explicit_edges_remain() {
        use rysmo_mesh::Element;
        let mut document = crate::Document::default();
        document.root.mesh = crate::terrain::grid(2., 2., 2.).unwrap();
        let faces = document.root.mesh.faces().map(|(id, _)| Element::Face(id)).collect::<Vec<_>>();
        assert_eq!(faces.len(), 2);
        let lines = source_segments(&document, None, &faces, DMat4::IDENTITY).unwrap();
        assert_eq!(lines.len(), 4);
        assert!(lines.iter().all(|[a, b]| a.x == b.x || a.y == b.y));
        let diagonal = document.root.mesh.edges().find(|(_, edge)| edge.soft).unwrap().0;
        let mut selected = faces.clone();
        selected.push(Element::Edge(diagonal));
        let lines = source_segments(&document, None, &selected, DMat4::IDENTITY).unwrap();
        assert_eq!(lines.len(), 5);
    }
}
