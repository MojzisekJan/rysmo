//! Model conversion entry points for a separate WASM instance with no UI or GPU.
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

/// Validate a native file and prepare its initial camera outside the UI thread.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn open_native(text: &str) -> Result<String, String> {
    let doc = rysmo_io::load(text).map_err(|error| error.to_string())?;
    let mut camera = doc.camera.unwrap_or_default();
    if doc.camera.is_none()
        && let Some((low, high)) = doc.definition_bounds(None)
    {
        camera.frame(low, high);
    }
    serde_json::to_string(&(doc, camera)).map_err(|error| error.to_string())
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn import_model(
    name: &str,
    bytes: &[u8],
    unit_scale: Option<f64>,
    z_up: bool,
    curve_segments: u32,
) -> Result<String, String> {
    use rysmo_io::import::{Options, UpAxis};
    let options = Options { unit_scale, obj_up: if z_up { UpAxis::Z } else { UpAxis::Y }, curve_segments };
    let imported = rysmo_io::import::read(name, bytes, options).map_err(|error| error.to_string())?;
    serde_json::to_string(&(imported.document, imported.warnings)).map_err(|error| error.to_string())
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[must_use]
pub struct SkpOutput {
    bytes: Vec<u8>,
    warnings: String,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl SkpOutput {
    pub fn take_bytes(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.bytes)
    }

    pub fn warnings(&self) -> String {
        self.warnings.clone()
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn export_skp(document: &str, version: u32) -> Result<SkpOutput, String> {
    let version = match version {
        2017 => rysmo_io::skp::Version::V2017,
        2026 => rysmo_io::skp::Version::V2026,
        _ => return Err("Unsupported SKP export version.".into()),
    };
    let doc: rysmo_model::Document = serde_json::from_str(document).map_err(|error| error.to_string())?;
    // Export validates the complete document, including hierarchy and geometry budgets.
    let result = rysmo_io::skp::export(&doc, version)?;
    Ok(SkpOutput {
        bytes: result.bytes,
        warnings: serde_json::to_string(&result.warnings).map_err(|error| error.to_string())?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_transfer_retains_shared_components_and_exports_both_versions() {
        let bytes = include_bytes!("../../rysmo_io/tests/fixtures/skp/shared-2017.skp");
        let json = import_model("shared.skp", bytes, None, false, 48).unwrap();
        let (doc, warnings): (rysmo_model::Document, Vec<String>) = serde_json::from_str(&json).unwrap();
        assert!(!warnings.is_empty());
        assert_eq!(doc.root.instances.len(), 2);
        let instances: Vec<_> = doc.root.instances.values().collect();
        assert_eq!(instances[0].definition, instances[1].definition);
        for version in [2017, 2026] {
            let mut output = export_skp(&serde_json::to_string(&doc).unwrap(), version).unwrap();
            let again = rysmo_io::skp::read(&output.take_bytes(), Default::default()).unwrap();
            assert_eq!(again.document.root.instances.len(), 2);
            assert!(output.take_bytes().is_empty());
            let _: Vec<String> = serde_json::from_str(&output.warnings()).unwrap();
        }
    }

    #[test]
    fn native_open_preserves_document_and_rejects_invalid_files() {
        let text = include_str!("../../../testdata/dimensions-preview.rysmo");
        let expected = rysmo_io::load(text).unwrap();
        let (document, camera): (rysmo_model::Document, serde_json::Value) =
            serde_json::from_str(&open_native(text).unwrap()).unwrap();
        assert_eq!(rysmo_io::save(&document), rysmo_io::save(&expected));
        assert!(camera.is_object());
        if let Some(saved) = expected.camera {
            assert_eq!(camera, serde_json::to_value(saved).unwrap());
        }
        for invalid in [
            "{",
            r#"{"format":"rysmo","version":999}"#,
            r#"{"format":"rysmo","version":20,"edges":[{"id":1,"v":[2,3]}]}"#,
        ] {
            assert!(open_native(invalid).is_err());
        }
    }

    #[test]
    fn invalid_jobs_return_errors() {
        assert!(import_model("bad.skp", b"invalid", None, false, 48).is_err());
        assert!(import_model("bad.obj", b"", Some(f64::NAN), false, 48).is_err());
        assert!(export_skp("{}", 2017).is_err());
        assert!(export_skp("{}", 2025).is_err());
    }
}
