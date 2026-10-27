//! Exact polyline point edits that retain IDs and refuse ambiguous deformation.
use crate::{CommandError, Mesh, VertexId};
use rysmo_math::{DVec3, EPS};
use rysmo_mesh::CurveId;
use std::collections::BTreeSet;
fn invalid(text: &str) -> CommandError {
    CommandError::Invalid(text.into())
}
pub fn vertices(mesh: &Mesh, curve: CurveId) -> Result<Vec<VertexId>, CommandError> {
    let descriptor = mesh.curve(curve).ok_or_else(|| invalid("Missing curve"))?;
    if descriptor.kind != rysmo_mesh::CurveKind::Polyline {
        return Err(invalid(
            "Edit analytic arcs and circles through their radius and segments, or explode and weld them first",
        ));
    }
    let edges = mesh.curve_edges(curve);
    if edges.is_empty() || edges.len() > 2048 || mesh.vertex_count() > 50_000 {
        return Err(invalid("Point editing is limited to 2048 curve edges and 50000 context vertices"));
    }
    mesh.edge_chain(&edges)
        .map(|(vertices, _)| vertices)
        .ok_or_else(|| invalid("Curve must be one connected unbranched chain or loop"))
}
pub fn preview(mesh: &Mesh, curve: CurveId, vertex: VertexId, position: DVec3) -> Result<Mesh, CommandError> {
    let vertices = vertices(mesh, curve)?;
    if !vertices.contains(&vertex) || !position.is_finite() {
        return Err(invalid("Choose a curve point and finite coordinates"));
    }
    for edge in mesh.edges_of_vertex(vertex) {
        let edge = mesh.edge(edge).expect("incident edge");
        if edge.procedural || edge.curve.is_some_and(|id| id != curve) {
            return Err(invalid("Point belongs to an opening boundary or another curve"));
        }
    }
    if mesh.vertices().any(|(id, other)| id != vertex && other.pos.distance(position) < EPS) {
        return Err(invalid("Point would merge with another vertex; use modelling tools to change topology"));
    }
    let mut staged = mesh.clone();
    staged.deform_vertices(&BTreeSet::from([vertex]), |_| position)?;
    let incident: Vec<_> = staged.edges_of_vertex(vertex);
    if incident.len().saturating_mul(staged.edge_count()) > 2_000_000 {
        return Err(invalid("Point edit exceeds 2000000 edge intersection checks"));
    }
    for id in incident {
        let edge = staged.edge(id).expect("incident edge");
        let (a, b) = staged.edge_points(id);
        for (other_id, other) in staged.edges() {
            if id == other_id {
                continue;
            }
            let (c, d) = staged.edge_points(other_id);
            if let Some((t, _)) = rysmo_math::segment_intersection(a, b, c, d) {
                let intersection = a.lerp(b, t);
                let shared = edge.v.iter().any(|v| other.v.contains(v) && staged.pos(*v).distance(intersection) < EPS);
                if !shared {
                    return Err(invalid("Point edit would create an unjoined edge crossing"));
                }
            }
            for (point_id, point) in [(edge.v[0], a), (edge.v[1], b)] {
                if !other.v.contains(&point_id) && near_segment(point, c, d) {
                    return Err(invalid("Point edit would overlap another edge"));
                }
            }
            for (point_id, point) in [(other.v[0], c), (other.v[1], d)] {
                if !edge.v.contains(&point_id) && near_segment(point, a, b) {
                    return Err(invalid("Point edit would overlap another edge"));
                }
            }
        }
    }
    Ok(staged)
}
fn near_segment(point: DVec3, a: DVec3, b: DVec3) -> bool {
    let ab = b - a;
    let t = ((point - a).dot(ab) / ab.length_squared()).clamp(0., 1.);
    point.distance(a + ab * t) < EPS
}
pub(crate) fn apply(mesh: &mut Mesh, curve: CurveId, vertex: VertexId, position: DVec3) -> Result<bool, CommandError> {
    let staged = preview(mesh, curve, vertex, position)?;
    let changed = staged != *mesh;
    if changed {
        *mesh = staged;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, Element};
    fn fixture() -> (Editor, CurveId, Vec<VertexId>) {
        let mut e = Editor::new();
        let out = e
            .apply(Command::DrawLines {
                lines: vec![[DVec3::ZERO, DVec3::X * 2.], [DVec3::X * 2., DVec3::new(3., 1., 0.)]],
                facing: None,
            })
            .unwrap();
        let edges = out
            .created
            .into_iter()
            .filter_map(|item| if let Element::Edge(id) = item { Some(id) } else { None })
            .collect();
        e.apply(Command::WeldEdges { edges }).unwrap();
        let curve = e.mesh().curves().next().unwrap().0;
        let points = vertices(e.mesh(), curve).unwrap();
        (e, curve, points)
    }
    #[test]
    fn preview_is_read_only_and_apply_preserves_ids_and_curve_through_undo() {
        let (mut e, curve, points) = fixture();
        let before = e.doc().clone();
        let target = DVec3::new(2., 2., 1.);
        let preview = preview(e.mesh(), curve, points[1], target).unwrap();
        assert_eq!(e.doc(), &before);
        assert_eq!(preview.pos(points[1]), target);
        e.apply(Command::SetCurvePoint { curve, vertex: points[1], position: target }).unwrap();
        assert_eq!(vertices(e.mesh(), curve).unwrap(), points);
        assert_eq!(e.mesh().edge_count(), before.root.mesh.edge_count());
        let after = e.doc().clone();
        e.undo();
        assert_eq!(e.doc(), &before);
        e.redo();
        assert_eq!(e.doc(), &after);
    }
    #[test]
    fn collapse_and_nonfinite_coordinates_reject_atomically() {
        let (mut e, curve, points) = fixture();
        let before = e.doc().clone();
        for position in [e.mesh().pos(points[0]), DVec3::splat(f64::NAN)] {
            assert!(e.apply(Command::SetCurvePoint { curve, vertex: points[1], position }).is_err());
            assert_eq!(e.doc(), &before);
        }
    }
    #[test]
    fn face_bending_is_refused_without_triangulating_or_losing_ids() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y * 2., facing: None })
            .unwrap();
        let edges = e.mesh().edges().map(|(id, _)| id).collect();
        e.apply(Command::WeldEdges { edges }).unwrap();
        let curve = e.mesh().curves().next().unwrap().0;
        let vertex = vertices(e.mesh(), curve).unwrap()[0];
        let before = e.doc().clone();
        assert!(e.apply(Command::SetCurvePoint { curve, vertex, position: e.mesh().pos(vertex) + DVec3::Z }).is_err());
        assert_eq!(e.doc(), &before);
    }
    #[test]
    fn new_unjoined_crossings_are_rejected_atomically() {
        let (mut e, curve, points) = fixture();
        e.apply(Command::DrawLines { lines: vec![[DVec3::new(1., 2., 0.), DVec3::new(3., 2., 0.)]], facing: None })
            .unwrap();
        let before = e.doc().clone();
        assert!(
            e.apply(Command::SetCurvePoint { curve, vertex: points[1], position: DVec3::new(2., 3., 0.) }).is_err()
        );
        assert_eq!(e.doc(), &before);
    }
}
