//! The faces on screen: meshes built from the occurrence list, one per material group
//! (untextured, each texture image, see-through), styled by the model's face style; and
//! the edge list that the gizmos draw from.
//!
//! In the flat lighting the faces are unlit and shaded here from a fixed light, which is
//! the default look and the same on every GPU. In sun lighting the materials are
//! lit by the sun and the sky, and can receive shadows and ambient occlusion.
//!
//! Faces of a glowing material give off their light: lit, they shine with it, and each
//! occurrence of them carries a point light of the same output at their centre.

use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormats, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor, ImageType,
};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use rysmo_math::{DMat4, DVec3};
use rysmo_mesh::spatial::Aabb;
use rysmo_mesh::{
    DefinitionId, EdgeId, Element, FaceId, ImageId, InstanceId, MaterialId, MeshView, VertexId, triangulate_face,
};
use rysmo_model::sections::SectionPlane;
use rysmo_model::{FaceStyle, Lighting, Occurrence};

use crate::state::App3d;

pub const FRONT: [f32; 3] = [0.96, 0.95, 0.92];
pub const BACK: [f32; 3] = [0.64, 0.70, 0.80];
pub const SELECTED: [f32; 3] = [0.35, 0.55, 1.0];

pub fn plugin(app: &mut App) {
    app.init_resource::<FaceMeshes>()
        .init_resource::<EdgeCache>()
        .init_resource::<DrawnCache>()
        .init_resource::<Highlight>()
        .add_systems(
            PostUpdate,
            (update_edge_cache, (rebuild_faces, rebuild_highlight)).chain().before(TransformSystems::Propagate),
        );
}

/// Which mesh a face's triangles go into.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Group {
    Plain,
    Textured(ImageId),
    /// Blended faces, with their texture if they have one.
    SeeThrough(Option<ImageId>),
    /// Faces of a glowing material, with its texture if it has one.
    Glowing(MaterialId, Option<ImageId>),
}

/// What every piece of the viewport depends on, so a change to it redraws them all.
#[derive(PartialEq)]
struct Look {
    style: rysmo_model::Style,
    materials: BTreeMap<MaterialId, rysmo_model::Material>,
    images: Vec<AssetId<Image>>,
}

/// The viewport is split into pieces, each with its own meshes, so an edit rebuilds only
/// those it touches. Occurrences of one branch of the instance tree share a piece, and
/// large meshes get a piece per chunk.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct PieceKey {
    branch: u64,
    owner: u64,
    block: u64,
}

struct Piece {
    signature: u64,
    /// Keeps the chunks alive, so a pointer in a signature cannot come to mean another.
    _chunks: Vec<Arc<FaceChunk>>,
    entities: Vec<Entity>,
    glows: Vec<((u64, MaterialId), (f64, DVec3))>,
}

struct PlanEntry {
    placed: usize,
    path: u64,
    faces: Arc<DrawnFaces>,
    chunk: Arc<FaceChunk>,
}

#[derive(Default)]
struct PiecePlan {
    entries: Vec<PlanEntry>,
    signature: u64,
}

/// What each piece should hold now. Equal signatures mean equal triangles.
fn plan_pieces(doc: &rysmo_model::Document, edges: &EdgeCache, drawn: &mut DrawnCache) -> HashMap<PieceKey, PiecePlan> {
    let mut plans: HashMap<PieceKey, (PiecePlan, DefaultHasher)> = HashMap::new();
    for (index, placed) in edges.placed.iter().enumerate() {
        let occ = &placed.occurrence;
        if doc.def(occ.definition).is_none() {
            continue;
        }
        let faces = drawn.get(doc, occ.definition);
        let branch = occ.path.first().map_or(0, |id| (id.0 >> PIECE_BITS) + 1);
        let mut state = DefaultHasher::new();
        occ.path.hash(&mut state);
        let path = state.finish();
        occ.transform.to_cols_array().map(f64::to_bits).hash(&mut state);
        occ.material.hash(&mut state);
        occ.in_context.hash(&mut state);
        for cut in &placed.cuts {
            for value in cut.origin.to_array().into_iter().chain(cut.normal.to_array()) {
                value.to_bits().hash(&mut state);
            }
        }
        let occurrence = state.finish();
        for chunk in &faces.chunks {
            let large = faces.chunks.len() > 1;
            let key = PieceKey {
                branch,
                owner: if large { occ.definition.map_or(1, |id| id.0 + 2) } else { 0 },
                block: if large { chunk.block + 1 } else { 0 },
            };
            let (plan, hasher) = plans.entry(key).or_default();
            occurrence.hash(hasher);
            Arc::as_ptr(chunk).hash(hasher);
            plan.entries.push(PlanEntry { placed: index, path, faces: faces.clone(), chunk: chunk.clone() });
        }
    }
    plans
        .into_iter()
        .map(|(key, (mut plan, hasher))| {
            plan.signature = hasher.finish();
            (key, plan)
        })
        .collect()
}

/// The most glowing occurrences that carry a light, the brightest first.
const MAX_LIGHTS: usize = 64;
/// How many of those lights cast shadows, when shadows are on.
const SHADOWED_LIGHTS: usize = 8;

type CachedImage = (Arc<Vec<u8>>, Handle<Image>);

#[derive(Resource, Default)]
struct FaceMeshes {
    key: Option<(u64, u64, Vec<InstanceId>)>,
    look: Option<Look>,
    pieces: HashMap<PieceKey, Piece>,
    materials: HashMap<(Group, Option<MaterialId>), Handle<StandardMaterial>>,
    lights: Vec<Entity>,
    texture_settings: Option<(u32, u16)>,
    /// Decoded images, by id, with the bytes they came from.
    images: HashMap<ImageId, CachedImage>,
    normals: HashMap<(ImageId, u32), CachedImage>,
    /// Metallic-roughness images, decoded as linear data.
    surfaces: HashMap<ImageId, CachedImage>,
}

/// Log2 of the id range a chunk of a definition's faces covers.
const CHUNK_BITS: u32 = 12;
/// Top-level instances that share a piece of the viewport, as a power of two.
const PIECE_BITS: u32 = 7;
/// Above this many changed vertices times chunks, rebuilding is cheaper than checking.
const CHUNK_CHECK_LIMIT: usize = 4_000_000;

