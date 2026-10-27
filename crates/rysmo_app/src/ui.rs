//! Editor chrome: top menus, the left tool shelf, the status bar with the VCB,
//! the inference marker and tooltip by the cursor, the context menu, and
//! on touch screens the Shift and Ctrl toggles.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use rysmo_infer::Kind;
use rysmo_math::DVec2;
use rysmo_model::Command;
use rysmo_tools::{Key, ToolKind};

use crate::actions::{self, Action};
use crate::files;
use crate::input::{apply_vcb, redo, tool_key, undo};
use crate::menu::{self, glyph as g};
use crate::panels;
use crate::state::App3d;

pub fn plugin(app: &mut App) {
    app.add_systems(EguiPrimaryContextPass, (setup_fonts, panels.in_set(Panels), overlay).chain());
    // iOS has no window title, and changing `Window` there resets its size to the safe area.
    #[cfg(not(target_os = "ios"))]
    app.add_systems(Update, window_title);
}

fn setup_fonts(mut contexts: EguiContexts, mut done: Local<bool>) -> Result {
    if *done {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    crate::theme::install(ctx);
    *done = true;
    Ok(())
}

#[cfg_attr(target_os = "ios", allow(dead_code))]
fn window_title(app: Res<App3d>, mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    let Ok(mut w) = windows.single_mut() else { return };
    let title = files::title(app.path.as_deref(), &app.editor);
    if w.title != title {
        w.title = title;
    }
}

/// The system that lays out the editor's panels and windows.
#[derive(SystemSet, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Panels;

/// Toolbar entries in display order.
pub(crate) enum Slot {
    Tool(ToolKind),
    Action(Action),
    Extents,
    Gap,
}

const LEFT_TOOLS: &[Slot] = &[
    Slot::Tool(ToolKind::Select),
    Slot::Tool(ToolKind::Eraser),
    Slot::Gap,
    Slot::Tool(ToolKind::Line),
    Slot::Tool(ToolKind::Freehand),
    Slot::Tool(ToolKind::Arc),
    Slot::Tool(ToolKind::Rectangle),
    Slot::Tool(ToolKind::Circle),
    Slot::Tool(ToolKind::Polygon),
    Slot::Gap,
    Slot::Tool(ToolKind::PushPull),
    Slot::Tool(ToolKind::Offset),
    Slot::Tool(ToolKind::FollowMe),
    Slot::Tool(ToolKind::PaintBucket),
    Slot::Gap,
    Slot::Tool(ToolKind::Move),
    Slot::Tool(ToolKind::Rotate),
    Slot::Tool(ToolKind::Scale),
    Slot::Action(Action::MakeComponent),
];
const TOP_TOOLS: &[Slot] = &[
    Slot::Tool(ToolKind::Orbit),
    Slot::Tool(ToolKind::Pan),
    Slot::Tool(ToolKind::Zoom),
    Slot::Tool(ToolKind::ZoomWindow),
    Slot::Extents,
    Slot::Tool(ToolKind::PositionCamera),
    Slot::Tool(ToolKind::LookAround),
    Slot::Tool(ToolKind::Walk),
    Slot::Gap,
    Slot::Tool(ToolKind::RotatedRectangle),
    Slot::Tool(ToolKind::ThreePointArc),
    Slot::Tool(ToolKind::CenterArc),
    Slot::Tool(ToolKind::Pie),
    Slot::Tool(ToolKind::TapeMeasure),
    Slot::Tool(ToolKind::Protractor),
    Slot::Tool(ToolKind::Axes),
    Slot::Tool(ToolKind::Dimension),
    Slot::Tool(ToolKind::Text),
    Slot::Action(Action::Text3d),
    Slot::Action(Action::SectionPlane),
    Slot::Gap,
    Slot::Action(Action::FromContours),
    Slot::Action(Action::TerrainGrid),
    Slot::Action(Action::Smoove),
    Slot::Action(Action::Drape),
    Slot::Action(Action::Stamp),
    Slot::Action(Action::AddDetail),
    Slot::Action(Action::FlipEdge),
];

/// A shared button implementation keeps selection and hover consistent in both toolbars.
pub(crate) fn slot_button(ui: &mut egui::Ui, app: &mut App3d, slot: &Slot, icon_size: f32) {
    match slot {
        Slot::Tool(kind) => {
            let active = app.tools.active() == *kind
                && app.smoove.is_none()
                && app.terrain.is_none()
                && app.flip.is_none()
                && app.detail.is_none()
                && app.stamp.is_none()
                && app.drape.is_none()
                && app.section_placing.is_none()
                && app.text3d.is_none()
                && app.text_creation.is_none();
            let r = crate::icons::button(ui, crate::icons::Icon::Tool(*kind), active, icon_size)
                .on_hover_text(format!("{} ({})", kind.name(), kind.shortcut()));
            if r.clicked() {
                app.set_tool(*kind);
            }
        }
        Slot::Action(action) => {
            let active = (*action == Action::Drape && app.drape.is_some())
                || (*action == Action::Stamp && app.stamp.is_some())
                || (*action == Action::SectionPlane && app.section_placing.is_some())
                || (*action == Action::AddDetail && app.detail.is_some())
                || (*action == Action::FlipEdge && app.flip.is_some())
                || (*action == Action::Smoove && app.smoove.is_some())
                || (*action == Action::TerrainGrid && app.terrain.is_some());
            if crate::icons::button(ui, crate::icons::Icon::Action(*action), active, icon_size)
                .on_hover_text(action.label())
                .clicked()
            {
                actions::run(app, *action);
            }
        }
        Slot::Extents => {
            if crate::icons::button(ui, crate::icons::Icon::Extents, false, icon_size)
                .on_hover_text("Zoom Extents (Shift+Z)")
                .clicked()
            {
                app.zoom_extents();
            }
        }
        Slot::Gap => {
            ui.add_space(8.);
        }
    }
}

/// Inner height of toolbar trays: a 20-point icon button.
const TRAY_HEIGHT: f32 = 34.;

/// A rounded tray grouping related tool buttons.
fn tool_tray(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(4, 3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2., 2.);
            // A fixed inner height keeps every tray, and what sits in it, on one centre line.
            ui.set_height(TRAY_HEIGHT);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), add);
        });
}

/// Width of a palette tray: its icon grid plus the tray's margins.
fn palette_tray_width(columns: usize, touch: bool) -> f32 {
    let icon = if touch { 26. } else { 22. } + 14.;
    columns as f32 * icon + (columns - 1) as f32 * 3. + 8.
}

/// Compact tool shelf: each group in a rounded tray, trays centred in the panel.
/// Short viewports use more columns instead of scrolling.
fn tool_palette(ui: &mut egui::Ui, app: &mut App3d, columns: usize) {
    let icon_size = if app.touch.enabled { 26. } else { 22. };
    let tray = palette_tray_width(columns, app.touch.enabled);
    for (group, slots) in LEFT_TOOLS.split(|slot| matches!(slot, Slot::Gap)).enumerate() {
        if group > 0 {
            ui.add_space(6.);
        }
        ui.vertical_centered(|ui| {
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .corner_radius(8)
                .inner_margin(egui::Margin::same(4))
                .show(ui, |ui| {
                    ui.set_width(tray - 8.);
                    egui::Grid::new(("tool-group", group))
                        .num_columns(columns)
                        .min_col_width(icon_size + 14.)
                        .spacing(egui::vec2(3., 3.))
                        .show(ui, |ui| {
                            for (index, slot) in slots.iter().enumerate() {
                                slot_button(ui, app, slot, icon_size);
                                if (index + 1) % columns == 0 {
                                    ui.end_row();
                                }
                            }
                            if slots.len() % columns != 0 {
                                ui.end_row();
                            }
                        });
                });
        });
    }
}

