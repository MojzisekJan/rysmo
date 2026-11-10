//! Guard destructive file actions and operating-system close requests.
use crate::{files, state::App3d};
use bevy::prelude::*;
use bevy_egui::egui;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    New,
    NewWithUnits(rysmo_model::Units),
    Open,
    Recent(String),
    Close,
    CloseTab,
}

#[derive(Default)]
pub struct Guard {
    pending: Option<Action>,
    saving: bool,
    collection: bool,
    pub close_ready: bool,
}

impl Guard {
    pub fn active(&self) -> bool {
        self.pending.is_some()
    }
}

pub fn request(app: &mut App3d, action: Action) {
    if app.unsaved.pending.is_some() {
        return;
    }
    if matches!(action, Action::New | Action::NewWithUnits(_)) && app.documents.full() {
        app.message = Some("Close a document before opening more than 16 models.".into());
        return;
    }
    if matches!(action, Action::New | Action::NewWithUnits(_) | Action::Open | Action::Recent(_)) {
        perform(app, action);
    } else if app.editor.is_dirty()
        && !(action == Action::Close && app.documents.discarded_for_close(app.editor.revision()))
    {
        app.unsaved.pending = Some(action);
    } else {
        perform(app, action);
    }
}

fn perform(app: &mut App3d, action: Action) {
    match action {
        Action::New => files::new(app),
        Action::NewWithUnits(units) => {
            files::new(app);
            let mut doc = rysmo_model::Document::new();
            doc.units = units;
            app.editor.replace_document(doc);
        }
        Action::Open => files::open(app),
        Action::Recent(name) => files::open_recent(app, &name),
        Action::CloseTab => crate::documents::close(app),
        Action::Close => {
            if let Some(id) = app.documents.first_dirty() {
                crate::documents::switch(app, id);
                request(app, Action::Close);
            } else if app.features.library.modified {
                app.unsaved.pending = Some(Action::Close);
                app.unsaved.collection = true;
            } else {
                app.unsaved.close_ready = true;
            }
        }
    }
}

pub fn close_requests(
    mut requests: MessageReader<bevy::window::WindowCloseRequested>,
    mut app: ResMut<App3d>,
    mut exit: MessageWriter<AppExit>,
) {
    if requests.read().next().is_some() {
        request(&mut app, Action::Close);
    }
    if app.unsaved.close_ready {
        crate::preferences::flush(&mut app);
        app.unsaved.close_ready = false;
        exit.write(AppExit::Success);
    }
}

pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if app.unsaved.pending.is_none() {
        return;
    }
    let dirty = if app.unsaved.collection { app.features.library.modified } else { app.editor.is_dirty() };
    if app.unsaved.saving && !dirty {
        if let Some(action) = app.unsaved.pending.take() {
            app.unsaved.saving = false;
            app.unsaved.collection = false;
            perform(app, action);
        }
        return;
    }
    // The mobile save picker is an in-app dialog and must remain interactive.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    if app.unsaved.saving && app.portable_files.is_some() {
        return;
    }
    let name = app.path.as_deref().and_then(std::path::Path::file_name);
    let title = if app.unsaved.collection {
        format!("Save changes to collection “{}”?", app.features.library.name())
    } else {
        match name {
            Some(name) => format!("Save changes to “{}”?", name.to_string_lossy()),
            None => "Save changes to this model?".to_owned(),
        }
    };
    let frame = egui::Frame::window(&ctx.global_style()).inner_margin(egui::Margin::same(20));
    let modal = egui::Modal::new(egui::Id::unique("unsaved_changes")).frame(frame).show(ctx, |ui| {
        let width = (ctx.content_rect().width() - 56.).clamp(1., 340.);
        ui.set_width(width);
        ui.vertical_centered(|ui| {
            ui.heading(title);
            ui.add_space(6.);
            ui.label("Your changes will be lost if you close without saving.");
            if app.unsaved.collection
                && let Some(error) = app.features.library.error()
            {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.add_space(14.);

            let button_width = ((width - 2. * ui.spacing().item_spacing.x) / 3.).max(1.);
            ui.horizontal(|ui| {
                let mut choice = None;
                for (label, action) in
                    [("Don’t Save", Choice::Discard), ("Cancel", Choice::Cancel), ("Save", Choice::Save)]
                {
                    if ui
                        .scope(|ui| {
                            let mut text = egui::RichText::new(label);
                            let accent = ui.visuals().selection.bg_fill;
                            let destructive = ui.visuals().error_fg_color;
                            let widgets = &mut ui.visuals_mut().widgets;
                            if matches!(action, Choice::Save) {
                                text = text.strong().color(egui::Color32::WHITE);
                                widgets.inactive.weak_bg_fill = accent;
                                widgets.hovered.weak_bg_fill = accent.gamma_multiply(1.15);
                                widgets.active.weak_bg_fill = accent.gamma_multiply(0.85);
                            } else if matches!(action, Choice::Discard) {
                                text = text.color(destructive);
                            }
                            ui.add_sized([button_width, 32.], egui::Button::new(text).frame(true)).clicked()
                        })
                        .inner
                    {
                        choice = Some(action);
                    }
                }
                choice
            })
            .inner
        })
        .inner
    });
    let mut choice = modal.inner;
    if ctx.input_mut(|i| {
        i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
            || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Period)
    }) {
        choice = Some(Choice::Cancel);
    } else if modal.is_top_modal
        && choice.is_none()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
    {
        choice = Some(Choice::Save);
    }
    if let Some(choice) = choice {
        choose(app, choice);
    }
}

