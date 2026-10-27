use rysmo_math::{DVec2, DVec3, OrbitCamera, View};
use rysmo_mesh::Element;
use rysmo_model::Editor;

use crate::*;

pub(crate) struct Rig {
    pub(crate) editor: Editor,
    pub(crate) tools: ToolBox,
    pub(crate) view: View,
    pub(crate) mods: Modifiers,
}

impl Rig {
    pub(crate) fn new(tool: ToolKind) -> Self {
        let cam = OrbitCamera {
            target: DVec3::new(1.0, 1.0, 0.5),
            distance: 10.0,
            yaw: -1.0,
            pitch: 0.6,
            fov_y: 0.7,
            parallel: false,
            eye_base_z: 0.0,
            roll: 0.,
        };
        let mut tools = ToolBox::new();
        tools.set_tool(tool);
        Rig { editor: Editor::new(), tools, view: cam.view(DVec2::new(1000.0, 800.0)), mods: Modifiers::default() }
    }

    pub(crate) fn px(&self, p: DVec3) -> DVec2 {
        self.view.project(p).unwrap()
    }

    pub(crate) fn ev(&mut self, p: DVec3, ev: Pointer) {
        let cursor = self.px(p);
        let ctx = Ctx { view: &self.view, cursor, modifiers: self.mods, scale: 1.0 };
        self.tools.pointer(&mut self.editor, &ctx, ev);
    }

    pub(crate) fn click(&mut self, p: DVec3) {
        self.clicks(p, 1);
    }

    pub(crate) fn clicks(&mut self, p: DVec3, n: u8) {
        self.ev(p, Pointer::Move);
        self.ev(p, Pointer::Down { clicks: n });
        self.ev(p, Pointer::Up);
    }

    pub(crate) fn hover(&mut self, p: DVec3) {
        self.ev(p, Pointer::Move);
    }

    pub(crate) fn type_vcb(&mut self, text: &str) -> Result<(), String> {
        let ctx = Ctx { view: &self.view, cursor: DVec2::ZERO, modifiers: self.mods, scale: 1.0 };
        self.tools.vcb_enter(&mut self.editor, &ctx, text)
    }

    pub(crate) fn key(&mut self, k: Key) {
        let ctx = Ctx { view: &self.view, cursor: DVec2::ZERO, modifiers: self.mods, scale: 1.0 };
        self.tools.key(&mut self.editor, &ctx, k);
    }

    pub(crate) fn rectangle(&mut self, a: DVec3, b: DVec3) {
        let t = self.tools.active();
        self.tools.set_tool(ToolKind::Rectangle);
        self.click(a);
        self.click(b);
        self.tools.set_tool(t);
    }
}

pub(crate) fn v(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y, z)
}

#[test]
fn line_tool_chains_until_a_face_closes() {
    let mut r = Rig::new(ToolKind::Line);
    let pts = [v(0.3, 0.3, 0.0), v(2.3, 0.3, 0.0), v(2.3, 1.3, 0.0), v(0.3, 1.3, 0.0)];
    r.click(pts[0]);
    for p in &pts[1..] {
        r.click(*p);
    }
    assert_eq!(r.editor.mesh().edge_count(), 3);
    assert!(r.tools.busy());
    r.click(pts[0]);
    assert_eq!(r.editor.mesh().face_count(), 1);
    assert!(!r.tools.busy(), "closing a face ends the line");
}

#[test]
fn press_drag_release_draws_a_line() {
    let mut r = Rig::new(ToolKind::Line);
    r.hover(v(0.5, 0.5, 0.0));
    r.ev(v(0.5, 0.5, 0.0), Pointer::Down { clicks: 1 });
    r.ev(v(1.5, 0.5, 0.0), Pointer::Move);
    r.ev(v(1.5, 0.5, 0.0), Pointer::Up);
    assert_eq!(r.editor.mesh().edge_count(), 1);
}

#[test]
fn typed_length_sets_the_line() {
    let mut r = Rig::new(ToolKind::Line);
    r.click(v(0.5, 0.5, 0.0));
    r.hover(v(1.7, 0.5, 0.0));
    assert_eq!(r.tools.vcb_label(), "Length");
    assert_eq!(r.tools.vcb_value(&r.editor), "1200 mm");
    r.type_vcb("750").unwrap();
    let (a, b) = r.editor.mesh().edge_points(r.editor.mesh().edges().next().unwrap().0);
    assert!((a.distance(b) - 0.75).abs() < 1e-9);
    // The line continues from its end.
    assert!(r.tools.busy());
    r.key(Key::Escape);
    assert!(!r.tools.busy());
}

#[test]
fn arrow_key_locks_an_axis() {
    let mut r = Rig::new(ToolKind::Line);
    r.click(v(0.5, 0.5, 0.0));
    r.key(Key::Up);
    // The cursor is far off the blue axis, but the point stays on it.
    r.hover(v(2.0, 1.0, 0.0));
    let p = r.tools.preview(&r.editor).inference.unwrap().point;
    assert!((p.x - 0.5).abs() < 1e-9 && (p.y - 0.5).abs() < 1e-9);
}

#[test]
fn rectangle_by_clicks_and_by_typed_dimensions() {
    let mut r = Rig::new(ToolKind::Rectangle);
    r.click(v(0.2, 0.2, 0.0));
    r.click(v(1.2, 0.7, 0.0));
    assert_eq!(r.editor.mesh().face_count(), 1);
    let f = r.editor.mesh().faces().next().unwrap().0;
    assert!((r.editor.mesh().face_area(f) - 0.5).abs() < 1e-6);

    let mut r = Rig::new(ToolKind::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(1.0, 1.0, 0.0));
    assert_eq!(r.tools.vcb_label(), "Dimensions");
    r.type_vcb("2000,500").unwrap();
    let f = r.editor.mesh().faces().next().unwrap().0;
    assert!((r.editor.mesh().face_area(f) - 1.0).abs() < 1e-9);
    assert!(r.editor.mesh().find_vertex(v(2.0, 0.5, 0.0)).is_some());
}

#[test]
fn rectangle_dimensions_typed_right_after_drawing_correct_it_in_one_undo_step() {
    let mut r = Rig::new(ToolKind::Rectangle);
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(1.0, 1.0, 0.0));
    r.type_vcb("2000,500").unwrap();
    r.type_vcb("600,400").unwrap();
    assert_eq!(r.editor.mesh().face_count(), 1);
    assert_eq!(r.editor.mesh().edge_count(), 4);
    assert!(r.editor.mesh().find_vertex(v(0.6, 0.4, 0.0)).is_some());
    let _ = r.type_vcb("600");
    assert!(r.editor.mesh().find_vertex(v(0.6, 0.4, 0.0)).is_some(), "one value is not a correction");
    r.editor.undo();
    assert_eq!(r.editor.mesh().face_count(), 0);
}

