#![forbid(unsafe_code)]
//! CPU CAD scene rendering. No kernel, transport, window or Python dependency.
use base64::{engine::general_purpose::STANDARD, Engine};
use cad_protocol::{Edge, Mesh};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
#[cfg(not(target_arch = "wasm32"))]
mod gpu;
#[cfg(not(target_arch = "wasm32"))]
pub use gpu::Renderer;
const AMBIGUOUS: u32 = u32::MAX;
fn world(p: &[cad_protocol::Finite; 3]) -> [f64; 3] {
    p.map(|n| n.get())
}
/// View-local orthographic orbit camera; never changes a CAD document revision.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Camera {
    pub yaw: f64,
    pub pitch: f64,
    pub zoom: f64,
    pub pan: [f64; 2],
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: std::f64::consts::FRAC_PI_4,
            pitch: (1. / 3f64.sqrt()).asin(),
            zoom: 1.,
            pan: [0., 0.],
        }
    }
}
impl Camera {
    /// Grab-style turntable: visible front surfaces follow the finger, independent of DPI.
    pub fn orbit(&mut self, delta: [f64; 2], viewport: [f64; 2]) {
        let gain = std::f64::consts::PI / viewport[0].min(viewport[1]).max(128.);
        self.yaw = (self.yaw + delta[0] * gain).rem_euclid(std::f64::consts::TAU);
        self.pitch = (self.pitch + delta[1] * gain).clamp(-1.4, 1.4);
    }
    pub fn projected_axis(self, axis: [f64; 3]) -> [f64; 2] {
        let p = self.transform(axis);
        [p[0], -p[1]]
    }
    pub fn direction(self) -> [f64; 3] {
        [
            self.pitch.cos() * self.yaw.cos(),
            self.pitch.cos() * self.yaw.sin(),
            self.pitch.sin(),
        ]
    }
    fn transform(self, p: [f64; 3]) -> [f64; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let [x, y, z] = p;
        [
            sy * x - cy * y,
            -sp * cy * x - sp * sy * y + cp * z,
            cp * cy * x + cp * sy * y + sp * z,
        ]
    }
}

fn png_bytes(width: u32, height: u32, rgb: &[u8]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_filter(png::FilterType::NoFilter);
        encoder
            .write_header()
            .map_err(|e| e.to_string())?
            .write_image_data(rgb)
            .map_err(|e| e.to_string())?;
    }
    Ok(bytes)
}
fn id_rgb(ids: &[u32]) -> Vec<u8> {
    ids.iter()
        .flat_map(|&id| {
            let id = if id == AMBIGUOUS { 0 } else { id };
            [id as u8, (id >> 8) as u8, (id >> 16) as u8]
        })
        .collect()
}
fn id_for(map: &mut BTreeMap<String, u32>, reference: &str) -> u32 {
    let next = map.len() as u32 + 1;
    *map.entry(reference.into()).or_insert(next)
}
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Frame {
    pub metadata: Value,
    #[serde(with = "packed_rgb")]
    rgb: Vec<u8>,
    #[serde(skip)]
    depth_rgb: Vec<u8>,
    width: u32,
    height: u32,
    #[serde(with = "packed_ids")]
    faces: Vec<u32>,
    #[serde(with = "packed_ids")]
    edges: Vec<u32>,
    face_refs: BTreeMap<u32, String>,
    edge_refs: BTreeMap<u32, String>,
    #[serde(default)]
    candidates: BTreeMap<usize, Vec<u32>>,
    #[serde(default)]
    truncated: BTreeSet<usize>,
}
impl Frame {
    pub fn rgb(&self) -> &[u8] {
        &self.rgb
    }
    pub fn viewport_rgb(&self, selected: Option<&str>) -> Vec<u8> {
        let mut rgb = self.rgb.clone();
        for (index, id) in self.edges.iter().enumerate() {
            if *id == 0 || *id == AMBIGUOUS {
                continue;
            }
            let color = if selected.is_some_and(|s| self.edge_refs.get(id).is_some_and(|r| r == s))
            {
                [255, 189, 64]
            } else {
                [52, 72, 89]
            };
            rgb[3 * index..3 * index + 3].copy_from_slice(&color);
        }
        rgb
    }

