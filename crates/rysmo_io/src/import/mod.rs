//! Bounded model import. Unsupported records are reported rather than silently hidden.
mod dxf;
mod geometry;
mod glb;
mod obj;

use rysmo_model::Document;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpAxis {
    Y,
    Z,
}
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Metres per source unit. None uses file units, or metres when unspecified.
    pub unit_scale: Option<f64>,
    pub obj_up: UpAxis,
    pub curve_segments: u32,
}
impl Default for Options {
    fn default() -> Self {
        Self { unit_scale: None, obj_up: UpAxis::Y, curve_segments: 48 }
    }
}
#[derive(Debug, thiserror::Error)]
#[error("Cannot import model: {0}")]
pub struct ImportError(pub String);
pub type Result<T> = std::result::Result<T, ImportError>;
impl From<rysmo_mesh::MeshError> for ImportError {
    fn from(error: rysmo_mesh::MeshError) -> Self {
        Self(error.to_string())
    }
}
impl From<serde_json::Error> for ImportError {
    fn from(error: serde_json::Error) -> Self {
        Self(error.to_string())
    }
}
pub struct Imported {
    pub document: Document,
    pub warnings: Vec<String>,
}
fn error<T>(message: impl Into<String>) -> Result<T> {
    Err(ImportError(message.into()))
}
pub fn read(name: &str, bytes: &[u8], options: Options) -> Result<Imported> {
    if bytes.len() > 64 * 1024 * 1024 {
        return error("File exceeds the 64 MiB import limit.");
    }
    if options.unit_scale.is_some_and(|s| !s.is_finite() || !(1e-9..=1e9).contains(&s))
        || !(8..=256).contains(&options.curve_segments)
    {
        return error("Invalid import scale or curve detail.");
    }
    let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let mut imported = match extension.as_str() {
        "obj" => {
            obj::read(std::str::from_utf8(bytes).map_err(|_| ImportError("OBJ must be UTF-8 text.".into()))?, options)
        }
        "dxf" => dxf::read(
            std::str::from_utf8(bytes).map_err(|_| ImportError("Use an ASCII/UTF-8 DXF, not binary DXF.".into()))?,
            options,
        ),
        "glb" => glb::read(bytes, options),
        "rysmo" => Ok(Imported {
            document: crate::load(std::str::from_utf8(bytes).map_err(|_| ImportError("Invalid model text.".into()))?)
                .map_err(|e| ImportError(e.to_string()))?,
            warnings: Vec::new(),
        }),
        _ => error("Choose OBJ, GLB, DXF or Rysmo."),
    }?;
    validate_expansion(&imported.document)?;
    if imported.document.definition_bounds(None).is_none() {
        return error("The file contains no supported geometry.");
    }
    imported.document.fix_next_id().map_err(|e| ImportError(e.to_string()))?;
    Ok(imported)
}

#[cfg(test)]
mod tests;

pub(crate) fn validate_expansion(doc: &Document) -> Result<()> {
    rysmo_model::limits::validate_document(doc).map_err(ImportError)
}
