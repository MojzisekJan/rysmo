//! Named connection frames and typed, spreadsheet-independent component metadata.
use crate::{CommandError, DefinitionId, Document, InstanceId};
use rysmo_math::{DMat3, DMat4, DVec3, DrawingAxes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Value {
    Text(String),
    Number(f64),
    Boolean(bool),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attribute {
    pub value: Value,
    #[serde(default)]
    pub unit: String,
}
pub type Attributes = BTreeMap<String, Attribute>;
pub type Connections = BTreeMap<String, DrawingAxes>;
fn invalid(message: &str) -> CommandError {
    CommandError::Invalid(message.into())
}
fn name_valid(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 128 && !name.chars().any(char::is_control)
}
pub fn validate_attributes(values: &Attributes) -> Result<(), CommandError> {
    if values.len() > 128
        || values.iter().any(|(key, attribute)| {
            !name_valid(key)
                || attribute.unit.len() > 32
                || attribute.unit.chars().any(char::is_control)
                || match &attribute.value {
                    Value::Text(text) => text.len() > 4096,
                    Value::Number(n) => !n.is_finite(),
                    Value::Boolean(_) => false,
                }
        })
    {
        return Err(invalid("Use up to 128 named attributes with finite numbers, bounded text and short units"));
    }
    Ok(())
}
pub fn validate_connections(values: &Connections) -> Result<(), CommandError> {
    if values.len() > 64 || values.iter().any(|(key, frame)| !name_valid(key) || !frame.valid()) {
        return Err(invalid("Use up to 64 named connection points with valid orthonormal local frames"));
    }
    Ok(())
}
pub fn attributes(doc: &Document, instance: &crate::Instance) -> Attributes {
    let mut values =
        doc.definitions.get(&instance.definition).map(|d| d.component.attributes.clone()).unwrap_or_default();
    values.extend(instance.attributes.clone());
    values
}
fn world_frame(transform: DMat4, frame: DrawingAxes) -> Result<DMat3, CommandError> {
    if !transform.is_finite() || !transform.inverse().is_finite() {
        return Err(invalid("Invalid connection transform"));
    }
    let z = transform
        .inverse()
        .transpose()
        .transform_vector3(frame.x.cross(frame.y))
        .try_normalize()
        .ok_or_else(|| invalid("Invalid connection normal"))?;
    let x = transform.transform_vector3(frame.x);
    let x = (x - z * x.dot(z)).try_normalize().ok_or_else(|| invalid("Invalid connection tangent"))?;
    Ok(DMat3::from_cols(x, z.cross(x), z))
}
/// A world-rigid placement retains source dimensions even under mirrored/scaled parents.
pub fn attachment(
    doc: &Document,
    owner: Option<DefinitionId>,
    context: DMat4,
    source: InstanceId,
    source_name: &str,
    target: InstanceId,
    target_name: &str,
) -> Result<DMat4, CommandError> {
    if source == target {
        return Err(invalid("Choose two different component occurrences"));
    }
    let def = doc.def(owner).ok_or_else(|| invalid("Missing connection context"))?;
    let source = def.instances.get(&source).ok_or_else(|| invalid("Missing source occurrence"))?;
    let target = def.instances.get(&target).ok_or_else(|| invalid("Missing target occurrence"))?;
    if source.hidden || target.hidden || !doc.tag_visible(source.tag) || !doc.tag_visible(target.tag) {
        return Err(invalid("Reveal both connection occurrences first"));
    }
    let frame = |definition, name: &str| {
        doc.definitions
            .get(&definition)
            .and_then(|d| d.component.connections.get(name))
            .copied()
            .ok_or_else(|| invalid("Missing named connection point"))
    };
    let a = frame(source.definition, source_name)?;
    let b = frame(target.definition, target_name)?;
    let source_world = context * source.transform;
    let target_world = context * target.transform;
    let from = world_frame(source_world, a)?;
    let to = world_frame(target_world, b)?;
    let rotation = DMat4::from_mat3(to * DMat3::from_diagonal(DVec3::new(1., -1., -1.)) * from.transpose());
    let result = context.inverse()
        * DMat4::from_translation(target_world.transform_point3(b.origin))
        * rotation
        * DMat4::from_translation(-source_world.transform_point3(a.origin))
        * source_world;
    if !result.is_finite() || !result.inverse().is_finite() {
        return Err(invalid("Cannot attach through a degenerate context"));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, Element};
    fn fixture() -> (Editor, InstanceId, InstanceId, DefinitionId) {
        let mut e = Editor::new();
        let elements = e
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap()
            .created;
        let out = e.apply(Command::MakeComponent { elements, name: Some("Part".into()) }).unwrap();
        let Element::Instance(a) = out.created[0] else { panic!() };
        let definition = e.doc().root.instances[&a].definition;
        let out = e
            .apply(Command::PlaceInstance { definition, origin: DVec3::new(4., 3., 2.), rotation: 90., scale: 2. })
            .unwrap();
        let Element::Instance(b) = out.created[0] else { panic!() };
        e.apply(Command::SetComponentData {
            definition,
            connections: BTreeMap::from([("joint".into(), DrawingAxes::default())]),
            attributes: Default::default(),
        })
        .unwrap();
        (e, a, b, definition)
    }
    #[test]
    fn connection_meets_opposed_normal_and_preserves_world_shape_in_scaled_context() {
        let (mut e, a, b, _) = fixture();
        let context = DMat4::from_scale(DVec3::new(-2., 3., 4.));
        let before = e.doc().clone();
        let old = context * before.root.instances[&a].transform;
        let target = context * before.root.instances[&b].transform;
        e.apply(Command::AttachConnections {
            source: a,
            source_name: "joint".into(),
            target: b,
            target_name: "joint".into(),
            context,
        })
        .unwrap();
        let attached = context * e.doc().root.instances[&a].transform;
        assert!(attached.transform_point3(DVec3::ZERO).distance(target.transform_point3(DVec3::ZERO)) < 1e-9);
        let normal = |m: DMat4| m.inverse().transpose().transform_vector3(DVec3::Z).normalize();
        assert!((normal(attached).dot(normal(target)) + 1.).abs() < 1e-9);
        for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
            assert!((attached.transform_vector3(axis).length() - old.transform_vector3(axis).length()).abs() < 1e-9);
        }
        e.undo();
        assert_eq!(e.doc(), &before);
    }
    #[test]
    fn typed_overrides_are_local_and_failed_metadata_edits_are_atomic() {
        let (mut e, a, b, definition) = fixture();
        let defaults = BTreeMap::from([("Cost".into(), Attribute { value: Value::Number(12.), unit: "EUR".into() })]);
        e.apply(Command::SetComponentData {
            definition,
            connections: Default::default(),
            attributes: defaults.clone(),
        })
        .unwrap();
        let overrides = BTreeMap::from([("Cost".into(), Attribute { value: Value::Number(20.), unit: "EUR".into() })]);
        e.apply(Command::SetInstanceAttributes { instance: a, attributes: overrides.clone() }).unwrap();
        assert_eq!(attributes(e.doc(), &e.doc().root.instances[&a]), overrides);
        assert_eq!(attributes(e.doc(), &e.doc().root.instances[&b]), defaults);
        let before = e.doc().clone();
        let invalid =
            BTreeMap::from([("bad".into(), Attribute { value: Value::Number(f64::NAN), unit: String::new() })]);
        assert!(e.apply(Command::SetInstanceAttributes { instance: a, attributes: invalid }).is_err());
        assert_eq!(e.doc(), &before);
    }
}
