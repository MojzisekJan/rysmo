//! The tablet layout, used with the touch layout: a header with the menus and view
//! controls, a rail of tool categories, an options bar with named modes for the active
//! tool, a status bar whose Measurements button opens a keypad, a tabbed sidebar for
//! the model panels, and a bar of actions for the current selection.
//!
//! Fingers and a pencil work alike; modifier keys become named modes, so nothing
//! has to be held.
use bevy_egui::egui;
use rysmo_tools::{Key, ToolKind as T};

use crate::actions::{self, Action};
use crate::input::{apply_vcb, redo, tool_key, undo};
use crate::menu::glyph as g;
use crate::state::App3d;
use crate::ui::{Slot, slot_button};

/// Touch targets follow the 44-point guideline; icons sit inside them.
const ICON: f32 = 26.;
const RAIL: f32 = 64.;
const SIDEBAR: f32 = 340.;

/// One rail button: a tool family. Tapping it picks the family's last tool; tapping it
/// again, or a family with several tools, opens the full list beside the rail.
struct Category {
    name: &'static str,
    slots: &'static [Slot],
}

const CATEGORIES: &[Category] = &[
    Category { name: "Select", slots: &[Slot::Tool(T::Select), Slot::Tool(T::Eraser)] },
    Category {
        name: "Draw",
        slots: &[
            Slot::Tool(T::Line),
            Slot::Tool(T::Freehand),
            Slot::Tool(T::Rectangle),
            Slot::Tool(T::RotatedRectangle),
            Slot::Tool(T::Circle),
            Slot::Tool(T::Polygon),
            Slot::Tool(T::Arc),
            Slot::Tool(T::ThreePointArc),
            Slot::Tool(T::CenterArc),
            Slot::Tool(T::Pie),
        ],
    },
    Category { name: "Shape", slots: &[Slot::Tool(T::PushPull), Slot::Tool(T::Offset), Slot::Tool(T::FollowMe)] },
    Category { name: "Move", slots: &[Slot::Tool(T::Move), Slot::Tool(T::Rotate), Slot::Tool(T::Scale)] },
    Category { name: "Paint", slots: &[Slot::Tool(T::PaintBucket)] },
    Category {
        name: "Measure",
        slots: &[
            Slot::Tool(T::TapeMeasure),
            Slot::Tool(T::Protractor),
            Slot::Tool(T::Dimension),
            Slot::Tool(T::Text),
            Slot::Action(Action::Text3d),
            Slot::Tool(T::Axes),
            Slot::Action(Action::SectionPlane),
        ],
    },
    Category { name: "Group", slots: &[Slot::Action(Action::MakeGroup), Slot::Action(Action::MakeComponent)] },
    Category {
        name: "Terrain",
        slots: &[
            Slot::Action(Action::FromContours),
            Slot::Action(Action::TerrainGrid),
            Slot::Action(Action::Smoove),
            Slot::Action(Action::Drape),
            Slot::Action(Action::Stamp),
            Slot::Action(Action::AddDetail),
            Slot::Action(Action::FlipEdge),
        ],
    },
];

/// Sidebar tabs: the window each one docks, and the flag that shows it.
const TABS: &[(&str, &str)] = &[
    ("Inspector", "Inspector"),
    ("Materials", "Materials"),
    ("Tags", "Tags"),
    ("Components", "Components"),
    ("Scenes", "Scenes"),
    ("Sections", "Section Planes"),
];

fn tab_flag(app: &mut App3d, index: usize) -> &mut bool {
    match index {
        0 => &mut app.show_entity_info,
        1 => &mut app.show_materials,
        2 => &mut app.show_tags,
        3 => &mut app.show_components,
        4 => &mut app.show_scenes,
        _ => &mut app.show_sections,
    }
}

/// Layout state kept in egui's memory: it is presentation only and never saved.
#[derive(Clone, Copy, Default)]
struct State {
    flyout: Option<usize>,
    last: [usize; CATEGORIES.len()],
    sidebar: bool,
    tab: usize,
    keypad: bool,
    tool: Option<T>,
}

fn state(ctx: &egui::Context) -> State {
    ctx.data(|d| d.get_temp(egui::Id::unique("tablet-layout"))).unwrap_or_default()
}

