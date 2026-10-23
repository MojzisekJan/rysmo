//! Editing session, transactional commands, undo history and selection.

use std::collections::BTreeSet;

use rysmo_math::{DMat4, OrbitCamera};

use crate::{Command, CommandError, Definition, DefinitionId, Document, Element, InstanceId, Mesh, Outcome};
use crate::{command, dimensions, text};

/// One undo step: the document before a command, with the command's label.
#[derive(Clone, Debug)]
struct Step {
    label: String,
    before: Document,
    /// Whether the command itself moved the camera. Otherwise Undo/Redo keep the current
    /// view, which saves record in the document without adding history.
    camera: bool,
}

/// A document being edited: history, selection, the editing context, and a revision
/// counter that viewers use to know when to redraw.
///
/// Undo keeps whole document snapshots. That is simple and always correct, and cheap
/// for models of thousands of elements; storing diffs of changed elements is on the
/// roadmap.
#[derive(Clone, Debug, Default)]
pub struct Editor {
    doc: Document,
    undo: Vec<Step>,
    redo: Vec<Step>,
    selection: BTreeSet<Element>,
    /// The groups and components being edited, outermost first. Empty: the model.
    context: Vec<InstanceId>,
    revision: u64,
    /// Changes since the last save.
    dirty: bool,
}

/// How many steps the history keeps.
pub const UNDO_LIMIT: usize = 200;

