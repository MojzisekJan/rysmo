#[test]
fn overflowing_move_is_atomic() {
    use rysmo_math::DVec3;
    let mut mesh = crate::Mesh::new();
    let edges = mesh.add_lines(&[(DVec3::new(1e308, 0., 0.), DVec3::new(1e308, 1., 0.))], None).unwrap();
    let before = mesh.clone();
    let elements = edges.into_iter().map(crate::Element::Edge).collect::<Vec<_>>();
    assert!(mesh.move_elements(&elements, DVec3::X * 1e308).is_err());
    assert_eq!(mesh, before);
}

#[test]
fn a_missing_transform_vertex_does_not_move_other_vertices() {
    let mut mesh = crate::Mesh::new();
    mesh.add_lines(&[(rysmo_math::DVec3::ZERO, rysmo_math::DVec3::X)], None).unwrap();
    let before = mesh.clone();
    let vertex = mesh.vertices().next().unwrap().0;
    let selected = std::collections::BTreeSet::from([vertex, crate::VertexId(9999)]);
    assert!(mesh.move_vertices(&selected, rysmo_math::DVec3::Y).is_err());
    assert_eq!(mesh, before);
}
#[test]
fn constructing_mesh_with_maximum_id_is_fallible() {
    let vertices =
        std::collections::BTreeMap::from([(crate::VertexId(u64::MAX), crate::Vertex { pos: rysmo_math::DVec3::ZERO })]);
    assert!(crate::Mesh::from_parts(vertices, Default::default(), Default::default()).is_err());
}

#[test]
fn copying_geometry_rejects_non_finite_results() {
    use rysmo_math::DVec3;
    for bad in [f64::INFINITY, f64::NAN] {
        let mut mesh = crate::Mesh::new();
        let edges = mesh.add_lines(&[(DVec3::ZERO, DVec3::X)], None).unwrap();
        let elements = edges.into_iter().map(crate::Element::Edge).collect::<Vec<_>>();
        let before = mesh.clone();
        assert!(mesh.copy_elements_with(&elements, 1, |p, _| p + DVec3::splat(bad)).is_err());
        assert_eq!(mesh, before);
    }
}

#[test]
fn stamping_geometry_rejects_non_finite_results() {
    use rysmo_math::DVec3;
    let mut mesh = crate::Mesh::new();
    mesh.add_lines(&[(DVec3::ZERO, DVec3::X)], None).unwrap();
    let template = crate::Template::whole(&mesh);
    let before = mesh.clone();
    assert!(mesh.stamp(&template, |_| DVec3::splat(f64::NAN)).is_err());
    assert_eq!(mesh, before);
}
