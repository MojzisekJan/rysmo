use crate::{Definition, DefinitionId, Document, Instance, InstanceId, MaterialId, Tag, TagId, TextId};
use rysmo_math::{DMat4, DVec2, DVec3};

fn instance(definition: u64) -> Instance {
    Instance {
        definition: DefinitionId(definition),
        transform: DMat4::IDENTITY,
        name: String::new(),
        tag: None,
        material: None,
        hidden: false,
        glued_to: None,
        attributes: Default::default(),
    }
}
fn fixture() -> Document {
    let mut doc = Document::new();
    let text = crate::text::TextDefaults::default().screen("Note".into(), DVec2::ZERO, DVec2::splat(100.)).unwrap();
    doc.root_mut().texts.insert(TextId(100), text.clone());
    let mut leaf = Definition::default();
    leaf.texts.insert(TextId(101), text);
    let mut branch = Definition::default();
    branch.instances.insert(InstanceId(20), instance(2));
    doc.definitions.insert(DefinitionId(1), branch.into());
    doc.definitions.insert(DefinitionId(2), leaf.into());
    let mut first = instance(1);
    first.material = Some(MaterialId(99));
    doc.root_mut().instances.insert(InstanceId(10), first);
    let mut second = instance(1);
    second.transform = DMat4::from_translation(DVec3::X * 2.);
    doc.root_mut().instances.insert(InstanceId(11), second);
    let mut hidden = instance(2);
    hidden.hidden = true;
    doc.root_mut().instances.insert(InstanceId(12), hidden);
    doc.tags.insert(TagId(30), Tag { name: "Hidden".into(), visible: false });
    let mut tagged = instance(2);
    tagged.tag = Some(TagId(30));
    doc.root_mut().instances.insert(InstanceId(13), tagged);
    doc
}
#[test]
fn annotation_traversal_matches_visible_full_traversal_in_each_context() {
    let doc = fixture();
    for context in [vec![], vec![InstanceId(10)], vec![InstanceId(10), InstanceId(20)]] {
        let expected: Vec<_> = doc
            .occurrences(&context)
            .into_iter()
            .filter(|o| {
                let definition = doc.def(o.definition).unwrap();
                !definition.texts.is_empty() || !definition.dimensions.is_empty()
            })
            .collect();
        assert_eq!(doc.annotation_occurrences(&context), expected);
        assert_eq!(expected.len(), 3);
    }
}
#[test]
fn occurrence_paths_and_inherited_values_do_not_leak_between_siblings() {
    let doc = fixture();
    let occurrences = doc.occurrences(&[InstanceId(10)]);
    let leaves: Vec<_> = occurrences.iter().filter(|o| o.definition == Some(DefinitionId(2))).collect();
    assert_eq!(leaves.len(), 2);
    assert_eq!(leaves[0].path, vec![InstanceId(10), InstanceId(20)]);
    assert_eq!(leaves[0].top, Some(InstanceId(20)));
    assert!(leaves[0].in_context);
    assert_eq!(leaves[0].material, Some(MaterialId(99)));
    assert_eq!(leaves[1].path, vec![InstanceId(11), InstanceId(20)]);
    assert_eq!(leaves[1].top, None);
    assert!(!leaves[1].in_context);
    assert_eq!(leaves[1].material, None);
    assert_eq!(leaves[1].transform, DMat4::from_translation(DVec3::X * 2.));
}
#[test]
fn annotation_free_models_return_no_occurrences() {
    let mut doc = fixture();
    doc.root_mut().texts.clear();
    doc.def_mut(Some(DefinitionId(2))).unwrap().texts.clear();
    assert!(doc.annotation_occurrences(&[]).is_empty());
    assert_eq!(doc.occurrences(&[]).len(), 5);
}