/// The frame of the editor's chrome panels: one fill, no edge lines.
fn chrome_frame(ui: &egui::Ui, x: i8, y: i8) -> egui::Frame {
    egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(x, y))
}

fn panels(
    mut contexts: EguiContexts,
    mut app: ResMut<App3d>,
    render_view: Res<crate::render_view::RenderView>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let ctx = &ctx;
    if !crate::theme::fonts_ready(ctx) {
        ctx.request_repaint();
        return Ok(());
    }
    crate::preferences::begin(ctx, &mut app);
    if crate::welcome::show(ctx, &mut app) {
        crate::preferences::end(ctx, &mut app);
        return Ok(());
    }
    let touch = app.touch.enabled;
    // egui panels live in a parent Ui; this one spans the window.
    let mut root = egui::Ui::new(
        ctx.clone(),
        egui::Id::unique("root"),
        egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.content_rect()),
    );
    let mut rects = Vec::new();

    if touch {
        crate::tablet::chrome(ctx, &mut root, &mut app, &mut rects);
    } else {
        crate::theme::undock(ctx);
        let top = egui::Panel::top("menu")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(root.visuals().panel_fill).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(&mut root, |ui| {
                #[cfg(target_os = "macos")]
                crate::macos::titlebar_area(ui, ui.max_rect().expand2(egui::vec2(100., 6.)));
                egui::MenuBar::new()
                    .style(menu::style)
                    .config(egui::containers::menu::MenuConfig::new().style(menu::style))
                    .ui(ui, |ui| {
                        // On macOS the menu row is the title bar; it starts after the traffic lights.
                        #[cfg(target_os = "macos")]
                        ui.add_space(crate::macos::TRAFFIC_LIGHTS);
                        if crate::brand::image(ui, 22.)
                            .interact(egui::Sense::click())
                            .on_hover_text("About Rysmo")
                            .clicked()
                        {
                            app.show_about = true;
                        }
                        menus(ui, &mut app);
                    });
                ui.add_space(2.);
                ui.horizontal(|ui| {
                    // Tool groups sit in quiet rounded trays; the panel toggle and command
                    // search stay at the right end.
                    ui.spacing_mut().item_spacing = egui::vec2(6., 3.);
                    for slots in TOP_TOOLS.split(|slot| matches!(slot, Slot::Gap)) {
                        tool_tray(ui, |ui| {
                            for slot in slots {
                                slot_button(ui, &mut app, slot, 20.);
                            }
                        });
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        tool_tray(ui, |ui| {
                            let search =
                                egui::Button::new(menu::label(menu::glyph::MAGNIFYING_GLASS, "Search commands"))
                                    .frame(false)
                                    .min_size(egui::vec2(0., TRAY_HEIGHT));
                            if ui.add(search).on_hover_text("Shift+S").clicked() {
                                crate::command_search::open(&mut app);
                            }
                            ui.add_space(6.);
                            ui.separator();
                            ui.add_space(6.);
                            let panel_open = crate::tablet::sidebar_open(&mut app);
                            let text = if panel_open { "Hide panel" } else { "Show panel" };
                            let panels = egui::Button::new(menu::label(menu::glyph::SIDEBAR_SIMPLE, text))
                                .frame(false)
                                .min_size(egui::vec2(0., TRAY_HEIGHT));
                            if ui.add(panels).on_hover_text("Show or hide the side panel").clicked() {
                                crate::tablet::toggle_sidebar(&mut app, ui.ctx());
                            }
                            ui.add_space(4.);
                        });
                    });
                });
            });
        rects.push(top.response.rect);
        if !app.editor.doc().scenes.is_empty() {
            let tabs = egui::Panel::top("scenes")
                .show_separator_line(false)
                .frame(chrome_frame(&root, 10, 4))
                .show(&mut root, |ui| crate::scene_panel::tabs(ui, &mut app));
            rects.push(tabs.response.rect);
        }

        if app.preferences.tool_panel {
            let options = egui::Panel::top("tool-options")
                .show_separator_line(false)
                .frame(chrome_frame(&root, 10, 4))
                .show(&mut root, |ui| crate::tool_panel::panel(ui, &mut app));
            rects.push(options.response.rect);
        }
        let bottom = egui::Panel::bottom("status").show_separator_line(false).frame(chrome_frame(&root, 10, 5)).show(
            &mut root,
            |ui| {
                if touch {
                    ui.horizontal(|ui| {
                        for (label, which) in [("Shift", 0), ("Ctrl", 1), ("Alt", 2)] {
                            let on = [app.touch.shift, app.touch.ctrl, app.touch.alt][which];
                            let b = egui::Button::new(egui::RichText::new(label).size(16.0))
                                .selected(on)
                                .min_size(egui::vec2(56.0, 32.0));
                            if ui.add(b).clicked() {
                                match which {
                                    0 => {
                                        app.touch.shift = !on;
                                        tool_key(&mut app, Key::Shift(!on));
                                    }
                                    1 => app.touch.ctrl = !on,
                                    _ => app.touch.alt = !on,
                                }
                            }
                        }
                    });
                }
                // One row, everything centred on the same line; the field keeps a visible edge.
                ui.horizontal_centered(|ui| {
                    ui.set_min_height(30.);
                    let status = app.status();
                    let (vcb_label, hint) = measurement_label(&app);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let field_width: f32 =
                            if matches!(app.tools.active(), ToolKind::Rectangle | ToolKind::RotatedRectangle) {
                                270.
                            } else {
                                170.
                            };
                        let edit = crate::theme::singleline(&mut app.vcb)
                            .background_color(ui.visuals().extreme_bg_color)
                            .hint_text(hint)
                            .desired_width(field_width.min((ui.available_width() * 0.4).max(90.)))
                            .font(egui::TextStyle::Monospace);
                        let r = ui.add(edit);
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            apply_vcb(&mut app);
                        }
                        ui.label(vcb_label);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            if ui
                                .add(egui::Button::new(app.tools.active().name()).frame(false))
                                .on_hover_text("Help for the active tool")
                                .clicked()
                            {
                                crate::help::open_current(&mut app);
                            }
                            if ui.available_width() > 420. {
                                let selected = app.editor.selection().len();
                                if ui.small_button(format!("{selected} selected")).clicked() {
                                    app.show_entity_info = true;
                                    app.inspector_tab = 1;
                                }
                                if ui
                                    .small_button(app.editor.doc().units.suffix())
                                    .on_hover_text("Model units")
                                    .clicked()
                                {
                                    app.show_settings = true;
                                    app.settings_tab = 1;
                                }
                            }
                            if app.editor.is_dirty() {
                                ui.label(egui::RichText::new("●").color(crate::theme::ACCENT))
                                    .on_hover_text("Unsaved changes");
                            }
                            if app.message.is_some() && ui.small_button("×").on_hover_text("Dismiss message").clicked()
                            {
                                app.message = None;
                            }
                            ui.add(egui::Label::new(&status).truncate()).on_hover_text(&status);
                        });
                    });
                });
            },
        );
        rects.push(bottom.response.rect);
        let columns = if root.available_rect_before_wrap().height() < if touch { 410. } else { 370. } { 4 } else { 2 };
        let palette_width = palette_tray_width(columns, touch) + 12.;
        let palette = egui::Panel::left("tool-palette")
            .show_separator_line(false)
            .frame(chrome_frame(&root, 6, 6))
            .default_size(palette_width)
            .size_range(palette_width..=palette_width)
            .resizable(false)
            .show(&mut root, |ui| {
                tool_palette(ui, &mut app, columns);
            });
        rects.push(palette.response.rect);
        crate::tablet::desktop_sidebar(&mut root, &mut app, &mut rects);
    }
    let canvas = root.available_rect_before_wrap();
    crate::theme::set_model_area(ctx, canvas);
    crate::features::photo::canvas(ctx, &mut root, canvas, &mut app);
    if crate::render::offscreen(&app) {
        let texture = contexts.add_image(bevy_egui::EguiTextureHandle::Strong(render_view.image.clone()));
        root.painter().image(
            texture,
            canvas,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            egui::Color32::WHITE,
        );
    }
    if app.show_entity_info {
        let mut open = true;
        let inspector = crate::theme::window("Inspector")
            .id(egui::Id::unique("floating-inspector"))
            .open(&mut open)
            .default_pos(egui::pos2((canvas.right() - 336.).max(canvas.left() + 8.), canvas.top() + 8.))
            .default_size(egui::vec2(320., (canvas.height() * 0.75).clamp(180., 520.)))
            .min_size(egui::vec2(280., 180.))
            .resizable(true)
            .vscroll(true)
            .constrain_to(canvas.shrink(8.))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (index, label) in ["Model", "Properties"].iter().enumerate() {
                        if crate::icons::tab(ui, label, app.inspector_tab == index).clicked() {
                            app.inspector_tab = index;
                            app.entity_edit_revision = None;
                            app.text_edit = None;
                        }
                    }
                });
                ui.separator();
                match app.inspector_tab {
                    1 => panels::entity_info_ui(ui, &mut app),
                    _ => {
                        ui.heading("Model");
                        ui.label(format!(
                            "{} faces · {} edges",
                            app.editor.mesh().faces().count(),
                            app.editor.mesh().edges().count()
                        ));
                        ui.add_space(8.);
                        if ui.button("Tags").clicked() {
                            app.show_tags = true;
                        }
                        if ui.button("Section Planes").clicked() {
                            app.show_sections = true;
                        }
                        ui.separator();
                        if ui.button("Components").clicked() {
                            app.show_components = true;
                        }
                        crate::component_panel::outliner(ui, &mut app);
                    }
                }
            });
        if let Some(inspector) = inspector {
            rects.push(inspector.response.rect);
        }
        app.show_entity_info = open;
    }
    if !app.show_entity_info {
        app.entity_edit_revision = None;
        app.text_edit = None;
    }
    app.canvas = Some((
        DVec2::new(canvas.min.x as f64, canvas.min.y as f64),
        DVec2::new(canvas.width() as f64, canvas.height() as f64),
    ));
    app.ui_rects = rects
        .iter()
        .map(|r| (DVec2::new(r.min.x as f64, r.min.y as f64), DVec2::new(r.max.x as f64, r.max.y as f64)))
        .collect();

    crate::portable_files::panel(ctx, &mut app);
    crate::texture_view::panel(ctx, &mut app);
    if let Some(at) = app.context_menu {
        let mut close = false;
        let mut open = true;
        let popup = egui::Popup::new(
            egui::Id::unique("context-menu"),
            ctx.clone(),
            egui::pos2(at.x as f32, at.y as f32),
            egui::LayerId::background(),
        );
        // A menu flipped above the pointer would take the opening button's release
        // for a click outside it.
        let popup =
            if app.context_menu_armed { popup } else { popup.close_behavior(egui::PopupCloseBehavior::IgnoreClicks) };
        popup
            .open_bool(&mut open)
            .kind(egui::PopupKind::Menu)
            .layout(egui::Layout::top_down_justified(egui::Align::Min))
            .show(|ui| {
                menu::style(ui.style_mut());
                ui.set_min_width(260.);
                ui.strong(if app.editor.selection().is_empty() { "Model" } else { "Selection" });
                ui.separator();
                egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 40.).max(80.)).show(ui, |ui| {
                    close = actions::menu(ui, &mut app);
                });
            });
        if close || !open {
            app.context_menu = None;
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            app.context_menu_armed = true;
        }
    }

    if !ctx.input(|i| i.pointer.any_down()) {
        app.camera_setting_editing = false;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_TEXTURE_PIN_PREVIEW").is_some() {
        crate::theme::window("Texture position")
            .default_pos(egui::pos2(100., 160.))
            .default_width(360.)
            .resizable(false)
            .show(ctx, |ui| {
                let mut placement = rysmo_model::TexturePlacement {
                    pins: Some([[0., 0.], [1., 0.], [1.3, 1.], [0.1, 0.8]]),
                    ..Default::default()
                };
                crate::texture_pins::controls(ui, &mut placement);
            });
    }
    #[cfg(not(target_arch = "wasm32"))]
    if std::env::var_os("RYSMO_SCREENSHOT").is_some() && std::env::var_os("RYSMO_ICON_PREVIEW").is_some() {
        crate::theme::window("Rysmo tool icons")
            .default_pos(egui::pos2(180., 40.))
            .default_size(egui::vec2(830., 570.))
            .resizable(false)
            .collapsible(false)
            .vscroll(true)
            .show(ctx, |ui| {
                egui::Grid::new("icon-preview-grid")
                    .num_columns(7)
                    .min_col_width(100.)
                    .spacing(egui::vec2(12., 8.))
                    .show(ui, |ui| {
                        let mut count = 0;
                        for slot in LEFT_TOOLS.iter().chain(TOP_TOOLS) {
                            let (icon, label) = match slot {
                                Slot::Tool(tool) => (crate::icons::Icon::Tool(*tool), tool.name()),
                                Slot::Action(action) => (crate::icons::Icon::Action(*action), action.label()),
                                Slot::Extents => (crate::icons::Icon::Extents, "Zoom Extents"),
                                Slot::Gap => continue,
                            };
                            ui.vertical_centered(|ui| {
                                let _ = crate::icons::button(ui, icon, false, 40.);
                                ui.small(label);
                            });
                            count += 1;
                            if count % 7 == 0 {
                                ui.end_row();
                            }
                        }
                    });
            });
    }
    crate::mirror::panel(ctx, &mut app);
    crate::solid_tools::panel(ctx, &mut app);
    crate::features::panels(ctx, &mut app);
    crate::print_check::panel(ctx, &mut app);
    crate::fillet::panel(ctx, &mut app);
    crate::divide::panel(ctx, &mut app);
    panels::materials(ctx, &mut app);
    panels::tags(ctx, &mut app);
    panels::settings(ctx, &mut app);
    crate::help::panel(ctx, &mut app);
    crate::section_panel::panel(ctx, &mut app);
    crate::scene_panel::panel(ctx, &mut app);
    crate::component_panel::panel(ctx, &mut app);
    crate::component_panel::rename_panel(ctx, &mut app);

    crate::command_search::panel(ctx, &mut app);
    panels::resize_confirmation(ctx, &mut app);
    crate::unsaved::panel(ctx, &mut app);
    if app.drape.as_ref().is_some_and(|d| !d.valid(&app)) {
        app.drape = None;
    }
    if app.stamp.as_ref().is_some_and(|d| !d.valid(&app.editor)) {
        app.stamp = None;
    }
    if app.detail.as_ref().is_some_and(|d| !d.valid(&app)) {
        app.detail = None;
    }
    if app.flip.as_ref().is_some_and(|s| !s.valid(&app)) {
        app.flip = None;
    }
    if app.smoove.as_ref().is_some_and(|d| !d.valid(&app)) {
        app.smoove = None;
    }
    if app.terrain.as_ref().is_some_and(|d| !d.valid(&app)) {
        app.terrain = None;
    }
    crate::text_tool::panel(ctx, &mut app);
    crate::text3d_tool::panel(ctx, &mut app);

    if let (true, Some((port, token))) = (app.show_mcp, app.mcp.clone()) {
        let mut open = true;
        crate::theme::window("MCP server").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.label(format!("Listening on 127.0.0.1:{port}. Requests must carry this token:"));
            ui.horizontal(|ui| {
                let mut shown = token.clone();
                ui.add(crate::theme::singleline(&mut shown).font(egui::TextStyle::Monospace).desired_width(280.0));
                if ui.button("Copy").clicked() {
                    ui.ctx().copy_text(token.clone());
                }
            });
            let command = format!("rysmo-mcp --connect 127.0.0.1:{port} --token {token}");
            ui.label("Connect a stdio MCP client with:");
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&command).monospace().small());
                if ui.button("Copy").clicked() {
                    ui.ctx().copy_text(command.clone());
                }
            });
        });
        app.show_mcp = open;
    }

    if app.show_about {
        let mut open = true;
        crate::theme::window("About Rysmo").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                crate::brand::image(ui, 80.);
                ui.vertical(|ui| {
                    ui.heading("Rysmo");
                    ui.label("A simple 3D modeller.");
                });
            });
            ui.label("Copyright (C) 2026 Jan Mojzisek. GPL-3.0-or-later.");
            #[cfg(target_arch = "wasm32")]
            ui.hyperlink_to("Download source code", "source.tar.gz");
            crate::widgets::section(ui, "Third-party licenses", false, |ui| {
                egui::ScrollArea::vertical().max_height(180.).show(ui, |ui| {
                    ui.set_max_width(460.);
                    ui.small(include_str!("../../../licenses/csg-js.txt"));
                });
            });
        });
        app.show_about = open;
    }
    crate::preferences::end(ctx, &mut app);
    Ok(())
}