fn set_state(ctx: &egui::Context, state: State) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("tablet-layout"), state));
}

/// Lays out the tablet chrome around the canvas and records the rectangles it covers.
pub fn chrome(ctx: &egui::Context, root: &mut egui::Ui, app: &mut App3d, rects: &mut Vec<egui::Rect>) {
    let mut st = state(ctx);
    // Named modes belong to one tool; switching tools starts the next one plain.
    if st.tool != Some(app.tools.active()) {
        if app.touch.shift {
            tool_key(app, Key::Shift(false));
        }
        (app.touch.shift, app.touch.ctrl, app.touch.alt) = (false, false, false);
        st.tool = Some(app.tools.active());
    }
    sync_sidebar(app, &mut st);

    let header =
        egui::Panel::top("tablet-header").frame(panel_frame(root, 8, 6)).show(root, |ui| header(ui, app, &mut st));
    rects.push(header.response.rect);
    if !app.editor.doc().scenes.is_empty() {
        let tabs = egui::Panel::top("scenes").show(root, |ui| crate::scene_panel::tabs(ui, app));
        rects.push(tabs.response.rect);
    }
    let options = egui::Panel::top("tablet-options").frame(panel_frame(root, 10, 4)).show(root, |ui| options(ui, app));
    rects.push(options.response.rect);
    let status =
        egui::Panel::bottom("tablet-status").frame(panel_frame(root, 10, 6)).show(root, |ui| status(ui, app, &mut st));
    rects.push(status.response.rect);
    let rail = egui::Panel::left("tablet-rail")
        .frame(panel_frame(root, 6, 8))
        .default_size(RAIL)
        .size_range(RAIL..=RAIL)
        .resizable(false)
        .show(root, |ui| rail(ui, app, &mut st));
    rects.push(rail.response.rect);
    crate::theme::undock(ctx);
    if st.sidebar {
        // Upright screens are too narrow for a side panel; it becomes a bottom sheet.
        let free = root.available_rect_before_wrap();
        let sidebar = if free.height() > free.width() {
            egui::Panel::bottom("tablet-sheet")
                .frame(panel_frame(root, 8, 8))
                .default_size(free.height() * 0.45)
                .resizable(false)
                .show(root, |ui| sidebar(ui, app, &mut st))
        } else {
            egui::Panel::right("tablet-sidebar")
                .frame(panel_frame(root, 8, 8))
                .default_size(SIDEBAR.min(free.width() * 0.45))
                .resizable(false)
                .show(root, |ui| sidebar(ui, app, &mut st))
        };
        rects.push(sidebar.response.rect);
    }
    let canvas = root.available_rect_before_wrap();
    if let Some(index) = st.flyout {
        flyout(ctx, app, &mut st, index, rail.response.rect, canvas, rects);
    }
    if st.keypad {
        keypad(ctx, app, &mut st, canvas, rects);
    }
    selection_bar(ctx, app, canvas, rects);
    set_state(ctx, st);
}

/// The desktop's docked right-hand panel: the same tabs as the tablet sidebar, shown while
/// one of them is open, resizable, with a button to close it.
pub fn desktop_sidebar(root: &mut egui::Ui, app: &mut App3d, rects: &mut Vec<egui::Rect>) {
    let mut st = state(root.ctx());
    sync_sidebar(app, &mut st);
    if st.sidebar {
        let free = root.available_rect_before_wrap();
        let panel = egui::Panel::right("desktop-sidebar")
            .frame(panel_frame(root, 10, 8))
            .default_size(330_f32.min(free.width() * 0.4))
            .size_range(280_f32..=(free.width() * 0.5).max(300.))
            .resizable(true)
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    // Tabs wrap onto a second row when the panel is narrow.
                    let close = 28.;
                    ui.allocate_ui(egui::vec2(ui.available_width() - close, 0.), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(2., 4.);
                            for (i, (label, _)) in TABS.iter().enumerate() {
                                if crate::icons::tab(ui, label, st.tab == i).clicked() {
                                    *tab_flag(app, st.tab) = false;
                                    st.tab = i;
                                    *tab_flag(app, i) = true;
                                }
                            }
                        });
                    });
                    let x = egui::Button::new(egui::RichText::new(crate::menu::glyph::X).size(14.)).frame(false);
                    if ui.add_sized(egui::vec2(close - 4., 24.), x).on_hover_text("Close panel").clicked() {
                        *tab_flag(app, st.tab) = false;
                    }
                });
                ui.add_space(6.);
                let rect = ui.available_rect_before_wrap();
                crate::theme::dock(ui.ctx(), TABS[st.tab].1, rect);
                ui.allocate_rect(rect, egui::Sense::hover());
            });
        rects.push(panel.response.rect);
    }
    set_state(root.ctx(), st);
}

