//! Constrained terrain triangulation from 3D contour segments.
use rysmo_math::{DVec2, DVec3};
use rysmo_mesh::{Edge, EdgeId, Face, FaceId, Mesh, Vertex, VertexId};
use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};
use std::collections::BTreeMap;
#[derive(Clone, Copy)]
struct Sample {
    site: Point2<f64>,
    world: DVec3,
}
impl HasPosition for Sample {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.site
    }
}

/// Produces a single-height terrain over the contours' XY convex hull. Contour
/// segments remain constrained edges; crossing equal-height lines are split.
pub fn triangulate(lines: &[[DVec3; 2]], first_id: u64) -> Result<Mesh, String> {
    if lines.is_empty() || lines.iter().flatten().any(|p| !p.is_finite()) {
        return Err("Select finite contour lines.".into());
    }
    let lo = lines.iter().flatten().fold(DVec3::splat(f64::INFINITY), |a, p| a.min(*p));
    let hi = lines.iter().flatten().fold(DVec3::splat(f64::NEG_INFINITY), |a, p| a.max(*p));
    let scale = (hi.x - lo.x).max(hi.y - lo.y);
    if !scale.is_finite() || scale < 1e-7 {
        return Err("Contours must span a horizontal area.".into());
    }
    let tolerance = (1e-7 / scale).max(f64::EPSILON * 64.);
    let project = |p: DVec3| DVec2::new((p.x - lo.x) / scale, (p.y - lo.y) / scale);
    let mut cdt = ConstrainedDelaunayTriangulation::<Sample>::new();
    let mut sites: BTreeMap<(u64, u64), (spade::handles::FixedVertexHandle, f64)> = BTreeMap::new();
    let mut constraints = Vec::new();
    for line in lines {
        let mut handles = Vec::new();
        for point in line {
            let site = project(*point);
            let bits = |v: f64| if v == 0. { 0f64.to_bits() } else { v.to_bits() };
            let key = (bits(site.x), bits(site.y));
            let handle = if let Some((handle, height)) = sites.get(&key) {
                if (point.z - *height).abs() > 1e-7 {
                    return Err("Contours have different heights at the same horizontal point.".into());
                }
                *handle
            } else {
                let handle = cdt
                    .insert(Sample { site: Point2::new(site.x, site.y), world: *point })
                    .map_err(|e| e.to_string())?;
                sites.insert(key, (handle, point.z));
                handle
            };
            handles.push(handle);
        }
        if handles[0] == handles[1] {
            return Err("A contour segment has no horizontal length.".into());
        }
        constraints.push([handles[0], handles[1]]);
    }
    let conflicting = std::cell::Cell::new(false);
    for [a, b] in constraints {
        let start = cdt.vertex(a).data().world;
        let end = cdt.vertex(b).data().world;
        let split_edges = cdt.add_constraint_and_split(a, b, |site| {
            let p = DVec2::new(site.x, site.y);
            let mut height: Option<f64> = None;
            for [a, b] in lines {
                let a2 = project(*a);
                let b2 = project(*b);
                let direction = b2 - a2;
                let t = (p - a2).dot(direction) / direction.length_squared();
                if (-1e-8..=1. + 1e-8).contains(&t) && (a2 + direction * t).distance(p) < tolerance {
                    let z = a.z + (b.z - a.z) * t;
                    if let Some(old) = height
                        && (z - old).abs() > 1e-7
                    {
                        conflicting.set(true);
                    }
                    height = Some(z);
                }
            }
            Sample { site, world: DVec3::new(lo.x + site.x * scale, lo.y + site.y * scale, height.unwrap_or(lo.z)) }
        });
        let start_xy = project(start);
        let direction = project(end) - start_xy;
        for edge in split_edges {
            for vertex in cdt.directed_edge(edge).vertices() {
                let sample = vertex.data();
                let p = DVec2::new(sample.site.x, sample.site.y);
                let t = (p - start_xy).dot(direction) / direction.length_squared();
                if (sample.world.z - (start.z + (end.z - start.z) * t)).abs() > 1e-7 {
                    conflicting.set(true);
                }
            }
        }
    }
    if conflicting.get() {
        return Err("Crossing contours disagree in height.".into());
    }
    if cdt.num_inner_faces() == 0 {
        return Err("Contours are collinear; no terrain can be formed.".into());
    }
    let mut next = first_id.max(1);
    let mut vertices = BTreeMap::new();
    let mut mapping = BTreeMap::new();
    for vertex in cdt.vertices() {
        let id = VertexId(next);
        next += 1;
        mapping.insert(vertex.fix().index(), id);
        vertices.insert(id, Vertex { pos: vertex.data().world });
    }
    let mut edges = BTreeMap::new();
    for edge in cdt.undirected_edges() {
        let [a, b] = edge.vertices();
        let mut value = Edge::new(mapping[&a.fix().index()], mapping[&b.fix().index()]);
        value.soft = !edge.is_constraint_edge() && !edge.is_part_of_convex_hull();
        value.smooth = value.soft;
        edges.insert(EdgeId(next), value);
        next += 1;
    }
    let mut faces = BTreeMap::new();
    for face in cdt.inner_faces() {
        let outer = face.vertices().iter().map(|v| mapping[&v.fix().index()]).collect();
        faces.insert(
            FaceId(next),
            Face { hidden: false, outer, holes: vec![], material: None, back_material: None, uv: None },
        );
        next += 1;
    }
    Mesh::from_parts(vertices, edges, faces).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_terrain_retains_complete_and_partial_curve_identity() {
        use crate::{Command, Editor, Element};
        use rysmo_mesh::CurveKind;
        for partial in [false, true] {
            let mut editor = Editor::new();
            editor
                .apply(Command::DrawCircle {
                    centre: DVec3::new(3., 2., 1.),
                    normal: DVec3::Z,
                    radius: 2.,
                    segments: 12,
                    start: Some(DVec3::X),
                })
                .unwrap();
            let (id, original) = editor.mesh().curves().next().map(|(id, c)| (id, *c)).unwrap();
            let edges = editor.mesh().curve_edges(id);
            let edges = if partial { edges[..6].to_vec() } else { edges };
            let source = editor.mesh().clone();
            let out = editor.apply(Command::TerrainFromContours { edges }).unwrap();
            assert_eq!(editor.mesh(), &source);
            let Element::Instance(instance) = out.created[0] else { panic!() };
            let inst = &editor.active().instances[&instance];
            let mesh = &editor.doc().definitions[&inst.definition].mesh;
            let (new_id, curve) = mesh.curves().next().unwrap();
            assert_ne!(new_id, id);
            assert_eq!(curve.kind, if partial { CurveKind::Polyline } else { CurveKind::Circle });
            if !partial {
                assert_eq!(curve.radius, original.radius);
                assert!(inst.transform.transform_point3(curve.centre).distance(original.centre) < 1e-9);
            }
            mesh.validate().unwrap();
        }
    }
    #[test]
    fn command_preserves_sources_uses_drawing_frame_and_undoes_atomically() {
        use crate::{Command, Editor, Element};
        for rotated in [false, true] {
            let mut editor = Editor::new();
            if rotated {
                editor.apply(Command::SetAxes { origin: DVec3::ZERO, x: DVec3::Y, y: DVec3::Z }).unwrap();
            }
            let basis = editor.active().axes.basis();
            let lines = (0..3)
                .map(|row| {
                    [DVec3::new(0., row as f64 * 2., row as f64), DVec3::new(4., row as f64 * 2., row as f64)]
                        .map(|p| basis.transform_vector3(p))
                })
                .collect::<Vec<_>>();
            editor.apply(Command::DrawLines { lines: lines.clone(), facing: None }).unwrap();
            let edges = editor.mesh().edges().map(|(id, _)| id).collect::<Vec<_>>();
            let source = editor.mesh().clone();
            let before = editor.doc().clone();
            let out = editor.apply(Command::TerrainFromContours { edges: edges.clone() }).unwrap();
            assert_eq!(editor.mesh(), &source);
            let Element::Instance(instance) = out.created[0] else { panic!() };
            let inst = &editor.active().instances[&instance];
            let terrain = &editor.doc().definitions[&inst.definition];
            assert!(terrain.group);
            assert_eq!(terrain.mesh.faces().count(), 4);
            assert_eq!(terrain.axes.x, editor.active().axes.x);
            for point in lines.iter().flatten() {
                assert!(
                    terrain
                        .mesh
                        .vertices()
                        .any(|(_, v)| inst.transform.transform_point3(v.pos).distance(*point) < 1e-9)
                );
            }
            assert!(terrain.mesh.vertices().all(|(id, _)| id.0 >= before.next_id));
            editor.undo();
            assert_eq!(editor.doc(), &before);
            assert!(editor.apply(Command::TerrainFromContours { edges: vec![rysmo_mesh::EdgeId(u64::MAX)] }).is_err());
            assert_eq!(editor.doc(), &before);
        }
    }
    #[test]
    fn invalid_degenerate_and_duplicate_height_input_is_rejected() {
        assert!(triangulate(&[], 1).is_err());
        assert!(triangulate(&[[DVec3::ZERO, DVec3::X], [DVec3::X, DVec3::X * 2.]], 1).is_err());
        assert!(triangulate(&[[DVec3::ZERO, DVec3::splat(f64::NAN)]], 1).is_err());
        assert!(triangulate(&[[DVec3::ZERO, DVec3::X], [DVec3::Z, DVec3::Y]], 1).is_err());
    }
    #[test]
    fn contours_preserve_samples_and_constraints_and_make_upward_triangles() {
        let lines = [
            [DVec3::ZERO, DVec3::X * 4.],
            [DVec3::new(0., 2., 1.), DVec3::new(4., 2., 1.)],
            [DVec3::new(0., 4., 2.), DVec3::new(4., 4., 2.)],
        ];
        let mesh = triangulate(&lines, 100).unwrap();
        for p in lines.iter().flatten() {
            assert!(mesh.find_vertex(*p).is_some());
        }
        for [a, b] in lines {
            let edge = mesh.edge_between(mesh.find_vertex(a).unwrap(), mesh.find_vertex(b).unwrap()).unwrap();
            assert!(!mesh.edge(edge).unwrap().soft);
        }
        assert_eq!(mesh.faces().count(), 4);
        assert!(mesh.faces().all(|(id, _)| mesh.face_normal(id).unwrap().z > 0.));
        mesh.validate().unwrap();
    }
    #[test]
    fn crossings_split_at_equal_height_and_conflicting_heights_fail() {
        let lines =
            [[DVec3::new(-1., 0., 0.), DVec3::new(1., 0., 0.)], [DVec3::new(0., -1., 0.), DVec3::new(0., 1., 0.)]];
        let mesh = triangulate(&lines, 1).unwrap();
        assert!(mesh.find_vertex(DVec3::ZERO).is_some());
        assert_eq!(mesh.faces().count(), 4);
        let mut invalid = lines;
        invalid[1][0].z = 1.;
        invalid[1][1].z = 1.;
        assert!(triangulate(&invalid, 1).is_err());
        let junction =
            [[DVec3::new(-1., 0., 0.), DVec3::new(1., 0., 0.)], [DVec3::new(0., 0., 1.), DVec3::new(0., 1., 1.)]];
        assert!(triangulate(&junction, 1).is_err());
    }
}