/// The Measurements box's label and the value it shows before anything is typed.
pub fn measurement_label(app: &App3d) -> (String, String) {
    let zoom = app.tools.active() == ToolKind::Zoom && !app.camera.parallel;
    let eye = matches!(app.tools.active(), ToolKind::PositionCamera | ToolKind::LookAround | ToolKind::Walk);
    let vcb_label = if let Some(d) = &app.stamp {
        match d.stage {
            crate::stamp_tool::Stage::Source | crate::stamp_tool::Stage::Terrain => "Offset",
            _ => "Elevation",
        }
    } else if app.detail.is_some() {
        "Offset"
    } else if let Some(d) = &app.smoove {
        d.measurement().0
    } else if app.terrain_last.as_ref().is_some_and(|d| d.valid(app)) {
        "Length, Width"
    } else if let Some(draft) = &app.terrain {
        draft.measurement().0
    } else if eye {
        "Eye Height"
    } else if zoom {
        if app.zoom_focal { "Focal Length" } else { "Field of view" }
    } else {
        app.tools.vcb_label()
    };
    let hint = if let Some(d) = &app.stamp {
        let amount = match d.stage {
            crate::stamp_tool::Stage::Source | crate::stamp_tool::Stage::Terrain => d.offset,
            crate::stamp_tool::Stage::Elevation { height, .. } => height,
        };
        app.editor.doc().format_length(amount)
    } else if let Some(d) = &app.detail {
        app.editor.doc().format_length(d.value(app.editor.active().axes.z()))
    } else if let Some(d) = &app.smoove {
        app.editor.doc().format_length(d.measurement().1)
    } else if let Some(last) = app.terrain_last.as_ref().filter(|d| d.valid(app)) {
        let (length, width) = last.dimensions();
        format!("{},{}", app.editor.doc().format_length(length), app.editor.doc().format_length(width))
    } else if let Some(draft) = &app.terrain {
        app.editor.doc().format_length(draft.measurement().1)
    } else if eye {
        app.editor.doc().format_length(app.eye_height)
    } else if zoom {
        if app.zoom_focal {
            format!("{:.2} mm", app.camera.focal_length())
        } else {
            format!("{:.2}°", app.camera.fov_y.to_degrees())
        }
    } else {
        app.tools.vcb_value(&app.editor)
    };
    (vcb_label.to_string(), hint)
}

