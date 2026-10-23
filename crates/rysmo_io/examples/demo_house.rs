//! Build the demo model for the README screenshot: a modern villa with a pool on hilly terrain.
#[path = "shared/cybertruck.rs"]
mod cybertruck;
#[path = "shared/textures.rs"]
mod textures;

use rysmo_math::{DVec2, DVec3, OrbitCamera};
use rysmo_model::{Command, Editor, Element, InstanceId, MaterialId};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Paints {
    render: MaterialId,
    concrete: MaterialId,
    wood: MaterialId,
    deck: MaterialId,
    clear: MaterialId,
    tinted: MaterialId,
    railing: MaterialId,
    dark: MaterialId,
    steel: MaterialId,
    stone: MaterialId,
    water: MaterialId,
    paving: MaterialId,
    asphalt: MaterialId,
    marking: MaterialId,
    embers: MaterialId,
    coping: MaterialId,
    tile: MaterialId,
    chrome: MaterialId,
    taillight: MaterialId,
    firewood: MaterialId,
    bark: MaterialId,
    /// Autumn foliage, chosen per tree from its position.
    autumn: [MaterialId; 4],
    pumpkin: MaterialId,
    conifer: MaterialId,
    hedge: MaterialId,
    fabric: MaterialId,
    linen: MaterialId,
    accent: MaterialId,
    outdoor: MaterialId,
    oak: MaterialId,
    teak: MaterialId,
    rug: MaterialId,
    solar: MaterialId,
    /// Ecru, navy and terracotta canvas.
    parasol: [MaterialId; 3],
    lamp: MaterialId,
    pool: MaterialId,
    plinth: MaterialId,
    terracotta: MaterialId,
    frame: MaterialId,
    rubber: MaterialId,
    coir: MaterialId,
    lacquer: MaterialId,
    worktop: MaterialId,
    cars: [MaterialId; 3],
    bins: [MaterialId; 2],
    gravel: MaterialId,
    /// Cut-out lawn tufts, ornamental grass and flowering perennials, with each image's
    /// height and width in metres.
    plants: [(MaterialId, [f64; 2]); 3],
}

fn material(e: &mut Editor, name: &str, color: [f32; 4]) -> Result<MaterialId> {
    let out = e.apply(Command::AddMaterial { name: name.into(), color, texture: None })?;
    Ok(out.material.ok_or("no material")?)
}

/// A material with its surface roughness and metalness, from an sRGB colour.
fn surface(e: &mut Editor, name: &str, color: [f32; 4], roughness: f32, metallic: f32) -> Result<MaterialId> {
    let m = material(e, name, color)?;
    let pbr = rysmo_model::Pbr { roughness, metallic, ..Default::default() };
    e.apply(Command::SetMaterialPbr { material: m, pbr })?;
    Ok(m)
}

/// A material that gives off light, in cd/m².
fn glowing(e: &mut Editor, name: &str, color: [f32; 4], glow: f32) -> Result<MaterialId> {
    let m = material(e, name, color)?;
    e.apply(Command::SetMaterialGlow { material: m, glow })?;
    Ok(m)
}

/// A pendant lamp: a cord from a ceiling rose and a glowing glass shade.
fn pendant(e: &mut Editor, at: DVec3, ceiling: f64, p: &Paints) -> Result<()> {
    column(e, at + DVec3::Z * 0.18, 0.004, ceiling - at.z - 0.2, 8, p.dark)?;
    column(e, DVec3::new(at.x, at.y, ceiling - 0.03), 0.05, 0.03, 24, p.dark)?;
    lathe(e, at, &[(0., 0.02), (0.15, 0.), (0.16, 0.012), (0.06, 0.16), (0.02, 0.2), (0., 0.2)], 40, p.lamp)?;
    Ok(())
}

/// A square downlight set into the ceiling.
fn downlight(e: &mut Editor, x: f64, y: f64, ceiling: f64, p: &Paints) -> Result<()> {
    cuboid(e, DVec3::new(x - 0.1, y - 0.1, ceiling - 0.02), DVec3::new(x + 0.1, y + 0.1, ceiling), p.lamp)?;
    Ok(())
}

/// Groups all loose geometry of the current context.
fn group_loose(e: &mut Editor) -> Result<InstanceId> {
    let mesh = e.mesh();
    let mut elements: Vec<Element> = mesh.faces().map(|(f, _)| Element::Face(f)).collect();
    elements.extend(mesh.edges().map(|(edge, _)| Element::Edge(edge)));
    let out = e.apply(Command::MakeGroup { elements })?;
    match out.created.first() {
        Some(Element::Instance(i)) => Ok(*i),
        _ => Err("no group".into()),
    }
}

fn paint(e: &mut Editor, group: InstanceId, paint: MaterialId) -> Result<InstanceId> {
    e.apply(Command::PaintInstances { instances: vec![group], material: Some(paint) })?;
    Ok(group)
}

/// Pushes the largest loose face up by `height` and groups it.
fn extrude(e: &mut Editor, height: f64, colour: MaterialId) -> Result<InstanceId> {
    let mesh = e.mesh();
    let face = mesh
        .faces()
        .map(|(f, _)| f)
        .max_by(|a, b| mesh.face_area(*a).total_cmp(&mesh.face_area(*b)))
        .ok_or("no face")?;
    let up = mesh.face_normal(face).ok_or("no normal")?.z.signum();
    e.apply(Command::PushPull { face, distance: height * up, keep_base: false })?;
    let group = group_loose(e)?;
    paint(e, group, colour)
}

/// A box between two opposite corners, as its own group.
fn cuboid(e: &mut Editor, a: DVec3, b: DVec3, colour: MaterialId) -> Result<InstanceId> {
    let (min, max) = (a.min(b), a.max(b));
    let size = max - min;
    e.apply(Command::DrawRectangle { corner: min, u: DVec3::X * size.x, v: DVec3::Y * size.y, facing: None })?;
    extrude(e, size.z, colour)
}

/// A horizontal slab on a four-cornered outline, `height` thick, as its own group.
fn slab(e: &mut Editor, outline: &[DVec3; 4], height: f64, colour: MaterialId) -> Result<InstanceId> {
    let lines = (0..4).map(|i| [outline[i], outline[(i + 1) % 4]]).collect();
    e.apply(Command::DrawLines { lines, facing: Some(DVec3::Z) })?;
    extrude(e, height, colour)
}

/// Smooths the edges of `group` between faces meeting at less than `crease` degrees, so
/// curved surfaces shade round while real corners stay crisp.
fn crease(e: &mut Editor, group: InstanceId, crease: f64) -> Result<InstanceId> {
    e.enter(group)?;
    let mesh = e.mesh();
    // A broad face stays flat right up to its edge: smoothing into it would tint the whole
    // face, so only edges between narrow faces (fillets, bevels, cylinder strips) round off.
    let narrow = |edge: rysmo_model::EdgeId| {
        let (a, b) = mesh.edge_points(edge);
        let length = a.distance(b).max(1e-9);
        mesh.faces_of_edge(edge).iter().all(|f| mesh.face_area(*f) / length < 0.12)
    };
    let edges: Vec<_> = mesh
        .edges()
        .map(|(edge, _)| edge)
        .filter(|edge| mesh.edge_angle(*edge).is_some_and(|a| a < 1e-6 || a < crease && narrow(*edge)))
        .collect();
    if !edges.is_empty() {
        e.apply(Command::SetEdges { edges, soft: Some(true), smooth: Some(true), hidden: None })?;
    }
    e.exit();
    Ok(group)
}

/// A closed solid from outward-wound triangles, as its own group, smoothed below `angle`.
fn solid(e: &mut Editor, triangles: &[[DVec3; 3]], angle: f64, colour: MaterialId) -> Result<InstanceId> {
    let normal = |t: &[DVec3; 3]| (t[1] - t[0]).cross(t[2] - t[0]);
    let mut triangles: Vec<[DVec3; 3]> = triangles.iter().copied().filter(|t| normal(t).length() > 1e-10).collect();
    // A closed mesh wound inside out has a negative volume; turn it the right way round.
    let volume: f64 = triangles.iter().map(|t| t[0].dot(t[1].cross(t[2]))).sum();
    if volume < 0. {
        triangles.iter_mut().for_each(|t| t.swap(1, 2));
    }
    let lines = triangles.iter().flat_map(|t| [[t[0], t[1]], [t[1], t[2]], [t[2], t[0]]]).collect();
    e.apply(Command::DrawLines { lines, facing: None })?;
    // Drawing lines loses the winding; turn each face to match its triangle.
    let key = |c: DVec3| ((c.x * 1e4).round() as i64, (c.y * 1e4).round() as i64, (c.z * 1e4).round() as i64);
    let wanted: std::collections::HashMap<_, DVec3> =
        triangles.iter().map(|t| (key((t[0] + t[1] + t[2]) / 3.), normal(t))).collect();
    let mesh = e.mesh();
    let flip: Vec<_> = mesh
        .faces()
        .filter(|(id, f)| {
            let c = f.outer.iter().map(|v| mesh.pos(*v)).sum::<DVec3>() / f.outer.len() as f64;
            let n = mesh.face_normal(*id).unwrap_or(DVec3::Z);
            wanted.get(&key(c)).is_some_and(|w| n.dot(*w) < 0.)
        })
        .map(|(id, _)| id)
        .collect();
    if !flip.is_empty() {
        e.apply(Command::ReverseFaces { faces: flip })?;
    }
    let group = group_loose(e)?;
    crease(e, group, angle)?;
    paint(e, group, colour)
}

/// A solid through closed rings of equal length, each wound anticlockwise seen from above
/// and listed from the bottom up; a ring of one repeated point closes an end.
fn loft(e: &mut Editor, rings: &[Vec<DVec3>], angle: f64, colour: MaterialId) -> Result<InstanceId> {
    let mut triangles = Vec::new();
    for pair in rings.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        for k in 0..a.len() {
            let n = (k + 1) % a.len();
            triangles.push([a[k], a[n], b[n]]);
            triangles.push([a[k], b[n], b[k]]);
        }
    }
    solid(e, &triangles, angle, colour)
}

/// A solid of revolution about a vertical axis through `foot`: `profile` lists (radius,
/// height) pairs from the bottom of the axis, up the outside and back to the axis.
fn lathe(e: &mut Editor, foot: DVec3, profile: &[(f64, f64)], sides: u32, colour: MaterialId) -> Result<InstanceId> {
    lathe_with(e, foot, profile, sides, |_| 1., colour)
}

/// A solid of revolution whose radius is scaled by `shape` of the angle (ribs, lobes).
fn lathe_with(
    e: &mut Editor,
    foot: DVec3,
    profile: &[(f64, f64)],
    sides: u32,
    shape: impl Fn(f64) -> f64,
    colour: MaterialId,
) -> Result<InstanceId> {
    let rings: Vec<Vec<DVec3>> = profile
        .iter()
        .map(|&(r, z)| {
            (0..sides)
                .map(|k| {
                    let a = k as f64 * std::f64::consts::TAU / sides as f64;
                    foot + DVec3::new(a.cos() * r * shape(a), a.sin() * r * shape(a), z)
                })
                .collect()
        })
        .collect();
    loft(e, &rings, 35., colour)
}

/// A rounded rectangle in plan at height `z`, corners of `radius` about the corner centres.
fn rounded_ring(min: DVec2, max: DVec2, inset: f64, radius: f64, z: f64, segments: usize) -> Vec<DVec3> {
    let r = radius - inset;
    let c = [
        DVec2::new(max.x - radius, max.y - radius),
        DVec2::new(min.x + radius, max.y - radius),
        DVec2::new(min.x + radius, min.y + radius),
        DVec2::new(max.x - radius, min.y + radius),
    ];
    let mut ring = Vec::new();
    for (i, centre) in c.iter().enumerate() {
        for k in 0..=segments {
            let a = (i as f64 + k as f64 / segments as f64) * std::f64::consts::FRAC_PI_2;
            ring.push((*centre + DVec2::new(a.cos(), a.sin()) * r).extend(z));
        }
    }
    ring
}

/// A box with rounded vertical edges and a rounded-off top edge, as its own group.
fn rounded(e: &mut Editor, a: DVec3, b: DVec3, radius: f64, colour: MaterialId) -> Result<InstanceId> {
    let (min, max) = (a.min(b), a.max(b));
    let size = max - min;
    let radius = radius.min(size.x.min(size.y) * 0.45).max(0.002);
    let bevel = (radius * 0.6).min(size.z * 0.35);
    let (lo, hi) = (min.truncate(), max.truncate());
    let middle = ((lo + hi) / 2.).extend(0.);
    // Small radii need few segments to look round.
    let segments = if radius < 0.015 {
        2
    } else if radius < 0.05 {
        3
    } else {
        5
    };
    let point = |z: f64| vec![middle + DVec3::Z * z; 4 * (segments + 1)];
    let mut rings = vec![point(min.z), rounded_ring(lo, hi, 0., radius, min.z, segments)];
    // Two rings round the top edge off in a quarter arc.
    for k in 0..=2 {
        let t = k as f64 / 2. * std::f64::consts::FRAC_PI_2;
        let z = max.z - bevel + bevel * t.sin();
        rings.push(rounded_ring(lo, hi, bevel * (1. - t.cos()), radius, z, segments));
    }
    rings.push(point(max.z));
    loft(e, &rings, 50., colour)
}

/// A vertical cylinder or prism (posts, pots, stools); round when it has many sides.
fn column(e: &mut Editor, foot: DVec3, radius: f64, height: f64, sides: u32, colour: MaterialId) -> Result<InstanceId> {
    lathe(e, foot, &[(0., 0.), (radius, 0.), (radius, height), (0., height)], sides, colour)
}

/// Turns groups about a vertical axis through `centre`.
fn turn(e: &mut Editor, groups: &[InstanceId], centre: DVec3, degrees: f64) -> Result<()> {
    if degrees != 0. {
        let elements = groups.iter().map(|g| Element::Instance(*g)).collect();
        e.apply(Command::Rotate { elements, centre, axis: DVec3::Z, angle: degrees, copies: 0 })?;
    }
    Ok(())
}

/// A small deterministic random generator (SplitMix64), so the scene is the same each run.
struct Random(u64);
impl Random {
    /// A value in `0..1`.
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        (z ^ (z >> 31)) as f64 / u64::MAX as f64
    }
    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.next()
    }
}

/// A repeatable pick among `count` variants from a position, so the scene is the same each run.
fn pick(at: DVec3, count: usize) -> usize {
    ((at.x * 7.31 + at.y * 3.17).abs() * 13.) as usize % count
}

/// A closed solid from triangle loops, smooth shaded, faces turned away from its centre.
fn blob_solid(e: &mut Editor, triangles: &[[DVec3; 3]], colour: MaterialId) -> Result<InstanceId> {
    let lines = triangles.iter().flat_map(|t| [[t[0], t[1]], [t[1], t[2]], [t[2], t[0]]]).collect();
    e.apply(Command::DrawLines { lines, facing: None })?;
    let mesh = e.mesh();
    let points: Vec<DVec3> = mesh.vertices().map(|(_, v)| v.pos).collect();
    let middle = points.iter().copied().sum::<DVec3>() / points.len().max(1) as f64;
    let inward: Vec<_> = mesh
        .faces()
        .filter(|(id, f)| {
            let c = f.outer.iter().map(|v| mesh.pos(*v)).sum::<DVec3>() / f.outer.len() as f64;
            mesh.face_normal(*id).is_some_and(|n| n.dot(c - middle) < 0.)
        })
        .map(|(id, _)| id)
        .collect();
    if !inward.is_empty() {
        e.apply(Command::ReverseFaces { faces: inward })?;
    }
    let group = group_loose(e)?;
    e.enter(group)?;
    let edges = e.mesh().edges().map(|(edge, _)| edge).collect();
    e.apply(Command::SetEdges { edges, soft: Some(true), smooth: Some(true), hidden: None })?;
    e.exit();
    paint(e, group, colour)
}

/// An irregular, rounded clump of leaves: a once-subdivided icosahedron with a lumpy surface.
fn clump(e: &mut Editor, centre: DVec3, radii: DVec3, random: &mut Random, colour: MaterialId) -> Result<()> {
    let t = (1. + 5f64.sqrt()) / 2.;
    let mut v: Vec<DVec3> = [
        (-1., t, 0.),
        (1., t, 0.),
        (-1., -t, 0.),
        (1., -t, 0.),
        (0., -1., t),
        (0., 1., t),
        (0., -1., -t),
        (0., 1., -t),
        (t, 0., -1.),
        (t, 0., 1.),
        (-t, 0., -1.),
        (-t, 0., 1.),
    ]
    .iter()
    .map(|&(x, y, z)| DVec3::new(x, y, z).normalize())
    .collect();
    let faces = [
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let mut mids = std::collections::BTreeMap::new();
    let mut mid = |a: usize, b: usize, v: &mut Vec<DVec3>| -> usize {
        *mids.entry((a.min(b), a.max(b))).or_insert_with(|| {
            v.push((v[a] + v[b]).normalize());
            v.len() - 1
        })
    };
    // Two subdivisions give a rounded mass of 320 triangles near the house; one, of 80, is
    // enough further away.
    let mut tris: Vec<[usize; 3]> = faces.to_vec();
    let levels = if centre.truncate().length() < 45. { 2 } else { 1 };
    for _ in 0..levels {
        let mut finer = Vec::new();
        for [a, b, c] in tris {
            let (ab, bc, ca) = (mid(a, b, &mut v), mid(b, c, &mut v), mid(c, a, &mut v));
            finer.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        tris = finer;
    }
    // A few soft bulges of leaves, and a little irregularity everywhere.
    let lobes: Vec<DVec3> = (0..5)
        .map(|_| {
            DVec3::new(random.range(-1., 1.), random.range(-1., 1.), random.range(-0.6, 1.)).normalize_or(DVec3::Z)
        })
        .collect();
    let bumps: Vec<f64> = v
        .iter()
        .map(|d| {
            let bulge: f64 = lobes.iter().map(|l| d.dot(*l).max(0.).powi(4)).sum();
            0.84 + 0.2 * bulge.min(1.2) + random.range(-0.04, 0.04)
        })
        .collect();
    let place = |i: usize| centre + v[i] * radii * bumps[i];
    let triangles: Vec<[DVec3; 3]> = tris.iter().map(|t| t.map(place)).collect();
    blob_solid(e, &triangles, colour)?;
    Ok(())
}

/// A straight round bar or tapered limb from `from` to `to`.
fn limb(e: &mut Editor, from: DVec3, to: DVec3, r0: f64, r1: f64, colour: MaterialId) -> Result<InstanceId> {
    let axis = (to - from).normalize();
    let side = axis.any_orthonormal_vector();
    let other = axis.cross(side);
    // Enough sides to look round at its size.
    let r = r0.max(r1);
    let sides = if r > 0.06 {
        24
    } else if r >= 0.02 {
        12
    } else {
        6
    };
    let ring = |c: DVec3, r: f64| -> Vec<DVec3> {
        (0..sides)
            .map(|k| {
                let a = k as f64 * std::f64::consts::TAU / sides as f64;
                c + (side * a.cos() + other * a.sin()) * r
            })
            .collect()
    };
    let (a, b) = (ring(from, r0), ring(to, r1));
    let mut triangles = Vec::new();
    for k in 0..sides {
        let n = (k + 1) % sides;
        triangles.push([a[k], a[n], b[n]]);
        triangles.push([a[k], b[n], b[k]]);
        triangles.push([from, a[n], a[k]]);
        triangles.push([to, b[k], b[n]]);
    }
    solid(e, &triangles, 40., colour)
}

/// A bent bar through `points`, as round segments.
fn rail(e: &mut Editor, points: &[DVec3], radius: f64, colour: MaterialId) -> Result<()> {
    for w in points.windows(2) {
        limb(e, w[0], w[1], radius, radius, colour)?;
    }
    Ok(())
}

/// A slab of `depth` behind a flat four-cornered `quad` in any orientation, as its own group.
fn prism(e: &mut Editor, quad: [DVec3; 4], depth: DVec3, colour: MaterialId) -> Result<InstanceId> {
    let back = quad.map(|q| q + depth);
    let centre = (quad.iter().sum::<DVec3>() + back.iter().sum::<DVec3>()) / 8.;
    let mut triangles = vec![
        [quad[0], quad[1], quad[2]],
        [quad[0], quad[2], quad[3]],
        [back[0], back[1], back[2]],
        [back[0], back[2], back[3]],
    ];
    for k in 0..4 {
        let n = (k + 1) % 4;
        triangles.push([quad[k], quad[n], back[n]]);
        triangles.push([quad[k], back[n], back[k]]);
    }
    // The prism is convex, so each face turns away from its middle.
    for t in &mut triangles {
        let c = (t[0] + t[1] + t[2]) / 3.;
        if (t[1] - t[0]).cross(t[2] - t[0]).dot(c - centre) < 0. {
            t.swap(1, 2);
        }
    }
    solid(e, &triangles, 20., colour)
}

/// Everything `build` makes at the top level.
fn built(e: &mut Editor, build: impl FnOnce(&mut Editor) -> Result<()>) -> Result<Vec<InstanceId>> {
    let before: std::collections::BTreeSet<InstanceId> = e.active().instances.keys().copied().collect();
    build(e)?;
    Ok(e.active().instances.keys().filter(|i| !before.contains(i)).copied().collect())
}

/// A broadleaf tree in autumn: a tapering trunk forking into limbs, each ending in a cluster of
/// leaf clumps on twigs, so every clump hangs on wood; fallen leaves lie on `ground`.
fn tree(e: &mut Editor, foot: DVec3, size: f64, ground: &dyn Fn(f64, f64) -> f64, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let mut random = Random((foot.x * 1000.) as i64 as u64 ^ ((foot.y * 1000.) as i64 as u64).rotate_left(32));
        let fork = foot + DVec3::Z * 2.4 * size;
        limb(e, foot - DVec3::Z * 0.1, fork + DVec3::Z * 0.3 * size, 0.18 * size, 0.11 * size, p.bark)?;
        // Root flare where the trunk meets the ground.
        lathe(
            e,
            foot - DVec3::Z * 0.05,
            &[(0., 0.), (0.32 * size, 0.), (0.2 * size, 0.12 * size), (0.15 * size, 0.35 * size), (0., 0.35 * size)],
            16,
            p.bark,
        )?;
        let hue = pick(foot, p.autumn.len());
        let limbs = 6;
        for k in 0..limbs {
            let a = k as f64 * std::f64::consts::TAU / limbs as f64 + random.range(-0.3, 0.3);
            let reach = random.range(1.0, 1.6) * size;
            let start = foot + DVec3::Z * random.range(2.1, 2.6) * size;
            let end = start + DVec3::new(a.cos() * reach, a.sin() * reach, random.range(1.0, 1.8) * size);
            limb(e, start, end, 0.08 * size, 0.035 * size, p.bark)?;
            // A twig on up from the elbow and leaf clumps round the limb's end.
            let tip = end + DVec3::new(a.cos() * 0.3, a.sin() * 0.3, 0.6) * size;
            limb(e, end, tip, 0.035 * size, 0.015 * size, p.bark)?;
            for j in 0..4 {
                let b = random.range(0., std::f64::consts::TAU);
                let r = random.range(0.42, 0.62) * size;
                let spread = if j == 0 { 0. } else { random.range(0.4, 0.75) * r };
                let at = if j == 0 { tip } else { end }
                    + DVec3::new(b.cos() * spread, b.sin() * spread, random.range(-0.2, 0.4) * r);
                if j > 0 {
                    limb(e, end, at, 0.018 * size, 0.01 * size, p.bark)?;
                }
                let radii = DVec3::new(r, r * random.range(0.85, 1.15), r * random.range(0.7, 0.9));
                // Mostly one hue per tree, with a neighbouring colour mixed in.
                let colour = if random.next() < 0.75 { p.autumn[hue] } else { p.autumn[(hue + 1) % p.autumn.len()] };
                clump(e, at, radii, &mut random, colour)?;
            }
        }
        // A clump over the fork closes the crown.
        let crown = fork + DVec3::Z * 1.6 * size;
        limb(e, fork, crown, 0.08 * size, 0.03 * size, p.bark)?;
        clump(e, crown, DVec3::new(0.8, 0.8, 0.65) * size, &mut random, p.autumn[hue])?;
        // Fallen leaves scattered under the crown, denser near the trunk.
        for _ in 0..22 {
            let angle = random.range(0., std::f64::consts::TAU);
            let reach = random.next().sqrt() * 2.6 * size + 0.4;
            let at = foot.truncate() + DVec2::new(angle.cos(), angle.sin()) * reach;
            let colour = p.autumn[(random.next() * p.autumn.len() as f64) as usize];
            leaf(e, at.extend(ground(at.x, at.y) + 0.002), &mut random, colour)?;
        }
        Ok(())
    })
}

