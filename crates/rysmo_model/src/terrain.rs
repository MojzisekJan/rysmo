//! Triangulated terrain geometry shared by the Sandbox tools.
use rysmo_math::{DVec2, DVec3};
use rysmo_mesh::{Edge, EdgeId, Face, FaceId, Mesh, Vertex, VertexId};
use std::collections::BTreeMap;

/// Builds an XY grid. Boundary cells retain the exact requested dimensions;
/// internal diagonals are soft/smooth, while square grid edges remain visible.
pub fn grid(length: f64, width: f64, spacing: f64) -> Result<Mesh, String> {
    grid_with_ids(length, width, spacing, 1)
}
pub(crate) fn grid_with_ids(length: f64, width: f64, spacing: f64, first_id: u64) -> Result<Mesh, String> {
    if [length, width, spacing].iter().any(|v| !v.is_finite() || *v < 1e-5) {
        return Err("Terrain dimensions and grid spacing must be positive.".into());
    }
    let count = |extent: f64| {
        let ratio = extent / spacing;
        if (ratio - ratio.round()).abs() < 1e-10 { ratio.round().max(1.) } else { ratio.ceil() }
    };
    let columns = count(length);
    let rows = count(width);
    if !columns.is_finite() || !rows.is_finite() || columns * rows > 131072. {
        return Err("Terrain grid is too dense; increase grid spacing.".into());
    }
    let columns = columns as usize;
    let rows = rows as usize;
    let mut vertices = BTreeMap::new();
    let mut edges = BTreeMap::new();
    let mut faces = BTreeMap::new();
    let mut next = first_id.max(1);
    let mut ids = Vec::with_capacity((rows + 1) * (columns + 1));
    for row in 0..=rows {
        for column in 0..=columns {
            let id = VertexId(next);
            next += 1;
            vertices.insert(
                id,
                Vertex {
                    pos: DVec3::new(
                        if column == columns { length } else { column as f64 * spacing },
                        if row == rows { width } else { row as f64 * spacing },
                        0.,
                    ),
                },
            );
            ids.push(id);
        }
    }
    let mut edge_ids = BTreeMap::new();
    for row in 0..rows {
        for column in 0..columns {
            let a = ids[row * (columns + 1) + column];
            let b = ids[row * (columns + 1) + column + 1];
            let c = ids[(row + 1) * (columns + 1) + column + 1];
            let d = ids[(row + 1) * (columns + 1) + column];
            for triangle in [[a, b, c], [a, c, d]] {
                for (start, end) in [(triangle[0], triangle[1]), (triangle[1], triangle[2]), (triangle[2], triangle[0])]
                {
                    let key = if start < end { (start, end) } else { (end, start) };
                    if let std::collections::btree_map::Entry::Vacant(entry) = edge_ids.entry(key) {
                        let id = EdgeId(next);
                        next += 1;
                        entry.insert(id);
                        let mut edge = Edge::new(start, end);
                        edge.soft = key == (a, c);
                        edge.smooth = edge.soft;
                        edges.insert(id, edge);
                    }
                }
                faces.insert(
                    FaceId(next),
                    Face {
                        hidden: false,
                        outer: triangle.to_vec(),
                        holes: vec![],
                        material: None,
                        back_material: None,
                        uv: None,
                    },
                );
                next += 1;
            }
        }
    }
    Mesh::from_parts(vertices, edges, faces).map_err(|e| e.to_string())
}

