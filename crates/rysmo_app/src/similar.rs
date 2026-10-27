//! Reference-based selection with an explicit match preview.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_model::{Element, InstanceId, similar::Criterion};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Material,
    Length,
    Definition,
}
#[derive(PartialEq, Eq)]
struct QueryKey {
    mode: Mode,
    percent: u64,
    absolute: String,
    include_hidden: bool,
}
struct Cache {
    key: QueryKey,
    result: Result<Vec<Element>, String>,
}
pub struct Draft {
    cache: Option<Cache>,
    seed: Element,
    mode: Mode,
    percent: f64,
    absolute: String,
    include_hidden: bool,
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
    fn result(&mut self, app: &App3d) -> &Result<Vec<Element>, String> {
        let key = QueryKey {
            mode: self.mode,
            percent: self.percent.to_bits(),
            absolute: self.absolute.clone(),
            include_hidden: self.include_hidden,
        };
        if self.cache.as_ref().is_none_or(|cached| cached.key != key) {
            let result = self.matches(app);
            self.cache = Some(Cache { key, result });
        }
        &self.cache.as_ref().expect("query cached").result
    }
    fn matches(&self, app: &App3d) -> Result<Vec<Element>, String> {
        let criterion = match self.mode {
            Mode::Material => Criterion::Material,
            Mode::Definition => Criterion::Definition,
            Mode::Length => Criterion::EdgeLength {
                relative_tolerance: self.percent / 100.,
                absolute_tolerance: rysmo_model::parse_length(&self.absolute, app.editor.doc().units)
                    .map_err(|error| error.to_string())?,
            },
        };
        rysmo_model::similar::find(&app.editor, self.seed, criterion, self.include_hidden)
            .map_err(|error| error.to_string())
    }
}
pub fn available(app: &App3d) -> bool {
    matches!(app.editor.selected().as_slice(), [Element::Edge(_) | Element::Face(_) | Element::Instance(_)])
}
pub fn begin(app: &mut App3d) {
    let selected = app.editor.selected();
    let [seed] = selected.as_slice() else {
        app.message = Some("Select one reference edge, face, group or component.".into());
        return;
    };
    let mode = match seed {
        Element::Edge(_) => Mode::Length,
        Element::Face(_) => Mode::Material,
        Element::Instance(_) => Mode::Definition,
        _ => {
            app.message = Some("Unsupported reference entity.".into());
            return;
        }
    };
    app.set_tool(rysmo_tools::ToolKind::Select);
    app.similar = Some(Draft {
        cache: None,
        seed: *seed,
        mode,
        percent: 0.1,
        absolute: "0.1mm".into(),
        include_hidden: false,
        revision: app.editor.revision(),
        generation: app.document_generation,
        context: app.editor.context().to_vec(),
    });
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(mut draft) = app.similar.take() else { return };
    if !draft.valid(app) {
        return;
    }
    let mut open = true;
    let mut apply = false;
    let mut cancel = false;
    let window = crate::theme::window("Select Similar")
        .vscroll(true)
        .default_size(egui::vec2(380., 360.))
        .beside_model()
        .open(&mut open)
        .default_width(380.);
    window.show(ctx, |ui| {
        ui.label("Match entities of the same kind in the current editing context.");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut draft.mode, Mode::Material, "Assigned material");
            if matches!(draft.seed, Element::Edge(_)) {
                ui.selectable_value(&mut draft.mode, Mode::Length, "Edge length");
            }
            if matches!(draft.seed, Element::Instance(_)) {
                ui.selectable_value(&mut draft.mode, Mode::Definition, "Same definition");
            }
        });
        if draft.mode == Mode::Length {
            ui.horizontal(|ui| {
                ui.label("Tolerance (%)");
                ui.add(crate::widgets::Number::new(&mut draft.percent).range(0. ..=100.));
            });
            ui.label("Minimum absolute tolerance");
            ui.add(egui::TextEdit::singleline(&mut draft.absolute).desired_width(f32::INFINITY));
            ui.small("The larger tolerance applies. Lengths include the editing context's scale.");
        }
        ui.checkbox(&mut draft.include_hidden, "Include hidden geometry and tags");
        let matches = draft.result(app);
        match &matches {
            Ok(found) => {
                ui.label(format!("{} matching entities", found.len()));
            }
            Err(error) => {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
        }
        ui.small("Faces use assigned front material, including Default. Child groups are not traversed.");
        ui.horizontal(|ui| {
            apply = ui.add_enabled(matches.is_ok(), egui::Button::new("Select Similar")).clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if apply {
        if let Some(Cache { result: Ok(found), .. }) = draft.cache.take() {
            let count = found.len();
            app.editor.set_selection(found);
            app.message = Some(format!("Selected {count} matching entities."));
        }
    } else if open && !cancel {
        app.similar = Some(draft);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::{Command, Editor};
    #[test]
    fn preview_is_read_only_and_escape_or_context_change_discards_it() {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawLines {
                lines: vec![[DVec3::ZERO, DVec3::X], [DVec3::Y, DVec3::Y + DVec3::X]],
                facing: None,
            })
            .unwrap();
        let edge = editor.mesh().edges().next().unwrap().0;
        editor.set_selection([Element::Edge(edge)]);
        let mut app = App3d::new(editor);
        let before = app.editor.doc().clone();
        let depth = app.editor.undo_depth();
        begin(&mut app);
        let draft = app.similar.as_ref().unwrap();
        assert_eq!(draft.matches(&app).unwrap().len(), 2);
        assert_eq!(app.editor.doc(), &before);
        assert_eq!(app.editor.undo_depth(), depth);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.similar.is_none());
        begin(&mut app);
        app.document_generation += 1;
        assert!(!app.similar.as_ref().unwrap().valid(&app));
    }
}
