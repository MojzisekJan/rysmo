//! Milestone 2 commands: arcs, rotate, scale, offset, Follow Me, groups and components,
//! tags, materials, guides and edge styles, each with undo and redo.

use std::collections::BTreeSet;

use rysmo_math::{DMat4, DVec3};

use crate::*;

fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

/// A unit box at `x` in the current context; returns its faces and edges.
fn add_box(e: &mut Editor, x: f64) -> Vec<Element> {
    let before: BTreeSet<FaceId> = e.mesh().faces().map(|(f, _)| f).collect();
    e.apply(Command::DrawRectangle { corner: v(x, 0.0, 0.0), u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
    let f = e.mesh().faces().map(|(f, _)| f).find(|f| !before.contains(f)).unwrap();
    e.apply(Command::PushPull { face: f, distance: -1.0, keep_base: false }).unwrap();
    let mut out: Vec<Element> =
        e.mesh().faces().map(|(f, _)| f).filter(|f| !before.contains(f)).map(Element::Face).collect();
    let edges: BTreeSet<EdgeId> = out
        .iter()
        .filter_map(|el| if let Element::Face(f) = el { Some(*f) } else { None })
        .flat_map(|f| e.mesh().face_edges(f))
        .collect();
    out.extend(edges.into_iter().map(Element::Edge));
    out
}

/// Undo then redo leaves the document as it was after the command.
fn round_trip(e: &mut Editor) {
    let after = e.doc().clone();
    assert!(e.undo());
    assert_ne!(e.doc(), &after);
    assert!(e.redo());
    assert_eq!(e.doc(), &after);
}

fn instance(el: Element) -> InstanceId {
    match el {
        Element::Instance(i) => i,
        other => panic!("expected an instance, got {other:?}"),
    }
}

/// Every id in the document is used once.
fn assert_unique_ids(doc: &Document) {
    let mut seen = BTreeSet::new();
    let mut add = |id: u64| assert!(seen.insert(id), "id {id} used twice");
    for d in std::iter::once(&doc.root).chain(doc.definitions.values()) {
        d.mesh.vertices().for_each(|(i, _)| add(i.0));
        d.mesh.edges().for_each(|(i, _)| add(i.0));
        d.mesh.faces().for_each(|(i, _)| add(i.0));
        d.instances.keys().for_each(|i| add(i.0));
        d.guides.keys().for_each(|i| add(i.0));
    }
    doc.definitions.keys().for_each(|i| add(i.0));
    doc.materials.keys().for_each(|i| add(i.0));
    doc.tags.keys().for_each(|i| add(i.0));
    assert!(seen.iter().all(|i| *i < doc.next_id()));
}

#[test]
fn arc_points_bulge_and_semicircle() {
    let pts = command::arc_points(v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), DVec3::Z, 1.0, 12).unwrap();
    assert_eq!(pts.len(), 13);
    // A bulge of half the chord is a semicircle about the chord's middle.
    for p in &pts {
        assert!((p.distance(v(1.0, 0.0, 0.0)) - 1.0).abs() < 1e-9);
        assert!(p.y >= -1e-9, "bulges to the left of the chord");
    }
    assert!(pts.iter().any(|p| p.distance(v(1.0, 1.0, 0.0)) < 1e-9));
    let shallow = command::arc_points(v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0), DVec3::Z, -0.2, 8).unwrap();
    assert!(shallow.iter().all(|p| p.y <= 1e-9));
    assert!(shallow.iter().any(|p| p.distance(v(1.0, -0.2, 0.0)) < 1e-9));
}

#[test]
fn arc_command_closes_a_face_with_a_line() {
    let mut e = Editor::new();
    e.apply(Command::DrawArc {
        start: v(0.0, 0.0, 0.0),
        end: v(2.0, 0.0, 0.0),
        normal: DVec3::Z,
        bulge: 1.0,
        segments: 12,
    })
    .unwrap();
    assert_eq!(e.mesh().edge_count(), 12);
    round_trip(&mut e);
    e.apply(Command::DrawLines { lines: vec![[v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0)]], facing: None }).unwrap();
    assert_eq!(e.mesh().face_count(), 1);
}

#[test]
fn rotate_and_radial_copies() {
    let mut e = Editor::new();
    let b = add_box(&mut e, 2.0);
    e.apply(Command::Rotate { elements: b.clone(), centre: DVec3::ZERO, axis: DVec3::Z, angle: 90.0, copies: 0 })
        .unwrap();
    assert!(e.mesh().find_vertex(v(-1.0, 3.0, 1.0)).is_some());
    round_trip(&mut e);
    e.apply(Command::Rotate { elements: b, centre: DVec3::ZERO, axis: DVec3::Z, angle: 90.0, copies: 3 }).unwrap();
    assert_eq!(e.mesh().face_count(), 24);
    round_trip(&mut e);
}

#[test]
fn scale_and_mirror() {
    let mut e = Editor::new();
    let b = add_box(&mut e, 0.0);
    e.apply(Command::Scale { elements: b.clone(), origin: DVec3::ZERO, factors: v(2.0, 1.0, 3.0) }).unwrap();
    assert!(e.mesh().find_vertex(v(2.0, 1.0, 3.0)).is_some());
    round_trip(&mut e);
    let faces: BTreeSet<FaceId> = e.mesh().faces().map(|(f, _)| f).collect();
    assert!(e.mesh().signed_volume(&faces) > 0.0);
    // Mirror along red: still facing outwards.
    e.apply(Command::Scale { elements: b, origin: DVec3::ZERO, factors: v(-1.0, 1.0, 1.0) }).unwrap();
    assert!(e.mesh().find_vertex(v(-2.0, 1.0, 3.0)).is_some());
    assert!(e.mesh().signed_volume(&faces) > 0.0);
    assert!(e.apply(Command::Scale { elements: vec![], origin: DVec3::ZERO, factors: v(0.0, 1.0, 1.0) }).is_err());
}

#[test]
fn offset_and_follow_me_commands() {
    let mut e = Editor::new();
    e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: v(2.0, 0.0, 0.0), v: v(0.0, 2.0, 0.0), facing: None })
        .unwrap();
    let f = e.mesh().faces().next().unwrap().0;
    e.apply(Command::OffsetFace { face: f, distance: 0.5 }).unwrap();
    assert_eq!(e.mesh().face_count(), 2);
    round_trip(&mut e);

    let mut e = Editor::new();
    // A profile standing on the ground and a path along red.
    e.apply(Command::DrawRectangle {
        corner: v(0.0, -0.2, 0.0),
        u: v(0.0, 0.4, 0.0),
        v: v(0.0, 0.0, 0.3),
        facing: Some(-DVec3::X),
    })
    .unwrap();
    let profile = e.mesh().faces().next().unwrap().0;
    let path = e.apply(Command::DrawLines { lines: vec![[v(0.0, 0.0, 0.0), v(3.0, 0.0, 0.0)]], facing: None }).unwrap();
    let path: Vec<EdgeId> =
        path.created.iter().filter_map(|el| if let Element::Edge(x) = el { Some(*x) } else { None }).collect();
    e.apply(Command::FollowMe { face: profile, path }).unwrap();
    // Two caps, four sides, and the path still lies on the bottom, splitting it in two.
    assert_eq!(e.mesh().face_count(), 7);
    round_trip(&mut e);
}

#[test]
fn make_group_moves_geometry_out_of_the_model() {
    let mut e = Editor::new();
    let b = add_box(&mut e, 2.0);
    let out = e.apply(Command::MakeGroup { elements: b }).unwrap();
    let i = instance(out.created[0]);
    assert_eq!(e.mesh().face_count(), 0);
    assert_eq!(e.doc().definitions.len(), 1);
    let inst = &e.doc().root.instances[&i];
    let def = &e.doc().definitions[&inst.definition];
    assert!(def.group);
    assert_eq!(def.mesh.face_count(), 6);
    // The definition's origin is the box corner; the instance puts it back.
    assert!(def.mesh.find_vertex(DVec3::ZERO).is_some());
    assert!((inst.transform.transform_point3(DVec3::ZERO) - v(2.0, 0.0, 0.0)).length() < 1e-12);
    assert_unique_ids(e.doc());
    round_trip(&mut e);
}

#[test]
fn components_share_their_definition() {
    let mut e = Editor::new();
    let b = add_box(&mut e, 0.0);
    let first = instance(e.apply(Command::MakeComponent { elements: b, name: Some("Box".into()) }).unwrap().created[0]);
    let copy = e
        .apply(Command::Copy { elements: vec![Element::Instance(first)], offset: v(3.0, 0.0, 0.0), copies: 2 })
        .unwrap();
    assert_eq!(copy.created.len(), 2);
    assert_eq!(e.doc().definitions.len(), 1);
    assert_eq!(e.doc().root.instances.len(), 3);
    // Edit one in context: a line across its top shows up in all.
    e.enter(first).unwrap();
    assert_eq!(e.context(), &[first]);
    e.apply(Command::DrawLines { lines: vec![[v(0.5, 0.0, 1.0), v(0.5, 1.0, 1.0)]], facing: None }).unwrap();
    let def = e.active_definition().unwrap();
    assert_eq!(e.doc().definitions[&def].mesh.face_count(), 7);
    assert!(e.doc().root.instances.values().all(|i| i.definition == def));
    assert!(e.exit());
    assert!(e.is_selected(Element::Instance(first)));
    // Occurrences: the model and three boxes.
    assert_eq!(e.doc().occurrences(&[]).len(), 4);
    assert_unique_ids(e.doc());
}

#[test]
fn editing_a_copied_group_makes_it_unique() {
    let mut e = Editor::new();
    let b = add_box(&mut e, 0.0);
    let g = instance(e.apply(Command::MakeGroup { elements: b }).unwrap().created[0]);
    let c =
        e.apply(Command::Copy { elements: vec![Element::Instance(g)], offset: v(3.0, 0.0, 0.0), copies: 1 }).unwrap();
    let c = instance(c.created[0]);
    assert_eq!(e.doc().definitions.len(), 1);
    e.enter(c).unwrap();
    assert_eq!(e.doc().definitions.len(), 2);
    assert_eq!(e.undo_label(), Some("Make Unique"));
    e.apply(Command::DrawLines { lines: vec![[v(0.5, 0.0, 1.0), v(0.5, 1.0, 1.0)]], facing: None }).unwrap();
    let original = e.doc().root.instances[&g].definition;
    assert_eq!(e.doc().definitions[&original].mesh.face_count(), 6);
    assert_unique_ids(e.doc());
}

