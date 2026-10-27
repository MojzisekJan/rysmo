//! The browser side: the page's canvas, files, storage, and handing over from the
//! loading screen.
//!
//! The open document is autosaved into IndexedDB at the configured interval and comes
//! back on the next visit; the recent files are kept there too. Where the browser has
//! the File System Access API, Open and Save work on the real file and Save writes it in
//! place; elsewhere Open uses a file input and Save downloads. The JavaScript half is
//! `web_files.js`. See docs/web.md.

use std::cell::RefCell;

use bevy::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;

use crate::autosave::DocumentVersion;
use crate::files;
use crate::state::App3d;

#[wasm_bindgen(module = "/src/web_files.js")]
extern "C" {
    fn install_shortcuts();
    fn set_unsaved_changes(dirty: bool);
    fn set_render_diagnostics(text: &str);
    async fn restore() -> JsValue;
    async fn restore_workspace() -> JsValue;
    async fn save_workspace(json: &str) -> JsValue;
    async fn recent_names() -> JsValue;
    async fn autosave(name: &str, text: &str, dirty: bool) -> JsValue;
    fn get_autosave_settings() -> JsValue;
    fn set_autosave_settings(enabled: bool, minutes: u32);
    fn forget_file();
    fn activate_document(id: f64);
    fn forget_document(id: f64);
    fn accept_open_file(token: f64);
    fn reject_open_file(token: f64);
    async fn pick_open() -> JsValue;
    async fn pick_texture_image() -> JsValue;
    async fn pick_data_file(accept: &str, max_bytes: u32) -> JsValue;
    async fn open_recent(name: &str) -> JsValue;
    async fn save(name: &str, text: &str, save_as: bool) -> JsValue;
    /// Hands the browser a file to keep in its downloads.
    pub fn download(name: &str, bytes: &[u8], mime: &str);
}

/// What the asynchronous side found, for the next frame.
enum Arrived {
    Workspace { documents: Vec<(f64, String, String, bool, f64)>, active: f64 },
    Restored { name: String, text: String, dirty: bool, token: f64 },
    Opened { name: String, text: String, token: f64, version: DocumentVersion },
    Saved { name: String, how: String, version: DocumentVersion },
    Autosaved { version: DocumentVersion, camera: rysmo_math::OrbitCamera, dirty: bool, success: bool },
    Recent(Vec<String>),
    Texture { name: String, bytes: Vec<u8> },
    Data { purpose: crate::features::data_files::Purpose, name: String, bytes: Vec<u8>, version: DocumentVersion },
    DataError(String),
}

thread_local! {
    static ARRIVED: RefCell<Vec<Arrived>> = const { RefCell::new(Vec::new()) };
}

fn arrive(a: Arrived) {
    ARRIVED.with(|q| q.borrow_mut().push(a));
}

#[derive(Resource, Default)]
struct Autosave {
    /// Capture the session generation; its initial value is not a sentinel.
    startup_generation: u64,
    /// The last recovery snapshot.
    written: Option<DocumentVersion>,
    in_flight: bool,
    timer: crate::autosave::Timer,
    preferences: Option<crate::autosave::Settings>,
    written_camera: Option<rysmo_math::OrbitCamera>,
    written_dirty: Option<bool>,
    /// Write without waiting for the interval, so recovery never predates a saved file.
    flush: bool,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Autosave>()
        .add_systems(Startup, start)
        .add_systems(Update, (take_arrived, autosave_now, hide_loader, unload_guard).chain())
        .add_systems(Last, render_diagnostics);
}

/// Whether the page's address has `?name` (or `&name`) in its query.
pub fn query_has(name: &str) -> bool {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok())
        .is_some_and(|p| p.has(name))
}

/// Whether the browser runs on a touch screen, such as an iPad (which reports itself as
/// a Mac but has touch points), so the tablet layout can start before the first touch.
pub fn touch_device() -> bool {
    field(&field(&js_sys::global(), "navigator"), "maxTouchPoints").as_f64().is_some_and(|n| n > 0.)
}

/// Whether the browser runs on a Mac, iPad or iPhone, where shortcuts read Cmd.
pub fn apple_platform() -> bool {
    let platform = field(&field(&js_sys::global(), "navigator"), "platform").as_string().unwrap_or_default();
    ["Mac", "iPad", "iPhone"].iter().any(|name| platform.starts_with(name))
}

