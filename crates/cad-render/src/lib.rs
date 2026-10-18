#![forbid(unsafe_code)]
//! CPU CAD scene rendering. No kernel, transport, window or Python dependency.
use base64::{engine::general_purpose::STANDARD, Engine};
use cad_protocol::{Edge, Mesh};
use serde_json::{json, Value};
use std::collections::BTreeMap;
const AMBIGUOUS: u32 = u32::MAX;
fn world(p: &[cad_protocol::Finite; 3]) -> [f64; 3] {
    p.map(|n| n.get())
}
fn camera(p: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = p;
    [
        (x - y) / 2f64.sqrt(),
        (2. * z - x - y) / 6f64.sqrt(),
        (x + y + z) / 3f64.sqrt(),
    ]
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
pub struct Frame {
    pub metadata: Value,
    width: u32,
    height: u32,
    faces: Vec<u32>,
    edges: Vec<u32>,
    face_refs: BTreeMap<u32, String>,
    edge_refs: BTreeMap<u32, String>,
}
impl Frame {
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
            for mode in ["shaded", "face-id", "edge-id"] {
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
    pub fn pick(&self, x: u32, y: u32, target: &str) -> Result<Value, String> {
        if x >= self.width || y >= self.height {
            return Err("invalid_params: pixel outside viewport".into());
        }
        let index = (y * self.width + x) as usize;
        let (id, refs) = match target {
            "edge" => (self.edges[index], &self.edge_refs),
            "face" => (self.faces[index], &self.face_refs),
            _ => return Err("invalid_params: target must be edge or face".into()),
        };
        Ok(json!({"reference":refs.get(&id),"ambiguous":id==AMBIGUOUS,"target":target,"x":x,"y":y}))
    }
}
pub fn render(mesh: &Mesh, edges: &[Edge], width: u32, height: u32) -> Result<Frame, String> {
    if !(64..=768).contains(&width) || !(64..=768).contains(&height) {
        return Err("invalid_params: dimensions must be 64..768".into());
    }
    if mesh.points.is_empty() || mesh.triangles.is_empty() {
        return Err("empty_document: no mesh to render".into());
    }
    let raw: Vec<_> = mesh.points.iter().map(|p| camera(world(p))).collect();
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in &raw {
        for i in 0..2 {
            lo[i] = lo[i].min(p[i]);
            hi[i] = hi[i].max(p[i]);
        }
    }
    let scale = ((width - 48) as f64 / (hi[0] - lo[0])).min((height - 48) as f64 / (hi[1] - lo[1]));
    let center = [lo[0] + (hi[0] - lo[0]) / 2., lo[1] + (hi[1] - lo[1]) / 2.];
    if !scale.is_finite() || scale <= 0. || center.iter().any(|n| !n.is_finite()) {
        return Err("validation_failed: non-finite or degenerate projection".into());
    }
    let project = |p: [f64; 3]| {
        [
            (p[0] - center[0]) * scale + width as f64 / 2.,
            height as f64 / 2. - (p[1] - center[1]) * scale,
            p[2],
        ]
    };
    let points: Vec<_> = raw.into_iter().map(project).collect();
    if points.iter().flatten().any(|n| !n.is_finite()) {
        return Err("validation_failed: non-finite mesh projection".into());
    }
    let count = (width * height) as usize;
    let mut depth = vec![f64::NEG_INFINITY; count];
    let mut face_pixels = vec![0u32; count];
    let mut edge_pixels = vec![0u32; count];
    let mut colors = [242, 245, 248].repeat(count);
    let mut face_ids = BTreeMap::new();
    let mut edge_ids = BTreeMap::new();
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
            .map(|p| project(camera(world(p))))
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
                        if ((p[2] - depth[idx]) * scale).abs() < 4. {
                            let old = edge_pixels[idx];
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
        "camera":{"projection":"orthographic","direction":[1,1,1],"scale":scale,"center":center},
        "images":{"shaded":STANDARD.encode(png_bytes(width,height,&colors)?),"face-id":STANDARD.encode(png_bytes(width,height,&id_rgb(&face_pixels))?),"edge-id":STANDARD.encode(png_bytes(width,height,&id_rgb(&edge_pixels))?)},
        "face_ids":face_refs,"edge_ids":edge_refs});
    Ok(Frame {
        metadata,
        width,
        height,
        faces: face_pixels,
        edges: edge_pixels,
        face_refs,
        edge_refs,
    })
}
