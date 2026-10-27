//! Bounded, addressable solid diagnostics and explicit transactional repairs.
use crate::{CommandError, EdgeId, FaceId, Mesh, VertexId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Repair {
    RemoveLooseEdges,
    OrientClosedShells,
}
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub open_edges: Vec<EdgeId>,
    pub non_manifold_edges: Vec<EdgeId>,
    pub orientation_edges: Vec<EdgeId>,
    pub loose_edges: Vec<EdgeId>,
    pub non_manifold_vertices: Vec<VertexId>,
    pub non_manifold_faces: Vec<FaceId>,
    pub components: Vec<Vec<FaceId>>,
    pub reason: Option<String>,
    pub volume: Option<f64>,
    pub hidden_faces: usize,
    pub hidden_edges: usize,
}
fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::Invalid(message.into())
}
fn connected(graph: &BTreeMap<FaceId, BTreeSet<FaceId>>, seed: FaceId) -> BTreeSet<FaceId> {
    let mut seen = BTreeSet::from([seed]);
    let mut queue = VecDeque::from([seed]);
    while let Some(face) = queue.pop_front() {
        for next in graph.get(&face).into_iter().flatten() {
            if seen.insert(*next) {
                queue.push_back(*next);
            }
        }
    }
    seen
}
pub fn inspect(mesh: &Mesh) -> Result<Report, CommandError> {
    if mesh.edge_count() > 100_000 || mesh.face_count() > 20_000 {
        return Err(invalid("Solid diagnostics exceed 100000 edges or 20000 faces"));
    }
    let mut uses: BTreeMap<EdgeId, Vec<(FaceId, bool)>> = BTreeMap::new();
    let mut adjacent: BTreeMap<FaceId, BTreeSet<FaceId>> = BTreeMap::new();
    let mut fans: BTreeMap<VertexId, BTreeMap<FaceId, BTreeSet<FaceId>>> = BTreeMap::new();
    let mut steps = 0usize;
    let mut triangles = 0usize;
    let mut report = Report::default();
    for (id, face) in mesh.faces() {
        report.hidden_faces += usize::from(face.hidden);
        let corners = face.loops().map(|outline| outline.len()).sum::<usize>();
        steps = steps.saturating_add(corners);
        if steps > 100_000 {
            return Err(invalid("Solid diagnostics exceed 100000 boundary steps"));
        }
        triangles =
            triangles.saturating_add(corners.saturating_add(face.holes.len().saturating_mul(2)).saturating_sub(2));
        adjacent.entry(id).or_default();
        for outline in face.loops() {
            for i in 0..outline.len() {
                let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
                let edge = mesh.edge_between(a, b).ok_or_else(|| invalid("A face boundary edge is missing"))?;
                uses.entry(edge).or_default().push((id, a < b));
                fans.entry(a).or_default().entry(id).or_default();
            }
        }
    }
    for (id, edge) in mesh.edges() {
        report.hidden_edges += usize::from(edge.hidden);
        match uses.get(&id).map(Vec::as_slice).unwrap_or_default() {
            [] => report.loose_edges.push(id),
            [_] => report.open_edges.push(id),
            [(a, ad), (b, bd)] if a != b => {
                if ad == bd {
                    report.orientation_edges.push(id);
                }
                adjacent.entry(*a).or_default().insert(*b);
                adjacent.entry(*b).or_default().insert(*a);
                for vertex in edge.v {
                    let fan = fans.entry(vertex).or_default();
                    fan.entry(*a).or_default().insert(*b);
                    fan.entry(*b).or_default().insert(*a);
                }
            }
            _ => report.non_manifold_edges.push(id),
        }
    }
    for (vertex, fan) in fans {
        if let Some(seed) = fan.keys().next()
            && connected(&fan, *seed).len() != fan.len()
        {
            report.non_manifold_vertices.push(vertex);
            report.non_manifold_faces.extend(fan.keys().copied());
        }
    }
    report.non_manifold_faces.sort();
    report.non_manifold_faces.dedup();
    let mut remaining: BTreeSet<_> = adjacent.keys().copied().collect();
    while let Some(seed) = remaining.first().copied() {
        let component = connected(&adjacent, seed);
        for face in &component {
            remaining.remove(face);
        }
        report.components.push(component.into_iter().collect());
    }
    if triangles > 4096 {
        report.reason = Some("Detailed intersection validation is limited to 4096 estimated triangles".into());
    } else {
        match crate::solid_validation::validate(mesh, false) {
            Ok(volume) => {
                report.volume = Some(volume);
                if volume < 0. {
                    report.reason = Some("Closed solid faces inward".into());
                }
            }
            Err(reason) => report.reason = Some(reason),
        }
    }
    Ok(report)
}
pub(crate) fn repair(mesh: &mut Mesh, kind: Repair) -> Result<bool, CommandError> {
    let report = inspect(mesh)?;
    match kind {
        Repair::RemoveLooseEdges => {
            if report.loose_edges.is_empty() {
                return Ok(false);
            }
            if report.loose_edges.iter().any(|id| mesh.edge(*id).is_some_and(|edge| edge.procedural)) {
                return Err(invalid("Component-opening boundaries are not editable"));
            }
            mesh.erase(&report.loose_edges.into_iter().map(crate::Element::Edge).collect::<Vec<_>>());
            Ok(true)
        }
        Repair::OrientClosedShells => {
            if report.hidden_faces > 0
                || !report.open_edges.is_empty()
                || !report.non_manifold_edges.is_empty()
                || !report.non_manifold_vertices.is_empty()
                || !report.loose_edges.is_empty()
            {
                return Err(invalid(
                    "Unhide faces and resolve open, loose or non-manifold geometry before repairing shell orientation",
                ));
            }
            if report.components.len() > 64 {
                return Err(invalid("Orientation repair is limited to 64 shells"));
            }
            let mut staged = mesh.clone();
            for component in &report.components {
                if let Some(seed) = component.first() {
                    staged.orient_faces(*seed)?;
                }
            }
            let nesting = crate::solid_validation::analyse_shell_nesting(&staged).map_err(invalid)?;
            for shell in nesting.shells {
                let expected = if shell.depth % 2 == 0 { 1. } else { -1. };
                if shell.volume.signum() != expected {
                    staged.reverse_faces(&shell.faces.into_iter().collect::<Vec<_>>());
                }
            }
            crate::solid_validation::validate(&staged, false).map_err(invalid)?;
            let changed = staged != *mesh;
            if changed {
                *mesh = staged;
            }
            Ok(changed)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, Element};
    use rysmo_math::DVec3;
    fn cube(editor: &mut Editor, corner: DVec3, size: f64) {
        // Construct independent shells without sticky drawing or through-cuts.
        let mut source = Editor::new();
        source
            .apply(Command::DrawRectangle { corner, u: DVec3::X * size, v: DVec3::Y * size, facing: Some(DVec3::Z) })
            .unwrap();
        let face = source.mesh().faces().next().unwrap().0;
        let distance = size * source.mesh().face_normal(face).unwrap().z;
        source.apply(Command::PushPull { face, distance, keep_base: false }).unwrap();
        let (lo, hi) = source.mesh().bounds().unwrap();
        assert!(
            lo.distance(corner) < 1e-9 && hi.distance(corner + DVec3::splat(size)) < 1e-9,
            "cube bounds: {lo:?} .. {hi:?}, expected {corner:?}"
        );
        let mut doc = editor.doc().clone();
        let added = source.mesh().with_fresh_ids(|| doc.reserve_id()).unwrap();
        let current = doc.mesh();
        let combined = Mesh::from_parts_with_curves(
            current.vertices().chain(added.vertices()).map(|(id, vertex)| (id, vertex.clone())).collect(),
            current.edges().chain(added.edges()).map(|(id, edge)| (id, edge.clone())).collect(),
            current.faces().chain(added.faces()).map(|(id, face)| (id, face.clone())).collect(),
            current.curves().chain(added.curves()).map(|(id, curve)| (id, *curve)).collect(),
        )
        .unwrap();
        doc.root_mut().mesh = combined;
        *editor = Editor::with_document(doc);
    }
    fn triangles(points: &[DVec3], loops: &[[usize; 3]]) -> Mesh {
        use rysmo_mesh::{Edge, Face, Vertex};
        let mut ids = points.len() as u64 + 1..;
        let vertices = points.iter().enumerate().map(|(i, p)| (VertexId(i as u64 + 1), Vertex { pos: *p })).collect();
        let mut pairs = BTreeMap::new();
        let mut edges = BTreeMap::new();
        let mut faces = BTreeMap::new();
        for outline in loops {
            let outer: Vec<_> = outline.iter().map(|i| VertexId(*i as u64 + 1)).collect();
            for i in 0..3 {
                let (a, b) = (outer[i], outer[(i + 1) % 3]);
                let key = (a.min(b), a.max(b));
                pairs.entry(key).or_insert_with(|| {
                    let id = EdgeId(ids.next().unwrap());
                    edges.insert(id, Edge::new(a, b));
                    id
                });
            }
            let id = FaceId(ids.next().unwrap());
            faces.insert(
                id,
                Face { outer, holes: vec![], hidden: false, material: None, back_material: None, uv: None },
            );
        }
        Mesh::from_parts(vertices, edges, faces).unwrap()
    }
    #[test]
    fn categories_identify_open_non_manifold_and_reversed_edges() {
        let points = [DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z, -DVec3::Y];
        let mesh = triangles(&points, &[[0, 1, 2], [0, 1, 3]]);
        let report = inspect(&mesh).unwrap();
        assert_eq!(report.open_edges.len(), 4);
        assert_eq!(report.orientation_edges.len(), 1);
        assert!(report.non_manifold_edges.is_empty());
        assert!(report.reason.is_some());
        let mesh = triangles(&points, &[[0, 1, 2], [0, 1, 3], [1, 0, 4]]);
        assert_eq!(inspect(&mesh).unwrap().non_manifold_edges.len(), 1);
    }
    #[test]
    fn pinched_vertex_is_addressable_even_when_every_edge_has_two_faces() {
        let mesh = triangles(
            &[DVec3::ZERO, DVec3::X, DVec3::Y, DVec3::Z, -DVec3::X, -DVec3::Y, -DVec3::Z],
            &[[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3], [0, 5, 4], [0, 4, 6], [4, 5, 6], [5, 0, 6]],
        );
        let report = inspect(&mesh).unwrap();
        assert!(report.open_edges.is_empty());
        assert!(report.non_manifold_edges.is_empty());
        assert_eq!(report.non_manifold_vertices, vec![VertexId(1)]);
        let mut staged = mesh.clone();
        assert!(repair(&mut staged, Repair::OrientClosedShells).is_err());
        assert_eq!(staged, mesh);
    }
    #[test]
    fn repair_preserves_cavities_and_islands_in_one_undo() {
        let mut editor = Editor::new();
        cube(&mut editor, DVec3::ZERO, 10.);
        cube(&mut editor, DVec3::splat(3.), 4.);
        cube(&mut editor, DVec3::splat(4.), 2.);
        let face = editor.mesh().faces().next().unwrap().0;
        editor.apply(Command::ReverseFaces { faces: vec![face] }).unwrap();
        let before = editor.doc().clone();
        let depth = editor.undo_depth();
        assert!(editor.apply(Command::RepairSolid { repair: Repair::OrientClosedShells }).unwrap().changed);
        let report = inspect(editor.mesh()).unwrap();
        assert!(report.reason.is_none());
        let shells = crate::solid_validation::analyse(editor.mesh())
            .unwrap()
            .shells
            .into_iter()
            .map(|shell| {
                let (lo, hi) = shell
                    .faces
                    .iter()
                    .flat_map(|id| editor.mesh().loop_points(&editor.mesh().face(*id).unwrap().outer))
                    .fold((DVec3::splat(f64::INFINITY), DVec3::splat(f64::NEG_INFINITY)), |(lo, hi), p| {
                        (lo.min(p), hi.max(p))
                    });
                (shell.depth, shell.volume, lo, hi)
            })
            .collect::<Vec<_>>();
        assert!((report.volume.unwrap() - 944.).abs() < 1e-8, "volume: {:?}, shells: {shells:?}", report.volume);
        assert_eq!(editor.undo_depth(), depth + 1);
        assert_eq!(editor.undo_label(), Some("Repair Solid"));
        let after = editor.doc().clone();
        let revision = editor.revision();
        assert!(!editor.apply(Command::RepairSolid { repair: Repair::OrientClosedShells }).unwrap().changed);
        assert_eq!(editor.revision(), revision);
        assert_eq!(editor.undo_depth(), depth + 1);
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
        assert!(editor.redo());
        assert_eq!(editor.doc(), &after);
    }
    #[test]
    fn loose_edge_removal_is_selective_and_undoable() {
        let mut editor = Editor::new();
        cube(&mut editor, DVec3::ZERO, 2.);
        editor
            .apply(Command::DrawLines { lines: vec![[DVec3::splat(5.), DVec3::splat(5.) + DVec3::X]], facing: None })
            .unwrap();
        let before = editor.doc().clone();
        let faces: Vec<_> = editor.mesh().faces().map(|(id, f)| (id, f.clone())).collect();
        assert_eq!(inspect(editor.mesh()).unwrap().loose_edges.len(), 1);
        editor.apply(Command::RepairSolid { repair: Repair::RemoveLooseEdges }).unwrap();
        assert!(inspect(editor.mesh()).unwrap().reason.is_none());
        assert_eq!(editor.mesh().faces().map(|(id, f)| (id, f.clone())).collect::<Vec<_>>(), faces);
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
    }
    #[test]
    fn open_hidden_and_intersecting_shells_are_not_silently_repaired() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let before = editor.doc().clone();
        let revision = editor.revision();
        let depth = editor.undo_depth();
        assert!(editor.apply(Command::RepairSolid { repair: Repair::OrientClosedShells }).is_err());
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.revision(), revision);
        assert_eq!(editor.undo_depth(), depth);
        let mut editor = Editor::new();
        cube(&mut editor, DVec3::ZERO, 2.);
        cube(&mut editor, DVec3::ONE, 2.);
        let before = editor.doc().clone();
        assert!(editor.apply(Command::RepairSolid { repair: Repair::OrientClosedShells }).is_err());
        assert_eq!(editor.doc(), &before);
        let mut editor = Editor::new();
        cube(&mut editor, DVec3::ZERO, 2.);
        let face = editor.mesh().faces().next().unwrap().0;
        editor.apply(Command::SetGeometryHidden { elements: vec![Element::Face(face)], hidden: true }).unwrap();
        assert_eq!(inspect(editor.mesh()).unwrap().hidden_faces, 1);
        let before = editor.doc().clone();
        assert!(editor.apply(Command::RepairSolid { repair: Repair::OrientClosedShells }).is_err());
        assert_eq!(editor.doc(), &before);
    }
}
