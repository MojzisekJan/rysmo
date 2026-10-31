//! Tests for the milestone 2 operations: transforms, copies, extraction, edge flags,
//! offset and Follow Me.

use std::collections::BTreeSet;

use rysmo_math::{DMat4, DVec3};

use crate::tests::{assert_closed, assert_outward, face_at, only_face, rect, unit_box, v};
use crate::*;

fn circle(m: &mut Mesh, centre: DVec3, r: f64, n: usize) {
    let pts: Vec<DVec3> = (0..n)
        .map(|i| {
            let a = i as f64 / n as f64 * std::f64::consts::TAU;
            centre + v(a.cos() * r, a.sin() * r, 0.0)
        })
        .collect();
    let lines: Vec<_> = (0..n).map(|i| (pts[i], pts[(i + 1) % n])).collect();
    m.add_lines(&lines, None).unwrap();
}

fn all(m: &Mesh) -> Vec<Element> {
    m.edges().map(|(e, _)| Element::Edge(e)).chain(m.faces().map(|(f, _)| Element::Face(f))).collect()
}

#[test]
fn rotating_a_box_keeps_it_closed() {
    let mut m = unit_box();
    let rot = DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2);
    m.transform_elements(&all(&m), |p| rot.transform_point3(p)).unwrap();
    assert!(m.find_vertex(v(-1.0, 1.0, 1.0)).is_some());
    assert_closed(&m);
    assert_outward(&m, v(-0.5, 0.5, 0.5));
}

#[test]
fn scaling_the_top_face_makes_a_frustum() {
    let mut m = unit_box();
    let top = face_at(&m, v(0.5, 0.5, 1.0)).unwrap();
    let c = v(0.5, 0.5, 1.0);
    m.transform_elements(&[Element::Face(top)], |p| c + (p - c) * 0.5).unwrap();
    // The sides become trapezoids, still flat.
    assert_eq!(m.face_count(), 6);
    assert!(m.find_vertex(v(0.75, 0.75, 1.0)).is_some());
    assert_closed(&m);
}

#[test]
fn mirrored_copy_faces_outwards() {
    let mut m = unit_box();
    let elements = all(&m);
    m.copy_elements_with(&elements, 1, |p, _| v(-p.x - 2.0, p.y, p.z)).unwrap();
    assert_eq!(m.face_count(), 12);
    assert_closed(&m);
    for (f, _) in m.faces() {
        let c = m.loop_points(&m.face(f).unwrap().outer).iter().copied().sum::<DVec3>() / 4.0;
        let centre = if c.x < -1.0 { v(-2.5, 0.5, 0.5) } else { v(0.5, 0.5, 0.5) };
        assert!(m.face_normal(f).unwrap().dot(c - centre) > 0.0);
    }
}

#[test]
fn radial_copies() {
    let mut m = Mesh::new();
    rect(&mut m, 2.0, -0.25, 2.5, 0.25, 0.0);
    let f = only_face(&m);
    m.copy_elements_with(&[Element::Face(f)], 5, |p, k| DMat4::from_rotation_z(k as f64 * 1.0).transform_point3(p))
        .unwrap();
    assert_eq!(m.face_count(), 6);
}

#[test]
fn extract_and_stamp_back() {
    let mut m = unit_box();
    rect(&mut m, 3.0, 0.0, 4.0, 1.0, 0.0);
    let ground = face_at(&m, v(3.5, 0.5, 0.0)).unwrap();
    let boxed: Vec<Element> = m.faces().filter(|(f, _)| *f != ground).map(|(f, _)| Element::Face(f)).collect();
    let ids_before: BTreeSet<u64> = m.vertices().map(|(v, _)| v.0).collect();
    let part = m.extract(&boxed);
    assert_eq!(part.face_count(), 6);
    assert_eq!(part.edge_count(), 12);
    assert_closed(&part);
    // Only the separate rectangle stays.
    assert_eq!(m.face_count(), 1);
    assert_eq!(m.edge_count(), 4);
    // Fresh ids: none of the extracted vertices reuse an id that was in use.
    assert!(part.vertices().all(|(v, _)| !ids_before.contains(&v.0)));
    assert!(m.next_id() >= part.next_id());
    // Put it back, moved.
    let t = Template::whole(&part);
    m.stamp(&t, |p| p + v(0.0, 3.0, 0.0)).unwrap();
    assert_eq!(m.face_count(), 7);
    m.validate().unwrap();
}