#[derive(Debug)]
struct SeedNode {
    point: DVec2,
    left: Option<Box<SeedNode>>,
    right: Option<Box<SeedNode>>,
    axis: usize,
}
impl SeedNode {
    fn build(points: &mut [DVec2], depth: usize) -> Option<Box<Self>> {
        if points.is_empty() {
            return None;
        }
        let axis = depth % 2;
        let middle = points.len() / 2;
        points.select_nth_unstable_by(middle, |a, b| a[axis].total_cmp(&b[axis]));
        let (left, rest) = points.split_at_mut(middle);
        let (point, right) = rest.split_first_mut().unwrap();
        Some(Box::new(Self {
            point: *point,
            left: Self::build(left, depth + 1),
            right: Self::build(right, depth + 1),
            axis,
        }))
    }
    fn nearest(&self, point: DVec2, best: &mut f64) {
        *best = best.min(self.point.distance_squared(point));
        let delta = point[self.axis] - self.point[self.axis];
        let (near, far) = if delta < 0. { (&self.left, &self.right) } else { (&self.right, &self.left) };
        if let Some(node) = near {
            node.nearest(point, best);
        }
        if delta * delta < *best
            && let Some(node) = far
        {
            node.nearest(point, best);
        }
    }
}

/// Reusable nearest-centre index for deformation and brush highlighting.
pub struct SmooveInfluence {
    seed_positions: std::collections::HashSet<(u64, u64, u64)>,
    centres: Vec<DVec3>,
    normal: DVec3,
    origin: DVec3,
    u: DVec3,
    v: DVec3,
    tree: Option<Box<SeedNode>>,
}
impl SmooveInfluence {
    pub fn new(centres: &[DVec3], normal: DVec3) -> Result<Self, String> {
        if centres.is_empty() || centres.iter().any(|p| !p.is_finite()) {
            return Err("Invalid Smoove centres.".into());
        }
        let normal = normal.try_normalize().ok_or("Invalid Smoove normal.")?;
        let axis = if normal.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
        let u = normal.cross(axis).normalize();
        let v = normal.cross(u);
        let origin = centres[0];
        let mut projected = centres
            .iter()
            .map(|p| {
                let delta = *p - origin;
                DVec2::new(delta.dot(u), delta.dot(v))
            })
            .collect::<Vec<_>>();
        projected.sort_unstable_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        projected.dedup();
        let tree = if projected.iter().all(|p| p.is_finite()) { SeedNode::build(&mut projected, 0) } else { None };
        let seed_positions = centres.iter().map(|p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits())).collect();
        Ok(Self { seed_positions, centres: centres.to_vec(), normal, origin, u, v, tree })
    }
    pub fn is_seed(&self, point: DVec3) -> bool {
        self.seed_positions.contains(&(point.x.to_bits(), point.y.to_bits(), point.z.to_bits()))
    }
    pub fn weight(&self, point: DVec3, radius: f64) -> f64 {
        if !radius.is_finite() || radius <= 0. || !point.is_finite() {
            return 0.;
        }
        if self.seed_positions.contains(&(point.x.to_bits(), point.y.to_bits(), point.z.to_bits())) {
            return 1.;
        }
        let delta = point - self.origin;
        let projected = DVec2::new(delta.dot(self.u), delta.dot(self.v));
        let distance = if projected.is_finite()
            && let Some(tree) = &self.tree
        {
            let mut squared = radius * radius;
            tree.nearest(projected, &mut squared);
            squared.sqrt()
        } else {
            self.centres
                .iter()
                .map(|centre| {
                    let delta = point - *centre;
                    (delta - self.normal * delta.dot(self.normal)).length()
                })
                .fold(f64::INFINITY, f64::min)
        };
        if distance >= radius { 0. } else { (1. + (std::f64::consts::PI * distance / radius).cos()) * 0.5 }
    }
}

