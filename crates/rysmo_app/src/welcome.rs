//! Welcome page uses the same guarded file operations as the editor menus.
use crate::{state::App3d, unsaved};
use bevy_egui::egui;
pub fn show(ctx: &egui::Context, app: &mut App3d) -> bool {
    if !app.welcome {
        return false;
    }
    if !crate::theme::fonts_ready(ctx) {
        ctx.request_repaint();
        return true;
    }
    let content = ctx.content_rect();
    let margin = if content.width() < 600. { 16 } else { 24 };
    // The macOS title bar is transparent; keep its strip clear for the traffic lights.
    let title = if cfg!(target_os = "macos") { 28 } else { 0 };
    egui::Area::new("welcome-page".into()).fixed_pos(content.min).order(egui::Order::Background).show(ctx, |ui| {
        #[cfg(target_os = "macos")]
        crate::macos::titlebar_area(ui, egui::Rect::from_min_size(content.min, egui::vec2(content.width(), 28.)));
        let frame = egui::Frame::new()
            .fill(ui.visuals().panel_fill)
            .inner_margin(egui::Margin { top: margin + title, ..egui::Margin::same(margin) });
        frame.show(ui, |ui| {
            // An Area does not limit its width, so wrapping needs the page size set here.
            let inset = egui::vec2(2. * margin as f32, 2. * margin as f32 + title as f32);
            let page = (content.size() - inset).max(egui::vec2(240., 240.));
            ui.set_min_size(page);
            ui.set_max_size(page);
            egui::ScrollArea::vertical().id_salt("welcome-scroll").show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(800.));
                ui.horizontal(|ui| {
                    crate::brand::image(ui, 56.);
                    ui.vertical(|ui| {
                        ui.heading("Rysmo");
                        ui.weak("A simple 3D modeller.");
                    });
                });
                ui.add_space(24.);
                ui.heading("Start creating");
                ui.label("Choose a starting unit, or open an existing model.");
                ui.add_space(12.);
                ui.horizontal_wrapped(|ui| {
                    for (label, units) in [
                        ("Millimetres", rysmo_model::Units::Millimetres),
                        ("Metres", rysmo_model::Units::Metres),
                        ("Feet & inches", rysmo_model::Units::Architectural),
                    ] {
                        if ui.add_sized([160., 48.], egui::Button::new(format!("New · {label}"))).clicked() {
                            unsaved::request(app, unsaved::Action::NewWithUnits(units));
                        }
                    }
                });
                ui.add_space(12.);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Continue editing").clicked() {
                        app.welcome = false;
                    }
                    if ui.button(crate::menu::label(crate::menu::glyph::FOLDER_OPEN, "Open model...")).clicked() {
                        unsaved::request(app, unsaved::Action::Open);
                    }
                    if ui.button("Help & tools").clicked() {
                        app.help.open = true;
                    }
                    if ui.button("Settings").clicked() {
                        app.show_settings = true;
                        app.settings_tab = 3;
                    }
                });
                ui.add_space(24.);
                ui.strong("Recent models");
                if app.recent.is_empty() {
                    ui.weak("Models you open or save will appear here.");
                }
                for name in app.recent.clone().iter().take(8) {
                    let label = std::path::Path::new(name).file_name().unwrap_or_default().to_string_lossy();
                    if ui
                        .add_sized([ui.available_width().min(600.), 36.], egui::Button::new(label).truncate())
                        .on_hover_text(name)
                        .clicked()
                    {
                        unsaved::request(app, unsaved::Action::Recent(name.clone()));
                    }
                }
                ui.add_space(24.);
                ui.separator();
                crate::widgets::checkbox(ui, &mut app.preferences.show_welcome, "Show this page at startup");
                ui.weak("Space selects · R draws a rectangle · P pulls a face · Shift+S searches commands");
                if let Some(message) = &app.message {
                    ui.add_space(8.);
                    ui.label(message);
                }
            });
        });
    });
    crate::panels::settings(ctx, app);
    crate::help::panel(ctx, app);
    crate::portable_files::panel(ctx, app);
    crate::unsaved::panel(ctx, app);
    app.canvas = None;
    app.ui_rects = vec![(rysmo_math::DVec2::ZERO, app.viewport)];
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn welcome_does_not_modify_the_model_or_undo_history() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.welcome = true;
        let before = app.editor.doc().clone();
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960., 640.))),
                    ..Default::default()
                },
                |ui| {
                    assert!(show(ui.ctx(), &mut app));
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(&before, app.editor.doc());
        assert!(!app.editor.is_dirty());
        assert!(app.editor.undo_label().is_none());
        assert_eq!(app.ui_rects.len(), 1);
    }
}

#[cfg(test)]
mod window_layer_tests {
    use super::*;
    #[test]
    fn settings_remain_above_the_welcome_page() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.welcome = true;
        app.show_settings = true;
        app.settings_tab = 3;
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960., 640.))),
                    ..Default::default()
                },
                |ui| {
                    show(ui.ctx(), &mut app);
                },
            );
            output.textures_delta.clear();
        }
        let r = crate::preferences::layout(&ctx).0["Settings"];
        let layer = ctx.layer_id_at(egui::pos2(r[0] + 40., r[1] + 12.)).unwrap();
        assert_eq!(layer.order, egui::Order::Middle);
    }
}
