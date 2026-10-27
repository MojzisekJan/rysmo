//! Equal edge subdivisions with a bounded model-space point preview.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_model::{Command, EdgeId, Element, InstanceId};

pub struct Draft {
    edges: Vec<EdgeId>,
    segments: u32,
    revision: u64,
    generation: u64,
    context: Vec<InstanceId>,
}
impl Draft {
    fn valid(&self, app: &App3d) -> bool {
        self.revision == app.editor.revision()
            && self.generation == app.document_generation
            && self.context == app.editor.context()
    }
}
pub fn available(app: &App3d) -> bool {
    let selection = app.editor.selection();
    !selection.is_empty()
        && selection.len() <= 4096
        && selection.iter().all(|element| {
            let Element::Edge(id) = element else { return false };
            app.editor.mesh().edge(*id).is_some_and(|edge| !edge.procedural && edge.curve.is_none())
        })
}
pub fn begin(app: &mut App3d) {
    let selection = app.editor.selected();
    let edges: Vec<_> =
        selection.iter().filter_map(|e| if let Element::Edge(id) = e { Some(*id) } else { None }).collect();
    if edges.is_empty() || edges.len() != selection.len() {
        app.message = Some("Select only straight edges to divide.".into());
        return;
    }
    if let Err(error) = app.editor.mesh().divide_edges_plan(&edges, 2) {
        app.message = Some(error.to_string());
        return;
    }
    app.set_tool(rysmo_tools::ToolKind::Select);
    app.divide = Some(Draft {
        edges,
        segments: 2,
        revision: app.editor.revision(),
        generation: app.document_generation,
        context: app.editor.context().to_vec(),
    });
}
fn apply(app: &mut App3d, draft: &Draft) -> bool {
    if !draft.valid(app) {
        return false;
    }
    match app.editor.apply(Command::DivideEdges { edges: draft.edges.clone(), segments: draft.segments }) {
        Ok(out) => {
            app.editor.set_selection(out.created);
            app.message = Some("Edges divided. Undo restores the original geometry.".into());
            true
        }
        Err(error) => {
            app.message = Some(error.to_string());
            false
        }
    }
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(mut draft) = app.divide.take() else { return };
    if !draft.valid(app) {
        return;
    }
    let mut open = true;
    let mut commit = false;
    let mut cancel = false;
    crate::theme::window("Divide Edges").beside_model().open(&mut open).default_width(360.).show(ctx, |ui| {
        ui.label("Divide each selected straight edge into equal segments.");
        ui.horizontal(|ui| {
            ui.label("Segments per edge");
            ui.add(crate::widgets::Number::new(&mut draft.segments).range(2..=512));
        });
        let plan = app.editor.mesh().divide_edges_plan(&draft.edges, draft.segments);
        match &plan {
            Ok(_) => {
                ui.label(format!(
                    "{} edges → {} segments",
                    draft.edges.len(),
                    draft.edges.len() * draft.segments as usize
                ));
                if draft.edges.len() == 1 {
                    let (a, b) = app.editor.mesh().edge_points(draft.edges[0]);
                    ui.label(format!(
                        "Segment length: {}",
                        app.editor.doc().format_length(a.distance(b) / draft.segments as f64)
                    ));
                }
            }
            Err(error) => {
                ui.colored_label(egui::Color32::from_rgb(255, 170, 110), error.to_string());
            }
        }
        ui.small(
            "Preview points become joined edge endpoints. Adjacent faces and materials are retained. Escape cancels.",
        );
        ui.horizontal(|ui| {
            commit = ui.add_enabled(plan.is_ok(), egui::Button::new("Divide")).clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if commit {
        if !apply(app, &draft) {
            app.divide = Some(draft);
        }
    } else if open && !cancel {
        app.divide = Some(draft);
    }
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(draft) = app.divide.as_ref().filter(|d| d.valid(app)) else { return };
    let Ok(plan) = app.editor.mesh().divide_edges_plan(&draft.edges, draft.segments) else { return };
    let view = app.local_view();
    for division in plan {
        for point in division.points {
            if let Some(point) = view.project(point) {
                painter.circle_filled(
                    egui::pos2(point.x as f32, point.y as f32),
                    4.,
                    egui::Color32::from_rgb(80, 220, 150),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    fn fixture() -> App3d {
        let mut editor = rysmo_model::Editor::new();
        let out = editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X * 8.]], facing: None }).unwrap();
        editor.set_selection(out.created);
        App3d::new(editor)
    }
    #[test]
    fn divide_action_is_one_undo_and_cancel_or_stale_preview_does_not_edit() {
        let mut app = fixture();
        let before = app.editor.doc().clone();
        assert!(available(&app));
        begin(&mut app);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.divide.is_none());
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        let draft = app.divide.take().unwrap();
        app.document_generation += 1;
        assert!(!apply(&mut app, &draft));
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        let mut draft = app.divide.take().unwrap();
        draft.segments = 4;
        let depth = app.editor.undo_depth();
        assert!(apply(&mut app, &draft));
        assert_eq!(app.editor.selection().len(), 4);
        assert_eq!(app.editor.undo_depth(), depth + 1);
        assert!(app.editor.undo());
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn changing_tool_or_document_invalidates_the_draft_and_selection_controls_availability() {
        let mut app = fixture();
        begin(&mut app);
        app.set_tool(rysmo_tools::ToolKind::Line);
        assert!(app.divide.is_none());
        begin(&mut app);
        let draft = app.divide.take().unwrap();
        app.editor.apply(Command::DrawLines { lines: vec![[DVec3::Y, DVec3::Y + DVec3::X]], facing: None }).unwrap();
        let before = app.editor.doc().clone();
        assert!(!apply(&mut app, &draft));
        assert_eq!(app.editor.doc(), &before);
        app.editor.set_selection([]);
        assert!(!available(&app));
    }
}
