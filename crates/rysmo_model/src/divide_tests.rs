use crate::{Command, Editor, Element};
use rysmo_math::DVec3;

#[test]
fn divided_solid_remains_closed_and_round_trips_in_nested_context() {
    for nested in [false, true] {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::ZERO,
                u: DVec3::X * 4.,
                v: DVec3::Y * 2.,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        editor.apply(Command::PushPull { face, distance: 3., keep_base: false }).unwrap();
        if nested {
            let elements = editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
            let out = editor.apply(Command::MakeGroup { elements }).unwrap();
            let Element::Instance(id) = out.created[0] else { panic!() };
            editor.enter(id).unwrap();
        }
        let before = editor.doc().clone();
        let depth = editor.undo_depth();
        let faces: Vec<_> = editor.mesh().faces().map(|(id, _)| id).collect();
        let edge = editor.mesh().edges().next().unwrap().0;
        let out = editor.apply(Command::DivideEdges { edges: vec![edge], segments: 5 }).unwrap();
        assert_eq!(out.created.len(), 5);
        assert_eq!(editor.undo_depth(), depth + 1);
        assert_eq!(editor.undo_label(), Some("Divide Edges"));
        assert_eq!(editor.mesh().faces().map(|(id, _)| id).collect::<Vec<_>>(), faces);
        assert!((crate::solid_validation::validate(editor.mesh(), false).unwrap() - 24.).abs() < 1e-8);
        let after = editor.doc().clone();
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
        assert!(editor.redo());
        assert_eq!(editor.doc(), &after);
    }
}

#[test]
fn rejected_curve_division_preserves_document_revision_and_history() {
    let mut editor = Editor::new();
    editor
        .apply(Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 2., segments: 24, start: None })
        .unwrap();
    let edge = editor.mesh().edges().next().unwrap().0;
    let before = editor.doc().clone();
    let revision = editor.revision();
    let depth = editor.undo_depth();
    assert!(editor.apply(Command::DivideEdges { edges: vec![edge], segments: 4 }).is_err());
    assert_eq!(editor.doc(), &before);
    assert_eq!(editor.revision(), revision);
    assert_eq!(editor.undo_depth(), depth);
}
