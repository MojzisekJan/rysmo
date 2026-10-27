//! New, Open, Save and Export.
//!
//! On the desktop these use the system's file dialogs, which block the frame while open.
//! In the browser, Open uses a file input and Save and Export download the file (see
//! `web.rs` and docs/web.md).

use std::path::Path;

use rysmo_model::{Document, Editor};

use crate::state::App3d;

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn report(app: &mut App3d, result: Result<String, String>) {
    app.message = Some(match result {
        Ok(m) => m,
        Err(e) => e,
    });
}

fn reset_interaction(app: &mut App3d) {
    app.welcome = false;
    app.features.import = None;
    app.features.parametric = Default::default();
    app.features.photo = Default::default();
    app.portable_files = None;
    app.paste_place = None;
    app.document_generation = crate::documents::generation();
    app.tools.set_tool(app.tools.active());
    app.tools.set_paint(None);
    app.vcb.clear();
    app.camera_drag = None;
    app.orbit_pivot = None;
    app.positioning = None;
    app.walking = None;
    app.dimension_text_edit = None;
    app.text_edit = None;
    app.drape = None;
    app.stamp = None;
    app.section_placing = None;
    app.texture_session = None;
    app.selected_scene = None;
    app.scene_update = None;
    app.component_place = None;
    app.component_edit = None;
    app.component_selected = None;
    app.outliner_collapsed.clear();
    app.outliner_rename = None;
    app.detail = None;
    app.flip = None;
    app.mirror = None;
    app.solid_tools = None;
    app.print_check = None;
    app.fillet = None;
    app.smoove = None;
    app.terrain_last = None;
    app.terrain = None;
    app.text3d = None;
    app.text_creation = None;
    app.left_down = false;
    app.zoom_window = None;
    app.last_wheel_time = None;
    app.camera_setting_editing = false;
    app.zoom_focal = false;
    app.context_menu = None;
    app.ctrl_toggle = false;
    app.touch.gestures = Default::default();
    app.touch.orbiting = false;
    app.touch.navigation_started = false;
}

pub fn new(app: &mut App3d) {
    if !crate::documents::create(app) {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    crate::web::new_document();
    reset_interaction(app);
    app.editor.replace_document(Document::new());
    app.path = None;
    app.camera = Default::default();
    app.camera_history.clear();
    app.zoom_window = None;
    app.last_wheel_time = None;
    app.message = Some("New model.".into());
    #[cfg(target_arch = "wasm32")]
    crate::web::persist_tabs(app);
}

/// Puts a loaded document in the window. `name` is where it came from.
pub fn open_text(app: &mut App3d, text: &str, name: &str) -> Result<String, String> {
    let doc = rysmo_io::load(text).map_err(|e| format!("Cannot open {name}: {e}"))?;
    if !crate::documents::create(app) {
        return Err("Document limit reached".into());
    }
    reset_interaction(app);
    app.camera_history.clear();
    app.zoom_window = None;
    app.last_wheel_time = None;
    let mut camera = doc.camera.unwrap_or_default();
    if doc.camera.is_none()
        && let Some((lo, hi)) = doc.definition_bounds(None)
    {
        camera.frame(lo, hi);
    }
    app.camera = camera;
    app.editor.replace_document(doc);
    app.path = Some(name.into());
    #[cfg(not(target_arch = "wasm32"))]
    {
        app.preferences.remember(name);
        app.recent = app.preferences.recent.clone();
    }
    Ok(format!("Opened {name}."))
}

pub fn open_path(app: &mut App3d, path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    open_text(app, &text, &path.display().to_string())
}

/// The file name to offer, from the current path.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn stem(app: &App3d) -> String {
    app.path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Untitled".into())
}