#[test]
fn explode_puts_geometry_back_transformed_and_painted() {
    let mut e = Editor::new();
    let b = add_box(&mut e, 0.0);
    let g = instance(e.apply(Command::MakeGroup { elements: b }).unwrap().created[0]);
    e.apply(Command::Move { elements: vec![Element::Instance(g)], offset: v(5.0, 0.0, 0.0) }).unwrap();
    let m = e
        .apply(Command::AddMaterial { name: "Red".into(), color: [1.0, 0.0, 0.0, 1.0], texture: None })
        .unwrap()
        .material;
    e.apply(Command::PaintInstances { instances: vec![g], material: m }).unwrap();
    e.apply(Command::Explode { instance: g }).unwrap();
    assert!(e.doc().root.instances.is_empty());
    assert!(e.doc().definitions.is_empty(), "the unused definition is purged");
    assert_eq!(e.mesh().face_count(), 6);
    assert!(e.mesh().find_vertex(v(6.0, 1.0, 1.0)).is_some());
    assert!(e.mesh().faces().all(|(_, f)| f.material == m));
    let faces: BTreeSet<FaceId> = e.mesh().faces().map(|(f, _)| f).collect();
    assert!(e.mesh().signed_volume(&faces) > 0.0);
    round_trip(&mut e);
}

#[test]
fn undoing_the_group_closes_its_context() {
    let mut e = Editor::new();
    let b = add_box(&mut e, 0.0);
    let g = instance(e.apply(Command::MakeGroup { elements: b }).unwrap().created[0]);
    e.enter(g).unwrap();
    e.undo();
    assert!(e.context().is_empty());
    assert_eq!(e.mesh().face_count(), 6);
}

#[test]
fn tags_hide_and_delete() {
    let mut e = Editor::new();
    let b = add_box(&mut e, 0.0);
    let g = instance(e.apply(Command::MakeGroup { elements: b }).unwrap().created[0]);
    let t = e.apply(Command::AddTag { name: "Furniture".into() }).unwrap().tag.unwrap();
    e.apply(Command::AssignTag { instances: vec![g], tag: Some(t) }).unwrap();
    assert_eq!(e.doc().occurrences(&[]).len(), 2);
    e.set_selection([Element::Instance(g)]);
    e.apply(Command::SetTag { tag: t, name: None, visible: Some(false) }).unwrap();
    assert_eq!(e.doc().occurrences(&[]).len(), 1);
    assert!(e.selection().is_empty(), "an instance on a hidden tag cannot stay selected");
    round_trip(&mut e);
    e.apply(Command::DeleteTag { tag: t }).unwrap();
    assert_eq!(e.doc().root.instances[&g].tag, None);
    assert_eq!(e.doc().occurrences(&[]).len(), 2);
    assert!(e.apply(Command::AssignTag { instances: vec![g], tag: Some(t) }).is_err());
}

#[test]
fn guides_and_edge_styles() {
    let mut e = Editor::new();
    let g = e.apply(Command::AddGuideLine { origin: DVec3::ZERO, dir: v(2.0, 0.0, 0.0) }).unwrap().created[0];
    e.apply(Command::AddGuidePoint { point: v(1.0, 1.0, 0.0) }).unwrap();
    assert_eq!(e.active().guides.len(), 2);
    e.apply(Command::Erase { elements: vec![g] }).unwrap();
    assert_eq!(e.active().guides.len(), 1);
    e.apply(Command::DeleteGuides).unwrap();
    assert!(e.active().guides.is_empty());
    round_trip(&mut e);

    let b = add_box(&mut e, 0.0);
    let edges: Vec<EdgeId> =
        b.iter().filter_map(|el| if let Element::Edge(x) = el { Some(*x) } else { None }).collect();
    e.apply(Command::SoftenEdges { edges: edges.clone(), angle: 100.0 }).unwrap();
    assert!(e.mesh().edges().all(|(_, x)| x.soft && x.smooth));
    e.apply(Command::SetEdges { edges: edges[..2].to_vec(), soft: Some(false), smooth: None, hidden: Some(true) })
        .unwrap();
    assert_eq!(e.mesh().edges().filter(|(_, x)| x.hidden).count(), 2);
    round_trip(&mut e);
}

#[test]
fn material_edit() {
    let mut e = Editor::new();
    let m = e
        .apply(Command::AddMaterial { name: "Glass".into(), color: [0.5, 0.7, 0.9, 1.0], texture: None })
        .unwrap()
        .material
        .unwrap();
    e.apply(Command::SetMaterial { material: m, name: None, color: Some([0.5, 0.7, 0.9, 0.3]) }).unwrap();
    assert_eq!(e.doc().materials[&m].color[3], 0.3);
    round_trip(&mut e);
}

#[test]
fn material_glow() {
    let mut e = Editor::new();
    let m = e
        .apply(Command::AddMaterial { name: "Lamp".into(), color: [1.0, 0.8, 0.6, 1.0], texture: None })
        .unwrap()
        .material
        .unwrap();
    assert_eq!(e.doc().materials[&m].glow, 0.0);
    e.apply(Command::SetMaterialGlow { material: m, glow: 2000.0 }).unwrap();
    assert_eq!(e.doc().materials[&m].glow, 2000.0);
    round_trip(&mut e);
    for bad in [-1.0, f32::NAN, f32::INFINITY, crate::MAX_GLOW * 2.0] {
        assert!(e.apply(Command::SetMaterialGlow { material: m, glow: bad }).is_err());
    }
    assert_eq!(e.doc().materials[&m].glow, 2000.0);
}

#[test]
fn place_another_instance() {
    let mut e = Editor::new();
    let b = add_box(&mut e, 0.0);
    let first = instance(e.apply(Command::MakeComponent { elements: b, name: None }).unwrap().created[0]);
    let def = e.doc().root.instances[&first].definition;
    let placed = instance(
        e.apply(Command::PlaceInstance { definition: def, origin: v(5.0, 0.0, 0.0), rotation: 90.0, scale: 2.0 })
            .unwrap()
            .created[0],
    );
    let t = e.doc().root.instances[&placed].transform;
    assert!((t.transform_point3(v(1.0, 0.0, 0.0)) - v(5.0, 2.0, 0.0)).length() < 1e-9);
    round_trip(&mut e);
    // Not inside itself.
    e.enter(first).unwrap();
    assert!(
        e.apply(Command::PlaceInstance { definition: def, origin: DVec3::ZERO, rotation: 0.0, scale: 1.0 }).is_err()
    );
}

#[test]
fn textured_material_placement_and_style() {
    let mut e = Editor::new();
    let b = add_box(&mut e, 0.0);
    let img = e
        .apply(Command::AddImage { name: "brick.png".into(), mime: "image/png".into(), data: vec![1, 2, 3] })
        .unwrap()
        .image
        .unwrap();
    let tex = Texture { image: img, size: [2.0, 1.0] };
    let m = e
        .apply(Command::AddMaterial { name: "Brick".into(), color: [1.0; 4], texture: Some(tex) })
        .unwrap()
        .material
        .unwrap();
    assert!(
        e.apply(Command::AddMaterial {
            name: "Bad".into(),
            color: [1.0; 4],
            texture: Some(Texture { image: ImageId(999), size: [1.0, 1.0] })
        })
        .is_err()
    );
    let faces: Vec<FaceId> =
        b.iter().filter_map(|el| if let Element::Face(f) = el { Some(*f) } else { None }).collect();
    e.apply(Command::Paint { faces: faces.clone(), material: Some(m), back: false }).unwrap();
    let placement =
        TexturePlacement { offset: [0.25, 0.0], rotation: 15.0, scale: [0.5, 0.5], ..TexturePlacement::default() };
    e.apply(Command::SetTexturePlacement { faces: faces[..1].to_vec(), placement: Some(placement) }).unwrap();
    assert_eq!(e.mesh().face(faces[0]).unwrap().uv, Some(placement));
    round_trip(&mut e);
    e.apply(Command::SetMaterialTexture { material: m, texture: None }).unwrap();
    assert!(e.doc().materials[&m].texture.is_none());
    round_trip(&mut e);
    let style = Style { faces: FaceStyle::XRay, shadows: true, lighting: Lighting::Sun, ..Default::default() };
    e.apply(Command::SetStyle { style }).unwrap();
    assert_eq!(e.doc().style.faces, FaceStyle::XRay);
    round_trip(&mut e);
    assert!(!e.apply(Command::SetStyle { style }).unwrap().changed);
}

#[test]
fn moving_a_face_carries_its_texture() {
    let mut e = Editor::new();
    let b = add_box(&mut e, 0.0);
    let top = b
        .iter()
        .filter_map(|el| if let Element::Face(f) = el { Some(*f) } else { None })
        .find(|f| e.mesh().face_normal(*f).unwrap().z > 0.9)
        .unwrap();
    let corner = DVec3::new(0.0, 0.0, 1.0);
    let n = e.mesh().face_normal(top).unwrap();
    let before = uv_at(corner, n, [1.0, 1.0], &TexturePlacement::default());
    e.apply(Command::Move { elements: vec![Element::Face(top)], offset: v(0.3, 0.2, 0.5) }).unwrap();
    let pl = e.mesh().face(top).unwrap().uv.unwrap();
    let after = uv_at(corner + v(0.3, 0.2, 0.5), n, [1.0, 1.0], &pl);
    assert!((before[0] - after[0]).abs() < 1e-12 && (before[1] - after[1]).abs() < 1e-12);
}

#[test]
fn textured_material_import_is_atomic_and_reuses_images() {
    let mut editor = Editor::new();
    let command = |name: &str, size| Command::AddTexturedMaterial {
        name: name.into(),
        color: [1.0; 4],
        image_name: "sample.png".into(),
        mime: "image/png".into(),
        data: vec![1, 2, 3],
        size,
    };
    editor.apply(command("First", [1.0, 2.0])).unwrap();
    editor.apply(command("Second", [2.0, 4.0])).unwrap();
    assert_eq!(editor.doc().images.len(), 1);
    assert_eq!(editor.doc().materials.len(), 2);
    assert!(editor.apply(command("Invalid", [f64::NAN, 1.0])).is_err());
    assert_eq!(editor.doc().materials.len(), 2);
    editor.undo();
    assert_eq!(editor.doc().materials.len(), 1);
    assert_eq!(editor.doc().images.len(), 1);
    editor.undo();
    assert!(editor.doc().materials.is_empty());
    assert!(editor.doc().images.is_empty());
    editor.redo();
    assert_eq!(editor.doc().materials.len(), 1);
    assert_eq!(editor.doc().images.len(), 1);
}

#[test]
fn polygon_extrusion_is_faceted_even_with_many_sides() {
    for segments in [6, 32] {
        for polygon in [false, true] {
            let mut e = Editor::new();
            let cmd = if polygon {
                Command::DrawPolygon { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1.0, segments, start: None }
            } else {
                Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1.0, segments, start: None }
            };
            e.apply(cmd).unwrap();
            let f = e.mesh().faces().next().unwrap().0;
            e.apply(Command::PushPull { face: f, distance: -1.0, keep_base: false }).unwrap();
            let soft = e.mesh().edges().filter(|(_, edge)| edge.soft).count();
            assert_eq!(soft, if polygon { 0 } else { segments as usize });
            assert_eq!(e.mesh().curves().count(), 2);
            for (_, curve) in e.mesh().curves() {
                assert_eq!(
                    curve.kind,
                    if polygon { rysmo_mesh::CurveKind::Polygon } else { rysmo_mesh::CurveKind::Circle }
                );
            }
        }
    }
}

