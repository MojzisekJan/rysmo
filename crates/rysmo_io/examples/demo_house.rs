//! Build the demo model for the README screenshot: a modern villa with a pool on hilly terrain.
#[path = "shared/cybertruck.rs"]
mod cybertruck;

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
    cushion: MaterialId,
    oak: MaterialId,
    rug: MaterialId,
    solar: MaterialId,
    parasol: MaterialId,
    lamp: MaterialId,
}

fn material(e: &mut Editor, name: &str, color: [f32; 4]) -> Result<MaterialId> {
    let out = e.apply(Command::AddMaterial { name: name.into(), color, texture: None })?;
    Ok(out.material.ok_or("no material")?)
}

/// A material with its surface roughness and metalness.
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

/// A pendant lamp: a cord from the ceiling and a glowing shade.
fn pendant(e: &mut Editor, at: DVec3, ceiling: f64, p: &Paints) -> Result<()> {
    column(e, at + DVec3::Z * 0.2, 0.012, ceiling - at.z - 0.2, 6, p.dark)?;
    column(e, at, 0.16, 0.2, 12, p.lamp)?;
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

/// A box whose vertical edges are rounded with `radius`, as its own group.
fn rounded(e: &mut Editor, a: DVec3, b: DVec3, radius: f64, colour: MaterialId) -> Result<InstanceId> {
    let (min, max) = (a.min(b), a.max(b));
    let size = max - min;
    let radius = radius.min(size.x.min(size.y) * 0.45);
    e.apply(Command::DrawRectangle { corner: min, u: DVec3::X * size.x, v: DVec3::Y * size.y, facing: None })?;
    // Fillet each square corner; the arcs meet the remaining sides at shallow angles.
    for _ in 0..4 {
        let mesh = e.mesh();
        let corner = mesh.vertices().find_map(|(v, _)| {
            let edges = mesh.edges_of_vertex(v);
            let [first, second] = edges.as_slice() else { return None };
            let direction = |edge| {
                let (p, q) = mesh.edge_points(edge);
                (q - p).normalize()
            };
            (direction(*first).dot(direction(*second)).abs() < 1e-6).then_some([*first, *second])
        });
        let Some(edges) = corner else { break };
        e.apply(Command::FilletEdges { edges, radius, segments: 4 })?;
    }
    extrude(e, size.z, colour)
}

/// A vertical regular prism (posts, pots, stools).
fn column(e: &mut Editor, foot: DVec3, radius: f64, height: f64, sides: u32, colour: MaterialId) -> Result<InstanceId> {
    e.apply(Command::DrawPolygon { centre: foot, normal: DVec3::Z, radius, segments: sides, start: None })?;
    extrude(e, height, colour)
}

/// Turns groups about a vertical axis through `centre`.
fn turn(e: &mut Editor, groups: &[InstanceId], centre: DVec3, degrees: f64) -> Result<()> {
    if degrees != 0. {
        let elements = groups.iter().map(|g| Element::Instance(*g)).collect();
        e.apply(Command::Rotate { elements, centre, axis: DVec3::Z, angle: degrees, copies: 0 })?;
    }
    Ok(())
}

/// The cap of the loose solid facing up (`1.`) or down (`-1.`).
fn cap(e: &Editor, direction: f64) -> Result<rysmo_model::FaceId> {
    let mesh = e.mesh();
    Ok(mesh
        .faces()
        .map(|(f, _)| f)
        .find(|f| mesh.face_normal(*f).is_some_and(|n| n.z * direction > 0.99))
        .ok_or("no cap")?)
}

/// Scales the cap facing `direction` about its centre.
fn taper(e: &mut Editor, direction: f64, factor: f64) -> Result<()> {
    let face = cap(e, direction)?;
    let mesh = e.mesh();
    let points: Vec<DVec3> = mesh.face(face).ok_or("no face")?.outer.iter().map(|v| mesh.pos(*v)).collect();
    let middle = points.iter().copied().sum::<DVec3>() / points.len() as f64;
    e.apply(Command::Scale {
        elements: vec![Element::Face(face)],
        origin: middle,
        factors: DVec3::new(factor, factor, 1.),
    })?;
    Ok(())
}

/// A low-poly crown: a prism tapered at both ends.
fn crown(e: &mut Editor, centre: DVec3, radius: f64, up: f64, down: f64, sides: u32, colour: MaterialId) -> Result<()> {
    e.apply(Command::DrawPolygon { centre, normal: DVec3::Z, radius, segments: sides, start: None })?;
    let face = cap(e, 1.).or_else(|_| cap(e, -1.))?;
    let sign = e.mesh().face_normal(face).ok_or("no normal")?.z.signum();
    e.apply(Command::PushPull { face, distance: up * sign, keep_base: false })?;
    taper(e, 1., 0.3)?;
    if down > 0. {
        let bottom = cap(e, -1.)?;
        e.apply(Command::PushPull { face: bottom, distance: down, keep_base: false })?;
        taper(e, -1., 0.3)?;
    }
    let group = group_loose(e)?;
    // Smooth shading turns the faceted prism into a soft, organic mass.
    e.enter(group)?;
    let edges = e.mesh().edges().map(|(edge, _)| edge).collect();
    e.apply(Command::SetEdges { edges, soft: Some(true), smooth: Some(true), hidden: None })?;
    e.exit();
    paint(e, group, colour)?;
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

fn tree(e: &mut Editor, foot: DVec3, size: f64, p: &Paints) -> Result<()> {
    column(e, foot, 0.12 * size, 2.4 * size, 6, p.bark)?;
    // A main crown with two smaller clusters, in neighbouring autumn colours.
    let first = pick(foot, p.autumn.len());
    crown(e, foot + DVec3::Z * 3.4 * size, 1.6 * size, 2.2 * size, 1.4 * size, 14, p.autumn[first])?;
    for (k, (dx, dy, dz, r)) in [(0.9, 0.5, 2.9, 0.95), (-0.8, -0.6, 3.6, 0.85)].into_iter().enumerate() {
        let colour = p.autumn[(first + k + 1) % p.autumn.len()];
        let at = foot + DVec3::new(dx, dy, dz) * size;
        crown(e, at, r * size, 1.3 * size, 0.8 * size, 12, colour)?;
    }
    // Fallen leaves scattered under the crown, denser near the trunk.
    let mut random = Random((foot.x * 1000.) as i64 as u64 ^ ((foot.y * 1000.) as i64 as u64).rotate_left(32));
    for _ in 0..18 {
        let angle = random.range(0., std::f64::consts::TAU);
        let reach = random.next().sqrt() * 2.4 * size;
        let at = foot + DVec3::new(angle.cos() * reach, angle.sin() * reach, 0.12);
        let colour = p.autumn[(random.next() * p.autumn.len() as f64) as usize];
        leaf(e, at, &mut random, colour)?;
    }
    Ok(())
}

/// A fallen leaf: a small flat hexagon with a random size and turn.
fn leaf(e: &mut Editor, at: DVec3, random: &mut Random, colour: MaterialId) -> Result<()> {
    let turn = random.range(0., std::f64::consts::TAU);
    let start = Some(DVec3::new(turn.cos(), turn.sin(), 0.));
    let radius = random.range(0.06, 0.13);
    e.apply(Command::DrawPolygon { centre: at, normal: DVec3::Z, radius, segments: 6, start })?;
    extrude(e, 0.01, colour)?;
    Ok(())
}

/// A pumpkin: a ribbed, flattened low-poly crown with a stalk.
fn pumpkin(e: &mut Editor, foot: DVec3, size: f64, p: &Paints) -> Result<()> {
    crown(e, foot + DVec3::Z * 0.12 * size, 0.24 * size, 0.18 * size, 0.12 * size, 16, p.pumpkin)?;
    column(e, foot + DVec3::Z * 0.28 * size, 0.025 * size, 0.07 * size, 5, p.bark)?;
    Ok(())
}

fn conifer(e: &mut Editor, foot: DVec3, size: f64, p: &Paints) -> Result<()> {
    column(e, foot, 0.1 * size, 1.2 * size, 6, p.bark)?;
    for (level, radius) in [(0.8, 1.5), (2.2, 1.15), (3.4, 0.8)] {
        e.apply(Command::DrawPolygon {
            centre: foot + DVec3::Z * level * size,
            normal: DVec3::Z,
            radius: radius * size,
            segments: 14,
            start: None,
        })?;
        let face = cap(e, 1.).or_else(|_| cap(e, -1.))?;
        let sign = e.mesh().face_normal(face).ok_or("no normal")?.z.signum();
        e.apply(Command::PushPull { face, distance: 1.6 * size * sign, keep_base: false })?;
        taper(e, 1., 0.08)?;
        let group = group_loose(e)?;
        e.enter(group)?;
        let edges = e.mesh().edges().map(|(edge, _)| edge).collect();
        e.apply(Command::SetEdges { edges, soft: Some(true), smooth: Some(true), hidden: None })?;
        e.exit();
        paint(e, group, p.conifer)?;
    }
    Ok(())
}

fn bush(e: &mut Editor, foot: DVec3, size: f64, p: &Paints) -> Result<()> {
    // Some shrubs turn red in autumn, the rest stay olive.
    let colour = if pick(foot, 3) == 0 { p.autumn[1] } else { p.hedge };
    crown(e, foot + DVec3::Z * 0.25 * size, 0.7 * size, 0.6 * size, 0.2 * size, 12, colour)
}

/// Boards laid side by side across a rectangle, running along X or Y.
fn boards(e: &mut Editor, min: DVec3, max: DVec3, along_x: bool, width: f64, colour: MaterialId) -> Result<()> {
    let span = if along_x { max.y - min.y } else { max.x - min.x };
    let count = (span / width).floor() as usize;
    for i in 0..count {
        let s = i as f64 * width;
        let (a, b) = if along_x {
            (DVec3::new(min.x, min.y + s, min.z), DVec3::new(max.x, min.y + s + width - 0.015, max.z))
        } else {
            (DVec3::new(min.x + s, min.y, min.z), DVec3::new(min.x + s + width - 0.015, max.y, max.z))
        };
        let board = cuboid(e, a, b, colour)?;
        e.enter(board)?;
        let edges = e.mesh().edges().map(|(edge, _)| edge).collect();
        e.apply(Command::SetEdges { edges, soft: Some(true), smooth: Some(false), hidden: None })?;
        e.exit();
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
    cuboid(e, at(0., z0 + 0.06, -0.2), at(length, z1 - 0.06, 0.2), glass)?;
    let bar = 0.06;
    cuboid(e, at(0., z0, -1.), at(length, z0 + bar, 1.), p.dark)?;
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
    // The group sits at the world origin, so its local coordinates are world coordinates.
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
    Ok(())
}

/// A wall along `a`-`b` in plan, `thickness` towards `inward`, with framed glass
/// openings `(from, width, sill, height, panes)` measured from `a`.
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
        glazing(
            e,
            middle + along * from,
            middle + along * (from + width),
            (z0 + sill)..=(z0 + sill + height),
            panes,
            p.clear,
            p,
        )?;
        cursor = from + width;
    }
    piece(e, cursor, length, z0, z1)
}

fn bed(e: &mut Editor, min: DVec2, max: DVec2, p: &Paints) -> Result<()> {
    cuboid(e, min.extend(0.), max.extend(0.3), p.oak)?;
    rounded(e, (min + DVec2::splat(0.05)).extend(0.3), (max - DVec2::splat(0.05)).extend(0.52), 0.08, p.cushion)?;
    cuboid(e, DVec3::new(min.x + 0.05, min.y + 0.05, 0.52), DVec3::new(max.x - 0.05, min.y + 1.2, 0.56), p.rug)?;
    for x in [min.x + 0.2, (min.x + max.x) / 2. + 0.05] {
        rounded(e, DVec3::new(x, max.y - 0.6, 0.52), DVec3::new(x + 0.65, max.y - 0.15, 0.66), 0.1, p.cushion)?;
    }
    cuboid(e, DVec3::new(min.x - 0.2, max.y, 0.), DVec3::new(max.x + 0.2, max.y + 0.1, 1.1), p.fabric)?;
    Ok(())
}

fn chair(e: &mut Editor, at: DVec3, degrees: f64, p: &Paints) -> Result<()> {
    let mut parts = Vec::new();
    for (dx, dy) in [(-0.2, -0.2), (0.17, -0.2), (-0.2, 0.17), (0.17, 0.17)] {
        parts.push(cuboid(e, at + DVec3::new(dx, dy, 0.), at + DVec3::new(dx + 0.03, dy + 0.03, 0.45), p.steel)?);
    }
    parts.push(rounded(e, at + DVec3::new(-0.22, -0.22, 0.45), at + DVec3::new(0.22, 0.22, 0.5), 0.05, p.oak)?);
    parts.push(rounded(e, at + DVec3::new(-0.22, 0.17, 0.55), at + DVec3::new(0.22, 0.22, 0.9), 0.02, p.oak)?);
    for x in [-0.2, 0.17] {
        parts.push(cuboid(e, at + DVec3::new(x, 0.17, 0.5), at + DVec3::new(x + 0.03, 0.2, 0.55), p.steel)?);
    }
    turn(e, &parts, at, degrees)
}

fn table(e: &mut Editor, min: DVec2, max: DVec2, height: f64, top: MaterialId, p: &Paints) -> Result<()> {
    for (x, y) in [
        (min.x + 0.1, min.y + 0.1),
        (max.x - 0.16, min.y + 0.1),
        (min.x + 0.1, max.y - 0.16),
        (max.x - 0.16, max.y - 0.16),
    ] {
        cuboid(e, DVec3::new(x, y, 0.), DVec3::new(x + 0.06, y + 0.06, height - 0.04), p.steel)?;
    }
    rounded(e, min.extend(height - 0.04), max.extend(height), 0.06, top)?;
    Ok(())
}

/// Lifts everything made by `build` by `dz`.
fn raised(e: &mut Editor, dz: f64, build: impl FnOnce(&mut Editor) -> Result<()>) -> Result<()> {
    let before: std::collections::BTreeSet<InstanceId> = e.active().instances.keys().copied().collect();
    build(e)?;
    let new: Vec<Element> =
        e.active().instances.keys().filter(|i| !before.contains(i)).map(|i| Element::Instance(*i)).collect();
    e.apply(Command::Move { elements: new, offset: DVec3::Z * dz })?;
    Ok(())
}

fn lounger(e: &mut Editor, at: DVec3, degrees: f64, p: &Paints) -> Result<()> {
    let mut parts = Vec::new();
    // Slim frame on four legs, with slats under the mattress.
    for (x, y) in [(-0.33, -0.93), (0.29, -0.93), (-0.33, 0.89), (0.29, 0.89)] {
        parts.push(cuboid(e, at + DVec3::new(x, y, 0.), at + DVec3::new(x + 0.04, y + 0.04, 0.26), p.steel)?);
    }
    for x in [-0.35, 0.31] {
        parts.push(cuboid(e, at + DVec3::new(x, -0.95, 0.22), at + DVec3::new(x + 0.04, 0.95, 0.28), p.steel)?);
    }
    for i in 0..9 {
        let y = -0.9 + i as f64 * 0.15;
        parts.push(cuboid(e, at + DVec3::new(-0.31, y, 0.26), at + DVec3::new(0.31, y + 0.1, 0.28), p.oak)?);
    }
    parts.push(rounded(e, at + DVec3::new(-0.32, -0.92, 0.28), at + DVec3::new(0.32, 0.4, 0.36), 0.08, p.cushion)?);
    let back = rounded(e, at + DVec3::new(-0.32, 0.4, 0.28), at + DVec3::new(0.32, 0.95, 0.36), 0.08, p.cushion)?;
    e.apply(Command::Rotate {
        elements: vec![Element::Instance(back)],
        centre: at + DVec3::new(0., 0.4, 0.32),
        axis: DVec3::X,
        angle: 50.,
        copies: 0,
    })?;
    parts.push(back);
    turn(e, &parts, at, degrees)
}

fn sofa(e: &mut Editor, min: DVec2, max: DVec2, back_side: DVec2, p: &Paints) -> Result<()> {
    rounded(e, min.extend(0.), max.extend(0.42), 0.1, p.fabric)?;
    let (b0, b1) = if back_side.y > 0. {
        (DVec2::new(min.x, max.y - 0.2), max)
    } else if back_side.y < 0. {
        (min, DVec2::new(max.x, min.y + 0.2))
    } else if back_side.x > 0. {
        (DVec2::new(max.x - 0.2, min.y), max)
    } else {
        (min, DVec2::new(min.x + 0.2, max.y))
    };
    rounded(e, b0.extend(0.42), b1.extend(0.8), 0.08, p.fabric)?;
    Ok(())
}

/// A horizontal log along X or Y, its foot at `at`.
fn log(e: &mut Editor, at: DVec3, radius: f64, length: f64, along_x: bool, colour: MaterialId) -> Result<()> {
    let normal = if along_x { DVec3::X } else { DVec3::Y };
    e.apply(Command::DrawPolygon { centre: at + DVec3::Z * radius, normal, radius, segments: 7, start: None })?;
    let mesh = e.mesh();
    let face = mesh.faces().next().ok_or("no log")?.0;
    let sign = mesh.face_normal(face).ok_or("no normal")?.dot(normal).signum();
    e.apply(Command::PushPull { face, distance: length * sign, keep_base: false })?;
    let group = group_loose(e)?;
    paint(e, group, colour)?;
    Ok(())
}

/// A ring of stones around glowing embers and a few logs.
fn fire_pit(e: &mut Editor, at: DVec3, p: &Paints) -> Result<()> {
    for i in 0..10 {
        let a = i as f64 * std::f64::consts::TAU / 10.;
        crown(e, at + DVec3::new(a.cos() * 0.6, a.sin() * 0.6, 0.12), 0.16, 0.14, 0.1, 6, p.stone)?;
    }
    column(e, at, 0.42, 0.06, 10, p.embers)?;
    log(e, at + DVec3::new(-0.35, -0.12, 0.06), 0.06, 0.7, true, p.bark)?;
    log(e, at + DVec3::new(0.1, -0.35, 0.1), 0.05, 0.6, false, p.bark)?;
    Ok(())
}

/// A garden bench with a rounded seat and its back on +Y, so it faces -Y before turning.
fn bench(e: &mut Editor, at: DVec3, degrees: f64, p: &Paints) -> Result<()> {
    let mut parts = Vec::new();
    for x in [-0.7, 0.62] {
        parts.push(cuboid(e, at + DVec3::new(x, -0.2, 0.), at + DVec3::new(x + 0.08, 0.2, 0.42), p.dark)?);
    }
    parts.push(rounded(e, at + DVec3::new(-0.8, -0.22, 0.42), at + DVec3::new(0.8, 0.22, 0.47), 0.05, p.oak)?);
    parts.push(rounded(e, at + DVec3::new(-0.8, 0.18, 0.6), at + DVec3::new(0.8, 0.24, 0.85), 0.05, p.oak)?);
    turn(e, &parts, at, degrees)
}

/// A swept heap of leaves with a rake lying beside it.
fn leaf_pile(e: &mut Editor, at: DVec3, p: &Paints) -> Result<()> {
    crown(e, at + DVec3::Z * 0.02, 0.7, 0.35, 0., 9, p.autumn[0])?;
    crown(e, at + DVec3::new(0.5, 0.3, 0.02), 0.45, 0.25, 0., 8, p.autumn[2])?;
    cuboid(e, at + DVec3::new(0.9, -0.9, 0.), at + DVec3::new(0.93, 0.6, 0.03), p.oak)?;
    cuboid(e, at + DVec3::new(0.72, -1.0, 0.), at + DVec3::new(1.11, -0.94, 0.05), p.steel)?;
    Ok(())
}

/// Split logs stacked against a wall, with a small roof.
fn woodpile(e: &mut Editor, min: DVec2, length: f64, p: &Paints) -> Result<()> {
    for row in 0..5 {
        for i in 0..((length / 0.2) as usize) {
            let shift = if row % 2 == 0 { 0. } else { 0.1 };
            let y = min.y + shift + i as f64 * 0.2;
            if y + 0.2 > min.y + length {
                continue;
            }
            log(e, DVec3::new(min.x, y + 0.1, row as f64 * 0.17), 0.09, 0.5, true, p.firewood)?;
        }
    }
    cuboid(e, DVec3::new(min.x - 0.1, min.y - 0.1, 0.9), DVec3::new(min.x + 0.65, min.y + length + 0.1, 0.95), p.dark)?;
    Ok(())
}

/// A post-mounted letterbox.
fn letterbox(e: &mut Editor, at: DVec3, p: &Paints) -> Result<()> {
    cuboid(e, at + DVec3::new(-0.04, -0.04, 0.), at + DVec3::new(0.04, 0.04, 1.0), p.dark)?;
    rounded(e, at + DVec3::new(-0.2, -0.14, 1.0), at + DVec3::new(0.2, 0.14, 1.32), 0.06, p.taillight)?;
    Ok(())
}

fn parasol(e: &mut Editor, at: DVec3, p: &Paints) -> Result<()> {
    column(e, at, 0.25, 0.08, 16, p.steel)?;
    column(e, at + DVec3::Z * 2.45, 0.04, 0.08, 8, p.steel)?;
    column(e, at, 0.03, 2.3, 6, p.steel)?;
    e.apply(Command::DrawPolygon {
        centre: at + DVec3::Z * 2.0,
        normal: DVec3::Z,
        radius: 1.5,
        segments: 8,
        start: None,
    })?;
    let face = cap(e, 1.).or_else(|_| cap(e, -1.))?;
    let sign = e.mesh().face_normal(face).ok_or("no normal")?.z.signum();
    e.apply(Command::PushPull { face, distance: 0.45 * sign, keep_base: false })?;
    taper(e, 1., 0.05)?;
    let canopy = group_loose(e)?;
    paint(e, canopy, p.parasol)?;
    Ok(())
}

/// The time of day; `DEMO_HOUR` overrides it.
const HOUR: f64 = 16.4;

fn main() -> Result<()> {
    let path = std::env::args().nth(1).ok_or("Usage: demo_house output.rysmo")?;
    let mut e = Editor::new();
    let grass = surface(&mut e, "Autumn grass", [0.5, 0.52, 0.28, 1.], 0.98, 0.)?;
    let p = Paints {
        render: surface(&mut e, "White render", [0.94, 0.93, 0.9, 1.], 0.9, 0.0)?,
        concrete: surface(&mut e, "Concrete", [0.74, 0.73, 0.7, 1.], 0.9, 0.0)?,
        wood: surface(&mut e, "Larch cladding", [0.6, 0.4, 0.24, 1.], 0.8, 0.0)?,
        deck: surface(&mut e, "Decking", [0.55, 0.38, 0.25, 1.], 0.75, 0.0)?,
        clear: surface(&mut e, "Clear glass", [0.7, 0.82, 0.88, 0.22], 0.03, 0.0)?,
        tinted: surface(&mut e, "Tinted glass", [0.24, 0.32, 0.38, 0.85], 0.06, 0.0)?,
        railing: surface(&mut e, "Railing glass", [0.7, 0.85, 0.9, 0.3], 0.03, 0.0)?,
        dark: surface(&mut e, "Anthracite", [0.18, 0.19, 0.21, 1.], 0.55, 0.2)?,
        steel: surface(&mut e, "Steel", [0.3, 0.31, 0.33, 1.], 0.35, 0.9)?,
        stone: surface(&mut e, "Stone", [0.62, 0.59, 0.52, 1.], 0.85, 0.0)?,
        water: surface(&mut e, "Water", [0.25, 0.62, 0.8, 0.9], 0.02, 0.0)?,
        paving: surface(&mut e, "Paving", [0.52, 0.52, 0.5, 1.], 0.9, 0.0)?,
        asphalt: surface(&mut e, "Asphalt", [0.17, 0.17, 0.18, 1.], 0.95, 0.0)?,
        marking: material(&mut e, "Yellow road marking", [0.95, 0.75, 0.12, 1.])?,
        embers: glowing(&mut e, "Embers", [1., 0.42, 0.12, 1.], 20000.)?,
        firewood: material(&mut e, "Firewood", [0.62, 0.45, 0.3, 1.])?,
        coping: surface(&mut e, "Coping stone", [0.82, 0.8, 0.75, 1.], 0.75, 0.0)?,
        tile: surface(&mut e, "Waterline tile", [0.12, 0.33, 0.5, 1.], 0.15, 0.0)?,
        chrome: surface(&mut e, "Chrome", [0.78, 0.8, 0.82, 1.], 0.12, 1.0)?,
        taillight: material(&mut e, "Tail light", [0.7, 0.06, 0.05, 1.])?,
        bark: material(&mut e, "Bark", [0.35, 0.26, 0.18, 1.])?,
        autumn: [
            material(&mut e, "Orange leaves", [0.86, 0.45, 0.13, 1.])?,
            material(&mut e, "Red leaves", [0.66, 0.17, 0.1, 1.])?,
            material(&mut e, "Golden leaves", [0.9, 0.68, 0.2, 1.])?,
            material(&mut e, "Rust leaves", [0.55, 0.28, 0.12, 1.])?,
        ],
        pumpkin: material(&mut e, "Pumpkin", [0.92, 0.48, 0.1, 1.])?,
        conifer: material(&mut e, "Spruce", [0.18, 0.34, 0.2, 1.])?,
        hedge: material(&mut e, "Hedge", [0.38, 0.4, 0.18, 1.])?,
        fabric: material(&mut e, "Sofa fabric", [0.55, 0.56, 0.58, 1.])?,
        cushion: material(&mut e, "Cushion", [0.9, 0.88, 0.82, 1.])?,
        oak: surface(&mut e, "Oak", [0.72, 0.56, 0.38, 1.], 0.55, 0.0)?,
        rug: material(&mut e, "Rug", [0.66, 0.42, 0.3, 1.])?,
        solar: surface(&mut e, "Solar panel", [0.12, 0.17, 0.3, 1.], 0.12, 0.3)?,
        parasol: material(&mut e, "Parasol", [0.95, 0.94, 0.9, 1.])?,
        lamp: glowing(&mut e, "Warm lamp", [1., 0.8, 0.55, 1.], 30000.)?,
    };

    // Terrain 80 x 64 m: hills behind and to the sides, a flat pad for the house and garden.
    let origin = DVec3::new(-40., -30., 0.);
    let out = e.apply(Command::CreateTerrainGrid { origin, u: DVec3::X * 80., v: DVec3::Y * 64., spacing: 2. })?;
    let Some(Element::Instance(terrain)) = out.created.first().copied() else { return Err("no terrain".into()) };
    e.enter(terrain)?;
    for (centre, radius, height) in [
        (DVec2::new(-10., 30.), 24., 7.),
        (DVec2::new(22., 28.), 18., 5.),
        (DVec2::new(-36., 4.), 14., 3.5),
        (DVec2::new(36., -4.), 12., 2.5),
        (DVec2::new(0., 18.), 10., 2.),
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
    let pad = [(-22., -18.), (20., -18.), (20., 8.6), (-22., 8.6)];
    let outline = pad.iter().map(|&(x, y)| DVec2::new(x, y) - origin.truncate()).collect();
    e.apply(Command::StampTerrainOutline { outline, offset: 5., height: 0. })?;
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
    let ground_points: Vec<DVec3> = e.mesh().vertices().map(|(_, v)| v.pos + origin).collect();
    e.exit();
    paint(&mut e, terrain, grass)?;
    let ground_at = |x: f64, y: f64| {
        let here = DVec2::new(x, y);
        ground_points
            .iter()
            .min_by(|a, b| (a.truncate() - here).length().total_cmp(&(b.truncate() - here).length()))
            .map_or(0., |q| q.z)
    };

    // Stone garden wall with a concrete coping along the back of the pad.
    cuboid(&mut e, DVec3::new(-22., 8., 0.), DVec3::new(20., 8.5, 1.0), p.stone)?;
    cuboid(&mut e, DVec3::new(-22.05, 7.95, 1.0), DVec3::new(20.05, 8.55, 1.08), p.concrete)?;

    // Ground floor: a floor slab, white walls, a glass wall to the garden and a furnished interior.
    cuboid(&mut e, DVec3::new(-14., -4., 0.), DVec3::new(4., 6., 0.25), p.concrete)?;
    let (floor, ceiling) = (0.25, 3.2);
    cuboid(&mut e, DVec3::new(-14., 5.7, floor), DVec3::new(4., 6., ceiling), p.render)?;
    cuboid(&mut e, DVec3::new(-14., -4., floor), DVec3::new(-13.7, 5.7, ceiling), p.render)?;
    cuboid(&mut e, DVec3::new(3.7, 1.8, floor), DVec3::new(4., 5.7, ceiling), p.render)?;
    let wing = cuboid(&mut e, DVec3::new(-14., -4., floor), DVec3::new(-8.5, -3.7, ceiling), p.render)?;
    window(&mut e, wing, -DVec3::Y, 1.2, 3., 0.9, 1.5, 3, &p)?;
    cuboid(&mut e, DVec3::new(-8.8, -3.7, floor), DVec3::new(-8.5, 5.7, ceiling), p.render)?;
    glazing(&mut e, DVec2::new(-8.5, -3.85), DVec2::new(3.85, -3.85), floor..=ceiling, 6, p.clear, &p)?;
    glazing(&mut e, DVec2::new(3.85, -3.85), DVec2::new(3.85, 1.8), floor..=ceiling, 3, p.clear, &p)?;
    raised(&mut e, floor, |e| {
        // Living room.
        cuboid(e, DVec3::new(-7.6, -2.6, 0.), DVec3::new(-3.4, 1.2, 0.02), p.rug)?;
        sofa(e, DVec2::new(-7.4, 0.3), DVec2::new(-3.8, 1.2), DVec2::Y, &p)?;
        sofa(e, DVec2::new(-8.3, -2.2), DVec2::new(-7.4, 1.2), -DVec2::X, &p)?;
        table(e, DVec2::new(-6.3, -1.4), DVec2::new(-4.6, -0.6), 0.4, p.oak, &p)?;
        cuboid(e, DVec3::new(-8.0, 5.2, 0.), DVec3::new(-2.5, 5.7, 2.2), p.oak)?;
        column(e, DVec3::new(-3.2, -2.9, 0.), 0.22, 0.45, 8, p.render)?;
        crown(e, DVec3::new(-3.2, -2.9, 0.75), 0.45, 0.8, 0.3, 7, p.hedge)?;
        // Kitchen and dining.
        cuboid(e, DVec3::new(-1.5, 5.0, 0.), DVec3::new(3.6, 5.7, 0.9), p.render)?;
        cuboid(e, DVec3::new(-1.5, 5.0, 0.9), DVec3::new(3.6, 5.7, 0.94), p.dark)?;
        cuboid(e, DVec3::new(-1.5, 5.35, 1.5), DVec3::new(3.6, 5.7, 2.3), p.render)?;
        cuboid(e, DVec3::new(-0.5, 2.6, 0.), DVec3::new(2.6, 3.6, 0.9), p.render)?;
        rounded(e, DVec3::new(-0.55, 2.55, 0.9), DVec3::new(2.65, 3.65, 0.95), 0.08, p.dark)?;
        table(e, DVec2::new(-0.2, -1.6), DVec2::new(2.4, -0.6), 0.76, p.oak, &p)?;
        for (x, y, turn) in [(0.4, -2.0, 180.), (1.6, -2.0, 180.), (0.4, -0.2, 0.), (1.6, -0.2, 0.)] {
            chair(e, DVec3::new(x, y, 0.), turn, &p)?;
        }
        let room = ceiling - floor;
        for x in [0.3, 1.1, 1.9] {
            pendant(e, DVec3::new(x, -1.1, 1.9), room, &p)?;
        }
        for (x, y) in [(-7., -2.), (-4.5, -2.), (-7., 1.5), (-4.5, 1.5), (-1., 4.), (2., 4.), (-2., -2.8), (2., 1.5)] {
            downlight(e, x, y, room, &p)?;
        }
        Ok(())
    })?;

    // Ceiling slab and the cantilevered timber upper floor.
    cuboid(&mut e, DVec3::new(-14.3, -4.4, ceiling), DVec3::new(4.3, 6.3, 3.5), p.concrete)?;
    cuboid(&mut e, DVec3::new(4.3, -3.2, ceiling), DVec3::new(11., 4.8, 3.5), p.concrete)?;
    let (level, top) = (3.5, 6.5);
    wall(
        &mut e,
        DVec2::new(-6., -3.2),
        DVec2::new(11., -3.2),
        DVec2::Y,
        0.25,
        level,
        top,
        &[(0.8, 9., 0.7, 1.8, 5), (10.6, 5.6, 0.05, 2.8, 4)],
        p.wood,
        &p,
    )?;
    wall(
        &mut e,
        DVec2::new(-6., 4.8),
        DVec2::new(11., 4.8),
        -DVec2::Y,
        0.25,
        level,
        top,
        &[(1.5, 6., 0.7, 1.8, 3)],
        p.wood,
        &p,
    )?;
    wall(
        &mut e,
        DVec2::new(-6., -2.95),
        DVec2::new(-6., 4.55),
        DVec2::X,
        0.25,
        level,
        top,
        &[(2.3, 3., 0.7, 1.8, 2)],
        p.wood,
        &p,
    )?;
    wall(
        &mut e,
        DVec2::new(11., -2.95),
        DVec2::new(11., 4.55),
        -DVec2::X,
        0.25,
        level,
        top,
        &[(0.6, 6.4, 0.05, 2.8, 3)],
        p.wood,
        &p,
    )?;
    // Partition between the study and the bedroom, with a doorway.
    cuboid(&mut e, DVec3::new(4.3, -2.95, level), DVec3::new(4.42, 0.4, top), p.render)?;
    cuboid(&mut e, DVec3::new(4.3, 1.4, level), DVec3::new(4.42, 4.55, top), p.render)?;
    cuboid(&mut e, DVec3::new(4.3, 0.4, level + 2.2), DVec3::new(4.42, 1.4, top), p.render)?;
    raised(&mut e, level, |e| {
        // Bedroom behind the fins.
        cuboid(e, DVec3::new(5.6, -1.0, 0.), DVec3::new(9.8, 3.6, 0.02), p.rug)?;
        bed(e, DVec2::new(6.5, 1.8), DVec2::new(8.5, 4.0), &p)?;
        cuboid(e, DVec3::new(5.8, 3.5, 0.), DVec3::new(6.3, 4.0, 0.5), p.oak)?;
        cuboid(e, DVec3::new(8.7, 3.5, 0.), DVec3::new(9.2, 4.0, 0.5), p.oak)?;
        cuboid(e, DVec3::new(4.42, 1.8, 0.), DVec3::new(5.0, 4.5, 2.3), p.render)?;
        chair(e, DVec3::new(9.8, -1.6, 0.), 220., &p)?;
        // Study and lounge.
        table(e, DVec2::new(-4.6, -2.7), DVec2::new(-2.4, -2.0), 0.75, p.oak, &p)?;
        chair(e, DVec3::new(-3.5, -1.5, 0.), 0., &p)?;
        cuboid(e, DVec3::new(-5.75, 0.2, 0.), DVec3::new(-5.35, 4.4, 2.2), p.oak)?;
        cuboid(e, DVec3::new(-1.5, -0.6, 0.), DVec3::new(2.6, 2.4, 0.02), p.rug)?;
        sofa(e, DVec2::new(-1.2, 1.4), DVec2::new(2.3, 2.3), DVec2::Y, &p)?;
        sofa(e, DVec2::new(1.6, -0.6), DVec2::new(2.5, 0.8), DVec2::X, &p)?;
        table(e, DVec2::new(-0.4, 0.0), DVec2::new(1.0, 0.8), 0.4, p.oak, &p)?;
        column(e, DVec3::new(-1.6, 2.1, 0.), 0.15, 1.6, 6, p.steel)?;
        column(e, DVec3::new(-1.6, 2.1, 1.6), 0.25, 0.3, 8, p.lamp)?;
        column(e, DVec3::new(6.05, 3.75, 0.5), 0.12, 0.25, 8, p.lamp)?;
        column(e, DVec3::new(8.95, 3.75, 0.5), 0.12, 0.25, 8, p.lamp)?;
        column(e, DVec3::new(-2.6, -2.5, 0.75), 0.1, 0.3, 8, p.lamp)?;
        for (x, y) in [(7.7, 0.), (0.5, 0.7), (-3.5, -1.5)] {
            downlight(e, x, y, top - level, &p)?;
        }
        column(e, DVec3::new(3.6, -2.4, 0.), 0.25, 0.5, 8, p.render)?;
        crown(e, DVec3::new(3.6, -2.4, 0.85), 0.5, 0.9, 0.3, 7, p.hedge)?;
        Ok(())
    })?;
    // Timber fins over the cantilever.
    for i in 0..15 {
        let x = 4.2 + i as f64 * 0.45;
        cuboid(&mut e, DVec3::new(x, -3.6, 3.5), DVec3::new(x + 0.08, -3.2, 6.45), p.wood)?;
    }
    // Steel column carrying the cantilever end.
    cuboid(&mut e, DVec3::new(10.4, -2.8, 0.), DVec3::new(10.6, -2.6, 3.5), p.steel)?;
    // Roof with a fascia, solar panels, a skylight and a flue.
    cuboid(&mut e, DVec3::new(-6.4, -3.7, 6.5), DVec3::new(11.4, 5.3, 6.75), p.dark)?;
    cuboid(&mut e, DVec3::new(-6.4, -3.7, 6.75), DVec3::new(11.4, -3.55, 6.95), p.dark)?;
    cuboid(&mut e, DVec3::new(-6.4, 5.15, 6.75), DVec3::new(11.4, 5.3, 6.95), p.dark)?;
    cuboid(&mut e, DVec3::new(-6.4, -3.55, 6.75), DVec3::new(-6.25, 5.15, 6.95), p.dark)?;
    cuboid(&mut e, DVec3::new(11.25, -3.55, 6.75), DVec3::new(11.4, 5.15, 6.95), p.dark)?;
    for row in 0..2 {
        for col in 0..6 {
            let x = 1.5 + col as f64 * 1.15;
            let y = 0.8 + row as f64 * 1.9;
            cuboid(&mut e, DVec3::new(x, y, 6.85), DVec3::new(x + 1.05, y + 1.7, 6.95), p.solar)?;
        }
    }
    cuboid(&mut e, DVec3::new(-4., 0., 6.75), DVec3::new(-2., 2., 7.1), p.railing)?;
    cuboid(&mut e, DVec3::new(-5.2, 3.6, 6.75), DVec3::new(-4.7, 4.1, 7.6), p.render)?;

    // Roof terrace over the left wing: boards, glass railing, planters and loungers.
    boards(&mut e, DVec3::new(-13.9, -4.1, 3.5), DVec3::new(-6.1, 6.0, 3.56), false, 0.16, p.deck)?;
    cuboid(&mut e, DVec3::new(-14.3, -4.4, 3.5), DVec3::new(-6.05, -4.36, 4.5), p.railing)?;
    cuboid(&mut e, DVec3::new(-14.3, -4.4, 3.5), DVec3::new(-14.26, 6.3, 4.5), p.railing)?;
    cuboid(&mut e, DVec3::new(-14.32, -4.42, 4.5), DVec3::new(-6.05, -4.34, 4.55), p.steel)?;
    cuboid(&mut e, DVec3::new(-14.32, -4.42, 4.5), DVec3::new(-14.24, 6.3, 4.55), p.steel)?;
    raised(&mut e, 3.56, |e| {
        lounger(e, DVec3::new(-12.6, -1.0, 0.), 0., &p)?;
        lounger(e, DVec3::new(-11.2, -1.0, 0.), 0., &p)?;
        rounded(e, DVec3::new(-13.8, 3.5, 0.), DVec3::new(-13.0, 5.8, 0.5), 0.12, p.concrete)?;
        bush(e, DVec3::new(-13.4, 4.1, 0.5), 0.8, &p)?;
        bush(e, DVec3::new(-13.4, 5.2, 0.5), 0.7, &p)?;
        Ok(())
    })?;

    // Garden terrace: boards, a dining set, an outdoor sofa and a parasol.
    boards(&mut e, DVec3::new(-14., -10., 0.), DVec3::new(4., -4.4, 0.25), true, 0.16, p.deck)?;
    cuboid(&mut e, DVec3::new(-14., -10.6, 0.), DVec3::new(4., -10., 0.12), p.deck)?;
    raised(&mut e, 0.25, |e| {
        table(e, DVec2::new(-11.5, -8.6), DVec2::new(-8.5, -7.6), 0.76, p.oak, &p)?;
        for x in [-11.0, -10.0, -9.0] {
            chair(e, DVec3::new(x, -9.0, 0.), 180., &p)?;
            chair(e, DVec3::new(x, -7.2, 0.), 0., &p)?;
        }
        parasol(e, DVec3::new(-10., -8.1, 0.), &p)?;
        sofa(e, DVec2::new(-5.5, -9.6), DVec2::new(-2.0, -8.8), -DVec2::Y, &p)?;
        sofa(e, DVec2::new(-6.3, -9.6), DVec2::new(-5.5, -6.8), -DVec2::X, &p)?;
        table(e, DVec2::new(-4.6, -7.9), DVec2::new(-3.0, -7.0), 0.35, p.concrete, &p)?;
        for x in [-13.4, -12.6, 2.6, 3.4] {
            column(e, DVec3::new(x, -5.0, 0.), 0.3, 0.6, 8, p.concrete)?;
            bush(e, DVec3::new(x, -5.0, 0.6), 0.6, &p)?;
        }
        Ok(())
    })?;

    // Pool in a stone surround, with steps and loungers.
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
    e.apply(Command::Paint { faces: vec![pool], material: Some(p.water), back: false })?;
    e.apply(Command::Paint { faces: vec![pool], material: Some(p.water), back: true })?;
    let pool_group = group_loose(&mut e)?;
    // Lights under the water.
    for x in [8., 11.5, 15.] {
        cuboid(&mut e, DVec3::new(x - 0.15, -12.3, 0.03), DVec3::new(x + 0.15, -12.2, 0.12), p.lamp)?;
    }
    paint(&mut e, pool_group, p.stone)?;
    e.apply(Command::Move { elements: vec![Element::Instance(pool_group)], offset: DVec3::Z * 0.02 })?;
    // Entry steps with softened nosings.
    for (i, top) in [0.17, 0.34].into_iter().enumerate() {
        let x1 = 7.6 - i as f64 * 0.5;
        rounded(&mut e, DVec3::new(6., -12.5, 0.02), DVec3::new(x1, -7.5, 0.02 + top), 0.04, p.coping)?;
    }
    // Coping: a slightly overhanging rim with rounded outer corners, in four pieces.
    let (rim, lip, top) = (0.45, 0.04, 0.52);
    for (a, b) in [
        (DVec2::new(6. - rim, -12.5 - rim), DVec2::new(17. + rim, -12.5 + lip)),
        (DVec2::new(6. - rim, -7.5 - lip), DVec2::new(17. + rim, -7.5 + rim)),
        (DVec2::new(6. - rim, -12.5 + lip), DVec2::new(6. + lip, -7.5 - lip)),
        (DVec2::new(17. - lip, -12.5 + lip), DVec2::new(17. + rim, -7.5 - lip)),
    ] {
        rounded(&mut e, a.extend(top), b.extend(top + 0.06), 0.03, p.coping)?;
    }
    // A band of waterline tiles on the inner walls, and a skimmer box at each end.
    let band = (0.36, 0.5);
    cuboid(&mut e, DVec3::new(6., -12.5, band.0), DVec3::new(17., -12.48, band.1), p.tile)?;
    cuboid(&mut e, DVec3::new(6., -7.52, band.0), DVec3::new(17., -7.5, band.1), p.tile)?;
    cuboid(&mut e, DVec3::new(16.98, -12.48, band.0), DVec3::new(17., -7.52, band.1), p.tile)?;
    for x in [8.5, 14.5] {
        rounded(&mut e, DVec3::new(x, -7.4, top + 0.06), DVec3::new(x + 0.3, -7.1, top + 0.065), 0.05, p.dark)?;
    }
    // A stainless ladder: two posts, curved over the coping as short segments, and rungs.
    for x in [15.6, 16.1] {
        column(&mut e, DVec3::new(x, -7.62, 0.1), 0.02, 0.62, 8, p.chrome)?;
        cuboid(&mut e, DVec3::new(x - 0.02, -7.62, 0.7), DVec3::new(x + 0.02, -7.2, 0.74), p.chrome)?;
        column(&mut e, DVec3::new(x, -7.2, top + 0.06), 0.02, 0.18, 8, p.chrome)?;
    }
    for z in [0.18, 0.34] {
        rounded(&mut e, DVec3::new(15.6, -7.7, z), DVec3::new(16.1, -7.6, z + 0.03), 0.01, p.chrome)?;
    }
    for x in [9., 10.8, 12.6, 14.4] {
        lounger(&mut e, DVec3::new(x, -5.6, 0.52), 0., &p)?;
    }
    parasol(&mut e, DVec3::new(16.6, -5.4, 0.52), &p)?;

    // Driveway with a car, hedges and garden lights; stepping stones across the lawn.
    cuboid(&mut e, DVec3::new(-20., -30., 0.), DVec3::new(-15., 7.9, 0.05), p.asphalt)?;
    // Solid yellow edge lines and a dashed centre line, just above the asphalt.
    for x in [-19.85, -15.25] {
        cuboid(&mut e, DVec3::new(x, -30., 0.05), DVec3::new(x + 0.1, 7.6, 0.056), p.marking)?;
    }
    for i in 0..10 {
        let y = -29. + i as f64 * 3.6;
        cuboid(&mut e, DVec3::new(-17.56, y, 0.05), DVec3::new(-17.44, y + 2., 0.056), p.marking)?;
    }
    cybertruck::build(&mut e, DVec3::new(-17.5, -1.5, 0.05))?;
    for i in 0..9 {
        let y = -28. + i as f64 * 3.2;
        rounded(&mut e, DVec3::new(-21.4, y, 0.), DVec3::new(-20.4, y + 2.9, 1.3), 0.35, p.hedge)?;
        cuboid(&mut e, DVec3::new(-14.8, y + 1.4, 0.), DVec3::new(-14.65, y + 1.55, 0.6), p.dark)?;
        cuboid(&mut e, DVec3::new(-14.8, y + 1.4, 0.6), DVec3::new(-14.65, y + 1.55, 0.7), p.lamp)?;
    }
    for i in 0..7 {
        let a = DVec3::new(-1.5 + i as f64 * 1.15, -11.4 - i as f64 * 0.9, 0.);
        let a = DVec3::new(a.x, a.y, ground_at(a.x + 0.45, a.y + 0.3));
        rounded(&mut e, a, a + DVec3::new(0.9, 0.6, 0.06), 0.22, p.paving)?;
    }
    for (x, y, size) in
        [(-12., -12.5, 1.), (-9., -13.5, 0.8), (1., -15.5, 0.9), (19., -2., 1.1), (-3., 7.2, 0.9), (3., 7.3, 1.)]
    {
        bush(&mut e, DVec3::new(x, y, ground_at(x, y)), size, &p)?;
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
        let z = if on_deck { 0.26 } else { ground_at(x, y) + 0.12 };
        let colour = p.autumn[(random.next() * p.autumn.len() as f64) as usize];
        leaf(&mut e, DVec3::new(x, y, z), &mut random, colour)?;
    }
    for (x, y, size) in [(4.6, -4.9, 1.), (4.9, -5.5, 0.8), (-14.6, -5.2, 0.9), (-15.0, -4.6, 0.7)] {
        pumpkin(&mut e, DVec3::new(x, y, 0.25), size, &p)?;
    }

    // A fire pit with a bench on the lawn, a swept leaf pile, firewood and the letterbox.
    fire_pit(&mut e, DVec3::new(-7., -15., ground_at(-7., -15.)), &p)?;
    bench(&mut e, DVec3::new(-7., -16.9, ground_at(-7., -16.9)), 180., &p)?;
    bench(&mut e, DVec3::new(-8.9, -15., ground_at(-8.9, -15.)), 90., &p)?;
    leaf_pile(&mut e, DVec3::new(-5.5, -20.5, ground_at(-5.5, -20.5)), &p)?;
    woodpile(&mut e, DVec2::new(-14.85, 1.0), 2.4, &p)?;
    letterbox(&mut e, DVec3::new(-14.5, -28.5, ground_at(-14.5, -28.5)), &p)?;

    // Trees around the garden and on the hills.
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
    ] {
        tree(&mut e, DVec3::new(x, y, ground_at(x, y) - 0.1), size, &p)?;
    }
    for (x, y, size) in
        [(-14., 15., 1.3), (-18., 18., 1.1), (16., 21., 1.2), (20., 17., 1.), (31., 2., 1.1), (-34., -6., 1.2)]
    {
        conifer(&mut e, DVec3::new(x, y, ground_at(x, y) - 0.1), size, &p)?;
    }

    let mut doc = e.doc().clone();
    doc.style.lighting = rysmo_model::Lighting::Sun;
    doc.style.shadows = true;
    doc.style.ambient_occlusion = true;
    doc.style.edges = false;
    doc.style.tone_mapping = rysmo_model::ToneMapping::TonyMcMapface;
    doc.style.exposure = std::env::var("DEMO_EXPOSURE").ok().and_then(|x| x.parse().ok()).unwrap_or(3.);
    doc.style.atmosphere = true;
    doc.style.ground = true;
    doc.style.ground_color = [0.5, 0.52, 0.28];
    // A low autumn afternoon sun with the lamps already on, north turned to light the front.
    doc.style.sun.day = 290;
    doc.style.sun.hour = std::env::var("DEMO_HOUR").ok().and_then(|h| h.parse().ok()).unwrap_or(HOUR);
    doc.style.sun.north = 65.;
    doc.camera = Some(OrbitCamera {
        target: DVec3::new(-1., -4., 2.),
        distance: 36.,
        yaw: -1.1,
        pitch: 0.28,
        fov_y: 0.7,
        parallel: false,
        eye_base_z: 0.,
        roll: 0.,
    });
    std::fs::write(&path, rysmo_io::save(&doc))?;
    Ok(())
}
