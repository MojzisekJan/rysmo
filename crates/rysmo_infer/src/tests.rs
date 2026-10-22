use rysmo_math::{Axis, DVec2, DVec3, OrbitCamera, Plane, View};
use rysmo_mesh::Mesh;

use crate::*;

fn unit_box() -> Mesh {
    let mut m = Mesh::new();
    let p = [DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y];
    let lines: Vec<_> = (0..4).map(|i| (p[i], p[(i + 1) % 4])).collect();
    m.add_lines(&lines, None).unwrap();
    let f = m.faces().next().unwrap().0;
    m.push_pull(f, -1.0, false).unwrap();
    m
}

fn view() -> View {
    // Looking from the front right, above: the corner (1, 0, 1) faces the camera and
    // (0, 1, 0) is hidden behind the box.
    let cam = OrbitCamera {
        target: DVec3::new(0.5, 0.5, 0.5),
        distance: 6.0,
        yaw: -0.8,
        pitch: 0.5,
        fov_y: 0.7,
        parallel: false,
        eye_base_z: 0.0,
    };
    cam.view(DVec2::new(800.0, 600.0))
}

fn query<'a>(m: &'a Mesh, v: &'a View, at: DVec3, from: Option<DVec3>, hints: &'a [DVec3]) -> Inference {
    let cursor = v.project(at).unwrap();
    infer(&Query {
        show_hidden: false,
        clips: Vec::new(),
        axes: Default::default(),
        mesh: m,
        view: v,
        cursor,
        from,
        lock: None,
        hints,
        scale: 1.0,
        others: &[],
        guides: &[],
    })
}

#[test]
fn snaps_to_a_visible_endpoint() {
    let m = unit_box();
    let v = view();
    let i = query(&m, &v, DVec3::new(1.0, 0.0, 1.0), None, &[]);
    assert_eq!(i.kind, Kind::Endpoint);
    assert!(i.point.distance(DVec3::new(1.0, 0.0, 1.0)) < 1e-9);
    assert!(i.edge.is_some());
    assert_eq!(i.kind.tooltip(), "Endpoint");
}

#[test]
fn endpoint_snaps_from_a_few_pixels_away() {
    let m = unit_box();
    let v = view();
    let c = v.project(DVec3::new(1.0, 0.0, 1.0)).unwrap() + DVec2::new(4.0, -3.0);
    let i = infer(&Query {
        show_hidden: false,
        clips: Vec::new(),
        axes: Default::default(),
        mesh: &m,
        view: &v,
        cursor: c,
        from: None,
        lock: None,
        hints: &[],
        scale: 1.0,
        others: &[],
        guides: &[],
    });
    assert_eq!(i.kind, Kind::Endpoint);
}

#[test]
fn hidden_endpoint_is_ignored() {
    let m = unit_box();
    let v = view();
    let i = query(&m, &v, DVec3::new(0.0, 1.0, 0.0), None, &[]);
    assert_ne!(i.kind, Kind::Endpoint);
}

#[test]
fn snaps_to_a_midpoint() {
    let m = unit_box();
    let v = view();
    let i = query(&m, &v, DVec3::new(0.5, 0.0, 1.0), None, &[]);
    assert_eq!(i.kind, Kind::Midpoint);
    assert!(i.point.distance(DVec3::new(0.5, 0.0, 1.0)) < 1e-9);
}

#[test]
fn snaps_onto_an_edge() {
    let m = unit_box();
    let v = view();
    let i = query(&m, &v, DVec3::new(0.2, 0.0, 1.0), None, &[]);
    assert_eq!(i.kind, Kind::OnEdge);
    assert!(i.point.distance(DVec3::new(0.2, 0.0, 1.0)) < 1e-6);
    assert!(i.edge.is_some());
}

#[test]
fn finds_a_face_and_its_plane() {
    let m = unit_box();
    let v = view();
    let i = query(&m, &v, DVec3::new(0.3, 0.4, 1.0), None, &[]);
    assert_eq!(i.kind, Kind::OnFace);
    assert!(i.point.distance(DVec3::new(0.3, 0.4, 1.0)) < 1e-6);
    let plane = i.plane.unwrap();
    assert!(plane.normal.z > 0.99);
    assert!(i.face.is_some());
}

#[test]
fn falls_back_to_the_ground() {
    let m = unit_box();
    let v = view();
    let i = query(&m, &v, DVec3::new(3.0, -2.0, 0.0), None, &[]);
    assert_eq!(i.kind, Kind::Ground);
    assert!(i.point.distance(DVec3::new(3.0, -2.0, 0.0)) < 1e-6);
}

