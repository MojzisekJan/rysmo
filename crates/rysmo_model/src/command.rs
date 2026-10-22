//! Commands: serialized editing operations and their execution.
//!
//! Data, labels, geometry helpers and group/transform operations have separate
//! modules. Dispatch retains common glue validation and definition cleanup;
//! the Editor owns the document snapshot and Undo transaction.

mod annotations;
mod drawing;
mod execute;
mod groups;
mod labels;
mod settings;
mod transforms;
mod types;

pub use drawing::{arc_points, centre_arc_points};
pub(crate) use groups::explode;
pub(crate) use transforms::{rotation, transform_items, validate_rotation};
pub use types::{Command, CommandError, MAX_GLOW, Outcome};