/// Triangles of the faces in one id range, ready to place at each occurrence. A chunk
/// stays while its faces and vertices do, so one edit leaves most of a mesh's alone.
struct FaceChunk {
    block: u64,
    /// In id order.
    faces: Vec<DrawnFace>,
    vertices: HashSet<VertexId>,
    smooth: HashMap<(FaceId, VertexId), DVec3>,
}

struct DrawnFace {
    id: FaceId,
    normal: DVec3,
    triangles: Vec<[VertexId; 3]>,
}

impl FaceChunk {
    fn new(mesh: &rysmo_mesh::Mesh, view: &MeshView, block: u64, ids: &[FaceId]) -> Self {
        let faces: Vec<_> = ids
            .iter()
            .filter_map(|id| {
                Some(DrawnFace { id: *id, normal: mesh.face_normal(*id)?, triangles: triangulate_face(mesh, *id) })
            })
            .collect();
        let vertices: HashSet<VertexId> =
            ids.iter().filter_map(|id| mesh.face(*id)).flat_map(|f| f.loops().flatten().copied()).collect();
        let corners: BTreeSet<_> = vertices.iter().copied().collect();
        let mut smooth = mesh.smooth_normals_at(view, &corners);
        smooth.retain(|(face, _), _| face.0 >> CHUNK_BITS == block);
        FaceChunk { block, faces, vertices, smooth }
    }
}

/// The chunks of a definition's faces.
struct DrawnFaces {
    mesh: rysmo_mesh::Mesh,
    /// By block number.
    chunks: Vec<Arc<FaceChunk>>,
}

/// The vertices of `mesh` whose triangles or corner normals may differ from `older`.
fn affected_vertices(
    mesh: &rysmo_mesh::Mesh,
    older: &rysmo_mesh::Mesh,
    view: &MeshView,
) -> (BTreeSet<FaceId>, BTreeSet<VertexId>) {
    let (faces, vertices) = mesh.changed_since(older);
    let mut affected: BTreeSet<VertexId> = vertices.iter().copied().collect();
    // A corner's normal depends on all of the faces at it, and those on all their corners.
    let moved = view.faces_touching(mesh, &affected);
    let faces: BTreeSet<_> = faces.into_iter().chain(moved).collect();
    for face in faces.iter().flat_map(|id| mesh.face(*id).into_iter().chain(older.face(*id))) {
        affected.extend(face.loops().flatten().copied());
    }
    for edge in mesh.changed_edges_since(older).iter().filter_map(|id| mesh.edge(*id)) {
        affected.extend(edge.v);
    }
    (faces, affected)
}

impl DrawnFaces {
    /// Builds the chunks of `mesh`, keeping those of `older` that still hold.
    fn new(mesh: rysmo_mesh::Mesh, older: Option<&DrawnFaces>) -> Self {
        let mut by_block: BTreeMap<u64, Vec<FaceId>> = BTreeMap::new();
        for (id, _) in mesh.faces() {
            by_block.entry(id.0 >> CHUNK_BITS).or_default().push(id);
        }
        let view = mesh.view();
        let changes = older.map(|older| affected_vertices(&mesh, &older.mesh, &view));
        let dirty: BTreeSet<u64> = changes.iter().flat_map(|(faces, _)| faces).map(|id| id.0 >> CHUNK_BITS).collect();
        let vertices: Vec<_> = changes.iter().flat_map(|(_, vertices)| vertices).copied().collect();
        let check_vertices = vertices.len().saturating_mul(by_block.len()) <= CHUNK_CHECK_LIMIT;
        let chunks = by_block
            .into_iter()
            .map(|(block, ids)| {
                let known = older
                    .filter(|_| check_vertices && !dirty.contains(&block))
                    .and_then(|older| older.chunk(block))
                    .filter(|chunk| !vertices.iter().any(|v| chunk.vertices.contains(v)));
                known.cloned().unwrap_or_else(|| Arc::new(FaceChunk::new(&mesh, &view, block, &ids)))
            })
            .collect();
        DrawnFaces { mesh, chunks }
    }

    fn chunk(&self, block: u64) -> Option<&Arc<FaceChunk>> {
        let index = self.chunks.binary_search_by_key(&block, |chunk| chunk.block).ok()?;
        Some(&self.chunks[index])
    }

    fn face(&self, id: FaceId) -> Option<&DrawnFace> {
        let faces = &self.chunk(id.0 >> CHUNK_BITS)?.faces;
        faces.binary_search_by_key(&id, |face| face.id).ok().map(|index| &faces[index])
    }
}

/// Triangulations by definition. Holding the definition makes an edit copy it rather
/// than change it in place, so an equal pointer means the same geometry. Evaluated
/// meshes depend on other definitions and are looked at again each content revision.
#[derive(Resource, Default)]
struct DrawnCache {
    kept: HashMap<Option<DefinitionId>, (Arc<rysmo_model::Definition>, Arc<DrawnFaces>)>,
    /// Which evaluated meshes are up to date.
    evaluated: HashSet<Option<DefinitionId>>,
    revision: Option<(u64, u64)>,
}

impl DrawnCache {
    fn prepare(&mut self, doc: &rysmo_model::Document, revision: (u64, u64)) {
        if self.revision != Some(revision) {
            self.revision = Some(revision);
            self.evaluated.clear();
            self.kept.retain(|owner, _| doc.def(*owner).is_some());
        }
    }

    fn get(&mut self, doc: &rysmo_model::Document, owner: Option<DefinitionId>) -> Arc<DrawnFaces> {
        let definition = doc.def_arc(owner).expect("an occurrence's definition");
        let known = self.kept.get(&owner);
        if let Some((held, faces)) = known
            && (self.evaluated.contains(&owner) || (Arc::ptr_eq(held, definition) && !has_openings(definition)))
        {
            return faces.clone();
        }
        let evaluated = doc.effective_mesh(owner);
        let faces = Arc::new(DrawnFaces::new(evaluated.into_owned(), known.map(|(_, faces)| faces.as_ref())));
        if has_openings(definition) {
            self.evaluated.insert(owner);
        }
        self.kept.insert(owner, (definition.clone(), faces.clone()));
        faces
    }
}

fn has_openings(definition: &rysmo_model::Definition) -> bool {
    definition.instances.values().any(|instance| instance.glued_to.is_some())
}

