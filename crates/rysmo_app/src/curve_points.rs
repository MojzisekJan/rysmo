//! Exact coordinate editing of grouped polylines with a read-only preview.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::DVec3;
use rysmo_mesh::CurveId;
use rysmo_model::{Command, Element, InstanceId, VertexId, parse_length};
pub struct Draft {
    curve: CurveId,
    vertices: Vec<VertexId>,
    index: usize,
    coordinates: [String; 3],
    revision: u64,
    generation: u64,
    context: Vec<InstanceId>,
    cached: Option<(VertexId, [String; 3], Result<rysmo_model::Mesh, String>)>,
}
impl Draft {
    fn valid(&self, app: &App3d) -> bool {
        self.revision == app.editor.revision()
            && self.generation == app.document_generation
            && self.context == app.editor.context()
    }
    fn load(&mut self, app: &App3d) {
        self.coordinates = app.editor.mesh().pos(self.vertices[self.index]).to_array().map(|n| format!("{n}m"));
    }
    fn position(&self, app: &App3d) -> Result<DVec3, String> {
        let mut values = [0.; 3];
        for (value, text) in values.iter_mut().zip(&self.coordinates) {
            *value = parse_length(text, app.editor.doc().units).map_err(|e| e.to_string())?;
        }
        Ok(DVec3::from_array(values))
    }
    fn update(&mut self, app: &App3d) {
        let vertex = self.vertices[self.index];
        if self.cached.as_ref().is_some_and(|(old, text, _)| *old == vertex && *text == self.coordinates) {
            return;
        }
        let result = self.position(app).and_then(|position| {
            rysmo_model::curve_points::preview(app.editor.mesh(), self.curve, vertex, position)
                .map_err(|e| e.to_string())
        });
        self.cached = Some((vertex, self.coordinates.clone(), result));
    }
}
fn selected_curve(app: &App3d) -> Option<CurveId> {
    let mut curve = None;
    for item in app.editor.selection() {
        let Element::Edge(edge) = item else {
            return None;
        };
        let id = app.editor.mesh().edge(*edge)?.curve?;
        if curve.is_some_and(|old| old != id) {
            return None;
        }
        curve = Some(id);
    }
    let id = curve?;
    (app.editor.mesh().curve(id)?.kind == rysmo_mesh::CurveKind::Polyline).then_some(id)
}
pub fn available(app: &App3d) -> bool {
    selected_curve(app).is_some()
}
pub fn begin(app: &mut App3d) {
    let Some(curve) = selected_curve(app) else {
        app.message = Some("Select edges from one grouped polyline.".into());
        return;
    };
    let vertices = match rysmo_model::curve_points::vertices(app.editor.mesh(), curve) {
        Ok(v) => v,
        Err(e) => {
            app.message = Some(e.to_string());
            return;
        }
    };
    app.set_tool(rysmo_tools::ToolKind::Select);
    let mut draft = Draft {
        curve,
        vertices,
        index: 0,
        coordinates: Default::default(),
        revision: app.editor.revision(),
        generation: app.document_generation,
        context: app.editor.context().to_vec(),
        cached: None,
    };
    draft.load(app);
    draft.update(app);
    app.curve_points = Some(draft);
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(mut draft) = app.curve_points.take().filter(|d| d.valid(app)) else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    let response = crate::theme::window("Edit Curve Points").vscroll(true).beside_model().open(&mut open).default_width(370.).show(ctx, |ui| {
        let old = draft.index;
        egui::ComboBox::from_label("Point").selected_text(format!("{} of {}", draft.index + 1, draft.vertices.len())).show_ui(ui, |ui| { for index in 0..draft.vertices.len() { ui.selectable_value(&mut draft.index, index, format!("Point {}", index + 1)); } });
        if draft.index != old { draft.load(app); }
        ui.label("Coordinates in the current editing context");
        for (axis, text) in ["X", "Y", "Z"].into_iter().zip(&mut draft.coordinates) { ui.horizontal(|ui| { ui.label(axis); ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY)); }); }
        draft.update(app);
        let valid = draft.cached.as_ref().is_some_and(|(_, _, result)| result.is_ok());
        if let Some((_, _, Err(error))) = &draft.cached { ui.colored_label(egui::Color32::LIGHT_RED, error); }
        ui.small("Preserves curve and vertex IDs. Merging vertices and bending faces is refused. Each Apply is one Undo step. Escape cancels the preview.");
        ui.horizontal(|ui| { apply = ui.add_enabled(valid, egui::Button::new("Apply point")).clicked(); cancel = ui.button("Close").clicked(); });
    });
    crate::features::remember(app, &response);
    if apply && let Ok(position) = draft.position(app) {
        match app.editor.apply(Command::SetCurvePoint {
            curve: draft.curve,
            vertex: draft.vertices[draft.index],
            position,
        }) {
            Ok(_) => {
                draft.revision = app.editor.revision();
                draft.cached = None;
                draft.load(app);
                draft.update(app);
            }
            Err(error) => app.message = Some(error.to_string()),
        }
    }
    if open && !cancel {
        app.curve_points = Some(draft);
    }
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(draft) = app.curve_points.as_ref().filter(|d| d.valid(app)) else {
        return;
    };
    let Some((_, _, Ok(mesh))) = &draft.cached else {
        return;
    };
    let view = app.local_view();
    let color = egui::Color32::from_rgb(90, 220, 160);
    for edge in mesh.curve_edges(draft.curve) {
        let (a, b) = mesh.edge_points(edge);
        if let (Some(a), Some(b)) = (view.project(a), view.project(b)) {
            painter.line_segment(
                [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                egui::Stroke::new(2., color),
            );
        }
    }
    if let Some(point) = view.project(mesh.pos(draft.vertices[draft.index])) {
        painter.circle_filled(egui::pos2(point.x as f32, point.y as f32), 5., color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancel_and_stale_curve_drafts_do_not_mutate_the_document() {
        let mut editor = rysmo_model::Editor::new();
        let out = editor
            .apply(Command::DrawLines {
                lines: vec![[DVec3::ZERO, DVec3::X], [DVec3::X, DVec3::new(2., 1., 0.)]],
                facing: None,
            })
            .unwrap();
        let edges =
            out.created.iter().filter_map(|e| if let Element::Edge(id) = e { Some(*id) } else { None }).collect();
        editor.apply(Command::WeldEdges { edges }).unwrap();
        editor.set_selection(out.created);
        let mut app = App3d::new(editor);
        let before = app.editor.doc().clone();
        begin(&mut app);
        assert!(app.curve_points.is_some());
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.curve_points.is_none());
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        app.document_generation += 1;
        assert!(!app.curve_points.as_ref().unwrap().valid(&app));
        assert_eq!(app.editor.doc(), &before);
    }
}
