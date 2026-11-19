//! Owns a cancellable conversion in an isolated browser worker.
use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};

#[wasm_bindgen(module = "/src/web_worker.js")]
extern "C" {
    fn start_import(name: &str, bytes: &[u8], scale: Option<f64>, z_up: bool, segments: u32) -> f64;
    fn start_export(document: &str, version: u32) -> f64;
    fn start_open(document: &str) -> f64;
    fn poll_job(id: f64) -> JsValue;
    fn cancel_job(id: f64);
}

pub struct Job(f64);
impl Job {
    pub fn open(text: &str) -> Self {
        Self(start_open(text))
    }

    pub fn poll_open(&self) -> Option<Result<(rysmo_model::Document, rysmo_math::OrbitCamera), String>> {
        self.poll().map(|result| {
            let value = result?;
            serde_json::from_str(&string_field(&value, "document")?).map_err(|error| error.to_string())
        })
    }

    pub fn import(name: &str, bytes: &[u8], options: rysmo_io::import::Options) -> Self {
        Self(start_import(
            name,
            bytes,
            options.unit_scale,
            options.obj_up == rysmo_io::import::UpAxis::Z,
            options.curve_segments,
        ))
    }

    pub fn export(doc: &rysmo_model::Document, version: rysmo_io::skp::Version) -> Result<Self, String> {
        let document = serde_json::to_string(doc).map_err(|error| error.to_string())?;
        let version = match version {
            rysmo_io::skp::Version::V2017 => 2017,
            rysmo_io::skp::Version::V2026 => 2026,
        };
        Ok(Self(start_export(&document, version)))
    }

    fn poll(&self) -> Option<Result<JsValue, String>> {
        let result = poll_job(self.0);
        if result.is_null() {
            return None;
        }
        Some(match js_sys::Reflect::get(&result, &"error".into()).ok().and_then(|v| v.as_string()) {
            Some(error) => Err(error),
            None => Ok(result),
        })
    }

    pub fn poll_import(&self) -> Option<Result<rysmo_io::import::Imported, String>> {
        self.poll().map(|result| {
            let value = result?;
            let json = string_field(&value, "document")?;
            let (document, warnings) = serde_json::from_str(&json).map_err(|error| error.to_string())?;
            Ok(rysmo_io::import::Imported { document, warnings })
        })
    }

    pub fn poll_export(&self) -> Option<Result<rysmo_io::skp::Export, String>> {
        self.poll().map(|result| {
            let value = result?;
            let warnings =
                serde_json::from_str(&string_field(&value, "warnings")?).map_err(|error| error.to_string())?;
            let bytes = js_sys::Reflect::get(&value, &"bytes".into()).map_err(|_| "Missing export bytes.")?;
            if !bytes.is_instance_of::<js_sys::Uint8Array>() {
                return Err("Invalid export bytes.".into());
            }
            Ok(rysmo_io::skp::Export { bytes: js_sys::Uint8Array::new(&bytes).to_vec(), warnings })
        })
    }
}

fn string_field(value: &JsValue, key: &str) -> Result<String, String> {
    js_sys::Reflect::get(value, &key.into())
        .ok()
        .and_then(|v| v.as_string())
        .ok_or_else(|| format!("Missing conversion field: {key}"))
}

impl Drop for Job {
    fn drop(&mut self) {
        cancel_job(self.0);
    }
}
