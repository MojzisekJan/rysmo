//! Desktop model I/O. One bounded worker queue serializes atomic file writes.
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU8, Ordering},
    mpsc,
};

use bevy::prelude::*;
use bevy_egui::egui;
use rysmo_math::OrbitCamera;
use rysmo_model::{Document, Editor, SavePoint};

use crate::state::App3d;

const QUEUED: u8 = 0;
const READING: u8 = 1;
const PARSING: u8 = 2;
const PREPARING: u8 = 3;
const SERIALIZING: u8 = 4;
const WRITING: u8 = 5;

static WORKER: Mutex<Option<mpsc::SyncSender<Request>>> = Mutex::new(None);

struct Loaded {
    document: Document,
    camera: OrbitCamera,
}

enum Work {
    Open,
    Save(Box<Document>),
}
struct Request {
    work: Work,
    path: PathBuf,
    stage: Arc<AtomicU8>,
    cancelled: Arc<AtomicBool>,
    reply: mpsc::Sender<Result<Option<Loaded>, String>>,
}
struct Pending {
    generation: u64,
    path: PathBuf,
    saved: Option<SavePoint>,
    stage: Arc<AtomicU8>,
    cancelled: Arc<AtomicBool>,
    reply: Mutex<mpsc::Receiver<Result<Option<Loaded>, String>>>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        // Closing/cancelling an opening must never install its late result.
        if self.saved.is_none() {
            self.cancelled.store(true, Ordering::Relaxed);
        }
    }
}

#[derive(Default)]
pub struct State {
    pending: Option<Pending>,
}
struct Completed {
    pending: Pending,
    result: Result<Option<Loaded>, String>,
}

impl State {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn saving(&self) -> bool {
        self.pending.as_ref().is_some_and(|pending| pending.saved.is_some())
    }
    fn status(&self) -> Option<&'static str> {
        let pending = self.pending.as_ref()?;
        Some(match pending.stage.load(Ordering::Relaxed) {
            READING => "Reading model...",
            PARSING => "Parsing model...",
            PREPARING => "Preparing view...",
            SERIALIZING => "Preparing save...",
            WRITING => "Writing model...",
            _ => "File operation queued...",
        })
    }
    fn take(&mut self) -> Option<Completed> {
        let received = match self.pending.as_ref()?.reply.lock() {
            Ok(receiver) => receiver.try_recv(),
            Err(_) => Err(mpsc::TryRecvError::Disconnected),
        };
        let result = match received {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err("The model file worker stopped unexpectedly.".into()),
        };
        Some(Completed { pending: self.pending.take()?, result })
    }
}

fn process(request: &Request) -> Result<Option<Loaded>, String> {
    if request.cancelled.load(Ordering::Relaxed) {
        return Ok(None);
    }
    match &request.work {
        Work::Open => {
            request.stage.store(READING, Ordering::Relaxed);
            let text = std::fs::read_to_string(&request.path).map_err(|error| error.to_string())?;
            if request.cancelled.load(Ordering::Relaxed) {
                return Ok(None);
            }
            request.stage.store(PARSING, Ordering::Relaxed);
            let document = rysmo_io::load(&text).map_err(|error| error.to_string())?;
            if request.cancelled.load(Ordering::Relaxed) {
                return Ok(None);
            }
            request.stage.store(PREPARING, Ordering::Relaxed);
            let mut camera = document.camera.unwrap_or_default();
            if document.camera.is_none()
                && let Some((low, high)) = document.definition_bounds(None)
            {
                camera.frame(low, high);
            }
            Ok(Some(Loaded { document, camera }))
        }
        Work::Save(document) => {
            request.stage.store(SERIALIZING, Ordering::Relaxed);
            let text = rysmo_io::save(document);
            request.stage.store(WRITING, Ordering::Relaxed);
            rysmo_io::atomic::write(&request.path, text.as_bytes()).map_err(|error| error.to_string())?;
            Ok(None)
        }
    }
}

