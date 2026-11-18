use crate::state::App3d;
use bevy_egui::egui;
use rysmo_io::import::{Imported, Options, UpAxis};

pub struct Draft {
    name: String,
    bytes: std::sync::Arc<Vec<u8>>,
    options: Options,
    imported: Option<Imported>,
    preview: Option<super::library::Preview>,
    error: Option<String>,
    preserve_origin: bool,
    needs_update: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<Pending>,
}
#[cfg(not(target_arch = "wasm32"))]
struct Pending {
    options: Options,
    result: std::sync::Mutex<std::sync::mpsc::Receiver<Result<Imported, rysmo_io::import::ImportError>>>,
}
impl Draft {
    pub fn new(name: String, bytes: Vec<u8>) -> Self {
        let mut result = Self {
            name,
            bytes: std::sync::Arc::new(bytes),
            options: Options::default(),
            imported: None,
            preview: None,
            error: None,
            preserve_origin: false,
            needs_update: false,
            #[cfg(not(target_arch = "wasm32"))]
            pending: None,
        };
        result.parse();
        result
    }
    pub fn busy(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending.is_some()
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }
    fn accept(&mut self, result: Result<Imported, rysmo_io::import::ImportError>) {
        match result {
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
    }
    fn parse(&mut self) {
        if self.busy() {
            self.needs_update = true;
            return;
        }
        self.needs_update = false;
        self.imported = None;
        self.preview = None;
        self.error = None;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            let name = self.name.clone();
            let bytes = self.bytes.clone();
            let options = self.options;
            match std::thread::Builder::new().name("model-import".into()).spawn(move || {
                let _ = tx.send(rysmo_io::import::read(&name, &bytes, options));
            }) {
                Ok(_) => self.pending = Some(Pending { options, result: std::sync::Mutex::new(rx) }),
                Err(error) => self.error = Some(format!("Cannot start import: {error}")),
            }
        }
        // TODO: Move browser parsing to a worker or a cooperative parser so large
        // model imports do not block input. Keep stale-option result guards.
        #[cfg(target_arch = "wasm32")]
        self.accept(rysmo_io::import::read(&self.name, &self.bytes, self.options));
    }
    fn poll(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(pending) = &self.pending {
            let options = pending.options;
            let result = pending.result.lock().map_err(|_| "Import receiver failed.".to_owned()).and_then(|receiver| {
                match receiver.try_recv() {
                    Ok(value) => Ok(Some(value)),
                    Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        Err("Import worker stopped unexpectedly.".into())
                    }
                }
            });
            match result {
                Ok(None) => {}
                Ok(Some(value)) => {
                    self.pending = None;
                    if self.options != options {
                        self.parse();
                    } else {
                        self.needs_update = false;
                        self.accept(value);
                    }
                }
                Err(error) => {
                    self.pending = None;
                    self.error = Some(error);
                }
            }
        }
    }
}
pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(mut draft) = app.features.import.take() else { return };
    draft.poll();
    let mut open = true;
    let mut place = false;
    let mut open_document = false;
    let result=crate::theme::window("Import model").open(&mut open).default_width(420.).resizable(true).max_height((ctx.content_rect().height()-100.).max(180.)).vscroll(true).show(ctx,|ui| {
        ui.label(&draft.name);
        if draft.busy() {ui.spinner();ui.label("Reading model…");}
        if draft.name.to_ascii_lowercase().ends_with(".skp") {
            ui.weak("SKP stores physical dimensions. Source display units are retained when opened as a document.");
        } else {
        let old=draft.options.unit_scale;
        egui::ComboBox::from_label("Source units").selected_text(match old {None=>"From file / metres",Some(0.001)=>"Millimetres",Some(0.01)=>"Centimetres",Some(0.0254)=>"Inches",Some(0.3048)=>"Feet",_=>"Metres"}).show_ui(ui,|ui| {
            for (label,value) in [("From file / metres",None),("Millimetres",Some(0.001)),("Centimetres",Some(0.01)),("Metres",Some(1.)),("Inches",Some(0.0254)),("Feet",Some(0.3048))] {ui.selectable_value(&mut draft.options.unit_scale,value,label);}
        });
        draft.needs_update|=old!=draft.options.unit_scale;
        }
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
            open_document=ui.add_enabled(!draft.needs_update,egui::Button::new("Open as new document")).clicked();
            ui.weak("Opening a new document keeps source cameras, units and scenes. The current document stays in its own tab.");
        }
    });
    super::remember(app, &result);
    // Escape cancels the preview just as it cancels the placement that follows.
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.egui_wants_keyboard_input() {
        open = false;
    }
    if open_document {
        if let Some(imported) = draft.imported.take()
            && let Err(error) = crate::files::import_document(app, imported.document, &draft.name)
        {
            app.message = Some(error);
        }
    } else if place {
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod background_tests {
    use super::*;
    #[test]
    fn changed_options_replace_an_obsolete_parse_without_accepting_its_geometry() {
        let mut draft = Draft::new("triangle.obj".into(), b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".to_vec());
        draft.options.unit_scale = Some(0.001);
        draft.parse();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while draft.busy() {
            draft.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let imported = draft.imported.as_ref().unwrap();
        let (_, hi) = imported.document.definition_bounds(None).unwrap();
        assert!((hi.max_element() - 0.001).abs() < 1e-12);
        assert!(!draft.needs_update);
        assert!(draft.error.is_none());
    }
    #[test]
    fn invalid_background_import_reports_error_without_a_preview() {
        let mut draft = Draft::new("bad.skp".into(), vec![1, 2, 3]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while draft.busy() {
            draft.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(draft.error.is_some());
        assert!(draft.preview.is_none());
        assert!(draft.imported.is_none());
    }
}
