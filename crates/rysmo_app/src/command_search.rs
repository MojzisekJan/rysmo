//! Search commands without changing the document until an enabled result is chosen.
use crate::{
    actions::{self, Action},
    state::App3d,
};
use bevy_egui::egui;
use rysmo_tools::ToolKind;

#[derive(Default)]
pub struct SearchState {
    pub open: bool,
    pub query: String,
    selected: usize,
    focus: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Settings,
    Inspector,
    Materials,
    Tags,
    Components,
    Scenes,
    Sections,
    Help,
    About,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum File {
    New,
    Open,
    Save,
    SaveAs,
    Obj,
    Glb,
    Dae,
    Dxf,
    Stl,
    ThreeMf,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Tool(ToolKind),
    Action(Action),
    Panel(Panel),
    File(File),
    Copy,
    Cut,
    Paste,
    PasteInPlace,
    Duplicate,
    Flip,
    Solid(rysmo_model::solids::BooleanOp),
    Redo,
    Previous,
    ShowGuides,
    ShowHiddenGeometry,
    PositionTexture,
}
struct Entry {
    command: Command,
    label: String,
    category: &'static str,
    shortcut: &'static str,
    enabled: bool,
}

pub fn open(app: &mut App3d) {
    let selected =
        entries(app).iter().position(|entry| entry.command == Command::Tool(app.tools.active())).unwrap_or(0);
    app.command_search = SearchState { open: true, focus: true, selected, ..Default::default() };
    app.context_menu = None;
}

fn entries(app: &App3d) -> Vec<Entry> {
    let mut result = Vec::new();
    for tool in ToolKind::ALL {
        result.push(Entry {
            command: Command::Tool(tool),
            label: tool.name().into(),
            category: "Tool",
            shortcut: tool.shortcut(),
            enabled: true,
        });
    }
    for (action, enabled) in actions::available(app) {
        let label = match action {
            Action::OpenGroup => "Edit Group or Component",
            Action::CloseGroup => "Close Group or Component",
            _ => action.label(),
        };
        result.push(Entry {
            command: Command::Action(action),
            label: label.into(),
            category: "Edit",
            shortcut: "",
            enabled,
        });
    }
    for (file, label, shortcut) in [
        (File::New, "New", "Ctrl+N"),
        (File::Open, "Open...", "Ctrl+O"),
        (File::Save, "Save", "Ctrl+S"),
        (File::SaveAs, "Save As...", "Ctrl+Shift+S"),
        (File::Obj, "Export OBJ...", ""),
        (File::Glb, "Export glTF (.glb)...", ""),
        (File::Dae, "Export COLLADA (.dae)...", ""),
        (File::Dxf, "Export DXF (.dxf)...", ""),
        (File::Stl, "Export STL for 3D printing...", ""),
        (File::ThreeMf, "Export 3MF for 3D printing...", ""),
    ] {
        result.push(Entry {
            command: Command::File(file),
            label: label.into(),
            category: "File",
            shortcut,
            enabled: true,
        });
    }
    for (panel, label) in [
        (Panel::Settings, "Settings"),
        (Panel::Inspector, "Inspector"),
        (Panel::Materials, "Materials"),
        (Panel::Tags, "Tags"),
        (Panel::Components, "Components"),
        (Panel::Scenes, "Scenes"),
        (Panel::Sections, "Section Planes"),
        (Panel::Help, "Editor Help"),
        (Panel::About, "About Rysmo"),
    ] {
        result.push(Entry {
            command: Command::Panel(panel),
            label: label.into(),
            category: "Window",
            shortcut: "",
            enabled: true,
        });
    }
    let selected = !app.editor.selection().is_empty();
    let clipboard = app.object_clipboard.is_some();
    for (command, label, shortcut, enabled) in [
        (Command::Copy, "Copy", "Ctrl+C", selected),
        (Command::Cut, "Cut", "Ctrl+X", selected),
        (Command::Paste, "Paste", "Ctrl+V", clipboard),
        (Command::PasteInPlace, "Paste in Place", "Ctrl+Shift+V", clipboard),
        (Command::Duplicate, "Duplicate", "Ctrl+D", selected),
        (Command::Flip, "Flip / Mirror", "", selected),
        (Command::Redo, "Redo", "Ctrl+Shift+Z", app.editor.redo_label().is_some()),
        (Command::Previous, "Previous View", "", !app.camera_history.is_empty()),
        (
            Command::PositionTexture,
            "Position Texture",
            "",
            app.editor.selection().iter().any(|e| matches!(e, rysmo_model::Element::Face(_))),
        ),
    ] {
        result.push(Entry { command, label: label.into(), category: "Edit", shortcut, enabled });
    }
    let style = app.editor.doc().style;
    for (command, label, shown) in [
        (Command::ShowGuides, "Guides", style.show_guides),
        (Command::ShowHiddenGeometry, "Hidden Geometry", style.show_hidden_geometry),
    ] {
        result.push(Entry {
            command,
            label: format!("{} {label}", if shown { "Hide" } else { "Show" }),
            category: "Window",
            shortcut: "",
            enabled: true,
        });
    }
    let solid_enabled = app.editor.selection().len() == 2
        && app.editor.selection().iter().all(|e| matches!(e, rysmo_model::Element::Instance(_)));
    for operation in [
        rysmo_model::solids::BooleanOp::Union,
        rysmo_model::solids::BooleanOp::Subtract,
        rysmo_model::solids::BooleanOp::Intersect,
        rysmo_model::solids::BooleanOp::Trim,
        rysmo_model::solids::BooleanOp::Split,
        rysmo_model::solids::BooleanOp::OuterShell,
    ] {
        result.push(Entry {
            command: Command::Solid(operation),
            label: operation.name().into(),
            category: "Edit",
            shortcut: "",
            enabled: solid_enabled,
        });
    }
    result.sort_by(|a, b| a.label.cmp(&b.label).then(a.category.cmp(b.category)));
    result
}

fn matches(entry: &Entry, query: &str) -> bool {
    let aliases = match entry.command {
        Command::Panel(Panel::Settings) => "appearance renderer units precision shadows lighting",
        Command::Tool(ToolKind::TapeMeasure) => "ruler guides measure distance resize",
        Command::Tool(ToolKind::ThreePointArc) => "3 point through pivot curve",
        Command::Tool(ToolKind::CenterArc) => "centre radius angle sweep",
        Command::Tool(ToolKind::Pie) => "sector wedge radius angle",
        Command::Tool(ToolKind::RotatedRectangle) => "three point tilt plane",
        Command::Action(Action::ZoomExtents) => "fit model camera",
        Command::Action(Action::SolidTools) => "boolean solids group component",
        Command::Action(Action::Fillet) => "round corner radius tangent fillet",
        Command::Solid(rysmo_model::solids::BooleanOp::Trim | rysmo_model::solids::BooleanOp::Subtract) => {
            "boolean solid difference cut"
        }
        Command::Solid(rysmo_model::solids::BooleanOp::Intersect) => "boolean solid intersection shared",
        Command::Solid(rysmo_model::solids::BooleanOp::Union | rysmo_model::solids::BooleanOp::OuterShell) => {
            "boolean solid combine merge"
        }
        Command::Solid(rysmo_model::solids::BooleanOp::Split) => "boolean solid divide pieces",
        Command::File(File::Obj) => "wavefront export",
        Command::File(File::Glb) => "gltf glb export",
        Command::File(File::Dae) => "collada dae export textures",
        Command::File(File::Dxf) => "autocad dxf cad export",
        Command::File(File::Stl) => "stl 3d print printing slicer export",
        Command::File(File::ThreeMf) => "3mf 3d print printing slicer export",
        _ => "",
    };
    let haystack = format!("{} {} {} {}", entry.label, entry.category, entry.shortcut, aliases).to_lowercase();
    query.split_whitespace().all(|word| haystack.contains(&word.to_lowercase()))
}

/// Matching entries, best first: the exact name, then names starting with the query,
/// then names containing it, then matches of the category, shortcut or aliases.
fn ranked(entries: Vec<Entry>, query: &str) -> Vec<Entry> {
    let query = query.trim().to_lowercase();
    let mut results: Vec<Entry> = entries.into_iter().filter(|entry| matches(entry, &query)).collect();
    if !query.is_empty() {
        results.sort_by_key(|entry| {
            let label = entry.label.to_lowercase();
            if label == query {
                0
            } else if label.starts_with(&query) {
                1
            } else if label.contains(&query) {
                2
            } else {
                3
            }
        });
    }
    results
}

fn execute(app: &mut App3d, command: Command) {
    // Recheck availability against the current selection/history, including actions
    // that were enabled before another panel changed the model during this frame.
    if !entries(app).iter().any(|entry| entry.command == command && entry.enabled) {
        return;
    }
    app.command_search.open = false;
    match command {
        Command::Tool(tool) => app.set_tool(tool),
        Command::Action(action) => actions::run(app, action),
        Command::Panel(panel) => match panel {
            Panel::Settings => app.show_settings = true,
            Panel::Inspector => app.show_entity_info = true,
            Panel::Materials => app.show_materials = true,
            Panel::Tags => app.show_tags = true,
            Panel::Components => app.show_components = true,
            Panel::Scenes => app.show_scenes = true,
            Panel::Sections => app.show_sections = true,
            Panel::Help => app.help.open = true,
            Panel::About => app.show_about = true,
        },
        Command::File(file) => match file {
            File::New => crate::unsaved::request(app, crate::unsaved::Action::New),
            File::Open => crate::unsaved::request(app, crate::unsaved::Action::Open),
            File::Save => crate::files::save(app, false),
            File::SaveAs => crate::files::save(app, true),
            File::Obj => crate::files::export_obj(app),
            File::Glb => crate::files::export_glb(app),
            File::Dae => crate::files::export_collada(app),
            File::Dxf => crate::files::export_dxf(app),
            File::Stl => crate::files::export_for_print(app, crate::print_check::Format::Stl),
            File::ThreeMf => crate::files::export_for_print(app, crate::print_check::Format::ThreeMf),
        },
        Command::Copy => {
            crate::object_clipboard::copy(app, false);
        }
        Command::Cut => {
            crate::object_clipboard::copy(app, true);
        }
        Command::Paste => crate::object_clipboard::paste(app, false),
        Command::PasteInPlace => crate::object_clipboard::paste(app, true),
        Command::Duplicate => crate::object_clipboard::duplicate(app),
        Command::Flip => crate::mirror::begin(app),
        Command::Solid(operation) => crate::solid_tools::begin_operation(app, operation),
        Command::Redo => crate::input::redo(app),
        Command::Previous => app.previous_view(),
        Command::PositionTexture => crate::texture_view::begin(app),
        Command::ShowGuides | Command::ShowHiddenGeometry => {
            let mut style = app.editor.doc().style;
            if command == Command::ShowGuides {
                style.show_guides = !style.show_guides;
            } else {
                style.show_hidden_geometry = !style.show_hidden_geometry;
            }
            if let Err(error) = app.editor.apply(rysmo_model::Command::SetStyle { style }) {
                app.message = Some(error.to_string());
            }
        }
    }
}

pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.command_search.open {
        return;
    }
    let mut open = true;
    let mut chosen = None;
    let result = crate::theme::window("Search Commands")
        .open(&mut open)
        .collapsible(false)
        .default_size(egui::vec2(560., 400.))
        .min_size(egui::vec2(280., 220.))
        .show(ctx, |ui| {
            let (up, down, enter, escape) = ui.input_mut(|input| {
                (
                    input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                    input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                )
            });
            if escape {
                app.command_search.open = false;
            }
            let opening = app.command_search.focus;
            let field = ui.add(
                crate::theme::singleline(&mut app.command_search.query)
                    .id(egui::Id::new("command-search-query"))
                    .desired_width(f32::INFINITY)
                    .hint_text("Search tools, actions, windows or shortcuts"),
            );
            if app.command_search.focus {
                field.request_focus();
                app.command_search.focus = false;
            }
            if field.changed() {
                app.command_search.selected = 0;
            }
            let results = ranked(entries(app), &app.command_search.query);
            let count = results.len();
            if count > 0 {
                app.command_search.selected = app.command_search.selected.min(count - 1);
                if down {
                    app.command_search.selected = (app.command_search.selected + 1) % count;
                }
                if up {
                    app.command_search.selected = (app.command_search.selected + count - 1) % count;
                }
                if enter && results[app.command_search.selected].enabled {
                    chosen = Some(results[app.command_search.selected].command);
                }
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height((ui.available_height() - 30.).max(80.))
                .show(ui, |ui| {
                    if results.is_empty() {
                        ui.weak("No matching commands.");
                    }
                    for (index, entry) in results.iter().enumerate() {
                        let label = format!("{}   ·   {}", entry.label, entry.category);
                        let response = ui
                            .add_enabled_ui(entry.enabled, |ui| {
                                ui.add_sized(
                                    [ui.available_width(), 34.],
                                    egui::Button::new(label)
                                        .selected(index == app.command_search.selected)
                                        .wrap_mode(egui::TextWrapMode::Truncate),
                                )
                            })
                            .inner;
                        if (opening || up || down || field.changed()) && index == app.command_search.selected {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                        if response.clicked() {
                            chosen = Some(entry.command);
                        }
                        if !entry.shortcut.is_empty() {
                            response.on_hover_text(entry.shortcut);
                        }
                    }
                });
            ui.small("Up/Down choose a command · Enter runs it · Escape closes search");
        });
    if let Some(result) = result {
        let rect = result.response.rect;
        app.ui_rects.push((
            rysmo_math::DVec2::new(rect.min.x as f64, rect.min.y as f64),
            rysmo_math::DVec2::new(rect.max.x as f64, rect.max.y as f64),
        ));
    }
    app.command_search.open &= open;
    if app.command_search.open
        && let Some(command) = chosen
    {
        execute(app, command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::{Command as ModelCommand, Editor};
    #[test]
    fn covers_tools_and_shared_actions_and_rechecks_selection_before_execution() {
        let mut app = App3d::new(Editor::new());
        for tool in ToolKind::ALL {
            assert!(entries(&app).iter().any(|e| e.command == Command::Tool(tool) && matches(e, tool.name())));
        }
        for (action, _) in actions::available(&app) {
            assert!(entries(&app).iter().any(|e| e.command == Command::Action(action)));
        }
        let elements = app
            .editor
            .apply(ModelCommand::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        app.editor.set_selection(elements);
        let command = entries(&app).into_iter().find(|e| matches(e, "MAKE GROUP")).unwrap();
        assert!(command.enabled);
        app.editor.clear_selection();
        let before = app.editor.doc().clone();
        open(&mut app);
        execute(&mut app, command.command);
        assert!(app.command_search.open);
        assert_eq!(app.editor.doc(), &before);
        execute(&mut app, Command::Tool(ToolKind::TapeMeasure));
        assert!(!app.command_search.open);
        assert_eq!(app.tools.active(), ToolKind::TapeMeasure);
        assert_eq!(app.editor.doc(), &before);
    }

    #[test]
    fn exact_and_prefix_names_rank_first() {
        let app = App3d::new(rysmo_model::Editor::new());
        for (query, tool) in [("text", ToolKind::Text), ("move", ToolKind::Move), ("arc", ToolKind::Arc)] {
            let first = ranked(entries(&app), query).into_iter().next().unwrap();
            assert_eq!(first.command, Command::Tool(tool), "{query}");
        }
    }

    #[test]
    fn solid_commands_open_the_selected_operation_and_recheck_operands() {
        use rysmo_model::solids::BooleanOp;
        let mut app = App3d::new(Editor::new());
        let mut groups = Vec::new();
        for corner in [DVec3::ZERO, DVec3::X * 3.] {
            let elements = app
                .editor
                .apply(ModelCommand::DrawRectangle { corner, u: DVec3::X, v: DVec3::Y, facing: None })
                .unwrap()
                .created;
            groups.extend(app.editor.apply(ModelCommand::MakeGroup { elements }).unwrap().created);
        }
        app.editor.set_selection(groups);
        let before = app.editor.doc().clone();
        let entry = entries(&app).into_iter().find(|e| e.command == Command::Solid(BooleanOp::Split)).unwrap();
        assert!(entry.enabled && matches(&entry, "solid split"));
        for (query, operation) in
            [("trim", BooleanOp::Trim), ("split", BooleanOp::Split), ("outer shell", BooleanOp::OuterShell)]
        {
            let first = entries(&app).into_iter().find(|entry| matches(entry, query)).unwrap();
            assert_eq!(first.command, Command::Solid(operation));
        }
        open(&mut app);
        execute(&mut app, entry.command);
        assert_eq!(app.solid_tools.as_ref().unwrap().operation, BooleanOp::Split);
        assert_eq!(app.editor.doc(), &before);
        app.editor.clear_selection();
        open(&mut app);
        execute(&mut app, Command::Solid(BooleanOp::Trim));
        assert!(app.command_search.open);
        assert_eq!(app.editor.doc(), &before);
    }

    fn frame(ctx: &egui::Context, app: &mut App3d, events: Vec<egui::Event>) {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 800.))),
                events,
                ..Default::default()
            },
            |ui| panel(ui.ctx(), app),
        );
        output.textures_delta.clear();
    }
    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn typing_and_enter_activate_a_tool_without_editing_the_document() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = App3d::new(Editor::new());
        let before = app.editor.doc().clone();
        open(&mut app);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        frame(&ctx, &mut app, vec![egui::Event::Text("push pull".into())]);
        assert_eq!(app.command_search.query, "push pull");
        frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert!(!app.command_search.open);
        assert_eq!(app.tools.active(), ToolKind::PushPull);
        assert_eq!(app.editor.doc(), &before);
        assert!(app.editor.undo_label().is_none());
    }

    #[test]
    fn escape_closes_search_without_cancelling_an_unfinished_line() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = App3d::new(Editor::new());
        app.set_tool(ToolKind::Line);
        let view = app.view();
        let at = view.project(DVec3::ZERO).unwrap();
        app.tools.pointer(
            &mut app.editor,
            &rysmo_tools::Ctx { view: &view, cursor: at, modifiers: Default::default(), scale: 1. },
            rysmo_tools::Pointer::Down { clicks: 1 },
        );
        assert!(app.tools.busy());
        let before = app.editor.doc().clone();
        app.vcb = "125".into();
        open(&mut app);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        frame(&ctx, &mut app, vec![key(egui::Key::Escape), key(egui::Key::Enter)]);
        assert_eq!(app.vcb, "125");
        assert!(!app.command_search.open);
        assert!(app.tools.busy());
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn arrow_keys_choose_results_without_moving_text_caret_or_running_empty_search() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = App3d::new(Editor::new());
        open(&mut app);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        frame(&ctx, &mut app, vec![egui::Event::Text("zoom".into())]);
        assert_eq!(app.command_search.selected, 0);
        frame(&ctx, &mut app, vec![key(egui::Key::ArrowDown)]);
        assert_eq!(app.command_search.selected, 1);
        frame(&ctx, &mut app, vec![key(egui::Key::ArrowUp)]);
        assert_eq!(app.command_search.selected, 0);
        assert_eq!(app.command_search.query, "zoom");
        frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert_eq!(app.tools.active(), ToolKind::Zoom);
        open(&mut app);
        app.command_search.query = "no-such-command".into();
        frame(&ctx, &mut app, vec![key(egui::Key::Enter)]);
        assert!(app.command_search.open);
        assert_eq!(app.tools.active(), ToolKind::Zoom);
        assert!(app.editor.undo_label().is_none());
    }
}
