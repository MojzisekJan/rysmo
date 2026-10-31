//! Group/component creation, extraction and definition lifetime.

use super::transforms::{map_guide, split};
use super::types::{CommandError, invalid};
use crate::{Definition, Document, Guide, Instance};
use rysmo_math::{DMat4, DVec3};
use rysmo_mesh::{DefinitionId, Element, FaceId, GuideId, InstanceId, Template};
use std::collections::{BTreeMap, BTreeSet};

/// Moves elements into a new definition with one instance where they were.
pub(super) fn make_definition(
    doc: &mut Document,
    def: Option<DefinitionId>,
    elements: &[Element],
    group: bool,
    name: Option<String>,
) -> Result<InstanceId, CommandError> {
    let (geo, inst, guides, dimensions) = split(elements);
    let sections: BTreeSet<_> =
        elements.iter().filter_map(|e| if let Element::Section(id) = e { Some(*id) } else { None }).collect();
    if sections.iter().any(|id| !doc.def(def).unwrap().sections.planes.contains_key(id)) {
        return invalid("section plane not found in this context");
    }
    let texts: BTreeSet<_> =
        elements.iter().filter_map(|e| if let Element::Text(id) = e { Some(*id) } else { None }).collect();
    if texts.iter().any(|id| !doc.def(def).unwrap().texts.contains_key(id)) {
        return invalid("text not found in this context");
    }
    if geo.is_empty()
        && inst.is_empty()
        && guides.is_empty()
        && dimensions.is_empty()
        && texts.is_empty()
        && sections.is_empty()
    {
        return invalid("select something first");
    }
    if dimensions.iter().any(|id| !doc.def(def).expect("checked").dimensions.contains_key(id)) {
        return invalid("dimension not found in this context");
    }
    let source_mesh = doc.def(def).expect("checked").mesh.clone();
    // Geometry out of the parent, with fresh ids.
    let mut mesh = doc.with_mesh(def, |m| m.extract(&geo)).expect("checked");
    doc.next_id = doc.next_id.max(mesh.next_id());
    let parent = doc.def_mut(def).expect("checked");
    let children: BTreeMap<InstanceId, Instance> =
        inst.iter().filter_map(|i| parent.instances.remove(i).map(|x| (*i, x))).collect();
    let moved_guides: BTreeMap<GuideId, Guide> =
        guides.iter().filter_map(|g| parent.guides.remove(g).map(|x| (*g, x))).collect();
    let moved_dimensions: BTreeMap<_, _> =
        dimensions.iter().map(|id| (*id, parent.dimensions.remove(id).expect("validated"))).collect();
    let moved_texts: BTreeMap<_, _> =
        texts.iter().map(|id| (*id, parent.texts.remove(id).expect("validated"))).collect();
    let moved_active = parent.sections.active.filter(|id| sections.contains(id));
    if moved_active.is_some() {
        parent.sections.active = None;
    }
    let moved_sections = sections
        .iter()
        .map(|id| (*id, parent.sections.planes.remove(id).expect("validated")))
        .collect::<BTreeMap<_, _>>();
    // Place the component axes at a corner of the new definition's bounding box.
    let mut tmp = Definition {
        mesh: mesh.clone(),
        instances: children.iter().map(|(id, instance)| (*id, instance.clone())).collect(),
        dimensions: moved_dimensions.clone(),
        texts: moved_texts.clone(),
        ..Default::default()
    };
    let id = DefinitionId(doc.reserve_id());
    doc.definitions.insert(id, (std::mem::take(&mut tmp)).into());
    let origin = doc
        .definition_bounds(Some(id))
        .map(|(lo, _)| lo)
        .or_else(|| moved_sections.values().next().map(|p| p.origin))
        .unwrap_or(DVec3::ZERO);
    mesh.map_all(|p| p - origin);
    let back = DMat4::from_translation(-origin);
    let children = children.into_iter().map(|(k, mut x)| {
        x.transform = back * x.transform;
        (k, x)
    });
    let guides = moved_guides.into_iter().map(|(k, g)| (k, map_guide(&g, &back)));
    let instance_map = inst.iter().map(|id| (*id, *id)).collect();
    let dimensions = moved_dimensions
        .into_iter()
        .map(|(id, dimension)| (id, dimension.remapped(&source_mesh, &mesh, back, &instance_map)))
        .collect();
    let texts = moved_texts
        .into_iter()
        .map(|(id, text)| (id, text.remapped(&source_mesh, &mesh, back, &instance_map)))
        .collect();
    let sections = crate::sections::Sections {
        planes: moved_sections.into_iter().map(|(id, plane)| (id, plane.transformed(back))).collect(),
        active: moved_active,
    };
    let count = doc.definitions.values().filter(|d| !d.group).count();
    let name = name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| if group { "Group".into() } else { format!("Component#{count}") });
    doc.definitions.insert(
        id,
        Definition {
            name,
            group,
            mesh,
            instances: children.collect(),
            guides: guides.collect(),
            sections,
            component: Default::default(),
            parametric: None,
            procedure: None,
            section_hatch: None,
            axes: Default::default(),
            dimensions,
            texts,
        }
        .into(),
    );
    let iid = InstanceId(doc.reserve_id());
    doc.def_mut(def).expect("checked").instances.insert(
        iid,
        Instance {
            glued_to: None,
            attributes: Default::default(),
            definition: id,
            transform: DMat4::from_translation(origin),
            name: String::new(),
            tag: None,
            material: None,
            hidden: false,
        },
    );
    let owners: Vec<_> = std::iter::once(None).chain(doc.definitions.keys().copied().map(Some)).collect();
    let document = &*doc;
    let target = &document.def(Some(id)).expect("created").mesh;
    let updates: Vec<_> = owners
        .into_iter()
        .flat_map(|owner| {
            document
                .def(owner)
                .unwrap()
                .dimensions
                .iter()
                .map(|(dimension_id, dimension)| {
                    (
                        owner,
                        *dimension_id,
                        dimension.reparented(crate::annotation_reparent::AnnotationReparent {
                            doc: document,
                            owner,
                            parent: def,
                            instance: iid,
                            source: &source_mesh,
                            target,
                            transform: back,
                            children: &inst,
                        }),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    for (owner, id, dimension) in updates {
        doc.def_mut(owner).unwrap().dimensions.insert(id, dimension);
    }
    let owners: Vec<_> = std::iter::once(None).chain(doc.definitions.keys().copied().map(Some)).collect();
    let document = &*doc;
    let target = &document.def(Some(id)).unwrap().mesh;
    let updates: Vec<_> = owners
        .into_iter()
        .flat_map(|owner| {
            document
                .def(owner)
                .unwrap()
                .texts
                .iter()
                .map(|(text_id, text)| {
                    (
                        owner,
                        *text_id,
                        text.reparented(crate::annotation_reparent::AnnotationReparent {
                            doc: document,
                            owner,
                            parent: def,
                            instance: iid,
                            source: &source_mesh,
                            target,
                            transform: back,
                            children: &inst,
                        }),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    for (owner, id, text) in updates {
        doc.def_mut(owner).unwrap().texts.insert(id, text);
    }
    Ok(iid)
}

pub(crate) fn explode(
    doc: &mut Document,
    def: Option<DefinitionId>,
    instance: InstanceId,
) -> Result<Vec<Element>, CommandError> {
    crate::components::bake_opening(doc, def, instance)?;
    let inst = doc
        .def_mut(def)
        .expect("checked")
        .instances
        .remove(&instance)
        .ok_or_else(|| CommandError::Invalid(format!("no group or component {instance} here")))?;
    let mut source = doc
        .definitions
        .get(&inst.definition)
        .map(|d| Definition::clone(d))
        .ok_or_else(|| CommandError::Invalid("broken instance".into()))?;
    let t = inst.transform;
    let face_placements = source
        .mesh
        .faces()
        .filter_map(|(id, face)| {
            let size = [face.material.or(inst.material), face.back_material.or(inst.material)]
                .into_iter()
                .flatten()
                .find_map(|m| doc.materials.get(&m).and_then(|m| m.texture.map(|t| t.size)));
            let size = size.or(face.uv.map(|_| [1., 1.]))?;
            let plane = source.mesh.face_plane(id)?;
            Some((id, crate::texture::transformed(plane, size, face.uv.unwrap_or_default(), t)))
        })
        .collect::<Vec<_>>();
    for (id, placement) in face_placements {
        if let Some(placement) = placement {
            source.mesh.face_mut(id).unwrap().uv = Some(placement);
        }
    }
    let template = Template::whole(&source.mesh);
    let before: BTreeSet<FaceId> = doc.def(def).expect("checked").mesh.faces().map(|(f, _)| f).collect();
    let mut out = doc.with_mesh(def, |m| m.stamp(&template, |p| t.transform_point3(p))).expect("checked")?;
    // A mirrored instance turns the stamped faces inside out; stamp already fixes facing
    // from the mapped normals. The instance's paint goes to faces without their own.
    if let Some(mat) = inst.material {
        doc.with_mesh(def, |m| {
            let fresh: Vec<FaceId> = m.faces().map(|(f, _)| f).filter(|f| !before.contains(f)).collect();
            for f in fresh {
                if m.face(f).is_some_and(|x| x.material.is_none()) {
                    let _ = m.paint(f, Some(mat), false);
                }
            }
        });
    }
    let mut instance_map = BTreeMap::new();
    for (old, x) in &source.instances {
        let id = InstanceId(doc.reserve_id());
        instance_map.insert(*old, id);
        doc.def_mut(def).expect("checked").instances.insert(id, Instance { transform: t * x.transform, ..x.clone() });
        out.push(Element::Instance(id));
    }
    for g in source.guides.values() {
        let id = GuideId(doc.reserve_id());
        doc.def_mut(def).expect("checked").guides.insert(id, map_guide(g, &t));
        out.push(Element::Guide(id));
    }
    for (old, plane) in &source.sections.planes {
        let id = doc.reserve_id();
        let sections = &mut doc.def_mut(def).unwrap().sections;
        sections.planes.insert(id, plane.transformed(t));
        out.push(Element::Section(id));
        if sections.active.is_none() && source.sections.active == Some(*old) {
            sections.active = Some(id);
        }
    }
    let target_mesh = doc.def(def).expect("checked").mesh.clone();
    for definition in doc.definitions_mut_where(|d| !d.dimensions.is_empty() || !d.texts.is_empty()) {
        for dimension in definition.dimensions.values_mut() {
            dimension.remap_instance_reference(instance, &source.mesh, &target_mesh, &instance_map, t, true);
        }
        for text in definition.texts.values_mut() {
            text.remap_instance_reference(instance, &source.mesh, &target_mesh, &instance_map, t, true);
        }
    }
    for text in source.texts.values() {
        let text = text.remapped(&source.mesh, &doc.def(def).unwrap().mesh, t, &instance_map);
        let id = rysmo_mesh::TextId(doc.reserve_id());
        doc.def_mut(def).unwrap().texts.insert(id, text);
    }
    for dimension in source.dimensions.values() {
        let dimension = dimension.remapped(&source.mesh, &doc.def(def).expect("checked").mesh, t, &instance_map);
        if !dimension.validate() {
            return invalid("exploded dimension has degenerate placement");
        }
        let id = rysmo_mesh::DimensionId(doc.reserve_id());
        doc.def_mut(def).expect("checked").dimensions.insert(id, dimension);
    }
    Ok(out)
}

pub(super) fn make_unique(
    doc: &mut Document,
    def: Option<DefinitionId>,
    instance: InstanceId,
) -> Result<(), CommandError> {
    let old = doc
        .def(def)
        .expect("checked")
        .instances
        .get(&instance)
        .map(|i| i.definition)
        .ok_or_else(|| CommandError::Invalid(format!("no group or component {instance} here")))?;
    let source = doc.definitions.get(&old).cloned().ok_or_else(|| CommandError::Invalid("broken instance".into()))?;
    // Fresh ids for everything inside, so ids stay unique in the document.
    let mut mesh = source.mesh.clone();
    mesh.ensure_next_id(doc.next_id);
    let all: Vec<Element> =
        mesh.edges().map(|(e, _)| Element::Edge(e)).chain(mesh.faces().map(|(f, _)| Element::Face(f))).collect();
    let fresh = mesh.extract(&all);
    doc.next_id = doc.next_id.max(fresh.next_id());
    let mut instance_map = BTreeMap::new();
    let instances: BTreeMap<InstanceId, Instance> = source
        .instances
        .iter()
        .map(|(old, x)| {
            let id = InstanceId(doc.reserve_id());
            instance_map.insert(*old, id);
            (id, x.clone())
        })
        .collect();
    let guides: BTreeMap<GuideId, Guide> = source.guides.values().map(|g| (GuideId(doc.reserve_id()), *g)).collect();
    let dimensions: BTreeMap<_, _> = source
        .dimensions
        .values()
        .map(|dimension| {
            let dimension = dimension.remapped(&source.mesh, &fresh, DMat4::IDENTITY, &instance_map);
            (rysmo_mesh::DimensionId(doc.reserve_id()), dimension)
        })
        .collect();
    let texts = source
        .texts
        .values()
        .map(|text| {
            (rysmo_mesh::TextId(doc.reserve_id()), text.remapped(&source.mesh, &fresh, DMat4::IDENTITY, &instance_map))
        })
        .collect();
    let mut sections = crate::sections::Sections::default();
    for (old, plane) in &source.sections.planes {
        let id = doc.reserve_id();
        sections.planes.insert(id, plane.clone());
        if source.sections.active == Some(*old) {
            sections.active = Some(id);
        }
    }
    let id = DefinitionId(doc.reserve_id());
    let name = if source.group { source.name.clone() } else { format!("{}#{}", source.name, doc.instance_count(old)) };
    for definition in doc.definitions_mut_where(|d| !d.dimensions.is_empty() || !d.texts.is_empty()) {
        for dimension in definition.dimensions.values_mut() {
            dimension.remap_instance_reference(instance, &source.mesh, &fresh, &instance_map, DMat4::IDENTITY, false);
        }
        for text in definition.texts.values_mut() {
            text.remap_instance_reference(instance, &source.mesh, &fresh, &instance_map, DMat4::IDENTITY, false);
        }
    }
    doc.definitions.insert(
        id,
        Definition {
            name,
            group: source.group,
            component: source.component.clone(),
            parametric: source.parametric.clone(),
            procedure: source.procedure.clone(),
            section_hatch: source.section_hatch,
            mesh: fresh,
            instances: instances.into_iter().collect(),
            guides,
            axes: source.axes,
            dimensions,
            texts,
            sections,
        }
        .into(),
    );
    doc.def_mut(def).expect("checked").instances.get_mut(&instance).expect("found above").definition = id;
    Ok(())
}

/// Removes definitions no instance uses any more (after erasing or exploding).
pub(super) fn purge_unused(doc: &mut Document) {
    loop {
        let used: BTreeSet<DefinitionId> = std::iter::once(&doc.root)
            .chain(doc.definitions.values())
            .flat_map(|d| d.instances.values().map(|i| i.definition))
            .collect();
        let before = doc.definitions.len();
        doc.definitions.retain(|id, definition| !definition.group || used.contains(id));
        if doc.definitions.len() == before {
            break;
        }
    }
}