fn enqueue(mut request: Request) -> Result<(), String> {
    let mut worker = WORKER.lock().map_err(|_| "The file worker lock is unavailable.".to_owned())?;
    if let Some(sender) = &*worker {
        match sender.try_send(request) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(_)) => {
                return Err("The file queue is full. Try again when an operation finishes.".into());
            }
            Err(mpsc::TrySendError::Disconnected(old)) => request = old,
        }
    }
    let (sender, receiver) = mpsc::sync_channel::<Request>(16);
    std::thread::Builder::new()
        .name("rysmo-model-files".into())
        .spawn(move || {
            for request in receiver {
                let result = process(&request);
                if !request.cancelled.load(Ordering::Relaxed) {
                    let _ = request.reply.send(result);
                }
            }
        })
        .map_err(|error| format!("Cannot start the file worker: {error}"))?;
    sender.try_send(request).map_err(|error| format!("Cannot queue the file operation: {error}"))?;
    *worker = Some(sender);
    Ok(())
}

fn begin(app: &mut App3d, path: PathBuf, work: Work, saved: Option<SavePoint>) -> Result<(), String> {
    if app.native_files.pending.is_some() {
        return Err("A file operation is already in progress in this tab. Wait or cancel the opening first.".into());
    }
    let (sender, reply) = mpsc::channel();
    let stage = Arc::new(AtomicU8::new(QUEUED));
    let cancelled = Arc::new(AtomicBool::new(false));
    enqueue(Request { work, path: path.clone(), stage: stage.clone(), cancelled: cancelled.clone(), reply: sender })?;
    app.native_files.pending =
        Some(Pending { generation: app.document_generation, path, saved, stage, cancelled, reply: Mutex::new(reply) });
    Ok(())
}

pub fn open(app: &mut App3d, path: PathBuf) {
    if app.documents.full() {
        app.message = Some("Close a document before opening more than 16 models.".into());
        return;
    }
    app.message = Some(
        begin(app, path, Work::Open, None)
            .map(|()| "Opening model in background.".into())
            .unwrap_or_else(|error| error),
    );
}

pub fn save(app: &mut App3d, path: PathBuf) {
    if app.native_files.pending.is_some() {
        app.message = Some("A file operation is already in progress. Save again when it finishes.".into());
        return;
    }
    crate::texture_view::finish(app, false);
    app.editor.set_camera(app.camera);
    let saved = app.editor.save_point();
    let document = app.editor.doc().clone();
    app.message = Some(
        begin(app, path, Work::Save(Box::new(document)), Some(saved))
            .map(|()| "Saving model in background.".into())
            .unwrap_or_else(|error| error),
    );
}

pub fn saving(app: &App3d) -> bool {
    app.documents.sessions(app).any(|(_, session)| session.native_files.saving())
}

fn complete(app: &mut App3d, completed: Completed) {
    let pending = completed.pending;
    let generation = pending.generation;
    let name = pending.path.to_string_lossy().into_owned();
    match completed.result {
        Err(error) => {
            let message = format!("Cannot {} {name}: {error}", if pending.saved.is_some() { "save" } else { "open" });
            if let Some(session) = app.documents.session_mut(generation) {
                session.message = Some(message.clone());
            }
            app.message = Some(message);
        }
        Ok(_) if pending.cancelled.load(Ordering::Relaxed) => {}
        Ok(Some(loaded)) => {
            let origin_active = app.document_generation == generation;
            if !origin_active && app.documents.session_mut(generation).is_none() {
                return;
            }
            // A completed read must not move an unsaved-changes prompt into an
            // inactive tab or replace a pending close decision.
            let active = origin_active && !app.unsaved.active() && !app.unsaved.close_ready;
            if active {
                let result = crate::files::install_loaded(app, loaded.document, loaded.camera, &name);
                if result.is_ok() {
                    app.path = Some(pending.path.clone());
                }
                app.message = Some(result.unwrap_or_else(|error| error));
            } else {
                let mut session = App3d::new(Editor::with_document(loaded.document));
                session.camera = loaded.camera;
                session.path = Some(pending.path.clone());
                session.welcome = false;
                session.message = Some(format!("Opened {name}."));
                if app.documents.insert_background(session) {
                    app.preferences.remember(&name);
                    app.recent = app.preferences.recent.clone();
                    app.message = Some(format!("Opened {name} in a background tab."));
                } else {
                    app.message = Some("Cannot open model: document limit reached.".into());
                }
            }
        }
        Ok(None) => {
            let Some(point) = &pending.saved else { return };
            let session = if app.document_generation == generation {
                Some(&mut *app)
            } else {
                app.documents.session_mut(generation)
            };
            let accepted = session.is_some_and(|session| {
                if !session.editor.mark_saved_point(point) {
                    return false;
                }
                session.path = Some(pending.path.clone());
                session.message = Some(format!("Saved {name}."));
                true
            });
            app.preferences.remember(&name);
            app.recent = app.preferences.recent.clone();
            app.message =
                Some(if accepted { format!("Saved {name}.") } else { format!("Saved previous model to {name}.") });
        }
    }
}