#[test]
fn snaps_to_the_origin() {
    let m = Mesh::new();
    let v = view();
    let i = query(&m, &v, DVec3::ZERO, None, &[]);
    assert_eq!(i.kind, Kind::Origin);
}

#[test]
fn infers_axes_from_the_start_point() {
    let m = Mesh::new();
    let v = view();
    let from = DVec3::new(2.0, -1.0, 0.0);
    for (a, target) in
        [(Axis::X, from + DVec3::X * 1.5), (Axis::Y, from + DVec3::Y * 1.5), (Axis::Z, from + DVec3::Z * 1.5)]
    {
        let i = query(&m, &v, target, Some(from), &[]);
        assert_eq!(i.kind, Kind::OnAxis(a), "expected the {a:?} axis");
        assert!(i.point.distance(target) < 1e-6);
        assert!(i.guide.is_some());
    }
    assert_eq!(Kind::OnAxis(Axis::Y).tooltip(), "On Green Axis");
}

#[test]
fn axis_inference_tolerates_a_slightly_off_cursor() {
    let m = Mesh::new();
    let v = view();
    let from = DVec3::new(2.0, -1.0, 0.0);
    let c = v.project(from + DVec3::X * 1.5).unwrap() + DVec2::new(0.0, 4.0);
    let i = infer(&Query {
        show_hidden: false,
        clips: Vec::new(),
        axes: Default::default(),
        mesh: &m,
        view: &v,
        cursor: c,
        from: Some(from),
        lock: None,
        hints: &[],
        scale: 1.0,
        others: &[],
        guides: &[],
    });
    assert_eq!(i.kind, Kind::OnAxis(Axis::X));
    assert!((i.point.y - from.y).abs() < 1e-9 && (i.point.z - from.z).abs() < 1e-9);
}

#[test]
fn two_axis_lines_cross() {
    let m = Mesh::new();
    let v = view();
    let from = DVec3::new(3.0, -1.0, 0.0);
    let hint = [DVec3::new(1.5, 1.0, 0.0)];
    let i = query(&m, &v, DVec3::new(3.0, 1.0, 0.0), Some(from), &hint);
    assert_eq!(i.kind, Kind::Intersection);
    assert!(i.point.distance(DVec3::new(3.0, 1.0, 0.0)) < 1e-9);
}

#[test]
fn from_point_inference_without_a_start() {
    let m = Mesh::new();
    let v = view();
    let hint = [DVec3::new(1.5, 1.0, 0.0)];
    let i = query(&m, &v, DVec3::new(3.5, 1.0, 0.0), None, &hint);
    assert_eq!(i.kind, Kind::FromPoint(Axis::X));
}

#[test]
fn locked_line_slides_and_borrows_points() {
    let m = unit_box();
    let v = view();
    let origin = DVec3::new(3.0, 0.0, 0.0);
    let lock = Some(Lock::Line { origin, dir: DVec3::Z, axis: Some(Axis::Z) });
    // Far from the line, the point still lies on it.
    let c = v.project(DVec3::new(2.0, -2.0, 0.5)).unwrap();
    let i = infer(&Query {
        show_hidden: false,
        clips: Vec::new(),
        axes: Default::default(),
        mesh: &m,
        view: &v,
        cursor: c,
        from: Some(origin),
        lock,
        hints: &[],
        scale: 1.0,
        others: &[],
        guides: &[],
    });
    assert!((i.point.x - 3.0).abs() < 1e-9 && i.point.y.abs() < 1e-9);
    // Hovering the box's top corner matches its height.
    let c = v.project(DVec3::new(1.0, 0.0, 1.0)).unwrap();
    let i = infer(&Query {
        show_hidden: false,
        clips: Vec::new(),
        axes: Default::default(),
        mesh: &m,
        view: &v,
        cursor: c,
        from: Some(origin),
        lock,
        hints: &[],
        scale: 1.0,
        others: &[],
        guides: &[],
    });
    assert!(i.point.distance(DVec3::new(3.0, 0.0, 1.0)) < 1e-9);
}

#[test]
fn locked_plane_projects() {
    let m = Mesh::new();
    let v = view();
    let plane = Plane::new(DVec3::Z, DVec3::new(0.0, 0.0, 2.0));
    let c = v.project(DVec3::new(1.0, 1.0, 2.0)).unwrap();
    let i = infer(&Query {
        show_hidden: false,
        clips: Vec::new(),
        axes: Default::default(),
        mesh: &m,
        view: &v,
        cursor: c,
        from: None,
        lock: Some(Lock::Plane(plane)),
        hints: &[],
        scale: 1.0,
        others: &[],
        guides: &[],
    });
    assert!(i.point.distance(DVec3::new(1.0, 1.0, 2.0)) < 1e-6);
}

