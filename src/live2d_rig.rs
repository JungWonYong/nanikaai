use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

use crate::builtin_tools::is_sensitive_path;
use crate::dialogue_parser::Scope;
use crate::shell_parser::RgbaBitmap;

pub const MAX_SIDECAR_PARTS: usize = 32;
pub const MIN_CANVAS_DIM: u32 = 16;
pub const MAX_CANVAS_DIM: u32 = 4096;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PartMotionRole {
    Planted,
    Skirt,
    Torso,
    Neck,
    Ribbon,
    Head,
    FaceFeatures,
    HairBack,
    HairSideL,
    HairSideR,
    HairBangs,
    CoatHem,
    Arm,
    Collar,
    Ahoge,
    MochiBody,
    MochiTuft,
    MochiFace,
    MochiPaws,
    MochiGlasses,
    #[default]
    Accessory,
    #[serde(other)]
    Custom,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatchMode {
    #[default]
    MaskedCopy,
    DiffOverlay,
    AlphaMask,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynamicPatchSpec {
    pub rect_px: [u32; 4],
    #[serde(default)]
    pub mode: PatchMode,
    #[serde(default)]
    pub mask_texture: Option<String>,
    #[serde(default)]
    pub ref_texture: Option<String>,
    #[serde(default)]
    pub exclude_mask_texture: Option<String>,
    #[serde(default)]
    pub threshold: Option<u8>,
}

fn default_two_zero() -> [f32; 2] {
    [0.0, 0.0]
}

fn default_one() -> f32 {
    1.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UvRigTuning {
    #[serde(default)]
    pub neck_v: Option<f32>,
    #[serde(default)]
    pub head_fade: Option<f32>,
    #[serde(default)]
    pub foot_lock_v: Option<f32>,
    #[serde(default)]
    pub breath_amp: Option<f32>,
    #[serde(default)]
    pub gaze_scale: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PartLayerSpec {
    pub id: String,
    #[serde(default)]
    pub parent: Option<String>,
    pub texture: String,
    #[serde(default)]
    pub z: i32,
    #[serde(default = "default_two_zero")]
    pub pivot_px: [f32; 2],
    #[serde(default)]
    pub role: PartMotionRole,
    #[serde(default = "default_one")]
    pub opacity: f32,
    #[serde(default = "default_two_zero")]
    pub parallax: [f32; 2],
    #[serde(default)]
    pub rot_deg: f32,
    #[serde(default)]
    pub bend_px: f32,
    #[serde(default = "default_two_zero")]
    pub breath_px: [f32; 2],
    #[serde(default = "default_two_zero")]
    pub scale_breath: [f32; 2],
    #[serde(default)]
    pub spring_omega: Option<f32>,
    #[serde(default)]
    pub spring_zeta: Option<f32>,
    #[serde(default)]
    pub phase_offset: f32,
    #[serde(default)]
    pub dynamic_patch: Option<DynamicPatchSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CharacterRigSpec {
    pub canvas_size: [u32; 2],
    #[serde(default)]
    pub uv_rig: Option<UvRigTuning>,
    #[serde(default)]
    pub surface_overrides: HashMap<String, String>,
    #[serde(default)]
    pub parts: Vec<PartLayerSpec>,
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SidecarLive2dManifest {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub sakura: Option<CharacterRigSpec>,
    #[serde(default)]
    pub kero: Option<CharacterRigSpec>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct PartGpuUniform {
    pub inv_row0: [f32; 4], // [ia, ib, ic, canvas_w]
    pub inv_row1: [f32; 4], // [id, ie, if_, canvas_h]
    pub deform: [f32; 4],   // [world_pivot_y, bend_dir_y, bend_span_px, bend_dx]
    pub meta: [f32; 4],     // [active_flag, alpha_mul, 0.0, 0.0]
}

impl PartGpuUniform {
    pub fn identity(canvas_w: f32, canvas_h: f32) -> Self {
        Self {
            inv_row0: [1.0, 0.0, 0.0, canvas_w.max(1.0)],
            inv_row1: [0.0, 1.0, 0.0, canvas_h.max(1.0)],
            deform: [0.0, 1.0, 100.0, 0.0],
            meta: [1.0, 1.0, 0.0, 0.0],
        }
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        let mut out = [0u8; 64];
        let floats = [
            self.inv_row0[0],
            self.inv_row0[1],
            self.inv_row0[2],
            self.inv_row0[3],
            self.inv_row1[0],
            self.inv_row1[1],
            self.inv_row1[2],
            self.inv_row1[3],
            self.deform[0],
            self.deform[1],
            self.deform[2],
            self.deform[3],
            self.meta[0],
            self.meta[1],
            self.meta[2],
            self.meta[3],
        ];
        for (i, v) in floats.iter().enumerate() {
            out[i * 4..(i + 1) * 4].copy_from_slice(&v.to_ne_bytes());
        }
        out
    }
}

#[derive(Clone, Debug, Default)]
struct PartSpring1D {
    pos: f32,
    vel: f32,
}

impl PartSpring1D {
    fn step(&mut self, target: f32, omega: f32, zeta: f32, dt: f32) {
        let sub_dt = dt.clamp(0.001, 0.045) * 0.5;
        let om = omega.clamp(2.0, 28.0);
        let z = zeta.clamp(0.12, 1.2);
        for _ in 0..2 {
            let acc = om * om * (target - self.pos) - 2.0 * z * om * self.vel;
            self.vel = (self.vel + acc * sub_dt).clamp(-10.0, 10.0);
            self.pos = (self.pos + self.vel * sub_dt).clamp(-2.0, 2.0);
        }
    }

    fn reset(&mut self) {
        self.pos = 0.0;
        self.vel = 0.0;
    }
}

#[derive(Clone, Debug)]
struct ResolvedDynamicPatch {
    rect_px: [u32; 4],
    mode: PatchMode,
    mask_alpha: Option<Vec<u8>>,
    mask_size: (u32, u32),
    ref_bitmap: Option<RgbaBitmap>,
    exclude_alpha: Option<Vec<u8>>,
    exclude_size: (u32, u32),
    threshold: u8,
}

#[derive(Clone, Debug)]
pub struct LoadedPartLayer {
    pub spec: PartLayerSpec,
    pub parent_idx: Option<usize>,
    pub base_bitmap: RgbaBitmap,
    pub current_bitmap: RgbaBitmap,
    patch: Option<ResolvedDynamicPatch>,
    spring_x: PartSpring1D,
    spring_y: PartSpring1D,
    pub uniform: PartGpuUniform,
    pub dirty_gpu: bool,
}

#[derive(Clone, Debug)]
pub struct Live2dMotionInputs {
    pub dt: f32,
    pub time_s: f32,
    pub breath: f32,
    pub body_sway: f32,
    pub squish: f32,
    pub talk_bob: f32,
    pub angle_x: f32,
    pub angle_y: f32,
    pub angle_z: f32,
    pub vel_x: f32,
    pub vel_y: f32,
    pub speech_impulse: bool,
}

#[derive(Clone, Debug)]
pub struct LoadedCharacterRig {
    pub sidecar_dir: PathBuf,
    pub canvas_size: [u32; 2],
    pub uv_rig: Option<UvRigTuning>,
    pub surface_overrides: HashMap<i32, RgbaBitmap>,
    pub parts: Vec<LoadedPartLayer>,
}

pub fn is_valid_shell_id(shell_id: &str) -> bool {
    let trimmed = shell_id.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return false;
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
        return false;
    }
    true
}

pub fn discover_sidecar_dir(
    shell_id: &str,
    shell_dir: &Path,
    user_data_dir: &Path,
    assets_dir: &Path,
) -> Option<PathBuf> {
    if !is_valid_shell_id(shell_id) {
        return None;
    }
    let candidates = [
        user_data_dir.join("live2d").join(shell_id),
        assets_dir.join("live2d").join(shell_id),
        shell_dir.to_path_buf(),
    ];
    for dir in candidates {
        if dir.join("live2d.json").is_file() {
            return Some(dir);
        }
    }
    None
}

pub fn validate_safe_rel_asset_path(sidecar_dir: &Path, rel: &str) -> Result<PathBuf, String> {
    let trimmed = rel.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err("Empty or NUL path in sidecar".to_string());
    }
    let rel_norm = trimmed.replace('\\', "/");
    if rel_norm.starts_with('/') || rel_norm.starts_with("\\\\") {
        return Err(format!("Absolute path rejected in sidecar: {rel}"));
    }
    static DRIVE_RE: OnceLock<Regex> = OnceLock::new();
    let drive_re = DRIVE_RE.get_or_init(|| Regex::new(r"^[A-Za-z]:").unwrap());
    if drive_re.is_match(&rel_norm) {
        return Err(format!("Drive-qualified path rejected in sidecar: {rel}"));
    }
    let rel_path = Path::new(&rel_norm);
    for comp in rel_path.components() {
        match comp {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Disallowed path component in sidecar: {rel}"));
            }
        }
    }
    if is_sensitive_path(rel_path) {
        return Err(format!("Sensitive asset filename blocked in sidecar: {rel}"));
    }
    let joined = sidecar_dir.join(rel_path);
    if is_sensitive_path(&joined) {
        return Err(format!("Sensitive target path blocked in sidecar: {rel}"));
    }
    let canon_base = fs::canonicalize(sidecar_dir)
        .map_err(|e| format!("Cannot canonicalize sidecar base: {e}"))?;
    let canon_file = fs::canonicalize(&joined)
        .map_err(|e| format!("Missing sidecar asset '{rel}': {e}"))?;
    if !canon_file.starts_with(&canon_base) {
        return Err(format!("Sidecar path escape rejected: {rel}"));
    }
    if !canon_file.is_file() {
        return Err(format!("Sidecar path is not a regular file: {rel}"));
    }
    Ok(canon_file)
}

fn load_rgba_file(path: &Path) -> Result<RgbaBitmap, String> {
    let img = image::ImageReader::open(path)
        .map_err(|e| format!("Failed opening {:?}: {e}", path))?
        .with_guessed_format()
        .map_err(|e| format!("Image format guess failed {:?}: {e}", path))?
        .decode()
        .map_err(|e| format!("Image decode failed {:?}: {e}", path))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    if w > MAX_CANVAS_DIM || h > MAX_CANVAS_DIM || w == 0 || h == 0 {
        return Err(format!(
            "Part bitmap dimensions out of bounds: {w}x{h} (limit {MAX_CANVAS_DIM})"
        ));
    }
    Ok(RgbaBitmap {
        width: w,
        height: h,
        data: rgba.into_raw(),
    })
}

fn extract_alpha_channel(bmp: &RgbaBitmap) -> Vec<u8> {
    bmp.data.chunks_exact(4).map(|px| px[3]).collect()
}

pub fn load_sidecar_manifest_from_dir(sidecar_dir: &Path) -> Result<SidecarLive2dManifest, String> {
    let manifest_path = validate_safe_rel_asset_path(sidecar_dir, "live2d.json")?;
    let raw = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("Cannot read live2d.json: {e}"))?;
    let manifest: SidecarLive2dManifest =
        serde_json::from_str(&raw).map_err(|e| format!("Invalid live2d.json schema: {e}"))?;
    Ok(manifest)
}

pub fn load_character_rig(
    sidecar_dir: &Path,
    scope: Scope,
) -> Result<Option<LoadedCharacterRig>, String> {
    let manifest = load_sidecar_manifest_from_dir(sidecar_dir)?;
    let spec_opt = match scope {
        Scope::Sakura => manifest.sakura,
        Scope::Kero => manifest.kero,
    };
    let Some(spec) = spec_opt else {
        return Ok(None);
    };
    let cw = spec.canvas_size[0];
    let ch = spec.canvas_size[1];
    if !(MIN_CANVAS_DIM..=MAX_CANVAS_DIM).contains(&cw)
        || !(MIN_CANVAS_DIM..=MAX_CANVAS_DIM).contains(&ch)
    {
        return Err(format!(
            "canvas_size out of allowed bounds ({MIN_CANVAS_DIM}..={MAX_CANVAS_DIM}): {cw}x{ch}"
        ));
    }
    if spec.parts.len() > MAX_SIDECAR_PARTS {
        return Err(format!(
            "Too many parts in sidecar ({} > {MAX_SIDECAR_PARTS})",
            spec.parts.len()
        ));
    }

    let mut ordered = spec.parts.clone();
    ordered.sort_by_key(|p| p.z);

    let mut id_to_idx: HashMap<String, usize> = HashMap::new();
    for (idx, p) in ordered.iter().enumerate() {
        let id_clean = p.id.trim();
        if id_clean.is_empty() {
            return Err("Part id must not be empty".to_string());
        }
        if id_to_idx.insert(id_clean.to_string(), idx).is_some() {
            return Err(format!("Duplicate part id in sidecar: {}", p.id));
        }
    }

    // Resolve parent indices and reject cycles cleanly.
    let mut parent_indices = vec![None; ordered.len()];
    for (idx, p) in ordered.iter().enumerate() {
        if let Some(ref pid) = p.parent {
            let pid_t = pid.trim();
            if !pid_t.is_empty() {
                let p_idx = *id_to_idx
                    .get(pid_t)
                    .ok_or_else(|| format!("Unknown parent bone '{pid_t}' for '{}'", p.id))?;
                parent_indices[idx] = Some(p_idx);
            }
        }
    }
    for i in 0..ordered.len() {
        let mut seen = HashSet::new();
        let mut cur = Some(i);
        while let Some(node) = cur {
            if !seen.insert(node) {
                return Err(format!(
                    "Cyclic parent bone chain detected starting at '{}'",
                    ordered[i].id
                ));
            }
            cur = parent_indices[node];
        }
    }

    let mut layers = Vec::with_capacity(ordered.len());
    for (idx, p) in ordered.into_iter().enumerate() {
        let tex_path = validate_safe_rel_asset_path(sidecar_dir, &p.texture)?;
        let base_bitmap = load_rgba_file(&tex_path)?;
        let current_bitmap = base_bitmap.clone();

        let patch = if let Some(ref dp) = p.dynamic_patch {
            if dp.rect_px[0] >= dp.rect_px[2] || dp.rect_px[1] >= dp.rect_px[3] {
                return Err(format!("Invalid dynamic_patch rect_px for '{}'", p.id));
            }
            let (mask_alpha, mask_size) = if let Some(ref mp) = dp.mask_texture {
                let mp_abs = validate_safe_rel_asset_path(sidecar_dir, mp)?;
                let bmp = load_rgba_file(&mp_abs)?;
                let sz = (bmp.width, bmp.height);
                (Some(extract_alpha_channel(&bmp)), sz)
            } else {
                (None, (0, 0))
            };
            let ref_bitmap = if let Some(ref rp) = dp.ref_texture {
                let rp_abs = validate_safe_rel_asset_path(sidecar_dir, rp)?;
                Some(load_rgba_file(&rp_abs)?)
            } else {
                None
            };
            let (exclude_alpha, exclude_size) = if let Some(ref ep) = dp.exclude_mask_texture {
                let ep_abs = validate_safe_rel_asset_path(sidecar_dir, ep)?;
                let bmp = load_rgba_file(&ep_abs)?;
                let sz = (bmp.width, bmp.height);
                (Some(extract_alpha_channel(&bmp)), sz)
            } else {
                (None, (0, 0))
            };
            Some(ResolvedDynamicPatch {
                rect_px: dp.rect_px,
                mode: dp.mode,
                mask_alpha,
                mask_size,
                ref_bitmap,
                exclude_alpha,
                exclude_size,
                threshold: dp.threshold.unwrap_or(20),
            })
        } else {
            None
        };

        let mut uniform = PartGpuUniform::identity(cw as f32, ch as f32);
        uniform.meta[1] = p.opacity.clamp(0.0, 1.0);
        layers.push(LoadedPartLayer {
            spec: p,
            parent_idx: parent_indices[idx],
            base_bitmap,
            current_bitmap,
            patch,
            spring_x: PartSpring1D::default(),
            spring_y: PartSpring1D::default(),
            uniform,
            dirty_gpu: true,
        });
    }

    let mut surface_overrides = HashMap::new();
    for (sid_str, rel_path) in &spec.surface_overrides {
        let sid = sid_str
            .trim()
            .parse::<i32>()
            .map_err(|_| format!("Invalid surface_overrides key '{sid_str}' in sidecar"))?;
        let abs = validate_safe_rel_asset_path(sidecar_dir, rel_path)?;
        surface_overrides.insert(sid, load_rgba_file(&abs)?);
    }

    Ok(Some(LoadedCharacterRig {
        sidecar_dir: sidecar_dir.to_path_buf(),
        canvas_size: [cw, ch],
        uv_rig: spec.uv_rig,
        surface_overrides,
        parts: layers,
    }))
}

pub fn discover_and_load_rig(
    shell_id: &str,
    shell_dir: &Path,
    user_data_dir: &Path,
    assets_dir: &Path,
    scope: Scope,
) -> Option<LoadedCharacterRig> {
    let dir = discover_sidecar_dir(shell_id, shell_dir, user_data_dir, assets_dir)?;
    match load_character_rig(&dir, scope) {
        Ok(opt) => opt,
        Err(err) => {
            eprintln!("[Live2D Sidecar] Ignored rig '{shell_id}': {err}");
            None
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct Mat2D {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Mat2D {
    fn identity() -> Self {
        Self {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 0.0,
            e: 1.0,
            f: 0.0,
        }
    }

    fn mul(self, rhs: Self) -> Self {
        Self {
            a: self.a * rhs.a + self.b * rhs.d,
            b: self.a * rhs.b + self.b * rhs.e,
            c: self.a * rhs.c + self.b * rhs.f + self.c,
            d: self.d * rhs.a + self.e * rhs.d,
            e: self.d * rhs.b + self.e * rhs.e,
            f: self.d * rhs.c + self.e * rhs.f + self.f,
        }
    }

    fn around_pivot(px: f32, py: f32, tx: f32, ty: f32, angle_rad: f32, sx: f32, sy: f32) -> Self {
        let ca = angle_rad.cos();
        let sa = angle_rad.sin();
        let m00 = ca * sx;
        let m01 = -sa * sy;
        let m10 = sa * sx;
        let m11 = ca * sy;
        Self {
            a: m00,
            b: m01,
            c: px + tx - (m00 * px + m01 * py),
            d: m10,
            e: m11,
            f: py + ty - (m10 * px + m11 * py),
        }
    }

    fn map_point(self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.b * y + self.c, self.d * x + self.e * y + self.f)
    }

    fn inverse(self) -> Option<Self> {
        let det = self.a * self.e - self.b * self.d;
        if det.abs() < 1e-5 {
            return None;
        }
        let inv_det = 1.0 / det;
        Some(Self {
            a: self.e * inv_det,
            b: -self.b * inv_det,
            c: (self.b * self.f - self.c * self.e) * inv_det,
            d: -self.d * inv_det,
            e: self.a * inv_det,
            f: (self.c * self.d - self.a * self.f) * inv_det,
        })
    }
}

impl LoadedCharacterRig {
    pub fn reset(&mut self) {
        let cw = self.canvas_size[0] as f32;
        let ch = self.canvas_size[1] as f32;
        for part in &mut self.parts {
            part.spring_x.reset();
            part.spring_y.reset();
            part.uniform = PartGpuUniform::identity(cw, ch);
            part.uniform.meta[1] = part.spec.opacity.clamp(0.0, 1.0);
        }
    }

    /// Synchronizes dynamic patches from the current surface ID + overlays, using any matching
    /// clean `surface_overrides` base bitmap (so e.g. Unyu's expressions load unbroken without
    /// painted-on sunglasses while SERIKO overlays and bind accessories still apply cleanly).
    pub fn sync_from_surface_state<'a>(
        &mut self,
        surface_id: i32,
        overlays: impl IntoIterator<Item = (i32, i32, bool, &'a RgbaBitmap)>,
        fallback_composited: &RgbaBitmap,
    ) -> Vec<usize> {
        if let Some(clean_base) = self.surface_overrides.get(&surface_id) {
            let mut comp = clean_base.clone();
            for (ox, oy, replace, ov) in overlays {
                if replace {
                    comp.replace_rect(ov, ox, oy);
                } else {
                    comp.alpha_composite(ov, ox, oy);
                }
            }
            self.sync_from_composited(&comp)
        } else {
            self.sync_from_composited(fallback_composited)
        }
    }

    /// Updates dynamic sub-rectangles (`dynamic_patch`) from the current 2D `composited` surface bitmap.
    /// Returns indices of parts whose RGBA buffer changed and should be re-uploaded to GPU.
    pub fn sync_from_composited(&mut self, composited: &RgbaBitmap) -> Vec<usize> {
        let mut dirty_indices = Vec::new();
        for (idx, part) in self.parts.iter_mut().enumerate() {
            let Some(ref dp) = part.patch else {
                continue;
            };
            let dst = &mut part.current_bitmap;
            let base = &part.base_bitmap;
            let [rx0, ry0, rx1, ry1] = dp.rect_px;
            let x0 = rx0.min(dst.width).min(composited.width);
            let x1 = rx1.min(dst.width).min(composited.width);
            let y0 = ry0.min(dst.height).min(composited.height);
            let y1 = ry1.min(dst.height).min(composited.height);
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            for y in y0..y1 {
                for x in x0..x1 {
                    let b_idx = ((y * dst.width + x) * 4) as usize;
                    let c_idx = ((y * composited.width + x) * 4) as usize;
                    match dp.mode {
                        PatchMode::MaskedCopy => {
                            let m_alpha = dp
                                .mask_alpha
                                .as_ref()
                                .and_then(|ma| {
                                    let (mw, mh) = dp.mask_size;
                                    if x < mw && y < mh {
                                        ma.get((y * mw + x) as usize).copied()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(255);
                            if m_alpha > 127 {
                                dst.data[b_idx..b_idx + 4]
                                    .copy_from_slice(&composited.data[c_idx..c_idx + 4]);
                            } else {
                                dst.data[b_idx..b_idx + 4]
                                    .copy_from_slice(&base.data[b_idx..b_idx + 4]);
                            }
                        }
                        PatchMode::AlphaMask => {
                            let m_alpha = dp
                                .mask_alpha
                                .as_ref()
                                .and_then(|ma| {
                                    let (mw, mh) = dp.mask_size;
                                    if x < mw && y < mh {
                                        ma.get((y * mw + x) as usize).copied()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(255) as u32;
                            let e_alpha = dp
                                .exclude_alpha
                                .as_ref()
                                .and_then(|ea| {
                                    let (ew, eh) = dp.exclude_size;
                                    if x < ew && y < eh {
                                        ea.get((y * ew + x) as usize).copied()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0) as u32;
                            let comp_a = composited.data[c_idx + 3] as u32;
                            let out_a = ((comp_a * m_alpha * (255 - e_alpha) + 32_512)
                                / (255 * 255))
                                .min(255) as u8;
                            if out_a > 0 {
                                dst.data[b_idx] = composited.data[c_idx];
                                dst.data[b_idx + 1] = composited.data[c_idx + 1];
                                dst.data[b_idx + 2] = composited.data[c_idx + 2];
                                dst.data[b_idx + 3] = out_a;
                            } else {
                                dst.data[b_idx..b_idx + 4].copy_from_slice(&[0, 0, 0, 0]);
                            }
                        }
                        PatchMode::DiffOverlay => {
                            let excl = dp
                                .exclude_alpha
                                .as_ref()
                                .and_then(|ea| {
                                    let (ew, eh) = dp.exclude_size;
                                    if x < ew && y < eh {
                                        ea.get((y * ew + x) as usize).copied()
                                    } else {
                                        None
                                    }
                                })
                                .unwrap_or(0)
                                > 127;
                            if excl {
                                dst.data[b_idx..b_idx + 4].copy_from_slice(&[0, 0, 0, 0]);
                                continue;
                            }
                            let diff_exceeds = if let Some(ref ref_bmp) = dp.ref_bitmap {
                                if x < ref_bmp.width && y < ref_bmp.height {
                                    let r_idx = ((y * ref_bmp.width + x) * 4) as usize;
                                    let dr = composited.data[c_idx]
                                        .abs_diff(ref_bmp.data[r_idx]);
                                    let dg = composited.data[c_idx + 1]
                                        .abs_diff(ref_bmp.data[r_idx + 1]);
                                    let db = composited.data[c_idx + 2]
                                        .abs_diff(ref_bmp.data[r_idx + 2]);
                                    let da = composited.data[c_idx + 3]
                                        .abs_diff(ref_bmp.data[r_idx + 3]);
                                    dr.max(dg).max(db).max(da) > dp.threshold
                                } else {
                                    composited.data[c_idx + 3] > dp.threshold
                                }
                            } else {
                                composited.data[c_idx + 3] > dp.threshold
                            };
                            if diff_exceeds {
                                dst.data[b_idx..b_idx + 4]
                                    .copy_from_slice(&composited.data[c_idx..c_idx + 4]);
                            } else {
                                dst.data[b_idx..b_idx + 4].copy_from_slice(&[0, 0, 0, 0]);
                            }
                        }
                    }
                }
            }
            part.dirty_gpu = true;
            dirty_indices.push(idx);
        }
        dirty_indices
    }

    pub fn step(&mut self, inp: &Live2dMotionInputs) {
        let cw = self.canvas_size[0] as f32;
        let ch = self.canvas_size[1] as f32;
        let n = self.parts.len();
        if n == 0 {
            return;
        }

        // First compute local transforms + per-part spring updates
        let mut locals = vec![Mat2D::identity(); n];
        let mut bend_meta = vec![(1.0_f32, 100.0_f32, 0.0_f32); n];

        for (i, part) in self.parts.iter_mut().enumerate() {
            let sp = &part.spec;
            let ph = sp.phase_offset;
            let omega = sp.spring_omega.unwrap_or(match sp.role {
                PartMotionRole::Ahoge | PartMotionRole::MochiTuft => 14.5,
                PartMotionRole::Skirt | PartMotionRole::CoatHem => 6.8,
                PartMotionRole::Arm => 7.4,
                PartMotionRole::Collar => 9.0,
                PartMotionRole::Ribbon => 9.5,
                PartMotionRole::HairBangs => 10.6,
                PartMotionRole::HairSideL | PartMotionRole::HairSideR => 9.6,
                PartMotionRole::MochiPaws => 11.2,
                PartMotionRole::MochiGlasses => 14.0,
                _ => 8.8,
            });
            let zeta = sp.spring_zeta.unwrap_or(match sp.role {
                PartMotionRole::Ahoge | PartMotionRole::MochiTuft => 0.25,
                PartMotionRole::Skirt | PartMotionRole::CoatHem => 0.44,
                PartMotionRole::Arm => 0.42,
                PartMotionRole::HairBangs => 0.34,
                PartMotionRole::MochiPaws => 0.32,
                PartMotionRole::MochiGlasses => 0.40,
                _ => 0.38,
            });

            let breeze = (inp.time_s * 1.35 + ph).sin() * 0.24 + (inp.time_s * 2.15 + ph).cos() * 0.10;
            let target_x = match sp.role {
                PartMotionRole::Ahoge | PartMotionRole::MochiTuft => {
                    (-inp.vel_x * 0.34
                        + (inp.time_s * 1.9 + ph).sin() * 0.34
                        + if inp.speech_impulse { 0.52 } else { 0.0 })
                    .clamp(-1.8, 1.8)
                }
                PartMotionRole::Skirt => {
                    (-inp.body_sway * 0.44 - inp.vel_x * 0.14 + (inp.time_s * 1.05 + ph).sin() * 0.24)
                        .clamp(-1.5, 1.5)
                }
                PartMotionRole::CoatHem | PartMotionRole::Collar => {
                    (-inp.body_sway * 0.48 - inp.vel_x * 0.18 + breeze * 1.15).clamp(-1.6, 1.6)
                }
                PartMotionRole::Arm => {
                    (-inp.body_sway * 0.38 - inp.vel_x * 0.14 + (inp.time_s * 1.1 + ph).sin() * 0.22)
                        .clamp(-1.5, 1.5)
                }
                PartMotionRole::Ribbon => {
                    (-inp.vel_x * 0.18 - inp.body_sway * 0.24 + (inp.time_s * 1.6 + ph).sin() * 0.28)
                        .clamp(-1.5, 1.5)
                }
                PartMotionRole::MochiPaws => {
                    ((inp.time_s * 2.35 + ph).sin() * 0.62
                        - inp.vel_x * 0.22
                        + inp.talk_bob * 0.85
                        + inp.squish * 18.0)
                        .clamp(-1.8, 1.8)
                }
                PartMotionRole::HairBangs => {
                    (-inp.vel_x * 0.20 - inp.body_sway * 0.14 + breeze * 0.90).clamp(-1.5, 1.5)
                }
                PartMotionRole::HairBack
                | PartMotionRole::HairSideL
                | PartMotionRole::HairSideR => {
                    (-inp.vel_x * 0.22 - inp.body_sway * 0.18 + breeze).clamp(-1.6, 1.6)
                }
                PartMotionRole::MochiGlasses => {
                    (-inp.vel_x * 0.25 + (inp.time_s * 1.45 + ph).sin() * 0.18).clamp(-1.2, 1.2)
                }
                _ => 0.0,
            };
            let target_y = if sp.role == PartMotionRole::MochiGlasses {
                (-inp.vel_y * 0.25
                    + inp.talk_bob * 0.95
                    + inp.squish * 30.0
                    + (inp.breath - 0.5) * 0.35)
                    .clamp(-1.6, 1.6)
            } else {
                (-inp.vel_y * 0.15
                    + inp.breath * 0.22
                    + inp.talk_bob * 0.55
                    + inp.squish * 24.0)
                    .clamp(-1.6, 1.6)
            };

            part.spring_x.step(target_x, omega, zeta, inp.dt);
            part.spring_y.step(target_y, omega * 1.12, zeta, inp.dt);

            let (tx, ty, rot_rad, sx, sy, bend_dir_y, bend_span_px, bend_dx) = match sp.role {
                PartMotionRole::Planted => (
                    0.0,
                    0.0,
                    0.0,
                    1.0 + sp.scale_breath[0] * (inp.breath - 0.5),
                    1.0 + sp.scale_breath[1] * (inp.breath - 0.5),
                    1.0,
                    120.0,
                    0.0,
                ),
                PartMotionRole::Torso => (
                    sp.parallax[0] * inp.body_sway * 2.5 + sp.breath_px[0] * inp.breath,
                    sp.parallax[1] * inp.angle_y * 2.2
                        + sp.breath_px[1] * inp.breath
                        + inp.talk_bob * 1.2
                        - inp.squish * 110.0,
                    (sp.rot_deg.to_radians() * 0.65) * (inp.body_sway * 0.65 + inp.angle_x * 0.35),
                    1.0 + sp.scale_breath[0] * (inp.breath - 0.5) + inp.squish * 1.1,
                    1.0 + sp.scale_breath[1] * (inp.breath - 0.5) - inp.squish * 0.7,
                    1.0,
                    180.0,
                    0.0,
                ),
                PartMotionRole::MochiBody => (
                    sp.parallax[0] * inp.body_sway * 7.5 + sp.breath_px[0] * inp.breath,
                    sp.parallax[1] * inp.angle_y * 4.5
                        + sp.breath_px[1] * inp.breath
                        + inp.talk_bob * 2.2
                        - inp.squish * 140.0,
                    (sp.rot_deg.to_radians() * 0.55) * (inp.body_sway * 0.65 + inp.angle_x * 0.35),
                    1.0 + sp.scale_breath[0] * (inp.breath - 0.5) + inp.squish * 1.1,
                    1.0 + sp.scale_breath[1] * (inp.breath - 0.5) - inp.squish * 0.7,
                    1.0,
                    180.0,
                    0.0,
                ),
                PartMotionRole::Neck => (
                    sp.parallax[0] * inp.angle_x * 0.8,
                    0.0,
                    inp.angle_z * 0.35 + sp.rot_deg.to_radians() * 0.25 * inp.angle_x,
                    1.0,
                    1.0,
                    1.0,
                    80.0,
                    0.0,
                ),
                PartMotionRole::Head => (
                    sp.parallax[0] * inp.angle_x * 1.4,
                    sp.parallax[1] * inp.angle_y * 1.1,
                    inp.angle_z * 1.05
                        + sp.rot_deg.to_radians() * (0.50 * inp.angle_x + 0.16 * inp.talk_bob),
                    1.0,
                    1.0,
                    1.0,
                    120.0,
                    0.0,
                ),
                PartMotionRole::FaceFeatures => (
                    sp.parallax[0] * inp.angle_x * 4.0,
                    sp.parallax[1] * inp.angle_y * 2.8,
                    0.0,
                    1.0,
                    1.0,
                    1.0,
                    100.0,
                    0.0,
                ),
                PartMotionRole::MochiFace => (
                    sp.parallax[0] * inp.angle_x * 8.5 + sp.breath_px[0] * inp.breath,
                    sp.parallax[1] * inp.angle_y * 6.0
                        + sp.breath_px[1] * inp.breath
                        + inp.talk_bob * 1.8,
                    inp.angle_z + sp.rot_deg.to_radians() * 0.45 * inp.angle_x,
                    1.0,
                    1.0,
                    1.0,
                    120.0,
                    0.0,
                ),
                PartMotionRole::HairBangs => (
                    sp.parallax[0] * inp.angle_x * 4.8 + part.spring_x.pos * 1.1,
                    sp.parallax[1] * inp.angle_y * 3.0 + part.spring_y.pos * 0.6,
                    sp.rot_deg.to_radians() * (part.spring_x.pos * 0.72),
                    1.0,
                    1.0,
                    1.0,
                    110.0,
                    sp.bend_px * part.spring_x.pos,
                ),
                PartMotionRole::Ahoge => (
                    sp.parallax[0] * inp.angle_x * 4.0 + part.spring_x.pos * 1.4,
                    sp.parallax[1] * inp.angle_y * 3.0 + part.spring_y.pos * 1.1,
                    sp.rot_deg.to_radians() * (part.spring_x.pos * 0.85),
                    1.0,
                    1.0,
                    -1.0,
                    82.0,
                    sp.bend_px * part.spring_x.pos,
                ),
                PartMotionRole::MochiTuft => (
                    0.0,
                    0.0,
                    sp.rot_deg.to_radians() * (part.spring_x.pos * 0.65),
                    1.0,
                    1.0,
                    -1.0,
                    92.0,
                    sp.bend_px * part.spring_x.pos,
                ),
                PartMotionRole::Skirt => (
                    sp.parallax[0] * (inp.body_sway * 4.0 + part.spring_x.pos * 3.0),
                    sp.breath_px[1] * inp.breath + part.spring_y.pos * 1.0,
                    sp.rot_deg.to_radians() * (part.spring_x.pos * 0.65),
                    1.0 + sp.scale_breath[0] * (inp.breath - 0.5),
                    1.0,
                    1.0,
                    175.0,
                    sp.bend_px * part.spring_x.pos,
                ),
                PartMotionRole::CoatHem => (
                    sp.parallax[0] * (inp.body_sway * 2.4 + part.spring_x.pos * 2.6),
                    sp.breath_px[1] * inp.breath + part.spring_y.pos * 0.7,
                    sp.rot_deg.to_radians() * (part.spring_x.pos * 0.75),
                    1.0 + sp.scale_breath[0] * (inp.breath - 0.5),
                    1.0,
                    1.0,
                    175.0,
                    sp.bend_px * part.spring_x.pos,
                ),
                PartMotionRole::Collar => (
                    sp.parallax[0] * inp.angle_x * 1.8 + part.spring_x.pos * 1.0,
                    sp.breath_px[1] * inp.breath + part.spring_y.pos * 0.5,
                    sp.rot_deg.to_radians() * (part.spring_x.pos * 0.75),
                    1.0 + sp.scale_breath[0] * (inp.breath - 0.5),
                    1.0,
                    1.0,
                    85.0,
                    sp.bend_px * part.spring_x.pos,
                ),
                PartMotionRole::Arm => (
                    sp.parallax[0] * inp.angle_x * 1.6 + part.spring_x.pos * 1.1,
                    sp.breath_px[1] * inp.breath + part.spring_y.pos * 0.8,
                    sp.rot_deg.to_radians()
                        * (part.spring_x.pos * 0.85 + (inp.breath - 0.5) * 0.25),
                    1.0,
                    1.0,
                    1.0,
                    260.0,
                    sp.bend_px * part.spring_x.pos,
                ),
                PartMotionRole::HairBack
                | PartMotionRole::HairSideL
                | PartMotionRole::HairSideR => (
                    sp.parallax[0] * inp.angle_x * 3.2 + part.spring_x.pos * 1.8,
                    sp.parallax[1] * inp.angle_y * 2.4 + part.spring_y.pos * 1.3,
                    sp.rot_deg.to_radians() * (part.spring_x.pos * 0.72),
                    1.0,
                    1.0,
                    1.0,
                    145.0,
                    sp.bend_px * part.spring_x.pos,
                ),
                PartMotionRole::Ribbon => (
                    sp.parallax[0] * inp.angle_x * 2.5 + part.spring_x.pos * 1.2,
                    sp.parallax[1] * inp.angle_y * 2.2
                        + sp.breath_px[1] * inp.breath
                        - inp.talk_bob * 3.6,
                    sp.rot_deg.to_radians()
                        * (part.spring_x.pos * 0.75 + inp.talk_bob * 0.45),
                    1.0 + sp.scale_breath[0] * (inp.breath - 0.5),
                    1.0,
                    1.0,
                    90.0,
                    sp.bend_px * part.spring_x.pos,
                ),
                PartMotionRole::MochiPaws => (
                    sp.parallax[0] * inp.angle_x * 2.5 + part.spring_x.pos * 2.8,
                    -2.2 * inp.breath - inp.talk_bob * 4.2 + part.spring_y.pos * 1.6,
                    sp.rot_deg.to_radians()
                        * (part.spring_x.pos * 0.95 + inp.talk_bob * 0.55),
                    1.0 + sp.scale_breath[0] * (inp.breath - 0.5),
                    1.0,
                    1.0,
                    90.0,
                    sp.bend_px * part.spring_x.pos,
                ),
                PartMotionRole::MochiGlasses => (
                    sp.parallax[0] * inp.angle_x * 6.5 + part.spring_x.pos * 1.6,
                    sp.parallax[1] * inp.angle_y * 5.0 + part.spring_y.pos * 2.6,
                    sp.rot_deg.to_radians() * (inp.angle_x * 0.55 + part.spring_x.pos * 0.65),
                    1.0,
                    1.0,
                    1.0,
                    60.0,
                    0.0,
                ),
                PartMotionRole::Accessory | PartMotionRole::Custom => (
                    sp.parallax[0] * inp.angle_x * 4.0,
                    sp.parallax[1] * inp.angle_y * 3.0 + sp.breath_px[1] * inp.breath,
                    sp.rot_deg.to_radians() * inp.angle_x * 0.45,
                    1.0,
                    1.0,
                    1.0,
                    80.0,
                    sp.bend_px * part.spring_x.pos,
                ),
            };

            locals[i] = Mat2D::around_pivot(
                sp.pivot_px[0],
                sp.pivot_px[1],
                tx,
                ty,
                rot_rad,
                sx,
                sy,
            );
            bend_meta[i] = (bend_dir_y, bend_span_px, bend_dx);
        }

        // Compose world matrices safely (supports forward or arbitrary tree index order).
        let mut worlds = vec![Mat2D::identity(); n];
        let mut resolved = vec![false; n];
        fn resolve_world(
            idx: usize,
            locals: &[Mat2D],
            parts: &[LoadedPartLayer],
            worlds: &mut [Mat2D],
            resolved: &mut [bool],
            depth: usize,
        ) -> Mat2D {
            if resolved[idx] {
                return worlds[idx];
            }
            let mat = if depth < locals.len() {
                if let Some(p_idx) = parts[idx].parent_idx {
                    let pw = resolve_world(p_idx, locals, parts, worlds, resolved, depth + 1);
                    pw.mul(locals[idx])
                } else {
                    locals[idx]
                }
            } else {
                locals[idx]
            };
            worlds[idx] = mat;
            resolved[idx] = true;
            mat
        }
        for i in 0..n {
            let _ = resolve_world(i, &locals, &self.parts, &mut worlds, &mut resolved, 0);
        }

        for (i, part) in self.parts.iter_mut().enumerate() {
            let w_mat = worlds[i];
            let inv = w_mat.inverse().unwrap_or_else(Mat2D::identity);
            let (_, world_py) = w_mat.map_point(part.spec.pivot_px[0], part.spec.pivot_px[1]);
            let (bdir, bspan, bdx) = bend_meta[i];
            part.uniform = PartGpuUniform {
                inv_row0: [inv.a, inv.b, inv.c, cw],
                inv_row1: [inv.d, inv.e, inv.f, ch],
                deform: [world_py, bdir, bspan, bdx],
                meta: [1.0, part.spec.opacity.clamp(0.0, 1.0), 0.0, 0.0],
            };
        }
    }

    /// Reference pure-Rust software renderer producing exact premultiplied-alpha then unpremultiplied RGBA
    /// matching the GPU `PART_SHADER_WGSL` math (`rendered` at rest or under current pose `uniform`s).
    pub fn render_cpu_rgba(&self) -> RgbaBitmap {
        let w = self.canvas_size[0];
        let h = self.canvas_size[1];
        let mut pm_acc = vec![0.0_f32; (w as usize) * (h as usize) * 4];

        for part in &self.parts {
            if part.uniform.meta[0] < 0.5 {
                continue;
            }
            let part_alpha = part.uniform.meta[1].clamp(0.0, 1.0);
            if part_alpha <= 0.0005 {
                continue;
            }
            let bmp = &part.current_bitmap;
            let bw = bmp.width as f32;
            let bh = bmp.height as f32;
            let cw = part.uniform.inv_row0[3].max(1.0);
            let ch = part.uniform.inv_row1[3].max(1.0);
            let scale_x = bw / cw;
            let scale_y = bh / ch;
            let [ia, ib, ic, _] = part.uniform.inv_row0;
            let [id, ie, if_, _] = part.uniform.inv_row1;
            let [wpy, bdir, bspan, bdx] = part.uniform.deform;

            for oy in 0..h {
                let fy = oy as f32 + 0.5;
                let rel = (((fy - wpy) * bdir) / bspan.max(1.0)).clamp(0.0, 1.0);
                let bend = bdx * rel * rel;
                for ox in 0..w {
                    let fx = ox as f32 + 0.5;
                    let sx = (ia * fx + ib * fy + ic - bend) * scale_x;
                    let sy = (id * fx + ie * fy + if_) * scale_y;
                    if sx < 0.0 || sx >= bw || sy < 0.0 || sy >= bh {
                        continue;
                    }
                    let px = sx - 0.5;
                    let py = sy - 0.5;
                    let x0 = (px.floor() as i32).clamp(0, bmp.width as i32 - 1) as u32;
                    let y0 = (py.floor() as i32).clamp(0, bmp.height as i32 - 1) as u32;
                    let x1 = (x0 + 1).min(bmp.width - 1);
                    let y1 = (y0 + 1).min(bmp.height - 1);
                    let fr_x = px - px.floor();
                    let fr_y = py - py.floor();

                    let sample_pm = |xx: u32, yy: u32| -> [f32; 4] {
                        let idx = ((yy * bmp.width + xx) * 4) as usize;
                        let a = bmp.data[idx + 3] as f32 / 255.0;
                        [
                            (bmp.data[idx] as f32 / 255.0) * a,
                            (bmp.data[idx + 1] as f32 / 255.0) * a,
                            (bmp.data[idx + 2] as f32 / 255.0) * a,
                            a,
                        ]
                    };
                    let p00 = sample_pm(x0, y0);
                    let p10 = sample_pm(x1, y0);
                    let p01 = sample_pm(x0, y1);
                    let p11 = sample_pm(x1, y1);
                    let mut src_pm = [0.0_f32; 4];
                    for c in 0..4 {
                        let top = p00[c] + (p10[c] - p00[c]) * fr_x;
                        let bot = p01[c] + (p11[c] - p01[c]) * fr_x;
                        src_pm[c] = (top + (bot - top) * fr_y) * part_alpha;
                    }
                    if src_pm[3] <= 0.0005 {
                        continue;
                    }
                    let d_idx = ((oy * w + ox) * 4) as usize;
                    let inv_a = 1.0 - src_pm[3];
                    for c in 0..4 {
                        pm_acc[d_idx + c] = src_pm[c] + pm_acc[d_idx + c] * inv_a;
                    }
                }
            }
        }

        let mut out = RgbaBitmap::new_transparent(w, h);
        for px_idx in 0..((w as usize) * (h as usize)) {
            let s = px_idx * 4;
            let a = pm_acc[s + 3].clamp(0.0, 1.0);
            if a <= 0.001 {
                continue;
            }
            out.data[s] = ((pm_acc[s] / a) * 255.0).round().clamp(0.0, 255.0) as u8;
            out.data[s + 1] = ((pm_acc[s + 1] / a) * 255.0).round().clamp(0.0, 255.0) as u8;
            out.data[s + 2] = ((pm_acc[s + 2] / a) * 255.0).round().clamp(0.0, 255.0) as u8;
            out.data[s + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_gpu_uniform_is_64_bytes() {
        assert_eq!(std::mem::size_of::<PartGpuUniform>(), 64);
    }

    #[test]
    fn neo_default_sidecar_loads_and_renders_expressions() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let assets = root.join("assets");
        let shell_dir = assets.join("shells").join("neo_default");
        let tmp_user = std::env::temp_dir().join("nanikaai-l2d-unit");
        let mut sak_rig =
            discover_and_load_rig("neo_default", &shell_dir, &tmp_user, &assets, Scope::Sakura)
                .expect("Sakura rig should load");
        assert!(sak_rig.parts.len() >= 18);
        let unyu_rig =
            discover_and_load_rig("neo_default", &shell_dir, &tmp_user, &assets, Scope::Kero)
                .expect("Unyu rig should load");
        assert!(unyu_rig.parts.len() >= 6);

        sak_rig.step(&Live2dMotionInputs {
            dt: 0.033,
            time_s: 1.4,
            breath: 0.7,
            body_sway: 0.4,
            squish: 0.01,
            talk_bob: 0.2,
            angle_x: 0.8,
            angle_y: -0.3,
            angle_z: 0.025,
            vel_x: 1.2,
            vel_y: -0.4,
            speech_impulse: true,
        });
        let rendered = sak_rig.render_cpu_rgba();
        assert!(rendered.count_opaque_pixels() > 50_000);
    }
}
