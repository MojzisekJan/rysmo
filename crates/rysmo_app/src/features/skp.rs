//! Preview interchange warnings before downloading or saving an SKP snapshot.
// TODO: Finish browser GUI checks for import into a new tab and downloads of both
// export versions, including warped textures. Record results in docs/verification.
// TODO: Move browser SKP encoding off the main thread or make it cooperative;
// desktop worker support does not prevent browser stalls on large models.
use crate::state::App3d;
use bevy_egui::egui;
use rysmo_io::skp::{Export, Version};
type Stamp = (u64, u64, rysmo_math::OrbitCamera);
fn stamp(app: &App3d) -> Stamp {
    (app.document_generation, app.editor.content_revision(), app.camera)
}
struct Prepared {
    stamp: Stamp,
    version: Version,
    result: Export,
}
#[cfg(not(target_arch = "wasm32"))]
struct Completed {
    stamp: Stamp,
    version: Version,
    result: Result<Export, String>,
}
#[derive(Default)]
pub struct State {
    pub open: bool,
    version: Version,
    prepared: Option<Prepared>,
    error: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    pending: Option<std::sync::Mutex<std::sync::mpsc::Receiver<Completed>>>,
}
impl State {
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
    fn prepare(&mut self, app: &App3d) {
        let stamp = stamp(app);
        let mut doc = app.editor.doc().clone();
        doc.camera = Some(app.camera);
        self.prepared = None;
        self.error = None;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let version = self.version;
            let (tx, rx) = std::sync::mpsc::channel();
            match std::thread::Builder::new().name("skp-export".into()).spawn(move || {
                let result = rysmo_io::skp::export(&doc, version);
                let _ = tx.send(Completed { stamp, version, result });
            }) {
                Ok(_) => self.pending = Some(std::sync::Mutex::new(rx)),
                Err(error) => self.error = Some(format!("Cannot start SKP export: {error}")),
            }
        }
        #[cfg(target_arch = "wasm32")]
        match rysmo_io::skp::export(&doc, self.version) {
            Ok(result) => self.prepared = Some(Prepared { stamp, version: self.version, result }),
            Err(error) => self.error = Some(error),
        }
    }
    fn poll(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(receiver) = &self.pending {
            let completion = receiver.lock().map_err(|_| "SKP export receiver failed.".to_owned()).and_then(
                |receiver| match receiver.try_recv() {
                    Ok(value) => Ok(Some(value)),
                    Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        Err("SKP export worker stopped unexpectedly.".into())
                    }
                },
            );
            match completion {
                Ok(None) => {}
                Ok(Some(value)) => {
                    self.pending = None;
                    match value.result {
                        Ok(result) => {
                            self.prepared = Some(Prepared { stamp: value.stamp, version: value.version, result })
                        }
                        Err(error) => self.error = Some(error),
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
    let mut state = std::mem::take(&mut app.features.skp);
    state.poll();
    if !state.open {
        app.features.skp = state;
        return;
    }
    let stamp = stamp(app);
    let mut open = true;
    let mut prepare = false;
    let mut save = false;
    let busy = state.busy();
    let result = crate::theme::window("Export SKP")
        .open(&mut open)
        .default_width(460.)
        .max_height((ctx.content_rect().height() - 100.).max(180.))
        .vscroll(true)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label("File version");
            ui.add_enabled_ui(!busy, |ui| {
                ui.radio_value(&mut state.version, Version::V2017, "2017 compatibility");
                ui.radio_value(&mut state.version, Version::V2026, "2026 container");
            });
            ui.weak("Export writes a separate file. Continue saving editable Rysmo projects as .rysmo.");
            if let Some(error) = &state.error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            if busy {
                ui.spinner();
                ui.label("Preparing export…");
            }
            if let Some(prepared) = &state.prepared {
                for warning in &prepared.result.warnings {
                    ui.colored_label(egui::Color32::from_rgb(230, 180, 80), warning);
                }
                if prepared.stamp != stamp || prepared.version != state.version {
                    ui.weak("The model or file version changed. Prepare the export again.");
                } else {
                    ui.label(format!("{} bytes ready", prepared.result.bytes.len()));
                    save = ui.button("Save SKP…").clicked();
                }
            }
            prepare = ui.add_enabled(!busy, egui::Button::new("Prepare export")).clicked();
        });
    super::remember(app, &result);
    if prepare {
        state.prepare(app);
    }
    if save
        && let Some(prepared) = &state.prepared
        && prepared.stamp == stamp
        && prepared.version == state.version
    {
        let name =
            app.path.as_ref().and_then(|p| p.file_stem()).and_then(|s| s.to_str()).unwrap_or("Untitled").to_owned();
        super::data_files::save(app, &name, "skp", &prepared.result.bytes, "application/octet-stream");
    }
    if !open {
        state = State { version: state.version, ..Default::default() };
    }
    state.open = open;
    app.features.skp = state;
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn export_captures_live_camera_and_detects_subsequent_navigation_without_marking_saved() {
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.camera.target = rysmo_math::DVec3::new(7., 8., 9.);
        app.camera.distance = 17.;
        app.camera.roll = 0.2;
        assert!(app.editor.doc().camera.is_none());
        let mut state = State::default();
        state.prepare(&app);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while state.busy() {
            state.poll();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let prepared = state.prepared.as_ref().unwrap();
        let loaded = rysmo_io::skp::read(&prepared.result.bytes, Default::default()).unwrap().document;
        let camera = loaded.camera.unwrap();
        assert!(camera.eye().distance(app.camera.eye()) < 1e-9);
        assert!(camera.target.distance(app.camera.target) < 1e-9);
        assert!(camera.up().distance(app.camera.up()) < 1e-9);
        assert_eq!(prepared.stamp, stamp(&app));
        app.camera.distance += 1.;
        assert_ne!(prepared.stamp, stamp(&app));
        assert!(app.editor.doc().camera.is_none());
        assert!(!app.editor.is_dirty());
        assert!(app.path.is_none());
    }
}