#[test]
fn arrow_keys_lock_the_plane_of_rectangles_circles_and_arcs() {
    // Right before the first click: the plane perpendicular to red, through the click.
    let mut r = Rig::new(ToolKind::Rectangle);
    r.key(Key::Right);
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(0.0, 1.0, 1.0));
    r.type_vcb("2000,1000").unwrap();
    assert_eq!(r.editor.mesh().face_count(), 1);
    assert!(r.editor.mesh().vertices().all(|(_, x)| x.pos.x.abs() < 1e-9));
    assert!(r.editor.mesh().vertices().any(|(_, x)| x.pos.z > 0.5));

    // Left after the centre: a circle in the plane perpendicular to green.
    let mut r = Rig::new(ToolKind::Circle);
    r.click(v(0.0, 0.0, 0.0));
    r.key(Key::Left);
    r.hover(v(0.5, 0.0, 0.5));
    r.type_vcb("500").unwrap();
    assert!(r.editor.mesh().vertices().all(|(_, x)| x.pos.y.abs() < 1e-9));
    assert!(r.editor.mesh().vertices().any(|(_, x)| x.pos.z > 0.4));

    // The lock ends with the operation: the next rectangle is on the ground again.
    let mut r = Rig::new(ToolKind::Rectangle);
    r.key(Key::Up);
    r.key(Key::Up);
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(1.0, 1.0, 0.0));
    r.type_vcb("1000,1000").unwrap();
    assert!(r.editor.mesh().vertices().all(|(_, x)| x.pos.z.abs() < 1e-9));
}

#[test]
fn arc_segments_and_bulge_typed_right_after_drawing_correct_it() {
    let mut r = Rig::new(ToolKind::Arc);
    r.click(v(0.0, 0.0, 0.0));
    r.click(v(1.0, 0.0, 0.0));
    r.hover(v(0.5, 0.3, 0.0));
    r.type_vcb("500").unwrap();
    assert_eq!(r.editor.mesh().edge_count(), 12);
    r.type_vcb("8s").unwrap();
    assert_eq!(r.editor.mesh().edge_count(), 8);
    r.type_vcb("250").unwrap();
    assert_eq!(r.editor.mesh().edge_count(), 8);
    let far = r.editor.mesh().vertices().map(|(_, x)| x.pos.y.abs()).fold(0.0, f64::max);
    assert!((far - 0.25).abs() < 1e-6, "bulge {far}");
    r.editor.undo();
    assert_eq!(r.editor.mesh().edge_count(), 0);
}

#[test]
fn circle_with_typed_sides_and_radius() {
    let mut r = Rig::new(ToolKind::Circle);
    r.type_vcb("12s").unwrap();
    r.click(v(1.0, 1.0, 0.0));
    r.hover(v(1.5, 1.0, 0.0));
    r.type_vcb("300").unwrap();
    assert_eq!(r.editor.mesh().edge_count(), 12);
    assert_eq!(r.editor.mesh().face_count(), 1);
    let far = r.editor.mesh().vertices().map(|(_, x)| x.pos.distance(v(1.0, 1.0, 0.0))).fold(0.0, f64::max);
    assert!((far - 0.3).abs() < 1e-9);
}

#[test]
fn push_pull_by_value_double_click_and_correction() {
    let mut r = Rig::new(ToolKind::PushPull);
    r.rectangle(v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0));
    r.click(v(0.5, 0.5, 0.0));
    r.hover(v(0.5, 0.5, 0.6));
    assert_eq!(r.tools.vcb_label(), "Distance");
    r.type_vcb("1000").unwrap();
    let m = r.editor.mesh();
    assert_eq!(m.face_count(), 6);
    assert!(m.find_vertex(v(1.0, 1.0, 1.0)).is_some());

    // Typing right after corrects the distance.
    r.type_vcb("400").unwrap();
    assert!(r.editor.mesh().find_vertex(v(1.0, 1.0, 0.4)).is_some());
    assert_eq!(r.editor.mesh().face_count(), 6);

    // Double-clicking the top repeats the distance: the box grows by 0.4.
    r.clicks(v(0.5, 0.5, 0.4), 1);
    r.ev(v(0.5, 0.5, 0.4), Pointer::Down { clicks: 2 });
    r.ev(v(0.5, 0.5, 0.4), Pointer::Up);
    assert!(r.editor.mesh().find_vertex(v(1.0, 1.0, 0.8)).is_some(), "double click should repeat");
    assert_eq!(r.editor.mesh().face_count(), 6);
}

#[test]
fn move_copies_with_ctrl_and_makes_an_array() {
    let mut r = Rig::new(ToolKind::Move);
    r.rectangle(v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0));
    let f = r.editor.mesh().faces().next().unwrap().0;
    r.editor.set_selection([Element::Face(f)]);
    r.mods.ctrl = true;
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(1.3, 0.0, 0.0));
    r.type_vcb("4500").unwrap();
    assert_eq!(r.editor.mesh().face_count(), 2);
    // Three copies dividing the distance.
    r.type_vcb("/3").unwrap();
    assert_eq!(r.editor.mesh().face_count(), 4);
    assert!(r.editor.mesh().find_vertex(v(2.5, 1.0, 0.0)).is_some());
    assert!(r.editor.mesh().find_vertex(v(5.5, 1.0, 0.0)).is_some());
    // Two copies at the full distance.
    r.type_vcb("2x").unwrap();
    assert_eq!(r.editor.mesh().face_count(), 3);
    assert!(r.editor.mesh().find_vertex(v(10.0, 1.0, 0.0)).is_some());
}

#[test]
fn move_picks_the_hovered_face_when_nothing_is_selected() {
    let mut r = Rig::new(ToolKind::Move);
    r.rectangle(v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0));
    r.click(v(0.5, 0.5, 0.0));
    r.click(v(0.5, 2.5, 0.0));
    assert!(r.editor.mesh().find_vertex(v(1.0, 3.0, 0.0)).is_some());
    assert_eq!(r.editor.undo_label(), Some("Move"));
}

#[test]
fn eraser_erases_the_clicked_edge() {
    let mut r = Rig::new(ToolKind::Eraser);
    r.rectangle(v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0));
    r.click(v(0.5, 0.0, 0.0));
    assert_eq!(r.editor.mesh().edge_count(), 3);
    assert_eq!(r.editor.mesh().face_count(), 0);
}