pub fn poll(mut app: ResMut<App3d>) {
    // Idle frames must not mark the application resource changed just for polling.
    if !app.documents.sessions(&app).any(|(_, session)| session.native_files.busy()) {
        return;
    }
    poll_app(&mut app);
}
pub(crate) fn poll_app(app: &mut App3d) {
    let mut completed = Vec::new();
    if let Some(event) = app.native_files.take() {
        app.message = None;
        completed.push(event);
    }
    for session in app.documents.inactive_mut() {
        if let Some(event) = session.native_files.take() {
            session.message = None;
            completed.push(event);
        }
    }
    for event in completed {
        complete(app, event);
    }
}

pub fn indicator(ui: &mut egui::Ui, app: &mut App3d) {
    let operation = app.documents.sessions(app).find_map(|(_, session)| {
        session
            .native_files
            .status()
            .map(|status| (session.document_generation, status, !session.native_files.saving()))
    });
    let Some((generation, status, opening)) = operation else { return };
    ui.add(egui::Spinner::new().size(14.)).on_hover_text(status);
    if opening && ui.button("Cancel opening").clicked() {
        let session =
            if app.document_generation == generation { Some(&mut *app) } else { app.documents.session_mut(generation) };
        if let Some(session) = session {
            session.native_files.pending = None;
        }
        app.message = Some("Opening cancelled.".into());
    }
    ui.add(egui::Label::new(status).truncate()).on_hover_text(status);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::DVec3;
    use rysmo_model::Command;
    use std::time::{Duration, Instant};

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rysmo-worker-{}-{}",
                std::process::id(),
                crate::documents::generation()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn model(&self) -> PathBuf {
            self.0.join("model.rysmo")
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn app() -> App3d {
        App3d::new(Editor::new())
    }
    fn draw(app: &mut App3d, y: f64) {
        app.editor
            .apply(Command::DrawLines { lines: vec![[DVec3::Y * y, DVec3::X + DVec3::Y * y]], facing: None })
            .unwrap();
    }
    fn wait(app: &mut App3d) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.documents.sessions(app).any(|(_, session)| session.native_files.busy()) {
            assert!(Instant::now() < deadline, "file task did not complete");
            poll_app(app);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn read(path: &std::path::Path) -> Document {
        rysmo_io::load(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn save_writes_the_captured_model_and_undo_finds_the_saved_state() {
        let directory = Directory::new();
        let mut app = app();
        draw(&mut app, 0.);
        save(&mut app, directory.model());
        draw(&mut app, 1.);
        wait(&mut app);
        assert_eq!(read(&directory.model()).root.mesh.edge_count(), 1);
        assert_eq!(app.editor.mesh().edge_count(), 2);
        assert!(app.editor.is_dirty());
        assert!(app.editor.undo());
        assert!(!app.editor.is_dirty());
        assert!(app.editor.redo());
        assert!(app.editor.is_dirty());
    }

    #[test]
    fn save_completion_updates_its_background_tab_not_the_active_document() {
        let directory = Directory::new();
        let mut app = app();
        draw(&mut app, 0.);
        let generation = app.document_generation;
        save(&mut app, directory.model());
        assert!(crate::documents::create(&mut app));
        draw(&mut app, 2.);
        wait(&mut app);
        assert!(app.editor.is_dirty());
        assert!(app.path.is_none());
        let saved = app.documents.session_mut(generation).unwrap();
        assert!(!saved.editor.is_dirty());
        assert_eq!(saved.path, Some(directory.model()));
    }

    #[test]
    fn replacement_editor_cannot_accept_an_older_save_even_with_repeated_state_ids() {
        let directory = Directory::new();
        let mut app = app();
        save(&mut app, directory.model());
        app.editor.replace_document(Document::new());
        app.editor.mark_dirty();
        wait(&mut app);
        assert!(app.editor.is_dirty());
        assert!(app.path.is_none());
        assert!(app.message.as_deref().unwrap().starts_with("Saved previous model"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn opening_preserves_non_utf8_paths_for_the_next_save() {
        use std::os::unix::ffi::OsStringExt;
        let directory = Directory::new();
        let path = directory.0.join(std::ffi::OsString::from_vec(b"model-\xff.rysmo".to_vec()));
        let mut app = app();
        std::fs::write(&path, rysmo_io::save(app.editor.doc())).unwrap();
        open(&mut app, path.clone());
        wait(&mut app);
        assert_eq!(app.path, Some(path.clone()));
        draw(&mut app, 0.);
        let destination = app.path.clone().unwrap();
        save(&mut app, destination);
        wait(&mut app);
        assert_eq!(read(&path).root.mesh.edge_count(), 1);
    }

    #[test]
    fn open_preserves_edits_made_while_the_worker_was_reading() {
        let directory = Directory::new();
        let mut source = app();
        draw(&mut source, 0.);
        std::fs::write(directory.model(), rysmo_io::save(source.editor.doc())).unwrap();
        let mut app = app();
        let origin = app.document_generation;
        open(&mut app, directory.model());
        draw(&mut app, 3.);
        wait(&mut app);
        assert_eq!(app.path, Some(directory.model()));
        assert_eq!(app.editor.mesh().edge_count(), 1);
        assert!(!app.editor.is_dirty());
        let original = app.documents.session_mut(origin).unwrap();
        assert!(original.editor.is_dirty());
        assert_eq!(original.editor.mesh().edge_count(), 1);
    }

    #[test]
    fn opening_after_a_tab_switch_adds_a_background_tab_without_stealing_focus() {
        let directory = Directory::new();
        let mut app = app();
        draw(&mut app, 0.);
        std::fs::write(directory.model(), rysmo_io::save(app.editor.doc())).unwrap();
        open(&mut app, directory.model());
        assert!(crate::documents::create(&mut app));
        draw(&mut app, 2.);
        let active = app.documents.active;
        wait(&mut app);
        assert_eq!(app.documents.active, active);
        assert!(app.path.is_none());
        assert!(app.editor.is_dirty());
        assert_eq!(app.documents.sessions(&app).count(), 3);
        assert!(app.documents.sessions(&app).any(|(_, session)| session.path == Some(directory.model())));
    }

    #[test]
    fn opening_does_not_dismiss_an_unsaved_changes_prompt() {
        let directory = Directory::new();
        let mut app = app();
        draw(&mut app, 0.);
        std::fs::write(directory.model(), rysmo_io::save(app.editor.doc())).unwrap();
        open(&mut app, directory.model());
        crate::unsaved::request(&mut app, crate::unsaved::Action::Close);
        assert!(app.unsaved.active());
        let active = app.documents.active;
        wait(&mut app);
        assert_eq!(app.documents.active, active);
        assert!(app.unsaved.active());
        assert!(app.path.is_none());
        assert!(app.editor.is_dirty());
        assert_eq!(app.documents.sessions(&app).count(), 2);
        assert!(app.documents.sessions(&app).any(|(_, session)| session.path == Some(directory.model())));
    }

    #[test]
    fn failed_write_retains_destination_and_dirty_state_and_close_waits_for_saves() {
        let directory = Directory::new();
        let mut app = app();
        draw(&mut app, 0.);
        app.path = Some("original.rysmo".into());
        save(&mut app, directory.0.clone());
        crate::documents::close(&mut app);
        assert_eq!(app.path, Some("original.rysmo".into()));
        app.editor.mark_saved();
        crate::unsaved::request(&mut app, crate::unsaved::Action::Close);
        assert!(!app.unsaved.close_ready);
        app.editor.mark_dirty();
        wait(&mut app);
        assert!(app.editor.is_dirty());
        assert_eq!(app.path, Some("original.rysmo".into()));
        assert!(app.message.as_deref().unwrap().starts_with("Cannot save"));
    }

    #[test]
    fn ordered_saves_from_two_tabs_cannot_overwrite_the_newer_request_out_of_order() {
        let directory = Directory::new();
        let mut first = app();
        draw(&mut first, 0.);
        let mut second = app();
        draw(&mut second, 0.);
        draw(&mut second, 1.);
        save(&mut first, directory.model());
        save(&mut second, directory.model());
        wait(&mut first);
        wait(&mut second);
        assert_eq!(read(&directory.model()).root.mesh.edge_count(), 2);
    }

    #[test]
    fn closing_waits_then_rechecks_newer_edits_instead_of_discarding_them() {
        let directory = Directory::new();
        for newer_edits in [false, true] {
            let mut app = app();
            draw(&mut app, 0.);
            save(&mut app, directory.model());
            if newer_edits {
                draw(&mut app, 1.);
            }
            crate::unsaved::request(&mut app, crate::unsaved::Action::Close);
            assert!(app.unsaved.active());
            assert!(!app.unsaved.close_ready);
            wait(&mut app);
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| crate::unsaved::panel(ui.ctx(), &mut app));
            output.textures_delta.clear();
            assert_eq!(app.unsaved.close_ready, !newer_edits);
            assert_eq!(app.unsaved.active(), newer_edits);
            assert_eq!(app.editor.is_dirty(), newer_edits);
        }
    }

    #[test]
    fn cancelled_open_is_not_installed_and_queued_cancel_does_not_read_the_path() {
        let directory = Directory::new();
        let mut app = app();
        std::fs::write(directory.model(), rysmo_io::save(app.editor.doc())).unwrap();
        open(&mut app, directory.model());
        app.native_files.pending = None;
        poll_app(&mut app);
        assert!(app.path.is_none());
        let (reply, _) = mpsc::channel();
        let request = Request {
            work: Work::Open,
            path: directory.0.join("does-not-exist"),
            reply,
            stage: Arc::new(AtomicU8::new(QUEUED)),
            cancelled: Arc::new(AtomicBool::new(true)),
        };
        assert!(process(&request).unwrap().is_none());
    }

    #[test]
    #[ignore = "manual disk I/O benchmark; set RYSMO_RECOVERY_BENCH_MODEL"]
    fn benchmark_background_file_dispatch() {
        let source = PathBuf::from(std::env::var("RYSMO_RECOVERY_BENCH_MODEL").expect("native model path"));
        let document = read(&source);
        let directory = Directory::new();
        fn summary(mut samples: Vec<f64>) -> serde_json::Value {
            samples.sort_by(f64::total_cmp);
            serde_json::json!({"median_ms": samples[10], "p95_ms": samples[19]})
        }
        let mut reports = Vec::new();
        for opening in [false, true] {
            let mut dispatch = Vec::new();
            let mut completion = Vec::new();
            let mut polling = Vec::new();
            for i in 0..24 {
                let mut app =
                    App3d::new(Editor::with_document(if opening { Document::new() } else { document.clone() }));
                app.camera = document.camera.unwrap_or_default();
                let started = Instant::now();
                if opening {
                    open(&mut app, source.clone());
                } else {
                    save(&mut app, directory.model());
                }
                assert!(app.native_files.busy(), "dispatch failed: {:?}", app.message);
                let submitted = started.elapsed().as_secs_f64() * 1000.;
                let mut slowest_poll = 0.0_f64;
                let deadline = Instant::now() + Duration::from_secs(30);
                while app.native_files.busy() {
                    assert!(Instant::now() < deadline);
                    let polled = Instant::now();
                    poll_app(&mut app);
                    slowest_poll = slowest_poll.max(polled.elapsed().as_secs_f64() * 1000.);
                    std::thread::sleep(Duration::from_millis(1));
                }
                if i >= 3 {
                    dispatch.push(submitted);
                    completion.push(started.elapsed().as_secs_f64() * 1000.);
                    polling.push(slowest_poll);
                }
                if opening {
                    assert_eq!(app.path, Some(source.clone()));
                    assert_eq!(app.editor.doc().definitions.len(), document.definitions.len());
                } else {
                    assert!(!app.editor.is_dirty());
                    let mut expected = document.clone();
                    expected.camera = Some(app.camera);
                    assert_eq!(std::fs::read_to_string(directory.model()).unwrap(), rysmo_io::save(&expected));
                }
            }
            reports.push(serde_json::json!({"operation": if opening { "open" } else { "save" },
                "dispatch": summary(dispatch), "completion_wall": summary(completion), "maximum_poll_per_operation": summary(polling)}));
        }
        println!(
            "background-file-benchmark: {}",
            serde_json::json!({"input_bytes": std::fs::metadata(source).unwrap().len(), "samples":21, "results":reports})
        );
    }
}
