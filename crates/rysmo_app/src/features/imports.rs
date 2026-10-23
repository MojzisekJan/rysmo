use crate::state::App3d;
use bevy_egui::egui;
use rysmo_io::import::{Imported, Options, UpAxis};

pub struct Draft {
    name: String,
    bytes: Vec<u8>,
    options: Options,
    imported: Option<Imported>,
    preview: Option<super::library::Preview>,
    error: Option<String>,
    preserve_origin: bool,
    needs_update: bool,
}
impl Draft {
    pub fn new(name: String, bytes: Vec<u8>) -> Self {
        let mut result = Self {
            name,
            bytes,
            options: Options::default(),
            imported: None,
            preview: None,
            error: None,
            preserve_origin: false,
            needs_update: false,
        };
        result.parse();
        result
    }
    fn parse(&mut self) {
        match rysmo_io::import::read(&self.name, &self.bytes, self.options) {
            Ok(imported) => {
                self.preview = Some(super::library::Preview::new(&imported.document));
                self.imported = Some(imported);
                self.error = None;
            }
            Err(error) => {
                self.imported = None;
                self.preview = None;
                self.error = Some(error.to_string());
            }
        }
        self.needs_update = false;
    }
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(mut draft) = app.features.import.take() else { return };
    let mut open = true;
    let mut place = false;
    let result=crate::theme::window("Import model").open(&mut open).default_width(420.).resizable(true).show(ctx,|ui| {
        ui.label(&draft.name);
        let old=draft.options.unit_scale;
        egui::ComboBox::from_label("Source units").selected_text(match old {None=>"From file / metres",Some(0.001)=>"Millimetres",Some(0.01)=>"Centimetres",Some(0.0254)=>"Inches",Some(0.3048)=>"Feet",_=>"Metres"}).show_ui(ui,|ui| {
            for (label,value) in [("From file / metres",None),("Millimetres",Some(0.001)),("Centimetres",Some(0.01)),("Metres",Some(1.)),("Inches",Some(0.0254)),("Feet",Some(0.3048))] {ui.selectable_value(&mut draft.options.unit_scale,value,label);}
        });
        draft.needs_update|=old!=draft.options.unit_scale;
        if draft.name.to_ascii_lowercase().ends_with(".obj") {
            let old=draft.options.obj_up;
            ui.horizontal(|ui|{ui.label("Source up axis");ui.selectable_value(&mut draft.options.obj_up,UpAxis::Y,"Y");ui.selectable_value(&mut draft.options.obj_up,UpAxis::Z,"Z");});
            draft.needs_update|=old!=draft.options.obj_up;
        }
        if draft.name.to_ascii_lowercase().ends_with(".dxf") {draft.needs_update|=ui.add(crate::widgets::Number::new(&mut draft.options.curve_segments).range(8..=256).prefix("Circle segments ")).changed();}
        crate::widgets::checkbox(ui,&mut draft.preserve_origin,"Keep source origin");
        // Reparse once a control settles; dragging a value would otherwise parse every frame.
        if draft.needs_update && !ui.ctx().egui_is_using_pointer() {draft.parse();}
        if let Some(error)=&draft.error {ui.colored_label(egui::Color32::LIGHT_RED,error);}
        if let Some(imported)=&draft.imported {
            if let Some(preview)=&draft.preview {
                super::library::thumbnail(ui,preview);
                ui.label(format!("{} · {}",count(preview.occurrences,"occurrence"),count(preview.faces,"face")));
            }
            for warning in &imported.warnings {ui.colored_label(egui::Color32::from_rgb(230,180,80),warning);}
            ui.weak("Import creates a component. Click in the model to place it; Escape cancels. One Undo removes the import.");
            place=ui.add_enabled(!draft.needs_update,egui::Button::new("Place in model")).clicked();
        }
    });
    super::remember(app, &result);
    if place {
        if let Some(imported) = draft.imported.take()
            && let Err(error) =
                crate::object_clipboard::place_external(app, imported.document, draft.name, draft.preserve_origin)
        {
            app.message = Some(error);
        }
    } else if open {
        app.features.import = Some(draft);
    }
}
fn count(n: usize, noun: &str) -> String {
    if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") }
}
