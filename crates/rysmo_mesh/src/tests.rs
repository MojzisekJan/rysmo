use std::collections::BTreeSet;

use rysmo_math::DVec3;

use crate::*;

pub(crate) fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

pub(crate) fn rect(m: &mut Mesh, x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Vec<EdgeId> {
    let a = v(x0, y0, z);
    let b = v(x1, y0, z);
    let c = v(x1, y1, z);
    let d = v(x0, y1, z);
    m.add_lines(&[(a, b), (b, c), (c, d), (d, a)], Some(DVec3::Z)).unwrap()
}

pub(crate) fn only_face(m: &Mesh) -> FaceId {
    assert_eq!(m.face_count(), 1, "expected one face");
    m.faces().next().unwrap().0
}

pub(crate) fn face_at(m: &Mesh, p: DVec3) -> Option<FaceId> {
    m.faces().map(|(id, _)| id).find(|f| m.face_contains(*f, p))
}

pub(crate) fn face_centroid(m: &Mesh, f: FaceId) -> DVec3 {
    let pts = m.loop_points(&m.face(f).unwrap().outer);
    pts.iter().copied().sum::<DVec3>() / pts.len() as f64
}

/// Every edge borders exactly two faces and adjacent faces run shared edges in
/// opposite directions: the mesh is a closed, consistently oriented surface.
pub(crate) fn assert_closed(m: &Mesh) {
    m.validate().unwrap();
    for (e, _) in m.edges() {
        let faces = m.faces_of_edge(e);
        assert_eq!(faces.len(), 2, "edge {e} borders {} faces", faces.len());
    }
    let mut directed = BTreeSet::new();
    for (_, f) in m.faces() {
        for step in f.loop_edges() {
            assert!(directed.insert(step), "two faces run {:?} the same way", step);
        }
    }
}

pub(crate) fn assert_outward(m: &Mesh, centre: DVec3) {
    for (f, _) in m.faces() {
        let n = m.face_normal(f).unwrap();
        assert!(n.dot(face_centroid(m, f) - centre) > 0.0, "face {f} faces inwards");
    }
}

pub(crate) fn unit_box() -> Mesh {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 1.0, 1.0, 0.0);
    let f = only_face(&m);
    let r = m.push_pull(f, -1.0, false).unwrap();
    // The ground face points down, so pulling it up is a negative distance.
    assert_eq!(r.face, Some(f));
    m
}

#[test]
fn rectangle_makes_a_face_facing_down_on_the_ground() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 2.0, 1.0, 0.0);
    assert_eq!((m.vertex_count(), m.edge_count()), (4, 4));
    let f = only_face(&m);
    assert!((m.face_normal(f).unwrap() - -DVec3::Z).length() < 1e-12);
    assert!((m.face_area(f) - 2.0).abs() < 1e-12);
    m.validate().unwrap();
}

#[test]
fn rectangle_in_the_air_faces_the_hint() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 1.0, 1.0, 2.0);
    let f = only_face(&m);
    assert!(m.face_normal(f).unwrap().z > 0.99);
}

#[test]
fn open_lines_make_no_face_until_the_loop_closes() {
    let mut m = Mesh::new();
    m.add_line(v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), None).unwrap();
    m.add_line(v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0), None).unwrap();
    m.add_line(v(1.0, 1.0, 0.0), v(0.0, 1.0, 0.0), None).unwrap();
    assert_eq!(m.face_count(), 0);
    m.add_line(v(0.0, 1.0, 0.0), v(0.0, 0.0, 0.0), None).unwrap();
    assert_eq!(m.face_count(), 1);
}

#[test]
fn zero_length_line_is_refused() {
    let mut m = Mesh::new();
    assert_eq!(m.add_line(v(1.0, 1.0, 1.0), v(1.0, 1.0, 1.0), None), Err(MeshError::ZeroLength));
}

#[test]
fn crossing_lines_split_each_other() {
    let mut m = Mesh::new();
    m.add_line(v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), None).unwrap();
    let parts = m.add_line(v(1.0, -1.0, 0.0), v(1.0, 1.0, 0.0), None).unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!((m.vertex_count(), m.edge_count()), (5, 4));
    assert!(m.find_vertex(v(1.0, 0.0, 0.0)).is_some());
    m.validate().unwrap();
}