pub(crate) fn document_text(app: &mut App3d) -> String {
    crate::texture_view::finish(app, false);
    let camera = app.camera;
    app.editor.set_camera(camera);
    rysmo_io::save(app.editor.doc())
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
mod imp {
    use std::path::{Path, PathBuf};

    use super::{App3d, document_text, open_path, report, stem};

    pub fn open(app: &mut App3d) {
        let Some(path) = rfd::FileDialog::new().add_filter("Rysmo model", &["rysmo"]).pick_file() else { return };
        let r = open_path(app, &path);
        report(app, r);
    }

    /// Saves to the current file, or asks where (always when `save_as`).
    pub fn save(app: &mut App3d, save_as: bool) {
        let path = match (&app.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => {
                let Some(mut p) = rfd::FileDialog::new()
                    .add_filter("Rysmo model", &["rysmo"])
                    .set_file_name(format!("{}.rysmo", stem(app)))
                    .save_file()
                else {
                    return;
                };
                if p.extension().is_none() {
                    p.set_extension("rysmo");
                }
                p
            }
        };
        let r = save_path(app, &path);
        report(app, r);
    }

    pub fn save_path(app: &mut App3d, path: &Path) -> Result<String, String> {
        let text = document_text(app);
        std::fs::write(path, text).map_err(|e| format!("Cannot save {}: {e}", path.display()))?;
        app.editor.mark_saved();
        app.path = Some(path.to_path_buf());
        app.preferences.remember(&path.to_string_lossy());
        app.recent = app.preferences.recent.clone();
        Ok(format!("Saved {}.", path.display()))
    }

    fn export_target(app: &App3d, ext: &str, name: &str) -> Option<PathBuf> {
        let mut p = rfd::FileDialog::new()
            .add_filter(name, &[ext])
            .set_file_name(format!("{}.{ext}", stem(app)))
            .save_file()?;
        if p.extension().is_none() {
            p.set_extension(ext);
        }
        Some(p)
    }

    pub fn export_obj(app: &mut App3d) {
        let Some(path) = export_target(app, "obj", "Wavefront OBJ") else { return };
        let mtl_path = path.with_extension("mtl");
        let mtl_name =
            mtl_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "model.mtl".into());
        let out = rysmo_io::obj::export(app.editor.doc(), &mtl_name);
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let r = std::fs::write(&path, out.obj)
            .and_then(|_| std::fs::write(&mtl_path, out.mtl))
            .and_then(|_| out.images.iter().try_for_each(|(name, bytes)| std::fs::write(dir.join(name), bytes)))
            .map(|_| format!("Exported {} (and {} images).", path.display(), out.images.len()))
            .map_err(|e| format!("Cannot export: {e}"));
        report(app, r);
    }

    pub fn export_collada(app: &mut App3d) {
        let Some(path) = export_target(app, "dae", "COLLADA") else { return };
        let result = super::write_collada(&path, app.editor.doc())
            .map(|()| format!("Exported {} and its textures.", path.display()));
        report(app, result);
    }
    pub fn export_dxf(app: &mut App3d) {
        let Some(path) = export_target(app, "dxf", "AutoCAD DXF") else { return };
        let result = std::fs::write(&path, rysmo_io::dxf::export(app.editor.doc()))
            .map(|()| format!("Exported {} (3D faces and lines, metres).", path.display()))
            .map_err(|e| e.to_string());
        report(app, result);
    }
    pub fn export_stl(app: &mut App3d) {
        let Some(path) = export_target(app, "stl", "STL") else { return };
        let r = std::fs::write(&path, rysmo_io::print::stl(app.editor.doc()))
            .map(|_| format!("Exported {} (millimetres).", path.display()))
            .map_err(|e| format!("Cannot export: {e}"));
        report(app, r);
    }
    pub fn export_3mf(app: &mut App3d) {
        let Some(path) = export_target(app, "3mf", "3D Manufacturing Format") else { return };
        let r = rysmo_io::print::three_mf(app.editor.doc())
            .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()))
            .map(|_| format!("Exported {} (millimetres).", path.display()))
            .map_err(|e| format!("Cannot export: {e}"));
        report(app, r);
    }
    pub fn export_glb(app: &mut App3d) {
        let Some(path) = export_target(app, "glb", "glTF binary") else { return };
        let r = std::fs::write(&path, rysmo_io::gltf::export(app.editor.doc()))
            .map(|_| format!("Exported {}.", path.display()))
            .map_err(|e| format!("Cannot export: {e}"));
        report(app, r);
    }
}

