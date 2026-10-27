//! Paper-space layouts keep several scaled views linked to the current model.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_model::{
    Command,
    sheets::{Note, Projection, Sheet, Viewport},
};
#[derive(Default)]
pub struct State {
    pub open: bool,
    generation: Option<u64>,
    base: Vec<Sheet>,
    draft: Vec<Sheet>,
    selected: usize,
    error: Option<String>,
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.features.sheets.open {
        return;
    }
    let mut state = std::mem::take(&mut app.features.sheets);
    if state.generation != Some(app.document_generation) || state.base != app.editor.doc().sheets {
        state.generation = Some(app.document_generation);
        state.base = app.editor.doc().sheets.clone();
        state.draft = state.base.clone();
        state.selected = state.selected.min(state.draft.len().saturating_sub(1));
        state.error = None;
    }
    let mut apply = false;
    let mut pdf = false;
    let mut svg = false;
    let window =
        crate::theme::window("Drawing Sheets").vscroll(true).open(&mut state.open).default_size(egui::vec2(620., 600.));
    let response = window.show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(state.draft.len() < 32, egui::Button::new("Add sheet")).clicked() {
                state.draft.push(Sheet { name: format!("Sheet {}", state.draft.len() + 1), ..Default::default() });
                state.selected = state.draft.len() - 1;
            }
            if ui.add_enabled(!state.draft.is_empty(), egui::Button::new("Delete sheet")).clicked() {
                state.draft.remove(state.selected);
                state.selected = state.selected.min(state.draft.len().saturating_sub(1));
            }
        });
        let name = state.draft.get(state.selected).map(|s| s.name.as_str()).unwrap_or("No sheets");
        egui::ComboBox::from_label("Sheet").selected_text(name).show_ui(ui, |ui| {
            for (index, sheet) in state.draft.iter().enumerate() {
                ui.selectable_value(&mut state.selected, index, &sheet.name);
            }
        });
        if let Some(sheet) = state.draft.get_mut(state.selected) {
            edit_sheet(ui, sheet, app.editor.doc());
        }
        let validation = rysmo_model::sheets::validate(&state.draft);
        if let Err(error) = &validation {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        ui.horizontal_wrapped(|ui| {
            apply = ui.add_enabled(validation.is_ok(), egui::Button::new("Apply layouts")).clicked();
            let valid = validation.is_ok() && !state.draft.is_empty();
            pdf = ui.add_enabled(valid, egui::Button::new("Export all sheets PDF...")).clicked();
            svg = ui.add_enabled(valid, egui::Button::new("Export selected SVG...")).clicked();
        });
        ui.small("Exports regenerate views from the model. Geometry must fit at the stated scale. Print at 100%.");
        ui.small("Preview shows page layout. Export evaluates vector geometry.");
        if let Some(error) = &state.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    });
    super::remember(app, &response);
    if apply {
        match app.editor.apply(Command::SetSheets { sheets: state.draft.clone() }) {
            Ok(_) => {
                state.base = app.editor.doc().sheets.clone();
                state.error = None;
            }
            Err(e) => state.error = Some(e.to_string()),
        }
    }
    if pdf {
        match rysmo_io::sheets::pdf(app.editor.doc(), &state.draft) {
            Ok(bytes) => {
                super::data_files::save(app, "drawing-sheets", "pdf", &bytes, "application/pdf");
                state.error = None;
            }
            Err(e) => state.error = Some(e),
        }
    }
    if svg && let Some(sheet) = state.draft.get(state.selected) {
        match rysmo_io::sheets::build(app.editor.doc(), sheet) {
            Ok(drawing) => {
                super::data_files::save(app, &sheet.name, "svg", drawing.svg().as_bytes(), "image/svg+xml");
                state.error = None;
            }
            Err(e) => state.error = Some(e),
        }
    }
    app.features.sheets = state;
}