/// The application menus, as menu-bar entries on the desktop or submenus on a tablet.
pub fn menus(ui: &mut egui::Ui, app: &mut App3d) {
    menu::top(ui, "File", |ui| {
        if menu::button(ui, g::FILE_PLUS, "New", "Ctrl+N").clicked() {
            crate::unsaved::request(app, crate::unsaved::Action::New);
        }
        if menu::button(ui, g::FOLDER_OPEN, "Open...", "Ctrl+O").clicked() {
            crate::unsaved::request(app, crate::unsaved::Action::Open);
        }
        if !app.recent.is_empty() {
            let recent = app.recent.clone();
            menu::submenu(ui, g::CLOCK_COUNTER_CLOCKWISE, "Open Recent", |ui| {
                for name in recent {
                    let label = std::path::Path::new(&name).file_name().unwrap_or_default().to_string_lossy();
                    if menu::button(ui, g::FILE, &label, "").on_hover_text(&name).clicked() {
                        crate::unsaved::request(app, crate::unsaved::Action::Recent(name.clone()));
                    }
                }
            });
        }
        if menu::button(ui, g::FLOPPY_DISK, "Save", "Ctrl+S").clicked() {
            files::save(app, false);
        }
        if menu::button(ui, g::FLOPPY_DISK_BACK, "Save As...", "Ctrl+Shift+S").clicked() {
            files::save(app, true);
        }
        ui.separator();
        menu::submenu(ui, g::FOLDER_OPEN, "Import", |ui| {
            if menu::button(ui, g::FOLDER_OPEN, "Model or DXF underlay...", "").clicked() {
                crate::features::data_files::pick(app, crate::features::data_files::Purpose::Model);
            }
            if menu::button(ui, g::FOLDER_OPEN, "Component collection...", "").clicked() {
                crate::features::data_files::pick(app, crate::features::data_files::Purpose::Collection);
            }
        });
        if menu::button(ui, g::FLOPPY_DISK, "Export drawing (SVG / PDF)...", "").clicked() {
            app.features.drawing.open = true;
        }
        if menu::button(ui, g::FOLDER_OPEN, "Component library...", "").clicked() {
            app.features.library.open = true;
        }
        menu::submenu(ui, g::EXPORT, "Export", |ui| {
            for (label, action) in [
                ("Wavefront OBJ...", files::export_obj as fn(&mut App3d)),
                ("glTF (.glb)...", files::export_glb),
                ("COLLADA (.dae)...", files::export_collada),
                ("DXF (.dxf)...", files::export_dxf),
            ] {
                if menu::button(ui, g::EXPORT, label, "").clicked() {
                    action(app);
                }
            }
            if menu::button(ui, g::EXPORT, "Selection as component...", "").clicked() {
                crate::features::library::export_selection(app);
            }
            ui.separator();
            for (label, format) in [
                ("STL for 3D printing...", crate::print_check::Format::Stl),
                ("3MF for 3D printing...", crate::print_check::Format::ThreeMf),
            ] {
                if menu::button(ui, g::EXPORT, label, "").clicked() {
                    files::export_for_print(app, format);
                }
            }
        });
        ui.separator();
        if menu::button(ui, g::HOUSE, "Welcome page", "").clicked() {
            app.welcome = true;
        }
        if menu::button(ui, g::GEAR_SIX, "Settings...", "").clicked() {
            app.show_settings = true;
        }
    });
    menu::top(ui, "Edit", |ui| {
        let undo_text = app.editor.undo_label().map(|l| format!("Undo {l}")).unwrap_or_else(|| "Undo".into());
        if menu::enabled(ui, app.editor.undo_label().is_some(), g::ARROW_U_UP_LEFT, &undo_text, "Ctrl+Z").clicked() {
            undo(app);
        }
        let redo_text = app.editor.redo_label().map(|l| format!("Redo {l}")).unwrap_or_else(|| "Redo".into());
        if menu::enabled(ui, app.editor.redo_label().is_some(), g::ARROW_U_UP_RIGHT, &redo_text, "Ctrl+Shift+Z")
            .clicked()
        {
            redo(app);
        }
        ui.separator();
        actions::edit_menu(ui, app);
    });
    menu::top(ui, "Model", |ui| {
        if menu::button(ui, g::CUBE, "Parametric components...", "").clicked() {
            app.features.parametric.open = true;
        }
        actions::model_menu(ui, app);
    });
    menu::top(ui, "Camera", |ui| {
        if menu::button(ui, g::CAMERA, "Photo matching...", "").clicked() {
            app.features.photo.open = true;
        }
        if menu::button(ui, g::CORNERS_OUT, "Zoom Extents", "").clicked() {
            actions::run(app, Action::ZoomExtents);
        }
        if menu::button(ui, g::CAMERA, "Position Camera", "").clicked() {
            app.set_tool(ToolKind::PositionCamera);
        }
        if menu::button(ui, g::PERSON_SIMPLE_WALK, "Walk", "").clicked() {
            app.set_tool(ToolKind::Walk);
        }
        if menu::button(ui, g::EYE, "Look Around", "").clicked() {
            app.set_tool(ToolKind::LookAround);
        }
        if menu::button(ui, g::MAGNIFYING_GLASS, "Zoom Window", "Ctrl+Shift+W").clicked() {
            app.set_tool(ToolKind::ZoomWindow);
        }
        let mut parallel = app.camera.parallel;
        if menu::toggle(ui, g::CUBE, "Parallel projection", &mut parallel).changed() {
            app.remember_view();
            app.camera.parallel = parallel;
        }
        menu::submenu(ui, g::CUBE, "Standard views", |ui| {
            use rysmo_math::StandardView;
            for (view, name) in [
                (StandardView::Top, "Top"),
                (StandardView::Bottom, "Bottom"),
                (StandardView::Front, "Front"),
                (StandardView::Back, "Back"),
                (StandardView::Left, "Left"),
                (StandardView::Right, "Right"),
                (StandardView::Iso, "Iso"),
            ] {
                if menu::button(ui, g::CUBE, name, "").clicked() {
                    app.remember_view();
                    app.camera.set_standard_view(view);
                }
            }
        });
        if menu::enabled(ui, !app.camera_history.is_empty(), g::ARROW_COUNTER_CLOCKWISE, "Previous view", "").clicked()
        {
            app.previous_view();
        }
        let mut fov = app.camera.fov_y.to_degrees();
        if ui
            .add_enabled(
                !app.camera.parallel,
                crate::widgets::Slider::new(&mut fov, 1.0..=120.0)
                    .clamping(egui::SliderClamping::Edits)
                    .text("Field of view (degrees)"),
            )
            .changed()
        {
            if !app.camera_setting_editing {
                app.remember_view();
            }
            app.camera_setting_editing = true;
            app.camera.fov_y = fov.to_radians();
            app.zoom_focal = false;
        }
    });
    menu::top(ui, "Window", |ui| {
        let mut style = app.editor.doc().style;
        if menu::toggle(ui, g::RULER, "Guides", &mut style.show_guides).changed() {
            let _ = app.editor.apply(Command::SetStyle { style });
        }
        if menu::toggle(ui, g::CUBE_TRANSPARENT, "Hidden Geometry", &mut style.show_hidden_geometry).changed()
            && let Err(error) = app.editor.apply(Command::SetStyle { style })
        {
            app.message = Some(error.to_string());
        }
        ui.separator();
        if menu::button(ui, g::ARROWS_OUT_CARDINAL, "Reset drawing axes", "").clicked() {
            let _ = app.editor.apply(Command::SetAxes {
                origin: rysmo_math::DVec3::ZERO,
                x: rysmo_math::DVec3::X,
                y: rysmo_math::DVec3::Y,
            });
        }
        ui.separator();
        menu::toggle(ui, g::PALETTE, "Materials", &mut app.show_materials);
        menu::toggle(ui, g::TAG, "Tags", &mut app.show_tags);
        menu::toggle(ui, g::SQUARE_SPLIT_VERTICAL, "Section Planes", &mut app.show_sections);
        menu::toggle(ui, g::STACK, "Scenes", &mut app.show_scenes);
        menu::toggle(ui, g::CUBE, "Components", &mut app.show_components);
        menu::toggle(ui, g::SLIDERS_HORIZONTAL, "Inspector", &mut app.show_entity_info);
        if app.mcp.is_some() {
            menu::toggle(ui, g::PLUG, "MCP server", &mut app.show_mcp);
        }
    });
    menu::top(ui, "Help", |ui| {
        if menu::button(ui, g::MAGNIFYING_GLASS, "Search Commands...", "Shift+S").clicked() {
            crate::command_search::open(app);
        }
        if menu::button(ui, g::BOOK_OPEN, "Editor Help", "").clicked() {
            app.help.open = true;
            ui.close();
        }
        if menu::button(ui, g::QUESTION, "Help for Current Tool", "F1").clicked() {
            crate::help::open_current(app);
            ui.close();
        }
        ui.separator();
        if menu::button(ui, g::INFO, "About Rysmo", "").clicked() {
            app.show_about = true;
        }
    });
}