#[test]
fn pulled_circle_has_soft_smooth_sides_and_box_does_not() {
    let mut m = Mesh::new();
    circle(&mut m, v(0.0, 0.0, 0.0), 1.0, 24);
    let f = only_face(&m);
    m.push_pull(f, -1.0, false).unwrap();
    let soft = m.edges().filter(|(_, e)| e.soft && e.smooth).count();
    assert_eq!(soft, 24);
    assert_closed(&m);
    let normals = m.smooth_normals();
    assert!(!normals.is_empty());
    // A corner on a side face gets a normal between two side faces, not the face's.
    let ((side, vx), n) = normals.iter().find(|((f, _), _)| m.face_normal(*f).unwrap().z.abs() < 0.1).unwrap();
    let _ = vx;
    assert!(n.dot(m.face_normal(*side).unwrap()) < 0.999);

    let b = unit_box();
    assert_eq!(b.edges().filter(|(_, e)| e.soft).count(), 0);
}

#[test]
fn soften_and_harden() {
    let mut m = unit_box();
    let edges: Vec<EdgeId> = m.edges().map(|(e, _)| e).collect();
    assert_eq!(m.soften_by_angle(&edges, 20.0), 0);
    assert_eq!(m.soften_by_angle(&edges, 100.0), 12);
    m.set_edge_flags(&edges, Some(false), Some(false), None);
    assert!(m.edges().all(|(_, e)| e.visible()));
    m.set_edge_flags(&edges[..1], None, None, Some(true));
    assert!(!m.edge(edges[0]).unwrap().visible());
}

#[test]
fn offset_face_inwards_and_outwards() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 2.0, 1.0, 0.0);
    let f = only_face(&m);
    m.offset_face(f, 0.25).unwrap();
    assert_eq!(m.face_count(), 2);
    let inner = face_at(&m, v(1.0, 0.5, 0.0)).unwrap();
    assert!((m.face_area(inner) - 0.75).abs() < 1e-9);
    m.validate().unwrap();

    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 2.0, 1.0, 0.0);
    let f = only_face(&m);
    m.offset_face(f, -0.5).unwrap();
    assert_eq!(m.face_count(), 2);
    let ring = face_at(&m, v(-0.25, 0.5, 0.0)).unwrap();
    assert!((m.face_area(ring) - (3.0 * 2.0 - 2.0)).abs() < 1e-9);
}

#[test]
fn offset_that_does_not_fit_is_refused() {
    let mut m = Mesh::new();
    rect(&mut m, 0.0, 0.0, 2.0, 1.0, 0.0);
    let f = only_face(&m);
    let before = m.clone();
    assert_eq!(m.offset_face(f, 0.6), Err(MeshError::OffsetTooLarge));
    assert_eq!(m, before);
}

#[test]
fn offset_an_l_shaped_face() {
    let mut m = Mesh::new();
    let p =
        [v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), v(2.0, 1.0, 0.0), v(1.0, 1.0, 0.0), v(1.0, 2.0, 0.0), v(0.0, 2.0, 0.0)];
    let lines: Vec<_> = (0..6).map(|i| (p[i], p[(i + 1) % 6])).collect();
    m.add_lines(&lines, Some(DVec3::Z)).unwrap();
    let f = only_face(&m);
    m.offset_face(f, 0.1).unwrap();
    assert_eq!(m.face_count(), 2);
    m.validate().unwrap();
}