#[test]
fn line_ending_on_an_edge_splits_it() {
    let mut m = Mesh::new();
    m.add_line(v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), None).unwrap();
    m.add_line(v(1.0, 1.0, 0.0), v(1.0, 0.0, 0.0), None).unwrap();
    assert_eq!((m.vertex_count(), m.edge_count()), (4, 3));
}

#[test]
fn collinear_overlap_reuses_edges() {
    let mut m = Mesh::new();
    m.add_line(v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), None).unwrap();
    let parts = m.add_line(v(1.0, 0.0, 0.0), v(3.0, 0.0, 0.0), None).unwrap();
    assert_eq!(parts.len(), 2);
    assert_eq!((m.vertex_count(), m.edge_count()), (4, 3));
    m.validate().unwrap();
}

#[test]
fn line_across_a_face_splits_it_and_keeps_its_material() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 2.0, 1.0, 0.0);
    let f = only_face(&m);
    m.paint(f, Some(MaterialId(99)), false).unwrap();
    m.add_line(v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0), None).unwrap();
    assert_eq!(m.face_count(), 2);
    let left = face_at(&m, v(0.5, 0.5, 0.0)).unwrap();
    let right = face_at(&m, v(1.5, 0.5, 0.0)).unwrap();
    assert_ne!(left, right);
    for g in [left, right] {
        assert_eq!(m.face(g).unwrap().material, Some(MaterialId(99)));
        assert!(m.face_normal(g).unwrap().z < -0.99);
        assert!((m.face_area(g) - 1.0).abs() < 1e-12);
    }
    // One half keeps the old id.
    assert!(left == f || right == f);
    m.validate().unwrap();
}

#[test]
fn diagonal_line_splits_into_triangles() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 1.0, 1.0, 0.0);
    m.add_line(v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0), None).unwrap();
    assert_eq!(m.face_count(), 2);
    for (f, face) in m.faces() {
        assert_eq!(face.outer.len(), 3);
        assert!((m.face_area(f) - 0.5).abs() < 1e-12);
    }
}

#[test]
fn dangling_line_on_a_face_leaves_it_alone() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 2.0, 2.0, 0.0);
    let f = only_face(&m);
    m.add_line(v(0.0, 1.0, 0.0), v(1.0, 1.0, 0.0), None).unwrap();
    // The boundary gained a vertex where the line starts, but there is still one face.
    assert_eq!(only_face(&m), f);
    assert!((m.face_area(f) - 4.0).abs() < 1e-12);
    m.validate().unwrap();
}

#[test]
fn rectangle_inside_a_face_cuts_a_hole() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 4.0, 4.0, 0.0);
    let outer = only_face(&m);
    rect(&mut m, 1.0, 1.0, 2.0, 2.0, 0.0);
    assert_eq!(m.face_count(), 2);
    let ring = face_at(&m, v(3.0, 3.0, 0.0)).unwrap();
    let inner = face_at(&m, v(1.5, 1.5, 0.0)).unwrap();
    assert_eq!(ring, outer);
    assert_eq!(m.face(ring).unwrap().holes.len(), 1);
    assert!((m.face_area(ring) - 15.0).abs() < 1e-12);
    assert!((m.face_area(inner) - 1.0).abs() < 1e-12);
    // Both face the same way as the face they were drawn on.
    assert!(m.face_normal(inner).unwrap().z < -0.99);
    m.validate().unwrap();
}

#[test]
fn rectangle_around_a_face_gets_it_as_a_hole() {
    let mut m = Mesh::new();
    rect(&mut m, 1.0, 1.0, 2.0, 2.0, 0.0);
    rect(&mut m, 0.0, 0.0, 3.0, 3.0, 0.0);
    assert_eq!(m.face_count(), 2);
    let ring = face_at(&m, v(0.5, 0.5, 0.0)).unwrap();
    assert_eq!(m.face(ring).unwrap().holes.len(), 1);
    assert!((m.face_area(ring) - 8.0).abs() < 1e-12);
}