/// Opens the desktop panel on its last tab, or closes it.
pub fn toggle_sidebar(app: &mut App3d, ctx: &egui::Context) {
    let st = state(ctx);
    let open = (0..TABS.len()).any(|i| *tab_flag(app, i));
    for i in 0..TABS.len() {
        *tab_flag(app, i) = !open && i == st.tab;
    }
}

pub fn sidebar_open(app: &mut App3d) -> bool {
    (0..TABS.len()).any(|i| *tab_flag(app, i))
}

fn panel_frame(ui: &egui::Ui, x: i8, y: i8) -> egui::Frame {
    egui::Frame::new().fill(ui.visuals().panel_fill).inner_margin(egui::Margin::symmetric(x, y))
}

/// A square glyph button the size of a touch target.
fn glyph_button(ui: &mut egui::Ui, glyph: &str, selected: bool, enabled: bool) -> egui::Response {
    let text = egui::RichText::new(glyph).family(egui::FontFamily::Name("phosphor".into())).size(22.);
    ui.add_enabled(enabled, egui::Button::new(text).selected(selected).min_size(egui::vec2(44., 44.)))
}

fn header(ui: &mut egui::Ui, app: &mut App3d, st: &mut State) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.;
        let menu = egui::RichText::new(g::LIST).family(egui::FontFamily::Name("phosphor".into())).size(22.);
        egui::containers::menu::MenuButton::new(menu)
            .config(egui::containers::menu::MenuConfig::new().style(crate::menu::style))
            .ui(ui, |ui| crate::ui::menus(ui, app))
            .0
            .on_hover_text("Menu");
        let title = crate::files::title(app.path.as_deref(), &app.editor);
        ui.add(egui::Label::new(egui::RichText::new(title).strong()).truncate());
        let undo_enabled = app.editor.undo_label().is_some();
        if glyph_button(ui, g::ARROW_U_UP_LEFT, false, undo_enabled).on_hover_text("Undo (two-finger tap)").clicked() {
            undo(app);
        }
        let redo_enabled = app.editor.redo_label().is_some();
        if glyph_button(ui, g::ARROW_U_UP_RIGHT, false, redo_enabled).on_hover_text("Redo (three-finger tap)").clicked()
        {
            redo(app);
        }
        ui.separator();
        for slot in
            [Slot::Tool(T::Orbit), Slot::Tool(T::Pan), Slot::Tool(T::Zoom), Slot::Tool(T::ZoomWindow), Slot::Extents]
        {
            slot_button(ui, app, &slot, ICON);
        }
        views_menu(ui, app);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if glyph_button(ui, g::SIDEBAR_SIMPLE, st.sidebar, true).on_hover_text("Panels").clicked() {
                st.sidebar = !st.sidebar;
                let tab = st.tab;
                for i in 0..TABS.len() {
                    *tab_flag(app, i) = st.sidebar && i == tab;
                }
            }
            if glyph_button(ui, g::MAGNIFYING_GLASS, false, true).on_hover_text("Search commands").clicked() {
                crate::command_search::open(app);
            }
        });
    });
}