/// A fallen leaf: a small flat hexagon with a random size and turn.
fn leaf(e: &mut Editor, at: DVec3, random: &mut Random, colour: MaterialId) -> Result<()> {
    let turn = random.range(0., std::f64::consts::TAU);
    let start = Some(DVec3::new(turn.cos(), turn.sin(), 0.));
    let radius = random.range(0.05, 0.1);
    e.apply(Command::DrawPolygon { centre: at, normal: DVec3::Z, radius, segments: 6, start })?;
    extrude(e, 0.006, colour)?;
    Ok(())
}

/// A pumpkin: a ribbed, flattened sphere with a stalk.
fn pumpkin(e: &mut Editor, foot: DVec3, size: f64, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let (r, h) = (0.2 * size, 0.26 * size);
        let profile: Vec<(f64, f64)> = (0..=12)
            .map(|k| {
                let t = -std::f64::consts::FRAC_PI_2 + k as f64 / 12. * std::f64::consts::PI;
                // A dimple at the top and bottom.
                (r * t.cos(), h / 2. * (1. + t.sin()))
            })
            .collect();
        lathe_with(e, foot, &profile, 40, |a| 1. - 0.06 * (1. - (10. * a).cos()) / 2., p.pumpkin)?;
        lathe(
            e,
            foot + DVec3::Z * (h - 0.01),
            &[(0., 0.), (0.02 * size, 0.), (0.014 * size, 0.07 * size), (0., 0.07 * size)],
            7,
            p.bark,
        )?;
        Ok(())
    })
}

/// A spruce: a straight trunk to the tip and drooping, slightly ragged tiers of branches.
fn conifer(e: &mut Editor, foot: DVec3, size: f64, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let height = 5.2 * size;
        limb(e, foot - DVec3::Z * 0.1, foot + DVec3::Z * height * 0.92, 0.14 * size, 0.03 * size, p.bark)?;
        let seed = foot.x * 1.7 + foot.y;
        let tiers = 6;
        for k in 0..tiers {
            let t = k as f64 / tiers as f64;
            let z = (0.6 + t * 3.9) * size;
            let r = (1.6 - t * 1.3) * size;
            let h = 1.3 * size * (1. - t * 0.35);
            let phase = seed + k as f64 * 2.1;
            lathe_with(
                e,
                foot + DVec3::Z * z,
                &[(0., 0.35 * h), (r, 0.), (r * 0.94, 0.06 * h), (r * 0.5, 0.45 * h), (0., h)],
                28,
                move |a| 1. + 0.08 * (7. * a + phase).sin() + 0.05 * (13. * a - phase).cos(),
                p.conifer,
            )?;
        }
        Ok(())
    })
}

/// A shrub: a few lumpy clumps resting on the ground.
fn bush(e: &mut Editor, foot: DVec3, size: f64, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        // Some shrubs turn red in autumn, the rest stay olive.
        let colour = if pick(foot, 3) == 0 { p.autumn[1] } else { p.hedge };
        let mut random = Random((foot.x * 977.) as i64 as u64 ^ ((foot.y * 613.) as i64 as u64).rotate_left(21));
        for k in 0..4 {
            let a = k as f64 * 2.1 + random.range(0., 0.8);
            let reach = if k == 0 { 0. } else { 0.35 * size };
            let r = random.range(0.38, 0.5) * size;
            let at = foot + DVec3::new(a.cos() * reach, a.sin() * reach, r * 0.75);
            clump(e, at, DVec3::new(r, r, r * 0.85), &mut random, colour)?;
        }
        Ok(())
    })
}

/// A pot with a rolled lip and a shrub in it.
fn potted(e: &mut Editor, foot: DVec3, radius: f64, height: f64, colour: MaterialId, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let r = radius;
        lathe(
            e,
            foot,
            &[
                (0., 0.),
                (r * 0.78, 0.),
                (r * 0.8, 0.02),
                (r, height - 0.05),
                (r * 1.06, height - 0.04),
                (r * 1.06, height),
                (r * 0.94, height),
                (r * 0.94, height - 0.06),
                (0., height - 0.06),
            ],
            40,
            colour,
        )?;
        bush(e, foot + DVec3::Z * (height - 0.06), r * 1.5, p)
    })
}

/// Boards laid side by side across a rectangle, running along X or Y, with rounded edges.
fn boards(e: &mut Editor, min: DVec3, max: DVec3, along_x: bool, width: f64, colour: MaterialId) -> Result<()> {
    let span = if along_x { max.y - min.y } else { max.x - min.x };
    let count = (span / width).floor() as usize;
    for i in 0..count {
        let s = i as f64 * width;
        let (a, b) = if along_x {
            (DVec3::new(min.x, min.y + s, min.z), DVec3::new(max.x, min.y + s + width - 0.008, max.z))
        } else {
            (DVec3::new(min.x + s, min.y, min.z), DVec3::new(min.x + s + width - 0.008, max.y, max.z))
        };
        rounded(e, a, b, 0.006, colour)?;
    }
    Ok(())
}

/// A framed glass wall along X (at `y`) or Y (at `x`), from `a` to `b` in plan.
fn glazing(
    e: &mut Editor,
    a: DVec2,
    b: DVec2,
    height: std::ops::RangeInclusive<f64>,
    panes: usize,
    glass: MaterialId,
    p: &Paints,
) -> Result<()> {
    let (z0, z1) = (*height.start(), *height.end());
    let along = (b - a).normalize();
    let across = DVec2::new(-along.y, along.x) * 0.05;
    let at = |s: f64, z: f64, side: f64| (a + along * s + across * side).extend(z);
    let length = (b - a).length();
    let bar = 0.06;
    cuboid(e, at(0., z0 + bar, -0.2), at(length, z1 - bar, 0.2), glass)?;
    rounded(e, at(0., z0, -1.), at(length, z0 + bar, 1.), 0.008, p.dark)?;
    cuboid(e, at(0., z1 - bar, -1.), at(length, z1, 1.), p.dark)?;
    for i in 0..=panes {
        let s = (length - bar) * i as f64 / panes as f64;
        cuboid(e, at(s, z0 + bar, -1.), at(s + bar, z1 - bar, 1.), p.dark)?;
    }
    Ok(())
}

/// Recesses a framed window into the side of a solid `group` facing `out`. `from` and
/// `width` run along the side from its left end seen from outside; `sill` and `height`
/// are above its foot.
#[allow(clippy::too_many_arguments)]
fn window(
    e: &mut Editor,
    group: InstanceId,
    out: DVec3,
    from: f64,
    width: f64,
    sill: f64,
    height: f64,
    panes: usize,
    p: &Paints,
) -> Result<()> {
    // Groups keep their geometry relative to their own origin.
    let place = e.active().instances.get(&group).ok_or("no group")?.transform;
    e.enter(group)?;
    let mesh = e.mesh();
    let side = mesh
        .faces()
        .map(|(f, _)| f)
        .filter(|f| mesh.face_normal(*f).is_some_and(|n| n.dot(out) > 0.99))
        .max_by(|a, b| mesh.face_area(*a).total_cmp(&mesh.face_area(*b)))
        .ok_or("no side")?;
    let points: Vec<DVec3> = mesh.face(side).ok_or("no face")?.outer.iter().map(|v| mesh.pos(*v)).collect();
    let along = DVec3::Z.cross(out);
    let lowest = points.iter().map(|q| q.z).fold(f64::MAX, f64::min);
    let start = points.iter().map(|q| q.dot(along)).fold(f64::MAX, f64::min);
    let base = points[0] - along * (points[0].dot(along) - start) - DVec3::Z * (points[0].z - lowest);
    let corner = base + along * from + DVec3::Z * sill;
    e.apply(Command::DrawRectangle { corner, u: along * width, v: DVec3::Z * height, facing: Some(out) })?;
    let mesh = e.mesh();
    let pane = mesh
        .faces()
        .map(|(f, _)| f)
        .filter(|f| mesh.face_normal(*f).is_some_and(|n| n.dot(out) > 0.99))
        .find(|f| (mesh.face_area(*f) - width * height).abs() < 1e-6)
        .ok_or("no pane")?;
    e.apply(Command::PushPull { face: pane, distance: -0.15, keep_base: false })?;
    e.apply(Command::Paint { faces: vec![pane], material: Some(p.tinted), back: false })?;
    e.exit();
    let corner = place.transform_point3(corner);
    let inner = corner - out * 0.1;
    let a = inner.truncate();
    let b = (inner + along * width).truncate();
    let bar = 0.06;
    let at = |s: f64, z: f64, d: f64| (a + (b - a).normalize() * s).extend(z) + out * d;
    cuboid(e, at(0., corner.z, 0.), at(width, corner.z + bar, 0.08), p.dark)?;
    cuboid(e, at(0., corner.z + height - bar, 0.), at(width, corner.z + height, 0.08), p.dark)?;
    for i in 0..=panes {
        let s = (width - bar) * i as f64 / panes as f64;
        cuboid(e, at(s, corner.z + bar, 0.), at(s + bar, corner.z + height - bar, 0.08), p.dark)?;
    }
    // A pressed aluminium sill with a drip, proud of the wall face.
    rounded(e, at(-0.03, corner.z - 0.04, 0.08), at(width + 0.03, corner.z, 0.16), 0.006, p.dark)?;
    Ok(())
}

/// A wall along `a`-`b` in plan, `thickness` towards `inward`, with openings
/// `(from, width, sill, height, panes)` measured from `a`: framed glass, or left open for a
/// door when `panes` is zero. Raised windows get an outside sill.
#[allow(clippy::too_many_arguments)]
fn wall(
    e: &mut Editor,
    a: DVec2,
    b: DVec2,
    inward: DVec2,
    thickness: f64,
    z0: f64,
    z1: f64,
    openings: &[(f64, f64, f64, f64, usize)],
    colour: MaterialId,
    p: &Paints,
) -> Result<()> {
    let along = (b - a).normalize();
    let length = (b - a).length();
    let piece = |e: &mut Editor, s0: f64, s1: f64, h0: f64, h1: f64| -> Result<()> {
        if s1 - s0 > 0.01 && h1 - h0 > 0.01 {
            let c0 = (a + along * s0).extend(h0);
            let c1 = (a + along * s1 + inward * thickness).extend(h1);
            cuboid(e, c0, c1, colour)?;
        }
        Ok(())
    };
    let mut cursor = 0.;
    for &(from, width, sill, height, panes) in openings {
        piece(e, cursor, from, z0, z1)?;
        piece(e, from, from + width, z0, z0 + sill)?;
        piece(e, from, from + width, z0 + sill + height, z1)?;
        let middle = a + inward * thickness / 2.;
        if panes > 0 {
            glazing(
                e,
                middle + along * from,
                middle + along * (from + width),
                (z0 + sill)..=(z0 + sill + height),
                panes,
                p.clear,
                p,
            )?;
        }
        if sill > 0.3 {
            let s0 = (a + along * (from - 0.03) - inward * 0.06).extend(z0 + sill - 0.04);
            let s1 = (a + along * (from + width + 0.03) + inward * thickness * 0.4).extend(z0 + sill);
            rounded(e, s0, s1, 0.006, p.dark)?;
        }
        cursor = from + width;
    }
    piece(e, cursor, length, z0, z1)
}

/// A door in a wall opening from `a` to `b` (on the wall's centre line), facing `out`: a
/// frame, a leaf, a lever or pull handle on both faces and three hinges.
#[allow(clippy::too_many_arguments)]
fn door(
    e: &mut Editor,
    a: DVec2,
    b: DVec2,
    out: DVec2,
    z0: f64,
    height: f64,
    leaf: MaterialId,
    pull: bool,
    p: &Paints,
) -> Result<()> {
    grouped(e, |e| {
        let along = (b - a).normalize();
        let width = (b - a).length();
        let at = |s: f64, d: f64, z: f64| (a + along * s + out * d).extend(z);
        let f = 0.07;
        cuboid(e, at(0., -0.06, z0), at(f, 0.06, z0 + height), p.dark)?;
        cuboid(e, at(width - f, -0.06, z0), at(width, 0.06, z0 + height), p.dark)?;
        cuboid(e, at(0., -0.06, z0 + height - f), at(width, 0.06, z0 + height), p.dark)?;
        rounded(e, at(f, -0.06, z0), at(width - f, 0.06, z0 + 0.02), 0.004, p.steel)?;
        rounded(
            e,
            at(f + 0.004, -0.025, z0 + 0.02),
            at(width - f - 0.004, 0.025, z0 + height - f - 0.004),
            0.004,
            leaf,
        )?;
        for z in [0.25, height / 2. + 0.1, height - 0.3] {
            cuboid(e, at(f - 0.004, 0.025, z0 + z), at(f + 0.016, 0.03, z0 + z + 0.1), p.steel)?;
        }
        let s = width - f - 0.1;
        for side in [1., -1.] {
            let face = 0.025 * side;
            if pull {
                // A long stainless pull bar on stand-offs.
                let bar = |z: f64| at(s, face + 0.065 * side, z0 + z);
                limb(e, bar(0.75), bar(1.75), 0.016, 0.016, p.chrome)?;
                for z in [0.85, 1.65] {
                    limb(e, at(s, face, z0 + z), bar(z), 0.01, 0.01, p.chrome)?;
                }
            } else {
                let rose = at(s, face, z0 + 1.02);
                limb(e, rose, rose + (out * 0.012 * side).extend(0.), 0.028, 0.028, p.chrome)?;
                let knuckle = rose + (out * 0.055 * side).extend(0.);
                limb(e, rose, knuckle, 0.009, 0.009, p.chrome)?;
                limb(e, knuckle, knuckle - (along * 0.13).extend(0.), 0.01, 0.01, p.chrome)?;
            }
        }
        Ok(())
    })
}

/// A frameless glass balustrade from `a` to `b` standing on their height: a steel base shoe,
/// toughened glass and a round handrail on top.
fn balustrade(e: &mut Editor, a: DVec3, b: DVec3, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let along = (b - a).normalize();
        let across = DVec3::new(-along.y, along.x, 0.);
        rounded(e, a - across * 0.03, b + across * 0.03 + DVec3::Z * 0.1, 0.006, p.dark)?;
        cuboid(e, a - across * 0.008 + DVec3::Z * 0.1, b + across * 0.008 + DVec3::Z * 1.0, p.railing)?;
        limb(e, a + DVec3::Z * 1.02, b + DVec3::Z * 1.02, 0.022, 0.022, p.chrome)?;
        Ok(())
    })
}

/// A rectangular window on a plain wall face: a frame proud of the face, a glazing bar,
/// glass and a sill. `along` runs left to right seen from outside, `out` away from the wall.
#[allow(clippy::too_many_arguments)]
fn facade_window(
    e: &mut Editor,
    corner: DVec3,
    along: DVec3,
    out: DVec3,
    width: f64,
    height: f64,
    frame: MaterialId,
    p: &Paints,
) -> Result<()> {
    grouped(e, |e| {
        let at = |s: f64, z: f64, d: f64| corner + along * s + DVec3::Z * z + out * d;
        let f = 0.07;
        cuboid(e, at(0., f, -0.01), at(width, height - f, 0.02), p.tinted)?;
        cuboid(e, at(0., 0., 0.), at(width, f, 0.06), frame)?;
        cuboid(e, at(0., height - f, 0.), at(width, height, 0.06), frame)?;
        for s in [0., width / 2. - f / 2., width - f] {
            cuboid(e, at(s, f, 0.), at(s + f, height - f, 0.06), frame)?;
        }
        cuboid(e, at(f, height * 0.62, 0.), at(width - f, height * 0.62 + 0.05, 0.05), frame)?;
        rounded(e, at(-0.05, -0.05, 0.), at(width + 0.05, 0., 0.1), 0.01, p.coping)?;
        Ok(())
    })
}

/// An open bookcase from `min` to `max` in plan, open towards `front`: a carcass with a back,
/// sides and shelves, filled with books of varied sizes and colours.
fn bookcase(e: &mut Editor, min: DVec2, max: DVec2, front: DVec2, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let size = max - min;
        let (o, u, v, w, d) = if front.y < 0. {
            (min, DVec2::X, DVec2::Y, size.x, size.y)
        } else if front.y > 0. {
            (max, -DVec2::X, -DVec2::Y, size.x, size.y)
        } else if front.x < 0. {
            (DVec2::new(min.x, max.y), -DVec2::Y, DVec2::X, size.y, size.x)
        } else {
            (DVec2::new(max.x, min.y), DVec2::Y, -DVec2::X, size.y, size.x)
        };
        let q = |s: f64, t: f64, z: f64| (o + u * s + v * t).extend(z);
        let (h, t) = (2.2, 0.025);
        cuboid(e, q(0., d - 0.01, 0.), q(w, d, h), p.oak)?;
        for s in [0., w - t] {
            cuboid(e, q(s, 0., 0.), q(s + t, d - 0.01, h), p.oak)?;
        }
        let shelves = 6;
        let books = [p.accent, p.fabric, p.linen, p.cars[1], p.terracotta, p.coir, p.worktop];
        let mut random = Random((min.x * 100.) as i64 as u64 + 7);
        for k in 0..shelves {
            let z = k as f64 * (h - t) / (shelves - 1) as f64;
            cuboid(e, q(t, 0., z), q(w - t, d - 0.01, z + t), p.oak)?;
            if k + 1 == shelves {
                break;
            }
            let mut s = t + random.range(0.0, 0.15);
            while s < w - t - 0.06 {
                let width = random.range(0.025, 0.055);
                let height = random.range(0.2, 0.3);
                let depth = random.range(0.15, 0.22).min(d - 0.03);
                if random.next() < 0.85 {
                    let colour = books[(random.next() * books.len() as f64) as usize];
                    cuboid(e, q(s, d - 0.01 - depth, z + t), q(s + width, d - 0.01, z + t + height), colour)?;
                }
                s += width + 0.002;
                if random.next() < 0.06 {
                    s += random.range(0.1, 0.25);
                }
            }
        }
        Ok(())
    })
}

fn bed(e: &mut Editor, min: DVec2, max: DVec2, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        // An upholstered base on short legs, a mattress, a throw, pillows and a headboard.
        for (x, y) in [
            (min.x + 0.1, min.y + 0.1),
            (max.x - 0.1, min.y + 0.1),
            (min.x + 0.1, max.y - 0.2),
            (max.x - 0.1, max.y - 0.2),
        ] {
            column(e, DVec3::new(x, y, 0.), 0.025, 0.1, 16, p.dark)?;
        }
        rounded(e, min.extend(0.1), DVec2::new(max.x, max.y - 0.1).extend(0.32), 0.04, p.fabric)?;
        rounded(
            e,
            (min + DVec2::splat(0.04)).extend(0.32),
            DVec2::new(max.x - 0.04, max.y - 0.14).extend(0.54),
            0.08,
            p.linen,
        )?;
        rounded(
            e,
            DVec3::new(min.x + 0.02, min.y + 0.02, 0.54),
            DVec3::new(max.x - 0.02, min.y + 0.75, 0.57),
            0.03,
            p.accent,
        )?;
        for x in [min.x + 0.15, (min.x + max.x) / 2. + 0.05] {
            rounded(e, DVec3::new(x, max.y - 0.65, 0.54), DVec3::new(x + 0.75, max.y - 0.2, 0.68), 0.1, p.linen)?;
        }
        rounded(e, DVec3::new(min.x - 0.15, max.y - 0.1, 0.), DVec3::new(max.x + 0.15, max.y, 1.15), 0.04, p.fabric)?;
        Ok(())
    })
}

/// A dining chair: round steel legs, the back pair rising to carry the back rest, and a
/// timber seat and back.
fn chair(e: &mut Editor, at: DVec3, degrees: f64, wood: MaterialId, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let mut parts = Vec::new();
        for (dx, dy, h) in [(-0.19, -0.19, 0.43), (0.19, -0.19, 0.43), (-0.19, 0.19, 0.86), (0.19, 0.19, 0.86)] {
            parts.push(column(e, at + DVec3::new(dx, dy, 0.), 0.014, h, 16, p.dark)?);
        }
        parts.push(rounded(e, at + DVec3::new(-0.22, -0.22, 0.43), at + DVec3::new(0.22, 0.21, 0.46), 0.04, wood)?);
        parts.push(rounded(e, at + DVec3::new(-0.21, 0.205, 0.58), at + DVec3::new(0.21, 0.225, 0.84), 0.03, wood)?);
        turn(e, &parts, at, degrees)
    })
}

