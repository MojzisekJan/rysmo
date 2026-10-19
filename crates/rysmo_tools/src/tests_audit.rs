//! Every tool's cancellation and stray input, on the same box.

use rysmo_mesh::Element;

use crate::tests::{Rig, v};
use crate::*;

/// A 2 x 1 x 1 box with the given tool active and nothing selected.
fn boxed(tool: ToolKind) -> Rig {
    let mut r = Rig::new(ToolKind::PushPull);
    r.rectangle(v(0.0, 0.0, 0.0), v(2.0, 1.0, 0.0));
    r.click(v(1.0, 0.5, 0.0));
    r.hover(v(1.0, 0.5, 0.5));
    r.type_vcb("1000").unwrap();
    r.key(Key::Escape);
    r.tools.set_tool(tool);
    r.editor.clear_selection();
    r
}

/// Tools that edit the model, with how many clicks leave an operation under way
/// without committing any part of it. Freehand draws while held instead, Scale works on
/// the selection, and the app runs text creation.
const OPERATIONS: [(ToolKind, usize); 20] = [
    (ToolKind::Line, 1),
    (ToolKind::Freehand, 1),
    (ToolKind::Arc, 2),
    (ToolKind::ThreePointArc, 2),
    (ToolKind::CenterArc, 2),
    (ToolKind::Pie, 2),
    (ToolKind::Rectangle, 1),
    (ToolKind::RotatedRectangle, 2),
    (ToolKind::Circle, 1),
    (ToolKind::Polygon, 1),
    (ToolKind::PushPull, 1),
    (ToolKind::Offset, 1),
    (ToolKind::FollowMe, 1),
    (ToolKind::Move, 1),
    (ToolKind::Rotate, 2),
    (ToolKind::Scale, 1),
    (ToolKind::TapeMeasure, 1),
    (ToolKind::Protractor, 2),
    (ToolKind::Axes, 2),
    (ToolKind::Dimension, 2),
];

