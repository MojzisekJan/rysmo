//! Saved views with independently selectable properties, matching the scene manager.
use crate::{DefinitionId, Document, EdgeId, Style, TagId};
use rysmo_math::{DrawingAxes, OrbitCamera};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneProperties {
    pub camera: bool,
    pub hidden_geometry: bool,
    pub tags: bool,
    pub sections: bool,
    pub style: bool,
    pub shadows: bool,
    pub axes: bool,
}
impl Default for SceneProperties {
    fn default() -> Self {
        Self { camera: true, hidden_geometry: true, tags: true, sections: true, style: true, shadows: true, axes: true }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ContextState {
    pub definition: Option<DefinitionId>,
    pub hidden_edges: BTreeSet<EdgeId>,
    #[serde(default)]
    pub hidden_faces: BTreeSet<crate::FaceId>,
    #[serde(default)]
    pub hidden_instances: BTreeSet<crate::InstanceId>,
    #[serde(default)]
    pub hidden_guides: BTreeSet<crate::GuideId>,
    pub section: Option<u64>,
    pub axes: DrawingAxes,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub include_in_animation: bool,
    pub properties: SceneProperties,
    pub camera: OrbitCamera,
    pub style: Style,
    pub tags: BTreeMap<TagId, bool>,
    pub contexts: Vec<ContextState>,
}
impl Scene {
    pub fn capture(doc: &Document, camera: OrbitCamera, name: String) -> Self {
        Self {
            id: 0,
            name,
            description: String::new(),
            include_in_animation: true,
            properties: Default::default(),
            camera,
            style: doc.style,
            tags: doc.tags.iter().map(|(id, tag)| (*id, tag.visible)).collect(),
            contexts: std::iter::once((None, &doc.root))
                .chain(doc.definitions.iter().map(|(id, d)| (Some(*id), d)))
                .map(|(definition, d)| ContextState {
                    definition,
                    hidden_faces: d.mesh.faces().filter(|(_, f)| f.hidden).map(|(id, _)| id).collect(),
                    hidden_edges: d.mesh.edges().filter(|(_, e)| e.hidden).map(|(id, _)| id).collect(),
                    hidden_instances: d.instances.iter().filter(|(_, i)| i.hidden).map(|(id, _)| *id).collect(),
                    hidden_guides: d.guides.iter().filter(|(_, g)| g.hidden()).map(|(id, _)| *id).collect(),
                    section: d.sections.active,
                    axes: d.axes,
                })
                .collect(),
        }
    }
    /// Update only the requested properties that this scene is configured to save.
    pub fn update(&mut self, doc: &Document, camera: OrbitCamera, requested: SceneProperties) {
        let current = Self::capture(doc, camera, self.name.clone());
        let p = self.properties;
        if p.camera && requested.camera {
            self.camera = camera;
        }
        if p.tags && requested.tags {
            self.tags = current.tags;
        }
        if p.style && requested.style {
            let previous = self.style;
            self.style = current.style;
            self.style.copy_lighting_from(&previous);
        }
        if p.shadows && requested.shadows {
            self.style.copy_lighting_from(&current.style);
        }
        for now in current.contexts {
            let index = self.contexts.iter().position(|c| c.definition == now.definition);
            let old = if let Some(index) = index {
                &mut self.contexts[index]
            } else {
                self.contexts.push(ContextState {
                    definition: now.definition,
                    hidden_edges: Default::default(),
                    hidden_faces: Default::default(),
                    hidden_instances: Default::default(),
                    hidden_guides: Default::default(),
                    section: None,
                    axes: Default::default(),
                });
                self.contexts.last_mut().unwrap()
            };
            if p.hidden_geometry && requested.hidden_geometry {
                old.hidden_edges = now.hidden_edges;
                old.hidden_faces = now.hidden_faces;
                old.hidden_instances = now.hidden_instances;
                old.hidden_guides = now.hidden_guides;
            }
            if p.sections && requested.sections {
                old.section = now.section;
            }
            if p.axes && requested.axes {
                old.axes = now.axes;
            }
        }
    }
    pub fn valid(&self) -> bool {
        let c = self.camera;
        let mut owners = BTreeSet::new();
        self.id > 0
            && c.target.is_finite()
            && c.distance.is_finite()
            && c.distance > 0.
            && c.roll.is_finite()
            && c.yaw.is_finite()
            && c.pitch.is_finite()
            && c.eye_base_z.is_finite()
            && c.fov_y.is_finite()
            && c.fov_y > 0.
            && c.fov_y < std::f64::consts::PI
            && self.style.valid_sections()
            && self.style.valid_render()
            && self.style.dimensions.validate()
            && self.style.dimension_defaults.validate()
            && self.style.text_defaults.validate()
            && self.contexts.iter().all(|s| s.axes.valid() && owners.insert(s.definition))
    }
    pub fn restore(&self, doc: &mut Document) {
        let p = self.properties;
        if p.camera {
            doc.camera = Some(self.camera);
        }
        if p.tags {
            for (id, visible) in &self.tags {
                if let Some(tag) = doc.tags.get_mut(id) {
                    tag.visible = *visible;
                }
            }
        }
        if p.style {
            let previous = doc.style;
            doc.style = self.style;
            doc.style.copy_lighting_from(&previous);
        }
        if p.shadows {
            doc.style.copy_lighting_from(&self.style);
        }
        for state in &self.contexts {
            let Some(d) = doc.def_mut(state.definition) else { continue };
            if p.axes {
                d.axes = state.axes;
            }
            if p.sections {
                d.sections.active = state.section.filter(|id| d.sections.planes.contains_key(id));
            }
            if p.hidden_geometry {
                let faces: Vec<_> = d.mesh.faces().map(|(id, _)| id).collect();
                for id in faces {
                    d.mesh.face_mut(id).unwrap().hidden = state.hidden_faces.contains(&id);
                }
                for (id, guide) in &mut d.guides {
                    guide.set_hidden(state.hidden_guides.contains(id));
                }
                for (id, instance) in &mut d.instances {
                    instance.hidden = state.hidden_instances.contains(id);
                }
                let edges: Vec<_> = d.mesh.edges().map(|(id, _)| id).collect();
                for id in edges {
                    d.mesh.set_edge_flags(&[id], None, None, Some(state.hidden_edges.contains(&id)));
                }
            }
        }
    }
    pub fn scaled(mut self, scale: f64) -> Self {
        self.camera.target *= scale;
        self.camera.distance *= scale;
        self.camera.eye_base_z *= scale;
        for state in &mut self.contexts {
            state.axes.origin *= scale;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Command, Editor, FaceStyle};
    use rysmo_math::DVec3;
    #[test]
    fn lighting_controls_follow_scene_shadow_property_independently() {
        let mut doc = Document::new();
        let mut scene = Scene::capture(&doc, OrbitCamera::default(), "Lighting".into());
        scene.style.sun_intensity = 2.;
        scene.style.ambient_intensity = 0.;
        scene.style.shadow_quality = crate::ShadowQuality::Low;
        scene.properties.shadows = false;
        scene.restore(&mut doc);
        assert_eq!(doc.style.sun_intensity, 1.);
        assert_eq!(doc.style.shadow_quality, crate::ShadowQuality::High);
        scene.properties.style = false;
        scene.properties.shadows = true;
        scene.restore(&mut doc);
        assert_eq!(doc.style.sun_intensity, 2.);
        assert_eq!(doc.style.ambient_intensity, 0.);
        assert_eq!(doc.style.shadow_quality, crate::ShadowQuality::Low);
    }
    #[test]
    fn scene_restores_selected_properties_and_history_without_changing_geometry() {
        let mut editor = Editor::new();
        editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let edge = editor.mesh().edges().next().unwrap().0;
        let tag = editor.apply(Command::AddTag { name: "Layer".into() }).unwrap().tag.unwrap();
        let mut source = editor.doc().clone();
        source.root.mesh.set_edge_flags(&[edge], None, None, Some(true));
        source.tags.get_mut(&tag).unwrap().visible = false;
        source.style.faces = FaceStyle::Wireframe;
        source.style.shadows = true;
        source.root.axes.origin = DVec3::X;
        let camera = OrbitCamera { parallel: true, target: DVec3::new(5., 4., 3.), ..Default::default() };
        let mut scene = Scene::capture(&source, camera, "Top".into());
        scene.properties.shadows = false;
        editor.apply(Command::AddScene { scene }).unwrap();
        let id = editor.doc().scenes[0].id;
        let before = editor.doc().clone();
        editor.apply(Command::RestoreScene { id }).unwrap();
        assert_eq!(editor.doc().camera, Some(camera));
        assert!(!editor.doc().tags[&tag].visible);
        assert!(editor.mesh().edge(edge).unwrap().hidden);
        assert_eq!(editor.doc().style.faces, FaceStyle::Wireframe);
        assert!(!editor.doc().style.shadows);
        assert_eq!(editor.active().axes.origin, DVec3::X);
        assert_eq!(editor.mesh().vertices().count(), before.mesh().vertices().count());
        let after = editor.doc().clone();
        assert!(editor.undo());
        assert_eq!(editor.doc(), &before);
        assert!(editor.redo());
        assert_eq!(editor.doc(), &after);
    }
    #[test]
    fn selective_update_and_deleted_references_are_safe() {
        let mut doc = Document::new();
        let camera = OrbitCamera::default();
        let mut scene = Scene::capture(&doc, camera, "Saved".into());
        scene.id = 1;
        scene.properties.camera = false;
        doc.style.shadows = true;
        doc.style.faces = FaceStyle::Wireframe;
        let requested = SceneProperties { shadows: false, ..Default::default() };
        scene.update(&doc, OrbitCamera { distance: 42., ..camera }, requested);
        assert_eq!(scene.camera, camera);
        assert_eq!(scene.style.faces, FaceStyle::Wireframe);
        assert!(!scene.style.shadows);
        scene.contexts[0].section = Some(999);
        scene.contexts.push(ContextState {
            definition: Some(DefinitionId(999)),
            hidden_edges: Default::default(),
            hidden_faces: Default::default(),
            hidden_instances: Default::default(),
            hidden_guides: Default::default(),
            section: None,
            axes: Default::default(),
        });
        assert!(scene.valid());
        scene.properties.shadows = false;
        scene.restore(&mut doc);
        assert_eq!(doc.root.sections.active, None);
        assert!(doc.style.shadows);
    }
    #[test]
    fn scene_reordering_deletion_and_invalid_edits_are_atomic() {
        let mut editor = Editor::new();
        for name in ["A", "B"] {
            let scene = Scene::capture(editor.doc(), OrbitCamera::default(), name.into());
            editor.apply(Command::AddScene { scene }).unwrap();
        }
        let id = editor.doc().scenes[0].id;
        editor.apply(Command::ReorderScene { id, index: 1 }).unwrap();
        assert_eq!(editor.doc().scenes[1].name, "A");
        let before = editor.doc().clone();
        let mut scene = before.scenes[1].clone();
        scene.camera.distance = -1.;
        assert!(editor.apply(Command::SetScene { id, scene: Some(scene) }).is_err());
        assert_eq!(editor.doc(), &before);
        editor.apply(Command::SetScene { id, scene: None }).unwrap();
        assert_eq!(editor.doc().scenes.len(), 1);
        editor.undo();
        assert_eq!(editor.doc(), &before);
    }
}
