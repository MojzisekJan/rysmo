//! Welcome page uses the same guarded file operations as the editor menus.
use crate::menu::glyph;
use crate::{state::App3d, unsaved};
use bevy_egui::egui;

/// Width of the centred welcome column.
const COLUMN: f32 = 760.;

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
    egui::Area::new("welcome-page".into())
        .fixed_pos(content.min)
        .order(egui::Order::Background)
        // A fade-in can stall half way when the renderer goes idle, leaving the editor showing through.
        .fade_in(false)
        .show(ctx, |ui| {
            #[cfg(target_os = "macos")]
            crate::macos::titlebar_area(ui, egui::Rect::from_min_size(content.min, egui::vec2(content.width(), 28.)));
            paint_glow(ui, content);
            let frame =
                egui::Frame::new().inner_margin(egui::Margin { top: margin + title, ..egui::Margin::same(margin) });
            frame.show(ui, |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                // An Area does not limit its width, so wrapping needs the page size set here.
                let inset = egui::vec2(2. * margin as f32, 2. * margin as f32 + title as f32);
                let page = (content.size() - inset).max(egui::vec2(240., 240.));
                ui.set_min_size(page);
                ui.set_max_size(page);
                egui::ScrollArea::vertical().id_salt("welcome-scroll").show(ui, |ui| {
                    // Centre the column vertically using the height measured in the previous frame.
                    let height_id = egui::Id::new("welcome-height");
                    let last = ui.ctx().data(|d| d.get_temp::<f32>(height_id)).unwrap_or(page.y);
                    ui.add_space(((page.y - last) / 2.).max(0.));
                    let width = ui.available_width().min(COLUMN);
                    let top = ui.cursor().top();
                    ui.horizontal(|ui| {
                        ui.add_space(((ui.available_width() - width) / 2.).max(0.));
                        ui.vertical(|ui| {
                            ui.set_width(width);
                            page_contents(ui, app, width);
                        });
                    });
                    let height = ui.cursor().top() - top;
                    ui.ctx().data_mut(|d| d.insert_temp(height_id, height));
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

fn page_contents(ui: &mut egui::Ui, app: &mut App3d, width: f32) {
    // Logo beside the name and tagline, the pair centred in the column.
    let logo = 128.;
    let name = ui.painter().layout_no_wrap("Rysmo".into(), egui::FontId::proportional(48.), egui::Color32::PLACEHOLDER);
    let tagline = ui.painter().layout_no_wrap(
        "A simple 3D modeller.".into(),
        egui::FontId::proportional(20.),
        egui::Color32::PLACEHOLDER,
    );
    let gap = 24.;
    let block = egui::vec2(logo + gap + name.size().x.max(tagline.size().x), logo);
    ui.horizontal(|ui| {
        ui.add_space(((width - block.x) / 2.).max(0.));
        crate::brand::image(ui, logo);
        ui.add_space(gap - ui.spacing().item_spacing.x);
        ui.vertical(|ui| {
            let text = name.size().y + 4. + tagline.size().y;
            ui.add_space(((logo - text) / 2.).max(0.));
            ui.spacing_mut().item_spacing.y = 4.;
            ui.label(egui::RichText::new("Rysmo").size(48.).strong());
            ui.weak(egui::RichText::new("A simple 3D modeller.").size(20.));
        });
    });
    ui.add_space(28.);
    section(ui, "New model");
    let units = [
        (glyph::RULER, "Millimetres", "Products and furniture", rysmo_model::Units::Millimetres),
        (glyph::BUILDINGS, "Metres", "Buildings, rooms and sites", rysmo_model::Units::Metres),
        (glyph::HOUSE_LINE, "Feet & inches", "Imperial building projects", rysmo_model::Units::Architectural),
    ];
    let columns = if width >= 540. { 3 } else { 1 };
    let gap = ui.spacing().item_spacing.x;
    // Narrow pages stack compact cards with the icon beside the text.
    let card = egui::vec2((width - gap * (columns - 1) as f32) / columns as f32, if columns == 1 { 64. } else { 84. });
    for row in units.chunks(columns) {
        ui.horizontal(|ui| {
            for (icon, label, detail, units) in row {
                if unit_card(ui, card, icon, label, detail).clicked() {
                    unsaved::request(app, unsaved::Action::NewWithUnits(*units));
                }
            }
        });
    }
    ui.add_space(6.);
    // Secondary actions share the card grid: four equal columns, or two on narrow pages.
    let current = app.path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
    let resume = match &current {
        Some(name) => format!("Continue {name}"),
        None => "Continue editing".to_owned(),
    };
    let actions = [
        (glyph::FOLDER_OPEN, "Open model...".to_owned()),
        (glyph::ARROW_RIGHT, resume),
        (glyph::QUESTION, "Help & tools".to_owned()),
        (glyph::GEAR_SIX, "Settings".to_owned()),
    ];
    let per_row = if width >= 540. { 4 } else { 2 };
    let button = egui::vec2((width - gap * (per_row - 1) as f32) / per_row as f32, 36.);
    for (row, chunk) in actions.chunks(per_row).enumerate() {
        ui.horizontal(|ui| {
            for (column, (icon, text)) in chunk.iter().enumerate() {
                let clicked = icon_button(ui, button, icon, text, egui::Align::Center).clicked();
                match (clicked, row * per_row + column) {
                    (true, 0) => unsaved::request(app, unsaved::Action::Open),
                    (true, 1) => app.welcome = false,
                    (true, 2) => app.help.open = true,
                    (true, _) => {
                        app.show_settings = true;
                        app.settings_tab = 3;
                    }
                    _ => {}
                }
            }
        });
    }
    ui.add_space(28.);
    section(ui, "Recent models");
    egui::Frame::new()
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(8)
        .inner_margin(6)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if app.recent.is_empty() {
                // Paint the hint at the exact centre of a fixed-height row.
                let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.), egui::Sense::hover());
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Models you open or save will appear here.",
                    egui::FontId::proportional(14.),
                    ui.visuals().weak_text_color(),
                );
            }
            for name in app.recent.clone().iter().take(8) {
                let label = std::path::Path::new(name).file_name().unwrap_or_default().to_string_lossy();
                let size = egui::vec2(ui.available_width(), 36.);
                if icon_button(ui, size, glyph::FILE, &label, egui::Align::Min).on_hover_text(name).clicked() {
                    unsaved::request(app, unsaved::Action::Recent(name.clone()));
                }
            }
        });
    ui.add_space(20.);
    ui.horizontal_wrapped(|ui| {
        crate::widgets::checkbox(ui, &mut app.preferences.show_welcome, "Show this page at startup");
        if width < 640. {
            // Keep the shortcut hint on its own line instead of wrapping it beside the checkbox.
            ui.end_row();
        } else {
            ui.add_space(12.);
        }
        ui.weak("Space selects · R draws a rectangle · P pulls a face · Shift+S searches commands");
    });
    if let Some(message) = &app.message {
        ui.add_space(8.);
        ui.label(message);
    }
}