#[test]
fn shift_lock_follows_the_inference() {
    let m = unit_box();
    let v = view();
    let from = DVec3::new(2.0, -1.0, 0.0);
    let i = query(&m, &v, from + DVec3::X, Some(from), &[]);
    assert_eq!(i.lock_for(Some(from), &m), Some(Lock::Line { origin: from, dir: DVec3::X, axis: Some(Axis::X) }));
    let f = query(&m, &v, DVec3::new(0.3, 0.4, 1.0), None, &[]);
    assert!(matches!(f.lock_for(None, &m), Some(Lock::Plane(_))));
}

#[test]
fn pick_edge_finds_the_nearest_visible_edge() {
    let m = unit_box();
    let v = view();
    let c = v.project(DVec3::new(0.3, 0.0, 1.0)).unwrap();
    let e = pick_edge(&m, &v, c, 1.0).unwrap();
    let (a, b) = m.edge_points(e);
    assert!((a.z - 1.0).abs() < 1e-9 && (b.z - 1.0).abs() < 1e-9 && a.y.abs() < 1e-9 && b.y.abs() < 1e-9);
}

#[test]
fn hidden_edge_is_ignored() {
    let m = unit_box();
    let v = view();
    // The back vertical edge at (0, 1) is behind the box from this camera.
    let i = query(&m, &v, DVec3::new(0.0, 1.0, 0.5), None, &[]);
    assert_eq!(i.kind, Kind::OnFace, "got {:?} at {:?}", i.kind, i.point);
}

#[test]
fn snaps_to_a_placed_component_and_picks_it() {
    let comp = unit_box();
    let empty = Mesh::new();
    let v = view();
    let t = rysmo_math::DMat4::from_translation(DVec3::new(2.0, 0.0, 0.0));
    let iid = rysmo_mesh::InstanceId(99);
    let others =
        [Other { clips: Vec::new(), mesh: std::borrow::Cow::Borrowed(&comp), to_local: t, instance: Some(iid) }];
    let q = |at: DVec3| {
        let cursor = v.project(at).unwrap();
        Query {
            show_hidden: false,
            clips: Vec::new(),
            axes: Default::default(),
            mesh: &empty,
            view: &v,
            cursor,
            from: None,
            lock: None,
            hints: &[],
            scale: 1.0,
            others: &others,
            guides: &[],
        }
    };
    let i = infer(&q(DVec3::new(3.0, 0.0, 1.0)));
    assert_eq!(i.kind, Kind::Endpoint);
    assert_eq!(i.instance, Some(iid));
    assert!(i.edge.is_none());
    let f = infer(&q(DVec3::new(2.5, 0.5, 1.0)));
    assert_eq!(f.kind, Kind::OnFace);
    assert!((f.point - DVec3::new(2.5, 0.5, 1.0)).length() < 1e-6);
    assert!(f.plane.unwrap().normal.z > 0.99);
    assert_eq!(pick(&q(DVec3::new(2.5, 0.5, 1.0))), Pick::Element(rysmo_mesh::Element::Instance(iid)));
    assert_eq!(pick(&q(DVec3::new(6.0, -3.0, 0.0))), Pick::Nothing);
    // Geometry outside the context is not selectable.
    let outside = [Other { clips: Vec::new(), mesh: std::borrow::Cow::Borrowed(&comp), to_local: t, instance: None }];
    let cursor = v.project(DVec3::new(2.5, 0.5, 1.0)).unwrap();
    let qo = Query {
        show_hidden: false,
        clips: Vec::new(),
        axes: Default::default(),
        mesh: &empty,
        view: &v,
        cursor,
        from: None,
        lock: None,
        hints: &[],
        scale: 1.0,
        others: &outside,
        guides: &[],
    };
    assert_eq!(pick(&qo), Pick::Outside);
}

