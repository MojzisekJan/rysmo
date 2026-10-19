//! Exact-radius planar corner rounding with a model-space preview.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_model::{Command, EdgeId, Element, InstanceId, parse_length};

pub struct Draft {
    edges: [EdgeId; 2],
    radius: String,
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
    fn plan(&self, app: &App3d) -> Result<rysmo_mesh::Fillet, String> {
        let radius = parse_length(&self.radius, app.editor.doc().units).map_err(|e| e.to_string())?;
        app.editor.mesh().fillet_plan(self.edges, radius, self.segments)
    }
}
pub fn begin(app: &mut App3d) {
    let selection = app.editor.selected();
    let [Element::Edge(a), Element::Edge(b)] = selection.as_slice() else {
        app.message = Some("Select exactly two adjacent straight edges for Fillet.".into());
        return;
    };
    let edges = [*a, *b];
    app.set_tool(rysmo_tools::ToolKind::Select);
    app.fillet = Some(Draft {
        edges,
        radius: "100mm".into(),
        segments: 12,
        revision: app.editor.revision(),
        generation: app.document_generation,
        context: app.editor.context().to_vec(),
    });
}
fn apply(app: &mut App3d, draft: &Draft) -> bool {
    if !draft.valid(app) {
        return false;
    }
    let plan = match draft.plan(app) {
        Ok(plan) => plan,
        Err(error) => {
            app.message = Some(error);
            return false;
        }
    };
    match app.editor.apply(Command::FilletEdges { edges: draft.edges, radius: plan.radius, segments: draft.segments }) {
        Ok(out) => {
            app.editor.set_selection(out.created);
            app.message = Some("Corner rounded. Undo restores the original edges.".into());
            true
        }
        Err(error) => {
            app.message = Some(error.to_string());
            false
        }
    }
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(mut draft) = app.fillet.take() else { return };
    if !draft.valid(app) {
        return;
    }
    let mut open = true;
    let mut commit = false;
    let mut cancel = false;
    crate::theme::window("Fillet").beside_model().open(&mut open).default_width(360.).show(ctx,|ui| {
        ui.label("Round the shared corner of two straight edges.");
        ui.label("Radius");
        ui.add(egui::TextEdit::singleline(&mut draft.radius).desired_width(f32::INFINITY));
        ui.horizontal(|ui|{ui.label("Segments");ui.add(crate::widgets::Number::new(&mut draft.segments).range(2..=512));});
        let plan=draft.plan(app);
        if let Err(error)=&plan { ui.colored_label(egui::Color32::from_rgb(255,170,110),error); }
        ui.small("Wire corners and convex face outlines are supported. Solid edges, holes, curves and branching corners are excluded.");
        ui.horizontal(|ui|{commit=ui.add_enabled(plan.is_ok(),egui::Button::new("Apply")).clicked();cancel=ui.button("Cancel").clicked();});
    });
    if commit {
        if !apply(app, &draft) {
            app.fillet = Some(draft);
        }
    } else if open && !cancel {
        app.fillet = Some(draft);
    }
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(draft) = app.fillet.as_ref().filter(|d| d.valid(app)) else { return };
    let Ok(plan) = draft.plan(app) else { return };
    let view = app.local_view();
    for line in plan.points.windows(2).chain(plan.remainders.iter().map(|p| p.as_slice())) {
        if let (Some(a), Some(b)) = (view.project(line[0]), view.project(line[1])) {
            painter.line_segment(
                [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                egui::Stroke::new(2., egui::Color32::from_rgb(60, 220, 220)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    fn fixture(nested: bool) -> App3d {
        let mut editor = rysmo_model::Editor::new();
        let out = editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y * 2., facing: None })
            .unwrap();
        if nested {
            let group = editor.apply(Command::MakeGroup { elements: out.created }).unwrap().created[0];
            let Element::Instance(id) = group else { panic!() };
            editor.enter(id).unwrap();
        }
        let corner = editor.mesh().find_vertex(DVec3::ZERO).unwrap();
        let edges = editor.mesh().edges_of_vertex(corner);
        editor.set_selection(edges.into_iter().map(Element::Edge));
        App3d::new(editor)
    }
    #[test]
    fn fillet_action_trims_and_round_trips_in_one_undo_including_nested_context() {
        for nested in [false, true] {
            let mut app = fixture(nested);
            let before = app.editor.doc().clone();
            let depth = app.editor.undo_depth();
            begin(&mut app);
            let draft = app.fillet.take().unwrap();
            assert!(apply(&mut app, &draft));
            assert_eq!(app.editor.undo_depth(), depth + 1);
            assert_eq!(app.editor.undo_label(), Some("Fillet"));
            assert!(app.editor.mesh().find_vertex(DVec3::ZERO).is_none());
            assert_eq!(app.editor.mesh().curves().count(), 1);
            let saved = rysmo_io::save(app.editor.doc());
            assert_eq!(rysmo_io::save(&rysmo_io::load(&saved).unwrap()), saved);
            assert!(app.editor.undo());
            assert_eq!(app.editor.doc(), &before);
            assert!(app.editor.redo());
            assert_eq!(app.editor.mesh().curves().count(), 1);
        }
    }
    #[test]
    fn invalid_radius_escape_and_stale_drafts_do_not_change_the_model() {
        let mut app = fixture(false);
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        begin(&mut app);
        let mut draft = app.fillet.take().unwrap();
        draft.radius = "50m".into();
        assert!(!apply(&mut app, &draft));
        assert_eq!(app.editor.doc(), &before);
        assert_eq!(app.editor.undo_depth(), depth);
        begin(&mut app);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.fillet.is_none());
        begin(&mut app);
        let draft = app.fillet.take().unwrap();
        app.document_generation += 1;
        assert!(!apply(&mut app, &draft));
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        let draft = app.fillet.take().unwrap();
        app.editor.apply(Command::DrawLines { lines: vec![[DVec3::X * 10., DVec3::X * 11.]], facing: None }).unwrap();
        let changed = app.editor.doc().clone();
        assert!(!apply(&mut app, &draft));
        assert_eq!(app.editor.doc(), &changed);
    }
    #[test]
    fn menu_and_search_expose_fillet_only_for_two_edges() {
        let mut app = fixture(false);
        assert!(crate::actions::available(&app).contains(&(crate::actions::Action::Fillet, true)));
        begin(&mut app);
        assert!(app.fillet.is_some());
        app.editor.clear_selection();
        app.fillet = None;
        begin(&mut app);
        assert!(app.fillet.is_none());
        assert!(crate::actions::available(&app).contains(&(crate::actions::Action::Fillet, false)));
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
pub fn preview(app: &mut App3d) {
    begin(app);
    if let Some(draft) = app.fillet.as_mut() {
        draft.radius = "500mm".into();
    }
}
