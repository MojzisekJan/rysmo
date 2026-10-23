//! Shared editor surfaces, control contrast and spacing.
use bevy_egui::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle};
#[cfg(not(target_os = "macos"))]
const BG: Color32 = Color32::from_rgb(0x1a, 0x1e, 0x25);
#[cfg(not(target_os = "macos"))]
const SURFACE: Color32 = Color32::from_rgb(0x24, 0x2a, 0x33);
#[cfg(not(target_os = "macos"))]
const BORDER: Color32 = Color32::from_rgb(0x66, 0x75, 0x8a);
#[cfg(not(target_os = "macos"))]
const TEXT: Color32 = Color32::from_rgb(0xf1, 0xf4, 0xf8);
#[cfg(target_os = "macos")]
use crate::macos::{BG, BORDER, SURFACE, TEXT};
pub const ACCENT: Color32 = Color32::from_rgb(0x73, 0xad, 0xff);

/// Shared compact chrome. Content keeps its own comfortable padding.
///
/// Windows stay inside the screen's safe content area and never start larger than it.
/// A window without a `default_pos` opens centred, like a dialog; one with a position
/// is a tool panel and reopens where it was left.
pub struct Window<'a> {
    inner: egui::Window<'a>,
    key: String,
    has_close: bool,
    pos: Option<egui::Pos2>,
    size: Option<egui::Vec2>,
    width: Option<f32>,
    min: Option<egui::Vec2>,
    max: egui::Vec2,
    constrain: Option<egui::Rect>,
    beside: bool,
}

/// Dialogs use macOS chrome natively and in browsers on Apple platforms.
fn mac_chrome() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        static APPLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *APPLE.get_or_init(crate::web::apple_platform)
    }
    #[cfg(not(target_arch = "wasm32"))]
    cfg!(target_os = "macos")
}

pub fn window(title: &str) -> Window<'_> {
    Window {
        // macOS windows have no collapse arrow; their close dot is painted below.
        inner: base_window(title).collapsible(!mac_chrome()),
        key: title.into(),
        has_close: false,
        pos: None,
        size: None,
        width: None,
        min: None,
        max: egui::Vec2::INFINITY,
        constrain: None,
        beside: false,
    }
}

macro_rules! window_option {
    ($name:ident, $ty:ty) => {
        pub fn $name(mut self, value: $ty) -> Self {
            self.inner = self.inner.$name(value);
            self
        }
    };
}

impl<'a> Window<'a> {
    window_option!(id, egui::Id);
    window_option!(resizable, bool);
    window_option!(collapsible, bool);
    window_option!(vscroll, bool);

    pub fn default_pos(mut self, pos: impl Into<egui::Pos2>) -> Self {
        self.pos = Some(pos.into());
        self
    }
    pub fn default_size(mut self, size: impl Into<egui::Vec2>) -> Self {
        self.size = Some(size.into());
        self
    }
    pub fn default_width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn max_width(mut self, width: f32) -> Self {
        self.max.x = width;
        self
    }
    pub fn max_height(mut self, height: f32) -> Self {
        self.max.y = height;
        self
    }
    pub fn min_size(mut self, size: impl Into<egui::Vec2>) -> Self {
        self.min = Some(size.into());
        self
    }
    /// A panel used while working in the model opens at the right edge, not over it.
    pub fn beside_model(mut self) -> Self {
        self.beside = true;
        self
    }
    pub fn constrain_to(mut self, rect: egui::Rect) -> Self {
        self.constrain = Some(rect);
        self
    }

    pub fn open(mut self, open: &'a mut bool) -> Self {
        self.inner = self.inner.open(open);
        self.has_close = true;
        self
    }