#[test]
fn curve_copy_group_and_nonuniform_deformation_preserve_entity_semantics() {
    let mut e = Editor::new();
    let out = e
        .apply(Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1.0, segments: 16, start: None })
        .unwrap();
    let elements: Vec<_> = out.created.iter().copied().filter(|x| matches!(x, Element::Edge(_))).collect();
    e.apply(Command::Copy { elements: elements.clone(), offset: v(4.0, 0.0, 0.0), copies: 1 }).unwrap();
    assert_eq!(e.mesh().curves().count(), 2);
    assert!(e.mesh().curves().any(|(_, curve)| (curve.centre - v(4.0, 0.0, 0.0)).length() < 1e-7));
    e.apply(Command::Scale { elements: elements.clone(), origin: DVec3::ZERO, factors: v(2.0, 1.0, 1.0) }).unwrap();
    assert!(e.mesh().curves().any(|(_, curve)| curve.kind == rysmo_mesh::CurveKind::Polyline));
    let group = e.apply(Command::MakeGroup { elements }).unwrap().created;
    let Element::Instance(instance) = group[0] else { panic!("group") };
    e.enter(instance).unwrap();
    assert_eq!(e.mesh().curves().count(), 1);
    assert_eq!(e.mesh().curves().next().unwrap().1.kind, rysmo_mesh::CurveKind::Polyline);
}

#[test]
fn curve_radius_changes_geometry_and_undo_restores_parameters() {
    let mut e = Editor::new();
    e.apply(Command::DrawCircle { centre: v(3.0, 2.0, 1.0), normal: DVec3::Z, radius: 1.0, segments: 16, start: None })
        .unwrap();
    let id = e.mesh().curves().next().unwrap().0;
    e.apply(Command::SetCurveRadius { curve: id, radius: 2.0 }).unwrap();
    assert!((e.mesh().curve(id).unwrap().radius - 2.0).abs() < 1e-7);
    for (_, vertex) in e.mesh().vertices() {
        assert!((vertex.pos.distance(v(3.0, 2.0, 1.0)) - 2.0).abs() < 1e-7);
    }
    e.undo();
    assert!((e.mesh().curve(id).unwrap().radius - 1.0).abs() < 1e-7);
    assert!(e.apply(Command::SetCurveRadius { curve: id, radius: 0.0 }).is_err());
    assert!((e.mesh().curve(id).unwrap().radius - 1.0).abs() < 1e-7);
}

#[test]
fn follow_me_preserves_polygon_facets() {
    for polygon in [false, true] {
        let mut e = Editor::new();
        let draw = if polygon {
            Command::DrawPolygon { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1.0, segments: 32, start: None }
        } else {
            Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1.0, segments: 32, start: None }
        };
        e.apply(draw).unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        let path = e
            .apply(Command::DrawLines { lines: vec![[DVec3::ZERO, v(0.0, 0.0, 2.0)]], facing: None })
            .unwrap()
            .created
            .into_iter()
            .filter_map(|e| if let Element::Edge(id) = e { Some(id) } else { None })
            .collect();
        e.apply(Command::FollowMe { face, path }).unwrap();
        assert_eq!(e.mesh().edges().filter(|(_, edge)| edge.soft).count(), if polygon { 0 } else { 32 });
    }
}

#[test]
fn resizing_a_group_does_not_resize_the_surrounding_model() {
    let mut e = Editor::new();
    e.apply(Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1.0, segments: 16, start: None })
        .unwrap();
    let selection = e.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
    let out = e.apply(Command::MakeGroup { elements: selection }).unwrap();
    let Element::Instance(instance) = out.created[0] else { panic!("group") };
    e.apply(Command::DrawRectangle { corner: v(5.0, 0.0, 0.0), u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
    let outside = e.mesh().clone();
    e.enter(instance).unwrap();
    e.apply(Command::AddGuidePoint { point: v(1.0, 2.0, 0.0) }).unwrap();
    e.apply(Command::ResizeContext { factor: 2.0 }).unwrap();
    assert!((e.mesh().curves().next().unwrap().1.radius - 2.0).abs() < 1e-7);
    assert!(matches!(e.active().guides.values().next(), Some(Guide::Point { p, .. }) if *p == v(2.0,4.0,0.0)));
    assert_eq!(e.doc().root.mesh, outside);
    e.undo();
    assert!((e.mesh().curves().next().unwrap().1.radius - 1.0).abs() < 1e-7);
}

#[test]
fn curve_segment_edit_keeps_faces_holes_identity_selection_and_undo() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawRectangle { corner: v(-3., -3., 0.), u: v(6., 0., 0.), v: v(0., 6., 0.), facing: None })
        .unwrap();
    editor
        .apply(Command::DrawCircle {
            centre: DVec3::ZERO,
            normal: DVec3::Z,
            radius: 1.,
            segments: 12,
            start: Some(DVec3::X),
        })
        .unwrap();
    let (curve, definition) = editor.mesh().curves().next().map(|(id, c)| (id, *c)).unwrap();
    let material = editor
        .apply(Command::AddMaterial { name: "Painted curve".into(), color: [0.2, 0.3, 0.4, 1.], texture: None })
        .unwrap()
        .material
        .unwrap();
    let all_faces: Vec<_> = editor.mesh().faces().map(|(id, _)| id).collect();
    editor.apply(Command::Paint { faces: all_faces.clone(), material: Some(material), back: false }).unwrap();
    editor
        .apply(Command::SetTexturePlacement {
            faces: all_faces,
            placement: Some(TexturePlacement {
                offset: [0.2, 0.3],
                rotation: 30.,
                scale: [2., 3.],
                ..TexturePlacement::default()
            }),
        })
        .unwrap();
    let faces: Vec<_> =
        editor.mesh().faces().map(|(id, face)| (id, face.material, face.uv, face.holes.len())).collect();
    let selected = editor.mesh().curve_edges(curve).into_iter().map(Element::Edge).collect::<Vec<_>>();
    editor.set_selection(selected);
    let before = editor.doc().clone();
    editor.apply(Command::SetCurveSegments { curve, segments: 20 }).unwrap();
    assert_eq!(editor.mesh().curve_edges(curve).len(), 20);
    assert_eq!(editor.selection().len(), 20);
    assert_eq!(editor.mesh().curves().count(), 1);
    assert_eq!(editor.mesh().curve(curve).unwrap().radius, definition.radius);
    for (id, material, uv, holes) in faces {
        let face = editor.mesh().face(id).unwrap();
        assert_eq!((face.material, face.uv, face.holes.len()), (material, uv, holes));
    }
    editor.mesh().validate().unwrap();
    let after = editor.doc().clone();
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
    assert!(editor.redo());
    assert_eq!(editor.doc(), &after);
}
#[test]
fn arc_segment_edit_preserves_chord_and_face_winding() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawArc { start: DVec3::ZERO, end: DVec3::X * 2., normal: DVec3::Z, bulge: 1., segments: 8 })
        .unwrap();
    editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: Some(DVec3::Z) }).unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    let face = editor.mesh().faces().next().unwrap().0;
    let normal = editor.mesh().face_plane(face).unwrap().normal;
    editor.apply(Command::SetCurveSegments { curve, segments: 16 }).unwrap();
    assert_eq!(editor.mesh().edge_count(), 17);
    assert_eq!(editor.mesh().curve_edges(curve).len(), 16);
    assert_eq!(editor.mesh().face(face).unwrap().outer.len(), 17);
    assert!(editor.mesh().face_plane(face).unwrap().normal.dot(normal) > 0.99);
    editor.mesh().validate().unwrap();
}
#[test]
fn segments_on_an_extruded_curve_fail_without_modifying_the_document() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawPolygon { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1., segments: 6, start: None })
        .unwrap();
    let face = editor.mesh().faces().next().unwrap().0;
    editor.apply(Command::PushPull { face, distance: 1., keep_base: false }).unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    let before = editor.doc().clone();
    assert!(editor.apply(Command::SetCurveSegments { curve, segments: 12 }).is_err());
    assert_eq!(editor.doc(), &before);
}

#[test]
fn model_resize_scales_the_camera_eye_height_reference() {
    let mut editor = Editor::new();
    let mut camera = rysmo_math::OrbitCamera::default();
    camera.place_eye(v(2., 3., 4.7), None);
    camera.set_eye_height(3., 1.7);
    editor.set_camera(camera);
    editor.apply(Command::ResizeContext { factor: 2. }).unwrap();
    let resized = editor.doc().camera.unwrap();
    assert_eq!(resized.eye_base_z, 6.);
    assert!((resized.eye().z - 9.4).abs() < 1e-8);
    editor.undo();
    assert_eq!(editor.doc().camera, Some(camera));
}

#[test]
fn resampled_polygon_keeps_faceted_extrusion() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawPolygon { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1., segments: 12, start: None })
        .unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    editor.apply(Command::SetCurveSegments { curve, segments: 5 }).unwrap();
    assert_eq!(editor.mesh().curve_edges(curve).len(), 5);
    assert!(editor.mesh().curve(curve).unwrap().faceted);
    let face = editor.mesh().faces().next().unwrap().0;
    editor.apply(Command::PushPull { face, distance: 1., keep_base: false }).unwrap();
    assert_eq!(editor.mesh().edges().filter(|(_, edge)| edge.soft || edge.smooth).count(), 0);
    assert_eq!(editor.mesh().faces().count(), 7);
    editor.mesh().validate().unwrap();
}

#[test]
fn resampling_keeps_edge_settings_in_their_curve_regions() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawCircle {
            centre: DVec3::ZERO,
            normal: DVec3::Z,
            radius: 1.,
            segments: 12,
            start: Some(DVec3::X),
        })
        .unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    let right: Vec<_> = editor
        .mesh()
        .curve_edges(curve)
        .into_iter()
        .filter(|edge| {
            let (a, b) = editor.mesh().edge_points(*edge);
            (a + b).x > 0.
        })
        .collect();
    let upper: Vec<_> = editor
        .mesh()
        .curve_edges(curve)
        .into_iter()
        .filter(|edge| {
            let (a, b) = editor.mesh().edge_points(*edge);
            (a + b).y > 0.
        })
        .collect();
    editor.apply(Command::SetEdges { edges: right, hidden: Some(true), soft: None, smooth: Some(true) }).unwrap();
    editor.apply(Command::SetEdges { edges: upper, hidden: None, soft: Some(true), smooth: None }).unwrap();
    let before = editor.doc().clone();
    editor.apply(Command::SetCurveSegments { curve, segments: 8 }).unwrap();
    for edge in editor.mesh().curve_edges(curve) {
        let (a, b) = editor.mesh().edge_points(edge);
        let flags = editor.mesh().edge(edge).unwrap();
        assert_eq!(flags.hidden, (a + b).x > 0.);
        assert_eq!(flags.smooth, (a + b).x > 0.);
        assert_eq!(flags.soft, (a + b).y > 0.);
    }
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
}

fn test_linear_dimension(from: DVec3, to: DVec3) -> crate::dimensions::Dimension {
    use crate::dimensions::*;
    Dimension {
        measure: DimensionMeasure::Linear {
            from: DimensionAnchor { point: from, reference: None },
            to: DimensionAnchor { point: to, reference: None },
            axis: None,
        },
        normal: DVec3::Z,
        offset: DVec3::Y,
        text: String::new(),
        alignment: Default::default(),
        font: Default::default(),
        text_position: Default::default(),
        endpoints: Default::default(),
    }
}

