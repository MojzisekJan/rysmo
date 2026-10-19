//! From Scratch: spacing, origin, baseline and perpendicular width.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_infer::Lock;
use rysmo_math::{DVec2, DVec3, Plane};
use rysmo_model::{Command, InstanceId};
use rysmo_tools::{Key, Pointer, ToolKind};
pub struct LastGrid {
    origin: DVec3,
    u: DVec3,
    v: DVec3,
    spacing: f64,
    context: Vec<InstanceId>,
    revision: u64,
}
impl LastGrid {
    pub fn dimensions(&self) -> (f64, f64) {
        (self.u.length(), self.v.length())
    }
    pub(crate) fn valid(&self, app: &App3d) -> bool {
        self.context == app.editor.context() && self.revision == app.editor.revision()
    }
}
pub fn correct(app: &mut App3d, text: &str) -> Result<(), String> {
    let last = app.terrain_last.as_ref().ok_or("no terrain to correct")?;
    if !last.valid(app) {
        app.terrain_last = None;
        return Err("The model changed; create a new terrain to correct its dimensions.".into());
    }
    let values = text
        .split(',')
        .map(|s| rysmo_model::parse_length(s.trim(), app.editor.doc().units).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() || values.len() > 2 || values.iter().any(|v| *v < 1e-5) {
        return Err("Enter width, or length,width.".into());
    }
    let u = if values.len() == 2 { last.u.normalize() * values[0] } else { last.u };
    let v = last.v.normalize() * values[values.len() - 1];
    let origin = last.origin;
    let spacing = last.spacing;
    let context = last.context.clone();
    let out = app.editor.amend_last(Command::CreateTerrainGrid { origin, u, v, spacing }).map_err(|e| e.to_string())?;
    app.editor.set_selection(out.created);
    app.terrain_last = Some(LastGrid { origin, u, v, spacing, context, revision: app.editor.revision() });
    Ok(())
}
pub struct Draft {
    pub start: Option<DVec3>,
    pub baseline: Option<DVec3>,
    pub current: DVec3,
    pub spacing: f64,
    axis: Option<rysmo_math::Axis>,
    shift: Option<DVec3>,
    context: Vec<InstanceId>,
    revision: u64,
}
impl Draft {
    pub(crate) fn valid(&self, app: &App3d) -> bool {
        self.context == app.editor.context() && self.revision == app.editor.revision()
    }
    pub fn measurement(&self) -> (&'static str, f64) {
        match (self.start, self.baseline) {
            (None, _) => ("Grid Spacing", self.spacing),
            (Some(start), None) => ("Length", self.current.distance(start)),
            (Some(start), Some(u)) => ("Width", (self.current - start - u).length()),
        }
    }
    pub fn status(&self) -> &'static str {
        if self.start.is_none() {
            "From Scratch: enter grid spacing, then click the origin."
        } else if self.baseline.is_none() {
            "From Scratch: click the length endpoint or enter a length."
        } else {
            "From Scratch: click the perpendicular width or enter a width. Escape cancels."
        }
    }
}
pub fn begin(app: &mut App3d) {
    app.set_tool(ToolKind::Move);
    app.terrain = Some(Draft {
        axis: None,
        shift: None,
        start: None,
        baseline: None,
        current: app.editor.active().axes.origin,
        spacing: app.terrain_spacing,
        context: app.editor.context().to_vec(),
        revision: app.editor.revision(),
    });
}
fn advance(app: &mut App3d) -> Result<(), String> {
    let draft = app.terrain.as_mut().ok_or("no terrain draft")?;
    if draft.start.is_none() {
        draft.start = Some(draft.current);
        return Ok(());
    }
    let start = draft.start.unwrap();
    if draft.baseline.is_none() {
        let u = draft.current - start;
        if u.length() < 1e-5 {
            return Err("Terrain length must be positive.".into());
        }
        draft.baseline = Some(u);
        draft.axis = None;
        draft.shift = None;
        return Ok(());
    }
    let u = draft.baseline.unwrap();
    let normal = app.editor.active().axes.z();
    let direction = normal.cross(u).normalize();
    let v = direction * (draft.current - start).dot(direction);
    if v.length() < 1e-5 {
        return Err("Terrain width must be positive.".into());
    }
    let spacing = draft.spacing;
    app.editor
        .apply(Command::CreateTerrainGrid { origin: start, u, v, spacing })
        .map(|out| app.editor.set_selection(out.created))
        .map_err(|e| e.to_string())?;
    app.terrain = None;
    app.set_tool(ToolKind::Select);
    app.terrain_last = Some(LastGrid {
        origin: start,
        u,
        v,
        spacing,
        context: app.editor.context().to_vec(),
        revision: app.editor.revision(),
    });
    Ok(())
}
pub fn pointer(app: &mut App3d, at: DVec2, event: Pointer) {
    if !app.terrain.as_ref().is_some_and(|d| d.valid(app)) {
        app.terrain = None;
        return;
    }
    let axes = app.editor.active().axes;
    let d = app.terrain.as_ref().unwrap();
    let plane = Plane::new(axes.z(), d.start.unwrap_or(axes.origin));
    let from = d.start.map(|start| start + d.baseline.unwrap_or(DVec3::ZERO));
    let direction = d.baseline.map(|u| axes.z().cross(u).normalize()).or(d.shift).or(d.axis.map(|a| axes.dir(a)));
    let lock = match (from, direction) {
        (Some(origin), Some(dir)) => Some(Lock::Line { origin, dir, axis: None }),
        _ if d.start.is_some() => Some(Lock::Plane(plane)),
        _ => None,
    };
    let (start, baseline) = (d.start, d.baseline);
    let inf = app.with_ctx(at, |tools, editor, ctx| tools.draft_inference(editor, ctx, from, lock));
    let point = Some(if start.is_some() { plane.project(inf.point) } else { inf.point });
    if let Some(mut p) = point {
        if let (Some(start), Some(u)) = (start, baseline) {
            let dir = axes.z().cross(u).normalize();
            p = start + u + dir * (p - start).dot(dir);
        }
        app.terrain.as_mut().unwrap().current = p;
    }
    if matches!(event, Pointer::Down { .. }) {
        app.message = advance(app).err();
    }
}
pub fn key(app: &mut App3d, key: Key) {
    let Some(d) = &mut app.terrain else { return };
    match key {
        Key::Escape => {
            app.terrain = None;
            return;
        }
        Key::Right | Key::Left if d.start.is_some() && d.baseline.is_none() => {
            let axis = if key == Key::Right { rysmo_math::Axis::X } else { rysmo_math::Axis::Y };
            d.axis = if d.axis == Some(axis) { None } else { Some(axis) };
        }
        Key::Down => d.axis = None,
        Key::Shift(true) if d.baseline.is_none() => {
            d.shift = d.start.and_then(|start| (d.current - start).try_normalize());
        }
        Key::Shift(false) => d.shift = None,
        _ => {}
    }
    let at = app.cursor.unwrap_or(app.viewport * 0.5);
    pointer(app, at, Pointer::Move);
}
pub fn value(app: &mut App3d, text: &str) -> Result<(), String> {
    if !app.terrain.as_ref().is_some_and(|d| d.valid(app)) {
        app.terrain = None;
        return Err("Terrain creation cancelled because the model changed.".into());
    }
    let value = rysmo_model::parse_length(text, app.editor.doc().units).map_err(|e| e.to_string())?;
    if value < 1e-5 {
        return Err("Enter a positive length.".into());
    }
    let axes = app.editor.active().axes;
    let d = app.terrain.as_mut().unwrap();
    if let Some(start) = d.start {
        let dir = if let Some(u) = d.baseline {
            let dir = axes.z().cross(u).normalize();
            dir * if (d.current - start).dot(dir) < 0. { -1. } else { 1. }
        } else {
            (d.current - start).try_normalize().unwrap_or(axes.x)
        };
        d.current = start + d.baseline.unwrap_or(DVec3::ZERO) + dir * value;
        advance(app)
    } else {
        d.spacing = value;
        app.terrain_spacing = value;
        Ok(())
    }
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(d) = &app.terrain else { return };
    if !d.valid(app) {
        return;
    }
    let Some(start) = d.start else { return };
    let view = app.local_view();
    let line = |a: DVec3, b: DVec3| {
        if let (Some(a), Some(b)) = (view.project(a), view.project(b)) {
            painter.line_segment(
                [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                egui::Stroke::new(1.3, egui::Color32::MAGENTA),
            );
        }
    };
    let u = d.baseline.unwrap_or(d.current - start);
    let length = u.length();
    if length < 1e-5 {
        return;
    }
    let v = if d.baseline.is_some() { d.current - start - u } else { DVec3::ZERO };
    let width = v.length();
    line(start, start + u);
    line(start + u, start + u + v);
    if width > 1e-5 {
        line(start, start + v);
        line(start + v, start + v + u);
    }
    let cols = (length / d.spacing).ceil().min(512.) as usize;
    let rows = (width / d.spacing).ceil().min(512.) as usize;
    for col in 0..=cols {
        let p = start + u * (col as f64 * d.spacing / length).min(1.);
        if width > 1e-5 {
            line(p, p + v);
        } else {
            let tick = app.editor.active().axes.z().cross(u).normalize() * d.spacing * 0.06;
            line(p - tick, p + tick);
        }
    }
    if width > 1e-5 {
        for row in 0..=rows {
            let p = start + v * (row as f64 * d.spacing / width).min(1.);
            line(p, p + u);
        }
    } else {
        line(start, start + u);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_model::{Editor, Element};
    fn click(app: &mut App3d, point: DVec3) {
        let at = app.view().project(point).unwrap();
        pointer(app, at, Pointer::Down { clicks: 1 });
    }
    #[test]
    fn terrain_can_start_at_an_inferred_elevated_point() {
        let mut app = App3d::new(Editor::new());
        let start = DVec3::new(3., 2., 5.);
        app.editor.apply(Command::AddGuidePoint { point: start }).unwrap();
        begin(&mut app);
        click(&mut app, start);
        assert!(app.terrain.as_ref().unwrap().start.unwrap().distance(start) < 1e-9);
        value(&mut app, "2m").unwrap();
        value(&mut app, "1m").unwrap();
        let Element::Instance(id) = app.editor.selected()[0] else { panic!() };
        assert!(app.editor.active().instances[&id].transform.transform_point3(DVec3::ZERO).distance(start) < 1e-9);
    }
    #[test]
    fn immediate_correction_replaces_creation_and_failures_preserve_history() {
        let mut app = App3d::new(Editor::new());
        let before = app.editor.doc().clone();
        begin(&mut app);
        value(&mut app, "50cm").unwrap();
        click(&mut app, DVec3::ZERO);
        value(&mut app, "2m").unwrap();
        value(&mut app, "1m").unwrap();
        correct(&mut app, "2m").unwrap();
        correct(&mut app, "3m,2m").unwrap();
        let Element::Instance(id) = app.editor.selected()[0] else { panic!() };
        let mesh = &app.editor.doc().definitions[&app.editor.active().instances[&id].definition].mesh;
        assert_eq!(mesh.faces().count(), 48);
        let corrected = app.editor.doc().clone();
        let revision = app.editor.revision();
        let selected = app.editor.selected();
        for invalid in ["bad", "0", "10000m,10000m"] {
            assert!(correct(&mut app, invalid).is_err());
        }
        assert_eq!(app.editor.doc(), &corrected);
        assert_eq!(app.editor.revision(), revision);
        assert_eq!(app.editor.selected(), selected);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
        assert_eq!(app.editor.undo_label(), None);
        assert!(correct(&mut app, "1m").is_err());
    }
    #[test]
    fn axis_and_shift_locks_use_the_chosen_start() {
        let mut app = App3d::new(Editor::new());
        begin(&mut app);
        let start = DVec3::new(3., 2., 0.);
        click(&mut app, start);
        let at = app.view().project(DVec3::new(7., 4., 0.)).unwrap();
        app.cursor = Some(at);
        key(&mut app, Key::Right);
        pointer(&mut app, at, Pointer::Move);
        assert!((app.terrain.as_ref().unwrap().current.y - start.y).abs() < 1e-9);
        key(&mut app, Key::Shift(true));
        key(&mut app, Key::Down);
        let at = app.view().project(DVec3::new(5., 8., 0.)).unwrap();
        app.cursor = Some(at);
        pointer(&mut app, at, Pointer::Move);
        assert!((app.terrain.as_ref().unwrap().current.y - start.y).abs() < 1e-9);
        key(&mut app, Key::Shift(false));
        pointer(&mut app, at, Pointer::Move);
        assert!((app.terrain.as_ref().unwrap().current.y - start.y).abs() > 1.);
        assert_eq!(app.editor.undo_label(), None);
    }
    #[test]
    fn negative_width_keeps_front_facing_and_context_changes_cancel() {
        let mut app = App3d::new(Editor::new());
        begin(&mut app);
        value(&mut app, "1m").unwrap();
        click(&mut app, DVec3::ZERO);
        click(&mut app, DVec3::X * 2.);
        click(&mut app, DVec3::new(2., -2., 0.));
        let Element::Instance(id) = app.editor.selected()[0] else { panic!() };
        let instance = &app.editor.active().instances[&id];
        let mesh = &app.editor.doc().definitions[&instance.definition].mesh;
        for (face, _) in mesh.faces() {
            assert!(instance.transform.transform_vector3(mesh.face_normal(face).unwrap()).dot(DVec3::Z) > 0.999);
        }
        begin(&mut app);
        app.editor.enter(id).unwrap();
        let before = app.editor.doc().clone();
        assert!(value(&mut app, "1m").is_err());
        assert!(app.terrain.is_none());
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        crate::files::new(&mut app);
        assert!(app.terrain.is_none());
    }
    #[test]
    fn three_click_grid_uses_spacing_preview_and_one_undo() {
        let mut app = App3d::new(Editor::new());
        let before = app.editor.doc().clone();
        begin(&mut app);
        value(&mut app, "1m").unwrap();
        click(&mut app, DVec3::ZERO);
        click(&mut app, DVec3::X * 3.);
        let at = app.view().project(DVec3::new(3., 2., 0.)).unwrap();
        pointer(&mut app, at, Pointer::Move);
        assert_eq!(app.editor.doc(), &before);
        assert_eq!(app.editor.undo_label(), None);
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert!(app.terrain.is_none());
        assert_eq!(app.editor.undo_label(), Some("From Scratch"));
        let Element::Instance(id) = app.editor.selected()[0] else { panic!() };
        let mesh = &app.editor.doc().definitions[&app.editor.active().instances[&id].definition].mesh;
        assert_eq!(mesh.faces().count(), 12);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn exact_lengths_cancellation_and_invalid_spacing() {
        let mut app = App3d::new(Editor::new());
        let before = app.editor.doc().clone();
        begin(&mut app);
        assert!(value(&mut app, "0").is_err());
        assert!(value(&mut app, "bad").is_err());
        value(&mut app, "50cm").unwrap();
        click(&mut app, DVec3::ZERO);
        value(&mut app, "2m").unwrap();
        value(&mut app, "1m").unwrap();
        let Element::Instance(id) = app.editor.selected()[0] else { panic!() };
        let mesh = &app.editor.doc().definitions[&app.editor.active().instances[&id].definition].mesh;
        assert_eq!(mesh.faces().count(), 16);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        click(&mut app, DVec3::ZERO);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.terrain.is_none());
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        app.set_tool(ToolKind::Line);
        assert!(app.terrain.is_none());
    }
}
