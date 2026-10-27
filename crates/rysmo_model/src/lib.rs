//! The Rysmo document and the commands that change it.
//!
//! [`Editor`] is a headless editing session: a [`Document`], an undo history, a
//! selection, and the context being edited (a path into groups and components). Every
//! change goes through [`Editor::apply`] with a [`Command`], which is plain data
//! (serde), so the interactive tools, scripts, tests and the MCP server all drive
//! the model the same way. Nothing here knows about Bevy or egui.

#![forbid(unsafe_code)]

mod annotation_reparent;
pub mod arcs;
pub mod clipboard;
pub mod command;
pub mod components;
mod context;
pub mod contours;
pub mod dimensions;
mod document;
mod editor;
mod intersect;
mod navigation;
pub mod parametric;
pub mod photo;
pub mod scenes;
pub mod sections;
mod solid_bsp;
mod solid_validation;
pub mod solids;
mod style;
pub mod terrain;
pub mod terrain_footprint;
pub mod terrain_region;
pub mod terrain_surface;
pub mod text;
pub mod text3d;
mod texture;
mod units;
mod volume;

pub use rysmo_mesh::{
    DefinitionId, DimensionId, EdgeId, Element, FaceId, GuideId, InstanceId, MaterialId, Mesh, MeshError, TagId,
    TextId, VertexId,
};

pub use command::{Command, CommandError, ENVIRONMENT_MIMES, MAX_GLOW, Outcome};
pub use context::ContextError;
pub use document::{Definition, Document, Guide, Instance, Material, Occurrence, Pbr, Tag, corners};
pub use editor::{Editor, UNDO_LIMIT};
pub use navigation::SurfaceHit;
pub use rysmo_mesh::{ImageId, TexturePlacement};
pub use style::{
    AntiAliasing, EdgeColor, FaceStyle, Lighting, ShadowQuality, StudioEnvironment, Style, Sun, ToneMapping,
};
pub use texture::{
    Image, Texture, moved as moved_placement, pin_mapping, point_at_uv, texture_axes,
    transformed as transformed_placement, uv_at, valid_placement, wrapped_placements,
};
pub use units::{
    MeasurementSettings, ParseLengthError, Units, format_length, format_length_with_precision, parse_length,
};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests2;

#[cfg(test)]
mod divide_tests;