#[test]
fn dimensions_create_edit_erase_and_invalid_changes_are_atomic() {
    let mut editor = Editor::new();
    let dimension = test_linear_dimension(DVec3::ZERO, DVec3::X * 2.);
    let before = editor.doc().clone();
    let id = editor.apply(Command::AddDimension { dimension: dimension.clone() }).unwrap().dimensions[0];
    let created = editor.doc().clone();
    assert_eq!(editor.active().dimensions[&id], dimension);
    let mut edited = dimension.clone();
    edited.text = "Width <>".into();
    edited.endpoints = crate::dimensions::DimensionEndpoints::OpenArrow;
    edited.alignment = crate::dimensions::DimensionTextAlignment::DimensionLine;
    edited.font.size = 28.;
    edited.font.monospace = true;
    edited.font.color = [180, 20, 20];
    editor.apply(Command::SetDimension { id, dimension: edited.clone() }).unwrap();
    let after_edit = editor.doc().clone();
    let revision = editor.revision();
    assert!(!editor.apply(Command::SetDimension { id, dimension: edited }).unwrap().changed);
    assert_eq!(editor.revision(), revision);
    let mut invalid = dimension;
    invalid.normal = DVec3::ZERO;
    assert!(editor.apply(Command::SetDimension { id, dimension: invalid }).is_err());
    assert_eq!(editor.doc(), &after_edit);
    editor.apply(Command::EraseDimensions { ids: vec![id] }).unwrap();
    assert!(editor.active().dimensions.is_empty());
    assert!(editor.undo());
    assert_eq!(editor.doc(), &after_edit);
    assert!(editor.undo());
    assert_eq!(editor.doc(), &created);
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
}

#[test]
fn erased_geometry_keeps_latest_dimension_values_and_undo_restores_link() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None }).unwrap();
    let edge = editor.mesh().edges().next().unwrap().0;
    let vertices = editor.mesh().edge(edge).unwrap().v;
    let anchor = |vertex, point| DimensionAnchor {
        point,
        reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex } }),
    };
    let dimension = Dimension {
        measure: DimensionMeasure::Linear {
            from: anchor(vertices[0], DVec3::ZERO),
            to: anchor(vertices[1], DVec3::X * 2.),
            axis: None,
        },
        normal: DVec3::Z,
        offset: DVec3::Y,
        text: String::new(),
        alignment: Default::default(),
        font: Default::default(),
        text_position: Default::default(),
        endpoints: Default::default(),
    };
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    editor
        .apply(Command::Scale { elements: vec![Element::Edge(edge)], origin: DVec3::ZERO, factors: DVec3::splat(2.) })
        .unwrap();
    editor.apply(Command::Erase { elements: vec![Element::Edge(edge)] }).unwrap();
    let result = editor.active().dimensions[&id].resolve(editor.doc(), None);
    assert!(!result.associated);
    assert!((result.length - 4.).abs() < 1e-8);
    assert!(editor.undo());
    assert!(editor.active().dimensions[&id].resolve(editor.doc(), None).associated);
}

#[test]
fn dimension_links_survive_group_unique_scale_and_explode() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor.apply(Command::DrawLines { lines: vec![[v(2., 3., 0.), v(4., 3., 0.)]], facing: None }).unwrap();
    let edge = editor.mesh().edges().next().unwrap().0;
    let anchor = |mesh: &Mesh, vertex| DimensionAnchor {
        point: mesh.pos(vertex),
        reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex } }),
    };
    let vertices = editor.mesh().edge(edge).unwrap().v;
    let dimension = Dimension {
        measure: DimensionMeasure::Linear {
            from: anchor(editor.mesh(), vertices[0]),
            to: anchor(editor.mesh(), vertices[1]),
            axis: None,
        },
        normal: DVec3::Z,
        offset: DVec3::Y,
        text: String::new(),
        alignment: Default::default(),
        font: Default::default(),
        text_position: Default::default(),
        endpoints: Default::default(),
    };
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let out = editor.apply(Command::MakeGroup { elements: vec![Element::Edge(edge)] }).unwrap();
    let Element::Instance(group) = out.created[0] else {
        panic!("group");
    };
    assert!(editor.active().dimensions[&id].resolve(editor.doc(), None).associated);
    editor.apply(Command::Move { elements: vec![Element::Instance(group)], offset: DVec3::Y }).unwrap();
    editor.apply(Command::MakeUnique { instance: group }).unwrap();
    let measured = editor.active().dimensions[&id].resolve(editor.doc(), None);
    assert!(measured.associated);
    assert!((measured.length - 2.).abs() < 1e-8);
    editor.enter(group).unwrap();
    let vertices = editor.mesh().edges().next().unwrap().1.v;
    let owned = Dimension {
        measure: DimensionMeasure::Linear {
            from: anchor(editor.mesh(), vertices[0]),
            to: anchor(editor.mesh(), vertices[1]),
            axis: None,
        },
        normal: DVec3::Z,
        offset: DVec3::Y,
        text: String::new(),
        alignment: Default::default(),
        font: Default::default(),
        text_position: Default::default(),
        endpoints: Default::default(),
    };
    editor.apply(Command::AddDimension { dimension: owned }).unwrap();
    editor.exit();
    editor
        .apply(Command::Scale {
            elements: vec![Element::Instance(group)],
            origin: DVec3::ZERO,
            factors: DVec3::splat(2.),
        })
        .unwrap();
    assert!((editor.active().dimensions[&id].resolve(editor.doc(), None).length - 4.).abs() < 1e-8);
    let before = editor.doc().clone();
    editor.apply(Command::Explode { instance: group }).unwrap();
    assert_eq!(editor.active().dimensions.len(), 2);
    for dimension in editor.active().dimensions.values() {
        let measured = dimension.resolve(editor.doc(), None);
        assert!(measured.associated);
        assert!((measured.length - 4.).abs() < 1e-8);
    }
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
}

#[test]
fn dimension_entities_use_common_selection_erase_and_undo() {
    let mut editor = Editor::new();
    let dimension = test_linear_dimension(DVec3::ZERO, DVec3::X * 2.);
    let out = editor.apply(Command::AddDimension { dimension }).unwrap();
    let entity = Element::Dimension(out.dimensions[0]);
    assert_eq!(out.created, vec![entity]);
    assert!(editor.contains(entity));
    editor.set_selection(vec![entity]);
    assert_eq!(editor.selected(), vec![entity]);
    let before = editor.doc().clone();
    assert!(editor.apply(Command::Move { elements: vec![entity], offset: DVec3::Z }).is_err());
    assert_eq!(editor.doc(), &before);
    assert_eq!(editor.selected(), vec![entity]);
    editor.apply(Command::Erase { elements: editor.selected() }).unwrap();
    assert!(!editor.contains(entity));
    assert!(editor.selection().is_empty());
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
    assert!(editor.contains(entity));
}

#[test]
fn moving_linked_dimension_repositions_string_without_moving_anchors() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None }).unwrap();
    let edge = editor.mesh().edges().next().unwrap().0;
    let vertices = editor.mesh().edge(edge).unwrap().v;
    let anchor = |vertex| DimensionAnchor {
        point: editor.mesh().pos(vertex),
        reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex } }),
    };
    let dimension = Dimension {
        measure: DimensionMeasure::Linear { from: anchor(vertices[0]), to: anchor(vertices[1]), axis: None },
        normal: DVec3::Z,
        offset: DVec3::Y,
        text: String::new(),
        alignment: Default::default(),
        font: Default::default(),
        text_position: Default::default(),
        endpoints: Default::default(),
    };
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let before = editor.doc().clone();
    editor.apply(Command::Move { elements: vec![Element::Dimension(id)], offset: DVec3::Y }).unwrap();
    let dimension = &editor.active().dimensions[&id];
    assert_eq!(dimension.offset, DVec3::Y * 2.);
    let measured = dimension.resolve(editor.doc(), None);
    assert_eq!(measured.length, 2.);
    assert!(measured.associated);
    assert_eq!(editor.mesh(), &before.root.mesh);
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
    editor
        .apply(Command::Move { elements: vec![Element::Dimension(id), Element::Edge(edge)], offset: DVec3::Y })
        .unwrap();
    assert_eq!(editor.active().dimensions[&id].offset, DVec3::Y);
    assert_eq!(editor.active().dimensions[&id].resolve(editor.doc(), None).from, DVec3::Y);
}

#[test]
fn moving_radial_dimension_keeps_radius_and_circle_geometry() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawCircle {
            centre: DVec3::ZERO,
            normal: DVec3::Z,
            radius: 1.,
            segments: 12,
            start: Some(DVec3::X),
        })
        .unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    let dimension = Dimension {
        measure: DimensionMeasure::Radial {
            reference: RadialReference {
                radial_axes: None,
                arc_sweep: None,
                path: vec![],
                curve: Some(curve),
                centre: DVec3::ZERO,
                normal: DVec3::Z,
                start: DVec3::X,
                radius: 1.,
            },
            diameter: true,
        },
        normal: DVec3::Z,
        offset: DVec3::X * 2.,
        text: String::new(),
        alignment: Default::default(),
        font: Default::default(),
        text_position: Default::default(),
        endpoints: Default::default(),
    };
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let before = editor.mesh().clone();
    editor.apply(Command::Move { elements: vec![Element::Dimension(id)], offset: DVec3::Y }).unwrap();
    assert_eq!(editor.mesh(), &before);
    assert_eq!(editor.active().dimensions[&id].offset, DVec3::new(2., 1., 0.));
    let measurement = editor.active().dimensions[&id].resolve(editor.doc(), None);
    assert!(measurement.associated);
    assert_eq!(measurement.length, 2.);
}

#[test]
fn radial_dimension_type_and_custom_text_follow_geometry_updates() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawCircle {
            centre: DVec3::ZERO,
            normal: DVec3::Z,
            radius: 1.,
            segments: 12,
            start: Some(DVec3::X),
        })
        .unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    let dimension = Dimension {
        measure: DimensionMeasure::Radial {
            reference: RadialReference {
                radial_axes: None,
                arc_sweep: None,
                path: vec![],
                curve: Some(curve),
                centre: DVec3::ZERO,
                normal: DVec3::Z,
                start: DVec3::X,
                radius: 1.,
            },
            diameter: false,
        },
        normal: DVec3::Z,
        offset: DVec3::X * 2.,
        text: String::new(),
        alignment: Default::default(),
        font: Default::default(),
        text_position: Default::default(),
        endpoints: Default::default(),
    };
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let mut edited = editor.active().dimensions[&id].clone();
    edited.text = "Size <>".into();
    if let DimensionMeasure::Radial { diameter, .. } = &mut edited.measure {
        *diameter = true;
    }
    editor.apply(Command::SetDimension { id, dimension: edited }).unwrap();
    editor.apply(Command::SetCurveRadius { curve, radius: 2. }).unwrap();
    let dimension = &editor.active().dimensions[&id];
    let measured = dimension.resolve(editor.doc(), None);
    assert!(measured.associated);
    assert_eq!(measured.length, 4.);
    assert_eq!(dimension.label(&measured, Units::Millimetres), "Size Ø4000 mm");
    assert_eq!(dimension.text, "Size <>");
}

