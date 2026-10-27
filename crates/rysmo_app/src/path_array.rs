//! Preview and confirmation for component occurrences along an edge chain.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DMat4, DVec3};
use rysmo_model::{
    Command, EdgeId, Element, InstanceId, parse_length,
    path_array::{self, Distribution, Options, Plan},
};
pub struct Draft {
    source: InstanceId,
    path: Vec<EdgeId>,
    options: Options,
    spacing: String,
    offset: String,
    count: u32,
    by_spacing: bool,
    revision: u64,
    generation: u64,
    context: Vec<InstanceId>,
    cached: Option<(Options, Result<Plan, String>)>,
    bounds: Option<[DVec3; 8]>,
}
impl Draft {
    fn valid(&self, app: &App3d) -> bool {
        self.revision == app.editor.revision()
            && self.generation == app.document_generation
            && self.context == app.editor.context()
    }
    fn options(&self, app: &App3d) -> Result<Options, String> {
        Ok(Options {
            distribution: if self.by_spacing {
                Distribution::Spacing(parse_length(&self.spacing, app.editor.doc().units).map_err(|e| e.to_string())?)
            } else {
                Distribution::Count(self.count)
            },
            start_offset: parse_length(&self.offset, app.editor.doc().units).map_err(|e| e.to_string())?,
            ..self.options
        })
    }
    fn update(&mut self, app: &App3d, options: Options) {
        if self.cached.as_ref().is_some_and(|(old, _)| *old == options) {
            return;
        }
        let result = path_array::plan(
            app.editor.doc(),
            app.editor.active_definition(),
            app.editor.context_transform(),
            self.source,
            &self.path,
            options,
        )
        .map_err(|e| e.to_string());
        if result.is_ok() && self.bounds.is_none() {
            let source = &app.editor.active().instances[&self.source];
            self.bounds = app
                .editor
                .doc()
                .definition_bounds(Some(source.definition))
                .map(|(lo, hi)| rysmo_model::corners(lo, hi));
        }
        self.cached = Some((options, result));
    }
}
fn selection(app: &App3d) -> Option<(InstanceId, Vec<EdgeId>)> {
    let mut source = None;
    let mut path = Vec::new();
    for item in app.editor.selection() {
        match item {
            Element::Instance(id) if source.is_none() => source = Some(*id),
            Element::Edge(id) => path.push(*id),
            _ => return None,
        }
    }
    let source = source?;
    let instance = app.editor.active().instances.get(&source)?;
    if path.is_empty() || path.len() > 2048 || app.editor.doc().definitions.get(&instance.definition)?.group {
        return None;
    }
    Some((source, path))
}
pub fn available(app: &App3d) -> bool {
    selection(app).is_some()
}
pub fn begin(app: &mut App3d) {
    let Some((source, path)) = selection(app) else {
        app.message = Some("Select one component and connected path edges.".into());
        return;
    };
    app.set_tool(rysmo_tools::ToolKind::Select);
    let mut draft = Draft {
        source,
        path,
        options: Options::default(),
        spacing: "1m".into(),
        offset: "0m".into(),
        count: 10,
        by_spacing: false,
        revision: app.editor.revision(),
        generation: app.document_generation,
        context: app.editor.context().to_vec(),
        cached: None,
        bounds: None,
    };
    draft.update(app, draft.options);
    app.path_array = Some(draft);
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(mut draft) = app.path_array.take().filter(|d| d.valid(app)) else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    let response = crate::theme::window("Array Along Path").vscroll(true).beside_model().open(&mut open).default_width(390.).show(ctx, |ui| {
        ui.label("The source remains in place. Count means new occurrences.");
        ui.checkbox(&mut draft.by_spacing, "Use measured spacing");
        if draft.by_spacing { ui.label("Spacing along the path"); ui.add(egui::TextEdit::singleline(&mut draft.spacing).desired_width(f32::INFINITY)); }
        else { ui.horizontal(|ui| { ui.label("New occurrences"); ui.add(crate::widgets::Number::new(&mut draft.count).range(1..=1024)); }); }
        ui.label("Start offset along the path"); ui.add(egui::TextEdit::singleline(&mut draft.offset).desired_width(f32::INFINITY));
        ui.checkbox(&mut draft.options.align_tangent, "Align component X axis to the tangent");
        ui.checkbox(&mut draft.options.reverse, "Reverse path direction");
        let parsed = draft.options(app);
        if let Ok(options) = parsed { draft.update(app, options); }
        let error = parsed.err().or_else(|| draft.cached.as_ref().and_then(|(_, result)| result.as_ref().err().cloned()));
        if let Some(error) = &error { ui.colored_label(egui::Color32::LIGHT_RED, error); }
        else if let Some((_, Ok(plan))) = &draft.cached { ui.label(format!("{} new occurrences; path {}{}", plan.transforms.len(), app.editor.doc().format_length(plan.length), if plan.closed { " (closed)" } else { "" })); }
        ui.small("The preview shows up to 256 bounding boxes. Escape cancels. Local coordinates and shared definitions are retained.");
        ui.horizontal(|ui| { apply = ui.add_enabled(error.is_none(), egui::Button::new("Create array")).clicked(); cancel = ui.button("Cancel").clicked(); });
    });
    crate::features::remember(app, &response);
    if apply && let Ok(options) = draft.options(app) {
        match app.editor.apply(Command::ArrayAlongPath {
            source: draft.source,
            path: draft.path.clone(),
            options,
            context: app.editor.context_transform(),
        }) {
            Ok(out) => {
                app.editor.set_selection(out.created);
                app.message = Some("Path array created. Undo removes the entire array.".into());
                return;
            }
            Err(error) => app.message = Some(error.to_string()),
        }
    }
    if open && !cancel {
        app.path_array = Some(draft);
    }
}
pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(draft) = app.path_array.as_ref().filter(|d| d.valid(app)) else {
        return;
    };
    if draft.options(app).is_err() {
        return;
    }
    let (Some(corners), Some((_, Ok(plan)))) = (&draft.bounds, &draft.cached) else {
        return;
    };
    let view = app.local_view();
    for transform in plan.transforms.iter().take(256) {
        draw_box(painter, &view, *transform, corners);
    }
}
fn draw_box(painter: &egui::Painter, view: &rysmo_math::View, transform: DMat4, corners: &[DVec3; 8]) {
    for (a, b) in [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)] {
        if let (Some(a), Some(b)) =
            (view.project(transform.transform_point3(corners[a])), view.project(transform.transform_point3(corners[b])))
        {
            painter.line_segment(
                [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                egui::Stroke::new(1.3, egui::Color32::from_rgb(90, 220, 160)),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancel_and_context_changes_invalidate_path_array_without_an_edit() {
        let mut editor = rysmo_model::Editor::new();
        let elements = editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        let made = editor.apply(Command::MakeComponent { elements, name: None }).unwrap();
        let line =
            editor.apply(Command::DrawLines { lines: vec![[DVec3::X * 5., DVec3::X * 10.]], facing: None }).unwrap();
        editor.set_selection(made.created.into_iter().chain(line.created));
        let mut app = App3d::new(editor);
        let before = app.editor.doc().clone();
        begin(&mut app);
        assert!(app.path_array.as_ref().unwrap().cached.as_ref().unwrap().1.is_ok());
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.path_array.is_none());
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        app.document_generation += 1;
        assert!(!app.path_array.as_ref().unwrap().valid(&app));
        assert_eq!(app.editor.doc(), &before);
    }
}
