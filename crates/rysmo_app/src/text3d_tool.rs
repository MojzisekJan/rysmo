//! 3D Text settings and placement; drafts stay outside document history.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_math::{DMat4, DVec2, DVec3};
use rysmo_mesh::Mesh;
use rysmo_model::{
    Command, InstanceId,
    text3d::{Text3dAlign, Text3dFont, Text3dSettings},
};
use rysmo_tools::{Pointer, ToolKind};

pub struct Draft {
    pub content: String,
    focus: bool,
    pub settings: Text3dSettings,
    pub height: String,
    pub depth: String,
    pub mesh: Option<Mesh>,
    pub origin: DVec3,
    pub context: Vec<InstanceId>,
    pub revision: u64,
}
impl Draft {
    fn valid(&self, app: &App3d) -> bool {
        self.context == app.editor.context() && self.revision == app.editor.revision()
    }
}
pub fn begin(app: &mut App3d) {
    app.set_tool(ToolKind::Move);
    let settings = app.text3d_defaults.clone();
    app.text3d = Some(Draft {
        content: String::new(),
        focus: true,
        height: format!("{} {}", settings.height * app.editor.doc().units.per_metre(), app.editor.doc().units.suffix()),
        depth: format!("{} {}", settings.depth * app.editor.doc().units.per_metre(), app.editor.doc().units.suffix()),
        settings,
        mesh: None,
        origin: app.editor.active().axes.origin,
        context: app.editor.context().to_vec(),
        revision: app.editor.revision(),
    });
}
pub fn prepare(app: &mut App3d) -> Result<(), String> {
    let draft = app.text3d.as_mut().ok_or("no 3D text draft")?;
    draft.settings.height =
        rysmo_model::parse_length(&draft.height, app.editor.doc().units).map_err(|e| e.to_string())?;
    if draft.settings.filled && draft.settings.extruded {
        draft.settings.depth =
            rysmo_model::parse_length(&draft.depth, app.editor.doc().units).map_err(|e| e.to_string())?;
    }
    draft.mesh = Some(rysmo_model::text3d::build_text(&draft.content, &draft.settings)?);
    app.text3d_defaults = draft.settings.clone();
    Ok(())
}
pub fn pointer(app: &mut App3d, at: DVec2, event: Pointer) {
    if !app.text3d.as_ref().is_some_and(|d| d.valid(app)) {
        app.text3d = None;
        return;
    }
    if app.text3d.as_ref().is_none_or(|d| d.mesh.is_none()) {
        return;
    }
    app.with_ctx(at, |tools, editor, ctx| tools.pointer(editor, ctx, Pointer::Move));
    if let Some(point) = app.tools.preview(&app.editor).inference.map(|i| i.point) {
        app.text3d.as_mut().unwrap().origin = point;
    }
    if matches!(event, Pointer::Down { .. }) {
        finish(app);
    }
}
pub fn finish(app: &mut App3d) {
    let Some(draft) = app.text3d.take() else { return };
    if !draft.valid(app) || draft.mesh.is_none() {
        return;
    }
    let axes = app.editor.active().axes;
    match app.editor.apply(Command::CreateText3d {
        content: draft.content,
        settings: draft.settings,
        origin: draft.origin,
        x: axes.x,
        y: axes.y,
    }) {
        Ok(out) => {
            app.editor.set_selection(out.created);
            app.set_tool(ToolKind::Move);
        }
        Err(e) => app.message = Some(e.to_string()),
    }
}
/// A precise distance from the current drawing origin along the preview direction.
pub fn distance(app: &mut App3d, text: &str) -> Result<(), String> {
    if !app.text3d.as_ref().is_some_and(|d| d.valid(app) && d.mesh.is_some()) {
        return Err("Choose Place before entering a distance.".into());
    }
    let length = rysmo_model::parse_length(text, app.editor.doc().units).map_err(|e| e.to_string())?;
    let axes = app.editor.active().axes;
    let draft = app.text3d.as_mut().unwrap();
    let delta = draft.origin - axes.origin;
    draft.origin = axes.origin + delta.try_normalize().unwrap_or(axes.x) * length;
    finish(app);
    Ok(())
}

pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(draft) = &app.text3d else { return };
    if !draft.valid(app) {
        return;
    }
    let Some(mesh) = &draft.mesh else { return };
    let transform =
        app.editor.context_transform() * DMat4::from_translation(draft.origin) * app.editor.active().axes.basis();
    let view = app.view();
    for (_, edge) in mesh.edges() {
        if let (Some(a), Some(b)) = (
            view.project(transform.transform_point3(mesh.pos(edge.v[0]))),
            view.project(transform.transform_point3(mesh.pos(edge.v[1]))),
        ) {
            painter.line_segment(
                [egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)],
                egui::Stroke::new(1.2, egui::Color32::MAGENTA),
            );
        }
    }
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    panel_sized(ctx, app, egui::vec2(420., 470.));
}

fn panel_sized(ctx: &egui::Context, app: &mut App3d, size: egui::Vec2) {
    if app.text3d.as_ref().is_some_and(|d| !d.valid(app)) {
        app.text3d = None;
    }
    let Some(draft) = app.text3d.as_mut() else { return };
    if draft.mesh.is_some() {
        return;
    }
    let mut open = true;
    let mut place = false;
    let mut cancel = false;
    crate::theme::window("3D Text")
        .open(&mut open)
        .collapsible(false)
        .default_size(size)
        .min_size(egui::vec2(340., 420.))
        .vscroll(true)
        .show(ctx, |ui| {
            let edit = ui.add(
                crate::theme::multiline(&mut draft.content)
                    .id(egui::Id::unique("text3d_content"))
                    .desired_width(f32::INFINITY)
                    .desired_rows(
                        ((ui.available_height().min(ctx.content_rect().height()) - 340.) / 20.).clamp(4., 16.) as usize,
                    )
                    .hint_text("Enter text"),
            );
            if draft.focus {
                edit.request_focus();
                draft.focus = false;
            }

            ui.horizontal(|ui| {
                ui.add_sized([52., 32.], egui::Label::new("Font"));
                crate::widgets::combo(egui::ComboBox::from_id_salt("text3d_font"))
                    .selected_text(match draft.settings.font {
                        Text3dFont::Sans => "Sans",
                        Text3dFont::Serif => "Serif",
                        Text3dFont::Mono => "Mono",
                    })
                    .show_ui(ui, |ui| {
                        for (font, name) in
                            [(Text3dFont::Sans, "Sans"), (Text3dFont::Serif, "Serif"), (Text3dFont::Mono, "Mono")]
                        {
                            ui.selectable_value(&mut draft.settings.font, font, name);
                        }
                    });
                crate::widgets::checkbox(ui, &mut draft.settings.bold, "Bold");
            });
            ui.horizontal(|ui| {
                ui.add_sized([52., 32.], egui::Label::new("Height"));
                ui.add(crate::theme::singleline(&mut draft.height).desired_width(ui.available_width()));
            });
            ui.horizontal(|ui| {
                ui.add_sized([52., 32.], egui::Label::new("Align"));
                for (align, name) in
                    [(Text3dAlign::Left, "Left"), (Text3dAlign::Centre, "Center"), (Text3dAlign::Right, "Right")]
                {
                    ui.selectable_value(&mut draft.settings.align, align, name);
                }
            });
            crate::widgets::checkbox(ui, &mut draft.settings.filled, "Filled");
            ui.add_enabled_ui(draft.settings.filled, |ui| {
                crate::widgets::checkbox(ui, &mut draft.settings.extruded, "Extruded");
                ui.add_enabled_ui(draft.settings.extruded, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_sized([52., 32.], egui::Label::new("Depth"));
                        ui.add(crate::theme::singleline(&mut draft.depth).desired_width(ui.available_width()));
                    });
                });
            });
            ui.small("Height and depth accept model units or a suffix such as cm. Escape cancels.");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                place = ui.button("Place").clicked();
                cancel = ui.button("Cancel").clicked();
            });
            cancel |= ui.input(|i| i.key_pressed(egui::Key::Escape));
            if let Some(message) = &app.message {
                ui.colored_label(egui::Color32::DARK_RED, message);
            }
        });
    if !open || cancel {
        app.text3d = None;
    } else if place {
        app.message = prepare(app).err();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_model::{Editor, Element};
    #[test]
    fn resized_dialog_expands_text_without_changing_document() {
        let mut widths = Vec::new();
        for size in [egui::vec2(360., 440.), egui::vec2(620., 700.)] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut app = App3d::new(Editor::new());
            let before = app.editor.doc().clone();
            begin(&mut app);
            for _ in 0..4 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200., 900.))),
                        ..Default::default()
                    },
                    |ui| panel_sized(ui.ctx(), &mut app, size),
                );
                output.textures_delta.clear();
            }
            let rect = ctx.read_response(egui::Id::unique("text3d_content")).unwrap().rect;
            assert!(rect.is_finite());
            widths.push(rect.width());
            assert_eq!(app.editor.doc(), &before);
        }
        assert!(widths[1] > widths[0] + 200., "text widths: {widths:?}");
    }

    fn draft(app: &mut App3d) {
        begin(app);
        let d = app.text3d.as_mut().unwrap();
        d.content = "Číslo 8".into();
        d.height = "25cm".into();
        d.depth = "2cm".into();
        prepare(app).unwrap();
    }
    #[test]
    fn settings_preview_placement_and_undo_are_atomic() {
        let mut app = App3d::new(Editor::new());
        let before = app.editor.doc().clone();
        draft(&mut app);
        assert_eq!(app.editor.doc(), &before);
        assert_eq!(app.editor.undo_label(), None);
        assert_eq!(app.text3d.as_ref().unwrap().settings.height, 0.25);
        let at = app.view().project(DVec3::new(2., 1., 0.)).unwrap();
        crate::input::tool_pointer(&mut app, at, Pointer::Move);
        let expected = app.text3d.as_ref().unwrap().origin;
        assert_eq!(app.editor.doc(), &before);
        crate::input::tool_pointer(&mut app, at, Pointer::Down { clicks: 1 });
        assert!(app.text3d.is_none());
        assert_eq!(app.editor.undo_label(), Some("3D Text"));
        let Element::Instance(id) = app.editor.selected()[0] else { panic!() };
        let instance = &app.editor.active().instances[&id];
        assert!(instance.transform.transform_point3(DVec3::ZERO).distance(expected) < 1e-9);
        app.editor.undo();
        assert_eq!(app.editor.doc(), &before);
    }
    #[test]
    fn escape_tool_switch_invalid_input_and_revision_cancel_drafts() {
        let mut app = App3d::new(Editor::new());
        let before = app.editor.doc().clone();
        draft(&mut app);
        crate::input::tool_key(&mut app, rysmo_tools::Key::Escape);
        assert!(app.text3d.is_none());
        assert_eq!(app.editor.doc(), &before);
        draft(&mut app);
        app.set_tool(ToolKind::Line);
        assert!(app.text3d.is_none());
        assert_eq!(app.editor.doc(), &before);
        begin(&mut app);
        app.text3d.as_mut().unwrap().content = "H".into();
        app.text3d.as_mut().unwrap().height = "bad".into();
        assert!(prepare(&mut app).is_err());
        assert_eq!(app.editor.doc(), &before);
        draft(&mut app);
        app.editor.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let changed = app.editor.doc().clone();
        pointer(&mut app, DVec2::ZERO, Pointer::Down { clicks: 1 });
        assert!(app.text3d.is_none());
        assert_eq!(app.editor.doc(), &changed);
    }
    #[test]
    fn coarse_display_precision_does_not_round_text_geometry() {
        let mut app = App3d::new(Editor::new());
        app.editor.apply(Command::SetUnits { units: rysmo_model::Units::Feet }).unwrap();
        app.editor.apply(Command::SetLengthPrecision { precision: Some(0) }).unwrap();
        app.text3d_defaults.height = 0.123456789;
        app.text3d_defaults.depth = 0.0123456789;
        let expected = app.text3d_defaults.clone();
        begin(&mut app);
        app.text3d.as_mut().unwrap().content = "H".into();
        prepare(&mut app).unwrap();
        assert!((app.text3d_defaults.height - expected.height).abs() < 1e-12);
        assert!((app.text3d_defaults.depth - expected.depth).abs() < 1e-12);
    }
    #[test]
    fn precise_placement_and_outline_options() {
        let mut app = App3d::new(Editor::new());
        draft(&mut app);
        app.text3d.as_mut().unwrap().origin = DVec3::Y;
        app.vcb = "2m".into();
        crate::input::apply_vcb(&mut app);
        let Element::Instance(id) = app.editor.selected()[0] else { panic!() };
        assert_eq!(app.editor.active().instances[&id].transform.transform_point3(DVec3::ZERO), DVec3::Y * 2.);
        begin(&mut app);
        let d = app.text3d.as_mut().unwrap();
        d.content = "O".into();
        d.settings.filled = false;
        d.depth = "ignored".into();
        prepare(&mut app).unwrap();
        assert_eq!(app.text3d.as_ref().unwrap().mesh.as_ref().unwrap().faces().count(), 0);
        assert!(distance(&mut app, "bad").is_err());
        assert!(app.text3d.is_some());
    }
}