/// Radial terrain deformation. The influence falls smoothly to zero at the rim;
/// multiple centres use their strongest influence, preserving selected ridges.
pub fn smoove(
    mesh: &mut Mesh,
    vertices: &[VertexId],
    centres: &[DVec3],
    radius: f64,
    normal: DVec3,
    offset: DVec3,
) -> Result<bool, String> {
    if vertices.is_empty()
        || centres.is_empty()
        || !radius.is_finite()
        || radius < 1e-5
        || !normal.is_finite()
        || normal.length() < 1e-7
        || !offset.is_finite()
        || centres.iter().any(|p| !p.is_finite())
    {
        return Err("Invalid Smoove radius, direction or selection.".into());
    }
    let normal = normal.try_normalize().ok_or("Invalid Smoove normal.")?;
    let eligible = vertices.iter().copied().collect::<std::collections::BTreeSet<_>>();
    let mut triangulated = std::collections::BTreeSet::new();
    let mut blocked = std::collections::BTreeSet::new();
    for (_, face) in mesh.faces() {
        if face.outer.len() == 3 && face.holes.is_empty() {
            triangulated.extend(face.outer.iter().copied());
        } else {
            blocked.extend(face.loops().flatten().copied());
        }
    }
    for vertex in &eligible {
        if mesh.vertex(*vertex).is_none() {
            return Err("Smoove vertex is missing.".into());
        }
        if !triangulated.contains(vertex) || blocked.contains(vertex) {
            return Err("Smoove requires triangulated terrain.".into());
        }
    }
    let influence = SmooveInfluence::new(centres, normal)?;
    let weight = |point| influence.weight(point, radius);
    if offset.length() < 1e-10 {
        return Ok(false);
    }
    let affected =
        eligible.into_iter().filter(|id| weight(mesh.pos(*id)) > 1e-12).collect::<std::collections::BTreeSet<_>>();
    if affected.is_empty() {
        return Ok(false);
    }
    if affected.iter().any(|id| !(mesh.pos(*id) + offset * weight(mesh.pos(*id))).is_finite()) {
        return Err("Smoove displacement exceeds the coordinate range.".into());
    }
    mesh.deform_vertices(&affected, |point| point + offset * weight(point)).map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detail_split_edge_text_keeps_fallback_and_recovers_through_undo() {
        use crate::dimensions::{PointEntity, PointReference};
        use crate::text::{Text, TextAnchor, TextEndpoint, TextLeader, TextPlacement, TextReference};
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, spacing: 1. })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        let edge = editor.mesh().edges().find(|(_, e)| e.soft).unwrap().0;
        let (a, b) = editor.mesh().edge_points(edge);
        let point = a.lerp(b, 0.25);
        let text = Text {
            content: "Edge".into(),
            placement: TextPlacement::Attached {
                anchor: TextAnchor {
                    point,
                    reference: Some(TextReference::Point {
                        reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.25 } },
                    }),
                },
                leader: TextLeader::None { offset: DVec3::ZERO },
            },
            font: Default::default(),
            endpoint: TextEndpoint::None,
        };
        let id = editor.apply(Command::AddText { text }).unwrap().texts[0];
        let before = editor.doc().clone();
        let face = editor.mesh().faces().next().unwrap().0;
        editor.apply(Command::AddTerrainDetail { face, point: a.lerp(b, 0.5), offset: DVec3::Z * 0.5 }).unwrap();
        let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&id].placement else { panic!() };
        assert_eq!(anchor.resolve(editor.doc(), editor.active_definition()), (point, false));
        editor.undo();
        assert_eq!(editor.doc(), &before);
        let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&id].placement else { panic!() };
        assert_eq!(anchor.resolve(editor.doc(), editor.active_definition()), (point, true));
    }
    #[test]
    fn detail_vertex_text_tracks_correction_and_exact_history() {
        use crate::dimensions::{PointEntity, PointReference};
        use crate::text::{Text, TextAnchor, TextEndpoint, TextLeader, TextPlacement, TextReference};
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::new(2., 3., 4.), u: DVec3::X, v: DVec3::Y, spacing: 1. })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        let vertex = editor.mesh().find_vertex(DVec3::ZERO).unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let text = Text {
            content: "Corner".into(),
            placement: TextPlacement::Attached {
                anchor: TextAnchor {
                    point: DVec3::ZERO,
                    reference: Some(TextReference::Point {
                        reference: PointReference { path: vec![], entity: PointEntity::Vertex { vertex } },
                    }),
                },
                leader: TextLeader::Pushpin { offset: DVec3::Y },
            },
            font: Default::default(),
            endpoint: TextEndpoint::ClosedArrow,
        };
        let id = editor.apply(Command::AddText { text }).unwrap().texts[0];
        let before = editor.doc().clone();
        let command = |height| Command::AddTerrainDetail { face, point: DVec3::ZERO, offset: DVec3::Z * height };
        editor.apply(command(0.5)).unwrap();
        editor.amend_last(command(1.)).unwrap();
        let TextPlacement::Attached { anchor, leader } = &editor.active().texts[&id].placement else { panic!() };
        assert_eq!(anchor.resolve(editor.doc(), editor.active_definition()), (DVec3::Z, true));
        assert_eq!(anchor.point, DVec3::Z);
        assert_eq!(leader, &TextLeader::Pushpin { offset: DVec3::Y });
        let corrected = editor.doc().clone();
        editor.undo();
        assert_eq!(editor.doc(), &before);
        editor.redo();
        assert_eq!(editor.doc(), &corrected);
        editor.amend_last(command(0.)).unwrap();
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn add_detail_splits_face_or_shared_edge_and_undoes_exactly() {
        use crate::{Command, Editor, Element};
        for on_edge in [false, true] {
            let mut editor = Editor::new();
            let out = editor
                .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, spacing: 1. })
                .unwrap();
            let Element::Instance(instance) = out.created[0] else { panic!() };
            editor.enter(instance).unwrap();
            let face = editor.mesh().faces().next().unwrap().0;
            let point = if on_edge { DVec3::new(0.5, 0.5, 0.) } else { DVec3::new(0.7, 0.2, 0.) };
            let before = editor.doc().clone();
            editor.apply(Command::AddTerrainDetail { face, point, offset: DVec3::Z * 0.5 }).unwrap();
            assert_eq!(editor.mesh().vertices().count(), 5);
            assert_eq!(editor.mesh().faces().count(), 4);
            assert!(editor.mesh().find_vertex(point + DVec3::Z * 0.5).is_some());
            assert!(editor.mesh().faces().all(|(id, _)| editor.mesh().face_normal(id).unwrap().z > 0.));
            editor.mesh().validate().unwrap();
            editor.undo();
            assert_eq!(editor.doc(), &before);
        }
    }
    #[test]
    fn add_detail_existing_vertex_noop_and_invalid_point_are_atomic() {
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, spacing: 1. })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let before = editor.doc().clone();
        let revision = editor.revision();
        assert!(
            !editor.apply(Command::AddTerrainDetail { face, point: DVec3::ZERO, offset: DVec3::ZERO }).unwrap().changed
        );
        assert_eq!(editor.revision(), revision);
        assert_eq!(editor.doc(), &before);
        assert!(
            editor.apply(Command::AddTerrainDetail { face, point: DVec3::new(3., 3., 0.), offset: DVec3::Z }).is_err()
        );
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn flipped_edge_text_keeps_fallback_and_undo_restores_its_link() {
        use crate::dimensions::{PointEntity, PointReference};
        use crate::text::{Text, TextAnchor, TextEndpoint, TextLeader, TextPlacement, TextReference};
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, spacing: 1. })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        let edge = editor.mesh().edges().find(|(_, e)| e.soft).unwrap().0;
        let (a, b) = editor.mesh().edge_points(edge);
        let point = a.lerp(b, 0.25);
        let text = Text {
            content: "Diagonal".into(),
            placement: TextPlacement::Attached {
                anchor: TextAnchor {
                    point,
                    reference: Some(TextReference::Point {
                        reference: PointReference { path: vec![], entity: PointEntity::Edge { edge, parameter: 0.25 } },
                    }),
                },
                leader: TextLeader::ViewBased { offset: DVec2::new(40., -20.) },
            },
            font: Default::default(),
            endpoint: TextEndpoint::ClosedArrow,
        };
        let id = editor.apply(Command::AddText { text }).unwrap().texts[0];
        let before = editor.doc().clone();
        editor.apply(Command::FlipTerrainEdge { edge }).unwrap();
        let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&id].placement else { panic!() };
        assert_eq!(anchor.resolve(editor.doc(), editor.active_definition()), (point, false));
        editor.undo();
        assert_eq!(editor.doc(), &before);
        let TextPlacement::Attached { anchor, .. } = &editor.active().texts[&id].placement else { panic!() };
        assert_eq!(anchor.resolve(editor.doc(), editor.active_definition()), (point, true));
    }
    #[test]
    fn flip_edge_replaces_diagonal_preserves_faces_flags_and_undo() {
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, spacing: 1. })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        editor.enter(instance).unwrap();
        let diagonal = editor.mesh().edges().find(|(_, edge)| edge.soft).unwrap().0;
        let old_vertices = editor.mesh().edge(diagonal).unwrap().v;
        let faces = editor.mesh().faces().map(|(id, _)| id).collect::<Vec<_>>();
        let before = editor.doc().clone();
        let out = editor.apply(Command::FlipTerrainEdge { edge: diagonal }).unwrap();
        let Element::Edge(new_edge) = out.created[0] else { panic!() };
        assert_ne!(new_edge, diagonal);
        assert!(editor.mesh().edge(diagonal).is_none());
        assert!(editor.mesh().edge(new_edge).unwrap().soft);
        assert!(editor.mesh().edge(new_edge).unwrap().v.iter().all(|id| !old_vertices.contains(id)));
        assert_eq!(editor.mesh().faces().map(|(id, _)| id).collect::<Vec<_>>(), faces);
        assert!(editor.mesh().faces().all(|(id, _)| editor.mesh().face_normal(id).unwrap().z > 0.));
        editor.undo();
        assert_eq!(editor.doc(), &before);
        let boundary = editor.mesh().edges().find(|(_, edge)| !edge.soft).unwrap().0;
        assert!(editor.apply(Command::FlipTerrainEdge { edge: boundary }).is_err());
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn external_face_text_tracks_smoove_correction_undo_and_zero_restore() {
        use crate::text::{Text, TextAnchor, TextEndpoint, TextLeader, TextPlacement};
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let origin = DVec3::new(4., 3., 2.);
        let out = editor
            .apply(Command::CreateTerrainGrid { origin, u: DVec3::X * 4., v: DVec3::Y * 4., spacing: 1. })
            .unwrap();
        let Element::Instance(instance) = out.created[0] else { panic!() };
        let definition = editor.active().instances[&instance].definition;
        let mesh = &editor.doc().definitions[&definition].mesh;
        let peak = mesh.find_vertex(DVec3::new(2., 2., 0.)).unwrap();
        let (face, triangle) = mesh.faces().find(|(_, f)| f.outer.contains(&peak)).unwrap();
        let frame = triangle.outer.clone();
        let point = frame.iter().map(|id| mesh.pos(*id)).sum::<DVec3>() / 3.;
        let anchor = TextAnchor::on_face(editor.doc(), None, vec![instance], face, origin + point).unwrap();
        let text = Text {
            content: "Terrain".into(),
            placement: TextPlacement::Attached { anchor, leader: TextLeader::Pushpin { offset: DVec3::Z } },
            font: Default::default(),
            endpoint: TextEndpoint::ClosedArrow,
        };
        let text_id = editor.apply(Command::AddText { text }).unwrap().texts[0];
        use crate::dimensions::{Dimension, DimensionAnchor, DimensionMeasure, PointEntity, PointReference};
        let neighbour = *frame.iter().find(|id| **id != peak).unwrap();
        let anchor = |vertex| DimensionAnchor {
            point: origin + editor.doc().definitions[&definition].mesh.pos(vertex),
            reference: Some(PointReference { path: vec![instance], entity: PointEntity::Vertex { vertex } }),
        };
        let dimension = Dimension {
            measure: DimensionMeasure::Linear { from: anchor(peak), to: anchor(neighbour), axis: None },
            normal: DVec3::Z,
            offset: DVec3::Y,
            text: String::new(),
            alignment: Default::default(),
            font: Default::default(),
            text_position: Default::default(),
            endpoints: Default::default(),
        };
        let dim_id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
        editor.enter(instance).unwrap();
        let before = editor.doc().clone();
        let vertices = editor.mesh().vertices().map(|(id, _)| id).collect::<Vec<_>>();
        let command = |height| Command::Smoove {
            vertices: vertices.clone(),
            centres: vec![DVec3::new(2., 2., 0.)],
            radius: 2.,
            normal: DVec3::Z,
            offset: DVec3::Z * height,
        };
        editor.apply(command(1.)).unwrap();
        editor.amend_last(command(2.)).unwrap();
        let expected = origin + frame.iter().map(|id| editor.mesh().pos(*id)).sum::<DVec3>() / 3.;
        let TextPlacement::Attached { anchor, leader } = &editor.doc().root.texts[&text_id].placement else { panic!() };
        let (actual, linked) = anchor.resolve(editor.doc(), None);
        assert!(linked);
        assert!(actual.distance(expected) < 1e-9);
        assert!(anchor.point.distance(expected) < 1e-9);
        assert_eq!(leader, &TextLeader::Pushpin { offset: DVec3::Z });
        let resolved = editor.doc().root.dimensions[&dim_id].resolve(editor.doc(), None);
        assert!(resolved.associated);
        assert!(resolved.from.distance(origin + editor.mesh().pos(peak)) < 1e-9);
        assert!(resolved.to.distance(origin + editor.mesh().pos(neighbour)) < 1e-9);
        assert!((resolved.length - editor.mesh().pos(peak).distance(editor.mesh().pos(neighbour))).abs() < 1e-9);
        let corrected = editor.doc().clone();
        editor.undo();
        assert_eq!(editor.doc(), &before);
        editor.redo();
        assert_eq!(editor.doc(), &corrected);
        editor.amend_last(command(0.)).unwrap();
        assert_eq!(editor.doc(), &before);
        let TextPlacement::Attached { anchor, .. } = &editor.doc().root.texts[&text_id].placement else { panic!() };
        assert_eq!(anchor.resolve(editor.doc(), None), (origin + point, true));
    }
    #[test]
    fn influence_index_matches_non_axis_aligned_projection_and_duplicates() {
        let normal = DVec3::new(1., 2., 3.).normalize();
        let centres = vec![DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::X];
        let influence = SmooveInfluence::new(&centres, normal).unwrap();
        for point in [DVec3::new(2., 1., 0.5), DVec3::new(-1., 0.25, 0.75), DVec3::new(4., 3., 2.)] {
            let distance = centres
                .iter()
                .map(|c| {
                    let d = point - *c;
                    (d - normal * d.dot(normal)).length()
                })
                .fold(f64::INFINITY, f64::min);
            let radius = 1.75;
            let expected =
                if distance >= radius { 0. } else { (1. + (std::f64::consts::PI * distance / radius).cos()) * 0.5 };
            assert!((influence.weight(point, radius) - expected).abs() < 1e-12);
        }
        assert_eq!(influence.weight(DVec3::ZERO, 0.), 0.);
    }
    #[test]
    fn partial_selection_tree_matches_exhaustive_falloff() {
        let mut mesh = grid(100., 100., 1.).unwrap();
        let original = mesh.clone();
        let vertices = mesh.vertices().map(|(id, _)| id).collect::<Vec<_>>();
        let centres = mesh
            .vertices()
            .filter(|(_, v)| v.pos.x < 50. && v.pos.y % 3. == 0.)
            .map(|(_, v)| v.pos)
            .collect::<Vec<_>>();
        let radius = 3.25;
        let started = std::time::Instant::now();
        smoove(&mut mesh, &vertices, &centres, radius, DVec3::Z, DVec3::Z * 0.75).unwrap();
        eprintln!("Smoove indexed: {} vertices, {} centres, {:?}", vertices.len(), centres.len(), started.elapsed());
        for (id, vertex) in original.vertices() {
            let distance = centres.iter().map(|c| vertex.pos.distance(*c)).fold(f64::INFINITY, f64::min);
            let expected = if distance >= radius {
                0.
            } else {
                0.75 * (1. + (std::f64::consts::PI * distance / radius).cos()) * 0.5
            };
            assert!((mesh.pos(id).z - expected).abs() < 1e-10);
        }
    }
    #[test]
    fn large_preselected_grid_moves_uniformly_without_changing_topology() {
        let mut mesh = grid(40., 40., 1.).unwrap();
        let vertices = mesh.vertices().map(|(id, _)| id).collect::<Vec<_>>();
        let centres = mesh.vertices().map(|(_, v)| v.pos).collect::<Vec<_>>();
        assert!(smoove(&mut mesh, &vertices, &centres, 2., DVec3::Z, DVec3::Z * 0.5).unwrap());
        assert_eq!(mesh.faces().count(), 3200);
        assert!(mesh.vertices().all(|(_, v)| v.pos.z == 0.5));
        mesh.validate().unwrap();
    }
    #[test]
    fn smoove_free_offset_and_collapsed_geometry_are_atomic() {
        let mut mesh = grid(4., 4., 1.).unwrap();
        let before = mesh.clone();
        let centre = DVec3::new(2., 2., 0.);
        let vertex = mesh.find_vertex(centre).unwrap();
        assert!(smoove(&mut mesh, &[vertex], &[centre], 0.25, DVec3::Z, DVec3::X).is_err());
        assert_eq!(mesh, before);
        smoove(&mut mesh, &[vertex], &[centre], 0.25, DVec3::Z, DVec3::X * 0.25).unwrap();
        assert_eq!(mesh.pos(vertex), centre + DVec3::X * 0.25);
        for (id, original) in before.vertices() {
            if id != vertex {
                assert_eq!(mesh.pos(id), original.pos);
            }
        }
        mesh.validate().unwrap();
    }
    #[test]
    fn smoove_falloff_keeps_rim_and_topology_and_supports_ridges() {
        let mut mesh = grid(4., 4., 1.).unwrap();
        let original = mesh.clone();
        let vertices = mesh.vertices().map(|(id, _)| id).collect::<Vec<_>>();
        smoove(&mut mesh, &vertices, &[DVec3::new(2., 2., 0.)], 2., DVec3::Z, DVec3::Z * 2.).unwrap();
        for (id, vertex) in original.vertices() {
            let distance = (vertex.pos - DVec3::new(2., 2., 0.)).length();
            let expected = if distance >= 2. { 0. } else { 1. + (std::f64::consts::PI * distance / 2.).cos() };
            assert!((mesh.pos(id).z - expected).abs() < 1e-9);
        }
        assert_eq!(
            mesh.faces().map(|(id, f)| (id, f.clone())).collect::<Vec<_>>(),
            original.faces().map(|(id, f)| (id, f.clone())).collect::<Vec<_>>()
        );
        assert_eq!(
            mesh.edges().map(|(id, e)| (id, e.clone())).collect::<Vec<_>>(),
            original.edges().map(|(id, e)| (id, e.clone())).collect::<Vec<_>>()
        );
        let mut ridge = original.clone();
        smoove(&mut ridge, &vertices, &[DVec3::new(1., 2., 0.), DVec3::new(3., 2., 0.)], 1.5, DVec3::Z, -DVec3::Z)
            .unwrap();
        for x in [1., 3.] {
            let id = original.find_vertex(DVec3::new(x, 2., 0.)).unwrap();
            assert_eq!(ridge.pos(id).z, -1.);
        }
        ridge.validate().unwrap();
    }
    #[test]
    fn smoove_command_undo_noop_and_errors_are_atomic() {
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        let out = editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X * 4., v: DVec3::Y * 4., spacing: 1. })
            .unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        editor.enter(id).unwrap();
        let vertices = editor.mesh().vertices().map(|(id, _)| id).collect::<Vec<_>>();
        let before = editor.doc().clone();
        let command = |radius, offset| Command::Smoove {
            vertices: vertices.clone(),
            centres: vec![DVec3::new(2., 2., 0.)],
            radius,
            normal: DVec3::Z,
            offset,
        };
        let revision = editor.revision();
        assert!(!editor.apply(command(2., DVec3::ZERO)).unwrap().changed);
        assert_eq!(editor.revision(), revision);
        for (radius, offset) in [(0., DVec3::Z), (f64::NAN, DVec3::Z), (2., DVec3::splat(f64::INFINITY))] {
            assert!(editor.apply(command(radius, offset)).is_err());
            assert_eq!(editor.doc(), &before);
        }
        editor.apply(command(2., DVec3::Z)).unwrap();
        assert_eq!(editor.undo_label(), Some("Smoove"));
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn creation_uses_placement_plane_fresh_ids_and_atomic_undo() {
        use crate::{Command, Editor, Element};
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let before = editor.doc().clone();
        let result = editor
            .apply(Command::CreateTerrainGrid {
                origin: DVec3::new(3., 4., 5.),
                u: DVec3::Y * 2.5,
                v: DVec3::Z * 1.5,
                spacing: 1.,
            })
            .unwrap();
        let Element::Instance(id) = result.created[0] else { panic!() };
        let instance = &editor.active().instances[&id];
        assert_eq!(instance.transform.transform_point3(DVec3::ZERO), DVec3::new(3., 4., 5.));
        assert_eq!(instance.transform.transform_vector3(DVec3::Z), DVec3::X);
        let definition = &editor.doc().definitions[&instance.definition];
        assert!(definition.group);
        assert_eq!(definition.mesh.faces().count(), 12);
        assert_eq!(definition.mesh.edges().filter(|(_, e)| e.soft).count(), 6);
        assert!(definition.mesh.vertices().all(|(id, _)| id.0 >= before.next_id));
        editor.undo();
        assert_eq!(editor.doc(), &before);
        assert!(
            editor
                .apply(Command::CreateTerrainGrid {
                    origin: DVec3::ZERO,
                    u: DVec3::X,
                    v: DVec3::new(1., 1., 0.),
                    spacing: 1.
                })
                .is_err()
        );
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn floating_point_boundary_does_not_add_a_sliver_cell() {
        let mesh = grid(2.0000000000000004, 1., 1.).unwrap();
        assert_eq!(mesh.faces().count(), 4);
        assert_eq!(mesh.bounds().unwrap().1.x, 2.0000000000000004);
    }
    #[test]
    fn grid_preserves_exact_bounds_spacing_and_triangulation() {
        let mesh = grid(2.5, 1.5, 1.).unwrap();
        assert_eq!(mesh.vertices().count(), 12);
        assert_eq!(mesh.faces().count(), 12);
        assert_eq!(mesh.edges().filter(|(_, e)| e.soft && e.smooth).count(), 6);
        assert_eq!(mesh.bounds(), Some((DVec3::ZERO, DVec3::new(2.5, 1.5, 0.))));
        let total = mesh.faces().map(|(id, _)| mesh.face_area(id)).sum::<f64>();
        assert!((total - 3.75).abs() < 1e-9);
        for (id, face) in mesh.faces() {
            assert_eq!(face.outer.len(), 3);
            assert_eq!(mesh.face_normal(id), Some(DVec3::Z));
        }
        for (id, edge) in mesh.edges() {
            let adjacent = mesh.faces_of_edge(id).len();
            assert!((1..=2).contains(&adjacent));
            if edge.soft {
                assert_eq!(adjacent, 2);
            }
        }
        mesh.validate().unwrap();
    }
    #[test]
    fn invalid_and_oversized_grids_fail_before_allocation() {
        for (l, w, s) in [(0., 1., 1.), (1., -1., 1.), (1., 1., 0.), (f64::NAN, 1., 1.), (1000., 1000., 0.001)] {
            assert!(grid(l, w, s).is_err());
        }
        assert_eq!(grid(0.5, 0.2, 1.).unwrap().faces().count(), 2);
    }
}