#[test]
fn select_click_double_click_and_box() {
    let mut r = Rig::new(ToolKind::Select);
    r.rectangle(v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0));
    r.click(v(0.5, 0.5, 0.0));
    assert_eq!(r.editor.selection().len(), 1);
    r.clicks(v(0.5, 0.5, 0.0), 2);
    assert_eq!(r.editor.selection().len(), 5);
    // Clicking empty space clears.
    r.click(v(4.0, -3.0, 0.0));
    assert!(r.editor.selection().is_empty());
    // A window from left to right around everything.
    let a = r.px(v(-1.0, -1.0, 0.0)).min(r.px(v(2.0, 2.0, 0.0))) - DVec2::splat(50.0);
    let b = r.px(v(-1.0, -1.0, 0.0)).max(r.px(v(2.0, 2.0, 0.0))) + DVec2::splat(50.0);
    let ctx = |c| Ctx { view: &r.view, cursor: c, modifiers: Modifiers::default(), scale: 1.0 };
    let (view_a, view_b) = (ctx(a), ctx(b));
    r.tools.pointer(&mut r.editor, &view_a, Pointer::Down { clicks: 1 });
    r.tools.pointer(&mut r.editor, &view_b, Pointer::Move);
    assert!(r.tools.preview(&r.editor).select_box.is_some());
    r.tools.pointer(&mut r.editor, &view_b, Pointer::Up);
    assert_eq!(r.editor.selection().len(), 5);
}

#[test]
fn shift_toggles_selection() {
    let mut r = Rig::new(ToolKind::Select);
    r.rectangle(v(0.0, 0.0, 0.0), v(1.0, 1.0, 0.0));
    r.click(v(0.5, 0.5, 0.0));
    r.mods.shift = true;
    r.click(v(0.5, 0.0, 0.0));
    assert_eq!(r.editor.selection().len(), 2);
    r.click(v(0.5, 0.0, 0.0));
    assert_eq!(r.editor.selection().len(), 1);
}

#[test]
fn errors_go_to_the_status_bar() {
    let mut r = Rig::new(ToolKind::Line);
    assert_eq!(r.tools.status(), "Select start point.");
    assert!(r.type_vcb("abc").is_err());
    let mut r = Rig::new(ToolKind::Move);
    assert!(r.type_vcb("5x").is_err());
}

#[test]
fn every_tool_has_a_default_shortcut() {
    let keys: Vec<_> = ToolKind::ALL.iter().map(|t| t.shortcut()).collect();
    assert_eq!(
        keys,
        [
            "Space",
            "E",
            "L",
            "",
            "A",
            "", // Three Point Arc
            "", // Center Arc
            "", // Pie
            "R",
            "", // Rotated Rectangle has no default shortcut.
            "C",
            "",
            "P",
            "F",
            "",
            "M",
            "Q",
            "S",
            "T",
            "",
            "",
            "B",
            "O",
            "H",
            "Z",
            "Ctrl+Shift+W",
            "",
            "",
            "",
            "",
            ""
        ]
    );
}

#[test]
fn polygon_exact_radius_and_immediate_side_correction() {
    let mut r = Rig::new(ToolKind::Polygon);
    r.type_vcb("5").unwrap();
    assert_eq!(r.tools.polygon_segments(), 5);
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(1.0, 0.0, 0.0));
    r.type_vcb("2mr").unwrap();
    assert_eq!(r.editor.mesh().edge_count(), 5);
    assert_eq!(r.editor.mesh().face_count(), 1);
    for (_, vertex) in r.editor.mesh().vertices() {
        assert!((vertex.pos.length() - 2.0).abs() < 1e-7);
    }
    r.type_vcb("8s").unwrap();
    assert_eq!(r.editor.mesh().edge_count(), 8);
    r.type_vcb("1m").unwrap();
    for (_, vertex) in r.editor.mesh().vertices() {
        assert!((vertex.pos.length() - 1.0).abs() < 1e-7);
    }
    r.editor.undo();
    assert_eq!(r.editor.mesh().edge_count(), 0);
}

#[test]
fn radial_correction_refuses_an_intervening_model_change() {
    let mut r = Rig::new(ToolKind::Circle);
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(1.0, 0.0, 0.0));
    r.type_vcb("1m").unwrap();
    r.editor
        .apply(rysmo_model::Command::AddMaterial { name: "Keep me".into(), color: [1.0; 4], texture: None })
        .unwrap();
    assert!(r.type_vcb("12s").is_err());
    assert_eq!(r.editor.doc().materials.len(), 1);
    assert_eq!(r.editor.mesh().edge_count(), 24);
}

#[test]
fn polygon_sides_do_not_change_circle_settings_and_escape_cancels() {
    let mut r = Rig::new(ToolKind::Polygon);
    r.type_vcb("7s").unwrap();
    assert_eq!(r.tools.circle_segments(), 24);
    r.click(v(0.0, 0.0, 0.0));
    r.key(Key::Escape);
    assert!(!r.tools.busy());
    assert_eq!(r.editor.mesh().edge_count(), 0);
    assert!(r.type_vcb("2s").is_err());
}

#[test]
fn freehand_drag_closes_a_face_and_undoes_as_one_operation() {
    let mut r = Rig::new(ToolKind::Freehand);
    let start = v(0.0, 0.0, 0.0);
    r.ev(start, Pointer::Down { clicks: 1 });
    for point in [v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0), v(0.0, 1.0, 0.0)] {
        r.ev(point, Pointer::Move);
    }
    r.ev(start, Pointer::Up);
    assert_eq!(r.editor.mesh().face_count(), 1);
    assert_eq!(r.editor.mesh().edge_count(), 4);
    assert!(!r.tools.busy());
    r.editor.undo();
    assert_eq!(r.editor.mesh().edge_count(), 0);
}

#[test]
fn freehand_tap_creates_nothing_and_escape_discards_the_preview() {
    let mut r = Rig::new(ToolKind::Freehand);
    r.click(v(0.0, 0.0, 0.0));
    assert_eq!(r.editor.mesh().edge_count(), 0);
    r.ev(v(0.0, 0.0, 0.0), Pointer::Down { clicks: 1 });
    r.ev(v(1.0, 0.0, 0.0), Pointer::Move);
    r.key(Key::Escape);
    r.ev(v(1.0, 0.0, 0.0), Pointer::Up);
    assert_eq!(r.editor.mesh().edge_count(), 0);
}

