use rysmo_math::DVec3;
use rysmo_model::{Command, Editor};

#[test]
fn welded_and_exploded_curve_metadata_survives_native_round_trips() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawLines {
            lines: vec![[DVec3::ZERO, DVec3::X], [DVec3::X, DVec3::X + DVec3::Y]],
            facing: None,
        })
        .unwrap();
    let edges: Vec<_> = editor.mesh().edges().map(|(id, _)| id).collect();
    editor.apply(Command::WeldEdges { edges: edges.clone() }).unwrap();
    let source = crate::save(editor.doc());
    let loaded = crate::load(&source).unwrap();
    assert_eq!(crate::save(&loaded), source);
    assert_eq!(loaded.root.mesh.curves().count(), 1);
    assert_eq!(loaded.root.mesh.curves().next().unwrap().1.kind, rysmo_mesh::CurveKind::Polyline);
    let mut editor = Editor::with_document(loaded);
    editor.apply(Command::ExplodeCurves { edges: vec![edges[0]] }).unwrap();
    let source = crate::save(editor.doc());
    let loaded = crate::load(&source).unwrap();
    assert_eq!(crate::save(&loaded), source);
    assert_eq!(loaded.root.mesh.curves().count(), 0);
    assert_eq!(loaded.root.mesh.edge_count(), 2);
}
#[test]
fn consistent_face_orientation_survives_save_open_and_keeps_solid_volume() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
        .unwrap();
    let face = editor.mesh().faces().next().unwrap().0;
    editor.apply(Command::PushPull { face, distance: 2., keep_base: false }).unwrap();
    let faces: Vec<_> = editor.mesh().faces().map(|(id, _)| id).collect();
    editor.apply(Command::ReverseFaces { faces: vec![faces[1]] }).unwrap();
    editor.apply(Command::OrientFaces { seed: faces[0] }).unwrap();
    let source = crate::save(editor.doc());
    let loaded = crate::load(&source).unwrap();
    assert_eq!(crate::save(&loaded), source);
    let faces = loaded.root.mesh.faces().map(|(id, _)| id).collect();
    assert!((loaded.root.mesh.signed_volume(&faces) - 2.).abs() < 1e-8);
    assert!(loaded.root.mesh.orientation_plan(faces.iter().copied().next().unwrap()).unwrap().is_empty());
}