/// The browser (see `web.rs`): Open picks a file, through the File System Access API
/// where there is one, and the file arrives a few frames later; Save writes the picked
/// file in place or downloads; Export downloads.
#[cfg(target_arch = "wasm32")]
mod imp {
    use super::{App3d, document_text, stem};
    use crate::web;

    pub fn open(app: &mut App3d) {
        web::pick_file(crate::autosave::DocumentVersion {
            generation: app.document_generation,
            revision: app.editor.revision(),
        });
        app.message = Some("Choose a .rysmo file.".into());
    }

    pub fn open_recent(app: &mut App3d, name: &str) {
        web::open_recent_file(
            name.to_string(),
            crate::autosave::DocumentVersion { generation: app.document_generation, revision: app.editor.revision() },
        );
        app.message = Some(format!("Opening {name}..."));
    }

    pub fn save(app: &mut App3d, save_as: bool) {
        let name = format!("{}.rysmo", stem(app));
        let text = document_text(app);
        web::save_file(
            name,
            text,
            save_as,
            crate::autosave::DocumentVersion { generation: app.document_generation, revision: app.editor.revision() },
        );
    }

    pub fn export_obj(app: &mut App3d) {
        let base = stem(app);
        let out = rysmo_io::obj::export(app.editor.doc(), &format!("{base}.mtl"));
        web::download(&format!("{base}.obj"), out.obj.as_bytes(), "text/plain");
        web::download(&format!("{base}.mtl"), out.mtl.as_bytes(), "text/plain");
        for (name, bytes) in &out.images {
            web::download(name, bytes, "image/png");
        }
        app.message = Some(format!("Downloaded {base}.obj, {base}.mtl and {} images.", out.images.len()));
    }

    pub fn export_collada(app: &mut App3d) {
        let base = stem(app);
        let out = rysmo_io::collada::export(app.editor.doc());
        if out.images.is_empty() {
            let name = format!("{base}.dae");
            web::download(&name, out.dae.as_bytes(), "model/vnd.collada+xml");
            app.message = Some(format!("Downloaded {name}."));
        } else {
            let mut files = out.images;
            files.insert(0, (format!("{base}.dae"), out.dae.into_bytes()));
            app.message = Some(match rysmo_io::archive::zip(&files) {
                Ok(bytes) => {
                    let name = format!("{base}-collada.zip");
                    web::download(&name, &bytes, "application/zip");
                    format!("Downloaded {name}. Extract the DAE and textures together.")
                }
                Err(error) => format!("Cannot export COLLADA: {error}"),
            });
        }
    }
    pub fn export_dxf(app: &mut App3d) {
        let name = format!("{}.dxf", stem(app));
        web::download(&name, rysmo_io::dxf::export(app.editor.doc()).as_bytes(), "application/dxf");
        app.message = Some(format!("Downloaded {name} (3D faces and lines, metres)."));
    }
    pub fn export_stl(app: &mut App3d) {
        let name = format!("{}.stl", stem(app));
        web::download(&name, &rysmo_io::print::stl(app.editor.doc()), "model/stl");
        app.message = Some(format!("Downloaded {name} (millimetres)."));
    }
    pub fn export_3mf(app: &mut App3d) {
        let name = format!("{}.3mf", stem(app));
        app.message = Some(match rysmo_io::print::three_mf(app.editor.doc()) {
            Ok(bytes) => {
                web::download(&name, &bytes, "model/3mf");
                format!("Downloaded {name} (millimetres).")
            }
            Err(error) => format!("Cannot export 3MF: {error}"),
        });
    }
    pub fn export_glb(app: &mut App3d) {
        let name = format!("{}.glb", stem(app));
        web::download(&name, &rysmo_io::gltf::export(app.editor.doc()), "model/gltf-binary");
        app.message = Some(format!("Downloaded {name}."));
    }
}

#[cfg(target_arch = "wasm32")]
pub use imp::open_recent;
pub use imp::{export_3mf, export_collada, export_dxf, export_glb, export_obj, export_stl, open, save};

/// STL and 3MF go through the printability check first.
pub fn export_for_print(app: &mut App3d, format: crate::print_check::Format) {
    crate::print_check::begin(app, format);
}

