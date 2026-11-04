use crate::{Command, Editor, Units};
#[test]
fn undo_to_saved_state_is_clean() {
    let mut editor = Editor::new();
    editor.apply(Command::SetUnits { units: Units::Metres }).unwrap();
    editor.mark_saved();
    editor.apply(Command::SetUnits { units: Units::Centimetres }).unwrap();
    editor.undo();
    assert_eq!(editor.doc().units, Units::Metres);
    assert!(!editor.is_dirty(), "Undo reached the saved model but it is still marked dirty");
}
#[test]
fn redo_to_saved_state_is_clean() {
    let mut editor = Editor::new();
    editor.apply(Command::SetUnits { units: Units::Metres }).unwrap();
    editor.mark_saved();
    editor.undo();
    editor.redo();
    assert!(!editor.is_dirty(), "Redo reached the saved model but it is still marked dirty");
}
#[test]
fn amendment_preserves_the_later_view() {
    let mut editor = Editor::new();
    let mut camera = editor.doc().camera.unwrap_or_default();
    camera.distance = 10.;
    editor.set_camera(camera);
    editor.apply(Command::SetUnits { units: Units::Metres }).unwrap();
    camera.distance = 3.;
    editor.set_camera(camera);
    editor.amend_last(Command::SetUnits { units: Units::Centimetres }).unwrap();
    assert_eq!(editor.doc().camera, Some(camera), "Amendment restored the old camera snapshot");
}

#[test]
fn a_new_branch_does_not_reuse_the_saved_state_identity() {
    let mut editor = Editor::new();
    editor.apply(Command::SetUnits { units: Units::Metres }).unwrap();
    editor.mark_saved();
    editor.undo();
    editor.apply(Command::SetUnits { units: Units::Centimetres }).unwrap();
    assert!(editor.is_dirty());
    assert!(editor.redo_label().is_none());
}
#[test]
fn no_op_amendment_returns_to_the_saved_baseline_without_moving_the_view() {
    let mut editor = Editor::new();
    let mut camera = editor.doc().camera.unwrap_or_default();
    camera.distance = 10.;
    editor.set_camera(camera);
    editor.mark_saved();
    editor.apply(Command::SetUnits { units: Units::Metres }).unwrap();
    camera.distance = 3.;
    editor.set_camera(camera);
    editor.amend_last(Command::SetUnits { units: Units::Millimetres }).unwrap();
    assert_eq!(editor.doc().camera, Some(camera));
    assert!(!editor.is_dirty());
    assert_eq!(editor.undo_depth(), 0);
}
#[test]
fn amended_camera_operations_keep_camera_undo_and_redo() {
    let mut editor = Editor::new();
    let mut camera = editor.doc().camera.unwrap_or_default();
    camera.distance = 10.;
    editor.set_camera(camera);
    editor.apply(Command::ResizeContext { factor: 2. }).unwrap();
    editor.amend_last(Command::ResizeContext { factor: 3. }).unwrap();
    assert_eq!(editor.doc().camera.unwrap().distance, 30.);
    editor.undo();
    assert_eq!(editor.doc().camera, Some(camera));
    editor.redo();
    assert_eq!(editor.doc().camera.unwrap().distance, 30.);
    editor.amend_last(Command::ResizeContext { factor: 1. }).unwrap();
    assert_eq!(editor.doc().camera, Some(camera));
    assert!(!editor.is_dirty());
}
#[test]
fn an_amendment_after_saving_is_a_new_unsaved_state() {
    let mut editor = Editor::new();
    editor.apply(Command::SetUnits { units: Units::Metres }).unwrap();
    editor.mark_saved();
    editor.amend_last(Command::SetUnits { units: Units::Centimetres }).unwrap();
    assert!(editor.is_dirty());
    editor.undo();
    assert!(editor.is_dirty());
    editor.redo();
    assert!(editor.is_dirty());
}
#[test]
fn a_failed_amendment_keeps_document_history_and_saved_state() {
    let mut editor = Editor::new();
    editor.apply(Command::SetUnits { units: Units::Metres }).unwrap();
    editor.mark_saved();
    let before = editor.clone();
    assert!(editor.amend_last(Command::ResizeContext { factor: -1. }).is_err());
    assert_eq!(editor.doc(), before.doc());
    assert_eq!(editor.history_position(), before.history_position());
    assert_eq!(editor.revision(), before.revision());
    assert!(!editor.is_dirty());
}