#[test]
fn offset_an_edge_chain() {
    let mut m = Mesh::new();
    m.add_lines(&[(v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0)), (v(2.0, 0.0, 0.0), v(2.0, 2.0, 0.0))], None).unwrap();
    let edges: Vec<EdgeId> = m.edges().map(|(e, _)| e).collect();
    let (chain, closed) = m.edge_chain(&edges).unwrap();
    assert_eq!(chain.len(), 3);
    assert!(!closed);
    m.offset_chain(&edges, DVec3::Z, 0.5).unwrap();
    assert_eq!(m.edge_count(), 4);
    // Whichever end the chain started from, the copy runs half a metre away.
    let has = |p: DVec3| m.find_vertex(p).is_some();
    assert!((has(v(1.5, 0.5, 0.0)) && has(v(0.0, 0.5, 0.0))) || (has(v(2.5, -0.5, 0.0)) && has(v(0.0, -0.5, 0.0))));
}

#[test]
fn chains_close_and_branch() {
    let m = unit_box();
    let top: Vec<EdgeId> = m.face_edges(face_at(&m, v(0.5, 0.5, 1.0)).unwrap());
    let (chain, closed) = m.edge_chain(&top).unwrap();
    assert!(closed && chain.len() == 4);
    let corner_edges = m.edges_of_vertex(m.find_vertex(v(0.0, 0.0, 0.0)).unwrap());
    assert!(m.edge_chain(&corner_edges).is_none());
}

#[test]
fn follow_me_along_an_l_path() {
    let mut m = Mesh::new();
    // A square profile in the YZ plane at the origin, and a path along X then Y.
    let s = [v(0.0, -0.25, 0.0), v(0.0, 0.25, 0.0), v(0.0, 0.25, 0.5), v(0.0, -0.25, 0.5)];
    let lines: Vec<_> = (0..4).map(|i| (s[i], s[(i + 1) % 4])).collect();
    m.add_lines(&lines, Some(-DVec3::X)).unwrap();
    let profile = only_face(&m);
    let path = [v(0.0, 0.0, 0.0), v(3.0, 0.0, 0.0), v(3.0, 3.0, 0.0)];
    let faces = m.follow_me(profile, &path, false).unwrap();
    // Two caps and four sides on each of the two legs.
    assert_eq!(faces.len(), 10);
    assert_eq!(m.face_count(), 10);
    assert_closed(&m);
    // The mitre at the corner: a profile corner lands on the diagonal plane.
    assert!(m.find_vertex(v(3.25, -0.25, 0.0)).is_some());
    // Square sides meet at right angles, so nothing is soft.
    assert_eq!(m.edges().filter(|(_, e)| e.soft).count(), 0);
}

#[test]
fn follow_me_round_a_closed_path_makes_a_ring() {
    let mut m = Mesh::new();
    // A small square profile standing at the corner of a 2 m square path.
    let s = [v(0.0, 0.0, 0.0), v(0.0, 0.0, 0.3), v(-0.3, 0.0, 0.3), v(-0.3, 0.0, 0.0)];
    let lines: Vec<_> = (0..4).map(|i| (s[i], s[(i + 1) % 4])).collect();
    m.add_lines(&lines, None).unwrap();
    let profile = only_face(&m);
    let path = [v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), v(2.0, 2.0, 0.0), v(0.0, 2.0, 0.0)];
    m.follow_me(profile, &path, true).unwrap();
    // Four legs of four sides; no caps.
    assert_eq!(m.face_count(), 16);
    assert_closed(&m);
}

#[test]
fn follow_me_circle_along_a_line_is_a_soft_cylinder() {
    let mut m = Mesh::new();
    let n = 16;
    let pts: Vec<DVec3> = (0..n)
        .map(|i| {
            let a = i as f64 / n as f64 * std::f64::consts::TAU;
            v(0.0, a.cos() * 0.5, a.sin() * 0.5)
        })
        .collect();
    let lines: Vec<_> = (0..n).map(|i| (pts[i], pts[(i + 1) % n])).collect();
    m.add_lines(&lines, None).unwrap();
    let profile = only_face(&m);
    m.follow_me(profile, &[v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0)], false).unwrap();
    assert_eq!(m.face_count(), n + 2);
    assert_closed(&m);
    assert_eq!(m.edges().filter(|(_, e)| e.soft).count(), n);
}