#[test]
fn dimension_visibility_style_edits_are_undoable_and_invalid_values_are_atomic() {
    let mut editor = Editor::new();
    let before = editor.doc().clone();
    let mut style = editor.doc().style;
    style.dimensions.hide_small = true;
    style.dimensions.minimum_pixels = 60.;
    style.dimensions.hide_foreshortened = true;
    style.dimensions.angle_degrees = 30.;
    style.dimensions.highlight_non_associated = true;
    editor.apply(Command::SetStyle { style }).unwrap();
    let edited = editor.doc().clone();
    for value in [-1., f64::NAN, 4097.] {
        let mut invalid = style;
        invalid.dimensions.minimum_pixels = value;
        assert!(editor.apply(Command::SetStyle { style: invalid }).is_err());
        assert_eq!(editor.doc(), &edited);
    }
    let mut invalid = style;
    invalid.dimensions.angle_degrees = 91.;
    assert!(editor.apply(Command::SetStyle { style: invalid }).is_err());
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
}

#[test]
fn dimension_defaults_update_selection_atomically_preserving_measurement_and_text() {
    let mut editor = Editor::new();
    let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X * 2.);
    dimension.text = "Width <>".into();
    let a = editor.apply(Command::AddDimension { dimension: dimension.clone() }).unwrap().dimensions[0];
    let b = editor.apply(Command::AddDimension { dimension: dimension.clone() }).unwrap().dimensions[0];
    let mut style = editor.doc().style;
    style.dimension_defaults.font.size = 28.;
    style.dimension_defaults.font.color = [180, 20, 20];
    style.dimension_defaults.endpoints = crate::dimensions::DimensionEndpoints::Slash;
    style.dimension_defaults.alignment = crate::dimensions::DimensionTextAlignment::DimensionLine;
    editor.apply(Command::SetStyle { style }).unwrap();
    assert_eq!(editor.active().dimensions[&a], dimension);
    let before = editor.doc().clone();
    assert!(editor.apply(Command::ApplyDimensionDefaults { ids: vec![a, crate::DimensionId(u64::MAX)] }).is_err());
    assert_eq!(editor.doc(), &before);
    editor.apply(Command::ApplyDimensionDefaults { ids: vec![a, b] }).unwrap();
    let mut expected = dimension;
    style.dimension_defaults.apply(&mut expected);
    assert_eq!(editor.active().dimensions[&a], expected);
    assert_eq!(editor.active().dimensions[&b], expected);
    assert!(!editor.apply(Command::ApplyDimensionDefaults { ids: vec![a, b] }).unwrap().changed);
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
}

#[test]
fn grouping_selected_dimensions_preserves_moved_geometry_links_and_external_dimensions() {
    use crate::dimensions::*;
    for component in [false, true] {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[v(2., 3., 0.), v(4., 3., 0.)]], facing: None }).unwrap();
        let (edge, member) = editor.mesh().edges().next().unwrap();
        let vertices = member.v;
        let anchor = |vertex| DimensionAnchor {
            point: editor.mesh().pos(vertex),
            reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex } }),
        };
        let mut dimension = test_linear_dimension(v(2., 3., 0.), v(4., 3., 0.));
        dimension.measure = DimensionMeasure::Linear { from: anchor(vertices[0]), to: anchor(vertices[1]), axis: None };
        dimension.text = "Width <>".into();
        dimension.font.size = 24.;
        let owned = editor.apply(Command::AddDimension { dimension: dimension.clone() }).unwrap().dimensions[0];
        let external = editor.apply(Command::AddDimension { dimension: dimension.clone() }).unwrap().dimensions[0];
        let before = editor.doc().clone();
        let elements = vec![Element::Edge(edge), Element::Dimension(owned)];
        let command = if component {
            Command::MakeComponent { elements, name: Some("Measured".into()) }
        } else {
            Command::MakeGroup { elements }
        };
        let out = editor.apply(command).unwrap();
        let Element::Instance(group) = out.created[0] else { panic!("instance") };
        assert!(!editor.active().dimensions.contains_key(&owned));
        let definition = Some(editor.active().instances[&group].definition);
        let moved = &editor.doc().def(definition).unwrap().dimensions[&owned];
        assert!(moved.resolve(editor.doc(), definition).associated);
        assert_eq!(moved.text, dimension.text);
        assert_eq!(moved.font, dimension.font);
        let world = moved.layout(editor.doc(), definition, editor.active().instances[&group].transform, 0.1).unwrap();
        assert_eq!(world.measurement.from, v(2., 3., 0.));
        assert_eq!(world.measurement.to, v(4., 3., 0.));
        assert!(editor.active().dimensions[&external].resolve(editor.doc(), None).associated);
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
        assert!(editor.redo());
        editor.apply(Command::Explode { instance: group }).unwrap();
        assert_eq!(editor.active().dimensions.len(), 2);
        for dimension in editor.active().dimensions.values() {
            let measurement = dimension.resolve(editor.doc(), None);
            assert!(measurement.associated);
            assert_eq!(measurement.length, 2.);
        }
    }
}

#[test]
fn grouping_dimension_without_its_geometry_detaches_only_the_moved_annotation() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None }).unwrap();
    let (edge, member) = editor.mesh().edges().next().unwrap();
    let vertices = member.v;
    let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X * 2.);
    dimension.measure = DimensionMeasure::Linear {
        from: DimensionAnchor {
            point: DVec3::ZERO,
            reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[0] } }),
        },
        to: DimensionAnchor {
            point: DVec3::X * 2.,
            reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[1] } }),
        },
        axis: None,
    };
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let before = editor.doc().clone();
    let out = editor.apply(Command::MakeGroup { elements: vec![Element::Dimension(id)] }).unwrap();
    let Element::Instance(group) = out.created[0] else { panic!("group") };
    let owner = Some(editor.active().instances[&group].definition);
    let measurement = editor.doc().def(owner).unwrap().dimensions[&id].resolve(editor.doc(), owner);
    assert_eq!(measurement.length, 2.);
    assert!(!measurement.associated);
    let (lo, hi) = editor.doc().definition_bounds(owner).unwrap();
    assert!(hi.x - lo.x >= 2. && hi.y - lo.y >= 1.);
    assert!(editor.mesh().edge(edge).is_some());
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
}

#[test]
fn grouping_radial_and_nested_instance_dimensions_keeps_associations() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawCircle {
            centre: v(3., 4., 0.),
            normal: DVec3::Z,
            radius: 1.,
            segments: 12,
            start: Some(DVec3::X),
        })
        .unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X);
    dimension.measure = DimensionMeasure::Radial {
        reference: RadialReference {
            radial_axes: None,
            arc_sweep: None,
            path: vec![],
            curve: Some(curve),
            centre: v(3., 4., 0.),
            normal: DVec3::Z,
            start: DVec3::X,
            radius: 1.,
        },
        diameter: false,
    };
    dimension.offset = DVec3::X * 2.;
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let mut elements = editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect::<Vec<_>>();
    elements.push(Element::Dimension(id));
    let out = editor.apply(Command::MakeGroup { elements }).unwrap();
    let Element::Instance(inner) = out.created[0] else { panic!("inner") };
    let inner_owner = Some(editor.active().instances[&inner].definition);
    let inner_dimension = &editor.doc().def(inner_owner).unwrap().dimensions[&id];
    assert!(inner_dimension.resolve(editor.doc(), inner_owner).associated);
    let curve = editor.doc().def(inner_owner).unwrap().mesh.curves().next().unwrap().0;
    let mut external = inner_dimension.clone();
    if let DimensionMeasure::Radial { reference, .. } = &mut external.measure {
        reference.path = vec![inner];
        reference.curve = Some(curve);
        reference.centre = v(3., 4., 0.);
    }
    let external_id = editor.apply(Command::AddDimension { dimension: external }).unwrap().dimensions[0];
    let out = editor
        .apply(Command::MakeGroup { elements: vec![Element::Instance(inner), Element::Dimension(external_id)] })
        .unwrap();
    let Element::Instance(outer) = out.created[0] else { panic!("outer") };
    let owner = Some(editor.active().instances[&outer].definition);
    let dimension = &editor.doc().def(owner).unwrap().dimensions[&external_id];
    let result = dimension.resolve(editor.doc(), owner);
    assert!(result.associated);
    assert_eq!(result.length, 1.);
    let DimensionMeasure::Radial { reference, .. } = &dimension.measure else { panic!("radial") };
    assert_eq!(reference.path, vec![inner]);
}

#[test]
fn dimension_copy_array_follows_each_copied_edge_and_undo_restores_original() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None }).unwrap();
    let (edge, member) = editor.mesh().edges().next().unwrap();
    let vertices = member.v;
    let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X * 2.);
    dimension.measure = DimensionMeasure::Linear {
        from: DimensionAnchor {
            point: DVec3::ZERO,
            reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[0] } }),
        },
        to: DimensionAnchor {
            point: DVec3::X * 2.,
            reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[1] } }),
        },
        axis: None,
    };
    dimension.text = "Length <>".into();
    dimension.font.size = 24.;
    let original = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let before = editor.doc().clone();
    let out = editor
        .apply(Command::Copy {
            elements: vec![Element::Edge(edge), Element::Dimension(original)],
            offset: DVec3::X * 3.,
            copies: 2,
        })
        .unwrap();
    let ids = out
        .created
        .iter()
        .filter_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None })
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    for (index, id) in ids.iter().enumerate() {
        let dimension = &editor.active().dimensions[id];
        let measurement = dimension.resolve(editor.doc(), None);
        assert!(measurement.associated);
        assert_eq!(measurement.from, DVec3::X * (3. * (index + 1) as f64));
        assert_eq!(dimension.offset, DVec3::Y);
        assert_eq!(dimension.font.size, 24.);
        assert_eq!(dimension.text, "Length <>");
    }
    editor.apply(Command::Move { elements: vec![Element::Edge(edge)], offset: DVec3::Y * 2. }).unwrap();
    assert_eq!(editor.active().dimensions[&original].resolve(editor.doc(), None).from, DVec3::Y * 2.);
    for id in ids {
        assert_eq!(editor.active().dimensions[&id].resolve(editor.doc(), None).from.y, 0.);
    }
    assert!(editor.undo());
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
}