#[test]
fn adjacent_rectangles_share_an_edge() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 1.0, 1.0, 0.0);
    rect(&mut m, 1.0, 0.0, 2.0, 1.0, 0.0);
    assert_eq!(m.face_count(), 2);
    assert_eq!(m.edge_count(), 7);
    let shared = m.edges_along(v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0));
    assert_eq!(shared.len(), 1);
    assert_eq!(m.faces_of_edge(shared[0]).len(), 2);
}

#[test]
fn overlapping_rectangles_split_into_three() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 2.0, 2.0, 0.0);
    rect(&mut m, 1.0, 1.0, 3.0, 3.0, 0.0);
    // The first face splits into two by the new lines; the second's two regions: one is
    // inside the old face, one is new.
    assert_eq!(m.face_count(), 3);
    assert!(face_at(&m, v(0.5, 0.5, 0.0)).is_some());
    assert!(face_at(&m, v(1.5, 1.5, 0.0)).is_some());
    assert!(face_at(&m, v(2.5, 2.5, 0.0)).is_some());
    m.validate().unwrap();
}

#[test]
fn circle_polygon_makes_one_face() {
    let mut m = Mesh::new();
    let n = 24;
    let pts: Vec<DVec3> = (0..n)
        .map(|i| {
            let a = i as f64 / n as f64 * std::f64::consts::TAU;
            v(a.cos(), a.sin(), 0.0)
        })
        .collect();
    let lines: Vec<_> = (0..n).map(|i| (pts[i], pts[(i + 1) % n])).collect();
    m.add_lines(&lines, None).unwrap();
    let f = only_face(&m);
    assert_eq!(m.face(f).unwrap().outer.len(), n);
}

#[test]
fn vertical_rectangle_makes_a_face() {
    let mut m = Mesh::new();
    let a = v(0.0, 0.0, 0.0);
    let b = v(1.0, 0.0, 0.0);
    let c = v(1.0, 0.0, 1.0);
    let d = v(0.0, 0.0, 1.0);
    m.add_lines(&[(a, b), (b, c), (c, d), (d, a)], Some(-DVec3::Y)).unwrap();
    let f = only_face(&m);
    assert!(m.face_normal(f).unwrap().y < -0.99);
}

#[test]
fn push_pull_free_face_makes_a_closed_box() {
    let m = unit_box();
    assert_eq!((m.vertex_count(), m.edge_count(), m.face_count()), (8, 12, 6));
    assert_closed(&m);
    assert_outward(&m, v(0.5, 0.5, 0.5));
}

#[test]
fn push_pull_zero_is_refused() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 1.0, 1.0, 0.0);
    let f = only_face(&m);
    assert_eq!(m.push_pull(f, 0.0, false), Err(MeshError::ZeroDistance));
}

#[test]
fn pulling_the_top_of_a_box_stretches_it() {
    let mut m = unit_box();
    let top = face_at(&m, v(0.5, 0.5, 1.0)).unwrap();
    m.push_pull(top, 1.0, false).unwrap();
    // The new side faces merge with the old ones and the old top corners heal away.
    assert_eq!((m.vertex_count(), m.edge_count(), m.face_count()), (8, 12, 6));
    assert_closed(&m);
    assert_outward(&m, v(0.5, 0.5, 1.0));
    assert!(m.find_vertex(v(1.0, 1.0, 2.0)).is_some());
}

#[test]
fn ctrl_push_pull_keeps_the_starting_face() {
    let mut m = unit_box();
    let top = face_at(&m, v(0.5, 0.5, 1.0)).unwrap();
    m.push_pull(top, 1.0, true).unwrap();
    // A face stays at z = 1 between two stacked boxes.
    assert_eq!(m.face_count(), 11);
    m.validate().unwrap();
}

#[test]
fn pulling_a_rectangle_drawn_on_a_box() {
    let mut m = unit_box();
    rect(&mut m, 0.25, 0.25, 0.75, 0.75, 1.0);
    assert_eq!(m.face_count(), 7);
    let inner = face_at(&m, v(0.5, 0.5, 1.0)).unwrap();
    assert!(m.face_normal(inner).unwrap().z > 0.99);
    m.push_pull(inner, 0.5, false).unwrap();
    assert_eq!(m.face_count(), 11);
    assert_closed(&m);
}