#[test]
fn escape_cancels_every_operation_without_changing_the_model() {
    let points = [v(0.6, 0.3, 1.0), v(1.4, 0.7, 1.0), v(1.7, 0.4, 1.0)];
    let mut failures = Vec::new();
    for (tool, clicks) in OPERATIONS {
        let mut r = boxed(tool);
        let before = r.editor.doc().clone();
        let depth = r.editor.undo_depth();
        if tool == ToolKind::Scale {
            let top = r.editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect::<Vec<_>>();
            r.editor.set_selection(top);
        }
        r.hover(points[2]);
        let idle = r.tools.preview(&r.editor).lines;
        if tool == ToolKind::Freehand {
            r.hover(points[0]);
            r.ev(points[0], Pointer::Down { clicks: 1 });
            r.ev(points[1], Pointer::Move);
        } else {
            for p in &points[..clicks] {
                r.click(*p);
            }
            r.hover(points[clicks]);
        }
        let started = r.tools.busy();
        r.key(Key::Escape);
        r.hover(points[2]);
        let preview = r.tools.preview(&r.editor);
        let problems = [
            (!started, "no operation was under way"),
            (r.editor.doc() != &before, "the model changed"),
            (r.editor.undo_depth() != depth, "an undo step was added"),
            (r.tools.busy(), "still busy after Escape"),
            (preview.lines != idle, "the preview differs from the idle tool's"),
            (preview.dimension.is_some(), "a dimension preview remains"),
        ];
        for (failed, what) in problems {
            if failed {
                failures.push(format!("{tool:?}: {what}"));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn typing_a_value_with_nothing_under_way_leaves_the_model_alone() {
    let mut failures = Vec::new();
    for (tool, _) in OPERATIONS {
        let mut r = boxed(tool);
        let before = r.editor.doc().clone();
        r.hover(v(1.0, 0.5, 1.0));
        let _ = r.type_vcb("500");
        if r.editor.doc() != &before {
            failures.push(format!("{tool:?}"));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn escape_after_selecting_keeps_the_selection_for_modifying_tools() {
    for tool in [ToolKind::Move, ToolKind::Rotate, ToolKind::Scale] {
        let mut r = boxed(tool);
        let face = r.editor.mesh().faces().next().map(|(id, _)| id).unwrap();
        r.editor.set_selection([Element::Face(face)]);
        r.click(v(0.6, 0.3, 1.0));
        r.hover(v(1.4, 0.7, 1.0));
        r.key(Key::Escape);
        assert!(r.editor.selection().contains(&Element::Face(face)), "{tool:?}");
    }
}

#[test]
fn arrow_keys_lock_toggle_and_release_the_axis_from_the_start_point() {
    let start = v(0.6, 0.3, 1.0);
    let off = v(1.5, 0.9, 1.0);
    let on_x = |p: rysmo_math::DVec3| (p.y - start.y).abs() < 1e-9 && (p.z - start.z).abs() < 1e-9;
    let mut failures = Vec::new();
    for tool in [ToolKind::Line, ToolKind::Move, ToolKind::TapeMeasure, ToolKind::Dimension, ToolKind::Axes] {
        let mut r = boxed(tool);
        r.click(start);
        let point = |r: &mut Rig| {
            r.hover(off);
            r.tools.preview(&r.editor).inference.map(|i| i.point)
        };
        r.key(Key::Right);
        if !point(&mut r).is_some_and(on_x) {
            failures.push(format!("{tool:?}: Right does not lock the red axis"));
        }
        r.key(Key::Right);
        if point(&mut r).is_some_and(on_x) {
            failures.push(format!("{tool:?}: a second Right does not unlock"));
        }
        r.key(Key::Right);
        r.key(Key::Down);
        if point(&mut r).is_some_and(on_x) {
            failures.push(format!("{tool:?}: Down does not release the lock"));
        }
        r.key(Key::Escape);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn shift_holds_an_axis_inference_until_released() {
    let start = v(0.6, 0.3, 1.0);
    let along = v(1.4, 0.3, 1.0);
    let off = v(1.5, 0.9, 1.0);
    let on_x = |p: rysmo_math::DVec3| (p.y - start.y).abs() < 1e-9 && (p.z - start.z).abs() < 1e-9;
    let mut failures = Vec::new();
    for tool in [ToolKind::Line, ToolKind::Move, ToolKind::TapeMeasure, ToolKind::Dimension, ToolKind::Axes] {
        let mut r = boxed(tool);
        r.click(start);
        let point = |r: &mut Rig, p| {
            r.hover(p);
            r.tools.preview(&r.editor).inference.map(|i| i.point)
        };
        if !point(&mut r, along).is_some_and(on_x) {
            failures.push(format!("{tool:?}: no red axis inference to hold"));
            continue;
        }
        r.key(Key::Shift(true));
        if !point(&mut r, off).is_some_and(on_x) {
            failures.push(format!("{tool:?}: Shift does not hold the red axis"));
        }
        r.key(Key::Shift(false));
        if point(&mut r, off).is_some_and(on_x) {
            failures.push(format!("{tool:?}: releasing Shift keeps the lock"));
        }
        r.key(Key::Escape);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn tools_without_a_value_refuse_typing_and_keep_the_operation() {
    for tool in [ToolKind::Axes, ToolKind::Freehand] {
        let mut r = boxed(tool);
        let before = r.editor.doc().clone();
        r.hover(v(0.6, 0.3, 1.0));
        r.ev(v(0.6, 0.3, 1.0), Pointer::Down { clicks: 1 });
        if tool == ToolKind::Axes {
            r.ev(v(0.6, 0.3, 1.0), Pointer::Up);
        }
        r.hover(v(1.4, 0.3, 1.0));
        assert!(r.type_vcb("500").is_err(), "{tool:?}");
        assert_eq!(r.editor.doc(), &before, "{tool:?}");
        assert!(r.tools.busy(), "{tool:?}");
    }
}