/// An edge as the gizmos draw it: ends, the normals of its faces, a colour for
/// colour-by-material.
pub struct EdgeInfo {
    pub ghost: bool,
    pub id: EdgeId,
    pub a: DVec3,
    pub b: DVec3,
    pub normals: Vec<DVec3>,
    pub material: [f32; 3],
}

/// One place to draw a definition, with what that takes worked out once.
pub struct Placed {
    pub occurrence: Occurrence,
    pub inverse: DMat4,
    pub cuts: Vec<SectionPlane>,
    /// The first occurrence inside the context, which is the context itself.
    pub active: bool,
}

/// A definition's edges and the sphere around them.
#[derive(Default)]
pub struct DefinitionEdges {
    pub list: Vec<EdgeInfo>,
    pub centre: DVec3,
    pub radius: f64,
    /// Kept only for geometry that depends on nothing but its own definition.
    source: Option<Arc<rysmo_model::Definition>>,
    /// What the list was built from, to tell what has changed since.
    mesh: rysmo_mesh::Mesh,
}

/// Visible edges of every definition and the occurrence list, rebuilt when the document
/// changes, so profiles can be found every frame without searching for faces.
#[derive(Resource, Default)]
pub struct EdgeCache {
    revision: Option<(u64, u64, Vec<InstanceId>)>,
    edge_style: Option<(bool, Vec<[f32; 3]>)>,
    pub defs: HashMap<Option<DefinitionId>, DefinitionEdges>,
    pub placed: Vec<Placed>,
}

/// The edges of a definition. Those of `older` are kept where neither the edge, its
/// ends nor the faces along it have changed.
fn definition_edges(
    doc: &rysmo_model::Document,
    owner: Option<DefinitionId>,
    older: Option<DefinitionEdges>,
) -> DefinitionEdges {
    type Key = (VertexId, VertexId);
    let key = |a: VertexId, b: VertexId| if a < b { (a, b) } else { (b, a) };
    let evaluated = doc.effective_mesh(owner);
    let mesh = &*evaluated;
    let (older, source_mesh) = match older {
        Some(DefinitionEdges { list, mesh: known, .. }) => (list, Some(known)),
        None => (Vec::new(), None),
    };
    let changes = source_mesh.as_ref().map(|known| {
        let (faces, vertices) = mesh.changed_since(known);
        let mut along: HashSet<Key> = HashSet::new();
        for face in faces.iter().flat_map(|id| mesh.face(*id).into_iter().chain(known.face(*id))) {
            along.extend(face.loop_edges().into_iter().map(|(a, b)| key(a, b)));
        }
        (along, vertices.into_iter().collect::<HashSet<_>>(), mesh.changed_edges_since(known))
    });
    let stale = |id: EdgeId, e: &rysmo_mesh::Edge| {
        changes.as_ref().is_none_or(|(along, vertices, edges)| {
            edges.contains(&id) || e.v.iter().any(|v| vertices.contains(v)) || along.contains(&key(e.v[0], e.v[1]))
        })
    };
    let wanted: HashSet<Key> = mesh.edges().filter(|(id, e)| stale(*id, e)).map(|(_, e)| key(e.v[0], e.v[1])).collect();
    let mut faces_of: HashMap<Key, Vec<FaceId>> = HashMap::new();
    for (fid, f) in mesh.faces() {
        for (a, b) in f.loop_edges() {
            if wanted.contains(&key(a, b)) {
                faces_of.entry(key(a, b)).or_default().push(fid);
            }
        }
    }
    let mut older = older.into_iter().peekable();
    let list: Vec<_> = mesh
        .edges()
        .filter(|(_, e)| !e.procedural && (e.visible() || doc.style.show_hidden_geometry))
        .map(|(eid, e)| {
            while older.next_if(|info| info.id < eid).is_some() {}
            let known = older.next_if(|info| info.id == eid);
            if let Some(info) = known.filter(|_| !stale(eid, e)) {
                return info;
            }
            let k = key(e.v[0], e.v[1]);
            let fs = faces_of.get(&k).cloned().unwrap_or_default();
            let material = e
                .material
                .and_then(|id| doc.materials.get(&id))
                .map(|m| [m.color[0], m.color[1], m.color[2]])
                .unwrap_or([0., 0., 0.]);
            let (a, b) = mesh.edge_points(eid);
            EdgeInfo {
                ghost: !e.visible(),
                id: eid,
                a,
                b,
                normals: fs.iter().filter_map(|f| mesh.face_normal(*f)).collect(),
                material,
            }
        })
        .collect();
    let bounds = Aabb::of(list.iter().flat_map(|e| [e.a, e.b]));
    let (centre, radius) = if list.is_empty() {
        (DVec3::ZERO, 0.0)
    } else {
        ((bounds.lo + bounds.hi) * 0.5, (bounds.hi - bounds.lo).length() * 0.5)
    };
    let source = matches!(evaluated, Cow::Borrowed(_)).then(|| doc.def_arc(owner).cloned()).flatten();
    DefinitionEdges { list, centre, radius, source, mesh: evaluated.into_owned() }
}

pub fn update_edge_cache(app: Res<App3d>, mut cache: ResMut<EdgeCache>) {
    let editor = &app.editor;
    let revision = (app.document_generation, editor.content_revision(), editor.context().to_vec());
    if cache.revision.as_ref() == Some(&revision) {
        return;
    }
    let doc = editor.doc();
    let colours = doc.materials.values().map(|m| [m.color[0], m.color[1], m.color[2]]).collect();
    let edge_style = Some((doc.style.show_hidden_geometry, colours));
    if cache.edge_style != edge_style {
        cache.defs.clear();
        cache.edge_style = edge_style;
    }
    let owners = std::iter::once(None).chain(doc.definitions.keys().copied().map(Some));
    let mut defs = HashMap::with_capacity(doc.definitions.len() + 1);
    for owner in owners {
        let known = cache.defs.remove(&owner);
        let same = known.as_ref().is_some_and(|edges| {
            edges.source.as_ref().is_some_and(|held| doc.def_arc(owner).is_some_and(|d| Arc::ptr_eq(held, d)))
        });
        let edges = match known {
            Some(edges) if same => edges,
            known => definition_edges(doc, owner, known),
        };
        defs.insert(owner, edges);
    }
    cache.defs = defs;
    let mut active_seen = false;
    cache.placed = doc
        .occurrences(editor.context())
        .into_iter()
        .map(|occurrence| {
            let active = occurrence.in_context && !active_seen;
            active_seen |= occurrence.in_context;
            let inverse = occurrence.transform.inverse();
            let cuts = doc.section_planes(&occurrence.path);
            Placed { occurrence, inverse, cuts, active }
        })
        .collect();
    cache.revision = Some(revision);
}

