//! Binary glTF 2.0 (`.glb`) export.
//!
//! One mesh with a primitive per material. Vertices are repeated per face so normals
//! stay flat, as in the viewport. Materials are double-sided, since an editable
//! model often has faces seen from both sides.

use std::collections::BTreeMap;

use rysmo_mesh::{MaterialId, triangulate_face};
use rysmo_model::Document;
use serde_json::json;

use crate::y_up;

#[derive(Default)]
struct Prim {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

/// The `.glb` bytes for a document.
pub fn export(doc: &Document) -> Vec<u8> {
    let mut prims: BTreeMap<Option<MaterialId>, Prim> = BTreeMap::new();
    for occ in doc.occurrences(&[]) {
        let Some(_def) = doc.def(occ.definition) else { continue };
        let evaluated = doc.effective_mesh(occ.definition);
        let mesh = &*evaluated;
        let t = occ.transform;
        let mirrored = t.determinant() < 0.0;
        let nt = t.inverse().transpose();
        for (fid, face) in mesh.faces().filter(|(_, face)| !face.hidden) {
            let Some(n) = mesh.face_normal(fid) else { continue };
            let n = y_up(nt.transform_vector3(n).normalize());
            let mat = face.material.or(occ.material).filter(|m| doc.materials.contains_key(m));
            let texture = mat.and_then(|m| doc.materials[&m].texture).filter(|t| doc.images.contains_key(&t.image));
            let local_n = mesh.face_normal(fid).unwrap_or_default();
            let placement = face.uv.unwrap_or_default();
            let prim = prims.entry(mat).or_default();
            for mut tri in triangulate_face(mesh, fid) {
                if mirrored {
                    tri.swap(1, 2);
                }
                for v in tri {
                    prim.indices.push(prim.positions.len() as u32);
                    prim.positions.push(y_up(t.transform_point3(mesh.pos(v))));
                    prim.normals.push(n);
                    let uv_size = texture.map(|t| t.size).or_else(|| {
                        mat.and_then(|id| doc.materials.get(&id))
                            .filter(|m| m.pbr.images().next().is_some())
                            .map(|_| [1., 1.])
                    });
                    let uv = uv_size
                        .map(|size| rysmo_model::uv_at(mesh.pos(v), local_n, size, &placement))
                        .unwrap_or([0.0, 0.0]);
                    prim.uvs.push([uv[0] as f32, uv[1] as f32]);
                }
            }
        }
    }
    prims.retain(|_, p| !p.indices.is_empty());

    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut materials = Vec::new();
    let mut primitives = Vec::new();
    let mut material_index: BTreeMap<Option<MaterialId>, usize> = BTreeMap::new();
    let mut images = Vec::new();
    let mut textures = Vec::new();
    let mut texture_index: BTreeMap<rysmo_mesh::ImageId, usize> = BTreeMap::new();
    let mut emissive_strength = false;
    let mut normal_texture_index: BTreeMap<(rysmo_mesh::ImageId, bool), Option<usize>> = BTreeMap::new();
    for (mat, prim) in &prims {
        let texture = mat
            .and_then(|m| doc.materials.get(&m))
            .and_then(|m| m.texture)
            .filter(|t| doc.images.contains_key(&t.image));
        let surface = mat.and_then(|id| doc.materials.get(&id)).map(|m| m.pbr).unwrap_or_default();
        let mut embed = |id: rysmo_mesh::ImageId| {
            *texture_index.entry(id).or_insert_with(|| {
                let img = &doc.images[&id];
                while !bin.len().is_multiple_of(4) {
                    bin.push(0);
                }
                let offset = bin.len();
                bin.extend_from_slice(&img.bytes);
                views.push(json!({"buffer": 0, "byteOffset": offset, "byteLength": img.bytes.len()}));
                let mime = if img.mime.contains("jpeg") { "image/jpeg" } else { "image/png" };
                images.push(json!({"name": img.name, "bufferView": views.len() - 1, "mimeType": mime}));
                textures.push(json!({"source": images.len() - 1, "sampler": 0}));
                textures.len() - 1
            })
        };
        let tex_index = texture.map(|t| embed(t.image));
        let surface_index = surface.metallic_roughness_map.filter(|id| doc.images.contains_key(id)).map(&mut embed);
        let normal_index = surface.normal_map.and_then(|id| {
            let key = (id, surface.normal_flip_y);
            if let Some(index) = normal_texture_index.get(&key) {
                return *index;
            }
            let image = doc.images.get(&id)?;
            let index = (|| {
                let format = image::guess_format(&image.bytes).ok()?;
                let (w, h) = image::ImageReader::with_format(std::io::Cursor::new(&*image.bytes), format)
                    .into_dimensions()
                    .ok()?;
                if w as u64 * h as u64 > 16_777_216 {
                    return None;
                }
                let mut rgba = image::load_from_memory(&image.bytes).ok()?.to_rgba8();
                if surface.normal_flip_y {
                    for pixel in rgba.pixels_mut() {
                        pixel.0[1] = 255 - pixel.0[1];
                    }
                }
                let mut out = std::io::Cursor::new(Vec::new());
                rgba.write_to(&mut out, image::ImageFormat::Png).ok()?;
                let bytes = out.into_inner();
                while !bin.len().is_multiple_of(4) {
                    bin.push(0);
                }
                let offset = bin.len();
                bin.extend_from_slice(&bytes);
                views.push(json!({"buffer":0,"byteOffset":offset,"byteLength":bytes.len()}));
                images.push(json!({"name":image.name,"bufferView":views.len()-1,"mimeType":"image/png"}));
                textures.push(json!({"source":images.len()-1,"sampler":0}));
                Some(textures.len() - 1)
            })();
            normal_texture_index.insert(key, index);
            index
        });
        let mi = *material_index.entry(*mat).or_insert_with(|| {
            let (name, color) = match mat.and_then(|m| doc.materials.get(&m)) {
                // A textured material's colour is the texture's own; the image carries it.
                Some(m) if m.texture.is_some() && !m.pbr.tint_texture => (m.name.clone(), [1.0, 1.0, 1.0, m.color[3]]),
                Some(m) => (m.name.clone(), m.color),
                None => ("Default".to_string(), [1.0, 1.0, 1.0, 1.0]),
            };
            // glTF base colours are linear; material colours are sRGB.
            let lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
            let mut pbr = json!({
                "baseColorFactor": [lin(color[0]), lin(color[1]), lin(color[2]), color[3]],
                "metallicFactor": surface.metallic,
                "roughnessFactor": surface.roughness
            });
            if let Some(ti) = tex_index {
                pbr["baseColorTexture"] = json!({"index": ti});
            }
            if let Some(index) = surface_index {
                pbr["metallicRoughnessTexture"] = json!({"index": index});
            }
            let mut m = json!({"name": name, "doubleSided": true, "pbrMetallicRoughness": pbr});
            if let Some(index) = normal_index {
                m["normalTexture"] = json!({"index":index,"scale":surface.normal_scale});
            }
            if color[3] < 1.0 {
                m["alphaMode"] = json!("BLEND");
            } else if let Some(cutoff) = surface.alpha_cutoff {
                m["alphaMode"] = json!("MASK");
                m["alphaCutoff"] = json!(cutoff);
            }
            if let Some(glow) = mat.and_then(|m| doc.materials.get(&m)).map(|m| m.glow).filter(|g| *g > 0.) {
                // The importer reads a full emissive factor as 500 cd/m²; the colour peaks at one.
                let strength = glow / 500.;
                let linear = [lin(color[0]), lin(color[1]), lin(color[2])];
                let peak = linear.into_iter().fold(1e-6, f32::max);
                let f = strength.min(1.) / peak;
                m["emissiveFactor"] = json!(linear.map(|c| c * f));
                if strength > 1. {
                    emissive_strength = true;
                    m["extensions"] = json!({"KHR_materials_emissive_strength": {"emissiveStrength": strength}});
                }
            }
            materials.push(m);
            materials.len() - 1
        });
        let mut push_view = |bytes: &[u8], target: u32| {
            while !bin.len().is_multiple_of(4) {
                bin.push(0);
            }
            let offset = bin.len();
            bin.extend_from_slice(bytes);
            views.push(json!({"buffer": 0, "byteOffset": offset, "byteLength": bytes.len(), "target": target}));
            views.len() - 1
        };
        let pos_bytes: Vec<u8> = prim.positions.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        let nrm_bytes: Vec<u8> = prim.normals.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
        let idx_bytes: Vec<u8> = prim.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let pv = push_view(&pos_bytes, 34962);
        let nv = push_view(&nrm_bytes, 34962);
        let iv = push_view(&idx_bytes, 34963);
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for p in &prim.positions {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
            }
        }
        let count = prim.positions.len();
        accessors.push(
            json!({"bufferView": pv, "componentType": 5126, "count": count, "type": "VEC3", "min": min, "max": max}),
        );
        let pa = accessors.len() - 1;
        accessors.push(json!({"bufferView": nv, "componentType": 5126, "count": count, "type": "VEC3"}));
        let na = accessors.len() - 1;
        accessors.push(json!({"bufferView": iv, "componentType": 5125, "count": prim.indices.len(), "type": "SCALAR"}));
        let ia = accessors.len() - 1;
        let mut attributes = json!({"POSITION": pa, "NORMAL": na});
        if tex_index.is_some() || normal_index.is_some() || surface_index.is_some() {
            let uv_bytes: Vec<u8> = prim.uvs.iter().flatten().flat_map(|f| f.to_le_bytes()).collect();
            let uvv = push_view(&uv_bytes, 34962);
            accessors.push(json!({"bufferView": uvv, "componentType": 5126, "count": count, "type": "VEC2"}));
            attributes["TEXCOORD_0"] = json!(accessors.len() - 1);
        }
        primitives.push(json!({"attributes": attributes, "indices": ia, "material": mi}));
    }
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }

    let mut gltf = json!({
        "asset": {"version": "2.0", "generator": "Rysmo"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "Model"}],
        "meshes": [{"name": "Model", "primitives": primitives}],
        "materials": materials,
        "accessors": accessors,
        "images": images,
        "textures": textures,
        "samplers": [{"wrapS": 10497, "wrapT": 10497}],
        "bufferViews": views,
        "buffers": [{"byteLength": bin.len()}]
    });
    if emissive_strength {
        gltf["extensionsUsed"] = json!(["KHR_materials_emissive_strength"]);
    }
    if textures_empty(&gltf) {
        for key in ["images", "textures", "samplers"] {
            gltf.as_object_mut().expect("object").remove(key);
        }
    }
    if primitives_empty(&gltf) {
        // A valid file with nothing in it.
        gltf["nodes"] = json!([{"name": "Model"}]);
        gltf.as_object_mut().expect("object").remove("meshes");
    }
    let mut json_bytes = serde_json::to_vec(&gltf).expect("json serialises");
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }

    let total = 12 + 8 + json_bytes.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&json_bytes);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    out
}