/// A button whose icon and text are positioned separately: the icon font sits higher than
/// the text's visual centre when both share a baseline.
fn icon_button(ui: &mut egui::Ui, size: egui::Vec2, icon: &str, text: &str, align: egui::Align) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
    let style = ui.style().interact(&response);
    let painter = ui.painter();
    let flat = align == egui::Align::Min && !response.hovered() && !response.is_pointer_button_down_on();
    if !flat {
        painter.rect(rect, style.corner_radius, style.weak_bg_fill, style.bg_stroke, egui::StrokeKind::Inside);
    }
    let color = style.text_color();
    let icon = painter.layout_no_wrap(
        icon.to_owned(),
        egui::FontId::new(18., egui::FontFamily::Name("phosphor".into())),
        color,
    );
    let gap = 8.;
    let room = rect.width() - 24. - icon.size().x - gap;
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), egui::FontId::proportional(15.), color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(room.max(0.));
    let label = painter.layout_job(job);
    let width = icon.size().x + gap + label.size().x;
    let left = match align {
        egui::Align::Center => rect.center().x - width / 2.,
        _ => rect.left() + 12.,
    };
    // Centre the visible ink of the icon and of the text (without descenders) on the row.
    let centre = rect.center().y;
    let icon_top = centre - ink(&icon, false).center().y;
    let label_top = centre - ink(&label, true).center().y;
    painter.galley(egui::pos2(left, icon_top), icon, color);
    painter.galley(egui::pos2(left + width - label.size().x, label_top), label, color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The painted bounds of a galley's glyphs, relative to its origin. With `body` set,
/// descenders are ignored so text centres on its letter bodies.
fn ink(galley: &egui::Galley, body: bool) -> egui::Rect {
    let mut bounds = egui::Rect::NOTHING;
    for row in &galley.rows {
        for glyph in &row.row.glyphs {
            if glyph.uv_rect.size.y <= 0. || (body && "gjpqy,;".contains(glyph.chr)) {
                continue;
            }
            let min = row.pos + glyph.pos.to_vec2() + glyph.uv_rect.offset;
            bounds = bounds.union(egui::Rect::from_min_size(min, glyph.uv_rect.size));
        }
    }
    if bounds.is_positive() { bounds } else { egui::Rect::from_min_size(egui::Pos2::ZERO, galley.size()) }
}

/// Soft, low-contrast colour glows behind the page, in the logo and accent hues. The
/// background is rendered once per page size into an opaque, dithered image, so the
/// faint gradients show no 8-bit banding.
fn paint_glow(ui: &egui::Ui, rect: egui::Rect) {
    const SCALE: f32 = 0.5;
    let size = [(rect.width() * SCALE).ceil().max(1.) as usize, (rect.height() * SCALE).ceil().max(1.) as usize];
    let id = egui::Id::new("welcome-glow");
    let cached = ui.ctx().data(|d| d.get_temp::<([usize; 2], egui::TextureHandle)>(id));
    let texture = match cached {
        Some((cached, texture)) if cached == size => texture,
        _ => {
            let texture = ui.ctx().load_texture(
                "welcome-glow",
                glow_image(size, ui.visuals().panel_fill),
                egui::TextureOptions::LINEAR,
            );
            ui.ctx().data_mut(|d| d.insert_temp(id, (size, texture.clone())));
            texture
        }
    };
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
        egui::Color32::WHITE,
    );
}