#[test]
fn pushing_a_rectangle_in_makes_a_pocket() {
    let mut m = unit_box();
    rect(&mut m, 0.25, 0.25, 0.75, 0.75, 1.0);
    let inner = face_at(&m, v(0.5, 0.5, 1.0)).unwrap();
    m.push_pull(inner, -0.5, false).unwrap();
    assert_eq!(m.face_count(), 11);
    assert_closed(&m);
    let bottom = face_at(&m, v(0.5, 0.5, 0.5)).unwrap();
    assert!(m.face_normal(bottom).unwrap().z > 0.99);
    // Pocket walls face into the pocket.
    let wall = face_at(&m, v(0.25, 0.5, 0.75)).unwrap();
    assert!(m.face_normal(wall).unwrap().x > 0.99);
}

#[test]
fn pushing_through_cuts_a_hole() {
    let mut m = unit_box();
    rect(&mut m, 0.25, 0.25, 0.75, 0.75, 1.0);
    let inner = face_at(&m, v(0.5, 0.5, 1.0)).unwrap();
    let r = m.push_pull(inner, -1.0, false).unwrap();
    assert_eq!(r.face, None);
    // Top ring, bottom ring, four outer and four inner walls.
    assert_eq!(m.face_count(), 10);
    assert!(face_at(&m, v(0.5, 0.5, 0.0)).is_none());
    assert!(face_at(&m, v(0.5, 0.5, 1.0)).is_none());
    let bottom = face_at(&m, v(0.1, 0.1, 0.0)).unwrap();
    assert_eq!(m.face(bottom).unwrap().holes.len(), 1);
    assert_closed(&m);
}

#[test]
fn push_pull_face_with_hole() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 3.0, 3.0, 0.0);
    rect(&mut m, 1.0, 1.0, 2.0, 2.0, 0.0);
    let inner = face_at(&m, v(1.5, 1.5, 0.0)).unwrap();
    m.erase(&[Element::Face(inner)]);
    let ring = only_face(&m);
    m.push_pull(ring, -1.0, false).unwrap();
    // Top and bottom rings, four outside and four inside walls.
    assert_eq!(m.face_count(), 10);
    assert_closed(&m);
}

#[test]
fn erasing_a_splitting_line_merges_the_faces() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 2.0, 1.0, 0.0);
    m.add_line(v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0), None).unwrap();
    assert_eq!(m.face_count(), 2);
    let mid = m.edges_along(v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0));
    m.erase(&[Element::Edge(mid[0])]);
    // The halves merge, and the vertices the line left on the boundary heal.
    assert_eq!((m.vertex_count(), m.edge_count(), m.face_count()), (4, 4, 1));
    let f = only_face(&m);
    assert!((m.face_area(f) - 2.0).abs() < 1e-12);
    m.validate().unwrap();
}

#[test]
fn erasing_a_box_edge_removes_its_faces() {
    let mut m = unit_box();
    let e = m.edges_along(v(0.0, 0.0, 1.0), v(1.0, 0.0, 1.0))[0];
    m.erase(&[Element::Edge(e)]);
    assert_eq!(m.face_count(), 4);
    assert_eq!(m.edge_count(), 11);
    m.validate().unwrap();
}

#[test]
fn erasing_a_face_keeps_its_edges() {
    let mut m = unit_box();
    let top = face_at(&m, v(0.5, 0.5, 1.0)).unwrap();
    m.erase(&[Element::Face(top)]);
    assert_eq!((m.edge_count(), m.face_count()), (12, 5));
}

#[test]
fn redrawing_an_edge_restores_a_face() {
    let mut m = unit_box();
    let top = face_at(&m, v(0.5, 0.5, 1.0)).unwrap();
    m.erase(&[Element::Face(top)]);
    m.add_line(v(0.0, 0.0, 1.0), v(1.0, 0.0, 1.0), Some(DVec3::Z)).unwrap();
    assert_eq!(m.face_count(), 6);
    assert_closed(&m);
    assert_outward(&m, v(0.5, 0.5, 0.5));
}

