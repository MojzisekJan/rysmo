use rysmo_math::DVec3;

use crate::*;

fn rect(e: &mut Editor) -> Outcome {
    e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap()
}

fn the_face(e: &Editor) -> FaceId {
    e.mesh().faces().next().unwrap().0
}

#[test]
fn undo_and_redo_round_trip() {
    let mut e = Editor::new();
    let empty = e.doc().clone();
    rect(&mut e);
    let after_rect = e.doc().clone();
    let f = the_face(&e);
    e.apply(Command::PushPull { face: f, distance: -2.0, keep_base: false }).unwrap();
    let after_box = e.doc().clone();
    assert_eq!(e.mesh().face_count(), 6);
    assert_eq!(e.undo_label(), Some("Push/Pull"));

    assert!(e.undo());
    assert_eq!(e.doc(), &after_rect);
    assert!(e.undo());
    assert_eq!(e.doc(), &empty);
    assert!(!e.undo());

    assert!(e.redo());
    assert_eq!(e.doc(), &after_rect);
    assert!(e.redo());
    assert_eq!(e.doc(), &after_box);
    assert!(!e.redo());
}

#[test]
fn new_command_clears_redo() {
    let mut e = Editor::new();
    rect(&mut e);
    e.undo();
    assert_eq!(e.redo_label(), Some("Rectangle"));
    e.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::Z]], facing: None }).unwrap();
    assert!(!e.redo());
}

#[test]
fn failed_command_changes_nothing() {
    let mut e = Editor::new();
    rect(&mut e);
    let before = e.doc().clone();
    let rev = e.revision();
    let f = the_face(&e);
    assert!(e.apply(Command::PushPull { face: f, distance: 0.0, keep_base: false }).is_err());
    assert!(e.apply(Command::PushPull { face: FaceId(9999), distance: 1.0, keep_base: false }).is_err());
    assert_eq!(e.doc(), &before);
    assert_eq!(e.revision(), rev);
    assert_eq!(e.undo_label(), Some("Rectangle"));
}

#[test]
fn erased_elements_leave_the_selection() {
    let mut e = Editor::new();
    let out = rect(&mut e);
    e.set_selection(out.created.clone());
    assert_eq!(e.selection().len(), 5);
    let f = the_face(&e);
    e.apply(Command::Erase { elements: vec![Element::Face(f)] }).unwrap();
    assert_eq!(e.selection().len(), 4);
    assert!(!e.is_selected(Element::Face(f)));
}

#[test]
fn materials_and_paint() {
    let mut e = Editor::new();
    rect(&mut e);
    let m = e
        .apply(Command::AddMaterial { name: "Brick".into(), color: [0.7, 0.3, 0.2, 1.0], texture: None })
        .unwrap()
        .material
        .unwrap();
    let f = the_face(&e);
    e.apply(Command::Paint { faces: vec![f], material: Some(m), back: false }).unwrap();
    assert_eq!(e.mesh().face(f).unwrap().material, Some(m));
    assert_eq!(e.doc().material_by_name("Brick"), Some(m));
    assert!(e.apply(Command::Paint { faces: vec![f], material: Some(MaterialId(12345)), back: false }).is_err());
    e.undo();
    assert_eq!(e.mesh().face(f).unwrap().material, None);
}

#[test]
fn circle_command_makes_a_face_with_the_asked_segments() {
    let mut e = Editor::new();
    e.apply(Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 0.5, segments: 12, start: None })
        .unwrap();
    assert_eq!(e.mesh().face_count(), 1);
    assert_eq!(e.mesh().edge_count(), 12);
}

#[test]
fn copy_array_and_units() {
    let mut e = Editor::new();
    rect(&mut e);
    let f = the_face(&e);
    e.apply(Command::Copy { elements: vec![Element::Face(f)], offset: DVec3::new(2.0, 0.0, 0.0), copies: 4 }).unwrap();
    assert_eq!(e.mesh().face_count(), 5);
    let out = e.apply(Command::SetUnits { units: Units::Metres }).unwrap();
    assert!(out.changed);
    assert!(!e.apply(Command::SetUnits { units: Units::Metres }).unwrap().changed);
}

#[test]
fn commands_are_json() {
    let c = Command::PushPull { face: FaceId(7), distance: 0.25, keep_base: true };
    let s = serde_json::to_string(&c).unwrap();
    assert_eq!(s, r#"{"op":"push_pull","face":7,"distance":0.25,"keep_base":true}"#);
    let back: Command = serde_json::from_str(&s).unwrap();
    assert_eq!(back, c);
    let lines: Command = serde_json::from_str(r#"{"op":"draw_lines","lines":[[[0,0,0],[1,0,0]]]}"#).unwrap();
    assert!(matches!(lines, Command::DrawLines { .. }));
    let erase: Command = serde_json::from_str(r#"{"op":"erase","elements":[{"kind":"edge","id":3}]}"#).unwrap();
    assert_eq!(erase, Command::Erase { elements: vec![Element::Edge(EdgeId(3))] });
}
