//! Portable CSV export using the same bounded headless report for all targets.
use crate::state::App3d;
use bevy_egui::egui;
#[derive(Default)]
pub struct State {
    pub open: bool,
    options: rysmo_io::report::Options,
    group_attribute: String,
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.features.report.open {
        return;
    }
    let mut open = true;
    let mut export = false;
    let response = crate::theme::window("Model Report").vscroll(true).beside_model().open(&mut open).default_width(410.).show(ctx, |ui| {
        ui.checkbox(&mut app.features.report.options.visible_only, "Visible occurrences and geometry only");
        ui.checkbox(&mut app.features.report.options.summaries, "Include component definition summaries");
        ui.label("Optional attribute grouping"); ui.add(crate::theme::singleline(&mut app.features.report.group_attribute).hint_text("Attribute name").desired_width(f32::INFINITY));
        ui.label("CSV includes occurrence paths, counts, own edge lengths, face areas and available solid volumes.");
        ui.label("Units: metres, square metres and cubic metres. Parent transforms and component scale are included.");
        ui.small("Nested geometry is listed separately to prevent double counting. Unavailable volumes have an explanation. Summary rows are excluded from the total. Section planes do not trim quantities. Soft edges are included. Single-shell volumes require closed, consistently oriented geometry; use Solid Check for geometric diagnostics.");
        export = ui.button("Export CSV...").clicked();
    });
    crate::features::remember(app, &response);
    app.features.report.open = open;
    if export {
        match rysmo_io::report::build(
            app.editor.doc(),
            rysmo_io::report::Options {
                group_attribute: (!app.features.report.group_attribute.trim().is_empty())
                    .then(|| app.features.report.group_attribute.trim().into()),
                ..app.features.report.options.clone()
            },
        ) {
            Ok(report) => {
                let csv = report.csv();
                crate::features::data_files::save(app, "model-report", "csv", csv.as_bytes(), "text/csv;charset=utf-8");
            }
            Err(error) => app.message = Some(error),
        }
    }
}