/// Lambert shading from a fixed light, kept bright so colours stay true.
fn shade(color: [f32; 4], normal: DVec3) -> [f32; 4] {
    let sun = DVec3::new(-0.35, -0.55, 0.76).normalize();
    let k = (0.62 + 0.38 * normal.dot(sun).max(0.0)) as f32;
    [color[0] * k, color[1] * k, color[2] * k, color[3]]
}

/// Geometry outside the context being edited is drawn faded.
fn fade(c: [f32; 4]) -> [f32; 4] {
    [c[0] * 0.35 + 0.6, c[1] * 0.35 + 0.62, c[2] * 0.35 + 0.65, c[3]]
}

#[derive(Default)]
struct Buffers {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
    /// The last colour converted. Faces repeat a colour, and conversion is slow.
    colour: Option<([f32; 4], [f32; 4])>,
}

impl Buffers {
    /// `c` is sRGB, like material colours; vertex colours are linear.
    fn push(&mut self, p: DVec3, n: DVec3, c: [f32; 4], uv: [f64; 2]) {
        self.idx.push(self.pos.len() as u32);
        self.pos.push([p.x as f32, p.y as f32, p.z as f32]);
        self.nrm.push([n.x as f32, n.y as f32, n.z as f32]);
        let linear = match self.colour {
            Some((srgb, linear)) if srgb == c => linear,
            _ => {
                let linear = LinearRgba::from(Color::srgba(c[0], c[1], c[2], c[3])).to_f32_array();
                self.colour = Some((c, linear));
                linear
            }
        };
        self.col.push(linear);
        self.uv.push([uv[0] as f32, uv[1] as f32]);
    }

    fn into_mesh(self) -> Mesh {
        // Picking and edits use the model mesh. These disposable draw buffers
        // can leave CPU memory once uploaded, and are rebuilt on model changes.
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_indices(Indices::U32(self.idx))
    }
}

fn decode(bytes: &[u8], mime: &str, limit: u32, anisotropy: u16) -> Option<Image> {
    let format = image::guess_format(bytes).ok()?;
    let (w, h) = image::ImageReader::with_format(std::io::Cursor::new(bytes), format).into_dimensions().ok()?;
    if w == 0 || h == 0 || w as u64 * h as u64 > 33_554_432 {
        return None;
    }
    let decoded = Image::from_buffer(
        bytes,
        ImageType::MimeType(mime),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::default(),
        RenderAssetUsages::default(),
    )
    .ok()?;
    let rgba = decoded.try_into_dynamic().ok()?.to_rgba8();
    let cap = if cfg!(target_arch = "wasm32") { 4096 } else { 8192 };
    let limit = if limit == 0 { cap } else { limit.min(cap) };
    let dynamic = image::DynamicImage::ImageRgba8(rgba);
    let mut level = if dynamic.width().max(dynamic.height()) > limit {
        dynamic.resize(limit, limit, image::imageops::FilterType::Triangle).to_rgba8()
    } else {
        dynamic.to_rgba8()
    };
    let (width, height) = level.dimensions();
    let mut data = Vec::new();
    let mut levels = 0;
    loop {
        data.extend_from_slice(level.as_raw());
        levels += 1;
        if level.width() == 1 && level.height() == 1 {
            break;
        }
        level = image::imageops::resize(
            &level,
            (level.width() / 2).max(1),
            (level.height() / 2).max(1),
            image::imageops::FilterType::Triangle,
        );
    }
    let mut image = Image::new_uninit(
        bevy::render::render_resource::Extent3d { width, height, depth_or_array_layers: 1 },
        bevy::render::render_resource::TextureDimension::D2,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        min_filter: ImageFilterMode::Linear,
        mag_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: anisotropy,
        ..ImageSamplerDescriptor::linear()
    });
    Some(image)
}