#[test]
fn moving_a_face_keeps_things_flat() {
    let mut m = unit_box();
    let top = face_at(&m, v(0.5, 0.5, 1.0)).unwrap();
    m.move_elements(&[Element::Face(top)], v(0.0, 0.0, 0.5)).unwrap();
    assert!(m.find_vertex(v(1.0, 1.0, 1.5)).is_some());
    assert_closed(&m);
}

#[test]
fn moving_one_corner_up_folds_the_faces() {
    let mut m = unit_box();
    let corner = m.find_vertex(v(1.0, 1.0, 1.0)).unwrap();
    m.move_vertices(&BTreeSet::from([corner]), v(0.0, 0.0, 0.3)).unwrap();
    // The top folds into two triangles; the sides through the corner stay flat.
    assert_eq!(m.face_count(), 7);
    assert_closed(&m);
    assert_outward(&m, v(0.5, 0.5, 0.5));
}

#[test]
fn moving_a_corner_onto_another_welds_it() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 1.0, 1.0, 0.0);
    let corner = m.find_vertex(v(1.0, 1.0, 0.0)).unwrap();
    m.move_vertices(&BTreeSet::from([corner]), v(-1.0, -1.0, 0.0)).unwrap();
    // The square collapses to the triangle that is left.
    assert_eq!(m.vertex_count(), 3);
    assert_eq!(m.face(only_face(&m)).unwrap().outer.len(), 3);
    m.validate().unwrap();
}

#[test]
fn moving_an_edge_along_its_face_reshapes_it() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 1.0, 1.0, 0.0);
    let e = m.edges_along(v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0))[0];
    m.move_elements(&[Element::Edge(e)], v(1.0, 0.0, 0.0)).unwrap();
    let f = only_face(&m);
    assert!((m.face_area(f) - 2.0).abs() < 1e-12);
}

#[test]
fn copying_a_face_makes_an_array() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 1.0, 1.0, 0.0);
    let f = only_face(&m);
    m.paint(f, Some(MaterialId(7)), false).unwrap();
    let new = m.copy_elements(&[Element::Face(f)], v(2.0, 0.0, 0.0), 3).unwrap();
    assert_eq!(m.face_count(), 4);
    let copies: Vec<_> = new.iter().filter(|e| matches!(e, Element::Face(_))).collect();
    assert_eq!(copies.len(), 3);
    for (g, face) in m.faces() {
        assert_eq!(face.material, Some(MaterialId(7)));
        assert!(m.face_normal(g).unwrap().z < -0.99);
    }
}

#[test]
fn copying_a_box_copies_all_faces() {
    let mut m = unit_box();
    let all: Vec<Element> = m.faces().map(|(f, _)| Element::Face(f)).collect();
    m.copy_elements(&all, v(3.0, 0.0, 0.0), 1).unwrap();
    assert_eq!(m.face_count(), 12);
    assert_closed(&m);
    assert_outward_split(&m);
}

fn assert_outward_split(m: &Mesh) {
    for (f, _) in m.faces() {
        let c = face_centroid(m, f);
        let centre = if c.x < 2.0 { v(0.5, 0.5, 0.5) } else { v(3.5, 0.5, 0.5) };
        assert!(m.face_normal(f).unwrap().dot(c - centre) > 0.0);
    }
}

#[test]
fn reverse_swaps_sides() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 1.0, 1.0, 0.0);
    let f = only_face(&m);
    m.paint(f, Some(MaterialId(3)), false).unwrap();
    m.reverse_faces(&[f]);
    assert!(m.face_normal(f).unwrap().z > 0.99);
    assert_eq!(m.face(f).unwrap().back_material, Some(MaterialId(3)));
    assert_eq!(m.face(f).unwrap().material, None);
}

#[test]
fn triangulation_covers_a_face_with_a_hole() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 4.0, 4.0, 0.0);
    rect(&mut m, 1.0, 1.0, 2.0, 2.0, 0.0);
    let ring = face_at(&m, v(3.0, 3.0, 0.0)).unwrap();
    let tris = triangulate_face(&m, ring);
    let n = m.face_normal(ring).unwrap();
    let mut area = 0.0;
    for t in &tris {
        let (a, b, c) = (m.pos(t[0]), m.pos(t[1]), m.pos(t[2]));
        let cr = (b - a).cross(c - a);
        assert!(cr.dot(n) > 0.0, "triangle wound the wrong way");
        area += cr.length() * 0.5;
    }
    assert!((area - 15.0).abs() < 1e-9);
}

