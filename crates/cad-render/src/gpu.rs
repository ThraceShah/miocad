//! Headless wgpu two-pass rasterization. No window/surface is created.
use super::*;
use std::time::Instant;
use wgpu::util::DeviceExt;
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    surface: wgpu::ComputePipeline,
    edge: wgpu::ComputePipeline,
    info: Value,
    cache: Option<(Vec<u8>, wgpu::Buffer, wgpu::Buffer)>,
}
pub struct Renderer {
    gpu: Option<Gpu>,
    reason: Option<String>,
    uploads: u64,
}
impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}
impl Renderer {
    pub fn new() -> Self {
        match pollster::block_on(Gpu::new()) {
            Ok(gpu) => Self {
                gpu: Some(gpu),
                reason: None,
                uploads: 0,
            },
            Err(reason) => Self {
                gpu: None,
                reason: Some(reason),
                uploads: 0,
            },
        }
    }
    pub fn cpu(reason: impl Into<String>) -> Self {
        Self {
            gpu: None,
            reason: Some(reason.into()),
            uploads: 0,
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        mesh: &Mesh,
        edges: &[Edge],
        width: u32,
        height: u32,
        camera: Camera,
        fit: bool,
        backend: &str,
    ) -> Result<Frame, String> {
        let begin = Instant::now();
        let mut fallback = self.reason.clone();
        let result = if backend != "cpu" {
            if let Some(gpu) = &mut self.gpu {
                match gpu.render(mesh, edges, width, height, camera, fit, &mut self.uploads) {
                    Ok(frame) => Some(frame),
                    Err(e) => {
                        if backend == "gpu" {
                            return Err(e);
                        }
                        fallback = Some(e);
                        None
                    }
                }
            } else {
                None
            }
        } else {
            fallback = Some("explicit CPU selection".into());
            None
        };
        if backend == "gpu" && result.is_none() {
            return Err(format!("gpu_unavailable: {}", fallback.unwrap_or_default()));
        }
        let mut frame = match result {
            Some(frame) => frame,
            None => rasterize(mesh, edges, width, height, camera, fit)?,
        };
        frame.metadata["fallback_reason"] = if frame.metadata["backend"] == "cpu" {
            json!(fallback)
        } else {
            Value::Null
        };
        frame.encode_images()?;
        frame.metadata["timing_ms"] = json!(begin.elapsed().as_secs_f64() * 1000.);
        frame.metadata["geometry_uploads"] = json!(self.uploads);
        frame.metadata["edge_tolerance_px"] = json!(2);
        Ok(frame)
    }
}
impl Gpu {
    async fn new() -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
            ..Default::default()
        });
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .map_err(|e| e.to_string())?;
        if !adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        {
            return Err("adapter lacks compute shaders".into());
        }
        let info = adapter.get_info();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("CAD headless renderer"),
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("CAD shaded/depth and edge IDs"),
            source: wgpu::ShaderSource::Wgsl(include_str!("raster.wgsl").into()),
        });
        let entries: Vec<_> = (0..4)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 3 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding < 2,
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let surface = pipeline("surface");
        let edge = pipeline("edge");
        if let Some(error) = device.pop_error_scope().await {
            return Err(format!("CAD GPU pipeline validation: {error}"));
        }
        Ok(Self {
            device,
            queue,
            layout,
            surface,
            edge,
            info: json!({"name":info.name,"backend":format!("{:?}",info.backend),"device_type":format!("{:?}",info.device_type)}),
            cache: None,
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        mesh: &Mesh,
        edges: &[Edge],
        width: u32,
        height: u32,
        camera: Camera,
        fit: bool,
        uploads: &mut u64,
    ) -> Result<Frame, String> {
        let (projection, points) = projection(mesh, width, height, camera, fit)?;
        let origin_z = points[0][2];
        let mut face_ids = BTreeMap::new();
        let mut edge_ids = BTreeMap::new();
        let mut triangles = Vec::<[f32; 4]>::new();
        let mut segments = Vec::<[f32; 4]>::new();
        for t in &mesh.triangles {
            if t.vertices.iter().any(|i| *i >= points.len()) {
                return Err("validation_failed: mesh vertex index out of range".into());
            }
            let id = id_for(&mut face_ids, &t.face);
            let [a, b, c] = t.vertices.map(|i| world(&mesh.points[i]));
            let u = std::array::from_fn::<_, 3, _>(|i| b[i] - a[i]);
            let v = std::array::from_fn::<_, 3, _>(|i| c[i] - a[i]);
            let n = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let len = n.iter().map(|x| x * x).sum::<f64>().sqrt();
            if len <= 0. || !len.is_finite() {
                return Err("validation_failed: triangle normal".into());
            }
            for index in t.vertices {
                let p = points[index];
                triangles.push([
                    p[0] as f32,
                    p[1] as f32,
                    ((p[2] - origin_z) * projection.scale) as f32,
                    id as f32,
                ]);
            }
            let light = 0.35 + 0.65 * ((n[0] * 0.35 + n[1] * 0.25 + n[2] * 0.9028) / len).abs();
            triangles
                .push([112., 169., 212., 0.].map(|v| (v * light).clamp(0., 255.) as u8 as f32));
        }
        for edge in edges {
            let id = id_for(&mut edge_ids, &edge.reference);
            for pair in edge.points.windows(2) {
                for p in pair {
                    let p = projection.project(camera.transform(world(p)));
                    segments.push([
                        p[0] as f32,
                        p[1] as f32,
                        ((p[2] - origin_z) * projection.scale) as f32,
                        id as f32,
                    ]);
                }
            }
        }
        if triangles
            .iter()
            .chain(&segments)
            .flatten()
            .any(|n| !n.is_finite())
        {
            return Err("gpu_precision: projected coordinate exceeds f32 range".into());
        }
        let mut key = bytemuck::cast_slice(&triangles).to_vec();
        key.extend_from_slice(bytemuck::cast_slice(&segments));
        key.extend_from_slice(&(triangles.len() as u64).to_ne_bytes());
        let hit = self.cache.as_ref().is_some_and(|c| c.0 == key);
        if !hit {
            let storage = |data: &[u8]| {
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("CAD geometry"),
                        contents: if data.is_empty() { &[0u8; 32] } else { data },
                        usage: wgpu::BufferUsages::STORAGE,
                    })
            };
            self.cache = Some((
                key,
                storage(bytemuck::cast_slice(&triangles)),
                storage(bytemuck::cast_slice(&segments)),
            ));
            *uploads += 1;
        }
        let count = (width * height) as usize;
        let size = (count * 40) as u64;
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("CAD color/face/edge/depth"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mut params = vec![
            width,
            height,
            (triangles.len() / 4) as u32,
            (segments.len() / 2) as u32,
        ];
        params.extend([1f32.to_bits(), 0, 0, 0]);
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let (_, tri, seg) = self.cache.as_ref().unwrap();
        let entries: Vec<_> = [tri, seg, &output, &uniform]
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &entries,
        });
        let read = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("CAD readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for pipeline in [&self.surface, &self.edge] {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("CAD offscreen pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &read, 0, size);
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        read.slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(15)),
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(std::time::Duration::from_secs(1))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let bytes = read.slice(..).get_mapped_range();
        let mut rgb = Vec::with_capacity(count * 3);
        let mut faces = Vec::with_capacity(count);
        let mut edge_pixels = Vec::with_capacity(count);
        let mut candidates = BTreeMap::new();
        let mut truncated = BTreeSet::new();
        let mut depths = Vec::with_capacity(count * 3);
        let finite_z: Vec<_> = bytes
            .chunks_exact(40)
            .filter_map(|p| {
                let face = u32::from_ne_bytes(p[4..8].try_into().unwrap());
                (face != 0).then(|| f32::from_ne_bytes(p[12..16].try_into().unwrap()))
            })
            .collect();
        let lo = finite_z.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = finite_z.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        for (index, p) in bytes.chunks_exact(40).enumerate() {
            let words: Vec<_> = p
                .chunks_exact(4)
                .map(|v| u32::from_ne_bytes(v.try_into().unwrap()))
                .collect();
            rgb.extend([
                words[0] as u8,
                (words[0] >> 8) as u8,
                (words[0] >> 16) as u8,
            ]);
            faces.push(words[1]);
            edge_pixels.push(words[2]);
            if words[2] == AMBIGUOUS {
                candidates.insert(
                    index,
                    words[4..7].iter().copied().filter(|id| *id != 0).collect(),
                );
            }
            if words[7] != 0 {
                truncated.insert(index);
            }
            let gray = if words[1] == 0 {
                0
            } else {
                (1. + 254. * ((f32::from_bits(words[3]) - lo) / (hi - lo).max(1e-6))) as u8
            };
            depths.extend([gray; 3]);
        }
        drop(bytes);
        read.unmap();
        let face_refs = face_ids.into_iter().map(|(k, v)| (v, k)).collect();
        let edge_refs = edge_ids.into_iter().map(|(k, v)| (v, k)).collect();
        let frame = Frame {
            metadata: json!({"width":width,"height":height,"backend":"wgpu","adapter":self.info,"mime_type":"image/png","camera":{"projection":"orthographic","direction":camera.direction(),"yaw":camera.yaw,"pitch":camera.pitch,"zoom":camera.zoom,"pan":camera.pan,"scale":projection.scale,"center":projection.center},"face_ids":{},"edge_ids":{},"geometry_cache_hit":hit}),
            rgb,
            depth_rgb: depths,
            width,
            height,
            faces,
            edges: edge_pixels,
            face_refs,
            edge_refs,
            candidates,
            truncated,
        };
        let mut frame = frame;
        frame.metadata["face_ids"] = json!(frame.face_refs);
        frame.metadata["edge_ids"] = json!(frame.edge_refs);
        Ok(frame)
    }
}