fn views_menu(ui: &mut egui::Ui, app: &mut App3d) {
    let label = egui::RichText::new(g::CUBE).family(egui::FontFamily::Name("phosphor".into())).size(22.);
    egui::containers::menu::MenuButton::new(label)
        .config(egui::containers::menu::MenuConfig::new().style(crate::menu::style))
        .ui(ui, |ui| {
            use rysmo_math::StandardView;
            for (view, name) in [
                (StandardView::Iso, "Iso"),
                (StandardView::Top, "Top"),
                (StandardView::Front, "Front"),
                (StandardView::Back, "Back"),
                (StandardView::Left, "Left"),
                (StandardView::Right, "Right"),
                (StandardView::Bottom, "Bottom"),
            ] {
                if crate::menu::button(ui, g::CUBE, name, "").clicked() {
                    app.remember_view();
                    app.camera.set_standard_view(view);
                }
            }
            ui.separator();
            let mut parallel = app.camera.parallel;
            if crate::menu::toggle(ui, g::CUBE, "Parallel projection", &mut parallel).changed() {
                app.remember_view();
                app.camera.parallel = parallel;
            }
            if crate::menu::enabled(ui, !app.camera_history.is_empty(), g::ARROW_COUNTER_CLOCKWISE, "Previous view", "")
                .clicked()
            {
                app.previous_view();
            }
            ui.separator();
            for (kind, glyph) in
                [(T::PositionCamera, g::CAMERA), (T::LookAround, g::EYE), (T::Walk, g::PERSON_SIMPLE_WALK)]
            {
                if crate::menu::button(ui, glyph, kind.name(), "").clicked() {
                    app.set_tool(kind);
                }
            }
        })
        .0
        .on_hover_text("Views and camera");
}

/// A tool's modifier keys as named modes. `exclusive` modes replace one another; the
/// others switch on and off independently.
struct Modes {
    exclusive: bool,
    modes: &'static [(&'static str, Mods)],
    axes: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Mods {
    shift: bool,
    ctrl: bool,
    alt: bool,
}

const SHIFT: Mods = Mods { shift: true, ctrl: false, alt: false };
const CTRL: Mods = Mods { shift: false, ctrl: true, alt: false };
const ALT: Mods = Mods { shift: false, ctrl: false, alt: true };
const CTRL_SHIFT: Mods = Mods { shift: true, ctrl: true, alt: false };

fn modes(tool: T) -> Modes {
    let independent = |modes, axes| Modes { exclusive: false, modes, axes };
    let exclusive = |modes| Modes { exclusive: true, modes, axes: false };
    match tool {
        T::Select => exclusive(&[("Add", CTRL), ("Toggle", SHIFT), ("Remove", CTRL_SHIFT)]),
        T::Eraser => exclusive(&[("Soften", CTRL), ("Hide", SHIFT), ("Unsoften", CTRL_SHIFT)]),
        T::PaintBucket => exclusive(&[
            ("Adjacent matches", CTRL),
            ("All matches", SHIFT),
            ("Connected matches", CTRL_SHIFT),
            ("Sample", ALT),
        ]),
        T::Line | T::Freehand => independent(&[("Lock direction", SHIFT)], true),
        // For these the axis buttons pick the drawing plane.
        T::Rectangle | T::Circle | T::Polygon => independent(&[], true),
        T::Arc | T::ThreePointArc | T::CenterArc | T::Pie => independent(&[("Lock direction", SHIFT)], true),
        T::RotatedRectangle => independent(&[("Set baseline", ALT)], false),
        T::PushPull => independent(&[("New starting face", CTRL)], false),
        T::FollowMe => independent(&[("Surface boundary", ALT)], false),
        T::Move => independent(&[("Copy", CTRL)], true),
        T::Rotate => independent(&[("Copy", CTRL)], true),
        T::Scale => independent(&[("Uniform", SHIFT), ("About centre", CTRL)], false),
        T::TapeMeasure => independent(&[("Measure only", CTRL)], true),
        T::Protractor => independent(&[("Angled guide", CTRL)], true),
        T::Orbit => independent(&[("Pan", SHIFT)], false),
        T::PositionCamera => independent(&[("On surface", SHIFT)], false),
        T::Walk => independent(&[("Up and down", SHIFT), ("Run", CTRL), ("Through walls", ALT)], false),
        _ => independent(&[], false),
    }
}

fn set_mods(app: &mut App3d, mods: Mods) {
    if app.touch.shift != mods.shift {
        tool_key(app, Key::Shift(mods.shift));
    }
    (app.touch.shift, app.touch.ctrl, app.touch.alt) = (mods.shift, mods.ctrl, mods.alt);
}

