//! Saved scenes, display settings and measurement preferences.

use super::types::{CommandError, Outcome, invalid};
use crate::Document;
use crate::{Style, Units};

pub(super) fn add_scene(doc: &mut Document, mut scene: crate::scenes::Scene) -> Result<(), CommandError> {
    scene.id = doc.reserve_id();
    if !scene.valid() {
        return invalid("invalid scene");
    }
    doc.scenes.push(scene);
    Ok(())
}

pub(super) fn set_scene(
    doc: &mut Document,
    out: &mut Outcome,
    id: u64,
    scene: Option<crate::scenes::Scene>,
) -> Result<(), CommandError> {
    let index =
        doc.scenes.iter().position(|s| s.id == id).ok_or_else(|| CommandError::Invalid("scene is missing".into()))?;
    if let Some(scene) = scene {
        if scene.id != id || !scene.valid() {
            return invalid("invalid scene");
        }
        out.changed = doc.scenes[index] != scene;
        doc.scenes[index] = scene;
    } else {
        doc.scenes.remove(index);
    }
    Ok(())
}

pub(super) fn restore_scene(doc: &mut Document, id: u64) -> Result<(), CommandError> {
    let scene = doc
        .scenes
        .iter()
        .find(|s| s.id == id)
        .cloned()
        .ok_or_else(|| CommandError::Invalid("scene is missing".into()))?;
    scene.restore(doc);
    Ok(())
}

pub(super) fn reorder_scene(doc: &mut Document, out: &mut Outcome, id: u64, index: usize) -> Result<(), CommandError> {
    let old =
        doc.scenes.iter().position(|s| s.id == id).ok_or_else(|| CommandError::Invalid("scene is missing".into()))?;
    if index >= doc.scenes.len() {
        return invalid("invalid scene order");
    }
    out.changed = old != index;
    let scene = doc.scenes.remove(old);
    doc.scenes.insert(index, scene);
    Ok(())
}

pub(super) fn set_style(doc: &mut Document, out: &mut Outcome, style: Style) -> Result<(), CommandError> {
    if !style.valid_sections()
        || !style.valid_render()
        || !style.dimensions.validate()
        || !style.dimension_defaults.validate()
        || !style.text_defaults.validate()
    {
        return Err(CommandError::Invalid("invalid display settings".into()));
    }
    if style.environment_image.is_some_and(|id| !doc.images.contains_key(&id)) {
        return invalid("the environment image is missing");
    }
    out.changed = doc.style != style;
    let previous = doc.style.environment_image;
    doc.style = style;
    super::execute::drop_unused_image(doc, previous);
    Ok(())
}

pub(super) fn set_measurement_settings(
    doc: &mut Document,
    out: &mut Outcome,
    settings: crate::MeasurementSettings,
) -> Result<(), CommandError> {
    if !settings.valid() {
        return invalid("Invalid measurement settings.");
    }
    out.changed = doc.measurement != settings;
    doc.measurement = settings;
    Ok(())
}

pub(super) fn set_length_precision(
    doc: &mut Document,
    out: &mut Outcome,
    precision: Option<u8>,
) -> Result<(), CommandError> {
    if precision.is_some_and(|p| p > 6) {
        return invalid("Length precision must be between 0 and 6 decimal places.");
    }
    out.changed = doc.length_precision != precision;
    doc.length_precision = precision;
    Ok(())
}

pub(super) fn set_units(doc: &mut Document, out: &mut Outcome, units: Units) -> Result<(), CommandError> {
    out.changed = doc.units != units;
    doc.units = units;
    Ok(())
}
