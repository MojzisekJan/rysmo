//! Reading and writing files.
//!
//! - [`native`]: the `.rysmo` text format, JSON with one element per line and stable
//!   ids, described in `docs/file-format.md`.
//! - [`obj`]: Wavefront OBJ export with an MTL file.
//! - [`gltf`]: binary glTF (`.glb`) export.
//! - [`collada`]: COLLADA (`.dae`) with texture companions.
//! - [`dxf`]: ASCII DXF 3D faces and loose lines.
//! - [`print`]: STL and 3MF for 3D printing, and a printability check.
//!
//! Exports convert from Rysmo's Z-up coordinates to the Y-up convention of OBJ and
//! glTF and COLLADA. DXF, STL and 3MF retain Z-up coordinates.

#![forbid(unsafe_code)]

pub mod archive;
pub mod collada;
pub mod dxf;
mod exchange;
pub mod gltf;
pub mod native;
pub mod obj;
pub mod print;

pub use native::{load, save};

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("this is not a Rysmo file: {0}")]
    Format(String),
    #[error("the file is version {0}; this build reads up to version {max}", max = native::VERSION)]
    Version(u32),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Mesh(#[from] rysmo_mesh::MeshError),
}

/// Model (Z up) to export (Y up) coordinates.
pub(crate) fn y_up(p: rysmo_math::DVec3) -> [f32; 3] {
    [p.x as f32, p.z as f32, -p.y as f32]
}
