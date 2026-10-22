//! Moving, rotating and copying geometry, annotations and instances.

use super::types::{CommandError, invalid};
use crate::{Document, Guide, Instance};
use rysmo_math::{DMat4, DQuat, DVec3, EPS};
use rysmo_mesh::{DefinitionId, Element, GuideId, InstanceId};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn validate_rotation(centre: DVec3, axis: DVec3, angle: f64) -> Result<(), CommandError> {
    if !centre.is_finite()
        || !axis.is_finite()
        || !axis.length().is_finite()
        || axis.length() < EPS
        || !angle.is_finite()
    {
        return invalid("rotation needs a finite centre, angle and usable axis");
    }
    Ok(())
}

pub(crate) fn rotation(centre: DVec3, axis: DVec3, degrees: f64) -> DMat4 {
    DMat4::from_translation(centre)
        * DMat4::from_quat(DQuat::from_axis_angle(axis.normalize(), degrees.to_radians()))
        * DMat4::from_translation(-centre)
}

pub(super) fn split(
    elements: &[Element],
) -> (Vec<Element>, Vec<InstanceId>, Vec<GuideId>, Vec<rysmo_mesh::DimensionId>) {
    let mut geo = Vec::new();
    let mut inst = Vec::new();
    let mut guides = Vec::new();
    let mut dimensions = Vec::new();
    for e in elements {
        match e {
            Element::Edge(_) | Element::Face(_) => geo.push(*e),
            Element::Instance(i) => inst.push(*i),
            Element::Guide(g) => guides.push(*g),
            Element::Dimension(id) => dimensions.push(*id),
            Element::Text(_) | Element::Section(_) => {}
        }
    }
    (geo, inst, guides, dimensions)
}

