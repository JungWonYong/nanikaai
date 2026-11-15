use encoding_rs::SHIFT_JIS;
use image::GenericImageView;
use rand::seq::SliceRandom;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;
use zip::ZipArchive;

use crate::dialogue_parser::Scope;

pub const CURATED_SHELL_ORDER: &[&str] = &[
    "master",
    "mayura",
    "mayura_santa",
    "futaba",
    "naru",
    "seriko",
    "ninni",
    "dokuko",
    "yohko",
    "kikka_master",
    "neo_default",
    "akari_default",
];

pub const CURATED_BALLOON_ORDER: &[&str] = &[
    "twin tail",
    "wiz",
    "emily4",
    "switch_L",
    "default",
    "default_dot",
    "clean_default",
];

#[derive(Debug, Clone)]
pub struct RgbaBitmap {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl RgbaBitmap {
    pub fn new_transparent(width: u32, height: u32) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        Self {
            width: w,
            height: h,
            data: vec![0u8; (w as usize) * (h as usize) * 4],
        }
    }

    pub fn from_solid(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let mut data = Vec::with_capacity((w as usize) * (h as usize) * 4);
        for _ in 0..(w as usize * h as usize) {
            data.extend_from_slice(&[r, g, b, a]);
        }
        Self {
            width: w,
            height: h,
            data,
        }
    }

    pub fn is_opaque_at(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return false;
        }
        let idx = ((y as u32 * self.width + x as u32) * 4 + 3) as usize;
        self.data.get(idx).copied().unwrap_or(0) > 20
    }

    pub fn count_opaque_pixels(&self) -> usize {
        self.data.chunks_exact(4).filter(|px| px[3] > 20).count()
    }

    pub fn opaque_bbox(&self) -> Option<(i32, i32, i32, i32)> {
        let mut min_x = i32::MAX;
        let mut min_y = i32::MAX;
        let mut max_x = i32::MIN;
        let mut max_y = i32::MIN;
        let w = self.width as i32;
        let h = self.height as i32;
        for y in 0..h {
            for x in 0..w {
                let idx = ((y as u32 * self.width + x as u32) * 4 + 3) as usize;
                if self.data[idx] > 20 {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x + 1);
                    max_y = max_y.max(y + 1);
                }
            }
        }
        if min_x < max_x && min_y < max_y {
            Some((min_x, min_y, max_x, max_y))
        } else {
            None
        }
    }

    pub fn resized(&self, new_w: u32, new_h: u32) -> Self {
        let nw = new_w.max(1);
        let nh = new_h.max(1);
        if nw == self.width && nh == self.height {
            return self.clone();
        }
        if let Some(img) = image::RgbaImage::from_raw(self.width, self.height, self.data.clone()) {
            let scaled = image::imageops::resize(
                &img,
                nw,
                nh,
                image::imageops::FilterType::CatmullRom,
            );
            return Self {
                width: nw,
                height: nh,
                data: scaled.into_raw(),
            };
        }
        Self::new_transparent(nw, nh)
    }

    pub fn alpha_composite(&mut self, src: &RgbaBitmap, dst_x: i32, dst_y: i32) {
        let sw = src.width as i32;
        let sh = src.height as i32;
        let dw = self.width as i32;
        let dh = self.height as i32;

        for sy in 0..sh {
            let dy = dst_y + sy;
            if dy < 0 || dy >= dh {
                continue;
            }
            for sx in 0..sw {
                let dx = dst_x + sx;
                if dx < 0 || dx >= dw {
                    continue;
                }
                let sidx = ((sy as u32 * src.width + sx as u32) * 4) as usize;
                let sa = src.data[sidx + 3] as u32;
                if sa == 0 {
                    continue;
                }
                let didx = ((dy as u32 * self.width + dx as u32) * 4) as usize;
                if sa == 255 {
                    self.data[didx..didx + 4].copy_from_slice(&src.data[sidx..sidx + 4]);
                    continue;
                }
                let da = self.data[didx + 3] as u32;
                let sr = src.data[sidx] as u32;
                let sg = src.data[sidx + 1] as u32;
                let sb = src.data[sidx + 2] as u32;
                let dr = self.data[didx] as u32;
                let dg = self.data[didx + 1] as u32;
                let db = self.data[didx + 2] as u32;

                let inv_sa = 255 - sa;
                let out_a = sa + (da * inv_sa + 127) / 255;
                if out_a == 0 {
                    self.data[didx..didx + 4].copy_from_slice(&[0, 0, 0, 0]);
                } else {
                    let r_num = sr * sa + (dr * da * inv_sa + 127) / 255;
                    let g_num = sg * sa + (dg * da * inv_sa + 127) / 255;
                    let b_num = sb * sa + (db * da * inv_sa + 127) / 255;
                    self.data[didx] = (r_num / out_a).min(255) as u8;
                    self.data[didx + 1] = (g_num / out_a).min(255) as u8;
                    self.data[didx + 2] = (b_num / out_a).min(255) as u8;
                    self.data[didx + 3] = out_a.min(255) as u8;
                }
            }
        }
    }

    pub fn blend_pixel(&mut self, x: i32, y: i32, r: u8, g: u8, b: u8, a: u8) {
        if a == 0 || x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let didx = ((y as u32 * self.width + x as u32) * 4) as usize;
        if a == 255 {
            self.data[didx] = r;
            self.data[didx + 1] = g;
            self.data[didx + 2] = b;
            self.data[didx + 3] = 255;
            return;
        }
        let sa = a as u32;
        let da = self.data[didx + 3] as u32;
        let inv_sa = 255 - sa;
        let out_a = sa + (da * inv_sa + 127) / 255;
        if out_a > 0 {
            self.data[didx] = ((r as u32 * sa + (self.data[didx] as u32 * da * inv_sa + 127) / 255) / out_a).min(255) as u8;
            self.data[didx + 1] = ((g as u32 * sa + (self.data[didx + 1] as u32 * da * inv_sa + 127) / 255) / out_a).min(255) as u8;
            self.data[didx + 2] = ((b as u32 * sa + (self.data[didx + 2] as u32 * da * inv_sa + 127) / 255) / out_a).min(255) as u8;
            self.data[didx + 3] = out_a.min(255) as u8;
        }
    }

    pub fn fill_rect_alpha(&mut self, x: i32, y: i32, w: i32, h: i32, r: u8, g: u8, b: u8, a: u8) {
        for py in y..(y + h) {
            for px in x..(x + w) {
                self.blend_pixel(px, py, r, g, b, a);
            }
        }
    }

    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        let img = image::RgbaImage::from_raw(self.width, self.height, self.data.clone())
            .ok_or_else(|| "Invalid RGBA buffer dimensions".to_string())?;
        img.save(path).map_err(|e| e.to_string())
    }
}

pub fn decode_ukagaka_bytes(buf: &[u8]) -> String {
    let head = String::from_utf8_lossy(&buf[..buf.len().min(512)]);
    static CHARSET_RE: OnceLock<Regex> = OnceLock::new();
    let re = CHARSET_RE.get_or_init(|| Regex::new(r"(?im)^\s*charset\s*,\s*([^\r\n]+)").unwrap());
    if let Some(caps) = re.captures(&head) {
        let cs = caps[1].trim().to_ascii_lowercase();
        if matches!(cs.as_str(), "shift_jis" | "shift-jis" | "sjis" | "cp932" | "windows-31j") {
            let (decoded, _, _) = SHIFT_JIS.decode(buf);
            return decoded.into_owned();
        }
        if matches!(cs.as_str(), "utf-8" | "utf8") {
            return String::from_utf8_lossy(buf)
                .trim_start_matches('\u{feff}')
                .to_string();
        }
    }
    if let Ok(s) = std::str::from_utf8(buf) {
        return s.trim_start_matches('\u{feff}').to_string();
    }
    let (decoded, _, _) = SHIFT_JIS.decode(buf);
    decoded.into_owned()
}

pub fn load_ukagaka_rgba(png_path: &Path, pna_path: Option<&Path>) -> Option<RgbaBitmap> {
    let raw_im = image::open(png_path).ok()?;
    let has_native_alpha = raw_im.color().has_alpha();
    let (w, h) = raw_im.dimensions();
    let mut data = raw_im.to_rgba8().into_raw();

    if let Some(pna) = pna_path {
        if pna.is_file() {
            if let Ok(pna_im) = image::open(pna) {
                if pna_im.dimensions() == (w, h) {
                    let gray = pna_im.to_luma8();
                    for (i, alpha_px) in gray.as_raw().iter().enumerate() {
                        data[i * 4 + 3] = *alpha_px;
                    }
                    return Some(RgbaBitmap { width: w, height: h, data });
                }
            }
        }
    }

    if has_native_alpha {
        return Some(RgbaBitmap { width: w, height: h, data });
    }

    if w == 0 || h == 0 || data.len() < 4 {
        return Some(RgbaBitmap { width: w, height: h, data });
    }

    let (kr, kg, kb) = (data[0], data[1], data[2]);
    for px in data.chunks_exact_mut(4) {
        if px[0] == kr && px[1] == kg && px[2] == kb {
            px[3] = 0;
        }
    }
    Some(RgbaBitmap { width: w, height: h, data })
}

