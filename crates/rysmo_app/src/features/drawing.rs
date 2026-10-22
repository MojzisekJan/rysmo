use crate::state::App3d;
use bevy_egui::egui;
use rysmo_io::drawing::{Options, Orientation};
#[derive(Default)]
pub struct State {
    pub open: bool,
    options: Options,
    error: Option<String>,
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    if !app.features.drawing.open {
        return;
    }
    let mut state = std::mem::take(&mut app.features.drawing);
    let mut export = None;
    let result=crate::theme::window("Export drawing").open(&mut state.open).default_width(420.).resizable(true).show(ctx,|ui| {
        ui.label("Title");ui.text_edit_singleline(&mut state.options.title);
        egui::ComboBox::from_label("View").selected_text(state.options.orientation.name()).show_ui(ui,|ui| {
            for view in [Orientation::Top,Orientation::Front,Orientation::Right,Orientation::Isometric] {ui.selectable_value(&mut state.options.orientation,view,view.name());}
        });
        ui.horizontal_wrapped(|ui| {
            for (label,page) in [("A4 landscape",[297.,210.]),("A4 portrait",[210.,297.]),("A3 landscape",[420.,297.]),("A3 portrait",[297.,420.])] {ui.selectable_value(&mut state.options.page_mm,page,label);}
        });
        ui.add(crate::widgets::Number::new(&mut state.options.scale).range(0.001..=1e6).prefix("Scale 1:").speed(1.));
        ui.add(crate::widgets::Number::new(&mut state.options.margin_mm).range(0. ..=100.).suffix(" mm").prefix("Margin "));
        crate::widgets::checkbox(ui,&mut state.options.hidden_lines,"Remove hidden lines");
        crate::widgets::checkbox(ui,&mut state.options.dimensions,"Include dimensions");
        crate::widgets::checkbox(ui,&mut state.options.annotations,"Include attached text");
        ui.weak("Active sections and visibility are respected. Output uses vector linework and physical millimetres. Print at 100% to preserve scale. Screen-fixed notes stay outside the drawing.");
        if let Some(error)=&state.error {ui.colored_label(egui::Color32::LIGHT_RED,error);}
        ui.horizontal(|ui|{if ui.button("Export SVG...").clicked(){export=Some(false);}
if ui.button("Export PDF...").clicked(){export=Some(true);}});
    });
    super::remember(app, &result);
    if let Some(pdf) = export {
        match rysmo_io::drawing::build(app.editor.doc(), &state.options) {
            Ok(drawing) => {
                state.error = None;
                if pdf {
                    super::data_files::save(app, &state.options.title, "pdf", &drawing.pdf(), "application/pdf");
                } else {
                    super::data_files::save(
                        app,
                        &state.options.title,
                        "svg",
                        drawing.svg().as_bytes(),
                        "image/svg+xml",
                    );
                }
            }
            Err(error) => state.error = Some(error),
        }
    }
    app.features.drawing = state;
}
