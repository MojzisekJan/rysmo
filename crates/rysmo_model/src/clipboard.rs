//! Immutable object clipboard and transactional import into another editing context.
use crate::{
    Command, CommandError, Definition, DefinitionId, Document, Editor, Element, Instance, InstanceId, MaterialId, TagId,
};
use rysmo_math::{DMat4, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clipboard {
    pub document: Document,
    /// Coordinates of the clipboard's origin and axes in the source model.
    pub source_transform: DMat4,
}
impl Clipboard {
    pub fn capture(editor: &Editor, elements: Vec<Element>) -> Result<Self, CommandError> {
        let mut document = editor.doc().clone();
        document.refresh_dimensions();
        document.refresh_texts();
        let owner = editor.active_definition();
        let made = Command::MakeGroup { elements }.run(&mut document, owner)?;
        let Element::Instance(id) = made.created[0] else { unreachable!() };
        let instance = document.def(owner).unwrap().instances[&id].clone();
        document.root = document.definitions[&instance.definition].clone();
        let inherited = editor
            .doc()
            .occurrences(editor.context())
            .into_iter()
            .find(|o| o.path == editor.context())
            .and_then(|o| o.material);
        if let Some(material) = inherited {
            let faces = document.root.mesh.faces().map(|(id, _)| id).collect::<Vec<_>>();
            for id in faces {
                let face = document.root.mesh.face_mut(id).unwrap();
                face.material = face.material.or(Some(material));
                face.back_material = face.back_material.or(Some(material));
            }
            for child in document.root.instances.values_mut() {
                child.material = child.material.or(Some(material));
            }
        }

        let offset = -instance.transform.w_axis.truncate();
        let faces = document
            .root
            .mesh
            .faces()
            .filter_map(|(id, f)| {
                let textured = [f.material, f.back_material]
                    .into_iter()
                    .flatten()
                    .any(|m| document.materials.get(&m).is_some_and(|m| m.texture.is_some()));
                (textured || f.uv.is_some())
                    .then(|| (id, f.uv.unwrap_or_default(), document.root.mesh.face_normal(id).unwrap()))
            })
            .collect::<Vec<_>>();
        for (id, placement, normal) in faces {
            document.root.mesh.face_mut(id).unwrap().uv = Some(crate::texture::moved(placement, normal, offset));
        }
        let mut keep = BTreeSet::new();
        fn visit(doc: &Document, def: &Definition, keep: &mut BTreeSet<DefinitionId>) {
            for instance in def.instances.values() {
                if keep.insert(instance.definition) {
                    visit(doc, &doc.definitions[&instance.definition], keep);
                }
            }
        }
        visit(&document, &document.root, &mut keep);
        document.definitions.retain(|id, _| keep.contains(id));
        let mut materials = BTreeSet::new();
        let mut tags = BTreeSet::new();
        for def in std::iter::once(&document.root).chain(document.definitions.values()) {
            for (_, face) in def.mesh.faces() {
                materials.extend([face.material, face.back_material].into_iter().flatten());
            }
            for (_, edge) in def.mesh.edges() {
                materials.extend(edge.material);
            }
            for instance in def.instances.values() {
                materials.extend(instance.material);
                tags.extend(instance.tag);
            }
        }
        document.materials.retain(|id, _| materials.contains(id));
        document.tags.retain(|id, _| tags.contains(id));
        let images = document.materials.values().filter_map(|m| m.texture.map(|t| t.image)).collect::<BTreeSet<_>>();
        document.images.retain(|id, _| images.contains(id));
        document.scenes.clear();
        document.camera = None;
        let source_transform = editor.context_transform() * instance.transform;
        Ok(Self { document, source_transform })
    }
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        self.document.definition_bounds(None)
    }
}

