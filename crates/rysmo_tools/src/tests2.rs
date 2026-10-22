//! Milestone 2 tools driven through pointer events and the VCB.

use rysmo_math::DVec3;
use rysmo_mesh::Element;
use rysmo_model::{Command, Guide};

use crate::tests::{Rig, v};
use crate::*;

fn square(r: &mut Rig, size: f64) {
    r.editor
        .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: v(size, 0.0, 0.0), v: v(0.0, size, 0.0), facing: None })
        .unwrap();
}

fn cube(r: &mut Rig) {
    square(r, 1.0);
    let f = r.editor.mesh().faces().next().unwrap().0;
    r.editor.apply(Command::PushPull { face: f, distance: -1.0, keep_base: false }).unwrap();
}

fn all(r: &Rig) -> Vec<Element> {
    let m = r.editor.mesh();
    m.edges().map(|(e, _)| Element::Edge(e)).chain(m.faces().map(|(f, _)| Element::Face(f))).collect()
}

#[test]
fn rotate_by_clicks_snaps_to_ninety() {
    let mut r = Rig::new(ToolKind::Rotate);
    square(&mut r, 1.0);
    let face = r.editor.mesh().faces().next().unwrap().0;
    r.editor.set_selection([Element::Face(face)]);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(2.0, 0.0, 0.0));
    r.hover(v(0.02, 2.0, 0.0));
    assert_eq!(r.tools.vcb_label(), "Angle");
    r.click(v(0.02, 2.0, 0.0));
    assert!(r.editor.mesh().find_vertex(v(-1.0, 1.0, 0.0)).is_some(), "rotated a quarter turn");
    assert_eq!(r.editor.undo_label(), Some("Rotate"));
}

#[test]
fn rotate_typed_angle_then_radial_array() {
    let mut r = Rig::new(ToolKind::Rotate);
    r.editor
        .apply(Command::DrawRectangle {
            corner: v(2.0, -0.25, 0.0),
            u: v(0.5, 0.0, 0.0),
            v: v(0.0, 0.5, 0.0),
            facing: None,
        })
        .unwrap();
    let face = r.editor.mesh().faces().next().unwrap().0;
    r.editor.set_selection([Element::Face(face)]);
    r.mods.ctrl = true;
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1.0, 0.0, 0.0));
    r.hover(v(1.0, 0.5, 0.0));
    r.type_vcb("45").unwrap();
    assert_eq!(r.editor.mesh().face_count(), 2);
    r.type_vcb("7x").unwrap();
    assert_eq!(r.editor.mesh().face_count(), 8);
}

#[test]
fn scale_by_corner_grip_and_typed_factor() {
    let mut r = Rig::new(ToolKind::Scale);
    cube(&mut r);
    let elements = all(&r);
    r.editor.set_selection(elements);
    // The grips show before any click.
    r.hover(v(0.5, 0.5, 0.5));
    assert_eq!(r.tools.preview(&r.editor).markers.len(), 26);
    r.click(v(1.0, 1.0, 1.0));
    r.hover(v(1.5, 1.5, 1.5));
    r.type_vcb("2").unwrap();
    assert!(r.editor.mesh().find_vertex(v(2.0, 2.0, 2.0)).is_some());
    // Typing again corrects it: a length sets the size along the grip's first axis.
    r.type_vcb("3m").unwrap();
    assert!(r.editor.mesh().find_vertex(v(3.0, 3.0, 3.0)).is_some());
}

#[test]
fn scale_face_grip_about_the_centre_with_ctrl() {
    let mut r = Rig::new(ToolKind::Scale);
    cube(&mut r);
    let elements = all(&r);
    r.editor.set_selection(elements);
    r.hover(v(0.5, 0.5, 0.5));
    r.mods.ctrl = true;
    r.click(v(1.0, 0.5, 0.5));
    r.type_vcb("3").unwrap();
    // Only red changes, about the middle.
    assert!(r.editor.mesh().find_vertex(v(2.0, 1.0, 1.0)).is_some());
    assert!(r.editor.mesh().find_vertex(v(-1.0, 0.0, 0.0)).is_some());
}

#[test]
fn offset_a_face_and_repeat_with_double_click() {
    let mut r = Rig::new(ToolKind::Offset);
    square(&mut r, 2.0);
    r.click(v(1.0, 1.0, 0.0));
    r.hover(v(1.0, 0.8, 0.0));
    assert!(!r.tools.preview(&r.editor).lines.is_empty());
    r.type_vcb("250").unwrap();
    assert_eq!(r.editor.mesh().face_count(), 2);
    // Double-clicking the inner face repeats.
    r.clicks(v(1.0, 1.0, 0.0), 1);
    r.ev(v(1.0, 1.0, 0.0), Pointer::Down { clicks: 2 });
    r.ev(v(1.0, 1.0, 0.0), Pointer::Up);
    assert_eq!(r.editor.mesh().face_count(), 3);
}

#[test]
fn arc_with_typed_segments_and_half_circle() {
    let mut r = Rig::new(ToolKind::Arc);
    r.type_vcb("6s").unwrap();
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(2.0, 0.0, 0.0));
    r.hover(v(1.0, 1.0, 0.0));
    assert_eq!(r.tools.preview(&r.editor).label.as_deref(), Some("Half Circle"));
    r.click(v(1.0, 1.0, 0.0));
    assert_eq!(r.editor.mesh().edge_count(), 6);
    assert!(
        r.editor.mesh().find_vertex(v(1.0, 1.0, 0.0)).is_some()
            || r.editor.mesh().vertices().all(|(_, x)| (x.pos.distance(v(1.0, 0.0, 0.0)) - 1.0).abs() < 1e-9)
    );
}

#[test]
fn tape_makes_parallel_guides_and_points() {
    let mut r = Rig::new(ToolKind::TapeMeasure);
    square(&mut r, 2.0);
    // From an edge: a parallel guide line at the typed distance.
    r.click(v(0.6, 0.0, 0.0));
    r.hover(v(0.6, 0.7, 0.0));
    r.type_vcb("300").unwrap();
    let guides: Vec<Guide> = r.editor.active().guides.values().copied().collect();
    assert_eq!(guides.len(), 1);
    match guides[0] {
        Guide::Line { origin, dir, .. } => {
            assert!((origin.y - 0.3).abs() < 1e-9);
            assert!(dir.cross(DVec3::X).length() < 1e-9);
        }
        _ => panic!("expected a line"),
    }
    // From the origin: a guide point.
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(1.0, 1.0, 0.0));
    r.type_vcb("500").unwrap();
    assert_eq!(r.editor.active().guides.len(), 2);
    // Ctrl: measure only.
    r.mods.ctrl = true;
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(2.0, 0.0, 0.0));
    assert_eq!(r.editor.active().guides.len(), 2);
    assert!(r.tools.status().starts_with("Length 2000 mm"));
}

#[test]
fn paint_and_sample() {
    let mut r = Rig::new(ToolKind::PaintBucket);
    square(&mut r, 2.0);
    let m = r
        .editor
        .apply(Command::AddMaterial { name: "Brick".into(), color: [0.7, 0.3, 0.2, 1.0], texture: None })
        .unwrap()
        .material;
    r.tools.set_paint(m);
    r.click(v(1.0, 1.0, 0.0));
    let f = r.editor.mesh().faces().next().unwrap().1.clone();
    // The ground face points down and the camera looks from above: its back is painted.
    assert_eq!(f.back_material, m);
    r.tools.set_paint(None);
    r.mods.alt = true;
    r.click(v(1.0, 1.0, 0.0));
    assert_eq!(r.tools.paint(), m);
}

#[test]
fn eraser_with_ctrl_softens() {
    let mut r = Rig::new(ToolKind::Eraser);
    cube(&mut r);
    r.mods.ctrl = true;
    r.click(v(0.5, 0.0, 1.0));
    assert_eq!(r.editor.mesh().edges().filter(|(_, e)| e.soft).count(), 1);
    assert_eq!(r.editor.mesh().face_count(), 6);
}