#[test]
fn annotation_only_copy_preserves_anchors_and_rejects_out_of_plane_offset_atomically() {
    let mut editor = Editor::new();
    let dimension = test_linear_dimension(DVec3::ZERO, DVec3::X * 2.);
    let id = editor.apply(Command::AddDimension { dimension: dimension.clone() }).unwrap().dimensions[0];
    let before = editor.doc().clone();
    assert!(
        editor.apply(Command::Copy { elements: vec![Element::Dimension(id)], offset: DVec3::Z, copies: 1 }).is_err()
    );
    assert_eq!(editor.doc(), &before);
    let out =
        editor.apply(Command::Copy { elements: vec![Element::Dimension(id)], offset: DVec3::Y, copies: 2 }).unwrap();
    for (i, element) in out.created.iter().enumerate() {
        let Element::Dimension(id) = element else { panic!("dimension") };
        let copied = &editor.active().dimensions[id];
        assert_eq!(copied.measure, dimension.measure);
        assert_eq!(copied.offset, DVec3::Y * (i + 2) as f64);
    }
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
}

#[test]
fn radial_dimension_copies_retarget_curves_and_copied_instance_paths() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawCircle {
            centre: v(3., 4., 0.),
            normal: DVec3::Z,
            radius: 1.,
            segments: 12,
            start: Some(DVec3::X),
        })
        .unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    let face = editor.mesh().faces().next().unwrap().0;
    let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X);
    dimension.measure = DimensionMeasure::Radial {
        reference: RadialReference {
            radial_axes: None,
            arc_sweep: None,
            path: vec![],
            curve: Some(curve),
            centre: v(3., 4., 0.),
            normal: DVec3::Z,
            start: DVec3::X,
            radius: 1.,
        },
        diameter: false,
    };
    dimension.offset = DVec3::X * 2.;
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let out = editor
        .apply(Command::Copy {
            elements: vec![Element::Face(face), Element::Dimension(id)],
            offset: DVec3::X * 4.,
            copies: 1,
        })
        .unwrap();
    let copied =
        out.created.iter().find_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None }).unwrap();
    let measured = editor.active().dimensions[&copied].resolve(editor.doc(), None);
    assert!(measured.associated);
    assert_eq!(measured.from, v(7., 4., 0.));
    assert_eq!(measured.length, 1.);
    let out = editor.apply(Command::MakeGroup { elements: vec![Element::Face(face), Element::Dimension(id)] }).unwrap();
    let Element::Instance(group) = out.created[0] else { panic!("group") };
    let owner = Some(editor.active().instances[&group].definition);
    let mut external = editor.doc().def(owner).unwrap().dimensions[&id].clone();
    if let DimensionMeasure::Radial { reference, .. } = &mut external.measure {
        reference.path = vec![group];
    }
    let id = editor.apply(Command::AddDimension { dimension: external }).unwrap().dimensions[0];
    let out = editor
        .apply(Command::Copy {
            elements: vec![Element::Instance(group), Element::Dimension(id)],
            offset: DVec3::X * 4.,
            copies: 1,
        })
        .unwrap();
    let group_copy =
        out.created.iter().find_map(|e| if let Element::Instance(id) = e { Some(*id) } else { None }).unwrap();
    let dimension_copy =
        out.created.iter().find_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None }).unwrap();
    let dimension = &editor.active().dimensions[&dimension_copy];
    let DimensionMeasure::Radial { reference, .. } = &dimension.measure else { panic!("radial") };
    assert_eq!(reference.path, vec![group_copy]);
    let measured = dimension.resolve(editor.doc(), None);
    assert!(measured.associated);
    assert_eq!(measured.from, v(7., 4., 0.));
    assert_eq!(measured.length, 1.);
}

#[test]
fn dimension_move_previews_match_commit_without_changing_document_or_history() {
    use crate::dimensions::*;
    for (copy, with_geometry) in [false, true]
        .into_iter()
        .flat_map(|copy| [false, true].into_iter().map(move |with_geometry| (copy, with_geometry)))
    {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None }).unwrap();
        let (edge, member) = editor.mesh().edges().next().unwrap();
        let vertices = member.v;
        let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X * 2.);
        dimension.measure = DimensionMeasure::Linear {
            from: DimensionAnchor {
                point: DVec3::ZERO,
                reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[0] } }),
            },
            to: DimensionAnchor {
                point: DVec3::X * 2.,
                reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[1] } }),
            },
            axis: None,
        };
        let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
        let before = editor.doc().clone();
        let revision = editor.revision();
        let label = editor.undo_label().map(str::to_string);
        let elements = if with_geometry {
            vec![Element::Edge(edge), Element::Dimension(id)]
        } else {
            vec![Element::Dimension(id)]
        };
        let offset = DVec3::Y * 2.;
        let preview = editor.preview_dimension_move(&elements, offset, copy).unwrap();
        assert_eq!(preview.len(), 1);
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.revision(), revision);
        assert_eq!(editor.undo_label(), label.as_deref());
        let preview_layout = preview[0].layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
        let out = editor
            .apply(if copy {
                Command::Copy { elements, offset, copies: 1 }
            } else {
                Command::Move { elements, offset }
            })
            .unwrap();
        let id = if copy {
            out.created.iter().find_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None }).unwrap()
        } else {
            id
        };
        let actual = editor.active().dimensions[&id].layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
        assert_eq!(preview_layout.segments, actual.segments);
        assert_eq!(preview_layout.text_position, actual.text_position);
        assert_eq!(preview_layout.measurement.length, actual.measurement.length);
    }
}

#[test]
fn dimension_move_preview_uses_active_scaled_group_coordinates() {
    let mut editor = Editor::new();
    let id = editor
        .apply(Command::AddDimension { dimension: test_linear_dimension(DVec3::ZERO, DVec3::X * 2.) })
        .unwrap()
        .dimensions[0];
    let out = editor.apply(Command::MakeGroup { elements: vec![Element::Dimension(id)] }).unwrap();
    let Element::Instance(group) = out.created[0] else { panic!("group") };
    editor
        .apply(Command::Scale {
            elements: vec![Element::Instance(group)],
            origin: DVec3::ZERO,
            factors: DVec3::splat(2.),
        })
        .unwrap();
    editor.enter(group).unwrap();
    let before = editor.doc().clone();
    let owner = editor.active_definition();
    let transform = editor.context_transform();
    let preview = editor.preview_dimension_move(&[Element::Dimension(id)], DVec3::Y, false).unwrap();
    let layout = preview[0].layout(editor.doc(), owner, transform, 0.1).unwrap();
    assert_eq!(layout.measurement.length, 4.);
    assert_eq!(layout.text_position.y, 4.);
    assert_eq!(editor.doc(), &before);
    editor.apply(Command::Move { elements: vec![Element::Dimension(id)], offset: DVec3::Y }).unwrap();
    let actual = editor.active().dimensions[&id].layout(editor.doc(), owner, transform, 0.1).unwrap();
    assert_eq!(actual.segments, layout.segments);
}

#[test]
fn rotating_dimensions_preserves_moved_links_and_detaches_stationary_geometry() {
    use crate::dimensions::*;
    for with_geometry in [false, true] {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None }).unwrap();
        let (edge, member) = editor.mesh().edges().next().unwrap();
        let vertices = member.v;
        let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X * 2.);
        dimension.measure = DimensionMeasure::Linear {
            from: DimensionAnchor {
                point: DVec3::ZERO,
                reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[0] } }),
            },
            to: DimensionAnchor {
                point: DVec3::X * 2.,
                reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[1] } }),
            },
            axis: Some(DVec3::X),
        };
        dimension.text = "Length <>".into();
        dimension.font.size = 24.;
        let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
        let before = editor.doc().clone();
        let elements = if with_geometry {
            vec![Element::Edge(edge), Element::Dimension(id)]
        } else {
            vec![Element::Dimension(id)]
        };
        editor.apply(Command::Rotate { elements, centre: DVec3::ZERO, axis: DVec3::Z, angle: 90., copies: 0 }).unwrap();
        let dimension = &editor.active().dimensions[&id];
        let measured = dimension.resolve(editor.doc(), None);
        assert_eq!(measured.associated, with_geometry);
        assert!(measured.to.distance(DVec3::Y * 2.) < 1e-8);
        assert!((measured.length - 2.).abs() < 1e-8);
        assert!(dimension.offset.distance(-DVec3::X) < 1e-8);
        assert_eq!(dimension.font.size, 24.);
        assert_eq!(dimension.text, "Length <>");
        if !with_geometry {
            assert!(editor.mesh().find_vertex(DVec3::X * 2.).is_some());
        }
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
    }
}

#[test]
fn rotating_radial_dimension_with_circle_changes_plane_and_keeps_radius() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawCircle {
            centre: v(3., 4., 0.),
            normal: DVec3::Z,
            radius: 1.,
            segments: 12,
            start: Some(DVec3::X),
        })
        .unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    let face = editor.mesh().faces().next().unwrap().0;
    let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X);
    dimension.measure = DimensionMeasure::Radial {
        reference: RadialReference {
            radial_axes: None,
            arc_sweep: None,
            path: vec![],
            curve: Some(curve),
            centre: v(3., 4., 0.),
            normal: DVec3::Z,
            start: DVec3::X,
            radius: 1.,
        },
        diameter: false,
    };
    dimension.offset = DVec3::X * 2.;
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let before = editor.doc().clone();
    editor
        .apply(Command::Rotate {
            elements: vec![Element::Face(face), Element::Dimension(id)],
            centre: DVec3::ZERO,
            axis: DVec3::X,
            angle: 90.,
            copies: 0,
        })
        .unwrap();
    let dimension = &editor.active().dimensions[&id];
    let measured = dimension.resolve(editor.doc(), None);
    assert!(measured.associated);
    assert!(measured.from.distance(v(3., 0., 4.)) < 1e-8);
    assert!((measured.length - 1.).abs() < 1e-8);
    assert!(dimension.normal.distance(-DVec3::Y) < 1e-8);
    assert!(dimension.validate());
    assert!(dimension.layout(editor.doc(), None, DMat4::IDENTITY, 0.1).is_some());
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);

    let out = editor
        .apply(Command::Rotate {
            elements: vec![Element::Face(face), Element::Dimension(id)],
            centre: DVec3::ZERO,
            axis: DVec3::X,
            angle: 90.,
            copies: 1,
        })
        .unwrap();
    let copied =
        out.created.iter().find_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None }).unwrap();
    let measured = editor.active().dimensions[&copied].resolve(editor.doc(), None);
    assert!(measured.associated);
    assert!(measured.from.distance(v(3., 0., 4.)) < 1e-8);
    assert!((measured.length - 1.).abs() < 1e-8);
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
    let out = editor.apply(Command::MakeGroup { elements: vec![Element::Face(face)] }).unwrap();
    let Element::Instance(group) = out.created[0] else { panic!("group") };
    let grouped = editor.doc().clone();
    editor
        .apply(Command::Rotate {
            elements: vec![Element::Instance(group), Element::Dimension(id)],
            centre: DVec3::ZERO,
            axis: DVec3::X,
            angle: 90.,
            copies: 0,
        })
        .unwrap();
    let dimension = &editor.active().dimensions[&id];
    let measured = dimension.resolve(editor.doc(), None);
    assert!(measured.associated);
    assert!(measured.from.distance(v(3., 0., 4.)) < 1e-8);
    let DimensionMeasure::Radial { reference, .. } = &dimension.measure else { panic!("radial") };
    assert_eq!(reference.path, vec![group]);
    assert!(editor.undo());
    assert_eq!(editor.doc(), &grouped);
}