#[allow(clippy::too_many_arguments)]
fn rebuild_faces(
    mut commands: Commands,
    app: Res<App3d>,
    mut state: ResMut<FaceMeshes>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    edges: Res<EdgeCache>,
    mut drawn: ResMut<DrawnCache>,
) {
    let revision = (app.document_generation, app.editor.content_revision());
    let key = (revision.0, revision.1, app.editor.context().to_vec());
    if state.key.as_ref() == Some(&key) {
        return;
    }
    let editor = &app.editor;
    let doc = editor.doc();
    let style = doc.style;
    let lit = style.lighting != Lighting::Flat;
    if state.texture_settings != Some((style.texture_limit, style.texture_anisotropy)) {
        state.images.clear();
        state.normals.clear();
        state.surfaces.clear();
        state.texture_settings = Some((style.texture_limit, style.texture_anisotropy));
    }
    // Images: decode new ones, drop those gone.
    let color_images: BTreeSet<_> = doc.materials.values().filter_map(|m| m.texture.map(|t| t.image)).collect();
    state.images.retain(|id, (bytes, _)| {
        color_images.contains(id) && doc.images.get(id).is_some_and(|i| Arc::ptr_eq(&i.bytes, bytes))
    });
    for (id, img) in &doc.images {
        if color_images.contains(id)
            && !state.images.contains_key(id)
            && let Some(decoded) = decode(&img.bytes, &img.mime, style.texture_limit, style.texture_anisotropy)
        {
            let handle = images.add(decoded);
            state.images.insert(*id, (img.bytes.clone(), handle));
        }
    }
    let normal_keys: std::collections::HashSet<_> =
        doc.materials.values().filter_map(|m| m.pbr.normal_map.map(|id| (id, m.pbr.normal_scale.to_bits()))).collect();
    state.normals.retain(|key, (bytes, _)| {
        normal_keys.contains(key) && doc.images.get(&key.0).is_some_and(|image| Arc::ptr_eq(&image.bytes, bytes))
    });
    for (id, strength) in doc.materials.values().filter_map(|m| m.pbr.normal_map.map(|id| (id, m.pbr.normal_scale))) {
        let key = (id, strength.to_bits());
        if !state.normals.contains_key(&key)
            && let Some(image) = doc.images.get(&id)
            && let Some(mut decoded) = decode(&image.bytes, &image.mime, style.texture_limit, style.texture_anisotropy)
        {
            decoded.texture_descriptor.format = bevy::render::render_resource::TextureFormat::Rgba8Unorm;
            if let Some(data) = decoded.data.as_mut() {
                for pixel in data.as_chunks_mut::<4>().0 {
                    let normal = Vec3::new(
                        (pixel[0] as f32 / 127.5 - 1.) * strength,
                        (pixel[1] as f32 / 127.5 - 1.) * strength,
                        pixel[2] as f32 / 127.5 - 1.,
                    )
                    .normalize_or(Vec3::Z);
                    for (value, component) in pixel[..3].iter_mut().zip(normal.to_array()) {
                        *value = ((component + 1.) * 127.5).round() as u8;
                    }
                }
            }
            let handle = images.add(decoded);
            state.normals.insert(key, (image.bytes.clone(), handle));
        }
    }
    let surface_images: BTreeSet<_> = doc.materials.values().filter_map(|m| m.pbr.metallic_roughness_map).collect();
    state.surfaces.retain(|id, (bytes, _)| {
        surface_images.contains(id) && doc.images.get(id).is_some_and(|i| Arc::ptr_eq(&i.bytes, bytes))
    });
    for id in surface_images {
        if !state.surfaces.contains_key(&id)
            && let Some(image) = doc.images.get(&id)
            && let Some(mut decoded) = decode(&image.bytes, &image.mime, style.texture_limit, style.texture_anisotropy)
        {
            decoded.texture_descriptor.format = bevy::render::render_resource::TextureFormat::Rgba8Unorm;
            let handle = images.add(decoded);
            state.surfaces.insert(id, (image.bytes.clone(), handle));
        }
    }
    drawn.prepare(doc, revision);
    // What every piece's colours and materials depend on.
    let look = Look {
        style,
        materials: doc.materials.clone(),
        images: state.images.values().map(|(_, handle)| handle.id()).collect(),
    };
    if state.look.as_ref() != Some(&look) {
        for piece in std::mem::take(&mut state.pieces).into_values() {
            for entity in piece.entities {
                commands.entity(entity).despawn();
            }
        }
        state.materials.clear();
        state.look = Some(look);
    }
    let mut pieces = std::mem::take(&mut state.pieces);
    let mut shared_materials = std::mem::take(&mut state.materials);
    let plans = if style.faces == FaceStyle::Wireframe { HashMap::new() } else { plan_pieces(doc, &edges, &mut drawn) };
    pieces.retain(|key, piece| {
        let keep = plans.contains_key(key);
        if !keep {
            for entity in piece.entities.drain(..) {
                commands.entity(entity).despawn();
            }
        }
        keep
    });
    let glowing = |m: Option<MaterialId>| {
        m.filter(|m| {
            lit && matches!(style.faces, FaceStyle::Shaded | FaceStyle::Textured)
                && doc.materials.get(m).is_some_and(|m| m.glow > 0.0)
        })
    };
    let background = [style.background[0], style.background[1], style.background[2], 1.0];
    let colour = |m: Option<rysmo_mesh::MaterialId>, default: [f32; 3]| -> ([f32; 4], Option<rysmo_model::Texture>) {
        let rgba = [default[0], default[1], default[2], 1.0];
        match style.faces {
            FaceStyle::Monochrome | FaceStyle::Wireframe => (rgba, None),
            FaceStyle::HiddenLine => (background, None),
            FaceStyle::XRay => (rgba, None),
            FaceStyle::Shaded => (m.and_then(|m| doc.materials.get(&m)).map(|m| m.color).unwrap_or(rgba), None),
            FaceStyle::Textured => match m.and_then(|m| doc.materials.get(&m)) {
                Some(mat) => match mat.texture.filter(|t| state.images.contains_key(&t.image)) {
                    // The texture carries the colour; the vertex colour only shades it.
                    Some(t) => (if mat.pbr.tint_texture { mat.color } else { [1.0, 1.0, 1.0, mat.color[3]] }, Some(t)),
                    None => (mat.color, None),
                },
                None => (rgba, None),
            },
        }
    };
    // The model is Z up; Bevy is Y up. One rotation on each mesh maps one to the other.
    let to_y_up = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
    let unlit = !lit || style.faces == FaceStyle::HiddenLine;
    for (key, plan) in plans {
        if pieces.get(&key).is_some_and(|piece| piece.signature == plan.signature) {
            continue;
        }
        if let Some(old) = pieces.remove(&key) {
            for entity in old.entities {
                commands.entity(entity).despawn();
            }
        }
        let mut groups: HashMap<(Group, Option<MaterialId>), Buffers> = HashMap::new();
        // Glowing area by occurrence and material: total area and area-weighted centre.
        let mut glows: HashMap<(u64, MaterialId), (f64, DVec3)> = HashMap::new();
        for entry in &plan.entries {
            let placed = &edges.placed[entry.placed];
            let occ = &placed.occurrence;
            let mesh = &entry.faces.mesh;
            let t = occ.transform;
            let section_planes = &placed.cuts;
            let inverse = placed.inverse;
            let nt = inverse.transpose();
            let mirrored = t.determinant() < 0.0;
            for drawn_face in &entry.chunk.faces {
                let (fid, n, tris) = (drawn_face.id, drawn_face.normal, &drawn_face.triangles);
                let Some(face) = mesh.face(fid) else { continue };
                if face.hidden && !style.show_hidden_geometry {
                    continue;
                }
                let placement = face.uv.unwrap_or_default();
                for (side, material, default) in
                    [(1.0f64, face.material.or(occ.material), FRONT), (-1.0, face.back_material.or(occ.material), BACK)]
                {
                    let (mut color, texture) = colour(material, default);
                    if face.hidden {
                        color[3] = 0.12;
                    }
                    if style.faces == FaceStyle::XRay {
                        color[3] = style.xray_opacity;
                    }
                    if !occ.in_context {
                        color = fade(color);
                    }
                    let glow = glowing(material).filter(|_| !face.hidden && occ.in_context);
                    let group = if let Some(m) = glow {
                        Group::Glowing(m, texture.map(|t| t.image))
                    } else if color[3] < 0.999 {
                        Group::SeeThrough(texture.map(|t| t.image))
                    } else if let Some(t) = texture {
                        Group::Textured(t.image)
                    } else {
                        Group::Plain
                    };
                    let surface =
                        material.filter(|id| doc.materials.get(id).is_some_and(|m| m.pbr != Default::default()));
                    let buf = groups.entry((group, surface)).or_default();
                    let perspective = placement
                        .pins
                        .and_then(rysmo_model::pin_mapping)
                        .is_some_and(|m| m.x_axis.z.abs() + m.y_axis.z.abs() > 1e-12);
                    let normal_map =
                        material.and_then(|id| doc.materials.get(&id)).is_some_and(|m| m.pbr.images().next().is_some());
                    let uv_size = texture.map(|t| t.size).or_else(|| {
                        normal_map.then(|| {
                            material
                                .and_then(|id| doc.materials.get(&id))
                                .and_then(|m| m.texture)
                                .map(|t| t.size)
                                .unwrap_or([1., 1.])
                        })
                    });
                    let detail = if uv_size.is_some() && perspective { 8 } else { 1 };
                    let sample_weights = subdivided_weights(detail);
                    for tri in tris {
                        let mut order = if side > 0.0 { [tri[0], tri[1], tri[2]] } else { [tri[0], tri[2], tri[1]] };
                        if mirrored {
                            order.swap(1, 2);
                        }
                        let points = order.map(|v| t.transform_point3(mesh.pos(v)));
                        let normals = order.map(|v| {
                            let normal = if entry.chunk.smooth.is_empty() {
                                n
                            } else {
                                entry.chunk.smooth.get(&(fid, v)).copied().unwrap_or(n)
                            };
                            nt.transform_vector3(normal * side).normalize_or(DVec3::Z)
                        });
                        let uvs = order.map(|v| {
                            uv_size.map(|size| rysmo_model::uv_at(mesh.pos(v), n, size, &placement)).unwrap_or([0., 0.])
                        });
                        let polygon = rysmo_model::sections::clip_triangle(points, section_planes);
                        for i in 1..polygon.len().saturating_sub(1) {
                            let triangle = [polygon[0], polygon[i], polygon[i + 1]];
                            if let Some(m) = glow {
                                let [a, b, c] = triangle.map(|p| p.0);
                                let area = (b - a).cross(c - a).length() / 2.0;
                                let entry = glows.entry((entry.path, m)).or_insert((0.0, DVec3::ZERO));
                                entry.0 += area;
                                entry.1 += (a + b + c) / 3.0 * area;
                            }
                            for samples in &sample_weights {
                                for sample in samples {
                                    let point =
                                        triangle[0].0 * sample.x + triangle[1].0 * sample.y + triangle[2].0 * sample.z;
                                    let weights: [f64; 3] = std::array::from_fn(|j| {
                                        triangle[0].1[j] * sample.x
                                            + triangle[1].1[j] * sample.y
                                            + triangle[2].1[j] * sample.z
                                    });
                                    let world_n =
                                        (normals[0] * weights[0] + normals[1] * weights[1] + normals[2] * weights[2])
                                            .normalize_or(DVec3::Z);
                                    let shaded = if lit || style.faces == FaceStyle::HiddenLine {
                                        color
                                    } else {
                                        shade(color, world_n)
                                    };
                                    let uv = std::array::from_fn(|j| {
                                        uvs[0][j] * weights[0] + uvs[1][j] * weights[1] + uvs[2][j] * weights[2]
                                    });
                                    let uv = if let Some(size) = uv_size.filter(|_| placement.pins.is_some()) {
                                        rysmo_model::uv_at(inverse.transform_point3(point), n, size, &placement)
                                    } else {
                                        uv
                                    };
                                    buf.push(point, world_n, shaded, uv);
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut entities = Vec::new();
        for ((group, material_id), buf) in groups {
            let pbr = material_id.and_then(|id| doc.materials.get(&id)).map(|m| m.pbr).unwrap_or_default();
            if buf.idx.is_empty() {
                continue;
            }
            let texture = match group {
                Group::Textured(id) | Group::SeeThrough(Some(id)) | Group::Glowing(_, Some(id)) => {
                    state.images.get(&id).map(|(_, h)| h.clone())
                }
                _ => None,
            };
            // Emission in nits, scaled by the exposure like the lights.
            let emissive = match group {
                Group::Glowing(m, _) => doc.materials.get(&m).map_or(LinearRgba::BLACK, |m| {
                    let c = LinearRgba::from(Color::srgb(m.color[0], m.color[1], m.color[2]));
                    c * m.glow
                }),
                _ => LinearRgba::BLACK,
            };
            let material = shared_materials
                .entry((group, material_id))
                .or_insert_with(|| {
                    materials.add(StandardMaterial {
                        base_color: Color::WHITE,
                        base_color_texture: texture.clone(),
                        emissive,
                        emissive_texture: texture.filter(|_| emissive != LinearRgba::BLACK),
                        unlit,
                        perceptual_roughness: pbr.roughness,
                        metallic: pbr.metallic,
                        normal_map_texture: pbr.normal_map.and_then(|id| {
                            state.normals.get(&(id, pbr.normal_scale.to_bits())).map(|(_, h)| h.clone())
                        }),
                        flip_normal_map_y: pbr.normal_flip_y,
                        metallic_roughness_texture: pbr
                            .metallic_roughness_map
                            .and_then(|id| state.surfaces.get(&id).map(|(_, h)| h.clone())),
                        reflectance: 0.2,
                        alpha_mode: match (group, pbr.alpha_cutoff) {
                            (Group::SeeThrough(_), _) => AlphaMode::Blend,
                            (Group::Textured(_) | Group::Glowing(_, Some(_)), Some(cutoff)) => AlphaMode::Mask(cutoff),
                            _ => AlphaMode::Opaque,
                        },
                        // Each side is its own triangles, wound to face its way; culling shows one.
                        ..default()
                    })
                })
                .clone();
            let mut mesh = buf.into_mesh();
            if pbr.normal_map.is_some() {
                let _ = mesh.generate_tangents();
            }
            let e = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), to_y_up)).id();
            entities.push(e);
        }
        let chunks = plan.entries.iter().map(|entry| entry.chunk.clone()).collect();
        pieces.insert(
            key,
            Piece { signature: plan.signature, _chunks: chunks, entities, glows: glows.into_iter().collect() },
        );
    }
    for entity in state.lights.drain(..) {
        commands.entity(entity).despawn();
    }
    // A Lambertian face of luminance L and area A gives off π·L·A lumens.
    let mut glows: HashMap<(u64, MaterialId), (f64, DVec3)> = HashMap::new();
    for (key, (area, sum)) in pieces.values().flat_map(|piece| &piece.glows) {
        let entry = glows.entry(*key).or_insert((0.0, DVec3::ZERO));
        entry.0 += area;
        entry.1 += *sum;
    }
    let mut lights: Vec<(f32, Color, DVec3, f64)> = glows
        .into_iter()
        .filter(|(_, (area, _))| *area > 1e-9)
        .filter_map(|((_, m), (area, sum))| {
            let mat = doc.materials.get(&m)?;
            let flux = std::f32::consts::PI * mat.glow * area as f32;
            Some((flux, Color::srgb(mat.color[0], mat.color[1], mat.color[2]), sum / area, area))
        })
        .collect();
    lights.sort_by(|a, b| b.0.total_cmp(&a.0));
    // Each shadowed lamp redraws the model six times. In daylight its shadows vanish under the
    // sun's, so only lamps after dark or in the studio cast them.
    let daylight = style.lighting == Lighting::Sun && style.sun.position().1 > 0.;
    let lamp_shadows = style.shadows && !daylight;
    for (rank, (flux, color, centre, area)) in lights.into_iter().take(MAX_LIGHTS).enumerate() {
        // Out to where the light falls to about 0.05 lx.
        let range = (flux / (4.0 * std::f32::consts::PI * 0.05)).sqrt().clamp(1.0, 200.0);
        let light = PointLight {
            color,
            intensity: flux,
            range,
            radius: (area / std::f64::consts::PI).sqrt().min(0.5) as f32 * 0.5,
            shadow_maps_enabled: lamp_shadows && rank < SHADOWED_LIGHTS,
            ..default()
        };
        let at = Vec3::new(centre.x as f32, centre.z as f32, -centre.y as f32);
        state.lights.push(commands.spawn((light, Transform::from_translation(at))).id());
    }
    state.pieces = pieces;
    state.materials = shared_materials;
    state.key = Some(key);
}

/// The selected and hovered faces, as a see-through blue skin over the model. Kept apart
/// from the model's meshes so that a click or a hover does not rebuild them.
#[derive(Resource, Default)]
struct Highlight {
    key: Option<(u64, u64, Vec<FaceId>, Vec<InstanceId>)>,
    entity: Option<Entity>,
}

const HIGHLIGHT_OPACITY: f32 = 0.45;

fn rebuild_highlight(
    mut commands: Commands,
    app: Res<App3d>,
    mut state: ResMut<Highlight>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    edges: Res<EdgeCache>,
    mut drawn: ResMut<DrawnCache>,
) {
    let preview = app.tools.preview(&app.editor);
    let key = (app.document_generation, app.editor.revision(), preview.faces, preview.instances);
    if state.key.as_ref() == Some(&key) {
        return;
    }
    if let Some(entity) = state.entity.take() {
        commands.entity(entity).despawn();
    }
    let editor = &app.editor;
    let doc = editor.doc();
    let mut faces: BTreeSet<FaceId> = key.2.iter().copied().collect();
    let mut instances: BTreeSet<InstanceId> = key.3.iter().copied().collect();
    for element in editor.selection() {
        match element {
            Element::Face(id) => {
                faces.insert(*id);
            }
            Element::Instance(id) => {
                instances.insert(*id);
            }
            _ => {}
        }
    }
    let mut buffer = Buffers::default();
    drawn.prepare(doc, (app.document_generation, editor.content_revision()));
    if doc.style.faces != FaceStyle::Wireframe && (!faces.is_empty() || !instances.is_empty()) {
        let colour = [SELECTED[0], SELECTED[1], SELECTED[2], HIGHLIGHT_OPACITY];
        for placed in &edges.placed {
            let occ = &placed.occurrence;
            let whole = occ.top.is_some_and(|i| instances.contains(&i));
            if (!whole && !placed.active) || doc.def(occ.definition).is_none() {
                continue;
            }
            let drawn = drawn.get(doc, occ.definition);
            let mut add = |face: &DrawnFace| {
                if drawn.mesh.face(face.id).is_none_or(|f| f.hidden && !doc.style.show_hidden_geometry) {
                    return;
                }
                for triangle in &face.triangles {
                    let points = triangle.map(|v| occ.transform.transform_point3(drawn.mesh.pos(v)));
                    let polygon = rysmo_model::sections::clip_triangle(points, &placed.cuts);
                    for i in 1..polygon.len().saturating_sub(1) {
                        for point in [polygon[0].0, polygon[i].0, polygon[i + 1].0] {
                            buffer.push(point, DVec3::Z, colour, [0., 0.]);
                        }
                    }
                }
            };
            if whole {
                drawn.chunks.iter().flat_map(|chunk| &chunk.faces).for_each(&mut add);
            } else {
                for id in &faces {
                    if let Some(face) = drawn.face(*id) {
                        add(face);
                    }
                }
            }
        }
    }
    if !buffer.idx.is_empty() {
        let material = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            depth_bias: 1.0,
            ..default()
        });
        let to_y_up = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
        let entity = commands.spawn((Mesh3d(meshes.add(buffer.into_mesh())), MeshMaterial3d(material), to_y_up)).id();
        state.entity = Some(entity);
    }
    state.key = Some(key);
}

#[cfg(test)]
mod edge_material_tests {
    use super::*;
    #[test]
    fn edge_cache_uses_explicit_edge_color_without_recoloring_adjacent_faces() {
        use rysmo_model::{Command, Editor, Element};
        let mut editor = Editor::new();
        editor.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let face = editor.mesh().faces().next().unwrap().0;
        let edge = editor.mesh().face_edges(face)[0];
        let red = editor
            .apply(Command::AddMaterial { name: "Red face".into(), color: [1., 0., 0., 1.], texture: None })
            .unwrap()
            .material;
        let blue = editor
            .apply(Command::AddMaterial { name: "Blue edge".into(), color: [0., 0., 1., 1.], texture: None })
            .unwrap()
            .material;
        editor.apply(Command::Paint { faces: vec![face], material: red, back: false }).unwrap();
        let mut app = App::new();
        app.insert_resource(App3d::new(editor)).init_resource::<EdgeCache>().add_systems(Update, update_edge_cache);
        app.update();
        assert!(app.world().resource::<EdgeCache>().defs[&None].list.iter().all(|e| e.material == [0., 0., 0.]));
        app.world_mut()
            .resource_mut::<App3d>()
            .editor
            .apply(Command::PaintElements { elements: vec![Element::Edge(edge)], material: blue, back: false })
            .unwrap();
        app.update();
        let cache = app.world().resource::<EdgeCache>();
        assert_eq!(cache.defs[&None].list.iter().find(|e| e.id == edge).unwrap().material, [0., 0., 1.]);
        assert_eq!(app.world().resource::<App3d>().editor.mesh().face(face).unwrap().material, red);
    }
}

/// Barycentric samples for a regular triangle grid. Used only for warped textures.
fn subdivided_weights(count: usize) -> Vec<[DVec3; 3]> {
    let at = |i: usize, j: usize| {
        DVec3::new(1. - (i + j) as f64 / count as f64, i as f64 / count as f64, j as f64 / count as f64)
    };
    let mut out = Vec::with_capacity(count * count);
    for i in 0..count {
        for j in 0..count - i {
            out.push([at(i, j), at(i + 1, j), at(i, j + 1)]);
            if i + j + 1 < count {
                out.push([at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]);
            }
        }
    }
    out
}

#[cfg(test)]
mod subdivision_tests {
    use super::*;
    #[test]
    fn warped_texture_subdivision_covers_triangle_without_reversing_normals() {
        for count in [1, 8] {
            let tris = subdivided_weights(count);
            assert_eq!(tris.len(), count * count);
            let mut area = 0.;
            for tri in tris {
                for p in tri {
                    assert!((p.element_sum() - 1.).abs() < 1e-12);
                    assert!(p.min_element() >= -1e-12);
                }
                let cross = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
                assert!(cross.min_element() > 0.);
                area += cross.length() / 2.;
            }
            assert!((area - 3_f64.sqrt() / 2.).abs() < 1e-12);
        }
    }
}

#[cfg(test)]
mod document_cache_tests {
    use super::*;
    use rysmo_model::{Command, Editor};
    #[test]
    fn opening_another_model_refreshes_edge_geometry_and_hidden_view_exposes_soft_edges() {
        let mut first = Editor::new();
        first.apply(Command::DrawLines { lines: vec![[DVec3::ZERO, DVec3::X]], facing: None }).unwrap();
        let mut second = Editor::new();
        second
            .apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X * 2., v: DVec3::Y * 2., facing: None })
            .unwrap();
        assert_eq!(first.revision(), second.revision());
        let mut app = App::new();
        app.insert_resource(App3d::new(first)).init_resource::<EdgeCache>().add_systems(Update, update_edge_cache);
        app.update();
        assert_eq!(app.world().resource::<EdgeCache>().defs[&None].list.len(), 1);
        let text = rysmo_io::save(second.doc());
        crate::files::open_text(&mut app.world_mut().resource_mut::<App3d>(), &text, "second.rysmo").unwrap();
        app.update();
        assert_eq!(app.world().resource::<EdgeCache>().defs[&None].list.len(), 4);
        {
            let mut state = app.world_mut().resource_mut::<App3d>();
            let edges = state.editor.mesh().edges().map(|(id, _)| id).collect();
            state.editor.apply(Command::SetEdges { edges, soft: Some(true), smooth: None, hidden: None }).unwrap();
        }
        app.update();
        assert!(app.world().resource::<EdgeCache>().defs[&None].list.is_empty());
        {
            let mut state = app.world_mut().resource_mut::<App3d>();
            let mut style = state.editor.doc().style;
            style.show_hidden_geometry = true;
            state.editor.apply(Command::SetStyle { style }).unwrap();
        }
        app.update();
        assert_eq!(app.world().resource::<EdgeCache>().defs[&None].list.len(), 4);
        assert!(app.world().resource::<EdgeCache>().defs[&None].list.iter().all(|edge| edge.ghost));
    }
}