#[test]
fn double_click_opens_a_group_and_escape_closes_it() {
    let mut r = Rig::new(ToolKind::Select);
    cube(&mut r);
    let elements = all(&r);
    r.editor.apply(Command::MakeGroup { elements }).unwrap();
    // A single click on the group's face selects the group.
    r.click(v(0.5, 0.5, 1.0));
    assert!(matches!(r.editor.selected()[..], [Element::Instance(_)]));
    r.clicks(v(0.5, 0.5, 1.0), 2);
    assert_eq!(r.editor.context().len(), 1);
    // Inside, the faces are selectable.
    r.click(v(0.5, 0.5, 1.0));
    assert!(matches!(r.editor.selected()[..], [Element::Face(_)]));
    r.key(Key::Escape);
    assert!(r.editor.context().is_empty());
}

#[test]
fn move_hovered_group() {
    let mut r = Rig::new(ToolKind::Move);
    cube(&mut r);
    let elements = all(&r);
    r.editor.apply(Command::MakeGroup { elements }).unwrap();
    r.click(v(0.5, 0.5, 1.0));
    r.hover(v(0.5, 2.5, 1.0));
    r.type_vcb("2000").unwrap();
    let inst = r.editor.doc().root.instances.values().next().unwrap();
    assert!((inst.transform.w_axis.y - 2.0).abs() < 1e-6);
}

#[test]
fn follow_me_tool_sweeps_along_the_selection() {
    let mut r = Rig::new(ToolKind::FollowMe);
    r.editor
        .apply(Command::DrawRectangle {
            corner: v(0.0, -0.2, 0.0),
            u: v(0.0, 0.4, 0.0),
            v: v(0.0, 0.0, 0.3),
            facing: Some(v(-1.0, 0.0, 0.0)),
        })
        .unwrap();
    let path =
        r.editor.apply(Command::DrawLines { lines: vec![[v(0.0, 0.5, 0.0), v(3.0, 0.5, 0.0)]], facing: None }).unwrap();
    r.editor.set_selection(path.created);
    r.click(v(0.0, 0.0, 0.15));
    assert_eq!(r.editor.undo_label(), Some("Follow Me"));
    assert_eq!(r.editor.mesh().face_count(), 6);
}

#[test]
fn protractor_measures_without_changing_the_model() {
    let mut r = Rig::new(ToolKind::Protractor);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1.0, 0.0, 0.0));
    r.click(v(0.0, 1.0, 0.0));
    assert_eq!(r.editor.mesh().edge_count(), 0);
    assert!(r.editor.active().guides.is_empty());
    assert!(r.tools.message.as_ref().unwrap().contains("90.0"));
}

#[test]
fn protractor_typed_guide_and_plane_lock() {
    let mut r = Rig::new(ToolKind::Protractor);
    r.key(Key::Right);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(0.0, 1.0, 0.0));
    r.mods.ctrl = true;
    r.type_vcb("45°").unwrap();
    let guide = r.editor.active().guides.values().next().unwrap();
    let rysmo_model::Guide::Line { origin, dir, .. } = guide else { panic!("expected line guide") };
    assert!(origin.length() < 1e-7);
    assert!(dir.x.abs() < 1e-7);
    assert!((dir.y - 0.5f64.sqrt()).abs() < 1e-7);
    assert!((dir.z - 0.5f64.sqrt()).abs() < 1e-7);
    r.editor.undo();
    assert!(r.editor.active().guides.is_empty());
}

#[test]
fn tape_resize_requires_confirmation_and_undo_restores_model() {
    let mut r = Rig::new(ToolKind::TapeMeasure);
    square(&mut r, 1.0);
    r.mods.ctrl = true;
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1.0, 0.0, 0.0));
    let before = r.editor.doc().clone();
    assert_eq!(r.tools.vcb_value(&r.editor), "1000 mm");
    r.type_vcb("2m").unwrap();
    assert_eq!(r.tools.pending_resize_factor(), Some(2.0));
    assert_eq!(r.editor.doc(), &before);
    r.tools.confirm_resize(&mut r.editor, true).unwrap();
    assert_eq!(r.tools.vcb_value(&r.editor), "2000 mm");
    let (lo, hi) = r.editor.doc().definition_bounds(None).unwrap();
    assert!(((hi - lo).x - 2.0).abs() < 1e-7);
    r.editor.undo();
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn tape_resize_cancel_and_intervening_edits_are_safe() {
    let mut r = Rig::new(ToolKind::TapeMeasure);
    square(&mut r, 1.0);
    r.mods.ctrl = true;
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1.0, 0.0, 0.0));
    let before = r.editor.doc().clone();
    r.type_vcb("2m").unwrap();
    r.tools.confirm_resize(&mut r.editor, false).unwrap();
    assert_eq!(r.editor.doc(), &before);
    r.type_vcb("2m").unwrap();
    r.editor.apply(Command::AddMaterial { name: "New".into(), color: [1.0; 4], texture: None }).unwrap();
    assert!(r.tools.confirm_resize(&mut r.editor, true).is_err());
    assert_eq!(r.editor.doc().materials.len(), 1);
    let (lo, hi) = r.editor.doc().definition_bounds(None).unwrap();
    assert!(((hi - lo).x - 1.0).abs() < 1e-7);
}

#[test]
fn tape_measure_only_typed_value_does_not_create_a_guide() {
    let mut r = Rig::new(ToolKind::TapeMeasure);
    square(&mut r, 1.0);
    r.mods.ctrl = true;
    let before = r.editor.doc().clone();
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(1.0, 0.0, 0.0));
    r.type_vcb("2m").unwrap();
    assert!(r.editor.active().guides.is_empty());
    assert_eq!(r.editor.doc(), &before);
    r.type_vcb("4m").unwrap();
    assert_eq!(r.tools.pending_resize_factor(), Some(2.0));
}

#[test]
fn axes_tool_repositions_frame_and_undo_restores_it() {
    let mut r = Rig::new(ToolKind::Axes);
    r.click(v(2.0, 2.0, 0.0));
    r.click(v(3.0, 3.0, 0.0));
    r.click(v(1.0, 3.0, 0.0));
    let axes = r.editor.active().axes;
    assert!((axes.origin - v(2.0, 2.0, 0.0)).length() < 1e-7);
    assert!((axes.x - v(1.0, 1.0, 0.0).normalize()).length() < 1e-7);
    assert!((axes.y - v(-1.0, 1.0, 0.0).normalize()).length() < 1e-7);
    assert!(axes.z().distance(DVec3::Z) < 1e-7);
    r.editor.undo();
    assert_eq!(r.editor.active().axes, rysmo_math::DrawingAxes::default());
}

#[test]
fn rotated_axes_drive_arrow_lock_and_precise_rectangle() {
    let mut r = Rig::new(ToolKind::Line);
    let x = v(1.0, 1.0, 0.0).normalize();
    let y = v(-1.0, 1.0, 0.0).normalize();
    r.editor.apply(Command::SetAxes { origin: v(2.0, 2.0, 0.0), x, y }).unwrap();
    r.click(v(2.0, 2.0, 0.0));
    r.key(Key::Right);
    r.hover(v(3.0, 3.2, 0.0));
    r.type_vcb("1m").unwrap();
    let edge = r.editor.mesh().edges().next().unwrap().0;
    let (a, b) = r.editor.mesh().edge_points(edge);
    assert!((a - b).normalize().cross(x).length() < 1e-7);
    let mut r = Rig::new(ToolKind::Rectangle);
    r.editor.apply(Command::SetAxes { origin: v(2.0, 2.0, 0.0), x, y }).unwrap();
    r.click(v(2.0, 2.0, 0.0));
    r.hover(v(2.0, 4.0, 0.0));
    r.type_vcb("2m,1m").unwrap();
    for (id, _) in r.editor.mesh().edges() {
        let (a, b) = r.editor.mesh().edge_points(id);
        let dir = (b - a).normalize();
        assert!(dir.cross(x).length() < 1e-7 || dir.cross(y).length() < 1e-7);
    }
}