pub(crate) fn paste(
    doc: &mut Document,
    owner: Option<DefinitionId>,
    clip: &Clipboard,
    transform: DMat4,
    reuse: bool,
) -> Result<Vec<Element>, CommandError> {
    if !transform.is_finite() || transform.determinant().abs() < 1e-12 {
        return Err(CommandError::Invalid("Paste needs a finite, invertible transform.".into()));
    }
    let source = &clip.document;
    let mut images = BTreeMap::new();
    for (old, image) in &source.images {
        let existing = doc.images.iter().find(|(_, i)| *i == image).map(|(id, _)| *id);
        let id = existing.unwrap_or_else(|| {
            let id = crate::ImageId(doc.reserve_id());
            doc.images.insert(id, image.clone());
            id
        });
        images.insert(*old, id);
    }
    let mut materials: BTreeMap<MaterialId, MaterialId> = BTreeMap::new();
    for (old, material) in &source.materials {
        let mut m = material.clone();
        if let Some(texture) = &mut m.texture {
            texture.image = *images
                .get(&texture.image)
                .ok_or_else(|| CommandError::Invalid("Clipboard texture image is missing.".into()))?;
        }
        let existing = doc.materials.iter().find(|(_, v)| **v == m).map(|(id, _)| *id);
        let id = existing.unwrap_or_else(|| {
            let id = MaterialId(doc.reserve_id());
            doc.materials.insert(id, m);
            id
        });
        materials.insert(*old, id);
    }
    let mut tags: BTreeMap<TagId, TagId> = BTreeMap::new();
    for (old, tag) in &source.tags {
        let existing = doc.tags.iter().find(|(_, v)| *v == tag).map(|(id, _)| *id);
        let id = existing.unwrap_or_else(|| {
            let id = TagId(doc.reserve_id());
            doc.tags.insert(id, tag.clone());
            id
        });
        tags.insert(*old, id);
    }
    struct Import<'a> {
        source: &'a Document,
        materials: &'a BTreeMap<MaterialId, MaterialId>,
        tags: &'a BTreeMap<TagId, TagId>,
        definitions: BTreeMap<DefinitionId, DefinitionId>,
        visiting: BTreeSet<DefinitionId>,
        reuse: bool,
    }
    impl Import<'_> {
        fn definition(&mut self, doc: &mut Document, id: DefinitionId) -> Result<DefinitionId, CommandError> {
            if let Some(id) = self.definitions.get(&id) {
                return Ok(*id);
            }
            if !self.visiting.insert(id) || self.visiting.len() > 64 {
                return Err(CommandError::Invalid(
                    "Clipboard contains a cyclic or excessively deep component hierarchy.".into(),
                ));
            }
            let source = self
                .source
                .definitions
                .get(&id)
                .ok_or_else(|| CommandError::Invalid("Clipboard component is missing.".into()))?
                .clone();
            let new = if self.reuse && !source.group && doc.definitions.contains_key(&id) {
                id
            } else {
                let definition = self.build(doc, &source)?;
                let new = DefinitionId(doc.reserve_id());
                doc.definitions.insert(new, definition);
                new
            };
            self.visiting.remove(&id);
            self.definitions.insert(id, new);
            Ok(new)
        }
        fn build(&mut self, doc: &mut Document, source: &Definition) -> Result<Definition, CommandError> {
            let mut mesh = source.mesh.with_fresh_ids(|| doc.reserve_id())?;
            let faces = mesh.faces().map(|(id, f)| (id, f.material, f.back_material)).collect::<Vec<_>>();
            for (id, front, back) in faces {
                for (material, side) in [(front, false), (back, true)] {
                    let m = material
                        .map(|m| {
                            self.materials
                                .get(&m)
                                .copied()
                                .ok_or_else(|| CommandError::Invalid("Clipboard material is missing.".into()))
                        })
                        .transpose()?;
                    mesh.paint(id, m, side)?;
                }
            }
            let edges = mesh.edges().map(|(id, e)| (id, e.material)).collect::<Vec<_>>();
            for (id, m) in edges {
                let m = m
                    .map(|m| {
                        self.materials
                            .get(&m)
                            .copied()
                            .ok_or_else(|| CommandError::Invalid("Clipboard edge material is missing.".into()))
                    })
                    .transpose()?;
                mesh.paint_edge(id, m)?;
            }
            let mut result = Definition {
                name: source.name.clone(),
                group: source.group,
                mesh,
                axes: source.axes,
                component: source.component.clone(),
                ..Default::default()
            };
            let mut instances = BTreeMap::new();
            for (old, instance) in &source.instances {
                let definition = self.definition(doc, instance.definition)?;
                let id = InstanceId(doc.reserve_id());
                instances.insert(*old, id);
                result.instances.insert(
                    id,
                    Instance {
                        definition,
                        material: instance.material.and_then(|m| self.materials.get(&m).copied()),
                        tag: instance.tag.and_then(|t| self.tags.get(&t).copied()),
                        ..instance.clone()
                    },
                );
            }
            for guide in source.guides.values() {
                result.guides.insert(crate::GuideId(doc.reserve_id()), *guide);
            }
            for (old, plane) in &source.sections.planes {
                let id = doc.reserve_id();
                result.sections.planes.insert(id, plane.clone());
                if source.sections.active == Some(*old) {
                    result.sections.active = Some(id);
                }
            }
            for text in source.texts.values() {
                result.texts.insert(
                    crate::TextId(doc.reserve_id()),
                    text.remapped(&source.mesh, &result.mesh, DMat4::IDENTITY, &instances),
                );
            }
            for dimension in source.dimensions.values() {
                result.dimensions.insert(
                    crate::DimensionId(doc.reserve_id()),
                    dimension.remapped(&source.mesh, &result.mesh, DMat4::IDENTITY, &instances),
                );
            }
            Ok(result)
        }
    }
    let mut import = Import {
        source,
        materials: &materials,
        tags: &tags,
        definitions: BTreeMap::new(),
        visiting: BTreeSet::new(),
        reuse,
    };
    let root = import.build(doc, &source.root)?;
    for instance in root.instances.values() {
        if owner == Some(instance.definition) || owner.is_some_and(|o| doc.contains_definition(instance.definition, o))
        {
            return Err(CommandError::Invalid("A pasted component cannot contain itself.".into()));
        }
    }
    let definition = DefinitionId(doc.reserve_id());
    doc.definitions.insert(definition, root);
    let id = InstanceId(doc.reserve_id());
    doc.def_mut(owner).unwrap().instances.insert(
        id,
        Instance {
            glued_to: None,
            definition,
            transform,
            name: String::new(),
            tag: None,
            material: None,
            hidden: false,
        },
    );
    crate::command::explode(doc, owner, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Texture, TexturePlacement};
    fn fixture() -> (Editor, crate::FaceId) {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle {
            corner: DVec3::new(2., 3., 1.),
            u: DVec3::X * 2.,
            v: DVec3::Y * 2.,
            facing: None,
        })
        .unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        let image = e
            .apply(Command::AddImage { name: "Tile.png".into(), mime: "image/png".into(), data: vec![1, 2, 3] })
            .unwrap()
            .image
            .unwrap();
        let material = e
            .apply(Command::AddMaterial {
                name: "Tile".into(),
                color: [1.; 4],
                texture: Some(Texture { image, size: [0.5, 1.] }),
            })
            .unwrap()
            .material;
        e.apply(Command::Paint { faces: vec![face], material, back: false }).unwrap();
        e.apply(Command::SetTexturePlacement {
            faces: vec![face],
            placement: Some(TexturePlacement {
                offset: [0.2, 0.3],
                rotation: 20.,
                pins: Some([[0., 0.], [1., 0.], [1.2, 1.], [0., 1.]]),
                ..Default::default()
            }),
        })
        .unwrap();
        (e, face)
    }
    #[test]
    fn copied_snapshot_survives_erase_paste_undo_and_json_transport() {
        let (mut e, face) = fixture();
        let before = e.doc().clone();
        let depth = e.undo_depth();
        let clip = Clipboard::capture(&e, vec![Element::Face(face)]).unwrap();
        assert_eq!(e.doc(), &before);
        assert_eq!(e.undo_depth(), depth);
        let clip: Clipboard = serde_json::from_str(&serde_json::to_string(&clip).unwrap()).unwrap();
        e.apply(Command::Erase { elements: vec![Element::Face(face)] }).unwrap();
        let before_paste = e.doc().clone();
        let transform = DMat4::from_translation(DVec3::X * 5.) * clip.source_transform;
        let out = e.apply(Command::Paste { clipboard: Box::new(clip), transform, reuse_definitions: true }).unwrap();
        assert!(out.created.iter().any(|e| matches!(e, Element::Face(_))));
        let pasted = e.mesh().faces().next().unwrap().0;
        let texture = e.doc().materials[&e.mesh().face(pasted).unwrap().material.unwrap()].texture.unwrap();
        let original = before.root.mesh.face(face).unwrap().uv.unwrap();
        let placement = e.mesh().face(pasted).unwrap().uv.unwrap();
        let a = crate::uv_at(DVec3::new(2.4, 3.5, 1.), DVec3::Z, texture.size, &original);
        let b = crate::uv_at(DVec3::new(7.4, 3.5, 1.), DVec3::Z, texture.size, &placement);
        assert!((a[0] - b[0]).abs() < 1e-8 && (a[1] - b[1]).abs() < 1e-8);
        assert_eq!(e.doc().materials.len(), 1);
        assert_eq!(e.doc().images.len(), 1);
        e.undo();
        assert_eq!(e.doc(), &before_paste);
        e.redo();
        assert_eq!(e.mesh().faces().count(), 1);
    }
    #[test]
    fn cross_document_paste_remaps_colliding_ids_and_keeps_materials() {
        let (e, face) = fixture();
        let clip = Clipboard::capture(&e, vec![Element::Face(face)]).unwrap();
        let mut other = Editor::new();
        other.apply(Command::AddMaterial { name: "Tile".into(), color: [0., 0., 0., 1.], texture: None }).unwrap();
        other
            .apply(Command::Paste {
                transform: clip.source_transform,
                clipboard: Box::new(clip),
                reuse_definitions: false,
            })
            .unwrap();
        assert_eq!(other.doc().materials.len(), 2);
        other.mesh().validate().unwrap();
        let material = other.mesh().faces().next().unwrap().1.material.unwrap();
        assert!(other.doc().materials[&material].texture.is_some());
    }
    #[test]
    fn components_keep_definition_links_while_groups_get_frozen_copies() {
        for group in [false, true] {
            let (mut e, face) = fixture();
            let out = e
                .apply(if group {
                    Command::MakeGroup { elements: vec![Element::Face(face)] }
                } else {
                    Command::MakeComponent { elements: vec![Element::Face(face)], name: Some("Panel".into()) }
                })
                .unwrap();
            let Element::Instance(original) = out.created[0] else { panic!() };
            let definition = e.active().instances[&original].definition;
            let clip = Clipboard::capture(&e, out.created).unwrap();
            let transform = DMat4::from_translation(DVec3::X * 5.) * clip.source_transform;
            let out =
                e.apply(Command::Paste { clipboard: Box::new(clip), transform, reuse_definitions: true }).unwrap();
            let copied =
                out.created.iter().find_map(|e| if let Element::Instance(i) = e { Some(*i) } else { None }).unwrap();
            assert_eq!(e.active().instances[&copied].definition == definition, !group);
        }
    }
    #[test]
    fn pasting_a_face_does_not_fill_its_hole() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 4., v: DVec3::Y * 4., facing: None })
            .unwrap();
        e.apply(Command::DrawRectangle { corner: DVec3::new(1., 1., 0.), u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let hole = e.mesh().faces().find(|(f, _)| e.mesh().face_contains(*f, DVec3::new(1.5, 1.5, 0.))).unwrap().0;
        e.apply(Command::Erase { elements: vec![Element::Face(hole)] }).unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        assert_eq!(e.mesh().face(face).unwrap().holes.len(), 1);
        let clip = Clipboard::capture(&e, vec![Element::Face(face)]).unwrap();
        let transform = DMat4::from_translation(DVec3::X * 6.) * clip.source_transform;
        e.apply(Command::Paste { clipboard: Box::new(clip), transform, reuse_definitions: true }).unwrap();
        assert_eq!(e.mesh().faces().count(), 2);
        assert!(!e.mesh().faces().any(|(f, _)| e.mesh().face_contains(f, DVec3::new(7.5, 1.5, 0.))));
    }
    #[test]
    fn hole_paste_does_not_erase_an_existing_destination_face() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 4., v: DVec3::Y * 4., facing: None })
            .unwrap();
        e.apply(Command::DrawRectangle { corner: DVec3::new(1., 1., 0.), u: DVec3::X, v: DVec3::Y, facing: None })
            .unwrap();
        let hole = e.mesh().faces().find(|(f, _)| e.mesh().face_contains(*f, DVec3::new(1.5, 1.5, 0.))).unwrap().0;
        e.apply(Command::Erase { elements: vec![Element::Face(hole)] }).unwrap();
        let source = e.mesh().faces().next().unwrap().0;
        let clip = Clipboard::capture(&e, vec![Element::Face(source)]).unwrap();
        e.apply(Command::DrawRectangle { corner: DVec3::X * 6., u: DVec3::X * 4., v: DVec3::Y * 4., facing: None })
            .unwrap();
        e.apply(Command::Paste {
            transform: DMat4::from_translation(DVec3::X * 6.) * clip.source_transform,
            clipboard: Box::new(clip),
            reuse_definitions: true,
        })
        .unwrap();
        assert!(e.mesh().faces().any(|(f, _)| e.mesh().face_contains(f, DVec3::new(7.5, 1.5, 0.))));
        e.mesh().validate().unwrap();
    }
    #[test]
    fn copying_from_a_painted_group_keeps_inherited_material() {
        let (mut e, face) = fixture();
        let material = e.mesh().face(face).unwrap().material;
        e.apply(Command::Paint { faces: vec![face], material: None, back: false }).unwrap();
        let made = e.apply(Command::MakeGroup { elements: vec![Element::Face(face)] }).unwrap();
        let Element::Instance(group) = made.created[0] else { panic!() };
        e.apply(Command::PaintInstances { instances: vec![group], material }).unwrap();
        e.enter(group).unwrap();
        let face = e.mesh().faces().next().unwrap().0;
        let clip = Clipboard::capture(&e, vec![Element::Face(face)]).unwrap();
        let copied = clip.document.root.mesh.faces().next().unwrap().1;
        assert_eq!(copied.material, material);
        assert_eq!(copied.back_material, material);
    }
}