    pub fn show<R>(
        mut self,
        ctx: &egui::Context,
        contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>> {
        if let Some(dock) = docked(ctx, &self.key) {
            // A tablet sidebar tab: fill the sidebar and keep the floating layout untouched.
            let window = self.inner.fixed_rect(dock).title_bar(false).resizable(false).movable(false);
            return window.constrain_to(dock).show(ctx, contents);
        }
        // Keep floating windows clear of the toolbars and status bar unless the model area is too small.
        let content = ctx.content_rect();
        let central = ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new(MODEL_AREA))).unwrap_or(content);
        let area = if central.width() >= 360. && central.height() >= 360. { central } else { content };
        let area = area.shrink(8.);
        let area = self.constrain.map_or(area, |r| r.intersect(area));
        let fit = |v: egui::Vec2| v.min(area.size()).max(egui::Vec2::ZERO);
        self.inner = self.inner.constrain_to(area).max_size(self.max.min(area.size()));
        if let Some(min) = self.min {
            self.inner = self.inner.min_size(fit(min));
        }
        if let Some(width) = self.width {
            self.inner = self.inner.default_width(width.min(area.width()));
        }
        let mut size = self.size.map(fit);
        let mut layout = crate::preferences::layout(ctx);
        let saved = layout.0.get(&self.key).copied();
        if let Some(r) = saved {
            size = Some(fit(egui::vec2(r[2], r[3])));
        }
        if let Some(size) = size {
            self.inner = self.inner.default_size(size);
        }
        if self.beside && self.pos.is_none() {
            let width = size.map_or(self.width.unwrap_or(300.), |s| s.x).min(area.width());
            self.pos = Some(egui::pos2(area.right() - width, area.top() + 96.));
        }
        match (self.pos, saved) {
            (Some(_), Some(r)) => {
                let size = size.unwrap_or_default();
                let pos = egui::pos2(
                    r[0].clamp(area.left(), (area.right() - size.x).max(area.left())),
                    r[1].clamp(area.top(), (area.bottom() - size.y).max(area.top())),
                );
                self.inner = self.inner.default_pos(pos);
            }
            (Some(pos), None) => {
                self.inner = self.inner.default_pos(pos.clamp(area.min, area.max));
            }
            (None, _) => {
                self.inner = self.inner.pivot(egui::Align2::CENTER_CENTER).default_pos(area.center());
            }
        }
        let result = self.inner.show(ctx, contents);
        if let Some(window) = &result {
            let r = window.response.rect;
            layout.0.insert(self.key, [r.left(), r.top(), r.width(), r.height()]);
            crate::preferences::set_layout(ctx, layout);
        }
        let mac = mac_chrome();
        #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
        let mac = mac
            || (std::env::var_os("RYSMO_MAC_CHROME_PREVIEW").is_some()
                && std::env::var_os("RYSMO_SCREENSHOT").is_some());
        if mac
            && self.has_close
            && let Some(window) = &result
        {
            paint_mac_close(ctx, &window.response);
        }
        result
    }
}

const MODEL_AREA: &str = "model-area";

/// Records the model area between the toolbars, palette and status bar for floating windows.
pub fn set_model_area(ctx: &egui::Context, rect: egui::Rect) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(MODEL_AREA), rect));
}

/// Places the window titled `key` in `rect` this frame, as a tab of the tablet sidebar.
pub fn dock(ctx: &egui::Context, key: &str, rect: egui::Rect) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("tablet-dock"), Some((key.to_owned(), rect))));
}

/// Ends any docking, so windows float again.
pub fn undock(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp::<Option<(String, egui::Rect)>>(egui::Id::new("tablet-dock"), None));
}

fn docked(ctx: &egui::Context, key: &str) -> Option<egui::Rect> {
    ctx.data(|d| d.get_temp::<Option<(String, egui::Rect)>>(egui::Id::new("tablet-dock")))
        .flatten()
        .filter(|(docked, _)| docked == key)
        .map(|(_, rect)| rect)
}

// Keep egui's close interaction, accessibility and window dragging/resizing intact.
// Only replace the painted glyph, on the same layer as its owning window.
fn paint_mac_close(ctx: &egui::Context, window: &egui::Response) {
    let heading = egui::TextStyle::Heading.resolve(&ctx.style_of(egui::Theme::Dark));
    let row = ctx.fonts_mut(|fonts| fonts.row_height(&heading));
    let center = egui::pos2(window.rect.right() - 10. - row / 2., window.rect.top() + 4. + row / 2.);
    let target = egui::Rect::from_center_size(center, egui::Vec2::splat(16.));
    let hovered =
        ctx.layer_id_at(center) == Some(window.layer_id) && ctx.pointer_hover_pos().is_some_and(|p| target.contains(p));
    let active = ctx.top_layer_id() == Some(window.layer_id);
    let background = if active { ctx.style_of(egui::Theme::Dark).visuals.widgets.open.weak_bg_fill } else { SURFACE };
    let painter = ctx.layer_painter(window.layer_id).with_clip_rect(window.rect);
    painter.rect_filled(target.expand(1.), 0., background);
    painter.circle(center, 6., Color32::from_rgb(255, 95, 87), Stroke::new(0.75, Color32::from_rgb(218, 65, 58)));
    if hovered {
        let d = egui::vec2(2.3, 2.3);
        let stroke = Stroke::new(1., Color32::from_rgb(100, 22, 19));
        painter.line_segment([center - d, center + d], stroke);
        painter.line_segment([center + egui::vec2(-d.x, d.y), center + egui::vec2(d.x, -d.y)], stroke);
    }
}