#[test]
fn guides_snap_and_pick() {
    let m = Mesh::new();
    let v = view();
    let g = [
        (rysmo_mesh::GuideId(5), SnapGuide::Line { origin: DVec3::new(0.0, -1.0, 0.0), dir: DVec3::X }),
        (rysmo_mesh::GuideId(6), SnapGuide::Point(DVec3::new(1.0, 2.0, 0.0))),
    ];
    let q = |at: DVec3| {
        let cursor = v.project(at).unwrap();
        Query {
            show_hidden: false,
            clips: Vec::new(),
            axes: Default::default(),
            mesh: &m,
            view: &v,
            cursor,
            from: None,
            lock: None,
            hints: &[],
            scale: 1.0,
            others: &[],
            guides: &g,
        }
    };
    let on = infer(&q(DVec3::new(3.0, -1.0, 0.0)));
    assert_eq!(on.kind, Kind::OnGuide);
    assert_eq!(on.kind.tooltip(), "On Line");
    let pt = infer(&q(DVec3::new(1.0, 2.0, 0.0)));
    assert_eq!(pt.kind, Kind::GuidePoint);
    assert_eq!(pick(&q(DVec3::new(1.0, 2.0, 0.0))), Pick::Element(rysmo_mesh::Element::Guide(rysmo_mesh::GuideId(6))));
}

#[test]
fn soft_edges_do_not_snap() {
    let mut m = unit_box();
    let all: Vec<_> = m.edges().map(|(e, _)| e).collect();
    m.set_edge_flags(&all, Some(true), Some(true), None);
    let v = view();
    let i = query(&m, &v, DVec3::new(0.2, 0.0, 1.0), None, &[]);
    assert_ne!(i.kind, Kind::OnEdge);
    assert_ne!(i.kind, Kind::Midpoint);
}

#[test]
fn custom_axes_origin_axis_and_shift_lock_follow_the_frame() {
    let m = Mesh::new();
    let v = view();
    let axes =
        rysmo_math::DrawingAxes::new(DVec3::new(2.0, 2.0, 0.0), DVec3::new(1.0, 1.0, 0.0), DVec3::new(-1.0, 1.0, 0.0))
            .unwrap();
    let origin = axes.origin;
    let point = origin + axes.x;
    let mut q = Query {
        show_hidden: false,
        clips: Vec::new(),
        axes,
        mesh: &m,
        view: &v,
        cursor: v.project(point).unwrap(),
        from: Some(origin),
        lock: None,
        hints: &[],
        scale: 1.0,
        others: &[],
        guides: &[],
    };
    let inf = infer(&q);
    assert_eq!(inf.kind, Kind::OnAxis(Axis::X));
    let lock = inf.lock_for(Some(origin), &m).unwrap();
    let Lock::Line { dir, .. } = lock else { panic!("axis lock") };
    assert!(dir.cross(axes.x).length() < 1e-7);
    q.from = None;
    q.cursor = v.project(origin).unwrap();
    assert_eq!(infer(&q).kind, Kind::Origin);
}

#[test]
fn overlapping_edge_pick_prefers_front_depth_in_either_source_order() {
    let mut line = Mesh::new();
    line.add_lines(&[(DVec3::X * -2., DVec3::X * 2.)], None).unwrap();
    let empty = Mesh::new();
    let mut camera = OrbitCamera { parallel: true, ..Default::default() };
    camera.set_standard_view(rysmo_math::StandardView::Top);
    let view = camera.view(DVec2::new(800., 600.));
    let front = rysmo_mesh::InstanceId(202);
    let back = rysmo_mesh::InstanceId(101);
    let mut others = [
        Other {
            clips: Vec::new(),
            mesh: std::borrow::Cow::Borrowed(&line),
            to_local: rysmo_math::DMat4::IDENTITY,
            instance: Some(back),
        },
        Other {
            clips: Vec::new(),
            mesh: std::borrow::Cow::Borrowed(&line),
            to_local: rysmo_math::DMat4::from_translation(DVec3::Z * 2.),
            instance: Some(front),
        },
    ];
    for reverse in [false, true] {
        if reverse {
            others.reverse();
        }
        let query = Query {
            show_hidden: false,
            clips: Vec::new(),
            axes: Default::default(),
            mesh: &empty,
            view: &view,
            cursor: view.project(DVec3::Z * 2.).unwrap(),
            from: None,
            lock: None,
            hints: &[],
            scale: 1.,
            others: &others,
            guides: &[],
        };
        assert_eq!(pick(&query), Pick::Element(Element::Instance(front)));
    }
}