#[test]
fn scale_grips_and_typed_length_follow_rotated_drawing_axes() {
    let mut r = Rig::new(ToolKind::Scale);
    let x = v(1.0, 1.0, 0.0).normalize();
    let y = v(-1.0, 1.0, 0.0).normalize();
    r.editor.apply(Command::SetAxes { origin: DVec3::ZERO, x, y }).unwrap();
    r.editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: x * 2.0, v: y, facing: None }).unwrap();
    let faces: Vec<_> = r.editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
    r.editor.set_selection(faces);
    let handle = x * 2.0 + y * 0.5;
    r.hover(handle);
    assert!(r.tools.preview(&r.editor).markers.iter().any(|(p, _)| p.distance(handle) < 1e-7));
    r.click(handle);
    r.type_vcb("4m").unwrap();
    let along_x: Vec<_> = r.editor.mesh().vertices().map(|(_, v)| v.pos.dot(x)).collect();
    let min = along_x.iter().copied().reduce(f64::min).unwrap();
    let max = along_x.iter().copied().reduce(f64::max).unwrap();
    assert!((max - min - 4.0).abs() < 1e-7);
    let along_y: Vec<_> = r.editor.mesh().vertices().map(|(_, v)| v.pos.dot(y)).collect();
    assert!(
        (along_y.iter().copied().reduce(f64::max).unwrap() - along_y.iter().copied().reduce(f64::min).unwrap() - 1.0)
            .abs()
            < 1e-7
    );
    r.type_vcb("3m").unwrap();
    let values: Vec<_> = r.editor.mesh().vertices().map(|(_, v)| v.pos.dot(x)).collect();
    assert!(
        (values.iter().copied().reduce(f64::max).unwrap() - values.iter().copied().reduce(f64::min).unwrap() - 3.0)
            .abs()
            < 1e-7
    );
    r.editor.undo();
    let values: Vec<_> = r.editor.mesh().vertices().map(|(_, v)| v.pos.dot(x)).collect();
    assert!(
        (values.iter().copied().reduce(f64::max).unwrap() - values.iter().copied().reduce(f64::min).unwrap() - 2.0)
            .abs()
            < 1e-7
    );
}

#[test]
fn scale_about_centre_length_uses_full_extent_and_rejects_stale_correction() {
    let mut r = Rig::new(ToolKind::Scale);
    square(&mut r, 1.0);
    let faces: Vec<_> = r.editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
    r.editor.set_selection(faces);
    r.mods.ctrl = true;
    r.click(v(1.0, 0.5, 0.0));
    assert!(r.type_vcb("0").is_err());
    assert!(r.tools.busy());
    r.type_vcb("2m").unwrap();
    let (lo, hi) = r.editor.doc().definition_bounds(None).unwrap();
    assert!((hi.x - lo.x - 2.0).abs() < 1e-7);
    assert!(((lo.x + hi.x) * 0.5 - 0.5).abs() < 1e-7);
    r.editor.apply(Command::AddMaterial { name: "Keep".into(), color: [1.0; 4], texture: None }).unwrap();
    assert!(r.type_vcb("3m").is_err());
    assert_eq!(r.editor.doc().materials.len(), 1);
}

#[test]
fn scale_shift_toggles_uniform_mode_and_ctrl_can_change_mid_operation() {
    let mut r = Rig::new(ToolKind::Scale);
    square(&mut r, 1.0);
    let faces: Vec<_> = r.editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
    r.editor.set_selection(faces);
    r.click(v(1.0, 0.5, 0.0));
    r.mods.shift = true;
    r.key(Key::Shift(true));
    r.mods.ctrl = true;
    r.type_vcb("2").unwrap();
    let (lo, hi) = r.editor.doc().definition_bounds(None).unwrap();
    assert!((hi.x - lo.x - 2.0).abs() < 1e-7);
    assert!((hi.y - lo.y - 2.0).abs() < 1e-7);
    assert!(((hi + lo) * 0.5 - v(0.5, 0.5, 0.0)).length() < 1e-7);
}

#[test]
fn scale_shift_corner_drag_allows_independent_planar_factors() {
    let mut r = Rig::new(ToolKind::Scale);
    square(&mut r, 1.0);
    let faces: Vec<_> = r.editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
    r.editor.set_selection(faces);
    r.mods.shift = true;
    r.click(v(1.0, 1.0, 0.0));
    r.hover(v(2.0, 3.0, 0.0));
    assert!(r.tools.preview(&r.editor).markers.iter().any(|(p, _)| p.distance(v(2.0, 3.0, 0.0)) < 1e-7));
    r.click(v(2.0, 3.0, 0.0));
    let (lo, hi) = r.editor.doc().definition_bounds(None).unwrap();
    assert!((hi.x - lo.x - 2.0).abs() < 1e-7);
    assert!((hi.y - lo.y - 3.0).abs() < 1e-7);
}

#[test]
fn offset_hole_preview_correction_undo_and_invalid_input() {
    let mut r = Rig::new(ToolKind::Offset);
    r.rectangle(v(0., 0., 0.), v(4., 4., 0.));
    r.rectangle(v(1.5, 1.5, 0.), v(2.5, 2.5, 0.));
    let before = r.editor.doc().clone();
    r.click(v(0.8, 0.8, 0.));
    r.hover(v(1.25, 2., 0.));
    let preview = r.tools.preview(&r.editor);
    assert_eq!(preview.lines.len(), 8);
    r.type_vcb("250").unwrap();
    let first = r.editor.doc().clone();
    assert!(r.type_vcb("800").is_err());
    assert_eq!(r.editor.doc(), &first);
    r.type_vcb("300").unwrap();
    let after = r.editor.doc().clone();
    assert_ne!(after, first);
    r.editor.undo();
    assert_eq!(r.editor.doc(), &before);
    r.editor.redo();
    assert_eq!(r.editor.doc(), &after);
    r.editor.apply(rysmo_model::Command::AddTag { name: "Later".into() }).unwrap();
    let changed = r.editor.doc().clone();
    assert!(r.type_vcb("400").is_err());
    assert_eq!(r.editor.doc(), &changed);
}

fn follow_me_interactive_rig() -> Rig {
    let mut r = Rig::new(ToolKind::FollowMe);
    r.editor
        .apply(Command::DrawRectangle {
            corner: v(0., -0.2, 0.),
            u: v(0., 0.4, 0.),
            v: v(0., 0., 0.3),
            facing: Some(v(-1., 0., 0.)),
        })
        .unwrap();
    r.editor
        .apply(Command::DrawLines {
            lines: vec![[v(0., 0.5, 0.), v(3., 0.5, 0.)], [v(3., 0.5, 0.), v(3., 3., 0.)]],
            facing: None,
        })
        .unwrap();
    r
}
#[test]
fn follow_me_traces_connected_edges_preview_commit_and_one_undo() {
    let mut r = follow_me_interactive_rig();
    let before = r.editor.doc().clone();
    r.click(v(0., 0., 0.15));
    assert!(r.tools.busy());
    r.hover(v(1.5, 0.5, 0.));
    assert!(!r.tools.preview(&r.editor).lines.is_empty());
    r.hover(v(3., 1.5, 0.));
    assert_eq!(r.editor.doc(), &before);
    let preview = r.tools.preview(&r.editor);
    assert!(preview.lines.iter().filter(|l| l.color == [0.9, 0.1, 0.1]).count() >= 2);
    let mut expected = r.editor.clone();
    let crate::tools::State::Following { face, path, .. } = r.tools.state.clone() else {
        panic!("missing Follow Me draft")
    };
    expected.apply(Command::FollowMe { face, path }).unwrap();
    r.click(v(3., 1.5, 0.));
    assert_eq!(r.editor.doc(), expected.doc());
    assert!(!r.tools.busy());
    assert_eq!(r.editor.undo_label(), Some("Follow Me"));
    r.editor.mesh().validate().unwrap();
    let after = r.editor.doc().clone();
    r.editor.undo();
    assert_eq!(r.editor.doc(), &before);
    r.editor.redo();
    assert_eq!(r.editor.doc(), &after);
}
#[test]
fn follow_me_drag_release_escape_and_stale_draft_are_safe() {
    let mut r = follow_me_interactive_rig();
    let before = r.editor.doc().clone();
    r.ev(v(0., 0., 0.15), Pointer::Down { clicks: 1 });
    r.hover(v(1.5, 0.5, 0.));
    r.hover(v(3., 1.5, 0.));
    r.ev(v(3., 1.5, 0.), Pointer::Up);
    assert_eq!(r.editor.undo_label(), Some("Follow Me"));
    r.editor.undo();
    assert_eq!(r.editor.doc(), &before);
    r.tools.set_tool(ToolKind::FollowMe);
    r.click(v(0., 0., 0.15));
    r.hover(v(1.5, 0.5, 0.));
    r.key(Key::Escape);
    assert_eq!(r.editor.doc(), &before);
    assert!(!r.tools.busy());
    r.click(v(0., 0., 0.15));
    r.hover(v(1.5, 0.5, 0.));
    r.editor.apply(Command::AddTag { name: "Changed".into() }).unwrap();
    let changed = r.editor.doc().clone();
    r.click(v(3., 1.5, 0.));
    assert_eq!(r.editor.doc(), &changed);
    assert!(!r.tools.busy());
}
#[test]
fn follow_me_can_preselect_a_path_face() {
    let mut r = Rig::new(ToolKind::FollowMe);
    r.editor
        .apply(Command::DrawRectangle { corner: v(0., 0., 0.), u: v(3., 0., 0.), v: v(0., 3., 0.), facing: None })
        .unwrap();
    let path = r.editor.mesh().faces().next().unwrap().0;
    r.editor
        .apply(Command::DrawRectangle {
            corner: v(0., -0.2, 0.),
            u: v(0., 0.4, 0.),
            v: v(0., 0., 0.3),
            facing: Some(v(-1., 0., 0.)),
        })
        .unwrap();
    r.editor.set_selection([Element::Face(path)]);
    r.click(v(0., 0., 0.15));
    assert_eq!(r.editor.undo_label(), Some("Follow Me"));
    r.editor.mesh().validate().unwrap();
}

