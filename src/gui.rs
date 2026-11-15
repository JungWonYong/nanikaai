use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use arboard::Clipboard;
use cosmic_text::{
    Attrs, Buffer, Color as TextColor, Family, FontSystem, Metrics, Shaping, SwashCache, Wrap,
};
use mouse_position::mouse_position::Mouse;
use serde_json::{json, Value};
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon as TrayIconImage, TrayIcon, TrayIconBuilder};
use wgpu::util::DeviceExt;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

#[cfg(target_os = "macos")]
use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS, WindowAttributesExtMacOS};
#[cfg(target_os = "windows")]
use winit::platform::windows::WindowAttributesExtWindows;

use crate::agent_engine::{provider_catalog, AgentEngine, AgentReply, StatusCallback};
use crate::builtin_tools::BuiltinToolRunner;
use crate::config_store::{default_user_data_dir, legacy_user_data_dir, ConfigPatch, ConfigStore};
use crate::dialogue_parser::{parse_dialogue, ActionKind, DialogueAction, Scope};
use crate::shell_parser::{
    install_nar_archive, BalloonManager, BalloonSkin, BalloonSummary, CollisionRect, RgbaBitmap,
    ShellManifest, ShellManager, ShellSummary, StrokeDetector,
};
use crate::sstp_server::{start_dual_server, ServerBackend, ServerHandle};

const POLL_INTERVAL_MS: u64 = 50;
const TYPEWRITER_INTERVAL_MS: u64 = 24;

#[repr(C)]
#[derive(Copy, Clone)]
struct ShaderParams {
    premul: u32,
    _pad: [u32; 3],
}

fn as_bytes_shader_params(p: &ShaderParams) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[0..4].copy_from_slice(&p.premul.to_ne_bytes());
    out
}

const QUAD_SHADER_WGSL: &str = r#"
struct Params {
    premul: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var u_tex: texture_2d<f32>;
@group(0) @binding(1) var u_samp: sampler;
@group(0) @binding(2) var<uniform> u_params: Params;

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 3.0,  1.0),
        vec2<f32>(-1.0, -3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(positions[vid], 0.0, 1.0);
    out.uv = uvs[vid];
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(u_tex, u_samp, in.uv);
    if (u_params.premul != 0u) {
        return vec4<f32>(c.rgb * c.a, c.a);
    }
    return c;
}
"#;

pub fn resolve_assets_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let mac_res = dir.join("../Resources/assets");
            if mac_res.join("shells").is_dir() {
                return mac_res;
            }
            let local_assets = dir.join("assets");
            if local_assets.join("shells").is_dir() {
                return local_assets;
            }
        }
    }
    let manifest_assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
    if manifest_assets.join("shells").is_dir() {
        return manifest_assets;
    }
    PathBuf::from("assets")
}

struct GpuContext {
    _instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    params_buffer: wgpu::Buffer,
    surface_format: wgpu::TextureFormat,
    texture_format: wgpu::TextureFormat,
    alpha_mode: wgpu::CompositeAlphaMode,
}

impl GpuContext {
    fn new(instance: wgpu::Instance, probe_surface: &wgpu::Surface<'_>) -> Result<Self, String> {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(probe_surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| "사용 가능한 GPU 어댑터를 찾을 수 없습니다.".to_string())?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("nanikaai-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
            },
            None,
        ))
        .map_err(|e| format!("GPU 디바이스 생성 실패: {e}"))?;

        let caps = probe_surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Rgba8UnormSrgb
                )
            })
            .unwrap_or_else(|| caps.formats[0]);

        let alpha_mode = [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ]
        .into_iter()
        .find(|m| caps.alpha_modes.contains(m))
        .unwrap_or(caps.alpha_modes[0]);

        let texture_format = if surface_format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };

        let premul = if alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied {
            1
        } else {
            0
        };
        let params = ShaderParams {
            premul,
            _pad: [0; 3],
        };
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shader-params"),
            contents: &as_bytes_shader_params(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("quad-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad-bgl"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad-shader"),
            source: wgpu::ShaderSource::Wgsl(QUAD_SHADER_WGSL.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("quad-pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quad-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Ok(Self {
            _instance: instance,
            device,
            queue,
            pipeline,
            bind_group_layout,
            sampler,
            params_buffer,
            surface_format,
            texture_format,
            alpha_mode,
        })
    }

    fn make_surface_config(&self, phys_w: u32, phys_h: u32) -> wgpu::SurfaceConfiguration {
        wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.surface_format,
            width: phys_w.max(1),
            height: phys_h.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: self.alpha_mode,
            view_formats: vec![],
        }
    }
}

struct WindowSurface {
    // Drop `surface` before `window`!
    surface: wgpu::Surface<'static>,
    window: Arc<Window>,
    config: wgpu::SurfaceConfiguration,
    texture: Option<wgpu::Texture>,
    bind_group: Option<wgpu::BindGroup>,
    tex_size: (u32, u32),
}

impl WindowSurface {
    fn from_existing(
        surface: wgpu::Surface<'static>,
        window: Arc<Window>,
        gpu: &GpuContext,
    ) -> Self {
        let size = window.inner_size();
        let config = gpu.make_surface_config(size.width, size.height);
        surface.configure(&gpu.device, &config);
        Self {
            surface,
            window,
            config,
            texture: None,
            bind_group: None,
            tex_size: (0, 0),
        }
    }

    fn resize(&mut self, gpu: &GpuContext, phys: PhysicalSize<u32>) {
        let w = phys.width.max(1);
        let h = phys.height.max(1);
        if self.config.width != w || self.config.height != h {
            self.config.width = w;
            self.config.height = h;
            self.surface.configure(&gpu.device, &self.config);
        }
    }

    fn upload_rgba(&mut self, gpu: &GpuContext, bmp: &RgbaBitmap) {
        let w = bmp.width.max(1);
        let h = bmp.height.max(1);
        if self.texture.is_none() || self.tex_size != (w, h) {
            let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("ui-sprite-tex"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: gpu.texture_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
            let bg = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ui-sprite-bg"),
                layout: &gpu.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&gpu.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: gpu.params_buffer.as_entire_binding(),
                    },
                ],
            });
            self.texture = Some(tex);
            self.bind_group = Some(bg);
            self.tex_size = (w, h);
        }
        if let Some(ref tex) = self.texture {
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &bmp.data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * w),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    fn render(&mut self, gpu: &GpuContext) {
        let Some(ref bg) = self.bind_group else {
            return;
        };
        let frame = match self.surface.get_current_texture() {
            Ok(f) => f,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&gpu.device, &self.config);
                return;
            }
            Err(_) => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("quad-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("quad-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&gpu.pipeline);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }
}

struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl TextRenderer {
    fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
        }
    }

    fn measure_lines(&mut self, text: &str, font_px: f32, line_h: f32, max_w: f32) -> (usize, f32) {
        let mut buf = Buffer::new(&mut self.font_system, Metrics::new(font_px, line_h));
        buf.set_wrap(&mut self.font_system, Wrap::WordOrGlyph);
        buf.set_size(&mut self.font_system, Some(max_w.max(20.0)), None);
        buf.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        buf.shape_until_scroll(&mut self.font_system, false);
        let mut lines = 0usize;
        for _run in buf.layout_runs() {
            lines += 1;
        }
        let lines = lines.max(1);
        (lines, (lines as f32) * line_h)
    }

    fn draw_text(
        &mut self,
        bmp: &mut RgbaBitmap,
        text: &str,
        x: i32,
        y: i32,
        max_w: f32,
        max_h: Option<f32>,
        font_px: f32,
        line_h: f32,
        rgb: (u8, u8, u8),
        scroll_px: f32,
    ) -> f32 {
        let mut buf = Buffer::new(&mut self.font_system, Metrics::new(font_px, line_h));
        buf.set_wrap(&mut self.font_system, Wrap::WordOrGlyph);
        buf.set_size(&mut self.font_system, Some(max_w.max(20.0)), max_h);
        buf.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        buf.shape_until_scroll(&mut self.font_system, false);
        let color = TextColor::rgba(rgb.0, rgb.1, rgb.2, 255);
        let clip_top = y;
        let clip_bot = max_h
            .map(|h| y + h.ceil() as i32)
            .unwrap_or(bmp.height as i32);
        let mut total_lines = 0usize;
        for _ in buf.layout_runs() {
            total_lines += 1;
        }
        let off_y = y - scroll_px.round() as i32;
        buf.draw(&mut self.font_system, &mut self.swash_cache, color, |gx, gy, _gw, _gh, col| {
            let py = off_y + gy;
            if py < clip_top || py >= clip_bot {
                return;
            }
            let px = x + gx;
            if px < 0 || px >= bmp.width as i32 || py < 0 || py >= bmp.height as i32 {
                return;
            }
            let (r, g, b, a) = col.as_rgba_tuple();
            if a > 0 {
                bmp.blend_pixel(px, py, r, g, b, a);
            }
        });
        (total_lines.max(1) as f32) * line_h
    }
}

#[derive(Clone, Debug)]
struct ActiveSerikoRun {
    steps: Vec<(i32, String, i32, i32, i32, u64)>,
    idx: usize,
    next_step_at: Instant,
}

struct CharacterWindowState {
    scope: Scope,
    ws: WindowSurface,
    surface_id: i32,
    display_scale: f64,
    base_bmp: Option<RgbaBitmap>,
    overlays: HashMap<i32, (i32, i32, RgbaBitmap)>,
    composited: RgbaBitmap,
    win_size: (u32, u32),
    logical_pos: (f64, f64),
    anim_next: HashMap<(i32, i32), Instant>,
    active_run: Option<ActiveSerikoRun>,
    lip_on: bool,
    next_lip_at: Instant,
    stroke_detector: StrokeDetector,
    last_hittest: Option<bool>,
    last_local_cursor: Option<(f64, f64)>,
    dragging: bool,
    drag_moved: bool,
    drag_start_cursor_global: (f64, f64),
    drag_start_win_pos: (f64, f64),
    last_click_at: Option<Instant>,
    visible: bool,
}

impl CharacterWindowState {
    fn sync_pos_from_window(&mut self) {
        if let Ok(pos) = self.ws.window.outer_position() {
            let l = pos.to_logical::<f64>(self.ws.window.scale_factor().max(1.0));
            self.logical_pos = (l.x, l.y);
        }
    }

    fn set_logical_pos(&mut self, x: f64, y: f64) {
        self.logical_pos = (x, y);
        self.ws.window.set_outer_position(LogicalPosition::new(x, y));
    }

    fn is_full_feet(&self) -> bool {
        let w = self.composited.width as i32;
        let h = self.composited.height as i32;
        if w <= 0 || h < 4 {
            return false;
        }
        for y in (h - 2)..h {
            for x in 0..w {
                if self.composited.is_opaque_at(x, y) {
                    return false;
                }
            }
        }
        true
    }

    fn foot_bottom_offset_logical(&self) -> f64 {
        let Some((_x0, _y0, _x1, y1)) = self.composited.opaque_bbox() else {
            return 0.0;
        };
        let pad_px = ((self.composited.height as i32) - 1 - y1).max(0) as f64;
        pad_px / self.display_scale.max(0.5)
    }

    fn set_surface(
        &mut self,
        manifest: &mut ShellManifest,
        gpu: &GpuContext,
        requested_id: i32,
        reset_pos: bool,
    ) {
        self.display_scale = manifest.descript.display_scale.clamp(0.5, 4.0);
        let resolved = manifest.resolve_surface_id(self.scope, requested_id);
        let base = manifest
            .compose_base_rgba(resolved)
            .unwrap_or_else(|| RgbaBitmap::from_solid(180, 260, 120, 140, 200, 220));
        self.surface_id = resolved;
        self.base_bmp = Some(base);
        self.overlays.clear();
        self.active_run = None;
        self.rebuild_composited(gpu, reset_pos);
        self.schedule_seriko_timers(manifest);
    }

    fn schedule_seriko_timers(&mut self, manifest: &ShellManifest) {
        let now = Instant::now();
        if let Some(sdef) = manifest.surfaces.get(&self.surface_id) {
            for anim in &sdef.animations {
                let key = (self.surface_id, anim.aid);
                if self.anim_next.contains_key(&key) {
                    continue;
                }
                let iv = anim.interval.as_str();
                if iv.starts_with("sometimes")
                    || iv.starts_with("rarely")
                    || iv.starts_with("random")
                    || iv.starts_with("periodic")
                    || iv == "always"
                    || iv == "runonce"
                {
                    let delay_ms = if iv == "runonce" || iv == "always" {
                        350
                    } else if iv.starts_with("rarely") {
                        11_000
                    } else if iv.starts_with("sometimes") {
                        5_500
                    } else if let Some((_, n_str)) = iv.split_once(',') {
                        let secs = n_str.trim().parse::<u64>().unwrap_or(5).max(1);
                        secs * 1000
                    } else {
                        6_000
                    };
                    self.anim_next
                        .insert(key, now + Duration::from_millis(delay_ms));
                }
            }
        }
    }

