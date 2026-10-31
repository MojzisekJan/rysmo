//! The edge-and-face mesh at the heart of Rysmo.
//!
//! The model contains vertices, edges between two vertices, and planar faces
//! bounded by an outer loop and any number of hole loops. Edges are first-class: an edge
//! may border no face (a stray line), one face, two faces, or more than two (where several
//! surfaces meet). A half-edge structure cannot hold the last case or the stray edges, so
//! faces store their loops as vertex lists and adjacency is looked up from the edges.
//!
//! Geometry is "sticky":
//!
//! - a line drawn across existing edges splits them where they cross, and is split there
//!   itself;
//! - a line that closes a planar loop creates a face;
//! - a line drawn across a face, from one boundary to another, splits the face in two;
//! - erasing the edge between two coplanar faces merges them.
//!
//! All of this goes through one routine, `Mesh::rebuild_plane`, which recomputes the
//! faces of one plane from the planar graph of its edges and keeps the identity, material
//! and facing of every face whose region did not change.
//!
//! Every element has a stable id from one counter, so an id names one element across the
//! whole document and the file format.

#![forbid(unsafe_code)]

pub mod blocks;
mod curves;
mod divide;
mod fillet;
mod flags;
mod follow;
mod ids;
mod insert;
mod mesh;
mod offset;
mod ops;
mod region;
pub mod spatial;
mod terrain_edit;
mod topology_tools;
mod transform;
mod triangulate;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests2;

pub use curves::{Curve, CurveKind};
pub use divide::EdgeDivision;
pub use fillet::Fillet;
pub use ids::{
    CurveId, DefinitionId, DimensionId, EdgeId, Element, FaceId, GuideId, ImageId, InstanceId, MaterialId, TagId,
    TextId, VertexId,
};
pub use mesh::{Edge, Face, Mesh, MeshError, TexturePlacement, Vertex};
pub use offset::offset_polyline;
pub use ops::PushPullResult;
pub use transform::Template;
pub use triangulate::triangulate_face;

/// Edges between faces that meet at less than this angle (degrees) are softened and
/// smoothed when push/pull or Follow Me makes them, so curved surfaces look round.
pub const SOFTEN_ANGLE_DEG: f64 = 25.0;