fn pos(p: DVec2) -> egui::Pos2 {
    egui::pos2(p.x as f32, p.y as f32)
}

fn c32(c: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

/// The inference marker and tooltip, and the selection rectangle, drawn over the view.
fn overlay(mut contexts: EguiContexts, mut app: ResMut<App3d>) -> Result {
    let ctx = contexts.ctx_mut()?;
    if app.welcome {
        return Ok(());
    }
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::unique("overlay")));
    app.dimension_text_sizes = draw_dimensions(&painter, &app);
    app.text_sizes = draw_texts(&painter, &app);
    crate::text3d_tool::draw(&painter, &app);
    crate::component_panel::draw(&painter, &app);
    crate::object_clipboard::draw(&painter, &app);
    crate::mirror::draw(&painter, &app);
    crate::solid_tools::draw(&painter, &app);
    crate::print_check::draw(&painter, &app);
    crate::fillet::draw(&painter, &app);
    crate::divide::draw(&painter, &app);
    crate::terrain_tool::draw(&painter, &app);
    crate::smoove_tool::draw(&painter, &app);
    crate::flip_tool::draw(&painter, &app);
    crate::drape_tool::draw(&painter, &app);
    crate::stamp_tool::draw(&painter, &app);
    crate::detail_tool::draw(&painter, &app);
    let mut preview = app.tools.preview(&app.editor);
    if let Some(draft) = &app.terrain
        && let Some(inference) = &mut preview.inference
    {
        inference.point = draft.current;
    }
    if app.text_creation.is_some() || app.texture_session.is_some() {
        preview.inference = None;
    }
    if let Some((_, start)) = app.positioning
        && let Some(end) = preview.inference.map(|i| app.editor.context_transform().transform_point3(i.point))
        && let (Some(a), Some(b)) = (app.view().project(start), app.view().project(end))
    {
        painter.extend(egui::Shape::dashed_line(
            &[pos(a), pos(b)],
            egui::Stroke::new(1.5, egui::Color32::DARK_GRAY),
            5.0,
            4.0,
        ));
    }
    if let Some((anchor, _)) = app.walking {
        let a = pos(anchor);
        let stroke = egui::Stroke::new(2.0, egui::Color32::BLACK);
        painter.line_segment([a - egui::vec2(8.0, 0.0), a + egui::vec2(8.0, 0.0)], stroke);
        painter.line_segment([a - egui::vec2(0.0, 8.0), a + egui::vec2(0.0, 8.0)], stroke);
    }
    if let Some((a, b)) = app.zoom_window {
        painter.rect_stroke(
            egui::Rect::from_two_pos(pos(a), pos(b)),
            0.0,
            egui::Stroke::new(1.5, egui::Color32::from_rgb(40, 80, 220)),
            egui::StrokeKind::Inside,
        );
    }
    if let Some((a, b, crossing)) = preview.select_box {
        let rect = egui::Rect::from_two_pos(pos(a), pos(b));
        let blue = egui::Color32::from_rgb(40, 80, 220);
        painter.rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(60, 110, 230, 30));
        if crossing {
            let pts = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
            for w in pts.windows(2) {
                painter.extend(egui::Shape::dashed_line(&[w[0], w[1]], egui::Stroke::new(1.0, blue), 5.0, 4.0));
            }
        } else {
            painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, blue), egui::StrokeKind::Inside);
        }
    }
    let view = app.local_view();
    for (p, c) in &preview.markers {
        if let Some(q) = view.project(*p) {
            let r = egui::Rect::from_center_size(pos(q), egui::vec2(9.0, 9.0));
            painter.rect(r, 0.0, c32(*c), egui::Stroke::new(1.0, egui::Color32::WHITE), egui::StrokeKind::Middle);
        }
    }
    if preview.dimension.is_some() {
        return Ok(());
    }
    let Some(inf) = preview.inference else { return Ok(()) };
    if app.camera_drag.is_some() || app.touch.orbiting {
        return Ok(());
    }
    // The last hover in the model stays in the preview while the pointer is over a panel or
    // outside the window; touches leave no cursor.
    let over_ui = |c: DVec2| app.over_panel(c) || ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
    if !app.left_down && app.cursor.map_or(!app.touch.enabled, over_ui) {
        return Ok(());
    }
    let Some(p) = view.project(inf.point) else { return Ok(()) };
    let p = pos(p);
    let color = c32(inf.kind.color());
    let stroke = egui::Stroke::new(1.5, egui::Color32::from_gray(30));
    match inf.kind {
        Kind::Endpoint | Kind::Midpoint | Kind::Centre | Kind::Origin => {
            painter.circle(p, 6.0, color, stroke);
        }
        Kind::OnEdge => {
            painter.rect(
                egui::Rect::from_center_size(p, egui::vec2(10.0, 10.0)),
                0.0,
                color,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Kind::OnFace => {
            let d = 7.0;
            let pts =
                vec![p + egui::vec2(0.0, -d), p + egui::vec2(d, 0.0), p + egui::vec2(0.0, d), p + egui::vec2(-d, 0.0)];
            painter.add(egui::Shape::convex_polygon(pts, color, stroke));
        }
        Kind::Intersection => {
            let d = 6.0;
            let s = egui::Stroke::new(2.5, color);
            painter.line_segment([p + egui::vec2(-d, -d), p + egui::vec2(d, d)], s);
            painter.line_segment([p + egui::vec2(-d, d), p + egui::vec2(d, -d)], s);
        }
        Kind::OnAxis(_) | Kind::FromPoint(_) | Kind::Parallel | Kind::Perpendicular | Kind::Tangent => {
            painter.circle(p, 4.0, color, stroke);
        }
        Kind::GuidePoint => {
            let d = 6.0;
            let st = egui::Stroke::new(2.0, egui::Color32::BLACK);
            painter.line_segment([p + egui::vec2(-d, 0.0), p + egui::vec2(d, 0.0)], st);
            painter.line_segment([p + egui::vec2(0.0, -d), p + egui::vec2(0.0, d)], st);
        }
        Kind::OnGuide => {
            painter.rect(
                egui::Rect::from_center_size(p, egui::vec2(9.0, 9.0)),
                0.0,
                color,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Kind::Ground | Kind::Free => {}
    }
    let tip = preview.label.clone().unwrap_or_else(|| {
        let t = inf.kind.tooltip();
        // Include the group or component type in the tooltip.
        match inf.instance.and_then(|i| app.editor.active().instances.get(&i)) {
            Some(inst) if !t.is_empty() => {
                let group = app.editor.doc().definitions.get(&inst.definition).is_some_and(|d| d.group);
                format!("{t} in {}", if group { "Group" } else { "Component" })
            }
            _ => t.to_string(),
        }
    });
    if !tip.is_empty() {
        let at = p + egui::vec2(14.0, 16.0);
        let galley = painter.layout_no_wrap(tip, egui::FontId::proportional(13.0), egui::Color32::BLACK);
        let rect = egui::Rect::from_min_size(at, galley.size()).expand(3.0);
        painter.rect(
            rect,
            2.0,
            egui::Color32::from_rgb(255, 255, 225),
            egui::Stroke::new(1.0, egui::Color32::from_gray(120)),
            egui::StrokeKind::Middle,
        );
        painter.galley(at, galley, egui::Color32::BLACK);
    }
    Ok(())
}

fn draw_dimensions(
    painter: &egui::Painter,
    app: &App3d,
) -> std::collections::BTreeMap<rysmo_model::dimensions::DimensionTextKey, DVec2> {
    let mut sizes = std::collections::BTreeMap::new();
    let document = app.editor.doc();
    let view = app.view();
    for occurrence in document.occurrences(app.editor.context()) {
        let Some(definition) = document.def(occurrence.definition) else { continue };
        for (id, dimension) in &definition.dimensions {
            let location =
                occurrence.transform.transform_point3(dimension.resolve(document, occurrence.definition).from);
            let extension = view.pixel_size_at(location) * 6.0;
            let Some(layout) = dimension.layout(document, occurrence.definition, occurrence.transform, extension)
            else {
                continue;
            };
            if !layout.visible(&view, document.style.dimensions) {
                continue;
            }
            let selected = occurrence.in_context
                && occurrence.definition == app.editor.active_definition()
                && app.editor.selection().contains(&rysmo_model::Element::Dimension(*id));
            let color = if selected {
                egui::Color32::from_rgb(50, 100, 240)
            } else if occurrence.in_context {
                let rgb = if document.style.dimensions.highlight_non_associated && !layout.measurement.associated {
                    document.style.dimensions.highlight_color
                } else {
                    dimension.font.color
                };
                egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
            } else {
                egui::Color32::from_gray(140)
            };
            if let Some((text, size)) = paint_dimension(painter, &view, dimension, &layout, document, color, &[]) {
                sizes.insert(text, size);
            }
        }
    }
    let preview = app.tools.preview(&app.editor);
    let avoid = preview
        .markers
        .iter()
        .filter_map(|(point, _)| view.project(app.editor.context_transform().transform_point3(*point)))
        .collect::<Vec<_>>();
    let context_transform = app.editor.context_transform();
    let previews = preview
        .dimension
        .into_iter()
        .chain(preview.moving_dimensions)
        .map(|dimension| (dimension, rysmo_math::DMat4::IDENTITY))
        .chain(preview.instanced_dimensions);
    for (dimension, local_transform) in previews {
        let transform = context_transform * local_transform;
        if let Some(layout) = dimension.layout(
            document,
            app.editor.active_definition(),
            transform,
            view.pixel_size_at(
                transform.transform_point3(dimension.resolve(document, app.editor.active_definition()).from),
            ) * 6.0,
        ) {
            let _ = paint_dimension(
                painter,
                &view,
                &dimension,
                &layout,
                document,
                egui::Color32::from_rgb(180, 0, 180),
                &avoid,
            );
        }
    }
    sizes
}

fn draw_texts(painter: &egui::Painter, app: &App3d) -> std::collections::BTreeMap<rysmo_model::text::TextKey, DVec2> {
    let mut sizes = std::collections::BTreeMap::new();
    let doc = app.editor.doc();
    let view = app.view();
    let preview = app.tools.preview(&app.editor);
    for occurrence in doc.occurrences(app.editor.context()) {
        let Some(definition) = doc.def(occurrence.definition) else { continue };
        for (id, text) in &definition.texts {
            let Some(layout) = text.layout(doc, occurrence.definition, occurrence.transform, &view) else { continue };
            let rgb = text.font.color;
            let selected = occurrence.in_context
                && app.editor.is_selected(
                    occurrence.top.map(rysmo_model::Element::Instance).unwrap_or(rysmo_model::Element::Text(*id)),
                );
            let color = if selected {
                egui::Color32::from_rgb(50, 100, 240)
            } else if occurrence.in_context {
                egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
            } else {
                egui::Color32::from_gray(140)
            };
            if let Some((key, size)) = paint_text(painter, text, &layout, color) {
                sizes.insert(key, size);
            }
        }
    }
    if let Some(draft) = &app.text_creation
        && draft.valid(app)
        && let Some(layout) =
            draft.text.layout(doc, app.editor.active_definition(), app.editor.context_transform(), &view)
    {
        let _ = paint_text(painter, &draft.text, &layout, egui::Color32::from_rgb(180, 0, 180));
    }
    for text in preview.moving_texts {
        if let Some(layout) = text.layout(doc, app.editor.active_definition(), app.editor.context_transform(), &view) {
            let _ = paint_text(painter, &text, &layout, egui::Color32::from_rgb(180, 0, 180));
        }
    }
    for (text, transform) in preview.instanced_texts {
        if let Some(layout) =
            text.layout(doc, app.editor.active_definition(), app.editor.context_transform() * transform, &view)
        {
            let _ = paint_text(painter, &text, &layout, egui::Color32::from_rgb(180, 0, 180));
        }
    }
    sizes
}

fn paint_text(
    painter: &egui::Painter,
    text: &rysmo_model::text::Text,
    layout: &rysmo_model::text::TextLayout,
    color: egui::Color32,
) -> Option<(rysmo_model::text::TextKey, DVec2)> {
    use rysmo_model::dimensions::DimensionEndMarker;
    let family = if text.font.monospace { egui::FontFamily::Monospace } else { egui::FontFamily::Proportional };
    let galley = painter.layout_no_wrap(text.content.clone(), egui::FontId::new(text.font.size, family), color);
    let size = DVec2::new(galley.size().x as f64, galley.size().y as f64);
    let geometry = text.geometry(layout, size)?;
    let stroke = egui::Stroke::new(1., color);
    for [a, b] in geometry.segments {
        painter.line_segment([pos(a), pos(b)], stroke);
    }
    for marker in geometry.markers {
        match marker {
            DimensionEndMarker::Line([a, b]) => {
                painter.line_segment([pos(a), pos(b)], stroke);
            }
            DimensionEndMarker::Triangle(points) => {
                painter.add(egui::Shape::convex_polygon(
                    points.into_iter().map(pos).collect(),
                    color,
                    egui::Stroke::NONE,
                ));
            }
            DimensionEndMarker::Dot { centre, radius } => {
                painter.circle_filled(pos(centre), radius as f32, color);
            }
        }
    }
    painter.galley(pos(geometry.rect.bounds().0), galley, color);
    Some((text.text_key(), size))
}

fn paint_dimension(
    painter: &egui::Painter,
    view: &rysmo_math::View,
    dimension: &rysmo_model::dimensions::Dimension,
    layout: &rysmo_model::dimensions::DimensionLayout,
    document: &rysmo_model::Document,
    color: egui::Color32,
    avoid: &[DVec2],
) -> Option<(rysmo_model::dimensions::DimensionTextKey, DVec2)> {
    let stroke = egui::Stroke::new(1.0, color);
    for [a, b] in &layout.segments {
        if let Some((a, b)) = view.project_segment(*a, *b) {
            painter.line_segment([pos(a), pos(b)], stroke);
        }
    }
    for marker in layout.screen_endpoints(view) {
        use rysmo_model::dimensions::DimensionEndMarker;
        match marker {
            DimensionEndMarker::Line([a, b]) => {
                painter.line_segment([pos(a), pos(b)], stroke);
            }
            DimensionEndMarker::Triangle(triangle) => {
                painter.add(egui::Shape::convex_polygon(
                    triangle.into_iter().map(pos).collect(),
                    color,
                    egui::Stroke::NONE,
                ));
            }
            DimensionEndMarker::Dot { centre, radius } => {
                painter.circle_filled(pos(centre), radius as f32, color);
            }
        }
    }
    layout.text_on_screen(view)?;
    let text = dimension.label_in_document(&layout.measurement, document);
    let family = if dimension.font.monospace { egui::FontFamily::Monospace } else { egui::FontFamily::Proportional };
    let galley = painter.layout_no_wrap(text.clone(), egui::FontId::new(dimension.font.size, family), color);
    let size = DVec2::new(galley.size().x as f64, galley.size().y as f64);
    let mut rect = dimension.text_rect(layout, view, size)?;
    let original = rect.centre;
    let at = layout.text_on_screen(view)?;
    let direction = view
        .project(layout.text_position + layout.text_direction * view.pixel_size_at(layout.text_position) * 20.)
        .map(|end| (end - at).normalize_or_zero())
        .filter(|d| d.length_squared() > 0.)
        .unwrap_or(DVec2::X);
    for marker in avoid {
        if rect.expanded(7.).distance(*marker) < 1e-6 {
            let delta = rect.centre - *marker;
            let away = if delta.length_squared() > 0.01 { delta.normalize() } else { direction };
            let side = DVec2::new(-rect.axis.y, rect.axis.x);
            let extent = (away.dot(rect.axis).abs() * size.x + away.dot(side).abs() * size.y) * 0.5 + 12.;
            rect.centre = *marker + away * extent;
        }
    }
    if rect.centre.distance(original) > 1e-6 {
        painter.line_segment([pos(original), pos(rect.centre)], stroke);
    }

    painter.add(egui::Shape::convex_polygon(
        rect.expanded(2.).corners().into_iter().map(pos).collect(),
        egui::Color32::WHITE,
        egui::Stroke::NONE,
    ));
    let origin = pos(rect.centre) - galley.size() * 0.5;
    painter.add(
        egui::epaint::TextShape::new(origin, galley, color)
            .with_angle_and_anchor(rect.axis.y.atan2(rect.axis.x) as f32, egui::Align2::CENTER_CENTER),
    );
    Some((dimension.text_key(text), size))
}

#[cfg(test)]
mod dimension_font_tests {
    use super::*;
    #[test]
    fn different_font_sizes_keep_independent_measured_pick_bounds() {
        let mut doc = rysmo_io::load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        let small_id = rysmo_model::DimensionId(10);
        doc.root_mut().dimensions.retain(|id, _| *id == small_id);
        let mut app = App3d::new(rysmo_model::Editor::with_document(doc));
        let small = app.editor.active().dimensions[&small_id].clone();
        let mut big = small.clone();
        big.font.size = 28.;
        big.font.monospace = true;
        big.font.color = [180, 20, 20];
        big.offset = rysmo_math::DVec3::Y * -1.5;
        let big_id =
            app.editor.apply(rysmo_model::Command::AddDimension { dimension: big.clone() }).unwrap().dimensions[0];
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.))),
                ..Default::default()
            },
            |ui| {
                app.dimension_text_sizes = draw_dimensions(ui.painter(), &app);
            },
        );
        output.textures_delta.clear();
        assert_eq!(app.dimension_text_sizes.len(), 2);
        let layout = big.layout(app.editor.doc(), None, rysmo_math::DMat4::IDENTITY, 0.1).unwrap();
        let text = big.label_in_document(&layout.measurement, app.editor.doc());
        let small_size = app.dimension_text_sizes[&small.text_key(text.clone())];
        let big_size = app.dimension_text_sizes[&big.text_key(text)];
        assert!(big_size.x > small_size.x * 1.5 && big_size.y > small_size.y * 1.5);
        let (lo, hi) = big.text_bounds(&layout, &app.view(), big_size).unwrap();
        let point = (lo + hi) * 0.5 + DVec2::new(big_size.x * 0.45, 0.);
        assert_eq!(
            app.pick_annotation(point),
            Some(rysmo_infer::Pick::Element(rysmo_model::Element::Dimension(big_id)))
        );
        let mut style = app.editor.doc().style;
        style.dimensions.hide_small = true;
        style.dimensions.minimum_pixels = 4096.;
        app.editor.apply(rysmo_model::Command::SetStyle { style }).unwrap();
        assert!(app.pick_annotation(point).is_none());
        assert!(app.dimension_box(DVec2::ZERO, app.viewport).is_empty());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            assert!(draw_dimensions(ui.painter(), &app).is_empty());
        });
        output.textures_delta.clear();
    }
}