fn textures_empty(g: &serde_json::Value) -> bool {
    g["textures"].as_array().is_none_or(|a| a.is_empty())
}

fn primitives_empty(g: &serde_json::Value) -> bool {
    g["meshes"][0]["primitives"].as_array().is_none_or(|a| a.is_empty())
}

#[cfg(test)]
mod tests {
    use rysmo_math::DVec3;
    use rysmo_model::{Command, Editor};

    #[test]
    fn glb_has_valid_header_and_chunks() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let f = e.mesh().faces().next().unwrap().0;
        e.apply(Command::PushPull { face: f, distance: -1.0, keep_base: false }).unwrap();
        let glb = super::export(e.doc());
        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize, glb.len());
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        assert_eq!(&glb[16..20], b"JSON");
        let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        // Six quads, two triangles each, three vertices per triangle.
        assert_eq!(json["accessors"][0]["count"], 36);
        let bin_len = u32::from_le_bytes(glb[20 + json_len..24 + json_len].try_into().unwrap()) as usize;
        assert_eq!(json["buffers"][0]["byteLength"], bin_len);
        assert_eq!(glb.len() % 4, 0);
    }

    #[test]
    fn empty_document_exports() {
        let glb = super::export(&rysmo_model::Document::new());
        assert_eq!(&glb[0..4], b"glTF");
    }

    #[test]
    fn textured_material_embeds_its_image_and_uvs() {
        let mut e = Editor::new();
        e.apply(Command::DrawRectangle { corner: DVec3::ZERO, u: DVec3::X, v: DVec3::Y, facing: None }).unwrap();
        let f = e.mesh().faces().next().unwrap().0;
        let img = e
            .apply(Command::AddImage { name: "b.png".into(), mime: "image/png".into(), data: vec![9; 37] })
            .unwrap()
            .image
            .unwrap();
        let m = e
            .apply(Command::AddMaterial {
                name: "B".into(),
                color: [0.5, 0.5, 0.5, 1.0],
                texture: Some(rysmo_model::Texture { image: img, size: [0.5, 0.5] }),
            })
            .unwrap()
            .material;
        e.apply(Command::Paint { faces: vec![f], material: m, back: false }).unwrap();
        let glb = super::export(e.doc());
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let json: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        assert_eq!(json["images"][0]["mimeType"], "image/png");
        assert_eq!(json["textures"][0]["source"], 0);
        assert_eq!(json["materials"][0]["pbrMetallicRoughness"]["baseColorTexture"]["index"], 0);
        let prim = &json["meshes"][0]["primitives"][0];
        let uv = prim["attributes"]["TEXCOORD_0"].as_u64().unwrap() as usize;
        assert_eq!(json["accessors"][uv]["type"], "VEC2");
        let view = json["images"][0]["bufferView"].as_u64().unwrap() as usize;
        assert_eq!(json["bufferViews"][view]["byteLength"], 37);
    }
}
