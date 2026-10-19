//! The interactive tools, the VCB and touch gestures, independent of any UI toolkit.
//!
//! [`ToolBox`] holds the active tool and its state. The app feeds it pointer and key
//! events with the camera's [`View`] and the cursor position; the tools work out
//! inferences, apply [`rysmo_model::Command`]s to the [`rysmo_model::Editor`], and
//! describe what to draw as a [`Preview`]. Nothing here knows about Bevy or egui, so the
//! tools can be driven by tests.

#![forbid(unsafe_code)]

pub mod gesture;
mod tools;
pub mod vcb;

pub use tools::{AnnotationBoxHits, Ctx, Key, Modifiers, Pointer, Preview, PreviewLine, ToolBox, ToolKind, box_lines};
pub use vcb::{VcbInput, parse_vcb};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests2;
#[cfg(test)]
mod tests_audit;