#[test]
fn clicking_one_curve_segment_selects_the_curve() {
    let mut r = Rig::new(ToolKind::Circle);
    r.click(v(0.0, 0.0, 0.0));
    r.hover(v(1.0, 0.0, 0.0));
    r.type_vcb("1m").unwrap();
    let edge = r.editor.mesh().edges().next().unwrap().0;
    let (a, b) = r.editor.mesh().edge_points(edge);
    r.tools.set_tool(ToolKind::Select);
    r.click((a + b) * 0.5);
    assert_eq!(r.editor.selected().len(), 24);
    assert!(r.editor.selected().iter().all(|e| matches!(e, Element::Edge(_))));
}

#[test]
fn crossing_box_inside_a_face_selects_it_but_window_does_not() {
    let mut r = Rig::new(ToolKind::Select);
    r.rectangle(v(-3., -3., 0.), v(3., 3., 0.));
    let centre = r.px(v(0., 0., 0.));
    let lo = centre - DVec2::splat(8.);
    let hi = centre + DVec2::splat(8.);
    let ctx = Ctx { view: &r.view, cursor: lo, modifiers: Modifiers::default(), scale: 1.0 };
    r.tools.box_select(&mut r.editor, &ctx, hi, lo);
    assert_eq!(r.editor.selection().len(), 1);
    assert!(matches!(r.editor.selected()[0], Element::Face(_)));
    r.tools.box_select(&mut r.editor, &ctx, lo, hi);
    assert!(r.editor.selection().is_empty());
}

#[test]
fn crossing_box_inside_a_hole_does_not_select_the_surrounding_face() {
    let mut r = Rig::new(ToolKind::Select);
    r.rectangle(v(-3., -3., 0.), v(3., 3., 0.));
    r.rectangle(v(-1., -1., 0.), v(1., 1., 0.));
    let outer = r.editor.mesh().faces().find(|(_, face)| !face.holes.is_empty()).map(|(id, _)| id).unwrap();
    let centre = r.px(v(0., 0., 0.));
    let lo = centre - DVec2::splat(8.);
    let hi = centre + DVec2::splat(8.);
    let ctx = Ctx { view: &r.view, cursor: lo, modifiers: Modifiers::default(), scale: 1.0 };
    r.tools.box_select(&mut r.editor, &ctx, hi, lo);
    assert!(!r.editor.selection().contains(&Element::Face(outer)));
    assert_eq!(r.editor.selection().len(), 1);
}

#[test]
fn crossing_box_inside_a_group_selects_it_and_respects_hidden_tags() {
    let mut r = Rig::new(ToolKind::Select);
    r.rectangle(v(-3., -3., 0.), v(3., 3., 0.));
    let elements = r.editor.mesh().faces().map(|(id, _)| Element::Face(id)).collect();
    let out = r.editor.apply(rysmo_model::Command::MakeGroup { elements }).unwrap();
    let Element::Instance(group) = out.created[0] else {
        panic!("group");
    };
    let centre = r.px(v(0., 0., 0.));
    let lo = centre - DVec2::splat(8.);
    let hi = centre + DVec2::splat(8.);
    let ctx = Ctx { view: &r.view, cursor: lo, modifiers: Modifiers::default(), scale: 1.0 };
    r.tools.box_select(&mut r.editor, &ctx, hi, lo);
    assert_eq!(r.editor.selected(), vec![Element::Instance(group)]);
    r.tools.box_select(&mut r.editor, &ctx, lo, hi);
    assert!(r.editor.selection().is_empty());
    let tag = r.editor.apply(rysmo_model::Command::AddTag { name: "Hidden group".into() }).unwrap().tag.unwrap();
    r.editor.apply(rysmo_model::Command::AssignTag { instances: vec![group], tag: Some(tag) }).unwrap();
    r.editor.apply(rysmo_model::Command::SetTag { tag, name: None, visible: Some(false) }).unwrap();
    r.tools.box_select(&mut r.editor, &ctx, hi, lo);
    assert!(r.editor.selection().is_empty());
}

#[test]
fn rectangle_square_and_golden_section_snaps_and_commits() {
    for (target, label, ratio) in
        [(v(2., 1.98, 0.), "Square", 1.0), (v(3.2360679775, 1.98, 0.), "Golden Section", (1.0 + 5.0_f64.sqrt()) * 0.5)]
    {
        let mut r = Rig::new(ToolKind::Rectangle);
        r.click(DVec3::ZERO);
        r.hover(target);
        let preview = r.tools.preview(&r.editor);
        assert_eq!(preview.label.as_deref(), Some(label));
        let diagonals: Vec<_> = preview.lines.iter().filter(|line| line.dashed).collect();
        if label == "Square" {
            assert_eq!(diagonals.len(), 1);
            assert_eq!(diagonals[0].a, DVec3::ZERO);
            assert_eq!(diagonals[0].b, preview.lines[1].b);
            r.hover(v(2., 1., 0.));
            assert!(r.tools.preview(&r.editor).lines.iter().all(|line| !line.dashed));
        } else {
            assert!(diagonals.is_empty());
        }
        r.click(target);
        let (lo, hi) = r.editor.mesh().bounds().unwrap();
        assert!(((hi.x - lo.x) / (hi.y - lo.y) - ratio).abs() < 1e-7);
        assert!(r.editor.undo());
        assert_eq!(r.editor.mesh().face_count(), 0);
    }
}
#[test]
fn rectangle_shape_snapping_does_not_override_an_endpoint_or_typed_dimensions() {
    let mut r = Rig::new(ToolKind::Rectangle);
    let target = v(2., 1.98, 0.);
    r.editor.apply(rysmo_model::Command::DrawLines { lines: vec![[target, v(3., 1.98, 0.)]], facing: None }).unwrap();
    r.click(DVec3::ZERO);
    r.hover(target);
    assert_eq!(r.tools.preview(&r.editor).inference.unwrap().point, target);
    assert_ne!(r.tools.preview(&r.editor).label.as_deref(), Some("Square"));
    r.type_vcb("2000,1980").unwrap();
    assert!(r.editor.mesh().find_vertex(target).is_some());
    let face = r.editor.mesh().faces().next().unwrap().0;
    assert!((r.editor.mesh().face_area(face) - 3.96).abs() < 1e-8);
}

#[test]
fn rectangle_square_snap_follows_rotated_drawing_axes() {
    let mut r = Rig::new(ToolKind::Rectangle);
    let x = v(1., 1., 0.).normalize();
    let y = v(-1., 1., 0.).normalize();
    r.editor.apply(rysmo_model::Command::SetAxes { origin: DVec3::ZERO, x, y }).unwrap();
    r.click(DVec3::ZERO);
    let target = x * 2. + y * 1.98;
    r.hover(target);
    let preview = r.tools.preview(&r.editor);
    assert_eq!(preview.label.as_deref(), Some("Square"));
    let diagonal = preview.lines.iter().find(|line| line.dashed).unwrap();
    assert_eq!(diagonal.a, DVec3::ZERO);
    assert_eq!(diagonal.b, preview.lines[1].b);
    assert!((diagonal.b.dot(x).abs() - diagonal.b.dot(y).abs()).abs() < 1e-8);
    r.click(target);
    let lengths: Vec<_> = r
        .editor
        .mesh()
        .edges()
        .map(|(id, _)| {
            let (a, b) = r.editor.mesh().edge_points(id);
            a.distance(b)
        })
        .collect();
    assert_eq!(lengths.len(), 4);
    assert!(lengths.iter().all(|length| (length - lengths[0]).abs() < 1e-8));
}