fn options(ui: &mut egui::Ui, app: &mut App3d) {
    let tool = app.tools.active();
    let modes = modes(tool);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6., 6.);
        let current = Mods { shift: app.touch.shift, ctrl: app.touch.ctrl, alt: app.touch.alt };
        for &(label, mods) in modes.modes {
            let on = if modes.exclusive {
                current == mods
            } else {
                (!mods.shift || current.shift) && (!mods.ctrl || current.ctrl) && (!mods.alt || current.alt)
            };
            if ui.add(egui::Button::new(label).selected(on).min_size(egui::vec2(44., 36.))).clicked() {
                let next = if modes.exclusive {
                    if on { Mods { shift: false, ctrl: false, alt: false } } else { mods }
                } else {
                    Mods {
                        shift: current.shift ^ mods.shift,
                        ctrl: current.ctrl ^ mods.ctrl,
                        alt: current.alt ^ mods.alt,
                    }
                };
                set_mods(app, next);
            }
        }
        if modes.axes {
            if !modes.modes.is_empty() {
                ui.separator();
            }
            for (label, key, color) in [
                ("Red", Key::Right, egui::Color32::from_rgb(0xe0, 0x55, 0x50)),
                ("Green", Key::Left, egui::Color32::from_rgb(0x4c, 0xb8, 0x5c)),
                ("Blue", Key::Up, egui::Color32::from_rgb(0x5a, 0x8c, 0xf0)),
                ("Free", Key::Down, ui.visuals().text_color()),
            ] {
                let text = egui::RichText::new(label).color(color);
                if ui
                    .add(egui::Button::new(text).min_size(egui::vec2(44., 36.)))
                    .on_hover_text("Lock an axis")
                    .clicked()
                {
                    tool_key(app, key);
                }
            }
        }
        if !modes.modes.is_empty() || modes.axes {
            ui.separator();
        }
        crate::tool_panel::panel(ui, app);
    });
}

fn status(ui: &mut egui::Ui, app: &mut App3d, st: &mut State) {
    ui.horizontal(|ui| {
        let (label, hint) = crate::ui::measurement_label(app);
        let value = if app.vcb.is_empty() { hint } else { app.vcb.clone() };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let text = format!("{label}  {value}");
            let pill = egui::Button::new(
                egui::RichText::new(format!("{}  {text}", g::NUMPAD)).family(egui::FontFamily::Proportional).size(16.),
            )
            .selected(st.keypad)
            .min_size(egui::vec2(180., 40.));
            if ui.add(pill).on_hover_text("Measurements: type an exact value").clicked() {
                st.keypad = !st.keypad;
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                if app.editor.is_dirty() {
                    ui.label(egui::RichText::new("●").color(crate::theme::ACCENT)).on_hover_text("Unsaved changes");
                }
                if app.message.is_some() && ui.small_button("×").on_hover_text("Dismiss message").clicked() {
                    app.message = None;
                }
                let status = app.status();
                ui.add(egui::Label::new(&status).truncate());
            });
        });
    });
}

fn rail(ui: &mut egui::Ui, app: &mut App3d, st: &mut State) {
    ui.spacing_mut().item_spacing.y = 4.;
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            for (index, category) in CATEGORIES.iter().enumerate() {
                let current = category.slots.iter().position(|slot| slot_active(app, slot));
                let shown = current.unwrap_or(st.last[index].min(category.slots.len() - 1));
                let response = slot_icon(ui, &category.slots[shown], current.is_some() || st.flyout == Some(index));
                let response = response.on_hover_text(category.name);
                ui.label(egui::RichText::new(category.name).size(10.).weak());
                if response.clicked() {
                    let several = category.slots.len() > 1;
                    if st.flyout == Some(index) {
                        st.flyout = None;
                    } else if current.is_some() && several {
                        st.flyout = Some(index);
                    } else {
                        st.flyout = None;
                        run_slot(app, &category.slots[shown]);
                    }
                }
                if response.long_touched() || response.secondary_clicked() {
                    st.flyout = Some(index);
                }
            }
        });
    });
}

fn slot_active(app: &App3d, slot: &Slot) -> bool {
    match slot {
        Slot::Tool(kind) => app.tools.active() == *kind,
        Slot::Action(Action::SectionPlane) => app.section_placing.is_some(),
        Slot::Action(Action::Text3d) => app.text3d.is_some(),
        Slot::Action(Action::Drape) => app.drape.is_some(),
        Slot::Action(Action::Stamp) => app.stamp.is_some(),
        Slot::Action(Action::AddDetail) => app.detail.is_some(),
        Slot::Action(Action::FlipEdge) => app.flip.is_some(),
        Slot::Action(Action::Smoove) => app.smoove.is_some(),
        Slot::Action(Action::TerrainGrid) => app.terrain.is_some(),
        _ => false,
    }
}

