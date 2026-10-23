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
}

#[derive(Default)]
pub struct Guard {
    pending: Option<Action>,
    saving: bool,
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
    if app.editor.is_dirty() {
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
        Action::Close => app.unsaved.close_ready = true,
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
    if app.unsaved.saving && !app.editor.is_dirty() {
        if let Some(action) = app.unsaved.pending.take() {
            app.unsaved.saving = false;
            perform(app, action);
        }
        return;
    }
    // The mobile save picker is an in-app dialog and must remain interactive.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    if app.unsaved.saving && app.portable_files.is_some() {
        return;
    }
    enum Choice {
        Save,
        Discard,
        Cancel,
    }
    let mut choice = None;
    egui::Modal::new(egui::Id::new("unsaved_changes")).frame(egui::Frame::window(&ctx.global_style())).show(
        ctx,
        |ui| {
            ui.set_width(360.);
            ui.heading("Save changes?");
            ui.add_space(8.);
            ui.label("This model has unsaved changes. Save them before continuing?");
            ui.add_space(12.);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(Choice::Save);
                }
                if ui.button("Discard").clicked() {
                    choice = Some(Choice::Discard);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        },
    );
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        choice = Some(Choice::Cancel);
    }
    match choice {
        Some(Choice::Save) => {
            app.unsaved.saving = true;
            files::save(app, false);
        }
        Some(Choice::Discard) => {
            if let Some(action) = app.unsaved.pending.take() {
                app.unsaved.saving = false;
                perform(app, action);
            }
        }
        Some(Choice::Cancel) => {
            app.unsaved.pending = None;
            app.unsaved.saving = false;
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dirty_model_requires_confirmation_and_repeated_close_keeps_pending_action() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor.mark_dirty();
        request(&mut app, Action::New);
        request(&mut app, Action::Close);
        assert_eq!(app.unsaved.pending, Some(Action::New));
        assert!(app.editor.is_dirty());
        assert!(!app.unsaved.close_ready);
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
    fn template_creation_waits_for_unsaved_changes() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor.mark_dirty();
        let before = app.editor.doc().clone();
        request(&mut app, Action::NewWithUnits(rysmo_model::Units::Architectural));
        assert_eq!(app.unsaved.pending, Some(Action::NewWithUnits(rysmo_model::Units::Architectural)));
        assert_eq!(app.editor.doc(), &before);
    }
}