#[test]
fn rotated_dimension_arrays_retarget_selected_edges_and_detach_uncopied_links() {
    use crate::dimensions::*;
    for with_geometry in [false, true] {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[DVec3::X * 2., DVec3::X * 4.]], facing: None }).unwrap();
        let (edge, member) = editor.mesh().edges().next().unwrap();
        let vertices = member.v;
        let mut dimension = test_linear_dimension(DVec3::X * 2., DVec3::X * 4.);
        dimension.measure = DimensionMeasure::Linear {
            from: DimensionAnchor {
                point: DVec3::X * 2.,
                reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[0] } }),
            },
            to: DimensionAnchor {
                point: DVec3::X * 4.,
                reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[1] } }),
            },
            axis: None,
        };
        let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
        let before = editor.doc().clone();
        let elements = if with_geometry {
            vec![Element::Edge(edge), Element::Dimension(id)]
        } else {
            vec![Element::Dimension(id)]
        };
        let out = editor
            .apply(Command::Rotate { elements, centre: DVec3::ZERO, axis: DVec3::Z, angle: 90., copies: 2 })
            .unwrap();
        let ids = out
            .created
            .iter()
            .filter_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None })
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 2);
        for (id, expected) in ids.iter().zip([DVec3::Y * 2., -DVec3::X * 2.]) {
            let measured = editor.active().dimensions[id].resolve(editor.doc(), None);
            assert_eq!(measured.associated, with_geometry);
            assert!(measured.from.distance(expected) < 1e-8);
            assert!((measured.length - 2.).abs() < 1e-8);
        }
        assert!(editor.active().dimensions[&id].resolve(editor.doc(), None).associated);
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
    }
}

#[test]
fn arc_rotation_previews_match_clamped_arrow_geometry_and_preserve_editor_state() {
    use crate::dimensions::*;
    for (copy, with_geometry) in [false, true]
        .into_iter()
        .flat_map(|copy| [false, true].into_iter().map(move |with_geometry| (copy, with_geometry)))
    {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawArc {
                start: DVec3::ZERO,
                end: DVec3::X * 2.,
                normal: DVec3::Z,
                bulge: 1.,
                segments: 12,
            })
            .unwrap();
        let curve = editor.mesh().curves().next().unwrap().0;
        let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X);
        dimension.measure = DimensionMeasure::Radial {
            reference: RadialReference {
                radial_axes: None,
                arc_sweep: None,
                path: vec![],
                curve: Some(curve),
                centre: DVec3::X,
                normal: DVec3::Z,
                start: -DVec3::X,
                radius: 1.,
            },
            diameter: false,
        };
        dimension.offset = v(-1., -2., 0.);
        let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
        let before = editor.doc().clone();
        let revision = editor.revision();
        let mut elements = if with_geometry {
            editor.mesh().curve_edges(curve).into_iter().map(Element::Edge).collect::<Vec<_>>()
        } else {
            vec![]
        };
        elements.push(Element::Dimension(id));
        let preview = editor.preview_dimension_rotate(&elements, DVec3::ZERO, DVec3::X, 90., copy).unwrap();
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.revision(), revision);
        let expected = preview[0].layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
        let out = editor
            .apply(Command::Rotate {
                elements,
                centre: DVec3::ZERO,
                axis: DVec3::X,
                angle: 90.,
                copies: if copy { 1 } else { 0 },
            })
            .unwrap();
        let id = if copy {
            out.created.iter().find_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None }).unwrap()
        } else {
            id
        };
        let actual = editor.active().dimensions[&id].layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
        assert_eq!(actual.segments.len(), expected.segments.len());
        for (a, b) in actual.segments.iter().flatten().zip(expected.segments.iter().flatten()) {
            assert!(a.distance(*b) < 1e-8);
        }
        assert!(actual.arrows[0].0.distance(expected.arrows[0].0) < 1e-8);
    }
}

#[test]
fn exploding_mirrored_arc_dimension_preserves_label_and_arrow_positions() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawArc { start: DVec3::ZERO, end: DVec3::X * 2., normal: DVec3::Z, bulge: 1., segments: 12 })
        .unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X);
    dimension.measure = DimensionMeasure::Radial {
        reference: RadialReference {
            radial_axes: None,
            arc_sweep: None,
            path: vec![],
            curve: Some(curve),
            centre: DVec3::X,
            normal: DVec3::Z,
            start: -DVec3::X,
            radius: 1.,
        },
        diameter: false,
    };
    dimension.offset = v(-1., -2., 0.);
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let mut elements = editor.mesh().curve_edges(curve).into_iter().map(Element::Edge).collect::<Vec<_>>();
    elements.push(Element::Dimension(id));
    let out = editor.apply(Command::MakeGroup { elements }).unwrap();
    let Element::Instance(group) = out.created[0] else { panic!("group") };
    editor
        .apply(Command::Scale {
            elements: vec![Element::Instance(group)],
            origin: DVec3::ZERO,
            factors: v(-1., 1., 1.),
        })
        .unwrap();
    let instance = &editor.active().instances[&group];
    let owner = Some(instance.definition);
    let before =
        editor.doc().def(owner).unwrap().dimensions[&id].layout(editor.doc(), owner, instance.transform, 0.1).unwrap();
    editor.apply(Command::Explode { instance: group }).unwrap();
    let dimension = editor.active().dimensions.values().next().unwrap();
    let after = dimension.layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
    assert!(after.measurement.associated);
    assert!(after.text_position.distance(before.text_position) < 1e-8);
    assert!(after.arrows[0].0.distance(before.arrows[0].0) < 1e-8);
}

#[test]
fn invalid_rotation_arguments_and_array_overflow_are_atomic() {
    let mut editor = Editor::new();
    let id = editor
        .apply(Command::AddDimension { dimension: test_linear_dimension(DVec3::ZERO, DVec3::X) })
        .unwrap()
        .dimensions[0];
    let before = editor.doc().clone();
    for (axis, angle, copies) in [(DVec3::ZERO, 90., 0), (DVec3::Z, f64::NAN, 0), (DVec3::Z, f64::MAX, 2)] {
        assert!(
            editor
                .apply(Command::Rotate {
                    elements: vec![Element::Dimension(id)],
                    centre: DVec3::ZERO,
                    axis,
                    angle,
                    copies
                })
                .is_err()
        );
        assert_eq!(editor.doc(), &before);
    }
}

#[test]
fn scaling_linear_dimensions_preserves_moved_links_and_screen_font_size() {
    use crate::dimensions::*;
    for (with_geometry, factors) in [false, true]
        .into_iter()
        .flat_map(|linked| [v(2., 3., 1.), v(-2., 1., 1.)].into_iter().map(move |factors| (linked, factors)))
    {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[v(1., 2., 0.), v(3., 2., 0.)]], facing: None }).unwrap();
        let (edge, member) = editor.mesh().edges().next().unwrap();
        let vertices = member.v;
        let mut dimension = test_linear_dimension(v(1., 2., 0.), v(3., 2., 0.));
        dimension.measure = DimensionMeasure::Linear {
            from: DimensionAnchor {
                point: v(1., 2., 0.),
                reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[0] } }),
            },
            to: DimensionAnchor {
                point: v(3., 2., 0.),
                reference: Some(PointReference { path: vec![], entity: PointEntity::Vertex { vertex: vertices[1] } }),
            },
            axis: Some(DVec3::X),
        };
        dimension.font.size = 24.;
        dimension.text = "Width <>".into();
        let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
        let before = editor.doc().clone();
        let origin = v(1., 1., 0.);
        let basis = DMat4::from_rotation_z(std::f64::consts::FRAC_PI_4);
        let matrix = DMat4::from_translation(origin)
            * basis
            * DMat4::from_scale(factors)
            * basis.inverse()
            * DMat4::from_translation(-origin);
        let elements = if with_geometry {
            vec![Element::Edge(edge), Element::Dimension(id)]
        } else {
            vec![Element::Dimension(id)]
        };
        let preview = editor.preview_dimension_scale(&elements, origin, factors, basis).unwrap();
        assert_eq!(editor.doc(), &before);
        editor
            .apply(Command::ScaleInAxes {
                elements,
                origin,
                factors,
                x: basis.x_axis.truncate(),
                y: basis.y_axis.truncate(),
            })
            .unwrap();
        let dimension = &editor.active().dimensions[&id];
        let measured = dimension.resolve(editor.doc(), None);
        assert_eq!(measured.associated, with_geometry);
        assert!(measured.from.distance(matrix.transform_point3(v(1., 2., 0.))) < 1e-8);
        assert!(measured.to.distance(matrix.transform_point3(v(3., 2., 0.))) < 1e-8);
        assert_eq!(dimension.font.size, 24.);
        assert_eq!(dimension.text, "Width <>");
        let actual = dimension.layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
        let expected = preview[0].layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
        for (a, b) in actual.segments.iter().flatten().zip(expected.segments.iter().flatten()) {
            assert!(a.distance(*b) < 1e-8);
        }
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
    }
}

#[test]
fn anisotropic_radial_scaling_matches_the_affine_curve_image_and_keeps_preview_exact() {
    use crate::dimensions::*;
    for (offset, factors) in [DVec3::X * 2., DVec3::Y * 2., v(2., 2., 0.)]
        .into_iter()
        .flat_map(|offset| [v(2., 1., 1.), v(2., 2., 2.)].into_iter().map(move |factors| (offset, factors)))
    {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawCircle {
                centre: DVec3::ZERO,
                normal: DVec3::Z,
                radius: 1.,
                segments: 24,
                start: Some(DVec3::X),
            })
            .unwrap();
        let curve = editor.mesh().curves().next().unwrap().0;
        let face = editor.mesh().faces().next().unwrap().0;
        let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X);
        dimension.measure = DimensionMeasure::Radial {
            reference: RadialReference {
                radial_axes: None,
                arc_sweep: None,
                path: vec![],
                curve: Some(curve),
                centre: DVec3::ZERO,
                normal: DVec3::Z,
                start: DVec3::X,
                radius: 1.,
            },
            diameter: false,
        };
        dimension.offset = offset;
        let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
        let before = editor.doc().clone();
        let matrix = DMat4::from_scale(factors);
        let expected = editor.active().dimensions[&id].layout(editor.doc(), None, matrix, 0.1).unwrap();
        let elements = vec![Element::Face(face), Element::Dimension(id)];
        let preview = editor.preview_dimension_scale(&elements, DVec3::ZERO, factors, DMat4::IDENTITY).unwrap();
        assert_eq!(editor.doc(), &before);
        editor.apply(Command::Scale { elements, origin: DVec3::ZERO, factors }).unwrap();
        let dimension = &editor.active().dimensions[&id];
        let actual = dimension.layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
        assert_eq!(actual.measurement.associated, (factors.x - factors.y).abs() < 1e-9);
        assert!((actual.measurement.length - expected.measurement.length).abs() < 1e-8);
        assert!(actual.arrows[0].0.distance(expected.arrows[0].0) < 1e-8);
        assert!(actual.text_position.distance(expected.text_position) < 1e-8);
        let predicted = preview[0].layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
        assert!(predicted.arrows[0].0.distance(actual.arrows[0].0) < 1e-8);
        let DimensionMeasure::Radial { reference, .. } = &dimension.measure else { panic!("radial") };
        assert_eq!(reference.radial_axes.is_some(), (factors.x - factors.y).abs() > 1e-9);
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
    }
}