#[test]
fn annotation_pointer_pick_uses_select_modifiers_and_is_one_event_only() {
    use rysmo_model::dimensions::*;
    let mut r = Rig::new(ToolKind::Select);
    let dimension = Dimension {
        measure: DimensionMeasure::Linear {
            from: DimensionAnchor { point: DVec3::ZERO, reference: None },
            to: DimensionAnchor { point: DVec3::X, reference: None },
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
    let out = r.editor.apply(rysmo_model::Command::AddDimension { dimension }).unwrap();
    let entity = Element::Dimension(out.dimensions[0]);
    for (mods, selected) in [
        (Modifiers::default(), true),
        (Modifiers { shift: true, ..Default::default() }, false),
        (Modifiers { ctrl: true, ..Default::default() }, true),
        (Modifiers { ctrl: true, shift: true, ..Default::default() }, false),
    ] {
        let ctx = Ctx { view: &r.view, cursor: DVec2::new(10., 10.), modifiers: mods, scale: 1.0 };
        r.tools.pointer_with_pick(&mut r.editor, &ctx, Pointer::Down { clicks: 1 }, rysmo_infer::Pick::Element(entity));
        r.tools.pointer(&mut r.editor, &ctx, Pointer::Up);
        assert_eq!(r.editor.selection().contains(&entity), selected);
    }
    let ctx = Ctx { view: &r.view, cursor: DVec2::new(10., 10.), modifiers: Modifiers::default(), scale: 1.0 };
    r.tools.pointer_with_pick(&mut r.editor, &ctx, Pointer::Down { clicks: 1 }, rysmo_infer::Pick::Element(entity));
    r.tools.pointer(&mut r.editor, &ctx, Pointer::Up);
    r.tools.pointer(&mut r.editor, &ctx, Pointer::Down { clicks: 1 });
    r.tools.pointer(&mut r.editor, &ctx, Pointer::Up);
    assert!(r.editor.selection().is_empty());
}

#[test]
fn dimension_tool_two_endpoints_exact_offset_preview_commit_and_escape() {
    let mut r = Rig::new(ToolKind::Dimension);
    r.editor
        .apply(rysmo_model::Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None })
        .unwrap();
    r.click(DVec3::ZERO);
    r.click(DVec3::X * 2.);
    r.hover(v(1., 1., 0.));
    assert!(r.tools.preview(&r.editor).dimension.is_some());
    r.type_vcb("500mm").unwrap();
    assert_eq!(r.editor.active().dimensions.len(), 1);
    let dimension = r.editor.active().dimensions.values().next().unwrap();
    assert!((dimension.offset.length() - 0.5).abs() < 1e-8);
    let measured = dimension.resolve(r.editor.doc(), None);
    assert!(measured.associated);
    assert_eq!(measured.length, 2.);
    assert!(!r.tools.busy());
    assert!(r.editor.undo());
    assert!(r.editor.active().dimensions.is_empty());
    r.click(DVec3::ZERO);
    r.key(Key::Escape);
    assert!(!r.tools.busy());
    assert!(r.tools.preview(&r.editor).dimension.is_none());
}
#[test]
fn dimension_tool_edge_and_circle_shortcuts() {
    use rysmo_model::dimensions::DimensionMeasure;
    let mut r = Rig::new(ToolKind::Dimension);
    r.editor
        .apply(rysmo_model::Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None })
        .unwrap();
    r.click(DVec3::X * 0.3);
    assert!(r.tools.preview(&r.editor).dimension.is_some());
    r.click(v(1., 1., 0.));
    assert_eq!(r.editor.active().dimensions.len(), 1);
    let mut r = Rig::new(ToolKind::Dimension);
    r.editor
        .apply(rysmo_model::Command::DrawCircle {
            centre: v(1., 1., 0.),
            normal: DVec3::Z,
            radius: 1.,
            segments: 12,
            start: Some(DVec3::X),
        })
        .unwrap();
    let edge = r.editor.mesh().edges().next().unwrap().0;
    let (a, b) = r.editor.mesh().edge_points(edge);
    r.click((a + b) * 0.5);
    assert!(matches!(
        r.tools.preview(&r.editor).dimension.unwrap().measure,
        DimensionMeasure::Radial { diameter: true, .. }
    ));
    r.type_vcb("1500mm").unwrap();
    let dimension = r.editor.active().dimensions.values().next().unwrap();
    assert_eq!(dimension.resolve(r.editor.doc(), None).length, 2.);
}

#[test]
fn dimension_creation_in_scaled_group_uses_physical_offset() {
    for factor in [0.5, 2.0] {
        let mut r = Rig::new(ToolKind::Dimension);
        r.editor
            .apply(rysmo_model::Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None })
            .unwrap();
        let elements = r.editor.mesh().edges().map(|(id, _)| Element::Edge(id)).collect();
        let out = r.editor.apply(rysmo_model::Command::MakeGroup { elements }).unwrap();
        let Element::Instance(group) = out.created[0] else {
            panic!("group");
        };
        r.editor
            .apply(rysmo_model::Command::Scale {
                elements: vec![Element::Instance(group)],
                origin: DVec3::ZERO,
                factors: DVec3::splat(factor),
            })
            .unwrap();
        r.editor.enter(group).unwrap();
        r.view = r.view.local(r.editor.context_transform());
        r.click(DVec3::ZERO);
        r.click(DVec3::X * 2.);
        r.hover(v(1., 1., 0.));
        r.type_vcb("500mm").unwrap();
        let dimension = r.editor.active().dimensions.values().next().unwrap();
        assert!((r.editor.context_transform().transform_vector3(dimension.offset).length() - 0.5).abs() < 1e-8);
        assert!(dimension.resolve(r.editor.doc(), r.editor.active_definition()).associated);
    }
}
#[test]
fn arc_dimension_creation_defaults_to_radius_and_keeps_association() {
    let mut r = Rig::new(ToolKind::Dimension);
    r.editor
        .apply(rysmo_model::Command::DrawArc {
            start: DVec3::ZERO,
            end: DVec3::X * 2.,
            normal: DVec3::Z,
            bulge: 1.,
            segments: 12,
        })
        .unwrap();
    let edge = r.editor.mesh().edges().next().unwrap().0;
    let (a, b) = r.editor.mesh().edge_points(edge);
    r.click((a + b) * 0.5);
    r.type_vcb("1500mm").unwrap();
    let dimension = r.editor.active().dimensions.values().next().unwrap();
    assert!(matches!(dimension.measure, rysmo_model::dimensions::DimensionMeasure::Radial { diameter: false, .. }));
    assert!(dimension.resolve(r.editor.doc(), None).associated);
}

