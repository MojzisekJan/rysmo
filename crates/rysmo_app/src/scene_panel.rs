use crate::state::App3d;
use bevy_egui::egui;
use rysmo_model::{
    Command,
    scenes::{Scene, SceneProperties},
};

pub fn restore(app: &mut App3d, id: u64) {
    app.editor.set_camera(app.camera);
    if let Err(error) = app.editor.apply(Command::RestoreScene { id }) {
        app.message = Some(error.to_string());
        return;
    }
    app.remember_view();
    app.camera = app.editor.doc().camera.unwrap_or(app.camera);
    app.set_tool(rysmo_tools::ToolKind::Select);
    app.entity_edit_revision = None;
    app.style_edit_revision = None;
    app.text_edit = None;
    app.dimension_text_edit = None;
    app.vcb.clear();
    app.selected_scene = Some(id);
}
fn properties(ui: &mut egui::Ui, p: &mut SceneProperties, allowed: SceneProperties) {
    for (value, enabled, label) in [
        (&mut p.camera, allowed.camera, "Camera location"),
        (&mut p.hidden_geometry, allowed.hidden_geometry, "Hidden geometry"),
        (&mut p.tags, allowed.tags, "Visible tags"),
        (&mut p.sections, allowed.sections, "Active section planes"),
        (&mut p.style, allowed.style, "Drawing style"),
        (&mut p.shadows, allowed.shadows, "Shadow settings"),
        (&mut p.axes, allowed.axes, "Axes location"),
    ] {
        ui.add_enabled(enabled, crate::widgets::Check::new(value, label));
    }
}
pub fn tabs(ui: &mut egui::Ui, app: &mut App3d) {
    let scenes = app.editor.doc().scenes.clone();
    if scenes.is_empty() {
        return;
    }
    egui::ScrollArea::horizontal().id_salt("scene_tabs").show(ui, |ui| {
        ui.horizontal(|ui| {
            for scene in scenes {
                if ui
                    .selectable_label(app.selected_scene == Some(scene.id), &scene.name)
                    .on_hover_text(&scene.description)
                    .clicked()
                {
                    restore(app, scene.id);
                }
            }
        });
    });
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.show_scenes {
        return;
    }
    let mut open = true;
    let result = crate::theme::window("Scenes")
        .open(&mut open)
        .default_width(300.)
        .beside_model()
        .max_height((ctx.content_rect().height() - 170.).max(200.))
        .vscroll(true)
        .show(ctx, |ui| {
            if ui.button("Add scene from current view").clicked() {
                let scene = Scene::capture(
                    app.editor.doc(),
                    app.camera,
                    format!("Scene {}", app.editor.doc().scenes.len() + 1),
                );
                match app.editor.apply(Command::AddScene { scene }) {
                    Ok(_) => app.selected_scene = app.editor.doc().scenes.last().map(|s| s.id),
                    Err(error) => app.message = Some(error.to_string()),
                }
            }
            let scenes = app.editor.doc().scenes.clone();
            egui::ScrollArea::vertical().max_height(160.).show(ui, |ui| {
                for scene in &scenes {
                    if ui.selectable_label(app.selected_scene == Some(scene.id), &scene.name).clicked() {
                        app.selected_scene = Some(scene.id);
                    }
                }
            });
            let Some(index) = scenes.iter().position(|s| Some(s.id) == app.selected_scene) else { return };
            let mut scene = scenes[index].clone();
            ui.separator();
            let mut edit = false;
            ui.label("Name");
            let key = egui::Id::unique(("scene_name", scene.id));
            let mut name = ctx.data_mut(|d| d.get_temp::<String>(key)).unwrap_or_else(|| scene.name.clone());
            let response = ui.add(crate::theme::singleline(&mut name).desired_width(f32::INFINITY));
            if response.changed() {
                ctx.data_mut(|d| d.insert_temp(key, name.clone()));
            }
            if response.lost_focus() {
                scene.name = name;
                edit = true;
                ctx.data_mut(|d| d.remove::<String>(key));
            }
            ui.label("Description");
            let key = egui::Id::unique(("scene_description", scene.id));
            let mut description =
                ctx.data_mut(|d| d.get_temp::<String>(key)).unwrap_or_else(|| scene.description.clone());
            let response = ui.add(crate::theme::multiline(&mut description));
            if response.changed() {
                ctx.data_mut(|d| d.insert_temp(key, description.clone()));
            }
            if response.lost_focus() {
                scene.description = description;
                edit = true;
                ctx.data_mut(|d| d.remove::<String>(key));
            }
            edit |= crate::widgets::checkbox(ui, &mut scene.include_in_animation, "Include in animation").changed();
            ui.label("Properties to save");
            let before = scene.properties;
            properties(ui, &mut scene.properties, SceneProperties::default());
            edit |= before != scene.properties;
            if edit {
                let _ = app.editor.apply(Command::SetScene { id: scene.id, scene: Some(scene.clone()) });
            }
            ui.horizontal(|ui| {
                if ui.button("Restore").clicked() {
                    restore(app, scene.id);
                }
                if ui.button("Update…").clicked() {
                    app.scene_update = Some((scene.id, scene.properties));
                }
                if ui.button("Delete").clicked() {
                    let _ = app.editor.apply(Command::SetScene { id: scene.id, scene: None });
                    app.selected_scene = None;
                }
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(index > 0, egui::Button::new("Move up")).clicked() {
                    let _ = app.editor.apply(Command::ReorderScene { id: scene.id, index: index - 1 });
                }
                if ui.add_enabled(index + 1 < scenes.len(), egui::Button::new("Move down")).clicked() {
                    let _ = app.editor.apply(Command::ReorderScene { id: scene.id, index: index + 1 });
                }
            });
        });
    app.show_scenes = open;
    if !open {
        app.scene_update = None;
    }
    if let Some(result) = result {
        track(app, result.response.rect);
    }
    if let Some((id, mut requested)) = app.scene_update {
        let Some(mut scene) = app.editor.doc().scenes.iter().find(|s| s.id == id).cloned() else {
            app.scene_update = None;
            return;
        };
        let mut open = true;
        let result = crate::theme::window("Properties to update").open(&mut open).show(ctx, |ui| {
            properties(ui, &mut requested, scene.properties);
            ui.horizontal(|ui| {
                if ui.button("Update").clicked() {
                    scene.update(app.editor.doc(), app.camera, requested);
                    let _ = app.editor.apply(Command::SetScene { id, scene: Some(scene) });
                    app.scene_update = None;
                }
                if ui.button("Cancel").clicked() {
                    app.scene_update = None;
                }
            });
        });
        if !open {
            app.scene_update = None;
        }
        if app.scene_update.is_some() {
            app.scene_update = Some((id, requested));
        }
        if let Some(result) = result {
            track(app, result.response.rect);
        }
    }
}
fn track(app: &mut App3d, rect: egui::Rect) {
    app.ui_rects.push((
        rysmo_math::DVec2::new(rect.min.x as f64, rect.min.y as f64),
        rysmo_math::DVec2::new(rect.max.x as f64, rect.max.y as f64),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::{DVec2, DVec3, OrbitCamera};
    use rysmo_model::Editor;
    #[test]
    fn scene_recall_cancels_drafts_and_undo_redo_restore_camera() {
        let mut app = App3d::new(Editor::new());
        let original = app.camera;
        let saved = OrbitCamera { target: DVec3::new(12., 4., 2.), parallel: true, ..original };
        let scene = Scene::capture(app.editor.doc(), saved, "Saved".into());
        app.editor.apply(Command::AddScene { scene }).unwrap();
        let id = app.editor.doc().scenes[0].id;
        app.zoom_window = Some((DVec2::ZERO, DVec2::ONE));
        app.section_placing = Some((app.editor.revision(), vec![]));
        app.vcb = "400mm".into();
        restore(&mut app, id);
        assert_eq!(app.camera, saved);
        assert!(app.zoom_window.is_none() && app.section_placing.is_none());
        assert!(app.vcb.is_empty());
        crate::input::undo(&mut app);
        assert_eq!(app.camera, original);
        crate::input::redo(&mut app);
        assert_eq!(app.camera, saved);
        app.previous_view();
        assert_eq!(app.camera, original);
    }
    #[test]
    fn scene_manager_and_tabs_render_with_a_selected_scene() {
        let mut app = App3d::new(Editor::new());
        let scene = Scene::capture(app.editor.doc(), app.camera, "Saved".into());
        app.editor.apply(Command::AddScene { scene }).unwrap();
        app.selected_scene = Some(app.editor.doc().scenes[0].id);
        app.show_scenes = true;
        app.scene_update = Some((app.selected_scene.unwrap(), SceneProperties::default()));
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            panel(ui.ctx(), &mut app);
            egui::CentralPanel::default().show(ui, |ui| tabs(ui, &mut app));
        });
        output.textures_delta.clear();
        assert!(!app.ui_rects.is_empty());
        assert!(app.scene_update.is_some());
    }
}
