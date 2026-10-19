//! Export for 3D printing: check the model first, show what is wrong and mark it.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_io::print::PrintCheck;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Stl,
    ThreeMf,
}

impl Format {
    fn name(self) -> &'static str {
        match self {
            Format::Stl => "STL",
            Format::ThreeMf => "3MF",
        }
    }
}

pub struct Dialog {
    format: Format,
    report: PrintCheck,
    revision: u64,
    generation: u64,
}

/// Export right away when the model is ready to print, otherwise explain why not.
pub fn begin(app: &mut App3d, format: Format) {
    let report = rysmo_io::print::check(app.editor.doc());
    if report.triangles == 0 {
        app.message = Some("There are no faces to export.".into());
        return;
    }
    if report.is_printable() && report.loose_lines == 0 {
        export(app, format);
        return;
    }
    app.print_check =
        Some(Dialog { format, report, revision: app.editor.revision(), generation: app.document_generation });
}

fn export(app: &mut App3d, format: Format) {
    match format {
        Format::Stl => crate::files::export_stl(app),
        Format::ThreeMf => crate::files::export_3mf(app),
    }
}

fn valid(dialog: &Dialog, app: &App3d) -> bool {
    dialog.revision == app.editor.revision() && dialog.generation == app.document_generation
}

fn lines(report: &PrintCheck) -> Vec<String> {
    let mut lines = Vec::new();
    let count = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
    if !report.open_edges.is_empty() {
        lines.push(format!(
            "{} with a face on one side only: the surface has holes.",
            count(report.open_edges.len(), "edge", "edges")
        ));
    }
    if !report.bad_edges.is_empty() {
        lines.push(format!(
            "{} where faces meet wrongly: a reversed face, a face inside the object, or more than two faces at an edge.",
            count(report.bad_edges.len(), "edge", "edges")
        ));
    }
    if !report.inside_out.is_empty() {
        lines.push(format!("Inside out (Reverse Faces fixes it): {}.", report.inside_out.join(", ")));
    }
    if report.loose_lines > 0 {
        lines.push(format!("{} not printed.", count(report.loose_lines, "loose line is", "loose lines are")));
    }
    lines
}

pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(dialog) = app.print_check.take() else { return };
    if !valid(&dialog, app) {
        return;
    }
    let mut open = true;
    let (mut export_now, mut cancel) = (false, false);
    let title = format!("Export {}", dialog.format.name());
    crate::theme::window(&title).beside_model().open(&mut open).default_width(370.).show(ctx, |ui| {
        let report = &dialog.report;
        if report.is_printable() {
            ui.label("The model is closed and ready to print.");
        } else {
            ui.label("The model is not ready to print. Problem edges are marked in red.");
        }
        for line in lines(report) {
            ui.label(format!("• {line}"));
        }
        ui.separator();
        let s = report.size_mm;
        ui.label(format!("Size: {:.1} × {:.1} × {:.1} mm", s.x, s.y, s.z));
        if report.is_printable() {
            ui.label(format!("Volume: {:.2} cm³", report.volume_mm3 / 1000.));
        }
        ui.small("Exported in millimetres. Groups and components are flattened.");
        ui.horizontal(|ui| {
            let label = if report.is_printable() { "Export" } else { "Export anyway" };
            export_now = ui.button(label).clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if export_now {
        export(app, dialog.format);
    } else if open && !cancel {
        app.print_check = Some(dialog);
    }
}

pub fn draw(painter: &egui::Painter, app: &App3d) {
    let Some(dialog) = app.print_check.as_ref().filter(|d| valid(d, app)) else { return };
    let view = app.view();
    let stroke = egui::Stroke::new(3., egui::Color32::from_rgb(220, 40, 40));
    for [a, b] in dialog.report.open_edges.iter().chain(&dialog.report.bad_edges) {
        if let Some((a, b)) = view.project_segment(*a, *b) {
            painter.line_segment([egui::pos2(a.x as f32, a.y as f32), egui::pos2(b.x as f32, b.y as f32)], stroke);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::Command;

    #[test]
    fn an_open_surface_opens_the_dialog_and_an_edit_closes_it() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        begin(&mut app, Format::Stl);
        let dialog = app.print_check.as_ref().unwrap();
        assert_eq!(dialog.report.open_edges.len(), 4);
        assert!(lines(&dialog.report)[0].starts_with("4 edges with a face on one side only"));
        app.editor.apply(Command::DrawRectangle { corner: DVec3::Z, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        assert!(!valid(app.print_check.as_ref().unwrap(), &app));
    }

    #[test]
    fn an_empty_model_says_so_instead_of_exporting() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        begin(&mut app, Format::ThreeMf);
        assert!(app.print_check.is_none());
        assert_eq!(app.message.as_deref(), Some("There are no faces to export."));
    }
}
