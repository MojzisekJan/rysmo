//! Geometry context shared when annotations follow elements into a new group.
use crate::Document;
use rysmo_math::DMat4;
use rysmo_mesh::{DefinitionId, InstanceId, Mesh};

pub(crate) struct AnnotationReparent<'a> {
    pub doc: &'a Document,
    pub owner: Option<DefinitionId>,
    pub parent: Option<DefinitionId>,
    pub instance: InstanceId,
    pub source: &'a Mesh,
    pub target: &'a Mesh,
    pub transform: DMat4,
    pub children: &'a [InstanceId],
}