    fn tick_seriko(&mut self, manifest: &ShellManifest, gpu: &GpuContext, is_speaking_here: bool) {
        let now = Instant::now();
        let mut dirty = false;

        // Subtle lip-sync overlay if classic shell has talk animation or surface+1 exists? No, keep pure SERIKO!
        if is_speaking_here && now >= self.next_lip_at {
            self.lip_on = !self.lip_on;
            self.next_lip_at = now + Duration::from_millis(160);
        }

        if let Some(mut run) = self.active_run.take() {
            if now >= run.next_step_at {
                if run.idx < run.steps.len() {
                    let (pid, ref method, sid, ox, oy, delay_ms) = run.steps[run.idx].clone();
                    match method.as_str() {
                        "overlay" | "overlayfast" => {
                            if sid < 0 {
                                self.overlays.remove(&pid);
                            } else if let Some(ov_bmp) = manifest.compose_base_rgba(sid) {
                                self.overlays.insert(pid, (ox, oy, ov_bmp));
                            }
                            dirty = true;
                        }
                        "base" => {
                            if sid >= 0 {
                                if let Some(new_base) = manifest.compose_base_rgba(sid) {
                                    self.base_bmp = Some(new_base);
                                    dirty = true;
                                }
                            }
                        }
                        _ => {}
                    }
                    run.idx += 1;
                    if run.idx < run.steps.len() {
                        run.next_step_at = now + Duration::from_millis(delay_ms.max(40));
                        self.active_run = Some(run);
                    }
                }
            } else {
                self.active_run = Some(run);
            }
        }

        if self.active_run.is_none() {
            let mut fire_aid = None;
            if let Some(sdef) = manifest.surfaces.get(&self.surface_id) {
                for anim in &sdef.animations {
                    let key = (self.surface_id, anim.aid);
                    if let Some(&due) = self.anim_next.get(&key) {
                        if now >= due {
                            fire_aid = Some(anim.aid);
                            let next_ms = if anim.interval == "runonce" {
                                86_400_000
                            } else if anim.interval.starts_with("rarely") {
                                12_000 + (rand::random::<u64>() % 8_000)
                            } else if anim.interval.starts_with("sometimes") {
                                4_500 + (rand::random::<u64>() % 5_000)
                            } else if let Some((_, n_str)) = anim.interval.split_once(',') {
                                n_str.trim().parse::<u64>().unwrap_or(5).max(1) * 1000
                            } else {
                                5_500
                            };
                            self.anim_next
                                .insert(key, now + Duration::from_millis(next_ms));
                            break;
                        }
                    }
                }
                if let Some(aid) = fire_aid {
                    if let Some(anim) = sdef.animations.iter().find(|a| a.aid == aid) {
                        let steps: Vec<_> = anim
                            .patterns
                            .iter()
                            .enumerate()
                            .map(|(i, p)| {
                                (
                                    aid * 100 + (i as i32).min(99),
                                    p.method.clone(),
                                    p.surface_id,
                                    p.offset_x,
                                    p.offset_y,
                                    p.delay_ms.max(60),
                                )
                            })
                            .collect();
                        if !steps.is_empty() {
                            self.active_run = Some(ActiveSerikoRun {
                                steps,
                                idx: 0,
                                next_step_at: now,
                            });
                        }
                    }
                }
            }
        }

        if dirty {
            self.rebuild_composited(gpu, false);
        }
    }

    fn rebuild_composited(&mut self, gpu: &GpuContext, skip_anchor: bool) {
        let Some(base) = self.base_bmp.as_ref() else {
            return;
        };
        let mut comp = base.clone();
        let mut keys: Vec<i32> = self.overlays.keys().copied().collect();
        keys.sort_unstable();
        for k in keys {
            if let Some((ox, oy, ref ov)) = self.overlays.get(&k) {
                comp.alpha_composite(ov, *ox, *oy);
            }
        }
        let ds = self.display_scale.clamp(0.5, 4.0);
        let new_w = ((comp.width as f64) / ds).round().max(40.0) as u32;
        let new_h = ((comp.height as f64) / ds).round().max(40.0) as u32;
        if self.win_size != (new_w, new_h) {
            let old = self.win_size;
            if !skip_anchor && old.0 > 0 && old.1 > 0 {
                self.sync_pos_from_window();
                let nx = self.logical_pos.0 + (old.0 as f64 - new_w as f64) / 2.0;
                let ny = self.logical_pos.1 + (old.1 as f64 - new_h as f64);
                let _ = self
                    .ws
                    .window
                    .request_inner_size(LogicalSize::new(new_w, new_h));
                self.set_logical_pos(nx, ny);
            } else {
                let _ = self
                    .ws
                    .window
                    .request_inner_size(LogicalSize::new(new_w, new_h));
            }
            self.win_size = (new_w, new_h);
        }
        let phys = self.ws.window.inner_size();
        self.ws.resize(gpu, phys);
        self.ws.upload_rgba(gpu, &comp);
        self.composited = comp;
        self.ws.window.request_redraw();
    }
}

#[derive(Clone, Debug)]
struct ChoiceItem {
    label: String,
    choice_id: String,
    rect: (f64, f64, f64, f64), // logical coords inside balloon window
}

struct BalloonWindowState {
    scope: Scope,
    ws: WindowSurface,
    visible: bool,
    side: String,
    large: bool,
    text: String,
    choices: Vec<ChoiceItem>,
    hovered_choice: Option<usize>,
    status_line: String,
    scroll_px: f32,
    max_scroll_px: f32,
    win_size: (u32, u32),
    last_render_bmp: RgbaBitmap,
}

impl BalloonWindowState {
    fn redraw_content(
        &mut self,
        skin: &BalloonSkin,
        text_renderer: &mut TextRenderer,
        gpu: &GpuContext,
    ) {
        let (frame_raw, metrics) = skin.load_frame_rgba(self.scope, &self.side, self.large);
        let b_scale = skin.display_scale.clamp(0.5, 4.0) as f32;
        let logical_w = ((frame_raw.width as f32) / b_scale).round().max(160.0) as u32;
        let logical_h = ((frame_raw.height as f32) / b_scale).round().max(80.0) as u32;
        if self.win_size != (logical_w, logical_h) {
            let _ = self
                .ws
                .window
                .request_inner_size(LogicalSize::new(logical_w, logical_h));
            self.win_size = (logical_w, logical_h);
        }

        let scale = self.ws.window.scale_factor().clamp(1.0, 3.0) as f32;
        let phys_w = ((logical_w as f32) * scale).round().max(1.0) as u32;
        let phys_h = ((logical_h as f32) * scale).round().max(1.0) as u32;
        let mut canvas = frame_raw.resized(phys_w, phys_h);

        let left = (metrics.origin_x.max(12) as f32) * scale;
        let top = (metrics.origin_y.max(10) as f32) * scale;
        let right = if metrics.wordwrappoint_x < 0 {
            (logical_w as i32 + metrics.wordwrappoint_x).max(60) as f32 * scale
        } else {
            metrics.wordwrappoint_x.max(60) as f32 * scale
        };
        let bot = if metrics.validrect_bottom < 0 {
            (logical_h as i32 + metrics.validrect_bottom).max(40) as f32 * scale
        } else {
            metrics.validrect_bottom.max(40) as f32 * scale
        };
        let status_reserve = if self.status_line.is_empty() {
            0.0
        } else {
            18.0 * scale
        };
        let usable_w = (right - left).max(80.0);
        let usable_h = (bot - top - status_reserve).max(36.0);

        let font_px = (skin.font_height.clamp(11, 16) as f32) * scale;
        let line_h = font_px * 1.42;

        // Calculate needed content height so scroll bounds stay accurate
        let (_, text_h) = text_renderer.measure_lines(&self.text, font_px, line_h, usable_w);
        let choice_h = (self.choices.len() as f32) * (line_h + 6.0 * scale);
        let total_content_h = text_h + if self.choices.is_empty() { 0.0 } else { 8.0 * scale + choice_h };
        self.max_scroll_px = (total_content_h - usable_h).max(0.0);
        self.scroll_px = self.scroll_px.clamp(0.0, self.max_scroll_px);

        let drawn_h = text_renderer.draw_text(
            &mut canvas,
            &self.text,
            left.round() as i32,
            top.round() as i32,
            usable_w,
            Some(usable_h),
            font_px,
            line_h,
            skin.font_rgb,
            self.scroll_px,
        );

        let mut y_cursor = top + drawn_h + 6.0 * scale - self.scroll_px;
        let bot_i = (bot - status_reserve) as f64;
        for (idx, ch) in self.choices.iter_mut().enumerate() {
            let item_h = line_h + 4.0 * scale;
            if y_cursor + item_h >= top && (y_cursor as f64) <= bot_i {
                let hovered = self.hovered_choice == Some(idx);
                if hovered {
                    canvas.fill_rect_alpha(
                        left.round() as i32 - 4,
                        y_cursor.round() as i32 - 2,
                        usable_w.round() as i32 + 8,
                        item_h.round() as i32,
                        skin.cursor_rgb.0,
                        skin.cursor_rgb.1,
                        skin.cursor_rgb.2,
                        55,
                    );
                }
                let rgb = if hovered {
                    skin.anchor_rgb
                } else {
                    (
                        skin.anchor_rgb.0.min(40),
                        skin.anchor_rgb.1.max(85),
                        skin.anchor_rgb.2.max(185),
                    )
                };
                text_renderer.draw_text(
                    &mut canvas,
                    &format!("▸ {}", ch.label),
                    left.round() as i32,
                    y_cursor.round() as i32,
                    usable_w,
                    Some(line_h + 2.0 * scale),
                    font_px,
                    line_h,
                    rgb,
                    0.0,
                );
                ch.rect = (
                    (left / scale) as f64,
                    (y_cursor / scale) as f64,
                    ((left + usable_w) / scale) as f64,
                    ((y_cursor + item_h) / scale) as f64,
                );
            } else {
                ch.rect = (0.0, 0.0, 0.0, 0.0);
            }
            y_cursor += item_h + 2.0 * scale;
        }

        if !self.status_line.is_empty() {
            let st_font = (font_px * 0.85).max(10.0 * scale);
            text_renderer.draw_text(
                &mut canvas,
                &self.status_line,
                left.round() as i32,
                (bot - 15.0 * scale).round() as i32,
                usable_w,
                Some(16.0 * scale),
                st_font,
                st_font * 1.25,
                (110, 120, 145),
                0.0,
            );
        }

        if self.max_scroll_px > 1.0 {
            let track_x = (right + 6.0 * scale).min((phys_w as f32) - 10.0 * scale).round() as i32;
            let track_h = (usable_h - 4.0 * scale).max(20.0);
            let ratio = (self.scroll_px / self.max_scroll_px).clamp(0.0, 1.0);
            let thumb_h = (track_h * (usable_h / (usable_h + self.max_scroll_px))).max(18.0 * scale);
            let thumb_y = (top + ratio * (track_h - thumb_h)).round() as i32;
            canvas.fill_rect_alpha(
                track_x,
                top.round() as i32,
                (3.0 * scale).round().max(2.0) as i32,
                track_h.round() as i32,
                148,
                163,
                184,
                50,
            );
            canvas.fill_rect_alpha(
                track_x,
                thumb_y,
                (3.0 * scale).round().max(2.0) as i32,
                thumb_h.round() as i32,
                59,
                130,
                246,
                150,
            );
        }

        let phys = self.ws.window.inner_size();
        self.ws.resize(gpu, phys);
        self.ws.upload_rgba(gpu, &canvas);
        self.last_render_bmp = canvas;
        self.ws.window.request_redraw();
    }
}

struct CommunicateWindowState {
    ws: WindowSurface,
    visible: bool,
    text: String,
    ime_preedit: String,
    win_size: (u32, u32),
    cursor_on: bool,
    last_blink: Instant,
}

impl CommunicateWindowState {
    fn redraw(&mut self, skin: &BalloonSkin, text_renderer: &mut TextRenderer, gpu: &GpuContext) {
        let (logical_w, logical_h) = self.win_size;
        let scale = self.ws.window.scale_factor().clamp(1.0, 3.0) as f32;
        let pw = ((logical_w as f32) * scale).round() as u32;
        let ph = ((logical_h as f32) * scale).round() as u32;

        let mut canvas = if let Some(bg) = skin.load_communicate_rgba() {
            bg.resized(pw, ph)
        } else {
            let mut b = RgbaBitmap::from_solid(pw, ph, 248, 250, 255, 242);
            b.fill_rect_alpha(0, 0, pw as i32, (3.0 * scale) as i32, 95, 130, 210, 220);
            b
        };

        // Subtle inner input card border
        let pad = (12.0 * scale).round() as i32;
        let field_y = (28.0 * scale).round() as i32;
        let field_h = (ph as i32 - field_y - (14.0 * scale).round() as i32).max(24);
        let field_w = (pw as i32 - pad * 2).max(40);
        canvas.fill_rect_alpha(pad, field_y, field_w, field_h, 255, 255, 255, 235);
        canvas.fill_rect_alpha(pad, field_y + field_h - 2, field_w, 2, 110, 145, 225, 210);

        let header_font = 11.5 * scale;
        text_renderer.draw_text(
            &mut canvas,
            "💬 메시지 입력 (Enter 전송 / Esc 닫기)",
            pad,
            (8.0 * scale).round() as i32,
            field_w as f32,
            Some(18.0 * scale),
            header_font,
            header_font * 1.3,
            (70, 85, 115),
            0.0,
        );

        let caret = if self.cursor_on { "|" } else { "" };
        let shown = if self.ime_preedit.is_empty() {
            if self.text.is_empty() {
                "여기에 말을 걸어보세요...".to_string()
            } else {
                format!("{}{}", self.text, caret)
            }
        } else {
            format!("{}[{}]{}", self.text, self.ime_preedit, caret)
        };
        let text_rgb = if self.text.is_empty() && self.ime_preedit.is_empty() {
            (150, 155, 170)
        } else {
            (25, 28, 38)
        };
        let input_font = 13.5 * scale;
        text_renderer.draw_text(
            &mut canvas,
            &shown,
            pad + (8.0 * scale).round() as i32,
            field_y + (8.0 * scale).round() as i32,
            (field_w as f32 - 16.0 * scale).max(20.0),
            Some((field_h as f32 - 14.0 * scale).max(18.0)),
            input_font,
            input_font * 1.35,
            text_rgb,
            0.0,
        );

        let phys = self.ws.window.inner_size();
        self.ws.resize(gpu, phys);
        self.ws.upload_rgba(gpu, &canvas);
        self.ws.window.request_redraw();
    }
}

#[derive(Clone, Debug)]
enum MenuCommand {
    OpenCommunicate,
    SwitchShell(String),
    SwitchBalloon(String),
    SwitchProvider(String, String),
    ToggleKero,
    ToggleAlwaysOnTop,
    InstallNarDialog,
    ResetPositions,
    ClearHistory,
    Quit,
    Separator,
}

#[derive(Clone, Debug)]
struct MenuRow {
    label: String,
    cmd: MenuCommand,
}

struct MenuWindowState {
    ws: WindowSurface,
    visible: bool,
    rows: Vec<MenuRow>,
    hovered_idx: Option<usize>,
    scroll_row: usize,
    win_size: (u32, u32),
    row_h: f64,
    max_visible_rows: usize,
}

