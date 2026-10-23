//! Phosphor command icons with consistent menu rows and shortcut columns.
use bevy_egui::egui::Color32;
#[cfg(not(target_os = "macos"))]
use bevy_egui::egui::RichText;
use bevy_egui::egui::{self, FontId};
pub use egui_phosphor::regular as glyph;

#[cfg(target_os = "macos")]
use crate::macos::MENU_ICON;
#[cfg(not(target_os = "macos"))]
const MENU_ROW: f32 = 32.;
#[cfg(not(target_os = "macos"))]
const MENU_ICON: f32 = 18.;

pub fn style(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    style.spacing.button_padding = egui::vec2(10., 6.);
    style.spacing.item_spacing = egui::vec2(4., 2.);
}

/// An icon and text label. Both are centred on the row, because the icon font is larger
/// and would otherwise sit on the text baseline, above the text's visual centre.
/// Text size beside an icon in buttons and menu rows.
const LABEL_TEXT: f32 = if cfg!(target_os = "macos") { 13. } else { 15. };

pub fn label(icon: &str, text: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        icon,
        0.,
        egui::TextFormat {
            font_id: FontId::new(MENU_ICON, egui::FontFamily::Name("phosphor".into())),
            color: Color32::PLACEHOLDER,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    job.append(
        text,
        10.,
        egui::TextFormat {
            font_id: FontId::proportional(LABEL_TEXT),
            color: Color32::PLACEHOLDER,
            valign: egui::Align::Center,
            ..Default::default()
        },
    );
    job
}

pub fn button(ui: &mut egui::Ui, icon: &str, text: &str, shortcut: &str) -> egui::Response {
    enabled(ui, true, icon, text, shortcut)
}
pub fn enabled(ui: &mut egui::Ui, enabled: bool, icon: &str, text: &str, shortcut: &str) -> egui::Response {
    #[cfg(target_os = "macos")]
    {
        let item = egui_native_look::MenuItem::new(text)
            .shortcut(crate::macos::shortcut(shortcut))
            .icon(icon, FontId::new(MENU_ICON, egui::FontFamily::Name("phosphor".into())));
        let response = ui.add_enabled(enabled, item);
        if response.clicked() {
            ui.close();
        }
        response
    }
    #[cfg(not(target_os = "macos"))]
    enabled_row(ui, enabled, icon, text, shortcut)
}

#[cfg(not(target_os = "macos"))]
fn enabled_row(ui: &mut egui::Ui, enabled: bool, icon: &str, text: &str, shortcut: &str) -> egui::Response {
    #[cfg(target_os = "ios")]
    let shortcut = shortcut.replace("Ctrl", "Cmd");
    #[cfg(target_arch = "wasm32")]
    let shortcut = if crate::web::apple_platform() { shortcut.replace("Ctrl", "Cmd") } else { shortcut.to_owned() };
    #[cfg(not(any(target_os = "ios", target_arch = "wasm32")))]
    let shortcut = shortcut.to_owned();
    let response = ui.add_enabled(
        enabled,
        egui::Button::new(label(icon, text))
            .shortcut_text(RichText::new(shortcut).weak())
            .frame_when_inactive(false)
            .min_size(egui::vec2(MENU_ROW * 8., MENU_ROW)),
    );
    if response.clicked() {
        ui.close();
    }
    response
}
pub fn toggle(ui: &mut egui::Ui, icon: &str, text: &str, value: &mut bool) -> egui::Response {
    #[cfg(target_os = "macos")]
    let mut response = ui.add(
        egui_native_look::MenuItem::new(text)
            .icon(icon, FontId::new(MENU_ICON, egui::FontFamily::Name("phosphor".into())))
            .checked(*value),
    );
    #[cfg(not(target_os = "macos"))]
    let mut response = ui.add(
        egui::Button::new(label(icon, text))
            .shortcut_text(if *value { glyph::CHECK } else { "" })
            .frame_when_inactive(false)
            .min_size(egui::vec2(MENU_ROW * 8., MENU_ROW)),
    );
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
        ui.close();
    }
    response
}

/// A row that opens a submenu beside it; on macOS an AppKit item with a chevron.
pub fn submenu<R>(
    ui: &mut egui::Ui,
    icon: &str,
    text: &str,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    #[cfg(target_os = "macos")]
    {
        egui_native_look::MenuItem::new(text)
            .icon(icon, FontId::new(MENU_ICON, egui::FontFamily::Name("phosphor".into())))
            .submenu(ui, contents)
    }
    #[cfg(not(target_os = "macos"))]
    {
        ui.menu_button(label(icon, text), contents)
    }
}

/// Top-level menu with an opt-in screenshot fixture for repeatable visual checks.
pub fn top<R>(
    ui: &mut egui::Ui,
    title: &str,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let menu = ui.menu_button(title, contents);
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var("RYSMO_MENU_PREVIEW").as_deref() == Ok(title) {
        egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&menu.response));
    }
    menu
}
