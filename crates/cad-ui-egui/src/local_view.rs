//! Client-side depth-tested CAD rasterization. No network or GPU readback in the orbit loop.
use cad_protocol::Scene;
use cad_render::Camera;
use egui_wgpu::{wgpu, Callback, CallbackResources, CallbackTrait, RenderState, ScreenDescriptor};
use std::sync::Arc;
use wgpu::util::DeviceExt;

struct Geometry {
    scene: Arc<Scene>,
    selected: Option<String>,
    hovered: Option<String>,
    vertices: wgpu::Buffer,
    faces: u32,
    edges: u32,
}
struct Resources {
    faces: wgpu::RenderPipeline,
    edges: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    geometry: Option<Geometry>,
}
/// The owning eframe render pass must have a 24-bit depth buffer, cleared each frame.
pub fn install(state: &RenderState) {
    let device = &state.device;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("CAD local raster"),
        source: wgpu::ShaderSource::Wgsl(include_str!("local_view.wgsl").into()),
    });
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("CAD camera"),
        contents: &[0; 64],
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("CAD camera"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("CAD camera"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("CAD local raster"),
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    let pipeline = |edges| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(if edges {
                "CAD visible edges"
            } else {
                "CAD shaded faces"
            }),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(if edges { "vs_edge" } else { "vs" }),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 44,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32x2],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(if state.target_format.is_srgb() {
                    "fs_srgb"
                } else {
                    "fs"
                }),
                targets: &[Some(state.target_format.into())],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth24Plus,
                depth_write_enabled: !edges,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        })
    };
    state.renderer.write().callback_resources.insert(Resources {
        faces: pipeline(false),
        edges: pipeline(true),
        uniform,
        bind,
        geometry: None,
    });
}