enum Choice {
    Save,
    Discard,
    Cancel,
}

fn choose(app: &mut App3d, choice: Choice) {
    match choice {
        Choice::Save => {
            app.unsaved.saving = true;
            if app.unsaved.collection {
                crate::features::library::save(app);
            } else {
                files::save(app, false);
            }
        }
        Choice::Discard => {
            if let Some(action) = app.unsaved.pending.take() {
                app.unsaved.saving = false;
                if app.unsaved.collection {
                    app.unsaved.collection = false;
                    app.unsaved.close_ready = true;
                    return;
                }
                if action == Action::Close {
                    app.documents.discard_for_close(app.editor.revision());
                }
                perform(app, action);
            }
        }
        Choice::Cancel => {
            app.unsaved.pending = None;
            app.unsaved.collection = false;
            app.unsaved.saving = false;
            app.documents.cancel_close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_collection_requires_save_discard_or_cancel_even_with_a_clean_model() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.features.library.modified = true;
        request(&mut app, Action::Close);
        assert!(app.unsaved.collection);
        assert!(!app.unsaved.close_ready);
        choose(&mut app, Choice::Cancel);
        assert!(app.features.library.modified);
        assert!(!app.unsaved.active());
        request(&mut app, Action::Close);
        app.unsaved.saving = true;
        frame(&mut app, vec![]);
        assert!(!app.unsaved.close_ready);
        app.features.library.modified = false;
        frame(&mut app, vec![]);
        assert!(app.unsaved.close_ready);
    }
    #[test]
    fn models_and_collection_are_confirmed_separately_before_exit() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor.mark_dirty();
        app.features.library.modified = true;
        request(&mut app, Action::Close);
        assert!(!app.unsaved.collection);
        choose(&mut app, Choice::Discard);
        assert!(app.unsaved.collection);
        assert!(!app.unsaved.close_ready);
        choose(&mut app, Choice::Cancel);
        assert!(app.editor.is_dirty());
        request(&mut app, Action::Close);
        choose(&mut app, Choice::Discard);
        choose(&mut app, Choice::Discard);
        assert!(app.unsaved.close_ready);
        assert!(app.editor.is_dirty());
        assert!(app.features.library.modified);
    }
    #[test]
    fn dirty_model_requires_confirmation_and_repeated_close_keeps_pending_action() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor.mark_dirty();
        request(&mut app, Action::CloseTab);
        request(&mut app, Action::Close);
        assert_eq!(app.unsaved.pending, Some(Action::CloseTab));
        assert!(app.editor.is_dirty());
        assert!(!app.unsaved.close_ready);
    }
    #[test]
    fn cancelling_multi_document_close_keeps_discarded_models_dirty() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor.mark_dirty();
        let first = app.documents.active;
        assert!(crate::documents::create(&mut app));
        app.editor.mark_dirty();
        let second = app.documents.active;
        request(&mut app, Action::Close);
        choose(&mut app, Choice::Discard);
        assert_eq!(app.documents.active, first);
        assert!(app.unsaved.active());
        choose(&mut app, Choice::Cancel);
        crate::documents::switch(&mut app, second);
        assert!(app.editor.is_dirty());
        request(&mut app, Action::Close);
        assert!(app.unsaved.active());
        choose(&mut app, Choice::Discard);
        choose(&mut app, Choice::Discard);
        assert!(app.unsaved.close_ready);
        assert!(app.editor.is_dirty());
    }
    #[test]
    fn editing_invalidates_a_close_discard_decision() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor.mark_dirty();
        app.documents.discard_for_close(app.editor.revision());
        app.editor.apply(rysmo_model::Command::SetUnits { units: rysmo_model::Units::Metres }).unwrap();
        request(&mut app, Action::Close);
        assert!(app.unsaved.active());
    }
    #[test]
    fn clean_model_can_close_immediately() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        request(&mut app, Action::Close);
        assert!(app.unsaved.close_ready);
        assert!(app.unsaved.pending.is_none());
    }
    fn frame(app: &mut App3d, events: Vec<egui::Event>) {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 600.))),
                events,
                ..Default::default()
            },
            |ui| panel(ui.ctx(), app),
        );
        output.textures_delta.clear();
    }
    #[test]
    fn escape_cancels_close_without_losing_changes() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor.mark_dirty();
        request(&mut app, Action::Close);
        frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!app.unsaved.active());
        assert!(!app.unsaved.close_ready);
        assert!(app.editor.is_dirty());
    }
    #[test]
    fn save_must_succeed_before_pending_close_continues() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor.mark_dirty();
        request(&mut app, Action::Close);
        app.unsaved.saving = true;
        // A cancelled or failed save leaves the editor dirty and the window open.
        frame(&mut app, vec![]);
        assert!(app.unsaved.active());
        assert!(!app.unsaved.close_ready);
        app.editor.mark_saved();
        frame(&mut app, vec![]);
        assert!(!app.unsaved.active());
        assert!(app.unsaved.close_ready);
    }
    #[test]
    fn operating_system_close_request_does_not_exit_a_dirty_model() {
        let mut app = App::new();
        let mut model = App3d::new(rysmo_model::Editor::new());
        model.editor.mark_dirty();
        app.insert_resource(model)
            .add_message::<bevy::window::WindowCloseRequested>()
            .add_message::<AppExit>()
            .add_systems(Update, close_requests);
        let window = app.world_mut().spawn_empty().id();
        app.world_mut().write_message(bevy::window::WindowCloseRequested { window });
        app.update();
        assert!(app.world().resource::<App3d>().unsaved.active());
        assert!(app.world().resource::<Messages<AppExit>>().is_empty());
        app.world_mut().resource_mut::<App3d>().unsaved.pending = None;
        app.world_mut().resource_mut::<App3d>().editor.mark_saved();
        app.world_mut().write_message(bevy::window::WindowCloseRequested { window });
        app.update();
        assert!(!app.world().resource::<Messages<AppExit>>().is_empty());
    }
}

#[cfg(test)]
mod welcome_guard_tests {
    use super::*;
    #[test]
    fn template_creation_preserves_the_dirty_document_in_another_tab() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor.mark_dirty();
        let previous = app.documents.active;
        request(&mut app, Action::NewWithUnits(rysmo_model::Units::Architectural));
        assert!(app.unsaved.pending.is_none());
        assert_eq!(app.editor.doc().units, rysmo_model::Units::Architectural);
        crate::documents::switch(&mut app, previous);
        assert!(app.editor.is_dirty());
    }
}
