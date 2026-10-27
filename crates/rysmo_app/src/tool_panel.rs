//! A compact contextual shelf follows the selected tool, without covering the drawing.
use crate::{state::App3d, theme};
use bevy_egui::egui;
use rysmo_tools::ToolKind as T;
pub fn has_options(app: &App3d) -> bool {
    matches!(
        app.tools.active(),
        T::Circle | T::Polygon | T::Arc | T::ThreePointArc | T::CenterArc | T::Pie | T::PaintBucket
    ) && app.text3d.is_none()
        && app.fillet.is_none()
        && app.terrain.is_none()
        && app.smoove.is_none()
        && app.stamp.is_none()
        && app.drape.is_none()
        && app.mirror.is_none()
        && app.solid_tools.is_none()
}

pub fn panel(ui: &mut egui::Ui, app: &mut App3d) {
    if !has_options(app) {
        return;
    }
    let tool = app.tools.active();
    ui.horizontal_wrapped(|ui| {
        ui.strong(tool.name());
        ui.separator();
        match tool {
            T::Circle | T::Polygon | T::Arc | T::ThreePointArc | T::CenterArc | T::Pie => {
                let mut n = match tool {
                    T::Circle => app.tools.circle_segments(),
                    T::Polygon => app.tools.polygon_segments(),
                    _ => app.tools.arc_segments(),
                };
                ui.label("Segments");
                if ui.add(crate::widgets::Number::new(&mut n).range(3..=360)).changed() {
                    app.tools.set_curve_segments(n);
                }
            }
            T::PaintBucket => {
                let mut surface = app.tools.paint_surface();
                if crate::widgets::checkbox(ui, &mut surface, "Paint curved surface").changed() {
                    app.tools.set_paint_surface(surface);
                }
                if ui.button("Materials...").clicked() {
                    app.show_materials = true;
                }
            }
            _ => {}
        }
    });
}
pub fn application_settings(ui: &mut egui::Ui, app: &mut App3d, ctx: &egui::Context) {
    match app.settings_tab {
        3 => {
            theme::section(ui, "Startup & power", "Preferences apply to this device. They do not change the model.");
            crate::widgets::checkbox(ui, &mut app.preferences.show_welcome, "Show welcome page at startup");
            crate::widgets::checkbox(ui, &mut app.preferences.sleep_renderer, "Reduce rendering when idle")
                .on_hover_text(
                    "Render in response to input. A one-second heartbeat keeps recovery and background work running.",
                );
            if let Some(error) = &app.preference_store.error {
                ui.colored_label(theme::ACCENT, format!("Preferences could not be saved: {error}"));
            }
            theme::section(ui, "Files", "Save your model file to keep a portable copy.");
            #[cfg(target_arch = "wasm32")]
            {
                crate::widgets::checkbox(ui, &mut app.autosave_settings.enabled, "Enable browser recovery auto-save");
                theme::field(ui, "Save every", |ui| {
                    ui.add_enabled(
                        app.autosave_settings.enabled,
                        crate::widgets::Number::new(&mut app.autosave_settings.minutes).range(1..=120).suffix(" min"),
                    );
                });
                ui.weak("Recovery and recent models are stored in this browser. Storage can be unavailable in private mode.");
            }
            #[cfg(not(target_arch = "wasm32"))]
            ui.weak("Use Save or Save As to write a model to disk.");
        }
        4 => {
            theme::section(
                ui,
                "Mouse navigation",
                "Middle drag orbits; Shift + middle drag pans. Scroll zooms toward the cursor.",
            );
            theme::field(ui, "Orbit / pan speed", |ui| {
                ui.add(crate::widgets::Slider::new(&mut app.preferences.orbit_sensitivity, 0.25..=3.).suffix("×"));
            });
            theme::field(ui, "Wheel zoom speed", |ui| {
                ui.add(crate::widgets::Slider::new(&mut app.preferences.zoom_sensitivity, 0.25..=3.).suffix("×"));
            });
            crate::widgets::checkbox(ui, &mut app.preferences.reverse_zoom, "Reverse mouse wheel zoom");
            crate::widgets::checkbox(
                ui,
                &mut app.preferences.shift_middle_orbits,
                "Pan with middle drag; Shift + middle drag orbits",
            );
            theme::section(
                ui,
                "Touch",
                "Tap to draw or select. Two fingers pan and pinch. Long press opens the context menu.",
            );
            crate::widgets::checkbox(ui, &mut app.touch.enabled, "Touch layout with on-screen modifiers");
            theme::section(
                ui,
                "Keyboard",
                "Tool shortcuts work when Measurements is empty. Typing numbers enters a precise measurement.",
            );
            ui.label("Space Select · L Line · R Rectangle · C Circle · P Push/Pull\nM Move · Q Rotate · S Scale · B Paint Bucket · O Orbit · H Pan\nCtrl+C / V Copy / Paste · Ctrl+D Duplicate · Ctrl+Z Undo\nShift+S Search commands · F1 Tool help · Esc Cancel");
            if ui.button("Reset navigation controls").clicked() {
                let defaults = crate::preferences::Preferences::default();
                app.preferences.orbit_sensitivity = defaults.orbit_sensitivity;
                app.preferences.zoom_sensitivity = defaults.zoom_sensitivity;
                app.preferences.reverse_zoom = false;
                app.preferences.shift_middle_orbits = false;
                app.touch.enabled = false;
            }
        }
        5 => {
            theme::section(
                ui,
                "Workspace",
                "Window positions and sizes are restored and constrained to the current viewport.",
            );
            crate::widgets::checkbox(ui, &mut app.preferences.remember_layout, "Remember window layout");
            crate::widgets::checkbox(ui, &mut app.preferences.tool_panel, "Show contextual tool options");
            crate::widgets::checkbox(ui, &mut app.show_entity_info, "Show Inspector");
            if ui.button("Reset window layout").clicked() {
                crate::preferences::reset_layout(ctx, app);
            }
        }
        _ => {}
    }
}
