use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use arboard::Clipboard;
use chrono::{Datelike, Local, Timelike, Utc};
use cosmic_text::{
    Attrs, Buffer, Color as TextColor, Family, FontSystem, Metrics, Shaping, SwashCache, Wrap,
};
use mouse_position::mouse_position::Mouse;
use rand::seq::SliceRandom;
use serde_json::{json, Value};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use tray_icon::{Icon as TrayIconImage, TrayIcon, TrayIconBuilder};
use wgpu::util::DeviceExt;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

#[cfg(target_os = "macos")]
use winit::platform::macos::{
    ActivationPolicy, EventLoopBuilderExtMacOS, WindowAttributesExtMacOS, WindowExtMacOS,
};
#[cfg(target_os = "windows")]
use winit::platform::windows::{WindowAttributesExtWindows, WindowExtWindows};

use crate::agent_engine::{provider_catalog, AgentEngine, AgentReply, StatusCallback};
use crate::builtin_tools::BuiltinToolRunner;
use crate::config_store::{
    default_user_data_dir, legacy_user_data_dirs, ConfigPatch, ConfigStore,
};
use crate::dialogue_parser::{parse_dialogue, ActionKind, DialogueAction, Scope};
use crate::live2d_rig::{
    discover_and_load_rig, Live2dMotionInputs, LoadedCharacterRig,
};
use crate::settings_panel::{
    SettingsAction, SettingsPanelData, SettingsPanelModel, SliderKind, SETTINGS_LOGICAL_H,
    SETTINGS_LOGICAL_W,
};
use crate::shell_parser::{
    install_nar_archive, BalloonManager, BalloonSkin, BalloonSummary, CollisionRect, RgbaBitmap,
    ShellManifest, ShellManager, ShellSummary, StrokeDetector,
};
use crate::sstp_server::{start_dual_server, ServerBackend, ServerHandle, SettingsPatchReq};

const POLL_INTERVAL_MS: u64 = 50;
const TYPEWRITER_INTERVAL_MS: u64 = 38;

#[repr(C)]
#[derive(Copy, Clone)]
struct ShaderParams {
    premul: u32,
    live2d: u32,
    rig_kind: u32,
    pad1: u32,
    breath: f32,
    angle_x: f32,
    angle_y: f32,
    angle_z: f32,
    hair_swing_x: f32,
    hair_swing_y: f32,
    ahoge_swing: f32,
    cloth_swing: f32,
    body_sway: f32,
    squish: f32,
    talk_bob: f32,
    top_v: f32,
    anchor_v: f32,
    center_u: f32,
    aspect_hw: f32,
    pad2: f32,
    extra: [f32; 4],
}

impl ShaderParams {
    fn idle(premul: u32) -> Self {
        Self {
            premul,
            live2d: 0,
            rig_kind: 0,
            pad1: 0,
            breath: 0.0,
            angle_x: 0.0,
            angle_y: 0.0,
            angle_z: 0.0,
            hair_swing_x: 0.0,
            hair_swing_y: 0.0,
            ahoge_swing: 0.0,
            cloth_swing: 0.0,
            body_sway: 0.0,
            squish: 0.0,
            talk_bob: 0.0,
            top_v: 0.02,
            anchor_v: 0.96,
            center_u: 0.50,
            aspect_hw: 1.5,
            pad2: 1.0,
            extra: [0.0; 4],
        }
    }
}

fn as_bytes_shader_params(p: &ShaderParams) -> [u8; 96] {
    let mut out = [0u8; 96];
    out[0..4].copy_from_slice(&p.premul.to_ne_bytes());
    out[4..8].copy_from_slice(&p.live2d.to_ne_bytes());
    out[8..12].copy_from_slice(&p.rig_kind.to_ne_bytes());
    out[12..16].copy_from_slice(&p.pad1.to_ne_bytes());
    out[16..20].copy_from_slice(&p.breath.to_ne_bytes());
    out[20..24].copy_from_slice(&p.angle_x.to_ne_bytes());
    out[24..28].copy_from_slice(&p.angle_y.to_ne_bytes());
    out[28..32].copy_from_slice(&p.angle_z.to_ne_bytes());
    out[32..36].copy_from_slice(&p.hair_swing_x.to_ne_bytes());
    out[36..40].copy_from_slice(&p.hair_swing_y.to_ne_bytes());
    out[40..44].copy_from_slice(&p.ahoge_swing.to_ne_bytes());
    out[44..48].copy_from_slice(&p.cloth_swing.to_ne_bytes());
    out[48..52].copy_from_slice(&p.body_sway.to_ne_bytes());
    out[52..56].copy_from_slice(&p.squish.to_ne_bytes());
    out[56..60].copy_from_slice(&p.talk_bob.to_ne_bytes());
    out[60..64].copy_from_slice(&p.top_v.to_ne_bytes());
    out[64..68].copy_from_slice(&p.anchor_v.to_ne_bytes());
    out[68..72].copy_from_slice(&p.center_u.to_ne_bytes());
    out[72..76].copy_from_slice(&p.aspect_hw.to_ne_bytes());
    out[76..80].copy_from_slice(&p.pad2.to_ne_bytes());
    out[80..84].copy_from_slice(&p.extra[0].to_ne_bytes());
    out[84..88].copy_from_slice(&p.extra[1].to_ne_bytes());
    out[88..92].copy_from_slice(&p.extra[2].to_ne_bytes());
    out[92..96].copy_from_slice(&p.extra[3].to_ne_bytes());
    out
}

const QUAD_SHADER_WGSL: &str = r#"
struct Params {
    premul: u32,
    live2d: u32,
    rig_kind: u32,
    pad1: u32,
    breath: f32,
    angle_x: f32,
    angle_y: f32,
    angle_z: f32,
    hair_swing_x: f32,
    hair_swing_y: f32,
    ahoge_swing: f32,
    cloth_swing: f32,
    body_sway: f32,
    squish: f32,
    talk_bob: f32,
    top_v: f32,
    anchor_v: f32,
    center_u: f32,
    aspect_hw: f32,
    pad2: f32,
    extra: vec4<f32>,
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
    var su = in.uv.x;
    var sv = in.uv.y;
    if (u_params.live2d != 0u) {
        let tv = clamp(u_params.top_v, 0.0, 0.35);
        let av = clamp(u_params.anchor_v, 0.55, 0.995);
        let span = max(av - tv, 0.25);
        let yn = clamp((in.uv.y - tv) / span, 0.0, 1.0);
        let cx = clamp(u_params.center_u, 0.32, 0.68);
        let aspect = max(u_params.aspect_hw, 0.5);
        let dx_raw = in.uv.x - cx;
        let abs_dx = abs(dx_raw);

        if (u_params.rig_kind == 0u) {
            // === Humanoid Live2D Hierarchical Rig (\0 Sakura) ===
            // 1. Boots/soles (yn >= 0.89) remain 100% rigid & planted
            let foot_lock = smoothstep(0.89, 0.68, yn);
            // 2. Neck-pivoted head rotation (aspect-corrected so zero squashing/shearing)
            let head_mask = smoothstep(0.35, 0.25, yn);
            let neck_v = mix(tv, av, 0.31);
            let rot_ang = u_params.angle_z * head_mask;
            let ca = cos(rot_ang);
            let sa = sin(rot_ang);
            let dy_neck = (sv - neck_v) * aspect;
            let rx_n = dx_raw * ca + dy_neck * sa;
            let ry_n = (-dx_raw * sa + dy_neck * ca) / aspect;
            su = rx_n + cx;
            sv = ry_n + neck_v;

            // 3. Localized ParamBreath chest lift & ribcage expand (legs/head unstretched)
            let chest_lift = smoothstep(0.60, 0.35, yn);
            let chest_band = smoothstep(0.24, 0.35, yn) * (1.0 - smoothstep(0.43, 0.56, yn));
            sv = sv + u_params.breath * 0.0046 * chest_lift + u_params.talk_bob * 0.0042 * smoothstep(0.55, 0.22, yn);
            su = su - (su - cx) * (u_params.breath * 0.014 * chest_band + u_params.squish * 0.018 * chest_band);

            // 4. Upper-body weight shift & squish (zero effect on planted boots)
            let upper_shift = smoothstep(0.86, 0.36, yn);
            su = su - u_params.body_sway * 0.0085 * upper_shift;
            sv = sv - u_params.squish * 0.010 * smoothstep(0.88, 0.20, yn);

            // 5. Sailor ribbon & pleated skirt hem cloth pendulum
            let ribbon_w = smoothstep(0.31, 0.36, yn)
                * (1.0 - smoothstep(0.44, 0.50, yn))
                * (1.0 - smoothstep(0.05, 0.14, abs_dx));
            let skirt_root = smoothstep(0.46, 0.65, yn);
            let skirt_hem = smoothstep(0.47, 0.65, yn) * (1.0 - smoothstep(0.66, 0.74, yn));
            let pleat = 1.0 + 0.32 * sin(dx_raw * 26.0);
            su = su - u_params.cloth_swing * (0.007 * ribbon_w + 0.013 * skirt_hem * pleat);
            sv = sv + abs(u_params.cloth_swing) * 0.0035 * skirt_root * skirt_hem;

            // 6. 2.5D differential face vs head outline parallax (AngleX / AngleY)
            let face_v = mix(tv, av, 0.225);
            let fx = (su - cx) / 0.19;
            let fy = (sv - face_v) / 0.105;
            let r2 = fx * fx + fy * fy;
            let face_inner = exp(-r2 * 1.45) * head_mask;
            let head_outer = exp(-r2 * 0.35) * head_mask;
            su = su - u_params.angle_x * (0.0065 * head_outer + 0.0125 * face_inner);
            sv = sv - u_params.angle_y * (0.0040 * head_outer + 0.0075 * face_inner);
            // Cheek perspective foreshortening when turning head
            su = su + (su - cx) * (u_params.angle_x * 0.040 * face_inner);

            // 7. Side hair curl & crown ahoge damped-spring pendulums
            let curl_t = clamp((yn - 0.12) / 0.24, 0.0, 1.0);
            let curl_cubic = curl_t * curl_t * (3.0 - 2.0 * curl_t);
            let side_hair = smoothstep(0.075, 0.17, abs_dx)
                * smoothstep(0.08, 0.20, yn)
                * (1.0 - smoothstep(0.32, 0.42, yn));
            su = su - u_params.hair_swing_x * (0.014 * curl_cubic * side_hair);
            sv = sv - u_params.hair_swing_y * (0.0065 * curl_cubic * side_hair);

            let ahoge_mask = smoothstep(0.115, 0.02, yn) * (1.0 - smoothstep(0.06, 0.16, abs_dx));
            su = su - u_params.ahoge_swing * 0.019 * ahoge_mask;
            sv = sv - abs(u_params.ahoge_swing) * 0.004 * ahoge_mask;

            // Blend full deformation cleanly inside foot_lock envelope
            su = mix(in.uv.x, su, max(foot_lock, head_mask));
            sv = mix(in.uv.y, sv, max(foot_lock, head_mask));
        } else if (u_params.rig_kind == 1u) {
            // === Mochi Companion Live2D Rig (\1 Unyu) ===
            let sole_lock = smoothstep(0.94, 0.80, yn);
            let face_mask = smoothstep(0.76, 0.26, yn);
            let head_v = mix(tv, av, 0.46);
            let rot_ang = u_params.angle_z * face_mask;
            let ca = cos(rot_ang);
            let sa = sin(rot_ang);
            let dy_h = (sv - head_v) * aspect;
            su = (dx_raw * ca + dy_h * sa) + cx;
            sv = (-dx_raw * sa + dy_h * ca) / aspect + head_v;

            // Plump mochi belly breath + bouncy squish + speech bounce
            let belly = smoothstep(0.90, 0.58, yn) * smoothstep(0.14, 0.48, yn);
            su = su - (su - cx) * (u_params.breath * 0.018 * belly + u_params.squish * 0.028 * belly);
            sv = sv + (u_params.breath * 0.006 * smoothstep(0.88, 0.42, yn)
                - u_params.squish * 0.014 * smoothstep(0.92, 0.24, yn)
                + u_params.talk_bob * 0.009 * smoothstep(0.86, 0.30, yn));

            // 2.5D spherical gaze parallax (eyes & low-perched round sunglasses)
            let fx = (su - cx) / 0.26;
            let fy = (yn - 0.42) / 0.22;
            let inner = exp(-(fx * fx + fy * fy) * 1.15) * face_mask;
            su = su - (u_params.angle_x * (0.012 * inner + 0.006 * face_mask) + u_params.body_sway * 0.008 * sole_lock);
            sv = sv - (u_params.angle_y * (0.008 * inner + 0.003 * face_mask));

            // Top spiky crest / tuft spring + paw wave on speech
            let tuft = smoothstep(0.32, 0.06, yn) * smoothstep(0.56, 0.24, su);
            su = su - u_params.ahoge_swing * 0.018 * tuft - u_params.hair_swing_x * 0.010 * smoothstep(0.38, 0.14, yn);
            let paws = smoothstep(0.16, 0.28, abs_dx) * smoothstep(0.48, 0.58, yn) * (1.0 - smoothstep(0.68, 0.78, yn));
            sv = sv + u_params.talk_bob * 0.008 * paws;

            su = mix(in.uv.x, su, sole_lock);
            sv = mix(in.uv.y, sv, sole_lock);
        } else {
            // === Gentle Classic Shell Rig (bust-cut / tall-cap / humanoid \1 safe) ===
            let base_lock = smoothstep(0.95, 0.78, yn);
            let upper_mask = smoothstep(0.65, 0.20, yn);
            let pivot_v = mix(tv, av, 0.55);
            let rot_ang = u_params.angle_z * 0.38 * upper_mask;
            let ca = cos(rot_ang);
            let sa = sin(rot_ang);
            let dy_p = (sv - pivot_v) * aspect;
            su = (dx_raw * ca + dy_p * sa) + cx;
            sv = (-dx_raw * sa + dy_p * ca) / aspect + pivot_v;

            sv = sv + u_params.breath * 0.0032 * upper_mask
                - u_params.squish * 0.008 * smoothstep(0.88, 0.22, yn)
                + u_params.talk_bob * 0.0035 * upper_mask;
            su = su - (u_params.body_sway * 0.0045 + u_params.angle_x * 0.0045) * upper_mask;

            su = mix(in.uv.x, su, base_lock);
            sv = mix(in.uv.y, sv, base_lock);
        }
    }
    if (su < 0.0 || su > 1.0 || sv < 0.0 || sv > 1.0) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    if (u_params.premul == 999u) {
        return textureSampleLevel(u_tex, u_samp, vec2<f32>(su, sv), 0.0);
    }
    let dims_u = textureDimensions(u_tex, 0);
    let dims = vec2<f32>(dims_u);
    let px = vec2<f32>(su, sv) * dims - vec2<f32>(0.5, 0.5);
    let p0 = vec2<i32>(floor(px));
    let f = fract(px);
    let max_c = vec2<i32>(dims_u) - vec2<i32>(1, 1);
    let t00 = textureLoad(u_tex, clamp(p0, vec2<i32>(0, 0), max_c), 0);
    let t10 = textureLoad(u_tex, clamp(p0 + vec2<i32>(1, 0), vec2<i32>(0, 0), max_c), 0);
    let t01 = textureLoad(u_tex, clamp(p0 + vec2<i32>(0, 1), vec2<i32>(0, 0), max_c), 0);
    let t11 = textureLoad(u_tex, clamp(p0 + vec2<i32>(1, 1), vec2<i32>(0, 0), max_c), 0);
    let pm00 = vec4<f32>(t00.rgb * t00.a, t00.a);
    let pm10 = vec4<f32>(t10.rgb * t10.a, t10.a);
    let pm01 = vec4<f32>(t01.rgb * t01.a, t01.a);
    let pm11 = vec4<f32>(t11.rgb * t11.a, t11.a);
    let pm = mix(mix(pm00, pm10, f.x), mix(pm01, pm11, f.x), f.y);
    let op = clamp(u_params.pad2, 0.0, 1.0);
    let scaled_pm = pm * op;
    if (scaled_pm.a <= 0.0001) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    var out_col: vec4<f32>;
    if (u_params.premul != 0u) {
        out_col = scaled_pm;
    } else {
        out_col = vec4<f32>(scaled_pm.rgb / scaled_pm.a, scaled_pm.a);
    }
    if (u_params.pad1 != 0u) {
        let clamped_rgb = clamp(out_col.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
        let lin_lo = clamped_rgb / 12.92;
        let lin_hi = pow((clamped_rgb + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
        let lin = select(lin_lo, lin_hi, clamped_rgb > vec3<f32>(0.04045));
        return vec4<f32>(lin, out_col.a);
    }
    return out_col;
}
"#;

const PART_SHADER_WGSL: &str = r#"
struct PartUniform {
    inv_row0: vec4<f32>,
    inv_row1: vec4<f32>,
    deform: vec4<f32>,
    flags_alpha: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var u_tex: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_part: PartUniform;

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
    if (u_part.flags_alpha.x < 0.5) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    let cw = max(u_part.inv_row0.w, 1.0);
    let ch = max(u_part.inv_row1.w, 1.0);
    let op = in.uv * vec2<f32>(cw, ch);
    let rel = clamp(((op.y - u_part.deform.x) * u_part.deform.y) / max(u_part.deform.z, 1.0), 0.0, 1.0);
    let bend = u_part.deform.w * rel * rel;
    let sx_c = u_part.inv_row0.x * op.x + u_part.inv_row0.y * op.y + u_part.inv_row0.z - bend;
    let sy_c = u_part.inv_row1.x * op.x + u_part.inv_row1.y * op.y + u_part.inv_row1.z;

    let dims_u = textureDimensions(u_tex, 0);
    let dims = vec2<f32>(dims_u);
    let scale_uv = dims / vec2<f32>(cw, ch);
    let sx = sx_c * scale_uv.x;
    let sy = sy_c * scale_uv.y;
    if (sx < 0.0 || sx >= dims.x || sy < 0.0 || sy >= dims.y) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    let px = vec2<f32>(sx, sy) - vec2<f32>(0.5, 0.5);
    let p0 = vec2<i32>(floor(px));
    let f = fract(px);
    let max_c = vec2<i32>(dims_u) - vec2<i32>(1, 1);
    let t00 = textureLoad(u_tex, clamp(p0, vec2<i32>(0, 0), max_c), 0);
    let t10 = textureLoad(u_tex, clamp(p0 + vec2<i32>(1, 0), vec2<i32>(0, 0), max_c), 0);
    let t01 = textureLoad(u_tex, clamp(p0 + vec2<i32>(0, 1), vec2<i32>(0, 0), max_c), 0);
    let t11 = textureLoad(u_tex, clamp(p0 + vec2<i32>(1, 1), vec2<i32>(0, 0), max_c), 0);
    let pm00 = vec4<f32>(t00.rgb * t00.a, t00.a);
    let pm10 = vec4<f32>(t10.rgb * t10.a, t10.a);
    let pm01 = vec4<f32>(t01.rgb * t01.a, t01.a);
    let pm11 = vec4<f32>(t11.rgb * t11.a, t11.a);
    let pm = mix(mix(pm00, pm10, f.x), mix(pm01, pm11, f.x), f.y);
    return pm * u_part.flags_alpha.y;
}
"#;

const PRESENT_OFFSCREEN_WGSL: &str = r#"
struct PresentParams {
    premul: u32,
    pad0: u32,
    opacity: f32,
    pad2: u32,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@group(0) @binding(0) var u_tex: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u_present: PresentParams;

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
    let dims_u = textureDimensions(u_tex, 0);
    let coord = clamp(vec2<i32>(floor(in.uv * vec2<f32>(dims_u))), vec2<i32>(0, 0), vec2<i32>(dims_u) - vec2<i32>(1, 1));
    let op = clamp(u_present.opacity, 0.0, 1.0);
    let pm = textureLoad(u_tex, coord, 0) * op;
    if (pm.a <= 0.0001) {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    var out_col: vec4<f32>;
    if (u_present.premul != 0u) {
        out_col = pm;
    } else {
        out_col = vec4<f32>(pm.rgb / pm.a, pm.a);
    }
    if (u_present.pad0 != 0u) {
        let clamped_rgb = clamp(out_col.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
        let lin_lo = clamped_rgb / 12.92;
        let lin_hi = pow((clamped_rgb + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
        let lin = select(lin_lo, lin_hi, clamped_rgb > vec3<f32>(0.04045));
        return vec4<f32>(lin, out_col.a);
    }
    return out_col;
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

pub fn resolve_tools_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let mac_tools = dir.join("../Resources/tools");
            if mac_tools.is_dir() {
                return mac_tools;
            }
            let local_tools = dir.join("tools");
            if local_tools.is_dir() {
                return local_tools;
            }
        }
    }
    let manifest_tools = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools");
    if manifest_tools.is_dir() {
        return manifest_tools;
    }
    PathBuf::from("tools")
}

fn open_local_path_in_os(target: &Path) -> Result<(), String> {
    let resolved = target.to_string_lossy().to_string();
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&resolved)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&resolved)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

struct GpuContext {
    _instance: wgpu::Instance,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    part_pipeline: wgpu::RenderPipeline,
    part_bind_group_layout: wgpu::BindGroupLayout,
    present_pipeline: wgpu::RenderPipeline,
    present_bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    premul: u32,
    surface_is_srgb: u32,
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
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .unwrap_or_else(|| caps.formats[0]);

        let surface_is_srgb = u32::from(surface_format.is_srgb());

        let alpha_mode = [
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
        ]
        .into_iter()
        .find(|m| caps.alpha_modes.contains(m))
        .unwrap_or(caps.alpha_modes[0]);

        // Keep sprite & multi-part offscreen textures linear-addressed UNORM so 2D alpha
        // blending executes in standard 2D sRGB perceptual space (matching CPU composition).
        let texture_format = wgpu::TextureFormat::Rgba8Unorm;

        let premul = if alpha_mode == wgpu::CompositeAlphaMode::PreMultiplied {
            1
        } else {
            0
        };

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

        let part_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("part-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
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

        let part_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("part-shader"),
            source: wgpu::ShaderSource::Wgsl(PART_SHADER_WGSL.into()),
        });

        let part_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("part-pl"),
            bind_group_layouts: &[&part_bind_group_layout],
            push_constant_ranges: &[],
        });

        let part_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("part-pipeline"),
            layout: Some(&part_pl),
            vertex: wgpu::VertexState {
                module: &part_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &part_shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: texture_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let present_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("present-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
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

        let present_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("present-shader"),
            source: wgpu::ShaderSource::Wgsl(PRESENT_OFFSCREEN_WGSL.into()),
        });

        let present_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("present-pl"),
            bind_group_layouts: &[&present_bind_group_layout],
            push_constant_ranges: &[],
        });

        let present_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("present-pipeline"),
            layout: Some(&present_pl),
            vertex: wgpu::VertexState {
                module: &present_shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &present_shader,
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
            part_pipeline,
            part_bind_group_layout,
            present_pipeline,
            present_bind_group_layout,
            sampler,
            premul,
            surface_is_srgb,
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

struct GpuPartLayer {
    texture: wgpu::Texture,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    tex_size: (u32, u32),
}

struct GpuOffscreenTarget {
    _texture: wgpu::Texture,
    view: wgpu::TextureView,
    present_bind_group: wgpu::BindGroup,
    size: (u32, u32),
}

struct WindowSurface {
    // Drop `surface` before `window`!
    surface: wgpu::Surface<'static>,
    window: Arc<Window>,
    config: wgpu::SurfaceConfiguration,
    texture: Option<wgpu::Texture>,
    bind_group: Option<wgpu::BindGroup>,
    params_buffer: wgpu::Buffer,
    present_params_buffer: wgpu::Buffer,
    opacity: f32,
    last_params: ShaderParams,
    part_layers: Vec<GpuPartLayer>,
    offscreen_target: Option<GpuOffscreenTarget>,
    multipart_active: bool,
    last_live2d_active: bool,
    tex_size: (u32, u32),
}

impl WindowSurface {
    fn encode_present_params(gpu: &GpuContext, opacity: f32) -> [u8; 16] {
        let mut present_bytes = [0u8; 16];
        present_bytes[0..4].copy_from_slice(&gpu.premul.to_ne_bytes());
        present_bytes[4..8].copy_from_slice(&gpu.surface_is_srgb.to_ne_bytes());
        present_bytes[8..12].copy_from_slice(&opacity.clamp(0.0, 1.0).to_ne_bytes());
        present_bytes
    }

    fn from_existing(
        surface: wgpu::Surface<'static>,
        window: Arc<Window>,
        gpu: &GpuContext,
    ) -> Self {
        let size = window.inner_size();
        let config = gpu.make_surface_config(size.width, size.height);
        surface.configure(&gpu.device, &config);
        let mut init_params = ShaderParams::idle(gpu.premul);
        init_params.pad1 = gpu.surface_is_srgb;
        init_params.pad2 = 1.0;
        let params_buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("window-shader-params"),
            contents: &as_bytes_shader_params(&init_params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let present_params_buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("window-present-params"),
            contents: &Self::encode_present_params(gpu, 1.0),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            surface,
            window,
            config,
            texture: None,
            bind_group: None,
            params_buffer,
            present_params_buffer,
            opacity: 1.0,
            last_params: init_params,
            part_layers: Vec::new(),
            offscreen_target: None,
            multipart_active: false,
            last_live2d_active: false,
            tex_size: (0, 0),
        }
    }

    fn update_params(&mut self, gpu: &GpuContext, params: &ShaderParams) {
        let mut eff = *params;
        eff.pad1 = gpu.surface_is_srgb;
        eff.pad2 = self.opacity;
        self.last_params = eff;
        gpu.queue
            .write_buffer(&self.params_buffer, 0, &as_bytes_shader_params(&eff));
        self.last_live2d_active = params.live2d != 0;
    }

    fn set_opacity(&mut self, gpu: &GpuContext, opacity: f32) -> bool {
        let op = opacity.clamp(0.0, 1.0);
        if (self.opacity - op).abs() < 0.002 {
            return false;
        }
        self.opacity = op;
        self.last_params.pad1 = gpu.surface_is_srgb;
        self.last_params.pad2 = op;
        gpu.queue
            .write_buffer(&self.params_buffer, 0, &as_bytes_shader_params(&self.last_params));
        gpu.queue
            .write_buffer(&self.present_params_buffer, 0, &Self::encode_present_params(gpu, op));
        self.window.request_redraw();
        true
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

    fn sync_part_rig(&mut self, gpu: &GpuContext, rig_opt: Option<&mut LoadedCharacterRig>) {
        let Some(rig) = rig_opt else {
            self.part_layers.clear();
            self.offscreen_target = None;
            self.multipart_active = false;
            return;
        };
        if rig.parts.is_empty() {
            self.part_layers.clear();
            self.offscreen_target = None;
            self.multipart_active = false;
            return;
        }
        self.multipart_active = true;
        if self.part_layers.len() != rig.parts.len() {
            self.part_layers.clear();
            for part in &mut rig.parts {
                let bw = part.current_bitmap.width.max(1);
                let bh = part.current_bitmap.height.max(1);
                let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("part-layer-tex"),
                    size: wgpu::Extent3d {
                        width: bw,
                        height: bh,
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
                let uniform_buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("part-uniform-buf"),
                    contents: &part.uniform.to_bytes(),
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                });
                let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("part-layer-bg"),
                    layout: &gpu.part_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: uniform_buffer.as_entire_binding(),
                        },
                    ],
                });
                self.part_layers.push(GpuPartLayer {
                    texture: tex,
                    uniform_buffer,
                    bind_group,
                    tex_size: (bw, bh),
                });
                part.dirty_gpu = true;
            }
        }

        for (i, part) in rig.parts.iter_mut().enumerate() {
            let gpu_layer = &mut self.part_layers[i];
            let bw = part.current_bitmap.width.max(1);
            let bh = part.current_bitmap.height.max(1);
            if part.dirty_gpu {
                if gpu_layer.tex_size != (bw, bh) {
                    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("part-layer-tex"),
                        size: wgpu::Extent3d {
                            width: bw,
                            height: bh,
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
                    gpu_layer.bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("part-layer-bg"),
                        layout: &gpu.part_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: gpu_layer.uniform_buffer.as_entire_binding(),
                            },
                        ],
                    });
                    gpu_layer.texture = tex;
                    gpu_layer.tex_size = (bw, bh);
                }
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &gpu_layer.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &part.current_bitmap.data,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * bw),
                        rows_per_image: Some(bh),
                    },
                    wgpu::Extent3d {
                        width: bw,
                        height: bh,
                        depth_or_array_layers: 1,
                    },
                );
                part.dirty_gpu = false;
            }
            gpu.queue
                .write_buffer(&gpu_layer.uniform_buffer, 0, &part.uniform.to_bytes());
        }
    }

    fn ensure_offscreen_target(&mut self, gpu: &GpuContext, w: u32, h: u32) {
        let tw = w.max(1);
        let th = h.max(1);
        let needs_new = self
            .offscreen_target
            .as_ref()
            .is_none_or(|t| t.size != (tw, th));
        if !needs_new {
            return;
        }
        let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("multipart-offscreen-tex"),
            size: wgpu::Extent3d {
                width: tw,
                height: th,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: gpu.texture_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let present_bg = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present-offscreen-bg"),
            layout: &gpu.present_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.present_params_buffer.as_entire_binding(),
                },
            ],
        });
        self.offscreen_target = Some(GpuOffscreenTarget {
            _texture: tex,
            view,
            present_bind_group: present_bg,
            size: (tw, th),
        });
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
                        resource: self.params_buffer.as_entire_binding(),
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
        let use_multipart = self.multipart_active && !self.part_layers.is_empty();
        if !use_multipart && self.bind_group.is_none() {
            return;
        }
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

        if use_multipart {
            self.ensure_offscreen_target(gpu, self.config.width, self.config.height);
            if let Some(ref target) = self.offscreen_target {
                {
                    let mut p_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("multipart-offscreen-pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &target.view,
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
                    p_pass.set_pipeline(&gpu.part_pipeline);
                    for layer in &self.part_layers {
                        p_pass.set_bind_group(0, &layer.bind_group, &[]);
                        p_pass.draw(0..3, 0..1);
                    }
                }
                {
                    let mut pres_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("multipart-present-pass"),
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
                    pres_pass.set_pipeline(&gpu.present_pipeline);
                    pres_pass.set_bind_group(0, &target.present_bind_group, &[]);
                    pres_pass.draw(0..3, 0..1);
                }
            }
        } else if let Some(ref bg) = self.bind_group {
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

pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
}

impl TextRenderer {
    pub fn new() -> Self {
        Self {
            font_system: FontSystem::new(),
            swash_cache: SwashCache::new(),
        }
    }

    pub fn measure_lines(&mut self, text: &str, font_px: f32, line_h: f32, max_w: f32) -> (usize, f32) {
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

    pub fn measure_inline_width(&mut self, text: &str, font_px: f32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let line_h = font_px * 1.3;
        let measure_raw = |fs: &mut FontSystem, s: &str| -> f32 {
            let mut b = Buffer::new(fs, Metrics::new(font_px, line_h));
            b.set_wrap(fs, Wrap::None);
            b.set_size(fs, None, None);
            b.set_text(fs, s, Attrs::new().family(Family::SansSerif), Shaping::Advanced);
            b.shape_until_scroll(fs, false);
            b.layout_runs().map(|r| r.line_w).fold(0.0_f32, f32::max)
        };
        let with_sentinel = format!("{text}|");
        let w_all = measure_raw(&mut self.font_system, &with_sentinel);
        let w_mark = measure_raw(&mut self.font_system, "|");
        (w_all - w_mark).max(0.0)
    }

    pub fn draw_inline_text(
        &mut self,
        bmp: &mut RgbaBitmap,
        text: &str,
        x: i32,
        y: i32,
        clip_x0: i32,
        clip_x1: i32,
        font_px: f32,
        line_h: f32,
        rgb: (u8, u8, u8),
    ) {
        if text.is_empty() {
            return;
        }
        let mut buf = Buffer::new(&mut self.font_system, Metrics::new(font_px, line_h));
        buf.set_wrap(&mut self.font_system, Wrap::None);
        buf.set_size(&mut self.font_system, None, None);
        buf.set_text(
            &mut self.font_system,
            text,
            Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
        );
        buf.shape_until_scroll(&mut self.font_system, false);
        let color = TextColor::rgba(rgb.0, rgb.1, rgb.2, 255);
        buf.draw(&mut self.font_system, &mut self.swash_cache, color, |gx, gy, _gw, _gh, col| {
            let px = x + gx;
            let py = y + gy;
            if px < clip_x0 || px >= clip_x1 || py < 0 || py >= bmp.height as i32 {
                return;
            }
            let (r, g, b, a) = col.as_rgba_tuple();
            if a > 0 {
                bmp.blend_pixel(px, py, r, g, b, a);
            }
        });
    }

    pub fn draw_text(
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
        // Keep vertical layout unbounded so scrolled lines remain shapeable prior to clipping.
        buf.set_size(&mut self.font_system, Some(max_w.max(20.0)), None);
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

#[derive(Clone, Debug, Default)]
struct Spring1D {
    pos: f32,
    vel: f32,
}

impl Spring1D {
    fn step(&mut self, target: f32, omega: f32, zeta: f32, dt: f32) {
        let sub_dt = dt.clamp(0.001, 0.045) * 0.5;
        for _ in 0..2 {
            let acc = omega * omega * (target - self.pos) - 2.0 * zeta * omega * self.vel;
            self.vel = (self.vel + acc * sub_dt).clamp(-8.0, 8.0);
            self.pos = (self.pos + self.vel * sub_dt).clamp(-2.0, 2.0);
        }
    }
}

#[derive(Clone, Debug)]
struct Live2dRigState {
    last_tick: Instant,
    angle_x: f32,
    angle_y: f32,
    angle_z: f32,
    prev_angle_x: f32,
    prev_angle_y: f32,
    hair_x: Spring1D,
    hair_y: Spring1D,
    ahoge: Spring1D,
    cloth: Spring1D,
}

impl Live2dRigState {
    fn new() -> Self {
        Self {
            last_tick: Instant::now(),
            angle_x: 0.0,
            angle_y: 0.0,
            angle_z: 0.0,
            prev_angle_x: 0.0,
            prev_angle_y: 0.0,
            hair_x: Spring1D::default(),
            hair_y: Spring1D::default(),
            ahoge: Spring1D::default(),
            cloth: Spring1D::default(),
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

#[derive(Clone, Debug)]
struct SerikoStep {
    aid: i32,
    method: String,
    surface_id: i32,
    offset_x: i32,
    offset_y: i32,
    delay_ms: u64,
    alt_ids: Vec<i32>,
}

#[derive(Clone, Debug)]
struct ActiveSerikoRun {
    steps: Vec<SerikoStep>,
    idx: usize,
    next_step_at: Instant,
}

struct CharacterWindowState {
    scope: Scope,
    ws: WindowSurface,
    surface_id: i32,
    is_neo_default: bool,
    display_scale: f64,
    user_scale: f64,
    hovered_opaque: bool,
    base_bmp: Option<RgbaBitmap>,
    overlays: HashMap<i32, (i32, i32, bool, RgbaBitmap)>,
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
    anim_start: Instant,
    phase_offset: f32,
    smooth_gaze: (f32, f32),
    bounce_at: Option<Instant>,
    bounce_strength: f32,
    top_v: f32,
    anchor_v: f32,
    center_u: f32,
    aspect_hw: f32,
    rig: Live2dRigState,
    rig_parts: Option<LoadedCharacterRig>,
    active_binds: Vec<i32>,
    bind_overlays: Vec<(i32, i32, RgbaBitmap)>,
    was_speaking: bool,
    visible: bool,
}

impl CharacterWindowState {
    fn sync_rig_with_current_surface(&mut self, gpu: &GpuContext) {
        if let Some(ref mut rig) = self.rig_parts {
            let mut keys: Vec<i32> = self.overlays.keys().copied().collect();
            keys.sort_unstable();
            let combined = self
                .bind_overlays
                .iter()
                .map(|(ox, oy, bmp)| (*ox, *oy, false, bmp))
                .chain(
                    keys.iter()
                        .filter_map(|k| self.overlays.get(k).map(|(ox, oy, rep, ov)| (*ox, *oy, *rep, ov))),
                );
            let _ = rig.sync_from_surface_state(self.surface_id, combined, &self.composited);
            self.ws.sync_part_rig(gpu, Some(rig));
        } else {
            self.ws.sync_part_rig(gpu, None);
        }
    }

    fn reload_sidecar_rig(
        &mut self,
        manifest: &ShellManifest,
        user_data_dir: &Path,
        assets_dir: &Path,
        gpu: &GpuContext,
    ) {
        self.rig_parts = discover_and_load_rig(
            &manifest.shell_id,
            &manifest.dir_path,
            user_data_dir,
            assets_dir,
            self.scope,
        );
        self.sync_rig_with_current_surface(gpu);
    }
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

    fn effective_display_scale(&self) -> f64 {
        (self.display_scale / self.user_scale.clamp(0.5, 1.5)).clamp(0.25, 8.0)
    }

    fn foot_bottom_offset_logical(&self) -> f64 {
        let Some((_x0, _y0, _x1, y1)) = self.composited.opaque_bbox() else {
            return 0.0;
        };
        let pad_px = ((self.composited.height as i32) - 1 - y1).max(0) as f64;
        pad_px / self.effective_display_scale()
    }

    fn trigger_bounce(&mut self, strength: f32) {
        self.bounce_at = Some(Instant::now());
        self.bounce_strength = strength.clamp(0.0, 0.045);
    }

    fn set_surface(
        &mut self,
        manifest: &mut ShellManifest,
        gpu: &GpuContext,
        requested_id: i32,
        reset_pos: bool,
    ) {
        self.is_neo_default = manifest.shell_id == "neo_default";
        self.display_scale = manifest.descript.display_scale.clamp(0.5, 4.0);
        let resolved = manifest.resolve_surface_id(self.scope, requested_id);
        self.bind_overlays = manifest.collect_bind_overlays(resolved, &self.active_binds);
        let base = manifest
            .compose_surface_rgba(resolved, &self.active_binds)
            .unwrap_or_else(|| RgbaBitmap::from_solid(180, 260, 120, 140, 200, 220));
        self.surface_id = resolved;
        self.base_bmp = Some(base);
        self.overlays.clear();
        self.active_run = None;
        self.lip_on = false;
        self.next_lip_at = Instant::now();
        self.rebuild_composited(gpu, reset_pos);
        self.schedule_seriko_timers(manifest);
        if !reset_pos {
            self.trigger_bounce(0.014);
        }
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
                        2_200 + (rand::random::<u64>() % 1_800)
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

        const LIP_OVERLAY_PID: i32 = 9001;
        if is_speaking_here {
            if now >= self.next_lip_at {
                self.lip_on = !self.lip_on;
                self.next_lip_at = now + Duration::from_millis(115);
                if self.lip_on {
                    let talk_pat = manifest.surfaces.get(&self.surface_id).and_then(|sdef| {
                        sdef.animations
                            .iter()
                            .find(|a| a.interval.starts_with("talk"))
                            .and_then(|a| {
                                a.patterns.iter().find(|p| {
                                    p.surface_id >= 0
                                        && matches!(
                                            p.method.as_str(),
                                            "overlay" | "overlayfast" | "overlayf" | "replace"
                                        )
                                })
                            })
                            .map(|p| (p.method.clone(), p.surface_id, p.offset_x, p.offset_y))
                    });
                    if let Some((method, sid, ox, oy)) = talk_pat {
                        if let Some(ov_bmp) = manifest.compose_base_rgba(sid) {
                            let replace = method == "replace";
                            self.overlays.insert(LIP_OVERLAY_PID, (ox, oy, replace, ov_bmp));
                            dirty = true;
                        }
                    }
                } else if self.overlays.remove(&LIP_OVERLAY_PID).is_some() {
                    dirty = true;
                }
            }
        } else {
            self.lip_on = false;
            self.next_lip_at = now;
            if self.overlays.remove(&LIP_OVERLAY_PID).is_some() {
                dirty = true;
            }
        }

        if let Some(mut run) = self.active_run.take() {
            if now >= run.next_step_at {
                if run.idx < run.steps.len() {
                    let step = run.steps[run.idx].clone();
                    match step.method.as_str() {
                        "overlay" | "overlayfast" | "overlayf" | "replace" => {
                            if step.surface_id < 0 {
                                self.overlays.remove(&step.aid);
                            } else if let Some(ov_bmp) = manifest.compose_base_rgba(step.surface_id) {
                                let replace = step.method == "replace";
                                self.overlays.insert(
                                    step.aid,
                                    (step.offset_x, step.offset_y, replace, ov_bmp),
                                );
                            }
                            dirty = true;
                        }
                        "base" => {
                            let target_sid = if step.surface_id >= 0 {
                                step.surface_id
                            } else {
                                self.surface_id
                            };
                            if let Some(new_base) = manifest.compose_surface_rgba(target_sid, &self.active_binds) {
                                self.base_bmp = Some(new_base);
                                dirty = true;
                            }
                        }
                        "alternativestart" => {
                            if run.steps.len() < 32 {
                                let mut rng = rand::thread_rng();
                                if let Some(&target_aid) = step.alt_ids.choose(&mut rng) {
                                    if let Some(sdef) = manifest.surfaces.get(&self.surface_id) {
                                        if let Some(sub_anim) =
                                            sdef.animations.iter().find(|a| a.aid == target_aid)
                                        {
                                            let extra: Vec<SerikoStep> = sub_anim
                                                .patterns
                                                .iter()
                                                .map(|p| SerikoStep {
                                                    aid: target_aid,
                                                    method: p.method.clone(),
                                                    surface_id: p.surface_id,
                                                    offset_x: p.offset_x,
                                                    offset_y: p.offset_y,
                                                    delay_ms: p.delay_ms.max(40),
                                                    alt_ids: p.alt_ids.clone(),
                                                })
                                                .collect();
                                            let insert_at = run.idx + 1;
                                            run.steps.splice(insert_at..insert_at, extra);
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    run.idx += 1;
                    if run.idx < run.steps.len() {
                        run.next_step_at = now + Duration::from_millis(step.delay_ms.max(40));
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
                                3_000 + (rand::random::<u64>() % 3_200)
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
                        let steps: Vec<SerikoStep> = anim
                            .patterns
                            .iter()
                            .map(|p| SerikoStep {
                                // Key all patterns of an animation by aid so pattern -1 cleanly clears the slot.
                                aid,
                                method: p.method.clone(),
                                surface_id: p.surface_id,
                                offset_x: p.offset_x,
                                offset_y: p.offset_y,
                                delay_ms: p.delay_ms.max(40),
                                alt_ids: p.alt_ids.clone(),
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
            if let Some((ox, oy, replace, ref ov)) = self.overlays.get(&k) {
                if *replace {
                    comp.replace_rect(ov, *ox, *oy);
                } else {
                    comp.alpha_composite(ov, *ox, *oy);
                }
            }
        }
        let ds = self.effective_display_scale();
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
        let h_f = comp.height.max(1) as f32;
        let w_f = comp.width.max(1) as f32;
        if let Some((x0, y0, x1, y1)) = comp.opaque_bbox() {
            self.top_v = ((y0 as f32) / h_f).clamp(0.0, 0.35);
            self.anchor_v = (((y1 as f32) + 1.0) / h_f).clamp(0.55, 0.995);
            self.center_u = (((x0 + x1) as f32 * 0.5) / w_f).clamp(0.35, 0.65);
        } else {
            self.top_v = 0.02;
            self.anchor_v = 0.96;
            self.center_u = 0.50;
        }
        self.aspect_hw = (h_f / w_f).clamp(0.5, 4.0);
        let phys = self.ws.window.inner_size();
        self.ws.resize(gpu, phys);
        self.ws.upload_rgba(gpu, &comp);
        self.composited = comp;
        self.sync_rig_with_current_surface(gpu);
        self.ws.window.request_redraw();
    }

    fn tick_live2d(
        &mut self,
        gpu: &GpuContext,
        enabled: bool,
        global_cursor: Option<(f64, f64)>,
        is_speaking: bool,
    ) {
        let was_speaking = self.was_speaking;
        if !was_speaking && is_speaking && enabled {
            self.trigger_bounce(0.016);
        }
        self.was_speaking = is_speaking;

        if !enabled {
            self.smooth_gaze = (0.0, 0.0);
            self.bounce_at = None;
            self.rig.reset();
            let mut need_redraw = self.ws.last_live2d_active;
            if let Some(ref mut rig) = self.rig_parts {
                rig.reset();
                self.ws.sync_part_rig(gpu, Some(rig));
                need_redraw = true;
            }
            if self.ws.last_live2d_active {
                self.ws.update_params(gpu, &ShaderParams::idle(gpu.premul));
            }
            if need_redraw {
                self.ws.window.request_redraw();
            }
            return;
        }

        let dt = self.rig.last_tick.elapsed().as_secs_f32().clamp(0.005, 0.05);
        self.rig.last_tick = Instant::now();

        let (breath_amp, gaze_scale) = self
            .rig_parts
            .as_ref()
            .and_then(|r| r.uv_rig.as_ref())
            .map(|u| {
                (
                    u.breath_amp.unwrap_or(1.0).clamp(0.2, 2.5),
                    u.gaze_scale.unwrap_or(1.0).clamp(0.2, 2.5),
                )
            })
            .unwrap_or((1.0, 1.0));

        let t = self.anim_start.elapsed().as_secs_f32() + self.phase_offset;
        // Smooth biological chest inhalation curve (0.0 .. 1.0 biased upward on inhale)
        let breath_raw = 0.5 - 0.5 * (t * 1.75).cos();
        let breath = breath_raw * (0.75 + 0.25 * (t * 0.62).sin()) * 0.92 * breath_amp;
        let body_sway = (t * 0.88).sin() * 0.55 + (t * 0.41).cos() * 0.28;

        let squish = if let Some(bt) = self.bounce_at {
            let b_dt = bt.elapsed().as_secs_f32();
            if b_dt < 0.72 {
                self.bounce_strength * (-b_dt * 6.0).exp() * (b_dt * 20.0).sin()
            } else {
                self.bounce_at = None;
                0.0
            }
        } else {
            0.0
        };

        // Target gaze angle (-1..1) from cursor relative to head pivot + gentle autonomous idle saccades
        let idle_x = (t * 0.47).sin() * 0.16 + (t * 0.21).cos() * 0.09;
        let idle_y = (t * 0.36 + 0.5).sin() * 0.12;
        let target_angle = if self.dragging {
            (0.0_f32, 0.0_f32)
        } else if let Some((gx, gy)) = global_cursor {
            if let Ok(outer) = self.ws.window.outer_position() {
                let scale = self.ws.window.scale_factor().max(1.0);
                #[cfg(target_os = "macos")]
                let (wx, wy) = {
                    let wl = outer.to_logical::<f64>(scale);
                    (wl.x, wl.y)
                };
                #[cfg(not(target_os = "macos"))]
                let (wx, wy) = (outer.x as f64 / scale, outer.y as f64 / scale);
                let head_cx = wx + (self.win_size.0 as f64) * (self.center_u as f64);
                let head_ratio = if self.scope == Scope::Sakura { 0.22 } else { 0.40 };
                let head_cy = wy + (self.win_size.1 as f64) * head_ratio;
                #[cfg(target_os = "macos")]
                let (cgx, cgy) = (gx, gy);
                #[cfg(not(target_os = "macos"))]
                let (cgx, cgy) = (gx / scale, gy / scale);
                let dx = cgx - head_cx;
                let dy = cgy - head_cy;
                let dist = (dx * dx + dy * dy).sqrt();
                let focus_mix = (1.0 - ((dist - 220.0) / 700.0).clamp(0.0, 0.85)) as f32;
                let cur_x = (dx / 380.0).clamp(-1.0, 1.0) as f32;
                let cur_y = (dy / 320.0).clamp(-1.0, 1.0) as f32;
                (
                    cur_x * focus_mix + idle_x * (1.0 - focus_mix),
                    cur_y * focus_mix + idle_y * (1.0 - focus_mix),
                )
            } else {
                (idle_x, idle_y)
            }
        } else {
            (idle_x, idle_y)
        };

        let follow = 1.0 - (-dt * 8.5).exp();
        self.rig.angle_x += (target_angle.0 * gaze_scale - self.rig.angle_x) * follow;
        self.rig.angle_y += (target_angle.1 * gaze_scale - self.rig.angle_y) * follow;
        self.smooth_gaze = (self.rig.angle_x, self.rig.angle_y);

        let vel_x = (self.rig.angle_x - self.rig.prev_angle_x) / dt;
        let vel_y = (self.rig.angle_y - self.rig.prev_angle_y) / dt;
        self.rig.prev_angle_x = self.rig.angle_x;
        self.rig.prev_angle_y = self.rig.angle_y;

        let talk_bob = if is_speaking {
            (t * 12.5).sin().abs() * 0.72
        } else {
            0.0
        };
        let target_z = self.rig.angle_x * 0.032
            + (t * 0.92 + self.phase_offset).sin() * 0.015
            + if is_speaking { (t * 6.4).sin() * 0.015 } else { 0.0 };
        self.rig.angle_z += (target_z - self.rig.angle_z) * (1.0 - (-dt * 9.0).exp());

        // Step 3 physical damped-spring pendulums driven by head velocity + idle breeze + speech impulse
        let breeze = (t * 1.35).sin() * 0.22 + (t * 2.2).cos() * 0.10;
        let head_lag_x = (-vel_x * 0.22 - body_sway * 0.18 + breeze).clamp(-1.6, 1.6);
        let head_lag_y = (-vel_y * 0.16 + breath * 0.22 + talk_bob * 0.55 + squish * 24.0).clamp(-1.6, 1.6);
        let ahoge_target = (-vel_x * 0.34 + (t * 1.9).sin() * 0.32 + if !was_speaking && is_speaking { 0.5 } else { 0.0 }).clamp(-1.8, 1.8);
        let cloth_target = (-body_sway * 0.42 - vel_x * 0.12 + (t * 1.05).sin() * 0.24).clamp(-1.5, 1.5);

        self.rig.hair_x.step(head_lag_x, 9.5, 0.36, dt);
        self.rig.hair_y.step(head_lag_y, 11.0, 0.38, dt);
        self.rig.ahoge.step(ahoge_target, 15.0, 0.24, dt);
        self.rig.cloth.step(cloth_target, 6.8, 0.44, dt);

        if let Some(ref mut rig) = self.rig_parts {
            rig.step(&Live2dMotionInputs {
                dt,
                time_s: t,
                breath,
                body_sway,
                squish,
                talk_bob,
                angle_x: self.rig.angle_x,
                angle_y: self.rig.angle_y,
                angle_z: self.rig.angle_z,
                vel_x,
                vel_y,
                speech_impulse: !was_speaking && is_speaking,
            });
            self.ws.sync_part_rig(gpu, Some(rig));
        }

        let rig_kind = if !self.is_neo_default {
            2
        } else if self.scope == Scope::Kero {
            1
        } else {
            0
        };
        let params = ShaderParams {
            premul: gpu.premul,
            live2d: 1,
            rig_kind,
            pad1: 0,
            breath,
            angle_x: self.rig.angle_x,
            angle_y: self.rig.angle_y,
            angle_z: self.rig.angle_z,
            hair_swing_x: self.rig.hair_x.pos,
            hair_swing_y: self.rig.hair_y.pos,
            ahoge_swing: self.rig.ahoge.pos,
            cloth_swing: self.rig.cloth.pos,
            body_sway,
            squish,
            talk_bob,
            top_v: self.top_v,
            anchor_v: self.anchor_v,
            center_u: self.center_u,
            aspect_hw: self.aspect_hw,
            pad2: 0.0,
            extra: [0.0; 4],
        };
        self.ws.update_params(gpu, &params);
        self.ws.window.request_redraw();
    }
}

#[derive(Clone, Debug)]
struct ChoiceItem {
    label: String,
    choice_id: String,
    is_url: bool,
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
    user_scrolled_up: bool,
    utterance_min_w: u32,
    win_size: (u32, u32),
    last_render_bmp: RgbaBitmap,
}

impl BalloonWindowState {
    fn render_bitmap(
        &mut self,
        skin: &BalloonSkin,
        speaker_name: &str,
        auto_follow_bottom: bool,
        text_renderer: &mut TextRenderer,
        scale: f32,
    ) -> ((u32, u32), RgbaBitmap) {
        let is_custom = skin.balloon_id != "clean_default";
        let font_pt = (skin.font_height.clamp(12, 16) as f32).max(13.0);
        let line_h_pt = font_pt * 1.44;
        let choice_h_pt = line_h_pt + 10.0;

        if !is_custom {
            let min_w_pt = if self.scope == Scope::Sakura { 216.0 } else { 202.0 };
            let max_w_pt = if self.scope == Scope::Sakura { 366.0 } else { 334.0 };
            let min_h_pt = 78.0_f32;
            let max_h_pt = 288.0_f32;

            let left_out = if self.side == "right" { 14.0_f32 } else { 6.0 };
            let right_out = if self.side == "left" { 14.0_f32 } else { 6.0 };
            let top_out = 5.0_f32;
            let bot_out = 8.0_f32;
            let pad_x = 16.0_f32;
            let pad_top = 12.0_f32;
            let pad_bot = 12.0_f32;
            let header_h = if speaker_name.trim().is_empty() { 0.0 } else { 23.0 };
            let status_h = if self.status_line.is_empty() { 0.0 } else { 18.0 };

            // 1. Adapt width smoothly from single-line content width (never shrinking mid-utterance)
            let mut widest_pt = if speaker_name.trim().is_empty() {
                0.0_f32
            } else {
                text_renderer.measure_inline_width(&format!("✦ {}", speaker_name.trim()), 11.0 * scale) / scale + 28.0
            };
            let display_str = if self.text.is_empty() { " " } else { self.text.as_str() };
            for raw_ln in display_str.lines() {
                let lw = text_renderer.measure_inline_width(raw_ln, font_pt * scale) / scale;
                widest_pt = widest_pt.max(lw);
            }
            for ch in &self.choices {
                let prefix = if ch.is_url { "🔗" } else { "▸" };
                let cw = text_renderer.measure_inline_width(&format!("{prefix} {}", ch.label), font_pt * scale) / scale + 24.0;
                widest_pt = widest_pt.max(cw);
            }
            let desired_w = (left_out + right_out + pad_x * 2.0 + widest_pt + 16.0)
                .ceil()
                .clamp(min_w_pt, max_w_pt) as u32;
            self.utterance_min_w = self.utterance_min_w.max(desired_w).min(max_w_pt as u32);
            let logical_w = self.utterance_min_w.max(desired_w);

            // 2. Wrap & measure total height at logical_w (reserve scroll gutter proactively if near cap)
            let font_px = font_pt * scale;
            let line_h_px = line_h_pt * scale;
            let inner_w_no_scroll = ((logical_w as f32) - left_out - right_out - pad_x * 2.0 - 6.0).max(80.0);
            let (_, est_text_h_px) =
                text_renderer.measure_lines(display_str, font_px, line_h_px, inner_w_no_scroll * scale);
            let choices_block_pt = if self.choices.is_empty() {
                0.0
            } else {
                8.0 + (self.choices.len() as f32) * (choice_h_pt + 5.0)
            };
            let est_total_pt = (est_text_h_px / scale) + choices_block_pt;
            let chrome_h_pt = top_out + bot_out + pad_top + header_h + status_h + pad_bot;
            let needs_scroll = (chrome_h_pt + est_total_pt) > max_h_pt;
            let scroll_gutter_pt = if needs_scroll { 12.0 } else { 0.0 };

            let usable_w_pt = ((logical_w as f32) - left_out - right_out - pad_x * 2.0 - scroll_gutter_pt).max(80.0);
            let usable_w = usable_w_pt * scale;
            let (_, text_h_px) = text_renderer.measure_lines(display_str, font_px, line_h_px, usable_w);
            let total_content_pt = (text_h_px / scale) + choices_block_pt;
            let logical_h = (chrome_h_pt + total_content_pt)
                .ceil()
                .clamp(min_h_pt, max_h_pt) as u32;

            let phys_w = ((logical_w as f32) * scale).round().max(1.0) as u32;
            let phys_h = ((logical_h as f32) * scale).round().max(1.0) as u32;
            let mut canvas = RgbaBitmap::new_transparent(phys_w, phys_h);

            let cx0 = left_out * scale;
            let cy0 = top_out * scale;
            let cx1 = (logical_w as f32 - right_out) * scale;
            let cy1 = (logical_h as f32 - bot_out) * scale;
            let border_w = 1.6 * scale;
            let (top_c, bot_c, br_l, br_r, badge_c) = if self.scope == Scope::Kero {
                (
                    [247, 250, 255, 246],
                    [229, 238, 255, 246],
                    [120, 134, 246, 235],
                    [92, 108, 242, 235],
                    [99, 102, 241, 255],
                )
            } else {
                (
                    [250, 253, 255, 247],
                    [234, 244, 255, 247],
                    [96, 146, 235, 235],
                    [59, 130, 246, 238],
                    [37, 99, 235, 255],
                )
            };

            canvas.draw_aa_rounded_box(
                (cx0, cy0, cx1, cy1),
                16.0 * scale,
                top_c,
                bot_c,
                border_w,
                br_l,
                br_r,
                5.5 * scale,
                2.0 * scale,
                [18, 35, 72, 68],
                185,
            );

            // Side pointer tail seamlessly integrated with glass gradient
            let tail_y = (cy0 + 34.0 * scale).min(cy1 - 18.0 * scale).max(cy0 + 18.0 * scale);
            let ty_tail = ((tail_y - cy0) / (cy1 - cy0).max(1.0)).clamp(0.0, 1.0);
            let tail_fill = [
                ((top_c[0] as f32) + (bot_c[0] as f32 - top_c[0] as f32) * ty_tail).round() as u8,
                ((top_c[1] as f32) + (bot_c[1] as f32 - top_c[1] as f32) * ty_tail).round() as u8,
                ((top_c[2] as f32) + (bot_c[2] as f32 - top_c[2] as f32) * ty_tail).round() as u8,
                top_c[3],
            ];
            let side_right_edge = self.side == "left";
            canvas.draw_aa_tail_pointer(
                (cx0, cy0, cx1, cy1),
                side_right_edge,
                tail_y,
                10.5 * scale,
                7.0 * scale,
                tail_fill,
                border_w,
                if side_right_edge { br_r } else { br_l },
            );

            // Speaker pill header & delicate gradient accent line
            let content_left = cx0 + pad_x * scale;
            if header_h > 0.0 {
                let bx = content_left;
                let by = cy0 + 8.5 * scale;
                canvas.draw_aa_circle(
                    bx + 4.5 * scale,
                    by + 7.0 * scale,
                    3.2 * scale,
                    [147, 197, 253, 255],
                    badge_c,
                );
                let sp_font = 11.0 * scale;
                text_renderer.draw_inline_text(
                    &mut canvas,
                    speaker_name.trim(),
                    (bx + 11.0 * scale).round() as i32,
                    (by + 0.5 * scale).round() as i32,
                    bx.round() as i32,
                    (cx1 - 24.0 * scale).round() as i32,
                    sp_font,
                    sp_font * 1.25,
                    (46, 82, 165),
                );
                let line_y = (cy0 + (pad_top + header_h - 6.0) * scale).round() as i32;
                let line_w = (cx1 - content_left - pad_x * scale).max(20.0) as i32;
                canvas.fill_rect_alpha(content_left.round() as i32, line_y, line_w, (1.0 * scale).round().max(1.0) as i32, 96, 146, 235, 65);
            }

            let content_top = cy0 + (pad_top + header_h) * scale;
            let content_bot = cy1 - (pad_bot + status_h) * scale;
            let usable_h = (content_bot - content_top).max(24.0 * scale);
            let total_content_px = total_content_pt * scale;
            self.max_scroll_px = (total_content_px - usable_h).max(0.0);
            if auto_follow_bottom && !self.user_scrolled_up && self.max_scroll_px > 0.5 {
                self.scroll_px = self.max_scroll_px;
            } else {
                self.scroll_px = self.scroll_px.clamp(0.0, self.max_scroll_px);
            }

            let drawn_h = text_renderer.draw_text(
                &mut canvas,
                &self.text,
                content_left.round() as i32,
                content_top.round() as i32,
                usable_w,
                Some(usable_h),
                font_px,
                line_h_px,
                skin.font_rgb,
                self.scroll_px,
            );

            let mut y_cursor = content_top + drawn_h + 7.0 * scale - self.scroll_px;
            for (idx, ch) in self.choices.iter_mut().enumerate() {
                let item_h = choice_h_pt * scale;
                if y_cursor + item_h >= content_top && y_cursor <= content_bot {
                    let hovered = self.hovered_choice == Some(idx);
                    let (ctop, cbot, cbr, txt_rgb) = if hovered {
                        (
                            [219, 234, 254, 240],
                            [191, 219, 254, 240],
                            [37, 99, 235, 225],
                            (24, 72, 185),
                        )
                    } else {
                        (
                            [240, 246, 255, 220],
                            [228, 239, 255, 220],
                            [147, 185, 245, 165],
                            (34, 92, 200),
                        )
                    };
                    let rx0 = content_left - 2.0 * scale;
                    let ry0 = y_cursor.max(content_top);
                    let rx1 = content_left + usable_w + 2.0 * scale;
                    let ry1 = (y_cursor + item_h).min(content_bot);
                    if ry1 - ry0 > 6.0 * scale {
                        canvas.draw_aa_rounded_box(
                            (rx0, ry0, rx1, ry1),
                            8.0 * scale,
                            ctop,
                            cbot,
                            1.1 * scale,
                            cbr,
                            cbr,
                            0.0,
                            0.0,
                            [0, 0, 0, 0],
                            90,
                        );
                        let prefix = if ch.is_url { "🔗" } else { "▸" };
                        text_renderer.draw_text(
                            &mut canvas,
                            &format!("{prefix} {}", ch.label),
                            (content_left + 8.0 * scale).round() as i32,
                            (y_cursor + 4.5 * scale).round() as i32,
                            (usable_w - 14.0 * scale).max(20.0),
                            Some((ry1 - ry0).max(12.0)),
                            font_px,
                            line_h_px,
                            txt_rgb,
                            0.0,
                        );
                    }
                    ch.rect = (
                        (rx0 / scale) as f64,
                        (ry0 / scale) as f64,
                        (rx1 / scale) as f64,
                        (ry1 / scale) as f64,
                    );
                } else {
                    ch.rect = (0.0, 0.0, 0.0, 0.0);
                }
                y_cursor += item_h + 5.0 * scale;
            }

            if !self.status_line.is_empty() {
                let st_font = (font_px * 0.82).max(10.0 * scale);
                text_renderer.draw_inline_text(
                    &mut canvas,
                    &self.status_line,
                    content_left.round() as i32,
                    (cy1 - (pad_bot + 14.0) * scale).round() as i32,
                    content_left.round() as i32,
                    (cx1 - 12.0 * scale).round() as i32,
                    st_font,
                    st_font * 1.2,
                    (96, 114, 146),
                );
            }

            if self.max_scroll_px > 1.0 {
                let track_x = (cx1 - 9.5 * scale).max(content_left + usable_w + 3.0 * scale);
                let track_w = 3.5 * scale;
                let track_top = content_top + 2.0 * scale;
                let track_h = (usable_h - 4.0 * scale).max(20.0 * scale);
                canvas.draw_aa_rounded_box(
                    (track_x, track_top, track_x + track_w, track_top + track_h),
                    track_w * 0.5,
                    [148, 163, 184, 55],
                    [148, 163, 184, 55],
                    0.0,
                    [0, 0, 0, 0],
                    [0, 0, 0, 0],
                    0.0,
                    0.0,
                    [0, 0, 0, 0],
                    0,
                );
                let ratio = (self.scroll_px / self.max_scroll_px).clamp(0.0, 1.0);
                let thumb_h = (track_h * (usable_h / (usable_h + self.max_scroll_px))).max(18.0 * scale);
                let thumb_y = track_top + ratio * (track_h - thumb_h);
                canvas.draw_aa_rounded_box(
                    (track_x, thumb_y, track_x + track_w, thumb_y + thumb_h),
                    track_w * 0.5,
                    [96, 165, 250, 215],
                    [37, 99, 235, 215],
                    0.0,
                    [0, 0, 0, 0],
                    [0, 0, 0, 0],
                    0.0,
                    0.0,
                    [0, 0, 0, 0],
                    0,
                );
            }

            return ((logical_w, logical_h), canvas);
        }

        // Custom `.nar` balloon skin: preserve bitmap art via 3-slice vertical stretching with dynamic height + scroll
        let (frame_raw, metrics) = skin.load_frame_rgba(self.scope, &self.side, self.large);
        let b_scale = skin.display_scale.clamp(0.5, 4.0) as f32;
        let base_w = ((frame_raw.width as f32) / b_scale).round().clamp(200.0, 420.0) as u32;
        let base_h = ((frame_raw.height as f32) / b_scale).round().clamp(84.0, 320.0);

        let left_pt = metrics.origin_x.max(12) as f32;
        let top_pt = metrics.origin_y.max(10) as f32;
        let right_pt = if metrics.wordwrappoint_x < 0 {
            (base_w as i32 + metrics.wordwrappoint_x).max(80) as f32
        } else {
            metrics.wordwrappoint_x.max(80) as f32
        };
        let bot_margin_pt = if metrics.validrect_bottom < 0 {
            (-metrics.validrect_bottom).max(12) as f32
        } else {
            (base_h - metrics.validrect_bottom as f32).max(12.0)
        };
        let usable_w_pt = (right_pt - left_pt).max(80.0);
        let status_reserve_pt = if self.status_line.is_empty() { 0.0 } else { 18.0 };

        let font_px = font_pt * scale;
        let line_h = line_h_pt * scale;
        let (_, text_h_px) = text_renderer.measure_lines(
            if self.text.is_empty() { " " } else { &self.text },
            font_px,
            line_h,
            usable_w_pt * scale,
        );
        let choice_h_total_pt = if self.choices.is_empty() {
            0.0
        } else {
            8.0 + (self.choices.len() as f32) * (choice_h_pt + 2.0)
        };
        let needed_h_pt = (top_pt + (text_h_px / scale) + choice_h_total_pt + status_reserve_pt + bot_margin_pt)
            .ceil()
            .clamp(84.0, 292.0);
        let logical_w = base_w;
        let logical_h = needed_h_pt as u32;

        let phys_w = ((logical_w as f32) * scale).round().max(1.0) as u32;
        let phys_h = ((logical_h as f32) * scale).round().max(1.0) as u32;
        let mut canvas = frame_raw.resized_three_slice_v(phys_w, phys_h, 0.24);

        let left = left_pt * scale;
        let top = top_pt * scale;
        let usable_w = usable_w_pt * scale;
        let bot = ((logical_h as f32) - bot_margin_pt) * scale;
        let status_reserve = status_reserve_pt * scale;
        let usable_h = (bot - top - status_reserve).max(32.0);

        let total_content_h = text_h_px + choice_h_total_pt * scale;
        self.max_scroll_px = (total_content_h - usable_h).max(0.0);
        if auto_follow_bottom && !self.user_scrolled_up && self.max_scroll_px > 0.5 {
            self.scroll_px = self.max_scroll_px;
        } else {
            self.scroll_px = self.scroll_px.clamp(0.0, self.max_scroll_px);
        }

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
                let prefix = if ch.is_url { "🔗" } else { "▸" };
                text_renderer.draw_text(
                    &mut canvas,
                    &format!("{prefix} {}", ch.label),
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
            let track_x = (left + usable_w + 4.0 * scale).min((phys_w as f32) - 10.0 * scale).round() as i32;
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
                55,
            );
            canvas.fill_rect_alpha(
                track_x,
                thumb_y,
                (3.0 * scale).round().max(2.0) as i32,
                thumb_h.round() as i32,
                59,
                130,
                246,
                170,
            );
        }

        ((logical_w, logical_h), canvas)
    }

    fn redraw_content(
        &mut self,
        skin: &BalloonSkin,
        speaker_name: &str,
        auto_follow_bottom: bool,
        text_renderer: &mut TextRenderer,
        gpu: &GpuContext,
    ) -> bool {
        let scale = self.ws.window.scale_factor().clamp(1.0, 3.0) as f32;
        let ((logical_w, logical_h), canvas) =
            self.render_bitmap(skin, speaker_name, auto_follow_bottom, text_renderer, scale);
        let size_changed = self.win_size != (logical_w, logical_h);
        if size_changed {
            let _ = self
                .ws
                .window
                .request_inner_size(LogicalSize::new(logical_w, logical_h));
            self.win_size = (logical_w, logical_h);
        }
        let phys = PhysicalSize::new(canvas.width.max(1), canvas.height.max(1));
        self.ws.resize(gpu, phys);
        self.ws.upload_rgba(gpu, &canvas);
        self.last_render_bmp = canvas;
        self.ws.window.request_redraw();
        size_changed
    }
}

struct CommunicateWindowState {
    ws: WindowSurface,
    visible: bool,
    focused: bool,
    text: String,
    ime_preedit: String,
    win_size: (u32, u32),
    cursor_on: bool,
    last_blink: Instant,
    anim_start: Instant,
    hovered_close: bool,
    hovered_send: bool,
    hovered_attach: bool,
    last_local_cursor: Option<(f64, f64)>,
    close_rect: (f64, f64, f64, f64),
    send_rect: (f64, f64, f64, f64),
    attach_rect: (f64, f64, f64, f64),
    attached_image: Option<crate::agent_engine::ImageAttachment>,
    last_render_bmp: RgbaBitmap,
}

impl CommunicateWindowState {
    fn render_bitmap(
        &mut self,
        text_renderer: &mut TextRenderer,
        scale: f32,
        phase_s: f32,
        ui_lang: crate::i18n::UiLang,
    ) -> RgbaBitmap {
        let (logical_w, logical_h) = self.win_size;
        let pw = ((logical_w as f32) * scale).round().max(1.0) as u32;
        let ph = ((logical_h as f32) * scale).round().max(1.0) as u32;
        let mut canvas = RgbaBitmap::new_transparent(pw, ph);

        // Generous 10pt transparent glow margin around pill so shadow/aura never touches window edges
        let px0 = 10.0 * scale;
        let py0 = 10.0 * scale;
        let px1 = (logical_w as f32 - 10.0) * scale;
        let py1 = (logical_h as f32 - 10.0) * scale;

        let pulse = 0.5 + 0.5 * (phase_s * 2.8).sin();
        let shift = 0.5 + 0.5 * (phase_s * 1.4).sin();
        let aura_alpha = if self.focused {
            (72.0 + 48.0 * pulse).round() as u8
        } else {
            40
        };
        let br_left = [
            (56.0 + 38.0 * (1.0 - shift)).round() as u8,
            (130.0 + 60.0 * shift).round() as u8,
            248,
            248,
        ];
        let br_right = [
            (37.0 + 58.0 * shift).round() as u8,
            (99.0 + 36.0 * (1.0 - shift)).round() as u8,
            246,
            248,
        ];

        canvas.draw_aa_rounded_box(
            (px0, py0, px1, py1),
            22.0 * scale,
            [14, 24, 52, 242],
            [21, 36, 74, 246],
            if self.focused { 2.1 * scale } else { 1.4 * scale },
            br_left,
            br_right,
            if self.focused { 8.5 * scale } else { 5.0 * scale },
            0.0,
            [56, 189, 248, aura_alpha],
            105,
        );

        // Generously padded header inside pill localized by UI language (shows attached image chip when present)
        let ui_str = crate::i18n::ui_strings(ui_lang);
        let hdr_font = 10.5 * scale;
        let (hdr_text, hdr_col) = if let Some(ref img) = self.attached_image {
            let short_name: String = img.file_name.chars().take(24).collect();
            (format!("🖼 {short_name} ({}x{})", img.width, img.height), (110, 231, 183))
        } else {
            (ui_str.comm_header.to_string(), (147, 197, 253))
        };
        text_renderer.draw_inline_text(
            &mut canvas,
            &hdr_text,
            (28.0 * scale).round() as i32,
            (17.5 * scale).round() as i32,
            (24.0 * scale).round() as i32,
            (315.0 * scale).round() as i32,
            hdr_font,
            hdr_font * 1.25,
            hdr_col,
        );
        let hint_font = 9.5 * scale;
        text_renderer.draw_inline_text(
            &mut canvas,
            "Enter · Esc",
            (328.0 * scale).round() as i32,
            (18.5 * scale).round() as i32,
            (318.0 * scale).round() as i32,
            (398.0 * scale).round() as i32,
            hint_font,
            hint_font * 1.25,
            (125, 152, 198),
        );

        // Top-right Close (×) button inside pill header
        let cl_cx = (logical_w as f32 - 33.0) * scale;
        let cl_cy = 24.5 * scale;
        let cl_r = 8.8 * scale;
        self.close_rect = (
            (cl_cx / scale - 13.0) as f64,
            (cl_cy / scale - 13.0) as f64,
            (cl_cx / scale + 13.0) as f64,
            (cl_cy / scale + 13.0) as f64,
        );
        let (cl_in, cl_out) = if self.hovered_close {
            ([239, 68, 68, 235], [185, 28, 28, 235])
        } else {
            ([44, 68, 122, 220], [30, 48, 92, 220])
        };
        canvas.draw_aa_circle(cl_cx, cl_cy, cl_r, cl_in, cl_out);
        let d_x = 3.2 * scale;
        canvas.draw_aa_segment(cl_cx - d_x, cl_cy - d_x, cl_cx + d_x, cl_cy + d_x, 1.7 * scale, [241, 245, 255, 245]);
        canvas.draw_aa_segment(cl_cx - d_x, cl_cy + d_x, cl_cx + d_x, cl_cy - d_x, 1.7 * scale, [241, 245, 255, 245]);

        // Inner dark sapphire editable input well + active glowing AI orb + Attach Image (🖼) + Send (↑)
        let well_x0 = 22.0 * scale;
        let well_y0 = 38.0 * scale;
        let well_x1 = (logical_w as f32 - 86.0) * scale;
        let well_y1 = 72.0 * scale;
        canvas.draw_aa_rounded_box(
            (well_x0, well_y0, well_x1, well_y1),
            12.0 * scale,
            [9, 16, 36, 220],
            [12, 22, 46, 225],
            1.1 * scale,
            [59, 130, 246, if self.focused { 130 } else { 65 }],
            [56, 189, 248, if self.focused { 130 } else { 65 }],
            0.0,
            0.0,
            [0, 0, 0, 0],
            20,
        );

        // Pulsing Glowing Blue AI Orb at left of input well
        let orb_cx = 39.0 * scale;
        let orb_cy = 55.0 * scale;
        let orb_halo = (7.2 + 1.4 * pulse) * scale;
        canvas.draw_aa_circle(orb_cx, orb_cy, orb_halo, [56, 189, 248, 85], [37, 99, 235, 0]);
        canvas.draw_aa_circle(orb_cx, orb_cy, 4.8 * scale, [186, 230, 253, 255], [37, 99, 235, 255]);

        // Attach Image (🖼) button right next to Send button
        let at_cx = (logical_w as f32 - 64.0) * scale;
        let at_cy = 55.0 * scale;
        let at_r = 12.2 * scale;
        self.attach_rect = (
            (at_cx / scale - 13.5) as f64,
            (at_cy / scale - 13.5) as f64,
            (at_cx / scale + 13.5) as f64,
            (at_cy / scale + 13.5) as f64,
        );
        let (at_in, at_out) = if self.attached_image.is_some() {
            if self.hovered_attach {
                ([52, 211, 153, 255], [16, 185, 129, 255])
            } else {
                ([16, 185, 129, 235], [5, 150, 105, 235])
            }
        } else if self.hovered_attach {
            ([59, 130, 246, 235], [37, 99, 235, 235])
        } else {
            ([34, 54, 102, 195], [24, 38, 76, 195])
        };
        canvas.draw_aa_circle(at_cx, at_cy, at_r, at_in, at_out);
        let icon_lbl = if self.attached_image.is_some() { "✓" } else { "🖼" };
        let icon_f = 11.0 * scale;
        let icon_w = text_renderer.measure_inline_width(icon_lbl, icon_f);
        text_renderer.draw_inline_text(
            &mut canvas,
            icon_lbl,
            (at_cx - icon_w * 0.5).round() as i32,
            (at_cy - icon_f * 0.62).round() as i32,
            (at_cx - at_r).round() as i32,
            (at_cx + at_r).round() as i32,
            icon_f,
            icon_f * 1.2,
            (248, 250, 252),
        );

        // Right Send (↑) button
        let sd_cx = (logical_w as f32 - 33.0) * scale;
        let sd_cy = 55.0 * scale;
        let sd_r = 13.0 * scale;
        self.send_rect = (
            (sd_cx / scale - 15.0) as f64,
            (sd_cy / scale - 15.0) as f64,
            (sd_cx / scale + 15.0) as f64,
            (sd_cy / scale + 15.0) as f64,
        );
        let has_input =
            !self.text.trim().is_empty() || !self.ime_preedit.is_empty() || self.attached_image.is_some();
        let (sd_in, sd_out) = if has_input {
            if self.hovered_send {
                ([96, 165, 250, 255], [37, 99, 235, 255])
            } else {
                ([59, 130, 246, 245], [29, 78, 216, 245])
            }
        } else if self.hovered_send {
            ([48, 76, 138, 210], [32, 52, 98, 210])
        } else {
            ([34, 54, 102, 185], [24, 38, 76, 185])
        };
        canvas.draw_aa_circle(sd_cx, sd_cy, sd_r, sd_in, sd_out);
        let arrow_col = if has_input { [255, 255, 255, 255] } else { [148, 174, 214, 200] };
        canvas.draw_aa_segment(sd_cx, sd_cy + 4.5 * scale, sd_cx, sd_cy - 4.5 * scale, 1.9 * scale, arrow_col);
        canvas.draw_aa_segment(sd_cx, sd_cy - 4.8 * scale, sd_cx - 4.0 * scale, sd_cy - 0.8 * scale, 1.9 * scale, arrow_col);
        canvas.draw_aa_segment(sd_cx, sd_cy - 4.8 * scale, sd_cx + 4.0 * scale, sd_cy - 0.8 * scale, 1.9 * scale, arrow_col);

        // Single-line input text, Siri-style blue IME underline, and unmistakable blinking caret
        let clip_x0 = (52.0 * scale).round() as i32;
        let clip_x1 = (well_x1 - 8.0 * scale).round() as i32;
        let text_y = (45.0 * scale).round() as i32;
        let input_font = 13.8 * scale;
        let input_line_h = input_font * 1.28;

        let w_text = text_renderer.measure_inline_width(&self.text, input_font);
        let w_ime = text_renderer.measure_inline_width(&self.ime_preedit, input_font);
        let total_w = w_text + w_ime;
        let vis_w = ((clip_x1 - clip_x0) as f32 - 8.0 * scale).max(40.0);
        let h_scroll = (total_w - vis_w).max(0.0);
        let text_x = (clip_x0 as f32) + 2.0 * scale - h_scroll;

        if self.text.is_empty() && self.ime_preedit.is_empty() {
            let ph_x = ((clip_x0 as f32) + 7.5 * scale).round() as i32;
            text_renderer.draw_inline_text(
                &mut canvas,
                ui_str.comm_placeholder,
                ph_x,
                text_y,
                clip_x0,
                clip_x1,
                input_font,
                input_line_h,
                (148, 174, 214),
            );
        } else {
            text_renderer.draw_inline_text(
                &mut canvas,
                &self.text,
                text_x.round() as i32,
                text_y,
                clip_x0,
                clip_x1,
                input_font,
                input_line_h,
                (245, 249, 255),
            );
            if !self.ime_preedit.is_empty() {
                let ime_x = text_x + w_text;
                text_renderer.draw_inline_text(
                    &mut canvas,
                    &self.ime_preedit,
                    ime_x.round() as i32,
                    text_y,
                    clip_x0,
                    clip_x1,
                    input_font,
                    input_line_h,
                    (255, 255, 255),
                );
                let ul_x0 = ime_x.max(clip_x0 as f32);
                let ul_x1 = (ime_x + w_ime).min(clip_x1 as f32);
                if ul_x1 > ul_x0 + 1.0 {
                    let ul_y = (text_y as f32) + 18.8 * scale;
                    canvas.draw_aa_segment(ul_x0, ul_y, ul_x1, ul_y, 2.2 * scale, [56, 189, 248, 255]);
                }
            }
        }

        // Always render blinking electric-blue vertical caret (even when empty!) so focus/input readiness is unmistakable
        if self.cursor_on {
            let caret_x = if self.text.is_empty() && self.ime_preedit.is_empty() {
                (clip_x0 as f32) + 2.0 * scale
            } else {
                (text_x + total_w + 1.5 * scale).min((clip_x1 as f32) - 2.5 * scale)
            };
            let cx_i = caret_x.round() as i32;
            let cy_i = (well_y0 + 7.0 * scale).round() as i32;
            let ch_i = (19.5 * scale).round() as i32;
            let cw_i = (2.2 * scale).round().max(2.0) as i32;
            canvas.fill_rect_alpha(cx_i - 2, cy_i - 1, cw_i + 4, ch_i + 2, 56, 189, 248, 90);
            canvas.fill_rect_alpha(cx_i, cy_i, cw_i, ch_i, 96, 165, 250, 255);
        }

        canvas
    }

    fn redraw(
        &mut self,
        _skin: &BalloonSkin,
        text_renderer: &mut TextRenderer,
        gpu: &GpuContext,
        ui_lang: crate::i18n::UiLang,
    ) {
        let scale = self.ws.window.scale_factor().clamp(1.0, 3.0) as f32;
        let phase_s = self.anim_start.elapsed().as_secs_f32();
        let canvas = self.render_bitmap(text_renderer, scale, phase_s, ui_lang);
        let phys = PhysicalSize::new(canvas.width.max(1), canvas.height.max(1));
        self.ws.resize(gpu, phys);
        self.ws.upload_rgba(gpu, &canvas);
        self.last_render_bmp = canvas;
        self.ws.window.request_redraw();
    }
}

const TTS_ENGINE_CHOICES: &[(&str, &str)] = &[
    ("auto", "自動 (AivisSpeech→VOICEVOX→OS)"),
    ("aivisspeech", "AivisSpeech (port 10101)"),
    ("voicevox", "VOICEVOX (port 50021)"),
    ("os", "OS 標準音声 (Native TTS)"),
];

const KERO_ARCHETYPE_CHOICES: &[(&str, &str)] = &[
    ("boy", "少年/中性 (うにゅう標準)"),
    ("girl", "女の子相棒 (女性パートナ)"),
    ("male", "青年/男性 (渋めパートナ)"),
];

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum MenuSection {
    Skins,
    Voice,
    Ai,
    Tools,
}

#[derive(Clone, Debug)]
enum MenuCommand {
    OpenCommunicate,
    OpenSettings,
    OpenSettingsTab(usize),
    AskClipboard,
    ShowQuickMemos,
    ShowCalendarAgenda,
    ResetDefaultSkin,
    ToggleMenuSection(MenuSection),
    OpenConfigFolder,
    ToggleClickThrough,
    SwitchShell(String),
    SwitchBalloon(String),
    SwitchProvider(String, String),
    SetupLocalAi,
    ToggleKero,
    ToggleAlwaysOnTop,
    ToggleLive2dMotion,
    OpenLive2dStudio,
    OpenLive2dFolder,
    ToggleBind(i32),
    ToggleHourlyChime,
    CycleAutoTalk,
    CycleUiLanguage,
    ToggleQuietMode,
    ToggleHideCharacters,
    TogglePomodoro,
    StopPomodoro,
    SkipPomodoro,
    ShowBacklog,
    CopyLastReply,
    MoveNextMonitor,
    ToggleVoiceTts,
    TestVoice(u8),
    CycleSakuraVoice,
    CycleKeroVoice,
    SelectTtsEngine(String),
    SelectKeroVoiceArchetype(Option<String>),
    SelectVoiceSpeaker { scope: u8, style_id: Option<i64> },
    InstallAivmModel { uuid: String, name: String },
    CheckUpdates,
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
    scroll_accum: f64,
    win_size: (u32, u32),
    row_h: f64,
    max_visible_rows: usize,
    last_render_bmp: RgbaBitmap,
}

impl MenuWindowState {
    fn render_bitmap(&self, text_renderer: &mut TextRenderer, scale: f32) -> ((u32, u32), RgbaBitmap) {
        let total = self.rows.len();
        let vis_count = total.min(self.max_visible_rows).max(1);
        let logical_w = 320u32;
        let logical_h = ((vis_count as f64) * self.row_h + 16.0).ceil() as u32;
        let pw = ((logical_w as f32) * scale).round().max(1.0) as u32;
        let ph = ((logical_h as f32) * scale).round().max(1.0) as u32;

        let mut canvas = RgbaBitmap::new_transparent(pw, ph);
        let pad = 2.0 * scale;
        canvas.draw_aa_rounded_box(
            (pad, pad, pw as f32 - pad, ph as f32 - pad),
            12.0 * scale,
            [20, 27, 44, 248],
            [13, 18, 32, 250],
            1.4 * scale,
            [96, 146, 235, 195],
            [59, 130, 246, 195],
            3.0 * scale,
            1.0 * scale,
            [8, 12, 24, 80],
            55,
        );

        let has_scroll = total > vis_count;
        let right_reserve = if has_scroll { 16.0 * scale } else { 8.0 * scale };
        let font_px = 12.4 * scale;
        let row_px = (self.row_h as f32) * scale;
        let max_scroll = total.saturating_sub(vis_count);
        let start = self.scroll_row.min(max_scroll);
        let end = (start + vis_count).min(total);

        for (v_idx, r_idx) in (start..end).enumerate() {
            let y = (8.0 * scale) + (v_idx as f32) * row_px;
            let row = &self.rows[r_idx];
            if matches!(row.cmd, MenuCommand::Separator) {
                if row.label.is_empty() {
                    canvas.fill_rect_alpha(
                        (12.0 * scale) as i32,
                        (y + row_px * 0.5) as i32,
                        (pw as f32 - 12.0 * scale - right_reserve).max(20.0) as i32,
                        (1.0 * scale).round().max(1.0) as i32,
                        100,
                        125,
                        175,
                        120,
                    );
                } else {
                    text_renderer.draw_inline_text(
                        &mut canvas,
                        &row.label,
                        (12.0 * scale) as i32,
                        (y + 5.0 * scale) as i32,
                        (10.0 * scale) as i32,
                        (pw as f32 - right_reserve) as i32,
                        font_px * 0.88,
                        font_px * 1.15,
                        (125, 185, 255),
                    );
                }
                continue;
            }
            if self.hovered_idx == Some(r_idx) {
                canvas.draw_aa_rounded_box(
                    (7.0 * scale, y + 1.5 * scale, pw as f32 - right_reserve - 1.0 * scale, y + row_px - 1.5 * scale),
                    6.5 * scale,
                    [62, 110, 218, 232],
                    [37, 86, 200, 232],
                    0.8 * scale,
                    [147, 197, 253, 190],
                    [96, 165, 250, 190],
                    0.0,
                    0.0,
                    [0, 0, 0, 0],
                    45,
                );
            } else if matches!(row.cmd, MenuCommand::ToggleMenuSection(_)) {
                let is_open = row.label.starts_with('▾');
                let (fa, fb, brd) = if is_open {
                    ([36, 62, 114, 210], [28, 48, 92, 210], [116, 174, 248, 150])
                } else {
                    ([28, 40, 68, 180], [22, 32, 56, 180], [78, 116, 186, 100])
                };
                canvas.draw_aa_rounded_box(
                    (7.0 * scale, y + 1.5 * scale, pw as f32 - right_reserve - 1.0 * scale, y + row_px - 1.5 * scale),
                    6.5 * scale,
                    fa,
                    fb,
                    0.8 * scale,
                    brd,
                    brd,
                    0.0,
                    0.0,
                    [0, 0, 0, 0],
                    0,
                );
            }
            let rgb = match row.cmd {
                MenuCommand::ResetDefaultSkin
                | MenuCommand::OpenSettings
                | MenuCommand::OpenSettingsTab(_) => (147, 215, 255),
                MenuCommand::ToggleMenuSection(_) => (206, 232, 255),
                _ => (240, 244, 252),
            };
            text_renderer.draw_inline_text(
                &mut canvas,
                &row.label,
                (13.0 * scale) as i32,
                (y + 4.5 * scale) as i32,
                (11.0 * scale) as i32,
                (pw as f32 - right_reserve - 4.0 * scale) as i32,
                font_px,
                font_px * 1.25,
                rgb,
            );
        }

        if has_scroll && max_scroll > 0 {
            let track_x0 = pw as f32 - 11.0 * scale;
            let track_w = 4.0 * scale;
            let track_y0 = 10.0 * scale;
            let track_h = (ph as f32 - 20.0 * scale).max(20.0 * scale);
            canvas.draw_aa_rounded_box(
                (track_x0, track_y0, track_x0 + track_w, track_y0 + track_h),
                track_w * 0.5,
                [148, 163, 184, 55],
                [148, 163, 184, 55],
                0.0,
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            let frac_vis = (vis_count as f32) / (total as f32);
            let thumb_h = (track_h * frac_vis).max(22.0 * scale);
            let frac_pos = ((start as f32) / (max_scroll as f32)).clamp(0.0, 1.0);
            let thumb_y0 = track_y0 + frac_pos * (track_h - thumb_h);
            canvas.draw_aa_rounded_box(
                (track_x0, thumb_y0, track_x0 + track_w, thumb_y0 + thumb_h),
                track_w * 0.5,
                [125, 185, 255, 225],
                [59, 130, 246, 225],
                0.0,
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
        }

        ((logical_w, logical_h), canvas)
    }

    fn redraw(&mut self, text_renderer: &mut TextRenderer, gpu: &GpuContext) {
        let scale = self.ws.window.scale_factor().clamp(1.0, 3.0) as f32;
        let ((logical_w, logical_h), canvas) = self.render_bitmap(text_renderer, scale);
        if self.win_size != (logical_w, logical_h) {
            let _ = self
                .ws
                .window
                .request_inner_size(LogicalSize::new(logical_w, logical_h));
            self.win_size = (logical_w, logical_h);
        }
        let phys = PhysicalSize::new(canvas.width.max(1), canvas.height.max(1));
        self.ws.resize(gpu, phys);
        self.ws.upload_rgba(gpu, &canvas);
        self.last_render_bmp = canvas;
        self.ws.window.request_redraw();
    }
}

struct SettingsWindowState {
    ws: WindowSurface,
    visible: bool,
    win_size: (u32, u32),
    model: SettingsPanelModel,
    dragging_header: bool,
    drag_start_cursor: (f64, f64),
    drag_start_pos: (f64, f64),
    last_local_cursor: Option<(f64, f64)>,
    last_render_bmp: RgbaBitmap,
}

impl SettingsWindowState {
    fn redraw(&mut self, data: &SettingsPanelData, text_renderer: &mut TextRenderer, gpu: &GpuContext) {
        let scale = self.ws.window.scale_factor().clamp(1.0, 3.0) as f32;
        let canvas = self.model.render(data, text_renderer, scale);
        let phys = PhysicalSize::new(canvas.width.max(1), canvas.height.max(1));
        self.ws.resize(gpu, phys);
        self.ws.upload_rgba(gpu, &canvas);
        self.last_render_bmp = canvas;
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
    last_char_spoken: Option<(Scope, Instant)>,
    auto_hide_at: Option<Instant>,
    quick_mode: bool,
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
            last_char_spoken: None,
            auto_hide_at: None,
            quick_mode: false,
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

    fn is_speaking(&self, scope: Scope) -> bool {
        self.last_char_spoken
            .map_or(false, |(sc, t)| sc == scope && t.elapsed() < Duration::from_millis(240))
    }

    fn enqueue(&mut self, actions: Vec<DialogueAction>) {
        self.steps.clear();
        self.paused_until = None;
        self.auto_hide_at = None;
        self.quick_mode = false;
        self.active_speaker = Scope::Sakura;
        self.last_char_spoken = None;
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
                    ActionKind::Choice | ActionKind::Link => {
                        let is_url = act.kind == ActionKind::Link;
                        let item = ChoiceItem {
                            label: act.label,
                            choice_id: act.choice_id,
                            is_url,
                            rect: (0.0, 0.0, 0.0, 0.0),
                        };
                        if self.active_speaker == Scope::Kero {
                            if !is_url || !self.kero_choices.iter().any(|c| c.choice_id == item.choice_id) {
                                self.kero_choices.push(item);
                            }
                            self.kero_show = true;
                            self.kero_dirty = true;
                        } else {
                            if !is_url || !self.sakura_choices.iter().any(|c| c.choice_id == item.choice_id) {
                                self.sakura_choices.push(item);
                            }
                            self.sakura_show = true;
                            self.sakura_dirty = true;
                        }
                    }
                    ActionKind::QuickToggle => {
                        self.quick_mode = !self.quick_mode;
                    }
                    ActionKind::Pause => {}
                    ActionKind::Surface | ActionKind::BalloonSize => imm_actions.push(act),
                    ActionKind::Text => {}
                },
            }
        }
        self.paused_until = None;
        self.last_char_spoken = None;
        if self.sakura_choices.is_empty() && self.kero_choices.is_empty() {
            self.auto_hide_at = Some(Instant::now() + Duration::from_secs(18));
        }
        imm_actions
    }
}

#[derive(Debug)]
enum UserEvent {
    PlayActions(Vec<DialogueAction>),
    AutoTalkActions(Vec<DialogueAction>),
    PlayActionsNoBacklog(Vec<DialogueAction>),
    StatusUpdate(String, bool),
    AivmActionDone {
        status: String,
    },
    CalendarSyncDone {
        status: String,
    },
    ServerGetHealth(mpsc::Sender<Value>),
    ServerGetSkins(mpsc::Sender<Value>),
    ServerGetVoices(mpsc::Sender<Value>),
    ServerGetBacklog(mpsc::Sender<Value>),
    ServerConfigureVoice {
        req: crate::sstp_server::VoiceConfigReq,
        reply: mpsc::Sender<Result<Value, String>>,
    },
    ServerGetSettings(mpsc::Sender<Value>),
    ServerUpdateSettings {
        req: SettingsPatchReq,
        reply: mpsc::Sender<Result<Value, String>>,
    },
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
    RefreshMenus,
    SystemProbeUpdate {
        fullscreen: bool,
        battery: crate::accessory_utils::BatteryStatus,
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
            if let Ok(v) = rx.recv_timeout(Duration::from_millis(800)) {
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
            if let Ok(v) = rx.recv_timeout(Duration::from_millis(800)) {
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
        let _ = self.proxy.send_event(UserEvent::RefreshMenus);
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

    fn get_voices(&self) -> Value {
        let (tx, rx) = mpsc::channel();
        if self.proxy.send_event(UserEvent::ServerGetVoices(tx)).is_ok() {
            if let Ok(v) = rx.recv_timeout(Duration::from_millis(800)) {
                return v;
            }
        }
        let cfg = self.config_store.get();
        let (eng_lbl, voices) = crate::companion_extras::cached_tts_voices();
        json!({
            "active_shell": cfg.active_shell_id,
            "voice_tts": cfg.voice_tts,
            "tts_engine": cfg.tts_engine,
            "engine_label": eng_lbl,
            "voices": voices.into_iter().map(|v| json!({
                "style_id": v.style_id,
                "speaker_name": v.speaker_name,
                "style_name": v.style_name,
                "label": v.label,
            })).collect::<Vec<_>>(),
        })
    }

    fn on_configure_voice(
        &self,
        req: crate::sstp_server::VoiceConfigReq,
    ) -> Result<Value, String> {
        if let Some(ref eng) = req.tts_engine {
            let norm = eng.trim().to_ascii_lowercase();
            if !matches!(
                norm.as_str(),
                "auto" | "aivisspeech" | "aivis" | "voicevox" | "vv" | "coeiroink" | "os" | "native"
            ) {
                return Err(format!("Invalid tts_engine: {eng}"));
            }
        }
        if let Some(ref arch) = req.kero_voice_archetype {
            let norm = arch.trim().to_ascii_lowercase();
            if !norm.is_empty()
                && norm != "auto"
                && norm != "default"
                && crate::config_store::normalize_kero_archetype_str(arch).is_none()
            {
                return Err(format!("Invalid kero_voice_archetype: {arch}"));
            }
        }
        let install_uuid = req
            .install_aivm_uuid
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if let Some(ref uuid) = install_uuid {
            if !crate::companion_extras::is_valid_aivm_uuid(uuid) {
                return Err(format!("Invalid AIVM UUID format: {uuid}"));
            }
        }
        let toggled_off = req.voice_tts == Some(false);
        let switched_to_os = req
            .tts_engine
            .as_deref()
            .is_some_and(|e| crate::config_store::normalize_tts_engine(e) == "os");
        let uninstall_uuid = req
            .uninstall_aivm_uuid
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        if let Some(ref uuid) = uninstall_uuid {
            if !crate::companion_extras::is_valid_aivm_uuid(uuid) {
                return Err(format!("Invalid AIVM UUID format: {uuid}"));
            }
        }
        let need_engine_refresh =
            req.tts_engine.is_some() || req.voice_tts == Some(true) || install_uuid.is_some();

        let (tx, rx) = mpsc::channel();
        self.proxy
            .send_event(UserEvent::ServerConfigureVoice { req, reply: tx })
            .map_err(|e| format!("GUI イベント送信失敗: {e}"))?;
        let _ = rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|e| format!("音声設定タイムアウト: {e}"))??;

        let cfg = self.config_store.get();
        if toggled_off {
            crate::companion_extras::stop_managed_tts_engine();
        } else if switched_to_os {
            crate::companion_extras::stop_managed_tts_engine();
            let _ = crate::companion_extras::ensure_tts_engine_ready_with_mode_blocking(
                &cfg.voicevox_url,
                "os",
                false,
            );
        } else if need_engine_refresh {
            let _ = crate::companion_extras::ensure_tts_engine_ready_with_mode_blocking(
                &cfg.voicevox_url,
                &cfg.tts_engine,
                cfg.voice_tts,
            );
        }

        if let Some(ref uuid) = install_uuid {
            let mut aivis_base = crate::companion_extras::cached_tts_base_url()
                .filter(|u| u.contains(":10101"));
            if aivis_base.is_none() {
                let st = crate::companion_extras::ensure_tts_engine_ready_with_mode_blocking(
                    "http://127.0.0.1:10101",
                    "aivisspeech",
                    true,
                );
                aivis_base = st.base_url.filter(|u| u.contains(":10101"));
            }
            let base = aivis_base.ok_or_else(|| {
                "AivisSpeech (port 10101) が起動していないためモデルを導入できません。".to_string()
            })?;
            crate::companion_extras::install_aivm_model_blocking(&base, uuid.trim())?;
            if need_engine_refresh && cfg.tts_engine != "aivisspeech" {
                let _ = crate::companion_extras::ensure_tts_engine_ready_with_mode_blocking(
                    &cfg.voicevox_url,
                    &cfg.tts_engine,
                    false,
                );
            }
        }

        if let Some(ref uuid) = uninstall_uuid {
            let mut aivis_base = crate::companion_extras::cached_tts_base_url()
                .filter(|u| u.contains(":10101"));
            if aivis_base.is_none() {
                let st = crate::companion_extras::ensure_tts_engine_ready_with_mode_blocking(
                    "http://127.0.0.1:10101",
                    "aivisspeech",
                    false,
                );
                aivis_base = st.base_url.filter(|u| u.contains(":10101"));
            }
            let base = aivis_base.ok_or_else(|| {
                "AivisSpeech (port 10101) が起動していないためモデルを削除できません。".to_string()
            })?;
            let (_, updated_voices) =
                crate::companion_extras::uninstall_aivm_model_blocking(&base, uuid.trim())?;
            let active_shell = self.config_store.get().active_shell_id;
            let (sak_sp, ker_sp, _) = self.config_store.get_shell_voice_profile(&active_shell);
            if let Some(id) = sak_sp {
                if !updated_voices.iter().any(|v| v.style_id == id) {
                    self.config_store.set_shell_voice_speaker(&active_shell, 0, None);
                }
            }
            if let Some(id) = ker_sp {
                if !updated_voices.iter().any(|v| v.style_id == id) {
                    self.config_store.set_shell_voice_speaker(&active_shell, 1, None);
                }
            }
        }

        let _ = self.proxy.send_event(UserEvent::RefreshMenus);
        Ok(self.get_voices())
    }

    fn get_settings(&self) -> Value {
        let (tx, rx) = mpsc::channel();
        if self.proxy.send_event(UserEvent::ServerGetSettings(tx)).is_ok() {
            if let Ok(v) = rx.recv_timeout(Duration::from_millis(120)) {
                return v;
            }
        }
        AppRuntime::settings_snapshot_from_store(&self.config_store)
    }

    fn get_backlog(&self) -> Value {
        let (tx, rx) = mpsc::channel();
        if self.proxy.send_event(UserEvent::ServerGetBacklog(tx)).is_ok() {
            if let Ok(v) = rx.recv_timeout(Duration::from_millis(200)) {
                return v;
            }
        }
        json!({ "count": 0, "entries": [] })
    }

    fn on_update_settings(&self, req: SettingsPatchReq) -> Result<Value, String> {
        crate::sstp_server::validate_settings_patch(&req)?;
        let (tx, rx) = mpsc::channel();
        self.proxy
            .send_event(UserEvent::ServerUpdateSettings { req, reply: tx })
            .map_err(|e| format!("GUI 이벤트 전송 실패: {e}"))?;
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|e| format!("설정 변경 타임아웃: {e}"))?
    }

    fn get_memos(&self) -> Value {
        let store = crate::accessory_utils::MemoStore::new(&self.config_store.config_dir);
        let memos = store.list();
        json!({ "count": memos.len(), "memos": memos })
    }

    fn on_memo_action(&self, req: crate::sstp_server::MemoActionReq) -> Result<Value, String> {
        let store = crate::accessory_utils::MemoStore::new(&self.config_store.config_dir);
        let act = req
            .action
            .as_deref()
            .unwrap_or("add")
            .trim()
            .to_ascii_lowercase();
        match act.as_str() {
            "add" => {
                let txt = req.text.as_deref().unwrap_or("");
                let item = store.add(txt)?;
                let memos = store.list();
                Ok(json!({ "memo": item, "count": memos.len(), "memos": memos }))
            }
            "toggle" => {
                let id = req.id.ok_or_else(|| "id required".to_string())?;
                let item = store
                    .toggle_done(id)
                    .ok_or_else(|| format!("memo #{id} not found"))?;
                let memos = store.list();
                Ok(json!({ "memo": item, "count": memos.len(), "memos": memos }))
            }
            "delete" => {
                let id = req.id.ok_or_else(|| "id required".to_string())?;
                if !store.delete(id) {
                    return Err(format!("memo #{id} not found"));
                }
                let memos = store.list();
                Ok(json!({ "deleted_id": id, "count": memos.len(), "memos": memos }))
            }
            "clear_done" => {
                let removed = store.clear_done();
                let memos = store.list();
                Ok(json!({ "removed": removed, "count": memos.len(), "memos": memos }))
            }
            other => Err(format!("Invalid memo action: {other}")),
        }
    }

    fn get_calendar(&self) -> Value {
        calendar_snapshot_from_store(&self.config_store)
    }

    fn on_calendar_action(&self, req: crate::sstp_server::CalendarConfigReq) -> Result<Value, String> {
        let mut cfg = self.config_store.get();
        let mut next_urls = cfg.calendar_ics_urls.clone();
        if req.clear_ics_urls {
            next_urls.clear();
        }
        if let Some(ref raw_url) = req.add_ics_url {
            let valid = crate::calendar_sync::validate_calendar_ics_url(raw_url)?;
            if !next_urls.iter().any(|u| u == &valid) {
                next_urls.push(valid);
            }
        }
        cfg = self.config_store.update(ConfigPatch {
            calendar_enabled: req.enabled.or_else(|| if req.add_ics_url.is_some() { Some(true) } else { None }),
            calendar_use_os_native: req.use_os_native,
            calendar_ics_urls: if req.clear_ics_urls || req.add_ics_url.is_some() {
                Some(next_urls)
            } else {
                None
            },
            calendar_home_location: req.home_location,
            calendar_default_travel_mins: req.default_travel_mins,
            calendar_prep_mins: req.prep_mins,
            calendar_remind_mins: req.remind_mins,
            calendar_briefing_interval_mins: req.briefing_interval_mins,
            ..Default::default()
        });
        let do_sync = req
            .action
            .as_deref()
            .is_some_and(|a| a.eq_ignore_ascii_case("sync"))
            || req.add_ics_url.is_some();
        if do_sync {
            let mut st = crate::calendar_sync::CalendarSyncState::default();
            st.load_from_disk(&self.config_store.config_dir);
            let evs = crate::calendar_sync::sync_all_calendars_blocking(
                &cfg.calendar_ics_urls,
                cfg.calendar_use_os_native,
                &self.config_store.config_dir,
                &cfg.calendar_home_location,
                cfg.calendar_default_travel_mins,
                &mut st.geocode_cache,
            )?;
            st.events = evs;
            st.last_sync_ts = chrono::Utc::now().timestamp();
            st.save_to_disk(&self.config_store.config_dir);
        }
        let _ = self.proxy.send_event(UserEvent::RefreshMenus);
        Ok(self.get_calendar())
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
    settings: Option<SettingsWindowState>,
    expanded_menu_section: Option<MenuSection>,
    playback: PlaybackQueue,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    tray: Option<TrayIcon>,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    tray_actions: HashMap<MenuId, MenuCommand>,
    server: Option<ServerHandle>,
    modifiers: ModifiersState,
    auto_talk_last: Instant,
    last_chime_key: Option<(i32, u32)>,
    reminder_poll_last: Instant,
    closing_at: Option<Instant>,
    os_insets: DesktopInsets,
    initialized: bool,
    characters_hidden_by_user: bool,
    hidden_by_fullscreen: bool,
    dock_left_edge: bool,
    pomodoro: crate::accessory_utils::PomodoroTimer,
    backlog: crate::accessory_utils::DialogueBacklog,
    battery_status: crate::accessory_utils::BatteryStatus,
    low_battery_notified: bool,
    last_pomo_ui_refresh: Instant,
    pending_auto_talk_actions: Option<Vec<DialogueAction>>,
    aivm_busy_label: Option<String>,
    calendar_state: Arc<Mutex<crate::calendar_sync::CalendarSyncState>>,
    calendar_last_sync: Instant,
    calendar_last_alert_check: Instant,
    calendar_last_briefing: Instant,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum UiWindowTarget {
    Char(Scope),
    Balloon(Scope),
    Communicate,
    Menu,
    Settings,
}

impl AppRuntime {
    fn classify_window(&self, window_id: WindowId) -> Option<UiWindowTarget> {
        if self.sakura.as_ref().is_some_and(|w| w.ws.window.id() == window_id) {
            Some(UiWindowTarget::Char(Scope::Sakura))
        } else if self.kero.as_ref().is_some_and(|w| w.ws.window.id() == window_id) {
            Some(UiWindowTarget::Char(Scope::Kero))
        } else if self
            .sakura_balloon
            .as_ref()
            .is_some_and(|b| b.ws.window.id() == window_id)
        {
            Some(UiWindowTarget::Balloon(Scope::Sakura))
        } else if self
            .kero_balloon
            .as_ref()
            .is_some_and(|b| b.ws.window.id() == window_id)
        {
            Some(UiWindowTarget::Balloon(Scope::Kero))
        } else if self.communicate.as_ref().is_some_and(|c| c.ws.window.id() == window_id) {
            Some(UiWindowTarget::Communicate)
        } else if self.menu.as_ref().is_some_and(|m| m.ws.window.id() == window_id) {
            Some(UiWindowTarget::Menu)
        } else if self.settings.as_ref().is_some_and(|s| s.ws.window.id() == window_id) {
            Some(UiWindowTarget::Settings)
        } else {
            None
        }
    }

    fn window_scale_factor(&self, window_id: WindowId) -> f64 {
        match self.classify_window(window_id) {
            Some(UiWindowTarget::Char(Scope::Sakura)) => self
                .sakura
                .as_ref()
                .map(|c| c.ws.window.scale_factor().max(1.0))
                .unwrap_or(1.0),
            Some(UiWindowTarget::Char(Scope::Kero)) => self
                .kero
                .as_ref()
                .map(|c| c.ws.window.scale_factor().max(1.0))
                .unwrap_or(1.0),
            Some(UiWindowTarget::Balloon(Scope::Sakura)) => self
                .sakura_balloon
                .as_ref()
                .map(|b| b.ws.window.scale_factor().max(1.0))
                .unwrap_or(1.0),
            Some(UiWindowTarget::Balloon(Scope::Kero)) => self
                .kero_balloon
                .as_ref()
                .map(|b| b.ws.window.scale_factor().max(1.0))
                .unwrap_or(1.0),
            Some(UiWindowTarget::Communicate) => self
                .communicate
                .as_ref()
                .map(|c| c.ws.window.scale_factor().max(1.0))
                .unwrap_or(1.0),
            Some(UiWindowTarget::Menu) => self
                .menu
                .as_ref()
                .map(|m| m.ws.window.scale_factor().max(1.0))
                .unwrap_or(1.0),
            Some(UiWindowTarget::Settings) => self
                .settings
                .as_ref()
                .map(|s| s.ws.window.scale_factor().max(1.0))
                .unwrap_or(1.0),
            None => 1.0,
        }
    }
    fn create_borderless_window(
        event_loop: &ActiveEventLoop,
        title: &str,
        w: u32,
        h: u32,
        always_on_top: bool,
        active: bool,
        visible: bool,
    ) -> Result<Arc<Window>, String> {
        Self::create_borderless_window_opts(
            event_loop,
            title,
            w,
            h,
            always_on_top,
            active,
            visible,
            false,
            true,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_borderless_window_opts(
        event_loop: &ActiveEventLoop,
        title: &str,
        w: u32,
        h: u32,
        always_on_top: bool,
        active: bool,
        visible: bool,
        has_shadow: bool,
        accepts_first_mouse: bool,
        skip_taskbar: bool,
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
            attrs = attrs
                .with_has_shadow(has_shadow)
                .with_accepts_first_mouse(accepts_first_mouse);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = (has_shadow, accepts_first_mouse);
        #[cfg(target_os = "windows")]
        {
            attrs = attrs
                .with_skip_taskbar(skip_taskbar)
                .with_undecorated_shadow(has_shadow);
        }
        #[cfg(not(target_os = "windows"))]
        let _ = skip_taskbar;

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
            if self.config_store.get().linux_respect_workarea {
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
        }
        let ins = &self.os_insets;
        let sx = mx + ins.left;
        let sy = my + ins.top;
        let sw = (mw - ins.left - ins.right).max(320.0);
        let sh = (mh - ins.top - ins.bottom).max(240.0);
        (sx, sy, sw, sh)
    }

    fn reset_character_positions(&mut self) {
        self.dock_left_edge = false;
        let (_mx, my, _mw, mh) = self.screen_work_bounds();
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

        let sak_x = (safe_right - sak_w - 28.0).max(sx_safe);
        let sak_floor_y = if sak_full_feet {
            safe_bottom - 6.0
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

    fn speaker_name(&self, scope: Scope) -> String {
        let m = self.manifest.lock().expect("manifest mutex poisoned");
        let name = match scope {
            Scope::Sakura => m.descript.sakura_name.trim(),
            Scope::Kero => m.descript.kero_name.trim(),
        };
        if !name.is_empty() {
            return name.to_string();
        }
        match scope {
            Scope::Sakura => "何か藍".to_string(),
            Scope::Kero => "うにゅう".to_string(),
        }
    }

    fn active_kero_voice_archetype(
        &self,
    ) -> (crate::companion_extras::KeroVoiceArchetype, Option<String>) {
        let cfg = self.config_store.get();
        let (_, _, arch_override) = self
            .config_store
            .get_shell_voice_profile(&cfg.active_shell_id);
        let (shell_name, kero_name) = {
            let m = self.manifest.lock().expect("manifest mutex poisoned");
            let sname = m.descript.name.clone();
            let kname = m.descript.kero_name.trim().to_string();
            (
                sname,
                if kname.is_empty() {
                    "うにゅう".to_string()
                } else {
                    kname
                },
            )
        };
        let arch = crate::companion_extras::infer_kero_voice_archetype(
            &cfg.active_shell_id,
            &shell_name,
            &kero_name,
            arch_override.as_deref(),
        );
        (arch, arch_override)
    }

    fn build_voices_snapshot(&self) -> Value {
        let cfg = self.config_store.get();
        let (sak_sp, ker_sp, arch_override) = self
            .config_store
            .get_shell_voice_profile(&cfg.active_shell_id);
        let (arch, _) = self.active_kero_voice_archetype();
        let (eng_lbl, all_voices) = crate::companion_extras::cached_tts_voices();
        let base_url = crate::companion_extras::cached_tts_base_url();
        let installed_uuids = crate::companion_extras::cached_installed_aivm_uuids();
        let resolved = crate::companion_extras::resolve_effective_voice_selection(
            &all_voices,
            arch,
            sak_sp,
            ker_sp,
        );
        let catalog: Vec<Value> = crate::companion_extras::CURATED_AIVISHUB_MODELS
            .iter()
            .map(|m| {
                let installed = installed_uuids
                    .iter()
                    .any(|u| u.eq_ignore_ascii_case(m.uuid));
                json!({
                    "uuid": m.uuid,
                    "name": m.name,
                    "role_tag": m.role_tag,
                    "menu_label": m.menu_label,
                    "installed": installed,
                })
            })
            .collect();
        let voices_json: Vec<Value> = all_voices
            .iter()
            .map(|v| {
                json!({
                    "style_id": v.style_id,
                    "speaker_name": v.speaker_name,
                    "style_name": v.style_name,
                    "label": v.label,
                })
            })
            .collect();
        let installed_models: Vec<Value> =
            crate::companion_extras::cached_installed_aivm_models()
                .into_iter()
                .map(|m| {
                    json!({
                        "uuid": m.uuid,
                        "name": m.name,
                        "is_default_model": m.is_default_model,
                        "speaker_names": m.speaker_names,
                    })
                })
                .collect();
        json!({
            "active_shell": cfg.active_shell_id,
            "voice_tts": cfg.voice_tts,
            "tts_engine": cfg.tts_engine,
            "voice_speed": cfg.voice_speed,
            "voice_intonation": cfg.voice_intonation,
            "engine_label": eng_lbl,
            "engine_base_url": base_url,
            "sakura_speaker_override": sak_sp,
            "kero_speaker_override": ker_sp,
            "kero_voice_archetype_override": arch_override,
            "effective_kero_voice_archetype": arch.short_code(),
            "effective_kero_voice_label": arch.label_ja(),
            "resolved_sakura_id": resolved.sakura_id,
            "resolved_kero_id": resolved.kero_id,
            "resolved_sakura_voice": resolved.sakura_label,
            "resolved_kero_voice": resolved.kero_label,
            "used_female_fallback": resolved.used_female_fallback,
            "installed_aivm_uuids": installed_uuids,
            "installed_aivm_models": installed_models,
            "curated_aivm_catalog": catalog,
            "voices": voices_json,
        })
    }

    pub(crate) fn settings_snapshot_from_store(store: &ConfigStore) -> Value {
        let cfg = store.get();
        let eff_lang = crate::i18n::resolve_ui_lang(&cfg.ui_language);
        let mut out = json!({
            "config_dir": store.config_dir.display().to_string(),
            "config_file": store.file_path.display().to_string(),
            "os_platform": std::env::consts::OS,
            "ui_language": cfg.ui_language,
            "effective_ui_language": eff_lang.code(),
            "quiet_mode": cfg.quiet_mode,
            "auto_hide_on_fullscreen": cfg.auto_hide_on_fullscreen,
            "auto_start_at_login": cfg.auto_start_at_login,
            "power_save_mode": cfg.power_save_mode,
            "low_battery_alert": cfg.low_battery_alert,
            "pomodoro_preset_idx": cfg.pomodoro_preset_idx,
            "skin_opacity": cfg.skin_opacity,
            "balloon_opacity": cfg.balloon_opacity,
            "character_scale": cfg.character_scale,
            "opaque_on_hover": cfg.opaque_on_hover,
            "eye_tracking": cfg.eye_tracking,
            "lock_position": cfg.lock_position,
            "snap_to_bottom": cfg.snap_to_bottom,
            "click_through_mode": cfg.click_through_mode,
            "show_kero": cfg.show_kero,
            "always_on_top": cfg.always_on_top,
            "live2d_motion": cfg.live2d_motion,
            "hourly_chime": cfg.hourly_chime,
            "voice_tts": cfg.voice_tts,
            "tts_engine": cfg.tts_engine,
            "active_shell_id": cfg.active_shell_id,
            "active_balloon_id": cfg.active_balloon_id,
            "provider": cfg.provider,
            "model": cfg.model,
            "enable_tools": cfg.enable_tools,
            "enable_clipboard_tool": cfg.enable_clipboard_tool,
            "auto_talk_interval_sec": cfg.auto_talk_interval_sec,
            "interest_news_weight": cfg.interest_news_weight,
            "interests": store.active_interests(),
        });
        if let Some(map) = out.as_object_mut() {
            map.insert("voice_speed".to_string(), json!(cfg.voice_speed));
            map.insert("voice_intonation".to_string(), json!(cfg.voice_intonation));
            map.insert("macos_accept_first_mouse".to_string(), json!(cfg.macos_accept_first_mouse));
            map.insert("macos_window_shadow".to_string(), json!(cfg.macos_window_shadow));
            map.insert("macos_show_in_dock".to_string(), json!(cfg.macos_show_in_dock));
            map.insert("windows_skip_taskbar".to_string(), json!(cfg.windows_skip_taskbar));
            map.insert("windows_undecorated_shadow".to_string(), json!(cfg.windows_undecorated_shadow));
            map.insert("linux_prefer_x11".to_string(), json!(cfg.linux_prefer_x11));
            map.insert("linux_respect_workarea".to_string(), json!(cfg.linux_respect_workarea));
        }
        out
    }

    fn build_settings_snapshot(&self) -> Value {
        let mut snap = Self::settings_snapshot_from_store(&self.config_store);
        if let Some(map) = snap.as_object_mut() {
            map.insert(
                "characters_hidden".to_string(),
                json!(self.characters_hidden_by_user),
            );
            map.insert(
                "hidden_by_fullscreen".to_string(),
                json!(self.hidden_by_fullscreen),
            );
            map.insert(
                "on_battery".to_string(),
                json!(self.battery_status.on_battery),
            );
            map.insert(
                "battery_percent".to_string(),
                json!(self.battery_status.percent),
            );
            map.insert("pomodoro".to_string(), self.pomodoro.snapshot_json());
        }
        snap
    }

    fn settings_panel_data(&self) -> SettingsPanelData {
        let cfg = self.config_store.get();
        let ui_lang = crate::i18n::resolve_ui_lang(&cfg.ui_language);
        let s = crate::i18n::ui_strings(ui_lang);
        let ui_language_label = crate::i18n::format_ui_language_label(&cfg.ui_language, ui_lang);
        let characters_hidden = self.characters_hidden_by_user;
        let pomodoro_preset_label = self.pomodoro.format_preset_label(ui_lang);
        let pomodoro_button_label = self.pomodoro.format_button_label(ui_lang);
        let power_save_label = crate::i18n::power_save_label(ui_lang, &cfg.power_save_mode).to_string();
        let active_shell_name = self
            .shell_manager
            .list_shells()
            .into_iter()
            .find(|sh| sh.id == cfg.active_shell_id)
            .map(|sh| sh.name)
            .unwrap_or_else(|| cfg.active_shell_id.clone());
        let active_balloon_name = self
            .balloon_manager
            .list_balloons()
            .into_iter()
            .find(|b| b.id == cfg.active_balloon_id)
            .map(|b| b.name)
            .unwrap_or_else(|| cfg.active_balloon_id.clone());
        let provider_fallback = provider_catalog()
            .into_iter()
            .find(|p| p.id == cfg.provider)
            .map(|p| p.label)
            .unwrap_or_else(|| cfg.provider.clone());
        let provider_label = crate::i18n::provider_label(ui_lang, &cfg.provider, &provider_fallback);
        let tts_engine_label = crate::i18n::tts_engine_label(ui_lang, &cfg.tts_engine);

        let (eng_lbl, all_voices) = crate::companion_extras::cached_tts_voices();
        let (sak_sp, ker_sp, _) = self
            .config_store
            .get_shell_voice_profile(&cfg.active_shell_id);
        let (arch, _) = self.active_kero_voice_archetype();
        let resolved = crate::companion_extras::resolve_effective_voice_selection(
            &all_voices,
            arch,
            sak_sp,
            ker_sp,
        );
        let sakura_voice_label = if sak_sp.is_none() {
            format!("{} ({}) [{eng_lbl}]", s.voice_auto_prefix, resolved.sakura_label)
        } else {
            format!("{} [{eng_lbl}]", resolved.sakura_label)
        };
        let kero_voice_label = if ker_sp.is_none() {
            format!("{} ({}) [{eng_lbl}]", s.voice_auto_prefix, resolved.kero_label)
        } else {
            format!("{} [{eng_lbl}]", resolved.kero_label)
        };
        let kero_archetype_label =
            crate::i18n::kero_archetype_label(ui_lang, arch.short_code()).to_string();
        let config_path_display = self.config_store.file_path.display().to_string();
        let interests_list = self.config_store.active_interests();
        let interests_summary = if interests_list.is_empty() {
            s.interests_empty.to_string()
        } else {
            interests_list.join(", ")
        };

        let (cal_count, cal_connected) = {
            let now_ts = chrono::Utc::now().timestamp();
            let ev_cnt = self
                .calendar_state
                .lock()
                .map(|st| st.upcoming_events(now_ts, 48).len())
                .unwrap_or(0);
            let conn = cfg.calendar_use_os_native || !cfg.calendar_ics_urls.is_empty();
            (ev_cnt, conn)
        };
        let calendar_status_summary = if !cal_connected {
            match ui_lang {
                crate::i18n::UiLang::Ko => "미연결".to_string(),
                crate::i18n::UiLang::En => "Not connected".to_string(),
                crate::i18n::UiLang::Ja => "未連携".to_string(),
            }
        } else {
            let feed_n = cfg.calendar_ics_urls.len();
            match ui_lang {
                crate::i18n::UiLang::Ko => format!("예정 {cal_count}건 · 피드 {feed_n}"),
                crate::i18n::UiLang::En => format!("{cal_count} upcoming · {feed_n} feeds"),
                crate::i18n::UiLang::Ja => format!("予定 {cal_count}件・フィード {feed_n}"),
            }
        };
        let calendar_briefing_label = if cfg.calendar_briefing_interval_mins == 0 {
            match ui_lang {
                crate::i18n::UiLang::Ko => "끄기".to_string(),
                crate::i18n::UiLang::En => "Off".to_string(),
                crate::i18n::UiLang::Ja => "OFF".to_string(),
            }
        } else {
            let m = cfg.calendar_briefing_interval_mins;
            if m % 60 == 0 {
                let h = m / 60;
                match ui_lang {
                    crate::i18n::UiLang::Ko => format!("{h}시간마다"),
                    crate::i18n::UiLang::En => format!("Every {h}h"),
                    crate::i18n::UiLang::Ja => format!("{h}時間ごと"),
                }
            } else {
                match ui_lang {
                    crate::i18n::UiLang::Ko => format!("{m}분마다"),
                    crate::i18n::UiLang::En => format!("Every {m}m"),
                    crate::i18n::UiLang::Ja => format!("{m}分ごと"),
                }
            }
        };
        let calendar_travel_label = match ui_lang {
            crate::i18n::UiLang::Ko => format!("{}분 (+준비 {}분)", cfg.calendar_default_travel_mins, cfg.calendar_prep_mins),
            crate::i18n::UiLang::En => format!("{}m (+{}m prep)", cfg.calendar_default_travel_mins, cfg.calendar_prep_mins),
            crate::i18n::UiLang::Ja => format!("{}分（+準備{}分）", cfg.calendar_default_travel_mins, cfg.calendar_prep_mins),
        };

        SettingsPanelData {
            cfg,
            config_path_display,
            active_shell_name,
            active_balloon_name,
            provider_label,
            tts_engine_label,
            sakura_voice_label,
            kero_voice_label,
            kero_archetype_label,
            recommended_local_ai_hint: crate::companion_extras::recommended_local_ai_summary(),
            local_ai_setup_busy: crate::companion_extras::is_local_ai_setup_running(),
            interests_summary,
            ui_lang,
            ui_language_label,
            characters_hidden,
            pomodoro_preset_label,
            pomodoro_button_label,
            power_save_label,
            installed_aivm_models: crate::companion_extras::cached_installed_aivm_models(),
            aivm_busy_label: self.aivm_busy_label.clone(),
            calendar_status_summary,
            calendar_briefing_label,
            calendar_travel_label,
        }
    }

    fn apply_live_settings(&mut self) {
        let cfg = self.config_store.get();
        let lvl = if cfg.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        };
        let target_scale = (cfg.character_scale as f64).clamp(0.50, 1.50);
        let mut scale_changed = false;

        if let Some(ref mut s) = self.sakura {
            s.ws.window.set_window_level(lvl);
            #[cfg(target_os = "macos")]
            s.ws.window.set_has_shadow(cfg.macos_window_shadow);
            #[cfg(target_os = "windows")]
            {
                s.ws.window.set_skip_taskbar(cfg.windows_skip_taskbar);
                s.ws.window
                    .set_undecorated_shadow(cfg.windows_undecorated_shadow);
            }
            if cfg.click_through_mode {
                if s.ws.window.set_cursor_hittest(false).is_ok() {
                    s.last_hittest = Some(false);
                }
                s.hovered_opaque = false;
            }
            if (s.user_scale - target_scale).abs() > 0.005 {
                s.user_scale = target_scale;
                if let Some(ref gpu) = self.gpu {
                    s.rebuild_composited(gpu, false);
                }
                scale_changed = true;
            }
            let op = if cfg.opaque_on_hover && s.hovered_opaque && !cfg.click_through_mode {
                1.0
            } else {
                cfg.skin_opacity
            };
            if let Some(ref gpu) = self.gpu {
                s.ws.set_opacity(gpu, op);
            }
        }
        if let Some(ref mut k) = self.kero {
            k.visible = cfg.show_kero;
            k.ws.window.set_visible(cfg.show_kero);
            k.ws.window.set_window_level(lvl);
            #[cfg(target_os = "macos")]
            k.ws.window.set_has_shadow(cfg.macos_window_shadow);
            #[cfg(target_os = "windows")]
            {
                k.ws.window.set_skip_taskbar(cfg.windows_skip_taskbar);
                k.ws.window
                    .set_undecorated_shadow(cfg.windows_undecorated_shadow);
            }
            if cfg.click_through_mode {
                if k.ws.window.set_cursor_hittest(false).is_ok() {
                    k.last_hittest = Some(false);
                }
                k.hovered_opaque = false;
            }
            if (k.user_scale - target_scale).abs() > 0.005 {
                k.user_scale = target_scale;
                if let Some(ref gpu) = self.gpu {
                    k.rebuild_composited(gpu, false);
                }
                scale_changed = true;
            }
            let op = if cfg.opaque_on_hover && k.hovered_opaque && !cfg.click_through_mode {
                1.0
            } else {
                cfg.skin_opacity
            };
            if let Some(ref gpu) = self.gpu {
                k.ws.set_opacity(gpu, op);
            }
        }
        if !cfg.show_kero {
            if let Some(ref mut kb) = self.kero_balloon {
                kb.visible = false;
                kb.ws.window.set_visible(false);
            }
        }
        if let Some(ref mut sb) = self.sakura_balloon {
            sb.ws.window.set_window_level(lvl);
            if let Some(ref gpu) = self.gpu {
                sb.ws.set_opacity(gpu, cfg.balloon_opacity);
            }
        }
        if let Some(ref mut kb) = self.kero_balloon {
            kb.ws.window.set_window_level(lvl);
            if let Some(ref gpu) = self.gpu {
                kb.ws.set_opacity(gpu, cfg.balloon_opacity);
            }
        }
        if !cfg.live2d_motion {
            if let Some(ref gpu) = self.gpu {
                if let Some(ref mut s) = self.sakura {
                    s.tick_live2d(gpu, false, None, false);
                }
                if let Some(ref mut k) = self.kero {
                    k.tick_live2d(gpu, false, None, false);
                }
            }
        }
        if scale_changed {
            self.reposition_balloon(Scope::Sakura);
            self.reposition_balloon(Scope::Kero);
        }
    }

    fn effective_ui_lang(&self) -> crate::i18n::UiLang {
        crate::i18n::resolve_ui_lang(&self.config_store.get().ui_language)
    }

    fn ui_strings(&self) -> &'static crate::i18n::UiStrings {
        crate::i18n::ui_strings(self.effective_ui_lang())
    }

    fn redraw_communicate_if_visible(&mut self) {
        let ui_lang = self.effective_ui_lang();
        if let (Some(ref gpu), Some(ref mut cw)) = (&self.gpu, &mut self.communicate) {
            if cw.visible {
                cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu, ui_lang);
            }
        }
    }

    fn redraw_settings_panel(&mut self) {
        if !self.settings.as_ref().is_some_and(|s| s.visible) {
            return;
        }
        let data = self.settings_panel_data();
        if let (Some(ref gpu), Some(ref mut sw)) = (&self.gpu, &mut self.settings) {
            sw.redraw(&data, &mut self.text_renderer, gpu);
        }
    }

    fn open_settings_panel(&mut self) {
        let (mx, my, mw, mh) = self.desktop_safe_work_area();
        let data = self.settings_panel_data();
        let Some(ref mut sw) = self.settings else { return };
        let w = sw.win_size.0 as f64;
        let h = sw.win_size.1 as f64;
        let px = (mx + (mw - w) * 0.5).clamp(mx + 8.0, (mx + mw - w - 8.0).max(mx + 8.0));
        let py = (my + (mh - h) * 0.45).clamp(my + 8.0, (my + mh - h - 8.0).max(my + 8.0));
        sw.visible = true;
        sw.dragging_header = false;
        sw.last_local_cursor = None;
        sw.model.dragging_slider = None;
        sw.ws.window.set_outer_position(LogicalPosition::new(px, py));
        sw.ws.window.set_visible(true);
        sw.ws.window.focus_window();
        if let Some(ref gpu) = self.gpu {
            sw.redraw(&data, &mut self.text_renderer, gpu);
        }
    }

    fn hide_settings_panel(&mut self) {
        if let Some(ref mut sw) = self.settings {
            if sw.visible {
                sw.visible = false;
                sw.dragging_header = false;
                sw.model.dragging_slider = None;
                sw.ws.window.set_visible(false);
            }
        }
    }

    fn handle_settings_key(&mut self, logical_key: &Key) {
        if matches!(logical_key, Key::Named(NamedKey::Escape)) {
            self.hide_settings_panel();
        }
    }

    fn cycle_shell(&mut self, delta: i32) {
        let shells = self.shell_manager.list_shells();
        if shells.len() <= 1 {
            return;
        }
        let cur = self.config_store.get().active_shell_id;
        let pos = shells.iter().position(|s| s.id == cur).unwrap_or(0) as isize;
        let next = (pos + delta as isize).rem_euclid(shells.len() as isize) as usize;
        let _ = self.apply_skin_switch(Some(shells[next].id.clone()), None);
    }

    fn cycle_balloon(&mut self, delta: i32) {
        let balloons = self.balloon_manager.list_balloons();
        if balloons.len() <= 1 {
            return;
        }
        let cur = self.config_store.get().active_balloon_id;
        let pos = balloons.iter().position(|b| b.id == cur).unwrap_or(0) as isize;
        let next = (pos + delta as isize).rem_euclid(balloons.len() as isize) as usize;
        let _ = self.apply_skin_switch(None, Some(balloons[next].id.clone()));
    }

    fn cycle_provider(&mut self, delta: i32) {
        let cat = provider_catalog();
        if cat.is_empty() {
            return;
        }
        let cur = self.config_store.get().provider;
        let pos = cat.iter().position(|p| p.id == cur).unwrap_or(0) as isize;
        let next = (pos + delta as isize).rem_euclid(cat.len() as isize) as usize;
        let p = &cat[next];
        if self.smoke_mode {
            self.config_store.update(ConfigPatch {
                provider: Some(p.id.clone()),
                model: Some(p.default_model.clone()),
                ..Default::default()
            });
        } else {
            let _ = self.handle_provider_switch(p.id.clone(), Some(p.default_model.clone()));
        }
    }

    fn cycle_tts_engine(&mut self, delta: i32) {
        let cur = self.config_store.get().tts_engine;
        let pos = TTS_ENGINE_CHOICES
            .iter()
            .position(|(id, _)| *id == cur)
            .unwrap_or(0) as isize;
        let next = (pos + delta as isize).rem_euclid(TTS_ENGINE_CHOICES.len() as isize) as usize;
        let next_id = TTS_ENGINE_CHOICES[next].0.to_string();
        self.execute_menu_command_inner(None, MenuCommand::SelectTtsEngine(next_id));
    }

    fn cycle_voice_speaker(&mut self, scope: u8, delta: i32) {
        let (_, all_voices) = crate::companion_extras::cached_tts_voices();
        let options = Self::representative_voice_options(&all_voices, 12);
        if options.is_empty() {
            return;
        }
        let cur_shell = self.config_store.get().active_shell_id;
        let (sak_sp, ker_sp, _) = self.config_store.get_shell_voice_profile(&cur_shell);
        let cur_id = if scope == 1 { ker_sp } else { sak_sp };
        let mut slots: Vec<Option<i64>> = vec![None];
        for v in options {
            slots.push(Some(v.style_id));
        }
        let pos = slots.iter().position(|s| *s == cur_id).unwrap_or(0) as isize;
        let next = (pos + delta as isize).rem_euclid(slots.len() as isize) as usize;
        self.execute_menu_command_inner(
            None,
            MenuCommand::SelectVoiceSpeaker {
                scope,
                style_id: slots[next],
            },
        );
    }

    fn cycle_kero_archetype(&mut self, delta: i32) {
        let cur_shell = self.config_store.get().active_shell_id;
        let (_, _, cur_arch) = self.config_store.get_shell_voice_profile(&cur_shell);
        let mut slots: Vec<Option<String>> = vec![None];
        for (code, _) in KERO_ARCHETYPE_CHOICES {
            slots.push(Some((*code).to_string()));
        }
        let pos = slots.iter().position(|s| *s == cur_arch).unwrap_or(0) as isize;
        let next = (pos + delta as isize).rem_euclid(slots.len() as isize) as usize;
        self.execute_menu_command_inner(
            None,
            MenuCommand::SelectKeroVoiceArchetype(slots[next].clone()),
        );
    }

    fn cycle_autotalk(&mut self, delta: i32) {
        const INTERVALS: [u64; 7] = [0, 30, 60, 120, 180, 300, 600];
        let cur = self.config_store.get().auto_talk_interval_sec;
        let pos = INTERVALS
            .iter()
            .position(|&v| v == cur)
            .unwrap_or_else(|| {
                INTERVALS
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, &v)| v.abs_diff(cur))
                    .map(|(i, _)| i)
                    .unwrap_or(4)
            }) as isize;
        let next = (pos + delta as isize).rem_euclid(INTERVALS.len() as isize) as usize;
        self.config_store.update(ConfigPatch {
            auto_talk_interval_sec: Some(INTERVALS[next]),
            ..Default::default()
        });
        self.auto_talk_last = Instant::now();
    }

    fn cycle_ui_language(&mut self, delta: i32) {
        let choices = crate::i18n::UI_LANGUAGE_CHOICES;
        if choices.is_empty() {
            return;
        }
        let cur = crate::i18n::normalize_ui_language(&self.config_store.get().ui_language);
        let pos = choices.iter().position(|&c| c == cur).unwrap_or(0) as isize;
        let next = (pos + delta as isize).rem_euclid(choices.len() as isize) as usize;
        self.config_store.update(ConfigPatch {
            ui_language: Some(choices[next].to_string()),
            ..Default::default()
        });
    }

    fn characters_effectively_hidden(&self) -> bool {
        let cfg = self.config_store.get();
        self.characters_hidden_by_user || (cfg.auto_hide_on_fullscreen && self.hidden_by_fullscreen)
    }

    fn apply_visibility_state(&mut self) {
        let hide = self.characters_effectively_hidden();
        let show_kero_cfg = self.config_store.get().show_kero;
        if let Some(ref mut s) = self.sakura {
            s.visible = !hide;
            s.ws.window.set_visible(!hide);
        }
        if let Some(ref mut k) = self.kero {
            let vis = !hide && show_kero_cfg;
            k.visible = vis;
            k.ws.window.set_visible(vis);
        }
        if hide {
            if let Some(ref sb) = self.sakura_balloon {
                sb.ws.window.set_visible(false);
            }
            if let Some(ref kb) = self.kero_balloon {
                kb.ws.window.set_visible(false);
            }
            if let Some(ref mut cw) = self.communicate {
                if cw.visible {
                    cw.visible = false;
                    cw.focused = false;
                    cw.ws.window.set_visible(false);
                }
            }
            self.hide_menu();
        } else {
            if let Some(ref sb) = self.sakura_balloon {
                sb.ws.window.set_visible(sb.visible);
            }
            if let Some(ref kb) = self.kero_balloon {
                kb.ws.window.set_visible(kb.visible && show_kero_cfg);
            }
        }
    }

    fn move_to_next_monitor_or_edge(&mut self) {
        let (sak_w, sak_h, sak_full_feet, sak_foot_off, sak_pos) = match self.sakura {
            Some(ref mut s) => {
                s.sync_pos_from_window();
                (
                    s.win_size.0 as f64,
                    s.win_size.1 as f64,
                    s.is_full_feet(),
                    s.foot_bottom_offset_logical(),
                    s.logical_pos,
                )
            }
            None => return,
        };
        let (kero_w, kero_h, kero_full_feet, kero_foot_off) = self
            .kero
            .as_ref()
            .map(|k| {
                (
                    k.win_size.0 as f64,
                    k.win_size.1 as f64,
                    k.is_full_feet(),
                    k.foot_bottom_offset_logical(),
                )
            })
            .unwrap_or((200.0, 250.0, true, 8.0));

        let mon_rects: Vec<(i32, i32, u32, u32)> = self
            .sakura
            .as_ref()
            .map(|s| {
                s.ws.window
                    .available_monitors()
                    .map(|m| {
                        let sc = m.scale_factor().max(1.0);
                        let p = m.position().to_logical::<i32>(sc);
                        let sz = m.size().to_logical::<u32>(sc);
                        (p.x, p.y, sz.width, sz.height)
                    })
                    .collect()
            })
            .unwrap_or_default();

        let cur_center = (
            (sak_pos.0 + sak_w * 0.5).round() as i32,
            (sak_pos.1 + sak_h * 0.5).round() as i32,
        );
        if let Some(next_idx) =
            crate::accessory_utils::select_next_monitor_index(&mon_rects, cur_center)
        {
            let (mx, my, mw, mh) = mon_rects[next_idx];
            let mx = mx as f64;
            let my = my as f64;
            let mw = mw as f64;
            let mh = mh as f64;
            let sak_x = (mx + mw - sak_w - 28.0).max(mx + 12.0);
            let sak_floor_y = if sak_full_feet {
                my + mh - 24.0
            } else {
                my + mh - 16.0
            };
            let sak_y = (sak_floor_y - sak_h + if sak_full_feet { sak_foot_off } else { 0.0 })
                .clamp(my + 24.0, (my + mh - sak_h).max(my + 24.0));
            if let Some(ref mut s) = self.sakura {
                s.set_logical_pos(sak_x, sak_y);
            }
            let kero_x = (sak_x - kero_w - 24.0).max(mx + 12.0);
            let kero_y = (sak_floor_y - kero_h + if kero_full_feet { kero_foot_off } else { 0.0 })
                .clamp(my + 24.0, (my + mh - kero_h).max(my + 24.0));
            if let Some(ref mut k) = self.kero {
                k.set_logical_pos(kero_x, kero_y);
            }
            self.reposition_balloon(Scope::Sakura);
            self.reposition_balloon(Scope::Kero);
            let msg = format!("{}{}", self.ui_strings().status_monitor_moved_prefix, next_idx + 1);
            self.apply_status_feedback(msg);
            return;
        }

        self.dock_left_edge = !self.dock_left_edge;
        if self.dock_left_edge {
            let (sx_safe, sy_safe, _sw_safe, sh_safe) = self.desktop_safe_work_area();
            let (_mx, my, _mw, mh) = self.screen_work_bounds();
            let safe_bottom = sy_safe + sh_safe;
            let sak_x = sx_safe + 28.0;
            let sak_floor_y = if sak_full_feet {
                safe_bottom - 6.0
            } else {
                safe_bottom
            };
            let sak_y = (sak_floor_y - sak_h + if sak_full_feet { sak_foot_off } else { 0.0 })
                .clamp(sy_safe, (my + mh - sak_h).max(sy_safe));
            if let Some(ref mut s) = self.sakura {
                s.set_logical_pos(sak_x, sak_y);
            }
            let kero_x = sak_x + sak_w + 24.0;
            let kero_y = (sak_floor_y - kero_h + if kero_full_feet { kero_foot_off } else { 0.0 })
                .clamp(sy_safe, (my + mh - kero_h).max(sy_safe));
            if let Some(ref mut k) = self.kero {
                k.set_logical_pos(kero_x, kero_y);
            }
            self.reposition_balloon(Scope::Sakura);
            self.reposition_balloon(Scope::Kero);
            let msg = self.ui_strings().status_dock_edge_left.to_string();
            self.apply_status_feedback(msg);
        } else {
            self.reset_character_positions();
            let msg = self.ui_strings().status_dock_edge_right.to_string();
            self.apply_status_feedback(msg);
        }
    }

    fn copy_last_reply_to_clipboard(&mut self) {
        let Some(text) = self.backlog.latest_plain_text() else {
            let msg = self.ui_strings().status_copy_empty.to_string();
            self.apply_status_feedback(msg);
            return;
        };
        if !self.smoke_mode {
            if let Ok(mut cb) = Clipboard::new() {
                let _ = cb.set_text(text);
            }
        }
        let msg = self.ui_strings().status_copied_reply.to_string();
        self.apply_status_feedback(msg);
    }

    fn show_backlog_in_balloon(&mut self) {
        if self.characters_effectively_hidden() {
            self.characters_hidden_by_user = false;
            self.apply_visibility_state();
        }
        let script = self
            .backlog
            .format_replay_script(8, self.effective_ui_lang());
        let _ = self
            .proxy
            .send_event(UserEvent::PlayActionsNoBacklog(parse_dialogue(&script)));
    }

    fn cycle_pomodoro_preset(&mut self, delta: i32) {
        let total = crate::accessory_utils::POMODORO_PRESETS.len() as isize;
        let cur = self.config_store.get().pomodoro_preset_idx as isize;
        let next = (cur + delta as isize).rem_euclid(total) as usize;
        self.config_store.update(ConfigPatch {
            pomodoro_preset_idx: Some(next),
            ..Default::default()
        });
        self.pomodoro.set_preset(next);
    }

    fn sync_pomodoro_pose(&mut self) {
        let active = self.pomodoro.is_active();
        let (sak_target, ker_target) = if active { (9, 13) } else { (0, 10) };
        if let Some(ref gpu) = self.gpu {
            let mut m = self.manifest.lock().expect("manifest mutex poisoned");
            if let Some(ref mut s) = self.sakura {
                if m.surfaces.contains_key(&sak_target) && s.surface_id != sak_target {
                    s.set_surface(&mut m, gpu, sak_target, false);
                }
            }
            if let Some(ref mut k) = self.kero {
                if m.surfaces.contains_key(&ker_target) && k.surface_id != ker_target {
                    k.set_surface(&mut m, gpu, ker_target, false);
                }
            }
        }
    }

    fn apply_pomodoro_action(&mut self, action: &str) {
        match action.trim().to_ascii_lowercase().as_str() {
            "start" => self.pomodoro.start_focus(),
            "pause" => {
                if self.pomodoro.is_active() && !self.pomodoro.paused {
                    self.pomodoro.toggle_start_pause();
                }
            }
            "stop" => self.pomodoro.stop(),
            "skip" => {
                if let Some(trans) = self.pomodoro.skip_phase() {
                    let script = crate::accessory_utils::build_pomodoro_transition_script(
                        trans,
                        self.effective_ui_lang(),
                    );
                    let _ = self
                        .proxy
                        .send_event(UserEvent::PlayActionsNoBacklog(parse_dialogue(&script)));
                }
            }
            _ => self.pomodoro.toggle_start_pause(),
        }
        self.sync_pomodoro_pose();
        let lbl = self.pomodoro.format_button_label(self.effective_ui_lang());
        self.apply_status_feedback(lbl);
    }

    fn cycle_power_save_mode(&mut self, delta: i32) {
        let choices = crate::accessory_utils::POWER_SAVE_CHOICES;
        let cur = crate::accessory_utils::normalize_power_save_mode(
            &self.config_store.get().power_save_mode,
        );
        let pos = choices.iter().position(|&c| c == cur).unwrap_or(0) as isize;
        let next = (pos + delta as isize).rem_euclid(choices.len() as isize) as usize;
        let mode = choices[next].to_string();
        self.config_store.update(ConfigPatch {
            power_save_mode: Some(mode.clone()),
            ..Default::default()
        });
        let msg = format!(
            "{}{}",
            self.ui_strings().status_power_save_prefix,
            crate::i18n::power_save_label(self.effective_ui_lang(), &mode)
        );
        self.apply_status_feedback(msg);
    }

    fn apply_autostart_toggle(&mut self, enabled: bool) {
        if !self.smoke_mode {
            if let Ok(exe) = std::env::current_exe() {
                if let Err(err) = crate::accessory_utils::apply_os_autostart(enabled, &exe) {
                    let msg = format!("{}{err}", self.ui_strings().status_autostart_err);
                    self.apply_status_feedback(msg);
                    return;
                }
            }
        }
        self.config_store.update(ConfigPatch {
            auto_start_at_login: Some(enabled),
            ..Default::default()
        });
        let msg = if enabled {
            self.ui_strings().status_autostart_on
        } else {
            self.ui_strings().status_autostart_off
        }
        .to_string();
        self.apply_status_feedback(msg);
    }

    fn start_one_click_local_ai_setup(&mut self) {
        if self.smoke_mode {
            self.config_store.update(ConfigPatch {
                provider: Some("ollama".to_string()),
                endpoint_url: Some("http://127.0.0.1:11434".to_string()),
                model: Some("auto".to_string()),
                ..Default::default()
            });
            let msg = self.ui_strings().status_local_ai_dry_run.to_string();
            self.apply_status_feedback(msg);
            self.redraw_settings_panel();
            self.rebuild_tray_menu();
            return;
        }
        if crate::companion_extras::is_local_ai_setup_running() {
            let msg = self.ui_strings().status_local_ai_busy.to_string();
            self.apply_status_feedback(msg);
            return;
        }
        let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(
            "\\0\\s[5]このPCのスペックを確認して、無料ローカルAI（Ollama＋軽量モデル）を自動で準備するね！\\1\\s[10]ちょっと待っててな！\\e",
        )));
        let checking = self.ui_strings().status_local_ai_checking.to_string();
        self.apply_status_feedback(checking);

        let proxy = self.proxy.clone();
        let store = self.config_store.clone();
        std::thread::spawn(move || {
            let mut notified_busy = false;
            let res = crate::companion_extras::prepare_local_ollama_blocking(
                "http://127.0.0.1:11434",
                |progress| {
                    if !notified_busy {
                        notified_busy = true;
                        let _ = proxy.send_event(UserEvent::RefreshMenus);
                    }
                    let _ = proxy.send_event(UserEvent::StatusUpdate(progress, true));
                },
            );
            match res {
                Ok(info) => {
                    if info.installed_ready {
                        store.update(ConfigPatch {
                            provider: Some("ollama".to_string()),
                            endpoint_url: Some("http://127.0.0.1:11434".to_string()),
                            model: Some(info.chosen_model.clone()),
                            ..Default::default()
                        });
                    }
                    let _ = proxy.send_event(UserEvent::StatusUpdate(String::new(), false));
                    let _ = proxy.send_event(UserEvent::RefreshMenus);
                    let _ = proxy.send_event(UserEvent::PlayActions(parse_dialogue(
                        &info.summary_script,
                    )));
                }
                Err(err) => {
                    let short_err: String = err.chars().take(120).collect();
                    let _ = proxy.send_event(UserEvent::StatusUpdate(String::new(), false));
                    let _ = proxy.send_event(UserEvent::RefreshMenus);
                    let msg = format!(
                        "\\0\\s[3]無料ローカルAIの自動準備に失敗しちゃった…。\\n({short_err})\\n\\q[🔄 もう一度試す,__setup_local_ai__]\\1\\s[11]ネット接続やディスク空き容量を確認してみよか！\\e"
                    );
                    let _ = proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
                }
            }
        });
    }

    fn execute_settings_action(&mut self, act: SettingsAction) {
        let cur = self.config_store.get();
        match act {
            SettingsAction::Close => {
                self.hide_settings_panel();
                return;
            }
            SettingsAction::SelectTab(tab) => {
                if let Some(ref mut sw) = self.settings {
                    sw.model.select_tab(tab);
                }
                self.redraw_settings_panel();
                return;
            }
            SettingsAction::SetSkinOpacity(v) => {
                self.config_store.update(ConfigPatch {
                    skin_opacity: Some(v),
                    ..Default::default()
                });
            }
            SettingsAction::SetBalloonOpacity(v) => {
                self.config_store.update(ConfigPatch {
                    balloon_opacity: Some(v),
                    ..Default::default()
                });
            }
            SettingsAction::SetCharacterScale(v) => {
                self.config_store.update(ConfigPatch {
                    character_scale: Some(v),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleOpaqueOnHover => {
                self.config_store.update(ConfigPatch {
                    opaque_on_hover: Some(!cur.opaque_on_hover),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleLive2dMotion => {
                self.config_store.update(ConfigPatch {
                    live2d_motion: Some(!cur.live2d_motion),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleEyeTracking => {
                self.config_store.update(ConfigPatch {
                    eye_tracking: Some(!cur.eye_tracking),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleKero => {
                self.config_store.update(ConfigPatch {
                    show_kero: Some(!cur.show_kero),
                    ..Default::default()
                });
            }
            SettingsAction::CycleShell(d) => self.cycle_shell(d),
            SettingsAction::CycleBalloon(d) => self.cycle_balloon(d),
            SettingsAction::ToggleAlwaysOnTop => {
                self.config_store.update(ConfigPatch {
                    always_on_top: Some(!cur.always_on_top),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleClickThrough => {
                self.execute_menu_command_inner(None, MenuCommand::ToggleClickThrough);
                return;
            }
            SettingsAction::ToggleLockPosition => {
                self.config_store.update(ConfigPatch {
                    lock_position: Some(!cur.lock_position),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleSnapToBottom => {
                self.config_store.update(ConfigPatch {
                    snap_to_bottom: Some(!cur.snap_to_bottom),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleHourlyChime => {
                self.config_store.update(ConfigPatch {
                    hourly_chime: Some(!cur.hourly_chime),
                    ..Default::default()
                });
            }
            SettingsAction::CycleUiLanguage(d) => {
                self.cycle_ui_language(d);
                let setting_now = self.config_store.get().ui_language;
                let msg = crate::i18n::format_ui_lang_status(
                    &setting_now,
                    self.effective_ui_lang(),
                );
                self.apply_status_feedback(msg);
                self.redraw_communicate_if_visible();
            }
            SettingsAction::ToggleQuietMode => {
                let next = !cur.quiet_mode;
                self.config_store.update(ConfigPatch {
                    quiet_mode: Some(next),
                    ..Default::default()
                });
                let msg = if next {
                    self.ui_strings().status_quiet_on
                } else {
                    self.ui_strings().status_quiet_off
                }
                .to_string();
                self.apply_status_feedback(msg);
            }
            SettingsAction::ToggleAutoHideFullscreen => {
                let next = !cur.auto_hide_on_fullscreen;
                self.config_store.update(ConfigPatch {
                    auto_hide_on_fullscreen: Some(next),
                    ..Default::default()
                });
                if !next {
                    self.hidden_by_fullscreen = false;
                }
                self.apply_visibility_state();
            }
            SettingsAction::ToggleHideCharacters => {
                self.characters_hidden_by_user = !self.characters_hidden_by_user;
                self.apply_visibility_state();
                if !self.characters_hidden_by_user {
                    let msg = self.ui_strings().status_chars_shown.to_string();
                    self.apply_status_feedback(msg);
                }
            }
            SettingsAction::MoveNextMonitor => {
                self.move_to_next_monitor_or_edge();
            }
            SettingsAction::CyclePomodoroPreset(d) => {
                self.cycle_pomodoro_preset(d);
            }
            SettingsAction::TogglePomodoro => {
                self.apply_pomodoro_action("toggle");
            }
            SettingsAction::StopPomodoro => {
                self.apply_pomodoro_action("stop");
            }
            SettingsAction::SkipPomodoro => {
                self.apply_pomodoro_action("skip");
            }
            SettingsAction::ShowBacklog => {
                self.show_backlog_in_balloon();
                return;
            }
            SettingsAction::CopyLastReply => {
                self.copy_last_reply_to_clipboard();
                return;
            }
            SettingsAction::ToggleCalendarEnabled => {
                let next = !cur.calendar_enabled;
                self.config_store.update(ConfigPatch {
                    calendar_enabled: Some(next),
                    ..Default::default()
                });
                if next {
                    self.spawn_calendar_sync(true);
                }
            }
            SettingsAction::ToggleCalendarOsNative => {
                let next = !cur.calendar_use_os_native;
                self.config_store.update(ConfigPatch {
                    calendar_use_os_native: Some(next),
                    calendar_enabled: if next { Some(true) } else { None },
                    ..Default::default()
                });
                if next {
                    self.spawn_calendar_sync(true);
                }
            }
            SettingsAction::CycleCalendarBriefing(d) => {
                const OPTIONS: [u32; 5] = [0, 60, 120, 180, 240];
                let idx = OPTIONS
                    .iter()
                    .position(|&v| v == cur.calendar_briefing_interval_mins)
                    .unwrap_or(2) as isize;
                let next = OPTIONS[(idx + d as isize).rem_euclid(OPTIONS.len() as isize) as usize];
                self.config_store.update(ConfigPatch {
                    calendar_briefing_interval_mins: Some(next),
                    ..Default::default()
                });
            }
            SettingsAction::CycleCalendarTravelMins(d) => {
                const OPTIONS: [u32; 5] = [15, 30, 45, 60, 90];
                let idx = OPTIONS
                    .iter()
                    .position(|&v| v == cur.calendar_default_travel_mins)
                    .unwrap_or(1) as isize;
                let next = OPTIONS[(idx + d as isize).rem_euclid(OPTIONS.len() as isize) as usize];
                self.config_store.update(ConfigPatch {
                    calendar_default_travel_mins: Some(next),
                    ..Default::default()
                });
            }
            SettingsAction::PasteCalendarIcsUrl => {
                self.paste_calendar_ics_from_clipboard();
                return;
            }
            SettingsAction::SyncCalendarNow => {
                self.spawn_calendar_sync(true);
                return;
            }
            SettingsAction::ShowCalendarAgenda => {
                self.show_calendar_agenda_in_balloon();
                return;
            }
            SettingsAction::ToggleAutoStart => {
                self.apply_autostart_toggle(!cur.auto_start_at_login);
            }
            SettingsAction::CyclePowerSaveMode(d) => {
                self.cycle_power_save_mode(d);
            }
            SettingsAction::ToggleLowBatteryAlert => {
                self.config_store.update(ConfigPatch {
                    low_battery_alert: Some(!cur.low_battery_alert),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleMacosAcceptFirstMouse => {
                self.config_store.update(ConfigPatch {
                    macos_accept_first_mouse: Some(!cur.macos_accept_first_mouse),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleMacosShadow => {
                self.config_store.update(ConfigPatch {
                    macos_window_shadow: Some(!cur.macos_window_shadow),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleMacosDock => {
                self.config_store.update(ConfigPatch {
                    macos_show_in_dock: Some(!cur.macos_show_in_dock),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleWindowsSkipTaskbar => {
                self.config_store.update(ConfigPatch {
                    windows_skip_taskbar: Some(!cur.windows_skip_taskbar),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleWindowsShadow => {
                self.config_store.update(ConfigPatch {
                    windows_undecorated_shadow: Some(!cur.windows_undecorated_shadow),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleLinuxPreferX11 => {
                self.config_store.update(ConfigPatch {
                    linux_prefer_x11: Some(!cur.linux_prefer_x11),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleLinuxWorkarea => {
                self.config_store.update(ConfigPatch {
                    linux_respect_workarea: Some(!cur.linux_respect_workarea),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleVoiceTts => {
                self.execute_menu_command_inner(None, MenuCommand::ToggleVoiceTts);
                self.redraw_settings_panel();
                return;
            }
            SettingsAction::CycleTtsEngine(d) => {
                self.cycle_tts_engine(d);
                self.redraw_settings_panel();
                return;
            }
            SettingsAction::CycleSakuraVoice(d) => {
                self.cycle_voice_speaker(0, d);
                self.redraw_settings_panel();
                return;
            }
            SettingsAction::CycleKeroVoice(d) => {
                self.cycle_voice_speaker(1, d);
                self.redraw_settings_panel();
                return;
            }
            SettingsAction::CycleKeroArchetype(d) => {
                self.cycle_kero_archetype(d);
                self.redraw_settings_panel();
                return;
            }
            SettingsAction::SetVoiceSpeed(v) => {
                self.config_store.update(ConfigPatch {
                    voice_speed: Some(v.clamp(0.75, 1.35)),
                    ..Default::default()
                });
            }
            SettingsAction::SetVoiceIntonation(v) => {
                self.config_store.update(ConfigPatch {
                    voice_intonation: Some(v.clamp(0.70, 1.50)),
                    ..Default::default()
                });
            }
            SettingsAction::PreviewVoice(scope) => {
                self.preview_voice(scope);
                return;
            }
            SettingsAction::InstallAivmModel(uuid, name) => {
                self.spawn_install_aivm_model(uuid, name);
                return;
            }
            SettingsAction::UninstallAivmModel(uuid, name) => {
                self.spawn_uninstall_aivm_model(uuid, name);
                return;
            }
            SettingsAction::CycleProvider(d) => self.cycle_provider(d),
            SettingsAction::SetupLocalAi => {
                self.start_one_click_local_ai_setup();
                return;
            }
            SettingsAction::ToggleEnableTools => {
                self.config_store.update(ConfigPatch {
                    enable_tools: Some(!cur.enable_tools),
                    ..Default::default()
                });
            }
            SettingsAction::ToggleEnableClipboard => {
                self.config_store.update(ConfigPatch {
                    enable_clipboard_tool: Some(!cur.enable_clipboard_tool),
                    ..Default::default()
                });
            }
            SettingsAction::CycleAutoTalkInterval(d) => self.cycle_autotalk(d),
            SettingsAction::SetAutoTalkInterval(sec) => {
                self.config_store.update(ConfigPatch {
                    auto_talk_interval_sec: Some(sec),
                    ..Default::default()
                });
                self.auto_talk_last = Instant::now();
            }
            SettingsAction::SetInterestWeight(w) => {
                self.config_store.update(ConfigPatch {
                    interest_news_weight: Some(w),
                    ..Default::default()
                });
            }
            SettingsAction::ClearInterests => {
                self.config_store.clear_interests();
                let msg = self.ui_strings().status_clear_interests.to_string();
                self.apply_status_feedback(msg);
            }
            SettingsAction::OpenConfigFolder => {
                self.execute_menu_command_inner(None, MenuCommand::OpenConfigFolder);
                return;
            }
            SettingsAction::ClearHistory => {
                self.execute_menu_command_inner(None, MenuCommand::ClearHistory);
                return;
            }
            SettingsAction::ResetPositions => {
                self.reset_character_positions();
            }
            SettingsAction::ResetDefaults => {
                let reset_cfg = self.config_store.reset_convenience_and_appearance_defaults();
                self.pomodoro.set_preset(reset_cfg.pomodoro_preset_idx);
                self.apply_visibility_state();
                self.redraw_communicate_if_visible();
            }
        }
        self.apply_live_settings();
        self.redraw_settings_panel();
        self.rebuild_tray_menu();
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

        let sp_name = self.speaker_name(scope);
        let bal = match scope {
            Scope::Sakura => self.sakura_balloon.as_mut(),
            Scope::Kero => self.kero_balloon.as_mut(),
        };
        let Some(bal) = bal else { return };

        let prefer_left = match align.as_str() {
            "left" => true,
            "right" => false,
            _ => (char_x + char_w * 0.5) > (mx + mw * 0.45),
        };
        let new_side = if prefer_left { "left" } else { "right" }.to_string();
        if bal.side != new_side {
            bal.side = new_side;
            if let Some(ref gpu) = self.gpu {
                bal.redraw_content(&self.balloon_skin, &sp_name, false, &mut self.text_renderer, gpu);
            }
        }
        let bh = bal.win_size.1 as f64;
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
        let prev_shell_id = cfg.active_shell_id.clone();
        let switched_shell = shell_id.is_some();
        if let Some(ref sid) = shell_id {
            let loaded = self.shell_manager.load_shell(sid)?;
            let new_ds = loaded.descript.display_scale.clamp(0.5, 4.0);
            let next_binds = self
                .config_store
                .get_active_binds(sid, &loaded.default_bind_ids());
            *self.manifest.lock().expect("manifest mutex poisoned") = loaded;
            cfg = self.config_store.update(ConfigPatch {
                active_shell_id: Some(sid.clone()),
                ..Default::default()
            });
            let user_data_dir = self.config_store.config_dir.clone();
            let assets_dir = resolve_assets_dir();
            if let (Some(ref gpu), Some(ref mut sak)) = (&self.gpu, &mut self.sakura) {
                let mut m = self.manifest.lock().expect("manifest mutex poisoned");
                sak.display_scale = new_ds;
                sak.active_binds = next_binds.clone();
                sak.anim_next.clear();
                sak.reload_sidecar_rig(&m, &user_data_dir, &assets_dir, gpu);
                sak.set_surface(&mut m, gpu, 0, true);
            }
            if let (Some(ref gpu), Some(ref mut ker)) = (&self.gpu, &mut self.kero) {
                let mut m = self.manifest.lock().expect("manifest mutex poisoned");
                ker.display_scale = new_ds;
                ker.active_binds = next_binds;
                ker.anim_next.clear();
                ker.reload_sidecar_rig(&m, &user_data_dir, &assets_dir, gpu);
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
            let sp_sak = self.speaker_name(Scope::Sakura);
            let sp_ker = self.speaker_name(Scope::Kero);
            if let Some(ref gpu) = self.gpu {
                if let Some(ref mut sb) = self.sakura_balloon {
                    sb.redraw_content(&self.balloon_skin, &sp_sak, false, &mut self.text_renderer, gpu);
                }
                if let Some(ref mut kb) = self.kero_balloon {
                    kb.redraw_content(&self.balloon_skin, &sp_ker, false, &mut self.text_renderer, gpu);
                }
            }
        }
        if switched_shell {
            self.reset_character_positions();
        } else {
            self.reposition_balloon(Scope::Sakura);
            self.reposition_balloon(Scope::Kero);
        }
        self.apply_live_settings();
        self.redraw_settings_panel();
        self.rebuild_tray_menu();
        if switched_shell && !self.smoke_mode && shell_id.as_deref() != Some(prev_shell_id.as_str()) {
            let greet = self
                .engine
                .build_skin_switch_greeting(&prev_shell_id, &cfg.active_shell_id);
            let _ = self
                .proxy
                .send_event(UserEvent::PlayActions(parse_dialogue(&greet)));
        }
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
                    "\\0\\s[5]スキンを導入しました！（シェル {}個、バルーン {}個）\\e",
                    res.shells.len(),
                    res.balloons.len()
                );
                let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
            }
            Err(e) => {
                let msg = format!("\\0\\s[3]スキンの導入に失敗しました: {e}\\e");
                let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
            }
        }
    }

    fn build_menu_rows(&self) -> Vec<MenuRow> {
        let cfg = self.config_store.get();
        let ui_lang = crate::i18n::resolve_ui_lang(&cfg.ui_language);
        let ui_s = crate::i18n::ui_strings(ui_lang);
        let shells: Vec<ShellSummary> = self.shell_manager.list_shells();
        let balloons: Vec<BalloonSummary> = self.balloon_manager.list_balloons();
        let mut rows = vec![
            MenuRow {
                label: ui_s.menu_chat.to_string(),
                cmd: MenuCommand::OpenCommunicate,
            },
            MenuRow {
                label: ui_s.menu_ask_clipboard.to_string(),
                cmd: MenuCommand::AskClipboard,
            },
            MenuRow {
                label: ui_s.menu_quick_memos.to_string(),
                cmd: MenuCommand::ShowQuickMemos,
            },
            MenuRow {
                label: ui_s.menu_calendar_agenda.to_string(),
                cmd: MenuCommand::ShowCalendarAgenda,
            },
            MenuRow {
                label: ui_s.menu_settings.to_string(),
                cmd: MenuCommand::OpenSettings,
            },
            MenuRow {
                label: ui_s.menu_default_skin.to_string(),
                cmd: MenuCommand::ResetDefaultSkin,
            },
            MenuRow {
                label: String::new(),
                cmd: MenuCommand::Separator,
            },
        ];

        // 1) Skins & Dress-up collapsible section
        let skins_open = self.expanded_menu_section == Some(MenuSection::Skins);
        rows.push(MenuRow {
            label: format!(
                "{} {} ({})",
                if skins_open { "▾" } else { "▸" },
                ui_s.menu_sec_skins_prefix,
                cfg.active_shell_id
            ),
            cmd: MenuCommand::ToggleMenuSection(MenuSection::Skins),
        });
        if skins_open {
            rows.push(MenuRow {
                label: ui_s.menu_install_nar.to_string(),
                cmd: MenuCommand::InstallNarDialog,
            });
            rows.push(MenuRow {
                label: ui_s.menu_sec_shells.to_string(),
                cmd: MenuCommand::Separator,
            });
            for sh in shells {
                let mark = if cfg.active_shell_id == sh.id { "✓" } else { " " };
                rows.push(MenuRow {
                    label: format!("  {mark} {}", sh.name),
                    cmd: MenuCommand::SwitchShell(sh.id),
                });
            }
            rows.push(MenuRow {
                label: ui_s.menu_sec_balloons.to_string(),
                cmd: MenuCommand::Separator,
            });
            for b in balloons {
                let mark = if cfg.active_balloon_id == b.id { "✓" } else { " " };
                rows.push(MenuRow {
                    label: format!("  {mark} {}", b.name),
                    cmd: MenuCommand::SwitchBalloon(b.id),
                });
            }
            let (bind_groups, active_binds) = {
                let m = self.manifest.lock().expect("manifest mutex poisoned");
                let groups = m.effective_bind_groups();
                let active = self
                    .config_store
                    .get_active_binds(&cfg.active_shell_id, &m.default_bind_ids());
                (groups, active)
            };
            if !bind_groups.is_empty() {
                rows.push(MenuRow {
                    label: ui_s.menu_sec_bind.to_string(),
                    cmd: MenuCommand::Separator,
                });
                for bg in bind_groups {
                    let mark = if active_binds.contains(&bg.bind_id) {
                        "✓"
                    } else {
                        " "
                    };
                    let target_tag = if bg.scope == 1 { "\\1" } else { "\\0" };
                    rows.push(MenuRow {
                        label: format!("  {mark} ({target_tag}) {} / {}", bg.category, bg.name),
                        cmd: MenuCommand::ToggleBind(bg.bind_id),
                    });
                }
            }
        }

        // 2) Voice & TTS collapsible section
        let voice_open = self.expanded_menu_section == Some(MenuSection::Voice);
        rows.push(MenuRow {
            label: format!(
                "{} {} ({})",
                if voice_open { "▾" } else { "▸" },
                ui_s.menu_sec_voice_prefix,
                if cfg.voice_tts { "ON" } else { "OFF" }
            ),
            cmd: MenuCommand::ToggleMenuSection(MenuSection::Voice),
        });
        if voice_open {
            let (_eng_lbl, all_voices) = crate::companion_extras::cached_tts_voices();
            let (sak_sp, ker_sp, _) = self
                .config_store
                .get_shell_voice_profile(&cfg.active_shell_id);
            let (arch, _) = self.active_kero_voice_archetype();
            let resolved = crate::companion_extras::resolve_effective_voice_selection(
                &all_voices,
                arch,
                sak_sp,
                ker_sp,
            );
            rows.push(MenuRow {
                label: format!(
                    "  {} {}",
                    if cfg.voice_tts { "✓" } else { " " },
                    ui_s.menu_voice_tts
                ),
                cmd: MenuCommand::ToggleVoiceTts,
            });
            rows.push(MenuRow {
                label: format!("  {}", ui_s.menu_test_sakura_voice),
                cmd: MenuCommand::TestVoice(0),
            });
            rows.push(MenuRow {
                label: format!("  {}", ui_s.menu_test_kero_voice),
                cmd: MenuCommand::TestVoice(1),
            });
            rows.push(MenuRow {
                label: format!("  \\0: {} ↻", resolved.sakura_label),
                cmd: MenuCommand::CycleSakuraVoice,
            });
            rows.push(MenuRow {
                label: format!("  \\1: {} ↻", resolved.kero_label),
                cmd: MenuCommand::CycleKeroVoice,
            });
            rows.push(MenuRow {
                label: format!("  {}", ui_s.menu_manage_voices),
                cmd: MenuCommand::OpenSettingsTab(2),
            });
        }

        // 3) AI Providers collapsible section
        let ai_open = self.expanded_menu_section == Some(MenuSection::Ai);
        rows.push(MenuRow {
            label: format!(
                "{} {} ({})",
                if ai_open { "▾" } else { "▸" },
                ui_s.menu_sec_ai_prefix,
                cfg.provider
            ),
            cmd: MenuCommand::ToggleMenuSection(MenuSection::Ai),
        });
        if ai_open {
            rows.push(MenuRow {
                label: ui_s.menu_setup_local_ai.to_string(),
                cmd: MenuCommand::SetupLocalAi,
            });
            for p in provider_catalog() {
                let mark = if cfg.provider == p.id { "✓" } else { " " };
                let plbl = crate::i18n::provider_label(ui_lang, &p.id, &p.label);
                rows.push(MenuRow {
                    label: format!("  {mark} {plbl}"),
                    cmd: MenuCommand::SwitchProvider(p.id, p.default_model),
                });
            }
        }

        // 4) Tools & Language collapsible section
        let tools_open = self.expanded_menu_section == Some(MenuSection::Tools);
        rows.push(MenuRow {
            label: format!(
                "{} {}",
                if tools_open { "▾" } else { "▸" },
                ui_s.menu_sec_tools
            ),
            cmd: MenuCommand::ToggleMenuSection(MenuSection::Tools),
        });
        if tools_open {
            rows.push(MenuRow {
                label: crate::i18n::format_menu_ui_lang_row(ui_lang, &cfg.ui_language, false),
                cmd: MenuCommand::CycleUiLanguage,
            });
            rows.push(MenuRow {
                label: format!(
                    "  {} {}",
                    if cfg.quiet_mode { "✓" } else { " " },
                    ui_s.menu_quiet_mode
                ),
                cmd: MenuCommand::ToggleQuietMode,
            });
            rows.push(MenuRow {
                label: self.pomodoro.format_menu_label(ui_lang, false),
                cmd: MenuCommand::TogglePomodoro,
            });
            if self.pomodoro.is_active() {
                rows.push(MenuRow {
                    label: format!("  {}", ui_s.menu_pomo_skip),
                    cmd: MenuCommand::SkipPomodoro,
                });
                rows.push(MenuRow {
                    label: format!("  {}", ui_s.menu_pomo_stop),
                    cmd: MenuCommand::StopPomodoro,
                });
            }
            rows.push(MenuRow {
                label: format!("  {}", ui_s.menu_show_backlog),
                cmd: MenuCommand::ShowBacklog,
            });
            rows.push(MenuRow {
                label: format!("  {}", ui_s.menu_copy_last),
                cmd: MenuCommand::CopyLastReply,
            });
            rows.push(MenuRow {
                label: format!("  {}", ui_s.menu_next_monitor),
                cmd: MenuCommand::MoveNextMonitor,
            });
            rows.push(MenuRow {
                label: format!(
                    "  {}",
                    if self.characters_hidden_by_user {
                        ui_s.menu_show_chars
                    } else {
                        ui_s.menu_hide_chars
                    }
                ),
                cmd: MenuCommand::ToggleHideCharacters,
            });
            rows.push(MenuRow {
                label: crate::i18n::format_menu_autotalk_row(
                    ui_lang,
                    cfg.auto_talk_interval_sec,
                    false,
                ),
                cmd: MenuCommand::CycleAutoTalk,
            });
            rows.push(MenuRow {
                label: format!(
                    "  {} {}",
                    if cfg.hourly_chime { "✓" } else { " " },
                    ui_s.menu_hourly_chime
                ),
                cmd: MenuCommand::ToggleHourlyChime,
            });
            rows.push(MenuRow {
                label: format!("  {}", ui_s.menu_l2d_studio),
                cmd: MenuCommand::OpenLive2dStudio,
            });
            rows.push(MenuRow {
                label: format!("  {}", ui_s.menu_l2d_folder),
                cmd: MenuCommand::OpenLive2dFolder,
            });
            rows.push(MenuRow {
                label: format!("  {}", ui_s.menu_check_updates),
                cmd: MenuCommand::CheckUpdates,
            });
            rows.push(MenuRow {
                label: format!("  {}", ui_s.menu_clear_history),
                cmd: MenuCommand::ClearHistory,
            });
        }

        rows.push(MenuRow {
            label: String::new(),
            cmd: MenuCommand::Separator,
        });
        rows.push(MenuRow {
            label: format!(
                "{} {}",
                if cfg.show_kero { "✓" } else { "  " },
                ui_s.menu_kero
            ),
            cmd: MenuCommand::ToggleKero,
        });
        rows.push(MenuRow {
            label: format!(
                "{} {}",
                if cfg.always_on_top { "✓" } else { "  " },
                ui_s.menu_always_on_top
            ),
            cmd: MenuCommand::ToggleAlwaysOnTop,
        });
        rows.push(MenuRow {
            label: format!(
                "{} {}",
                if cfg.live2d_motion { "✓" } else { "  " },
                ui_s.menu_live2d_motion
            ),
            cmd: MenuCommand::ToggleLive2dMotion,
        });
        rows.push(MenuRow {
            label: ui_s.menu_reset_pos.to_string(),
            cmd: MenuCommand::ResetPositions,
        });
        rows.push(MenuRow {
            label: ui_s.menu_open_cfg_folder.to_string(),
            cmd: MenuCommand::OpenConfigFolder,
        });
        rows.push(MenuRow {
            label: ui_s.menu_quit.to_string(),
            cmd: MenuCommand::Quit,
        });
        rows
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn rebuild_tray_menu(&mut self) {
        let Some(ref tray) = self.tray else { return };
        let (menu, map) = self.create_tray_menu_model();
        self.tray_actions = map;
        tray.set_menu(Some(Box::new(menu)));
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn rebuild_tray_menu(&mut self) {}

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn create_tray_menu_model(&self) -> (Menu, HashMap<MenuId, MenuCommand>) {
        let cfg = self.config_store.get();
        let ui_lang = crate::i18n::resolve_ui_lang(&cfg.ui_language);
        let ui_s = crate::i18n::ui_strings(ui_lang);
        let menu = Menu::new();
        let mut map = HashMap::new();

        let chat_item = MenuItem::new(ui_s.tray_chat, true, None);
        map.insert(chat_item.id().clone(), MenuCommand::OpenCommunicate);
        let _ = menu.append(&chat_item);

        let settings_item = MenuItem::new(ui_s.menu_settings, true, None);
        map.insert(settings_item.id().clone(), MenuCommand::OpenSettings);
        let _ = menu.append(&settings_item);

        let hide_lbl = if self.characters_hidden_by_user {
            ui_s.menu_show_chars
        } else {
            ui_s.menu_hide_chars
        };
        let hide_item = MenuItem::new(hide_lbl, true, None);
        map.insert(hide_item.id().clone(), MenuCommand::ToggleHideCharacters);
        let _ = menu.append(&hide_item);

        let quiet_item = MenuItem::new(
            format!(
                "{}{}",
                if cfg.quiet_mode { "✓ " } else { "   " },
                ui_s.menu_quiet_mode
            ),
            true,
            None,
        );
        map.insert(quiet_item.id().clone(), MenuCommand::ToggleQuietMode);
        let _ = menu.append(&quiet_item);

        let reset_skin_item = MenuItem::new(ui_s.menu_default_skin, true, None);
        map.insert(reset_skin_item.id().clone(), MenuCommand::ResetDefaultSkin);
        let _ = menu.append(&reset_skin_item);

        let _ = menu.append(&PredefinedMenuItem::separator());

        let mut add_item = |target: &Submenu, label: &str, cmd: MenuCommand| {
            let item = MenuItem::new(label, true, None);
            map.insert(item.id().clone(), cmd);
            let _ = target.append(&item);
        };

        // 1) Skins Submenu (Shells, Balloons, Dress-up, Install .nar)
        let skins_sub = Submenu::new(
            format!("{} ({})", ui_s.menu_sec_skins_prefix, cfg.active_shell_id),
            true,
        );
        add_item(
            &skins_sub,
            ui_s.menu_install_nar.trim_start(),
            MenuCommand::InstallNarDialog,
        );
        let shell_sub = Submenu::new(ui_s.tray_sub_shells, true);
        for sh in self.shell_manager.list_shells() {
            let mark = if cfg.active_shell_id == sh.id { "✓ " } else { "   " };
            add_item(&shell_sub, &format!("{mark}{}", sh.name), MenuCommand::SwitchShell(sh.id));
        }
        let _ = skins_sub.append(&shell_sub);

        let bal_sub = Submenu::new(ui_s.tray_sub_balloons, true);
        for b in self.balloon_manager.list_balloons() {
            let mark = if cfg.active_balloon_id == b.id { "✓ " } else { "   " };
            add_item(&bal_sub, &format!("{mark}{}", b.name), MenuCommand::SwitchBalloon(b.id));
        }
        let _ = skins_sub.append(&bal_sub);

        let (bind_groups, active_binds) = {
            let m = self.manifest.lock().expect("manifest mutex poisoned");
            let groups = m.effective_bind_groups();
            let active = self
                .config_store
                .get_active_binds(&cfg.active_shell_id, &m.default_bind_ids());
            (groups, active)
        };
        if !bind_groups.is_empty() {
            let bind_sub = Submenu::new(ui_s.tray_sub_bind, true);
            for bg in bind_groups {
                let mark = if active_binds.contains(&bg.bind_id) {
                    "✓ "
                } else {
                    "   "
                };
                let target_tag = if bg.scope == 1 { "\\1" } else { "\\0" };
                add_item(
                    &bind_sub,
                    &format!("{mark}({target_tag}) {} / {}", bg.category, bg.name),
                    MenuCommand::ToggleBind(bg.bind_id),
                );
            }
            let _ = skins_sub.append(&bind_sub);
        }
        let _ = menu.append(&skins_sub);

        // 2) Voice & TTS Submenu
        let voice_sub = Submenu::new(
            format!(
                "{} ({})",
                ui_s.menu_sec_voice_prefix,
                if cfg.voice_tts { "ON" } else { "OFF" }
            ),
            true,
        );
        add_item(
            &voice_sub,
            &format!(
                "{}{}",
                if cfg.voice_tts { "✓ " } else { "   " },
                ui_s.menu_voice_tts
            ),
            MenuCommand::ToggleVoiceTts,
        );
        {
            let (eng_lbl, all_voices) = crate::companion_extras::cached_tts_voices();
            let base_url = crate::companion_extras::cached_tts_base_url();
            let installed_uuids = crate::companion_extras::cached_installed_aivm_uuids();
            let (sak_sp, ker_sp, arch_override) = self
                .config_store
                .get_shell_voice_profile(&cfg.active_shell_id);
            let (arch, _) = self.active_kero_voice_archetype();
            let resolved = crate::companion_extras::resolve_effective_voice_selection(
                &all_voices,
                arch,
                sak_sp,
                ker_sp,
            );

            let eng_sub = Submenu::new(ui_s.tray_sub_engine, true);
            for (eid, _) in TTS_ENGINE_CHOICES {
                let mark = if cfg.tts_engine == *eid { "✓ " } else { "   " };
                let elbl = crate::i18n::tts_engine_label(ui_lang, eid);
                add_item(
                    &eng_sub,
                    &format!("{mark}{elbl}"),
                    MenuCommand::SelectTtsEngine((*eid).to_string()),
                );
            }
            let _ = voice_sub.append(&eng_sub);

            let arch_sub = Submenu::new(ui_s.tray_sub_arch, true);
            add_item(
                &arch_sub,
                &format!(
                    "{}{} ({})",
                    if arch_override.is_none() { "✓ " } else { "   " },
                    ui_s.voice_auto_prefix,
                    crate::i18n::kero_archetype_label(ui_lang, arch.short_code())
                ),
                MenuCommand::SelectKeroVoiceArchetype(None),
            );
            for (code, _) in KERO_ARCHETYPE_CHOICES {
                let mark = if arch_override.as_deref() == Some(*code) {
                    "✓ "
                } else {
                    "   "
                };
                let albl = crate::i18n::kero_archetype_label(ui_lang, code);
                add_item(
                    &arch_sub,
                    &format!("{mark}{albl}"),
                    MenuCommand::SelectKeroVoiceArchetype(Some((*code).to_string())),
                );
            }
            let _ = voice_sub.append(&arch_sub);

            if !all_voices.is_empty() {
                let choices = Self::representative_voice_options(&all_voices, 20);
                let sak_voice_sub = Submenu::new(
                    format!("{} ({eng_lbl})", ui_s.tray_sub_sak_voice),
                    true,
                );
                add_item(
                    &sak_voice_sub,
                    &format!(
                        "{}{} ({})",
                        if sak_sp.is_none() { "✓ " } else { "   " },
                        ui_s.voice_auto_prefix,
                        resolved.sakura_label
                    ),
                    MenuCommand::SelectVoiceSpeaker {
                        scope: 0,
                        style_id: None,
                    },
                );
                for v in &choices {
                    let mark = if sak_sp == Some(v.style_id) { "✓ " } else { "   " };
                    add_item(
                        &sak_voice_sub,
                        &format!("{mark}{}", v.label),
                        MenuCommand::SelectVoiceSpeaker {
                            scope: 0,
                            style_id: Some(v.style_id),
                        },
                    );
                }
                let _ = voice_sub.append(&sak_voice_sub);

                let ker_voice_sub = Submenu::new(
                    format!("{} ({eng_lbl})", ui_s.tray_sub_ker_voice),
                    true,
                );
                add_item(
                    &ker_voice_sub,
                    &format!(
                        "{}{} ({})",
                        if ker_sp.is_none() { "✓ " } else { "   " },
                        ui_s.voice_auto_prefix,
                        resolved.kero_label
                    ),
                    MenuCommand::SelectVoiceSpeaker {
                        scope: 1,
                        style_id: None,
                    },
                );
                for v in &choices {
                    let mark = if ker_sp == Some(v.style_id) { "✓ " } else { "   " };
                    add_item(
                        &ker_voice_sub,
                        &format!("{mark}{}", v.label),
                        MenuCommand::SelectVoiceSpeaker {
                            scope: 1,
                            style_id: Some(v.style_id),
                        },
                    );
                }
                let _ = voice_sub.append(&ker_voice_sub);
            }

            let show_aivis_catalog = cfg.tts_engine == "auto"
                || cfg.tts_engine == "aivisspeech"
                || base_url.as_deref().is_some_and(|u| u.contains(":10101"));
            if show_aivis_catalog {
                let hub_sub = Submenu::new(ui_s.tray_sub_aivis, true);
                for m in crate::companion_extras::CURATED_AIVISHUB_MODELS {
                    let installed = installed_uuids
                        .iter()
                        .any(|u| u.eq_ignore_ascii_case(m.uuid));
                    let mark = if installed { "✓ " } else { "   " };
                    let suffix = if installed { ui_s.suffix_installed } else { "" };
                    add_item(
                        &hub_sub,
                        &format!("{mark}{}{suffix}", m.menu_label),
                        MenuCommand::InstallAivmModel {
                            uuid: m.uuid.to_string(),
                            name: m.name.to_string(),
                        },
                    );
                }
                let _ = voice_sub.append(&hub_sub);
            }
        }
        let _ = menu.append(&voice_sub);

        // 3) AI Providers Submenu
        let prov_sub = Submenu::new(
            format!("{} ({})", ui_s.menu_sec_ai_prefix, cfg.provider),
            true,
        );
        add_item(
            &prov_sub,
            ui_s.menu_setup_local_ai.trim_start(),
            MenuCommand::SetupLocalAi,
        );
        let _ = prov_sub.append(&PredefinedMenuItem::separator());
        for p in provider_catalog() {
            let mark = if cfg.provider == p.id { "✓ " } else { "   " };
            let plbl = crate::i18n::provider_label(ui_lang, &p.id, &p.label);
            add_item(
                &prov_sub,
                &format!("{mark}{plbl}"),
                MenuCommand::SwitchProvider(p.id, p.default_model),
            );
        }
        let _ = menu.append(&prov_sub);

        // 4) View, Language & Tools Submenu (also accessible while Click-Through is active)
        let view_sub = Submenu::new(ui_s.tray_sec_view_tools, true);
        add_item(
            &view_sub,
            &format!("{}{}", if cfg.show_kero { "✓ " } else { "   " }, ui_s.menu_kero),
            MenuCommand::ToggleKero,
        );
        add_item(
            &view_sub,
            &format!(
                "{}{}",
                if cfg.always_on_top { "✓ " } else { "   " },
                ui_s.menu_always_on_top
            ),
            MenuCommand::ToggleAlwaysOnTop,
        );
        add_item(
            &view_sub,
            &format!(
                "{}{}",
                if cfg.live2d_motion { "✓ " } else { "   " },
                ui_s.menu_live2d_motion
            ),
            MenuCommand::ToggleLive2dMotion,
        );
        add_item(
            &view_sub,
            &format!(
                "{}{}",
                if cfg.click_through_mode { "✓ " } else { "   " },
                ui_s.menu_click_through
            ),
            MenuCommand::ToggleClickThrough,
        );
        add_item(
            &view_sub,
            &crate::i18n::format_menu_ui_lang_row(ui_lang, &cfg.ui_language, true),
            MenuCommand::CycleUiLanguage,
        );
        add_item(
            &view_sub,
            &self.pomodoro.format_menu_label(ui_lang, true),
            MenuCommand::TogglePomodoro,
        );
        if self.pomodoro.is_active() {
            add_item(&view_sub, ui_s.menu_pomo_stop, MenuCommand::StopPomodoro);
        }
        add_item(&view_sub, ui_s.menu_show_backlog, MenuCommand::ShowBacklog);
        add_item(&view_sub, ui_s.menu_copy_last, MenuCommand::CopyLastReply);
        add_item(&view_sub, ui_s.menu_next_monitor, MenuCommand::MoveNextMonitor);
        add_item(
            &view_sub,
            &crate::i18n::format_menu_autotalk_row(
                ui_lang,
                cfg.auto_talk_interval_sec,
                true,
            ),
            MenuCommand::CycleAutoTalk,
        );
        add_item(
            &view_sub,
            &format!(
                "{}{}",
                if cfg.hourly_chime { "✓ " } else { "   " },
                ui_s.menu_hourly_chime
            ),
            MenuCommand::ToggleHourlyChime,
        );
        add_item(&view_sub, ui_s.menu_l2d_studio, MenuCommand::OpenLive2dStudio);
        add_item(&view_sub, ui_s.menu_l2d_folder, MenuCommand::OpenLive2dFolder);
        add_item(
            &view_sub,
            ui_s.menu_open_cfg_folder,
            MenuCommand::OpenConfigFolder,
        );
        add_item(&view_sub, ui_s.menu_check_updates, MenuCommand::CheckUpdates);
        add_item(&view_sub, ui_s.menu_clear_history, MenuCommand::ClearHistory);
        let _ = menu.append(&view_sub);

        let _ = menu.append(&PredefinedMenuItem::separator());

        let reset_item = MenuItem::new(ui_s.menu_reset_pos, true, None);
        map.insert(reset_item.id().clone(), MenuCommand::ResetPositions);
        let _ = menu.append(&reset_item);

        let quit_item = MenuItem::new(ui_s.menu_quit, true, None);
        map.insert(quit_item.id().clone(), MenuCommand::Quit);
        let _ = menu.append(&quit_item);

        (menu, map)
    }

    fn preview_voice(&mut self, scope: u8) {
        if self.smoke_mode {
            return;
        }
        let cfg = self.config_store.get();
        let ui_lang = self.effective_ui_lang();
        let (sak_sp, ker_sp, _) = self
            .config_store
            .get_shell_voice_profile(&cfg.active_shell_id);
        let (arch, _) = self.active_kero_voice_archetype();
        let sample_text = match (scope, ui_lang) {
            (1, crate::i18n::UiLang::Ko) => "야, 내 목소리 잘 들리나? 포모도로랑 잡담 맡겨만 둬라!",
            (1, crate::i18n::UiLang::En) => "Hey, how does my voice sound? Ready whenever you are!",
            (1, crate::i18n::UiLang::Ja) => "おう、わしの声はこんな感じやで！ちょうどええやろ？",
            (_, crate::i18n::UiLang::Ko) => "안녕하세요! 이 목소리로 이야기해 드릴까요?",
            (_, crate::i18n::UiLang::En) => "Hello! How does this voice sound to you?",
            (_, crate::i18n::UiLang::Ja) => "こんにちは！この声でゆっくりお話ししようね。",
        };
        crate::companion_extras::speak_dialogue_batch_with_prosody_async(
            vec![(scope, sample_text.to_string())],
            &cfg.voicevox_url,
            &cfg.tts_engine,
            arch,
            sak_sp,
            ker_sp,
            cfg.voice_speed,
            cfg.voice_intonation,
        );
    }

    fn spawn_install_aivm_model(&mut self, uuid: String, name: String) {
        let busy = format!("⬇ 「{name}」を導入中…");
        self.aivm_busy_label = Some(busy.clone());
        self.apply_status_feedback(busy);
        self.redraw_settings_panel();
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let base_opt = crate::companion_extras::cached_tts_base_url()
                .filter(|u| u.contains(":10101"))
                .or_else(|| {
                    crate::companion_extras::ensure_tts_engine_ready_with_mode_blocking(
                        "http://127.0.0.1:10101",
                        "aivisspeech",
                        true,
                    )
                    .base_url
                    .filter(|u| u.contains(":10101"))
                });
            let (status, msg) = match base_opt {
                Some(base) => match crate::companion_extras::install_aivm_model_blocking(&base, &uuid) {
                    Ok(_) => (
                        format!("✓ 「{name}」を導入しました"),
                        format!(
                            "\\0\\s[5]AivisHub から音声モデル「{name}」を導入したよ！\\1\\s[10]自動/手動ですぐ使えるようになったで！\\e"
                        ),
                    ),
                    Err(e) => (
                        format!("✕ 導入失敗: {e}"),
                        format!("\\0\\s[3]音声モデル「{name}」の導入に失敗しました: {e}\\e"),
                    ),
                },
                None => (
                    "AivisSpeech 未起動".to_string(),
                    "\\0\\s[3]AivisSpeech (port 10101) が起動していないため、AivisHub モデルを導入できません。\\e".to_string(),
                ),
            };
            let _ = proxy.send_event(UserEvent::AivmActionDone { status });
            let _ = proxy.send_event(UserEvent::RefreshMenus);
            let _ = proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
        });
    }

    fn spawn_uninstall_aivm_model(&mut self, uuid: String, name: String) {
        let busy = format!("🗑 「{name}」を削除中…");
        self.aivm_busy_label = Some(busy.clone());
        self.apply_status_feedback(busy);
        self.redraw_settings_panel();
        let proxy = self.proxy.clone();
        let store = self.config_store.clone();
        let active_shell = self.config_store.get().active_shell_id;
        std::thread::spawn(move || {
            let base_opt = crate::companion_extras::cached_tts_base_url()
                .filter(|u| u.contains(":10101"))
                .or_else(|| {
                    crate::companion_extras::ensure_tts_engine_ready_with_mode_blocking(
                        "http://127.0.0.1:10101",
                        "aivisspeech",
                        false,
                    )
                    .base_url
                    .filter(|u| u.contains(":10101"))
                });
            let (status, msg) = match base_opt {
                Some(base) => match crate::companion_extras::uninstall_aivm_model_blocking(&base, &uuid) {
                    Ok(_) => {
                        let (_, remaining_voices) = crate::companion_extras::cached_tts_voices();
                        let (sak_sp, ker_sp, _) = store.get_shell_voice_profile(&active_shell);
                        if let Some(sid) = sak_sp {
                            if !remaining_voices.iter().any(|v| v.style_id == sid) {
                                store.set_shell_voice_speaker(&active_shell, 0, None);
                            }
                        }
                        if let Some(kid) = ker_sp {
                            if !remaining_voices.iter().any(|v| v.style_id == kid) {
                                store.set_shell_voice_speaker(&active_shell, 1, None);
                            }
                        }
                        (
                            format!("✓ 「{name}」を削除しました"),
                            format!(
                                "\\0\\s[0]音声モデル「{name}」をアンインストールしたよ！\\1\\s[10]スッキリしたな！\\e"
                            ),
                        )
                    }
                    Err(e) => (
                        format!("✕ 削除失敗: {e}"),
                        format!("\\0\\s[3]音声モデル「{name}」の削除に失敗しました: {e}\\e"),
                    ),
                },
                None => (
                    "AivisSpeech 未起動".to_string(),
                    "\\0\\s[3]AivisSpeech (port 10101) に接続できないため削除できません。\\e".to_string(),
                ),
            };
            let _ = proxy.send_event(UserEvent::AivmActionDone { status });
            let _ = proxy.send_event(UserEvent::RefreshMenus);
            let _ = proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
        });
    }

    fn show_quick_memos_in_balloon(&mut self) {
        let store = crate::accessory_utils::MemoStore::new(&self.config_store.config_dir);
        let script = store.format_balloon_script(self.effective_ui_lang());
        let _ = self
            .proxy
            .send_event(UserEvent::PlayActionsNoBacklog(parse_dialogue(&script)));
    }

    fn show_calendar_agenda_in_balloon(&mut self) {
        let cfg = self.config_store.get();
        let connected = cfg.calendar_use_os_native || !cfg.calendar_ics_urls.is_empty();
        let evs = self
            .calendar_state
            .lock()
            .map(|st| st.events.clone())
            .unwrap_or_default();
        let script = crate::calendar_sync::format_agenda_balloon_script(
            &evs,
            connected,
            cfg.calendar_prep_mins,
            self.effective_ui_lang(),
        );
        let _ = self
            .proxy
            .send_event(UserEvent::PlayActionsNoBacklog(parse_dialogue(&script)));
    }

    fn paste_calendar_ics_from_clipboard(&mut self) {
        let ui_lang = self.effective_ui_lang();
        let clip = arboard::Clipboard::new()
            .and_then(|mut c| c.get_text())
            .unwrap_or_default();
        match crate::calendar_sync::validate_calendar_ics_url(&clip) {
            Ok(valid) => {
                let label = crate::calendar_sync::detect_calendar_source_label(&valid);
                let mut urls = self.config_store.get().calendar_ics_urls;
                if !urls.iter().any(|u| u == &valid) {
                    urls.push(valid);
                }
                self.config_store.update(ConfigPatch {
                    calendar_enabled: Some(true),
                    calendar_ics_urls: Some(urls),
                    ..Default::default()
                });
                let msg = match ui_lang {
                    crate::i18n::UiLang::Ko => format!("\\0\\s[5]{label} 캘린더를 연결했어요! 일정을 동기화하고 있어요…\\e"),
                    crate::i18n::UiLang::En => format!("\\0\\s[5]Connected {label} Calendar! Syncing your schedule…\\e"),
                    crate::i18n::UiLang::Ja => format!("\\0\\s[5]{label}カレンダーを連携したよ！予定を同期中…\\e"),
                };
                let _ = self
                    .proxy
                    .send_event(UserEvent::PlayActionsNoBacklog(parse_dialogue(&msg)));
                self.spawn_calendar_sync(true);
            }
            Err(e) => {
                let msg = match ui_lang {
                    crate::i18n::UiLang::Ko => format!("\\0\\s[3]클립보드에 올바른 https:// 또는 webcal:// .ics 주소가 없어요: {e}\\e"),
                    crate::i18n::UiLang::En => format!("\\0\\s[3]Clipboard doesn't contain a valid https:// or webcal:// .ics URL: {e}\\e"),
                    crate::i18n::UiLang::Ja => format!("\\0\\s[3]クリップボードに有効な https:// または webcal:// の .ics URL がありません: {e}\\e"),
                };
                let _ = self
                    .proxy
                    .send_event(UserEvent::PlayActionsNoBacklog(parse_dialogue(&msg)));
            }
        }
    }

    fn spawn_calendar_sync(&mut self, notify_status: bool) {
        if self.smoke_mode {
            return;
        }
        self.calendar_last_sync = Instant::now();
        let cfg = self.config_store.get();
        let config_dir = self.config_store.config_dir.clone();
        let cal_arc = self.calendar_state.clone();
        let proxy = self.proxy.clone();
        let ui_lang = self.effective_ui_lang();
        if notify_status {
            let busy = match ui_lang {
                crate::i18n::UiLang::Ko => "📅 캘린더 동기화 중…",
                crate::i18n::UiLang::En => "📅 Syncing calendars…",
                crate::i18n::UiLang::Ja => "📅 カレンダー同期中…",
            };
            self.apply_status_feedback(busy.to_string());
        }
        std::thread::spawn(move || {
            let mut geo_cache = cal_arc
                .lock()
                .map(|g| g.geocode_cache.clone())
                .unwrap_or_default();
            let res = crate::calendar_sync::sync_all_calendars_blocking(
                &cfg.calendar_ics_urls,
                cfg.calendar_use_os_native,
                &config_dir,
                &cfg.calendar_home_location,
                cfg.calendar_default_travel_mins,
                &mut geo_cache,
            );
            let status = match res {
                Ok(evs) => {
                    let cnt = evs.len();
                    if let Ok(mut g) = cal_arc.lock() {
                        g.events = evs;
                        g.last_sync_ts = chrono::Utc::now().timestamp();
                        g.last_error = None;
                        g.geocode_cache = geo_cache;
                        g.save_to_disk(&config_dir);
                    }
                    match ui_lang {
                        crate::i18n::UiLang::Ko => format!("📅 캘린더 동기화 완료 (예정 {cnt}건)"),
                        crate::i18n::UiLang::En => format!("📅 Calendar synced ({cnt} upcoming)"),
                        crate::i18n::UiLang::Ja => format!("📅 カレンダー同期完了（予定 {cnt}件）"),
                    }
                }
                Err(e) => {
                    if let Ok(mut g) = cal_arc.lock() {
                        g.last_error = Some(e.clone());
                    }
                    format!("📅 同期エラー: {e}")
                }
            };
            if notify_status {
                let _ = proxy.send_event(UserEvent::CalendarSyncDone { status });
            } else {
                let _ = proxy.send_event(UserEvent::RefreshMenus);
            }
        });
    }

    fn ask_clipboard_ai(&mut self) {
        if self.smoke_mode {
            return;
        }
        let ui_lang = self.effective_ui_lang();
        let clip = arboard::Clipboard::new()
            .and_then(|mut c| c.get_text())
            .unwrap_or_default();
        match crate::accessory_utils::sanitize_clipboard_for_ai(&clip) {
            Ok(wrapped) => {
                let prompt = match ui_lang {
                    crate::i18n::UiLang::Ko => format!(
                        "다음 클립보드 내용을 핵심만 간결하게 요약·설명하거나 외국어라면 자연스럽게 번역해줘:\n{wrapped}"
                    ),
                    crate::i18n::UiLang::En => format!(
                        "Briefly summarize, explain, or translate the following clipboard text:\n{wrapped}"
                    ),
                    crate::i18n::UiLang::Ja => format!(
                        "次のクリップボード内容を要点を絞って簡潔に要約・解説するか、外国語なら自然な日本語に翻訳して:\n{wrapped}"
                    ),
                };
                self.spawn_user_chat(prompt);
            }
            Err(err_key) => {
                let msg = match (err_key.as_str(), ui_lang) {
                    ("empty", crate::i18n::UiLang::Ko) => {
                        "\\0\\s[4]클립보드가 비어 있어요. 텍스트를 복사한 뒤 다시 눌러 주세요!\\e"
                    }
                    ("empty", crate::i18n::UiLang::En) => {
                        "\\0\\s[4]Your clipboard is empty. Copy some text first!\\e"
                    }
                    ("empty", _) => {
                        "\\0\\s[4]クリップボードが空っぽだよ。テキストをコピーしてからもう一度試してね！\\e"
                    }
                    (_, crate::i18n::UiLang::Ko) => {
                        "\\0\\s[3]API 키나 비밀 키로 보이는 내용이 감지되어 보안을 위해 AI 전송을 차단했어요!\\e"
                    }
                    (_, crate::i18n::UiLang::En) => {
                        "\\0\\s[3]Clipboard looks like a secret/API key, so sending to AI was blocked for safety!\\e"
                    }
                    (_, _) => {
                        "\\0\\s[3]APIキーや秘密鍵らしき文字列を検知したため、安全のためAI送信をブロックしたよ！\\e"
                    }
                };
                let _ = self
                    .proxy
                    .send_event(UserEvent::PlayActionsNoBacklog(parse_dialogue(msg)));
            }
        }
    }

    fn execute_menu_command(&mut self, event_loop: &ActiveEventLoop, cmd: MenuCommand) {
        self.execute_menu_command_inner(Some(event_loop), cmd);
    }

    fn execute_menu_command_inner(
        &mut self,
        event_loop: Option<&ActiveEventLoop>,
        cmd: MenuCommand,
    ) {
        match cmd {
            MenuCommand::Separator => {}
            MenuCommand::OpenCommunicate => {
                self.open_communicate_near_sakura();
            }
            MenuCommand::OpenSettings => {
                self.open_settings_panel();
            }
            MenuCommand::OpenSettingsTab(tab) => {
                self.open_settings_panel();
                if let Some(ref mut sw) = self.settings {
                    sw.model.select_tab(tab);
                }
                self.redraw_settings_panel();
            }
            MenuCommand::AskClipboard => {
                self.ask_clipboard_ai();
            }
            MenuCommand::ShowQuickMemos => {
                self.show_quick_memos_in_balloon();
            }
            MenuCommand::ShowCalendarAgenda => {
                self.show_calendar_agenda_in_balloon();
            }
            MenuCommand::ToggleMenuSection(sec) => {
                self.expanded_menu_section = if self.expanded_menu_section == Some(sec) {
                    None
                } else {
                    Some(sec)
                };
            }
            MenuCommand::OpenConfigFolder => {
                let dir = self.config_store.config_dir.clone();
                let _ = std::fs::create_dir_all(&dir);
                if !self.smoke_mode {
                    let _ = open_local_path_in_os(&dir);
                }
            }
            MenuCommand::ToggleClickThrough => {
                let cur = self.config_store.get().click_through_mode;
                let updated = self.config_store.update(ConfigPatch {
                    click_through_mode: Some(!cur),
                    ..Default::default()
                });
                self.apply_live_settings();
                self.redraw_settings_panel();
                self.rebuild_tray_menu();
                if !self.smoke_mode {
                    let msg = if updated.click_through_mode {
                        "\\0\\s[5]すり抜けモード（クリック透過）を ON にしたよ！解除はトレイアイコンまたは設定パネルから行えるよ。\\e"
                    } else {
                        "\\0\\s[5]すり抜けモードを解除したよ！またドラッグして動かせるよ。\\e"
                    };
                    let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(msg)));
                }
            }
            MenuCommand::ResetDefaultSkin => {
                let _ = self.apply_skin_switch(
                    Some("neo_default".to_string()),
                    Some("clean_default".to_string()),
                );
                self.redraw_settings_panel();
            }
            MenuCommand::SwitchShell(sid) => {
                let _ = self.apply_skin_switch(Some(sid), None);
                self.redraw_settings_panel();
            }
            MenuCommand::SwitchBalloon(bid) => {
                let _ = self.apply_skin_switch(None, Some(bid));
                self.redraw_settings_panel();
            }
            MenuCommand::SwitchProvider(pid, model) => {
                let _ = self.handle_provider_switch(pid, Some(model));
                self.redraw_settings_panel();
            }
            MenuCommand::SetupLocalAi => {
                self.start_one_click_local_ai_setup();
            }
            MenuCommand::ToggleKero => {
                let cur = self.config_store.get().show_kero;
                self.config_store.update(ConfigPatch {
                    show_kero: Some(!cur),
                    ..Default::default()
                });
                self.apply_live_settings();
                self.redraw_settings_panel();
                self.rebuild_tray_menu();
            }
            MenuCommand::ToggleAlwaysOnTop => {
                let cur = self.config_store.get().always_on_top;
                self.config_store.update(ConfigPatch {
                    always_on_top: Some(!cur),
                    ..Default::default()
                });
                self.apply_live_settings();
                self.redraw_settings_panel();
                self.rebuild_tray_menu();
            }
            MenuCommand::ToggleLive2dMotion => {
                let cur = self.config_store.get().live2d_motion;
                self.config_store.update(ConfigPatch {
                    live2d_motion: Some(!cur),
                    ..Default::default()
                });
                self.apply_live_settings();
                self.redraw_settings_panel();
                self.rebuild_tray_menu();
            }
            MenuCommand::OpenLive2dStudio => {
                if !self.smoke_mode {
                    let studio_path = resolve_tools_dir().join("live2d_studio.html");
                    if studio_path.is_file() {
                        let _ = open_local_path_in_os(&studio_path);
                    } else {
                        self.apply_status_feedback(
                            "tools/live2d_studio.html が見つかりませんでした。".to_string(),
                        );
                    }
                }
            }
            MenuCommand::OpenLive2dFolder => {
                let shell_id = self.config_store.get().active_shell_id;
                let dir = self.config_store.config_dir.join("live2d").join(&shell_id);
                let _ = std::fs::create_dir_all(&dir.join("parts"));
                let readme_path = dir.join("README-live2d.txt");
                if !readme_path.exists() {
                    let guide = format!(
                        "何か藍 (NanikaAi) External Live2D Sidecar Directory\n\
                         Shell ID: {shell_id}\n\
                         1. Put separated RGBA part PNG files under parts/ (same canvas dimensions as surface0/surface10).\n\
                         2. Export live2d.json using tools/live2d_studio.html and place it right in this folder.\n\
                         3. User sidecar files in this directory take priority over bundled defaults.\n"
                    );
                    let _ = std::fs::write(&readme_path, guide);
                }
                if !self.smoke_mode {
                    let _ = open_local_path_in_os(&dir);
                }
            }
            MenuCommand::ToggleBind(bind_id) => {
                let shell_id = self.config_store.get().active_shell_id;
                let (next_binds, enabled, label) = {
                    let m = self.manifest.lock().expect("manifest mutex poisoned");
                    let defs = m.effective_bind_groups();
                    let label = defs
                        .iter()
                        .find(|b| b.bind_id == bind_id)
                        .map(|b| b.name.clone())
                        .unwrap_or_else(|| format!("Dress-up #{bind_id}"));
                    let (binds, enabled) =
                        self.config_store
                            .toggle_shell_bind(&shell_id, bind_id, &m.default_bind_ids());
                    (binds, enabled, label)
                };
                if let Some(ref gpu) = self.gpu {
                    let mut m = self.manifest.lock().expect("manifest mutex poisoned");
                    if let Some(ref mut s) = self.sakura {
                        s.active_binds = next_binds.clone();
                        let cur = s.surface_id;
                        s.set_surface(&mut m, gpu, cur, false);
                    }
                    if let Some(ref mut k) = self.kero {
                        k.active_binds = next_binds;
                        let cur = k.surface_id;
                        k.set_surface(&mut m, gpu, cur, false);
                    }
                }
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
                if !self.smoke_mode {
                    let state_str = if enabled { "装着" } else { "外し" };
                    let msg = format!("\\0\\s[5]「{label}」を{state_str}たよ！\\e");
                    let _ = self
                        .proxy
                        .send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
                }
            }
            MenuCommand::ToggleHourlyChime => {
                let cur = self.config_store.get().hourly_chime;
                let updated = self.config_store.update(ConfigPatch {
                    hourly_chime: Some(!cur),
                    ..Default::default()
                });
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
                if !self.smoke_mode {
                    let st = if updated.hourly_chime { "ON" } else { "OFF" };
                    let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(&format!(
                        "\\0\\s[5]毎時00分の時報を {st} にしたよ！\\e"
                    ))));
                }
            }
            MenuCommand::CycleAutoTalk => {
                self.cycle_autotalk(1);
                let sec = self.config_store.get().auto_talk_interval_sec;
                let msg = crate::i18n::format_autotalk_status(self.effective_ui_lang(), sec);
                self.apply_status_feedback(msg);
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
            }
            MenuCommand::CycleUiLanguage => {
                self.cycle_ui_language(1);
                let setting_now = self.config_store.get().ui_language;
                let msg = crate::i18n::format_ui_lang_status(
                    &setting_now,
                    self.effective_ui_lang(),
                );
                self.apply_status_feedback(msg);
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
                self.redraw_communicate_if_visible();
            }
            MenuCommand::ToggleQuietMode => {
                let next = !self.config_store.get().quiet_mode;
                self.config_store.update(ConfigPatch {
                    quiet_mode: Some(next),
                    ..Default::default()
                });
                let msg = if next {
                    self.ui_strings().status_quiet_on
                } else {
                    self.ui_strings().status_quiet_off
                }
                .to_string();
                self.apply_status_feedback(msg);
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
            }
            MenuCommand::ToggleHideCharacters => {
                self.characters_hidden_by_user = !self.characters_hidden_by_user;
                self.apply_visibility_state();
                if !self.characters_hidden_by_user {
                    let msg = self.ui_strings().status_chars_shown.to_string();
                    self.apply_status_feedback(msg);
                }
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
            }
            MenuCommand::TogglePomodoro => {
                self.apply_pomodoro_action("toggle");
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
            }
            MenuCommand::StopPomodoro => {
                self.apply_pomodoro_action("stop");
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
            }
            MenuCommand::SkipPomodoro => {
                self.apply_pomodoro_action("skip");
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
            }
            MenuCommand::TestVoice(scope) => {
                self.preview_voice(scope);
            }
            MenuCommand::CycleSakuraVoice => {
                self.cycle_voice_speaker(0, 1);
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
            }
            MenuCommand::CycleKeroVoice => {
                self.cycle_voice_speaker(1, 1);
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
            }
            MenuCommand::ShowBacklog => {
                self.show_backlog_in_balloon();
            }
            MenuCommand::CopyLastReply => {
                self.copy_last_reply_to_clipboard();
            }
            MenuCommand::MoveNextMonitor => {
                self.move_to_next_monitor_or_edge();
            }
            MenuCommand::ToggleVoiceTts => {
                let cur = self.config_store.get().voice_tts;
                let updated = self.config_store.update(ConfigPatch {
                    voice_tts: Some(!cur),
                    ..Default::default()
                });
                let (sak_sp, ker_sp, _) = self
                    .config_store
                    .get_shell_voice_profile(&updated.active_shell_id);
                let (arch, _) = self.active_kero_voice_archetype();
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
                if !updated.voice_tts {
                    crate::companion_extras::stop_managed_tts_engine();
                }
                if !self.smoke_mode {
                    if updated.voice_tts {
                        let prep_msg = self.ui_strings().status_tts_preparing.to_string();
                        self.apply_status_feedback(prep_msg);
                        let proxy = self.proxy.clone();
                        let v_url = updated.voicevox_url.clone();
                        let eng_mode = updated.tts_engine.clone();
                        std::thread::spawn(move || {
                            let status =
                                crate::companion_extras::ensure_tts_engine_ready_with_mode_blocking(
                                    &v_url, &eng_mode, true,
                                );
                            if let Some(ref base) = status.base_url {
                                if base.contains(":10101") {
                                    let proxy_hub = proxy.clone();
                                    crate::companion_extras::maybe_auto_install_boyish_aivm_async(
                                        base.clone(),
                                        arch,
                                        move |_| {
                                            let _ = proxy_hub.send_event(UserEvent::RefreshMenus);
                                        },
                                    );
                                }
                            }
                            let _ = proxy.send_event(UserEvent::RefreshMenus);
                            let (sak_lbl, ker_lbl) = if status.voices.is_empty() {
                                (
                                    status.sakura_voice_label.clone(),
                                    status.kero_voice_label.clone(),
                                )
                            } else {
                                let res = crate::companion_extras::resolve_effective_voice_selection(
                                    &status.voices,
                                    arch,
                                    sak_sp,
                                    ker_sp,
                                );
                                (res.sakura_label, res.kero_label)
                            };
                            let boot_tag = if status.auto_spawned {
                                "（自動起動）"
                            } else {
                                ""
                            };
                            let msg = format!(
                                "\\0\\s[5]音声読み上げ ON！[{}{boot_tag}] 私の声は「{sak_lbl}」だよ。\\1\\s[10]わしの声は「{ker_lbl}」やで！\\e",
                                status.engine_label,
                            );
                            let _ = proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
                        });
                    } else {
                        let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(
                            "\\0\\s[0]音声読み上げを OFF にしたよ。\\e",
                        )));
                    }
                }
            }
            MenuCommand::SelectTtsEngine(mode) => {
                let norm = crate::config_store::normalize_tts_engine(&mode);
                let updated = self.config_store.update(ConfigPatch {
                    voice_tts: Some(true),
                    tts_engine: Some(norm.clone()),
                    ..Default::default()
                });
                let (sak_sp, ker_sp, _) = self
                    .config_store
                    .get_shell_voice_profile(&updated.active_shell_id);
                let (arch, _) = self.active_kero_voice_archetype();
                if norm == "os" {
                    crate::companion_extras::stop_managed_tts_engine();
                }
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
                if !self.smoke_mode {
                    self.apply_status_feedback(format!("音声エンジンを「{norm}」へ切替中…"));
                    let proxy = self.proxy.clone();
                    let v_url = updated.voicevox_url.clone();
                    let allow_spawn = norm != "os";
                    std::thread::spawn(move || {
                        let status =
                            crate::companion_extras::ensure_tts_engine_ready_with_mode_blocking(
                                &v_url,
                                &norm,
                                allow_spawn,
                            );
                        if let Some(ref base) = status.base_url {
                            if base.contains(":10101") {
                                let proxy_hub = proxy.clone();
                                crate::companion_extras::maybe_auto_install_boyish_aivm_async(
                                    base.clone(),
                                    arch,
                                    move |_| {
                                        let _ = proxy_hub.send_event(UserEvent::RefreshMenus);
                                    },
                                );
                            }
                        }
                        let _ = proxy.send_event(UserEvent::RefreshMenus);
                        let (sak_lbl, ker_lbl) = if status.voices.is_empty() {
                            (status.sakura_voice_label, status.kero_voice_label)
                        } else {
                            let res = crate::companion_extras::resolve_effective_voice_selection(
                                &status.voices,
                                arch,
                                sak_sp,
                                ker_sp,
                            );
                            (res.sakura_label, res.kero_label)
                        };
                        let msg = format!(
                            "\\0\\s[5]音声エンジンを「{}」に切り替えたよ！（私: {sak_lbl}）\\1\\s[10]こっちは「{ker_lbl}」や！\\e",
                            status.engine_label,
                        );
                        let _ = proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
                    });
                }
            }
            MenuCommand::SelectKeroVoiceArchetype(arch_opt) => {
                let updated = self.config_store.update(ConfigPatch {
                    voice_tts: Some(true),
                    ..Default::default()
                });
                let shell_id = updated.active_shell_id;
                self.config_store
                    .set_shell_kero_archetype(&shell_id, arch_opt);
                let (arch, _) = self.active_kero_voice_archetype();
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
                if !self.smoke_mode {
                    let msg = format!(
                        "\\1\\s[10]パートナー声タイプを「{}」にしたで！\\e",
                        arch.label_ja()
                    );
                    let _ = self
                        .proxy
                        .send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
                }
            }
            MenuCommand::SelectVoiceSpeaker { scope, style_id } => {
                let updated = self.config_store.update(ConfigPatch {
                    voice_tts: Some(true),
                    ..Default::default()
                });
                let shell_id = updated.active_shell_id;
                self.config_store
                    .set_shell_voice_speaker(&shell_id, scope, style_id);
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
                if !self.smoke_mode {
                    let (_, voices) = crate::companion_extras::cached_tts_voices();
                    let v_name = match style_id {
                        Some(id) => voices
                            .iter()
                            .find(|v| v.style_id == id)
                            .map(|v| v.label.clone())
                            .unwrap_or_else(|| format!("Speaker #{id}")),
                        None => "自動 (Auto)".to_string(),
                    };
                    let script = if scope == 1 {
                        format!("\\1\\s[10]わしの声を「{v_name}」に変えたで！\\e")
                    } else {
                        format!("\\0\\s[5]声色を「{v_name}」に変えたよ、よろしくね！\\e")
                    };
                    let _ = self
                        .proxy
                        .send_event(UserEvent::PlayActions(parse_dialogue(&script)));
                }
            }
            MenuCommand::InstallAivmModel { uuid, name } => {
                self.spawn_install_aivm_model(uuid, name);
            }
            MenuCommand::CheckUpdates => {
                let msg = self.ui_strings().status_checking_updates.to_string();
                self.apply_status_feedback(msg);
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let msg = match crate::companion_extras::check_github_release_update(
                        env!("CARGO_PKG_VERSION"),
                    ) {
                        Ok(info) if info.has_update => {
                            format!(
                                "\\0\\s[5]新しいバージョン v{} が公開されてるよ！（現在 v{}）\\n\\_a[{}]GitHub Releases を開く\\_a\\e",
                                info.latest_version, info.current_version, info.release_url
                            )
                        }
                        Ok(info) => {
                            format!(
                                "\\0\\s[0]現在最新の v{} を利用中だよ！（Release: v{}）\\e",
                                info.current_version, info.latest_version
                            )
                        }
                        Err(e) => {
                            format!(
                                "\\0\\s[3]更新確認できなかったよ。（{e}）\\e"
                            )
                        }
                    };
                    let _ = proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
                });
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
                    "\\0\\s[1]会話履歴をクリアしたよ！\\e",
                )));
            }
            MenuCommand::Quit => {
                if let Some(el) = event_loop {
                    self.begin_graceful_quit(el);
                }
            }
        }
    }

    fn begin_graceful_quit(&mut self, event_loop: &ActiveEventLoop) {
        if self.smoke_mode || self.closing_at.is_some() {
            crate::companion_extras::stop_managed_tts_engine();
            crate::companion_extras::stop_managed_ollama_engine();
            if let Some(mut srv) = self.server.take() {
                srv.stop();
            }
            event_loop.exit();
            return;
        }
        let farewell = self.engine.build_farewell_script();
        let actions = parse_dialogue(&farewell);
        self.maybe_speak_actions(&actions);
        self.playback.enqueue(actions);
        let imm = self.playback.fast_forward();
        self.apply_immediate_surface_actions(imm);
        self.sync_balloons_to_playback();
        self.closing_at = Some(Instant::now() + Duration::from_millis(1350));
    }

    fn representative_voice_options(
        voices: &[crate::companion_extras::TtsVoiceOption],
        max_items: usize,
    ) -> Vec<crate::companion_extras::TtsVoiceOption> {
        if max_items == 0 || voices.is_empty() {
            return Vec::new();
        }
        let mut by_speaker: Vec<(&str, Vec<&crate::companion_extras::TtsVoiceOption>)> = Vec::new();
        for v in voices {
            if let Some((_, list)) = by_speaker
                .iter_mut()
                .find(|(sp, _)| *sp == v.speaker_name.as_str())
            {
                list.push(v);
            } else {
                by_speaker.push((v.speaker_name.as_str(), vec![v]));
            }
        }
        for (_, list) in &mut by_speaker {
            if let Some(pos) = list
                .iter()
                .position(|v| v.style_name == "ノーマル" || v.style_name == "標準")
            {
                if pos > 0 {
                    let primary = list.remove(pos);
                    list.insert(0, primary);
                }
            }
        }
        let mut picked = Vec::new();
        let max_depth = by_speaker.iter().map(|(_, l)| l.len()).max().unwrap_or(0);
        for depth in 0..max_depth {
            for (_, list) in &by_speaker {
                if let Some(v) = list.get(depth) {
                    picked.push((*v).clone());
                    if picked.len() >= max_items {
                        return picked;
                    }
                }
            }
        }
        picked
    }

    fn apply_immediate_surface_actions(&mut self, imm: Vec<DialogueAction>) {
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
    }

    fn maybe_speak_actions(&self, actions: &[DialogueAction]) {
        if self.smoke_mode || self.characters_effectively_hidden() {
            return;
        }
        let cfg = self.config_store.get();
        if !cfg.voice_tts || cfg.quiet_mode {
            return;
        }
        let segs = crate::companion_extras::extract_speech_segments_from_actions(actions);
        if segs.is_empty() {
            return;
        }
        let (sak_sp, ker_sp, _) = self
            .config_store
            .get_shell_voice_profile(&cfg.active_shell_id);
        let (arch, _) = self.active_kero_voice_archetype();
        crate::companion_extras::speak_dialogue_batch_with_prosody_async(
            segs,
            &cfg.voicevox_url,
            &cfg.tts_engine,
            arch,
            sak_sp,
            ker_sp,
            cfg.voice_speed,
            cfg.voice_intonation,
        );
    }

    fn spawn_dropped_file_inspection(&mut self, path: &Path) {
        match crate::companion_extras::inspect_dropped_path(path) {
            Ok(payload) => {
                self.apply_status_feedback(format!("「{}」確認中…", payload.file_name));
                let eng = self.engine.clone();
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    let reply = eng.handle_dropped_payload(&payload);
                    let _ = proxy.send_event(UserEvent::PlayActions(reply.actions));
                });
            }
            Err(err_msg) => {
                let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(&format!(
                    "\\0\\s[4]{err_msg}\\e"
                ))));
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
            return Err(format!("不明なプロバイダー: {provider}"));
        };
        let next_model = model.filter(|m| !m.trim().is_empty()).unwrap_or(p.default_model);
        crate::agent_engine::assert_allowed_model(&next_model)?;
        let cfg = self.config_store.update(ConfigPatch {
            provider: Some(p.id.clone()),
            model: Some(next_model.clone()),
            ..Default::default()
        });
        self.rebuild_tray_menu();
        let msg = format!(
            "\\0\\s[5]AI エンジンを {} ({}) に切り替えたよ！\\e",
            p.label, next_model
        );
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
        let ui_lang = self.effective_ui_lang();
        if let Some(ref mut cw) = self.communicate {
            let x = (sx + sw * 0.5 - cw.win_size.0 as f64 * 0.5)
                .clamp(mx + 12.0, (mx + mw - cw.win_size.0 as f64 - 12.0).max(mx + 12.0));
            let y = (sy + sh - 54.0).clamp(my + 6.0, (my + mh - cw.win_size.1 as f64 - 8.0).max(my + 6.0));
            cw.visible = true;
            cw.focused = true;
            cw.cursor_on = true;
            cw.last_blink = Instant::now();
            cw.anim_start = Instant::now();
            cw.hovered_close = false;
            cw.hovered_send = false;
            cw.text.clear();
            cw.ime_preedit.clear();
            cw.ws.window.set_outer_position(LogicalPosition::new(x, y));
            cw.ws.window.set_ime_allowed(true);
            cw.ws.window.set_visible(true);
            cw.ws.window.focus_window();
            if let Some(ref gpu) = self.gpu {
                cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu, ui_lang);
            }
        }
    }

    /// Background-discover installed TTS engines & speaker voices and emit `RefreshMenus`.
    fn spawn_tts_discovery_refresh(&self, allow_spawn: bool) {
        let cfg = self.config_store.get();
        let v_url = cfg.voicevox_url;
        let eng_mode = cfg.tts_engine;
        let (arch, _) = self.active_kero_voice_archetype();
        let proxy_tts = self.proxy.clone();
        std::thread::spawn(move || {
            let status =
                crate::companion_extras::ensure_tts_engine_ready_with_mode_blocking(
                    &v_url,
                    &eng_mode,
                    allow_spawn,
                );
            if let Some(base) = status.base_url {
                if base.contains(":10101") {
                    let proxy_hub = proxy_tts.clone();
                    crate::companion_extras::maybe_auto_install_boyish_aivm_async(
                        base,
                        arch,
                        move |_| {
                            let _ = proxy_hub.send_event(UserEvent::RefreshMenus);
                        },
                    );
                }
            }
            let _ = proxy_tts.send_event(UserEvent::RefreshMenus);
        });
    }

    fn open_menu_near_cursor(&mut self, scope: Scope) {
        if !self.smoke_mode && crate::companion_extras::cached_tts_voices().1.is_empty() {
            let allow_spawn = self.config_store.get().voice_tts;
            self.spawn_tts_discovery_refresh(allow_spawn);
        }
        let (mx, my, mw, mh) = self.desktop_safe_work_area();
        let fallback_pos = match scope {
            Scope::Sakura => self.sakura.as_ref().map(|c| (c.logical_pos.0 + 40.0, c.logical_pos.1 + 40.0)),
            Scope::Kero => self.kero.as_ref().map(|c| (c.logical_pos.0 + 20.0, c.logical_pos.1 + 20.0)),
        }
        .unwrap_or((300.0, 300.0));

        self.expanded_menu_section = None;
        let rows = self.build_menu_rows();
        let Some(ref mut menu) = self.menu else { return };
        menu.rows = rows;
        menu.scroll_row = 0;
        menu.scroll_accum = 0.0;
        menu.hovered_idx = None;
        menu.max_visible_rows = (((mh - 36.0) / menu.row_h).floor() as usize).clamp(16, 38);
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
        if let Some(deferred) = self.pending_auto_talk_actions.take() {
            let _ = self.proxy.send_event(UserEvent::PlayActions(deferred));
        }
    }

    fn spawn_user_chat(&mut self, text: String) {
        self.spawn_user_chat_with_images(text, Vec::new());
    }

    fn spawn_user_chat_with_images(
        &mut self,
        text: String,
        images: Vec<crate::agent_engine::ImageAttachment>,
    ) {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() && images.is_empty() {
            return;
        }
        self.auto_talk_last = Instant::now();
        // Immediate visual responsiveness: character reacts & thinking indicator appears synchronously (< 50ms)
        if let Some(ref mut s) = self.sakura {
            s.trigger_bounce(0.016);
        }
        let thinking = if images.is_empty() {
            self.ui_strings().status_thinking.to_string()
        } else {
            format!("🖼 {} …", self.ui_strings().status_thinking)
        };
        self.apply_status_feedback(thinking);

        let eng = self.engine.clone();
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let reply = if images.is_empty() {
                eng.handle_user_message(&trimmed)
            } else {
                eng.handle_user_message_with_images(&trimmed, &images)
            };
            let _ = proxy.send_event(UserEvent::PlayActions(reply.actions));
            let _ = proxy.send_event(UserEvent::RefreshMenus);
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

    fn apply_status_feedback(&mut self, msg: String) {
        if self.characters_effectively_hidden() {
            return;
        }
        let sp_sak = self.speaker_name(Scope::Sakura);
        let mut need_repo = false;
        if let Some(ref mut sb) = self.sakura_balloon {
            sb.status_line = msg.clone();
            if sb.visible {
                if let Some(ref gpu) = self.gpu {
                    need_repo = sb.redraw_content(
                        &self.balloon_skin,
                        &sp_sak,
                        !sb.user_scrolled_up,
                        &mut self.text_renderer,
                        gpu,
                    );
                }
            } else if !msg.is_empty() {
                sb.scroll_px = 0.0;
                sb.user_scrolled_up = false;
                sb.utterance_min_w = 0;
                self.playback.sakura_buf = "…".to_string();
                self.playback.sakura_show = true;
                self.playback.sakura_dirty = true;
                self.sync_balloons_to_playback();
            }
        }
        if need_repo {
            self.reposition_balloon(Scope::Sakura);
        }
    }

    fn handle_pointer_move(&mut self, window_id: WindowId, local: (f64, f64)) {
        match self.classify_window(window_id) {
            Some(UiWindowTarget::Char(sc)) => {
                let cw = if sc == Scope::Sakura { self.sakura.as_mut() } else { self.kero.as_mut() };
                if let Some(c) = cw {
                    c.last_local_cursor = Some(local);
                    if c.dragging {
                        if let Mouse::Position { x, y } = Mouse::get_mouse_position() {
                            #[cfg(target_os = "macos")]
                            let (gx, gy) = (x as f64, y as f64);
                            #[cfg(not(target_os = "macos"))]
                            let (gx, gy) = {
                                let scale = c.ws.window.scale_factor().max(1.0);
                                (x as f64 / scale, y as f64 / scale)
                            };
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
            }
            Some(UiWindowTarget::Balloon(sb)) => {
                let sp_name = self.speaker_name(sb);
                let mut need_repo = false;
                if let Some(b) = if sb == Scope::Sakura {
                    self.sakura_balloon.as_mut()
                } else {
                    self.kero_balloon.as_mut()
                } {
                    let hovered = b.choices.iter().position(|ch| {
                        ch.rect.2 > ch.rect.0
                            && local.0 >= ch.rect.0
                            && local.0 <= ch.rect.2
                            && local.1 >= ch.rect.1
                            && local.1 <= ch.rect.3
                    });
                    if b.hovered_choice != hovered {
                        b.hovered_choice = hovered;
                        if let Some(ref gpu) = self.gpu {
                            need_repo = b.redraw_content(
                                &self.balloon_skin,
                                &sp_name,
                                false,
                                &mut self.text_renderer,
                                gpu,
                            );
                        }
                    }
                }
                if need_repo {
                    self.reposition_balloon(sb);
                }
            }
            Some(UiWindowTarget::Communicate) => {
                let ui_lang = self.effective_ui_lang();
                if let Some(ref mut cw) = self.communicate {
                    cw.last_local_cursor = Some(local);
                    let in_rect = |r: (f64, f64, f64, f64)| -> bool {
                        r.2 > r.0 && local.0 >= r.0 && local.0 <= r.2 && local.1 >= r.1 && local.1 <= r.3
                    };
                    let has_text = !cw.text.trim().is_empty()
                        || !cw.ime_preedit.trim().is_empty()
                        || cw.attached_image.is_some();
                    let new_close = in_rect(cw.close_rect);
                    let new_send = has_text && in_rect(cw.send_rect);
                    let new_attach = in_rect(cw.attach_rect);
                    if cw.hovered_close != new_close
                        || cw.hovered_send != new_send
                        || cw.hovered_attach != new_attach
                    {
                        cw.hovered_close = new_close;
                        cw.hovered_send = new_send;
                        cw.hovered_attach = new_attach;
                        if let Some(ref gpu) = self.gpu {
                            cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu, ui_lang);
                        }
                    }
                }
            }
            Some(UiWindowTarget::Menu) => {
                if let Some(ref mut m) = self.menu {
                    let rel_row = ((local.1 - 7.0) / m.row_h).floor() as isize;
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
            Some(UiWindowTarget::Settings) => {
                let mut drag_pos: Option<(f64, f64)> = None;
                let mut action_opt: Option<SettingsAction> = None;
                let mut need_redraw = false;
                if let Some(ref mut sw) = self.settings {
                    sw.last_local_cursor = Some(local);
                    if sw.dragging_header {
                        if let Mouse::Position { x, y } = Mouse::get_mouse_position() {
                            #[cfg(target_os = "macos")]
                            let (gx, gy) = (x as f64, y as f64);
                            #[cfg(not(target_os = "macos"))]
                            let (gx, gy) = {
                                let scale = sw.ws.window.scale_factor().max(1.0);
                                (x as f64 / scale, y as f64 / scale)
                            };
                            let dx = gx - sw.drag_start_cursor.0;
                            let dy = gy - sw.drag_start_cursor.1;
                            drag_pos = Some((sw.drag_start_pos.0 + dx, sw.drag_start_pos.1 + dy));
                        }
                    } else {
                        let (changed, act) = sw.model.handle_pointer_move(local);
                        need_redraw = changed;
                        action_opt = act;
                    }
                }
                if let (Some((px, py)), Some(ref sw)) = (drag_pos, &self.settings) {
                    sw.ws.window.set_outer_position(LogicalPosition::new(px, py));
                } else if let Some(act) = action_opt {
                    self.execute_settings_action(act);
                } else if need_redraw {
                    self.redraw_settings_panel();
                }
            }
            None => {}
        }
    }

    fn handle_scroll(&mut self, window_id: WindowId, dy: f64) {
        match self.classify_window(window_id) {
            Some(UiWindowTarget::Balloon(sb)) => {
                let sp_name = self.speaker_name(sb);
                let mut need_repo = false;
                if let Some(b) = if sb == Scope::Sakura {
                    self.sakura_balloon.as_mut()
                } else {
                    self.kero_balloon.as_mut()
                } {
                    b.scroll_px = (b.scroll_px - dy as f32).clamp(0.0, b.max_scroll_px);
                    b.user_scrolled_up = b.max_scroll_px > 1.0 && (b.scroll_px + 4.0) < b.max_scroll_px;
                    if let Some(ref gpu) = self.gpu {
                        need_repo = b.redraw_content(
                            &self.balloon_skin,
                            &sp_name,
                            false,
                            &mut self.text_renderer,
                            gpu,
                        );
                    }
                }
                if need_repo {
                    self.reposition_balloon(sb);
                }
            }
            Some(UiWindowTarget::Menu) => {
                if let Some(ref mut m) = self.menu {
                    let max_scroll = m.rows.len().saturating_sub(m.max_visible_rows);
                    if max_scroll == 0 {
                        m.scroll_row = 0;
                        m.scroll_accum = 0.0;
                    } else {
                        m.scroll_accum += dy;
                        let step = 18.0;
                        let prev_row = m.scroll_row;
                        while m.scroll_accum <= -step {
                            m.scroll_accum += step;
                            if m.scroll_row < max_scroll {
                                m.scroll_row += 1;
                            } else {
                                m.scroll_accum = 0.0;
                                break;
                            }
                        }
                        while m.scroll_accum >= step {
                            m.scroll_accum -= step;
                            if m.scroll_row > 0 {
                                m.scroll_row -= 1;
                            } else {
                                m.scroll_accum = 0.0;
                                break;
                            }
                        }
                        if m.scroll_row == 0 && m.scroll_accum > 0.0 {
                            m.scroll_accum = 0.0;
                        } else if m.scroll_row == max_scroll && m.scroll_accum < 0.0 {
                            m.scroll_accum = 0.0;
                        }
                        if prev_row != m.scroll_row {
                            if let Some(ref gpu) = self.gpu {
                                m.redraw(&mut self.text_renderer, gpu);
                            }
                        }
                    }
                }
            }
            Some(UiWindowTarget::Settings) => {
                let scrolled = self
                    .settings
                    .as_mut()
                    .is_some_and(|sw| sw.model.scroll_by(dy as f32));
                if scrolled {
                    self.redraw_settings_panel();
                }
            }
            _ => {}
        }
    }

    fn handle_mouse_button(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        button: MouseButton,
        state: ElementState,
    ) {
        match self.classify_window(window_id) {
            Some(UiWindowTarget::Char(sc)) => match (button, state) {
                (MouseButton::Right, ElementState::Pressed) => {
                    self.open_menu_near_cursor(sc);
                }
                (MouseButton::Left, ElementState::Pressed) => {
                    self.hide_menu();
                    let locked = self.config_store.get().lock_position;
                    let cw = if sc == Scope::Sakura { self.sakura.as_mut() } else { self.kero.as_mut() };
                    if let Some(c) = cw {
                        if !locked {
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
                }
                (MouseButton::Left, ElementState::Released) => {
                    let mut open_chat = false;
                    let mut touch_region = None;
                    let mut snapped_balloon = false;
                    let snap_floor = self.config_store.get().snap_to_bottom;
                    let (_, sy_safe, _, sh_safe) = self.desktop_safe_work_area();
                    let safe_bottom = sy_safe + sh_safe;
                    {
                        let cw = if sc == Scope::Sakura { self.sakura.as_mut() } else { self.kero.as_mut() };
                        if let Some(c) = cw {
                            let was_drag = c.drag_moved;
                            c.dragging = false;
                            c.drag_moved = false;
                            if was_drag && snap_floor {
                                c.sync_pos_from_window();
                                let foot_off = if c.is_full_feet() {
                                    c.foot_bottom_offset_logical()
                                } else {
                                    0.0
                                };
                                let floor_y = if c.is_full_feet() {
                                    safe_bottom - 6.0
                                } else {
                                    safe_bottom
                                };
                                let target_y = floor_y - c.win_size.1 as f64 + foot_off;
                                if (c.logical_pos.1 - target_y).abs() <= 32.0 {
                                    c.set_logical_pos(c.logical_pos.0, target_y);
                                    snapped_balloon = true;
                                }
                            } else if !was_drag {
                                let now = Instant::now();
                                let is_dbl = c
                                    .last_click_at
                                    .is_some_and(|t| now.duration_since(t) <= Duration::from_millis(380));
                                c.last_click_at = if is_dbl { None } else { Some(now) };
                                if is_dbl {
                                    if let Some((lx, ly)) = c.last_local_cursor {
                                        let ds = c.effective_display_scale();
                                        let ix = (lx * ds).floor() as i32;
                                        let iy = (ly * ds).floor() as i32;
                                        let m = self.manifest.lock().expect("manifest mutex poisoned");
                                        let col = c.base_bmp.as_ref().and_then(|bb| {
                                            m.hit_collision(c.surface_id, sc, bb, ix, iy)
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
                    if snapped_balloon {
                        self.reposition_balloon(sc);
                    }
                    if open_chat {
                        self.open_communicate_near_sakura();
                    } else if let Some(reg) = touch_region {
                        match sc {
                            Scope::Sakura => {
                                if let Some(ref mut s) = self.sakura {
                                    s.trigger_bounce(0.022);
                                }
                            }
                            Scope::Kero => {
                                if let Some(ref mut k) = self.kero {
                                    k.trigger_bounce(0.022);
                                }
                            }
                        }
                        self.spawn_touch(sc, "double_click", reg);
                    }
                }
                _ => {}
            },
            Some(UiWindowTarget::Balloon(sb)) => {
                if button == MouseButton::Left && state == ElementState::Pressed {
                    let mut picked_choice: Option<(String, String, bool)> = None;
                    let mut should_ff = false;
                    let mut should_hide = false;
                    let mut link_status: Option<String> = None;
                    if let Some(b) = if sb == Scope::Sakura {
                        self.sakura_balloon.as_mut()
                    } else {
                        self.kero_balloon.as_mut()
                    } {
                        if let Some(idx) = b.hovered_choice {
                            if let Some(ch) = b.choices.get(idx) {
                                picked_choice =
                                    Some((ch.label.clone(), ch.choice_id.clone(), ch.is_url));
                            }
                        } else if !self.playback.steps.is_empty() {
                            should_ff = true;
                        } else {
                            should_hide = true;
                            b.visible = false;
                            b.ws.window.set_visible(false);
                        }
                    }
                    if let Some((lbl, cid, is_url)) = picked_choice {
                        if is_url {
                            let ui_s = self.ui_strings();
                            link_status = Some(
                                match crate::companion_extras::open_validated_browser_url(
                                    &cid,
                                    self.smoke_mode,
                                ) {
                                    Ok(url) => format!("{}{url}", ui_s.status_link_opened_prefix),
                                    Err(err) => format!("{}{err}", ui_s.status_url_blocked_prefix),
                                },
                            );
                        } else if cid == "__setup_local_ai__" {
                            self.playback.sakura_choices.clear();
                            self.playback.kero_choices.clear();
                            self.start_one_click_local_ai_setup();
                        } else if cid == "__open_settings_ai__" {
                            self.playback.sakura_choices.clear();
                            self.playback.kero_choices.clear();
                            self.open_settings_panel();
                            if let Some(ref mut sw) = self.settings {
                                sw.model.select_tab(3);
                            }
                            self.redraw_settings_panel();
                        } else if cid == "__copy_last_reply__" {
                            self.copy_last_reply_to_clipboard();
                        } else if cid == "__pomo_start__" {
                            self.playback.sakura_choices.clear();
                            self.playback.kero_choices.clear();
                            self.apply_pomodoro_action("start");
                            self.rebuild_tray_menu();
                            self.redraw_settings_panel();
                        } else if let Some(memo_id) = cid.strip_prefix("__memo_done_") {
                            self.playback.sakura_choices.clear();
                            self.playback.kero_choices.clear();
                            if let Ok(id) = memo_id.parse::<u64>() {
                                let store = crate::accessory_utils::MemoStore::new(
                                    &self.config_store.config_dir,
                                );
                                let _ = store.set_done(id, true);
                            }
                            self.show_quick_memos_in_balloon();
                        } else if cid == "__memo_clear_done__" {
                            self.playback.sakura_choices.clear();
                            self.playback.kero_choices.clear();
                            let store = crate::accessory_utils::MemoStore::new(
                                &self.config_store.config_dir,
                            );
                            let _ = store.clear_done();
                            self.show_quick_memos_in_balloon();
                        } else if cid == "__cal_sync__" {
                            self.playback.sakura_choices.clear();
                            self.playback.kero_choices.clear();
                            self.spawn_calendar_sync(true);
                        } else {
                            self.playback.sakura_choices.clear();
                            self.playback.kero_choices.clear();
                            self.spawn_user_chat(format!("[선택지 선택] {lbl} (id={cid})"));
                        }
                    } else if should_ff {
                        let imm = self.playback.fast_forward();
                        self.apply_immediate_surface_actions(imm);
                        self.sync_balloons_to_playback();
                    } else if should_hide {
                        if sb == Scope::Sakura {
                            self.playback.sakura_show = false;
                        } else {
                            self.playback.kero_show = false;
                        }
                    }
                    if let Some(st) = link_status {
                        let sp = self.speaker_name(sb);
                        if let (Some(ref gpu), Some(b)) = (
                            &self.gpu,
                            if sb == Scope::Sakura {
                                self.sakura_balloon.as_mut()
                            } else {
                                self.kero_balloon.as_mut()
                            },
                        ) {
                            b.status_line = st;
                            b.redraw_content(
                                &self.balloon_skin,
                                &sp,
                                false,
                                &mut self.text_renderer,
                                gpu,
                            );
                        }
                    }
                }
            }
            Some(UiWindowTarget::Communicate) if button == MouseButton::Left && state == ElementState::Pressed => {
                let mut submit_payload: Option<(String, Vec<crate::agent_engine::ImageAttachment>)> = None;
                let mut open_image_picker = false;
                let mut closed_now = false;
                let ui_lang = self.effective_ui_lang();
                if let Some(ref mut cw) = self.communicate {
                    let in_rect = |r: (f64, f64, f64, f64), p: (f64, f64)| -> bool {
                        r.2 > r.0 && p.0 >= r.0 && p.0 <= r.2 && p.1 >= r.1 && p.1 <= r.3
                    };
                    if let Some(pos) = cw.last_local_cursor {
                        if in_rect(cw.close_rect, pos) {
                            cw.visible = false;
                            cw.focused = false;
                            cw.hovered_close = false;
                            cw.hovered_send = false;
                            cw.hovered_attach = false;
                            cw.ime_preedit.clear();
                            cw.ws.window.set_visible(false);
                            closed_now = true;
                        } else if in_rect(cw.attach_rect, pos) {
                            if cw.attached_image.is_some() {
                                cw.attached_image = None;
                            } else {
                                open_image_picker = true;
                            }
                        } else if in_rect(cw.send_rect, pos) {
                            let combined = format!("{}{}", cw.text, cw.ime_preedit);
                            let trimmed = combined.trim().to_string();
                            let imgs: Vec<_> = cw.attached_image.take().into_iter().collect();
                            if !trimmed.is_empty() || !imgs.is_empty() {
                                submit_payload = Some((trimmed, imgs));
                                cw.text.clear();
                                cw.ime_preedit.clear();
                                cw.visible = false;
                                cw.focused = false;
                                cw.hovered_close = false;
                                cw.hovered_send = false;
                                cw.hovered_attach = false;
                                cw.ws.window.set_visible(false);
                                closed_now = true;
                            }
                        }
                    }
                    if !closed_now && !open_image_picker {
                        cw.focused = true;
                        cw.cursor_on = true;
                        cw.last_blink = Instant::now();
                        cw.ws.window.focus_window();
                        if let Some(ref gpu) = self.gpu {
                            cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu, ui_lang);
                        }
                    }
                }
                if open_image_picker && !self.smoke_mode {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Images (PNG / JPG / BMP)", &["png", "jpg", "jpeg", "bmp"])
                        .pick_file()
                    {
                        match crate::agent_engine::load_image_attachment(&path) {
                            Ok(att) => {
                                if let Some(ref mut cw) = self.communicate {
                                    cw.attached_image = Some(att);
                                    cw.focused = true;
                                    cw.ws.window.focus_window();
                                    if let Some(ref gpu) = self.gpu {
                                        cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu, ui_lang);
                                    }
                                }
                            }
                            Err(e) => {
                                self.apply_status_feedback(format!("🖼 {e}"));
                            }
                        }
                    }
                }
                if let Some((msg, imgs)) = submit_payload {
                    self.spawn_user_chat_with_images(msg, imgs);
                }
            }
            Some(UiWindowTarget::Menu) if button == MouseButton::Left && state == ElementState::Released => {
                let chosen = self.menu.as_ref().and_then(|m| {
                    m.hovered_idx.and_then(|i| m.rows.get(i).map(|r| r.cmd.clone()))
                });
                match chosen {
                    Some(MenuCommand::ToggleMenuSection(sec)) => {
                        self.expanded_menu_section = if self.expanded_menu_section == Some(sec) {
                            None
                        } else {
                            Some(sec)
                        };
                        let (mx, my, mw, mh) = self.desktop_safe_work_area();
                        let rows = self.build_menu_rows();
                        if let (Some(ref gpu), Some(ref mut m)) = (&self.gpu, &mut self.menu) {
                            m.rows = rows;
                            m.scroll_row = 0;
                            m.scroll_accum = 0.0;
                            m.hovered_idx = m.rows.iter().position(|r| {
                                matches!(r.cmd, MenuCommand::ToggleMenuSection(s) if s == sec)
                            });
                            m.redraw(&mut self.text_renderer, gpu);
                            if let Ok(outer) = m.ws.window.outer_position() {
                                let sc = m.ws.window.scale_factor().max(1.0);
                                let ol = outer.to_logical::<f64>(sc);
                                let cx = ol
                                    .x
                                    .clamp(mx + 8.0, (mx + mw - m.win_size.0 as f64 - 8.0).max(mx + 8.0));
                                let cy = ol
                                    .y
                                    .clamp(my + 6.0, (my + mh - m.win_size.1 as f64 - 8.0).max(my + 6.0));
                                m.ws.window.set_outer_position(LogicalPosition::new(cx, cy));
                            }
                        }
                    }
                    Some(cmd) => {
                        self.hide_menu();
                        self.execute_menu_command(event_loop, cmd);
                    }
                    None => {
                        self.hide_menu();
                    }
                }
            }
            Some(UiWindowTarget::Settings) if button == MouseButton::Left => {
                if state == ElementState::Pressed {
                    let mut action_opt: Option<SettingsAction> = None;
                    let mut need_redraw = false;
                    if let Some(ref mut sw) = self.settings {
                        let local_opt = sw.last_local_cursor.or_else(|| {
                            let scale = sw.ws.window.scale_factor().max(1.0);
                            if let (Mouse::Position { x, y }, Ok(outer)) =
                                (Mouse::get_mouse_position(), sw.ws.window.outer_position())
                            {
                                #[cfg(target_os = "macos")]
                                {
                                    let wl = outer.to_logical::<f64>(scale);
                                    Some((x as f64 - wl.x, y as f64 - wl.y))
                                }
                                #[cfg(not(target_os = "macos"))]
                                {
                                    Some((
                                        (x as f64 - outer.x as f64) / scale,
                                        (y as f64 - outer.y as f64) / scale,
                                    ))
                                }
                            } else {
                                None
                            }
                        });
                        if let Some(local) = local_opt {
                            sw.last_local_cursor = Some(local);
                            let (red, start_drag, act) = sw.model.handle_mouse_press(local);
                            need_redraw = red;
                            action_opt = act;
                            if start_drag {
                                sw.dragging_header = true;
                                let scale = sw.ws.window.scale_factor().max(1.0);
                                if let Ok(outer) = sw.ws.window.outer_position() {
                                    let wl = outer.to_logical::<f64>(scale);
                                    sw.drag_start_pos = (wl.x, wl.y);
                                }
                                if let Mouse::Position { x, y } = Mouse::get_mouse_position() {
                                    #[cfg(target_os = "macos")]
                                    {
                                        sw.drag_start_cursor = (x as f64, y as f64);
                                    }
                                    #[cfg(not(target_os = "macos"))]
                                    {
                                        sw.drag_start_cursor =
                                            (x as f64 / scale, y as f64 / scale);
                                    }
                                }
                            }
                        }
                    }
                    if let Some(act) = action_opt {
                        self.execute_settings_action(act);
                    } else if need_redraw {
                        self.redraw_settings_panel();
                    }
                } else if state == ElementState::Released {
                    let mut was_interacting = false;
                    if let Some(ref mut sw) = self.settings {
                        was_interacting =
                            sw.dragging_header || sw.model.dragging_slider.is_some();
                        sw.dragging_header = false;
                        sw.model.handle_mouse_release();
                    }
                    if was_interacting {
                        self.redraw_settings_panel();
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_ime_event(&mut self, window_id: WindowId, ime_ev: Ime) {
        if self.classify_window(window_id) != Some(UiWindowTarget::Communicate) {
            return;
        }
        let ui_lang = self.effective_ui_lang();
        if let Some(ref mut cw) = self.communicate {
            cw.focused = true;
            cw.cursor_on = true;
            cw.last_blink = Instant::now();
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
                cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu, ui_lang);
            }
        }
    }

    fn handle_communicate_key(&mut self, logical_key: &Key, text: Option<&str>) {
        let mut submit_payload: Option<(String, Vec<crate::agent_engine::ImageAttachment>)> = None;
        let ui_lang = self.effective_ui_lang();
        if let Some(ref mut cw) = self.communicate {
            cw.focused = true;
            cw.cursor_on = true;
            cw.last_blink = Instant::now();
            let cmd_or_ctrl = self.modifiers.super_key() || self.modifiers.control_key();
            match logical_key {
                Key::Named(NamedKey::Escape) => {
                    cw.visible = false;
                    cw.focused = false;
                    cw.hovered_close = false;
                    cw.hovered_send = false;
                    cw.hovered_attach = false;
                    cw.ime_preedit.clear();
                    cw.ws.window.set_visible(false);
                }
                Key::Named(NamedKey::Enter) => {
                    if cw.ime_preedit.is_empty() {
                        let imgs: Vec<_> = cw.attached_image.take().into_iter().collect();
                        submit_payload = Some((cw.text.clone(), imgs));
                        cw.text.clear();
                        cw.visible = false;
                        cw.focused = false;
                        cw.hovered_close = false;
                        cw.hovered_send = false;
                        cw.hovered_attach = false;
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
                    cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu, ui_lang);
                }
            }
        }
        if let Some((msg, imgs)) = submit_payload {
            self.spawn_user_chat_with_images(msg, imgs);
        }
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

        while processed < if self.playback.quick_mode { 2048 } else { 6 } {
            let Some(front) = self.playback.steps.front().cloned() else {
                break;
            };
            match front {
                PlaybackStep::Char(ch) => {
                    if !self.playback.quick_mode && now < self.playback.next_char_at {
                        break;
                    }
                    self.playback.steps.pop_front();
                    self.playback.last_char_spoken = Some((self.playback.active_speaker, now));
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
                    self.playback.next_char_at = if self.playback.quick_mode {
                        now
                    } else {
                        now + Duration::from_millis(delay)
                    };
                    processed += 1;
                }
                PlaybackStep::Action(act) => {
                    self.playback.steps.pop_front();
                    match act.kind {
                        ActionKind::Speaker => {
                            if self.playback.active_speaker != act.scope {
                                self.playback.last_char_spoken = None;
                            }
                            self.playback.active_speaker = act.scope;
                        }
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
                                if let Some(ref mut kb) = self.kero_balloon {
                                    kb.scroll_px = 0.0;
                                    kb.user_scrolled_up = false;
                                    kb.utterance_min_w = 0;
                                }
                            } else {
                                self.playback.sakura_buf.clear();
                                self.playback.sakura_choices.clear();
                                self.playback.sakura_dirty = true;
                                if let Some(ref mut sb) = self.sakura_balloon {
                                    sb.scroll_px = 0.0;
                                    sb.user_scrolled_up = false;
                                    sb.utterance_min_w = 0;
                                }
                            }
                        }
                        ActionKind::Choice | ActionKind::Link => {
                            let is_url = act.kind == ActionKind::Link;
                            let item = ChoiceItem {
                                label: act.label,
                                choice_id: act.choice_id,
                                is_url,
                                rect: (0.0, 0.0, 0.0, 0.0),
                            };
                            if self.playback.active_speaker == Scope::Kero {
                                if !is_url
                                    || !self
                                        .playback
                                        .kero_choices
                                        .iter()
                                        .any(|c| c.choice_id == item.choice_id)
                                {
                                    self.playback.kero_choices.push(item);
                                }
                                self.playback.kero_show = true;
                                self.playback.kero_dirty = true;
                            } else {
                                if !is_url
                                    || !self
                                        .playback
                                        .sakura_choices
                                        .iter()
                                        .any(|c| c.choice_id == item.choice_id)
                                {
                                    self.playback.sakura_choices.push(item);
                                }
                                self.playback.sakura_show = true;
                                self.playback.sakura_dirty = true;
                            }
                        }
                        ActionKind::QuickToggle => {
                            self.playback.quick_mode = !self.playback.quick_mode;
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
        let eff_hidden = self.characters_effectively_hidden();
        let sp_sak = self.speaker_name(Scope::Sakura);
        let sp_ker = self.speaker_name(Scope::Kero);
        if self.playback.sakura_dirty {
            self.playback.sakura_dirty = false;
            let mut need_reposition = self.playback.sakura_show;
            if let (Some(ref gpu), Some(ref mut sb)) = (&self.gpu, &mut self.sakura_balloon) {
                let was_vis = sb.visible;
                sb.text = self.playback.sakura_buf.clone();
                sb.choices = self.playback.sakura_choices.clone();
                sb.large = sb.text.chars().count() > 85 || !sb.choices.is_empty();
                sb.visible = self.playback.sakura_show;
                if sb.visible {
                    if !was_vis {
                        sb.scroll_px = 0.0;
                        sb.user_scrolled_up = false;
                    }
                    let resized = sb.redraw_content(
                        &self.balloon_skin,
                        &sp_sak,
                        true,
                        &mut self.text_renderer,
                        gpu,
                    );
                    if !was_vis && !eff_hidden {
                        sb.ws.window.set_visible(true);
                    }
                    need_reposition = !was_vis || resized;
                } else {
                    sb.ws.window.set_visible(false);
                    need_reposition = false;
                }
            }
            if self.playback.sakura_show && need_reposition && !eff_hidden {
                self.reposition_balloon(Scope::Sakura);
            }
        }
        if self.playback.kero_dirty {
            self.playback.kero_dirty = false;
            let show_kero = self.config_store.get().show_kero;
            let mut need_reposition = self.playback.kero_show && show_kero;
            if let (Some(ref gpu), Some(ref mut kb)) = (&self.gpu, &mut self.kero_balloon) {
                let was_vis = kb.visible;
                kb.text = self.playback.kero_buf.clone();
                kb.choices = self.playback.kero_choices.clone();
                kb.large = kb.text.chars().count() > 75 || !kb.choices.is_empty();
                kb.visible = self.playback.kero_show && show_kero;
                if kb.visible {
                    if !was_vis {
                        kb.scroll_px = 0.0;
                        kb.user_scrolled_up = false;
                    }
                    let resized = kb.redraw_content(
                        &self.balloon_skin,
                        &sp_ker,
                        true,
                        &mut self.text_renderer,
                        gpu,
                    );
                    if !was_vis && !eff_hidden {
                        kb.ws.window.set_visible(true);
                    }
                    need_reposition = !was_vis || resized;
                } else {
                    kb.ws.window.set_visible(false);
                    need_reposition = false;
                }
            }
            if self.playback.kero_show && show_kero && need_reposition && !eff_hidden {
                self.reposition_balloon(Scope::Kero);
            }
        }
    }

    fn poll_cursor_and_strokes(&mut self) {
        let now = Instant::now();
        let cfg_cur = self.config_store.get();
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
            if cfg_cur.click_through_mode {
                if !cw.dragging && cw.last_hittest != Some(false) {
                    if cw.ws.window.set_cursor_hittest(false).is_ok() {
                        cw.last_hittest = Some(false);
                    }
                }
                if cw.hovered_opaque {
                    cw.hovered_opaque = false;
                    if let Some(ref gpu) = self.gpu {
                        cw.ws.set_opacity(gpu, cfg_cur.skin_opacity);
                    }
                }
                cw.stroke_detector.clear_if_stale(now);
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

            let prev_hover = cw.hovered_opaque;
            if let Some((lx, ly)) = local_opt {
                let inside = lx >= 0.0
                    && ly >= 0.0
                    && lx < cw.win_size.0 as f64
                    && ly < cw.win_size.1 as f64;
                if inside {
                    let ds = cw.effective_display_scale();
                    let ix = (lx * ds).floor() as i32;
                    let iy = (ly * ds).floor() as i32;
                    let pixel_opaque = cw.composited.is_opaque_at(ix, iy);
                    cw.hovered_opaque = pixel_opaque;
                    let opaque = cw.dragging || pixel_opaque;
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
                    cw.hovered_opaque = false;
                    cw.stroke_detector.clear_if_stale(now);
                    if !cw.dragging && cw.last_hittest != Some(false) {
                        if cw.ws.window.set_cursor_hittest(false).is_ok() {
                            cw.last_hittest = Some(false);
                        }
                    }
                }
            } else {
                cw.hovered_opaque = false;
            }
            if cfg_cur.opaque_on_hover && cw.hovered_opaque != prev_hover {
                if let Some(ref gpu) = self.gpu {
                    let op = if cw.hovered_opaque {
                        1.0
                    } else {
                        cfg_cur.skin_opacity
                    };
                    cw.ws.set_opacity(gpu, op);
                }
            }
        }
        drop(m_guard);

        if let Some((scope, region)) = stroke_hit {
            match scope {
                Scope::Sakura => {
                    if let Some(ref mut s) = self.sakura {
                        s.trigger_bounce(0.026);
                    }
                }
                Scope::Kero => {
                    if let Some(ref mut k) = self.kero {
                        k.trigger_bounce(0.026);
                    }
                }
            }
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
            if summaries.first().map(|s| s.id.as_str()) != Some("neo_default") {
                return Err(format!(
                    "기본 셸 정렬 오류: neo_default가 첫 번째여야 합니다 (actual={:?})",
                    summaries.first().map(|s| s.id.as_str())
                ));
            }
            let bal_summaries = self.balloon_manager.list_balloons();
            if bal_summaries.first().map(|b| b.id.as_str()) != Some("clean_default") {
                return Err(format!(
                    "기본 벌룬 정렬 오류: clean_default가 첫 번째여야 합니다 (actual={:?})",
                    bal_summaries.first().map(|b| b.id.as_str())
                ));
            }

            let proof_dir = std::env::var_os("NANIKAAI_SMOKE_PROOF_DIR")
                .map(PathBuf::from)
                .or_else(|| self.smoke_user_dir.as_ref().map(|d| d.join("artifacts")))
                .unwrap_or_else(|| std::env::temp_dir().join("nanikaai-smoke-artifacts"));
            let _ = std::fs::create_dir_all(&proof_dir);

            let user_data_smoke = self.config_store.config_dir.clone();
            let assets_smoke = resolve_assets_dir();
            let mut exported_neo = false;
            let mut exported_classic = false;
            for sum in &summaries {
                let mut manifest = self.shell_manager.load_shell(&sum.id)?;
                let ds = manifest.descript.display_scale.clamp(0.5, 4.0);
                sak_win.display_scale = ds;
                ker_win.display_scale = ds;
                sak_win.reload_sidecar_rig(&manifest, &user_data_smoke, &assets_smoke, gpu);
                ker_win.reload_sidecar_rig(&manifest, &user_data_smoke, &assets_smoke, gpu);
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
                    let sak_part_cnt = sak_win.rig_parts.as_ref().map(|r| r.parts.len()).unwrap_or(0);
                    let ker_part_cnt = ker_win.rig_parts.as_ref().map(|r| r.parts.len()).unwrap_or(0);
                    if sak_part_cnt < 18 || ker_part_cnt < 6 {
                        return Err(format!(
                            "neo_default Live2D sidecar part count mismatch: sak={sak_part_cnt}, ker={ker_part_cnt}"
                        ));
                    }
                    sak_win.tick_live2d(gpu, true, Some((180.0, 200.0)), true);
                    ker_win.tick_live2d(gpu, true, Some((180.0, 200.0)), false);
                    sak_win.ws.render(gpu);
                    ker_win.ws.render(gpu);
                    if let Some(ref rig) = sak_win.rig_parts {
                        rig.render_cpu_rgba().save_png(&proof_dir.join("proof-live2d-sakura.png"))?;
                    }
                    if let Some(ref rig) = ker_win.rig_parts {
                        rig.render_cpu_rgba().save_png(&proof_dir.join("proof-live2d-unyu.png"))?;
                    }
                    ker_win.set_surface(&mut manifest, gpu, 11, true);
                    ker_win.tick_live2d(gpu, true, Some((220.0, 160.0)), true);
                    ker_win.ws.render(gpu);
                    if let Some(ref rig) = ker_win.rig_parts {
                        rig.render_cpu_rgba().save_png(&proof_dir.join("proof-live2d-unyu-s11.png"))?;
                    }
                    ker_win.set_surface(&mut manifest, gpu, 10, true);
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

            // Optional local classic-shell visual proof export (.pna / seriko compatibility)
            let mut classic_shell_roots =
                vec![crate::config_store::default_user_data_dir().join("shells")];
            for legacy in crate::config_store::legacy_user_data_dirs() {
                let d = legacy.join("shells");
                if !classic_shell_roots.contains(&d) {
                    classic_shell_roots.push(d);
                }
            }
            classic_shell_roots.retain(|d| d.is_dir());
            if !classic_shell_roots.is_empty() {
                let user_mgr = crate::shell_parser::ShellManager::new(classic_shell_roots);
                for classic_id in ["mayura", "dokuko"] {
                    if let Ok(mut cm) = user_mgr.load_shell(classic_id) {
                        sak_win.display_scale = cm.descript.display_scale.clamp(0.5, 4.0);
                        ker_win.display_scale = sak_win.display_scale;
                        sak_win.set_surface(&mut cm, gpu, 0, true);
                        ker_win.set_surface(&mut cm, gpu, 10, true);
                        let _ = sak_win
                            .composited
                            .save_png(&proof_dir.join(format!("{classic_id}-0.png")));
                        if classic_id == "mayura" {
                            let _ = ker_win
                                .composited
                                .save_png(&proof_dir.join("mayura-10.png"));
                        }
                    }
                }
            }

            // Re-activate default active shell & render dual balloons + export balloon PNG proof
            let active_id = self.config_store.get().active_shell_id;
            let mut active_manifest = self.shell_manager.load_shell(&active_id)?;
            let active_ds = active_manifest.descript.display_scale.clamp(0.5, 4.0);
            sak_win.display_scale = active_ds;
            ker_win.display_scale = active_ds;
            sak_win.reload_sidecar_rig(&active_manifest, &user_data_smoke, &assets_smoke, gpu);
            ker_win.reload_sidecar_rig(&active_manifest, &user_data_smoke, &assets_smoke, gpu);
            sak_win.set_surface(&mut active_manifest, gpu, 0, true);
            ker_win.set_surface(&mut active_manifest, gpu, 10, true);
            *self.manifest.lock().expect("manifest mutex poisoned") = active_manifest;
            self.reset_character_positions();

            // Phase A: short greeting -> compact dynamic balloon (< 120 pt tall)
            if let Some(ref mut sb) = self.sakura_balloon {
                sb.scroll_px = 0.0;
                sb.user_scrolled_up = false;
                sb.utterance_min_w = 0;
                sb.large = false;
            }
            let short_script = "\\0\\s[0]안녕! 何か藍(NanikaAi) v0.1.11!\\e";
            self.playback.enqueue(parse_dialogue(short_script));
            let _ = self.playback.fast_forward();
            self.sync_balloons_to_playback();
            let sb_short_h = self
                .sakura_balloon
                .as_ref()
                .ok_or("사쿠라 말풍선 창 없음")?
                .win_size
                .1;
            if sb_short_h >= 120 {
                return Err(format!("단문 말풍선 동적 축소 실패: h={sb_short_h}"));
            }
            self.sakura_balloon
                .as_ref()
                .ok_or("사쿠라 말풍선 창 없음")?
                .last_render_bmp
                .save_png(&proof_dir.join("proof-balloon-short.png"))?;

            // Phase B: long multi-paragraph text -> capped max height + scrollbar
            if let Some(ref mut sb) = self.sakura_balloon {
                sb.scroll_px = 0.0;
                sb.user_scrolled_up = false;
                sb.utterance_min_w = 0;
                sb.large = false;
            }
            let demo_script = "\\0\\s[0]안녕! 100% Safe Rust 何か藍(NanikaAi) v0.1.11 テスト完了だよ！\\n\\n長い文章では吹き出しが内容に合わせて滑らかに広がり、最大サイズに達すると自動的にスクロールバーとトラックが表示されて、マウスホイールで快適に読み進められます。\\n\\n1行目: 外付けサイドカー Live2D 解剖学的関節・多層髪・衣装揺れ有効化。\\n2行目: うにゅうの40%半透明サングラスと無欠損表情レイヤーを保持。\\n3行目: Gemma/SentencePieceのバイトフォールバック文字も完全復元。\\w3\\1\\s[10]40%半透明サングラスの上からもバッチリ見えてるぜ。\\e";
            self.playback.enqueue(parse_dialogue(demo_script));
            let _ = self.playback.fast_forward();
            self.sync_balloons_to_playback();

            let (sb_long_h, sb_max_scroll) = {
                let sb = self.sakura_balloon.as_ref().ok_or("사쿠라 말풍선 창 없음")?;
                sb.last_render_bmp
                    .save_png(&proof_dir.join("native-balloon-sakura.png"))?;
                sb.last_render_bmp
                    .save_png(&proof_dir.join("neo-balloon-sakura.png"))?;
                sb.last_render_bmp
                    .save_png(&proof_dir.join("proof-balloon-scroll.png"))?;
                (sb.win_size.1, sb.max_scroll_px)
            };
            if sb_long_h < 270 || sb_max_scroll <= 40.0 {
                return Err(format!(
                    "장문 말풍선 확장/스크롤 검증 실패: h={sb_long_h}, max_scroll={sb_max_scroll:.1}"
                ));
            }

            // Phase C: Communicate Siri-style sapphire input bar proofs
            let smoke_ui_lang = self.effective_ui_lang();
            if let Some(ref mut cw) = self.communicate {
                cw.focused = true;
                cw.cursor_on = true;
                cw.text.clear();
                cw.ime_preedit.clear();
                cw.render_bitmap(&mut self.text_renderer, 2.0, 0.35, smoke_ui_lang)
                    .save_png(&proof_dir.join("proof-comm-empty.png"))?;
                cw.text = "入力".to_string();
                cw.ime_preedit = "中です".to_string();
                cw.render_bitmap(&mut self.text_renderer, 2.0, 1.10, smoke_ui_lang)
                    .save_png(&proof_dir.join("proof-comm-ime.png"))?;
                cw.text.clear();
                cw.ime_preedit.clear();
            }

            // Phase D: Right-click Menu visual & 1-click default skin restore proof
            let menu_rows = self.build_menu_rows();
            if !menu_rows
                .iter()
                .any(|r| matches!(r.cmd, MenuCommand::ResetDefaultSkin))
            {
                return Err("우클릭 메뉴에 기본 스킨 복구 항목(ResetDefaultSkin)이 없습니다.".to_string());
            }
            if let Some(ref mut m) = self.menu {
                m.rows = menu_rows;
                m.scroll_row = 0;
                m.scroll_accum = 0.0;
                m.hovered_idx = Some(1);
                m.max_visible_rows = 24;
                let (_, menu_bmp) = m.render_bitmap(&mut self.text_renderer, 2.0);
                menu_bmp.save_png(&proof_dir.join("proof-menu.png"))?;
            }

            // Phase E: Interactive GUI Operability & <= 3,000 ms Responsiveness SLA Verification
            const SLA_MS: u128 = 3000;
            let check_sla = |label: &str, started: Instant| -> Result<u128, String> {
                let ms = started.elapsed().as_millis();
                if ms > SLA_MS {
                    Err(format!("反応性SLA超過 ({label}): {ms}ms > {SLA_MS}ms"))
                } else {
                    Ok(ms)
                }
            };
            let sak_id = self.sakura.as_ref().ok_or("사쿠라 창 없음")?.ws.window.id();
            let sb_id = self
                .sakura_balloon
                .as_ref()
                .ok_or("사쿠라 말풍선 창 없음")?
                .ws
                .window
                .id();
            let comm_id = self.communicate.as_ref().ok_or("입력창 없음")?.ws.window.id();
            let menu_id = self.menu.as_ref().ok_or("메뉴 창 없음")?.ws.window.id();

            let closed_port = std::net::TcpListener::bind("127.0.0.1:0")
                .and_then(|l| l.local_addr())
                .map(|a| a.port())
                .unwrap_or(9);
            self.config_store.update(ConfigPatch {
                provider: Some("ollama".to_string()),
                model: Some("gemma4:e4b".to_string()),
                endpoint_url: Some(format!("http://127.0.0.1:{closed_port}")),
                ..Default::default()
            });

            let ui_t0 = Instant::now();

            // E1: Right-click menu open, hover hit-test, wheel scroll, and click command execution
            let t_menu = Instant::now();
            self.handle_mouse_button(event_loop, sak_id, MouseButton::Right, ElementState::Pressed);
            let menu_row_h = {
                let m = self.menu.as_ref().ok_or("메뉴 창 없음")?;
                if !m.visible || m.rows.len() < 10 {
                    return Err("右クリックメニュー展開失敗".to_string());
                }
                m.row_h
            };
            check_sla("右クリックメニュー表示", t_menu)?;

            self.handle_pointer_move(menu_id, (60.0, 7.0 + menu_row_h * 1.5));
            if self.menu.as_ref().and_then(|m| m.hovered_idx) != Some(1) {
                return Err("メニュー行ホバーヒット判定失敗".to_string());
            }
            if let Some(ref mut m) = self.menu {
                m.max_visible_rows = 8;
                if let Some(ref gpu) = self.gpu {
                    m.redraw(&mut self.text_renderer, gpu);
                }
            }
            self.handle_scroll(menu_id, -78.0);
            if self.menu.as_ref().map(|m| m.scroll_row).unwrap_or(0) == 0 {
                return Err("メニューホイールスクロール失敗".to_string());
            }
            let kero_idx = {
                let m = self.menu.as_mut().ok_or("메뉴 창 없음")?;
                m.max_visible_rows = 24;
                m.scroll_row = 0;
                m.scroll_accum = 0.0;
                if let Some(ref gpu) = self.gpu {
                    m.redraw(&mut self.text_renderer, gpu);
                }
                m.rows
                    .iter()
                    .position(|r| matches!(r.cmd, MenuCommand::ToggleKero))
                    .ok_or("ToggleKero メニュー行なし")?
            };
            self.handle_pointer_move(menu_id, (60.0, 7.0 + menu_row_h * (kero_idx as f64 + 0.5)));
            self.handle_mouse_button(event_loop, menu_id, MouseButton::Left, ElementState::Released);
            if self.menu.as_ref().is_some_and(|m| m.visible) {
                return Err("メニュークリック後にメニューが閉じませんでした".to_string());
            }
            if self.kero.as_ref().is_some_and(|k| k.visible) {
                return Err("メニュー ToggleKero 実行が反映されませんでした".to_string());
            }
            self.execute_menu_command(event_loop, MenuCommand::ToggleKero);
            if !self.kero.as_ref().is_some_and(|k| k.visible) {
                return Err("ToggleKero 復元失敗".to_string());
            }

            // E2: Character double-click (outside head/face collision boxes) opens Communicate bar
            let t_dbl = Instant::now();
            self.handle_pointer_move(sak_id, (8.0, 8.0));
            self.handle_mouse_button(event_loop, sak_id, MouseButton::Left, ElementState::Pressed);
            self.handle_mouse_button(event_loop, sak_id, MouseButton::Left, ElementState::Released);
            self.handle_mouse_button(event_loop, sak_id, MouseButton::Left, ElementState::Pressed);
            self.handle_mouse_button(event_loop, sak_id, MouseButton::Left, ElementState::Released);
            if !self.communicate.as_ref().is_some_and(|c| c.visible && c.focused) {
                return Err("キャラクターダブルクリックで入力バーが開きませんでした".to_string());
            }
            check_sla("ダブルクリック入力バー展開", t_dbl)?;

            // E3: Communicate IME Preedit/Commit, Keyboard edits, Close(×), Escape, and Send(↑) reaction
            self.handle_ime_event(comm_id, Ime::Preedit("てすと".to_string(), None));
            if self.communicate.as_ref().map(|c| c.ime_preedit.as_str()) != Some("てすと") {
                return Err("IME Preedit 反映失敗".to_string());
            }
            self.handle_ime_event(comm_id, Ime::Commit("テスト".to_string()));
            if !self
                .communicate
                .as_ref()
                .is_some_and(|c| c.ime_preedit.is_empty() && c.text == "テスト")
            {
                return Err("IME Commit 反映失敗".to_string());
            }
            self.handle_communicate_key(&Key::Named(NamedKey::Backspace), None);
            self.handle_communicate_key(&Key::Character("ト!".into()), Some("ト!"));
            if self.communicate.as_ref().map(|c| c.text.as_str()) != Some("テスト!") {
                return Err("キーボード編集 (Backspace/入力) 反映失敗".to_string());
            }
            let close_center = {
                let cw = self.communicate.as_ref().ok_or("입력창 없음")?;
                (
                    (cw.close_rect.0 + cw.close_rect.2) * 0.5,
                    (cw.close_rect.1 + cw.close_rect.3) * 0.5,
                )
            };
            self.handle_pointer_move(comm_id, close_center);
            if !self.communicate.as_ref().is_some_and(|c| c.hovered_close) {
                return Err("入力バー閉じるボタン(×)ホバー失敗".to_string());
            }
            self.handle_mouse_button(event_loop, comm_id, MouseButton::Left, ElementState::Pressed);
            if self.communicate.as_ref().is_some_and(|c| c.visible) {
                return Err("入力バー閉じるボタン(×)クリックで閉じませんでした".to_string());
            }
            self.execute_menu_command(event_loop, MenuCommand::OpenCommunicate);
            self.handle_communicate_key(&Key::Named(NamedKey::Escape), None);
            if self.communicate.as_ref().is_some_and(|c| c.visible) {
                return Err("Escape キーで入力バーが閉じませんでした".to_string());
            }

            if let Some(ref mut sb) = self.sakura_balloon {
                sb.visible = false;
                sb.status_line.clear();
            }
            self.execute_menu_command(event_loop, MenuCommand::OpenCommunicate);
            self.handle_communicate_key(&Key::Character("こんにちは".into()), Some("こんにちは"));
            let send_center = {
                let cw = self.communicate.as_ref().ok_or("입력창 없음")?;
                (
                    (cw.send_rect.0 + cw.send_rect.2) * 0.5,
                    (cw.send_rect.1 + cw.send_rect.3) * 0.5,
                )
            };
            self.handle_pointer_move(comm_id, send_center);
            if !self.communicate.as_ref().is_some_and(|c| c.hovered_send) {
                return Err("入力バー送信ボタン(↑)ホバー失敗".to_string());
            }
            let t_send = Instant::now();
            self.handle_mouse_button(event_loop, comm_id, MouseButton::Left, ElementState::Pressed);
            if self.communicate.as_ref().is_some_and(|c| c.visible) {
                return Err("送信ボタンクリック後に入力バーが閉じませんでした".to_string());
            }
            let expected_thinking = self.ui_strings().status_thinking;
            if !self
                .sakura_balloon
                .as_ref()
                .is_some_and(|b| b.visible && b.status_line.contains(expected_thinking))
            {
                return Err("送信ボタン押下後の即時応答ステータス（…表示）が反映されませんでした".to_string());
            }
            check_sla("送信ボタン即時視覚反応", t_send)?;

            // E4: Balloon fast-forward, choice hover, scroll, choice click, dismiss & reset
            if let Some(ref mut sb) = self.sakura_balloon {
                sb.status_line.clear();
                sb.user_scrolled_up = false;
                sb.scroll_px = 0.0;
            }
            let op_script = "\\0\\s[5]操作性テスト開始！\\n\\n行1\\n行2\\n行3\\n行4\\n行5\\n行6\\n行7\\n行8\\n行9\\n行10\\n行11\\n行12\\n行13\\n行14\\n行15\\n行16\\q[選択肢A,opt_a]\\q[選択肢B,opt_b]\\e";
            self.playback.enqueue(parse_dialogue(op_script));
            self.tick_playback();
            if self.playback.steps.is_empty() {
                return Err("タイプライター再生前ステップが空です".to_string());
            }
            self.handle_mouse_button(event_loop, sb_id, MouseButton::Left, ElementState::Pressed);
            if !self.playback.steps.is_empty() {
                return Err("吹き出しクリックによる文字早送り(fast_forward)に失敗しました".to_string());
            }
            let (ch0_center, max_scr) = {
                let sb = self.sakura_balloon.as_ref().ok_or("사쿠라 말풍선 창 없음")?;
                if sb.choices.len() != 2 {
                    return Err(format!("吹き出し選択肢数が不正: {}", sb.choices.len()));
                }
                let r = sb.choices[0].rect;
                (((r.0 + r.2) * 0.5, (r.1 + r.3) * 0.5), sb.max_scroll_px)
            };
            self.handle_pointer_move(sb_id, ch0_center);
            if self.sakura_balloon.as_ref().and_then(|b| b.hovered_choice) != Some(0) {
                return Err("吹き出し選択肢ホバー判定に失敗しました".to_string());
            }
            if max_scr > 20.0 {
                self.handle_scroll(sb_id, 60.0);
                let after_up = self.sakura_balloon.as_ref().map(|b| (b.scroll_px, b.user_scrolled_up));
                if !after_up.is_some_and(|(px, up)| up && px < max_scr) {
                    return Err("吹き出し上方向ホイールスクロールに失敗しました".to_string());
                }
                self.handle_scroll(sb_id, -60.0);
                let after_down = self.sakura_balloon.as_ref().map(|b| b.scroll_px).unwrap_or(0.0);
                if (after_down - max_scr).abs() > 1.5 {
                    return Err("吹き出し下方向ホイールスクロール復元失敗".to_string());
                }
            }
            self.handle_mouse_button(event_loop, sb_id, MouseButton::Left, ElementState::Pressed);
            if !self.playback.sakura_choices.is_empty() {
                return Err("選択肢クリック後の choices クリア失敗".to_string());
            }
            let expected_thinking = self.ui_strings().status_thinking;
            if !self
                .sakura_balloon
                .as_ref()
                .is_some_and(|b| b.visible && b.status_line.contains(expected_thinking))
            {
                return Err("選択肢クリック後の即時反応表示失敗".to_string());
            }
            self.handle_pointer_move(sb_id, (2.0, 2.0));
            if let Some(ref mut sb) = self.sakura_balloon {
                sb.choices.clear();
                sb.hovered_choice = None;
            }
            self.playback.steps.clear();
            self.handle_mouse_button(event_loop, sb_id, MouseButton::Left, ElementState::Pressed);
            if self.sakura_balloon.as_ref().is_some_and(|b| b.visible) {
                return Err("吹き出しクリック非表示(dismiss)に失敗しました".to_string());
            }

            // E5: Dress-up bindgroup toggle pixel overlay + restore check
            let count_opaque = |bmp: &RgbaBitmap| -> usize {
                bmp.data.chunks_exact(4).filter(|px| px[3] >= 16).count()
            };
            let before_opaque = count_opaque(
                &self.sakura.as_ref().ok_or("사쿠라 창 없음")?.composited,
            );
            self.execute_menu_command(event_loop, MenuCommand::ToggleBind(50));
            let after_bmp = &self.sakura.as_ref().ok_or("사쿠라 창 없음")?.composited;
            let after_opaque = count_opaque(after_bmp);
            if after_opaque <= before_opaque {
                return Err(format!(
                    "着せ替え(bindgroup 50)合成ピクセル増加に失敗: before={before_opaque}, after={after_opaque}"
                ));
            }
            after_bmp.save_png(&proof_dir.join("proof-dressup-sakura.png"))?;
            self.execute_menu_command(event_loop, MenuCommand::ToggleBind(50));
            let restored_opaque = count_opaque(
                &self.sakura.as_ref().ok_or("사쿠라 창 없음")?.composited,
            );
            if restored_opaque != before_opaque {
                return Err(format!(
                    "着せ替え(bindgroup 50)解除復元失敗: expected={before_opaque}, got={restored_opaque}"
                ));
            }

            // E6: Quick mode (\_q) instant drain + clickable \_a[url] dry-run click
            let link_script = r"\_q\0\s[5]クイックリンク確認: \_a[https://github.com/JungWonYong/nanikaai]公式リポジトリ\_a\_q\e";
            if let Some(ref mut sb) = self.sakura_balloon {
                sb.status_line.clear();
                sb.hovered_choice = None;
            }
            self.playback.enqueue(parse_dialogue(link_script));
            self.tick_playback();
            if !self.playback.steps.is_empty() {
                return Err("\\_q 一括即時表示が 1 tick で完了しませんでした".to_string());
            }
            let link_center = {
                let sb = self.sakura_balloon.as_ref().ok_or("사쿠라 말풍선 창 없음")?;
                if sb.choices.len() != 1 || !sb.choices[0].is_url {
                    return Err(format!(
                        "\\_a リンク生成失敗: count={}",
                        sb.choices.len()
                    ));
                }
                let r = sb.choices[0].rect;
                ((r.0 + r.2) * 0.5, (r.1 + r.3) * 0.5)
            };
            self.handle_pointer_move(sb_id, link_center);
            self.handle_mouse_button(event_loop, sb_id, MouseButton::Left, ElementState::Pressed);
            let expected_link_prefix = self.ui_strings().status_link_opened_prefix.trim();
            if !self
                .sakura_balloon
                .as_ref()
                .is_some_and(|b| b.status_line.contains(expected_link_prefix))
            {
                return Err("リンククリックのブラウザ起動dry-runステータス検証に失敗しました".to_string());
            }

            // E7: Compact Menu Accordion & Native Settings Panel Operability
            let t_settings = Instant::now();
            self.handle_mouse_button(event_loop, sak_id, MouseButton::Right, ElementState::Pressed);
            let collapsed_len = {
                let m = self.menu.as_ref().ok_or("메뉴 창 없음")?;
                if !m.visible || m.rows.len() > 20 {
                    return Err(format!(
                        "右クリックメニューが折りたたみ表示されませんでした: rows={}",
                        m.rows.len()
                    ));
                }
                m.rows.len()
            };
            let skins_sec_idx = self
                .menu
                .as_ref()
                .and_then(|m| {
                    m.rows
                        .iter()
                        .position(|r| matches!(r.cmd, MenuCommand::ToggleMenuSection(MenuSection::Skins)))
                })
                .ok_or("右クリックメニューに Skins セクション行がありません")?;
            self.handle_pointer_move(
                menu_id,
                (60.0, 7.0 + menu_row_h * (skins_sec_idx as f64 + 0.5)),
            );
            self.handle_mouse_button(event_loop, menu_id, MouseButton::Left, ElementState::Released);
            if !self
                .menu
                .as_ref()
                .is_some_and(|m| m.visible && m.rows.len() > collapsed_len)
            {
                return Err("メニューアコーディオン展開に失敗しました".to_string());
            }
            self.hide_menu();

            self.execute_menu_command(event_loop, MenuCommand::OpenSettings);
            let settings_id = {
                let sw = self.settings.as_ref().ok_or("設定パネルウィンドウなし")?;
                if !sw.visible {
                    return Err("OpenSettings 後に設定パネルが表示されませんでした".to_string());
                }
                if sw.last_render_bmp.count_opaque_pixels() <= 50_000 {
                    return Err(format!(
                        "設定パネル描画ピクセル不足: {}",
                        sw.last_render_bmp.count_opaque_pixels()
                    ));
                }
                sw.last_render_bmp
                    .save_png(&proof_dir.join("proof-settings-appearance.png"))?;
                sw.ws.window.id()
            };

            let pt_slider = self
                .settings
                .as_ref()
                .and_then(|sw| sw.model.slider_point(SliderKind::SkinOpacity, 0.65))
                .ok_or("SkinOpacity スライダーのヒット領域が見つかりません")?;
            self.handle_pointer_move(settings_id, pt_slider);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Pressed);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Released);
            if self.config_store.get().skin_opacity >= 0.85 {
                return Err(format!(
                    "SkinOpacity スライダークリックが config に反映されませんでした: {}",
                    self.config_store.get().skin_opacity
                ));
            }
            if self.sakura.as_ref().is_some_and(|s| s.ws.opacity >= 0.85) {
                return Err("SkinOpacity スライダー変更がキャラウィンドウ GPU opacity に反映されませんでした".to_string());
            }

            let pt_tab1 = self
                .settings
                .as_ref()
                .and_then(|sw| sw.model.target_center("tab_1"))
                .ok_or("設定パネル tab_1 が見つかりません")?;
            self.handle_pointer_move(settings_id, pt_tab1);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Pressed);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Released);
            if let Some(ref sw) = self.settings {
                sw.last_render_bmp
                    .save_png(&proof_dir.join("proof-settings-os.png"))?;
            }
            for (expected_lang, proof_name) in [
                ("ja", None),
                ("ko", Some("proof-settings-os-ko.png")),
                ("en", Some("proof-settings-os-en.png")),
            ] {
                let pt_lang = self
                    .settings
                    .as_ref()
                    .and_then(|sw| sw.model.target_center("cyc_ui_lang_next"))
                    .ok_or("設定パネル cyc_ui_lang_next が見つかりません")?;
                self.handle_pointer_move(settings_id, pt_lang);
                self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Pressed);
                self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Released);
                let got_lang = self.config_store.get().ui_language;
                if got_lang != expected_lang {
                    return Err(format!(
                        "UI 言語切替 (cyc_ui_lang_next) 不一致: expected={expected_lang}, got={got_lang}"
                    ));
                }
                if let Some(file_name) = proof_name {
                    if let Some(ref sw) = self.settings {
                        sw.last_render_bmp.save_png(&proof_dir.join(file_name))?;
                    }
                }
            }

            // Cycle 1 more time back to "auto" (OS default) and inspect the new Focus/Utilities section
            let pt_lang_back = self
                .settings
                .as_ref()
                .and_then(|sw| sw.model.target_center("cyc_ui_lang_next"))
                .ok_or("設定パネル cyc_ui_lang_next (return) が見つかりません")?;
            self.handle_pointer_move(settings_id, pt_lang_back);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Pressed);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Released);

            self.handle_scroll(settings_id, -312.0);
            if let Some(ref sw) = self.settings {
                sw.last_render_bmp
                    .save_png(&proof_dir.join("proof-settings-os-accessories.png"))?;
            }

            let click_target = |app: &mut Self, key: &str| -> Result<(), String> {
                let pt = app
                    .settings
                    .as_ref()
                    .and_then(|sw| sw.model.target_center(key))
                    .ok_or_else(|| format!("設定パネル {key} が見つかりません"))?;
                app.handle_pointer_move(settings_id, pt);
                app.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Pressed);
                app.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Released);
                Ok(())
            };

            click_target(self, "tog_quiet_mode")?;
            if !self.config_store.get().quiet_mode {
                return Err("tog_quiet_mode で quiet_mode が ON になりませんでした".to_string());
            }
            click_target(self, "btn_pomo_toggle")?;
            if !self.pomodoro.is_focusing() {
                return Err("btn_pomo_toggle でポモドーロ集中が開始されませんでした".to_string());
            }
            click_target(self, "btn_pomo_stop")?;
            if self.pomodoro.is_active() {
                return Err("btn_pomo_stop でポモドーロが停止されませんでした".to_string());
            }
            self.backlog.push_from_actions(
                &parse_dialogue("\\0\\s[5]バックログ検証メッセージ\\e"),
                "08:00:00".to_string(),
            );
            click_target(self, "btn_show_backlog")?;
            click_target(self, "btn_copy_last")?;
            let expected_copied = self.ui_strings().status_copied_reply;
            if !self
                .sakura_balloon
                .as_ref()
                .is_some_and(|b| b.status_line.contains(expected_copied))
            {
                return Err("btn_copy_last 実行後のコピー通知ステータスが反映されませんでした".to_string());
            }
            click_target(self, "btn_hide_chars")?;
            if !self.characters_hidden_by_user {
                return Err("btn_hide_chars で一時非表示になりませんでした".to_string());
            }
            click_target(self, "btn_hide_chars")?;
            if self.characters_hidden_by_user {
                return Err("btn_hide_chars 再クリックでキャラが復帰しませんでした".to_string());
            }
            click_target(self, "btn_next_monitor")?;

            self.handle_scroll(settings_id, -90.0);
            click_target(self, "cyc_power_save_next")?;
            if self.config_store.get().power_save_mode != "on" {
                return Err(format!(
                    "cyc_power_save_next が反映されませんでした: {}",
                    self.config_store.get().power_save_mode
                ));
            }
            click_target(self, "tog_low_batt_alert")?;
            if self.config_store.get().low_battery_alert {
                return Err("tog_low_batt_alert のトグルが反映されませんでした".to_string());
            }

            let pt_tab3 = self
                .settings
                .as_ref()
                .and_then(|sw| sw.model.target_center("tab_3"))
                .ok_or("設定パネル tab_3 が見つかりません")?;
            self.handle_pointer_move(settings_id, pt_tab3);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Pressed);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Released);

            let pt_autotalk = self
                .settings
                .as_ref()
                .and_then(|sw| sw.model.slider_point(SliderKind::AutoTalkInterval, 0.06))
                .ok_or("AutoTalkInterval スライダーのヒット領域が見つかりません")?;
            self.handle_pointer_move(settings_id, pt_autotalk);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Pressed);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Released);
            let cur_interval = self.config_store.get().auto_talk_interval_sec;
            if !(30..=60).contains(&cur_interval) {
                return Err(format!(
                    "AutoTalkInterval スライダー変更が config に反映されませんでした: {cur_interval}"
                ));
            }

            let pt_weight = self
                .settings
                .as_ref()
                .and_then(|sw| sw.model.slider_point(SliderKind::InterestWeight, 0.85))
                .ok_or("InterestWeight スライダーのヒット領域が見つかりません")?;
            self.handle_pointer_move(settings_id, pt_weight);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Pressed);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Released);
            let cur_weight = self.config_store.get().interest_news_weight;
            if !(0.60..=0.80).contains(&cur_weight) {
                return Err(format!(
                    "InterestWeight スライダー変更が config に反映されませんでした: {cur_weight}"
                ));
            }

            if let Some(ref sw) = self.settings {
                sw.last_render_bmp
                    .save_png(&proof_dir.join("proof-settings-ai.png"))?;
            }

            let pt_setup_ai = self
                .settings
                .as_ref()
                .and_then(|sw| sw.model.target_center("btn_setup_local_ai"))
                .ok_or("設定パネル btn_setup_local_ai が見つかりません")?;
            self.handle_pointer_move(settings_id, pt_setup_ai);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Pressed);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Released);
            if self.config_store.get().provider != "ollama" {
                return Err(format!(
                    "btn_setup_local_ai で provider が ollama になりませんでした: {}",
                    self.config_store.get().provider
                ));
            }
            let expected_dry_run = self.ui_strings().status_local_ai_dry_run;
            if !self
                .sakura_balloon
                .as_ref()
                .is_some_and(|b| b.status_line.contains(expected_dry_run))
            {
                return Err(format!(
                    "btn_setup_local_ai 実行後の status_line が期待値と異なります: {:?}",
                    self.sakura_balloon.as_ref().map(|b| &b.status_line)
                ));
            }

            let pt_reset = self
                .settings
                .as_ref()
                .and_then(|sw| sw.model.target_center("btn_reset_defaults"))
                .ok_or("設定パネル btn_reset_defaults が見つかりません")?;
            self.handle_pointer_move(settings_id, pt_reset);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Pressed);
            self.handle_mouse_button(event_loop, settings_id, MouseButton::Left, ElementState::Released);
            let reset_cfg = self.config_store.get();
            if (reset_cfg.skin_opacity - 1.0).abs() > 0.01 {
                return Err(format!(
                    "設定パネルの初期値復元 (btn_reset_defaults) が機能しませんでした: {}",
                    reset_cfg.skin_opacity
                ));
            }
            if reset_cfg.auto_talk_interval_sec != 180
                || (reset_cfg.interest_news_weight - 0.50).abs() > 0.02
                || reset_cfg.ui_language != "auto"
                || reset_cfg.quiet_mode
                || !reset_cfg.auto_hide_on_fullscreen
                || reset_cfg.power_save_mode != "auto"
                || reset_cfg.pomodoro_preset_idx != 0
            {
                return Err(format!(
                    "btn_reset_defaults で既定値（雑談/関心/UI言語/アクセサリ設定）が復元されませんでした: interval={}, weight={}, ui_language={}, quiet={}, ps={}",
                    reset_cfg.auto_talk_interval_sec,
                    reset_cfg.interest_news_weight,
                    reset_cfg.ui_language,
                    reset_cfg.quiet_mode,
                    reset_cfg.power_save_mode
                ));
            }

            self.window_event(event_loop, settings_id, WindowEvent::CloseRequested);
            if self.settings.as_ref().is_some_and(|sw| sw.visible) {
                return Err("CloseRequested で設定パネルが閉じませんでした".to_string());
            }
            if self.closing_at.is_some() {
                return Err("設定パネル閉じでアプリ全体が終了遷移しました".to_string());
            }
            self.execute_menu_command(event_loop, MenuCommand::OpenSettings);
            self.handle_settings_key(&Key::Named(NamedKey::Escape));
            if self.settings.as_ref().is_some_and(|sw| sw.visible) {
                return Err("Escape キーで設定パネルが閉じませんでした".to_string());
            }
            check_sla("折りたたみメニュー・設定パネル操作", t_settings)?;

            self.execute_menu_command(event_loop, MenuCommand::ResetPositions);
            self.execute_menu_command(event_loop, MenuCommand::ResetDefaultSkin);
            let ui_ops_ms = check_sla("GUI全操作スイート合計", ui_t0)?;

            // Exercise HTTP GET /api/health, POST /api/speak, and raw TCP SSTP/1.4 within SLA
            let ipc_t0 = Instant::now();
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
            let ipc_ms = check_sla("HTTP/SSTP制御応答合計", ipc_t0)?;

            let safe_area = self.desktop_safe_work_area();
            Ok(format!(
                "[SMOKE PASS] unsafe=0 shells=[{}] scale={} safe=({:.0},{:.0},{:.0},{:.0}) http={} tcp={} ui_ms={} ipc_ms={}",
                checked_shells.join(", "),
                active_ds,
                safe_area.0,
                safe_area.1,
                safe_area.2,
                safe_area.3,
                srv.http_port,
                srv.tcp_port,
                ui_ops_ms,
                ipc_ms
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
                if let Some(p) = std::env::var_os("NANIKAAI_SMOKE_LOG") {
                    let _ = std::fs::write(p, format!("{summary}\n"));
                }
                if let Ok(mut g) = self.smoke_passed.lock() {
                    *g = true;
                }
            }
            Err(err) => {
                eprintln!("[SMOKE FAIL] {err}");
                if let Some(p) = std::env::var_os("NANIKAAI_SMOKE_LOG") {
                    let _ = std::fs::write(p, format!("[SMOKE FAIL] {err}\n"));
                }
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
        let (init_ds, init_binds, sak_init, kero_init) = {
            let m = self.manifest.lock().expect("manifest mutex poisoned");
            let ds = m.descript.display_scale.clamp(0.5, 4.0);
            let binds = self
                .config_store
                .get_active_binds(&cfg.active_shell_id, &m.default_bind_ids());
            let s = m
                .compose_surface_rgba(0, &binds)
                .unwrap_or_else(|| RgbaBitmap::from_solid(560, 960, 120, 150, 220, 220));
            let k = m
                .compose_surface_rgba(10, &binds)
                .unwrap_or_else(|| RgbaBitmap::from_solid(400, 500, 150, 180, 230, 220));
            (ds, binds, s, k)
        };
        let init_user_scale = (cfg.character_scale as f64).clamp(0.50, 1.50);
        let init_eff_ds = (init_ds / init_user_scale).clamp(0.25, 8.0);
        let sak_lw = ((sak_init.width as f64) / init_eff_ds).round().max(40.0) as u32;
        let sak_lh = ((sak_init.height as f64) / init_eff_ds).round().max(40.0) as u32;
        let kero_lw = ((kero_init.width as f64) / init_eff_ds).round().max(40.0) as u32;
        let kero_lh = ((kero_init.height as f64) / init_eff_ds).round().max(40.0) as u32;

        let char_shadow = {
            #[cfg(target_os = "macos")]
            {
                cfg.macos_window_shadow
            }
            #[cfg(target_os = "windows")]
            {
                cfg.windows_undecorated_shadow
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            {
                false
            }
        };
        let instance = wgpu::Instance::default();
        let sak_win = match Self::create_borderless_window_opts(
            event_loop,
            "NanikaAi Sakura",
            sak_lw,
            sak_lh,
            cfg.always_on_top,
            false,
            true,
            char_shadow,
            cfg.macos_accept_first_mouse,
            cfg.windows_skip_taskbar,
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

        let kero_ws = Self::create_borderless_window_opts(
            event_loop,
            "NanikaAi Kero",
            kero_lw,
            kero_lh,
            cfg.always_on_top,
            false,
            cfg.show_kero,
            char_shadow,
            cfg.macos_accept_first_mouse,
            cfg.windows_skip_taskbar,
        )
        .ok()
        .and_then(|win| {
            gpu._instance
                .create_surface(Arc::clone(&win))
                .ok()
                .map(|surf| WindowSurface::from_existing(surf, win, &gpu))
        });
        let sak_bal_ws = make_ws("NanikaAi Balloon Sakura", 260, 108, cfg.always_on_top, false, false);
        let kero_bal_ws = make_ws("NanikaAi Balloon Kero", 240, 96, cfg.always_on_top, false, false);
        let comm_ws = make_ws("NanikaAi Communicate", 452, 92, true, true, false);
        let menu_ws = make_ws("NanikaAi Menu", 304, 420, true, true, false);
        let settings_ws = Self::create_borderless_window_opts(
            event_loop,
            "NanikaAi Settings",
            SETTINGS_LOGICAL_W,
            SETTINGS_LOGICAL_H,
            true,
            true,
            false,
            true,
            true,
            false,
        )
        .ok()
        .and_then(|win| {
            gpu._instance
                .create_surface(Arc::clone(&win))
                .ok()
                .map(|surf| WindowSurface::from_existing(surf, win, &gpu))
        });

        self.sakura = Some(CharacterWindowState {
            scope: Scope::Sakura,
            ws: sak_ws,
            surface_id: 0,
            base_bmp: None,
            overlays: HashMap::new(),
            composited: sak_init.clone(),
            win_size: (sak_lw, sak_lh),
            display_scale: init_ds,
            user_scale: init_user_scale,
            hovered_opaque: false,
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
            anim_start: Instant::now(),
            phase_offset: 0.0,
            smooth_gaze: (0.0, 0.0),
            bounce_at: None,
            bounce_strength: 0.0,
            anchor_v: 0.96,
            was_speaking: false,
            visible: true,
            top_v: 0.02,
            center_u: 0.50,
            aspect_hw: 1.5,
            rig: Live2dRigState::new(),
            rig_parts: None,
            active_binds: init_binds.clone(),
            bind_overlays: Vec::new(),
            is_neo_default: cfg.active_shell_id == "neo_default",
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
                user_scale: init_user_scale,
                hovered_opaque: false,
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
                anim_start: Instant::now(),
                phase_offset: 1.85,
                smooth_gaze: (0.0, 0.0),
                bounce_at: None,
                bounce_strength: 0.0,
                anchor_v: 0.96,
                was_speaking: false,
                visible: cfg.show_kero,
                top_v: 0.02,
                center_u: 0.50,
                aspect_hw: 1.5,
                rig: Live2dRigState::new(),
                rig_parts: None,
                active_binds: init_binds,
                bind_overlays: Vec::new(),
                is_neo_default: cfg.active_shell_id == "neo_default",
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
                user_scrolled_up: false,
                utterance_min_w: 0,
                win_size: (260, 108),
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
                user_scrolled_up: false,
                utterance_min_w: 0,
                win_size: (240, 96),
                last_render_bmp: RgbaBitmap::new_transparent(16, 16),
            });
        }
        if let Some(cws) = comm_ws {
            self.communicate = Some(CommunicateWindowState {
                ws: cws,
                visible: false,
                focused: false,
                text: String::new(),
                ime_preedit: String::new(),
                win_size: (452, 92),
                cursor_on: true,
                last_blink: Instant::now(),
                anim_start: Instant::now(),
                hovered_close: false,
                hovered_send: false,
                hovered_attach: false,
                last_local_cursor: None,
                close_rect: (0.0, 0.0, 0.0, 0.0),
                send_rect: (0.0, 0.0, 0.0, 0.0),
                attach_rect: (0.0, 0.0, 0.0, 0.0),
                attached_image: None,
                last_render_bmp: RgbaBitmap::new_transparent(16, 16),
            });
        }
        if let Some(mws) = menu_ws {
            self.menu = Some(MenuWindowState {
                ws: mws,
                visible: false,
                rows: Vec::new(),
                hovered_idx: None,
                scroll_row: 0,
                scroll_accum: 0.0,
                win_size: (320, 420),
                row_h: 26.0,
                max_visible_rows: 32,
                last_render_bmp: RgbaBitmap::new_transparent(16, 16),
            });
        }
        if let Some(sws) = settings_ws {
            self.settings = Some(SettingsWindowState {
                ws: sws,
                visible: false,
                win_size: (SETTINGS_LOGICAL_W, SETTINGS_LOGICAL_H),
                model: SettingsPanelModel::new(),
                dragging_header: false,
                drag_start_cursor: (0.0, 0.0),
                drag_start_pos: (0.0, 0.0),
                last_local_cursor: None,
                last_render_bmp: RgbaBitmap::new_transparent(16, 16),
            });
        }

        self.gpu = Some(gpu);

        // Render initial surfaces `\0` and `\1` (plus external Live2D sidecar rig if present)
        let init_user_dir = self.config_store.config_dir.clone();
        let init_assets_dir = resolve_assets_dir();
        if let (Some(ref gpu), Some(ref mut s)) = (&self.gpu, &mut self.sakura) {
            let mut m = self.manifest.lock().expect("manifest mutex poisoned");
            s.reload_sidecar_rig(&m, &init_user_dir, &init_assets_dir, gpu);
            s.set_surface(&mut m, gpu, 0, true);
        }
        if let (Some(ref gpu), Some(ref mut k)) = (&self.gpu, &mut self.kero) {
            let mut m = self.manifest.lock().expect("manifest mutex poisoned");
            k.reload_sidecar_rig(&m, &init_user_dir, &init_assets_dir, gpu);
            k.set_surface(&mut m, gpu, 10, true);
        }
        self.reset_character_positions();
        self.apply_live_settings();

        // Tray icon (skip during automated headless/ephemeral smoke test if desired, or build safely)
        #[cfg(any(target_os = "macos", target_os = "windows"))]
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
            self.spawn_tts_discovery_refresh(cfg.voice_tts);
            // Background fullscreen & battery monitor (no permission prompts)
            let probe_proxy = self.proxy.clone();
            let probe_store = self.config_store.clone();
            std::thread::spawn(move || {
                let mut last_fs: Option<bool> = None;
                let mut last_batt: Option<crate::accessory_utils::BatteryStatus> = None;
                let mut last_batt_poll = Instant::now() - Duration::from_secs(120);
                let sleep_step = if cfg!(target_os = "windows") {
                    Duration::from_secs(6)
                } else {
                    Duration::from_secs(3)
                };
                loop {
                    let fs_enabled = probe_store.get().auto_hide_on_fullscreen;
                    let fs_now = if fs_enabled {
                        crate::accessory_utils::probe_fullscreen_app_active()
                    } else {
                        false
                    };
                    let mut batt_now = last_batt.clone().unwrap_or_default();
                    if last_batt_poll.elapsed() >= Duration::from_secs(60) {
                        batt_now = crate::accessory_utils::probe_battery_status();
                        last_batt_poll = Instant::now();
                    }
                    if last_fs != Some(fs_now) || last_batt.as_ref() != Some(&batt_now) {
                        last_fs = Some(fs_now);
                        last_batt = Some(batt_now.clone());
                        if probe_proxy
                            .send_event(UserEvent::SystemProbeUpdate {
                                fullscreen: fs_now,
                                battery: batt_now,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    std::thread::sleep(sleep_step);
                }
            });
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
                if !actions.is_empty() {
                    self.auto_talk_last = Instant::now();
                    let ts = Local::now().format("%H:%M:%S").to_string();
                    self.backlog.push_from_actions(&actions, ts);
                }
                if actions.iter().any(|a| a.scope == Scope::Sakura) {
                    if let Some(ref mut sb) = self.sakura_balloon {
                        sb.scroll_px = 0.0;
                        sb.user_scrolled_up = false;
                        sb.utterance_min_w = 0;
                    }
                }
                if actions.iter().any(|a| a.scope == Scope::Kero) {
                    if let Some(ref mut kb) = self.kero_balloon {
                        kb.scroll_px = 0.0;
                        kb.user_scrolled_up = false;
                        kb.utterance_min_w = 0;
                    }
                }
                self.maybe_speak_actions(&actions);
                self.playback.enqueue(actions);
            }
            UserEvent::AutoTalkActions(actions) => {
                if self.menu.as_ref().is_some_and(|m| m.visible) {
                    self.pending_auto_talk_actions = Some(actions);
                } else {
                    let _ = self.proxy.send_event(UserEvent::PlayActions(actions));
                }
            }
            UserEvent::AivmActionDone { status } => {
                self.aivm_busy_label = None;
                self.apply_status_feedback(status);
                self.redraw_settings_panel();
                self.rebuild_tray_menu();
            }
            UserEvent::CalendarSyncDone { status } => {
                self.apply_status_feedback(status);
                self.redraw_settings_panel();
                self.rebuild_tray_menu();
            }
            UserEvent::PlayActionsNoBacklog(actions) => {
                if actions.iter().any(|a| a.scope == Scope::Sakura) {
                    if let Some(ref mut sb) = self.sakura_balloon {
                        sb.scroll_px = 0.0;
                        sb.user_scrolled_up = false;
                        sb.utterance_min_w = 0;
                    }
                }
                if actions.iter().any(|a| a.scope == Scope::Kero) {
                    if let Some(ref mut kb) = self.kero_balloon {
                        kb.scroll_px = 0.0;
                        kb.user_scrolled_up = false;
                        kb.utterance_min_w = 0;
                    }
                }
                self.playback.enqueue(actions);
            }
            UserEvent::StatusUpdate(msg, _active) => {
                self.apply_status_feedback(msg);
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
            UserEvent::ServerGetVoices(tx) => {
                let _ = tx.send(self.build_voices_snapshot());
            }
            UserEvent::ServerConfigureVoice { req, reply } => {
                let cfg_cur = self.config_store.get();
                let active_shell = cfg_cur.active_shell_id.clone();
                let norm_engine = req
                    .tts_engine
                    .as_deref()
                    .map(crate::config_store::normalize_tts_engine);
                if req.voice_tts.is_some()
                    || norm_engine.is_some()
                    || req.voice_speed.is_some()
                    || req.voice_intonation.is_some()
                {
                    self.config_store.update(ConfigPatch {
                        voice_tts: req.voice_tts,
                        tts_engine: norm_engine,
                        voice_speed: req.voice_speed.map(|v| v.clamp(0.75, 1.35)),
                        voice_intonation: req.voice_intonation.map(|v| v.clamp(0.70, 1.50)),
                        ..Default::default()
                    });
                }
                if req.clear_sakura_speaker {
                    self.config_store
                        .set_shell_voice_speaker(&active_shell, 0, None);
                } else if let Some(id) = req.sakura_speaker_id {
                    self.config_store
                        .set_shell_voice_speaker(&active_shell, 0, Some(id));
                }
                if req.clear_kero_speaker {
                    self.config_store
                        .set_shell_voice_speaker(&active_shell, 1, None);
                } else if let Some(id) = req.kero_speaker_id {
                    self.config_store
                        .set_shell_voice_speaker(&active_shell, 1, Some(id));
                }
                if let Some(ref arch_raw) = req.kero_voice_archetype {
                    let norm = arch_raw.trim().to_ascii_lowercase();
                    if norm.is_empty() || norm == "auto" || norm == "default" {
                        self.config_store
                            .set_shell_kero_archetype(&active_shell, None);
                    } else {
                        self.config_store
                            .set_shell_kero_archetype(&active_shell, Some(arch_raw.clone()));
                    }
                }
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
                let _ = reply.send(Ok(json!({ "updated": true })));
            }
            UserEvent::ServerGetSettings(tx) => {
                let _ = tx.send(self.build_settings_snapshot());
            }
            UserEvent::ServerGetBacklog(tx) => {
                let entries = self.backlog.entries_newest_first();
                let _ = tx.send(json!({
                    "count": entries.len(),
                    "entries": entries,
                }));
            }
            UserEvent::ServerUpdateSettings { req, reply } => {
                if let Err(err) = crate::sstp_server::validate_settings_patch(&req) {
                    let _ = reply.send(Err(err));
                    return;
                }
                if req.reset_defaults {
                    self.config_store.reset_convenience_and_appearance_defaults();
                }
                if req.clear_interests {
                    self.config_store.clear_interests();
                } else if let Some(ref topics) = req.interests {
                    self.config_store.set_interests(topics);
                }
                if let Some(en) = req.auto_start_at_login {
                    self.apply_autostart_toggle(en);
                }
                let updated_autotalk = req.auto_talk_interval_sec.is_some() || req.reset_defaults;
                let updated_cfg = self.config_store.update(ConfigPatch {
                    ui_language: req.ui_language,
                    quiet_mode: req.quiet_mode,
                    auto_hide_on_fullscreen: req.auto_hide_on_fullscreen,
                    power_save_mode: req.power_save_mode,
                    low_battery_alert: req.low_battery_alert,
                    pomodoro_preset_idx: req.pomodoro_preset_idx,
                    skin_opacity: req.skin_opacity,
                    balloon_opacity: req.balloon_opacity,
                    character_scale: req.character_scale,
                    opaque_on_hover: req.opaque_on_hover,
                    eye_tracking: req.eye_tracking,
                    lock_position: req.lock_position,
                    snap_to_bottom: req.snap_to_bottom,
                    click_through_mode: req.click_through_mode,
                    show_kero: req.show_kero,
                    always_on_top: req.always_on_top,
                    live2d_motion: req.live2d_motion,
                    hourly_chime: req.hourly_chime,
                    voice_speed: req.voice_speed,
                    voice_intonation: req.voice_intonation,
                    auto_talk_interval_sec: req.auto_talk_interval_sec,
                    interest_news_weight: req.interest_news_weight,
                    macos_accept_first_mouse: req.macos_accept_first_mouse,
                    macos_window_shadow: req.macos_window_shadow,
                    macos_show_in_dock: req.macos_show_in_dock,
                    windows_skip_taskbar: req.windows_skip_taskbar,
                    windows_undecorated_shadow: req.windows_undecorated_shadow,
                    linux_prefer_x11: req.linux_prefer_x11,
                    linux_respect_workarea: req.linux_respect_workarea,
                    ..Default::default()
                });
                self.pomodoro.set_preset(updated_cfg.pomodoro_preset_idx);
                if let Some(ref act) = req.pomodoro_action {
                    self.apply_pomodoro_action(act);
                }
                if let Some(hidden) = req.characters_hidden {
                    self.characters_hidden_by_user = hidden;
                }
                if !updated_cfg.auto_hide_on_fullscreen {
                    self.hidden_by_fullscreen = false;
                }
                if updated_autotalk {
                    self.auto_talk_last = Instant::now();
                }
                self.apply_live_settings();
                self.apply_visibility_state();
                self.redraw_settings_panel();
                self.redraw_communicate_if_visible();
                self.rebuild_tray_menu();
                let _ = reply.send(Ok(self.build_settings_snapshot()));
            }
            UserEvent::SystemProbeUpdate { fullscreen, battery } => {
                let cfg_now = self.config_store.get();
                let target_fs_hide = cfg_now.auto_hide_on_fullscreen && fullscreen;
                if self.hidden_by_fullscreen != target_fs_hide {
                    self.hidden_by_fullscreen = target_fs_hide;
                    self.apply_visibility_state();
                }
                self.battery_status = battery.clone();
                if !battery.on_battery {
                    self.low_battery_notified = false;
                } else if cfg_now.low_battery_alert {
                    if let Some(pct) = battery.percent {
                        if pct <= 20 && !self.low_battery_notified {
                            self.low_battery_notified = true;
                            if !cfg_now.quiet_mode
                                && !self.characters_effectively_hidden()
                                && !self.pomodoro.is_focusing()
                            {
                                let script = crate::accessory_utils::build_low_battery_script(
                                    pct,
                                    self.effective_ui_lang(),
                                );
                                let _ = self
                                    .proxy
                                    .send_event(UserEvent::PlayActions(parse_dialogue(&script)));
                            }
                        } else if pct >= 30 {
                            self.low_battery_notified = false;
                        }
                    }
                }
            }
            UserEvent::ServerSwitchSkin { shell, balloon, reply } => {
                let _ = reply.send(self.apply_skin_switch(shell, balloon));
            }
            UserEvent::ServerSwitchProvider { provider, model, reply } => {
                let _ = reply.send(self.handle_provider_switch(provider, model));
            }
            UserEvent::RefreshMenus => {
                self.rebuild_tray_menu();
                self.redraw_settings_panel();
                self.redraw_communicate_if_visible();
                if self.menu.as_ref().is_some_and(|m| m.visible) {
                    let rows = self.build_menu_rows();
                    if let (Some(ref gpu), Some(ref mut m)) = (&self.gpu, &mut self.menu) {
                        m.rows = rows;
                        m.hovered_idx = None;
                        let max_scroll = m.rows.len().saturating_sub(m.max_visible_rows);
                        m.scroll_row = m.scroll_row.min(max_scroll);
                        m.redraw(&mut self.text_renderer, gpu);
                    }
                }
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
        let win_target = self.classify_window(window_id);
        let is_comm = win_target == Some(UiWindowTarget::Communicate);
        let is_menu = win_target == Some(UiWindowTarget::Menu);
        let is_settings = win_target == Some(UiWindowTarget::Settings);

        match event {
            WindowEvent::CloseRequested => {
                if is_comm {
                    if let Some(ref mut c) = self.communicate {
                        c.visible = false;
                        c.ws.window.set_visible(false);
                    }
                } else if is_menu {
                    self.hide_menu();
                } else if is_settings {
                    self.hide_settings_panel();
                } else {
                    self.begin_graceful_quit(event_loop);
                }
            }
            WindowEvent::ModifiersChanged(mods) => {
                self.modifiers = mods.state();
            }
            WindowEvent::Resized(phys) => {
                if let Some(ref gpu) = self.gpu {
                    match win_target {
                        Some(UiWindowTarget::Char(Scope::Sakura)) => {
                            if let Some(ref mut c) = self.sakura {
                                c.ws.resize(gpu, phys);
                            }
                        }
                        Some(UiWindowTarget::Char(Scope::Kero)) => {
                            if let Some(ref mut c) = self.kero {
                                c.ws.resize(gpu, phys);
                            }
                        }
                        Some(UiWindowTarget::Balloon(Scope::Sakura)) => {
                            if let Some(ref mut b) = self.sakura_balloon {
                                b.ws.resize(gpu, phys);
                            }
                        }
                        Some(UiWindowTarget::Balloon(Scope::Kero)) => {
                            if let Some(ref mut b) = self.kero_balloon {
                                b.ws.resize(gpu, phys);
                            }
                        }
                        Some(UiWindowTarget::Communicate) => {
                            if let Some(ref mut c) = self.communicate {
                                c.ws.resize(gpu, phys);
                            }
                        }
                        Some(UiWindowTarget::Menu) => {
                            if let Some(ref mut m) = self.menu {
                                m.ws.resize(gpu, phys);
                            }
                        }
                        Some(UiWindowTarget::Settings) => {
                            if let Some(ref mut s) = self.settings {
                                s.ws.resize(gpu, phys);
                            }
                        }
                        None => {}
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(ref gpu) = self.gpu {
                    match win_target {
                        Some(UiWindowTarget::Char(Scope::Sakura)) => {
                            if let Some(ref mut c) = self.sakura {
                                c.ws.render(gpu);
                            }
                        }
                        Some(UiWindowTarget::Char(Scope::Kero)) => {
                            if let Some(ref mut c) = self.kero {
                                c.ws.render(gpu);
                            }
                        }
                        Some(UiWindowTarget::Balloon(Scope::Sakura)) => {
                            if let Some(ref mut b) = self.sakura_balloon {
                                b.ws.render(gpu);
                            }
                        }
                        Some(UiWindowTarget::Balloon(Scope::Kero)) => {
                            if let Some(ref mut b) = self.kero_balloon {
                                b.ws.render(gpu);
                            }
                        }
                        Some(UiWindowTarget::Communicate) => {
                            if let Some(ref mut c) = self.communicate {
                                c.ws.render(gpu);
                            }
                        }
                        Some(UiWindowTarget::Menu) => {
                            if let Some(ref mut m) = self.menu {
                                m.ws.render(gpu);
                            }
                        }
                        Some(UiWindowTarget::Settings) => {
                            if let Some(ref mut s) = self.settings {
                                s.ws.render(gpu);
                            }
                        }
                        None => {}
                    }
                }
            }
            WindowEvent::Focused(focused) => {
                if is_menu && !focused {
                    self.hide_menu();
                } else if is_comm {
                    let ui_lang = self.effective_ui_lang();
                    if let Some(ref mut cw) = self.communicate {
                        cw.focused = focused;
                        if focused {
                            cw.cursor_on = true;
                            cw.last_blink = Instant::now();
                        }
                        if cw.visible {
                            if let Some(ref gpu) = self.gpu {
                                cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu, ui_lang);
                            }
                        }
                    }
                }
            }
            WindowEvent::DroppedFile(path) => {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .unwrap_or_default();
                if ext == "nar" {
                    self.install_and_activate_nar(&path);
                } else if ext == "ics" {
                    let cal_dir = self.config_store.config_dir.join("calendars");
                    let _ = std::fs::create_dir_all(&cal_dir);
                    if let Some(fname) = path.file_name() {
                        if std::fs::copy(&path, cal_dir.join(fname)).is_ok() {
                            self.config_store.update(ConfigPatch {
                                calendar_enabled: Some(true),
                                ..Default::default()
                            });
                            self.spawn_calendar_sync(true);
                        }
                    }
                } else if crate::agent_engine::is_supported_image_path(&path) {
                    match crate::agent_engine::load_image_attachment(&path) {
                        Ok(att) => {
                            self.open_communicate_near_sakura();
                            let ui_lang = self.effective_ui_lang();
                            if let Some(ref mut cw) = self.communicate {
                                cw.attached_image = Some(att);
                                if let Some(ref gpu) = self.gpu {
                                    cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu, ui_lang);
                                }
                            }
                        }
                        Err(e) => {
                            let _ = self.proxy.send_event(UserEvent::PlayActionsNoBacklog(parse_dialogue(
                                &format!("\\0\\s[3]🖼 {e}\\e"),
                            )));
                        }
                    }
                } else if ext == "zip" {
                    let dest_base = self.config_store.config_dir.clone();
                    match install_nar_archive(
                        &path,
                        &dest_base.join("shells"),
                        &dest_base.join("balloons"),
                    ) {
                        Ok(res) => {
                            let first_shell = res.shells.first().cloned();
                            let first_balloon = res.balloons.first().cloned();
                            let _ = self.apply_skin_switch(first_shell, first_balloon);
                            let msg = format!(
                                "\\0\\s[5]スキンを導入しました！（シェル {}個、バルーン {}個）\\e",
                                res.shells.len(),
                                res.balloons.len()
                            );
                            let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
                        }
                        Err(err) if err.to_ascii_lowercase().contains("traversal") => {
                            let msg = format!("\\0\\s[3]スキンの導入に失敗しました: {err}\\e");
                            let _ = self.proxy.send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
                        }
                        Err(_) => {
                            self.spawn_dropped_file_inspection(&path);
                        }
                    }
                } else {
                    self.spawn_dropped_file_inspection(&path);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.window_scale_factor(window_id);
                let local = position.to_logical::<f64>(scale);
                self.handle_pointer_move(window_id, (local.x, local.y));
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64 * 26.0,
                    MouseScrollDelta::PixelDelta(p) => p.y,
                };
                self.handle_scroll(window_id, dy);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_mouse_button(event_loop, window_id, button, state);
            }
            WindowEvent::Ime(ime_ev) if is_comm => {
                self.handle_ime_event(window_id, ime_ev);
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
                self.handle_communicate_key(&logical_key, text.as_deref());
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } if is_settings => {
                self.handle_settings_key(&logical_key);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let cfg_now = self.config_store.get();
        let live2d_on = cfg_now.live2d_motion;
        let comm_visible = self.communicate.as_ref().is_some_and(|c| c.visible);
        let settings_visible = self.settings.as_ref().is_some_and(|s| s.visible);
        let eff_hidden = self.characters_effectively_hidden();
        let poll_ms = crate::accessory_utils::effective_poll_interval_ms(
            live2d_on,
            comm_visible || settings_visible,
            eff_hidden,
            &cfg_now.power_save_mode,
            self.battery_status.on_battery,
            POLL_INTERVAL_MS,
        );
        let mut next_wakeup = Instant::now() + Duration::from_millis(poll_ms);
        if let Some(deadline) = self.closing_at {
            if Instant::now() >= deadline {
                crate::companion_extras::stop_managed_tts_engine();
                crate::companion_extras::stop_managed_ollama_engine();
                if let Some(mut srv) = self.server.take() {
                    srv.stop();
                }
                event_loop.exit();
                return;
            }
            if deadline < next_wakeup {
                next_wakeup = deadline;
            }
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(next_wakeup));

        if !self.initialized || self.smoke_mode {
            return;
        }

        // Poll tray menu clicks
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let mut pending_cmds = Vec::new();
            while let Ok(ev) = MenuEvent::receiver().try_recv() {
                if let Some(cmd) = self.tray_actions.get(&ev.id).cloned() {
                    pending_cmds.push(cmd);
                }
            }
            for cmd in pending_cmds {
                self.execute_menu_command(event_loop, cmd);
            }
        }

        // Caret blink & Siri aura pulse inside Communicate window
        thread_local! {
            static LAST_COMM_AURA: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
        }
        let comm_ui_lang = self.effective_ui_lang();
        if let Some(ref mut cw) = self.communicate {
            if cw.visible {
                let mut need_redraw = false;
                if cw.last_blink.elapsed() >= Duration::from_millis(500) {
                    cw.cursor_on = !cw.cursor_on;
                    cw.last_blink = Instant::now();
                    need_redraw = true;
                }
                if cw.focused {
                    let aura_due = LAST_COMM_AURA.with(|cell| {
                        let now = Instant::now();
                        match cell.get() {
                            Some(prev) if now.duration_since(prev) < Duration::from_millis(33) => false,
                            _ => {
                                cell.set(Some(now));
                                true
                            }
                        }
                    });
                    if aura_due {
                        need_redraw = true;
                    }
                }
                if need_redraw {
                    if let Some(ref gpu) = self.gpu {
                        cw.redraw(&self.balloon_skin, &mut self.text_renderer, gpu, comm_ui_lang);
                    }
                }
            }
        }

        self.poll_cursor_and_strokes();
        self.tick_playback();

        // SERIKO animation & GPU Live2D 2.5D motion tick (paused when hidden / fullscreen-hidden)
        if !eff_hidden {
            let speaking_sakura = self.playback.is_speaking(Scope::Sakura);
            let speaking_kero = self.playback.is_speaking(Scope::Kero);
            let global_cursor = if cfg_now.eye_tracking {
                match Mouse::get_mouse_position() {
                    Mouse::Position { x, y } => Some((x as f64, y as f64)),
                    Mouse::Error => None,
                }
            } else {
                None
            };
            let pomo_peering_target = if self.pomodoro.is_active() {
                self.kero.as_ref().filter(|k| k.visible).map(|k| {
                    (
                        k.logical_pos.0 + k.win_size.0 as f64 * 0.55,
                        k.logical_pos.1 + k.win_size.1 as f64 * 0.72,
                    )
                })
            } else {
                None
            };
            let sakura_gaze_target = pomo_peering_target.or(global_cursor);
            if let Some(ref gpu) = self.gpu {
                let m = self.manifest.lock().expect("manifest mutex poisoned");
                if let Some(ref mut s) = self.sakura {
                    s.tick_seriko(&m, gpu, speaking_sakura);
                    s.tick_live2d(gpu, live2d_on, sakura_gaze_target, speaking_sakura);
                }
                if let Some(ref mut k) = self.kero {
                    if k.visible {
                        k.tick_seriko(&m, gpu, speaking_kero);
                        k.tick_live2d(gpu, live2d_on, global_cursor, speaking_kero);
                    }
                }
            }
        }

        // Pomodoro countdown & phase transition notifications
        if let Some(trans) = self.pomodoro.tick() {
            self.sync_pomodoro_pose();
            if !eff_hidden {
                let script = crate::accessory_utils::build_pomodoro_transition_script(
                    trans,
                    self.effective_ui_lang(),
                );
                let _ = self
                    .proxy
                    .send_event(UserEvent::PlayActions(parse_dialogue(&script)));
            }
            self.rebuild_tray_menu();
            self.redraw_settings_panel();
        } else if self.pomodoro.is_active()
            && !self.pomodoro.paused
            && self.last_pomo_ui_refresh.elapsed() >= Duration::from_secs(1)
        {
            self.last_pomo_ui_refresh = Instant::now();
            if self
                .settings
                .as_ref()
                .is_some_and(|sw| sw.visible && sw.model.active_tab == 1)
            {
                self.redraw_settings_panel();
            }
        }

        let menu_open = self.menu.as_ref().is_some_and(|m| m.visible);
        let suppress_interruptions = should_suppress_interruptions(
            cfg_now.quiet_mode,
            eff_hidden,
            self.pomodoro.is_focusing(),
            menu_open,
        );

        // Poll due reminders & hourly chime every second when idle/not closing
        if self.closing_at.is_none() && self.reminder_poll_last.elapsed() >= Duration::from_secs(1) {
            self.reminder_poll_last = Instant::now();
            let due = self.config_store.take_due_reminders(Utc::now().timestamp());
            if let Some(first) = due.into_iter().next() {
                let kero_name = self.speaker_name(Scope::Kero);
                let msg = format!(
                    "\\0\\s[5]リマインダーの時間だよ！\\n「{}」\\1\\s[10]{kero_name}: お忘れなくやで！\\e",
                    first.message
                );
                let _ = self
                    .proxy
                    .send_event(UserEvent::PlayActions(parse_dialogue(&msg)));
            } else if cfg_now.hourly_chime
                && !suppress_interruptions
                && self.playback.steps.is_empty()
            {
                let now_local = Local::now();
                let key = (now_local.ordinal() as i32, now_local.hour());
                if now_local.minute() == 0 && self.last_chime_key != Some(key) {
                    self.last_chime_key = Some(key);
                    let reply = self.engine.handle_hourly_chime(now_local.hour());
                    let _ = self.proxy.send_event(UserEvent::PlayActions(reply.actions));
                }
            }
        }

        // Auto-talk periodic idle talk (paused during Quiet Mode, Hidden, Pomodoro Focus, or open Menu)
        let interval = cfg_now.auto_talk_interval_sec;
        if interval > 0
            && !suppress_interruptions
            && self.auto_talk_last.elapsed() >= Duration::from_secs(interval)
        {
            self.auto_talk_last = Instant::now();
            if self.playback.steps.is_empty() {
                let eng = self.engine.clone();
                let proxy = self.proxy.clone();
                std::thread::spawn(move || {
                    if let Some(reply) = eng.handle_auto_talk() {
                        let _ = proxy.send_event(UserEvent::AutoTalkActions(reply.actions));
                    }
                });
            }
        }

        // Calendar background sync (every 15 min) + travel departure alerts (every 30s) + periodic briefing
        if cfg_now.calendar_enabled && self.closing_at.is_none() {
            if self.calendar_last_sync.elapsed() >= Duration::from_secs(15 * 60) {
                self.spawn_calendar_sync(false);
            }
            if self.calendar_last_alert_check.elapsed() >= Duration::from_secs(30) {
                self.calendar_last_alert_check = Instant::now();
                if !cfg_now.quiet_mode && !eff_hidden && !menu_open {
                    let now_ts = Utc::now().timestamp();
                    let due_alerts = self
                        .calendar_state
                        .lock()
                        .map(|mut st| {
                            st.poll_due_alerts(
                                now_ts,
                                cfg_now.calendar_prep_mins,
                                cfg_now.calendar_remind_mins,
                            )
                        })
                        .unwrap_or_default();
                    if let Some(first_alert) = due_alerts.into_iter().next() {
                        let script = crate::calendar_sync::format_calendar_alert_script(
                            &first_alert,
                            self.effective_ui_lang(),
                        );
                        let _ = self
                            .proxy
                            .send_event(UserEvent::PlayActions(parse_dialogue(&script)));
                        self.calendar_last_briefing = Instant::now();
                    } else if cfg_now.calendar_briefing_interval_mins > 0
                        && !suppress_interruptions
                        && self.playback.steps.is_empty()
                        && self.calendar_last_briefing.elapsed()
                            >= Duration::from_secs(cfg_now.calendar_briefing_interval_mins as u64 * 60)
                    {
                        self.calendar_last_briefing = Instant::now();
                        let evs = self
                            .calendar_state
                            .lock()
                            .map(|st| st.events.clone())
                            .unwrap_or_default();
                        if let Some(script) = crate::calendar_sync::format_periodic_briefing_script(
                            &evs,
                            now_ts,
                            cfg_now.calendar_prep_mins,
                            self.effective_ui_lang(),
                        ) {
                            let _ = self
                                .proxy
                                .send_event(UserEvent::AutoTalkActions(parse_dialogue(&script)));
                        }
                    }
                }
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
        let dir = std::env::temp_dir().join(format!("nanikaai-smoke-userdata-{ts}"));
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
    let mut shell_roots = vec![user_data.join("shells"), bundled_shells];
    let mut balloon_roots = vec![user_data.join("balloons"), bundled_balloons];
    for legacy_data in legacy_user_data_dirs() {
        if legacy_data != user_data && legacy_data.is_dir() {
            let ls = legacy_data.join("shells");
            if !shell_roots.contains(&ls) {
                shell_roots.push(ls);
            }
            let lb = legacy_data.join("balloons");
            if !balloon_roots.contains(&lb) {
                balloon_roots.push(lb);
            }
        }
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
        builder.with_activation_policy(if cfg.macos_show_in_dock && !smoke_test {
            ActivationPolicy::Regular
        } else {
            ActivationPolicy::Accessory
        });
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

    let cfg_dir_for_cal = config_store.config_dir.clone();
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
        settings: None,
        expanded_menu_section: None,
        playback: PlaybackQueue::new(),
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        tray: None,
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        tray_actions: HashMap::new(),
        server: None,
        os_insets: detect_os_insets(),
        modifiers: ModifiersState::empty(),
        auto_talk_last: Instant::now(),
        last_chime_key: {
            let now_l = Local::now();
            if now_l.minute() == 0 {
                Some((now_l.ordinal() as i32, now_l.hour()))
            } else {
                None
            }
        },
        reminder_poll_last: Instant::now(),
        closing_at: None,
        initialized: false,
        characters_hidden_by_user: false,
        hidden_by_fullscreen: false,
        dock_left_edge: false,
        pomodoro: crate::accessory_utils::PomodoroTimer::new(cfg.pomodoro_preset_idx),
        backlog: crate::accessory_utils::DialogueBacklog::default(),
        battery_status: crate::accessory_utils::BatteryStatus::default(),
        low_battery_notified: false,
        last_pomo_ui_refresh: Instant::now(),
        pending_auto_talk_actions: None,
        aivm_busy_label: None,
        calendar_state: {
            let mut st = crate::calendar_sync::CalendarSyncState::default();
            st.load_from_disk(&cfg_dir_for_cal);
            Arc::new(Mutex::new(st))
        },
        calendar_last_sync: Instant::now() - Duration::from_secs(14 * 60 + 50),
        calendar_last_alert_check: Instant::now(),
        calendar_last_briefing: Instant::now(),
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("GUI 이벤트 루프 오류: {e}"))?;

    crate::companion_extras::stop_managed_tts_engine();
    crate::companion_extras::stop_managed_ollama_engine();

    if smoke_test && !*smoke_passed.lock().expect("smoke mutex poisoned") {
        return Err("스모크 테스트가 통과되지 않았습니다.".to_string());
    }
    Ok(())
}

/// Pure helper determining whether periodic interruptions (auto-talk, hourly chime, calendar briefing)
/// should be suppressed (during Quiet Mode, Hidden/Fullscreen, Pomodoro Focus, or open Right-Click Menu).
pub fn should_suppress_interruptions(
    quiet_mode: bool,
    eff_hidden: bool,
    pomo_focusing: bool,
    menu_open: bool,
) -> bool {
    quiet_mode || eff_hidden || pomo_focusing || menu_open
}

/// Public snapshot helper for `/api/settings` (guarantees `api_key` and secret `calendar_ics_urls` are never exposed).
pub fn settings_snapshot_from_store(store: &ConfigStore) -> Value {
    AppRuntime::settings_snapshot_from_store(store)
}

/// Public snapshot helper for `/api/calendar` (guarantees secret `calendar_ics_urls` and raw `calendar_home_location` are never exposed).
pub fn calendar_snapshot_from_store(store: &ConfigStore) -> Value {
    let cfg = store.get();
    let mut st = crate::calendar_sync::CalendarSyncState::default();
    st.load_from_disk(&store.config_dir);
    let now_ts = chrono::Utc::now().timestamp();
    let evs = st.upcoming_events(now_ts, 48);
    let connected = cfg.calendar_use_os_native || !cfg.calendar_ics_urls.is_empty();
    let sources: Vec<&'static str> = cfg
        .calendar_ics_urls
        .iter()
        .map(|u| crate::calendar_sync::detect_calendar_source_label(u))
        .collect();
    json!({
        "enabled": cfg.calendar_enabled,
        "connected": connected,
        "use_os_native": cfg.calendar_use_os_native,
        "ics_feed_count": cfg.calendar_ics_urls.len(),
        "ics_sources": sources,
        "home_location_set": !cfg.calendar_home_location.trim().is_empty(),
        "default_travel_mins": cfg.calendar_default_travel_mins,
        "prep_mins": cfg.calendar_prep_mins,
        "remind_mins": cfg.calendar_remind_mins,
        "briefing_interval_mins": cfg.calendar_briefing_interval_mins,
        "last_sync_ts": st.last_sync_ts,
        "count": evs.len(),
        "events": evs,
    })
}

#[cfg(test)]
mod tests {
    use super::{Live2dRigState, ShaderParams, Spring1D};

    #[test]
    fn shader_params_uniform_layout_is_96_bytes() {
        assert_eq!(std::mem::size_of::<ShaderParams>(), 96);
    }

    #[test]
    fn spring_1d_and_live2d_rig_converge_smoothly() {
        let mut s = Spring1D::default();
        for _ in 0..80 {
            s.step(1.0, 12.0, 0.72, 0.033);
        }
        assert!((s.pos - 1.0).abs() < 0.05, "spring pos={}", s.pos);

        let mut rig = Live2dRigState::new();
        rig.hair_x.pos = 0.45;
        rig.angle_x = 0.8;
        rig.reset();
        assert_eq!(rig.hair_x.pos, 0.0);
        assert_eq!(rig.angle_x, 0.0);
    }
}