impl MenuWindowState {
    fn redraw(&mut self, text_renderer: &mut TextRenderer, gpu: &GpuContext) {
        let total = self.rows.len();
        let vis_count = total.min(self.max_visible_rows).max(1);
        let logical_w = 270u32;
        let logical_h = ((vis_count as f64) * self.row_h + 12.0).ceil() as u32;
        if self.win_size != (logical_w, logical_h) {
            let _ = self
                .ws
                .window
                .request_inner_size(LogicalSize::new(logical_w, logical_h));
            self.win_size = (logical_w, logical_h);
        }

        let scale = self.ws.window.scale_factor().clamp(1.0, 3.0) as f32;
        let pw = ((logical_w as f32) * scale).round() as u32;
        let ph = ((logical_h as f32) * scale).round() as u32;
        let mut canvas = RgbaBitmap::from_solid(pw, ph, 28, 31, 40, 245);
        // Subtle 1px slate border
        let pwi = pw as i32;
        let phi = ph as i32;
        canvas.fill_rect_alpha(0, 0, pwi, 2, 95, 110, 145, 220);
        canvas.fill_rect_alpha(0, phi - 2, pwi, 2, 95, 110, 145, 220);
        canvas.fill_rect_alpha(0, 0, 2, phi, 95, 110, 145, 220);
        canvas.fill_rect_alpha(pwi - 2, 0, 2, phi, 95, 110, 145, 220);

        let font_px = 12.5 * scale;
        let row_px = (self.row_h as f32) * scale;
        let end = (self.scroll_row + vis_count).min(self.rows.len());
        for (v_idx, r_idx) in (self.scroll_row..end).enumerate() {
            let y = (6.0 * scale) + (v_idx as f32) * row_px;
            let row = &self.rows[r_idx];
            if matches!(row.cmd, MenuCommand::Separator) {
                canvas.fill_rect_alpha(
                    (10.0 * scale) as i32,
                    (y + row_px * 0.5) as i32,
                    pwi - (20.0 * scale) as i32,
                    2,
                    90,
                    96,
                    118,
                    180,
                );
                continue;
            }
            if self.hovered_idx == Some(r_idx) {
                canvas.fill_rect_alpha(
                    (6.0 * scale) as i32,
                    y as i32,
                    pwi - (12.0 * scale) as i32,
                    row_px.round() as i32,
                    78,
                    112,
                    205,
                    210,
                );
            }
            let rgb = if row.label.starts_with('[') {
                (155, 185, 250)
            } else {
                (240, 243, 250)
            };
            text_renderer.draw_text(
                &mut canvas,
                &row.label,
                (14.0 * scale) as i32,
                (y + 4.0 * scale) as i32,
                pw as f32 - 28.0 * scale,
                Some(row_px),
                font_px,
                font_px * 1.25,
                rgb,
                0.0,
            );
        }

        let phys = self.ws.window.inner_size();
        self.ws.resize(gpu, phys);
        self.ws.upload_rgba(gpu, &canvas);
        self.ws.window.request_redraw();
    }
}

#[derive(Debug, Clone)]
enum PlaybackStep {
    Action(DialogueAction),
    Char(char),
}

struct PlaybackQueue {
    steps: VecDeque<PlaybackStep>,
    paused_until: Option<Instant>,
    next_char_at: Instant,
    active_speaker: Scope,
    auto_hide_at: Option<Instant>,
    sakura_buf: String,
    kero_buf: String,
    sakura_choices: Vec<ChoiceItem>,
    kero_choices: Vec<ChoiceItem>,
    sakura_dirty: bool,
    kero_dirty: bool,
    sakura_show: bool,
    kero_show: bool,
}

impl PlaybackQueue {
    fn new() -> Self {
        Self {
            steps: VecDeque::new(),
            paused_until: None,
            next_char_at: Instant::now(),
            active_speaker: Scope::Sakura,
            auto_hide_at: None,
            sakura_buf: String::new(),
            kero_buf: String::new(),
            sakura_choices: Vec::new(),
            kero_choices: Vec::new(),
            sakura_dirty: false,
            kero_dirty: false,
            sakura_show: false,
            kero_show: false,
        }
    }

    fn enqueue(&mut self, actions: Vec<DialogueAction>) {
        self.steps.clear();
        self.paused_until = None;
        self.auto_hide_at = None;
        self.active_speaker = Scope::Sakura;
        self.sakura_buf.clear();
        self.kero_buf.clear();
        self.sakura_choices.clear();
        self.kero_choices.clear();
        self.sakura_show = false;
        self.kero_show = false;
        self.sakura_dirty = true;
        self.kero_dirty = true;

        for act in actions {
            if act.kind == ActionKind::Text {
                for ch in act.text.chars() {
                    self.steps.push_back(PlaybackStep::Char(ch));
                }
            } else {
                self.steps.push_back(PlaybackStep::Action(act));
            }
        }
        self.next_char_at = Instant::now();
    }

    fn fast_forward(&mut self) -> Vec<DialogueAction> {
        let mut imm_actions = Vec::new();
        while let Some(step) = self.steps.pop_front() {
            match step {
                PlaybackStep::Char(ch) => {
                    if self.active_speaker == Scope::Kero {
                        self.kero_buf.push(ch);
                        self.kero_show = true;
                        self.kero_dirty = true;
                    } else {
                        self.sakura_buf.push(ch);
                        self.sakura_show = true;
                        self.sakura_dirty = true;
                    }
                }
                PlaybackStep::Action(act) => match act.kind {
                    ActionKind::Speaker => self.active_speaker = act.scope,
                    ActionKind::Clear => {
                        if self.active_speaker == Scope::Kero {
                            self.kero_buf.clear();
                            self.kero_choices.clear();
                            self.kero_dirty = true;
                        } else {
                            self.sakura_buf.clear();
                            self.sakura_choices.clear();
                            self.sakura_dirty = true;
                        }
                    }
                    ActionKind::Choice => {
                        let item = ChoiceItem {
                            label: act.label,
                            choice_id: act.choice_id,
                            rect: (0.0, 0.0, 0.0, 0.0),
                        };
                        if self.active_speaker == Scope::Kero {
                            self.kero_choices.push(item);
                            self.kero_show = true;
                            self.kero_dirty = true;
                        } else {
                            self.sakura_choices.push(item);
                            self.sakura_show = true;
                            self.sakura_dirty = true;
                        }
                    }
                    ActionKind::Pause => {}
                    ActionKind::Surface | ActionKind::BalloonSize => imm_actions.push(act),
                    ActionKind::Text => {}
                },
            }
        }
        self.paused_until = None;
        if self.sakura_choices.is_empty() && self.kero_choices.is_empty() {
            self.auto_hide_at = Some(Instant::now() + Duration::from_secs(18));
        }
        imm_actions
    }
}

#[derive(Debug)]
enum UserEvent {
    PlayActions(Vec<DialogueAction>),
    StatusUpdate(String, bool),
    ServerGetHealth(mpsc::Sender<Value>),
    ServerGetSkins(mpsc::Sender<Value>),
    ServerSwitchSkin {
        shell: Option<String>,
        balloon: Option<String>,
        reply: mpsc::Sender<Result<Value, String>>,
    },
    ServerSwitchProvider {
        provider: String,
        model: Option<String>,
        reply: mpsc::Sender<Result<Value, String>>,
    },
    TriggerSmokeStep,
}

#[derive(Clone)]
struct GuiServerBackend {
    proxy: EventLoopProxy<UserEvent>,
    config_store: ConfigStore,
    engine: AgentEngine,
}

impl ServerBackend for GuiServerBackend {
    fn get_health(&self) -> Value {
        let (tx, rx) = mpsc::channel();
        if self.proxy.send_event(UserEvent::ServerGetHealth(tx)).is_ok() {
            if let Ok(v) = rx.recv_timeout(Duration::from_secs(2)) {
                return v;
            }
        }
        let cfg = self.config_store.get();
        json!({
            "status": "ok",
            "provider": cfg.provider,
            "model": cfg.model,
            "shell": cfg.active_shell_id,
            "balloon": cfg.active_balloon_id,
        })
    }

    fn get_skins(&self) -> Value {
        let (tx, rx) = mpsc::channel();
        if self.proxy.send_event(UserEvent::ServerGetSkins(tx)).is_ok() {
            if let Ok(v) = rx.recv_timeout(Duration::from_secs(2)) {
                return v;
            }
        }
        json!({ "shells": [], "balloons": [] })
    }

    fn get_providers(&self) -> Value {
        let cfg = self.config_store.get();
        json!({
            "current": {
                "provider": cfg.provider,
                "model": cfg.model,
                "endpoint_url": cfg.endpoint_url,
                "remote_endpoint_url": cfg.remote_endpoint_url,
            },
            "providers": self.engine.get_providers_status(),
        })
    }

    fn on_speak(&self, script: &str) -> Vec<DialogueAction> {
        let actions = parse_dialogue(script);
        let _ = self.proxy.send_event(UserEvent::PlayActions(actions.clone()));
        actions
    }

    fn on_chat(&self, message: &str) -> AgentReply {
        let reply = self.engine.handle_user_message(message);
        let _ = self
            .proxy
            .send_event(UserEvent::PlayActions(reply.actions.clone()));
        reply
    }

    fn on_event(&self, event: &str, detail: &str) -> AgentReply {
        let reply = self.engine.handle_external_event(event, detail);
        let _ = self
            .proxy
            .send_event(UserEvent::PlayActions(reply.actions.clone()));
        reply
    }

    fn on_switch_skin(&self, shell: Option<String>, balloon: Option<String>) -> Result<Value, String> {
        let (tx, rx) = mpsc::channel();
        self.proxy
            .send_event(UserEvent::ServerSwitchSkin {
                shell,
                balloon,
                reply: tx,
            })
            .map_err(|e| format!("GUI 이벤트 전송 실패: {e}"))?;
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|e| format!("스킨 전환 타임아웃: {e}"))?
    }

    fn on_switch_provider(
        &self,
        provider: String,
        model: Option<String>,
    ) -> Result<Value, String> {
        let (tx, rx) = mpsc::channel();
        self.proxy
            .send_event(UserEvent::ServerSwitchProvider {
                provider,
                model,
                reply: tx,
            })
            .map_err(|e| format!("GUI 이벤트 전송 실패: {e}"))?;
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|e| format!("프로바이더 전환 타임아웃: {e}"))?
    }
}

#[derive(Clone, Debug)]
struct DesktopInsets {
    top: f64,
    left: f64,
    bottom: f64,
    right: f64,
    dock_centered_bottom: bool,
}