fn table(e: &mut Editor, min: DVec2, max: DVec2, height: f64, top: MaterialId, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        for (x, y) in [
            (min.x + 0.12, min.y + 0.12),
            (max.x - 0.12, min.y + 0.12),
            (min.x + 0.12, max.y - 0.12),
            (max.x - 0.12, max.y - 0.12),
        ] {
            column(e, DVec3::new(x, y, 0.), 0.03, height - 0.04, 24, p.dark)?;
        }
        rounded(e, min.extend(height - 0.04), max.extend(height), 0.03, top)?;
        Ok(())
    })
}

/// Makes everything `build` creates into one group, as one object.
fn grouped(e: &mut Editor, build: impl FnOnce(&mut Editor) -> Result<()>) -> Result<()> {
    let parts = built(e, build)?;
    if parts.len() > 1 {
        e.apply(Command::MakeGroup { elements: parts.into_iter().map(Element::Instance).collect() })?;
    }
    Ok(())
}

/// Moves and turns what `build` makes by a small random amount, as loose objects stand.
fn loose(
    e: &mut Editor,
    centre: DVec3,
    random: &mut Random,
    degrees: f64,
    offset: f64,
    build: impl FnOnce(&mut Editor) -> Result<()>,
) -> Result<()> {
    let parts = built(e, build)?;
    let d = DVec3::new(random.range(-offset, offset), random.range(-offset, offset), 0.);
    e.apply(Command::Move { elements: parts.iter().map(|i| Element::Instance(*i)).collect(), offset: d })?;
    turn(e, &parts, centre + d, random.range(-degrees, degrees))
}

/// A chair at a table, facing it from `at` and turned `degrees`: nudged and turned a little,
/// and now and then pulled out from the table.
fn seat(e: &mut Editor, at: DVec3, degrees: f64, wood: MaterialId, random: &mut Random, p: &Paints) -> Result<()> {
    let back = DVec3::new(-(degrees.to_radians()).sin(), degrees.to_radians().cos(), 0.);
    let pull = if random.next() < 0.35 { random.range(0.08, 0.2) } else { 0. };
    let nudge = DVec3::new(random.range(-0.04, 0.04), random.range(-0.04, 0.04), 0.);
    chair(e, at + back * pull + nudge, degrees + random.range(-7., 7.), wood, p)
}

/// A paving slab or stepping stone `w` by `d` at `c` turned `degrees`, let into the ground:
/// its foot below the lowest ground under it and its top just above the highest.
fn ground_slab(
    e: &mut Editor,
    c: DVec2,
    w: f64,
    d: f64,
    degrees: f64,
    ground: &dyn Fn(f64, f64) -> f64,
    colour: MaterialId,
) -> Result<()> {
    let (s, k) = degrees.to_radians().sin_cos();
    let heights: Vec<f64> = [(0., 0.), (-1., -1.), (1., -1.), (1., 1.), (-1., 1.)]
        .iter()
        .map(|&(a, b)| {
            let (x, y) = (a * w / 2., b * d / 2.);
            ground(c.x + x * k - y * s, c.y + x * s + y * k)
        })
        .collect();
    let low = heights.iter().copied().fold(f64::MAX, f64::min);
    let high = heights.iter().copied().fold(f64::MIN, f64::max);
    let at = c.extend(low - 0.03);
    let top = at + DVec3::new(w / 2., d / 2., high - low + 0.07);
    let slab = rounded(e, at - DVec3::new(w / 2., d / 2., 0.), top, 0.1, colour)?;
    turn(e, &[slab], at, degrees)
}

/// Lifts everything made by `build` by `dz`.
fn raised(e: &mut Editor, dz: f64, build: impl FnOnce(&mut Editor) -> Result<()>) -> Result<()> {
    let new: Vec<Element> = built(e, build)?.into_iter().map(Element::Instance).collect();
    e.apply(Command::Move { elements: new, offset: DVec3::Z * dz })?;
    Ok(())
}

fn lounger(e: &mut Editor, at: DVec3, degrees: f64, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let mut parts = Vec::new();
        // A slim teak frame on four legs, slats under a mattress and a raised back.
        for (x, y) in [(-0.31, -0.91), (0.31, -0.91), (-0.31, 0.87), (0.31, 0.87)] {
            parts.push(column(e, at + DVec3::new(x, y, 0.), 0.025, 0.24, 16, p.teak)?);
        }
        for x in [-0.35, 0.31] {
            parts.push(rounded(
                e,
                at + DVec3::new(x, -0.95, 0.2),
                at + DVec3::new(x + 0.04, 0.95, 0.28),
                0.01,
                p.teak,
            )?);
        }
        for i in 0..9 {
            let y = -0.9 + i as f64 * 0.15;
            parts.push(rounded(
                e,
                at + DVec3::new(-0.31, y, 0.24),
                at + DVec3::new(0.31, y + 0.1, 0.26),
                0.01,
                p.teak,
            )?);
        }
        parts.push(rounded(e, at + DVec3::new(-0.31, -0.92, 0.26), at + DVec3::new(0.31, 0.4, 0.34), 0.06, p.linen)?);
        let back = rounded(e, at + DVec3::new(-0.31, 0.4, 0.26), at + DVec3::new(0.31, 0.95, 0.34), 0.06, p.linen)?;
        e.apply(Command::Rotate {
            elements: vec![Element::Instance(back)],
            centre: at + DVec3::new(0., 0.4, 0.3),
            axis: DVec3::X,
            angle: 50.,
            copies: 0,
        })?;
        parts.push(back);
        // A prop under the raised back.
        parts.push(limb(e, at + DVec3::new(0., 0.4, 0.27), at + DVec3::new(0., 0.62, 0.52), 0.012, 0.012, p.steel)?);
        turn(e, &parts, at, degrees)
    })
}

/// A sofa with its back towards `back_side`: a plinth on short legs, seat and back cushions
/// and, if `arms`, an arm at each end.
#[allow(clippy::too_many_arguments)]
fn sofa(
    e: &mut Editor,
    min: DVec2,
    max: DVec2,
    back_side: DVec2,
    arms: bool,
    cover: MaterialId,
    cushions: MaterialId,
    p: &Paints,
) -> Result<()> {
    grouped(e, |e| {
        let centre = ((min + max) / 2.).extend(0.);
        let size = max - min;
        let (w, d) = if back_side.x != 0. { (size.y, size.x) } else { (size.x, size.y) };
        let degrees = if back_side.y > 0. {
            0.
        } else if back_side.y < 0. {
            180.
        } else if back_side.x > 0. {
            -90.
        } else {
            90.
        };
        let at = |x: f64, y: f64, z: f64| centre + DVec3::new(x, y, z);
        let (hw, hd) = (w / 2., d / 2.);
        let mut parts = Vec::new();
        for (x, y) in
            [(-hw + 0.08, -hd + 0.08), (hw - 0.08, -hd + 0.08), (-hw + 0.08, hd - 0.08), (hw - 0.08, hd - 0.08)]
        {
            parts.push(column(e, at(x, y, 0.), 0.02, 0.08, 16, p.dark)?);
        }
        parts.push(rounded(e, at(-hw, -hd, 0.08), at(hw, hd, 0.28), 0.03, cover)?);
        parts.push(rounded(e, at(-hw, hd - 0.18, 0.28), at(hw, hd, 0.72), 0.05, cover)?);
        let arm = if arms { 0.16 } else { 0. };
        if arms {
            for x in [-hw, hw - arm] {
                parts.push(rounded(e, at(x, -hd, 0.28), at(x + arm, hd - 0.18, 0.6), 0.05, cover)?);
            }
        }
        let inner = w - 2. * arm;
        let count = (inner / 0.8).round().max(1.) as usize;
        let each = inner / count as f64;
        for k in 0..count {
            let x = -hw + arm + k as f64 * each;
            parts.push(rounded(
                e,
                at(x + 0.005, -hd + 0.02, 0.28),
                at(x + each - 0.005, hd - 0.36, 0.44),
                0.06,
                cover,
            )?);
            parts.push(rounded(e, at(x + 0.005, hd - 0.36, 0.28), at(x + each - 0.005, hd - 0.18, 0.8), 0.07, cover)?);
        }
        // Two scatter cushions in a contrasting colour.
        for x in [-hw + arm + 0.1, hw - arm - 0.55] {
            let c = rounded(e, at(x, hd - 0.42, 0.44), at(x + 0.45, hd - 0.3, 0.84), 0.06, cushions)?;
            e.apply(Command::Rotate {
                elements: vec![Element::Instance(c)],
                centre: at(x, hd - 0.36, 0.44),
                axis: DVec3::X,
                angle: 12.,
                copies: 0,
            })?;
            // Dropped casually rather than squared up.
            e.apply(Command::Rotate {
                elements: vec![Element::Instance(c)],
                centre: at(x + 0.22, hd - 0.36, 0.44),
                axis: DVec3::Z,
                angle: ((centre.x * 7.3 + x * 3.1).sin() * 9.).round(),
                copies: 0,
            })?;
            parts.push(c);
        }
        turn(e, &parts, centre, degrees)
    })
}

/// A horizontal log along X or Y, its foot at `at`.
fn log(e: &mut Editor, at: DVec3, radius: f64, length: f64, along_x: bool, colour: MaterialId) -> Result<()> {
    let dir = if along_x { DVec3::X } else { DVec3::Y };
    let c = at + DVec3::Z * radius;
    limb(e, c, c + dir * length, radius, radius * 0.92, colour)?;
    Ok(())
}

/// A ring of stones around glowing embers and a few logs.
fn fire_pit(e: &mut Editor, at: DVec3, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let mut random = Random(17);
        for i in 0..12 {
            let a = i as f64 * std::f64::consts::TAU / 12.;
            let r = random.range(0.13, 0.17);
            clump(
                e,
                at + DVec3::new(a.cos() * 0.62, a.sin() * 0.62, r * 0.6),
                DVec3::new(r, r * 1.2, r * 0.75),
                &mut random,
                p.plinth,
            )?;
        }
        column(e, at, 0.5, 0.04, 32, p.dark)?;
        column(e, at + DVec3::Z * 0.04, 0.32, 0.03, 24, p.embers)?;
        log(e, at + DVec3::new(-0.3, -0.12, 0.07), 0.05, 0.6, true, p.bark)?;
        log(e, at + DVec3::new(0.1, -0.3, 0.07), 0.05, 0.55, false, p.bark)?;
        Ok(())
    })
}

/// A garden bench, back on +Y so it faces -Y before turning: cast side frames, a seat of
/// four slats and two back slats.
fn bench(e: &mut Editor, at: DVec3, degrees: f64, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let mut parts = Vec::new();
        for x in [-0.7, 0.62] {
            parts.push(rounded(
                e,
                at + DVec3::new(x, -0.22, 0.),
                at + DVec3::new(x + 0.08, -0.16, 0.4),
                0.015,
                p.dark,
            )?);
            // The rear leg continues up behind the seat and carries the back slats.
            parts.push(rounded(e, at + DVec3::new(x, 0.2, 0.), at + DVec3::new(x + 0.08, 0.26, 0.86), 0.015, p.dark)?);
            parts.push(rounded(
                e,
                at + DVec3::new(x, -0.22, 0.34),
                at + DVec3::new(x + 0.08, 0.26, 0.4),
                0.01,
                p.dark,
            )?);
        }
        for k in 0..4 {
            let y = -0.24 + k as f64 * 0.115;
            parts.push(rounded(
                e,
                at + DVec3::new(-0.82, y, 0.4),
                at + DVec3::new(0.82, y + 0.1, 0.44),
                0.015,
                p.teak,
            )?);
        }
        for z in [0.52, 0.7] {
            parts.push(rounded(
                e,
                at + DVec3::new(-0.82, 0.17, z),
                at + DVec3::new(0.82, 0.2, z + 0.12),
                0.012,
                p.teak,
            )?);
        }
        turn(e, &parts, at, degrees)
    })
}

/// A swept heap of leaves with a rake lying beside it.
fn leaf_pile(e: &mut Editor, at: DVec3, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let mut random = Random(5);
        clump(e, at + DVec3::Z * 0.05, DVec3::new(0.75, 0.6, 0.32), &mut random, p.autumn[0])?;
        clump(e, at + DVec3::new(0.5, 0.3, 0.04), DVec3::new(0.45, 0.4, 0.22), &mut random, p.autumn[2])?;
        limb(e, at + DVec3::new(0.95, -0.95, 0.015), at + DVec3::new(0.95, 0.6, 0.015), 0.014, 0.014, p.oak)?;
        rounded(e, at + DVec3::new(0.75, -1.0, 0.), at + DVec3::new(1.15, -0.94, 0.04), 0.01, p.steel)?;
        Ok(())
    })
}

/// Split logs stacked against a wall between two steel end frames, each row bedded in the
/// hollows of the one below.
fn woodpile(e: &mut Editor, min: DVec2, length: f64, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let r = 0.09;
        for row in 0..6 {
            let shift = if row % 2 == 0 { 0. } else { r + 0.01 };
            let mut y = min.y + 0.05 + r + shift;
            while y + r < min.y + length - 0.05 {
                log(e, DVec3::new(min.x, y, row as f64 * 0.15), r, 0.5, true, p.firewood)?;
                y += 2. * r + 0.02;
            }
        }
        for y in [min.y, min.y + length - 0.05] {
            cuboid(e, DVec3::new(min.x, y, 0.), DVec3::new(min.x + 0.5, y + 0.05, 1.0), p.dark)?;
        }
        Ok(())
    })
}

/// A wall-mounted letterbox with a slot and a flap.
fn letterbox(e: &mut Editor, at: DVec3, out: DVec3, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let along = DVec3::Z.cross(out);
        let q = |s: f64, z: f64, d: f64| at + along * s + DVec3::Z * z + out * d;
        rounded(e, q(-0.18, 0., 0.), q(0.18, 0.4, 0.13), 0.02, p.dark)?;
        cuboid(e, q(-0.12, 0.3, 0.13), q(0.12, 0.33, 0.135), p.steel)?;
        Ok(())
    })
}

/// A wheelie bin with a hinged lid, two wheels and a handle, front towards -Y.
fn bin(e: &mut Editor, at: DVec3, degrees: f64, colour: MaterialId, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let mut parts = built(e, |e| {
            rounded(e, at + DVec3::new(-0.27, -0.3, 0.05), at + DVec3::new(0.27, 0.3, 1.0), 0.04, colour)?;
            rounded(e, at + DVec3::new(-0.29, -0.34, 1.0), at + DVec3::new(0.29, 0.36, 1.05), 0.03, colour)?;
            limb(e, at + DVec3::new(-0.24, 0.36, 1.0), at + DVec3::new(0.24, 0.36, 1.0), 0.015, 0.015, p.dark)?;
            for x in [-0.3, 0.24] {
                let hub = at + DVec3::new(x, 0.26, 0.1);
                limb(e, hub, hub + DVec3::X * 0.06, 0.1, 0.1, p.dark)?;
            }
            Ok(())
        })?;
        parts.sort();
        turn(e, &parts, at, degrees)
    })
}

/// A street lamp: a tapered steel column on a base and a slim luminaire on an arm.
fn street_lamp(e: &mut Editor, foot: DVec3, out: DVec2, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        lathe(e, foot, &[(0., 0.), (0.13, 0.), (0.13, 0.5), (0.08, 0.55), (0.055, 6.0), (0., 6.0)], 24, p.dark)?;
        let head = foot + DVec3::Z * 6.0 + (out * 1.1).extend(0.12);
        limb(e, foot + DVec3::Z * 5.9, head, 0.035, 0.03, p.dark)?;
        let lamp = head + (out * 0.25).extend(-0.05);
        let a = DVec3::new(lamp.x - 0.35, lamp.y - 0.15, lamp.z - 0.05);
        let b = DVec3::new(lamp.x + 0.35, lamp.y + 0.15, lamp.z + 0.06);
        rounded(e, a, b, 0.06, p.dark)?;
        cuboid(e, a + DVec3::new(0.05, 0.03, -0.01), DVec3::new(b.x - 0.05, b.y - 0.03, a.z), p.lamp)?;
        Ok(())
    })
}

/// A plant on upright cut-out quads radiating from its foot, each showing the plant image
/// upright and scaled to `height`, turned `angle` radians.
fn plant(
    e: &mut Editor,
    foot: DVec3,
    height: f64,
    radius: f64,
    wings: usize,
    angle: f64,
    kind: (MaterialId, [f64; 2]),
) -> Result<()> {
    let (material, size) = kind;
    let base = foot - DVec3::Z * 0.02;
    let mut lines = Vec::new();
    for k in 0..wings {
        // Never square to the model axes, so the image's columns always run upright.
        let a = angle + 0.21 + k as f64 * std::f64::consts::TAU / wings as f64;
        let d = DVec3::new(a.cos(), a.sin(), 0.) * radius;
        let quad = [base, base + d, base + d + DVec3::Z * height, base + DVec3::Z * height];
        lines.extend((0..4).map(|i| [quad[i], quad[(i + 1) % 4]]));
    }
    e.apply(Command::DrawLines { lines, facing: None })?;
    let group = group_loose(e)?;
    e.enter(group)?;
    let mesh = e.mesh();
    let faces: Vec<_> = mesh.faces().map(|(f, _)| f).collect();
    // The wings radiate evenly, so the mean of all corners lies on the stem.
    let corners: Vec<DVec3> = mesh.vertices().map(|(_, v)| v.pos).collect();
    let stem = corners.iter().copied().sum::<DVec3>() / corners.len().max(1) as f64;
    let placements: Vec<_> = faces
        .iter()
        .map(|f| {
            let n = mesh.face_normal(*f).unwrap_or(DVec3::X);
            let (u, v) = rysmo_model::texture_axes(n);
            let points: Vec<DVec3> =
                mesh.face(*f).map(|face| face.outer.iter().map(|q| mesh.pos(*q)).collect()).unwrap_or_default();
            let low = |axis: DVec3| points.iter().map(|q| axis.dot(*q)).fold(f64::MAX, f64::min);
            // The image's middle row on the stem, its foot at the bottom of the wing.
            let placement = rysmo_model::TexturePlacement {
                offset: [low(u), v.dot(stem) - radius],
                scale: [height / size[0], radius * 2. / size[1]],
                ..Default::default()
            };
            (*f, placement)
        })
        .collect();
    e.apply(Command::Paint { faces: faces.clone(), material: Some(material), back: false })?;
    e.apply(Command::Paint { faces, material: Some(material), back: true })?;
    for (face, placement) in placements {
        e.apply(Command::SetTexturePlacement { faces: vec![face], placement: Some(placement) })?;
    }
    e.exit();
    Ok(())
}

/// A lawn tuft, a clump of ornamental grass (`kind` 1) or a flowering perennial (`kind` 2)
/// on the ground at `x`, `y`, sized and turned at random.
fn planting(
    e: &mut Editor,
    x: f64,
    y: f64,
    kind: usize,
    ground: &dyn Fn(f64, f64) -> f64,
    random: &mut Random,
    p: &Paints,
) -> Result<()> {
    let foot = DVec3::new(x, y, ground(x, y));
    let turn = random.range(0., std::f64::consts::TAU);
    match kind {
        0 => plant(e, foot, random.range(0.2, 0.32), random.range(0.16, 0.24), 4, turn, p.plants[0]),
        1 => plant(e, foot, random.range(0.7, 1.0), random.range(0.32, 0.42), 6, turn, p.plants[1]),
        _ => plant(e, foot, random.range(0.38, 0.52), random.range(0.26, 0.34), 5, turn, p.plants[2]),
    }
}

/// A low-voltage garden bollard with a glowing band under its cap.
fn bollard(e: &mut Editor, foot: DVec3, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        lathe(e, foot, &[(0., 0.), (0.07, 0.), (0.07, 0.55), (0., 0.55)], 32, p.dark)?;
        lathe(e, foot + DVec3::Z * 0.55, &[(0., 0.), (0.065, 0.), (0.065, 0.06), (0., 0.06)], 32, p.lamp)?;
        lathe(
            e,
            foot + DVec3::Z * 0.61,
            &[(0., 0.), (0.085, 0.), (0.085, 0.02), (0.06, 0.04), (0., 0.045)],
            32,
            p.dark,
        )?;
        Ok(())
    })
}

/// A parasol: a granite base, a timber pole, ribs and struts, and a sagging canvas canopy in
/// panels of two colours with a valance.
fn parasol(e: &mut Editor, at: DVec3, colours: [MaterialId; 2], p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let (edge, apex, radius) = (2.05, 2.55, 1.5);
        lathe(e, at, &[(0., 0.), (0.28, 0.), (0.28, 0.05), (0.25, 0.08), (0.05, 0.1), (0., 0.1)], 40, p.stone)?;
        column(e, at, 0.022, apex + 0.1, 24, p.oak)?;
        lathe(e, at + DVec3::Z * (apex + 0.1), &[(0., 0.), (0.035, 0.), (0.04, 0.04), (0., 0.09)], 16, p.oak)?;
        column(e, at + DVec3::Z * 1.5, 0.034, 0.12, 24, p.steel)?;
        // Eight panels sag between the ribs; the canopy has a thin, real thickness.
        let canopy = lathe_with(
            e,
            at,
            &[(0.0, apex - 0.012), (radius - 0.02, edge - 0.012), (radius, edge), (0.0, apex)],
            64,
            |a| 1. - 0.035 * (4. * a).sin().abs(),
            colours[0],
        )?;
        let place = e.active().instances.get(&canopy).ok_or("no canopy")?.transform;
        e.enter(canopy)?;
        let mesh = e.mesh();
        let stripes: Vec<_> = mesh
            .faces()
            .filter(|(_, f)| {
                let c = f.outer.iter().map(|v| mesh.pos(*v)).sum::<DVec3>() / f.outer.len() as f64;
                let c = place.transform_point3(c) - at;
                let sector = (c.y.atan2(c.x).rem_euclid(std::f64::consts::TAU) / (std::f64::consts::TAU / 8.)) as usize;
                sector % 2 == 1
            })
            .map(|(id, _)| id)
            .collect();
        e.apply(Command::Paint { faces: stripes.clone(), material: Some(colours[1]), back: false })?;
        e.apply(Command::Paint { faces: stripes, material: Some(colours[1]), back: true })?;
        e.exit();
        lathe(
            e,
            at,
            &[
                (radius - 0.012, edge - 0.15),
                (radius, edge - 0.15),
                (radius, edge),
                (radius - 0.012, edge),
                (radius - 0.012, edge - 0.15),
            ],
            64,
            colours[1],
        )?;
        for k in 0..8 {
            let a = (k as f64 + 0.5) * std::f64::consts::TAU / 8.;
            let dir = DVec3::new(a.cos(), a.sin(), 0.);
            let under = |r: f64| at + dir * r + DVec3::Z * (apex - 0.02 - (apex - edge) * r / radius);
            limb(e, under(0.04), under(radius - 0.03), 0.008, 0.007, p.steel)?;
            limb(e, at + DVec3::Z * 1.56 + dir * 0.03, under(0.75), 0.006, 0.006, p.steel)?;
        }
        Ok(())
    })
}