impl Editor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_document(doc: Document) -> Self {
        Editor { doc, ..Default::default() }
    }

    pub fn doc(&self) -> &Document {
        &self.doc
    }

    /// The definition being edited (`None`: the model itself).
    pub fn active_definition(&self) -> Option<DefinitionId> {
        self.doc.resolve(&self.context).unwrap_or(None)
    }

    /// The definition being edited.
    pub fn active(&self) -> &Definition {
        self.doc.def(self.active_definition()).unwrap_or(&self.doc.root)
    }

    /// The mesh being edited.
    pub fn mesh(&self) -> &Mesh {
        &self.active().mesh
    }

    pub fn context(&self) -> &[InstanceId] {
        &self.context
    }

    /// Predict selected dimension placement without changing the document or history.
    pub fn preview_text_rotation(
        &self,
        elements: &[Element],
        centre: rysmo_math::DVec3,
        axis: rysmo_math::DVec3,
        angle: f64,
        copy: bool,
    ) -> Result<Vec<text::Text>, CommandError> {
        command::validate_rotation(centre, axis, angle)?;
        if !copy {
            return self.preview_text_transform(elements, command::rotation(centre, axis, angle));
        }
        if !elements.iter().any(|e| matches!(e, Element::Text(_))) {
            return Ok(Vec::new());
        }
        let owner = self.active_definition();
        let mut doc = self.doc.clone();
        doc.refresh_texts();
        let out =
            Command::Rotate { elements: elements.to_vec(), centre, axis, angle, copies: 1 }.run(&mut doc, owner)?;
        Ok(out
            .created
            .iter()
            .filter_map(|e| {
                if let Element::Text(id) = e {
                    doc.def(owner)?.texts.get(id).map(|text| text.detached(&doc, owner))
                } else {
                    None
                }
            })
            .collect())
    }
    pub fn preview_text_transform(&self, elements: &[Element], matrix: DMat4) -> Result<Vec<text::Text>, CommandError> {
        if !elements.iter().any(|e| matches!(e, Element::Text(_))) {
            return Ok(Vec::new());
        }
        let owner = self.active_definition();
        let mut doc = self.doc.clone();
        doc.refresh_texts();
        command::transform_items(&mut doc, owner, elements, matrix)?;
        Ok(elements
            .iter()
            .filter_map(|e| {
                if let Element::Text(id) = e {
                    doc.def(owner)?.texts.get(id).map(|text| text.detached(&doc, owner))
                } else {
                    None
                }
            })
            .collect())
    }
    pub fn preview_text_copy(
        &self,
        elements: &[Element],
        offset: rysmo_math::DVec3,
        pivot: rysmo_math::DVec3,
        view: rysmo_math::View,
    ) -> Result<Vec<text::Text>, CommandError> {
        if !elements.iter().any(|e| matches!(e, Element::Text(_))) {
            return Ok(Vec::new());
        }
        let owner = self.active_definition();
        let mut doc = self.doc.clone();
        doc.refresh_texts();
        let out = Command::CopyWithText { elements: elements.to_vec(), offset, copies: 1, pivot, view }
            .run(&mut doc, owner)?;
        Ok(out
            .texts
            .iter()
            .filter_map(|id| doc.def(owner)?.texts.get(id).map(|text| text.detached(&doc, owner)))
            .collect())
    }
    pub fn preview_text_move(
        &self,
        elements: &[Element],
        offset: rysmo_math::DVec3,
        pivot: rysmo_math::DVec3,
        view: rysmo_math::View,
    ) -> Result<Vec<text::Text>, CommandError> {
        if !elements.iter().any(|e| matches!(e, Element::Text(_))) {
            return Ok(Vec::new());
        }
        let owner = self.active_definition();
        let mut doc = self.doc.clone();
        doc.refresh_texts();
        Command::MoveWithText { elements: elements.to_vec(), offset, pivot, view }.run(&mut doc, owner)?;
        Ok(elements
            .iter()
            .filter_map(|e| {
                if let Element::Text(id) = e {
                    doc.def(owner)?.texts.get(id).map(|text| text.detached(&doc, owner))
                } else {
                    None
                }
            })
            .collect())
    }

    pub fn preview_dimension_move(
        &self,
        elements: &[Element],
        offset: rysmo_math::DVec3,
        copy: bool,
    ) -> Result<Vec<dimensions::Dimension>, CommandError> {
        let ids = elements
            .iter()
            .filter_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None })
            .collect::<Vec<_>>();
        if ids.is_empty() || offset.length() < rysmo_math::EPS {
            return Ok(Vec::new());
        }
        let owner = self.active_definition();
        if elements.iter().all(|e| matches!(e, Element::Dimension(_))) {
            return ids
                .iter()
                .map(|id| {
                    let source = self
                        .active()
                        .dimensions
                        .get(id)
                        .ok_or_else(|| CommandError::Invalid("dimension not found in this context".into()))?;
                    let mut dimension = source.detached(&self.doc, owner);
                    dimension.offset += offset;
                    if !dimension.validate() {
                        return Err(CommandError::Invalid("dimension placement must stay in its plane".into()));
                    }
                    Ok(dimension)
                })
                .collect();
        }
        let mut doc = self.doc.clone();
        doc.refresh_dimensions();
        let command = if copy {
            Command::Copy { elements: elements.to_vec(), offset, copies: 1 }
        } else {
            Command::Move { elements: elements.to_vec(), offset }
        };
        let result = command.run(&mut doc, owner)?;
        doc.refresh_dimensions();
        let ids = if copy {
            result
                .created
                .iter()
                .filter_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None })
                .collect()
        } else {
            ids
        };
        Ok(ids.iter().filter_map(|id| doc.def(owner)?.dimensions.get(id).map(|d| d.detached(&doc, owner))).collect())
    }

    /// Predict selected rotated annotations without reserving real IDs or history.
    pub fn preview_dimension_rotate(
        &self,
        elements: &[Element],
        centre: rysmo_math::DVec3,
        axis: rysmo_math::DVec3,
        angle: f64,
        copy: bool,
    ) -> Result<Vec<dimensions::Dimension>, CommandError> {
        let ids = elements
            .iter()
            .filter_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None })
            .collect::<Vec<_>>();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        command::validate_rotation(centre, axis, angle)?;
        if angle.abs() < 1e-9 {
            return Ok(Vec::new());
        }
        let owner = self.active_definition();
        if elements.iter().all(|e| matches!(e, Element::Dimension(_))) {
            let matrix = command::rotation(centre, axis, angle);
            return ids
                .iter()
                .map(|id| {
                    let source = self
                        .active()
                        .dimensions
                        .get(id)
                        .ok_or_else(|| CommandError::Invalid("dimension not found in this context".into()))?;
                    let dimension = source.detached(&self.doc, owner).mapped(matrix);
                    if !dimension.validate() {
                        return Err(CommandError::Invalid("rotated dimension has invalid placement".into()));
                    }
                    Ok(dimension)
                })
                .collect();
        }
        let mut doc = self.doc.clone();
        doc.refresh_dimensions();
        let result =
            Command::Rotate { elements: elements.to_vec(), centre, axis, angle, copies: if copy { 1 } else { 0 } }
                .run(&mut doc, owner)?;
        doc.refresh_dimensions();
        let ids = if copy {
            result
                .created
                .iter()
                .filter_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None })
                .collect()
        } else {
            ids
        };
        Ok(ids.iter().filter_map(|id| doc.def(owner)?.dimensions.get(id).map(|d| d.detached(&doc, owner))).collect())
    }

    /// Predict Scale in the editing axes without changing the real model.
    pub fn preview_dimension_scale(
        &self,
        elements: &[Element],
        origin: rysmo_math::DVec3,
        factors: rysmo_math::DVec3,
        basis: DMat4,
    ) -> Result<Vec<dimensions::Dimension>, CommandError> {
        let ids = elements
            .iter()
            .filter_map(|e| if let Element::Dimension(id) = e { Some(*id) } else { None })
            .collect::<Vec<_>>();
        if ids.is_empty() || factors.distance(rysmo_math::DVec3::ONE) < 1e-12 {
            return Ok(Vec::new());
        }
        if !origin.is_finite()
            || !factors.is_finite()
            || factors.abs().min_element() < 1e-9
            || !basis.is_finite()
            || !basis.inverse().is_finite()
        {
            return Err(CommandError::Invalid("scale needs finite, invertible axes and nonzero factors".into()));
        }
        let basis =
            rysmo_math::DrawingAxes::new(rysmo_math::DVec3::ZERO, basis.x_axis.truncate(), basis.y_axis.truncate())
                .ok_or_else(|| CommandError::Invalid("invalid scale axes".into()))?
                .basis();
        let owner = self.active_definition();
        if elements.iter().all(|e| matches!(e, Element::Dimension(_))) {
            let matrix = DMat4::from_translation(origin)
                * basis
                * DMat4::from_scale(factors)
                * basis.inverse()
                * DMat4::from_translation(-origin);
            return ids
                .iter()
                .map(|id| {
                    let source = self
                        .active()
                        .dimensions
                        .get(id)
                        .ok_or_else(|| CommandError::Invalid("dimension not found in this context".into()))?;
                    let dimension = source.detached(&self.doc, owner).mapped(matrix);
                    if !dimension.validate() {
                        return Err(CommandError::Invalid("scaled dimension has invalid placement".into()));
                    }
                    Ok(dimension)
                })
                .collect();
        }
        let mut doc = self.doc.clone();
        doc.refresh_dimensions();
        Command::ScaleInAxes {
            elements: elements.to_vec(),
            origin,
            factors,
            x: basis.x_axis.truncate(),
            y: basis.y_axis.truncate(),
        }
        .run(&mut doc, owner)?;
        doc.refresh_dimensions();
        Ok(ids.iter().filter_map(|id| doc.def(owner)?.dimensions.get(id).map(|d| d.detached(&doc, owner))).collect())
    }

    pub fn preview_instance_texts(
        &self,
        elements: &[Element],
        matrix: DMat4,
    ) -> Result<Vec<(text::Text, DMat4)>, CommandError> {
        if !matrix.is_finite() || !matrix.inverse().is_finite() {
            return Err(CommandError::Invalid("preview transform must be finite and invertible".into()));
        }
        let instances = elements
            .iter()
            .filter_map(|e| {
                if let Element::Instance(id) = e {
                    self.active().instances.contains_key(id).then_some(*id)
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        if instances.is_empty() || matrix == DMat4::IDENTITY {
            return Ok(Vec::new());
        }
        let inverse = self.context_transform().inverse();
        let mut result = Vec::new();
        for occurrence in self.doc.occurrences(self.context()) {
            if !occurrence.in_context
                || !occurrence.path.get(self.context.len()).is_some_and(|id| instances.contains(id))
            {
                continue;
            }
            if let Some(definition) = self.doc.def(occurrence.definition) {
                for text in definition.texts.values() {
                    result.push((
                        text.detached(&self.doc, occurrence.definition),
                        matrix * inverse * occurrence.transform,
                    ));
                }
            }
        }
        Ok(result)
    }

    /// Annotation previews within selected child instances, with owner-to-context transforms.
    pub fn preview_instance_dimensions(
        &self,
        elements: &[Element],
        matrix: DMat4,
    ) -> Result<Vec<(dimensions::Dimension, DMat4)>, CommandError> {
        if !matrix.is_finite() || !matrix.inverse().is_finite() {
            return Err(CommandError::Invalid("preview transform must be finite and invertible".into()));
        }
        let instances = elements
            .iter()
            .filter_map(|e| {
                if let Element::Instance(id) = e {
                    self.active().instances.contains_key(id).then_some(*id)
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        if instances.is_empty() || matrix == DMat4::IDENTITY {
            return Ok(Vec::new());
        }
        let inverse = self.context_transform().inverse();
        let mut previews = Vec::new();
        for occurrence in self.doc.occurrences(self.context()) {
            if !occurrence.in_context
                || !occurrence.path.get(self.context.len()).is_some_and(|id| instances.contains(id))
            {
                continue;
            }
            let transform = matrix * inverse * occurrence.transform;
            let Some(definition) = self.doc.def(occurrence.definition) else { continue };
            for dimension in definition.dimensions.values() {
                previews.push((dimension.detached(&self.doc, occurrence.definition), transform));
            }
        }
        Ok(previews)
    }

    /// Coordinates of the context being edited to world coordinates.
    pub fn context_transform(&self) -> DMat4 {
        self.doc.path_transform(&self.context)
    }

    /// Increases whenever the document, the selection or the context changes.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    /// Marks unsaved changes (a document brought back from an autosave).
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Replaces the document (opening a file), clearing history, selection and context.
    pub fn replace_document(&mut self, doc: Document) {
        *self = Editor { doc, revision: self.revision + 1, ..Default::default() };
    }

    /// Runs a command in the current context as one undo step. On error the document is
    /// left as it was.
    pub fn apply(&mut self, cmd: Command) -> Result<Outcome, CommandError> {
        let before = self.doc.clone();
        self.doc.refresh_dimensions();
        self.doc.refresh_texts();
        let label = cmd.label().to_string();
        let def = self.active_definition();
        let reselect_curve = if let Command::SetCurveSegments { curve, .. } = &cmd {
            self.selection.iter().any(|element| matches!(element, Element::Edge(edge) if self.mesh().edge(*edge).is_some_and(|e| e.curve==Some(*curve))))
        } else {
            false
        };
        match cmd.run(&mut self.doc, def) {
            Ok(outcome) => {
                if outcome.changed {
                    self.doc.refresh_dimensions();
                    self.doc.refresh_texts();
                    let camera = before.camera != self.doc.camera;
                    self.undo.push(Step { label, before, camera });
                    if self.undo.len() > UNDO_LIMIT {
                        self.undo.remove(0);
                    }
                    self.redo.clear();
                    self.dirty = true;
                    self.fix_context();
                    self.prune_selection();
                    if reselect_curve {
                        self.selection.extend(outcome.created.iter().copied());
                    }
                    self.revision += 1;
                } else {
                    self.doc = before;
                }
                Ok(outcome)
            }
            Err(e) => {
                self.doc = before;
                Err(e)
            }
        }
    }

    /// Replaces the most recent operation atomically, retaining its original Undo baseline.
    pub fn amend_last(&mut self, cmd: Command) -> Result<Outcome, CommandError> {
        let step = self.undo.last().ok_or_else(|| CommandError::Invalid("There is no operation to correct.".into()))?;
        let mut document = step.before.clone();
        let label = cmd.label().to_string();
        document.refresh_dimensions();
        document.refresh_texts();
        let outcome = cmd.run(&mut document, self.active_definition())?;
        if !outcome.changed {
            self.doc = self.undo.last().unwrap().before.clone();
            self.undo.pop();
            self.redo.clear();
            self.dirty = true;
            self.revision += 1;
            self.fix_context();
            self.prune_selection();
            return Ok(Outcome { changed: true, ..outcome });
        }
        document.refresh_dimensions();
        document.refresh_texts();
        self.doc = document;
        self.undo.last_mut().unwrap().label = label;
        self.redo.clear();
        self.dirty = true;
        self.revision += 1;
        self.fix_context();
        self.prune_selection();
        Ok(outcome)
    }

    /// Number of operations currently available to Undo.
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Label of the step `undo` would revert.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn undo(&mut self) -> bool {
        let Some(Step { label, before, camera }) = self.undo.pop() else { return false };
        let after = self.restore(before, camera);
        self.redo.push(Step { label, before: after, camera });
        self.after_history_move();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(Step { label, before: after, camera }) = self.redo.pop() else { return false };
        let before = self.restore(after, camera);
        self.undo.push(Step { label, before, camera });
        self.after_history_move();
        true
    }

    /// Installs a history snapshot and returns the replaced document.
    fn restore(&mut self, mut snapshot: Document, camera: bool) -> Document {
        if !camera {
            snapshot.camera = self.doc.camera;
        }
        std::mem::replace(&mut self.doc, snapshot)
    }

    fn after_history_move(&mut self) {
        self.dirty = true;
        self.fix_context();
        self.prune_selection();
        self.revision += 1;
    }

    /// Shortens the context to the part that still exists.
    fn fix_context(&mut self) {
        while !self.context.is_empty()
            && (self.doc.resolve(&self.context).is_err()
                || !self.doc.occurrences(&self.context).iter().any(|occurrence| occurrence.path == self.context))
        {
            self.context.pop();
        }
    }

    // ----- editing context -----

    /// Opens a group or component of the current context for editing (double click
    /// with Select). A group shared with copies is made unique first,
    /// which is an undo step.
    pub fn enter(&mut self, instance: InstanceId) -> Result<(), CommandError> {
        let inst = self
            .active()
            .instances
            .get(&instance)
            .ok_or_else(|| CommandError::Invalid(format!("no group or component {instance} here")))?
            .clone();
        if inst.hidden || !self.doc.tag_visible(inst.tag) {
            return Err(CommandError::Invalid("Show the group/component and its tag before editing it.".into()));
        }
        let def = self
            .doc
            .definitions
            .get(&inst.definition)
            .ok_or_else(|| CommandError::Invalid("broken instance".into()))?;
        if def.group && self.doc.instance_count(inst.definition) > 1 {
            self.apply(Command::MakeUnique { instance })?;
        }
        self.context.push(instance);
        self.selection.clear();
        self.revision += 1;
        Ok(())
    }

    /// Closes the innermost open group or component. Returns false at the top.
    pub fn exit(&mut self) -> bool {
        let Some(i) = self.context.pop() else { return false };
        self.selection = BTreeSet::from([Element::Instance(i)]);
        self.revision += 1;
        true
    }

    pub fn exit_all(&mut self) {
        if !self.context.is_empty() {
            self.context.clear();
            self.selection.clear();
            self.revision += 1;
        }
    }

    // ----- selection (not part of the history) -----

    pub fn selection(&self) -> &BTreeSet<Element> {
        &self.selection
    }

    pub fn selected(&self) -> Vec<Element> {
        self.selection.iter().copied().collect()
    }

    pub fn is_selected(&self, e: Element) -> bool {
        self.selection.contains(&e)
    }

    /// Whether an element exists in the current context.
    pub fn contains(&self, e: Element) -> bool {
        let d = self.active();
        match e {
            Element::Instance(i) => d.instances.contains_key(&i),
            Element::Guide(g) => d.guides.contains_key(&g),
            Element::Dimension(id) => d.dimensions.contains_key(&id),
            Element::Text(id) => d.texts.contains_key(&id),
            Element::Section(id) => d.sections.planes.contains_key(&id),
            _ => d.mesh.contains(e),
        }
    }

    pub fn set_selection(&mut self, elements: impl IntoIterator<Item = Element>) {
        self.selection = elements.into_iter().filter(|e| self.contains(*e)).collect();
        self.revision += 1;
    }

    pub fn clear_selection(&mut self) {
        if !self.selection.is_empty() {
            self.selection.clear();
            self.revision += 1;
        }
    }

    pub fn add_to_selection(&mut self, elements: impl IntoIterator<Item = Element>) {
        for e in elements {
            if self.contains(e) {
                self.selection.insert(e);
            }
        }
        self.revision += 1;
    }

    pub fn toggle_selection(&mut self, elements: impl IntoIterator<Item = Element>) {
        for e in elements {
            if !self.selection.remove(&e) && self.contains(e) {
                self.selection.insert(e);
            }
        }
        self.revision += 1;
    }

    pub fn remove_from_selection(&mut self, elements: impl IntoIterator<Item = Element>) {
        for e in elements {
            self.selection.remove(&e);
        }
        self.revision += 1;
    }

    /// Drops what no longer exists or can no longer be seen: a hidden instance, or one on
    /// a hidden tag. Neither is drawn, even with hidden geometry shown.
    fn prune_selection(&mut self) {
        let (doc, active) = (&self.doc, self.active());
        let keep: BTreeSet<Element> = self
            .selection
            .iter()
            .copied()
            .filter(|e| match e {
                Element::Instance(i) => {
                    active.instances.get(i).is_some_and(|inst| !inst.hidden && doc.tag_visible(inst.tag))
                }
                _ => self.contains(*e),
            })
            .collect();
        self.selection = keep;
    }

    /// Camera to remember in the file; not an undo step.
    pub fn set_camera(&mut self, camera: OrbitCamera) {
        self.doc.camera = Some(camera);
    }
}

#[cfg(test)]
mod shared_history_tests {
    use super::*;
    use rysmo_math::DVec3;
    use std::sync::Arc;

    #[test]
    fn undo_steps_share_definitions_an_edit_leaves_alone() {
        let mut editor = Editor::new();
        editor
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: Some(DVec3::Z) })
            .unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        editor.apply(Command::MakeGroup { elements: vec![Element::Face(face)] }).unwrap();
        let group = *editor.doc().definitions.keys().next().unwrap();
        editor
            .apply(Command::DrawRectangle {
                corner: DVec3::new(3., 0., 0.),
                u: DVec3::X,
                v: DVec3::Y,
                facing: Some(DVec3::Z),
            })
            .unwrap();
        // The drawing changed only the model's own geometry; the group is one shared copy.
        let before = &editor.undo.last().unwrap().before;
        assert!(Arc::ptr_eq(&before.definitions[&group], &editor.doc().definitions[&group]));
        assert!(!Arc::ptr_eq(&before.root, &editor.doc().root));
        editor.undo();
        assert_eq!(editor.mesh().face_count(), 0);
    }
}