#[test]
fn follow_me_alt_traces_the_surface_boundary_without_preselection() {
    let mut r = Rig::new(ToolKind::FollowMe);
    r.editor
        .apply(Command::DrawRectangle { corner: v(0., 0., 0.), u: v(3., 0., 0.), v: v(0., 3., 0.), facing: None })
        .unwrap();
    r.editor
        .apply(Command::DrawRectangle {
            corner: v(0., -0.2, 0.),
            u: v(0., 0.4, 0.),
            v: v(0., 0., 0.3),
            facing: Some(v(-1., 0., 0.)),
        })
        .unwrap();
    let before = r.editor.doc().clone();
    r.mods.alt = true;
    r.click(v(0., 0., 0.15));
    r.hover(v(2., 2., 0.));
    assert_eq!(r.editor.doc(), &before);
    let preview = r.tools.preview(&r.editor);
    assert!(preview.lines.iter().filter(|l| l.color == [0.9, 0.1, 0.1]).count() >= 4);
    r.click(v(2., 2., 0.));
    assert_eq!(r.editor.undo_label(), Some("Follow Me"));
    assert!(!r.tools.busy());
    r.editor.mesh().validate().unwrap();
    r.editor.undo();
    assert_eq!(r.editor.doc(), &before);
}

fn completed_move(copy: bool) -> Rig {
    let mut r = Rig::new(ToolKind::Move);
    square(&mut r, 1.);
    r.editor.set_selection(all(&r));
    r.mods.ctrl = copy;
    r.click(DVec3::ZERO);
    r.hover(DVec3::X * 2.);
    r.type_vcb("2000").unwrap();
    r
}
#[test]
fn exact_corrections_refuse_intervening_edits_and_undo_redo() {
    for copy in [false, true] {
        let mut r = completed_move(copy);
        r.editor.apply(Command::AddTag { name: "Later edit".into() }).unwrap();
        let before = r.editor.doc().clone();
        let depth = r.editor.undo_depth();
        let revision = r.editor.revision();
        assert!(r.type_vcb(if copy { "3x" } else { "3000" }).is_err());
        assert_eq!(r.editor.doc(), &before);
        assert_eq!(r.editor.undo_depth(), depth);
        assert_eq!(r.editor.revision(), revision);
        r.editor.undo();
        r.editor.redo();
        assert!(r.type_vcb("4000").is_err());
        assert_eq!(r.editor.doc(), &before);
    }
}
#[test]
fn zero_move_correction_cannot_replace_an_earlier_history_step() {
    let mut r = completed_move(false);
    r.type_vcb("0").unwrap();
    let before = r.editor.doc().clone();
    assert_eq!(r.editor.undo_label(), Some("Rectangle"));
    assert!(r.type_vcb("1000").is_err());
    assert_eq!(r.editor.doc(), &before);
    assert_eq!(r.editor.undo_label(), Some("Rectangle"));
}
#[test]
fn invalid_array_correction_preserves_result_and_allows_valid_correction() {
    let mut r = completed_move(true);
    let before = r.editor.doc().clone();
    let revision = r.editor.revision();
    assert!(r.type_vcb("0x").is_err());
    assert_eq!(r.editor.doc(), &before);
    assert_eq!(r.editor.revision(), revision);
    r.type_vcb("2x").unwrap();
    assert_eq!(r.editor.mesh().face_count(), 3);
    r.editor.undo();
    assert_eq!(r.editor.mesh().face_count(), 1);
}

