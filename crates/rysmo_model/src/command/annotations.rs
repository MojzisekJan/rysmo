//! Text and dimension commands with checked context and record lookups.

#![deny(clippy::unwrap_used, clippy::expect_used)]

use super::types::{CommandError, Outcome, invalid};
use crate::{Definition, Document};
use rysmo_math::DMat4;
use rysmo_mesh::{DefinitionId, Element};
use std::collections::BTreeMap;

fn definition(doc: &Document, def: Option<DefinitionId>) -> Result<&Definition, CommandError> {
    doc.def(def).ok_or_else(|| CommandError::Invalid("the context being edited no longer exists".into()))
}
fn definition_mut(doc: &mut Document, def: Option<DefinitionId>) -> Result<&mut Definition, CommandError> {
    doc.def_mut(def).ok_or_else(|| CommandError::Invalid("the context being edited no longer exists".into()))
}
pub(super) fn apply_text_defaults(
    doc: &mut Document,
    def: Option<DefinitionId>,
    out: &mut Outcome,
    ids: Vec<rysmo_mesh::TextId>,
    camera: rysmo_math::OrbitCamera,
    viewport: rysmo_math::DVec2,
    transform: DMat4,
) -> Result<(), CommandError> {
    let defaults = doc.style.text_defaults;
    if !defaults.validate()
        || !viewport.is_finite()
        || viewport.min_element() <= 0.
        || !transform.is_finite()
        || !transform.inverse().is_finite()
    {
        return invalid("invalid text update view");
    }
    let view = camera.view(viewport);
    if !view.view.is_finite() || !view.proj.is_finite() {
        return invalid("invalid text update camera");
    }
    let mut updates = BTreeMap::new();
    for id in ids {
        let original =
            definition(doc, def)?.texts.get(&id).ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))?;
        let mut text = if matches!(original.placement, crate::text::TextPlacement::Attached { .. }) {
            original
                .with_leader(doc, def, transform, &view, defaults.leader)
                .ok_or_else(|| CommandError::Invalid("cannot convert a selected text leader in this view".into()))?
        } else {
            original.clone()
        };
        defaults.apply_appearance(&mut text);
        updates.insert(id, text);
    }
    out.changed = false;
    for (id, text) in updates {
        let previous = definition_mut(doc, def)?
            .texts
            .get_mut(&id)
            .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))?;
        out.changed |= *previous != text;
        *previous = text;
    }
    Ok(())
}

pub(super) fn apply_text_appearance(
    doc: &mut Document,
    def: Option<DefinitionId>,
    out: &mut Outcome,
    ids: Vec<rysmo_mesh::TextId>,
) -> Result<(), CommandError> {
    let defaults = doc.style.text_defaults;
    let definition = definition_mut(doc, def)?;
    if ids.iter().any(|id| !definition.texts.contains_key(id)) {
        return invalid("text not found in this context");
    }
    out.changed = false;
    for id in ids {
        let text = definition.texts.get_mut(&id).ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))?;
        let before = text.clone();
        defaults.apply_appearance(text);
        out.changed |= *text != before;
    }
    Ok(())
}

pub(super) fn add_text(
    doc: &mut Document,
    def: Option<DefinitionId>,
    out: &mut Outcome,
    text: crate::text::Text,
) -> Result<(), CommandError> {
    if !text.validate() {
        return invalid("invalid text annotation");
    }
    let id = rysmo_mesh::TextId(doc.reserve_id());
    definition_mut(doc, def)?.texts.insert(id, text);
    out.texts.push(id);
    out.created.push(Element::Text(id));
    Ok(())
}

pub(super) fn set_text(
    doc: &mut Document,
    def: Option<DefinitionId>,
    out: &mut Outcome,
    id: rysmo_mesh::TextId,
    text: crate::text::Text,
) -> Result<(), CommandError> {
    if !text.validate() {
        return invalid("invalid text annotation");
    }
    let previous = definition_mut(doc, def)?
        .texts
        .get_mut(&id)
        .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))?;
    out.changed = *previous != text;
    *previous = text;
    Ok(())
}

pub(super) fn erase_texts(
    doc: &mut Document,
    def: Option<DefinitionId>,
    out: &mut Outcome,
    ids: Vec<rysmo_mesh::TextId>,
) -> Result<(), CommandError> {
    let d = definition_mut(doc, def)?;
    if ids.iter().any(|id| !d.texts.contains_key(id)) {
        return invalid("text not found in this context");
    }
    out.changed = !ids.is_empty();
    for id in ids {
        d.texts.remove(&id);
    }
    Ok(())
}

pub(super) fn add_dimension(
    doc: &mut Document,
    def: Option<DefinitionId>,
    out: &mut Outcome,
    dimension: crate::dimensions::Dimension,
) -> Result<(), CommandError> {
    if !dimension.validate() {
        return invalid("invalid dimension geometry");
    }
    let id = rysmo_mesh::DimensionId(doc.reserve_id());
    definition_mut(doc, def)?.dimensions.insert(id, dimension);
    out.dimensions.push(id);
    out.created.push(Element::Dimension(id));
    Ok(())
}

pub(super) fn set_dimension(
    doc: &mut Document,
    def: Option<DefinitionId>,
    out: &mut Outcome,
    id: rysmo_mesh::DimensionId,
    dimension: crate::dimensions::Dimension,
) -> Result<(), CommandError> {
    if !dimension.validate() {
        return invalid("invalid dimension geometry");
    }
    let previous = definition_mut(doc, def)?
        .dimensions
        .get_mut(&id)
        .ok_or_else(|| CommandError::Invalid(format!("no dimension {id} here")))?;
    out.changed = *previous != dimension;
    *previous = dimension;
    Ok(())
}

pub(super) fn apply_dimension_defaults(
    doc: &mut Document,
    def: Option<DefinitionId>,
    out: &mut Outcome,
    ids: Vec<rysmo_mesh::DimensionId>,
) -> Result<(), CommandError> {
    out.changed = false;
    let defaults = doc.style.dimension_defaults;
    let d = definition_mut(doc, def)?;
    if ids.iter().any(|id| !d.dimensions.contains_key(id)) {
        return invalid("dimension not found in this context");
    }
    for id in ids {
        let dimension =
            d.dimensions.get_mut(&id).ok_or_else(|| CommandError::Invalid(format!("no dimension {id} here")))?;
        let previous = dimension.clone();
        defaults.apply(dimension);
        out.changed |= *dimension != previous;
    }
    Ok(())
}

pub(super) fn erase_dimensions(
    doc: &mut Document,
    def: Option<DefinitionId>,
    out: &mut Outcome,
    ids: Vec<rysmo_mesh::DimensionId>,
) -> Result<(), CommandError> {
    let d = definition_mut(doc, def)?;
    if ids.iter().any(|id| !d.dimensions.contains_key(id)) {
        return invalid("dimension not found in this context");
    }
    out.changed = !ids.is_empty();
    for id in ids {
        d.dimensions.remove(&id);
    }
    Ok(())
}