#[cfg(test)]
mod texture_quality_tests {
    use super::*;
    #[test]
    fn display_texture_limit_preserves_aspect_and_builds_complete_mips() {
        let source = image::RgbaImage::from_pixel(1024, 512, image::Rgba([200, 100, 50, 128]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(source).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let bytes = png.into_inner();
        let original = bytes.clone();
        let image = decode(&bytes, "image/png", 512, 8).unwrap();
        assert_eq!(image.texture_descriptor.size.width, 512);
        assert_eq!(image.texture_descriptor.size.height, 256);
        assert_eq!(image.texture_descriptor.mip_level_count, 10);
        assert_eq!(&image.data.as_ref().unwrap()[..4], &[200, 100, 50, 128]);
        assert_eq!(
            image.data.as_ref().unwrap().len(),
            (0..10).map(|i| 4 * (512usize >> i).max(1) * (256usize >> i).max(1)).sum::<usize>()
        );
        let ImageSampler::Descriptor(sampler) = image.sampler else { panic!() };
        assert_eq!(sampler.anisotropy_clamp, 8);
        assert_eq!(bytes, original);
        let full = decode(&bytes, "image/png", 0, 1).unwrap();
        assert_eq!(full.texture_descriptor.size.width, 1024);
    }
}

#[cfg(test)]
mod colour_tests {
    use super::*;
    #[test]
    fn material_colours_reach_the_gpu_as_linear_values() {
        let mut buffers = Buffers::default();
        buffers.push(DVec3::ZERO, DVec3::Z, [0.5, 1., 0., 0.25], [0., 0.]);
        let [r, g, b, a] = buffers.col[0];
        assert!((r - 0.214).abs() < 1e-3, "{r}");
        assert_eq!([g, b, a], [1., 0., 0.25]);
    }
}