/// A contemporary neighbouring house facing +Y on a `w` by `d` footprint, in one of four
/// styles: rendered ground floors under a dark metal, larch or rendered upper floor, or a
/// single storey with a larch panel; flat roofs with copings, large dark-framed windows,
/// a front door under a canopy, downpipes, and a car port at the +X side with an optional
/// car.
#[allow(clippy::too_many_arguments)]
fn neighbour(
    e: &mut Editor,
    base: DVec3,
    w: f64,
    d: f64,
    style: usize,
    door_colour: MaterialId,
    parked: Option<MaterialId>,
    p: &Paints,
) -> Result<()> {
    grouped(e, |e| {
        let at = |x: f64, y: f64, z: f64| base + DVec3::new(x, y, z);
        let (ground, upper) = (3.1, 6.0);
        let two = style != 2;
        let lower = if style == 3 { p.concrete } else { p.render };
        rounded(e, at(-0.05, -0.05, -0.3), at(w + 0.05, d + 0.05, 0.15), 0.01, p.plinth)?;
        cuboid(e, at(0., 0., 0.15), at(w, d, ground), lower)?;
        // The upper floor: its extent and finish vary by style.
        let (ux, uy, finish) = match style {
            0 => ((0., w * 0.85), (0., d + 1.2), p.dark),
            1 => ((w * 0.2, w), (0., d - 1.0), p.wood),
            _ => ((0., w), (-0.8, d), p.render),
        };
        let top = if two { upper } else { ground };
        if two {
            cuboid(e, at(ux.0, uy.0, ground), at(ux.1, uy.1, upper), finish)?;
            rounded(e, at(ux.0 - 0.04, uy.0 - 0.04, upper), at(ux.1 + 0.04, uy.1 + 0.04, upper + 0.08), 0.01, p.dark)?;
        }
        rounded(e, at(-0.04, -0.04, ground), at(w + 0.04, d + 0.04, ground + 0.08), 0.01, p.dark)?;
        if style == 2 {
            cuboid(e, at(0., d, 0.15), at(w * 0.4, d + 0.03, ground), p.wood)?;
        }
        // Front: a door under a canopy, a tall slim window and a wide one; the upper floor
        // has a band window.
        let front = d;
        let door = at(1.2, front, 0.15);
        cuboid(e, door, door + DVec3::new(1.1, 0.05, 2.25), p.dark)?;
        rounded(e, door + DVec3::new(0.05, 0.02, 0.02), door + DVec3::new(1.05, 0.07, 2.2), 0.008, door_colour)?;
        limb(e, door + DVec3::new(0.9, 0.12, 0.8), door + DVec3::new(0.9, 0.12, 1.6), 0.014, 0.014, p.chrome)?;
        if style != 0 {
            rounded(e, door + DVec3::new(-0.4, 0., 2.55), door + DVec3::new(1.5, 1.2, 2.68), 0.01, p.dark)?;
        }
        cuboid(e, door + DVec3::new(-0.3, 0.0, 1.9), door + DVec3::new(-0.18, 0.08, 2.1), p.lamp)?;
        rounded(e, at(0.9, front, 0.), at(2.6, front + 1.0, 0.15), 0.02, p.coping)?;
        facade_window(e, at(2.7, front, 0.15), DVec3::X, DVec3::Y, 0.6, 2.2, p.dark, p)?;
        let wide = (w - 1.0 - 4.0).min(3.2);
        if wide > 1. {
            facade_window(e, at(4.0, front, 1.0), DVec3::X, DVec3::Y, wide, 1.4, p.dark, p)?;
        }
        if two {
            let band = ((ux.1 - ux.0) - 2.0).min(4.2);
            facade_window(e, at(ux.0 + 1.0, uy.1, ground + 0.9), DVec3::X, DVec3::Y, band, 1.4, p.dark, p)?;
            // Back windows upstairs.
            let mut x = ux.1 - 1.0;
            while x - 1.8 > ux.0 + 0.8 {
                facade_window(e, at(x, uy.0, ground + 0.9), -DVec3::X, -DVec3::Y, 1.8, 1.4, p.dark, p)?;
                x -= 2.8;
            }
        }
        // Garden side: tall sliding doors along most of the ground floor.
        let mut x = w - 1.0;
        while x - 2.4 > 0.9 {
            facade_window(e, at(x, 0., 0.15), -DVec3::X, -DVec3::Y, 2.4, 2.4, p.dark, p)?;
            x -= 2.6;
        }
        facade_window(e, at(0., d - 2.0, 1.0), -DVec3::Y, -DVec3::X, 1.2, 1.4, p.dark, p)?;
        for x in [0.15, w - 0.15] {
            rail(e, &[at(x, -0.07, top + 0.05), at(x, -0.07, 0.)], 0.045, p.dark)?;
        }
        // The car port beside the house, roofed from the side wall to two posts.
        for y in [d - 5.6, d - 0.25] {
            rounded(e, at(w + 3.15, y, 0.), at(w + 3.27, y + 0.12, 2.6), 0.01, p.dark)?;
        }
        rounded(e, at(w, d - 5.8, 2.6), at(w + 3.4, d + 0.2, 2.78), 0.01, p.dark)?;
        rounded(e, at(w, d - 5.8, 0.), at(w + 3.4, d + 0.2, 0.04), 0.01, p.paving)?;
        if let Some(colour) = parked {
            car(e, at(w + 1.7, d - 2.9, 0.04), 0., colour, p)?;
        }
        Ok(())
    })
}

/// A small hatchback, nose towards +Y: a body lofted through cross-sections along its length
/// with a glasshouse, wheels with hubs, and lights.
fn car(e: &mut Editor, at: DVec3, degrees: f64, paint_colour: MaterialId, p: &Paints) -> Result<()> {
    grouped(e, |e| {
        let parts = built(e, |e| {
            // Height of the body's top along its length, from the tail to the nose.
            let profile = [
                (-2.05, 0.95),
                (-1.95, 1.05),
                (-1.75, 1.38),
                (-1.2, 1.46),
                (-0.3, 1.47),
                (0.35, 1.42),
                (0.95, 0.98),
                (1.6, 0.9),
                (2.0, 0.78),
                (2.1, 0.62),
            ];
            let ring = |y: f64, top: f64| -> Vec<DVec3> {
                let belt = (top - 0.06).min(0.9);
                let roof = 0.86 - (top - belt) * 0.32;
                [
                    (-0.82, 0.3),
                    (0.82, 0.3),
                    (0.88, 0.42),
                    (0.89, belt),
                    (roof, top),
                    (-roof, top),
                    (-0.89, belt),
                    (-0.88, 0.42),
                ]
                .iter()
                .map(|&(x, z)| at + DVec3::new(x, y, z))
                .collect()
            };
            let mut rings = vec![vec![at + DVec3::new(0., profile[0].0 - 0.02, 0.62); 8]];
            rings.extend(profile.iter().map(|&(y, top)| ring(y, top)));
            rings.push(vec![at + DVec3::new(0., 2.12, 0.5); 8]);
            let body = loft(e, &rings, 30., paint_colour)?;
            // The glasshouse: steep faces above the waist.
            let place = e.active().instances.get(&body).ok_or("no body")?.transform;
            e.enter(body)?;
            let mesh = e.mesh();
            let glass: Vec<_> = mesh
                .faces()
                .filter(|(id, f)| {
                    let c = place
                        .transform_point3(f.outer.iter().map(|v| mesh.pos(*v)).sum::<DVec3>() / f.outer.len() as f64);
                    let n = mesh.face_normal(*id).unwrap_or(DVec3::Z);
                    c.z - at.z > 0.98 && n.z < 0.85 && (c.y - at.y) > -1.9
                })
                .map(|(id, _)| id)
                .collect();
            e.apply(Command::Paint { faces: glass, material: Some(p.tinted), back: false })?;
            e.exit();
            for (x, y) in [(-0.8, -1.3), (0.8, -1.3), (-0.8, 1.32), (0.8, 1.32)] {
                let hub = at + DVec3::new(x, y, 0.31);
                let side = DVec3::X * x.signum();
                limb(e, hub - side * 0.08, hub + side * 0.1, 0.31, 0.31, p.rubber)?;
                limb(e, hub + side * 0.1, hub + side * 0.11, 0.2, 0.19, p.steel)?;
            }
            for x in [-0.72, 0.5] {
                rounded(e, at + DVec3::new(x, 2.0, 0.66), at + DVec3::new(x + 0.22, 2.09, 0.74), 0.02, p.lamp)?;
                rounded(e, at + DVec3::new(x, -2.08, 0.82), at + DVec3::new(x + 0.22, -1.97, 0.95), 0.02, p.taillight)?;
            }
            Ok(())
        })?;
        turn(e, &parts, at, degrees)
    })
}

/// A road along the Catmull-Rom curve through `route`, `width` wide, laid on the ground
/// with flush concrete edge strips; neighbouring pieces share their corners.
fn road(e: &mut Editor, route: &[DVec2], width: f64, ground: &dyn Fn(f64, f64) -> f64, p: &Paints) -> Result<()> {
    let n = route.len() - 1;
    let at = |t: f64| -> DVec2 {
        let k = ((t * n as f64).floor() as usize).min(n - 1);
        let u = t * n as f64 - k as f64;
        let (p0, p1, p2, p3) = (route[k.saturating_sub(1)], route[k], route[k + 1], route[(k + 2).min(n)]);
        0.5 * ((2. * p1)
            + (-p0 + p2) * u
            + (2. * p0 - 5. * p1 + 4. * p2 - p3) * u * u
            + (-p0 + 3. * p1 - 3. * p2 + p3) * u * u * u)
    };
    let length: f64 = route.windows(2).map(|w| w[0].distance(w[1])).sum();
    let steps = (length / 2.).ceil() as usize;
    let centre: Vec<DVec2> = (0..=steps).map(|i| at(i as f64 / steps as f64)).collect();
    let side = |i: usize| {
        let d = (centre[(i + 1).min(steps)] - centre[i.saturating_sub(1)]).normalize();
        DVec2::new(-d.y, d.x)
    };
    let half = width / 2.;
    // Level across, at each station just above the highest ground across it and its
    // neighbours, so the terrain never shows through; the sides reach down into the ground.
    let raw: Vec<f64> = (0..=steps)
        .map(|i| {
            [-1., -0.5, 0., 0.5, 1.]
                .iter()
                .map(|t| {
                    let q = centre[i] + side(i) * t * (half + 0.25);
                    ground(q.x, q.y)
                })
                .fold(f64::MIN, f64::max)
        })
        .collect();
    let level: Vec<f64> =
        (0..=steps).map(|i| raw[i.saturating_sub(1)].max(raw[i]).max(raw[(i + 1).min(steps)]) + 0.04).collect();
    let lift = |q: DVec2, i: usize, z: f64| q.extend(level[i] + z);
    for i in 0..steps {
        let (a, b, sa, sb) = (centre[i], centre[i + 1], side(i), side(i + 1));
        let quad = [
            lift(a - sa * half, i, 0.),
            lift(b - sb * half, i + 1, 0.),
            lift(b + sb * half, i + 1, 0.),
            lift(a + sa * half, i, 0.),
        ];
        prism(e, quad, -DVec3::Z * 0.6, p.asphalt)?;
        for s in [-1., 1.] {
            let (o, w) = (half, 0.25);
            let strip = [
                lift(a + sa * s * o, i, 0.005),
                lift(b + sb * s * o, i + 1, 0.005),
                lift(b + sb * s * (o + w), i + 1, 0.005),
                lift(a + sa * s * (o + w), i, 0.005),
            ];
            prism(e, strip, -DVec3::Z * 0.6, p.concrete)?;
        }
    }
    Ok(())
}

/// Builds with `build` in a fresh editor holding only the materials, then pastes the result
/// in one step. Every command copies the whole document for undo, so building in small
/// editors keeps a large scene quick to generate.
fn batch(e: &mut Editor, base: &rysmo_model::Document, build: impl FnOnce(&mut Editor) -> Result<()>) -> Result<()> {
    let mut scratch = Editor::with_document(base.clone());
    build(&mut scratch)?;
    let clipboard = rysmo_model::clipboard::Clipboard {
        document: scratch.doc().clone(),
        source_transform: rysmo_math::DMat4::IDENTITY,
    };
    e.apply(Command::Paste {
        clipboard: Box::new(clipboard),
        transform: rysmo_math::DMat4::IDENTITY,
        reuse_definitions: false,
    })?;
    Ok(())
}

/// The sky panorama: "Autumn Field (Pure Sky)" from Poly Haven, CC0; see `assets/sky/LICENSE.txt`.
const SKY: &[u8] = include_bytes!("assets/sky/autumn_field_puresky_2k.hdr");

/// The bearing of a panorama's sun (its brightest point in the upper half) about the
/// vertical, as the viewport maps it before any rotation, and its elevation, in degrees.
fn panorama_sun(bytes: &[u8]) -> Result<(f64, f64)> {
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Hdr)?.to_rgb32f();
    let (w, h) = image.dimensions();
    let mut best = (0, 0, f32::MIN);
    for y in 0..h / 2 {
        for x in 0..w {
            let c = image.get_pixel(x, y);
            let luminance = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
            if luminance > best.2 {
                best = (x, y, luminance);
            }
        }
    }
    let theta = (best.1 as f64 + 0.5) / h as f64 * std::f64::consts::PI;
    let phi = ((best.0 as f64 + 0.5) / w as f64 - 0.5) * std::f64::consts::TAU;
    Ok((phi.to_degrees(), 90. - theta.to_degrees()))
}

/// The time of day; `DEMO_HOUR` overrides it.
const HOUR: f64 = 16.4;

/// Finished ground floor, ground floor ceiling, upper floor and upper ceiling: 3.1 m floor
/// to floor and 2.8 m clear rooms.
const FLOOR: f64 = 0.25;
const CEILING: f64 = 3.05;
const LEVEL: f64 = 3.35;
const TOP: f64 = 6.15;
/// Top of the main roof slab.
const ROOF: f64 = 6.4;