    pub fn save(&self, directory: &str) -> Result<Value, String> {
        if directory.is_empty() || directory.contains('\0') {
            return Err("invalid_params: invalid render directory".into());
        }
        let save = || -> Result<Value, Box<dyn std::error::Error>> {
            let root = std::path::Path::new(directory);
            std::fs::create_dir_all(root)?;
            let root = root.canonicalize()?;
            let id = self.metadata["render_id"]
                .as_str()
                .ok_or("missing render ID")?;
            let mut files = serde_json::Map::new();
            for mode in ["shaded", "face-id", "edge-id", "depth"] {
                if self.metadata["images"].get(mode).is_none() {
                    continue;
                }
                let path = root.join(format!("{id}-{mode}.png"));
                let temporary = root.join(format!("{id}-{mode}.tmp"));
                let bytes = STANDARD.decode(
                    self.metadata["images"][mode]
                        .as_str()
                        .ok_or("missing PNG")?,
                )?;
                std::fs::write(&temporary, bytes)?;
                std::fs::rename(temporary, &path)?;
                files.insert(mode.into(), json!(path));
            }
            Ok(Value::Object(files))
        };
        save().map_err(|e| format!("io_error: {e}"))
    }
    /// Screen-space hit slop over visible IDs only. Tied nearest edges stay ambiguous.
    pub fn pick_edge_near(&self, point: [f64; 2], radius: [f64; 2]) -> Result<Value, String> {
        self.validate()?;
        if point.iter().any(|v| !v.is_finite())
            || radius
                .iter()
                .any(|v| !v.is_finite() || *v <= 0. || *v > 64.)
        {
            return Err("invalid_params: pick point/radius".into());
        }
        let [x, y] = point;
        let [rx, ry] = radius;
        let mut distances: BTreeMap<u32, f64> = BTreeMap::new();
        let mut truncated = false;
        for py in
            (y - ry).floor().max(0.) as u32..=((y + ry).ceil().max(0.) as u32).min(self.height - 1)
        {
            for px in (x - rx).floor().max(0.) as u32
                ..=((x + rx).ceil().max(0.) as u32).min(self.width - 1)
            {
                let d = ((px as f64 + 0.5 - x) / rx).powi(2) + ((py as f64 + 0.5 - y) / ry).powi(2);
                if d > 1. {
                    continue;
                }
                let index = (py * self.width + px) as usize;
                let id = self.edges[index];
                let ids: Vec<u32> = if id == AMBIGUOUS {
                    self.candidates.get(&index).cloned().unwrap_or_default()
                } else if id != 0 {
                    vec![id]
                } else {
                    Vec::new()
                };
                if self.truncated.contains(&index) {
                    truncated = true;
                }
                for id in ids {
                    distances
                        .entry(id)
                        .and_modify(|old| *old = old.min(d))
                        .or_insert(d);
                }
            }
        }
        let mut hits: Vec<_> = distances.into_iter().collect();
        hits.sort_by(|a, b| a.1.total_cmp(&b.1));
        // At a junction do not silently choose whichever edge happened to rasterize first.
        let ambiguous = hits.len() > 1 && (hits[1].1.sqrt() - hits[0].1.sqrt()) * rx.min(ry) < 1.;
        let reference = if ambiguous || truncated {
            None
        } else {
            hits.first().and_then(|(id, _)| self.edge_refs.get(id))
        };
        let nearest = hits.first().map_or(0., |(_, d)| d.sqrt());
        let candidates: Vec<_> = hits
            .iter()
            .filter(|(_, d)| !ambiguous || (d.sqrt() - nearest) * rx.min(ry) < 1.)
            .take(8)
            .filter_map(|(id, _)| self.edge_refs.get(id))
            .collect();
        Ok(
            json!({"reference":reference,"ambiguous":ambiguous || truncated,"candidates":candidates,"candidates_truncated":truncated || hits.len()>8}),
        )
    }
    pub fn pick(&self, x: u32, y: u32, target: &str) -> Result<Value, String> {
        self.validate()?;
        if x >= self.width || y >= self.height {
            return Err("invalid_params: pixel outside viewport".into());
        }
        let index = (y * self.width + x) as usize;
        let (id, refs) = match target {
            "edge" => (self.edges[index], &self.edge_refs),
            "face" => (self.faces[index], &self.face_refs),
            _ => return Err("invalid_params: target must be edge or face".into()),
        };
        let candidates: Vec<_> = if target == "edge" && id == AMBIGUOUS {
            self.candidates
                .get(&index)
                .into_iter()
                .flatten()
                .filter_map(|id| refs.get(id))
                .cloned()
                .collect()
        } else {
            refs.get(&id).cloned().into_iter().collect()
        };
        Ok(
            json!({"reference":refs.get(&id),"ambiguous":id==AMBIGUOUS,"candidates":candidates,"candidates_truncated":self.truncated.contains(&index),"target":target,"x":x,"y":y}),
        )
    }
}
pub fn render(mesh: &Mesh, edges: &[Edge], width: u32, height: u32) -> Result<Frame, String> {
    let mut frame = rasterize(mesh, edges, width, height, Camera::default(), true)?;
    frame.encode_images()?;
    Ok(frame)
}
/// Interactive local viewport; omits PNG encoding while sharing raster and pick maps.
pub fn render_view(
    mesh: &Mesh,
    edges: &[Edge],
    width: u32,
    height: u32,
    camera: Camera,
) -> Result<Frame, String> {
    rasterize(mesh, edges, width, height, camera, false)
}
fn rasterize(
    mesh: &Mesh,
    edges: &[Edge],
    width: u32,
    height: u32,
    camera: Camera,
    encode: bool,
) -> Result<Frame, String> {
    if !camera.yaw.is_finite()
        || !camera.pitch.is_finite()
        || !camera.zoom.is_finite()
        || camera.zoom <= 0.
        || camera.pan.iter().any(|v| !v.is_finite())
    {
        return Err("invalid_params: non-finite camera".into());
    }
    if !(64..=768).contains(&width) || !(64..=768).contains(&height) {
        return Err("invalid_params: dimensions must be 64..768".into());
    }
    if mesh.points.is_empty() || mesh.triangles.is_empty() {
        return Err("empty_document: no mesh to render".into());
    }
    let (projection, points) = projection(mesh, width, height, camera, encode)?;
    let scale = projection.scale;
    let center = projection.center;
    let project = |p| projection.project(p);
    let count = (width * height) as usize;
    let mut depth = vec![f64::NEG_INFINITY; count];
    let mut slope = vec![[0.; 2]; count];
    let mut face_pixels = vec![0u32; count];
    let mut edge_pixels = vec![0u32; count];
    let mut colors = [242, 245, 248].repeat(count);
    let mut face_ids = BTreeMap::new();
    let mut edge_ids = BTreeMap::new();
    let mut candidates: BTreeMap<usize, Vec<u32>> = BTreeMap::new();
    for triangle in &mesh.triangles {
        let indices = triangle.vertices;
        if indices.iter().any(|&i| i >= points.len()) {
            return Err("validation_failed: mesh vertex index out of range".into());
        }
        let [p, q, r] = indices.map(|i| points[i]);
        let det = (q[1] - r[1]) * (p[0] - r[0]) + (r[0] - q[0]) * (p[1] - r[1]);
        if det.abs() < 1e-10 {
            continue;
        }
        let [a, b, c] = indices.map(|i| world(&mesh.points[i]));
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = n.iter().map(|v| v * v).sum::<f64>().sqrt();
        let light = 0.35 + 0.65 * ((n[0] * 0.35 + n[1] * 0.25 + n[2] * 0.9028) / length).abs();
        if !light.is_finite() {
            return Err("validation_failed: non-finite triangle normal".into());
        }
        let color = [112., 169., 212.].map(|v| (v * light).clamp(0., 255.) as u8);
        let id = id_for(&mut face_ids, &triangle.face);
        let min_x = p[0].min(q[0]).min(r[0]).floor().max(0.) as u32;
        let max_x = p[0].max(q[0]).max(r[0]).ceil().min(width as f64 - 1.) as u32;
        let min_y = p[1].min(q[1]).min(r[1]).floor().max(0.) as u32;
        let max_y = p[1].max(q[1]).max(r[1]).ceil().min(height as f64 - 1.) as u32;
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let u = ((q[1] - r[1]) * (x as f64 + 0.5 - r[0])
                    + (r[0] - q[0]) * (y as f64 + 0.5 - r[1]))
                    / det;
                let v = ((r[1] - p[1]) * (x as f64 + 0.5 - r[0])
                    + (p[0] - r[0]) * (y as f64 + 0.5 - r[1]))
                    / det;
                let w = 1. - u - v;
                if u.min(v).min(w) < -1e-7 {
                    continue;
                }
                let z = u * p[2] + v * q[2] + w * r[2];
                let idx = (y * width + x) as usize;
                if z > depth[idx] {
                    depth[idx] = z;
                    slope[idx] = [
                        ((q[1] - r[1]) * (p[2] - r[2]) + (r[1] - p[1]) * (q[2] - r[2])) / det,
                        ((r[0] - q[0]) * (p[2] - r[2]) + (p[0] - r[0]) * (q[2] - r[2])) / det,
                    ];
                    face_pixels[idx] = id;
                    colors[3 * idx..3 * idx + 3].copy_from_slice(&color);
                }
            }
        }
    }
    for edge in edges {
        let id = id_for(&mut edge_ids, &edge.reference);
        let projected: Vec<_> = edge
            .points
            .iter()
            .map(|p| project(camera.transform(world(p))))
            .collect();
        if projected.iter().flatten().any(|n| !n.is_finite()) {
            return Err("validation_failed: non-finite edge projection".into());
        }
        for segment in projected.windows(2) {
            let [a, b] = [segment[0], segment[1]];
            let steps = ((b[0] - a[0]).hypot(b[1] - a[1]) * 2.).ceil().max(1.) as usize;
            if steps > 8 * (width + height) as usize {
                return Err("validation_failed: edge outside mesh bounds".into());
            }
            for step in 0..=steps {
                let t = step as f64 / steps as f64;
                let p = [
                    a[0] * (1. - t) + b[0] * t,
                    a[1] * (1. - t) + b[1] * t,
                    a[2] * (1. - t) + b[2] * t,
                ];
                for dy in -2..=2 {
                    for dx in -2..=2 {
                        let x = p[0].round() as i64 + dx;
                        let y = p[1].round() as i64 + dy;
                        if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
                            continue;
                        }
                        let idx = (y * width as i64 + x) as usize;
                        if ((p[2]
                            - (depth[idx]
                                + slope[idx][0] * (p[0] - x as f64 - 0.5)
                                + slope[idx][1] * (p[1] - y as f64 - 0.5)))
                            * scale)
                            .abs()
                            < 0.05
                        {
                            let old = edge_pixels[idx];
                            if old != 0 && old != id {
                                let list = candidates.entry(idx).or_default();
                                if old != AMBIGUOUS && !list.contains(&old) {
                                    list.push(old);
                                }
                                if !list.contains(&id) {
                                    list.push(id);
                                }
                            }
                            edge_pixels[idx] = if old == 0 || old == id { id } else { AMBIGUOUS };
                        }
                    }
                }
            }
        }
    }
    let face_refs = face_ids
        .into_iter()
        .map(|(k, v)| (v, k))
        .collect::<BTreeMap<_, _>>();
    let edge_refs = edge_ids
        .into_iter()
        .map(|(k, v)| (v, k))
        .collect::<BTreeMap<_, _>>();
    let metadata = json!({"width":width,"height":height,"backend":"cpu","mime_type":"image/png",
        "camera":{"projection":"orthographic","direction":camera.direction(),"yaw":camera.yaw,"pitch":camera.pitch,"zoom":camera.zoom,"pan":camera.pan,"scale":scale,"center":center},
        "images":{},
        "face_ids":face_refs,"edge_ids":edge_refs});
    Ok(Frame {
        metadata,
        depth_rgb: {
            let lo = depth
                .iter()
                .copied()
                .filter(|z| z.is_finite())
                .fold(f64::INFINITY, f64::min);
            let hi = depth
                .iter()
                .copied()
                .filter(|z| z.is_finite())
                .fold(f64::NEG_INFINITY, f64::max);
            depth
                .iter()
                .flat_map(|z| {
                    let gray = if !z.is_finite() {
                        0
                    } else {
                        (1. + 254. * ((z - lo) / (hi - lo).max(1e-9))) as u8
                    };
                    [gray; 3]
                })
                .collect()
        },
        rgb: colors,
        width,
        height,
        faces: face_pixels,
        edges: edge_pixels,
        face_refs,
        edge_refs,
        candidates,
        truncated: BTreeSet::new(),
    })
}