#[test]
fn picking_finds_the_nearest_face() {
    let m = unit_box();
    let ray = rysmo_math::Ray::new(v(0.5, 0.5, 5.0), -DVec3::Z);
    let (f, t) = m.pick_face(&ray).unwrap();
    assert!((t - 4.0).abs() < 1e-9);
    assert!(m.face_normal(f).unwrap().z > 0.99);
}

#[test]
fn from_parts_rejects_broken_loops() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 1.0, 1.0, 0.0);
    let mut faces = m.faces.clone();
    let f = faces.values_mut().next().unwrap();
    f.outer.swap(0, 1);
    assert!(Mesh::assemble(m.vertices.clone(), m.edges.clone(), faces, Default::default()).is_err());
    assert!(Mesh::assemble(m.vertices.clone(), m.edges.clone(), m.faces.clone(), Default::default()).is_ok());
}

#[test]
fn moving_a_wire_across_an_edge_splits_both_at_the_new_intersection() {
    let mut m = Mesh::new();
    m.add_line(DVec3::new(-1., 0., 0.), DVec3::new(1., 0., 0.), None).unwrap();
    let moved = m.add_line(DVec3::new(0., 1., 0.), DVec3::new(0., 3., 0.), None).unwrap()[0];
    m.move_elements(&[Element::Edge(moved)], DVec3::new(0., -2., 0.)).unwrap();
    assert_eq!(m.edge_count(), 4);
    let vertex = m.find_vertex(DVec3::ZERO).unwrap();
    assert_eq!(m.edges_of_vertex(vertex).len(), 4);
    m.validate().unwrap();
}

#[test]
fn moved_endpoint_landing_inside_an_edge_splits_its_host() {
    let mut m = Mesh::new();
    m.add_line(DVec3::new(-1., 0., 0.), DVec3::new(1., 0., 0.), None).unwrap();
    let moved = m.add_line(DVec3::new(0., 1., 0.), DVec3::new(0., 2., 0.), None).unwrap()[0];
    m.move_elements(&[Element::Edge(moved)], -DVec3::Y).unwrap();
    assert_eq!(m.edge_count(), 3);
    assert_eq!(m.edges_of_vertex(m.find_vertex(DVec3::ZERO).unwrap()).len(), 3);
    m.validate().unwrap();
}

#[test]
fn push_pull_back_to_a_free_faces_base_restores_one_flat_face() {
    let mut mesh = Mesh::new();
    rect(&mut mesh, 0., 0., 2., 2., 0.);
    let face = mesh.faces().next().unwrap().0;
    mesh.push_pull(face, -1., false).unwrap();
    mesh.push_pull(face, -1., false).unwrap();
    assert_eq!(mesh.face_count(), 1);
    assert_eq!(mesh.edge_count(), 4);
    assert!(mesh.vertices().all(|(_, v)| v.pos.z.abs() < 1e-9));
    mesh.validate().unwrap();
}

#[test]
fn push_pull_a_patch_back_flush_merges_its_surrounding_plane() {
    let mut mesh = Mesh::new();
    rect(&mut mesh, 0., 0., 4., 4., 0.);
    rect(&mut mesh, 1., 1., 2., 2., 0.);
    let face = mesh.faces().find(|(id, _)| mesh.face_contains(*id, v(1.5, 1.5, 0.))).unwrap().0;
    let normal = mesh.face_normal(face).unwrap();
    mesh.push_pull(face, 1., false).unwrap();
    mesh.push_pull(face, -1., false).unwrap();
    assert_eq!(mesh.face_count(), 1);
    assert!(mesh.vertices().all(|(_, v)| v.pos.z.abs() < 1e-9));
    assert!(mesh.face_normal(mesh.faces().next().unwrap().0).unwrap().dot(normal) > 0.99);
    mesh.validate().unwrap();
}