fn base_window(title: &str) -> egui::Window<'_> {
    egui::Window::new(egui::RichText::new(title).size(15.)).title_frame(
        egui::Frame::new()
            .fill(SURFACE)
            .inner_margin(egui::Margin::symmetric(10, 4))
            .corner_radius(CornerRadius::same(10)),
    )
}

pub fn singleline(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text).margin(egui::Margin::symmetric(10, 7))
}
pub fn multiline(text: &mut dyn egui::TextBuffer) -> egui::TextEdit<'_> {
    egui::TextEdit::multiline(text).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(10, 8))
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    #[cfg(target_os = "macos")]
    crate::macos::fonts(&mut fonts);
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::ThemePreference::Dark);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.window_stroke = Stroke::new(1., Color32::from_rgb(0x48, 0x55, 0x68));
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(8);
    visuals.override_text_color = Some(TEXT);
    visuals.weak_text_color = Some(Color32::from_rgb(0xb6, 0xc2, 0xd3));
    visuals.extreme_bg_color = Color32::from_rgb(0x15, 0x1b, 0x23);
    visuals.text_edit_bg_color = Some(visuals.extreme_bg_color);
    visuals.faint_bg_color = Color32::from_rgb(0x2b, 0x33, 0x3e);
    visuals.selection.bg_fill = Color32::from_rgb(0x2c, 0x52, 0x84);
    visuals.selection.stroke = Stroke::new(1., ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1., Color32::from_rgb(0x40, 0x4b, 0x5b));
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1., TEXT);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(5);
        widget.bg_stroke = Stroke::new(1., BORDER);
        widget.fg_stroke = Stroke::new(1., TEXT);
    }
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x3a, 0x46, 0x58);
    visuals.widgets.inactive.weak_bg_fill = visuals.widgets.inactive.bg_fill;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x4b, 0x5f, 0x79);
    visuals.widgets.hovered.weak_bg_fill = visuals.widgets.hovered.bg_fill;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1., Color32::from_rgb(0x9b, 0xc0, 0xf2));
    visuals.widgets.active.bg_fill = Color32::from_rgb(0x2c, 0x52, 0x84);
    visuals.widgets.active.weak_bg_fill = visuals.widgets.active.bg_fill;
    visuals.widgets.active.bg_stroke = Stroke::new(1., ACCENT);
    visuals.widgets.open.bg_fill = Color32::from_rgb(0x35, 0x48, 0x64);
    visuals.widgets.open.weak_bg_fill = Color32::from_rgb(0x2c, 0x33, 0x3e);
    visuals.widgets.open.bg_stroke = Stroke::new(1., ACCENT);
    ctx.set_visuals(visuals);
    ctx.all_styles_mut(|style| {
        style.spacing.button_padding = egui::vec2(10., 7.);
        style.spacing.item_spacing = egui::vec2(9., 8.);
        style.spacing.interact_size = egui::vec2(44., 32.);
        style.spacing.window_margin = egui::Margin::same(12);
        style.spacing.menu_margin = egui::Margin::same(6);
        style.spacing.icon_spacing = 8.;
        style.spacing.menu_width = 340.;
        style.text_styles.insert(TextStyle::Body, FontId::proportional(15.));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(15.));
        style.text_styles.insert(TextStyle::Heading, FontId::proportional(19.));
        style.text_styles.insert(TextStyle::Small, FontId::proportional(12.));
    });
    #[cfg(target_os = "macos")]
    crate::macos::install(ctx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_circle_matches_the_real_close_target() {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut open = true;
        let mut center = egui::Pos2::ZERO;
        for pass in 0..6 {
            let events = match pass {
                4 => vec![
                    egui::Event::PointerMoved(center),
                    egui::Event::PointerButton {
                        pos: center,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                5 => vec![egui::Event::PointerButton {
                    pos: center,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                _ => vec![],
            };
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 600.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if let Some(result) =
                        window("Close target").open(&mut open).default_pos(egui::pos2(120., 80.)).show(ui.ctx(), |ui| {
                            ui.label("Content");
                        })
                    {
                        paint_mac_close(ui.ctx(), &result.response);
                        let heading = egui::TextStyle::Heading.resolve(ui.style());
                        let row = ui.fonts_mut(|f| f.row_height(&heading));
                        center = egui::pos2(
                            result.response.rect.right() - 10. - row / 2.,
                            result.response.rect.top() + 4. + row / 2.,
                        );
                    }
                },
            );
            output.textures_delta.clear();
        }
        assert!(!open, "the painted macOS circle must close its owning window");
    }
}

/// One aligned, responsive form row shared by application settings and tool options.
pub fn field<R>(ui: &mut egui::Ui, label: &str, control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.horizontal(|ui| {
        ui.add_sized([(ui.available_width() * 0.38).clamp(100., 180.), 32.], egui::Label::new(label));
        control(ui)
    })
    .inner
}
pub fn section(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.add_space(8.);
    ui.strong(title);
    if !description.is_empty() {
        ui.weak(description);
    }
    ui.add_space(6.);
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    fn draw(ctx: &egui::Context, size: egui::Vec2) -> egui::Rect {
        let mut rect = egui::Rect::NOTHING;
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ui| {
                    rect = window("Remember layout")
                        .default_pos(egui::pos2(1200., 800.))
                        .default_size(egui::vec2(320., 240.))
                        .show(ui.ctx(), |ui| {
                            ui.label("Window layout");
                        })
                        .unwrap()
                        .response
                        .rect;
                },
            );
            output.textures_delta.clear();
        }
        rect
    }
    #[test]
    fn window_layout_restores_on_smaller_viewports_without_size_drift() {
        let first = egui::Context::default();
        install(&first);
        draw(&first, egui::vec2(1600., 1100.));
        let second = egui::Context::default();
        install(&second);
        crate::preferences::set_layout(&second, crate::preferences::layout(&first));
        let r = draw(&second, egui::vec2(800., 600.));
        assert!(r.left() >= 0. && r.top() >= 0. && r.right() <= 800. && r.bottom() <= 600., "{r:?}");
        let third = egui::Context::default();
        install(&third);
        crate::preferences::set_layout(&third, crate::preferences::layout(&second));
        let again = draw(&third, egui::vec2(800., 600.));
        assert!((again.width() - r.width()).abs() < 1. && (again.height() - r.height()).abs() < 1.);
    }
}

