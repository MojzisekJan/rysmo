//! User-selected binary files without changing the current model's Save handle.
use crate::state::App3d;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    Model,
    Collection,
    Photo,
    NormalMap(rysmo_model::MaterialId),
    Environment,
}
impl Purpose {
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Model => &["obj", "glb", "dxf", "rysmo"],
            Self::Collection => &["ryslib"],
            Self::Photo | Self::NormalMap(_) => &["png", "jpg", "jpeg"],
            Self::Environment => &["hdr", "png", "jpg", "jpeg"],
        }
    }
}
pub fn receive(app: &mut App3d, purpose: Purpose, name: String, bytes: Vec<u8>) {
    match purpose {
        Purpose::Photo => {
            let result = (|| -> Result<(), String> {
                let format = image::guess_format(&bytes).map_err(|e| e.to_string())?;
                if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
                    return Err("Choose a PNG or JPEG photograph.".into());
                }
                let (w, h) = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format)
                    .into_dimensions()
                    .map_err(|e| e.to_string())?;
                if w == 0 || h == 0 || w as u64 * h as u64 > 16_777_216 {
                    return Err("Photographs are limited to 16 million pixels.".into());
                }
                app.editor
                    .apply(rysmo_model::Command::ImportPhoto {
                        name,
                        mime: if format == image::ImageFormat::Png { "image/png" } else { "image/jpeg" }.into(),
                        data: bytes,
                        size: [w, h],
                    })
                    .map_err(|e| e.to_string())?;
                app.features.photo.open = true;
                Ok(())
            })();
            if let Err(error) = result {
                app.message = Some(error);
            }
        }
        Purpose::NormalMap(material) => {
            let result = (|| -> Result<(), String> {
                let format = image::guess_format(&bytes).map_err(|e| e.to_string())?;
                if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
                    return Err("Choose a PNG or JPEG normal map.".into());
                }
                let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
                let (width, height) = reader.into_dimensions().map_err(|e| e.to_string())?;
                if width == 0 || height == 0 || width as u64 * height as u64 > 16_777_216 {
                    return Err("Normal maps are limited to 16 million pixels.".into());
                }
                app.editor
                    .apply(rysmo_model::Command::AddNormalMap {
                        material,
                        name,
                        mime: if format == image::ImageFormat::Png { "image/png" } else { "image/jpeg" }.into(),
                        data: bytes,
                    })
                    .map_err(|e| e.to_string())?;
                Ok(())
            })();
            if let Err(error) = result {
                app.message = Some(error);
            }
        }
        Purpose::Environment => {
            let result = (|| -> Result<(), String> {
                let format = image::guess_format(&bytes).map_err(|e| e.to_string())?;
                let mime = match format {
                    image::ImageFormat::Hdr => "image/vnd.radiance",
                    image::ImageFormat::Png => "image/png",
                    image::ImageFormat::Jpeg => "image/jpeg",
                    _ => return Err("Choose a Radiance HDR, PNG or JPEG panorama.".into()),
                };
                let (width, height) = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format)
                    .into_dimensions()
                    .map_err(|e| e.to_string())?;
                if width < 8 || height < 4 || width as u64 * height as u64 > 33_554_432 {
                    return Err("Panoramas are limited to 32 million pixels.".into());
                }
                app.editor
                    .apply(rysmo_model::Command::SetEnvironment { name, mime: mime.into(), data: bytes })
                    .map_err(|e| e.to_string())?;
                Ok(())
            })();
            if let Err(error) = result {
                app.message = Some(error);
            }
        }
        Purpose::Model => app.features.import = Some(super::imports::Draft::new(name, bytes)),
        Purpose::Collection => match super::library::load(&bytes) {
            Ok(state) => match super::library::merge(app, state) {
                Ok(()) => app.message = Some(format!("Merged component collection {name}.")),
                Err(error) => app.message = Some(error),
            },
            Err(error) => app.message = Some(error),
        },
    }
}
pub fn pick(app: &mut App3d, purpose: Purpose) {
    #[cfg(target_arch = "wasm32")]
    crate::web::pick_data(
        purpose,
        crate::autosave::DocumentVersion { generation: app.document_generation, revision: app.editor.revision() },
    );
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        let Some(path) = rfd::FileDialog::new().add_filter("Import", purpose.extensions()).pick_file() else { return };
        let result = std::fs::metadata(&path).map_err(|e| e.to_string()).and_then(|metadata| {
            if metadata.len() > 64 * 1024 * 1024 {
                return Err("File exceeds the 64 MiB limit.".into());
            }
            std::fs::read(&path).map_err(|e| e.to_string())
        });
        match result {
            Ok(bytes) => {
                receive(app, purpose, path.file_name().unwrap_or_default().to_string_lossy().into_owned(), bytes)
            }
            Err(error) => app.message = Some(error),
        }
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    crate::portable_files::begin(app, crate::portable_files::Operation::DataImport(purpose));
}
pub fn safe_name(name: &str) -> String {
    let value: String = name.chars().filter(|c| !c.is_control() && !"/\\:".contains(*c)).take(120).collect();
    let value = value.trim().trim_matches('.');
    if value.is_empty() { "asset".into() } else { value.into() }
}
pub fn save(app: &mut App3d, name: &str, extension: &'static str, bytes: &[u8], mime: &str) -> bool {
    let name = format!("{}.{extension}", safe_name(name));
    #[cfg(target_arch = "wasm32")]
    {
        crate::web::download(&name, bytes, mime);
        app.message = Some(format!("Downloaded {name}."));
        true
    }
    #[cfg(not(any(target_arch = "wasm32", target_os = "android", target_os = "ios")))]
    {
        let _ = mime;
        let Some(mut path) =
            rfd::FileDialog::new().set_file_name(&name).add_filter(extension, &[extension]).save_file()
        else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension(extension);
        }
        match std::fs::write(&path, bytes) {
            Ok(()) => {
                app.message = Some(format!("Saved {}.", path.display()));
                true
            }
            Err(error) => {
                app.message = Some(error.to_string());
                false
            }
        }
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let _ = mime;
        crate::portable_files::save_data(app, name, extension, bytes.to_vec());
        false
    }
}