#[test]
fn offset_face_with_hole_offsets_both_boundaries_and_preserves_area() {
    let mut mesh = Mesh::new();
    rect(&mut mesh, 0., 0., 4., 4., 0.);
    rect(&mut mesh, 1.5, 1.5, 2.5, 2.5, 0.);
    let ring = face_at(&mesh, v(0.5, 0.5, 0.)).unwrap();
    assert_eq!(mesh.face(ring).unwrap().holes.len(), 1);
    let loops = mesh.offset_face_loops(ring, 0.25).unwrap();
    assert_eq!(loops.len(), 2);
    assert!(loops[0].iter().all(|p| p.x >= 0.25 && p.x <= 3.75 && p.y >= 0.25 && p.y <= 3.75));
    assert!(loops[1].iter().all(|p| p.x >= 1.25 && p.x <= 2.75 && p.y >= 1.25 && p.y <= 2.75));
    mesh.offset_face(ring, 0.25).unwrap();
    mesh.validate().unwrap();
    let inner_ring = face_at(&mesh, v(0.5, 0.5, 0.)).unwrap();
    assert!((mesh.face_area(inner_ring) - 10.).abs() < 1e-9);
    assert!((mesh.faces().map(|(id, _)| mesh.face_area(id)).sum::<f64>() - 16.).abs() < 1e-9);
}
#[test]
fn offset_hole_collapse_or_boundary_collision_is_atomic() {
    let mut mesh = Mesh::new();
    rect(&mut mesh, 0., 0., 4., 4., 0.);
    rect(&mut mesh, 1.5, 1.5, 2.5, 2.5, 0.);
    let ring = face_at(&mesh, v(0.5, 0.5, 0.)).unwrap();
    let before = mesh.clone();
    for distance in [0.75, 0.8, -0.5, -0.6] {
        assert_eq!(mesh.offset_face(ring, distance), Err(MeshError::OffsetTooLarge));
        assert_eq!(mesh, before);
    }
    let loops = mesh.offset_face_loops(ring, -0.25).unwrap();
    assert!(loops[0].iter().all(|p| p.x >= -0.25 && p.x <= 4.25));
    assert!(loops[1].iter().all(|p| p.x >= 1.75 && p.x <= 2.25));
}

#[test]
fn differently_painted_collinear_edges_do_not_heal_away_their_color_boundary() {
    let mut mesh = Mesh::new();
    mesh.add_lines(&[(DVec3::ZERO, DVec3::X), (DVec3::X, DVec3::X * 2.)], None).unwrap();
    let ids: Vec<_> = mesh.edges().map(|(id, _)| id).collect();
    mesh.paint_edge(ids[0], Some(MaterialId(100))).unwrap();
    mesh.paint_edge(ids[1], Some(MaterialId(101))).unwrap();
    mesh.heal_vertex(mesh.find_vertex(DVec3::X).unwrap());
    assert_eq!(mesh.edge_count(), 2);
    assert_eq!(mesh.edge(ids[0]).unwrap().material, Some(MaterialId(100)));
    assert_eq!(mesh.edge(ids[1]).unwrap().material, Some(MaterialId(101)));
}
#[test]
fn push_pull_preserves_painted_boundary_edges_on_the_new_cap() {
    let mut mesh = Mesh::new();
    rect(&mut mesh, 0., 0., 1., 1., 0.);
    let face = only_face(&mesh);
    let edges = mesh.face_edges(face);
    for id in edges {
        mesh.paint_edge(id, Some(MaterialId(100))).unwrap();
    }
    let result = mesh.push_pull(face, -1., false).unwrap();
    let cap = result.face.unwrap();
    assert!(mesh.face_edges(cap).iter().all(|id| mesh.edge(*id).unwrap().material == Some(MaterialId(100))));
    mesh.validate().unwrap();
}