fn slot_icon(ui: &mut egui::Ui, slot: &Slot, selected: bool) -> egui::Response {
    let icon = match slot {
        Slot::Tool(kind) => crate::icons::Icon::Tool(*kind),
        Slot::Action(action) => crate::icons::Icon::Action(*action),
        Slot::Extents | Slot::Gap => crate::icons::Icon::Extents,
    };
    crate::icons::button(ui, icon, selected, ICON)
}

fn slot_name(slot: &Slot) -> &'static str {
    match slot {
        Slot::Tool(kind) => kind.name(),
        Slot::Action(action) => action.label(),
        Slot::Extents => "Zoom Extents",
        Slot::Gap => "",
    }
}

fn run_slot(app: &mut App3d, slot: &Slot) {
    match slot {
        Slot::Tool(kind) => app.set_tool(*kind),
        Slot::Action(action) => actions::run(app, *action),
        Slot::Extents => app.zoom_extents(),
        Slot::Gap => {}
    }
}

/// The full list of a category's tools, beside its rail button.
fn flyout(
    ctx: &egui::Context,
    app: &mut App3d,
    st: &mut State,
    index: usize,
    rail: egui::Rect,
    canvas: egui::Rect,
    rects: &mut Vec<egui::Rect>,
) {
    let category = &CATEGORIES[index];
    let row = 52.;
    let top = (rail.top() + 8. + index as f32 * (ICON + 32.))
        .min(canvas.bottom() - category.slots.len() as f32 * row - 24.)
        .max(canvas.top() + 4.);
    let area = egui::Area::new(egui::Id::unique("tablet-flyout"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(rail.right() + 6., top))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(220.);
                ui.strong(category.name);
                for (i, slot) in category.slots.iter().enumerate() {
                    let selected = slot_active(app, slot);
                    let clicked = ui
                        .horizontal(|ui| {
                            let icon = slot_icon(ui, slot, selected);
                            let label =
                                ui.add(egui::Button::new(slot_name(slot)).frame(false).min_size(egui::vec2(140., 44.)));
                            icon.clicked() || label.clicked()
                        })
                        .inner;
                    if clicked {
                        st.last[index] = i;
                        st.flyout = None;
                        run_slot(app, slot);
                    }
                }
            });
        });
    rects.push(area.response.rect);
    // A tap elsewhere closes the list.
    if area.response.clicked_elsewhere() && st.flyout == Some(index) {
        let on_rail = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| rail.contains(p));
        if !on_rail {
            st.flyout = None;
        }
    }
}

/// Keeps one sidebar tab open and follows panels opened from menus or commands.
fn sync_sidebar(app: &mut App3d, st: &mut State) {
    if let Some(opened) = (0..TABS.len()).find(|&i| *tab_flag(app, i) && (!st.sidebar || i != st.tab)) {
        st.sidebar = true;
        st.tab = opened;
    }
    if st.sidebar && !*tab_flag(app, st.tab) {
        st.sidebar = false;
    }
    for i in 0..TABS.len() {
        *tab_flag(app, i) = st.sidebar && i == st.tab;
    }
}

fn sidebar(ui: &mut egui::Ui, app: &mut App3d, st: &mut State) {
    ui.horizontal_wrapped(|ui| {
        for (i, (label, _)) in TABS.iter().enumerate() {
            if crate::icons::tab(ui, label, st.tab == i).clicked() {
                *tab_flag(app, st.tab) = false;
                st.tab = i;
                *tab_flag(app, i) = true;
            }
        }
    });
    ui.separator();
    let rect = ui.available_rect_before_wrap();
    crate::theme::dock(ui.ctx(), TABS[st.tab].1, rect);
    ui.allocate_rect(rect, egui::Sense::hover());
}