#[test]
fn dimension_exact_offset_rejects_invalid_draft_without_committing() {
    let mut r = Rig::new(ToolKind::Dimension);
    r.click(DVec3::ZERO);
    r.click(DVec3::X * 2.);
    r.tools.dimension_draft.as_mut().unwrap().offset = DVec3::Y;
    r.tools.dimension_draft.as_mut().unwrap().normal = DVec3::ZERO;
    assert!(r.type_vcb("500mm").is_err());
    assert!(r.editor.active().dimensions.is_empty());
    assert!(r.tools.busy());
    r.key(Key::Escape);
    assert!(!r.tools.busy());
}

#[test]
fn dimension_creation_preview_and_commit_use_model_defaults() {
    let mut r = Rig::new(ToolKind::Dimension);
    let mut style = r.editor.doc().style;
    style.dimension_defaults.font.size = 28.;
    style.dimension_defaults.font.monospace = true;
    style.dimension_defaults.endpoints = rysmo_model::dimensions::DimensionEndpoints::Dot;
    style.dimension_defaults.alignment = rysmo_model::dimensions::DimensionTextAlignment::DimensionLine;
    r.editor.apply(rysmo_model::Command::SetStyle { style }).unwrap();
    r.click(DVec3::ZERO);
    r.click(DVec3::X * 2.);
    let draft = r.tools.preview(&r.editor).dimension.unwrap();
    assert_eq!(draft.font, style.dimension_defaults.font);
    assert_eq!(draft.alignment, style.dimension_defaults.alignment);
    assert_eq!(draft.endpoints, style.dimension_defaults.endpoints);
    r.type_vcb("500mm").unwrap();
    let created = r.editor.active().dimensions.values().next().unwrap();
    assert_eq!(created.font, draft.font);
    assert_eq!(created.endpoints, draft.endpoints);
}