/// Font updates installed during an egui pass become available on its next pass.
pub fn fonts_ready(ctx: &egui::Context) -> bool {
    ctx.fonts(|fonts| fonts.families().contains(&egui::FontFamily::Name("phosphor".into())))
}
#[cfg(test)]
mod font_startup_tests {
    use super::*;
    #[test]
    fn welcome_waits_for_fonts_installed_during_the_first_pass() {
        let ctx = egui::Context::default();
        let mut app = crate::state::App3d::new(rysmo_model::Editor::new());
        app.welcome = true;
        for pass in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960., 640.))),
                    ..Default::default()
                },
                |ui| {
                    if pass == 0 {
                        install(ui.ctx());
                    }
                    assert_eq!(fonts_ready(ui.ctx()), pass > 0);
                    crate::welcome::show(ui.ctx(), &mut app);
                },
            );
            output.textures_delta.clear();
        }
    }
}

#[cfg(test)]
mod window_placement_tests {
    use super::*;

    fn rects(screen: egui::Vec2, insets: egui::epaint::MarginF32) -> (egui::Rect, egui::Rect, egui::Rect) {
        let ctx = egui::Context::default();
        install(&ctx);
        let (mut dialog, mut panel, mut content) = (egui::Rect::NOTHING, egui::Rect::NOTHING, egui::Rect::NOTHING);
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen)),
                    safe_area_insets: Some(egui::SafeAreaInsets(insets)),
                    ..Default::default()
                },
                |ui| {
                    let ctx = ui.ctx();
                    content = ctx.content_rect();
                    dialog = window("Dialog")
                        .min_size(egui::vec2(340., 420.))
                        .show(ctx, |ui| ui.label("Text"))
                        .unwrap()
                        .response
                        .rect;
                    panel = window("Panel")
                        .default_pos(egui::pos2(2000., 20.))
                        .default_size(egui::vec2(900., 900.))
                        .show(ctx, |ui| ui.label("Text"))
                        .unwrap()
                        .response
                        .rect;
                },
            );
            output.textures_delta.clear();
        }
        (dialog, panel, content)
    }

    #[test]
    fn dialogs_open_centred_and_every_window_fits_the_safe_area() {
        let notch = egui::epaint::MarginF32 { left: 0., right: 0., top: 62., bottom: 34. };
        for (screen, insets) in [
            (egui::vec2(1280., 800.), egui::epaint::MarginF32::ZERO),
            (egui::vec2(744., 1133.), egui::epaint::MarginF32 { top: 24., bottom: 20., ..notch }),
            (egui::vec2(402., 874.), notch),
            (egui::vec2(960., 420.), egui::epaint::MarginF32::ZERO),
        ] {
            let (dialog, panel, content) = rects(screen, insets);
            assert!((dialog.center() - content.center()).length() < 2., "{screen:?}: {dialog:?} in {content:?}");
            for r in [dialog, panel] {
                assert!(content.expand(0.5).contains_rect(r), "{screen:?}: {r:?} outside {content:?}");
            }
        }
    }
}