#[test]
fn scaled_arc_fallback_preserves_affine_sector_and_mirrored_tip() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawArc { start: DVec3::ZERO, end: DVec3::X * 2., normal: DVec3::Z, bulge: 1., segments: 12 })
        .unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X);
    dimension.measure = DimensionMeasure::Radial {
        reference: RadialReference {
            radial_axes: None,
            arc_sweep: None,
            path: vec![],
            curve: Some(curve),
            centre: DVec3::X,
            normal: DVec3::Z,
            start: -DVec3::X,
            radius: 1.,
        },
        diameter: false,
    };
    dimension.offset = v(-1., -2., 0.);
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let factors = v(-2., 1., 1.);
    let matrix = DMat4::from_scale(factors);
    let expected = editor.active().dimensions[&id].layout(editor.doc(), None, matrix, 0.1).unwrap();
    let mut elements = editor.mesh().curve_edges(curve).into_iter().map(Element::Edge).collect::<Vec<_>>();
    elements.push(Element::Dimension(id));
    editor.apply(Command::Scale { elements, origin: DVec3::ZERO, factors }).unwrap();
    let actual = editor.active().dimensions[&id].layout(editor.doc(), None, DMat4::IDENTITY, 0.1).unwrap();
    assert!(!actual.measurement.associated);
    assert!(actual.arrows[0].0.distance(expected.arrows[0].0) < 1e-8);
    assert!(actual.text_position.distance(expected.text_position) < 1e-8);
    assert!((actual.measurement.length - expected.measurement.length).abs() < 1e-8);
}

#[test]
fn nested_group_annotation_previews_match_move_rotate_scale_and_copy() {
    for operation in 0..4 {
        let mut editor = Editor::new();
        editor.apply(Command::DrawLines { lines: vec![[v(2., 3., 0.), v(4., 3., 0.)]], facing: None }).unwrap();
        let edge = editor.mesh().edges().next().unwrap().0;
        let id = editor
            .apply(Command::AddDimension { dimension: test_linear_dimension(v(2., 3., 0.), v(4., 3., 0.)) })
            .unwrap()
            .dimensions[0];
        let out =
            editor.apply(Command::MakeGroup { elements: vec![Element::Edge(edge), Element::Dimension(id)] }).unwrap();
        let Element::Instance(inner) = out.created[0] else { panic!("inner") };
        let out = editor.apply(Command::MakeGroup { elements: vec![Element::Instance(inner)] }).unwrap();
        let Element::Instance(outer) = out.created[0] else { panic!("outer") };
        let elements = vec![Element::Instance(outer)];
        let (matrix, command) = match operation {
            0 => (
                DMat4::from_translation(DVec3::X * 3.),
                Command::Move { elements: elements.clone(), offset: DVec3::X * 3. },
            ),
            1 => (
                DMat4::from_rotation_z(std::f64::consts::FRAC_PI_2),
                Command::Rotate {
                    elements: elements.clone(),
                    centre: DVec3::ZERO,
                    axis: DVec3::Z,
                    angle: 90.,
                    copies: 0,
                },
            ),
            2 => (
                DMat4::from_scale(v(-2., 1., 1.)),
                Command::Scale { elements: elements.clone(), origin: DVec3::ZERO, factors: v(-2., 1., 1.) },
            ),
            _ => (
                DMat4::from_translation(DVec3::X * 3.),
                Command::Copy { elements: elements.clone(), offset: DVec3::X * 3., copies: 1 },
            ),
        };
        let before = editor.doc().clone();
        let revision = editor.revision();
        let previews = editor.preview_instance_dimensions(&elements, matrix).unwrap();
        assert_eq!(previews.len(), 1);
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.revision(), revision);
        let expected = previews[0].0.layout(editor.doc(), None, previews[0].1, 0.1).unwrap();
        let out = editor.apply(command).unwrap();
        let target = if operation == 3 {
            out.created.iter().find_map(|e| if let Element::Instance(id) = e { Some(*id) } else { None }).unwrap()
        } else {
            outer
        };
        let occurrence = editor
            .doc()
            .occurrences(&[])
            .into_iter()
            .find(|o| {
                o.path.first() == Some(&target) && editor.doc().def(o.definition).unwrap().dimensions.contains_key(&id)
            })
            .unwrap();
        let actual = editor.doc().def(occurrence.definition).unwrap().dimensions[&id]
            .layout(editor.doc(), occurrence.definition, occurrence.transform, 0.1)
            .unwrap();
        for (a, b) in actual.segments.iter().flatten().zip(expected.segments.iter().flatten()) {
            assert!(a.distance(*b) < 1e-8);
        }
        assert!(actual.text_position.distance(expected.text_position) < 1e-8);
        assert!((actual.measurement.length - expected.measurement.length).abs() < 1e-8);
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
    }
}

#[test]
fn instance_preview_uses_active_context_coordinates_and_preserves_affine_radial_layout() {
    use crate::dimensions::*;
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawCircle {
            centre: v(3., 4., 0.),
            normal: DVec3::Z,
            radius: 1.,
            segments: 12,
            start: Some(DVec3::X),
        })
        .unwrap();
    let curve = editor.mesh().curves().next().unwrap().0;
    let face = editor.mesh().faces().next().unwrap().0;
    let mut dimension = test_linear_dimension(DVec3::ZERO, DVec3::X);
    dimension.measure = DimensionMeasure::Radial {
        reference: RadialReference {
            radial_axes: None,
            arc_sweep: None,
            path: vec![],
            curve: Some(curve),
            centre: v(3., 4., 0.),
            normal: DVec3::Z,
            start: DVec3::X,
            radius: 1.,
        },
        diameter: false,
    };
    dimension.offset = v(2., 2., 0.);
    let id = editor.apply(Command::AddDimension { dimension }).unwrap().dimensions[0];
    let out = editor.apply(Command::MakeGroup { elements: vec![Element::Face(face), Element::Dimension(id)] }).unwrap();
    let Element::Instance(inner) = out.created[0] else { panic!("inner") };
    let out = editor.apply(Command::MakeGroup { elements: vec![Element::Instance(inner)] }).unwrap();
    let Element::Instance(outer) = out.created[0] else { panic!("outer") };
    editor
        .apply(Command::Scale {
            elements: vec![Element::Instance(outer)],
            origin: DVec3::ZERO,
            factors: DVec3::splat(2.),
        })
        .unwrap();
    editor.enter(outer).unwrap();
    let context = editor.context_transform();
    let matrix = DMat4::from_scale(v(2., 1., 1.));
    let before = editor.doc().clone();
    let previews = editor.preview_instance_dimensions(&[Element::Instance(inner)], matrix).unwrap();
    assert_eq!(previews.len(), 1);
    assert_eq!(editor.doc(), &before);
    let expected =
        previews[0].0.layout(editor.doc(), editor.active_definition(), context * previews[0].1, 0.1).unwrap();
    editor
        .apply(Command::Scale { elements: vec![Element::Instance(inner)], origin: DVec3::ZERO, factors: v(2., 1., 1.) })
        .unwrap();
    let instance = &editor.active().instances[&inner];
    let owner = Some(instance.definition);
    let actual = editor.doc().def(owner).unwrap().dimensions[&id]
        .layout(editor.doc(), owner, context * instance.transform, 0.1)
        .unwrap();
    assert!((expected.measurement.length - actual.measurement.length).abs() < 1e-8);
    assert!(expected.text_position.distance(actual.text_position) < 1e-8);
    assert!(expected.arrows[0].0.distance(actual.arrows[0].0) < 1e-8);
}

#[test]
fn guide_visibility_survives_copy_scene_and_undo() {
    let mut e = Editor::new();
    let element = e.apply(Command::AddGuideSegment { start: DVec3::ZERO, point: DVec3::X }).unwrap().created[0];
    e.apply(Command::SetGeometryHidden { elements: vec![element], hidden: true }).unwrap();
    assert!(e.active().guides.values().next().unwrap().hidden());
    let scene = crate::scenes::Scene::capture(e.doc(), rysmo_math::OrbitCamera::default(), "Guide hidden".into());
    assert_eq!(scene.contexts[0].hidden_guides.len(), 1);
    let copied = e.apply(Command::Copy { elements: vec![element], offset: DVec3::Y, copies: 1 }).unwrap();
    assert!(e.active().guides.values().all(Guide::hidden));
    e.apply(Command::SetGeometryHidden { elements: copied.created, hidden: false }).unwrap();
    assert_eq!(e.active().guides.values().filter(|g| g.hidden()).count(), 1);
    e.undo();
    assert!(e.active().guides.values().all(Guide::hidden));
}

#[test]
fn editing_an_extruded_circle_radius_updates_both_rims_and_undo() {
    let mut e = Editor::new();
    e.apply(Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1., segments: 24, start: None })
        .unwrap();
    let face = e.mesh().faces().next().unwrap().0;
    e.apply(Command::PushPull { face, distance: -3., keep_base: false }).unwrap();
    let before = e.doc().clone();
    let curve = e.mesh().curves().next().unwrap().0;
    e.apply(Command::SetCurveRadius { curve, radius: 2. }).unwrap();
    assert!(e.mesh().curves().all(|(_, curve)| (curve.radius - 2.).abs() < 1e-9));
    assert!(e.mesh().vertices().all(|(_, vertex)| (vertex.pos.truncate().length() - 2.).abs() < 1e-9));
    assert_eq!(e.mesh().face_count(), before.root.mesh.face_count());
    e.mesh().validate().unwrap();
    e.undo();
    assert_eq!(e.doc(), &before);
}

#[test]
fn moved_face_splits_keep_the_moved_texture_on_every_region() {
    let mut e = Editor::new();
    e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
    let face = e.mesh().faces().next().unwrap().0;
    let normal = e.mesh().face_normal(face).unwrap();
    let placement = crate::TexturePlacement { offset: [0.1, 0.2], ..Default::default() };
    e.apply(Command::SetTexturePlacement { faces: vec![face], placement: Some(placement) }).unwrap();
    e.apply(Command::DrawLines { lines: vec![[DVec3::new(2.5, -1., 0.), DVec3::new(2.5, 2., 0.)]], facing: None })
        .unwrap();
    let before = e.doc().clone();
    let moved = e.apply(Command::Move { elements: vec![Element::Face(face)], offset: DVec3::X * 2. }).unwrap();
    assert_eq!(moved.created.iter().filter(|e| matches!(e, Element::Face(_))).count(), 2);
    let expected = crate::texture::moved(placement, normal, DVec3::X * 2.);
    assert_eq!(e.mesh().face_count(), 2);
    assert!(e.mesh().faces().all(|(_, face)| face.uv == Some(expected)));
    e.mesh().validate().unwrap();
    e.undo();
    assert_eq!(e.doc(), &before);
}