#[test]
fn move_ctrl_copies_selected_dimension_with_precise_offset_and_array_correction() {
    use rysmo_model::dimensions::*;
    let mut r = Rig::new(ToolKind::Move);
    let dimension = Dimension {
        measure: DimensionMeasure::Linear {
            from: DimensionAnchor { point: DVec3::ZERO, reference: None },
            to: DimensionAnchor { point: DVec3::X * 2., reference: None },
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
    let out = r.editor.apply(rysmo_model::Command::AddDimension { dimension }).unwrap();
    r.editor.set_selection(out.created);
    r.mods.ctrl = true;
    r.click(DVec3::ZERO);
    r.hover(DVec3::Y * 2.);
    let before = r.editor.doc().clone();
    let preview = r.tools.preview(&r.editor);
    assert_eq!(preview.moving_dimensions.len(), 1);
    assert!((preview.moving_dimensions[0].offset.y - 3.).abs() < 1e-8);
    assert_eq!(r.editor.doc(), &before);
    r.type_vcb("1000mm").unwrap();
    assert_eq!(r.editor.active().dimensions.len(), 2);
    r.type_vcb("2x").unwrap();
    assert_eq!(r.editor.active().dimensions.len(), 3);
    let mut offsets = r.editor.active().dimensions.values().map(|d| d.offset.y).collect::<Vec<_>>();
    offsets.sort_by(f64::total_cmp);
    assert_eq!(offsets, vec![1., 2., 3.]);
    assert!(r.editor.undo());
    assert_eq!(r.editor.active().dimensions.len(), 1);
}

#[test]
fn escape_discards_dimension_move_preview_without_creating_an_undo_step() {
    use rysmo_model::dimensions::*;
    let mut r = Rig::new(ToolKind::Move);
    let dimension = Dimension {
        measure: DimensionMeasure::Linear {
            from: DimensionAnchor { point: DVec3::ZERO, reference: None },
            to: DimensionAnchor { point: DVec3::X * 2., reference: None },
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
    let out = r.editor.apply(rysmo_model::Command::AddDimension { dimension }).unwrap();
    r.editor.set_selection(out.created);
    let before = r.editor.doc().clone();
    r.click(DVec3::ZERO);
    r.hover(DVec3::Y * 2.);
    assert_eq!(r.tools.preview(&r.editor).moving_dimensions.len(), 1);
    r.key(Key::Escape);
    assert!(r.tools.preview(&r.editor).moving_dimensions.is_empty());
    assert_eq!(r.editor.doc(), &before);
    assert_eq!(r.editor.undo_label(), Some("Dimension"));
}

#[test]
fn rotate_dimension_by_precise_angle_and_correct_the_same_undo_step() {
    use rysmo_model::dimensions::*;
    let mut r = Rig::new(ToolKind::Rotate);
    let dimension = Dimension {
        measure: DimensionMeasure::Linear {
            from: DimensionAnchor { point: DVec3::ZERO, reference: None },
            to: DimensionAnchor { point: DVec3::X * 2., reference: None },
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
    let out = r.editor.apply(rysmo_model::Command::AddDimension { dimension }).unwrap();
    let id = out.dimensions[0];
    r.editor.set_selection(out.created);
    let before = r.editor.doc().clone();
    r.click(DVec3::ZERO);
    r.click(DVec3::X * 2.);
    r.hover(DVec3::Y * 2.);
    let preview = r.tools.preview(&r.editor);
    assert_eq!(preview.moving_dimensions.len(), 1);
    assert!(preview.moving_dimensions[0].resolve(r.editor.doc(), None).to.distance(DVec3::Y * 2.) < 1e-8);
    r.type_vcb("90").unwrap();
    assert!(r.editor.active().dimensions[&id].resolve(r.editor.doc(), None).to.distance(DVec3::Y * 2.) < 1e-8);
    r.type_vcb("45").unwrap();
    assert!(
        r.editor.active().dimensions[&id].resolve(r.editor.doc(), None).to.distance(v(2_f64.sqrt(), 2_f64.sqrt(), 0.))
            < 1e-8
    );
    assert!(r.editor.undo());
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn rotate_ctrl_copies_dimensions_with_precise_angle_and_radial_array() {
    use rysmo_model::dimensions::*;
    let mut r = Rig::new(ToolKind::Rotate);
    let dimension = Dimension {
        measure: DimensionMeasure::Linear {
            from: DimensionAnchor { point: DVec3::X * 2., reference: None },
            to: DimensionAnchor { point: DVec3::X * 4., reference: None },
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
    let out = r.editor.apply(rysmo_model::Command::AddDimension { dimension }).unwrap();
    r.editor.set_selection(out.created);
    let before = r.editor.doc().clone();
    r.mods.ctrl = true;
    r.click(DVec3::ZERO);
    r.click(DVec3::X);
    r.hover(DVec3::Y);
    let preview = r.tools.preview(&r.editor);
    assert_eq!(preview.moving_dimensions.len(), 1);
    assert!(preview.moving_dimensions[0].resolve(r.editor.doc(), None).from.distance(DVec3::Y * 2.) < 1e-8);
    assert_eq!(r.editor.doc(), &before);
    r.type_vcb("90").unwrap();
    assert_eq!(r.editor.active().dimensions.len(), 2);
    r.type_vcb("2x").unwrap();
    assert_eq!(r.editor.active().dimensions.len(), 3);
    assert!(
        r.editor
            .active()
            .dimensions
            .values()
            .any(|d| d.resolve(r.editor.doc(), None).from.distance(-DVec3::X * 2.) < 1e-8)
    );
    assert!(r.editor.undo());
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn scale_selected_dimension_by_grip_precise_factor_and_immediate_correction() {
    use rysmo_model::dimensions::*;
    let mut r = Rig::new(ToolKind::Scale);
    let dimension = Dimension {
        measure: DimensionMeasure::Linear {
            from: DimensionAnchor { point: DVec3::ZERO, reference: None },
            to: DimensionAnchor { point: DVec3::X * 2., reference: None },
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
    let out = r.editor.apply(rysmo_model::Command::AddDimension { dimension }).unwrap();
    let id = out.dimensions[0];
    r.editor.set_selection(out.created);
    let before = r.editor.doc().clone();
    r.hover(v(2., 0.5, 0.));
    assert!(!r.tools.preview(&r.editor).markers.is_empty());
    r.click(v(2., 0.5, 0.));
    r.hover(v(4., 0.5, 0.));
    let preview = r.tools.preview(&r.editor);
    assert_eq!(preview.moving_dimensions.len(), 1);
    assert_eq!(r.editor.doc(), &before);
    r.type_vcb("2").unwrap();
    assert!((r.editor.active().dimensions[&id].resolve(r.editor.doc(), None).length - 4.).abs() < 1e-8);
    r.type_vcb("3").unwrap();
    assert!((r.editor.active().dimensions[&id].resolve(r.editor.doc(), None).length - 6.).abs() < 1e-8);
    assert!(r.editor.undo());
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn dimension_circle_centre_anchor_follows_the_curve_without_becoming_an_edge_point() {
    use rysmo_model::dimensions::*;
    let mut r = Rig::new(ToolKind::Dimension);
    r.editor
        .apply(rysmo_model::Command::DrawCircle {
            centre: v(2., 2., 0.),
            normal: DVec3::Z,
            radius: 0.5,
            segments: 12,
            start: Some(DVec3::X),
        })
        .unwrap();
    let face = r.editor.mesh().faces().next().unwrap().0;
    let curve = r.editor.mesh().curves().next().unwrap().0;
    r.editor
        .apply(rysmo_model::Command::DrawLines { lines: vec![[v(4., 2., 0.), v(4., 3., 0.)]], facing: None })
        .unwrap();
    r.hover(v(2., 2., 0.));
    assert_eq!(r.tools.preview(&r.editor).inference.unwrap().kind, rysmo_infer::Kind::Centre);
    r.click(v(2., 2., 0.));
    r.click(v(4., 2., 0.));
    r.type_vcb("500mm").unwrap();
    let dimension = r.editor.active().dimensions.values().next().unwrap();
    let DimensionMeasure::Linear { from, .. } = &dimension.measure else { panic!("linear") };
    assert_eq!(from.reference.as_ref().unwrap().entity, PointEntity::CurveCentre { curve });
    assert!((dimension.resolve(r.editor.doc(), None).length - 2.).abs() < 1e-8);
    r.editor.apply(rysmo_model::Command::Move { elements: vec![Element::Face(face)], offset: DVec3::X }).unwrap();
    let measurement = r.editor.active().dimensions.values().next().unwrap().resolve(r.editor.doc(), None);
    assert!(measurement.associated);
    assert!((measurement.length - 1.).abs() < 1e-8);
}

#[test]
fn dimension_nested_circle_centre_keeps_instance_path_and_follows_group_move() {
    use rysmo_model::dimensions::*;
    let mut r = Rig::new(ToolKind::Dimension);
    r.editor
        .apply(rysmo_model::Command::DrawCircle {
            centre: v(2., 2., 0.),
            normal: DVec3::Z,
            radius: 0.5,
            segments: 12,
            start: Some(DVec3::X),
        })
        .unwrap();
    let face = r.editor.mesh().faces().next().unwrap().0;
    let out = r.editor.apply(rysmo_model::Command::MakeGroup { elements: vec![Element::Face(face)] }).unwrap();
    let Element::Instance(group) = out.created[0] else { panic!("group") };
    r.editor
        .apply(rysmo_model::Command::DrawLines { lines: vec![[v(4., 2., 0.), v(4., 3., 0.)]], facing: None })
        .unwrap();
    r.click(v(2., 2., 0.));
    r.click(v(4., 2., 0.));
    r.type_vcb("500mm").unwrap();
    let dimension = r.editor.active().dimensions.values().next().unwrap();
    let DimensionMeasure::Linear { from, .. } = &dimension.measure else { panic!("linear") };
    assert_eq!(from.reference.as_ref().unwrap().path, vec![group]);
    assert!(matches!(from.reference.as_ref().unwrap().entity, PointEntity::CurveCentre { .. }));
    r.editor.apply(rysmo_model::Command::Move { elements: vec![Element::Instance(group)], offset: DVec3::X }).unwrap();
    let measured = r.editor.active().dimensions.values().next().unwrap().resolve(r.editor.doc(), None);
    assert!(measured.associated);
    assert!((measured.length - 1.).abs() < 1e-8);
}

#[test]
fn dimension_arc_centre_anchor_follows_arc_translation() {
    use rysmo_model::dimensions::*;
    let mut r = Rig::new(ToolKind::Dimension);
    r.editor
        .apply(rysmo_model::Command::DrawArc {
            start: DVec3::ZERO,
            end: DVec3::X * 2.,
            normal: DVec3::Z,
            bulge: 1.,
            segments: 12,
        })
        .unwrap();
    let curve = r.editor.mesh().curves().next().unwrap().0;
    r.editor
        .apply(rysmo_model::Command::DrawLines { lines: vec![[DVec3::X * 3., v(3., 1., 0.)]], facing: None })
        .unwrap();
    r.hover(DVec3::X);
    assert_eq!(r.tools.preview(&r.editor).inference.unwrap().kind, rysmo_infer::Kind::Centre);
    r.click(DVec3::X);
    r.click(DVec3::X * 3.);
    r.type_vcb("500mm").unwrap();
    let elements = r.editor.mesh().curve_edges(curve).into_iter().map(Element::Edge).collect();
    r.editor.apply(rysmo_model::Command::Move { elements, offset: DVec3::Y }).unwrap();
    let dimension = r.editor.active().dimensions.values().next().unwrap();
    let measured = dimension.resolve(r.editor.doc(), None);
    assert!(measured.associated);
    assert!(measured.from.distance(v(1., 1., 0.)) < 1e-8);
    assert!((measured.length - 5_f64.sqrt()).abs() < 1e-8);
    let DimensionMeasure::Linear { from, .. } = &dimension.measure else { panic!("linear") };
    assert_eq!(from.reference.as_ref().unwrap().entity, PointEntity::CurveCentre { curve });
}

#[test]
fn group_owned_dimension_previews_are_emitted_for_move_rotate_and_scale_and_cancel_cleanly() {
    use rysmo_model::dimensions::*;
    for kind in [ToolKind::Move, ToolKind::Rotate, ToolKind::Scale] {
        let mut r = Rig::new(kind);
        r.editor
            .apply(rysmo_model::Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 2.]], facing: None })
            .unwrap();
        let edge = r.editor.mesh().edges().next().unwrap().0;
        let dimension = Dimension {
            measure: DimensionMeasure::Linear {
                from: DimensionAnchor { point: DVec3::ZERO, reference: None },
                to: DimensionAnchor { point: DVec3::X * 2., reference: None },
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
        let id = r.editor.apply(rysmo_model::Command::AddDimension { dimension }).unwrap().dimensions[0];
        let out = r
            .editor
            .apply(rysmo_model::Command::MakeGroup { elements: vec![Element::Edge(edge), Element::Dimension(id)] })
            .unwrap();
        r.editor.set_selection(out.created);
        let before = r.editor.doc().clone();
        match kind {
            ToolKind::Move => {
                r.click(DVec3::ZERO);
                r.hover(DVec3::Y * 2.);
            }
            ToolKind::Rotate => {
                r.click(DVec3::ZERO);
                r.click(DVec3::X * 2.);
                r.hover(DVec3::Y * 2.);
            }
            _ => {
                r.hover(v(2., 0.5, 0.));
                r.click(v(2., 0.5, 0.));
                r.hover(v(4., 0.5, 0.));
            }
        }
        assert_eq!(r.tools.preview(&r.editor).instanced_dimensions.len(), 1);
        assert_eq!(r.editor.doc(), &before);
        r.key(Key::Escape);
        assert!(r.tools.preview(&r.editor).instanced_dimensions.is_empty());
        assert_eq!(r.editor.doc(), &before);
    }
}

#[test]
fn moving_group_preview_contains_its_actual_visible_edge_not_only_the_box() {
    let mut r = Rig::new(ToolKind::Move);
    r.editor
        .apply(rysmo_model::Command::DrawLines { lines: vec![[DVec3::ZERO, v(1., 1., 0.)]], facing: None })
        .unwrap();
    let edge = r.editor.mesh().edges().next().unwrap().0;
    let out = r.editor.apply(rysmo_model::Command::MakeGroup { elements: vec![Element::Edge(edge)] }).unwrap();
    r.editor.set_selection(out.created);
    let before = r.editor.doc().clone();
    r.click(DVec3::ZERO);
    r.hover(DVec3::Y * 2.);
    assert!(
        r.tools
            .preview(&r.editor)
            .lines
            .iter()
            .any(|line| (line.a.distance(v(0., 2., 0.)) < 1e-8 && line.b.distance(v(1., 3., 0.)) < 1e-8)
                || (line.b.distance(v(0., 2., 0.)) < 1e-8 && line.a.distance(v(1., 3., 0.)) < 1e-8))
    );
    assert_eq!(r.editor.doc(), &before);
}

#[test]
fn drawing_facing_uses_the_offset_canvas_centre() {
    let mut r = Rig::new(ToolKind::Line);
    r.view.offset = DVec2::new(200., 150.);
    let ctx = Ctx { view: &r.view, cursor: DVec2::ZERO, modifiers: Default::default(), scale: 1. };
    let expected = r.view.view.inverse().transform_vector3(DVec3::Z).normalize();
    assert!(crate::tools::ToolBox::facing(&ctx).distance(expected) < 1e-9);
}

#[test]
fn welded_curve_selects_together_and_exploded_edges_select_independently() {
    let mut rig = Rig::new(ToolKind::Select);
    rig.editor
        .apply(rysmo_model::Command::DrawLines {
            lines: vec![[DVec3::ZERO, DVec3::X], [DVec3::X, DVec3::X + DVec3::Y]],
            facing: None,
        })
        .unwrap();
    let edges: Vec<_> = rig.editor.mesh().edges().map(|(id, _)| id).collect();
    rig.editor.apply(rysmo_model::Command::WeldEdges { edges: edges.clone() }).unwrap();
    let cursor = rig.px(DVec3::X * 0.5);
    let ctx = Ctx { view: &rig.view, cursor, modifiers: rig.mods, scale: 1. };
    rig.tools.pointer_with_pick(
        &mut rig.editor,
        &ctx,
        Pointer::Down { clicks: 1 },
        rysmo_infer::Pick::Element(Element::Edge(edges[0])),
    );
    assert_eq!(rig.editor.selection().len(), 2);
    rig.tools.pointer_with_pick(
        &mut rig.editor,
        &ctx,
        Pointer::Up,
        rysmo_infer::Pick::Element(Element::Edge(edges[0])),
    );
    rig.editor.apply(rysmo_model::Command::ExplodeCurves { edges: vec![edges[0]] }).unwrap();
    rig.editor.clear_selection();
    rig.tools.pointer_with_pick(
        &mut rig.editor,
        &ctx,
        Pointer::Down { clicks: 1 },
        rysmo_infer::Pick::Element(Element::Edge(edges[0])),
    );
    assert_eq!(rig.editor.selected(), vec![Element::Edge(edges[0])]);
}