#[cfg(test)]
mod dimension_preview_label_tests {
    use super::*;
    #[test]
    fn preview_text_clears_scale_grips_without_changing_layout() {
        use rysmo_model::dimensions::{DimensionTextAlignment as A, DimensionTextPosition as P, DimensionTextRect};
        let doc = rysmo_io::load(include_str!("../../../testdata/dimensions-preview.rysmo")).unwrap();
        let view = rysmo_math::OrbitCamera::default().view(DVec2::new(1280., 800.));
        for (id, alignment, position) in [
            (46, A::Screen, P::Centered),
            (46, A::DimensionLine, P::Centered),
            (10, A::Screen, P::OutsideStart),
            (10, A::DimensionLine, P::OutsideStart),
        ] {
            let mut dimension = doc.root.dimensions[&rysmo_model::DimensionId(id)].clone();
            dimension.alignment = alignment;
            dimension.text_position = position;
            let layout = dimension.layout(&doc, None, rysmo_math::DMat4::IDENTITY, 0.1).unwrap();
            let before = layout.clone();
            let marker = layout.text_on_screen(&view).unwrap();
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.))),
                    ..Default::default()
                },
                |ui| {
                    assert!(
                        paint_dimension(
                            ui.painter(),
                            &view,
                            &dimension,
                            &layout,
                            &doc,
                            egui::Color32::MAGENTA,
                            &[marker]
                        )
                        .is_some()
                    );
                },
            );
            let text = output
                .shapes
                .iter()
                .find_map(|shape| if let egui::Shape::Text(text) = &shape.shape { Some(text) } else { None })
                .unwrap();
            let axis = DVec2::new((text.angle as f64).cos(), (text.angle as f64).sin());
            let side = DVec2::new(-axis.y, axis.x);
            let size = DVec2::new(text.galley.size().x as f64, text.galley.size().y as f64);
            let centre = DVec2::new(text.pos.x as f64, text.pos.y as f64) + axis * size.x * 0.5 + side * size.y * 0.5;
            let rect = DimensionTextRect { centre, axis, size };
            assert!(rect.expanded(5.).distance(marker) > 0.);
            assert_eq!(layout, before);
            output.textures_delta.clear();
        }
    }
}

