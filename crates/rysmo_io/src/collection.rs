//! Portable reusable component collections; documents use the normal native codec.
use rysmo_model::{Document, clipboard::Clipboard};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub description: String,
    /// Native model text, retaining nested definitions, images and editable properties.
    pub model: String,
}
impl Asset {
    pub fn from_clipboard(name: String, description: String, clipboard: &Clipboard) -> Self {
        Self { name, description, model: crate::save(&clipboard.document) }
    }
    pub fn document(&self) -> Result<Document, crate::IoError> {
        let document = crate::load(&self.model)?;
        crate::import::validate_expansion(&document).map_err(|e| crate::IoError::Format(e.to_string()))?;
        Ok(document)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Collection {
    format: String,
    version: u32,
    pub name: String,
    pub assets: Vec<Asset>,
}
impl Default for Collection {
    fn default() -> Self {
        Self { format: "rysmo-components".into(), version: 1, name: "My components".into(), assets: Vec::new() }
    }
}
impl Collection {
    pub fn validate(&self) -> Result<(), String> {
        if self.format != "rysmo-components" || self.version != 1 {
            return Err("Unsupported component collection format or version.".into());
        }
        if self.name.trim().is_empty() || self.assets.len() > 256 {
            return Err("A collection needs a name and at most 256 assets.".into());
        }
        let mut total = 0usize;
        for asset in &self.assets {
            if asset.name.trim().is_empty() || asset.name.len() > 512 || asset.description.len() > 8192 {
                return Err("Invalid component name or description.".into());
            }
            total = total.checked_add(asset.model.len()).ok_or("Collection size overflows.")?;
            if total > 32 * 1024 * 1024 {
                return Err("Component collection exceeds 32 MiB.".into());
            }
            let doc = asset.document().map_err(|e| format!("{}: {e}", asset.name))?;
            if doc.definition_bounds(None).is_none() {
                return Err(format!("{} contains no geometry.", asset.name));
            }
        }
        Ok(())
    }
    pub fn read(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 40 * 1024 * 1024 {
            return Err("Collection file exceeds 40 MiB.".into());
        }
        let collection: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        collection.validate()?;
        Ok(collection)
    }
    pub fn write(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        serde_json::to_vec_pretty(self).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rysmo_math::{DMat4, DVec3};
    use rysmo_model::{Command, Editor};
    #[test]
    fn collection_roundtrip_imports_into_another_project_with_atomic_history() {
        let mut source = Editor::new();
        source
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
            .unwrap();
        let face = source.mesh().faces().next().unwrap().0;
        let clipboard = Clipboard::capture(&source, vec![rysmo_model::Element::Face(face)]).unwrap();
        let mut collection = Collection::default();
        collection.assets.push(Asset::from_clipboard("Panel".into(), "One square metre".into(), &clipboard));
        let loaded = Collection::read(&collection.write().unwrap()).unwrap();
        let document = loaded.assets[0].document().unwrap();
        let mut target = Editor::new();
        let before = target.doc().clone();
        target
            .apply(Command::Paste {
                clipboard: Box::new(Clipboard { document, source_transform: DMat4::IDENTITY }),
                transform: DMat4::from_translation(DVec3::X * 2.),
                reuse_definitions: false,
            })
            .unwrap();
        assert!(target.doc().definition_bounds(None).is_some());
        assert_eq!(target.undo_depth(), 1);
        target.undo();
        assert_eq!(target.doc(), &before);
    }
    #[test]
    fn invalid_asset_and_newer_versions_are_rejected_before_installation() {
        let mut collection = Collection::default();
        collection.assets.push(Asset { name: "broken".into(), description: String::new(), model: "not JSON".into() });
        assert!(collection.write().is_err());
        assert!(
            Collection::read(br#"{"format":"rysmo-components","version":99,"name":"Future","assets":[]}"#).is_err()
        );
    }
}
