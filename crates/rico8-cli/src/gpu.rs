//! GPU presentation: upload the 128x128 indexed framebuffer as an RGBA
//! texture and blit it with nearest-neighbor filtering at the largest
//! integer scale that fits, letterboxed in black. The GPU does nothing
//! clever on purpose — every pixel is decided by the software rasterizer.

use anyhow::{anyhow, Result};
use rico8_runtime::fb::{Framebuffer, HEIGHT, WIDTH};
use std::sync::Arc;
use winit::window::Window;

const SHADER: &str = r#"
@group(0) @binding(0) var screen_tex: texture_2d<f32>;
@group(0) @binding(1) var screen_samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    // Fullscreen triangle; the viewport restricts it to the screen rect.
    var out: VsOut;
    let x = f32(i32(i & 1u) * 4 - 1);
    let y = f32(i32(i >> 1u) * 4 - 1);
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, 1.0 - (y + 1.0) * 0.5);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(screen_tex, screen_samp, in.uv);
}
"#;

/// The rectangle (in physical pixels) where the virtual screen lands.
#[derive(Clone, Copy, Default)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Viewport {
    /// Map a physical window position to virtual screen coordinates.
    pub fn window_to_screen(&self, px: f64, py: f64) -> (i32, i32) {
        if self.w <= 0.0 || self.h <= 0.0 {
            return (0, 0);
        }
        let x = ((px as f32 - self.x) / self.w * WIDTH as f32).floor() as i32;
        let y = ((py as f32 - self.y) / self.h * HEIGHT as f32).floor() as i32;
        (x, y)
    }
}

pub struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    texture: wgpu::Texture,
    rgba: Vec<u8>,
    viewport: Viewport,
}

impl Gpu {
    pub fn new(
        window: Arc<Window>,
        display: winit::event_loop::OwnedDisplayHandle,
    ) -> Result<Self> {
        pollster::block_on(Self::new_async(window, display))
    }

    async fn new_async(
        window: Arc<Window>,
        display: winit::event_loop::OwnedDisplayHandle,
    ) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display)).with_env(),
        );
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| anyhow!("no graphics adapter found: {e}"))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .map_err(|e| anyhow!("could not create graphics device: {e}"))?;

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow!("surface is not supported by the adapter"))?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("rico8 screen"),
            size: wgpu::Extent3d {
                width: WIDTH as u32,
                height: HEIGHT as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rico8 blit"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rico8 blit"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group,
            texture,
            rgba: vec![0; WIDTH as usize * HEIGHT as usize * 4],
            viewport: Viewport::default(),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    fn compute_viewport(&mut self) {
        let (sw, sh) = (self.config.width as f32, self.config.height as f32);
        // Largest integer scale that fits; fall back to fractional fit on
        // windows smaller than 128 px.
        let scale = (sw / WIDTH as f32).min(sh / HEIGHT as f32);
        let scale = if scale >= 1.0 { scale.floor() } else { scale };
        let w = WIDTH as f32 * scale;
        let h = HEIGHT as f32 * scale;
        self.viewport = Viewport {
            x: ((sw - w) / 2.0).floor(),
            y: ((sh - h) / 2.0).floor(),
            w,
            h,
        };
    }

    pub fn render(&mut self, fb: &Framebuffer) -> Result<()> {
        fb.write_rgba(&mut self.rgba);
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH as u32 * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: WIDTH as u32,
                height: HEIGHT as u32,
                depth_or_array_layers: 1,
            },
        );

        use wgpu::CurrentSurfaceTexture as Cst;
        let frame = match self.surface.get_current_texture() {
            Cst::Success(f) | Cst::Suboptimal(f) => f,
            Cst::Lost | Cst::Outdated => {
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    Cst::Success(f) | Cst::Suboptimal(f) => f,
                    _ => return Ok(()), // skip this frame
                }
            }
            // Nothing to draw to right now; skip the frame.
            Cst::Timeout | Cst::Occluded | Cst::Validation => return Ok(()),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rico8 blit"),
                multiview_mask: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.compute_viewport();
            let vp = self.viewport;
            pass.set_viewport(vp.x, vp.y, vp.w, vp.h, 0.0, 1.0);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }
}