pub struct View {
    pub scene: Arc<Scene>,
    pub camera: Camera,
    pub selected: Option<String>,
    pub hovered: Option<String>,
    pub display_size: [f32; 2],
    pub size: [u32; 2],
}
impl View {
    pub fn callback(self, rect: egui::Rect) -> egui::PaintCallback {
        Callback::new_paint_callback(rect, self)
    }
}
fn bounds(scene: &Scene) -> Option<([f64; 3], f64)> {
    let mesh = scene.mesh.as_ref()?;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &mesh.points {
        for i in 0..3 {
            lo[i] = lo[i].min(p[i].get());
            hi[i] = hi[i].max(p[i].get());
        }
    }
    let center = std::array::from_fn(|i| lo[i] + (hi[i] - lo[i]) / 2.);
    let radius = mesh
        .points
        .iter()
        .map(|p| {
            (0..3)
                .map(|i| (p[i].get() - center[i]).powi(2))
                .sum::<f64>()
                .sqrt()
        })
        .fold(0f64, f64::max);
    (radius.is_finite() && radius > 0.).then_some((center, radius))
}
fn vertices(scene: &Scene, selected: Option<&str>, hovered: Option<&str>) -> (Vec<[f32; 11]>, u32) {
    let Some((center, radius)) = bounds(scene) else {
        return (Vec::new(), 0);
    };
    let mesh = scene.mesh.as_ref().unwrap();
    let mut out = Vec::new();
    let normalized =
        |p: [f64; 3]| std::array::from_fn::<_, 3, _>(|i| ((p[i] - center[i]) / radius) as f32);
    let vertex = |p: [f32; 3], color: [f32; 3], other: [f32; 3], corner: [f32; 2]| {
        [
            p[0], p[1], p[2], color[0], color[1], color[2], other[0], other[1], other[2],
            corner[0], corner[1],
        ]
    };
    for t in &mesh.triangles {
        if t.vertices.iter().any(|&i| i >= mesh.points.len()) {
            continue;
        }
        let [a, b, c] = t.vertices.map(|i| mesh.points[i].map(|v| v.get()));
        let u = std::array::from_fn::<_, 3, _>(|i| b[i] - a[i]);
        let v = std::array::from_fn::<_, 3, _>(|i| c[i] - a[i]);
        let n = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let length = n.iter().map(|n| n * n).sum::<f64>().sqrt();
        if length <= f64::EPSILON {
            continue;
        }
        let light =
            (0.35 + 0.65 * ((n[0] * 0.35 + n[1] * 0.25 + n[2] * 0.9028) / length).abs()) as f32;
        let color = [112., 169., 212.].map(|c| c * light / 255.);
        out.extend([a, b, c].map(|p| vertex(normalized(p), color, [0.; 3], [0.; 2])));
    }
    let faces = out.len() as u32;
    for edge in &scene.edges {
        let (color, half_width) = if selected == Some(edge.reference.as_str()) {
            ([255., 189., 64.], 2.5)
        } else if hovered == Some(edge.reference.as_str()) {
            ([44., 230., 210.], 1.8)
        } else {
            ([52., 72., 89.], 0.65)
        };
        let color = color.map(|v| v / 255.);
        for pair in edge.points.windows(2) {
            let a = normalized(pair[0].map(|v| v.get()));
            let b = normalized(pair[1].map(|v| v.get()));
            for corner in [
                [0., -half_width],
                [0., half_width],
                [1., half_width],
                [0., -half_width],
                [1., half_width],
                [1., -half_width],
            ] {
                out.push(vertex(a, color, b, corner));
            }
        }
    }
    (out, faces)
}
fn camera_uniform(camera: Camera, size: [u32; 2]) -> [f32; 12] {
    let [w, h] = size.map(|v| v.max(1) as f64);
    let scale = camera.zoom * (w.min(h) - 72.).max(16.);
    let (sy, cy) = camera.yaw.sin_cos();
    let (sp, cp) = camera.pitch.sin_cos();
    [
        sy * scale / w,
        -cy * scale / w,
        0.,
        2. * camera.pan[0],
        -sp * cy * scale / h,
        -sp * sy * scale / h,
        cp * scale / h,
        -2. * camera.pan[1],
        -cp * cy * 0.25,
        -cp * sy * 0.25,
        -sp * 0.25,
        0.5,
    ]
    .map(|v| v as f32)
}
impl CallbackTrait for View {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _: &ScreenDescriptor,
        _: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let resources = resources.get_mut::<Resources>().unwrap();
        if resources.geometry.as_ref().is_none_or(|g| {
            !Arc::ptr_eq(&g.scene, &self.scene)
                || g.selected != self.selected
                || g.hovered != self.hovered
        }) {
            let (vertices, faces) = vertices(
                &self.scene,
                self.selected.as_deref(),
                self.hovered.as_deref(),
            );
            let count = vertices.len() as u32;
            let bytes: &[u8] = if vertices.is_empty() {
                &[0; 44]
            } else {
                bytemuck::cast_slice(&vertices)
            };
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("CAD tessellation"),
                size: bytes.len() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buffer, 0, bytes);
            resources.geometry = Some(Geometry {
                scene: self.scene.clone(),
                selected: self.selected.clone(),
                hovered: self.hovered.clone(),
                vertices: buffer,
                faces,
                edges: count - faces,
            });
        }
        let mut uniform = camera_uniform(self.camera, self.size).to_vec();
        uniform.extend([self.display_size[0], self.display_size[1], 0., 0.]);
        queue.write_buffer(&resources.uniform, 0, bytemuck::cast_slice(&uniform));
        Vec::new()
    }
    fn paint(
        &self,
        _: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let resources = resources.get::<Resources>().unwrap();
        let Some(geometry) = &resources.geometry else {
            return;
        };
        pass.set_bind_group(0, &resources.bind, &[]);
        pass.set_vertex_buffer(0, geometry.vertices.slice(..));
        pass.set_pipeline(&resources.faces);
        pass.draw(0..geometry.faces, 0..1);
        pass.set_pipeline(&resources.edges);
        pass.draw(geometry.faces..geometry.faces + geometry.edges, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_rotation_keeps_pivot_fixed_and_depth_inside_clip_volume() {
        for yaw in [0., 0.7, 2.5, 5.] {
            let camera = Camera {
                yaw,
                pan: [0.1, -0.2],
                ..Default::default()
            };
            let m = camera_uniform(camera, [640, 480]);
            assert!((m[3] - 0.2).abs() < 1e-6);
            assert!((m[7] - 0.4).abs() < 1e-6);
            for axis in [[1., 0., 0.], [-1., 0., 0.], [0., 1., 0.], [0., 0., -1.]] {
                let z = m[8] * axis[0] + m[9] * axis[1] + m[10] * axis[2] + m[11];
                assert!((0.24..=0.76).contains(&z));
            }
        }
    }
}