fn detect_os_insets() -> DesktopInsets {
    #[cfg(target_os = "macos")]
    {
        let read_pref = |key: &str| -> Option<String> {
            std::process::Command::new("defaults")
                .args(["read", "com.apple.dock", key])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        let autohide = read_pref("autohide").as_deref() == Some("1");
        let orientation = read_pref("orientation").unwrap_or_else(|| "bottom".to_string());
        let tilesize = read_pref("tilesize")
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(64.0)
            .clamp(32.0, 128.0);
        let dock_ext = if autohide { 14.0 } else { tilesize + 24.0 };
        let mut insets = DesktopInsets {
            top: 38.0,
            left: 16.0,
            bottom: 16.0,
            right: 16.0,
            dock_centered_bottom: false,
        };
        match orientation.as_str() {
            "left" => insets.left = dock_ext + 8.0,
            "right" => insets.right = dock_ext + 8.0,
            _ => {
                insets.bottom = dock_ext;
                insets.dock_centered_bottom = true;
            }
        }
        insets
    }
    #[cfg(target_os = "windows")]
    {
        DesktopInsets {
            top: 8.0,
            left: 16.0,
            bottom: 50.0,
            right: 16.0,
            dock_centered_bottom: false,
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_ascii_uppercase();
        if desktop.contains("UBUNTU") {
            DesktopInsets {
                top: 32.0,
                left: 72.0,
                bottom: 16.0,
                right: 16.0,
                dock_centered_bottom: false,
            }
        } else if desktop.contains("GNOME") {
            DesktopInsets {
                top: 32.0,
                left: 16.0,
                bottom: 72.0,
                right: 16.0,
                dock_centered_bottom: true,
            }
        } else if desktop.contains("KDE")
            || desktop.contains("PLASMA")
            || desktop.contains("XFCE")
            || desktop.contains("CINNAMON")
            || desktop.contains("MATE")
        {
            DesktopInsets {
                top: 12.0,
                left: 16.0,
                bottom: 52.0,
                right: 16.0,
                dock_centered_bottom: false,
            }
        } else {
            DesktopInsets {
                top: 28.0,
                left: 12.0,
                bottom: 16.0,
                right: 12.0,
                dock_centered_bottom: false,
            }
        }
    }
}

struct AppRuntime {
    smoke_mode: bool,
    smoke_user_dir: Option<PathBuf>,
    smoke_phase: u8,
    smoke_passed: Arc<Mutex<bool>>,
    proxy: EventLoopProxy<UserEvent>,
    config_store: ConfigStore,
    shell_manager: ShellManager,
    balloon_manager: BalloonManager,
    manifest: Arc<Mutex<ShellManifest>>,
    balloon_skin: BalloonSkin,
    engine: AgentEngine,
    gpu: Option<GpuContext>,
    text_renderer: TextRenderer,
    sakura: Option<CharacterWindowState>,
    kero: Option<CharacterWindowState>,
    sakura_balloon: Option<BalloonWindowState>,
    kero_balloon: Option<BalloonWindowState>,
    communicate: Option<CommunicateWindowState>,
    menu: Option<MenuWindowState>,
    playback: PlaybackQueue,
    tray: Option<TrayIcon>,
    tray_actions: HashMap<MenuId, MenuCommand>,
    server: Option<ServerHandle>,
    modifiers: ModifiersState,
    auto_talk_last: Instant,
    os_insets: DesktopInsets,
    initialized: bool,
}

impl AppRuntime {
    fn create_borderless_window(
        event_loop: &ActiveEventLoop,
        title: &str,
        w: u32,
        h: u32,
        always_on_top: bool,
        active: bool,
        visible: bool,
    ) -> Result<Arc<Window>, String> {
        let mut attrs = WindowAttributes::default()
            .with_title(title)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_inner_size(LogicalSize::new(w.max(40), h.max(40)))
            .with_active(active)
            .with_visible(visible)
            .with_window_level(if always_on_top {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            });

        #[cfg(target_os = "macos")]
        {
            attrs = attrs.with_has_shadow(false);
        }
        #[cfg(target_os = "windows")]
        {
            attrs = attrs.with_skip_taskbar(true);
        }

        let win = event_loop
            .create_window(attrs)
            .map_err(|e| format!("창 생성 실패 ({title}): {e}"))?;
        Ok(Arc::new(win))
    }

    fn screen_work_bounds(&self) -> (f64, f64, f64, f64) {
        if let Some(ref s) = self.sakura {
            if let Some(mon) = s
                .ws
                .window
                .current_monitor()
                .or_else(|| s.ws.window.primary_monitor())
            {
                let scale = mon.scale_factor().max(1.0);
                let pos = mon.position().to_logical::<f64>(scale);
                let sz = mon.size().to_logical::<f64>(scale);
                return (pos.x, pos.y, sz.width, sz.height);
            }
        }
        (0.0, 0.0, 1440.0, 900.0)
    }

    fn desktop_safe_work_area(&self) -> (f64, f64, f64, f64) {
        let (mx, my, mw, mh) = self.screen_work_bounds();
        #[cfg(target_os = "linux")]
        {
            if let Some(ref s) = self.sakura {
                if let Ok(out) = std::process::Command::new("xprop")
                    .args(["-root", "_NET_WORKAREA"])
                    .output()
                {
                    if out.status.success() {
                        let txt = String::from_utf8_lossy(&out.stdout);
                        if let Some((_, rhs)) = txt.split_once('=') {
                            let nums: Vec<f64> = rhs
                                .split(',')
                                .filter_map(|p| p.trim().parse::<f64>().ok())
                                .collect();
                            if nums.len() >= 4 && nums[2] > 200.0 && nums[3] > 200.0 {
                                let sc = s.ws.window.scale_factor().max(1.0);
                                let wx = nums[0] / sc;
                                let wy = nums[1] / sc;
                                let ww = nums[2] / sc;
                                let wh = nums[3] / sc;
                                if wx >= mx - 1.0 && wx + ww <= mx + mw + 1.0 && wh <= mh + 1.0 {
                                    return (wx, wy, ww, wh);
                                }
                            }
                        }
                    }
                }
            }
        }
        let ins = &self.os_insets;
        let sx = mx + ins.left;
        let sy = my + ins.top;
        let sw = (mw - ins.left - ins.right).max(320.0);
        let sh = (mh - ins.top - ins.bottom).max(240.0);
        (sx, sy, sw, sh)
    }

    fn reset_character_positions(&mut self) {
        let (mx, my, mw, mh) = self.screen_work_bounds();
        let (sx_safe, sy_safe, sw_safe, sh_safe) = self.desktop_safe_work_area();
        let safe_right = sx_safe + sw_safe;
        let safe_bottom = sy_safe + sh_safe;

        let (sak_w, sak_h, sak_full_feet, sak_foot_off) = self
            .sakura
            .as_ref()
            .map(|c| {
                (
                    c.win_size.0 as f64,
                    c.win_size.1 as f64,
                    c.is_full_feet(),
                    c.foot_bottom_offset_logical(),
                )
            })
            .unwrap_or((280.0, 480.0, true, 8.0));

        let (kero_w, kero_h, kero_full_feet, kero_foot_off) = self
            .kero
            .as_ref()
            .map(|c| {
                (
                    c.win_size.0 as f64,
                    c.win_size.1 as f64,
                    c.is_full_feet(),
                    c.foot_bottom_offset_logical(),
                )
            })
            .unwrap_or((200.0, 250.0, true, 8.0));

        let dock_center_margin = if self.os_insets.dock_centered_bottom {
            mw * 0.62
        } else {
            f64::MAX
        };
        let sak_x = (safe_right - sak_w - 28.0).max(sx_safe);
        let sak_floor_y = if sak_full_feet {
            safe_bottom - 6.0
        } else if sak_x > mx + dock_center_margin {
            my + mh
        } else {
            safe_bottom
        };
        let sak_y = (sak_floor_y - sak_h + if sak_full_feet { sak_foot_off } else { 0.0 })
            .clamp(sy_safe, (my + mh - sak_h).max(sy_safe));

        if let Some(ref mut s) = self.sakura {
            s.set_logical_pos(sak_x, sak_y);
        }

        let kero_x = (sak_x - kero_w - 24.0).max(sx_safe);
        let kero_floor_y = if kero_full_feet {
            safe_bottom - 6.0
        } else if kero_x > mx + dock_center_margin {
            my + mh
        } else {
            safe_bottom
        };
        let kero_y = (kero_floor_y - kero_h + if kero_full_feet { kero_foot_off } else { 0.0 })
            .clamp(sy_safe, (my + mh - kero_h).max(sy_safe));

        if let Some(ref mut k) = self.kero {
            k.set_logical_pos(kero_x, kero_y);
        }
        self.reposition_balloon(Scope::Sakura);
        self.reposition_balloon(Scope::Kero);
    }

    fn reposition_balloon(&mut self, scope: Scope) {
        let (mx, my, mw, mh) = self.desktop_safe_work_area();
        let (char_x, char_y, char_w, _char_h, sid) = match scope {
            Scope::Sakura => {
                let Some(ref mut c) = self.sakura else { return };
                c.sync_pos_from_window();
                (c.logical_pos.0, c.logical_pos.1, c.win_size.0 as f64, c.win_size.1 as f64, c.surface_id)
            }
            Scope::Kero => {
                let Some(ref mut c) = self.kero else { return };
                c.sync_pos_from_window();
                (c.logical_pos.0, c.logical_pos.1, c.win_size.0 as f64, c.win_size.1 as f64, c.surface_id)
            }
        };

        let (ox, oy, align) = {
            let m = self.manifest.lock().expect("manifest mutex poisoned");
            let sdef = m.surfaces.get(&sid);
            match scope {
                Scope::Sakura => (
                    sdef.and_then(|s| s.sakura_balloon_offsetx)
                        .unwrap_or(m.descript.sakura_balloon_offsetx) as f64,
                    sdef.and_then(|s| s.sakura_balloon_offsety)
                        .unwrap_or(m.descript.sakura_balloon_offsety) as f64,
                    m.descript.sakura_balloon_alignment.clone(),
                ),
                Scope::Kero => (
                    sdef.and_then(|s| s.kero_balloon_offsetx)
                        .unwrap_or(m.descript.kero_balloon_offsetx) as f64,
                    sdef.and_then(|s| s.kero_balloon_offsety)
                        .unwrap_or(m.descript.kero_balloon_offsety) as f64,
                    m.descript.kero_balloon_alignment.clone(),
                ),
            }
        };

        let bal = match scope {
            Scope::Sakura => self.sakura_balloon.as_mut(),
            Scope::Kero => self.kero_balloon.as_mut(),
        };
        let Some(bal) = bal else { return };
        let bh = bal.win_size.1 as f64;

        let prefer_left = match align.as_str() {
            "left" => true,
            "right" => false,
            _ => (char_x + char_w * 0.5) > (mx + mw * 0.45),
        };
        let new_side = if prefer_left { "left" } else { "right" }.to_string();
        if bal.side != new_side {
            bal.side = new_side;
            if let Some(ref gpu) = self.gpu {
                bal.redraw_content(&self.balloon_skin, &mut self.text_renderer, gpu);
            }
        }
        let bw = bal.win_size.0 as f64;
        let bx = if bal.side == "left" {
            char_x - bw + ox
        } else {
            char_x + char_w - ox
        };
        let by = (char_y + oy - 24.0).clamp(my + 6.0, (my + mh - bh - 8.0).max(my + 6.0));
        let clamped_bx = bx.clamp(mx + 8.0, (mx + mw - bw - 8.0).max(mx + 8.0));
        bal.ws
            .window
            .set_outer_position(LogicalPosition::new(clamped_bx, by));
    }

    fn apply_skin_switch(&mut self, shell_id: Option<String>, balloon_id: Option<String>) -> Result<Value, String> {
        let mut cfg = self.config_store.get();
        let switched_shell = shell_id.is_some();
        if let Some(ref sid) = shell_id {
            let loaded = self.shell_manager.load_shell(sid)?;
            let new_ds = loaded.descript.display_scale.clamp(0.5, 4.0);
            *self.manifest.lock().expect("manifest mutex poisoned") = loaded;
            cfg = self.config_store.update(ConfigPatch {
                active_shell_id: Some(sid.clone()),
                ..Default::default()
            });
            if let (Some(ref gpu), Some(ref mut sak)) = (&self.gpu, &mut self.sakura) {
                let mut m = self.manifest.lock().expect("manifest mutex poisoned");
                sak.display_scale = new_ds;
                sak.anim_next.clear();
                sak.set_surface(&mut m, gpu, 0, true);
            }
            if let (Some(ref gpu), Some(ref mut ker)) = (&self.gpu, &mut self.kero) {
                let mut m = self.manifest.lock().expect("manifest mutex poisoned");
                ker.display_scale = new_ds;
                ker.anim_next.clear();
                ker.set_surface(&mut m, gpu, 10, true);
            }
        }
        if let Some(ref bid) = balloon_id {
            let loaded = self.balloon_manager.load_balloon(bid)?;
            self.balloon_skin = loaded;
            cfg = self.config_store.update(ConfigPatch {
                active_balloon_id: Some(bid.clone()),
                ..Default::default()
            });
            if let Some(ref gpu) = self.gpu {
                if let Some(ref mut sb) = self.sakura_balloon {
                    sb.redraw_content(&self.balloon_skin, &mut self.text_renderer, gpu);
                }
                if let Some(ref mut kb) = self.kero_balloon {
                    kb.redraw_content(&self.balloon_skin, &mut self.text_renderer, gpu);
                }
            }
        }
        if switched_shell {
            self.reset_character_positions();
        } else {
            self.reposition_balloon(Scope::Sakura);
            self.reposition_balloon(Scope::Kero);
        }
        self.rebuild_tray_menu();
        let active_name = self.manifest.lock().map(|m| m.descript.name.clone()).unwrap_or_default();
        Ok(json!({
            "shell": cfg.active_shell_id,
            "shell_name": active_name,
            "balloon": cfg.active_balloon_id,
        }))
    }

    fn install_and_activate_nar(&mut self, archive: &Path) {
        let dest_base = self.config_store.config_dir.clone();
        match install_nar_archive(archive, &dest_base.join("shells"), &dest_base.join("balloons")) {
            Ok(res) => {
                let first_shell = res.shells.first().cloned();
                let first_balloon = res.balloons.first().cloned();
                let _ = self.apply_skin_switch(first_shell.clone(), first_balloon.clone());
                let msg = format!(
                    "\\0\\s[5]스킨 설치 완료! (셸 {}개, 말풍선 {}개)\\e",
                    res.shells.len(),
                    res.balloons.len()
                );
                let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
            }
            Err(e) => {
                let msg = format!("\\0\\s[3]스킨 설치에 실패했어요: {e}\\e");
                let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
            }
        }
    }

    fn build_menu_rows(&self) -> Vec<MenuRow> {
        let cfg = self.config_store.get();
        let shells: Vec<ShellSummary> = self.shell_manager.list_shells();
        let balloons: Vec<BalloonSummary> = self.balloon_manager.list_balloons();
        let mut rows = vec![
            MenuRow {
                label: "💬 대화창 열기 (더블클릭)".to_string(),
                cmd: MenuCommand::OpenCommunicate,
            },
            MenuRow {
                label: "📁 .nar / .zip 스킨 설치...".to_string(),
                cmd: MenuCommand::InstallNarDialog,
            },
            MenuRow {
                label: if cfg.show_kero {
                    "✓ 파트너 (\\1) 표시"
                } else {
                    "   파트너 (\\1) 표시"
                }
                .to_string(),
                cmd: MenuCommand::ToggleKero,
            },
            MenuRow {
                label: if cfg.always_on_top {
                    "✓ 항상 위에 표시"
                } else {
                    "   항상 위에 표시"
                }
                .to_string(),
                cmd: MenuCommand::ToggleAlwaysOnTop,
            },
            MenuRow {
                label: String::new(),
                cmd: MenuCommand::Separator,
            },
            MenuRow {
                label: "[AI 모델 / 프로바이더]".to_string(),
                cmd: MenuCommand::Separator,
            },
        ];

        for p in provider_catalog() {
            let active = cfg.provider == p.id;
            let mark = if active { "✓" } else { " " };
            rows.push(MenuRow {
                label: format!("{mark} {}", p.label),
                cmd: MenuCommand::SwitchProvider(p.id, p.default_model),
            });
        }

        rows.push(MenuRow {
            label: "[캐릭터 셸 스킨]".to_string(),
            cmd: MenuCommand::Separator,
        });
        for s in shells {
            let mark = if cfg.active_shell_id == s.id { "✓" } else { " " };
            rows.push(MenuRow {
                label: format!("{mark} {}", s.name),
                cmd: MenuCommand::SwitchShell(s.id),
            });
        }

        rows.push(MenuRow {
            label: "[말풍선 스킨]".to_string(),
            cmd: MenuCommand::Separator,
        });
        for b in balloons {
            let mark = if cfg.active_balloon_id == b.id { "✓" } else { " " };
            rows.push(MenuRow {
                label: format!("{mark} {}", b.name),
                cmd: MenuCommand::SwitchBalloon(b.id),
            });
        }

        rows.push(MenuRow {
            label: String::new(),
            cmd: MenuCommand::Separator,
        });
        rows.push(MenuRow {
            label: "🧭 위치 초기화".to_string(),
            cmd: MenuCommand::ResetPositions,
        });
        rows.push(MenuRow {
            label: "🧹 대화 히스토리 초기화".to_string(),
            cmd: MenuCommand::ClearHistory,
        });
        rows.push(MenuRow {
            label: "❌ 종료 (Quit)".to_string(),
            cmd: MenuCommand::Quit,
        });
        rows
    }

    fn rebuild_tray_menu(&mut self) {
        let Some(ref tray) = self.tray else { return };
        let (menu, map) = self.create_tray_menu_model();
        self.tray_actions = map;
        tray.set_menu(Some(Box::new(menu)));
    }

    fn create_tray_menu_model(&self) -> (Menu, HashMap<MenuId, MenuCommand>) {
        let cfg = self.config_store.get();
        let menu = Menu::new();
        let mut map = HashMap::new();

        let chat_item = MenuItem::new("💬 대화창 열기", true, None);
        map.insert(chat_item.id().clone(), MenuCommand::OpenCommunicate);
        let _ = menu.append(&chat_item);

        let nar_item = MenuItem::new("📁 .nar / .zip 설치...", true, None);
        map.insert(nar_item.id().clone(), MenuCommand::InstallNarDialog);
        let _ = menu.append(&nar_item);

        let kero_item = MenuItem::new(
            if cfg.show_kero { "✓ 파트너 표시" } else { "파트너 표시" },
            true,
            None,
        );
        map.insert(kero_item.id().clone(), MenuCommand::ToggleKero);
        let _ = menu.append(&kero_item);

        let aot_item = MenuItem::new(
            if cfg.always_on_top { "✓ 항상 위에" } else { "항상 위에" },
            true,
            None,
        );
        map.insert(aot_item.id().clone(), MenuCommand::ToggleAlwaysOnTop);
        let _ = menu.append(&aot_item);

        let _ = menu.append(&PredefinedMenuItem::separator());

        let mut add_item = |target: &Submenu, label: &str, cmd: MenuCommand| {
            let item = MenuItem::new(label, true, None);
            map.insert(item.id().clone(), cmd);
            let _ = target.append(&item);
        };

        let shell_sub = Submenu::new("캐릭터 셸 전환", true);
        for s in self.shell_manager.list_shells() {
            let mark = if cfg.active_shell_id == s.id { "✓ " } else { "   " };
            add_item(&shell_sub, &format!("{mark}{}", s.name), MenuCommand::SwitchShell(s.id));
        }
        let _ = menu.append(&shell_sub);

        let bal_sub = Submenu::new("말풍선 전환", true);
        for b in self.balloon_manager.list_balloons() {
            let mark = if cfg.active_balloon_id == b.id { "✓ " } else { "   " };
            add_item(&bal_sub, &format!("{mark}{}", b.name), MenuCommand::SwitchBalloon(b.id));
        }
        let _ = menu.append(&bal_sub);

        let prov_sub = Submenu::new("AI 프로바이더", true);
        for p in provider_catalog() {
            let mark = if cfg.provider == p.id { "✓ " } else { "   " };
            add_item(
                &prov_sub,
                &format!("{mark}{}", p.label),
                MenuCommand::SwitchProvider(p.id, p.default_model),
            );
        }
        let _ = menu.append(&prov_sub);

        let _ = menu.append(&PredefinedMenuItem::separator());

        let reset_item = MenuItem::new("🧭 위치 초기화", true, None);
        map.insert(reset_item.id().clone(), MenuCommand::ResetPositions);
        let _ = menu.append(&reset_item);

        let clear_item = MenuItem::new("🧹 히스토리 초기화", true, None);
        map.insert(clear_item.id().clone(), MenuCommand::ClearHistory);
        let _ = menu.append(&clear_item);

        let quit_item = MenuItem::new("❌ 종료", true, None);
        map.insert(quit_item.id().clone(), MenuCommand::Quit);
        let _ = menu.append(&quit_item);

        (menu, map)
    }

    fn execute_menu_command(&mut self, event_loop: &ActiveEventLoop, cmd: MenuCommand) {
        match cmd {
            MenuCommand::Separator => {}
            MenuCommand::OpenCommunicate => {
                self.open_communicate_near_sakura();
            }
            MenuCommand::SwitchShell(sid) => {
                let _ = self.apply_skin_switch(Some(sid), None);
            }
            MenuCommand::SwitchBalloon(bid) => {
                let _ = self.apply_skin_switch(None, Some(bid));
            }
            MenuCommand::SwitchProvider(pid, model) => {
                let _ = self.handle_provider_switch(pid, Some(model));
            }
            MenuCommand::ToggleKero => {
                let cur = self.config_store.get().show_kero;
                let updated = self.config_store.update(ConfigPatch {
                    show_kero: Some(!cur),
                    ..Default::default()
                });
                if let Some(ref mut k) = self.kero {
                    k.visible = updated.show_kero;
                    k.ws.window.set_visible(updated.show_kero);
                }
                if !updated.show_kero {
                    if let Some(ref mut kb) = self.kero_balloon {
                        kb.visible = false;
                        kb.ws.window.set_visible(false);
                    }
                }
                self.rebuild_tray_menu();
            }
            MenuCommand::ToggleAlwaysOnTop => {
                let cur = self.config_store.get().always_on_top;
                let updated = self.config_store.update(ConfigPatch {
                    always_on_top: Some(!cur),
                    ..Default::default()
                });
                let lvl = if updated.always_on_top {
                    WindowLevel::AlwaysOnTop
                } else {
                    WindowLevel::Normal
                };
                if let Some(ref s) = self.sakura {
                    s.ws.window.set_window_level(lvl);
                }
                if let Some(ref k) = self.kero {
                    k.ws.window.set_window_level(lvl);
                }
                if let Some(ref sb) = self.sakura_balloon {
                    sb.ws.window.set_window_level(lvl);
                }
                if let Some(ref kb) = self.kero_balloon {
                    kb.ws.window.set_window_level(lvl);
                }
                self.rebuild_tray_menu();
            }
            MenuCommand::InstallNarDialog => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Ukagaka Archive", &["nar", "zip"])
                    .pick_file()
                {
                    self.install_and_activate_nar(&path);
                }
            }
            MenuCommand::ResetPositions => {
                self.reset_character_positions();
            }
            MenuCommand::ClearHistory => {
                self.engine.clear_history();
                let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(
                    "\\0\\s[1]대화 기억을 깨끗하게 비웠어요!\\e",
                )));
            }
            MenuCommand::Quit => {
                if let Some(mut srv) = self.server.take() {
                    srv.stop();
                }
                event_loop.exit();
            }
        }
    }

    fn handle_provider_switch(
        &mut self,
        provider: String,
        model: Option<String>,
    ) -> Result<Value, String> {
        let valid = provider_catalog().into_iter().find(|p| p.id == provider);
        let Some(p) = valid else {
            return Err(format!("알 수 없는 프로바이더: {provider}"));
        };
        let next_model = model.filter(|m| !m.trim().is_empty()).unwrap_or(p.default_model);
        crate::agent_engine::assert_allowed_model(&next_model)?;
        let cfg = self.config_store.update(ConfigPatch {
            provider: Some(p.id.clone()),
            model: Some(next_model.clone()),
            ..Default::default()
        });
        self.rebuild_tray_menu();
        let msg = format!("\\0\\s[5]AI 엔진을 {} ({}) 모드로 전환했어요!\\e", p.label, next_model);
        let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
        Ok(json!({ "provider": cfg.provider, "model": cfg.model }))
    }

    fn open_communicate_near_sakura(&mut self) {
        let (sx, sy, sw, sh) = if let Some(ref mut s) = self.sakura {
            s.sync_pos_from_window();
            (s.logical_pos.0, s.logical_pos.1, s.win_size.0 as f64, s.win_size.1 as f64)
        } else {
            (400.0, 400.0, 220.0, 300.0)
        };
        let (mx, my, mw, mh) = self.desktop_safe_work_area();
        if let Some(ref mut cw) = self.communicate {
            let x = (sx + sw * 0.5 - cw.win_size.0 as f64 * 0.5)
                .clamp(mx + 12.0, (mx + mw - cw.win_size.0 as f64 - 12.0).max(mx + 12.0));
            let y = (sy + sh - 40.0).clamp(my + 6.0, (my + mh - cw.win_size.1 as f64 - 8.0).max(my + 6.0));
            cw.visible = true;
            cw.text.clear();
            cw.ime_preedit.clear();
            cw.ws.window.set_outer_position(LogicalPosition::new(x, y));
            cw.ws.window.set_ime_allowed(true);
            cw.ws.window.set_visible(true);
            cw.ws.window.focus_window();
            if let Some(ref gpu) = self.gpu {
                cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu);
            }
        }
    }

    fn open_menu_near_cursor(&mut self, scope: Scope) {
        let (mx, my, mw, mh) = self.desktop_safe_work_area();
        let fallback_pos = match scope {
            Scope::Sakura => self.sakura.as_ref().map(|c| (c.logical_pos.0 + 40.0, c.logical_pos.1 + 40.0)),
            Scope::Kero => self.kero.as_ref().map(|c| (c.logical_pos.0 + 20.0, c.logical_pos.1 + 20.0)),
        }
        .unwrap_or((300.0, 300.0));

        let rows = self.build_menu_rows();
        let Some(ref mut menu) = self.menu else { return };
        menu.rows = rows;
        menu.scroll_row = 0;
        menu.hovered_idx = None;
        if let Some(ref gpu) = self.gpu {
            menu.redraw(&mut self.text_renderer, gpu);
        }
        let cursor_pos = match Mouse::get_mouse_position() {
            Mouse::Position { x, y } => {
                #[cfg(target_os = "macos")]
                {
                    (x as f64, y as f64)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    let scale = menu.ws.window.scale_factor().max(1.0);
                    (x as f64 / scale, y as f64 / scale)
                }
            }
            Mouse::Error => fallback_pos,
        };
        let px = cursor_pos
            .0
            .clamp(mx + 8.0, (mx + mw - menu.win_size.0 as f64 - 8.0).max(mx + 8.0));
        let py = cursor_pos
            .1
            .clamp(my + 6.0, (my + mh - menu.win_size.1 as f64 - 8.0).max(my + 6.0));
        menu.visible = true;
        menu.ws.window.set_outer_position(LogicalPosition::new(px, py));
        menu.ws.window.set_visible(true);
        menu.ws.window.focus_window();
    }

    fn hide_menu(&mut self) {
        if let Some(ref mut m) = self.menu {
            if m.visible {
                m.visible = false;
                m.ws.window.set_visible(false);
            }
        }
    }

    fn spawn_user_chat(&self, text: String) {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        let eng = self.engine.clone();
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let reply = eng.handle_user_message(&trimmed);
            let _ = proxy.send_event(UserEvent::PlayActions(reply.actions));
        });
    }

    fn spawn_touch(&self, scope: Scope, kind: &'static str, region: String) {
        let eng = self.engine.clone();
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            if let Some(reply) = eng.handle_touch(scope, kind, &region) {
                let _ = proxy.send_event(UserEvent::PlayActions(reply.actions));
            }
        });
    }

    fn tick_playback(&mut self) {
        let now = Instant::now();
        if let Some(until) = self.playback.paused_until {
            if now < until {
                return;
            }
            self.playback.paused_until = None;
        }

        let mut processed = 0usize;
        let mut sakura_surface_req = None;
        let mut kero_surface_req = None;

        while processed < 6 {
            let Some(front) = self.playback.steps.front().cloned() else {
                break;
            };
            match front {
                PlaybackStep::Char(ch) => {
                    if now < self.playback.next_char_at {
                        break;
                    }
                    self.playback.steps.pop_front();
                    if self.playback.active_speaker == Scope::Kero {
                        self.playback.kero_buf.push(ch);
                        self.playback.kero_show = true;
                        self.playback.kero_dirty = true;
                    } else {
                        self.playback.sakura_buf.push(ch);
                        self.playback.sakura_show = true;
                        self.playback.sakura_dirty = true;
                    }
                    let delay = if matches!(ch, '。' | '.' | '!' | '?' | '\n') {
                        TYPEWRITER_INTERVAL_MS * 2
                    } else {
                        TYPEWRITER_INTERVAL_MS
                    };
                    self.playback.next_char_at = now + Duration::from_millis(delay);
                    processed += 1;
                }
                PlaybackStep::Action(act) => {
                    self.playback.steps.pop_front();
                    match act.kind {
                        ActionKind::Speaker => self.playback.active_speaker = act.scope,
                        ActionKind::Surface => {
                            if act.scope == Scope::Kero {
                                kero_surface_req = Some(act.surface_id);
                            } else {
                                sakura_surface_req = Some(act.surface_id);
                            }
                        }
                        ActionKind::BalloonSize => {
                            let target = if act.scope == Scope::Kero {
                                self.kero_balloon.as_mut()
                            } else {
                                self.sakura_balloon.as_mut()
                            };
                            if let Some(b) = target {
                                b.large = act.balloon_id != 0;
                            }
                        }
                        ActionKind::Clear => {
                            if act.scope == Scope::Kero {
                                self.playback.kero_buf.clear();
                                self.playback.kero_choices.clear();
                                self.playback.kero_dirty = true;
                            } else {
                                self.playback.sakura_buf.clear();
                                self.playback.sakura_choices.clear();
                                self.playback.sakura_dirty = true;
                            }
                        }
                        ActionKind::Choice => {
                            let item = ChoiceItem {
                                label: act.label,
                                choice_id: act.choice_id,
                                rect: (0.0, 0.0, 0.0, 0.0),
                            };
                            if self.playback.active_speaker == Scope::Kero {
                                self.playback.kero_choices.push(item);
                                self.playback.kero_show = true;
                                self.playback.kero_dirty = true;
                            } else {
                                self.playback.sakura_choices.push(item);
                                self.playback.sakura_show = true;
                                self.playback.sakura_dirty = true;
                            }
                        }
                        ActionKind::Pause => {
                            self.playback.paused_until = Some(now + Duration::from_millis(act.ms.max(40)));
                            break;
                        }
                        ActionKind::Text => {}
                    }
                    processed += 1;
                }
            }
        }

        if self.playback.steps.is_empty() && self.playback.paused_until.is_none() && self.playback.auto_hide_at.is_none() {
            if self.playback.sakura_choices.is_empty() && self.playback.kero_choices.is_empty() {
                self.playback.auto_hide_at = Some(now + Duration::from_secs(18));
            }
        }

        if let Some(hide_at) = self.playback.auto_hide_at {
            if now >= hide_at && self.playback.steps.is_empty() {
                self.playback.auto_hide_at = None;
                self.playback.sakura_show = false;
                self.playback.kero_show = false;
                if let Some(ref mut sb) = self.sakura_balloon {
                    sb.visible = false;
                    sb.ws.window.set_visible(false);
                }
                if let Some(ref mut kb) = self.kero_balloon {
                    kb.visible = false;
                    kb.ws.window.set_visible(false);
                }
            }
        }

        if let Some(sid) = sakura_surface_req {
            if let (Some(ref gpu), Some(ref mut s)) = (&self.gpu, &mut self.sakura) {
                let mut m = self.manifest.lock().expect("manifest mutex poisoned");
                s.set_surface(&mut m, gpu, sid, false);
            }
            self.reposition_balloon(Scope::Sakura);
        }
        if let Some(sid) = kero_surface_req {
            if let (Some(ref gpu), Some(ref mut k)) = (&self.gpu, &mut self.kero) {
                let mut m = self.manifest.lock().expect("manifest mutex poisoned");
                k.set_surface(&mut m, gpu, sid, false);
            }
            self.reposition_balloon(Scope::Kero);
        }

        self.sync_balloons_to_playback();
    }

    fn sync_balloons_to_playback(&mut self) {
        if self.playback.sakura_dirty {
            self.playback.sakura_dirty = false;
            if let (Some(ref gpu), Some(ref mut sb)) = (&self.gpu, &mut self.sakura_balloon) {
                sb.text = self.playback.sakura_buf.clone();
                sb.choices = self.playback.sakura_choices.clone();
                sb.large = sb.large || sb.text.chars().count() > 85 || !sb.choices.is_empty();
                sb.visible = self.playback.sakura_show;
                if sb.visible {
                    sb.redraw_content(&self.balloon_skin, &mut self.text_renderer, gpu);
                    sb.ws.window.set_visible(true);
                } else {
                    sb.ws.window.set_visible(false);
                }
            }
            if self.playback.sakura_show {
                self.reposition_balloon(Scope::Sakura);
            }
        }
        if self.playback.kero_dirty {
            self.playback.kero_dirty = false;
            let show_kero = self.config_store.get().show_kero;
            if let (Some(ref gpu), Some(ref mut kb)) = (&self.gpu, &mut self.kero_balloon) {
                kb.text = self.playback.kero_buf.clone();
                kb.choices = self.playback.kero_choices.clone();
                kb.large = kb.large || kb.text.chars().count() > 75 || !kb.choices.is_empty();
                kb.visible = self.playback.kero_show && show_kero;
                if kb.visible {
                    kb.redraw_content(&self.balloon_skin, &mut self.text_renderer, gpu);
                    kb.ws.window.set_visible(true);
                } else {
                    kb.ws.window.set_visible(false);
                }
            }
            if self.playback.kero_show && show_kero {
                self.reposition_balloon(Scope::Kero);
            }
        }
    }

    fn poll_cursor_and_strokes(&mut self) {
        let now = Instant::now();
        let pos_opt = match Mouse::get_mouse_position() {
            Mouse::Position { x, y } => Some((x as f64, y as f64)),
            Mouse::Error => None,
        };
        let mut stroke_hit: Option<(Scope, String)> = None;
        let m_guard = self.manifest.lock().expect("manifest mutex poisoned");

        for scope in [Scope::Sakura, Scope::Kero] {
            let cw = match scope {
                Scope::Sakura => self.sakura.as_mut(),
                Scope::Kero => self.kero.as_mut(),
            };
            let Some(cw) = cw else { continue };
            if !cw.visible {
                continue;
            }
            let local_opt = if let Some((mx, my)) = pos_opt {
                if let Ok(outer) = cw.ws.window.outer_position() {
                    let scale = cw.ws.window.scale_factor().max(1.0);
                    #[cfg(target_os = "macos")]
                    {
                        let wl = outer.to_logical::<f64>(scale);
                        Some((mx - wl.x, my - wl.y))
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        Some(((mx - outer.x as f64) / scale, (my - outer.y as f64) / scale))
                    }
                } else {
                    cw.last_local_cursor
                }
            } else {
                cw.last_local_cursor
            };

            if let Some((lx, ly)) = local_opt {
                let inside = lx >= 0.0
                    && ly >= 0.0
                    && lx < cw.win_size.0 as f64
                    && ly < cw.win_size.1 as f64;
                if inside {
                    let ix = (lx * cw.display_scale).floor() as i32;
                    let iy = (ly * cw.display_scale).floor() as i32;
                    let opaque = cw.dragging || cw.composited.is_opaque_at(ix, iy);
                    if cw.last_hittest != Some(opaque) {
                        if cw.ws.window.set_cursor_hittest(opaque).is_ok() {
                            cw.last_hittest = Some(opaque);
                        }
                    }
                    let hit_col: Option<CollisionRect> = cw
                        .base_bmp
                        .as_ref()
                        .and_then(|bb| m_guard.hit_collision(cw.surface_id, scope, bb, ix, iy));
                    if let Some(region) = cw.stroke_detector.update_at((lx, ly), hit_col.as_ref(), now) {
                        stroke_hit = Some((scope, region));
                    }
                } else {
                    cw.stroke_detector.clear_if_stale(now);
                    if !cw.dragging && cw.last_hittest != Some(false) {
                        if cw.ws.window.set_cursor_hittest(false).is_ok() {
                            cw.last_hittest = Some(false);
                        }
                    }
                }
            }
        }
        drop(m_guard);

        if let Some((scope, region)) = stroke_hit {
            self.spawn_touch(scope, "stroke", region);
        }
    }

    fn run_smoke_verification(&mut self, event_loop: &ActiveEventLoop) {
        let result: Result<String, String> = (|| {
            let gpu = self.gpu.as_ref().ok_or("GPU 컨텍스트 없음")?;
            let sak_win = self.sakura.as_mut().ok_or("사쿠라 창 없음")?;
            let ker_win = self.kero.as_mut().ok_or("우뉴 창 없음")?;

            let mut checked_shells = Vec::new();
            let summaries = self.shell_manager.list_shells();
            if summaries.is_empty() {
                return Err("발견된 셸이 없습니다.".to_string());
            }

            let proof_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("artifacts");
            let _ = std::fs::create_dir_all(&proof_dir);

            let mut exported_neo = false;
            let mut exported_classic = false;
            for sum in &summaries {
                let mut manifest = self.shell_manager.load_shell(&sum.id)?;
                let ds = manifest.descript.display_scale.clamp(0.5, 4.0);
                sak_win.display_scale = ds;
                ker_win.display_scale = ds;
                sak_win.set_surface(&mut manifest, gpu, 0, true);
                ker_win.set_surface(&mut manifest, gpu, 10, true);

                let sak_px = sak_win.composited.count_opaque_pixels();
                let ker_px = ker_win.composited.count_opaque_pixels();
                if sak_px < 3000 {
                    return Err(format!("셸 '{}' \\0 불투명 픽셀 부족: {sak_px}", sum.id));
                }
                if ker_px < 1000 {
                    return Err(format!("셸 '{}' \\1 불투명 픽셀 부족: {ker_px}", sum.id));
                }
                if sak_win.surface_id == ker_win.surface_id {
                    return Err(format!("셸 '{}' \\0/\\1 표면 충돌: {}", sum.id, sak_win.surface_id));
                }
                if sak_win.composited.data == ker_win.composited.data {
                    return Err(format!("셸 '{}' \\0/\\1 이미지 동일(우뉴 소실 재발)", sum.id));
                }

                if sum.id == "neo_default" {
                    if (ds - 2.0).abs() > 0.01 {
                        return Err(format!("neo_default display.scale 오류: {ds}"));
                    }
                    if !sak_win.is_full_feet() || !ker_win.is_full_feet() {
                        return Err("neo_default 발바닥 여백 검증 실패 (하단 잘림 의심)".to_string());
                    }
                    sak_win.composited.save_png(&proof_dir.join("neo-sakura.png"))?;
                    ker_win.composited.save_png(&proof_dir.join("neo-unyu.png"))?;
                    exported_neo = true;
                }
                if sum.id == "master" || (!exported_classic && sum.id == "neo_default") {
                    sak_win.composited.save_png(&proof_dir.join("native-sakura.png"))?;
                    ker_win.composited.save_png(&proof_dir.join("native-unyu.png"))?;
                    if sum.id == "master" {
                        exported_classic = true;
                    }
                }
                checked_shells.push(format!("{}(0:{}px,1:{}px,@{:.1}x)", sum.id, sak_px, ker_px, ds));
            }
            if !exported_neo {
                return Err("neo_default 셸을 찾을 수 없습니다.".to_string());
            }

            // Re-activate default active shell & render dual balloons + export balloon PNG proof
            let active_id = self.config_store.get().active_shell_id;
            let mut active_manifest = self.shell_manager.load_shell(&active_id)?;
            let active_ds = active_manifest.descript.display_scale.clamp(0.5, 4.0);
            sak_win.display_scale = active_ds;
            ker_win.display_scale = active_ds;
            sak_win.set_surface(&mut active_manifest, gpu, 0, true);
            ker_win.set_surface(&mut active_manifest, gpu, 10, true);
            *self.manifest.lock().expect("manifest mutex poisoned") = active_manifest;
            self.reset_character_positions();

            let demo_script = "\\0\\s[0]안녕! 100% Safe Rust 何か藍(NanikaAi) v1.2 스모크 테스트 완료예요!\\w3\\1\\s[10]선글라스 아래로 내 눈 잘 보이지?\\e";
            self.playback.enqueue(parse_dialogue(demo_script));
            let _ = self.playback.fast_forward();
            self.sync_balloons_to_playback();

            let sb = self.sakura_balloon.as_ref().ok_or("사쿠라 말풍선 창 없음")?;
            sb.last_render_bmp
                .save_png(&proof_dir.join("native-balloon-sakura.png"))?;
            sb.last_render_bmp
                .save_png(&proof_dir.join("neo-balloon-sakura.png"))?;

            // Exercise HTTP GET /api/health, POST /api/speak, and raw TCP SSTP/1.4
            let srv = self.server.as_ref().ok_or("서버 미기동")?;
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .map_err(|e| e.to_string())?;
            let h_resp = client
                .get(format!("http://127.0.0.1:{}/api/health", srv.http_port))
                .send()
                .map_err(|e| format!("HTTP health 실패: {e}"))?;
            if !h_resp.status().is_success() {
                return Err(format!("HTTP health 상태 오류: {}", h_resp.status()));
            }
            let s_resp = client
                .post(format!("http://127.0.0.1:{}/api/speak", srv.http_port))
                .json(&json!({"script": "\\0\\s[1]HTTP 테스트 성공!\\e"}))
                .send()
                .map_err(|e| format!("HTTP speak 실패: {e}"))?;
            if !s_resp.status().is_success() {
                return Err(format!("HTTP speak 상태 오류: {}", s_resp.status()));
            }

            // TCP SSTP/1.4 test
            use std::io::{Read, Write};
            let mut tcp = std::net::TcpStream::connect(("127.0.0.1", srv.tcp_port))
                .map_err(|e| format!("TCP SSTP 연결 실패: {e}"))?;
            let payload = "SEND SSTP/1.4\r\nCharset: UTF-8\r\nSender: smoke-test\r\nScript: \\0\\s[0]SSTP 통과!\\e\r\n\r\n";
            tcp.write_all(payload.as_bytes()).map_err(|e| e.to_string())?;
            let mut sstp_buf = String::new();
            let _ = tcp.read_to_string(&mut sstp_buf);
            if !sstp_buf.contains("200 OK") {
                return Err(format!("SSTP 응답 오류: {sstp_buf}"));
            }

            let safe_area = self.desktop_safe_work_area();
            Ok(format!(
                "[SMOKE PASS] unsafe=0 shells=[{}] scale={} safe=({:.0},{:.0},{:.0},{:.0}) http={} tcp={}",
                checked_shells.join(", "),
                active_ds,
                safe_area.0,
                safe_area.1,
                safe_area.2,
                safe_area.3,
                srv.http_port,
                srv.tcp_port
            ))
        })();

        if let Some(mut srv) = self.server.take() {
            srv.stop();
        }
        if let Some(ref tmp) = self.smoke_user_dir {
            let _ = std::fs::remove_dir_all(tmp);
        }

        match result {
            Ok(summary) => {
                println!("{summary}");
                if let Ok(mut g) = self.smoke_passed.lock() {
                    *g = true;
                }
            }
            Err(err) => {
                eprintln!("[SMOKE FAIL] {err}");
            }
        }
        event_loop.exit();
    }
}

