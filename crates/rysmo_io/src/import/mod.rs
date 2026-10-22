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
    imported.document.fix_next_id();
    Ok(imported)
}

#[cfg(test)]
mod tests;

/// Shared definitions may expand exponentially even when the file itself is small.
pub(crate) fn validate_expansion(doc: &Document) -> Result<()> {
    use std::collections::{BTreeMap, BTreeSet};
    type Counts = (usize, usize, usize, usize);
    fn visit(
        doc: &Document,
        id: Option<rysmo_model::DefinitionId>,
        path: &mut BTreeSet<Option<rysmo_model::DefinitionId>>,
        memo: &mut BTreeMap<Option<rysmo_model::DefinitionId>, Counts>,
    ) -> Result<Counts> {
        if path.len() >= 64 || path.contains(&id) {
            return error("Component hierarchy is cyclic or exceeds 64 levels.");
        }
        if let Some(count) = memo.get(&id) {
            if path.len() + count.3 > 64 {
                return error("Component hierarchy exceeds 64 levels.");
            }
            return Ok(*count);
        }
        path.insert(id);
        let definition = doc.def(id).ok_or_else(|| ImportError("Missing component definition".into()))?;
        let mut counts = (1usize, definition.mesh.face_count(), definition.mesh.vertex_count(), 1usize);
        for instance in definition.instances.values() {
            let child = visit(doc, Some(instance.definition), path, memo)?;
            counts.0 = counts.0.saturating_add(child.0);
            counts.1 = counts.1.saturating_add(child.1);
            counts.2 = counts.2.saturating_add(child.2);
            counts.3 = counts.3.max(child.3 + 1);
            if counts.0 > 100_000 || counts.1 > 500_000 || counts.2 > 1_000_000 {
                return error("Expanded model exceeds 100,000 occurrences, 500,000 faces or one million vertices.");
            }
        }
        if counts.0 > 100_000 || counts.1 > 500_000 || counts.2 > 1_000_000 {
            return error("Expanded model exceeds the geometry budget.");
        }
        path.remove(&id);
        memo.insert(id, counts);
        Ok(counts)
    }
    visit(doc, None, &mut BTreeSet::new(), &mut BTreeMap::new())?;
    Ok(())
}
