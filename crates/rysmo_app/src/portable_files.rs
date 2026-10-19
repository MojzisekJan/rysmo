//! App-private document workspace for native mobile previews, without desktop dialogs.
use crate::state::App3d;
use bevy_egui::egui;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
pub enum Operation {
    Open,
    Save,
    Obj,
    Glb,
    Dae,
    Dxf,
    Stl,
    ThreeMf,
    Texture,
}
pub struct Dialog {
    operation: Operation,
    root: PathBuf,
    name: String,
    overwrite: bool,
    error: Option<String>,
}

fn file_path(root: &Path, name: &str, extension: &str) -> Result<PathBuf, String> {
    if name.chars().any(|c| c.is_control()) {
        return Err("File names cannot contain control characters.".into());
    }
    let name = name.trim();
    if name.is_empty() || name == "." || name == ".." || name.chars().any(|c| c.is_control() || "/\\:".contains(c)) {
        return Err("Enter a file name without folders.".into());
    }
    let mut path = root.join(name);
    if path.extension().is_none() {
        path.set_extension(extension);
    }
    if path.extension().and_then(|s| s.to_str()) != Some(extension) {
        return Err(format!("Use the .{extension} extension."));
    }
    if path.is_symlink() {
        return Err("Symbolic links cannot be used here.".into());
    }
    Ok(path)
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn begin(app: &mut App3d, operation: Operation) {
    #[cfg(target_os = "android")]
    let root = bevy::android::ANDROID_APP.get().and_then(|a| a.internal_data_path()).map(|p| p.join("models"));
    #[cfg(target_os = "ios")]
    let root = std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Documents"));
    let Some(root) = root else {
        app.message = Some("The device document directory is unavailable.".into());
        return;
    };
    if let Err(e) = std::fs::create_dir_all(&root) {
        app.message = Some(e.to_string());
        return;
    }
    let name = app.path.as_ref().and_then(|p| p.file_stem()).and_then(|p| p.to_str()).unwrap_or("Untitled").to_owned();
    app.portable_files = Some(Dialog { operation, root, name, overwrite: false, error: None });
}

impl Dialog {
    fn extension(&self) -> &'static str {
        match self.operation {
            Operation::Obj => "obj",
            Operation::Glb => "glb",
            Operation::Dae => "dae",
            Operation::Dxf => "dxf",
            Operation::Stl => "stl",
            Operation::ThreeMf => "3mf",
            _ => "rysmo",
        }
    }
    fn execute(&self, app: &mut App3d) -> Result<(), String> {
        if self.operation == Operation::Texture {
            let bytes = std::fs::read(self.root.join(&self.name)).map_err(|e| e.to_string())?;
            return crate::files::import_texture_bytes(app, &self.name, bytes);
        }
        let path = file_path(&self.root, &self.name, self.extension())?;
        if self.operation == Operation::Open {
            return crate::files::open_path(app, &path).map(|message| app.message = Some(message));
        }
        if path.exists() && !self.overwrite {
            return Err("This file exists. Confirm Replace existing file first.".into());
        }
        match self.operation {
            Operation::Save => {
                std::fs::write(&path, crate::files::document_text(app)).map_err(|e| e.to_string())?;
                app.path = Some(path.clone());
                app.editor.mark_saved();
            }
            Operation::Glb => {
                std::fs::write(&path, rysmo_io::gltf::export(app.editor.doc())).map_err(|e| e.to_string())?
            }
            Operation::Dae => {
                #[cfg(not(target_arch = "wasm32"))]
                crate::files::write_collada(&path, app.editor.doc())?;
            }
            Operation::Dxf => {
                std::fs::write(&path, rysmo_io::dxf::export(app.editor.doc())).map_err(|e| e.to_string())?;
            }
            Operation::Stl => {
                std::fs::write(&path, rysmo_io::print::stl(app.editor.doc())).map_err(|e| e.to_string())?;
            }
            Operation::ThreeMf => {
                let bytes = rysmo_io::print::three_mf(app.editor.doc())?;
                std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
            }
            Operation::Obj => {
                let mtl = path.with_extension("mtl");
                let name = mtl.file_name().unwrap().to_string_lossy();
                let out = rysmo_io::obj::export(app.editor.doc(), &name);
                // Require confirmation for all companion files, not just the OBJ.
                if !self.overwrite && (mtl.exists() || out.images.iter().any(|(name, _)| self.root.join(name).exists()))
                {
                    return Err("Export companion files exist. Confirm Replace existing file first.".into());
                }
                std::fs::write(&path, out.obj)
                    .and_then(|_| std::fs::write(mtl, out.mtl))
                    .and_then(|_| {
                        out.images.iter().try_for_each(|(name, bytes)| std::fs::write(self.root.join(name), bytes))
                    })
                    .map_err(|e| e.to_string())?;
            }
            _ => unreachable!(),
        }
        app.message = Some(format!("Saved {} in device documents.", path.file_name().unwrap().to_string_lossy()));
        Ok(())
    }
}