fn edit_sheet(ui: &mut egui::Ui, sheet: &mut Sheet, doc: &rysmo_model::Document) {
    ui.label("Sheet name");
    ui.add(crate::theme::singleline(&mut sheet.name).desired_width(f32::INFINITY));
    ui.horizontal_wrapped(|ui| {
        for (name, size) in
            [("A4 landscape", [297., 210.]), ("A4 portrait", [210., 297.]), ("A3 landscape", [420., 297.])]
        {
            ui.selectable_value(&mut sheet.page_mm, size, name);
        }
    });
    ui.label("Title block");
    ui.add(egui::TextEdit::multiline(&mut sheet.title_block).desired_width(f32::INFINITY).desired_rows(2));
    let mut remove = None;
    for (index, view) in sheet.viewports.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.collapsing(format!("Viewport {}: {}", index + 1, view.name), |ui| {
                edit_viewport(ui, view, doc);
                if ui.button("Remove viewport").clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        sheet.viewports.remove(index);
    }
    if ui.add_enabled(sheet.viewports.len() < 16, egui::Button::new("Add viewport")).clicked() {
        sheet.viewports.push(Viewport::default());
    }
    ui.collapsing("Paper notes", |ui| edit_notes(ui, &mut sheet.notes));
    let width = ui.available_width().min(500.);
    let scale = width / sheet.page_mm[0] as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, sheet.page_mm[1] as f32 * scale), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2., egui::Color32::from_gray(240));
    for view in &sheet.viewports {
        let [x, y, w, h] = view.rect_mm;
        let area = egui::Rect::from_min_size(
            rect.min + egui::vec2(x as f32 * scale, y as f32 * scale),
            egui::vec2(w as f32 * scale, h as f32 * scale),
        );
        ui.painter().rect_stroke(area, 0., egui::Stroke::new(1., egui::Color32::DARK_GRAY), egui::StrokeKind::Inside);
        ui.painter().text(
            area.center(),
            egui::Align2::CENTER_CENTER,
            format!("{}\n1:{}", view.name, view.scale),
            egui::FontId::proportional(12.),
            egui::Color32::BLACK,
        );
    }
}
fn edit_viewport(ui: &mut egui::Ui, view: &mut Viewport, doc: &rysmo_model::Document) {
    ui.add(crate::theme::singleline(&mut view.name).desired_width(f32::INFINITY));
    egui::ComboBox::from_label("Projection").selected_text(format!("{:?}", view.projection)).show_ui(ui, |ui| {
        for projection in [Projection::Top, Projection::Front, Projection::Right, Projection::Isometric] {
            ui.selectable_value(&mut view.projection, projection, format!("{projection:?}"));
        }
    });
    let scene = view.scene.and_then(|id| doc.scenes.iter().find(|scene| scene.id == id));
    let label =
        scene.map(|s| s.name.as_str()).unwrap_or(if view.scene.is_some() { "Missing scene" } else { "Current model" });
    egui::ComboBox::from_label("Visibility and sections").selected_text(label).show_ui(ui, |ui| {
        ui.selectable_value(&mut view.scene, None, "Current model");
        for scene in &doc.scenes {
            ui.selectable_value(&mut view.scene, Some(scene.id), &scene.name);
        }
    });
    ui.horizontal_wrapped(|ui| {
        for (label, value) in ["X", "Y", "Width", "Height"].into_iter().zip(&mut view.rect_mm) {
            ui.add(crate::widgets::Number::new(value).range(0. ..=2000.).prefix(format!("{label} ")).suffix(" mm"));
        }
    });
    ui.add(crate::widgets::Number::new(&mut view.scale).range(0.001..=1e6).prefix("Scale 1:"));
    ui.checkbox(&mut view.hidden_lines, "Remove hidden lines");
    ui.checkbox(&mut view.dimensions, "Model dimensions");
    ui.checkbox(&mut view.annotations, "Model annotations");
}
fn edit_notes(ui: &mut egui::Ui, notes: &mut Vec<Note>) {
    let mut remove = None;
    for (index, note) in notes.iter_mut().enumerate() {
        ui.push_id(index, |ui| {
            ui.add(egui::TextEdit::multiline(&mut note.text).desired_width(f32::INFINITY).desired_rows(2));
            ui.horizontal_wrapped(|ui| {
                ui.add(crate::widgets::Number::new(&mut note.position_mm[0]).prefix("X ").suffix(" mm"));
                ui.add(crate::widgets::Number::new(&mut note.position_mm[1]).prefix("Y ").suffix(" mm"));
                ui.add(crate::widgets::Number::new(&mut note.size_mm).range(1. ..=20.).prefix("Text ").suffix(" mm"));
                if ui.button("Remove").clicked() {
                    remove = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove {
        notes.remove(index);
    }
    if ui.add_enabled(notes.len() < 64, egui::Button::new("Add note")).clicked() {
        notes.push(Note { text: "Note".into(), position_mm: [15., 20.], size_mm: 3. });
    }
}
