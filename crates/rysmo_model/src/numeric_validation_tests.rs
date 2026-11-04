use crate::{Command, Editor, Element, Guide};
use rysmo_math::DVec3;

#[test]
fn invalid_guide_creation_is_atomic() {
    for command in [
        Command::AddGuidePoint { point: DVec3::splat(f64::NAN) },
        Command::AddGuideLine { origin: DVec3::splat(f64::INFINITY), dir: DVec3::X },
        Command::AddGuideLine { origin: DVec3::ZERO, dir: DVec3::splat(1e308) },
    ] {
        let mut editor = Editor::new();
        let before = editor.doc().clone();
        assert!(editor.apply(command).is_err());
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.undo_depth(), 0);
        assert!(!editor.is_dirty());
    }
}

#[test]
fn overflowing_guide_move_and_copy_are_atomic() {
    for copy in [false, true] {
        let mut editor = Editor::new();
        let made = editor.apply(Command::AddGuidePoint { point: DVec3::X * 1e308 }).unwrap();
        let before = editor.doc().clone();
        let depth = editor.undo_depth();
        let command = if copy {
            Command::Copy { elements: made.created, offset: DVec3::X * 1e308, copies: 1 }
        } else {
            Command::Move { elements: made.created, offset: DVec3::X * 1e308 }
        };
        assert!(editor.apply(command).is_err());
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.undo_depth(), depth);
    }
}

#[test]
fn overflowing_instance_move_and_copy_are_atomic() {
    for copy in [false, true] {
        let mut editor = Editor::new();
        let made = editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let made = editor.apply(Command::MakeGroup { elements: made.created }).unwrap();
        let elements = made.created;
        editor.apply(Command::Move { elements: elements.clone(), offset: DVec3::X * 1e308 }).unwrap();
        let before = editor.doc().clone();
        let depth = editor.undo_depth();
        let command = if copy {
            Command::Copy { elements, offset: DVec3::X * 1e308, copies: 1 }
        } else {
            Command::Move { elements, offset: DVec3::X * 1e308 }
        };
        assert!(editor.apply(command).is_err());
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.undo_depth(), depth);
    }
}

#[test]
fn ordinary_guides_still_move_copy_and_undo() {
    let mut editor = Editor::new();
    let made = editor.apply(Command::AddGuideLine { origin: DVec3::ZERO, dir: DVec3::X * 2. }).unwrap();
    let before = editor.doc().clone();
    editor.apply(Command::Move { elements: made.created.clone(), offset: DVec3::Y }).unwrap();
    let copied = editor.apply(Command::Copy { elements: made.created, offset: DVec3::Z, copies: 1 }).unwrap();
    let Element::Guide(id) = copied.created[0] else { panic!() };
    assert_eq!(editor.active().guides[&id], Guide::Line { origin: DVec3::Y + DVec3::Z, dir: DVec3::X, hidden: false });
    assert!(editor.undo());
    assert!(editor.undo());
    assert_eq!(editor.doc(), &before);
}

#[test]
fn overflowing_section_move_and_copy_are_atomic() {
    for copy in [false, true] {
        let mut editor = Editor::new();
        let made = editor
            .apply(Command::AddSection {
                plane: crate::sections::SectionPlane {
                    name: "Section".into(),
                    origin: DVec3::X * 1e308,
                    normal: DVec3::Z,
                },
            })
            .unwrap();
        let before = editor.doc().clone();
        let depth = editor.undo_depth();
        let command = if copy {
            Command::Copy { elements: made.created, offset: DVec3::X * 1e308, copies: 1 }
        } else {
            Command::Move { elements: made.created, offset: DVec3::X * 1e308 }
        };
        assert!(editor.apply(command).is_err());
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.undo_depth(), depth);
    }
}