fn field(v: &JsValue, key: &str) -> JsValue {
    js_sys::Reflect::get(v, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

fn strings(v: &JsValue) -> Vec<String> {
    js_sys::Array::from(v).iter().filter_map(|s| s.as_string()).collect()
}

/// `[name, text]` from JavaScript.
fn pair(v: &JsValue) -> Option<(String, String)> {
    if v.is_null() || v.is_undefined() {
        return None;
    }
    let a = js_sys::Array::from(v);
    Some((a.get(0).as_string()?, a.get(1).as_string()?))
}

/// Brings back the document from the last visit (unless the address asks for the demo)
/// and the recent list.
fn start(mut app: ResMut<App3d>, mut auto: ResMut<Autosave>) {
    let settings = js_sys::Array::from(&get_autosave_settings());
    app.autosave_settings = crate::autosave::Settings {
        enabled: settings.get(0).as_bool().unwrap_or(true),
        minutes: settings.get(1).as_f64().unwrap_or(1.) as u32,
    };
    auto.preferences = Some(app.autosave_settings);
    auto.startup_generation = app.document_generation;
    install_shortcuts();
    activate_tab(app.documents.active);
    let keep_demo = query_has("demo");
    wasm_bindgen_futures::spawn_local(async move {
        let state = restore().await;
        arrive(Arrived::Recent(strings(&field(&state, "recent"))));
        let workspace = if keep_demo { JsValue::NULL } else { restore_workspace().await };
        if !keep_demo && !workspace.is_null() && !workspace.is_undefined() {
            let documents = js_sys::Array::from(&field(&workspace, "documents"))
                .iter()
                .filter_map(|doc| {
                    Some((
                        field(&doc, "id").as_f64()?,
                        field(&doc, "name").as_string()?,
                        field(&doc, "text").as_string()?,
                        field(&doc, "dirty").as_bool().unwrap_or(false),
                        field(&doc, "token").as_f64().unwrap_or(0.),
                    ))
                })
                .collect::<Vec<_>>();
            if !documents.is_empty() {
                if let Some(token) = field(&state, "token").as_f64() {
                    reject_open_file(token);
                }
                arrive(Arrived::Workspace { documents, active: field(&workspace, "active").as_f64().unwrap_or(0.) });
                return;
            }
        }
        let doc = field(&state, "current");
        if !keep_demo
            && !doc.is_null()
            && !doc.is_undefined()
            && let (Some(name), Some(text)) = (field(&doc, "name").as_string(), field(&doc, "text").as_string())
        {
            let dirty = field(&doc, "dirty").as_bool().unwrap_or(false);
            let token = field(&state, "token").as_f64().unwrap_or(0.);
            arrive(Arrived::Restored { name, text, dirty, token });
        } else if let Some(token) = field(&state, "token").as_f64() {
            reject_open_file(token);
        }
    });
}

fn refresh_recent() {
    wasm_bindgen_futures::spawn_local(async {
        arrive(Arrived::Recent(strings(&recent_names().await)));
    });
}

/// File > Open: the browser's picker. The file arrives a few frames later.
pub fn pick_file(version: DocumentVersion) {
    wasm_bindgen_futures::spawn_local(async move {
        let result = pick_open().await;
        if let Some((name, text)) = pair(&result) {
            let token = js_sys::Array::from(&result).get(2).as_f64().unwrap_or(0.);
            arrive(Arrived::Opened { name, text, token, version });
        }
        refresh_recent();
    });
}

/// File > Open Recent.
pub fn open_recent_file(name: String, version: DocumentVersion) {
    wasm_bindgen_futures::spawn_local(async move {
        let result = open_recent(&name).await;
        if let Some((name, text)) = pair(&result) {
            let token = js_sys::Array::from(&result).get(2).as_f64().unwrap_or(0.);
            arrive(Arrived::Opened { name, text, token, version });
        }
        refresh_recent();
    });
}

/// File > Save (in place where it can) and Save As (always asks).
pub fn save_file(name: String, text: String, save_as: bool, version: DocumentVersion) {
    wasm_bindgen_futures::spawn_local(async move {
        if let Some((name, how)) = pair(&save(&name, &text, save_as).await) {
            arrive(Arrived::Saved { name, how, version });
        }
        refresh_recent();
    });
}

/// File > New: the next save asks for a file.
pub fn new_document() {
    forget_file();
}

fn take_arrived(mut app: ResMut<App3d>, mut auto: ResMut<Autosave>, time: Res<Time<Real>>) {
    let arrived: Vec<Arrived> = ARRIVED.with(|q| q.borrow_mut().drain(..).collect());
    for a in arrived {
        match a {
            Arrived::Workspace { documents, active } => {
                if !app.accepts_recovery(auto.startup_generation) {
                    for (_, _, _, _, token) in documents {
                        reject_open_file(token);
                    }
                    continue;
                }
                let mut chosen = None;
                for (id, name, text, dirty, token) in documents {
                    match files::open_text(&mut app, &text, &name) {
                        Ok(_) => {
                            accept_open_file(token);
                            if dirty {
                                app.editor.mark_dirty();
                            }
                            if id == active {
                                chosen = Some(app.documents.active);
                            }
                        }
                        Err(error) => {
                            reject_open_file(token);
                            app.message = Some(error);
                        }
                    }
                }
                if let Some(id) = chosen {
                    crate::documents::switch(&mut app, id);
                }
                app.message = Some("Restored document workspace from the last visit.".into());
            }
            Arrived::Restored { name, text, dirty, token } if app.accepts_recovery(auto.startup_generation) => {
                match files::open_text(&mut app, &text, &name) {
                    Ok(_) => {
                        accept_open_file(token);
                        if dirty {
                            app.editor.mark_dirty();
                        }
                        auto.written = Some(DocumentVersion {
                            generation: app.document_generation,
                            revision: app.editor.revision(),
                        });
                        auto.written_camera = Some(app.camera);
                        auto.written_dirty = Some(dirty);
                        app.message = Some(format!("Restored {name} from the last visit."));
                    }
                    Err(e) => {
                        reject_open_file(token);
                        app.message = Some(e);
                    }
                }
            }
            Arrived::Restored { token, .. } => reject_open_file(token),
            Arrived::Opened { name, text, token, version } => {
                let current = DocumentVersion { generation: app.document_generation, revision: app.editor.revision() };
                if current != version {
                    reject_open_file(token);
                    app.message = Some(
                        "The model changed while choosing a file. Open it again to replace the current model.".into(),
                    );
                    continue;
                }
                let r = files::open_text(&mut app, &text, &name);
                if r.is_ok() {
                    accept_open_file(token);
                    persist_tabs(&app);
                } else {
                    reject_open_file(token);
                }
                app.message = Some(r.unwrap_or_else(|e| e));
            }
            Arrived::Saved { name, how, version } => {
                let current = DocumentVersion { generation: app.document_generation, revision: app.editor.revision() };
                if !version.same_document(current) {
                    if let Some(session) = app.documents.session_mut(version.generation) {
                        if session.editor.revision() == version.revision {
                            session.editor.mark_saved();
                        }
                        session.path = Some(name.into());
                        persist_tabs(&app);
                    }
                    continue;
                }
                // Only clean if nothing changed while the browser was saving.
                if current == version {
                    app.editor.mark_saved();
                }
                app.path = Some(name.clone().into());
                app.message = Some(match how.as_str() {
                    "download" => format!("Downloaded {name}."),
                    _ => format!("Saved {name}."),
                });
                // Write the clean state into the autosave copy too, before a reload can restore an older one.
                auto.written = None;
                auto.flush = true;
            }
            Arrived::Autosaved { version, camera, dirty, success } => {
                auto.in_flight = false;
                let current = DocumentVersion { generation: app.document_generation, revision: app.editor.revision() };
                if !version.same_document(current) {
                    continue;
                }
                if success {
                    auto.written = Some(version);
                    auto.written_camera = Some(camera);
                    auto.written_dirty = Some(dirty);
                } else {
                    auto.timer.retry(time.elapsed_secs_f64());
                    app.message = Some(
                        "Browser recovery could not be saved. Use Save to keep a model file; recovery will retry."
                            .into(),
                    );
                }
            }
            Arrived::Recent(list) => app.recent = list,
            Arrived::Data { purpose, name, bytes, version } => {
                let current = DocumentVersion { generation: app.document_generation, revision: app.editor.revision() };
                if purpose != crate::features::data_files::Purpose::Collection && current != version {
                    app.message = Some("The model changed while choosing an import. Choose the file again.".into());
                } else {
                    crate::features::data_files::receive(&mut app, purpose, name, bytes);
                }
            }
            Arrived::DataError(error) => app.message = Some(error),
            Arrived::Texture { name, bytes } => {
                app.message = Some(match files::import_texture_bytes(&mut app, &name, bytes) {
                    Ok(()) => format!("Loaded texture {name}."),
                    Err(e) => e,
                });
            }
        }
    }
}

/// Keeps an independent recovery snapshot without postponing continuous edits.
fn autosave_now(mut app: ResMut<App3d>, mut auto: ResMut<Autosave>, time: Res<Time<Real>>) {
    if app.welcome && app.path.is_none() && app.accepts_recovery(auto.startup_generation) {
        return;
    }
    let settings = app.autosave_settings;
    if auto.preferences != Some(settings) {
        set_autosave_settings(settings.enabled, settings.minutes);
        auto.preferences = Some(settings);
    }
    if auto.in_flight {
        return;
    }
    let version = DocumentVersion { generation: app.document_generation, revision: app.editor.revision() };
    let pending = auto.written != Some(version)
        || auto.written_camera != Some(app.camera)
        || auto.written_dirty != Some(app.editor.is_dirty());
    auto.flush &= pending;
    let due = auto.timer.due(time.elapsed_secs_f64(), pending, app.document_generation, settings);
    if due || (auto.flush && settings.enabled) {
        auto.flush = false;
        let name = app
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled.rysmo".into());
        let camera = app.camera;
        app.editor.set_camera(camera);
        let text = rysmo_io::save(app.editor.doc());
        let dirty = app.editor.is_dirty();
        let workspace = workspace_text(&app);
        auto.in_flight = true;
        wasm_bindgen_futures::spawn_local(async move {
            let workspace_saved = save_workspace(&workspace).await.as_bool().unwrap_or(false);
            let single = autosave(&name, &text, dirty).await.as_bool().unwrap_or(false);
            let success = workspace_saved && single;
            arrive(Arrived::Autosaved { version, camera, dirty, success });
        });
    }
}

/// Removes the loading screen once the first frames are drawn.
fn hide_loader(mut frames: Local<u32>) {
    *frames += 1;
    if *frames == 3
        && let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("loading"))
    {
        el.remove();
    }
}

pub fn pick_texture() {
    wasm_bindgen_futures::spawn_local(async {
        let result = pick_texture_image().await;
        if !result.is_null() && !result.is_undefined() {
            let values = js_sys::Array::from(&result);
            if let Some(name) = values.get(0).as_string() {
                let bytes = js_sys::Uint8Array::new(&values.get(1)).to_vec();
                arrive(Arrived::Texture { name, bytes });
            }
        }
    });
}

fn unload_guard(app: Res<App3d>) {
    set_unsaved_changes(app.editor.is_dirty() || app.documents.dirty());
}

/// Measure completed editor updates separately from browser animation callbacks.
fn render_diagnostics(
    app: Res<App3d>,
    time: Res<Time<Real>>,
    cameras: Query<&Transform, With<crate::viewport::MainCamera>>,
    meshes: Query<&Mesh3d>,
    mut enabled: Local<Option<bool>>,
    mut sample: Local<(u64, f64, u64)>,
) {
    if !*enabled.get_or_insert_with(|| query_has("diagnostics")) {
        return;
    }
    sample.0 += 1;
    let now = time.elapsed_secs_f64();
    let elapsed = now - sample.1;
    if elapsed < 1. {
        return;
    }
    let camera_synced = cameras
        .single()
        .is_ok_and(|camera| camera.translation.distance(crate::state::to_bevy(app.camera.eye())) < 0.01);
    set_render_diagnostics(&format!(
        "Editor updates: {:.1} FPS\nEditor frame: {} / revision {}\nRoot instances: {} / mesh entities: {}\nCamera synced: {} / 3D scale: {:.0}%",
        (sample.0 - sample.2) as f64 / elapsed,
        sample.0,
        app.editor.revision(),
        app.editor.doc().root.instances.len(),
        meshes.iter().count(),
        camera_synced,
        app.editor.doc().style.render_scale * 100.,
    ));
    sample.1 = now;
    sample.2 = sample.0;
}

pub fn pick_data(purpose: crate::features::data_files::Purpose, version: DocumentVersion) {
    wasm_bindgen_futures::spawn_local(async move {
        let accept = purpose.extensions().iter().map(|s| format!(".{s}")).collect::<Vec<_>>().join(",");
        let result = pick_data_file(&accept, 64 * 1024 * 1024).await;
        if let Some(error) = field(&result, "error").as_string() {
            arrive(Arrived::DataError(error));
            return;
        }
        if result.is_null() || result.is_undefined() {
            return;
        }
        let values = js_sys::Array::from(&result);
        if let Some(name) = values.get(0).as_string() {
            arrive(Arrived::Data { purpose, name, bytes: js_sys::Uint8Array::new(&values.get(1)).to_vec(), version });
        }
    });
}

/// Switch the browser file destination together with the active editor tab.
pub fn activate_tab(id: u64) {
    activate_document(id as f64);
}

fn workspace_text(app: &App3d) -> String {
    let documents: Vec<_> = app.documents.sessions(app).map(|(id, session)| {
        let mut doc = session.editor.doc().clone();
        doc.camera = Some(session.camera);
        serde_json::json!({ "id": id, "name": session.path.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled.rysmo".into()), "text": rysmo_io::save(&doc), "dirty": session.editor.is_dirty() })
    }).collect();
    serde_json::json!({ "active": app.documents.active, "documents": documents }).to_string()
}
pub fn persist_tabs(app: &App3d) {
    if !app.autosave_settings.enabled {
        return;
    }
    let json = workspace_text(app);
    wasm_bindgen_futures::spawn_local(async move {
        save_workspace(&json).await;
    });
}

pub fn forget_tab(id: u64) {
    forget_document(id as f64);
}