impl ApplicationHandler<UserEvent> for AppRuntime {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.initialized {
            return;
        }
        self.initialized = true;

        let cfg = self.config_store.get();
        let init_ds = self
            .manifest
            .lock()
            .expect("manifest mutex poisoned")
            .descript
            .display_scale
            .clamp(0.5, 4.0);
        let sak_init = self
            .manifest
            .lock()
            .expect("manifest mutex poisoned")
            .compose_base_rgba(0)
            .unwrap_or_else(|| RgbaBitmap::from_solid(560, 960, 120, 150, 220, 220));
        let kero_init = self
            .manifest
            .lock()
            .expect("manifest mutex poisoned")
            .compose_base_rgba(10)
            .unwrap_or_else(|| RgbaBitmap::from_solid(400, 500, 150, 180, 230, 220));
        let sak_lw = ((sak_init.width as f64) / init_ds).round().max(40.0) as u32;
        let sak_lh = ((sak_init.height as f64) / init_ds).round().max(40.0) as u32;
        let kero_lw = ((kero_init.width as f64) / init_ds).round().max(40.0) as u32;
        let kero_lh = ((kero_init.height as f64) / init_ds).round().max(40.0) as u32;

        let instance = wgpu::Instance::default();
        let sak_win = match Self::create_borderless_window(
            event_loop,
            "NanikaAi Sakura",
            sak_lw,
            sak_lh,
            cfg.always_on_top,
            false,
            true,
        ) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("{e}");
                event_loop.exit();
                return;
            }
        };
        let sak_surface = match instance.create_surface(Arc::clone(&sak_win)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Sakura GPU surface 생성 실패: {e}");
                event_loop.exit();
                return;
            }
        };
        let gpu = match GpuContext::new(instance, &sak_surface) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("{e}");
                event_loop.exit();
                return;
            }
        };
        // Need instance again for secondary windows; GpuContext stores it cleanly via `&gpu._instance`
        let sak_ws = WindowSurface::from_existing(sak_surface, sak_win, &gpu);

        let make_ws = |title: &str, w: u32, h: u32, aot: bool, active: bool, vis: bool| -> Option<WindowSurface> {
            let win = Self::create_borderless_window(event_loop, title, w, h, aot, active, vis).ok()?;
            let surf = gpu._instance.create_surface(Arc::clone(&win)).ok()?;
            Some(WindowSurface::from_existing(surf, win, &gpu))
        };

        let kero_ws = make_ws(
            "NanikaAi Kero",
            kero_lw,
            kero_lh,
            cfg.always_on_top,
            false,
            cfg.show_kero,
        );
        let sak_bal_ws = make_ws("NanikaAi Balloon Sakura", 360, 168, cfg.always_on_top, false, false);
        let kero_bal_ws = make_ws("NanikaAi Balloon Kero", 330, 152, cfg.always_on_top, false, false);
        let comm_ws = make_ws("NanikaAi Communicate", 360, 108, true, true, false);
        let menu_ws = make_ws("NanikaAi Menu", 270, 320, true, true, false);

        self.sakura = Some(CharacterWindowState {
            scope: Scope::Sakura,
            ws: sak_ws,
            surface_id: 0,
            base_bmp: None,
            overlays: HashMap::new(),
            composited: sak_init.clone(),
            win_size: (sak_lw, sak_lh),
            display_scale: init_ds,
            logical_pos: (0.0, 0.0),
            anim_next: HashMap::new(),
            active_run: None,
            lip_on: false,
            next_lip_at: Instant::now(),
            stroke_detector: StrokeDetector::new(),
            last_hittest: None,
            last_local_cursor: None,
            dragging: false,
            drag_moved: false,
            drag_start_cursor_global: (0.0, 0.0),
            drag_start_win_pos: (0.0, 0.0),
            last_click_at: None,
            visible: true,
        });

        if let Some(kws) = kero_ws {
            self.kero = Some(CharacterWindowState {
                scope: Scope::Kero,
                ws: kws,
                surface_id: 10,
                base_bmp: None,
                overlays: HashMap::new(),
                composited: kero_init.clone(),
                win_size: (kero_lw, kero_lh),
                display_scale: init_ds,
                logical_pos: (0.0, 0.0),
                anim_next: HashMap::new(),
                active_run: None,
                lip_on: false,
                next_lip_at: Instant::now(),
                stroke_detector: StrokeDetector::new(),
                last_hittest: None,
                last_local_cursor: None,
                dragging: false,
                drag_moved: false,
                drag_start_cursor_global: (0.0, 0.0),
                drag_start_win_pos: (0.0, 0.0),
                last_click_at: None,
                visible: cfg.show_kero,
            });
        }

        if let Some(sbws) = sak_bal_ws {
            self.sakura_balloon = Some(BalloonWindowState {
                scope: Scope::Sakura,
                ws: sbws,
                visible: false,
                side: "left".to_string(),
                large: false,
                text: String::new(),
                choices: Vec::new(),
                hovered_choice: None,
                status_line: String::new(),
                scroll_px: 0.0,
                max_scroll_px: 0.0,
                win_size: (360, 168),
                last_render_bmp: RgbaBitmap::new_transparent(16, 16),
            });
        }
        if let Some(kbws) = kero_bal_ws {
            self.kero_balloon = Some(BalloonWindowState {
                scope: Scope::Kero,
                ws: kbws,
                visible: false,
                side: "left".to_string(),
                large: false,
                text: String::new(),
                choices: Vec::new(),
                hovered_choice: None,
                status_line: String::new(),
                scroll_px: 0.0,
                max_scroll_px: 0.0,
                win_size: (330, 152),
                last_render_bmp: RgbaBitmap::new_transparent(16, 16),
            });
        }
        if let Some(cws) = comm_ws {
            self.communicate = Some(CommunicateWindowState {
                ws: cws,
                visible: false,
                text: String::new(),
                ime_preedit: String::new(),
                win_size: (360, 108),
                cursor_on: true,
                last_blink: Instant::now(),
            });
        }
        if let Some(mws) = menu_ws {
            self.menu = Some(MenuWindowState {
                ws: mws,
                visible: false,
                rows: Vec::new(),
                hovered_idx: None,
                scroll_row: 0,
                win_size: (270, 320),
                row_h: 26.0,
                max_visible_rows: 18,
            });
        }

        self.gpu = Some(gpu);

        // Render initial surfaces `\0` and `\1`
        if let (Some(ref gpu), Some(ref mut s)) = (&self.gpu, &mut self.sakura) {
            let mut m = self.manifest.lock().expect("manifest mutex poisoned");
            s.set_surface(&mut m, gpu, 0, true);
        }
        if let (Some(ref gpu), Some(ref mut k)) = (&self.gpu, &mut self.kero) {
            let mut m = self.manifest.lock().expect("manifest mutex poisoned");
            k.set_surface(&mut m, gpu, 10, true);
        }
        self.reset_character_positions();

        // Tray icon (skip during automated headless/ephemeral smoke test if desired, or build safely)
        if !self.smoke_mode {
            let assets = resolve_assets_dir();
            let icon_rgba = image::open(assets.join("icon.png"))
                .ok()
                .map(|img| img.resize_exact(32, 32, image::imageops::FilterType::Lanczos3).to_rgba8())
                .and_then(|rgba| TrayIconImage::from_rgba(rgba.into_raw(), 32, 32).ok())
                .unwrap_or_else(|| {
                    let fallback = RgbaBitmap::from_solid(16, 16, 110, 145, 225, 255);
                    TrayIconImage::from_rgba(fallback.data, 16, 16).expect("valid 16x16 tray rgba")
                });
            let (tray_menu, map) = self.create_tray_menu_model();
            self.tray_actions = map;
            if let Ok(t) = TrayIconBuilder::new()
                .with_icon(icon_rgba)
                .with_tooltip("何か藍 · NanikaAi (Rust)")
                .with_menu(Box::new(tray_menu))
                .build()
            {
                self.tray = Some(t);
            }
        }

        // Start dual HTTP + SSTP server
        let backend = Arc::new(GuiServerBackend {
            proxy: self.proxy.clone(),
            config_store: self.config_store.clone(),
            engine: self.engine.clone(),
        });
        let (h_port, t_port) = if self.smoke_mode {
            (0, 0)
        } else {
            (cfg.http_port, cfg.sstp_tcp_port)
        };
        match start_dual_server(backend, h_port, t_port) {
            Ok(srv) => self.server = Some(srv),
            Err(e) => eprintln!("듀얼 서버 기동 경고: {e}"),
        }

        if self.smoke_mode {
            let _ = self.proxy.send_event(UserEvent::TriggerSmokeStep);
        } else {
            // Boot greeting on background thread
            let eng = self.engine.clone();
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                let reply = eng.handle_boot();
                let _ = proxy.send_event(UserEvent::PlayActions(reply.actions));
            });
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::PlayActions(actions) => {
                self.playback.enqueue(actions);
            }
            UserEvent::StatusUpdate(msg, _active) => {
                if let Some(ref mut sb) = self.sakura_balloon {
                    sb.status_line = msg.clone();
                    if sb.visible {
                        if let Some(ref gpu) = self.gpu {
                            sb.redraw_content(&self.balloon_skin, &mut self.text_renderer, gpu);
                        }
                    } else if !msg.is_empty() {
                        self.playback.sakura_buf = "…".to_string();
                        self.playback.sakura_show = true;
                        self.playback.sakura_dirty = true;
                        self.sync_balloons_to_playback();
                    }
                }
            }
            UserEvent::ServerGetHealth(tx) => {
                let cfg = self.config_store.get();
                let m = self.manifest.lock().expect("manifest mutex poisoned");
                let _ = tx.send(json!({
                    "status": "ok",
                    "runtime": "rust",
                    "unsafe_lines": 0,
                    "provider": cfg.provider,
                    "model": cfg.model,
                    "shell": cfg.active_shell_id,
                    "shell_name": m.descript.name,
                    "sakura_surface": self.sakura.as_ref().map(|s| s.surface_id).unwrap_or(0),
                    "kero_surface": self.kero.as_ref().map(|k| k.surface_id).unwrap_or(10),
                    "balloon": cfg.active_balloon_id,
                }));
            }
            UserEvent::ServerGetSkins(tx) => {
                let cfg = self.config_store.get();
                let shells: Vec<Value> = self
                    .shell_manager
                    .list_shells()
                    .into_iter()
                    .map(|s| json!({ "id": s.id, "name": s.name }))
                    .collect();
                let balloons: Vec<Value> = self
                    .balloon_manager
                    .list_balloons()
                    .into_iter()
                    .map(|b| json!({ "id": b.id, "name": b.name }))
                    .collect();
                let _ = tx.send(json!({
                    "active_shell": cfg.active_shell_id,
                    "active_balloon": cfg.active_balloon_id,
                    "shells": shells,
                    "balloons": balloons,
                }));
            }
            UserEvent::ServerSwitchSkin { shell, balloon, reply } => {
                let _ = reply.send(self.apply_skin_switch(shell, balloon));
            }
            UserEvent::ServerSwitchProvider { provider, model, reply } => {
                let _ = reply.send(self.handle_provider_switch(provider, model));
            }
            UserEvent::TriggerSmokeStep => {
                if self.smoke_mode && self.smoke_phase == 0 {
                    self.smoke_phase = 1;
                    self.run_smoke_verification(event_loop);
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        // Identify target window
        let target_char = if self.sakura.as_ref().is_some_and(|w| w.ws.window.id() == window_id) {
            Some(Scope::Sakura)
        } else if self.kero.as_ref().is_some_and(|w| w.ws.window.id() == window_id) {
            Some(Scope::Kero)
        } else {
            None
        };
        let target_bal = if self
            .sakura_balloon
            .as_ref()
            .is_some_and(|b| b.ws.window.id() == window_id)
        {
            Some(Scope::Sakura)
        } else if self
            .kero_balloon
            .as_ref()
            .is_some_and(|b| b.ws.window.id() == window_id)
        {
            Some(Scope::Kero)
        } else {
            None
        };
        let is_comm = self
            .communicate
            .as_ref()
            .is_some_and(|c| c.ws.window.id() == window_id);
        let is_menu = self
            .menu
            .as_ref()
            .is_some_and(|m| m.ws.window.id() == window_id);

        match event {
            WindowEvent::CloseRequested => {
                if is_comm {
                    if let Some(ref mut c) = self.communicate {
                        c.visible = false;
                        c.ws.window.set_visible(false);
                    }
                } else if is_menu {
                    self.hide_menu();
                } else {
                    if let Some(mut srv) = self.server.take() {
                        srv.stop();
                    }
                    event_loop.exit();
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }
            WindowEvent::Resized(phys) => {
                if let Some(ref gpu) = self.gpu {
                    if let Some(sc) = target_char {
                        let cw = if sc == Scope::Sakura { self.sakura.as_mut() } else { self.kero.as_mut() };
                        if let Some(c) = cw {
                            c.ws.resize(gpu, phys);
                        }
                    } else if let Some(sb) = target_bal {
                        let bw = if sb == Scope::Sakura { self.sakura_balloon.as_mut() } else { self.kero_balloon.as_mut() };
                        if let Some(b) = bw {
                            b.ws.resize(gpu, phys);
                        }
                    } else if is_comm {
                        if let Some(ref mut c) = self.communicate {
                            c.ws.resize(gpu, phys);
                        }
                    } else if is_menu {
                        if let Some(ref mut m) = self.menu {
                            m.ws.resize(gpu, phys);
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(ref gpu) = self.gpu {
                    if let Some(sc) = target_char {
                        let cw = if sc == Scope::Sakura { self.sakura.as_mut() } else { self.kero.as_mut() };
                        if let Some(c) = cw {
                            c.ws.render(gpu);
                        }
                    } else if let Some(sb) = target_bal {
                        let bw = if sb == Scope::Sakura { self.sakura_balloon.as_mut() } else { self.kero_balloon.as_mut() };
                        if let Some(b) = bw {
                            b.ws.render(gpu);
                        }
                    } else if is_comm {
                        if let Some(ref mut c) = self.communicate {
                            c.ws.render(gpu);
                        }
                    } else if is_menu {
                        if let Some(ref mut m) = self.menu {
                            m.ws.render(gpu);
                        }
                    }
                }
            }
            WindowEvent::Focused(focused) => {
                if is_menu && !focused {
                    self.hide_menu();
                }
            }
            WindowEvent::DroppedFile(path) => {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .unwrap_or_default();
                if matches!(ext.as_str(), "nar" | "zip") {
                    self.install_and_activate_nar(&path);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(sc) = target_char {
                    let cw = if sc == Scope::Sakura { self.sakura.as_mut() } else { self.kero.as_mut() };
                    if let Some(c) = cw {
                        let scale = c.ws.window.scale_factor().max(1.0);
                        let local = position.to_logical::<f64>(scale);
                        c.last_local_cursor = Some((local.x, local.y));
                        if c.dragging {
                            if let Mouse::Position { x, y } = Mouse::get_mouse_position() {
                                #[cfg(target_os = "macos")]
                                let (gx, gy) = (x as f64, y as f64);
                                #[cfg(not(target_os = "macos"))]
                                let (gx, gy) = (x as f64 / scale, y as f64 / scale);
                                let dx = gx - c.drag_start_cursor_global.0;
                                let dy = gy - c.drag_start_cursor_global.1;
                                if dx.abs() > 3.0 || dy.abs() > 3.0 {
                                    c.drag_moved = true;
                                }
                                c.set_logical_pos(
                                    c.drag_start_win_pos.0 + dx,
                                    c.drag_start_win_pos.1 + dy,
                                );
                                self.reposition_balloon(sc);
                            }
                        }
                    }
                } else if let Some(sb) = target_bal {
                    let mut redraw = false;
                    if let Some(b) = if sb == Scope::Sakura { self.sakura_balloon.as_mut() } else { self.kero_balloon.as_mut() } {
                        let local = position.to_logical::<f64>(b.ws.window.scale_factor().max(1.0));
                        let hovered = b.choices.iter().position(|ch| {
                            ch.rect.2 > ch.rect.0
                                && local.x >= ch.rect.0
                                && local.x <= ch.rect.2
                                && local.y >= ch.rect.1
                                && local.y <= ch.rect.3
                        });
                        if b.hovered_choice != hovered {
                            b.hovered_choice = hovered;
                            redraw = true;
                        }
                        if redraw {
                            if let Some(ref gpu) = self.gpu {
                                b.redraw_content(&self.balloon_skin, &mut self.text_renderer, gpu);
                            }
                        }
                    }
                } else if is_menu {
                    if let Some(ref mut m) = self.menu {
                        let local = position.to_logical::<f64>(m.ws.window.scale_factor().max(1.0));
                        let rel_row = ((local.y - 6.0) / m.row_h).floor() as isize;
                        let new_hover = if rel_row >= 0 {
                            let idx = m.scroll_row + rel_row as usize;
                            if idx < m.rows.len() && !matches!(m.rows[idx].cmd, MenuCommand::Separator) {
                                Some(idx)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        if m.hovered_idx != new_hover {
                            m.hovered_idx = new_hover;
                            if let Some(ref gpu) = self.gpu {
                                m.redraw(&mut self.text_renderer, gpu);
                            }
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64 * 26.0,
                    MouseScrollDelta::PixelDelta(p) => p.y,
                };
                if let Some(sb) = target_bal {
                    if let Some(b) = if sb == Scope::Sakura { self.sakura_balloon.as_mut() } else { self.kero_balloon.as_mut() } {
                        b.scroll_px = (b.scroll_px - dy as f32).clamp(0.0, b.max_scroll_px);
                        if let Some(ref gpu) = self.gpu {
                            b.redraw_content(&self.balloon_skin, &mut self.text_renderer, gpu);
                        }
                    }
                } else if is_menu {
                    if let Some(ref mut m) = self.menu {
                        let max_scroll = m.rows.len().saturating_sub(m.max_visible_rows);
                        if dy < 0.0 {
                            m.scroll_row = (m.scroll_row + 1).min(max_scroll);
                        } else if dy > 0.0 {
                            m.scroll_row = m.scroll_row.saturating_sub(1);
                        }
                        if let Some(ref gpu) = self.gpu {
                            m.redraw(&mut self.text_renderer, gpu);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(sc) = target_char {
                    match (button, state) {
                        (MouseButton::Right, ElementState::Pressed) => {
                            self.open_menu_near_cursor(sc);
                        }
                        (MouseButton::Left, ElementState::Pressed) => {
                            self.hide_menu();
                            let cw = if sc == Scope::Sakura { self.sakura.as_mut() } else { self.kero.as_mut() };
                            if let Some(c) = cw {
                                c.sync_pos_from_window();
                                c.dragging = true;
                                c.drag_moved = false;
                                c.drag_start_win_pos = c.logical_pos;
                                if let Mouse::Position { x, y } = Mouse::get_mouse_position() {
                                    #[cfg(target_os = "macos")]
                                    {
                                        c.drag_start_cursor_global = (x as f64, y as f64);
                                    }
                                    #[cfg(not(target_os = "macos"))]
                                    {
                                        let scale = c.ws.window.scale_factor().max(1.0);
                                        c.drag_start_cursor_global = (x as f64 / scale, y as f64 / scale);
                                    }
                                } else if c.ws.window.outer_position().is_err() {
                                    let _ = c.ws.window.drag_window();
                                }
                            }
                        }
                        (MouseButton::Left, ElementState::Released) => {
                            let mut open_chat = false;
                            let mut touch_region = None;
                            {
                                let cw = if sc == Scope::Sakura { self.sakura.as_mut() } else { self.kero.as_mut() };
                                if let Some(c) = cw {
                                    let was_drag = c.drag_moved;
                                    c.dragging = false;
                                    c.drag_moved = false;
                                    if !was_drag {
                                        let now = Instant::now();
                                        let is_dbl = c
                                            .last_click_at
                                            .is_some_and(|t| now.duration_since(t) <= Duration::from_millis(380));
                                        c.last_click_at = if is_dbl { None } else { Some(now) };
                                        if is_dbl {
                                            if let Some((lx, ly)) = c.last_local_cursor {
                                                let ix = (lx * c.display_scale).floor() as i32;
                                                let iy = (ly * c.display_scale).floor() as i32;
                                                let m = self.manifest.lock().expect("manifest mutex poisoned");
                                                let col = c.base_bmp.as_ref().and_then(|bb| {
                                                    m.hit_collision(
                                                        c.surface_id,
                                                        sc,
                                                        bb,
                                                        ix,
                                                        iy,
                                                    )
                                                });
                                                if let Some(col) = col {
                                                    touch_region = Some(col.name);
                                                } else if sc == Scope::Sakura {
                                                    open_chat = true;
                                                } else {
                                                    touch_region = Some("Body".to_string());
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            if open_chat {
                                self.open_communicate_near_sakura();
                            } else if let Some(reg) = touch_region {
                                self.spawn_touch(sc, "double_click", reg);
                            }
                        }
                        _ => {}
                    }
                } else if let Some(sb) = target_bal {
                    if button == MouseButton::Left && state == ElementState::Pressed {
                        let mut picked_choice: Option<(String, String)> = None;
                        let mut should_ff = false;
                        let mut should_hide = false;
                        if let Some(b) = if sb == Scope::Sakura { self.sakura_balloon.as_mut() } else { self.kero_balloon.as_mut() } {
                            if let Some(idx) = b.hovered_choice {
                                if let Some(ch) = b.choices.get(idx) {
                                    picked_choice = Some((ch.label.clone(), ch.choice_id.clone()));
                                }
                            } else if !self.playback.steps.is_empty() {
                                should_ff = true;
                            } else {
                                should_hide = true;
                                b.visible = false;
                                b.ws.window.set_visible(false);
                            }
                        }
                        if let Some((lbl, cid)) = picked_choice {
                            self.playback.sakura_choices.clear();
                            self.playback.kero_choices.clear();
                            self.spawn_user_chat(format!("[선택지 선택] {lbl} (id={cid})"));
                        } else if should_ff {
                            let imm = self.playback.fast_forward();
                            for act in imm {
                                if act.kind == ActionKind::Surface {
                                    if let Some(ref gpu) = self.gpu {
                                        let mut m = self.manifest.lock().expect("manifest mutex poisoned");
                                        if act.scope == Scope::Kero {
                                            if let Some(ref mut k) = self.kero {
                                                k.set_surface(&mut m, gpu, act.surface_id, false);
                                            }
                                        } else if let Some(ref mut s) = self.sakura {
                                            s.set_surface(&mut m, gpu, act.surface_id, false);
                                        }
                                    }
                                }
                            }
                            self.sync_balloons_to_playback();
                        } else if should_hide {
                            if sb == Scope::Sakura {
                                self.playback.sakura_show = false;
                            } else {
                                self.playback.kero_show = false;
                            }
                        }
                    }
                } else if is_menu && button == MouseButton::Left && state == ElementState::Released {
                    let chosen = self.menu.as_ref().and_then(|m| {
                        m.hovered_idx.and_then(|i| m.rows.get(i).map(|r| r.cmd.clone()))
                    });
                    self.hide_menu();
                    if let Some(cmd) = chosen {
                        self.execute_menu_command(event_loop, cmd);
                    }
                }
            }
            WindowEvent::Ime(ime_ev) if is_comm => {
                if let Some(ref mut cw) = self.communicate {
                    match ime_ev {
                        Ime::Enabled => {}
                        Ime::Preedit(s, _) => {
                            cw.ime_preedit = s;
                        }
                        Ime::Commit(s) => {
                            cw.ime_preedit.clear();
                            cw.text.push_str(&s);
                        }
                        Ime::Disabled => {
                            cw.ime_preedit.clear();
                        }
                    }
                    if let Some(ref gpu) = self.gpu {
                        cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu);
                    }
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        text,
                        ..
                    },
                ..
            } if is_comm => {
                let mut submit_text: Option<String> = None;
                if let Some(ref mut cw) = self.communicate {
                    let cmd_or_ctrl = self.modifiers.super_key() || self.modifiers.control_key();
                    match &logical_key {
                        Key::Named(NamedKey::Escape) => {
                            cw.visible = false;
                            cw.ime_preedit.clear();
                            cw.ws.window.set_visible(false);
                        }
                        Key::Named(NamedKey::Enter) => {
                            if cw.ime_preedit.is_empty() {
                                submit_text = Some(cw.text.clone());
                                cw.text.clear();
                                cw.visible = false;
                                cw.ws.window.set_visible(false);
                            }
                        }
                        Key::Named(NamedKey::Backspace) => {
                            if cw.ime_preedit.is_empty() {
                                cw.text.pop();
                            }
                        }
                        Key::Character(s) if cmd_or_ctrl && s.eq_ignore_ascii_case("v") => {
                            if let Ok(mut cb) = Clipboard::new() {
                                if let Ok(pasted) = cb.get_text() {
                                    cw.text.push_str(&pasted.replace(['\r', '\n'], " "));
                                }
                            }
                        }
                        Key::Named(NamedKey::Space) => {
                            if cw.ime_preedit.is_empty() {
                                cw.text.push(' ');
                            }
                        }
                        _ => {
                            if !cmd_or_ctrl && cw.ime_preedit.is_empty() {
                                if let Some(t) = text {
                                    for ch in t.chars() {
                                        if !ch.is_control() {
                                            cw.text.push(ch);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if cw.visible {
                        if let Some(ref gpu) = self.gpu {
                            cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu);
                        }
                    }
                }
                if let Some(msg) = submit_text {
                    self.spawn_user_chat(msg);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let next_wakeup = Instant::now() + Duration::from_millis(POLL_INTERVAL_MS);
        event_loop.set_control_flow(ControlFlow::WaitUntil(next_wakeup));

        if !self.initialized || self.smoke_mode {
            return;
        }

        // Poll tray menu clicks
        let mut pending_cmds = Vec::new();
        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            if let Some(cmd) = self.tray_actions.get(&ev.id).cloned() {
                pending_cmds.push(cmd);
            }
        }
        for cmd in pending_cmds {
            self.execute_menu_command(event_loop, cmd);
        }

        // Caret blink inside Communicate window
        if let Some(ref mut cw) = self.communicate {
            if cw.visible && cw.last_blink.elapsed() >= Duration::from_millis(500) {
                cw.cursor_on = !cw.cursor_on;
                cw.last_blink = Instant::now();
                if let Some(ref gpu) = self.gpu {
                    cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu);
                }
            }
        }

        self.poll_cursor_and_strokes();
        self.tick_playback();

        // SERIKO animation tick
        let speaking_sakura = !self.playback.steps.is_empty() && self.playback.active_speaker == Scope::Sakura;
        let speaking_kero = !self.playback.steps.is_empty() && self.playback.active_speaker == Scope::Kero;
        if let Some(ref gpu) = self.gpu {
            let m = self.manifest.lock().expect("manifest mutex poisoned");
            if let Some(ref mut s) = self.sakura {
                s.tick_seriko(&m, gpu, speaking_sakura);
            }
            if let Some(ref mut k) = self.kero {
                if k.visible {
                    k.tick_seriko(&m, gpu, speaking_kero);
                }
            }
        }

        // Auto-talk periodic idle talk
        let interval = self.config_store.get().auto_talk_interval_sec;
        if interval > 0 && self.auto_talk_last.elapsed() >= Duration::from_secs(interval) {
            self.auto_talk_last = Instant::now();
            if self.playback.steps.is_empty() {
                let eng = self.engine.clone();
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    if let Some(reply) = eng.handle_auto_talk() {
                        let _ = proxy.send_event(UserEvent::PlayActions(reply.actions));
                    }
                });
            }
        }
    }
}

pub fn run(smoke_test: bool) -> Result<(), String> {
    let assets_dir = resolve_assets_dir();
    let bundled_shells = assets_dir.join("shells");
    let bundled_balloons = assets_dir.join("balloons");

    let (config_store, smoke_tmp) = if smoke_test {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(".smoke-userdata-{ts}"));
        let store = ConfigStore::new(Some(dir.clone()));
        store.update(ConfigPatch {
            provider: Some("local_ollama".to_string()),
            model: Some("gemma4:31b".to_string()),
            active_shell_id: Some("neo_default".to_string()),
            active_balloon_id: Some("clean_default".to_string()),
            show_kero: Some(true),
            always_on_top: Some(true),
            ..Default::default()
        });
        (store, Some(dir))
    } else {
        (ConfigStore::new(None), None)
    };

    let cfg = config_store.get();
    let user_data = default_user_data_dir();
    let legacy_data = legacy_user_data_dir();
    let mut shell_roots = vec![user_data.join("shells"), bundled_shells];
    let mut balloon_roots = vec![user_data.join("balloons"), bundled_balloons];
    if legacy_data != user_data && legacy_data.is_dir() {
        shell_roots.push(legacy_data.join("shells"));
        balloon_roots.push(legacy_data.join("balloons"));
    }

    let shell_manager = ShellManager::new(shell_roots);
    let balloon_manager = BalloonManager::new(balloon_roots);

    let active_manifest = shell_manager
        .load_shell(&cfg.active_shell_id)
        .or_else(|_| shell_manager.load_shell("neo_default"))?;
    let active_balloon = balloon_manager
        .load_balloon(&cfg.active_balloon_id)
        .or_else(|_| balloon_manager.load_balloon("clean_default"))?;

    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        builder.with_activation_policy(ActivationPolicy::Accessory);
    }
    let event_loop = builder
        .build()
        .map_err(|e| format!("이벤트 루프 초기화 실패: {e}"))?;
    let proxy = event_loop.create_proxy();

    let status_proxy = proxy.clone();
    let status_cb: StatusCallback = Arc::new(move |msg, active| {
        let _ = status_proxy.send_event(UserEvent::StatusUpdate(msg, active));
    });

    let manifest_arc = Arc::new(Mutex::new(active_manifest));
    let tool_runner = BuiltinToolRunner::new(config_store.clone(), None);
    let engine = AgentEngine::new(
        config_store.clone(),
        tool_runner,
        Arc::clone(&manifest_arc),
        Some(status_cb),
    );

    let smoke_passed = Arc::new(Mutex::new(false));
    let mut app = AppRuntime {
        smoke_mode: smoke_test,
        smoke_user_dir: smoke_tmp,
        smoke_phase: 0,
        smoke_passed: Arc::clone(&smoke_passed),
        proxy,
        config_store,
        shell_manager,
        balloon_manager,
        manifest: manifest_arc,
        balloon_skin: active_balloon,
        engine,
        gpu: None,
        text_renderer: TextRenderer::new(),
        sakura: None,
        kero: None,
        sakura_balloon: None,
        kero_balloon: None,
        communicate: None,
        menu: None,
        playback: PlaybackQueue::new(),
        tray: None,
        tray_actions: HashMap::new(),
        server: None,
        os_insets: detect_os_insets(),
        modifiers: ModifiersState::empty(),
        auto_talk_last: Instant::now(),
        initialized: false,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("GUI 이벤트 루프 오류: {e}"))?;

    if smoke_test && !*smoke_passed.lock().expect("smoke mutex poisoned") {
        return Err("스모크 테스트가 통과되지 않았습니다.".to_string());
    }
    Ok(())
}
