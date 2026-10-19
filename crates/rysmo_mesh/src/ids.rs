//! Typed element ids. All kinds share one counter, so an id is unique in a document.

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub u64);

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(VertexId, "A vertex id.");
id_type!(CurveId, "A curve entity id.");
id_type!(EdgeId, "An edge id.");
id_type!(FaceId, "A face id.");
id_type!(MaterialId, "A material id, defined by the document.");
id_type!(DefinitionId, "A group or component definition id, defined by the document.");
id_type!(InstanceId, "A group or component instance id, defined by the document.");
id_type!(TagId, "A tag (layer) id, defined by the document.");
id_type!(TextId, "A text annotation id, defined by the document.");
id_type!(DimensionId, "A dimension entity id, defined by the document.");
id_type!(GuideId, "A construction guide id, defined by the document.");
id_type!(ImageId, "A texture image id, defined by the document.");

/// A selectable element. The mesh only knows edges and faces; instances and guides
/// belong to the document and the mesh ignores them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum Element {
    Edge(EdgeId),
    Face(FaceId),
    Instance(InstanceId),
    Guide(GuideId),
    Dimension(DimensionId),
    Text(TextId),
    Section(u64),
}