#[test]
fn section_cut_faces_do_not_occlude_remaining_faces() {
    let mut mesh = Mesh::new();
    for z in [0., 2.] {
        let points = [DVec3::new(0., 0., z), DVec3::new(2., 0., z), DVec3::new(2., 2., z), DVec3::new(0., 2., z)];
        mesh.add_lines(&(0..4).map(|i| (points[i], points[(i + 1) % 4])).collect::<Vec<_>>(), Some(DVec3::Z)).unwrap();
    }
    let rear = mesh.faces().find(|(id, _)| mesh.face_contains(*id, DVec3::new(1., 1., 0.))).unwrap().0;
    let mut camera = OrbitCamera::default();
    camera.set_standard_view(rysmo_math::StandardView::Top);
    camera.frame(DVec3::ZERO, DVec3::new(2., 2., 2.));
    let view = camera.view(DVec2::new(800., 600.));
    let mut query = Query::simple(&mesh, &view, view.project(DVec3::new(1., 1., 0.)).unwrap(), 1.);
    query.clips.push(Plane::new(DVec3::Z, DVec3::Z));
    assert_eq!(pick(&query), Pick::Element(Element::Face(rear)));
}

#[test]
fn hidden_faces_and_edges_are_pickable_only_in_hidden_geometry_view() {
    let mut mesh = Mesh::new();
    let points = [DVec3::ZERO, DVec3::X * 2., DVec3::new(2., 2., 0.), DVec3::Y * 2.];
    let lines: Vec<_> = (0..4).map(|i| (points[i], points[(i + 1) % 4])).collect();
    mesh.add_lines(&lines, None).unwrap();
    let face = mesh.faces().next().unwrap().0;
    mesh.face_mut(face).unwrap().hidden = true;
    let edges: Vec<_> = mesh.edges().map(|(id, _)| id).collect();
    mesh.set_edge_flags(&edges, None, None, Some(true));
    let view = view();
    let cursor = view.project(DVec3::new(1., 1., 0.)).unwrap();
    let mut query = Query::simple(&mesh, &view, cursor, 1.);
    assert_eq!(pick(&query), Pick::Nothing);
    query.show_hidden = true;
    assert_eq!(pick(&query), Pick::Element(Element::Face(face)));
}

#[test]
fn acquired_rotated_edges_supply_parallel_perpendicular_and_shift_locks() {
    let mut m = Mesh::new();
    m.add_lines(&[(DVec3::ZERO, DVec3::new(1., 1., 0.))], None).unwrap();
    let v = view();
    for (target, kind) in [(DVec3::new(2., 2., 0.), Kind::Parallel), (DVec3::new(-1., 1., 0.), Kind::Perpendicular)] {
        let inf = query(&m, &v, target, Some(DVec3::ZERO), &[]);
        assert_eq!(inf.kind, kind);
        assert!(inf.point.distance(target) < 1e-8);
        assert!(matches!(inf.lock_for(Some(DVec3::ZERO), &m), Some(Lock::Line { axis: None, .. })));
    }
    let endpoint = query(&m, &v, DVec3::new(1., 1., 0.), Some(DVec3::ZERO), &[]);
    assert_eq!(endpoint.kind, Kind::Endpoint);
}

#[test]
fn unacquired_hidden_or_clipped_edges_do_not_supply_direction_inferences() {
    let mut m = Mesh::new();
    m.add_lines(&[(DVec3::ZERO, DVec3::new(1., 1., 0.))], None).unwrap();
    let v = view();
    let from = DVec3::new(0., -1., 0.);
    let target = DVec3::new(2., 1., 0.);
    assert_ne!(query(&m, &v, target, Some(from), &[]).kind, Kind::Parallel);
    assert_eq!(query(&m, &v, target, Some(from), &[DVec3::ZERO]).kind, Kind::Parallel);
    let ids: Vec<_> = m.edges().map(|(id, _)| id).collect();
    m.set_edge_flags(&ids, None, None, Some(true));
    assert_ne!(query(&m, &v, target, Some(from), &[DVec3::ZERO]).kind, Kind::Parallel);
}

#[test]
fn measured_guide_segment_is_pickable_only_between_its_endpoints() {
    let mesh = Mesh::new();
    let view = view();
    let id = rysmo_mesh::GuideId(100);
    let guides = [(id, SnapGuide::Segment { start: DVec3::new(-1., 0., 0.), point: DVec3::new(1., 0., 0.) })];
    let mut q = Query::simple(&mesh, &view, view.project(DVec3::new(0.4, 0., 0.)).unwrap(), 1.);
    q.guides = &guides;
    assert_eq!(pick(&q), Pick::Element(Element::Guide(id)));
    q.cursor = view.project(DVec3::new(2., 0., 0.)).unwrap();
    assert_ne!(pick(&q), Pick::Element(Element::Guide(id)));
    q.cursor = view.project(DVec3::new(1., 0., 0.)).unwrap();
    assert_eq!(infer(&q).kind, Kind::GuidePoint);
}