fn int_safe(val: Option<&str>, default: i32) -> i32 {
    val.and_then(|s| s.trim().parse::<i32>().ok()).unwrap_or(default)
}

#[derive(Debug, Clone)]
pub struct ShellDescript {
    pub name: String,
    pub sakura_name: String,
    pub kero_name: String,
    pub sakura_balloon_offsetx: i32,
    pub sakura_balloon_offsety: i32,
    pub kero_balloon_offsetx: i32,
    pub kero_balloon_offsety: i32,
    pub sakura_balloon_alignment: String,
    pub kero_balloon_alignment: String,
    pub display_scale: f64,
}

impl Default for ShellDescript {
    fn default() -> Self {
        Self {
            name: "マスター".to_string(),
            sakura_name: "さくら".to_string(),
            kero_name: "うにゅう".to_string(),
            sakura_balloon_offsetx: 10,
            sakura_balloon_offsety: 60,
            kero_balloon_offsetx: 0,
            kero_balloon_offsety: 160,
            sakura_balloon_alignment: "none".to_string(),
            kero_balloon_alignment: "none".to_string(),
            display_scale: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollisionRect {
    pub cid: i32,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct SurfaceElement {
    pub eid: i32,
    pub method: String,
    pub filename: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone)]
pub struct SerikoPattern {
    pub index: i32,
    pub method: String,
    pub surface_id: i32,
    pub delay_ms: u64,
    pub offset_x: i32,
    pub offset_y: i32,
    pub alt_ids: Vec<i32>,
}

#[derive(Debug, Clone)]
pub struct SerikoAnimation {
    pub aid: i32,
    pub interval: String,
    pub patterns: Vec<SerikoPattern>,
}

#[derive(Debug, Clone)]
pub struct SurfaceDef {
    pub sid: i32,
    pub image_path: Option<PathBuf>,
    pub pna_path: Option<PathBuf>,
    pub elements: Vec<SurfaceElement>,
    pub collisions: Vec<CollisionRect>,
    pub animations: Vec<SerikoAnimation>,
    pub sakura_balloon_offsetx: Option<i32>,
    pub sakura_balloon_offsety: Option<i32>,
    pub kero_balloon_offsetx: Option<i32>,
    pub kero_balloon_offsety: Option<i32>,
}

impl SurfaceDef {
    pub fn new(sid: i32) -> Self {
        Self {
            sid,
            image_path: None,
            pna_path: None,
            elements: Vec::new(),
            collisions: Vec::new(),
            animations: Vec::new(),
            sakura_balloon_offsetx: None,
            sakura_balloon_offsety: None,
            kero_balloon_offsetx: None,
            kero_balloon_offsety: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShellManifest {
    pub shell_id: String,
    pub dir_path: PathBuf,
    pub descript: ShellDescript,
    pub surfaces: BTreeMap<i32, SurfaceDef>,
    pub sakura_aliases: HashMap<i32, Vec<i32>>,
    pub kero_aliases: HashMap<i32, Vec<i32>>,
    size_cache: HashMap<i32, Option<(u32, u32)>>,
    overlay_ids_cache: Option<HashSet<i32>>,
}

impl ShellManifest {
    pub fn surface_size(&mut self, sid: i32) -> Option<(u32, u32)> {
        if let Some(cached) = self.size_cache.get(&sid) {
            return *cached;
        }
        let mut sz: Option<(u32, u32)> = None;
        if let Some(sdef) = self.surfaces.get(&sid) {
            if sdef.elements.is_empty() {
                if let Some(ref p) = sdef.image_path {
                    if let Ok(dims) = image::image_dimensions(p) {
                        sz = Some(dims);
                    }
                }
            }
        }
        if sz.is_none() {
            if let Some(img) = self.compose_base_rgba(sid) {
                sz = Some((img.width, img.height));
            }
        }
        self.size_cache.insert(sid, sz);
        sz
    }

    fn overlay_surface_ids(&mut self) -> HashSet<i32> {
        if let Some(ref cached) = self.overlay_ids_cache {
            return cached.clone();
        }
        let mut alias_targets = HashSet::new();
        for targets in self.sakura_aliases.values().chain(self.kero_aliases.values()) {
            for &t in targets {
                alias_targets.insert(t);
            }
        }
        let mut overlays = HashSet::new();
        for sdef in self.surfaces.values() {
            for anim in &sdef.animations {
                for pat in &anim.patterns {
                    if pat.surface_id < 0
                        || matches!(pat.method.as_str(), "base" | "move" | "alternativestart")
                    {
                        continue;
                    }
                    if pat.surface_id == 0 || pat.surface_id == 10 || alias_targets.contains(&pat.surface_id) {
                        continue;
                    }
                    if pat.surface_id >= 100 || pat.offset_x != 0 || pat.offset_y != 0 {
                        overlays.insert(pat.surface_id);
                    }
                }
            }
        }
        self.overlay_ids_cache = Some(overlays.clone());
        overlays
    }

    pub fn is_full_pose_for_scope(
        &mut self,
        scope: Scope,
        sid: i32,
        allow_explicit_alias: bool,
    ) -> bool {
        if !self.surfaces.contains_key(&sid) {
            return false;
        }
        if (scope == Scope::Sakura && sid == 0) || (scope == Scope::Kero && sid == 10) {
            return true;
        }
        if scope == Scope::Kero && (0..=9).contains(&sid) {
            return false;
        }
        if scope == Scope::Sakura && (10..=19).contains(&sid) {
            return false;
        }
        if !allow_explicit_alias && self.overlay_surface_ids().contains(&sid) {
            return false;
        }

        let Some(sz) = self.surface_size(sid) else {
            return false;
        };
        let ref_sid = if scope == Scope::Kero && self.surfaces.contains_key(&10) {
            10
        } else {
            0
        };
        if let Some(ref_sz) = self.surface_size(ref_sid) {
            if (sz.0 as f64) < (ref_sz.0 as f64) * 0.45 || (sz.1 as f64) < (ref_sz.1 as f64) * 0.45 {
                return false;
            }
        }
        if (0..=9).contains(&sid) {
            return scope == Scope::Sakura;
        }
        if (10..=19).contains(&sid) {
            return scope == Scope::Kero;
        }

        let mut kero_alias_ids: HashSet<i32> = self.kero_aliases.keys().copied().collect();
        for ts in self.kero_aliases.values() {
            for &t in ts {
                kero_alias_ids.insert(t);
            }
        }
        let s0_sz = self.surface_size(0);
        let s10_sz = self.surface_size(10);
        let is_kero_extra = kero_alias_ids.contains(&sid)
            || matches!((s0_sz, s10_sz), (Some(a), Some(b)) if a != b && sz == b);
        if scope == Scope::Kero {
            is_kero_extra
        } else {
            !is_kero_extra
        }
    }

    pub fn valid_surface_ids(&mut self, scope: Scope, max_id: i32) -> Vec<i32> {
        let candidate_ids: Vec<i32> = self
            .surfaces
            .keys()
            .copied()
            .filter(|&sid| (0..=max_id).contains(&sid))
            .collect();
        let mut valid = BTreeSet::new();
        for sid in candidate_ids {
            if self.is_full_pose_for_scope(scope, sid, false) {
                valid.insert(sid);
            }
        }
        let alias_entries: Vec<(i32, Vec<i32>)> = match scope {
            Scope::Kero => self.kero_aliases.iter().map(|(&k, v)| (k, v.clone())).collect(),
            Scope::Sakura => self.sakura_aliases.iter().map(|(&k, v)| (k, v.clone())).collect(),
        };
        for (k, targets) in alias_entries {
            if !(0..=max_id).contains(&k) {
                continue;
            }
            if scope == Scope::Kero && k < 10 {
                continue;
            }
            if scope == Scope::Sakura && (10..=19).contains(&k) {
                continue;
            }
            if targets
                .into_iter()
                .any(|t| self.is_full_pose_for_scope(scope, t, true))
            {
                valid.insert(k);
            }
        }
        if valid.is_empty() {
            if scope == Scope::Kero && self.surfaces.contains_key(&10) {
                return vec![10];
            }
            if self.surfaces.contains_key(&0) {
                return vec![0];
            }
        }
        valid.into_iter().collect()
    }

    pub fn resolve_surface_id(&mut self, scope: Scope, requested_id: i32) -> i32 {
        let alias_targets = match scope {
            Scope::Kero => self.kero_aliases.get(&requested_id).cloned(),
            Scope::Sakura => self.sakura_aliases.get(&requested_id).cloned(),
        };
        if let Some(targets) = alias_targets {
            let mut cands = Vec::new();
            for cand in targets {
                if self.is_full_pose_for_scope(scope, cand, true) {
                    cands.push(cand);
                }
            }
            if !cands.is_empty() {
                let mut rng = rand::thread_rng();
                if let Some(&picked) = cands.choose(&mut rng) {
                    return picked;
                }
            }
        }

        if self.is_full_pose_for_scope(scope, requested_id, false) {
            return requested_id;
        }

        if scope == Scope::Kero {
            if (0..=9).contains(&requested_id) {
                if matches!(requested_id, 1 | 2 | 4 | 7 | 9)
                    && self.is_full_pose_for_scope(Scope::Kero, 11, false)
                {
                    return 11;
                }
                if matches!(requested_id, 3 | 5 | 6 | 8) {
                    if self.is_full_pose_for_scope(Scope::Kero, 12, false) {
                        return 12;
                    }
                    if requested_id == 8 && self.is_full_pose_for_scope(Scope::Kero, 11, false) {
                        return 11;
                    }
                }
            }
            if self.surfaces.contains_key(&10) {
                return 10;
            }
            if let Some(&first_kero) = self.surfaces.keys().find(|&&s| (10..=19).contains(&s)) {
                return first_kero;
            }
            return 0;
        }

        if (10..=19).contains(&requested_id) {
            let mapped = requested_id - 10;
            if self.sakura_aliases.contains_key(&mapped)
                || self.is_full_pose_for_scope(Scope::Sakura, mapped, false)
            {
                return self.resolve_surface_id(Scope::Sakura, mapped);
            }
        }
        0
    }

    pub fn compose_base_rgba(&self, sid: i32) -> Option<RgbaBitmap> {
        let sdef = self.surfaces.get(&sid)?;
        let mut canvas: Option<RgbaBitmap> = None;

        if !sdef.elements.is_empty() {
            let mut elems = sdef.elements.clone();
            elems.sort_by_key(|e| e.eid);
            for elem in elems {
                let rel = elem.filename.replace('\\', "/");
                let epath = self.dir_path.join(rel);
                if !epath.is_file() {
                    continue;
                }
                let pna = epath.with_extension("pna");
                let layer = load_ukagaka_rgba(&epath, if pna.is_file() { Some(&pna) } else { None })?;
                let ox = elem.x.max(0) as u32;
                let oy = elem.y.max(0) as u32;
                let need_w = (ox + layer.width).max(1);
                let need_h = (oy + layer.height).max(1);
                match canvas.as_mut() {
                    None => {
                        let mut c = RgbaBitmap::new_transparent(need_w, need_h);
                        c.alpha_composite(&layer, ox as i32, oy as i32);
                        canvas = Some(c);
                    }
                    Some(c) => {
                        if need_w > c.width || need_h > c.height {
                            let mut expanded = RgbaBitmap::new_transparent(
                                c.width.max(need_w),
                                c.height.max(need_h),
                            );
                            expanded.alpha_composite(c, 0, 0);
                            *c = expanded;
                        }
                        c.alpha_composite(&layer, ox as i32, oy as i32);
                    }
                }
            }
        }

        if canvas.is_none() {
            if let Some(ref img_path) = sdef.image_path {
                if img_path.is_file() {
                    canvas = load_ukagaka_rgba(img_path, sdef.pna_path.as_deref());
                }
            }
        }
        canvas
    }

    pub fn hit_collision(
        &self,
        sid: i32,
        scope: Scope,
        base_bmp: &RgbaBitmap,
        x: i32,
        y: i32,
    ) -> Option<CollisionRect> {
        let mut cols = self
            .surfaces
            .get(&sid)
            .map(|s| s.collisions.clone())
            .unwrap_or_default();
        if cols.is_empty() {
            let base_id = if scope == Scope::Kero { 10 } else { 0 };
            cols = self
                .surfaces
                .get(&base_id)
                .map(|s| s.collisions.clone())
                .unwrap_or_default();
        }
        for col in &cols {
            if x >= col.left && x <= col.right && y >= col.top && y <= col.bottom {
                return Some(col.clone());
            }
        }
        if !base_bmp.is_opaque_at(x, y) {
            return None;
        }
        let (bx0, by0, bx1, by1) = base_bmp.opaque_bbox()?;
        let bh = (by1 - by0).max(1);
        if cols.is_empty() {
            let head_bottom = by0 + ((bh as f64) * 0.38) as i32;
            if y >= by0 && y <= head_bottom {
                return Some(CollisionRect {
                    cid: -1,
                    left: bx0,
                    top: by0,
                    right: bx1,
                    bottom: head_bottom,
                    name: "Head".to_string(),
                });
            }
        } else {
            let has_head = cols.iter().any(|c| {
                let low = c.name.to_ascii_lowercase();
                low.contains("head") || c.name.contains('頭') || c.name.contains('髪')
            });
            if !has_head {
                let face_top = cols
                    .iter()
                    .filter(|c| c.name.to_ascii_lowercase().contains("face"))
                    .map(|c| c.top)
                    .min();
                let head_bottom = face_top
                    .map(|ft| ft - 1)
                    .unwrap_or_else(|| by0 + ((bh as f64) * 0.24) as i32);
                if y >= by0 && y <= head_bottom {
                    return Some(CollisionRect {
                        cid: -1,
                        left: bx0,
                        top: by0,
                        right: bx1,
                        bottom: head_bottom,
                        name: "Head".to_string(),
                    });
                }
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct StrokeDetector {
    hover_region: String,
    hover_dist: f64,
    hover_last_pt: Option<(f64, f64)>,
    hover_last_raw_pt: Option<(i32, i32)>,
    hover_last_ts: Instant,
}

impl Default for StrokeDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl StrokeDetector {
    pub fn new() -> Self {
        Self {
            hover_region: String::new(),
            hover_dist: 0.0,
            hover_last_pt: None,
            hover_last_raw_pt: None,
            hover_last_ts: Instant::now(),
        }
    }

    pub fn update_at(&mut self, pt: (f64, f64), col: Option<&CollisionRect>, now: Instant) -> Option<String> {
        let raw_i = (pt.0.round() as i32, pt.1.round() as i32);
        if self.hover_last_raw_pt == Some(raw_i) {
            return None;
        }
        self.hover_last_raw_pt = Some(raw_i);

        if let Some(col) = col {
            let elapsed = now.saturating_duration_since(self.hover_last_ts).as_secs_f64();
            if self.hover_region != col.name || elapsed > 0.90 {
                self.hover_region = col.name.clone();
                self.hover_dist = 0.0;
            } else if let Some((lx, ly)) = self.hover_last_pt {
                let step = (pt.0 - lx).hypot(pt.1 - ly);
                self.hover_dist += step.min(65.0);
            }
            self.hover_last_ts = now;
            self.hover_last_pt = Some(pt);
            if self.hover_dist >= 130.0 {
                self.hover_dist = 0.0;
                self.hover_last_pt = None;
                return Some(col.name.clone());
            }
        } else {
            let elapsed = now.saturating_duration_since(self.hover_last_ts).as_secs_f64();
            if elapsed > 0.65 {
                self.hover_region.clear();
                self.hover_dist = 0.0;
                self.hover_last_pt = None;
            } else {
                self.hover_last_pt = Some(pt);
            }
        }
        None
    }

    pub fn clear_if_stale(&mut self, now: Instant) {
        if !self.hover_region.is_empty()
            && now.saturating_duration_since(self.hover_last_ts).as_secs_f64() > 0.65
        {
            self.hover_region.clear();
            self.hover_dist = 0.0;
            self.hover_last_pt = None;
            self.hover_last_raw_pt = None;
        }
    }
}

pub fn parse_descript_txt(text: &str) -> ShellDescript {
    let mut d = ShellDescript::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(',') else {
            continue;
        };
        let key = k.trim().to_ascii_lowercase();
        let val = v.trim();
        match key.as_str() {
            "name" if !val.is_empty() => d.name = val.to_string(),
            "sakura.name" | "char0.name" if !val.is_empty() => d.sakura_name = val.to_string(),
            "kero.name" | "char1.name" if !val.is_empty() => d.kero_name = val.to_string(),
            "sakura.balloon.offsetx" | "char0.balloon.offsetx" => {
                d.sakura_balloon_offsetx = int_safe(Some(val), d.sakura_balloon_offsetx);
            }
            "sakura.balloon.offsety" | "char0.balloon.offsety" => {
                d.sakura_balloon_offsety = int_safe(Some(val), d.sakura_balloon_offsety);
            }
            "kero.balloon.offsetx" | "char1.balloon.offsetx" => {
                d.kero_balloon_offsetx = int_safe(Some(val), d.kero_balloon_offsetx);
            }
            "kero.balloon.offsety" | "char1.balloon.offsety" => {
                d.kero_balloon_offsety = int_safe(Some(val), d.kero_balloon_offsety);
            }
            "sakura.balloon.alignment" | "char0.balloon.alignment" => {
                let low = val.to_ascii_lowercase();
                d.sakura_balloon_alignment = if low == "left" || low == "right" {
                    low
                } else {
                    "none".to_string()
                };
            }
            "kero.balloon.alignment" | "char1.balloon.alignment" => {
                let low = val.to_ascii_lowercase();
                d.kero_balloon_alignment = if low == "left" || low == "right" {
                    low
                } else {
                    "none".to_string()
                };
            }
            "display.scale" | "scale" => {
                if let Ok(sc) = val.parse::<f64>() {
                    if sc.is_finite() && (0.5..=4.0).contains(&sc) {
                        d.display_scale = sc;
                    }
                }
            }
            _ => {}
        }
    }
    d
}

fn expand_surface_header(header: &str) -> (Vec<i32>, bool) {
    let clean = header.trim();
    let is_append = clean.to_ascii_lowercase().starts_with("surface.append");
    let mut ids = BTreeSet::new();
    static HDR_PREFIX_RE: OnceLock<Regex> = OnceLock::new();
    let pfx_re = HDR_PREFIX_RE.get_or_init(|| Regex::new(r"(?i)^surface(?:\.append)?").unwrap());
    static RNG_RE: OnceLock<Regex> = OnceLock::new();
    let rng_re = RNG_RE.get_or_init(|| Regex::new(r"^(\d+)\s*-\s*(\d+)$").unwrap());

    for part in clean.split(',') {
        let mut tok = part.trim();
        if tok.is_empty() {
            continue;
        }
        let excl = tok.starts_with('!');
        if excl {
            tok = tok[1..].trim();
        }
        let cleaned = pfx_re.replace(tok, "");
        let tok_c = cleaned.trim();
        if let Some(caps) = rng_re.captures(tok_c) {
            let a: i32 = caps[1].parse().unwrap_or(0);
            let b: i32 = caps[2].parse().unwrap_or(0);
            let lo = a.min(b);
            let hi = a.max(b).min(8192);
            for x in lo..=hi {
                if excl {
                    ids.remove(&x);
                } else {
                    ids.insert(x);
                }
            }
        } else if let Ok(x) = tok_c.parse::<i32>() {
            if excl {
                ids.remove(&x);
            } else {
                ids.insert(x);
            }
        }
    }
    (ids.into_iter().collect(), is_append)
}

fn upsert_anim(sdef: &mut SurfaceDef, aid: i32) -> &mut SerikoAnimation {
    if let Some(idx) = sdef.animations.iter().position(|a| a.aid == aid) {
        return &mut sdef.animations[idx];
    }
    sdef.animations.push(SerikoAnimation {
        aid,
        interval: "never".to_string(),
        patterns: Vec::new(),
    });
    let idx = sdef.animations.len() - 1;
    &mut sdef.animations[idx]
}

fn upsert_pat(anim: &mut SerikoAnimation, pat: SerikoPattern) {
    if let Some(idx) = anim.patterns.iter().position(|p| p.index == pat.index) {
        anim.patterns[idx] = pat;
    } else {
        anim.patterns.push(pat);
    }
    anim.patterns.sort_by_key(|p| p.index);
}

fn all_ints(s: &str) -> Vec<i32> {
    static INT_RE: OnceLock<Regex> = OnceLock::new();
    let re = INT_RE.get_or_init(|| Regex::new(r"-?\d+").unwrap());
    re.find_iter(s)
        .filter_map(|m| m.as_str().parse::<i32>().ok())
        .collect()
}

fn apply_surface_line(sdef: &mut SurfaceDef, line: &str) {
    let parts: Vec<&str> = line.split(',').map(|p| p.trim()).collect();
    if parts.is_empty() {
        return;
    }
    let key = parts[0].to_ascii_lowercase();

    static COL_RE: OnceLock<Regex> = OnceLock::new();
    static EL_RE: OnceLock<Regex> = OnceLock::new();
    static S1_INT_RE: OnceLock<Regex> = OnceLock::new();
    static S1_PAT_RE: OnceLock<Regex> = OnceLock::new();
    static S2_INT_RE: OnceLock<Regex> = OnceLock::new();
    static S2_PAT_RE: OnceLock<Regex> = OnceLock::new();

    if let Some(c) = COL_RE
        .get_or_init(|| Regex::new(r"^collision(\d+)$").unwrap())
        .captures(&key)
    {
        if parts.len() >= 6 {
            let left = int_safe(Some(parts[1]), 0);
            let top = int_safe(Some(parts[2]), 0);
            let right = int_safe(Some(parts[3]), 0);
            let bottom = int_safe(Some(parts[4]), 0);
            if right > left && bottom > top {
                let cid: i32 = c[1].parse().unwrap_or(0);
                sdef.collisions.retain(|x| x.cid != cid);
                sdef.collisions.push(CollisionRect {
                    cid,
                    left,
                    top,
                    right,
                    bottom,
                    name: parts[5..].join(",").trim().to_string(),
                });
            }
        }
        return;
    }

    if let Some(c) = EL_RE
        .get_or_init(|| Regex::new(r"^element(\d+)$").unwrap())
        .captures(&key)
    {
        if parts.len() >= 3 {
            let eid: i32 = c[1].parse().unwrap_or(0);
            sdef.elements.retain(|x| x.eid != eid);
            sdef.elements.push(SurfaceElement {
                eid,
                method: parts[1].to_ascii_lowercase(),
                filename: parts[2].to_string(),
                x: int_safe(parts.get(3).copied(), 0),
                y: int_safe(parts.get(4).copied(), 0),
            });
        }
        return;
    }

    if let Some(c) = S1_INT_RE
        .get_or_init(|| Regex::new(r"^(\d+)interval$").unwrap())
        .captures(&key)
    {
        if parts.len() >= 2 {
            let aid: i32 = c[1].parse().unwrap_or(0);
            upsert_anim(sdef, aid).interval = parts[1..].join(",").to_ascii_lowercase();
        }
        return;
    }

    if let Some(c) = S1_PAT_RE
        .get_or_init(|| Regex::new(r"^(\d+)patt[eu]rn(\d+)$").unwrap())
        .captures(&key)
    {
        if parts.len() >= 3 {
            let aid: i32 = c[1].parse().unwrap_or(0);
            let pidx: i32 = c[2].parse().unwrap_or(0);
            let method = parts
                .get(3)
                .map(|s| s.to_ascii_lowercase())
                .unwrap_or_else(|| "overlay".to_string());
            let anim = upsert_anim(sdef, aid);
            if matches!(method.as_str(), "alternativestart" | "parallelstart" | "start") {
                let tail = if parts.len() > 4 {
                    parts[4..].join(",")
                } else {
                    parts[1].to_string()
                };
                upsert_pat(
                    anim,
                    SerikoPattern {
                        index: pidx,
                        method: "alternativestart".to_string(),
                        surface_id: -1,
                        delay_ms: 50,
                        offset_x: 0,
                        offset_y: 0,
                        alt_ids: all_ints(&tail),
                    },
                );
            } else {
                upsert_pat(
                    anim,
                    SerikoPattern {
                        index: pidx,
                        method,
                        surface_id: int_safe(Some(parts[1]), -1),
                        delay_ms: (int_safe(Some(parts[2]), 5).max(0) as u64) * 10,
                        offset_x: int_safe(parts.get(4).copied(), 0),
                        offset_y: int_safe(parts.get(5).copied(), 0),
                        alt_ids: Vec::new(),
                    },
                );
            }
        }
        return;
    }

    if let Some(c) = S2_INT_RE
        .get_or_init(|| Regex::new(r"^animation(\d+)\.interval$").unwrap())
        .captures(&key)
    {
        if parts.len() >= 2 {
            let aid: i32 = c[1].parse().unwrap_or(0);
            upsert_anim(sdef, aid).interval = parts[1..].join(",").to_ascii_lowercase();
        }
        return;
    }

    if let Some(c) = S2_PAT_RE
        .get_or_init(|| Regex::new(r"^animation(\d+)\.pattern(\d+)$").unwrap())
        .captures(&key)
    {
        if parts.len() >= 3 {
            let aid: i32 = c[1].parse().unwrap_or(0);
            let pidx: i32 = c[2].parse().unwrap_or(0);
            let method = parts[1].to_ascii_lowercase();
            let anim = upsert_anim(sdef, aid);
            if matches!(method.as_str(), "alternativestart" | "parallelstart" | "start") {
                let tail = parts[2..].join(",");
                upsert_pat(
                    anim,
                    SerikoPattern {
                        index: pidx,
                        method: "alternativestart".to_string(),
                        surface_id: -1,
                        delay_ms: 50,
                        offset_x: 0,
                        offset_y: 0,
                        alt_ids: all_ints(&tail),
                    },
                );
            } else {
                upsert_pat(
                    anim,
                    SerikoPattern {
                        index: pidx,
                        method,
                        surface_id: int_safe(Some(parts[2]), -1),
                        delay_ms: int_safe(parts.get(3).copied(), 50).max(0) as u64,
                        offset_x: int_safe(parts.get(4).copied(), 0),
                        offset_y: int_safe(parts.get(5).copied(), 0),
                        alt_ids: Vec::new(),
                    },
                );
            }
        }
        return;
    }

    if parts.len() >= 2 {
        match key.as_str() {
            "sakura.balloon.offsetx" | "char0.balloon.offsetx" => {
                sdef.sakura_balloon_offsetx = Some(int_safe(Some(parts[1]), 0));
            }
            "sakura.balloon.offsety" | "char0.balloon.offsety" => {
                sdef.sakura_balloon_offsety = Some(int_safe(Some(parts[1]), 0));
            }
            "kero.balloon.offsetx" | "char1.balloon.offsetx" => {
                sdef.kero_balloon_offsetx = Some(int_safe(Some(parts[1]), 0));
            }
            "kero.balloon.offsety" | "char1.balloon.offsety" => {
                sdef.kero_balloon_offsety = Some(int_safe(Some(parts[1]), 0));
            }
            _ => {}
        }
    }
}

pub fn parse_surfaces_txt(
    text: &str,
) -> (
    BTreeMap<i32, SurfaceDef>,
    HashMap<i32, Vec<i32>>,
    HashMap<i32, Vec<i32>>,
) {
    let mut surfaces: BTreeMap<i32, SurfaceDef> = BTreeMap::new();
    let mut sakura_aliases: HashMap<i32, Vec<i32>> = HashMap::new();
    let mut kero_aliases: HashMap<i32, Vec<i32>> = HashMap::new();

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Surface,
        SakuraAlias,
        KeroAlias,
        Skip,
    }

    let mut pending_header = String::new();
    let mut mode: Option<Mode> = None;
    let mut active_ids: Vec<i32> = Vec::new();

    for raw in text.lines() {
        let trimmed = raw.trim();
        let stripped = if trimmed.starts_with("//") { "" } else { trimmed };
        if stripped.is_empty() {
            continue;
        }

        if mode.is_none() {
            if let Some((before, after)) = stripped.split_once('{') {
                let hdr = if before.trim().is_empty() {
                    pending_header.as_str()
                } else {
                    before.trim()
                };
                let hl = hdr.to_ascii_lowercase();
                let next_mode = if hl.starts_with("surface") {
                    let (ids, is_append) = expand_surface_header(hdr);
                    active_ids = ids;
                    if !is_append {
                        for &sid in &active_ids {
                            surfaces.entry(sid).or_insert_with(|| SurfaceDef::new(sid));
                        }
                    }
                    Mode::Surface
                } else if hl == "sakura.surface.alias" {
                    Mode::SakuraAlias
                } else if hl == "kero.surface.alias" {
                    Mode::KeroAlias
                } else {
                    Mode::Skip
                };
                pending_header.clear();
                mode = Some(next_mode);

                let mut rem = after.trim();
                if !rem.is_empty() && rem != "}" {
                    if let Some((b, _)) = rem.split_once('}') {
                        rem = b.trim();
                    }
                    if next_mode == Mode::Surface && !rem.is_empty() {
                        for &sid in &active_ids {
                            let sdef = surfaces.entry(sid).or_insert_with(|| SurfaceDef::new(sid));
                            apply_surface_line(sdef, rem);
                        }
                    }
                }
                if after.contains('}') {
                    mode = None;
                }
            } else {
                pending_header = stripped.to_string();
            }
            continue;
        }

        if let Some((before, _)) = stripped.split_once('}') {
            let b = before.trim();
            if !b.is_empty() && mode == Some(Mode::Surface) {
                for &sid in &active_ids {
                    let sdef = surfaces.entry(sid).or_insert_with(|| SurfaceDef::new(sid));
                    apply_surface_line(sdef, b);
                }
            }
            mode = None;
            continue;
        }

        match mode {
            Some(Mode::Surface) => {
                for &sid in &active_ids {
                    let sdef = surfaces.entry(sid).or_insert_with(|| SurfaceDef::new(sid));
                    apply_surface_line(sdef, stripped);
                }
            }
            Some(Mode::SakuraAlias) | Some(Mode::KeroAlias) => {
                if let Some((k_str, v_str)) = stripped.split_once(',') {
                    if let Ok(src_id) = k_str.trim().parse::<i32>() {
                        let targets = all_ints(v_str);
                        if !targets.is_empty() {
                            if mode == Some(Mode::SakuraAlias) {
                                sakura_aliases.insert(src_id, targets);
                            } else {
                                kero_aliases.insert(src_id, targets);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    (surfaces, sakura_aliases, kero_aliases)
}

#[derive(Debug, Clone)]
pub struct ShellSummary {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ShellManager {
    pub shell_roots: Vec<PathBuf>,
}

impl ShellManager {
    pub fn new(shell_roots: Vec<PathBuf>) -> Self {
        Self { shell_roots }
    }

    pub fn list_shells(&self) -> Vec<ShellSummary> {
        let mut found: BTreeMap<String, PathBuf> = BTreeMap::new();
        for root in self.shell_roots.iter().rev() {
            if !root.is_dir() {
                continue;
            }
            if let Ok(entries) = fs::read_dir(root) {
                let mut dirs: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok().map(|x| x.path()))
                    .filter(|p| p.is_dir())
                    .collect();
                dirs.sort();
                for sub in dirs {
                    let has_desc = sub.join("descript.txt").is_file();
                    let has_surface = fs::read_dir(&sub).ok().is_some_and(|rd| {
                        rd.filter_map(|e| e.ok()).any(|e| {
                            let n = e.file_name().to_string_lossy().to_ascii_lowercase();
                            n.starts_with("surface") && n.ends_with(".png")
                        })
                    });
                    if has_desc || has_surface {
                        if let Some(name) = sub.file_name().map(|s| s.to_string_lossy().to_string()) {
                            found.insert(name, sub);
                        }
                    }
                }
            }
        }

        let mut out: Vec<ShellSummary> = found
            .into_iter()
            .map(|(sid, sdir)| {
                let desc_path = sdir.join("descript.txt");
                let name = if desc_path.is_file() {
                    fs::read(&desc_path)
                        .map(|b| parse_descript_txt(&decode_ukagaka_bytes(&b)).name)
                        .unwrap_or_else(|_| sid.clone())
                } else {
                    sid.clone()
                };
                ShellSummary {
                    id: sid,
                    name,
                    path: sdir,
                }
            })
            .collect();

        out.sort_by_key(|item| {
            let idx = CURATED_SHELL_ORDER
                .iter()
                .position(|&x| x == item.id)
                .unwrap_or(CURATED_SHELL_ORDER.len());
            (idx, item.id.clone())
        });
        out
    }

    pub fn load_shell(&self, shell_id: &str) -> Result<ShellManifest, String> {
        let list = self.list_shells();
        if list.is_empty() {
            return Err(format!("No valid Ukagaka shells found in {:?}", self.shell_roots));
        }
        let chosen = list
            .iter()
            .find(|s| s.id == shell_id)
            .or_else(|| {
                if shell_id == "akari_default" {
                    list.iter().find(|s| s.id == "neo_default")
                } else {
                    None
                }
            })
            .or_else(|| list.iter().find(|s| s.id == "master"))
            .or_else(|| list.iter().find(|s| s.id == "neo_default"))
            .or_else(|| list.iter().find(|s| s.id == "akari_default"))
            .unwrap_or(&list[0]);

        let sdir = chosen.path.clone();
        let desc = fs::read(sdir.join("descript.txt"))
            .map(|b| parse_descript_txt(&decode_ukagaka_bytes(&b)))
            .unwrap_or_default();

        let (mut surfaces, mut sakura_aliases, mut kero_aliases) =
            if let Ok(b) = fs::read(sdir.join("surfaces.txt")) {
                parse_surfaces_txt(&decode_ukagaka_bytes(&b))
            } else {
                (BTreeMap::new(), HashMap::new(), HashMap::new())
            };

        if let Ok(b) = fs::read(sdir.join("alias.txt")) {
            let (_, extra_s, extra_k) = parse_surfaces_txt(&decode_ukagaka_bytes(&b));
            sakura_aliases.extend(extra_s);
            kero_aliases.extend(extra_k);
        }

        static LEGACY_TXT_RE: OnceLock<Regex> = OnceLock::new();
        static PNA_RE: OnceLock<Regex> = OnceLock::new();
        static PNG_RE: OnceLock<Regex> = OnceLock::new();
        let legacy_re = LEGACY_TXT_RE.get_or_init(|| Regex::new(r"(?i)^surface(\d+)[sa]\.txt$").unwrap());
        let pna_re = PNA_RE.get_or_init(|| Regex::new(r"(?i)^surface(\d+)\.pna$").unwrap());
        let png_re = PNG_RE.get_or_init(|| Regex::new(r"(?i)^surface(\d+)\.png$").unwrap());

        let mut dir_files: Vec<PathBuf> = fs::read_dir(&sdir)
            .map(|rd| rd.filter_map(|e| e.ok().map(|x| x.path())).collect())
            .unwrap_or_default();
        dir_files.sort();

        for p in &dir_files {
            if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
                if let Some(c) = legacy_re.captures(fname) {
                    if let Ok(sid) = c[1].parse::<i32>() {
                        if let Ok(b) = fs::read(p) {
                            let sdef = surfaces.entry(sid).or_insert_with(|| SurfaceDef::new(sid));
                            for raw in decode_ukagaka_bytes(&b).lines() {
                                let trimmed = raw.trim();
                                if !trimmed.is_empty() && !trimmed.starts_with("//") {
                                    apply_surface_line(sdef, trimmed);
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut pna_map: HashMap<i32, PathBuf> = HashMap::new();
        for p in &dir_files {
            if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
                if let Some(c) = pna_re.captures(fname) {
                    if let Ok(sid) = c[1].parse::<i32>() {
                        pna_map.insert(sid, p.clone());
                    }
                }
            }
        }

        for p in &dir_files {
            if let Some(fname) = p.file_name().and_then(|s| s.to_str()) {
                if let Some(c) = png_re.captures(fname) {
                    if let Ok(sid) = c[1].parse::<i32>() {
                        let sdef = surfaces.entry(sid).or_insert_with(|| SurfaceDef::new(sid));
                        sdef.image_path = Some(p.clone());
                        if let Some(pna) = pna_map.get(&sid) {
                            sdef.pna_path = Some(pna.clone());
                        }
                    }
                }
            }
        }

        Ok(ShellManifest {
            shell_id: chosen.id.clone(),
            dir_path: sdir,
            descript: desc,
            surfaces,
            sakura_aliases,
            kero_aliases,
            size_cache: HashMap::new(),
            overlay_ids_cache: None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct BalloonSurfaceMetrics {
    pub origin_x: i32,
    pub origin_y: i32,
    pub wordwrappoint_x: i32,
    pub validrect_bottom: i32,
}

impl Default for BalloonSurfaceMetrics {
    fn default() -> Self {
        Self {
            origin_x: 14,
            origin_y: 14,
            wordwrappoint_x: -16,
            validrect_bottom: -14,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BalloonSkin {
    pub balloon_id: String,
    pub name: String,
    pub dir_path: PathBuf,
    pub font_height: u32,
    pub font_rgb: (u8, u8, u8),
    pub anchor_rgb: (u8, u8, u8),
    pub cursor_rgb: (u8, u8, u8),
    pub sstp_msg_rgb: (u8, u8, u8),
    pub sstp_marker_x: i32,
    pub sstp_marker_y: i32,
    pub sstp_msg_x: i32,
    pub sstp_msg_y: i32,
    pub comm_x: i32,
    pub comm_y: i32,
    pub comm_w: i32,
    pub comm_h: i32,
    pub display_scale: f64,
    pub base_metrics: BalloonSurfaceMetrics,
    pub per_image_metrics: HashMap<String, BalloonSurfaceMetrics>,
}

impl BalloonSkin {
    pub fn load_frame_rgba(&self, scope: Scope, side: &str, large: bool) -> (RgbaBitmap, BalloonSurfaceMetrics) {
        let prefix = if scope == Scope::Kero { "balloonk" } else { "balloons" };
        let idx = (if large { 2 } else { 0 }) + (if side == "right" { 1 } else { 0 });
        let mut stem = format!("{prefix}{idx}");
        let mut cand = self.dir_path.join(format!("{stem}.png"));
        if !cand.is_file() {
            stem = format!("{prefix}{}", if side == "right" { 1 } else { 0 });
            cand = self.dir_path.join(format!("{stem}.png"));
        }
        let pna = cand.with_extension("pna");
        let rgba = load_ukagaka_rgba(&cand, if pna.is_file() { Some(&pna) } else { None })
            .unwrap_or_else(|| RgbaBitmap::from_solid(300, 150, 248, 248, 252, 240));
        let metrics = self
            .per_image_metrics
            .get(&stem)
            .cloned()
            .unwrap_or_else(|| self.base_metrics.clone());
        (rgba, metrics)
    }

    pub fn load_communicate_rgba(&self) -> Option<RgbaBitmap> {
        let cand = self.dir_path.join("balloonc0.png");
        if cand.is_file() {
            let pna = cand.with_extension("pna");
            return load_ukagaka_rgba(&cand, if pna.is_file() { Some(&pna) } else { None });
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct BalloonSummary {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct BalloonManager {
    pub balloon_roots: Vec<PathBuf>,
}

impl BalloonManager {
    pub fn new(balloon_roots: Vec<PathBuf>) -> Self {
        Self { balloon_roots }
    }

    pub fn list_balloons(&self) -> Vec<BalloonSummary> {
        let mut found: BTreeMap<String, PathBuf> = BTreeMap::new();
        for root in self.balloon_roots.iter().rev() {
            if !root.is_dir() {
                continue;
            }
            if let Ok(entries) = fs::read_dir(root) {
                let mut dirs: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok().map(|x| x.path()))
                    .filter(|p| p.is_dir() && p.join("balloons0.png").is_file())
                    .collect();
                dirs.sort();
                for sub in dirs {
                    if let Some(name) = sub.file_name().map(|s| s.to_string_lossy().to_string()) {
                        found.insert(name, sub);
                    }
                }
            }
        }

        let mut out: Vec<BalloonSummary> = found
            .into_iter()
            .map(|(bid, bdir)| {
                let mut name = bid.clone();
                if let Ok(b) = fs::read(bdir.join("descript.txt")) {
                    for ln in decode_ukagaka_bytes(&b).lines() {
                        if let Some((k, v)) = ln.split_once(',') {
                            if k.trim().eq_ignore_ascii_case("name") && !v.trim().is_empty() {
                                name = v.trim().to_string();
                                break;
                            }
                        }
                    }
                }
                BalloonSummary {
                    id: bid,
                    name,
                    path: bdir,
                }
            })
            .collect();

        out.sort_by_key(|item| {
            let idx = CURATED_BALLOON_ORDER
                .iter()
                .position(|&x| x == item.id)
                .unwrap_or(CURATED_BALLOON_ORDER.len());
            (idx, item.id.clone())
        });
        out
    }

    pub fn load_balloon(&self, balloon_id: &str) -> Result<BalloonSkin, String> {
        let list = self.list_balloons();
        if list.is_empty() {
            return Err(format!("No valid balloons found in {:?}", self.balloon_roots));
        }
        let chosen = list
            .iter()
            .find(|b| b.id == balloon_id)
            .or_else(|| list.iter().find(|b| b.id == "twin tail"))
            .or_else(|| list.iter().find(|b| b.id == "clean_default"))
            .unwrap_or(&list[0]);

        let bdir = chosen.path.clone();
        let mut skin = BalloonSkin {
            balloon_id: chosen.id.clone(),
            name: chosen.id.clone(),
            dir_path: bdir.clone(),
            font_height: 12,
            font_rgb: (212, 213, 216),
            anchor_rgb: (0, 192, 0),
            cursor_rgb: (194, 197, 192),
            sstp_msg_rgb: (166, 166, 170),
            sstp_marker_x: 16,
            sstp_marker_y: -24,
            sstp_msg_x: 28,
            sstp_msg_y: -25,
            comm_x: 13,
            comm_y: 14,
            comm_w: 320,
            comm_h: 20,
            display_scale: 1.0,
            base_metrics: BalloonSurfaceMetrics::default(),
            per_image_metrics: HashMap::new(),
        };

        let (mut fr, mut fg, mut fb) = skin.font_rgb;
        let (mut ar, mut ag, mut ab) = skin.anchor_rgb;
        let (mut cr, mut cg, mut cb) = skin.cursor_rgb;
        let (mut sr, mut sg, mut sb) = skin.sstp_msg_rgb;

        if let Ok(b) = fs::read(bdir.join("descript.txt")) {
            for raw in decode_ukagaka_bytes(&b).lines() {
                let ln = raw.trim();
                if ln.is_empty() || ln.starts_with("//") {
                    continue;
                }
                let Some((k, v)) = ln.split_once(',') else {
                    continue;
                };
                let kl = k.trim().to_ascii_lowercase();
                let val = v.trim();
                match kl.as_str() {
                    "name" if !val.is_empty() => skin.name = val.to_string(),
                    "origin.x" => skin.base_metrics.origin_x = int_safe(Some(val), 14),
                    "origin.y" => skin.base_metrics.origin_y = int_safe(Some(val), 14),
                    "wordwrappoint.x" => skin.base_metrics.wordwrappoint_x = int_safe(Some(val), -16),
                    "validrect.bottom" => skin.base_metrics.validrect_bottom = int_safe(Some(val), -14),
                    "font.height" => skin.font_height = int_safe(Some(val), 12).max(10) as u32,
                    "font.color.r" => fr = int_safe(Some(val), fr as i32).clamp(0, 255) as u8,
                    "font.color.g" => fg = int_safe(Some(val), fg as i32).clamp(0, 255) as u8,
                    "font.color.b" => fb = int_safe(Some(val), fb as i32).clamp(0, 255) as u8,
                    "anchor.font.color.r" => ar = int_safe(Some(val), ar as i32).clamp(0, 255) as u8,
                    "anchor.font.color.g" => ag = int_safe(Some(val), ag as i32).clamp(0, 255) as u8,
                    "anchor.font.color.b" => ab = int_safe(Some(val), ab as i32).clamp(0, 255) as u8,
                    "cursor.font.color.r" => cr = int_safe(Some(val), cr as i32).clamp(0, 255) as u8,
                    "cursor.font.color.g" => cg = int_safe(Some(val), cg as i32).clamp(0, 255) as u8,
                    "cursor.font.color.b" => cb = int_safe(Some(val), cb as i32).clamp(0, 255) as u8,
                    "sstpmessage.font.color.r" => sr = int_safe(Some(val), sr as i32).clamp(0, 255) as u8,
                    "sstpmessage.font.color.g" => sg = int_safe(Some(val), sg as i32).clamp(0, 255) as u8,
                    "sstpmessage.font.color.b" => sb = int_safe(Some(val), sb as i32).clamp(0, 255) as u8,
                    "sstpmarker.x" => skin.sstp_marker_x = int_safe(Some(val), skin.sstp_marker_x),
                    "sstpmarker.y" => skin.sstp_marker_y = int_safe(Some(val), skin.sstp_marker_y),
                    "sstpmessage.x" => skin.sstp_msg_x = int_safe(Some(val), skin.sstp_msg_x),
                    "sstpmessage.y" => skin.sstp_msg_y = int_safe(Some(val), skin.sstp_msg_y),
                    "communicatebox.x" => skin.comm_x = int_safe(Some(val), skin.comm_x),
                    "communicatebox.y" => skin.comm_y = int_safe(Some(val), skin.comm_y),
                    "communicatebox.width" => skin.comm_w = int_safe(Some(val), skin.comm_w),
                    "communicatebox.height" => skin.comm_h = int_safe(Some(val), skin.comm_h),
                    "display.scale" | "scale" => {
                        if let Ok(sc) = val.parse::<f64>() {
                            if sc.is_finite() && (0.5..=4.0).contains(&sc) {
                                skin.display_scale = sc;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        skin.font_rgb = (fr, fg, fb);
        skin.anchor_rgb = (ar, ag, ab);
        skin.cursor_rgb = (cr, cg, cb);
        skin.sstp_msg_rgb = (sr, sg, sb);

        if let Ok(entries) = fs::read_dir(&bdir) {
            for ent in entries.filter_map(|e| e.ok()) {
                let p = ent.path();
                let fname = ent.file_name().to_string_lossy().to_ascii_lowercase();
                if fname.starts_with("balloon") && fname.ends_with("s.txt") {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        let target_stem = stem.strip_suffix('s').unwrap_or(stem).to_string();
                        let mut m = skin.base_metrics.clone();
                        if let Ok(b) = fs::read(&p) {
                            for raw in decode_ukagaka_bytes(&b).lines() {
                                let ln = raw.trim();
                                if let Some((k, v)) = ln.split_once(',') {
                                    let val = v.trim();
                                    match k.trim().to_ascii_lowercase().as_str() {
                                        "origin.x" => m.origin_x = int_safe(Some(val), m.origin_x),
                                        "origin.y" => m.origin_y = int_safe(Some(val), m.origin_y),
                                        "wordwrappoint.x" => m.wordwrappoint_x = int_safe(Some(val), m.wordwrappoint_x),
                                        "validrect.bottom" => m.validrect_bottom = int_safe(Some(val), m.validrect_bottom),
                                        _ => {}
                                    }
                                }
                            }
                        }
                        skin.per_image_metrics.insert(target_stem, m);
                    }
                }
            }
        }

        Ok(skin)
    }
}

fn parse_kv_txt(text: &str) -> HashMap<String, String> {
    let mut kv = HashMap::new();
    for raw in text.lines() {
        let ln = raw.trim();
        if ln.is_empty() || ln.starts_with("//") || ln.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = ln.split_once(',') {
            kv.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    kv
}

fn safe_folder_id(raw_name: &str, fallback: &str) -> String {
    let cleaned: String = raw_name
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c| c == '.' || c == ' ');
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_safe_rel_path(rel: &str) -> bool {
    let p = Path::new(rel);
    !rel.is_empty()
        && !rel.starts_with('/')
        && p.components().all(|c| match c {
            Component::Normal(s) => !s.to_string_lossy().starts_with('.'),
            _ => false,
        })
}

#[derive(Debug, Clone)]
pub struct InstallNarResult {
    pub shells: Vec<String>,
    pub balloons: Vec<String>,
}

pub fn install_nar_archive(
    archive_path: &Path,
    shells_dest: &Path,
    balloons_dest: &Path,
) -> Result<InstallNarResult, String> {
    let file = fs::File::open(archive_path).map_err(|e| format!("Failed to open archive: {e}"))?;
    let mut zf = ZipArchive::new(file).map_err(|e| format!("Invalid ZIP/NAR archive: {e}"))?;

    let mut entries: Vec<(usize, String)> = Vec::new();
    let mut install_kv: HashMap<String, String> = HashMap::new();
    let mut ghost_kv: HashMap<String, String> = HashMap::new();

    for i in 0..zf.len() {
        let mut entry = zf.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let norm = entry
            .name()
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_string();
        if !is_safe_rel_path(&norm) {
            continue;
        }
        let nl = norm.to_ascii_lowercase();
        if nl == "install.txt" {
            let mut buf = Vec::new();
            let _ = entry.read_to_end(&mut buf);
            install_kv = parse_kv_txt(&decode_ukagaka_bytes(&buf));
        } else if nl == "ghost/master/descript.txt" {
            let mut buf = Vec::new();
            let _ = entry.read_to_end(&mut buf);
            ghost_kv = parse_kv_txt(&decode_ukagaka_bytes(&buf));
        }
        entries.push((i, norm));
    }

    static SURF_PNG_RE: OnceLock<Regex> = OnceLock::new();
    let surf_re = SURF_PNG_RE.get_or_init(|| Regex::new(r"(?i)^surface\d+\.png$").unwrap());

    let mut shell_dirs = BTreeSet::new();
    let mut balloon_dirs = BTreeSet::new();
    for (_, norm) in &entries {
        let (parent, fname) = match norm.rsplit_once('/') {
            Some((p, f)) => (p, f),
            None => ("", norm.as_str()),
        };
        if surf_re.is_match(fname) {
            shell_dirs.insert(parent.to_string());
        } else if fname.eq_ignore_ascii_case("balloons0.png") {
            balloon_dirs.insert(parent.to_string());
        }
    }

    let arch_stem = archive_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("custom_skin");

    let mut installed_shells = Vec::new();
    let mut installed_balloons = Vec::new();

    let extract_subtree = |zf: &mut ZipArchive<fs::File>,
                           prefix: &str,
                           dest_dir: &Path|
     -> Result<(), String> {
        let pfx = if prefix.is_empty() {
            String::new()
        } else {
            format!("{prefix}/")
        };
        let parent = dest_dir.parent().ok_or("Invalid dest dir")?;
        let tmp_dir = parent.join(format!(
            ".tmp_import_{}",
            dest_dir.file_name().unwrap_or_default().to_string_lossy()
        ));
        if tmp_dir.exists() {
            let _ = fs::remove_dir_all(&tmp_dir);
        }
        fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;

        for &(idx, ref norm) in &entries {
            if !pfx.is_empty() && !norm.starts_with(&pfx) {
                continue;
            }
            let rel = if pfx.is_empty() {
                norm.as_str()
            } else {
                &norm[pfx.len()..]
            };
            if !is_safe_rel_path(rel) {
                continue;
            }
            let fname = Path::new(rel)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if fname.eq_ignore_ascii_case("install.txt") || fname.eq_ignore_ascii_case("delete.txt") {
                continue;
            }
            let target = tmp_dir.join(rel);
            if let Some(tp) = target.parent() {
                fs::create_dir_all(tp).map_err(|e| e.to_string())?;
            }
            let mut zentry = zf.by_index(idx).map_err(|e| e.to_string())?;
            let mut buf = Vec::new();
            zentry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
            fs::write(&target, &buf).map_err(|e| e.to_string())?;
        }

        if dest_dir.exists() {
            let _ = fs::remove_dir_all(dest_dir);
        }
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        fs::rename(&tmp_dir, dest_dir).map_err(|e| e.to_string())?;
        Ok(())
    };

    for spfx in shell_dirs {
        let parts: Vec<&str> = spfx.split('/').filter(|p| !p.is_empty()).collect();
        let sid = if parts.is_empty() || spfx.eq_ignore_ascii_case("shell/master") {
            safe_folder_id(
                install_kv.get("directory").map(String::as_str).unwrap_or(arch_stem),
                arch_stem,
            )
        } else {
            safe_folder_id(parts.last().copied().unwrap_or(arch_stem), arch_stem)
        };
        let dst_dir = shells_dest.join(&sid);
        extract_subtree(&mut zf, &spfx, &dst_dir)?;

        let desc_file = dst_dir.join("descript.txt");
        if !ghost_kv.is_empty() && desc_file.is_file() {
            if let Ok(b) = fs::read(&desc_file) {
                let raw_desc = decode_ukagaka_bytes(&b);
                let d_kv = parse_kv_txt(&raw_desc);
                let mut additions = Vec::new();
                if !d_kv.contains_key("sakura.name") {
                    if let Some(v) = ghost_kv.get("sakura.name") {
                        additions.push(format!("sakura.name,{v}"));
                    }
                }
                if !d_kv.contains_key("kero.name") {
                    if let Some(v) = ghost_kv.get("kero.name") {
                        additions.push(format!("kero.name,{v}"));
                    }
                }
                if !additions.is_empty() {
                    let mut lines: Vec<String> = vec!["charset,UTF-8".to_string()];
                    lines.extend(
                        raw_desc
                            .lines()
                            .filter(|ln| !ln.trim().to_ascii_lowercase().starts_with("charset,"))
                            .map(|s| s.to_string()),
                    );
                    lines.extend(additions);
                    let _ = fs::write(&desc_file, lines.join("\n") + "\n");
                }
            }
        }
        installed_shells.push(sid);
    }

    for bpfx in balloon_dirs {
        let parts: Vec<&str> = bpfx.split('/').filter(|p| !p.is_empty()).collect();
        let fallback_bid = format!("{arch_stem}_balloon");
        let bid = if parts.is_empty()
            || bpfx.eq_ignore_ascii_case("balloon")
            || bpfx.eq_ignore_ascii_case("balloon/master")
        {
            safe_folder_id(
                install_kv
                    .get("balloon.directory")
                    .map(String::as_str)
                    .unwrap_or(&fallback_bid),
                &fallback_bid,
            )
        } else {
            safe_folder_id(parts.last().copied().unwrap_or(&fallback_bid), &fallback_bid)
        };
        let dst_dir = balloons_dest.join(&bid);
        extract_subtree(&mut zf, &bpfx, &dst_dir)?;
        installed_balloons.push(bid);
    }

    Ok(InstallNarResult {
        shells: installed_shells,
        balloons: installed_balloons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn bundled_cc0_assets_load_and_isolate_scopes() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        let mgr = ShellManager::new(vec![root.join("shells")]);
        let mut manifest = mgr.load_shell("neo_default").expect("load neo_default");
        assert!((manifest.descript.display_scale - 2.0).abs() < 0.01);
        let s0 = manifest.compose_base_rgba(0).expect("surface 0");
        let s10 = manifest.compose_base_rgba(10).expect("surface 10");
        assert!(s0.count_opaque_pixels() > 20000);
        assert!(s10.count_opaque_pixels() > 8000);

        // Uncut-feet guard: bottom 2 transparent padding rows must be empty
        for bmp in [&s0, &s10] {
            let bottom_opaque: usize = (bmp.height.saturating_sub(2)..bmp.height)
                .map(|y| (0..bmp.width).filter(|&x| bmp.is_opaque_at(x as i32, y as i32)).count())
                .sum();
            assert_eq!(bottom_opaque, 0, "bottom transparent padding must remain free of cropped soles");
        }

        // Unyu periwinkle body color guard: vivid blue channel dominant over red (never bleached white)
        let mut blue_cnt: u64 = 0;
        let mut sum_r: u64 = 0;
        let mut sum_b: u64 = 0;
        for px in s10.data.chunks_exact(4) {
            if px[3] > 220 && (px[2] as i32) > (px[0] as i32) + 40 {
                blue_cnt += 1;
                sum_r += px[0] as u64;
                sum_b += px[2] as u64;
            }
        }
        assert!(blue_cnt > 15_000, "unyu interior periwinkle pixels too few: {blue_cnt}");
        let mean_r = (sum_r as f64) / (blue_cnt as f64);
        let mean_b = (sum_b as f64) / (blue_cnt as f64);
        assert!(mean_b >= 225.0, "unyu blue channel faded: mean_b={mean_b:.1}");
        assert!(mean_b - mean_r >= 70.0, "unyu washed out toward white: mean_r={mean_r:.1}, mean_b={mean_b:.1}");
        assert!(mean_r <= 175.0, "unyu red channel too bright/white: mean_r={mean_r:.1}");

        // Lip-sync mouth overlays 102/112 exist & stay excluded from pose lists
        let s102 = manifest.compose_base_rgba(102).expect("surface 102");
        let s112 = manifest.compose_base_rgba(112).expect("surface 112");
        assert!(s102.count_opaque_pixels() > 50, "surface 102 mouth overlay empty");
        assert!(s112.count_opaque_pixels() > 50, "surface 112 mouth overlay empty");

        // Strict scope isolation check: kero must NEVER resolve to 0..=9 or blink/talk overlays
        let k_ids = manifest.valid_surface_ids(Scope::Kero, 40);
        assert!(k_ids.contains(&10));
        assert!(k_ids.iter().all(|&id| id >= 10 && id < 100));
        let s_ids = manifest.valid_surface_ids(Scope::Sakura, 120);
        assert!(!s_ids.contains(&102) && !s_ids.contains(&112), "mouth overlays leaked into Sakura poses: {s_ids:?}");
        let k_ids_wide = manifest.valid_surface_ids(Scope::Kero, 120);
        assert!(!k_ids_wide.contains(&102) && !k_ids_wide.contains(&112), "mouth overlays leaked into Kero poses: {k_ids_wide:?}");
        assert_eq!(manifest.resolve_surface_id(Scope::Kero, 0), 10);
        assert_eq!(manifest.resolve_surface_id(Scope::Kero, 5), 12);
        assert_eq!(manifest.resolve_surface_id(Scope::Kero, 1), 11);
        assert_eq!(manifest.resolve_surface_id(Scope::Sakura, 10), 0);

        let bmgr = BalloonManager::new(vec![root.join("balloons")]);
        let bskin = bmgr.load_balloon("clean_default").expect("load clean_default");
        assert!((bskin.display_scale - 2.0).abs() < 0.01);
        let (frame, _metrics) = bskin.load_frame_rgba(Scope::Sakura, "left", false);
        assert!(frame.count_opaque_pixels() > 40000);
    }

    #[test]
    fn wobble_tolerant_stroke_detector_triggers_cleanly() {
        let mut det = StrokeDetector::new();
        let head = CollisionRect {
            cid: 0,
            left: 50,
            top: 20,
            right: 170,
            bottom: 100,
            name: "Head".to_string(),
        };
        let t0 = Instant::now();
        assert_eq!(det.update_at((80.0, 60.0), Some(&head), t0), None);
        // Simulate 1-frame dip outside Head within 0.65s wobble grace window
        assert_eq!(
            det.update_at((120.0, 65.0), None, t0 + Duration::from_millis(50)),
            None
        );
        assert_eq!(
            det.update_at((150.0, 60.0), Some(&head), t0 + Duration::from_millis(100)),
            None
        );
        assert_eq!(
            det.update_at((70.0, 62.0), Some(&head), t0 + Duration::from_millis(150)),
            None
        );
        let triggered = det.update_at((155.0, 58.0), Some(&head), t0 + Duration::from_millis(200));
        assert_eq!(triggered.as_deref(), Some("Head"));
    }

    #[test]
    fn local_classic_shells_isolate_unyu_if_installed() {
        let user_shells = crate::config_store::default_user_data_dir().join("shells");
        if !user_shells.join("master").is_dir() {
            return;
        }
        let mgr = ShellManager::new(vec![user_shells]);
        let mut master = mgr.load_shell("master").expect("load master");
        assert!(master.is_full_pose_for_scope(Scope::Sakura, 0, false));
        assert!(!master.is_full_pose_for_scope(Scope::Kero, 0, false));
        assert_eq!(master.resolve_surface_id(Scope::Kero, 0), 10);
        let u_bmp = master.compose_base_rgba(10).expect("unyu surface10");
        assert!(u_bmp.count_opaque_pixels() > 1500);
        // Top of Unyu head (around (72, 165) in master surface10) should hit synthesized Head
        let (bx0, by0, bx1, _) = u_bmp.opaque_bbox().expect("bbox");
        let mid_x = (bx0 + bx1) / 2;
        let top_y = (by0..by0 + 60)
            .find(|&y| u_bmp.is_opaque_at(mid_x, y) || u_bmp.is_opaque_at(bx0 + 30, y))
            .unwrap_or(by0 + 10);
        let probe_x = if u_bmp.is_opaque_at(mid_x, top_y) { mid_x } else { bx0 + 30 };
        let hit = master.hit_collision(10, Scope::Kero, &u_bmp, probe_x, top_y);
        assert_eq!(hit.map(|c| c.name).as_deref(), Some("Head"));
    }
}