#[test]
fn locate_agrees_with_find_vertex() {
    let mut m = unit_box();
    let at = DVec3::new(2., 3., 4.);
    m.add_vertex(at);
    let points = [DVec3::ZERO, DVec3::new(1., 1., 1.), at + DVec3::X * 5e-7, at + DVec3::X * 3e-6, DVec3::ONE * -9.];
    let found = m.locate(&points, |_| true);
    let expected: Vec<_> = points.iter().map(|p| m.find_vertex(*p)).collect();
    assert_eq!(found, expected);
    let origin = m.find_vertex(DVec3::ZERO).unwrap();
    assert_eq!(m.locate(&points[..1], |v| v != origin), vec![None]);
}

#[test]
fn copying_a_clear_shell_matches_copying_through_the_lines() {
    let mut original = unit_box();
    let all: Vec<_> = original
        .faces()
        .map(|(id, _)| Element::Face(id))
        .chain(original.edges().map(|(id, _)| Element::Edge(id)))
        .collect();
    let faces: Vec<_> = original.faces().map(|(id, _)| id).collect();
    original.face_mut(faces[0]).unwrap().material = Some(MaterialId(7));
    let template = Template::of(&original, &all);
    assert_eq!(template.shells().len(), 1);
    for mirror in [1., -1.] {
        let map = |p: DVec3| DVec3::new(p.x * mirror + 9., p.y, p.z);
        let mut direct = original.clone();
        direct.copy_elements_with(&all, 1, |p, _| map(p)).unwrap();
        let mut lines = original.clone();
        lines.stamp(&template, map).unwrap();
        assert_eq!((direct.face_count(), direct.edge_count()), (12, 24));
        assert_eq!((lines.face_count(), lines.edge_count()), (12, 24));
        direct.validate().unwrap();
        let shape = |m: &Mesh| {
            let mut out: Vec<_> = m
                .faces()
                .map(|(id, f)| {
                    let n = m.face_normal(id).unwrap();
                    let centre = m.loop_points(&f.outer).iter().sum::<DVec3>() / 4.;
                    format!("{:.3} {:.3} {:?}", centre, n, f.material)
                })
                .collect();
            out.sort();
            out
        };
        assert_eq!(shape(&direct), shape(&lines));
    }
}

#[test]
fn a_copy_that_lands_on_geometry_still_joins_it() {
    let mut m = unit_box();
    let all: Vec<_> = m.faces().map(|(id, _)| Element::Face(id)).collect();
    m.copy_elements_with(&all, 1, |p, _| p + DVec3::X).unwrap();
    assert_eq!(m.vertex_count(), 12);
}

#[test]
fn edges_of_a_vertex_follow_every_edit() {
    let mut m = unit_box();
    let faces: Vec<_> = m.faces().map(|(id, _)| id).collect();
    m.push_pull(faces[0], 0.5, false).unwrap();
    m.add_line(DVec3::ZERO, DVec3::new(1., 1., 0.), None).unwrap();
    let mut check = m.clone();
    check.reindex();
    assert_eq!(m, check);
    for (v, _) in m.vertices() {
        let scan: Vec<_> = m.edges().filter(|(_, e)| e.v.contains(&v)).map(|(id, _)| id).collect();
        assert_eq!(m.edges_of_vertex(v), scan);
    }
}

#[test]
fn a_move_near_other_geometry_takes_the_full_path() {
    let m = unit_box();
    let all: Vec<_> = m.faces().map(|(id, _)| Element::Face(id)).collect();
    let mut far = m.clone();
    far.copy_elements_with(&all, 1, |p, _| p + DVec3::X * 5.).unwrap();
    let copy: Vec<_> =
        far.faces().filter(|(id, _)| !all.contains(&Element::Face(*id))).map(|(id, _)| Element::Face(id)).collect();
    // Clear of the first box: ids stay.
    let ids: Vec<_> = far.faces().map(|(id, _)| id).collect();
    far.move_elements(&copy, DVec3::Y).unwrap();
    assert_eq!(far.faces().map(|(id, _)| id).collect::<Vec<_>>(), ids);
    // Onto the first box: the touching corners weld.
    let vertices = far.vertex_count();
    far.move_elements(&copy, DVec3::new(-4., -1., 0.)).unwrap();
    assert!(far.vertex_count() < vertices);
    m.validate().unwrap();
}
