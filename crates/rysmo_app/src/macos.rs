//! The macOS appearance, through egui_native_look: AppKit-style controls, San Francisco
//! and the accent colour from System Settings, over Rysmo's own dark palette. The window
//! keeps a transparent title bar whose menu row sits beside the traffic lights.
use bevy_egui::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle};
use egui_native_look as look;

pub const BG: Color32 = Color32::from_rgb(0x26, 0x26, 0x28);
pub const SURFACE: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2e);
pub const BORDER: Color32 = Color32::from_rgb(0x4a, 0x4a, 0x4d);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xe8, 0xea);

/// The size of a menu's text and icons.
pub const MENU_ICON: f32 = 15.;

/// Width of the traffic-light buttons the transparent title bar draws over the menu row.
pub const TRAFFIC_LIGHTS: f32 = 72.;

/// Puts the system's faces in front of egui's.
pub fn fonts(fonts: &mut egui::FontDefinitions) {
    look::install_fonts(fonts, look::Look::Mac);
}

/// Rysmo's dark greys as the library's palette; the controls take the accent from the
/// system and their own AppKit shades.
fn palette() -> look::Palette {
    look::Palette {
        fg: TEXT,
        dim: Color32::from_rgb(0x9d, 0x9d, 0xa2),
        disabled: Color32::from_rgb(0x5a, 0x5a, 0x5e),
        bg: BG,
        surface: SURFACE,
        border: Color32::from_rgb(0x3a, 0x3a, 0x3d),
        grid: Color32::from_rgb(0x48, 0x48, 0x4b),
        button: Color32::from_rgb(0x4a, 0x4a, 0x4d),
        button_hover: Color32::from_rgb(0x56, 0x56, 0x5a),
        button_pressed: Color32::from_rgb(0x63, 0x63, 0x67),
        button_disabled: Color32::from_rgb(0x2f, 0x2f, 0x31),
        entry: Color32::from_rgb(0x1c, 0x1c, 0x1e),
        accent: Color32::from_rgb(0x0a, 0x84, 0xff),
        accent_hover: Color32::from_rgb(0x3d, 0x9b, 0xff),
        ..look::Palette::of(true)
    }
}

/// Installs the macOS look over the shared theme, with macOS spacing, shadows and text.
pub fn install(ctx: &egui::Context) {
    let theme = look::Theme::new(look::Look::Mac)
        .schemes(look::Schemes { dark: palette(), light: palette() })
        .metrics(look::Metrics::of(look::Look::Mac).system_bold());
    look::install(ctx, theme);
    ctx.all_styles_mut(|style| {
        let visuals = &mut style.visuals;
        visuals.faint_bg_color = Color32::from_rgb(0x30, 0x30, 0x33);
        visuals.window_shadow =
            egui::epaint::Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(110) };
        visuals.popup_shadow =
            egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(90) };
        style.spacing.button_padding = egui::vec2(10., 4.);
        style.spacing.item_spacing = egui::vec2(8., 6.);
        style.spacing.window_margin = egui::Margin::same(14);
        style.spacing.menu_margin = egui::Margin::same(5);
        style.spacing.icon_width = 16.;
        style.spacing.icon_width_inner = 10.;
        style.spacing.icon_spacing = 6.;
        style.spacing.menu_width = 280.;
        style.spacing.slider_rail_height = 4.;
        style.text_styles.insert(TextStyle::Heading, FontId::new(16., look::bold_family()));
        style.text_styles.insert(TextStyle::Small, FontId::proportional(11.));
        style.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.));
    });
}

/// Writes a shortcut the way macOS menus do: `Ctrl+Shift+S` becomes `⇧⌘S`.
pub fn shortcut(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let (mut command, mut shift, mut option) = (false, false, false);
    let mut key = "";
    for part in text.split('+') {
        match part {
            "Ctrl" | "Cmd" => command = true,
            "Shift" => shift = true,
            "Alt" | "Option" => option = true,
            other => key = other,
        }
    }
    // macOS lists modifiers as Option, Shift, Command.
    let modifiers: String = [(option, '⌥'), (shift, '⇧'), (command, '⌘')]
        .into_iter()
        .filter_map(|(on, symbol)| on.then_some(symbol))
        .collect();
    let key = match key {
        "Delete" => "⌫",
        "Enter" => "↩",
        "Escape" => "⎋",
        "Tab" => "⇥",
        "Space" => "Space",
        "Up" => "↑",
        "Down" => "↓",
        "Left" => "←",
        "Right" => "→",
        other => other,
    };
    format!("{modifiers}{key}")
}

/// What the title-bar area asked of the window this frame, kept in egui's memory.
#[derive(Clone, Copy, Default)]
struct Titlebar {
    drag: bool,
    zoom: bool,
}

pub fn plugin(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.add_systems(bevy_egui::EguiPrimaryContextPass, apply_titlebar.after(crate::ui::Panels));
}

/// Makes the empty part of `rect` behave like a title bar: dragging moves the window and
/// a double click zooms it. Call it before the row's widgets so they keep their clicks.
pub fn titlebar_area(ui: &mut egui::Ui, rect: egui::Rect) {
    let response = ui.interact(rect, ui.scope_id().with("macos-titlebar"), egui::Sense::click_and_drag());
    // Read the response before locking the context's data: these queries lock it too.
    let (drag, zoom) = (response.drag_started(), response.double_clicked());
    if drag || zoom {
        let id = egui::Id::unique("macos-titlebar");
        ui.ctx().data_mut(|d| {
            let t: &mut Titlebar = d.get_temp_mut_or_default(id);
            t.drag |= drag;
            t.zoom |= zoom;
        });
    }
}

fn apply_titlebar(
    mut contexts: bevy_egui::EguiContexts,
    mut windows: bevy::prelude::Query<&mut bevy::window::Window, bevy::prelude::With<bevy::window::PrimaryWindow>>,
    mut zoomed: bevy::prelude::Local<bool>,
) -> bevy::prelude::Result {
    let ctx = contexts.ctx_mut()?;
    let t: Titlebar = ctx.data_mut(|d| d.remove_temp(egui::Id::unique("macos-titlebar"))).unwrap_or_default();
    if let Ok(mut window) = windows.single_mut() {
        if t.drag {
            window.start_drag_move();
        }
        if t.zoom {
            *zoomed = !*zoomed;
            window.set_maximized(*zoomed);
        }
    }
    Ok(())
}

/// Number fields look like AppKit text fields: dark, inset, with a hairline border.
/// The library's field is filled like a button.
pub fn field_visuals(ctx: &egui::Context, visuals: &mut egui::Visuals) {
    let c = look::colors(ctx);
    for (widget, border) in [
        (&mut visuals.widgets.inactive, c.control_border),
        (&mut visuals.widgets.hovered, c.control_border),
        (&mut visuals.widgets.active, c.accent),
    ] {
        widget.bg_fill = c.entry;
        widget.weak_bg_fill = c.entry;
        widget.bg_stroke = Stroke::new(1., border);
        widget.corner_radius = CornerRadius::same(5);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_use_mac_modifier_symbols_in_menu_order() {
        assert_eq!(shortcut("Ctrl+S"), "⌘S");
        assert_eq!(shortcut("Ctrl+Shift+S"), "⇧⌘S");
        assert_eq!(shortcut("Ctrl+Alt+Shift+W"), "⌥⇧⌘W");
        assert_eq!(shortcut("F1"), "F1");
        assert_eq!(shortcut("Delete"), "⌫");
        assert_eq!(shortcut(""), "");
    }
}