#[cfg(test)]
mod text_paint_tests {
    use super::*;
    #[test]
    fn multiline_text_rendering_measures_fonts_and_reprojects_with_camera() {
        let doc = rysmo_io::load(include_str!("../../../testdata/text-preview.rysmo")).unwrap();
        let mut app = App3d::new(rysmo_model::Editor::with_document(doc));
        let context = egui::Context::default();
        for text in app.editor.active().texts.values() {
            if let rysmo_model::text::TextPlacement::Attached { anchor, .. } = &text.placement {
                assert!(anchor.resolve(app.editor.doc(), None).1);
            }
        }
        let mut previous = None;
        for _ in 0..2 {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.))),
                    ..Default::default()
                },
                |ui| {
                    app.text_sizes = draw_texts(ui.painter(), &app);
                },
            );
            output.textures_delta.clear();
            assert_eq!(app.text_sizes.len(), 5);
            let screen = &app.editor.doc().root.texts[&rysmo_model::TextId(1001)];
            let leader = &app.editor.doc().root.texts[&rysmo_model::TextId(1003)];
            let size = app.text_sizes[&leader.text_key()];
            assert!(size.x > 100. && size.y > 30.);
            let screen_layout =
                screen.layout(app.editor.doc(), None, rysmo_math::DMat4::IDENTITY, &app.view()).unwrap();
            let leader_layout =
                leader.layout(app.editor.doc(), None, rysmo_math::DMat4::IDENTITY, &app.view()).unwrap();
            if let Some((screen_before, leader_before)) = previous {
                assert_eq!(screen_layout.position, screen_before);
                assert!(leader_layout.position.distance(leader_before) > 20.);
            }
            let geometry = leader.geometry(&leader_layout, size).unwrap();
            assert_eq!(geometry.hit_distance(geometry.rect.centre), 0.);
            previous = Some((screen_layout.position, leader_layout.position));
            app.camera.pan(DVec2::new(100., 20.), app.viewport);
        }
    }
}

#[cfg(test)]
mod toolbar_tests {
    #[test]
    fn split_toolbars_keep_every_editor_tool_available_exactly_once() {
        for tool in rysmo_tools::ToolKind::ALL {
            assert_eq!(
                super::LEFT_TOOLS
                    .iter()
                    .chain(super::TOP_TOOLS)
                    .filter(|slot| matches!(slot, super::Slot::Tool(kind) if *kind == tool))
                    .count(),
                1,
                "{tool:?}"
            );
        }
        assert_eq!(super::LEFT_TOOLS.iter().filter(|slot| !matches!(slot, super::Slot::Gap)).count(), 16);
    }
}