#[test]
fn rotate_correction_is_atomic_and_rejects_later_edits() {
    let mut r = Rig::new(ToolKind::Rotate);
    square(&mut r, 1.);
    r.editor.set_selection(all(&r));
    r.click(DVec3::ZERO);
    r.click(DVec3::X * 2.);
    r.hover(DVec3::Y * 2.);
    r.type_vcb("90").unwrap();
    let before = r.editor.doc().clone();
    let revision = r.editor.revision();
    let depth = r.editor.undo_depth();
    assert!(r.type_vcb("NaN").is_err());
    assert_eq!(r.editor.doc(), &before);
    assert_eq!(r.editor.revision(), revision);
    assert_eq!(r.editor.undo_depth(), depth);
    r.type_vcb("45").unwrap();
    assert_eq!(r.editor.undo_depth(), depth);
    r.editor.apply(Command::AddTag { name: "Unrelated".into() }).unwrap();
    let before = r.editor.doc().clone();
    assert!(r.type_vcb("60").is_err());
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn entering_a_group_ends_its_parent_move_correction() {
    let mut r = Rig::new(ToolKind::Move);
    square(&mut r, 1.);
    let out = r.editor.apply(Command::MakeGroup { elements: all(&r) }).unwrap();
    let Element::Instance(group) = out.created[0] else { panic!() };
    r.editor.set_selection([Element::Instance(group)]);
    r.click(DVec3::ZERO);
    r.hover(DVec3::X * 2.);
    r.type_vcb("2000").unwrap();
    r.editor.enter(group).unwrap();
    let before = r.editor.doc().clone();
    let depth = r.editor.undo_depth();
    assert!(r.type_vcb("3000").is_err());
    assert_eq!(r.editor.doc(), &before);
    assert_eq!(r.editor.context(), &[group]);
    assert_eq!(r.editor.undo_depth(), depth);
}

fn paint_regions() -> (Rig, Vec<rysmo_mesh::FaceId>, rysmo_mesh::MaterialId, rysmo_mesh::MaterialId) {
    let mut r = Rig::new(ToolKind::PaintBucket);
    let mut faces = Vec::new();
    for x in [0., 1., 2., 5.] {
        r.editor
            .apply(Command::DrawRectangle { corner: v(x, 0., 1.), u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
            .unwrap();
    }
    for x in [0., 1., 2., 5.] {
        let face =
            r.editor.mesh().faces().find(|(f, _)| r.editor.mesh().face_contains(*f, v(x + 0.5, 0.5, 1.))).unwrap().0;
        assert!(r.editor.mesh().face_normal(face).unwrap().z > 0.);
        faces.push(face);
    }
    let barrier = r
        .editor
        .apply(Command::AddMaterial { name: "Barrier".into(), color: [1., 0., 0., 1.], texture: None })
        .unwrap()
        .material
        .unwrap();
    let paint = r
        .editor
        .apply(Command::AddMaterial { name: "New paint".into(), color: [0., 0., 1., 1.], texture: None })
        .unwrap()
        .material
        .unwrap();
    r.editor.apply(Command::Paint { faces: vec![faces[1]], material: Some(barrier), back: false }).unwrap();
    r.tools.set_paint(Some(paint));
    (r, faces, barrier, paint)
}
#[test]
fn paint_modifiers_distinguish_adjacent_fill_connected_replace_and_global_replace() {
    for (ctrl, shift, expected) in [(true, false, vec![0]), (true, true, vec![0, 2]), (false, true, vec![0, 2, 3])] {
        let (mut r, faces, barrier, paint) = paint_regions();
        let before = r.editor.doc().clone();
        r.mods.ctrl = ctrl;
        r.mods.shift = shift;
        r.click(v(0.5, 0.5, 1.));
        for (i, face) in faces.iter().enumerate() {
            assert_eq!(
                r.editor.mesh().face(*face).unwrap().material,
                if i == 1 {
                    Some(barrier)
                } else if expected.contains(&i) {
                    Some(paint)
                } else {
                    None
                }
            );
            assert_eq!(r.editor.mesh().face(*face).unwrap().back_material, None);
        }
        r.editor.undo();
        assert_eq!(r.editor.doc(), &before);
    }
}
#[test]
fn painting_a_selected_face_uses_only_selected_entities_for_every_modifier() {
    for (ctrl, shift) in [(false, false), (true, false), (false, true), (true, true)] {
        let (mut r, faces, barrier, paint) = paint_regions();
        r.editor.set_selection([Element::Face(faces[0]), Element::Face(faces[3])]);
        r.mods.ctrl = ctrl;
        r.mods.shift = shift;
        r.click(v(0.5, 0.5, 1.));
        for (i, face) in faces.iter().enumerate() {
            assert_eq!(
                r.editor.mesh().face(*face).unwrap().material,
                match i {
                    0 | 3 => Some(paint),
                    1 => Some(barrier),
                    _ => None,
                }
            );
        }
    }
}
#[test]
fn mixed_face_and_group_painting_is_one_atomic_history_step() {
    let (mut r, faces, _, paint) = paint_regions();
    let out = r
        .editor
        .apply(Command::DrawRectangle { corner: v(8., 0., 1.), u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
        .unwrap();
    let out = r.editor.apply(Command::MakeGroup { elements: out.created }).unwrap();
    let Element::Instance(group) = out.created[0] else { panic!() };
    r.editor.set_selection([Element::Face(faces[0]), Element::Instance(group)]);
    let before = r.editor.doc().clone();
    let depth = r.editor.undo_depth();
    r.click(v(0.5, 0.5, 1.));
    assert_eq!(r.editor.active().instances[&group].material, Some(paint));
    assert_eq!(r.editor.mesh().face(faces[0]).unwrap().material, Some(paint));
    assert_eq!(r.editor.undo_depth(), depth + 1);
    r.editor.undo();
    assert_eq!(r.editor.doc(), &before);
    let revision = r.editor.revision();
    assert!(
        r.editor
            .apply(Command::PaintElements {
                elements: vec![Element::Face(faces[0]), Element::Instance(rysmo_mesh::InstanceId(u64::MAX))],
                material: Some(paint),
                back: false
            })
            .is_err()
    );
    assert_eq!(r.editor.doc(), &before);
    assert_eq!(r.editor.revision(), revision);
}

#[test]
fn selected_face_painting_uses_the_clicked_side_for_the_entire_selection() {
    let (mut r, faces, _, paint) = paint_regions();
    r.view =
        rysmo_math::OrbitCamera { target: v(1., 0.5, 1.), pitch: -0.6, yaw: -1., distance: 10., ..Default::default() }
            .view(r.view.size);
    r.editor.set_selection([Element::Face(faces[0]), Element::Face(faces[3])]);
    r.click(v(0.5, 0.5, 1.));
    for index in [0, 3] {
        let face = r.editor.mesh().face(faces[index]).unwrap();
        assert_eq!(face.material, None);
        assert_eq!(face.back_material, Some(paint));
    }
    assert_eq!(r.editor.mesh().face(faces[2]).unwrap().back_material, None);
}

#[test]
fn connected_material_replacement_includes_faces_joined_by_wire_edges() {
    let (mut r, faces, barrier, paint) = paint_regions();
    r.editor.apply(Command::DrawLines { lines: vec![[v(3., 0., 1.), v(5., 0., 1.)]], facing: None }).unwrap();
    r.mods.ctrl = true;
    r.mods.shift = true;
    r.click(v(0.5, 0.5, 1.));
    for (i, face) in faces.iter().enumerate() {
        assert_eq!(r.editor.mesh().face(*face).unwrap().material, Some(if i == 1 { barrier } else { paint }));
    }
}

#[test]
fn paint_and_sample_an_independent_edge_then_preserve_it_through_copy_group_and_split() {
    let mut r = Rig::new(ToolKind::PaintBucket);
    r.editor.apply(Command::DrawLines { lines: vec![[v(0., 0., 1.), v(2., 0., 1.)]], facing: None }).unwrap();
    let edge = r.editor.mesh().edges().next().unwrap().0;
    let material = r
        .editor
        .apply(Command::AddMaterial { name: "Orange edge".into(), color: [1., 0.4, 0., 1.], texture: None })
        .unwrap()
        .material;
    r.tools.set_paint(material);
    let before = r.editor.doc().clone();
    r.click(v(1., 0., 1.));
    assert_eq!(r.editor.mesh().edge(edge).unwrap().material, material);
    r.tools.set_paint(None);
    r.mods.alt = true;
    r.click(v(1., 0., 1.));
    assert_eq!(r.tools.paint(), material);
    r.editor.undo();
    assert_eq!(r.editor.doc(), &before);
    r.editor.redo();
    let out = r
        .editor
        .apply(Command::Copy { elements: vec![Element::Edge(edge)], offset: DVec3::Y * 2., copies: 1 })
        .unwrap();
    let out = r.editor.apply(Command::MakeGroup { elements: out.created }).unwrap();
    let Element::Instance(group) = out.created[0] else { panic!() };
    let definition = r.editor.active().instances[&group].definition;
    assert!(r.editor.doc().definitions[&definition].mesh.edges().all(|(_, e)| e.material == material));
    r.editor.apply(Command::Explode { instance: group }).unwrap();
    r.editor.apply(Command::DrawLines { lines: vec![[v(1., -1., 1.), v(1., 1., 1.)]], facing: None }).unwrap();
    let parts: Vec<_> = r
        .editor
        .mesh()
        .edges()
        .filter(|(id, _)| {
            let (a, b) = r.editor.mesh().edge_points(*id);
            a.y.abs() < 1e-8 && b.y.abs() < 1e-8
        })
        .collect();
    assert_eq!(parts.len(), 2);
    assert!(parts.iter().all(|(_, edge)| edge.material == material));
}
#[test]
fn front_selection_paints_edges_but_back_selection_preserves_them() {
    let (mut r, faces, barrier, paint) = paint_regions();
    let edges = r.editor.mesh().face_edges(faces[0]);
    let selected: Vec<_> =
        std::iter::once(Element::Face(faces[0])).chain(edges.iter().copied().map(Element::Edge)).collect();
    r.editor.set_selection(selected.clone());
    r.click(v(0.5, 0.5, 1.));
    assert!(edges.iter().all(|id| r.editor.mesh().edge(*id).unwrap().material == Some(paint)));
    r.tools.set_paint(Some(barrier));
    r.view =
        rysmo_math::OrbitCamera { target: v(1., 0.5, 1.), pitch: -0.6, yaw: -1., distance: 10., ..Default::default() }
            .view(r.view.size);
    r.editor.set_selection(selected);
    r.click(v(0.5, 0.5, 1.));
    assert_eq!(r.editor.mesh().face(faces[0]).unwrap().back_material, Some(barrier));
    assert!(edges.iter().all(|id| r.editor.mesh().edge(*id).unwrap().material == Some(paint)));
}

#[test]
fn sampled_positioned_texture_transfers_in_one_undo_and_library_choice_clears_it() {
    let (mut r, faces, _, material) = paint_regions();
    let image = r
        .editor
        .apply(Command::AddImage { name: "tile.png".into(), mime: "image/png".into(), data: vec![1, 2, 3] })
        .unwrap()
        .image
        .unwrap();
    r.editor
        .apply(Command::SetMaterialTexture { material, texture: Some(rysmo_model::Texture { image, size: [1., 1.] }) })
        .unwrap();
    let placement = rysmo_model::TexturePlacement {
        offset: [0.2, 0.3],
        pins: Some([[0., 0.], [1., 0.], [1.2, 1.], [0., 1.]]),
        projected_normal: Some([0., 0., 1.]),
        ..Default::default()
    };
    r.editor.apply(Command::Paint { faces: vec![faces[0]], material: Some(material), back: false }).unwrap();
    r.editor.apply(Command::SetTexturePlacement { faces: vec![faces[0]], placement: Some(placement) }).unwrap();
    r.mods.alt = true;
    r.click(v(0.5, 0.5, 1.));
    r.mods.alt = false;
    let depth = r.editor.undo_depth();
    r.click(v(2.5, 0.5, 1.));
    assert_eq!(r.editor.mesh().face(faces[2]).unwrap().uv, Some(placement));
    assert_eq!(r.editor.mesh().face(faces[2]).unwrap().material, Some(material));
    assert_eq!(r.editor.undo_depth(), depth + 1);
    r.editor.undo();
    assert_eq!(r.editor.mesh().face(faces[2]).unwrap().uv, None);
    assert_eq!(r.editor.mesh().face(faces[2]).unwrap().material, None);
    r.tools.set_paint(Some(material));
    r.click(v(2.5, 0.5, 1.));
    assert_eq!(r.editor.mesh().face(faces[2]).unwrap().uv, None);
}

#[test]
fn projected_paint_sample_transfers_across_rotated_scaled_editing_contexts() {
    let (mut r, faces, _, material) = paint_regions();
    let image = r
        .editor
        .apply(Command::AddImage { name: "tile.png".into(), mime: "image/png".into(), data: vec![1] })
        .unwrap()
        .image
        .unwrap();
    r.editor
        .apply(Command::SetMaterialTexture { material, texture: Some(rysmo_model::Texture { image, size: [2., 1.] }) })
        .unwrap();
    let placement = rysmo_model::TexturePlacement {
        offset: [0.2, 0.3],
        rotation: 15.,
        projected_normal: Some([0., 0., 1.]),
        ..Default::default()
    };
    r.editor.apply(Command::Paint { faces: vec![faces[0]], material: Some(material), back: false }).unwrap();
    r.editor.apply(Command::SetTexturePlacement { faces: vec![faces[0]], placement: Some(placement) }).unwrap();
    r.mods.alt = true;
    r.click(v(0.5, 0.5, 1.));
    r.mods.alt = false;
    let geometry = r
        .editor
        .apply(Command::DrawRectangle { corner: v(7., 0., 1.), u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
        .unwrap();
    let group = r.editor.apply(Command::MakeGroup { elements: geometry.created }).unwrap().created[0];
    let Element::Instance(id) = group else { panic!() };
    r.editor
        .apply(Command::Rotate { elements: vec![group], centre: v(7., 0., 1.), axis: DVec3::Z, angle: 30., copies: 0 })
        .unwrap();
    r.editor.apply(Command::Scale { elements: vec![group], origin: v(7., 0., 1.), factors: v(1.5, 0.7, 1.) }).unwrap();
    r.editor.enter(id).unwrap();
    let transform = r.editor.context_transform();
    r.view = r.view.local(transform);
    let target = r.editor.mesh().faces().next().unwrap().0;
    let depth = r.editor.undo_depth();
    r.click(v(0.5, 0.5, 0.));
    let after = r.editor.mesh().face(target).unwrap().uv.unwrap();
    let actual = rysmo_model::uv_at(v(0.5, 0.5, 0.), DVec3::Z, [2., 1.], &after);
    let expected = rysmo_model::uv_at(transform.transform_point3(v(0.5, 0.5, 0.)), DVec3::Z, [2., 1.], &placement);
    assert!((actual[0] - expected[0]).abs() < 1e-8 && (actual[1] - expected[1]).abs() < 1e-8);
    assert_eq!(r.editor.undo_depth(), depth + 1);
    r.editor.undo();
    assert_eq!(r.editor.mesh().face(target).unwrap().uv, None);
}

#[test]
fn ordinary_textured_paint_wraps_a_soft_cylinder_and_surface_setting_allows_one_face() {
    for individual in [false, true] {
        let mut r = Rig::new(ToolKind::PaintBucket);
        r.editor
            .apply(Command::DrawCircle {
                centre: v(1., 1., 0.),
                normal: DVec3::Z,
                radius: 1.,
                segments: 24,
                start: None,
            })
            .unwrap();
        let disc = r.editor.mesh().faces().next().unwrap().0;
        r.editor.apply(Command::PushPull { face: disc, distance: -1., keep_base: false }).unwrap();
        let faces = r
            .editor
            .mesh()
            .faces()
            .filter(|(f, _)| r.editor.mesh().face_normal(*f).unwrap().z.abs() < 0.1)
            .map(|(f, _)| f)
            .collect::<Vec<_>>();
        let image = r
            .editor
            .apply(Command::AddImage { name: "Tile.png".into(), mime: "image/png".into(), data: vec![1] })
            .unwrap()
            .image
            .unwrap();
        let material = r
            .editor
            .apply(Command::AddMaterial {
                name: "Tile".into(),
                color: [1.; 4],
                texture: Some(rysmo_model::Texture { image, size: [4., 1.] }),
            })
            .unwrap()
            .material
            .unwrap();
        r.tools.set_paint_surface(!individual);
        r.tools.set_paint(Some(material));
        let face = *faces
            .iter()
            .max_by(|a, b| {
                let score = |f| {
                    let face = r.editor.mesh().face(f).unwrap();
                    let p = r.editor.mesh().loop_points(&face.outer).iter().copied().sum::<DVec3>()
                        / face.outer.len() as f64;
                    r.editor.mesh().face_normal(f).unwrap().dot((r.view.eye - p).normalize())
                };
                score(**a).total_cmp(&score(**b))
            })
            .unwrap();
        let data = r.editor.mesh().face(face).unwrap();
        let p = r.editor.mesh().loop_points(&data.outer).iter().copied().sum::<DVec3>() / data.outer.len() as f64;
        let depth = r.editor.undo_depth();
        r.click(p);
        let painted = faces.iter().filter(|f| r.editor.mesh().face(**f).unwrap().material == Some(material)).count();
        assert_eq!(painted, if individual { 1 } else { 24 });
        assert_eq!(r.editor.undo_depth(), depth + 1);
        r.editor.undo();
        assert!(faces.iter().all(|f| r.editor.mesh().face(*f).unwrap().material.is_none()));
    }
}

#[test]
fn display_precision_and_imperial_input_do_not_round_a_drawn_line() {
    let mut r = Rig::new(ToolKind::Line);
    r.editor.apply(Command::SetUnits { units: rysmo_model::Units::Feet }).unwrap();
    r.editor.apply(Command::SetLengthPrecision { precision: Some(0) }).unwrap();
    r.click(v(0.5, 0.5, 0.));
    r.hover(v(1.7, 0.5, 0.));
    assert_eq!(r.tools.vcb_value(&r.editor), "~ 4 ft");
    r.type_vcb("5' 3 1/2\"").unwrap();
    let (a, b) = r.editor.mesh().edge_points(r.editor.mesh().edges().next().unwrap().0);
    assert!((a.distance(b) - 1.6129).abs() < 1e-9);
    r.editor.undo();
    assert_eq!(r.editor.mesh().edge_count(), 0);
}

#[test]
fn length_snapping_never_overrides_exact_input_or_endpoint_inference() {
    let mut r = Rig::new(ToolKind::Line);
    let settings = rysmo_model::MeasurementSettings { length_snap: Some(0.5), ..Default::default() };
    r.editor.apply(Command::SetMeasurementSettings { settings }).unwrap();
    r.click(v(0.5, 0.5, 0.));
    r.hover(v(1.72, 0.5, 0.));
    assert_eq!(r.tools.vcb_value(&r.editor), "1000 mm");
    r.type_vcb("1.23m").unwrap();
    let (a, b) = r.editor.mesh().edge_points(r.editor.mesh().edges().next().unwrap().0);
    assert!((a.distance(b) - 1.23).abs() < 1e-9);
    r.key(Key::Escape);
    let target = v(2.11, 1.17, 0.);
    r.editor.apply(Command::DrawLines { lines: vec![[target, v(2.8, 1.5, 0.)]], facing: None }).unwrap();
    r.click(v(0.5, 0.5, 0.));
    r.hover(target);
    let inference = r.tools.preview(&r.editor).inference.unwrap();
    assert_eq!(inference.kind, rysmo_infer::Kind::Endpoint);
    assert!(inference.point.distance(target) < 1e-8);
}

#[test]
fn angle_settings_apply_to_rotate_preview_but_exact_angles_bypass_snapping() {
    let mut r = Rig::new(ToolKind::Rotate);
    square(&mut r, 1.);
    let face = r.editor.mesh().faces().next().unwrap().0;
    let mut settings =
        rysmo_model::MeasurementSettings { angle_snap: Some(20.), angle_precision: 3, ..Default::default() };
    r.editor.apply(Command::SetMeasurementSettings { settings }).unwrap();
    r.editor.set_selection([Element::Face(face)]);
    r.click(DVec3::ZERO);
    r.click(DVec3::X * 4.);
    let angle = 41_f64.to_radians();
    r.hover(v(angle.cos() * 4., angle.sin() * 4., 0.));
    assert_eq!(r.tools.vcb_value(&r.editor), "40.000°");
    settings.angle_snap = None;
    r.editor.apply(Command::SetMeasurementSettings { settings }).unwrap();
    r.hover(v(angle.cos() * 4., angle.sin() * 4., 0.));
    assert_eq!(r.tools.vcb_value(&r.editor), "41.000°");
    r.type_vcb("42.123").unwrap();
    let expected = rysmo_math::DQuat::from_axis_angle(DVec3::Z, 42.123_f64.to_radians()) * DVec3::X;
    assert!(r.editor.mesh().find_vertex(expected).is_some());
}

#[test]
fn escape_cancels_drawing_and_construction_drafts_without_history() {
    for tool in [
        ToolKind::Line,
        ToolKind::Rectangle,
        ToolKind::Circle,
        ToolKind::Polygon,
        ToolKind::Arc,
        ToolKind::TapeMeasure,
        ToolKind::Protractor,
        ToolKind::Axes,
        ToolKind::Dimension,
    ] {
        let mut r = Rig::new(tool);
        let before = r.editor.doc().clone();
        r.click(v(0.5, 0.5, 0.));
        r.hover(v(1.7, 0.9, 0.));
        r.key(Key::Escape);
        assert_eq!(r.editor.doc(), &before, "{}", tool.name());
        assert_eq!(r.editor.undo_depth(), 0, "{}", tool.name());
    }
}

#[test]
fn moving_a_glued_component_stays_in_the_host_plane() {
    let mut r = Rig::new(ToolKind::Move);
    square(&mut r, 1.);
    let made = r.editor.apply(Command::MakeComponent { elements: all(&r), name: Some("Panel".into()) }).unwrap();
    let Element::Instance(original) = made.created[0] else { panic!() };
    let definition = r.editor.active().instances[&original].definition;
    let mut settings = r.editor.doc().definitions[&definition].component.clone();
    settings.glue = rysmo_model::components::GlueMode::Any;
    r.editor.apply(Command::SetComponentProperties { definition, name: "Panel".into(), settings }).unwrap();
    r.editor
        .apply(Command::DrawRectangle { corner: v(2., 2., 0.), u: DVec3::X * 6., v: DVec3::Z * 4., facing: None })
        .unwrap();
    let face = r.editor.mesh().faces().find(|(id, _)| r.editor.mesh().face_area(*id) > 10.).unwrap().0;
    let made = r
        .editor
        .apply(Command::PlaceGluedInstance { definition, face, origin: v(4., 2., 1.), rotation: 0., scale: 1. })
        .unwrap();
    let Element::Instance(instance) = made.created[0] else { panic!() };
    r.editor.set_selection([Element::Instance(instance)]);
    r.click(v(4., 2., 1.));
    r.hover(v(5., 6., 1.3));
    assert!((r.tools.preview(&r.editor).inference.unwrap().point.y - 2.).abs() < 1e-8);
    r.click(v(5., 6., 1.3));
    assert_eq!(r.editor.active().instances[&instance].glued_to, Some(face));
    assert!((r.editor.active().instances[&instance].transform.transform_point3(DVec3::ZERO).y - 2.).abs() < 1e-8);
}

#[test]
fn select_joins_soft_facets_until_hidden_geometry_is_enabled() {
    let mut r = Rig::new(ToolKind::Select);
    r.editor
        .apply(Command::DrawCircle { centre: DVec3::ZERO, normal: DVec3::Z, radius: 1., segments: 12, start: None })
        .unwrap();
    let cap = r.editor.mesh().faces().next().unwrap().0;
    r.editor.apply(Command::PushPull { face: cap, distance: 1., keep_base: false }).unwrap();
    let side =
        r.editor.mesh().faces().find(|(id, _)| r.editor.mesh().face_normal(*id).unwrap().z.abs() < 0.5).unwrap().0;
    let ctx = Ctx { view: &r.view, cursor: r.px(v(1., 0., 0.5)), modifiers: r.mods, scale: 1. };
    r.tools.pointer_with_pick(
        &mut r.editor,
        &ctx,
        Pointer::Down { clicks: 1 },
        rysmo_infer::Pick::Element(Element::Face(side)),
    );
    assert_eq!(r.editor.selected().iter().filter(|e| matches!(e, Element::Face(_))).count(), 12);
    let mut style = r.editor.doc().style;
    style.show_hidden_geometry = true;
    r.editor.apply(Command::SetStyle { style }).unwrap();
    r.editor.clear_selection();
    r.tools.set_tool(ToolKind::Select);
    r.tools.pointer_with_pick(
        &mut r.editor,
        &ctx,
        Pointer::Down { clicks: 1 },
        rysmo_infer::Pick::Element(Element::Face(side)),
    );
    assert_eq!(r.editor.selected(), vec![Element::Face(side)]);
}

#[test]
fn tape_corrects_parallel_guide_without_resizing_geometry_and_single_undo() {
    let mut r = Rig::new(ToolKind::TapeMeasure);
    square(&mut r, 2.);
    let before = r.editor.doc().clone();
    r.click(v(0.6, 0., 0.));
    r.click(v(0.6, 0.7, 0.));
    r.type_vcb("300").unwrap();
    assert_eq!(r.tools.pending_resize_factor(), None);
    assert_eq!(r.editor.mesh(), &before.root.mesh);
    let guide = r.editor.active().guides.values().next().unwrap();
    assert!(matches!(guide, Guide::Line { origin, .. } if (origin.y - 0.3).abs() < 1e-9));
    r.type_vcb("-500").unwrap();
    assert!(
        matches!(r.editor.active().guides.values().next().unwrap(), Guide::Line { origin, .. } if (origin.y + 0.5).abs() < 1e-9)
    );
    r.editor.undo();
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn tape_point_retains_measured_origin_and_rejects_stale_correction() {
    let mut r = Rig::new(ToolKind::TapeMeasure);
    r.click(v(0., 0., 0.));
    r.hover(v(1., 0., 0.));
    r.type_vcb("500").unwrap();
    assert!(
        matches!(r.editor.active().guides.values().next().unwrap(), Guide::Point { p, origin: Some(origin), .. } if p.distance(v(0.5, 0., 0.)) < 1e-9 && origin.length() < 1e-9)
    );
    r.type_vcb("700").unwrap();
    assert_eq!(r.editor.active().guides.len(), 1);
    r.editor.apply(Command::AddGuidePoint { point: v(2., 2., 0.) }).unwrap();
    let before = r.editor.doc().clone();
    assert!(r.type_vcb("900").is_err());
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn arc_continues_an_unconnected_arc_with_a_cyan_tangent_preview() {
    let mut r = Rig::new(ToolKind::Arc);
    r.editor
        .apply(Command::DrawArc { start: v(-1., 0., 0.), end: DVec3::ZERO, normal: DVec3::Z, bulge: 0.5, segments: 24 })
        .unwrap();
    let before = r.editor.doc().clone();
    r.click(DVec3::ZERO);
    r.click(v(1., -1., 0.));
    let bulge = 0.5f64.sqrt() - 1.;
    let at = v(0.5, -0.5, 0.) + v(1., 1., 0.).normalize() * bulge;
    r.hover(at);
    assert_eq!(r.tools.preview(&r.editor).label.as_deref(), Some("Tangent"));
    r.click(at);
    assert_eq!(r.editor.mesh().curves().count(), 2);
    let curve = r.editor.mesh().curves().last().unwrap().1;
    assert!((curve.radius - 1.).abs() < 1e-8);
    assert!(curve.centre.distance(v(1., 0., 0.)) < 1e-8);
    r.editor.undo();
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn tape_measure_only_reports_full_distance_from_an_edge_including_mid_operation_toggle() {
    let mut r = Rig::new(ToolKind::TapeMeasure);
    square(&mut r, 2.);
    let before = r.editor.doc().clone();
    r.click(v(0.6, 0., 0.));
    r.mods.ctrl = true;
    r.hover(v(1.6, 0., 0.));
    assert_eq!(r.tools.vcb_value(&r.editor), "1000 mm");
    r.click(v(1.6, 0., 0.));
    assert_eq!(r.tools.vcb_value(&r.editor), "1000 mm");
    assert_eq!(r.editor.doc(), &before);
    r.type_vcb("2m").unwrap();
    assert!((r.tools.pending_resize_factor().unwrap() - 2.).abs() < 1e-9);
}

#[test]
fn guide_window_selection_hide_unhide_and_global_display_agree() {
    let mut r = Rig::new(ToolKind::Select);
    let element =
        r.editor.apply(Command::AddGuideSegment { start: v(0.5, 0.5, 0.), point: v(1.5, 0.5, 0.) }).unwrap().created[0];
    let a = r.px(v(0.5, 0.5, 0.));
    let b = r.px(v(1.5, 0.5, 0.));
    let lo = a.min(b) - rysmo_math::DVec2::splat(15.);
    let hi = a.max(b) + rysmo_math::DVec2::splat(15.);
    let ctx = Ctx { view: &r.view, cursor: hi, modifiers: r.mods, scale: 1. };
    r.tools.box_select(&mut r.editor, &ctx, lo, hi);
    assert!(r.editor.is_selected(element));
    r.editor.apply(Command::SetGeometryHidden { elements: vec![element], hidden: true }).unwrap();
    r.editor.clear_selection();
    r.tools.box_select(&mut r.editor, &ctx, lo, hi);
    assert!(!r.editor.is_selected(element));
    let mut style = r.editor.doc().style;
    style.show_hidden_geometry = true;
    r.editor.apply(Command::SetStyle { style }).unwrap();
    r.tools.box_select(&mut r.editor, &ctx, lo, hi);
    assert!(r.editor.is_selected(element));
    style.show_guides = false;
    r.editor.apply(Command::SetStyle { style }).unwrap();
    r.editor.clear_selection();
    r.tools.box_select(&mut r.editor, &ctx, lo, hi);
    assert!(!r.editor.is_selected(element));
}

#[test]
fn moved_crossing_edges_remain_selected_and_exact_correction_and_undo_work() {
    let mut r = Rig::new(ToolKind::Move);
    r.editor.apply(Command::DrawLines { lines: vec![[v(-1., 0., 0.), v(1., 0., 0.)]], facing: None }).unwrap();
    let moved = r
        .editor
        .apply(Command::DrawLines { lines: vec![[v(0., 1., 0.), v(0., 3., 0.)]], facing: None })
        .unwrap()
        .created;
    r.editor.set_selection(moved);
    let before = r.editor.doc().clone();
    r.click(v(0., 1., 0.));
    r.click(v(0., -1., 0.));
    assert_eq!(r.editor.mesh().edge_count(), 4);
    assert_eq!(r.editor.selected().len(), 2);
    assert!(r.editor.selected().iter().all(|e| r.editor.contains(*e)));
    r.type_vcb("2500").unwrap();
    assert_eq!(r.editor.selected().len(), 2);
    r.editor.undo();
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn parallel_inference_stays_magenta_when_shift_locked_and_accepts_exact_input() {
    let mut r = Rig::new(ToolKind::Line);
    r.editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, v(1., 1., 0.)]], facing: None }).unwrap();
    let before = r.editor.doc().clone();
    r.click(DVec3::ZERO);
    r.hover(v(2., 2., 0.));
    assert_eq!(r.tools.preview(&r.editor).inference.unwrap().kind, rysmo_infer::Kind::Parallel);
    r.key(Key::Shift(true));
    r.hover(v(2., 2.4, 0.));
    assert_eq!(r.tools.preview(&r.editor).inference.unwrap().kind, rysmo_infer::Kind::Parallel);
    r.type_vcb("2m").unwrap();
    assert!(r.editor.mesh().find_vertex(v(1., 1., 0.).normalize() * 2.).is_some());
    r.editor.undo();
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn group_rotation_handle_fixes_centre_and_plane_and_undoes() {
    let mut r = Rig::new(ToolKind::Rotate);
    cube(&mut r);
    let group = r.editor.apply(Command::MakeGroup { elements: all(&r) }).unwrap().created[0];
    r.editor.set_selection([group]);
    let before = r.editor.doc().clone();
    r.hover(v(0.5, 0.5, 1.0));
    assert_eq!(r.tools.preview(&r.editor).markers.len(), 6);
    r.click(v(0.5, 0.5, 1.0));
    match &r.tools.state {
        crate::tools::State::RotatePivot { centre, axis, .. } => {
            assert!(centre.distance(v(0.5, 0.5, 1.0)) < 1e-8);
            assert!(axis.distance(DVec3::Z) < 1e-8);
        }
        _ => panic!("rotation handle did not start the protractor"),
    }
    r.click(v(2.0, 0.5, 1.0));
    r.type_vcb("90").unwrap();
    assert_ne!(r.editor.doc(), &before);
    assert!(r.editor.undo());
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn scale_picks_exact_grip_in_a_crowded_projection() {
    let mut r = Rig::new(ToolKind::Scale);
    square(&mut r, 0.01);
    r.editor.set_selection(all(&r));
    r.click(v(0.01, 0.01, 0.0));
    match &r.tools.state {
        crate::tools::State::Scaling { grip, .. } => assert_eq!(grip.0, [1, 1, 0]),
        _ => panic!("scale grip was not picked"),
    }
    r.type_vcb("2").unwrap();
    assert!(r.editor.mesh().find_vertex(v(0.02, 0.02, 0.0)).is_some());
}

#[test]
fn hidden_group_rotation_handle_respects_explicit_plane_lock() {
    let mut r = Rig::new(ToolKind::Rotate);
    cube(&mut r);
    let group = r.editor.apply(Command::MakeGroup { elements: all(&r) }).unwrap().created[0];
    // A large unrelated face lies between the camera and the selected group.
    let centre = v(0.5, 0.5, 1.0);
    let screen = centre + (r.view.eye - centre) * 0.4;
    let normal = (r.view.eye - centre).normalize();
    let u = normal.any_orthonormal_vector() * 5.;
    let w = normal.cross(u);
    r.editor
        .apply(Command::DrawRectangle { corner: screen - u - w, u: u * 2., v: w * 2., facing: Some(normal) })
        .unwrap();
    r.editor.set_selection([group]);
    r.key(Key::Right);
    r.click(centre);
    match &r.tools.state {
        crate::tools::State::RotatePivot { centre: pivot, axis, .. } => {
            assert!(pivot.distance(centre) < 1e-8);
            assert!(axis.distance(DVec3::X) < 1e-8);
        }
        _ => panic!("occluding face intercepted the handle"),
    }
    r.key(Key::Escape);
    assert!(!r.tools.busy());
    assert_eq!(r.editor.selected(), vec![group]);
    r.tools.set_tool(ToolKind::Scale);
    r.click(v(1., 1., 1.));
    assert!(matches!(r.tools.state, crate::tools::State::Scaling { .. }));
}

#[test]
fn arc_tangent_to_a_straight_edge_can_be_held_with_shift_at_either_end() {
    for reverse in [false, true] {
        let mut r = Rig::new(ToolKind::Arc);
        r.editor
            .apply(Command::DrawLines { lines: vec![[v(-2., 0., 0.), DVec3::ZERO]], facing: Some(DVec3::Z) })
            .unwrap();
        if reverse {
            r.click(v(1., 1., 0.));
            r.click(DVec3::ZERO);
        } else {
            r.click(DVec3::ZERO);
            r.click(v(1., 1., 0.));
        }
        let bulge = if reverse { 1. - 0.5f64.sqrt() } else { 0.5f64.sqrt() - 1. };
        let chord = if reverse { v(-1., -1., 0.) } else { v(1., 1., 0.) };
        let at = v(0.5, 0.5, 0.) + DVec3::Z.cross(chord.normalize()) * bulge;
        r.hover(at);
        assert_eq!(r.tools.preview(&r.editor).label.as_deref(), Some("Tangent"));
        r.mods.shift = true;
        r.hover(v(0., 3., 0.));
        assert_eq!(r.tools.preview(&r.editor).label.as_deref(), Some("Tangent"));
        r.mods.shift = false;
        r.hover(v(0., 3., 0.));
        assert_ne!(r.tools.preview(&r.editor).label.as_deref(), Some("Tangent"));
    }
}