fn main() -> Result<()> {
    let path = std::env::args().nth(1).ok_or("Usage: demo_house output.rysmo")?;
    let mut e = Editor::new();
    let grass = [textures::grass(&mut e, 0)?, textures::grass(&mut e, 1)?, textures::grass(&mut e, 2)?];
    // Colours and foliage tints are sRGB albedos of real materials.
    let p = Paints {
        render: textures::render(&mut e)?,
        concrete: textures::concrete(&mut e)?,
        wood: textures::larch(&mut e)?,
        deck: textures::decking(&mut e)?,
        clear: surface(&mut e, "Clear glass", [0.6, 0.72, 0.78, 0.2], 0.03, 0.0)?,
        tinted: surface(&mut e, "Tinted glass", [0.14, 0.19, 0.23, 0.88], 0.05, 0.0)?,
        railing: surface(&mut e, "Railing glass", [0.62, 0.76, 0.8, 0.25], 0.03, 0.0)?,
        dark: surface(&mut e, "Anthracite", [0.1, 0.105, 0.115, 1.], 0.5, 0.3)?,
        steel: surface(&mut e, "Galvanised steel", [0.42, 0.43, 0.44, 1.], 0.45, 0.85)?,
        stone: textures::stone(&mut e)?,
        water: surface(&mut e, "Water", [0.16, 0.42, 0.52, 0.72], 0.02, 0.0)?,
        paving: textures::paving(&mut e)?,
        asphalt: textures::asphalt(&mut e)?,
        marking: surface(&mut e, "Yellow road marking", [0.8, 0.6, 0.08, 1.], 0.6, 0.)?,
        embers: glowing(&mut e, "Embers", [1., 0.42, 0.12, 1.], 20000.)?,
        firewood: surface(&mut e, "Split firewood", [0.46, 0.32, 0.2, 1.], 0.9, 0.)?,
        coping: surface(&mut e, "Granite", [0.55, 0.54, 0.51, 1.], 0.7, 0.0)?,
        tile: surface(&mut e, "Waterline tile", [0.06, 0.2, 0.32, 1.], 0.12, 0.0)?,
        chrome: surface(&mut e, "Stainless steel", [0.72, 0.73, 0.74, 1.], 0.18, 1.0)?,
        taillight: surface(&mut e, "Red lens", [0.5, 0.03, 0.02, 1.], 0.2, 0.)?,
        bark: textures::bark(&mut e)?,
        autumn: [
            textures::foliage(&mut e, "Orange leaves", [0.93, 0.58, 0.22, 1.])?,
            textures::foliage(&mut e, "Red leaves", [0.8, 0.29, 0.19, 1.])?,
            textures::foliage(&mut e, "Golden leaves", [0.93, 0.77, 0.25, 1.])?,
            textures::foliage(&mut e, "Rust leaves", [0.7, 0.44, 0.22, 1.])?,
        ],
        pumpkin: surface(&mut e, "Pumpkin", [0.8, 0.36, 0.05, 1.], 0.55, 0.)?,
        conifer: textures::foliage(&mut e, "Spruce", [0.27, 0.4, 0.29, 1.])?,
        hedge: textures::foliage(&mut e, "Hedge", [0.38, 0.45, 0.25, 1.])?,
        fabric: surface(&mut e, "Charcoal fabric", [0.2, 0.21, 0.22, 1.], 0.95, 0.)?,
        linen: surface(&mut e, "Linen", [0.66, 0.62, 0.55, 1.], 0.95, 0.)?,
        accent: surface(&mut e, "Terracotta fabric", [0.55, 0.22, 0.12, 1.], 0.95, 0.)?,
        outdoor: surface(&mut e, "Olive outdoor fabric", [0.3, 0.32, 0.24, 1.], 0.9, 0.)?,
        oak: surface(&mut e, "Oak", [0.5, 0.36, 0.22, 1.], 0.55, 0.0)?,
        teak: surface(&mut e, "Weathered teak", [0.42, 0.32, 0.22, 1.], 0.7, 0.0)?,
        rug: surface(&mut e, "Wool rug", [0.42, 0.36, 0.3, 1.], 1., 0.)?,
        solar: surface(&mut e, "Solar cells", [0.04, 0.06, 0.12, 1.], 0.12, 0.3)?,
        parasol: [
            surface(&mut e, "Ecru canvas", [0.72, 0.68, 0.58, 1.], 0.9, 0.)?,
            surface(&mut e, "Navy canvas", [0.08, 0.13, 0.26, 1.], 0.9, 0.)?,
            surface(&mut e, "Terracotta canvas", [0.58, 0.25, 0.13, 1.], 0.9, 0.)?,
        ],
        lamp: glowing(&mut e, "Warm lamp", [1., 0.8, 0.55, 1.], 30000.)?,
        pool: surface(&mut e, "Pool finish", [0.5, 0.66, 0.7, 1.], 0.35, 0.)?,
        plinth: surface(&mut e, "Grey render", [0.4, 0.4, 0.39, 1.], 0.9, 0.)?,
        terracotta: surface(&mut e, "Terracotta", [0.6, 0.3, 0.17, 1.], 0.85, 0.)?,
        frame: surface(&mut e, "White window frame", [0.82, 0.82, 0.8, 1.], 0.5, 0.)?,
        rubber: surface(&mut e, "Tyre rubber", [0.03, 0.03, 0.03, 1.], 0.9, 0.)?,
        coir: surface(&mut e, "Coir", [0.36, 0.25, 0.14, 1.], 1., 0.)?,
        lacquer: surface(&mut e, "Kitchen lacquer", [0.78, 0.77, 0.74, 1.], 0.35, 0.)?,
        worktop: surface(&mut e, "Basalt worktop", [0.12, 0.12, 0.12, 1.], 0.3, 0.)?,
        cars: [
            surface(&mut e, "Red car paint", [0.42, 0.04, 0.04, 1.], 0.25, 0.4)?,
            surface(&mut e, "Blue car paint", [0.06, 0.12, 0.26, 1.], 0.25, 0.4)?,
            surface(&mut e, "Silver car paint", [0.55, 0.56, 0.57, 1.], 0.3, 0.8)?,
        ],
        bins: [
            surface(&mut e, "Green bin", [0.08, 0.2, 0.1, 1.], 0.6, 0.)?,
            surface(&mut e, "Grey bin", [0.18, 0.19, 0.2, 1.], 0.6, 0.)?,
        ],
        gravel: textures::gravel(&mut e)?,
        plants: [
            (textures::grass_tuft(&mut e)?, [0.32, 0.5]),
            (textures::ornamental_grass(&mut e)?, [0.9, 0.9]),
            (textures::perennial(&mut e)?, [0.55, 0.7]),
        ],
    };

    let base = e.doc().clone();
    // Loose furniture stands a little out of line, the same way each run.
    let mut jit = Random(4711);

    // Terrain 180 x 130 m: a gently rolling site rising slowly to the north, with long, low
    // swells rather than mounds, flat pads for the houses and a flat street strip.
    let origin = DVec3::new(-160., -140., 0.);
    let out = e.apply(Command::CreateTerrainGrid { origin, u: DVec3::X * 320., v: DVec3::Y * 280., spacing: 2.5 })?;
    let Some(Element::Instance(terrain)) = out.created.first().copied() else { return Err("no terrain".into()) };
    e.enter(terrain)?;
    for (centre, radius, height) in [
        (DVec2::new(-5., 48.), 45., 4.5),
        (DVec2::new(35., 40.), 30., 2.2),
        (DVec2::new(-48., 18.), 26., 1.6),
        (DVec2::new(48., -2.), 22., 1.1),
        (DVec2::new(-40., -20.), 18., 0.6),
        (DVec2::new(30., -22.), 16., 0.5),
        (DVec2::new(-75., 30.), 30., 2.4),
        (DVec2::new(75., 20.), 28., 1.8),
        (DVec2::new(-120., 60.), 50., 6.),
        (DVec2::new(110., 80.), 55., 7.),
        (DVec2::new(0., 120.), 60., 6.5),
        (DVec2::new(-110., -110.), 45., 4.),
        (DVec2::new(100., -115.), 50., 5.),
        (DVec2::new(10., -125.), 40., 3.),
        (DVec2::new(-140., -20.), 35., 3.5),
        (DVec2::new(140., -10.), 35., 3.),
    ] {
        let vertices = e.mesh().vertices().map(|(v, _)| v).collect();
        let centre = centre.extend(0.) - origin;
        e.apply(Command::Smoove {
            vertices,
            centres: vec![centre],
            radius,
            normal: DVec3::Z,
            offset: DVec3::Z * height,
        })?;
    }
    // The street and the plots on either side of it stay level with the road.
    let stamp = |e: &mut Editor, x0: f64, y0: f64, x1: f64, y1: f64, blend: f64, height: f64| -> Result<()> {
        let outline = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
            .iter()
            .map(|&(x, y)| DVec2::new(x, y) - origin.truncate())
            .collect();
        e.apply(Command::StampTerrainOutline { outline, offset: blend, height })?;
        Ok(())
    };
    stamp(&mut e, -160., -78., 160., -30.5, 3., 0.)?;
    stamp(&mut e, -22., -30.5, 20., 8.6, 5., 0.)?;
    stamp(&mut e, -96., -30.5, -44., -6., 5., 0.)?;
    stamp(&mut e, 42., -30.5, 92., -6., 5., 0.)?;
    // Houses up the hill behind the plot, along a lane, each on a level pad cut into the
    // slope: x, width, depth and style.
    let uphill = [
        (-90., 11., 9., 1),
        (-66., 12., 9., 0),
        (-42., 10., 9., 2),
        (-18., 12., 9.5, 3),
        (6., 11., 10., 1),
        (30., 12., 9., 0),
        (54., 10., 9., 2),
        (78., 13., 10., 3),
    ];
    let mut pads = Vec::new();
    for &(x, w, d, _) in &uphill {
        let (x0, y0, x1, y1) = (x - 4.5, 27., x + w + 3., 30. + d + 3.);
        let mesh = e.mesh();
        let heights: Vec<f64> = mesh
            .vertices()
            .map(|(_, v)| v.pos + origin)
            .filter(|q| (x0..x1).contains(&q.x) && (y0..y1).contains(&q.y))
            .map(|q| q.z)
            .collect();
        let level = heights.iter().sum::<f64>() / heights.len().max(1) as f64;
        stamp(&mut e, x0, y0, x1, y1, 4., level)?;
        pads.push(level);
    }
    // A gentle bank around the south, east and west of the pool, so its stone surround sits
    // slightly sunk into the lawn. The pool footprint, the house side and the deck stay flat.
    let local = |x: f64, y: f64| DVec3::new(x, y, 0.) - origin;
    let outside_pool = |q: DVec3| {
        let q = q + origin;
        let pool = (4.45..=18.55).contains(&q.x) && (-15.05..=-4.35).contains(&q.y);
        let deck = (-14.2..=4.45).contains(&q.x) && (-10.8..=-4.).contains(&q.y);
        !pool && !deck && q.y < -4.4
    };
    let vertices = e.mesh().vertices().filter(|(_, v)| outside_pool(v.pos)).map(|(id, _)| id).collect();
    e.apply(Command::Smoove {
        vertices,
        centres: vec![local(4.5, -4.4), local(4.5, -15.), local(18.5, -15.), local(18.5, -4.4)],
        radius: 4.5,
        normal: DVec3::Z,
        offset: DVec3::Z * 0.38,
    })?;
    let edges = e.mesh().edges().map(|(edge, _)| edge).collect();
    e.apply(Command::SetEdges { edges, soft: Some(true), smooth: Some(true), hidden: None })?;
    // A patchwork of lawn variants in irregular drifts breaks up the texture repeat; the
    // ground under the cantilever, out of rain and sun, is worn and dry.
    let mesh = e.mesh();
    let mut patches: [Vec<rysmo_model::FaceId>; 3] = Default::default();
    for (id, f) in mesh.faces() {
        let c = f.outer.iter().map(|v| mesh.pos(*v)).sum::<DVec3>() / f.outer.len() as f64;
        let n = ((c.x * 0.11).sin() + (c.y * 0.13 + c.x * 0.05).cos() + (c.x * 0.31 - c.y * 0.27).sin() * 0.6) * 0.5;
        patches[if n < -0.25 {
            0
        } else if n < 0.3 {
            1
        } else {
            2
        }]
        .push(id);
    }
    for (k, faces) in patches.into_iter().enumerate() {
        e.apply(Command::Paint { faces: faces.clone(), material: Some(grass[k]), back: false })?;
        e.apply(Command::Paint { faces, material: Some(grass[k]), back: true })?;
    }
    let mesh = e.mesh();
    let ground: Vec<[DVec3; 3]> = mesh
        .faces()
        .flat_map(|(_, f)| {
            let q: Vec<DVec3> = f.outer.iter().map(|v| mesh.pos(*v) + origin).collect();
            (1..q.len().saturating_sub(1)).map(move |k| [q[0], q[k], q[k + 1]])
        })
        .collect();
    e.exit();
    // The terrain height under a point, interpolated across its triangle.
    let ground_at = |x: f64, y: f64| -> f64 {
        for [a, b, c] in &ground {
            let d = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
            if d.abs() < 1e-12 {
                continue;
            }
            let l1 = ((b.y - c.y) * (x - c.x) + (c.x - b.x) * (y - c.y)) / d;
            let l2 = ((c.y - a.y) * (x - c.x) + (a.x - c.x) * (y - c.y)) / d;
            let l3 = 1. - l1 - l2;
            if l1 >= -1e-9 && l2 >= -1e-9 && l3 >= -1e-9 {
                return l1 * a.z + l2 * b.z + l3 * c.z;
            }
        }
        0.
    };

    // Stone garden wall with a granite coping along the back of the pad.
    batch(&mut e, &base, |e| {
        cuboid(e, DVec3::new(-22., 8., 0.), DVec3::new(20., 8.45, 1.0), p.stone)?;
        rounded(e, DVec3::new(-22.05, 7.95, 1.0), DVec3::new(20.05, 8.5, 1.07), 0.015, p.coping)?;

        // Ground floor: a slab 250 mm above the garden, 300 mm rendered walls with real openings,
        // a glass wall to the garden and a furnished interior.
        cuboid(e, DVec3::new(-14., -4., 0.), DVec3::new(4., 6., FLOOR), p.concrete)?;
        // North wall: a utility window, the back door, a living room and a kitchen window.
        wall(
            e,
            DVec2::new(-14., 6.),
            DVec2::new(4., 6.),
            -DVec2::Y,
            0.3,
            FLOOR,
            CEILING,
            &[(1.6, 1.2, 0.9, 1.4, 1), (3.3, 1.0, 0., 2.2, 0), (10.0, 2.0, 0.9, 1.5, 2), (14.2, 2.2, 1.05, 1.1, 2)],
            p.render,
            &p,
        )?;
        // West wall: the front door and a hall window under the car port.
        wall(
            e,
            DVec2::new(-14., -3.7),
            DVec2::new(-14., 5.7),
            DVec2::X,
            0.3,
            FLOOR,
            CEILING,
            &[(0.7, 1.1, 0., 2.25, 0), (5.4, 1.2, 0.9, 1.4, 1)],
            p.render,
            &p,
        )?;
        // East wall: a glazed door to the covered terrace and a dining room window.
        wall(
            e,
            DVec2::new(4., -3.7),
            DVec2::new(4., 5.7),
            -DVec2::X,
            0.3,
            FLOOR,
            CEILING,
            &[(0.7, 1.0, 0., 2.2, 1), (2.7, 1.8, 0.9, 1.4, 2)],
            p.render,
            &p,
        )?;
        let wing = cuboid(e, DVec3::new(-14., -4., FLOOR), DVec3::new(-8.5, -3.7, CEILING), p.render)?;
        window(e, wing, -DVec3::Y, 1.2, 3., 0.65, 1.5, 3, &p)?;
        // The hall partition has a doorway into the living room.
        cuboid(e, DVec3::new(-8.8, -3.7, FLOOR), DVec3::new(-8.5, 2.0, CEILING), p.render)?;
        cuboid(e, DVec3::new(-8.8, 2.9, FLOOR), DVec3::new(-8.5, 5.7, CEILING), p.render)?;
        cuboid(e, DVec3::new(-8.8, 2.0, FLOOR + 2.15), DVec3::new(-8.5, 2.9, CEILING), p.render)?;
        // South wall: sliding doors from the living room and from the dining area to the
        // terrace, with solid wall between them.
        wall(
            e,
            DVec2::new(-8.5, -4.),
            DVec2::new(4., -4.),
            DVec2::Y,
            0.3,
            FLOOR,
            CEILING,
            &[(0.5, 6.0, 0., 2.45, 4), (8.5, 2.6, 0., 2.4, 2)],
            p.render,
            &p,
        )?;
        for x in [-5.0, 1.3] {
            for y in [-3.92, -3.78] {
                rounded(
                    e,
                    DVec3::new(x, y - 0.015, FLOOR + 0.9),
                    DVec3::new(x + 0.03, y + 0.015, FLOOR + 1.5),
                    0.01,
                    p.chrome,
                )?;
            }
        }
        // Front and back doors with steps, a doormat and wall lights.
        door(e, DVec2::new(-13.85, -3.0), DVec2::new(-13.85, -1.9), -DVec2::X, FLOOR, 2.25, p.oak, true, &p)?;
        rounded(e, DVec3::new(-15.25, -3.4, 0.), DVec3::new(-14.0, -1.5, 0.125), 0.015, p.coping)?;
        rounded(e, DVec3::new(-14.7, -3.4, 0.125), DVec3::new(-14.0, -1.5, FLOOR), 0.015, p.coping)?;
        rounded(e, DVec3::new(-14.6, -3.0, FLOOR), DVec3::new(-14.05, -1.9, FLOOR + 0.015), 0.01, p.coir)?;
        rounded(e, DVec3::new(-15.4, -3.6, 2.75), DVec3::new(-14.0, -1.3, 2.87), 0.03, p.concrete)?;
        door(e, DVec2::new(-10.7, 5.85), DVec2::new(-9.7, 5.85), DVec2::Y, FLOOR, 2.2, p.oak, false, &p)?;
        rounded(e, DVec3::new(-11.0, 6.0, 0.), DVec3::new(-9.4, 6.9, 0.125), 0.015, p.coping)?;
        rounded(e, DVec3::new(-11.0, 6.0, 0.125), DVec3::new(-9.4, 6.45, FLOOR), 0.015, p.coping)?;
        for (a, out) in [(DVec3::new(-14.0, -3.45, 2.05), -DVec3::X), (DVec3::new(-11.15, 6.0, 2.05), DVec3::Y)] {
            let along = DVec3::Z.cross(out);
            rounded(e, a - along * 0.08, a + along * 0.08 + out * 0.1 + DVec3::Z * 0.25, 0.02, p.dark)?;
            cuboid(e, a - along * 0.06 + out * 0.02 - DVec3::Z * 0.005, a + along * 0.06 + out * 0.08, p.lamp)?;
        }
        // The house number by the front door.
        let number = e.apply(Command::CreateText3d {
            content: "12".into(),
            settings: rysmo_model::text3d::Text3dSettings { height: 0.15, depth: 0.012, ..Default::default() },
            origin: DVec3::new(-14.0, -1.35, 1.95),
            x: -DVec3::Y,
            y: DVec3::Z,
        })?;
        if let Some(Element::Instance(number)) = number.created.first().copied() {
            paint(e, number, p.dark)?;
        }
        // An outside tap by the back door.
        let tap = DVec3::new(-11.6, 6.0, 0.6);
        limb(e, tap, tap + DVec3::Y * 0.08, 0.012, 0.012, p.chrome)?;
        limb(e, tap + DVec3::Y * 0.08, tap + DVec3::new(0., 0.1, -0.06), 0.01, 0.01, p.chrome)?;
        column(e, tap + DVec3::new(0., 0.08, 0.01), 0.022, 0.012, 16, p.chrome)?;

        raised(e, FLOOR, |e| {
            // Hall: a shoe cabinet with a bench top under the window.
            rounded(e, DVec3::new(-13.7, 0.3, 0.1), DVec3::new(-13.3, 1.6, 0.48), 0.01, p.lacquer)?;
            rounded(e, DVec3::new(-13.7, 0.28, 0.48), DVec3::new(-13.27, 1.62, 0.52), 0.01, p.oak)?;
            cuboid(e, DVec3::new(-13.66, 0.32, 0.), DVec3::new(-13.34, 1.58, 0.1), p.dark)?;
            // Living room: curtains drawn back beside the sliding doors and a print over the sofa.
            for x in [-8.45, -2.05] {
                rounded(e, DVec3::new(x, -3.68, 0.03), DVec3::new(x + 0.5, -3.56, 2.75), 0.05, p.linen)?;
            }
            rounded(e, DVec3::new(-8.5, -3.66, 2.74), DVec3::new(3.6, -3.62, 2.77), 0.01, p.dark)?;
            cuboid(e, DVec3::new(-8.5, -1.9, 1.3), DVec3::new(-8.47, -0.5, 2.1), p.dark)?;
            cuboid(e, DVec3::new(-8.47, -1.85, 1.35), DVec3::new(-8.465, -0.55, 2.05), p.cars[1])?;
            rounded(e, DVec3::new(-7.6, -2.6, 0.), DVec3::new(-3.4, 1.2, 0.015), 0.05, p.rug)?;
            sofa(e, DVec2::new(-7.4, 0.3), DVec2::new(-3.8, 1.2), DVec2::Y, false, p.fabric, p.accent, &p)?;
            sofa(e, DVec2::new(-8.3, -2.2), DVec2::new(-7.4, 1.2), -DVec2::X, false, p.fabric, p.linen, &p)?;
            loose(e, DVec3::new(-5.45, -1.0, 0.), &mut jit, 4., 0.04, |e| {
                table(e, DVec2::new(-6.3, -1.4), DVec2::new(-4.6, -0.6), 0.4, p.oak, &p)
            })?;
            bookcase(e, DVec2::new(-8.3, 5.3), DVec2::new(-5.8, 5.7), -DVec2::Y, &p)?;
            loose(e, DVec3::new(-3.2, -2.9, 0.), &mut jit, 20., 0.05, |e| {
                potted(e, DVec3::new(-3.2, -2.9, 0.), 0.24, 0.5, p.terracotta, &p)
            })?;
            // Kitchen along the north wall and an island, and the dining table.
            rounded(e, DVec3::new(-0.4, 5.12, 0.1), DVec3::new(3.6, 5.7, 0.86), 0.005, p.lacquer)?;
            cuboid(e, DVec3::new(-0.36, 5.18, 0.), DVec3::new(3.56, 5.7, 0.1), p.dark)?;
            rounded(e, DVec3::new(-0.42, 5.07, 0.86), DVec3::new(3.62, 5.7, 0.9), 0.01, p.worktop)?;
            cuboid(e, DVec3::new(0.8, 5.2, 0.8), DVec3::new(1.6, 5.6, 0.905), p.chrome)?;
            rail(
                e,
                &[DVec3::new(1.2, 5.6, 0.9), DVec3::new(1.2, 5.6, 1.2), DVec3::new(1.2, 5.38, 1.22)],
                0.012,
                p.chrome,
            )?;
            for (x0, x1) in [(-0.4, 0.1), (2.5, 3.6)] {
                rounded(e, DVec3::new(x0, 5.35, 1.5), DVec3::new(x1, 5.7, 2.3), 0.005, p.lacquer)?;
            }
            rounded(e, DVec3::new(0.2, 2.45, 0.1), DVec3::new(3.0, 3.35, 0.86), 0.005, p.lacquer)?;
            cuboid(e, DVec3::new(0.26, 2.5, 0.), DVec3::new(2.94, 3.3, 0.1), p.dark)?;
            rounded(e, DVec3::new(0.15, 2.4, 0.86), DVec3::new(3.05, 3.45, 0.9), 0.01, p.worktop)?;
            for x in [0.8, 1.6, 2.4] {
                let (x, y) = (x + jit.range(-0.06, 0.06), 2.05 + jit.range(-0.08, 0.05));
                lathe(
                    e,
                    DVec3::new(x, y, 0.),
                    &[(0., 0.), (0.2, 0.), (0.2, 0.015), (0.03, 0.03), (0.025, 0.62), (0., 0.62)],
                    32,
                    p.dark,
                )?;
                lathe(
                    e,
                    DVec3::new(x, 2.05, 0.62),
                    &[(0., 0.), (0.18, 0.), (0.19, 0.03), (0.18, 0.05), (0., 0.05)],
                    32,
                    p.oak,
                )?;
            }
            table(e, DVec2::new(-0.2, -1.6), DVec2::new(2.4, -0.6), 0.75, p.oak, &p)?;
            for (x, y, turn) in [(0.4, -2.0, 180.), (1.6, -2.0, 180.), (0.4, -0.2, 0.), (1.6, -0.2, 0.)] {
                seat(e, DVec3::new(x, y, 0.), turn, p.oak, &mut jit, &p)?;
            }
            let room = CEILING - FLOOR;
            for x in [0.3, 1.1, 1.9] {
                pendant(e, DVec3::new(x, -1.1, 1.6), room, &p)?;
            }
            for (x, y) in [
                (-7., -2.),
                (-4.5, -2.),
                (-7., 1.5),
                (-4.5, 1.5),
                (2., 4.2),
                (-2., -2.8),
                (2., 1.5),
                (-11., -1.),
                (-11., 3.),
            ] {
                downlight(e, x, y, room, &p)?;
            }
            Ok(())
        })?;
        // A straight stair to the upper floor: 18 risers of 172 mm and 280 mm oak treads on a
        // solid rendered flight, with glass balustrades on both sides.
        let rise = (LEVEL - FLOOR) / 18.;
        let (s0, s1, run) = (3.53, 4.48, 0.28);
        for k in 0..17 {
            let x0 = -5.4 + k as f64 * run;
            let z = FLOOR + (k + 1) as f64 * rise;
            cuboid(e, DVec3::new(x0, s0, FLOOR), DVec3::new(x0 + run, s1, z - 0.04), p.render)?;
            rounded(e, DVec3::new(x0 - 0.025, s0, z - 0.04), DVec3::new(x0 + run, s1, z), 0.008, p.oak)?;
        }
        let pitch = |x: f64| FLOOR + (x + 5.4) / run * rise;
        for (y0, y1) in [(s0 - 0.02, s0), (s1, s1 + 0.02)] {
            let (xa, xb) = (-5.4, -0.64);
            let quad = [
                DVec3::new(xa, y0, FLOOR + 0.05),
                DVec3::new(xb, y0, pitch(xb) - 0.25),
                DVec3::new(xb, y0, pitch(xb) + 0.9),
                DVec3::new(xa, y0, pitch(xa) + rise + 0.9),
            ];
            prism(e, quad, DVec3::Y * (y1 - y0), p.railing)?;
            let y = (y0 + y1) / 2.;
            limb(
                e,
                DVec3::new(xa, y, pitch(xa) + rise + 0.92),
                DVec3::new(xb, y, pitch(xb) + 0.92),
                0.02,
                0.02,
                p.chrome,
            )?;
        }
        Ok(())
    })?;

    // Ceiling slab with the stairwell opening, and the slab of the cantilevered upper floor.
    batch(&mut e, &base, |e| {
        for (a, b) in [
            (DVec2::new(-14.3, -4.4), DVec2::new(4.3, 3.5)),
            (DVec2::new(-14.3, 4.5), DVec2::new(4.3, 6.3)),
            (DVec2::new(-14.3, 3.5), DVec2::new(-4.4, 4.5)),
            (DVec2::new(-0.64, 3.5), DVec2::new(4.3, 4.5)),
        ] {
            cuboid(e, a.extend(CEILING), b.extend(LEVEL), p.concrete)?;
        }
        cuboid(e, DVec3::new(4.3, -3.7, CEILING), DVec3::new(11., 4.8, LEVEL), p.concrete)?;
        // Upper floor: 300 mm timber-clad walls, windows with 900 mm sills, a glazed door to the
        // roof terrace and floor-to-ceiling glass to the bedroom.
        wall(
            e,
            DVec2::new(-6., -3.2),
            DVec2::new(11., -3.2),
            DVec2::Y,
            0.3,
            LEVEL,
            TOP,
            &[(1.2, 1.8, 0.9, 1.5, 2), (4.5, 2.4, 0.9, 1.5, 2), (11.2, 2.0, 0.05, 2.5, 2)],
            p.wood,
            &p,
        )?;
        wall(
            e,
            DVec2::new(-6., 4.8),
            DVec2::new(11., 4.8),
            -DVec2::Y,
            0.3,
            LEVEL,
            TOP,
            &[(2.5, 3.0, 0.9, 1.5, 2), (15.0, 0.8, 1.4, 0.8, 1)],
            p.wood,
            &p,
        )?;
        wall(
            e,
            DVec2::new(-6., -2.9),
            DVec2::new(-6., 4.5),
            DVec2::X,
            0.3,
            LEVEL,
            TOP,
            &[(0.5, 1.0, 0.06, 2.2, 1), (3.0, 2.0, 0.9, 1.5, 2)],
            p.wood,
            &p,
        )?;
        wall(
            e,
            DVec2::new(11., -2.9),
            DVec2::new(11., 4.5),
            -DVec2::X,
            0.3,
            LEVEL,
            TOP,
            &[(2.0, 2.4, 0.9, 1.5, 2)],
            p.wood,
            &p,
        )?;
        rounded(e, DVec3::new(-5.8, -1.47, LEVEL + 0.9), DVec3::new(-5.77, -1.44, LEVEL + 1.4), 0.01, p.chrome)?;
        // Partition between the study and the bedroom, with a doorway.
        cuboid(e, DVec3::new(4.3, -2.9, LEVEL), DVec3::new(4.42, 0.4, TOP), p.render)?;
        cuboid(e, DVec3::new(4.3, 1.4, LEVEL), DVec3::new(4.42, 4.5, TOP), p.render)?;
        cuboid(e, DVec3::new(4.3, 0.4, LEVEL + 2.1), DVec3::new(4.42, 1.4, TOP), p.render)?;
        // Glass round the stairwell upstairs.
        balustrade(e, DVec3::new(-4.4, 3.47, LEVEL), DVec3::new(-0.64, 3.47, LEVEL), &p)?;
        balustrade(e, DVec3::new(-4.43, 3.47, LEVEL), DVec3::new(-4.43, 4.5, LEVEL), &p)?;
        raised(e, LEVEL, |e| {
            // Bedroom behind the fins, with an en-suite shower room in its north-east corner.
            rounded(e, DVec3::new(5.4, -1.0, 0.), DVec3::new(9.4, 2.3, 0.015), 0.05, p.rug)?;
            bed(e, DVec2::new(5.3, 2.3), DVec2::new(7.3, 4.5), &p)?;
            for x in [4.8, 7.4] {
                rounded(e, DVec3::new(x, 3.95, 0.), DVec3::new(x + 0.45, 4.45, 0.5), 0.02, p.oak)?;
                lathe(
                    e,
                    DVec3::new(x + 0.22, 4.2, 0.5),
                    &[(0., 0.), (0.07, 0.), (0.03, 0.04), (0.012, 0.3), (0., 0.3)],
                    24,
                    p.dark,
                )?;
                lathe(e, DVec3::new(x + 0.22, 4.2, 0.72), &[(0., 0.), (0.14, 0.), (0.1, 0.2), (0., 0.2)], 32, p.lamp)?;
            }
            // A wardrobe of three doors with recessed pulls, against the partition.
            rounded(e, DVec3::new(4.42, -2.9, 0.), DVec3::new(5.0, -0.2, 2.3), 0.005, p.lacquer)?;
            for y in [-2.0, -1.1] {
                cuboid(e, DVec3::new(5.0, y - 0.003, 0.02), DVec3::new(5.003, y + 0.003, 2.28), p.dark)?;
            }
            for y in [-2.08, -1.18, -0.28] {
                cuboid(e, DVec3::new(5.0, y, 0.9), DVec3::new(5.012, y + 0.03, 1.3), p.dark)?;
            }
            let shower = TOP - LEVEL;
            cuboid(e, DVec3::new(7.9, 2.6, 0.), DVec3::new(8.0, 4.5, shower), p.render)?;
            cuboid(e, DVec3::new(8.0, 2.5, 0.), DVec3::new(8.2, 2.6, shower), p.render)?;
            cuboid(e, DVec3::new(9.0, 2.5, 0.), DVec3::new(10.7, 2.6, shower), p.render)?;
            cuboid(e, DVec3::new(8.2, 2.5, 2.1), DVec3::new(9.0, 2.6, shower), p.render)?;
            rounded(e, DVec3::new(8.22, 2.53, 0.01), DVec3::new(8.98, 2.57, 2.09), 0.005, p.lacquer)?;
            rounded(e, DVec3::new(9.5, 4.0, 0.3), DVec3::new(10.6, 4.5, 0.85), 0.01, p.oak)?;
            rounded(e, DVec3::new(9.45, 3.95, 0.85), DVec3::new(10.65, 4.5, 0.88), 0.01, p.lacquer)?;
            lathe(e, DVec3::new(10.05, 4.22, 0.88), &[(0., 0.), (0.2, 0.), (0.21, 0.12), (0., 0.12)], 40, p.lacquer)?;
            cuboid(e, DVec3::new(9.5, 4.47, 1.1), DVec3::new(10.6, 4.5, 1.9), p.chrome)?;
            rounded(e, DVec3::new(8.25, 4.0, 0.), DVec3::new(8.75, 4.5, 0.42), 0.15, p.lacquer)?;
            rounded(e, DVec3::new(8.2, 4.32, 0.42), DVec3::new(8.8, 4.5, 0.85), 0.02, p.lacquer)?;
            rounded(e, DVec3::new(10.0, 2.62, 0.), DVec3::new(10.7, 3.6, 0.04), 0.01, p.lacquer)?;
            cuboid(e, DVec3::new(9.98, 2.62, 0.04), DVec3::new(10.0, 3.6, 2.0), p.railing)?;
            limb(e, DVec3::new(10.69, 3.5, 1.2), DVec3::new(10.69, 3.5, 2.1), 0.012, 0.012, p.chrome)?;
            // Roller blinds half down at the study and bedroom windows.
            cuboid(e, DVec3::new(-4.8, -2.9, 1.85), DVec3::new(-3.0, -2.88, 2.42), p.linen)?;
            cuboid(e, DVec3::new(9.0, 4.48, 1.4), DVec3::new(9.8, 4.5, 2.2), p.linen)?;
            cuboid(e, DVec3::new(10.68, -0.9, 1.6), DVec3::new(10.7, 1.5, 2.42), p.linen)?;
            // A print on the partition.
            cuboid(e, DVec3::new(4.27, 1.8, 1.2), DVec3::new(4.3, 3.0, 2.0), p.dark)?;
            cuboid(e, DVec3::new(4.265, 1.85, 1.25), DVec3::new(4.27, 2.95, 1.95), p.accent)?;
            seat(e, DVec3::new(9.8, -1.6, 0.), 220., p.oak, &mut jit, &p)?;
            // Study and lounge.
            table(e, DVec2::new(-4.6, -2.75), DVec2::new(-2.4, -2.05), 0.75, p.oak, &p)?;
            seat(e, DVec3::new(-3.5, -1.6, 0.), 180., p.oak, &mut jit, &p)?;
            bookcase(e, DVec2::new(3.92, -2.7), DVec2::new(4.3, 0.2), -DVec2::X, &p)?;
            rounded(e, DVec3::new(-1.5, -0.6, 0.), DVec3::new(2.6, 2.4, 0.015), 0.05, p.rug)?;
            sofa(e, DVec2::new(-1.2, 1.4), DVec2::new(2.3, 2.3), DVec2::Y, true, p.linen, p.accent, &p)?;
            sofa(e, DVec2::new(1.6, -0.6), DVec2::new(2.5, 0.8), DVec2::X, false, p.linen, p.fabric, &p)?;
            loose(e, DVec3::new(0.3, 0.4, 0.), &mut jit, 4., 0.04, |e| {
                table(e, DVec2::new(-0.4, 0.0), DVec2::new(1.0, 0.8), 0.4, p.oak, &p)
            })?;
            // A floor lamp and a desk lamp.
            lathe(
                e,
                DVec3::new(-1.6, 2.6, 0.),
                &[(0., 0.), (0.16, 0.), (0.16, 0.02), (0.015, 0.03), (0.012, 1.55), (0., 1.55)],
                32,
                p.dark,
            )?;
            lathe(e, DVec3::new(-1.6, 2.6, 1.45), &[(0., 0.), (0.22, 0.), (0.16, 0.28), (0., 0.28)], 40, p.lamp)?;
            lathe(
                e,
                DVec3::new(-2.7, -2.5, 0.75),
                &[(0., 0.), (0.08, 0.), (0.08, 0.02), (0.01, 0.03), (0.01, 0.38), (0., 0.38)],
                24,
                p.dark,
            )?;
            lathe(e, DVec3::new(-2.7, -2.5, 1.02), &[(0., 0.), (0.1, 0.), (0.06, 0.12), (0., 0.12)], 32, p.lamp)?;
            for (x, y) in [(7.7, 0.), (0.5, 0.7), (-3.5, -1.5)] {
                downlight(e, x, y, TOP - LEVEL, &p)?;
            }
            loose(e, DVec3::new(-5.25, 2.9, 0.), &mut jit, 20., 0.05, |e| {
                potted(e, DVec3::new(-5.25, 2.9, 0.), 0.25, 0.5, p.terracotta, &p)
            })?;
            Ok(())
        })?;
        // Timber fins over the cantilever, standing on its slab edge and fixed to the roof.
        for i in 0..15 {
            let x = 4.2 + i as f64 * 0.45;
            rounded(e, DVec3::new(x, -3.6, LEVEL), DVec3::new(x + 0.08, -3.2, TOP), 0.008, p.wood)?;
        }
        // Two steel columns on base plates carry the far end of the cantilever.
        for y in [-3.0, 4.2] {
            rounded(e, DVec3::new(10.4, y, 0.06), DVec3::new(10.6, y + 0.2, CEILING), 0.012, p.dark)?;
            cuboid(e, DVec3::new(10.3, y - 0.1, 0.06), DVec3::new(10.7, y + 0.3, 0.08), p.dark)?;
        }
        // A covered terrace under the cantilever, paved instead of lawn, with a herb planter.
        cuboid(e, DVec3::new(4.0, -3.7, 0.), DVec3::new(11.2, 4.8, 0.06), p.paving)?;
        rounded(e, DVec3::new(6.0, 3.6, 0.06), DVec3::new(9.6, 4.4, 0.55), 0.04, p.concrete)?;
        rounded(e, DVec3::new(6.05, 3.65, 0.06), DVec3::new(9.55, 4.35, 0.5), 0.02, p.coir)?;
        for x in [6.6, 7.8, 9.0] {
            bush(e, DVec3::new(x, 4.0, 0.5), 0.4, &p)?;
        }
        // Downpipes from the two roofs, on brackets, to gullies at the foot of the walls.
        let pipe = [
            DVec3::new(11.05, 4.3, ROOF - 0.1),
            DVec3::new(11.05, 4.3, CEILING - 0.1),
            DVec3::new(10.65, 4.3, CEILING - 0.4),
            DVec3::new(10.65, 4.3, 0.06),
        ];
        rail(e, &pipe, 0.045, p.dark)?;
        rail(e, &[DVec3::new(-13.8, 6.06, LEVEL - 0.1), DVec3::new(-13.8, 6.06, 0.0)], 0.045, p.dark)?;
        for z in [1.0, 2.2] {
            cuboid(e, DVec3::new(10.6, 4.25, z), DVec3::new(10.71, 4.35, z + 0.03), p.dark)?;
            cuboid(e, DVec3::new(-13.85, 6.0, z), DVec3::new(-13.75, 6.11, z + 0.03), p.dark)?;
        }
        Ok(())
    })?;

    // Main roof: slab, parapet with a capping, tilted solar panels on rails, a skylight on an
    // upstand and a stainless flue.
    batch(&mut e, &base, |e| {
        cuboid(e, DVec3::new(-6.4, -3.7, TOP), DVec3::new(11.4, 5.3, ROOF), p.dark)?;
        for (a, b) in [
            (DVec2::new(-6.4, -3.7), DVec2::new(11.4, -3.5)),
            (DVec2::new(-6.4, 5.1), DVec2::new(11.4, 5.3)),
            (DVec2::new(-6.4, -3.5), DVec2::new(-6.2, 5.1)),
            (DVec2::new(11.2, -3.5), DVec2::new(11.4, 5.1)),
        ] {
            cuboid(e, a.extend(ROOF), b.extend(ROOF + 0.2), p.dark)?;
        }
        for (a, b) in [
            (DVec2::new(-6.43, -3.73), DVec2::new(11.43, -3.47)),
            (DVec2::new(-6.43, 5.07), DVec2::new(11.43, 5.33)),
            (DVec2::new(-6.43, -3.47), DVec2::new(-6.17, 5.07)),
            (DVec2::new(11.17, -3.47), DVec2::new(11.43, 5.07)),
        ] {
            rounded(e, a.extend(ROOF + 0.2), b.extend(ROOF + 0.24), 0.008, p.steel)?;
        }
        let tilt = 10f64.to_radians();
        for row in 0..2 {
            let y = 0.6 + row as f64 * 2.2;
            for (rail_y, height) in [(y, 0.12), (y + 1.55, 0.12 + 1.55 * tilt.tan())] {
                cuboid(e, DVec3::new(1.4, rail_y, ROOF), DVec3::new(8.4, rail_y + 0.05, ROOF + height), p.steel)?;
            }
            for col in 0..6 {
                let x = 1.5 + col as f64 * 1.15;
                let panel =
                    cuboid(e, DVec3::new(x, y, ROOF + 0.12), DVec3::new(x + 1.1, y + 1.7, ROOF + 0.155), p.solar)?;
                let frame = rounded(
                    e,
                    DVec3::new(x - 0.01, y - 0.01, ROOF + 0.12),
                    DVec3::new(x + 1.11, y + 1.71, ROOF + 0.15),
                    0.01,
                    p.steel,
                )?;
                e.apply(Command::Rotate {
                    elements: vec![Element::Instance(panel), Element::Instance(frame)],
                    centre: DVec3::new(x, y, ROOF + 0.12),
                    axis: DVec3::X,
                    angle: 10.,
                    copies: 0,
                })?;
            }
        }
        rounded(e, DVec3::new(-4., 0., ROOF), DVec3::new(-2., 2., ROOF + 0.25), 0.02, p.render)?;
        rounded(e, DVec3::new(-4.05, -0.05, ROOF + 0.25), DVec3::new(-1.95, 2.05, ROOF + 0.3), 0.02, p.railing)?;
        lathe(
            e,
            DVec3::new(-4.95, 3.85, ROOF),
            &[(0., 0.), (0.2, 0.), (0.2, 0.03), (0.1, 0.06), (0.1, 1.0), (0., 1.0)],
            32,
            p.chrome,
        )?;
        lathe(
            e,
            DVec3::new(-4.95, 3.85, ROOF + 1.06),
            &[(0., 0.), (0.08, 0.), (0.2, 0.06), (0.2, 0.09), (0., 0.13)],
            32,
            p.chrome,
        )?;
        column(e, DVec3::new(-4.95, 3.85, ROOF + 1.0), 0.02, 0.06, 8, p.chrome)?;

        // Roof terrace over the hall wing: boards, glass balustrades round every open edge,
        // planters and loungers.
        boards(e, DVec3::new(-14.25, -4.35, LEVEL), DVec3::new(-6.05, 6.25, LEVEL + 0.04), true, 0.145, p.deck)?;
        balustrade(e, DVec3::new(-14.27, -4.37, LEVEL), DVec3::new(-6.0, -4.37, LEVEL), &p)?;
        balustrade(e, DVec3::new(-14.27, -4.37, LEVEL), DVec3::new(-14.27, 6.27, LEVEL), &p)?;
        balustrade(e, DVec3::new(-14.27, 6.27, LEVEL), DVec3::new(-6.0, 6.27, LEVEL), &p)?;
        balustrade(e, DVec3::new(-6.0, -4.37, LEVEL), DVec3::new(-6.0, -3.2, LEVEL), &p)?;
        balustrade(e, DVec3::new(-6.0, 4.8, LEVEL), DVec3::new(-6.0, 6.27, LEVEL), &p)?;
        raised(e, LEVEL + 0.04, |e| {
            for x in [-12.6, -11.2] {
                let nudge = DVec3::new(jit.range(-0.06, 0.06), jit.range(-0.1, 0.1), 0.);
                lounger(e, DVec3::new(x, -1.0, 0.) + nudge, jit.range(-5., 5.), &p)?;
            }
            lathe(
                e,
                DVec3::new(-11.9, -2.4, 0.),
                &[(0., 0.), (0.2, 0.), (0.2, 0.02), (0.03, 0.03), (0.03, 0.45), (0., 0.45)],
                32,
                p.dark,
            )?;
            lathe(
                e,
                DVec3::new(-11.9, -2.4, 0.45),
                &[(0., 0.), (0.24, 0.), (0.25, 0.02), (0.24, 0.035), (0., 0.035)],
                40,
                p.teak,
            )?;
            rounded(e, DVec3::new(-13.9, 3.5, 0.), DVec3::new(-13.1, 5.8, 0.5), 0.04, p.concrete)?;
            rounded(e, DVec3::new(-13.85, 3.55, 0.), DVec3::new(-13.15, 5.75, 0.46), 0.02, p.coir)?;
            bush(e, DVec3::new(-13.5, 4.1, 0.46), 0.7, &p)?;
            bush(e, DVec3::new(-13.5, 5.2, 0.46), 0.6, &p)?;
            Ok(())
        })?;
        Ok(())
    })?;

    // Garden terrace: boards on a recessed dark plinth, a deck step, a dining set under a
    // parasol, an outdoor lounge and potted shrubs.
    batch(&mut e, &base, |e| {
        cuboid(e, DVec3::new(-13.95, -9.95, 0.), DVec3::new(3.95, -4.4, 0.21), p.dark)?;
        boards(e, DVec3::new(-14., -10., 0.21), DVec3::new(4., -4.4, 0.25), true, 0.145, p.deck)?;
        cuboid(e, DVec3::new(-13.95, -10.55, 0.), DVec3::new(3.95, -10., 0.08), p.dark)?;
        boards(e, DVec3::new(-14., -10.6, 0.08), DVec3::new(4., -10., 0.12), true, 0.145, p.deck)?;
        raised(e, 0.25, |e| {
            // The dining set stands a few degrees off the line of the house.
            let set = built(e, |e| {
                table(e, DVec2::new(-11.5, -8.6), DVec2::new(-8.5, -7.6), 0.75, p.teak, &p)?;
                for x in [-11.0, -10.0, -9.0] {
                    seat(e, DVec3::new(x, -9.0, 0.), 180., p.teak, &mut jit, &p)?;
                    seat(e, DVec3::new(x, -7.2, 0.), 0., p.teak, &mut jit, &p)?;
                }
                parasol(e, DVec3::new(-10., -8.1, 0.), [p.parasol[2], p.parasol[2]], &p)
            })?;
            turn(e, &set, DVec3::new(-10., -8.1, 0.), 3.5)?;
            sofa(e, DVec2::new(-5.5, -9.6), DVec2::new(-2.0, -8.7), -DVec2::Y, false, p.outdoor, p.accent, &p)?;
            sofa(e, DVec2::new(-6.4, -9.6), DVec2::new(-5.5, -6.8), -DVec2::X, false, p.outdoor, p.linen, &p)?;
            loose(e, DVec3::new(-3.8, -7.45, 0.), &mut jit, 5., 0.05, |e| {
                table(e, DVec2::new(-4.6, -7.9), DVec2::new(-3.0, -7.0), 0.35, p.concrete, &p)
            })?;
            for x in [-13.4, -12.6, 2.6, 3.4] {
                loose(e, DVec3::new(x, -5.0, 0.), &mut jit, 30., 0.06, |e| {
                    potted(e, DVec3::new(x, -5.0, 0.), 0.3, 0.6, p.concrete, &p)
                })?;
            }
            Ok(())
        })?;
        Ok(())
    })?;

    // Pool: a surround of paving slabs on a stone plinth, a pool shell in a pale finish with
    // water 140 mm below the coping, entry steps under water, a waterline tile band, lights,
    // skimmers, a ladder and loungers.
    batch(&mut e, &base, |e| {
        e.apply(Command::DrawRectangle {
            corner: DVec3::new(4.5, -15., 0.),
            u: DVec3::X * 14.,
            v: DVec3::Y * 10.6,
            facing: None,
        })?;
        e.apply(Command::DrawRectangle {
            corner: DVec3::new(6., -12.5, 0.),
            u: DVec3::X * 11.,
            v: DVec3::Y * 5.,
            facing: None,
        })?;
        let mesh = e.mesh();
        let pool = mesh.faces().map(|(f, _)| f).find(|f| (mesh.face_area(*f) - 55.).abs() < 1e-6).ok_or("no pool")?;
        let surround = mesh.faces().map(|(f, _)| f).find(|f| *f != pool).ok_or("no surround")?;
        let up = mesh.face_normal(surround).ok_or("no normal")?.z.signum();
        e.apply(Command::PushPull { face: surround, distance: 0.5 * up, keep_base: false })?;
        let pool_group = group_loose(e)?;
        paint(e, pool_group, p.stone)?;
        let place = e.active().instances.get(&pool_group).ok_or("no pool group")?.transform;
        e.enter(pool_group)?;
        let mesh = e.mesh();
        let (mut top, mut shell) = (Vec::new(), Vec::new());
        for (id, f) in mesh.faces() {
            let c = place.transform_point3(f.outer.iter().map(|v| mesh.pos(*v)).sum::<DVec3>() / f.outer.len() as f64);
            let n = mesh.face_normal(id).unwrap_or(DVec3::Z);
            let inside = (5.99..=17.01).contains(&c.x) && (-12.51..=-7.49).contains(&c.y);
            if n.z.abs() > 0.9 && c.z > 0.4 {
                top.push(id);
            } else if inside {
                shell.push(id);
            }
        }
        e.apply(Command::Paint { faces: top, material: Some(p.paving), back: false })?;
        e.apply(Command::Paint { faces: shell.clone(), material: Some(p.pool), back: false })?;
        e.apply(Command::Paint { faces: shell, material: Some(p.pool), back: true })?;
        e.exit();
        e.apply(Command::Move { elements: vec![Element::Instance(pool_group)], offset: DVec3::Z * 0.02 })?;
        let (rim, lip, top) = (0.45, 0.04, 0.52);
        for (i, step) in [0.19, 0.36].into_iter().enumerate() {
            let x1 = 7.6 - i as f64 * 0.5;
            rounded(e, DVec3::new(6., -12.5, 0.02), DVec3::new(x1, -7.5, 0.02 + step), 0.03, p.pool)?;
        }
        // Waterline tiles on all four walls.
        let band = (0.34, top);
        cuboid(e, DVec3::new(6., -12.5, band.0), DVec3::new(17., -12.48, band.1), p.tile)?;
        cuboid(e, DVec3::new(6., -7.52, band.0), DVec3::new(17., -7.5, band.1), p.tile)?;
        cuboid(e, DVec3::new(16.98, -12.48, band.0), DVec3::new(17., -7.52, band.1), p.tile)?;
        cuboid(e, DVec3::new(6., -12.48, band.0), DVec3::new(6.02, -7.52, band.1), p.tile)?;
        for x in [8., 11.5, 15.] {
            cuboid(e, DVec3::new(x - 0.12, -12.48, 0.16), DVec3::new(x + 0.12, -12.45, 0.26), p.lamp)?;
        }
        cuboid(e, DVec3::new(6.02, -12.47, 0.02), DVec3::new(16.98, -7.53, top - 0.08), p.water)?;
        // Coping: a slightly overhanging rim with rounded edges, in four pieces.
        for (a, b) in [
            (DVec2::new(6. - rim, -12.5 - rim), DVec2::new(17. + rim, -12.5 + lip)),
            (DVec2::new(6. - rim, -7.5 - lip), DVec2::new(17. + rim, -7.5 + rim)),
            (DVec2::new(6. - rim, -12.5 + lip), DVec2::new(6. + lip, -7.5 - lip)),
            (DVec2::new(17. - lip, -12.5 + lip), DVec2::new(17. + rim, -7.5 - lip)),
        ] {
            rounded(e, a.extend(top), b.extend(top + 0.05), 0.025, p.coping)?;
        }
        for x in [8.5, 14.5] {
            rounded(e, DVec3::new(x, -7.35, top + 0.05), DVec3::new(x + 0.3, -7.1, top + 0.056), 0.03, p.dark)?;
        }
        // A stainless ladder: two bent handrails over the coping, fixed in sockets, and treads.
        for x in [15.6, 16.1] {
            rail(
                e,
                &[
                    DVec3::new(x, -7.58, 0.06),
                    DVec3::new(x, -7.58, 0.82),
                    DVec3::new(x, -7.45, 0.98),
                    DVec3::new(x, -7.28, 0.98),
                    DVec3::new(x, -7.2, 0.85),
                    DVec3::new(x, -7.2, top + 0.05),
                ],
                0.021,
                p.chrome,
            )?;
            column(e, DVec3::new(x, -7.2, top + 0.05), 0.04, 0.008, 24, p.chrome)?;
        }
        for z in [0.12, 0.3] {
            rounded(e, DVec3::new(15.6, -7.7, z), DVec3::new(16.1, -7.56, z + 0.03), 0.01, p.chrome)?;
        }
        for x in [9., 10.8, 12.6, 14.4] {
            let nudge = DVec3::new(jit.range(-0.08, 0.08), jit.range(-0.15, 0.1), 0.);
            lounger(e, DVec3::new(x, -5.6, top) + nudge, jit.range(-6., 6.), &p)?;
        }
        parasol(e, DVec3::new(16.6, -5.4, top), [p.parasol[0], p.parasol[1]], &p)?;
        // Steps down from the raised surround: one to the deck and two to the covered terrace.
        rounded(e, DVec3::new(4.0, -10.0, 0.), DVec3::new(4.52, -4.4, 0.385), 0.015, p.coping)?;
        rounded(e, DVec3::new(4.5, -4.42, 0.), DVec3::new(18.5, -4.05, 0.36), 0.015, p.coping)?;
        rounded(e, DVec3::new(4.5, -4.07, 0.), DVec3::new(18.5, -3.7, 0.2), 0.015, p.coping)?;
        Ok(())
    })?;

    // The street: a 6.5 m asphalt carriageway with yellow lines, granite kerbs with a dropped
    // section at the drive, gullies with grates, and pavements on both sides.
    batch(&mut e, &base, |e| {
        let crossing = (-14.1, -9.9);
        cuboid(e, DVec3::new(-160., -40., 0.), DVec3::new(160., -33.5, 0.04), p.asphalt)?;
        cuboid(e, DVec3::new(-160., -39.8, 0.04), DVec3::new(160., -39.68, 0.043), p.marking)?;
        cuboid(e, DVec3::new(-160., -33.82, 0.04), DVec3::new(160., -33.7, 0.043), p.marking)?;
        for i in 0..64 {
            let x = -159. + i as f64 * 5.;
            cuboid(e, DVec3::new(x, -36.81, 0.04), DVec3::new(x + 3., -36.69, 0.043), p.marking)?;
        }
        rounded(e, DVec3::new(-160., -40.15, 0.), DVec3::new(160., -40.0, 0.16), 0.02, p.coping)?;
        // The pavement and kerbs stop at the lane's two junctions and at the crossover.
        for (x0, x1) in [(-160., -106.5), (-101.5, crossing.0), (crossing.1, 101.5), (106.5, 160.)] {
            let k0 = if x0 == crossing.1 { x0 + 0.9 } else { x0 };
            let k1 = if x1 == crossing.0 { x1 - 0.9 } else { x1 };
            rounded(e, DVec3::new(k0, -33.5, 0.), DVec3::new(k1, -33.35, 0.16), 0.02, p.coping)?;
            cuboid(e, DVec3::new(x0, -33.35, 0.), DVec3::new(x1, -31.2, 0.14), p.paving)?;
            rounded(e, DVec3::new(x0, -31.2, 0.), DVec3::new(x1, -31.05, 0.16), 0.02, p.coping)?;
        }
        // The crossover: a dropped kerb between tapered transition kerbs, and the pavement
        // ramping up from it to the level of the drive.
        let (low, apron) = (0.07, 0.12);
        rounded(e, DVec3::new(crossing.0, -33.5, 0.), DVec3::new(crossing.1, -33.35, low), 0.02, p.coping)?;
        for (a, b) in [(crossing.0 - 0.9, crossing.0), (crossing.1 + 0.9, crossing.1)] {
            let top = |x: f64, y: f64| DVec3::new(x, y, if x == a { 0.16 } else { low });
            prism(e, [top(a, -33.5), top(b, -33.5), top(b, -33.35), top(a, -33.35)], -DVec3::Z * 0.16, p.coping)?;
        }
        let ramp = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
        prism(
            e,
            [
                ramp(crossing.0, -33.35, low),
                ramp(crossing.1, -33.35, low),
                ramp(crossing.1, -32.6, apron),
                ramp(crossing.0, -32.6, apron),
            ],
            -DVec3::Z * 0.12,
            p.paving,
        )?;
        cuboid(e, DVec3::new(crossing.0, -32.6, 0.), DVec3::new(crossing.1, -31.05, apron), p.paving)?;
        for x in [-150., -125., -100., -65., -40.5, -20.5, 4., 28.5, 53., 77., 102., 127., 152.] {
            for y in [-33.95, -40.0] {
                cuboid(e, DVec3::new(x, y, 0.04), DVec3::new(x + 0.45, y + 0.4, 0.045), p.dark)?;
                for k in 0..6 {
                    let bx = x + 0.04 + k as f64 * 0.07;
                    cuboid(e, DVec3::new(bx, y + 0.04, 0.045), DVec3::new(bx + 0.03, y + 0.36, 0.05), p.steel)?;
                }
            }
        }
        cuboid(e, DVec3::new(-160., -42.3, 0.), DVec3::new(160., -40.15, 0.14), p.paving)?;
        rounded(e, DVec3::new(-160., -42.45, 0.), DVec3::new(160., -42.3, 0.16), 0.02, p.coping)?;
        for x in [-122., -92., -62., -32., 1., 30., 60., 90., 120., 150.] {
            street_lamp(e, DVec3::new(x, -31.6, 0.14), -DVec2::Y, &p)?;
        }
        for x in [-135., -105., -75., -45., -15., 15., 45., 75., 105., 135.] {
            street_lamp(e, DVec3::new(x, -41.8, 0.14), DVec2::Y, &p)?;
        }
        Ok(())
    })?;

    // A curving driveway, 3.6 m wide, from the dropped kerb to the car port by the house,
    // with granite edge setts, a linear drain inside the gate and bollard lights.
    batch(&mut e, &base, |e| {
        let drive = [
            DVec2::new(-12.0, -31.05),
            DVec2::new(-12.0, -27.5),
            DVec2::new(-13.6, -20.5),
            DVec2::new(-16.8, -11.),
            DVec2::new(-17.6, -3.),
            DVec2::new(-17.6, 6.4),
        ];
        let spline = |t: f64| -> DVec2 {
            let n = drive.len() - 1;
            let k = ((t * n as f64).floor() as usize).min(n - 1);
            let u = t * n as f64 - k as f64;
            let p0 = drive[k.saturating_sub(1)];
            let (p1, p2) = (drive[k], drive[k + 1]);
            let p3 = drive[(k + 2).min(n)];
            // Catmull-Rom through the control points.
            0.5 * ((2. * p1)
                + (-p0 + p2) * u
                + (2. * p0 - 5. * p1 + 4. * p2 - p3) * u * u
                + (-p0 + 3. * p1 - 3. * p2 + p3) * u * u * u)
        };
        let steps = 48;
        let centre: Vec<DVec2> = (0..=steps).map(|i| spline(i as f64 / steps as f64)).collect();
        let side = |i: usize| {
            let a = centre[i.saturating_sub(1)];
            let b = centre[(i + 1).min(steps)];
            let d = (b - a).normalize();
            DVec2::new(-d.y, d.x)
        };
        let half = 1.8;
        for i in 0..steps {
            let (a, b) = (centre[i], centre[i + 1]);
            let (sa, sb) = (side(i), side(i + 1));
            let quad = [a - sa * half, b - sb * half, b + sb * half, a + sa * half].map(|q| q.extend(0.));
            slab(e, &quad, 0.12, p.asphalt)?;
            for s in [-1., 1.] {
                let (o, w) = (half, 0.14);
                let kerb =
                    [a + sa * s * o, b + sb * s * o, b + sb * s * (o + w), a + sa * s * (o + w)].map(|q| q.extend(0.));
                slab(e, &kerb, 0.15, p.coping)?;
            }
        }
        let g = spline(0.08);
        let gs = side((0.08 * steps as f64) as usize);
        let along = DVec2::new(gs.y, -gs.x) * 0.08;
        let grate = [g - gs * half - along, g + gs * half - along, g + gs * half + along, g - gs * half + along]
            .map(|q| q.extend(0.12));
        slab(e, &grate, 0.004, p.dark)?;
        for k in 0..30 {
            let t = -1. + 2. * (k as f64 + 0.5) / 30.;
            let c = g + gs * half * t * 0.97;
            let bar = [
                c - gs * 0.018 - along * 0.8,
                c + gs * 0.018 - along * 0.8,
                c + gs * 0.018 + along * 0.8,
                c - gs * 0.018 + along * 0.8,
            ]
            .map(|q| q.extend(0.124));
            slab(e, &bar, 0.004, p.steel)?;
        }
        for i in (6..steps - 6).step_by(6) {
            let q = centre[i] + side(i) * (half + 0.5);
            bollard(e, q.extend(ground_at(q.x, q.y)), &p)?;
        }
        // Grass grown up against the edge setts.
        let mut random = Random(303);
        for i in (3..steps - 2).step_by(2) {
            for s in [-1., 1.] {
                if random.next() < 0.6 {
                    let q = centre[i] + side(i) * s * (half + 0.2 + random.range(0., 0.12));
                    planting(e, q.x, q.y, 0, &ground_at, &mut random, &p)?;
                }
            }
        }
        Ok(())
    })?;

    // Front boundary: a stone wall with a coping, rendered piers with lamps, a sliding gate
    // parked behind the wall on the side away from the pedestrian gate, and the pedestrian
    // gate with the letterbox and house number beside it.
    batch(&mut e, &base, |e| {
        let (wall_y, opening) = (-30.9, (-14.4, -9.6));
        for (x0, x1) in [(-40., opening.0 - 0.55), (opening.1 + 0.55, -7.35), (-5.05, 40.)] {
            cuboid(e, DVec3::new(x0, wall_y, 0.), DVec3::new(x1, wall_y + 0.35, 0.95), p.stone)?;
            rounded(
                e,
                DVec3::new(x0 - 0.02, wall_y - 0.04, 0.95),
                DVec3::new(x1 + 0.02, wall_y + 0.39, 1.0),
                0.015,
                p.coping,
            )?;
        }
        for x in [opening.0 - 0.55, opening.1, -7.35, -5.6] {
            rounded(e, DVec3::new(x, wall_y - 0.08, 0.), DVec3::new(x + 0.55, wall_y + 0.43, 1.55), 0.01, p.render)?;
            rounded(
                e,
                DVec3::new(x - 0.03, wall_y - 0.11, 1.55),
                DVec3::new(x + 0.58, wall_y + 0.46, 1.61),
                0.015,
                p.coping,
            )?;
            rounded(
                e,
                DVec3::new(x + 0.17, wall_y - 0.14, 1.2),
                DVec3::new(x + 0.38, wall_y - 0.08, 1.4),
                0.02,
                p.dark,
            )?;
            cuboid(e, DVec3::new(x + 0.19, wall_y - 0.13, 1.195), DVec3::new(x + 0.36, wall_y - 0.09, 1.205), p.lamp)?;
        }
        // The sliding gate: a frame with vertical slats on two wheels running on a ground rail
        // across the opening, held upright by guide rollers on a post and closing into a catch on
        // the east pier. It stands open, parked behind the west wall.
        let gy = wall_y + 0.5;
        // The leaf runs on a concrete runway behind the wall, level with the drive, and a
        // steel track set into both.
        let lift = 0.12;
        cuboid(e, DVec3::new(-20.1, gy - 0.2, 0.), DVec3::new(-13.95, gy + 0.26, lift), p.concrete)?;
        rounded(e, DVec3::new(-20.0, gy - 0.02, lift), DVec3::new(-10.05, gy + 0.08, lift + 0.015), 0.006, p.steel)?;
        let (g0, g1) = (-19.6, -14.35);
        let z = |v: f64| lift + v;
        for (z0, z1) in [(0.14, 0.22), (1.36, 1.42)] {
            rounded(e, DVec3::new(g0, gy, z(z0)), DVec3::new(g1, gy + 0.06, z(z1)), 0.01, p.dark)?;
        }
        for x in [g0, g1 - 0.08] {
            rounded(e, DVec3::new(x, gy, z(0.14)), DVec3::new(x + 0.08, gy + 0.06, z(1.42)), 0.01, p.dark)?;
        }
        for k in 0..34 {
            let x = g0 + 0.12 + k as f64 * 0.15;
            rounded(e, DVec3::new(x, gy + 0.005, z(0.22)), DVec3::new(x + 0.07, gy + 0.055, z(1.36)), 0.01, p.dark)?;
        }
        for x in [g0 + 0.4, g1 - 0.4] {
            limb(e, DVec3::new(x, gy - 0.01, z(0.075)), DVec3::new(x, gy + 0.07, z(0.075)), 0.06, 0.06, p.steel)?;
            rounded(
                e,
                DVec3::new(x - 0.07, gy - 0.02, z(0.08)),
                DVec3::new(x + 0.07, gy + 0.08, z(0.14)),
                0.01,
                p.dark,
            )?;
        }
        // A guide post with two top rollers holding the leaf upright, and the catch on the
        // receiving pier.
        rounded(e, DVec3::new(-14.42, gy + 0.18, lift), DVec3::new(-14.3, gy + 0.3, z(1.6)), 0.01, p.dark)?;
        rounded(e, DVec3::new(-14.42, gy - 0.1, z(1.5)), DVec3::new(-14.3, gy + 0.3, z(1.56)), 0.005, p.dark)?;
        for y in [gy - 0.03, gy + 0.09] {
            column(e, DVec3::new(-14.36, y, z(1.38)), 0.03, 0.12, 24, p.steel)?;
        }
        rounded(
            e,
            DVec3::new(opening.1, wall_y + 0.43, z(1.15)),
            DVec3::new(opening.1 + 0.2, gy + 0.12, z(1.45)),
            0.01,
            p.dark,
        )?;
        // Pedestrian gate between the small piers: a slatted leaf on two hinges with a lever and
        // a lock box, a doorbell on the pier, the letterbox and the house number.
        let (p0, p1) = (-6.8, -5.6);
        let py = wall_y + 0.15;
        rounded(e, DVec3::new(p0 + 0.02, py, 0.06), DVec3::new(p1 - 0.02, py + 0.05, 0.12), 0.01, p.dark)?;
        rounded(e, DVec3::new(p0 + 0.02, py, 1.3), DVec3::new(p1 - 0.02, py + 0.05, 1.36), 0.01, p.dark)?;
        for x in [p0 + 0.02, p1 - 0.08] {
            rounded(e, DVec3::new(x, py, 0.06), DVec3::new(x + 0.06, py + 0.05, 1.36), 0.01, p.dark)?;
        }
        for k in 0..7 {
            let x = p0 + 0.13 + k as f64 * 0.15;
            rounded(e, DVec3::new(x, py + 0.005, 0.12), DVec3::new(x + 0.07, py + 0.045, 1.3), 0.01, p.dark)?;
        }
        for z in [0.25, 1.15] {
            column(e, DVec3::new(p0 + 0.01, py - 0.015, z), 0.015, 0.12, 16, p.steel)?;
        }
        rounded(e, DVec3::new(p1 - 0.2, py - 0.03, 0.95), DVec3::new(p1 - 0.04, py, 1.1), 0.01, p.dark)?;
        limb(e, DVec3::new(p1 - 0.14, py - 0.03, 1.05), DVec3::new(p1 - 0.14, py - 0.08, 1.05), 0.01, 0.01, p.chrome)?;
        limb(e, DVec3::new(p1 - 0.14, py - 0.08, 1.05), DVec3::new(p1 - 0.28, py - 0.08, 1.05), 0.01, 0.01, p.chrome)?;
        rounded(e, DVec3::new(-5.2, wall_y - 0.1, 1.1), DVec3::new(-5.12, wall_y - 0.08, 1.25), 0.008, p.chrome)?;
        letterbox(e, DVec3::new(-4.3, wall_y, 0.5), -DVec3::Y, &p)?;
        let number = e.apply(Command::CreateText3d {
            content: "12".into(),
            settings: rysmo_model::text3d::Text3dSettings { height: 0.12, depth: 0.01, ..Default::default() },
            origin: DVec3::new(-7.25, wall_y - 0.08, 0.95),
            x: DVec3::X,
            y: DVec3::Z,
        })?;
        if let Some(Element::Instance(number)) = number.created.first().copied() {
            paint(e, number, p.chrome)?;
        }
        // Wheelie bins inside the wall by the pedestrian gate.
        bin(e, DVec3::new(-4.2, wall_y + 0.75, 0.), 180., p.bins[0], &p)?;
        bin(e, DVec3::new(-3.5, wall_y + 0.75, 0.), 180., p.bins[1], &p)?;
        // A path of irregular, slightly turned slabs from the pedestrian gate to the front door.
        let walk = [
            DVec2::new(-6.2, -30.3),
            DVec2::new(-8.6, -22.),
            DVec2::new(-12.6, -14.5),
            DVec2::new(-14.6, -11.2),
            DVec2::new(-14.62, -5.2),
            DVec2::new(-14.62, -3.75),
        ];
        let mut slabs = Random(91);
        let mut along = 0.;
        for w in walk.windows(2) {
            let (a, b) = (w[0], w[1]);
            let length = a.distance(b);
            let dir = (b - a) / length;
            while along < length {
                // Narrower slabs where the path squeezes between the drive and the deck.
                let narrow = a.y > -15.;
                let jitter = if narrow { 0.04 } else { 0.12 };
                let c = a + dir * along + DVec2::new(-dir.y, dir.x) * slabs.range(-jitter, jitter);
                let (sw, sd) = if narrow {
                    (slabs.range(0.5, 0.62), slabs.range(0.4, 0.5))
                } else {
                    (slabs.range(0.55, 0.8), slabs.range(0.42, 0.58))
                };
                let heading = dir.y.atan2(dir.x).to_degrees() + 90.;
                let wobble = if narrow { 5. } else { 14. };
                ground_slab(e, c, sw, sd, heading + slabs.range(-wobble, wobble), &ground_at, p.paving)?;
                along += sd + slabs.range(0.12, 0.22);
            }
            along -= length;
        }
        Ok(())
    })?;

    // A car port over the end of the drive: three steel posts and a flat roof carried on the
    // house wall, with the firewood store against the wall beneath it.
    batch(&mut e, &base, |e| {
        for y in [-0.7, 2.8, 6.2] {
            rounded(e, DVec3::new(-20.05, y, 0.), DVec3::new(-19.9, y + 0.15, 2.6), 0.012, p.dark)?;
            cuboid(e, DVec3::new(-20.12, y - 0.07, 0.), DVec3::new(-19.83, y + 0.22, 0.02), p.dark)?;
        }
        cuboid(e, DVec3::new(-20.2, -0.9, 2.6), DVec3::new(-14.0, 6.5, 2.82), p.render)?;
        rounded(e, DVec3::new(-20.25, -0.95, 2.82), DVec3::new(-14.0, 6.55, 2.88), 0.01, p.dark)?;
        for y in [0.6, 3.4] {
            cuboid(e, DVec3::new(-18.0, y, 2.585), DVec3::new(-17.0, y + 0.2, 2.6), p.lamp)?;
        }
        woodpile(e, DVec2::new(-14.5, 3.6), 2.2, &p)?;
        Ok(())
    })?;
    cybertruck::build(&mut e, DVec3::new(-17.6, 2.3, 0.12))?;

    // Side and back boundaries: vertical-board timber fences on steel posts with caps, two
    // rails between posts, following the ground.
    batch(&mut e, &base, |e| {
        let fence = |e: &mut Editor, a: DVec2, b: DVec2| -> Result<()> {
            let length = a.distance(b);
            let dir = (b - a) / length;
            let normal = DVec2::new(-dir.y, dir.x);
            let bays = (length / 2.).ceil() as usize;
            let bay = length / bays as f64;
            let post = |s: f64| a + dir * s;
            for k in 0..=bays {
                let q = post(k as f64 * bay);
                let z = ground_at(q.x, q.y);
                rounded(
                    e,
                    (q - DVec2::splat(0.045)).extend(z - 0.1),
                    (q + DVec2::splat(0.045)).extend(z + 1.75),
                    0.01,
                    p.dark,
                )?;
                rounded(
                    e,
                    (q - DVec2::splat(0.06)).extend(z + 1.75),
                    (q + DVec2::splat(0.06)).extend(z + 1.79),
                    0.01,
                    p.dark,
                )?;
            }
            for k in 0..bays {
                let (q0, q1) = (post(k as f64 * bay), post((k + 1) as f64 * bay));
                let (z0, z1) = (ground_at(q0.x, q0.y), ground_at(q1.x, q1.y));
                for h in [0.3, 1.45] {
                    limb(
                        e,
                        (q0 + normal * 0.07).extend(z0 + h),
                        (q1 + normal * 0.07).extend(z1 + h),
                        0.025,
                        0.025,
                        p.steel,
                    )?;
                }
                let boards = ((bay - 0.1) / 0.155) as usize;
                for j in 0..boards {
                    let s = 0.06 + j as f64 * 0.155;
                    let (c0, c1) = (q0 + dir * s, q0 + dir * (s + 0.14));
                    let z = ground_at((c0.x + c1.x) / 2., (c0.y + c1.y) / 2.);
                    let quad = [c0 + normal * 0.095, c1 + normal * 0.095, c1 + normal * 0.115, c0 + normal * 0.115]
                        .map(|q| q.extend(z - 0.03));
                    slab(e, &quad, 1.73, p.wood)?;
                }
            }
            Ok(())
        };
        fence(e, DVec2::new(-40.2, -30.5), DVec2::new(-40.2, 8.2))?;
        fence(e, DVec2::new(40.2, 8.2), DVec2::new(40.2, -30.5))?;
        fence(e, DVec2::new(-40.2, 8.2), DVec2::new(-22.1, 8.2))?;
        fence(e, DVec2::new(20.1, 8.2), DVec2::new(40.2, 8.2))?;
        Ok(())
    })?;

    // A timber garden shed on a concrete base: clad walls, a pitched roof with fascias and
    // a gutter, a ledged and braced door on hinges, a framed window and a downpipe.
    batch(&mut e, &base, |e| {
        let (sx, sy) = (-34., -8.);
        let corners = [(sx, sy), (sx + 3., sy), (sx, sy + 2.4), (sx + 3., sy + 2.4)].map(|(x, y)| ground_at(x, y));
        let base_z = corners.iter().copied().fold(f64::MIN, f64::max);
        let low = corners.iter().copied().fold(f64::MAX, f64::min);
        let shed = DVec3::new(sx, sy, base_z + 0.1);
        rounded(
            e,
            DVec3::new(sx - 0.15, sy - 0.15, low - 0.1),
            DVec3::new(sx + 3.15, sy + 2.55, shed.z),
            0.02,
            p.concrete,
        )?;
        cuboid(e, shed, shed + DVec3::new(3.0, 2.4, 2.0), p.wood)?;
        let (rise, over) = (0.6, 0.25);
        for x in [0., 2.92] {
            let tri =
                [shed + DVec3::new(x, 0., 2.0), shed + DVec3::new(x, 2.4, 2.0), shed + DVec3::new(x, 1.2, 2.0 + rise)];
            let d = DVec3::X * 0.08;
            let mut triangles = vec![tri, [tri[0] + d, tri[2] + d, tri[1] + d]];
            for k in 0..3 {
                let n = (k + 1) % 3;
                triangles.push([tri[k], tri[n] + d, tri[n]]);
                triangles.push([tri[k], tri[k] + d, tri[n] + d]);
            }
            let centre = (tri[0] + tri[1] + tri[2]) / 3. + d / 2.;
            for t in &mut triangles {
                let c = (t[0] + t[1] + t[2]) / 3.;
                if (t[1] - t[0]).cross(t[2] - t[0]).dot(c - centre) < 0. {
                    t.swap(1, 2);
                }
            }
            solid(e, &triangles, 10., p.wood)?;
        }
        let slope = DVec2::new(1.2, rise).normalize();
        for (y0, sign) in [(-over, 1.), (2.4 + over, -1.)] {
            let drop = rise * over / 1.2;
            let eave = |x: f64| shed + DVec3::new(x, y0, 2.0 - drop);
            let ridge = |x: f64| shed + DVec3::new(x, 1.2, 2.0 + rise);
            let up = DVec3::new(0., -slope.y * sign, slope.x) * 0.05;
            prism(e, [eave(-over), eave(3. + over), ridge(3. + over), ridge(-over)], up, p.dark)?;
            rounded(
                e,
                eave(-over) + DVec3::new(0., -0.025 * sign, -0.12),
                eave(3. + over) + DVec3::new(0., 0., 0.03),
                0.008,
                p.wood,
            )?;
        }
        let gutter = shed + DVec3::new(-over, -over - 0.08, 2.0 - rise * over / 1.2 - 0.1);
        limb(e, gutter, gutter + DVec3::X * (3. + 2. * over), 0.05, 0.05, p.dark)?;
        rail(
            e,
            &[gutter + DVec3::X * 0.3, shed + DVec3::new(0.05, -0.06, 1.6), shed + DVec3::new(0.05, -0.06, -0.08)],
            0.03,
            p.dark,
        )?;
        rounded(e, shed + DVec3::new(0.9, -0.03, 0.03), shed + DVec3::new(1.85, 0.0, 1.9), 0.005, p.wood)?;
        for z in [0.25, 0.95, 1.65] {
            cuboid(e, shed + DVec3::new(0.92, -0.06, z), shed + DVec3::new(1.83, -0.03, z + 0.12), p.wood)?;
        }
        for z in [0.3, 1.6] {
            cuboid(e, shed + DVec3::new(0.86, -0.065, z), shed + DVec3::new(1.3, -0.055, z + 0.05), p.dark)?;
        }
        limb(e, shed + DVec3::new(1.75, -0.06, 1.0), shed + DVec3::new(1.75, -0.11, 1.0), 0.01, 0.01, p.dark)?;
        facade_window(e, shed + DVec3::new(2.15, 0., 1.0), DVec3::X, -DVec3::Y, 0.6, 0.6, p.frame, &p)?;
        rounded(e, shed + DVec3::new(0.7, -0.9, -0.12), shed + DVec3::new(2.1, -0.15, -0.05), 0.04, p.paving)?;
        Ok(())
    })?;

    // Contemporary neighbouring houses across the street and either side of the plot, with
    // front hedges, paths, drives to their car ports and parked cars.
    batch(&mut e, &base, |e| {
        for (k, (x, w)) in [(-80., 10.), (-56., 11.), (-32., 12.), (-6., 11.), (20., 10.), (44., 13.), (68., 11.)]
            .into_iter()
            .enumerate()
        {
            let (d, front) = (9., -51.);
            let door_colour = [p.oak, p.dark, p.teak, p.oak, p.dark, p.cars[1], p.teak][k];
            let parked = (k % 3 != 1).then_some(p.cars[k % 3]);
            neighbour(e, DVec3::new(x, front - d, 0.), w, d, k % 4, door_colour, parked, &p)?;
            for (h0, h1) in [(x - 1., x + 0.9), (x + 2.6, x + w - 0.2)] {
                rounded(e, DVec3::new(h0, -44.0, 0.), DVec3::new(h1, -43.1, 1.1), 0.3, p.hedge)?;
            }
            cuboid(e, DVec3::new(x + 1.2, front + 1.0, 0.), DVec3::new(x + 2.3, -42.45, 0.03), p.paving)?;
            cuboid(e, DVec3::new(x + w, front + 0.2, 0.), DVec3::new(x + w + 3.4, -42.45, 0.04), p.paving)?;
        }
        for (k, (x, w, style)) in [(-88., 11., 1), (-64., 12., 0), (48., 11., 3), (74., 12., 2)].into_iter().enumerate()
        {
            let (d, front) = (9., -20.);
            let parked = (k != 2).then_some(p.cars[(k + 1) % 3]);
            let parts = built(e, |e| neighbour(e, DVec3::new(x, front, 0.), w, d, style, p.dark, parked, &p))?;
            turn(e, &parts, DVec3::new(x + w / 2., front + d / 2., 0.), 180.)?;
            let door = x + w - 2.3;
            for (h0, h1) in [(x - 7., x - 3.6), (x + 0.2, door - 0.2), (door + 1.3, x + w + 2.)] {
                rounded(e, DVec3::new(h0, -31.0, 0.), DVec3::new(h1, -30.2, 1.2), 0.3, p.hedge)?;
            }
            cuboid(e, DVec3::new(door, -31.05, 0.), DVec3::new(door + 1.1, front - 1.0, 0.03), p.paving)?;
            cuboid(e, DVec3::new(x - 3.4, -31.05, 0.), DVec3::new(x, front - 0.2, 0.04), p.paving)?;
        }
        Ok(())
    })?;
    // The lane behind the plot: up from the street on each side and along the hillside,
    // following the ground, with the houses along it.
    batch(&mut e, &base, |e| {
        let route = [
            DVec2::new(-104., -33.4),
            DVec2::new(-104., -8.),
            DVec2::new(-102., 16.),
            DVec2::new(-95., 24.75),
            DVec2::new(95., 24.75),
            DVec2::new(102., 16.),
            DVec2::new(104., -8.),
            DVec2::new(104., -33.4),
        ];
        road(e, &route, 4.5, &ground_at, &p)
    })?;
    for (k, (&(x, w, d, style), &level)) in uphill.iter().zip(&pads).enumerate() {
        batch(&mut e, &base, |e| {
            let door_colour = [p.oak, p.dark, p.teak, p.cars[1], p.dark, p.oak, p.cars[0], p.dark][k];
            let parked = (k % 2 == 0).then_some(p.cars[(k / 2) % 3]);
            let parts = built(e, |e| neighbour(e, DVec3::new(x, 30., level), w, d, style, door_colour, parked, &p))?;
            turn(e, &parts, DVec3::new(x + w / 2., 30. + d / 2., level), 180.)?;
            let door = x + w - 2.3;
            for (h0, h1) in [(x - 6., x - 3.6), (x + 0.2, door - 0.2), (door + 1.3, x + w + 2.)] {
                rounded(e, DVec3::new(h0, 27.1, level - 0.1), DVec3::new(h1, 27.9, level + 1.1), 0.3, p.hedge)?;
            }
            // The path and the drive ramp from the lane's edge up or down to the pad.
            for (x0, x1, y1) in [(door, door + 1.1, 29.0), (x - 3.4, x, 29.8)] {
                let foot = |x: f64| DVec3::new(x, 26.95, ground_at(x, 26.6).max(ground_at(x, 27.2)) + 0.05);
                let quad = [foot(x0), foot(x1), DVec3::new(x1, y1, level + 0.04), DVec3::new(x0, y1, level + 0.04)];
                prism(e, quad, -DVec3::Z * 0.6, p.paving)?;
            }
            Ok(())
        })?;
    }

    // Stepping stones across the lawn: irregular sizes, turns and spacing.
    batch(&mut e, &base, |e| {
        let mut stones = Random(77);
        for i in 0..8 {
            let t = i as f64;
            let c =
                DVec2::new(-1.1 + t * 1.05 + stones.range(-0.18, 0.18), -11.2 - t * 0.85 + stones.range(-0.15, 0.15));
            let (w, d) = (stones.range(0.62, 0.95), stones.range(0.42, 0.62));
            ground_slab(e, c, w, d, stones.range(-35., 35.), &ground_at, p.paving)?;
        }
        for (x, y, size) in
            [(-12., -12.5, 1.), (-9., -13.5, 0.8), (1., -15.5, 0.9), (19., -2., 1.1), (-3., 7.2, 0.9), (3., 7.3, 1.)]
        {
            bush(e, DVec3::new(x, y, ground_at(x, y)), size, &p)?;
        }

        // A strip of washed gravel where the raised pool surround meets the lawn, with loose
        // pebbles and clumps of ornamental grass.
        let mut random = Random(808);
        let strip = |e: &mut Editor, a: DVec2, b: DVec2, across: DVec2| -> Result<()> {
            let length = a.distance(b);
            let steps = (length / 0.5).ceil() as usize;
            for k in 0..steps {
                let (s0, s1) = (a.lerp(b, k as f64 / steps as f64), a.lerp(b, (k + 1) as f64 / steps as f64));
                // Level across the strip, just above the higher ground at each end.
                let top = |q: DVec2| ground_at(q.x, q.y).max(ground_at(q.x + across.x, q.y + across.y)) + 0.02;
                let (z0, z1) = (top(s0), top(s1));
                let quad = [s0.extend(z0), s1.extend(z1), (s1 + across).extend(z1), (s0 + across).extend(z0)];
                prism(e, quad, -DVec3::Z * 0.3, p.gravel)?;
            }
            Ok(())
        };
        strip(e, DVec2::new(4.0, -15.0), DVec2::new(19.0, -15.0), DVec2::new(0., -0.55))?;
        strip(e, DVec2::new(18.5, -15.0), DVec2::new(18.5, -4.42), DVec2::new(0.55, 0.))?;
        strip(e, DVec2::new(4.5, -15.0), DVec2::new(4.5, -10.6), DVec2::new(-0.5, 0.))?;
        for _ in 0..70 {
            let (x, y) = if random.next() < 0.6 {
                (random.range(4.1, 18.9), random.range(-15.5, -15.05))
            } else {
                (random.range(18.55, 19.0), random.range(-15.4, -4.5))
            };
            let r = random.range(0.03, 0.07);
            let colour = if random.next() < 0.5 { p.coping } else { p.plinth };
            clump(
                e,
                DVec3::new(x, y, ground_at(x, y) + r * 0.4),
                DVec3::new(r, r * 1.2, r * 0.6),
                &mut random,
                colour,
            )?;
        }
        for k in 0..7 {
            let x = 5.2 + k as f64 * 2.15 + random.range(-0.3, 0.3);
            planting(e, x, -15.27, 1, &ground_at, &mut random, &p)?;
            planting(e, x + 0.7, -15.3, 0, &ground_at, &mut random, &p)?;
        }
        for k in 0..4 {
            let y = -13.6 + k as f64 * 2.6;
            planting(e, 18.77, y, 1, &ground_at, &mut random, &p)?;
            planting(e, 18.78, y + 0.9, 0, &ground_at, &mut random, &p)?;
        }
        // Borders of grasses and perennials inside the front wall and along the back wall,
        // tufts along the side fences and the front of the deck.
        let mut x = -39.3;
        while x < 39.5 {
            if !(-16.0..-2.6).contains(&x) {
                planting(
                    e,
                    x,
                    -29.95 + random.range(-0.15, 0.15),
                    if random.next() < 0.45 { 1 } else { 2 },
                    &ground_at,
                    &mut random,
                    &p,
                )?;
            }
            x += random.range(1.1, 1.6);
        }
        let mut x = -21.;
        while x < 19.5 {
            if !(-14.6..-8.8).contains(&x) {
                planting(e, x, 7.55, if random.next() < 0.35 { 1 } else { 2 }, &ground_at, &mut random, &p)?;
            }
            x += random.range(1.2, 1.8);
        }
        for fx in [-39.75, 39.75] {
            let mut y = -29.8;
            while y < 7.8 {
                planting(e, fx, y, if random.next() < 0.15 { 1 } else { 0 }, &ground_at, &mut random, &p)?;
                y += random.range(1.0, 2.0);
            }
        }
        let mut x = -13.6;
        while x < 3.8 {
            planting(e, x, -10.75, 0, &ground_at, &mut random, &p)?;
            x += random.range(0.9, 1.6);
        }

        // Leaves blown across the lawn and the terrace, and pumpkins by the steps.
        let mut random = Random(2026);
        for _ in 0..140 {
            let (x, y) = (random.range(-16., 20.), random.range(-26., -4.6));
            let on_deck = (-14.0..4.0).contains(&x) && (-10.0..-4.4).contains(&y);
            // The pool and its stone surround stay clear.
            if (4.5..18.5).contains(&x) && (-15.0..-4.4).contains(&y) {
                continue;
            }
            let z = if on_deck { 0.25 } else { ground_at(x, y) + 0.01 };
            let colour = p.autumn[(random.next() * p.autumn.len() as f64) as usize];
            leaf(e, DVec3::new(x, y, z), &mut random, colour)?;
        }
        for (x, y, size) in [(4.6, -3.3, 1.), (5.1, -3.0, 0.8), (-14.6, -5.2, 0.9), (-15.0, -4.6, 0.7)] {
            let z = if x > 0. { 0.06 } else { ground_at(x, y) };
            pumpkin(e, DVec3::new(x, y, z), size, &p)?;
        }

        // A fire pit with two benches on the lawn and a swept leaf pile.
        fire_pit(e, DVec3::new(-7., -15., ground_at(-7., -15.)), &p)?;
        for (x, y, facing) in [(-7., -16.9, 180.), (-8.9, -15., 90.)] {
            let (x, y) = (x + jit.range(-0.1, 0.1), y + jit.range(-0.1, 0.1));
            bench(e, DVec3::new(x, y, ground_at(x, y)), facing + jit.range(-8., 8.), &p)?;
        }
        leaf_pile(e, DVec3::new(-5.5, -20.5, ground_at(-5.5, -20.5)), &p)?;
        Ok(())
    })?;

    // Trees around the garden, on the hills and in the neighbouring gardens.
    for (x, y, size) in [
        (-25., -6., 1.1),
        (-27., 2., 0.9),
        (-24., 14., 1.2),
        (10., 14., 1.),
        (26., 10., 1.3),
        (22., -22., 1.1),
        (-8., -22., 0.8),
        (-30., -18., 1.),
        (-4., 16., 0.9),
        (-52., 4., 1.2),
        (60., 2., 1.1),
        (-68., -66., 1.0),
        (34., -64., 1.2),
        (-23., -67., 0.9),
    ] {
        batch(&mut e, &base, |e| {
            tree(e, DVec3::new(x, y, ground_at(x, y)), size, &ground_at, &p)?;
            // Uncut grass round the trunk.
            let mut random = Random((x * 31. + y * 17.).abs() as u64);
            for _ in 0..7 {
                let (a, r) = (random.range(0., std::f64::consts::TAU), random.range(0.4, 1.6));
                planting(e, x + a.cos() * r, y + a.sin() * r, 0, &ground_at, &mut random, &p)?;
            }
            Ok(())
        })?;
    }
    for (x, y, size) in [
        (-14., 15., 1.3),
        (-18., 18., 1.1),
        (16., 21., 1.2),
        (20., 17., 1.),
        (31., 2., 1.1),
        (-36., -2., 1.2),
        (-60., 20., 1.3),
        (70., 30., 1.2),
        (8., -66., 1.1),
    ] {
        batch(&mut e, &base, |e| conifer(e, DVec3::new(x, y, ground_at(x, y)), size, &p))?;
    }

    // Street trees in the pavement opposite, and rows of small trees and conifers inside the
    // front wall and along the side fences.
    for k in 0..11 {
        let x = -150. + k as f64 * 30.;
        batch(&mut e, &base, |e| {
            tree(
                e,
                DVec3::new(x, -41.3, 0.14),
                0.75,
                &|x, y| {
                    if (-42.3..=-40.15).contains(&y) {
                        0.14
                    } else if (-40.0..=-33.5).contains(&y) {
                        0.04
                    } else {
                        ground_at(x, y)
                    }
                },
                &p,
            )
        })?;
    }
    for (x, size) in
        [(-36., 0.8), (-30., 0.7), (-24., 0.85), (8., 0.75), (14., 0.8), (21., 0.7), (28., 0.85), (35., 0.75)]
    {
        batch(&mut e, &base, |e| tree(e, DVec3::new(x, -28.6, ground_at(x, -28.6)), size, &ground_at, &p))?;
    }
    for y in [-24., -15., -4., 4.] {
        for x in [-38.4, 38.4] {
            let size = 0.8 + 0.1 * ((x + y) * 0.37f64).sin();
            batch(&mut e, &base, |e| conifer(e, DVec3::new(x, y, ground_at(x, y)), size, &p))?;
        }
    }

    // Woods and hedgerow trees across the surrounding fields, away from the houses.
    let mut wood = Random(4242);
    let mut planted = 0;
    while planted < 80 {
        let (x, y) = (wood.range(-150., 150.), wood.range(-130., 130.));
        let built_up = (-100.0..100.0).contains(&x) && (-75.0..14.0).contains(&y)
            || (-118.0..115.0).contains(&x) && (14.0..52.0).contains(&y);
        let road = (-34.0..-28.0).contains(&y)
            || (-48.0..-28.0).contains(&y) && x.abs() > 95.
            || (96.0..112.0).contains(&x.abs()) && (-36.0..32.0).contains(&y);
        if built_up || road {
            continue;
        }
        let size = wood.range(0.9, 1.4);
        let foot = DVec3::new(x, y, ground_at(x, y));
        if wood.next() < 0.45 {
            batch(&mut e, &base, |e| conifer(e, foot, size, &p))?;
        } else {
            batch(&mut e, &base, |e| tree(e, foot, size, &ground_at, &p))?;
        }
        planted += 1;
    }

    // A CC0 autumn sky panorama as the background and the light of the sky.
    e.apply(Command::SetEnvironment {
        name: "autumn_field_puresky_2k.hdr".into(),
        mime: "image/vnd.radiance".into(),
        data: SKY.to_vec(),
    })?;
    let mut doc = e.doc().clone();
    doc.style.lighting = rysmo_model::Lighting::Sun;
    doc.style.environment_background = true;
    doc.style.shadows = true;
    doc.style.ambient_occlusion = true;
    doc.style.edges = false;
    doc.style.tone_mapping = rysmo_model::ToneMapping::AgX;
    // The panorama lights the shade strongly, so the sun is turned up and the sky's fill
    // down for shadows that read; the exposure then suits the sunlit model.
    doc.style.sun_intensity = 4.;
    doc.style.ambient_intensity = 0.25;
    doc.style.exposure = std::env::var("DEMO_EXPOSURE").ok().and_then(|x| x.parse().ok()).unwrap_or(0.);
    doc.style.atmosphere = true;
    doc.style.ground = true;
    doc.style.ground_color = [0.3, 0.31, 0.17];
    // A low autumn afternoon sun with the lamps already on.
    doc.style.sun.day = 290;
    doc.style.sun.hour = std::env::var("DEMO_HOUR").ok().and_then(|h| h.parse().ok()).unwrap_or(HOUR);
    // North turned so the afternoon sun comes from behind the hero camera's left and rakes
    // across the garden front.
    let (azimuth, _) = doc.style.sun.position();
    doc.style.sun.north = (azimuth + 120.5).rem_euclid(360.);
    // Turn the panorama so its sun stands at that bearing, then put the sun where the
    // panorama shows it, so the shadows agree with the sky.
    let (bearing, elevation) = panorama_sun(SKY)?;
    doc.style.studio_rotation = (bearing + doc.style.sun.north - azimuth + 180.).rem_euclid(360.) as f32 - 180.;
    doc.style.sun.from_time = false;
    doc.style.sun.azimuth = azimuth;
    doc.style.sun.elevation = elevation;
    doc.camera = Some(OrbitCamera {
        target: DVec3::new(-1., -2., 3.),
        distance: 38.,
        yaw: -1.25,
        pitch: 0.22,
        fov_y: 0.7,
        parallel: false,
        eye_base_z: 0.,
        roll: 0.,
    });
    std::fs::write(&path, rysmo_io::save(&doc))?;
    Ok(())
}
