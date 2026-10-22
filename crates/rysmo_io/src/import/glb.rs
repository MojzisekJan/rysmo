use super::{ImportError, Imported, Options, Result, error, geometry::Geometry};
use base64::Engine as _;
use rysmo_math::{DMat4, DQuat, DVec2, DVec3, DVec4};
use rysmo_mesh::{DefinitionId, ImageId, InstanceId, MaterialId};
use rysmo_model::{Definition, Document, Image, Instance, Material, Texture};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

fn integer(v: &Value, field: &str, default: usize) -> Result<usize> {
    v.get(field).map_or(Ok(default), |n| {
        n.as_u64().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| ImportError(format!("Invalid GLB {field}.")))
    })
}
fn list<'a>(root: &'a Value, field: &str) -> Result<&'a [Value]> {
    root.get(field).map_or(Ok(&[]), |v| {
        v.as_array().map(Vec::as_slice).ok_or_else(|| ImportError(format!("Invalid GLB {field} array.")))
    })
}
fn item<'a>(root: &'a Value, field: &str, index: usize) -> Result<&'a Value> {
    list(root, field)?.get(index).ok_or_else(|| ImportError(format!("Missing GLB {field}[{index}].")))
}
fn floats<const N: usize>(value: Option<&Value>, default: [f64; N]) -> Result<[f64; N]> {
    let Some(value) = value else { return Ok(default) };
    let values =
        value.as_array().filter(|v| v.len() == N).ok_or_else(|| ImportError("Invalid GLB vector size.".into()))?;
    let mut result = default;
    for (out, value) in result.iter_mut().zip(values) {
        *out =
            value.as_f64().filter(|n| n.is_finite()).ok_or_else(|| ImportError("Invalid GLB vector value.".into()))?;
    }
    Ok(result)
}
fn data_uri(uri: &str) -> Result<Vec<u8>> {
    let (header, data) = uri.split_once(',').ok_or_else(|| ImportError("Invalid GLB data URI.".into()))?;
    if !header.starts_with("data:") || !header.ends_with(";base64") {
        return error("GLB requires embedded base64 data, not external resources.");
    }
    base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| ImportError(e.to_string()))
}
fn buffer_view<'a>(root: &Value, buffers: &'a [Vec<u8>], index: usize) -> Result<&'a [u8]> {
    let view = item(root, "bufferViews", index)?;
    let buffer = buffers.get(integer(view, "buffer", 0)?).ok_or_else(|| ImportError("Missing GLB buffer.".into()))?;
    let offset = integer(view, "byteOffset", 0)?;
    let length = integer(view, "byteLength", 0)?;
    let end = offset.checked_add(length).ok_or_else(|| ImportError("GLB buffer range overflows.".into()))?;
    buffer.get(offset..end).ok_or_else(|| ImportError("GLB buffer view is out of bounds.".into()))
}
fn component(bytes: &[u8], kind: usize, normalized: bool) -> Result<f64> {
    let bad = || ImportError("Truncated GLB component.".into());
    let value = match kind {
        5120 => {
            let n = *bytes.first().ok_or_else(bad)? as i8;
            if normalized { (n as f64 / 127.).max(-1.) } else { n as f64 }
        }
        5121 => {
            let n = *bytes.first().ok_or_else(bad)?;
            if normalized { n as f64 / 255. } else { n as f64 }
        }
        5122 => {
            let n = i16::from_le_bytes(bytes.get(..2).ok_or_else(bad)?.try_into().map_err(|_| bad())?);
            if normalized { (n as f64 / 32767.).max(-1.) } else { n as f64 }
        }
        5123 => {
            let n = u16::from_le_bytes(bytes.get(..2).ok_or_else(bad)?.try_into().map_err(|_| bad())?);
            if normalized { n as f64 / 65535. } else { n as f64 }
        }
        5125 => u32::from_le_bytes(bytes.get(..4).ok_or_else(bad)?.try_into().map_err(|_| bad())?) as f64,
        5126 => f32::from_le_bytes(bytes.get(..4).ok_or_else(bad)?.try_into().map_err(|_| bad())?) as f64,
        _ => return error("Unsupported GLB accessor component type."),
    };
    if !value.is_finite() {
        return error("GLB contains a non-finite component.");
    }
    Ok(value)
}
fn component_size(kind: usize) -> Result<usize> {
    match kind {
        5120 | 5121 => Ok(1),
        5122 | 5123 => Ok(2),
        5125 | 5126 => Ok(4),
        _ => error("Invalid GLB component type."),
    }
}
fn accessor(root: &Value, buffers: &[Vec<u8>], index: usize, width: usize) -> Result<Vec<Vec<f64>>> {
    let a = item(root, "accessors", index)?;
    let count = integer(a, "count", 0)?;
    if count > 600_000 {
        return error("GLB accessor exceeds 600,000 entries.");
    }
    let declared = match a["type"].as_str() {
        Some("SCALAR") => 1,
        Some("VEC2") => 2,
        Some("VEC3") => 3,
        Some("VEC4") => 4,
        _ => return error("Unsupported GLB accessor shape."),
    };
    if declared != width {
        return error("GLB accessor has the wrong vector size.");
    }
    let kind = integer(a, "componentType", 0)?;
    let size = component_size(kind)?;
    let normalized = a["normalized"].as_bool().unwrap_or(false);
    let mut values = vec![vec![0.; width]; count];
    if let Some(view_index) = a.get("bufferView") {
        let index = view_index.as_u64().ok_or_else(|| ImportError("Invalid GLB view index.".into()))? as usize;
        let view = item(root, "bufferViews", index)?;
        let bytes = buffer_view(root, buffers, index)?;
        let stride = integer(view, "byteStride", size * width)?;
        let offset = integer(a, "byteOffset", 0)?;
        if stride < size * width || stride > 252 {
            return error("Invalid GLB byte stride.");
        }
        for (i, value) in values.iter_mut().enumerate() {
            let start = i
                .checked_mul(stride)
                .and_then(|s| s.checked_add(offset))
                .ok_or_else(|| ImportError("GLB accessor range overflows.".into()))?;
            for (j, v) in value.iter_mut().enumerate() {
                *v = component(
                    bytes
                        .get(start + j * size..)
                        .ok_or_else(|| ImportError("GLB accessor is out of bounds.".into()))?,
                    kind,
                    normalized,
                )?;
            }
        }
    } else if a.get("sparse").is_none() && count != 0 {
        return error("GLB accessor has no buffer view.");
    }
    if let Some(sparse) = a.get("sparse") {
        let n = integer(sparse, "count", 0)?;
        if n > count {
            return error("GLB sparse count exceeds accessor count.");
        }
        let indices = &sparse["indices"];
        let ikind = integer(indices, "componentType", 0)?;
        if ![5121, 5123, 5125].contains(&ikind) {
            return error("Invalid sparse index type.");
        }
        let ibytes = buffer_view(root, buffers, integer(indices, "bufferView", usize::MAX)?)?;
        let vbytes = buffer_view(root, buffers, integer(&sparse["values"], "bufferView", usize::MAX)?)?;
        let io = integer(indices, "byteOffset", 0)?;
        let vo = integer(&sparse["values"], "byteOffset", 0)?;
        let isize = component_size(ikind)?;
        let mut previous = None;
        for i in 0..n {
            let at = io.checked_add(i * isize).ok_or_else(|| ImportError("Sparse index overflows.".into()))?;
            let index = component(
                ibytes.get(at..).ok_or_else(|| ImportError("Truncated sparse indices.".into()))?,
                ikind,
                false,
            )? as usize;
            if index >= count || previous.is_some_and(|p| index <= p) {
                return error("GLB sparse indices are not increasing and in range.");
            }
            previous = Some(index);
            for (j, value) in values[index].iter_mut().enumerate() {
                let at = vo
                    .checked_add((i * width + j) * size)
                    .ok_or_else(|| ImportError("Sparse value overflows.".into()))?;
                *value = component(
                    vbytes.get(at..).ok_or_else(|| ImportError("Truncated sparse values.".into()))?,
                    kind,
                    normalized,
                )?;
            }
        }
    }
    Ok(values)
}
fn node_transform(node: &Value) -> Result<DMat4> {
    let t = if node.get("matrix").is_some() {
        DMat4::from_cols_array(&floats(node.get("matrix"), DMat4::IDENTITY.to_cols_array())?)
    } else {
        let translation = DVec3::from_array(floats(node.get("translation"), [0.; 3])?);
        let scale = DVec3::from_array(floats(node.get("scale"), [1.; 3])?);
        let q = floats(node.get("rotation"), [0., 0., 0., 1.])?;
        let rotation = DQuat::from_array(q);
        if rotation.length_squared() < 1e-12 {
            return error("GLB node has an invalid rotation.");
        }
        DMat4::from_scale_rotation_translation(scale, rotation.normalize(), translation)
    };
    if !t.is_finite() || !t.inverse().is_finite() || (t.row(3) - DVec4::W).abs().max_element() > 1e-8 {
        return error("GLB node has a singular or non-affine transform.");
    }
    let frame = DMat4::from_rotation_x(std::f64::consts::FRAC_PI_2);
    Ok(frame * t * frame.inverse())
}
fn from_y(p: DVec3) -> DVec3 {
    DVec3::new(p.x, -p.z, p.y)
}
fn add_instance(definition: DefinitionId, transform: DMat4, name: String) -> Instance {
    Instance { definition, transform, name, tag: None, material: None, hidden: false, glued_to: None }
}
struct Nodes<'a> {
    root: &'a Value,
    meshes: &'a [DefinitionId],
    built: BTreeMap<usize, DefinitionId>,
    visiting: BTreeSet<usize>,
}
impl Nodes<'_> {
    fn build(&mut self, doc: &mut Document, index: usize) -> Result<DefinitionId> {
        if let Some(id) = self.built.get(&index) {
            return Ok(*id);
        }
        if !self.visiting.insert(index) || self.visiting.len() > 64 || self.built.len() > 10_000 {
            return error("GLB node hierarchy is cyclic or too large.");
        }
        let node = item(self.root, "nodes", index)?;
        let name = node["name"].as_str().unwrap_or("Imported node").to_owned();
        let mut definition = Definition { name: name.clone(), ..Default::default() };
        if node.get("mesh").is_some() {
            let index = integer(node, "mesh", usize::MAX)?;
            let mesh = *self.meshes.get(index).ok_or_else(|| ImportError("Missing GLB mesh.".into()))?;
            let id = InstanceId(doc.reserve_id());
            definition.instances.insert(id, add_instance(mesh, DMat4::IDENTITY, name));
        }
        for child in list(node, "children")? {
            let index = child.as_u64().ok_or_else(|| ImportError("Invalid GLB child index.".into()))? as usize;
            let child = self.build(doc, index)?;
            let transform = node_transform(item(self.root, "nodes", index)?)?;
            let id = InstanceId(doc.reserve_id());
            definition.instances.insert(id, add_instance(child, transform, String::new()));
        }
        let id = DefinitionId(doc.reserve_id());
        doc.definitions.insert(id, definition);
        self.built.insert(index, id);
        self.visiting.remove(&index);
        Ok(id)
    }
}
pub(super) fn read(bytes: &[u8], options: Options) -> Result<Imported> {
    let u32_at = |offset| -> Result<u32> {
        Ok(u32::from_le_bytes(
            bytes
                .get(offset..offset + 4)
                .ok_or_else(|| ImportError("Truncated GLB header.".into()))?
                .try_into()
                .map_err(|_| ImportError("Invalid GLB header.".into()))?,
        ))
    };
    if bytes.get(..4) != Some(b"glTF") || u32_at(4)? != 2 || u32_at(8)? as usize != bytes.len() {
        return error("Invalid GLB 2.0 header or length.");
    }
    let mut offset = 12;
    let mut root = None;
    let mut bin = None;
    while offset < bytes.len() {
        let length = u32_at(offset)? as usize;
        let kind = u32_at(offset + 4)?;
        offset += 8;
        if !length.is_multiple_of(4) {
            return error("GLB chunks must be aligned to four bytes.");
        }
        let end = offset.checked_add(length).ok_or_else(|| ImportError("GLB chunk overflows.".into()))?;
        let chunk = bytes.get(offset..end).ok_or_else(|| ImportError("Truncated GLB chunk.".into()))?;
        match kind {
            0x4e4f534a => {
                if root.is_some() || offset != 20 {
                    return error("GLB JSON must be its first and only JSON chunk.");
                }
                root = Some(serde_json::from_slice::<Value>(chunk)?);
            }
            0x004e4942 => {
                if bin.is_some() {
                    return error("Duplicate GLB BIN chunk.");
                }
                bin = Some(chunk.to_vec());
            }
            _ => {}
        }
        offset = end;
    }
    let root = root.ok_or_else(|| ImportError("GLB has no JSON chunk.".into()))?;
    if root["asset"]["version"].as_str() != Some("2.0") {
        return error("Only glTF 2.0 is supported.");
    }
    if !list(&root, "extensionsRequired")?.is_empty() {
        return error("Required glTF extensions are not supported. Export an uncompressed core GLB 2.0.");
    }
    let mut buffers = Vec::new();
    for (i, buffer) in list(&root, "buffers")?.iter().enumerate() {
        let data = if let Some(uri) = buffer["uri"].as_str() {
            data_uri(uri)?
        } else if i == 0 {
            bin.take().ok_or_else(|| ImportError("Missing GLB BIN chunk.".into()))?
        } else {
            return error("GLB has a buffer with no embedded data.");
        };
        let length = integer(buffer, "byteLength", data.len())?;
        if length > data.len() {
            return error("GLB buffer is shorter than declared.");
        }
        buffers.push(data[..length].to_vec());
    }
    let mut document = Document::new();
    let mut images = Vec::new();
    let mut warnings = Vec::new();
    if !list(&root, "extensionsUsed")?.is_empty() {
        warnings.push(
            "Optional glTF extensions are not applied; only core geometry, materials and UVs are imported.".into(),
        );
    }
    let source_images = list(&root, "images")?;
    if source_images.len() > 1024 || list(&root, "nodes")?.len() > 10_000 || list(&root, "meshes")?.len() > 10_000 {
        return error("GLB exceeds the image, node or mesh count limit.");
    }
    let mut image_bytes = 0usize;
    let mut image_pixels = 0u64;
    for image in source_images {
        let bytes = if let Some(uri) = image["uri"].as_str() {
            data_uri(uri)?
        } else {
            buffer_view(&root, &buffers, integer(image, "bufferView", usize::MAX)?)?.to_vec()
        };
        image_bytes =
            image_bytes.checked_add(bytes.len()).ok_or_else(|| ImportError("Image size overflows.".into()))?;
        if image_bytes > 64 * 1024 * 1024 {
            return error("Embedded images exceed 64 MiB.");
        }
        let mime = image["mimeType"].as_str().unwrap_or(if bytes.starts_with(b"\x89PNG") {
            "image/png"
        } else {
            "image/jpeg"
        });
        if !["image/png", "image/jpeg"].contains(&mime) {
            return error("GLB textures must be embedded PNG or JPEG images.");
        }
        let format = image::guess_format(&bytes).map_err(|e| ImportError(e.to_string()))?;
        let (w, h) = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format)
            .into_dimensions()
            .map_err(|e| ImportError(e.to_string()))?;
        if w == 0 || h == 0 || w as u64 * h as u64 > 16_777_216 {
            return error("GLB image exceeds 16 million pixels.");
        }
        image_pixels += w as u64 * h as u64;
        if image_pixels > 33_554_432 {
            return error("GLB images exceed the 32 million pixel budget.");
        }
        let id = ImageId(document.reserve_id());
        document.images.insert(
            id,
            Image {
                name: image["name"].as_str().unwrap_or("Imported texture").into(),
                mime: mime.into(),
                bytes: Arc::new(bytes),
            },
        );
        images.push(id);
    }
    let mut materials = Vec::new();
    for material in list(&root, "materials")? {
        let pbr = &material["pbrMetallicRoughness"];
        let mut color = floats(pbr.get("baseColorFactor"), [1.; 4])?.map(|v| v as f32);
        if color.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return error("Invalid GLB material colour.");
        }
        for v in &mut color[..3] {
            *v = if *v <= 0.0031308 { *v * 12.92 } else { 1.055 * v.powf(1. / 2.4) - 0.055 };
        }
        if material["alphaMode"].as_str().unwrap_or("OPAQUE") != "BLEND" {
            color[3] = 1.;
        }
        let texture = if let Some(info) = pbr.get("baseColorTexture") {
            if integer(info, "texCoord", 0)? != 0 {
                return error("Only GLB texture coordinate set 0 is supported.");
            }
            let texture = item(&root, "textures", integer(info, "index", usize::MAX)?)?;
            let image = *images
                .get(integer(texture, "source", usize::MAX)?)
                .ok_or_else(|| ImportError("Missing GLB texture image.".into()))?;
            Some(Texture { image, size: [1., 1.] })
        } else {
            None
        };
        let normal_map = if let Some(info) = material.get("normalTexture") {
            if integer(info, "texCoord", 0)? != 0 {
                return error("Only normal map coordinate set 0 is supported.");
            }
            let texture = item(&root, "textures", integer(info, "index", usize::MAX)?)?;
            Some(
                *images
                    .get(integer(texture, "source", usize::MAX)?)
                    .ok_or_else(|| ImportError("Missing normal map image".into()))?,
            )
        } else {
            None
        };
        let surface = rysmo_model::Pbr {
            roughness: pbr
                .get("roughnessFactor")
                .map_or(Ok(1.), |v| v.as_f64().ok_or_else(|| ImportError("Invalid roughness".into())))?
                as f32,
            metallic: pbr
                .get("metallicFactor")
                .map_or(Ok(1.), |v| v.as_f64().ok_or_else(|| ImportError("Invalid metallic".into())))?
                as f32,
            normal_map,
            normal_scale: material
                .get("normalTexture")
                .and_then(|v| v.get("scale"))
                .map_or(Ok(1.), |v| v.as_f64().ok_or_else(|| ImportError("Invalid normal scale".into())))?
                as f32,
            normal_flip_y: false,
            tint_texture: true,
        };
        if !surface.valid() {
            return error("Unsupported material surface values.");
        }
        if pbr.get("metallicRoughnessTexture").is_some() {
            warnings.push("Metallic/roughness image channels are not loaded; scalar factors are retained.".into());
        }
        let id = MaterialId(document.reserve_id());
        document.materials.insert(
            id,
            Material {
                name: material["name"].as_str().unwrap_or("Imported material").into(),
                color,
                texture,
                glow: 0.,
                pbr: surface,
            },
        );
        materials.push(id);
    }
    let mut meshes = Vec::new();
    let mut total_faces = 0;
    let mut total_positions = 0usize;
    for source in list(&root, "meshes")? {
        let mut geometry = Geometry::default();
        for primitive in list(source, "primitives")? {
            let position = integer(&primitive["attributes"], "POSITION", usize::MAX)?;
            total_positions = total_positions
                .checked_add(integer(item(&root, "accessors", position)?, "count", 0)?)
                .ok_or_else(|| ImportError("Vertex count overflows.".into()))?;
            if total_positions > 400_000 {
                return error("GLB exceeds the decoded vertex budget.");
            }
            let raw = accessor(&root, &buffers, position, 3)?;
            if raw.len() > 200_000 {
                return error("GLB mesh exceeds the vertex limit.");
            }
            let positions: Vec<_> = raw.iter().map(|p| from_y(DVec3::new(p[0], p[1], p[2]))).collect();
            let uv = if primitive["attributes"].get("TEXCOORD_0").is_some() {
                Some(
                    accessor(&root, &buffers, integer(&primitive["attributes"], "TEXCOORD_0", usize::MAX)?, 2)?
                        .iter()
                        .map(|p| DVec2::new(p[0], p[1]))
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            };
            if uv.as_ref().is_some_and(|u| u.len() != positions.len()) {
                return error("GLB UV count differs from POSITION count.");
            }
            let indices = if primitive.get("indices").is_some() {
                let index = integer(primitive, "indices", usize::MAX)?;
                let a = item(&root, "accessors", index)?;
                if ![5121, 5123, 5125].contains(&integer(a, "componentType", 0)?)
                    || a["normalized"].as_bool() == Some(true)
                {
                    return error("Invalid GLB index accessor.");
                }
                accessor(&root, &buffers, index, 1)?.into_iter().map(|v| v[0] as usize).collect::<Vec<_>>()
            } else {
                (0..positions.len()).collect()
            };
            if indices.iter().any(|i| *i >= positions.len()) {
                return error("GLB triangle index is out of bounds.");
            }
            let material = primitive
                .get("material")
                .map(|_| {
                    materials
                        .get(integer(primitive, "material", usize::MAX)?)
                        .copied()
                        .ok_or_else(|| ImportError("Missing GLB material.".into()))
                })
                .transpose()?;
            let mode = integer(primitive, "mode", 4)?;
            if matches!(mode, 1..=3) {
                let mut pairs: Vec<[usize; 2]> = if mode == 1 {
                    if !indices.len().is_multiple_of(2) {
                        return error("GLB LINES has an odd index count.");
                    }
                    indices.as_chunks::<2>().0.to_vec()
                } else {
                    indices.windows(2).map(|w| [w[0], w[1]]).collect()
                };
                if mode == 2 && indices.len() > 1 {
                    pairs.push([indices[indices.len() - 1], indices[0]]);
                }
                for [a, b] in pairs {
                    geometry.line(positions[a], positions[b])?;
                }
                continue;
            }
            let triangles: Vec<[usize; 3]> = match mode {
                4 => {
                    if !indices.len().is_multiple_of(3) {
                        return error("GLB TRIANGLES has an incomplete triangle.");
                    }
                    indices.as_chunks::<3>().0.to_vec()
                }
                5 => indices
                    .windows(3)
                    .enumerate()
                    .map(|(i, w)| if i % 2 == 0 { [w[0], w[1], w[2]] } else { [w[1], w[0], w[2]] })
                    .collect(),
                6 => {
                    if indices.len() < 3 {
                        Vec::new()
                    } else {
                        (1..indices.len() - 1).map(|i| [indices[0], indices[i], indices[i + 1]]).collect()
                    }
                }
                _ => return error("GLB POINTS and unknown primitive modes are not supported."),
            };
            total_faces += triangles.len();
            if total_faces > 100_000 {
                return error("GLB exceeds 100,000 source triangles.");
            }
            for indices in triangles {
                let p = indices.map(|i| positions[i]);
                if (p[1] - p[0]).cross(p[2] - p[0]).length() < 1e-12 {
                    continue;
                }
                let uv = uv.as_ref().map(|uv| indices.map(|i| uv[i]));
                geometry.polygon(&p, uv.as_ref().map(|a| a.as_slice()), material)?;
            }
            if primitive.get("targets").is_some() {
                warnings.push("Morph targets are imported in their base shape.".into());
            }
        }
        let mesh = geometry.finish()?.with_fresh_ids(|| document.reserve_id())?;
        let id = DefinitionId(document.reserve_id());
        document.definitions.insert(
            id,
            Definition { name: source["name"].as_str().unwrap_or("Imported mesh").into(), mesh, ..Default::default() },
        );
        meshes.push(id);
    }
    let roots: Vec<usize> = if list(&root, "scenes")?.is_empty() {
        let mut children = BTreeSet::new();
        for node in list(&root, "nodes")? {
            for child in list(node, "children")? {
                children.insert(child.as_u64().ok_or_else(|| ImportError("Invalid GLB child.".into()))? as usize);
            }
        }
        (0..list(&root, "nodes")?.len()).filter(|i| !children.contains(i)).collect()
    } else {
        list(item(&root, "scenes", integer(&root, "scene", 0)?)?, "nodes")?
            .iter()
            .map(|v| v.as_u64().map(|v| v as usize).ok_or_else(|| ImportError("Invalid GLB scene root.".into())))
            .collect::<Result<Vec<_>>>()?
    };
    let mut nodes = Nodes { root: &root, meshes: &meshes, built: BTreeMap::new(), visiting: BTreeSet::new() };
    for index in roots {
        let definition = nodes.build(&mut document, index)?;
        let transform = DMat4::from_scale(DVec3::splat(options.unit_scale.unwrap_or(1.)))
            * node_transform(item(&root, "nodes", index)?)?;
        let id = InstanceId(document.reserve_id());
        let instance = add_instance(definition, transform, String::new());
        document.root.instances.insert(id, instance);
    }
    if !list(&root, "skins")?.is_empty() || !list(&root, "animations")?.is_empty() {
        warnings.push("Animation and skin deformation are not imported; this is a static model.".into());
    }
    warnings.sort();
    warnings.dedup();
    Ok(Imported { document, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn interleaved_accessors_and_sparse_overrides_use_independent_view_offsets() {
        let mut bytes = Vec::new();
        for v in [1f32, 2., 3., 999., 4., 5., 6., 999.] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(&[1, 0, 0, 0]);
        for v in [7f32, 8., 9.] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let buffers = [bytes];
        let mut root = json!({"bufferViews":[{"buffer":0,"byteLength":32,"byteStride":16},{"buffer":0,"byteOffset":32,"byteLength":1},{"buffer":0,"byteOffset":36,"byteLength":12}],"accessors":[{"bufferView":0,"componentType":5126,"count":2,"type":"VEC3","sparse":{"count":1,"indices":{"bufferView":1,"componentType":5121},"values":{"bufferView":2}}}]});
        assert_eq!(accessor(&root, &buffers, 0, 3).unwrap(), vec![vec![1., 2., 3.], vec![7., 8., 9.]]);
        root["bufferViews"][0]["byteStride"] = json!(8);
        assert!(accessor(&root, &buffers, 0, 3).is_err());
        root["bufferViews"][0]["byteStride"] = json!(16);
        root["accessors"][0]["sparse"]["count"] = json!(2);
        assert!(accessor(&root, &buffers, 0, 3).is_err());
    }
    #[test]
    fn cyclic_nodes_fail_before_a_document_is_installed() {
        let root = json!({"nodes":[{"children":[1]},{"children":[0]}]});
        let meshes = [];
        let mut nodes = Nodes { root: &root, meshes: &meshes, built: BTreeMap::new(), visiting: BTreeSet::new() };
        assert!(nodes.build(&mut Document::new(), 0).is_err());
    }
}