pub(super) fn map_guide(g: &Guide, m: &DMat4) -> Guide {
    match g {
        Guide::Line { origin, dir, hidden } => Guide::Line {
            origin: m.transform_point3(*origin),
            dir: m.transform_vector3(*dir).normalize_or(*dir),
            hidden: *hidden,
        },
        Guide::Point { p, origin, hidden } => {
            Guide::Point { p: m.transform_point3(*p), origin: origin.map(|p| m.transform_point3(p)), hidden: *hidden }
        }
    }
}
/// Applies an affine map to geometry, instances and guides of a definition.
pub(crate) fn transform_items(
    doc: &mut Document,
    def: Option<DefinitionId>,
    elements: &[Element],
    m: DMat4,
) -> Result<(), CommandError> {
    let (geo, inst, guides, dimensions) = split(elements);
    let translation = m.x_axis.truncate().distance(DVec3::X) < 1e-10
        && m.y_axis.truncate().distance(DVec3::Y) < 1e-10
        && m.z_axis.truncate().distance(DVec3::Z) < 1e-10;
    if !m.is_finite() || !m.inverse().is_finite() {
        return invalid("transform needs a finite, invertible matrix");
    }
    let text_ids =
        elements.iter().filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None }).collect::<Vec<_>>();
    let original_texts = text_ids
        .iter()
        .map(|id| {
            doc.def(def)
                .unwrap()
                .texts
                .get(id)
                .cloned()
                .map(|text| (*id, text))
                .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let source_mesh = (!text_ids.is_empty() || !dimensions.is_empty() && !translation)
        .then(|| doc.def(def).expect("checked").mesh.clone());
    let vertices = source_mesh.as_ref().map(|mesh| mesh.vertices_of(&geo)).unwrap_or_default();
    let original_dimensions: Vec<_> = dimensions
        .iter()
        .map(|id| {
            let dimension = doc
                .def(def)
                .expect("checked")
                .dimensions
                .get(id)
                .cloned()
                .ok_or_else(|| CommandError::Invalid(format!("no dimension {id} here")))?;
            let measured = dimension.resolve(doc, def);
            let origin =
                if matches!(dimension.measure, crate::dimensions::DimensionMeasure::Radial { diameter: true, .. }) {
                    (measured.from + measured.to) * 0.5
                } else {
                    measured.from
                };
            Ok((*id, dimension, origin))
        })
        .collect::<Result<_, CommandError>>()?;
    if !geo.is_empty() {
        doc.with_mesh(def, |mesh| mesh.transform_elements(&geo, |p| m.transform_point3(p))).expect("checked")?;
    }
    let sections: Vec<_> =
        elements.iter().filter_map(|e| if let Element::Section(id) = e { Some(*id) } else { None }).collect();
    for id in sections {
        let plane = doc
            .def_mut(def)
            .unwrap()
            .sections
            .planes
            .get_mut(&id)
            .ok_or_else(|| CommandError::Invalid("section plane is missing".into()))?;
        *plane = plane.transformed(m);
    }
    let d = doc.def_mut(def).expect("checked");
    for i in inst.iter() {
        if let Some(x) = d.instances.get_mut(i) {
            x.transform = m * x.transform;
        }
    }
    for g in guides {
        if let Some(x) = d.guides.get_mut(&g) {
            *x = map_guide(x, &m);
        }
    }
    for (id, text) in original_texts {
        let text = text.transformed_links(source_mesh.as_ref().unwrap(), m, &vertices, &inst);
        if !text.validate() {
            return invalid("invalid transformed text");
        }
        doc.def_mut(def).unwrap().texts.insert(id, text);
    }
    for (id, mut dimension, original_origin) in original_dimensions {
        if !translation {
            dimension = dimension.transformed_links(source_mesh.as_ref().unwrap(), m, &vertices, &inst);
            if !dimension.validate() {
                return invalid("transformed dimension has invalid placement");
            }
            doc.def_mut(def).expect("checked").dimensions.insert(id, dimension);
            continue;
        }
        let measured = dimension.resolve(doc, def);
        let origin = if matches!(dimension.measure, crate::dimensions::DimensionMeasure::Radial { diameter: true, .. })
        {
            (measured.from + measured.to) * 0.5
        } else {
            measured.from
        };
        dimension.offset += m.transform_point3(original_origin) - origin;
        if !dimension.validate() {
            return invalid("dimension placement must stay in its plane");
        }
        doc.def_mut(def).expect("checked").dimensions.insert(id, dimension);
    }
    Ok(())
}

/// Copies geometry, instances and guides `copies` times, the k-th mapped by `m(k)`.
pub(super) fn copy_items(
    doc: &mut Document,
    def: Option<DefinitionId>,
    elements: &[Element],
    copies: u32,
    m: impl Fn(usize) -> DMat4,
) -> Result<Vec<Element>, CommandError> {
    let (geo, inst, guides, dimensions) = split(elements);
    if dimensions.iter().any(|id| !doc.def(def).expect("checked").dimensions.contains_key(id)) {
        return invalid("dimension not found in this context");
    }
    let sections = elements
        .iter()
        .filter_map(|e| if let Element::Section(id) = e { Some(*id) } else { None })
        .map(|id| {
            doc.def(def)
                .unwrap()
                .sections
                .planes
                .get(&id)
                .cloned()
                .ok_or_else(|| CommandError::Invalid("section plane is missing".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let texts = elements
        .iter()
        .filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None })
        .map(|id| {
            doc.def(def)
                .unwrap()
                .texts
                .get(&id)
                .cloned()
                .ok_or_else(|| CommandError::Invalid(format!("no text {id} here")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let source_mesh =
        (!dimensions.is_empty() || !texts.is_empty()).then(|| doc.def(def).expect("checked").mesh.clone());
    let originals = dimensions
        .iter()
        .map(|id| {
            let dimension = doc.def(def).expect("checked").dimensions[id].clone();
            let measured = dimension.resolve(doc, def);
            let origin =
                if matches!(dimension.measure, crate::dimensions::DimensionMeasure::Radial { diameter: true, .. }) {
                    (measured.from + measured.to) * 0.5
                } else {
                    measured.from
                };
            (dimension, origin)
        })
        .collect::<Vec<_>>();
    let copied_edges = geo
        .iter()
        .filter(|_| source_mesh.is_some())
        .flat_map(|element| match element {
            Element::Edge(id) => vec![*id],
            Element::Face(id) => source_mesh.as_ref().unwrap().face_edges(*id),
            _ => Vec::new(),
        })
        .collect::<BTreeSet<_>>();
    let mut out = Vec::new();
    if !geo.is_empty() {
        let made = doc
            .with_mesh(def, |mesh| mesh.copy_elements_with(&geo, copies as usize, |p, k| m(k).transform_point3(p)))
            .expect("checked")?;
        out.extend(made);
    }
    for k in 1..=copies as usize {
        let mk = m(k);
        let translation =
            [DVec3::X, DVec3::Y, DVec3::Z].iter().all(|axis| mk.transform_vector3(*axis).distance(*axis) < 1e-10);
        for plane in &sections {
            let id = doc.reserve_id();
            doc.def_mut(def).unwrap().sections.planes.insert(id, plane.transformed(mk));
            out.push(Element::Section(id));
        }
        let mut instance_map = BTreeMap::new();
        for i in &inst {
            let Some(x) = doc.def(def).expect("checked").instances.get(i).cloned() else { continue };
            let id = InstanceId(doc.reserve_id());
            doc.def_mut(def).expect("checked").instances.insert(id, Instance { transform: mk * x.transform, ..x });
            instance_map.insert(*i, id);
            out.push(Element::Instance(id));
        }
        for (original, old_origin) in &originals {
            let mut dimension = original.copied_links(
                source_mesh.as_ref().unwrap(),
                &doc.def(def).expect("checked").mesh,
                mk,
                &instance_map,
                &copied_edges,
            );
            let measured = dimension.resolve(doc, def);
            let new_origin =
                if matches!(dimension.measure, crate::dimensions::DimensionMeasure::Radial { diameter: true, .. }) {
                    (measured.from + measured.to) * 0.5
                } else {
                    measured.from
                };
            if translation {
                dimension.offset += mk.transform_point3(*old_origin) - new_origin;
            }
            if !dimension.validate() {
                return invalid("copied dimension placement must stay in its plane");
            }
            let id = rysmo_mesh::DimensionId(doc.reserve_id());
            doc.def_mut(def).expect("checked").dimensions.insert(id, dimension);
            out.push(Element::Dimension(id));
        }
        for original in &texts {
            let text = original.copied_links(
                source_mesh.as_ref().unwrap(),
                &doc.def(def).unwrap().mesh,
                mk,
                &instance_map,
                &geo,
            );
            if !text.validate() {
                return invalid("invalid copied text");
            }
            let id = rysmo_mesh::TextId(doc.reserve_id());
            doc.def_mut(def).unwrap().texts.insert(id, text);
            out.push(Element::Text(id));
        }
        for g in &guides {
            let Some(x) = doc.def(def).expect("checked").guides.get(g).copied() else { continue };
            let id = GuideId(doc.reserve_id());
            doc.def_mut(def).expect("checked").guides.insert(id, map_guide(&x, &mk));
            out.push(Element::Guide(id));
        }
    }
    Ok(out)
}