pub fn panel(ctx: &egui::Context, app: &mut App3d) {
    let Some(mut dialog) = app.portable_files.take() else { return };
    let mut open = true;
    let mut submit = false;
    let title = match dialog.operation {
        Operation::Open => "Open model",
        Operation::Save => "Save model",
        Operation::Obj | Operation::Glb | Operation::Dae | Operation::Dxf | Operation::Stl | Operation::ThreeMf => {
            "Export model"
        }
        Operation::Texture => "Import texture",
    };
    let window = crate::theme::window(title)
        .id(egui::Id::new("device-files"))
        .open(&mut open)
        .default_width(400.)
        .max_height((ctx.content_rect().height() - 100.).max(180.))
        .vscroll(true)
        .show(ctx, |ui| {
            ui.label("Device documents");
            ui.weak("This preview uses app-private files. System document picking and sharing are still pending.");
            egui::ScrollArea::vertical().max_height(220.).show(ui, |ui| {
                let mut names: Vec<_> = std::fs::read_dir(&dialog.root)
                    .into_iter()
                    .flatten()
                    .filter_map(Result::ok)
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .filter(|name| {
                        if dialog.operation == Operation::Texture {
                            ["png", "jpg", "jpeg"].iter().any(|ext| name.to_lowercase().ends_with(&format!(".{ext}")))
                        } else {
                            name.ends_with(&format!(".{}", dialog.extension()))
                        }
                    })
                    .collect();
                names.sort();
                for name in names {
                    if ui
                        .add_sized(
                            egui::vec2(ui.available_width(), 32.),
                            egui::Button::new(&name).selected(dialog.name == name).truncate(),
                        )
                        .clicked()
                    {
                        dialog.name = name;
                        dialog.overwrite = false;
                    }
                }
            });
            let editing = !matches!(dialog.operation, Operation::Open | Operation::Texture);
            if editing {
                if ui.add(crate::theme::singleline(&mut dialog.name).hint_text("File name")).changed() {
                    dialog.overwrite = false;
                }
                crate::widgets::checkbox(ui, &mut dialog.overwrite, "Replace existing file");
            }
            if let Some(error) = &dialog.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            if ui.button(title).clicked() {
                submit = true;
            }
        });
    if let Some(window) = window {
        let r = window.response.rect;
        app.ui_rects.push((
            rysmo_math::DVec2::new(r.min.x as f64, r.min.y as f64),
            rysmo_math::DVec2::new(r.max.x as f64, r.max.y as f64),
        ));
    }
    if submit {
        match dialog.execute(app) {
            Ok(()) => open = false,
            Err(e) => dialog.error = Some(e),
        }
    }
    if open {
        app.portable_files = Some(dialog);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_round_trip_and_overwrite_confirmation_use_device_workspace() {
        let root = std::env::temp_dir().join(format!(
            "rysmo-mobile-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut app = App3d::new(rysmo_model::Editor::new());
        app.editor
            .apply(rysmo_model::Command::DrawLines {
                lines: vec![[rysmo_math::DVec3::ZERO, rysmo_math::DVec3::X]],
                facing: None,
            })
            .unwrap();
        let mut dialog = Dialog {
            operation: Operation::Save,
            root: root.clone(),
            name: "Test".into(),
            overwrite: false,
            error: None,
        };
        dialog.execute(&mut app).unwrap();
        let saved = app.editor.doc().clone();
        let bytes = std::fs::read(root.join("Test.rysmo")).unwrap();
        assert!(dialog.execute(&mut app).is_err());
        assert_eq!(std::fs::read(root.join("Test.rysmo")).unwrap(), bytes);
        app.editor.replace_document(rysmo_model::Document::new());
        dialog.operation = Operation::Open;
        dialog.execute(&mut app).unwrap();
        assert_eq!(app.editor.doc().root, saved.root);
        // The native writer intentionally rounds camera fields to its file precision.
        assert_eq!(rysmo_io::save(app.editor.doc()), String::from_utf8(bytes).unwrap());
        dialog.operation = Operation::Glb;
        dialog.execute(&mut app).unwrap();
        assert_eq!(&std::fs::read(root.join("Test.glb")).unwrap()[..4], b"glTF");
        dialog.operation = Operation::Dae;
        dialog.execute(&mut app).unwrap();
        assert!(std::fs::read_to_string(root.join("Test.dae")).unwrap().contains("<COLLADA"));
        dialog.operation = Operation::Dxf;
        dialog.execute(&mut app).unwrap();
        assert!(std::fs::read_to_string(root.join("Test.dxf")).unwrap().contains("LINE"));
        dialog.operation = Operation::Stl;
        dialog.execute(&mut app).unwrap();
        assert!(std::fs::read(root.join("Test.stl")).unwrap().starts_with(b"Rysmo STL"));
        // This model has lines only, and a 3MF package needs at least one face.
        dialog.operation = Operation::ThreeMf;
        assert_eq!(dialog.execute(&mut app), Err("There are no faces to export.".into()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn names_cannot_escape_the_workspace_and_extensions_are_explicit() {
        let root = Path::new("/tmp/rysmo-device-files");
        for name in ["", "..", "../model", "/model", "dir/file", "dir\\file", "x:y", "x\n"] {
            assert!(file_path(root, name, "rysmo").is_err(), "{name:?}");
        }
        assert_eq!(file_path(root, "model", "rysmo").unwrap(), root.join("model.rysmo"));
        assert!(file_path(root, "model.obj", "rysmo").is_err());
    }
}