/// A keypad for the Measurements box, so typing exact values needs no system keyboard.
fn keypad(ctx: &egui::Context, app: &mut App3d, st: &mut State, canvas: egui::Rect, rects: &mut Vec<egui::Rect>) {
    const KEYS: [[&str; 5]; 5] = [
        ["7", "8", "9", "⌫", "C"],
        ["4", "5", "6", ",", "x"],
        ["1", "2", "3", "-", "/"],
        [".", "0", "deg", "s", "Enter"],
        ["mm", "cm", "m", "'", "\""],
    ];
    let key = egui::vec2(56., 46.);
    let size = egui::vec2(key.x * 5. + 4. * 6. + 24., key.y * 5. + 4. * 6. + 64.);
    let pos = canvas.right_bottom() - size - egui::vec2(12., 12.);
    let area = egui::Area::new(egui::Id::unique("tablet-keypad"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos.max(canvas.min))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (label, hint) = crate::ui::measurement_label(app);
                    let value = if app.vcb.is_empty() { hint } else { app.vcb.clone() };
                    ui.label(egui::RichText::new(label).weak());
                    ui.label(egui::RichText::new(value).monospace().size(18.));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Close").clicked() {
                            st.keypad = false;
                        }
                    });
                });
                ui.spacing_mut().item_spacing = egui::vec2(6., 6.);
                egui::Grid::new("tablet-keypad-keys").spacing(egui::vec2(6., 6.)).show(ui, |ui| {
                    for row in KEYS {
                        for label in row {
                            let text = egui::RichText::new(label).size(18.);
                            let button = egui::Button::new(text).min_size(key).selected(label == "Enter");
                            if ui.add(button).clicked() {
                                match label {
                                    "⌫" => {
                                        app.vcb.pop();
                                    }
                                    "C" => app.vcb.clear(),
                                    "Enter" => apply_vcb(app),
                                    other => app.vcb.push_str(other),
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        });
    rects.push(area.response.rect);
}

/// Selection actions have no tool icon; they use the menu glyphs.
fn action_glyph(action: Action) -> &'static str {
    match action {
        Action::OpenGroup => g::FOLDER_OPEN,
        Action::Explode => g::ARROWS_OUT,
        Action::MakeGroup => g::BOUNDING_BOX,
        Action::MakeComponent => g::CUBE,
        Action::HideGeometry => g::EYE_SLASH,
        Action::Erase => g::TRASH,
        _ => g::DOTS_THREE,
    }
}

/// Actions for what is selected, shown while the Select tool is active.
fn selection_bar(ctx: &egui::Context, app: &mut App3d, canvas: egui::Rect, rects: &mut Vec<egui::Rect>) {
    if app.tools.active() != T::Select || app.editor.selection().is_empty() {
        return;
    }
    let available = actions::available(app);
    let enabled = |action: Action| available.iter().any(|&(a, on)| a == action && on);
    let faces = enabled(Action::ReverseFaces);
    let edges = enabled(Action::SoftenEdges);
    let instances = enabled(Action::Explode);
    let mut slots: Vec<Slot> = Vec::new();
    if faces {
        slots.extend([
            Slot::Tool(T::PushPull),
            Slot::Tool(T::Offset),
            Slot::Tool(T::FollowMe),
            Slot::Tool(T::PaintBucket),
        ]);
    }
    slots.extend([Slot::Tool(T::Move), Slot::Tool(T::Rotate), Slot::Tool(T::Scale)]);
    if instances {
        slots.extend([Slot::Action(Action::OpenGroup), Slot::Action(Action::Explode)]);
    }
    if faces || edges {
        slots.extend([Slot::Action(Action::MakeGroup), Slot::Action(Action::MakeComponent)]);
    }
    slots.extend([Slot::Action(Action::HideGeometry), Slot::Action(Action::Erase)]);
    let count = app.editor.selection().len();
    let area = egui::Area::new(egui::Id::unique("tablet-selection"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(canvas.center_bottom() - egui::vec2(0., 12.))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{count} selected")).weak());
                    for slot in &slots {
                        let response = match slot {
                            Slot::Action(action) => glyph_button(ui, action_glyph(*action), false, true),
                            _ => slot_icon(ui, slot, false),
                        };
                        if response.on_hover_text(slot_name(slot)).clicked() {
                            run_slot(app, slot);
                        }
                    }
                    if ui.add(egui::Button::new("Info").min_size(egui::vec2(44., 40.))).clicked() {
                        app.show_entity_info = true;
                        app.inspector_tab = 1;
                    }
                });
            });
        });
    rects.push(area.response.rect);
}