fn glow_image([w, h]: [usize; 2], base: egui::Color32) -> egui::ColorImage {
    let glows = [
        ([0.15, 0.15], 0.5, [0x4f, 0x8d, 0xf0], 0.18),
        ([0.88, 0.85], 0.5, [0xb0, 0x4c, 0xe6], 0.15),
        ([0.7, 0.02], 0.32, [0xff, 0x8a, 0x3d], 0.10),
    ];
    let span = w.max(h) as f32;
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let mut c = [base.r() as f32, base.g() as f32, base.b() as f32];
            for (at, radius, colour, strength) in glows {
                let dx = (x as f32 - at[0] * w as f32) / (radius * span);
                let dy = (y as f32 - at[1] * h as f32) / (radius * span);
                let weight = strength * (-2.5 * (dx * dx + dy * dy)).exp();
                for k in 0..3 {
                    c[k] += (colour[k] as f32 - c[k]) * weight;
                }
            }
            // Ordered dither of ±0.5 levels breaks up banding in the faint gradient.
            let dither = ((x * 7 + y * 13) % 16) as f32 / 16. - 0.5;
            let v = c.map(|v| (v + dither).round().clamp(0., 255.) as u8);
            pixels.push(egui::Color32::from_rgb(v[0], v[1], v[2]));
        }
    }
    egui::ColorImage { size: [w, h], pixels, source_size: egui::vec2(w as f32, h as f32) }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).size(13.).strong().color(ui.visuals().weak_text_color()));
    ui.add_space(2.);
}

/// A clickable card; hover changes only colours, so the layout never shifts.
fn unit_card(ui: &mut egui::Ui, size: egui::Vec2, icon: &str, label: &str, detail: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let visuals = ui.visuals();
    let (fill, stroke) = if response.is_pointer_button_down_on() {
        (visuals.widgets.active.bg_fill, visuals.widgets.active.bg_stroke)
    } else if response.hovered() || response.has_focus() {
        (visuals.widgets.hovered.bg_fill, visuals.widgets.hovered.bg_stroke)
    } else {
        (visuals.faint_bg_color, visuals.widgets.noninteractive.bg_stroke)
    };
    let painter = ui.painter();
    painter.rect(rect, 8., fill, egui::Stroke::new(1., stroke.color), egui::StrokeKind::Inside);
    // Large icon on the left, title and description centred vertically beside it.
    let icon_size = if rect.height() < 80. { 28. } else { 34. };
    let icon = painter.layout_no_wrap(
        icon.to_owned(),
        egui::FontId::new(icon_size, egui::FontFamily::Name("phosphor".into())),
        visuals.strong_text_color(),
    );
    let left = rect.left() + 18.;
    let icon_top = rect.center().y - ink(&icon, false).center().y;
    painter.galley(egui::pos2(left, icon_top), icon.clone(), visuals.strong_text_color());
    let text_left = left + icon.size().x + 14.;
    let title = painter.layout_no_wrap(label.to_owned(), egui::FontId::proportional(17.), egui::Color32::PLACEHOLDER);
    let detail = painter.layout(
        detail.to_owned(),
        egui::FontId::proportional(13.),
        visuals.weak_text_color(),
        (rect.right() - text_left - 14.).max(10.),
    );
    let height = title.size().y + 2. + detail.size().y;
    let top = rect.center().y - height / 2.;
    // The web build has no bold face. A second, fainter copy one physical pixel to the right
    // thickens the strokes evenly; sub-pixel offsets would round away for some titles.
    let pixel = 1. / ui.ctx().pixels_per_point();
    let text_left = (text_left / pixel).round() * pixel;
    let strong = visuals.strong_text_color();
    painter.galley(egui::pos2(text_left + pixel, top), title.clone(), strong.gamma_multiply(0.6));
    let below = top + title.size().y + 2.;
    painter.galley(egui::pos2(text_left, top), title, strong);
    painter.galley(egui::pos2(text_left, below), detail, visuals.weak_text_color());
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("New model in {}", label.to_lowercase()))
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
