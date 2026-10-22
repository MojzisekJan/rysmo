//! Checked navigation through group and component instance paths.

use rysmo_mesh::{DefinitionId, InstanceId};

use crate::Document;

/// Why a group/component context path cannot be followed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ContextError {
    /// An instance is absent from its expected owner; `None` identifies the model.
    #[error("instance {instance} is missing from the editing context at path index {depth}")]
    MissingInstance { depth: usize, within: Option<DefinitionId>, instance: InstanceId },
    /// The instance exists but its referenced definition does not.
    #[error("instance {instance} at path index {depth} references missing definition {definition}")]
    MissingDefinition { depth: usize, instance: InstanceId, definition: DefinitionId },
}

impl Document {
    /// Resolves a context path to a definition, or `None` for the model itself.
    ///
    /// # Errors
    /// Returns [`ContextError`] with the failing path index and element IDs if an
    /// instance or its target definition is missing, including the final target.
    pub fn resolve(&self, path: &[InstanceId]) -> Result<Option<DefinitionId>, ContextError> {
        let mut within = None;
        let mut definition = &self.root;
        for (depth, &instance) in path.iter().enumerate() {
            let entry =
                definition.instances.get(&instance).ok_or(ContextError::MissingInstance { depth, within, instance })?;
            definition = self.definitions.get(&entry.definition).ok_or(ContextError::MissingDefinition {
                depth,
                instance,
                definition: entry.definition,
            })?;
            within = Some(entry.definition);
        }
        Ok(within)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Definition, Instance};
    use rysmo_math::DMat4;

    const OUTER: DefinitionId = DefinitionId(1);
    const INNER: DefinitionId = DefinitionId(2);
    const FIRST: InstanceId = InstanceId(3);
    const SECOND: InstanceId = InstanceId(4);
    const MISSING: InstanceId = InstanceId(99);

    fn instance(definition: DefinitionId) -> Instance {
        Instance {
            definition,
            transform: DMat4::IDENTITY,
            name: String::new(),
            tag: None,
            material: None,
            hidden: false,
            glued_to: None,
        }
    }

    fn nested() -> Document {
        let mut doc = Document::new();
        let outer = Definition { instances: [(SECOND, instance(INNER))].into(), ..Default::default() };
        doc.definitions.insert(OUTER, outer);
        doc.definitions.insert(INNER, Definition::default());
        doc.root.instances.insert(FIRST, instance(OUTER));
        doc
    }

    #[test]
    fn root_and_nested_paths_resolve_without_changing_the_document() {
        let doc = nested();
        let before = doc.clone();
        assert_eq!(doc.resolve(&[]), Ok(None));
        assert_eq!(doc.resolve(&[FIRST]), Ok(Some(OUTER)));
        assert_eq!(doc.resolve(&[FIRST, SECOND]), Ok(Some(INNER)));
        assert_eq!(doc, before);
    }

    #[test]
    fn missing_instances_report_the_exact_owner_and_path_index() {
        let doc = nested();
        assert_eq!(
            doc.resolve(&[MISSING]),
            Err(ContextError::MissingInstance { depth: 0, within: None, instance: MISSING })
        );
        assert_eq!(
            doc.resolve(&[FIRST, MISSING]),
            Err(ContextError::MissingInstance { depth: 1, within: Some(OUTER), instance: MISSING })
        );
    }

    #[test]
    fn a_missing_final_definition_is_an_error_even_for_a_single_instance() {
        let mut doc = nested();
        doc.definitions.remove(&OUTER);
        let error = doc.resolve(&[FIRST]).unwrap_err();
        assert_eq!(error, ContextError::MissingDefinition { depth: 0, instance: FIRST, definition: OUTER });
        assert!(error.to_string().contains("missing definition 1"));
    }

    #[test]
    fn nested_dangling_definitions_report_the_failing_instance() {
        let mut doc = nested();
        doc.definitions.remove(&INNER);
        assert_eq!(
            doc.resolve(&[FIRST, SECOND]),
            Err(ContextError::MissingDefinition { depth: 1, instance: SECOND, definition: INNER })
        );
        // Checking an earlier prefix remains valid when only its child is broken.
        assert_eq!(doc.resolve(&[FIRST]), Ok(Some(OUTER)));
    }
    #[test]
    fn an_invalid_intersection_context_preserves_document_history_and_selection() {
        use crate::{Command, Editor, Element};
        let mut doc = nested();
        doc.definitions.remove(&INNER);
        let mut editor = Editor::with_document(doc);
        editor.set_selection([Element::Instance(FIRST)]);
        let before = editor.doc().clone();
        let selection = editor.selected();
        let revision = editor.revision();
        let result = editor.apply(Command::IntersectFaces {
            elements: Vec::new(),
            with_model: true,
            context: vec![FIRST, SECOND],
        });
        let error = result.unwrap_err().to_string();
        assert!(error.contains("missing definition 2"), "{error}");
        assert_eq!(editor.doc(), &before);
        assert_eq!(editor.selected(), selection);
        assert_eq!(editor.undo_depth(), 0);
        assert_eq!(editor.revision(), revision);
        assert!(!editor.is_dirty());
    }
}