/// Write the DAE and content-addressed images without replacing unrelated companions.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_collada(path: &Path, doc: &Document) -> Result<(), String> {
    let output = rysmo_io::collada::export(doc);
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    for (name, bytes) in &output.images {
        let target = directory.join(name);
        if target.exists() && std::fs::read(&target).map_err(|e| e.to_string())? != *bytes {
            return Err(format!("Texture {name} already exists with different contents. Choose another folder."));
        }
    }
    for (name, bytes) in output.images {
        let target = directory.join(name);
        if !target.exists() {
            std::fs::write(target, bytes).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(path, output.dae).map_err(|e| e.to_string())
}

/// The window title: file name, and a star while there are unsaved changes.
pub fn title(path: Option<&Path>, editor: &Editor) -> String {
    let name =
        path.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Untitled".into());
    format!("{}{} - Rysmo", name, if editor.is_dirty() { "*" } else { "" })
}

/// Validate the image with the same decoder used by the renderer before importing it.
pub fn import_texture_bytes(app: &mut App3d, name: &str, bytes: Vec<u8>) -> Result<(), String> {
    use bevy::asset::RenderAssetUsages;
    use bevy::image::{CompressedImageFormats, Image, ImageSampler, ImageType};
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else {
        return Err("Choose a PNG or JPEG image.".into());
    };
    let decoded = Image::from_buffer(
        &bytes,
        ImageType::MimeType(mime),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::default(),
    )
    .map_err(|e| format!("Cannot read texture: {e}"))?;
    let width = decoded.texture_descriptor.size.width;
    let height = decoded.texture_descriptor.size.height;
    let material_name =
        Path::new(name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Texture".into());
    let out = app
        .editor
        .apply(rysmo_model::Command::AddTexturedMaterial {
            name: material_name,
            color: [1.0; 4],
            image_name: name.into(),
            mime: mime.into(),
            data: bytes,
            size: [1.0, height as f64 / width.max(1) as f64],
        })
        .map_err(|e| e.to_string())?;
    app.tools.set_paint(out.material);
    app.set_tool(rysmo_tools::ToolKind::PaintBucket);
    Ok(())
}

pub fn import_texture(app: &mut App3d) {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    crate::portable_files::begin(app, crate::portable_files::Operation::Texture);
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        let Some(path) = rfd::FileDialog::new().add_filter("Texture image", &["png", "jpg", "jpeg"]).pick_file() else {
            return;
        };
        let name = path.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Texture".into());
        let result =
            std::fs::read(&path).map_err(|e| e.to_string()).and_then(|bytes| import_texture_bytes(app, &name, bytes));
        app.message = Some(match result {
            Ok(()) => format!("Loaded texture {name}."),
            Err(e) => e,
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = app;
        crate::web::pick_texture();
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
mod imp {
    use super::App3d;
    use crate::portable_files::{self, Operation};
    pub fn open(app: &mut App3d) {
        portable_files::begin(app, Operation::Open);
    }
    pub fn save(app: &mut App3d, save_as: bool) {
        if !save_as && let Some(path) = app.path.clone() {
            let result = std::fs::write(&path, super::document_text(app));
            match result {
                Ok(()) => {
                    app.editor.mark_saved();
                    app.message = Some("Saved model.".into());
                }
                Err(e) => app.message = Some(e.to_string()),
            }
        } else {
            portable_files::begin(app, Operation::Save);
        }
    }
    pub fn export_obj(app: &mut App3d) {
        portable_files::begin(app, Operation::Obj);
    }
    pub fn export_collada(app: &mut App3d) {
        portable_files::begin(app, Operation::Dae);
    }
    pub fn export_dxf(app: &mut App3d) {
        portable_files::begin(app, Operation::Dxf);
    }
    pub fn export_glb(app: &mut App3d) {
        portable_files::begin(app, Operation::Glb);
    }
    pub fn export_stl(app: &mut App3d) {
        portable_files::begin(app, Operation::Stl);
    }
    pub fn export_3mf(app: &mut App3d) {
        portable_files::begin(app, Operation::ThreeMf);
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn open_recent(app: &mut App3d, name: &str) {
    app.message = Some(open_path(app, Path::new(name)).unwrap_or_else(|e| e));
}