struct Projection {
    scale: f64,
    center: [f64; 2],
    width: u32,
    height: u32,
    camera: Camera,
}
impl Projection {
    fn project(&self, p: [f64; 3]) -> [f64; 3] {
        [
            (p[0] - self.center[0]) * self.scale + self.width as f64 * (0.5 + self.camera.pan[0]),
            self.height as f64 * (0.5 + self.camera.pan[1]) - (p[1] - self.center[1]) * self.scale,
            p[2],
        ]
    }
}
fn projection(
    mesh: &Mesh,
    width: u32,
    height: u32,
    camera: Camera,
    encode: bool,
) -> Result<(Projection, Vec<[f64; 3]>), String> {
    if !(64..=768).contains(&width)
        || !(64..=768).contains(&height)
        || !camera.yaw.is_finite()
        || !camera.pitch.is_finite()
        || !camera.zoom.is_finite()
        || camera.zoom <= 0.
        || camera.pan.iter().any(|n| !n.is_finite())
    {
        return Err("invalid_params: camera/dimensions".into());
    }
    if mesh.points.is_empty() || mesh.triangles.is_empty() {
        return Err("empty_document: no mesh".into());
    }
    let raw: Vec<_> = mesh
        .points
        .iter()
        .map(|p| camera.transform(world(p)))
        .collect();
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in &raw {
        for i in 0..2 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    let (scale, center) = if encode {
        (
            camera.zoom
                * ((width - 48) as f64 / (hi[0] - lo[0]))
                    .min((height - 48) as f64 / (hi[1] - lo[1])),
            [lo[0] + (hi[0] - lo[0]) / 2., lo[1] + (hi[1] - lo[1]) / 2.],
        )
    } else {
        // A world-space bounding sphere keeps orbit scale and pivot stable.
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for point in &mesh.points {
            for i in 0..3 {
                low[i] = low[i].min(point[i].get());
                high[i] = high[i].max(point[i].get());
            }
        }
        let pivot = std::array::from_fn(|i| low[i] + (high[i] - low[i]) / 2.);
        let radius = mesh
            .points
            .iter()
            .map(|p| {
                (0..3)
                    .map(|i| (p[i].get() - pivot[i]).powi(2))
                    .sum::<f64>()
                    .sqrt()
            })
            .fold(0f64, f64::max);
        let p = camera.transform(pivot);
        (
            camera.zoom * width.min(height).saturating_sub(72).max(16) as f64 / (2. * radius),
            [p[0], p[1]],
        )
    };
    if !scale.is_finite() || scale <= 0. || center.iter().any(|n| !n.is_finite()) {
        return Err("validation_failed: non-finite or degenerate projection".into());
    }
    let projection = Projection {
        scale,
        center,
        width,
        height,
        camera,
    };
    let points: Vec<_> = raw.into_iter().map(|p| projection.project(p)).collect();
    if points.iter().flatten().any(|n| !n.is_finite()) {
        return Err("validation_failed: projection".into());
    }
    Ok((projection, points))
}

impl Frame {
    pub fn encode_images(&mut self) -> Result<(), String> {
        let depth = self.metadata["images"].get("depth").cloned();
        self.metadata["images"] = json!({"shaded":STANDARD.encode(png_bytes(self.width,self.height,&self.rgb)?),"face-id":STANDARD.encode(png_bytes(self.width,self.height,&id_rgb(&self.faces))?),"edge-id":STANDARD.encode(png_bytes(self.width,self.height,&id_rgb(&self.edges))?)});
        if !self.depth_rgb.is_empty() {
            self.metadata["images"]["depth"] =
                json!(STANDARD.encode(png_bytes(self.width, self.height, &self.depth_rgb)?));
        } else if let Some(depth) = depth {
            self.metadata["images"]["depth"] = depth;
        }
        Ok(())
    }
    pub fn dimensions(&self) -> [u32; 2] {
        [self.width, self.height]
    }
}

// Wire planes are bounded zlib/base64 payloads. Avoid millions of JSON number nodes on
// every orbit frame; decode limits are checked before constructing pixel/id vectors.
fn pack(bytes: &[u8]) -> Result<String, std::io::Error> {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes)?;
    Ok(STANDARD.encode(encoder.finish()?))
}
fn unpack(value: &str, limit: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if value.len() > 8 * 1024 * 1024 {
        return Err("packed frame exceeds wire limit".into());
    }
    let compressed = STANDARD.decode(value).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    flate2::read::ZlibDecoder::new(&compressed[..])
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("decompressed frame exceeds limit".into());
    }
    Ok(bytes)
}
mod packed_rgb {
    use super::*;
    pub fn serialize<S: serde::Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&pack(bytes).map_err(serde::ser::Error::custom)?)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u8>, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(deserializer)?;
        unpack(&s, 768 * 768 * 3).map_err(serde::de::Error::custom)
    }
}
mod packed_ids {
    use super::*;
    pub fn serialize<S: serde::Serializer>(ids: &[u32], serializer: S) -> Result<S::Ok, S::Error> {
        let bytes: Vec<_> = ids.iter().flat_map(|v| v.to_le_bytes()).collect();
        serializer.serialize_str(&pack(&bytes).map_err(serde::ser::Error::custom)?)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u32>, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(deserializer)?;
        let bytes = unpack(&s, 768 * 768 * 4).map_err(serde::de::Error::custom)?;
        if bytes.len() % 4 != 0 {
            return Err(serde::de::Error::custom("invalid ID plane byte length"));
        }
        Ok(bytes
            .chunks_exact(4)
            .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
            .collect())
    }
}
impl Frame {
    pub fn validate(&self) -> Result<(), String> {
        let count = (self.width as usize)
            .checked_mul(self.height as usize)
            .ok_or("frame size overflow")?;
        if !(64..=768).contains(&self.width)
            || !(64..=768).contains(&self.height)
            || self.rgb.len() != count * 3
            || self.faces.len() != count
            || self.edges.len() != count
        {
            return Err("invalid frame dimensions/planes".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod interaction_tests;
