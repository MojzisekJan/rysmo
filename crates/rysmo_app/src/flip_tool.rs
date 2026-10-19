//! Hover/pick even soft terrain edges, without exposing occluded geometry.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DVec2, distance_point_segment_2d};
use rysmo_mesh::EdgeId;
use rysmo_model::{Command, InstanceId};
use rysmo_tools::{Pointer, ToolKind};
pub struct State {
    pub candidate: Option<EdgeId>,
    context: Vec<InstanceId>,
    revision: u64,
}
impl State {
    pub(crate) fn valid(&self, app: &App3d) -> bool {
        self.context == app.editor.context() && self.revision == app.editor.revision()
    }
}
pub fn begin(app: &mut App3d) {
    app.set_tool(ToolKind::Select);
    app.flip = Some(State { candidate: None, context: app.editor.context().to_vec(), revision: app.editor.revision() });
}
pub fn pointer(app: &mut App3d, at: DVec2, event: Pointer) {
    if !app.flip.as_ref().is_some_and(|s| s.valid(app)) {
        app.flip = None;
        return;
    }
    let view = app.local_view();
    let ray = view.ray(at);
    let mesh = app.editor.mesh();
    let candidate = mesh.pick_face(&ray).and_then(|(face, _)| {
        let mut choices = mesh
            .face_edges(face)
            .into_iter()
            .filter_map(|edge| {
                let data = mesh.edge(edge)?;
                if data.hidden {
                    return None;
                }
                let a = view.project(mesh.pos(data.v[0]))?;
                let b = view.project(mesh.pos(data.v[1]))?;
                let distance = distance_point_segment_2d(at, a, b);
                (distance <= 10. * app.pick_scale()).then_some((distance, edge))
            })
            .collect::<Vec<_>>();
        choices.sort_by(|a, b| a.0.total_cmp(&b.0));
        choices.into_iter().find(|(_, edge)| mesh.can_flip_terrain_edge(*edge)).map(|(_, edge)| edge)
    });
    app.flip.as_mut().unwrap().candidate = candidate;
    if matches!(event, Pointer::Down { .. }) {
        if let Some(edge) = candidate {
            match app.editor.apply(Command::FlipTerrainEdge { edge }) {
                Ok(out) => {
                    app.editor.set_selection(out.created);
                    let state = app.flip.as_mut().unwrap();
                    state.revision = app.editor.revision();
                    state.candidate = None;
                    app.message = None;
                }
                Err(e) => app.message = Some(e.to_string()),
            }
        } else {
            app.message =
                Some("Hover an interior edge between two terrain triangles. Edit the terrain group first.".into());
        }
    }
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(state) = &app.flip else { return };
    if !state.valid(app) {
        return;
    }
    let Some(edge) = state.candidate.and_then(|id| app.editor.mesh().edge(id)) else { return };
    let view = app.local_view();
    if let (Some(a), Some(b)) =
        (view.project(app.editor.mesh().pos(edge.v[0])), view.project(app.editor.mesh().pos(edge.v[1])))
    {
        painter.line_segment(
            [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
            egui::Stroke::new(3., egui::Color32::MAGENTA),
        );
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::{Editor, Element};
    #[test]
    fn escape_and_revision_changes_cancel_the_tool() {
        let mut app = App3d::new(Editor::new());
        begin(&mut app);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.flip.is_none());
        begin(&mut app);
        app.editor.apply(Command::AddGuidePoint { point: DVec3::ZERO }).unwrap();
        let before = app.editor.doc().clone();
        pointer(&mut app, DVec2::ZERO, Pointer::Down { clicks: 1 });
        assert!(app.flip.is_none());
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn hover_and_click_soft_diagonal_then_undo() {
        let mut app = App3d::new(Editor::new());
        let out = app
            .editor
            .apply(Command::CreateTerrainGrid { origin: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, spacing: 1. })
            .unwrap();
        let Element::Instance(id) = out.created[0] else { panic!() };
        app.editor.enter(id).unwrap();
        let before = app.editor.doc().clone();
        let diagonal = app.editor.mesh().edges().find(|(_, edge)| edge.soft).unwrap().0;
        begin(&mut app);
        let at = app.view().project(DVec3::new(0.5, 0.5, 0.)).unwrap();
        pointer(&mut app, at, Pointer::Move);
        assert_eq!(app.flip.as_ref().unwrap().candidate, Some(diagonal));
        assert_eq!(app.editor.doc(), &before);
        pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert!(app.editor.mesh().edge(diagonal).is_none());
        assert_eq!(app.editor.undo_label(), Some("Flip Edge"));
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
}
