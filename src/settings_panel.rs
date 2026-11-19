use crate::config_store::AgentConfig;
use crate::gui::TextRenderer;
use crate::i18n::{format_seconds, ui_strings, UiLang};
use crate::shell_parser::RgbaBitmap;

pub const SETTINGS_LOGICAL_W: u32 = 540;
pub const SETTINGS_LOGICAL_H: u32 = 476;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SliderKind {
    SkinOpacity,
    BalloonOpacity,
    CharacterScale,
    AutoTalkInterval,
    InterestWeight,
}

impl SliderKind {
    pub fn bounds(self) -> (f32, f32) {
        match self {
            SliderKind::SkinOpacity => (0.15, 1.00),
            SliderKind::BalloonOpacity => (0.25, 1.00),
            SliderKind::CharacterScale => (0.50, 1.50),
            SliderKind::AutoTalkInterval => (0.0, 600.0),
            SliderKind::InterestWeight => (0.0, 0.80),
        }
    }

    pub fn quantize_from_fraction(self, frac: f32) -> f32 {
        match self {
            SliderKind::AutoTalkInterval => {
                if frac <= 0.04 {
                    0.0
                } else {
                    (((30.0 + 570.0 * ((frac - 0.04) / 0.96).clamp(0.0, 1.0)) / 10.0).round()
                        * 10.0)
                        .clamp(30.0, 600.0)
                }
            }
            SliderKind::InterestWeight => {
                let raw = 0.80 * frac.clamp(0.0, 1.0);
                (raw * 20.0).round() / 20.0
            }
            _ => {
                let (min_v, max_v) = self.bounds();
                let raw = min_v + (max_v - min_v) * frac.clamp(0.0, 1.0);
                (raw * 100.0).round() / 100.0
            }
        }
    }

    pub fn fraction_of(self, value: f32) -> f32 {
        match self {
            SliderKind::AutoTalkInterval => {
                if value < 30.0 {
                    0.0
                } else {
                    (0.04 + 0.96 * ((value.clamp(30.0, 600.0) - 30.0) / 570.0)).clamp(0.04, 1.0)
                }
            }
            SliderKind::InterestWeight => (value / 0.80).clamp(0.0, 1.0),
            _ => {
                let (min_v, max_v) = self.bounds();
                ((value - min_v) / (max_v - min_v)).clamp(0.0, 1.0)
            }
        }
    }

    pub fn action(self, value: f32) -> SettingsAction {
        match self {
            SliderKind::SkinOpacity => SettingsAction::SetSkinOpacity(value),
            SliderKind::BalloonOpacity => SettingsAction::SetBalloonOpacity(value),
            SliderKind::CharacterScale => SettingsAction::SetCharacterScale(value),
            SliderKind::AutoTalkInterval => {
                SettingsAction::SetAutoTalkInterval(if value < 30.0 {
                    0
                } else {
                    (value.round() as u64).clamp(30, 1800)
                })
            }
            SliderKind::InterestWeight => SettingsAction::SetInterestWeight(value),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    Close,
    SelectTab(usize),
    SetSkinOpacity(f32),
    SetBalloonOpacity(f32),
    SetCharacterScale(f32),
    ToggleOpaqueOnHover,
    ToggleLive2dMotion,
    ToggleEyeTracking,
    ToggleKero,
    CycleShell(i32),
    CycleBalloon(i32),
    ToggleAlwaysOnTop,
    ToggleClickThrough,
    ToggleLockPosition,
    ToggleSnapToBottom,
    ToggleHourlyChime,
    CycleUiLanguage(i32),
    ToggleQuietMode,
    ToggleAutoHideFullscreen,
    ToggleHideCharacters,
    MoveNextMonitor,
    CyclePomodoroPreset(i32),
    TogglePomodoro,
    StopPomodoro,
    ShowBacklog,
    CopyLastReply,
    ToggleAutoStart,
    CyclePowerSaveMode(i32),
    ToggleLowBatteryAlert,
    ToggleMacosAcceptFirstMouse,
    ToggleMacosShadow,
    ToggleMacosDock,
    ToggleWindowsSkipTaskbar,
    ToggleWindowsShadow,
    ToggleLinuxPreferX11,
    ToggleLinuxWorkarea,
    ToggleVoiceTts,
    CycleTtsEngine(i32),
    CycleSakuraVoice(i32),
    CycleKeroVoice(i32),
    CycleKeroArchetype(i32),
    CycleProvider(i32),
    SetupLocalAi,
    ToggleEnableTools,
    ToggleEnableClipboard,
    CycleAutoTalkInterval(i32),
    SetAutoTalkInterval(u64),
    SetInterestWeight(f32),
    ClearInterests,
    OpenConfigFolder,
    ClearHistory,
    ResetPositions,
    ResetDefaults,
}

#[derive(Clone, Debug)]
pub struct SettingsPanelData {
    pub cfg: AgentConfig,
    pub config_path_display: String,
    pub active_shell_name: String,
    pub active_balloon_name: String,
    pub provider_label: String,
    pub tts_engine_label: String,
    pub sakura_voice_label: String,
    pub kero_voice_label: String,
    pub kero_archetype_label: String,
    pub recommended_local_ai_hint: String,
    pub local_ai_setup_busy: bool,
    pub interests_summary: String,
    pub ui_lang: UiLang,
    pub ui_language_label: String,
    pub characters_hidden: bool,
    pub pomodoro_preset_label: String,
    pub pomodoro_button_label: String,
    pub power_save_label: String,
}

#[derive(Clone, Debug)]
pub enum SettingsHitKind {
    Action(SettingsAction),
    Slider {
        kind: SliderKind,
        track_x0: f64,
        track_x1: f64,
    },
}

#[derive(Clone, Debug)]
pub struct SettingsHitTarget {
    pub key: &'static str,
    pub rect: (f64, f64, f64, f64),
    pub kind: SettingsHitKind,
}

#[derive(Debug)]
pub struct SettingsPanelModel {
    pub active_tab: usize,
    pub scroll_px: f32,
    pub max_scroll_px: f32,
    pub hovered_key: Option<&'static str>,
    pub dragging_slider: Option<(SliderKind, f64, f64)>,
    pub hit_targets: Vec<SettingsHitTarget>,
    pub header_drag_rect: (f64, f64, f64, f64),
    pub vp_rect: (f64, f64, f64, f64),
}

impl Default for SettingsPanelModel {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsPanelModel {
    pub fn new() -> Self {
        Self {
            active_tab: 0,
            scroll_px: 0.0,
            max_scroll_px: 0.0,
            hovered_key: None,
            dragging_slider: None,
            hit_targets: Vec::new(),
            header_drag_rect: (8.0, 6.0, 492.0, 48.0),
            vp_rect: (14.0, 92.0, 526.0, 422.0),
        }
    }

    pub fn select_tab(&mut self, tab: usize) {
        self.active_tab = tab.min(3);
        self.scroll_px = 0.0;
        self.dragging_slider = None;
        self.hovered_key = None;
    }

    pub fn scroll_by(&mut self, dy: f32) -> bool {
        if self.max_scroll_px <= 0.5 {
            if self.scroll_px != 0.0 {
                self.scroll_px = 0.0;
                return true;
            }
            return false;
        }
        let next = (self.scroll_px - dy).clamp(0.0, self.max_scroll_px);
        let changed = (next - self.scroll_px).abs() > 0.25;
        self.scroll_px = next;
        changed
    }

    pub fn target_center(&self, key: &str) -> Option<(f64, f64)> {
        self.hit_targets
            .iter()
            .find(|t| t.key == key)
            .map(|t| ((t.rect.0 + t.rect.2) * 0.5, (t.rect.1 + t.rect.3) * 0.5))
    }

    pub fn slider_point(&self, kind: SliderKind, frac: f32) -> Option<(f64, f64)> {
        self.hit_targets.iter().find_map(|t| match t.kind {
            SettingsHitKind::Slider {
                kind: k,
                track_x0,
                track_x1,
            } if k == kind => {
                let f = frac.clamp(0.0, 1.0) as f64;
                let x = track_x0 + (track_x1 - track_x0) * f;
                let y = (t.rect.1 + t.rect.3) * 0.5;
                Some((x, y))
            }
            _ => None,
        })
    }

    pub fn handle_pointer_move(&mut self, local: (f64, f64)) -> (bool, Option<SettingsAction>) {
        if let Some((kind, x0, x1)) = self.dragging_slider {
            let span = (x1 - x0).max(1.0);
            let frac = ((local.0 - x0) / span) as f32;
            let val = kind.quantize_from_fraction(frac);
            return (true, Some(kind.action(val)));
        }
        let new_hover = self
            .hit_targets
            .iter()
            .rev()
            .find(|t| {
                local.0 >= t.rect.0
                    && local.0 <= t.rect.2
                    && local.1 >= t.rect.1
                    && local.1 <= t.rect.3
            })
            .map(|t| t.key);
        let changed = self.hovered_key != new_hover;
        self.hovered_key = new_hover;
        (changed, None)
    }

    pub fn handle_mouse_press(
        &mut self,
        local: (f64, f64),
    ) -> (bool, bool, Option<SettingsAction>) {
        if let Some(hit) = self.hit_targets.iter().rev().find(|t| {
            local.0 >= t.rect.0 && local.0 <= t.rect.2 && local.1 >= t.rect.1 && local.1 <= t.rect.3
        }) {
            match &hit.kind {
                SettingsHitKind::Slider {
                    kind,
                    track_x0,
                    track_x1,
                } => {
                    let span = (track_x1 - track_x0).max(1.0);
                    let frac = ((local.0 - track_x0) / span) as f32;
                    let val = kind.quantize_from_fraction(frac);
                    self.dragging_slider = Some((*kind, *track_x0, *track_x1));
                    return (true, false, Some(kind.action(val)));
                }
                SettingsHitKind::Action(act) => {
                    return (true, false, Some(act.clone()));
                }
            }
        }
        let in_header = local.0 >= self.header_drag_rect.0
            && local.0 <= self.header_drag_rect.2
            && local.1 >= self.header_drag_rect.1
            && local.1 <= self.header_drag_rect.3;
        (false, in_header, None)
    }

    pub fn handle_mouse_release(&mut self) {
        self.dragging_slider = None;
    }

    pub fn render(
        &mut self,
        data: &SettingsPanelData,
        text_renderer: &mut TextRenderer,
        scale: f32,
    ) -> RgbaBitmap {
        let w_log = SETTINGS_LOGICAL_W as f32;
        let h_log = SETTINGS_LOGICAL_H as f32;
        let pw = (w_log * scale).round().max(1.0) as u32;
        let ph = (h_log * scale).round().max(1.0) as u32;
        let mut canvas = RgbaBitmap::new_transparent(pw, ph);

        self.hit_targets.clear();

        // Outer glassmorphism frame
        let pad = 4.0 * scale;
        canvas.draw_aa_rounded_box(
            (pad, pad, pw as f32 - pad, ph as f32 - pad),
            14.0 * scale,
            [16, 24, 46, 246],
            [11, 17, 34, 249],
            1.4 * scale,
            [96, 154, 245, 195],
            [59, 130, 246, 195],
            4.0 * scale,
            1.5 * scale,
            [6, 10, 22, 95],
            70,
        );

        let s = ui_strings(data.ui_lang);

        // Header bar
        text_renderer.draw_inline_text(
            &mut canvas,
            s.settings_title,
            (20.0 * scale).round() as i32,
            (18.5 * scale).round() as i32,
            (18.0 * scale).round() as i32,
            (484.0 * scale).round() as i32,
            14.0 * scale,
            18.0 * scale,
            (238, 246, 255),
        );

        // Close button (top right)
        let cl_rect = (498.0, 14.0, 526.0, 42.0);
        let cl_hov = self.hovered_key == Some("close");
        let cl_cx = 512.0 * scale;
        let cl_cy = 28.0 * scale;
        let (cl_in, cl_out) = if cl_hov {
            ([239, 68, 68, 240], [185, 28, 28, 240])
        } else {
            ([44, 68, 122, 225], [30, 48, 92, 225])
        };
        canvas.draw_aa_circle(cl_cx, cl_cy, 10.5 * scale, cl_in, cl_out);
        let dx = 3.8 * scale;
        canvas.draw_aa_segment(
            cl_cx - dx,
            cl_cy - dx,
            cl_cx + dx,
            cl_cy + dx,
            1.8 * scale,
            [244, 248, 255, 250],
        );
        canvas.draw_aa_segment(
            cl_cx - dx,
            cl_cy + dx,
            cl_cx + dx,
            cl_cy - dx,
            1.8 * scale,
            [244, 248, 255, 250],
        );
        self.hit_targets.push(SettingsHitTarget {
            key: "close",
            rect: cl_rect,
            kind: SettingsHitKind::Action(SettingsAction::Close),
        });

        canvas.fill_rect_alpha(
            (16.0 * scale) as i32,
            (48.0 * scale) as i32,
            ((w_log - 32.0) * scale) as i32,
            (1.0 * scale).max(1.0) as i32,
            90,
            125,
            195,
            110,
        );

        // Tab bar (y=54..84)
        let tab_specs: [(&'static str, &'static str, usize); 4] = [
            ("tab_0", s.tab_appearance, 0),
            ("tab_1", s.tab_os, 1),
            ("tab_2", s.tab_voice, 2),
            ("tab_3", s.tab_ai, 3),
        ];
        let tab_w = (508.0 - 18.0) / 4.0;
        for (i, (key, label, idx)) in tab_specs.iter().enumerate() {
            let x0 = 16.0 + (i as f64) * (tab_w + 6.0);
            let x1 = x0 + tab_w;
            let y0 = 54.0;
            let y1 = 83.0;
            let is_active = self.active_tab == *idx;
            let is_hov = self.hovered_key == Some(*key);
            let (fill_a, fill_b, brd, txt_rgb) = if is_active {
                (
                    [56, 118, 232, 240],
                    [37, 86, 198, 240],
                    [147, 197, 253, 210],
                    (255, 255, 255),
                )
            } else if is_hov {
                (
                    [36, 55, 98, 225],
                    [28, 43, 78, 225],
                    [110, 155, 230, 165],
                    (230, 240, 255),
                )
            } else {
                (
                    [23, 35, 64, 210],
                    [18, 28, 52, 210],
                    [72, 108, 175, 110],
                    (176, 200, 238),
                )
            };
            canvas.draw_aa_rounded_box(
                (
                    x0 as f32 * scale,
                    y0 as f32 * scale,
                    x1 as f32 * scale,
                    y1 as f32 * scale,
                ),
                8.0 * scale,
                fill_a,
                fill_b,
                1.0 * scale,
                brd,
                brd,
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            let txt_w = text_renderer.measure_inline_width(label, 11.8 * scale) / scale;
            let tx = x0 as f32 + (((x1 - x0) as f32 - txt_w) * 0.5).max(6.0);
            text_renderer.draw_inline_text(
                &mut canvas,
                label,
                (tx * scale).round() as i32,
                ((y0 as f32 + 7.5) * scale).round() as i32,
                ((x0 as f32 + 4.0) * scale).round() as i32,
                ((x1 as f32 - 4.0) * scale).round() as i32,
                11.8 * scale,
                15.5 * scale,
                txt_rgb,
            );
            self.hit_targets.push(SettingsHitTarget {
                key,
                rect: (x0, y0, x1, y1),
                kind: SettingsHitKind::Action(SettingsAction::SelectTab(*idx)),
            });
        }

        // Scrollable Content Viewport -> render into clipped sub-bitmap
        let (vp_x0, vp_y0, vp_x1, vp_y1) = self.vp_rect;
        let vp_w_log = (vp_x1 - vp_x0) as f32;
        let vp_h_log = (vp_y1 - vp_y0) as f32;
        let vp_pw = (vp_w_log * scale).round().max(1.0) as u32;
        let vp_ph = (vp_h_log * scale).round().max(1.0) as u32;
        let mut vp_bmp = RgbaBitmap::new_transparent(vp_pw, vp_ph);

        let content_h = self.measure_content_height();
        self.max_scroll_px = (content_h - vp_h_log).max(0.0);
        self.scroll_px = self.scroll_px.clamp(0.0, self.max_scroll_px);

        let right_edge = if self.max_scroll_px > 0.5 { 496.0 } else { 508.0 };
        let mut cur_y = 2.0_f32;
        let mut viewport_hits = Vec::new();

        match self.active_tab {
            0 => {
                cur_y = self.draw_section_header(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.hdr_opacity_scale,
                );
                cur_y = self.draw_slider_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "slider_skin_opacity",
                    SliderKind::SkinOpacity,
                    data.cfg.skin_opacity,
                    s.sld_skin_opacity_title,
                    s.sld_skin_opacity_sub,
                    &format!("{}%", (data.cfg.skin_opacity * 100.0).round() as i32),
                    &mut viewport_hits,
                );
                cur_y = self.draw_slider_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "slider_balloon_opacity",
                    SliderKind::BalloonOpacity,
                    data.cfg.balloon_opacity,
                    s.sld_balloon_opacity_title,
                    s.sld_balloon_opacity_sub,
                    &format!("{}%", (data.cfg.balloon_opacity * 100.0).round() as i32),
                    &mut viewport_hits,
                );
                cur_y = self.draw_slider_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "slider_character_scale",
                    SliderKind::CharacterScale,
                    data.cfg.character_scale,
                    s.sld_scale_title,
                    s.sld_scale_sub,
                    &format!("{:.0}%", data.cfg.character_scale * 100.0),
                    &mut viewport_hits,
                );

                cur_y += 6.0;
                cur_y = self.draw_section_header(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.hdr_motion_skins,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_opaque_hover",
                    s.tog_opaque_hover,
                    data.cfg.opaque_on_hover,
                    SettingsAction::ToggleOpaqueOnHover,
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_live2d",
                    s.tog_live2d,
                    data.cfg.live2d_motion,
                    SettingsAction::ToggleLive2dMotion,
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_eye_track",
                    s.tog_eye_track,
                    data.cfg.eye_tracking,
                    SettingsAction::ToggleEyeTracking,
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_kero",
                    s.tog_kero,
                    data.cfg.show_kero,
                    SettingsAction::ToggleKero,
                    &mut viewport_hits,
                );
                cur_y = self.draw_cycle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "cyc_shell",
                    s.lbl_shell,
                    &data.active_shell_name,
                    SettingsAction::CycleShell(-1),
                    SettingsAction::CycleShell(1),
                    &mut viewport_hits,
                );
                let _ = self.draw_cycle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "cyc_balloon",
                    s.lbl_balloon,
                    &data.active_balloon_name,
                    SettingsAction::CycleBalloon(-1),
                    SettingsAction::CycleBalloon(1),
                    &mut viewport_hits,
                );
            }
            1 => {
                cur_y = self.draw_section_header(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.hdr_desktop_behavior,
                );
                cur_y = self.draw_cycle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "cyc_ui_lang",
                    s.lbl_ui_language,
                    &data.ui_language_label,
                    SettingsAction::CycleUiLanguage(-1),
                    SettingsAction::CycleUiLanguage(1),
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_aot",
                    s.tog_always_on_top,
                    data.cfg.always_on_top,
                    SettingsAction::ToggleAlwaysOnTop,
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_click_through",
                    s.tog_click_through,
                    data.cfg.click_through_mode,
                    SettingsAction::ToggleClickThrough,
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_lock_pos",
                    s.tog_lock_pos,
                    data.cfg.lock_position,
                    SettingsAction::ToggleLockPosition,
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_snap_bottom",
                    s.tog_snap_bottom,
                    data.cfg.snap_to_bottom,
                    SettingsAction::ToggleSnapToBottom,
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_chime",
                    s.tog_hourly_chime,
                    data.cfg.hourly_chime,
                    SettingsAction::ToggleHourlyChime,
                    &mut viewport_hits,
                );
                cur_y = self.draw_action_button_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "btn_reset_pos",
                    s.btn_reset_pos,
                    SettingsAction::ResetPositions,
                    &mut viewport_hits,
                );

                cur_y += 6.0;
                cur_y = self.draw_section_header(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.hdr_focus_accessories,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_quiet_mode",
                    s.tog_quiet_mode,
                    data.cfg.quiet_mode,
                    SettingsAction::ToggleQuietMode,
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_auto_hide_fs",
                    s.tog_auto_hide_fs,
                    data.cfg.auto_hide_on_fullscreen,
                    SettingsAction::ToggleAutoHideFullscreen,
                    &mut viewport_hits,
                );
                let hide_lbl = if data.characters_hidden {
                    s.btn_show_chars
                } else {
                    s.btn_hide_chars
                };
                cur_y = self.draw_dual_action_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "btn_hide_chars",
                    hide_lbl,
                    SettingsAction::ToggleHideCharacters,
                    "btn_next_monitor",
                    s.btn_next_monitor,
                    SettingsAction::MoveNextMonitor,
                    &mut viewport_hits,
                );
                cur_y = self.draw_cycle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "cyc_pomodoro_preset",
                    s.lbl_pomodoro_preset,
                    &data.pomodoro_preset_label,
                    SettingsAction::CyclePomodoroPreset(-1),
                    SettingsAction::CyclePomodoroPreset(1),
                    &mut viewport_hits,
                );
                cur_y = self.draw_dual_action_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "btn_pomo_toggle",
                    &data.pomodoro_button_label,
                    SettingsAction::TogglePomodoro,
                    "btn_pomo_stop",
                    s.btn_pomo_stop,
                    SettingsAction::StopPomodoro,
                    &mut viewport_hits,
                );
                cur_y = self.draw_dual_action_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "btn_show_backlog",
                    s.btn_show_backlog,
                    SettingsAction::ShowBacklog,
                    "btn_copy_last",
                    s.btn_copy_last,
                    SettingsAction::CopyLastReply,
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_autostart",
                    s.tog_autostart,
                    data.cfg.auto_start_at_login,
                    SettingsAction::ToggleAutoStart,
                    &mut viewport_hits,
                );
                cur_y = self.draw_cycle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "cyc_power_save",
                    s.lbl_power_save,
                    &data.power_save_label,
                    SettingsAction::CyclePowerSaveMode(-1),
                    SettingsAction::CyclePowerSaveMode(1),
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_low_batt_alert",
                    s.tog_low_batt_alert,
                    data.cfg.low_battery_alert,
                    SettingsAction::ToggleLowBatteryAlert,
                    &mut viewport_hits,
                );

                cur_y += 6.0;
                #[cfg(target_os = "macos")]
                let os_order = ["macos", "windows", "linux"];
                #[cfg(target_os = "windows")]
                let os_order = ["windows", "macos", "linux"];
                #[cfg(not(any(target_os = "macos", target_os = "windows")))]
                let os_order = ["linux", "macos", "windows"];

                for os_id in os_order {
                    match os_id {
                        "macos" => {
                            let hdr = if cfg!(target_os = "macos") {
                                s.os_hdr_macos_cur
                            } else {
                                s.os_hdr_macos_shared
                            };
                            cur_y = self.draw_section_header(
                                &mut vp_bmp,
                                text_renderer,
                                scale,
                                cur_y,
                                right_edge,
                                hdr,
                            );
                            cur_y = self.draw_toggle_row(
                                &mut vp_bmp,
                                text_renderer,
                                scale,
                                cur_y,
                                right_edge,
                                "tog_mac_first_mouse",
                                s.tog_mac_first_mouse,
                                data.cfg.macos_accept_first_mouse,
                                SettingsAction::ToggleMacosAcceptFirstMouse,
                                &mut viewport_hits,
                            );
                            cur_y = self.draw_toggle_row(
                                &mut vp_bmp,
                                text_renderer,
                                scale,
                                cur_y,
                                right_edge,
                                "tog_mac_shadow",
                                s.tog_mac_shadow,
                                data.cfg.macos_window_shadow,
                                SettingsAction::ToggleMacosShadow,
                                &mut viewport_hits,
                            );
                            cur_y = self.draw_toggle_row(
                                &mut vp_bmp,
                                text_renderer,
                                scale,
                                cur_y,
                                right_edge,
                                "tog_mac_dock",
                                s.tog_mac_dock,
                                data.cfg.macos_show_in_dock,
                                SettingsAction::ToggleMacosDock,
                                &mut viewport_hits,
                            );
                        }
                        "windows" => {
                            let hdr = if cfg!(target_os = "windows") {
                                s.os_hdr_win_cur
                            } else {
                                s.os_hdr_win_shared
                            };
                            cur_y = self.draw_section_header(
                                &mut vp_bmp,
                                text_renderer,
                                scale,
                                cur_y,
                                right_edge,
                                hdr,
                            );
                            cur_y = self.draw_toggle_row(
                                &mut vp_bmp,
                                text_renderer,
                                scale,
                                cur_y,
                                right_edge,
                                "tog_win_skip_taskbar",
                                s.tog_win_skip_taskbar,
                                data.cfg.windows_skip_taskbar,
                                SettingsAction::ToggleWindowsSkipTaskbar,
                                &mut viewport_hits,
                            );
                            cur_y = self.draw_toggle_row(
                                &mut vp_bmp,
                                text_renderer,
                                scale,
                                cur_y,
                                right_edge,
                                "tog_win_shadow",
                                s.tog_win_shadow,
                                data.cfg.windows_undecorated_shadow,
                                SettingsAction::ToggleWindowsShadow,
                                &mut viewport_hits,
                            );
                        }
                        _ => {
                            let hdr = if !cfg!(any(target_os = "macos", target_os = "windows")) {
                                s.os_hdr_linux_cur
                            } else {
                                s.os_hdr_linux_shared
                            };
                            cur_y = self.draw_section_header(
                                &mut vp_bmp,
                                text_renderer,
                                scale,
                                cur_y,
                                right_edge,
                                hdr,
                            );
                            cur_y = self.draw_toggle_row(
                                &mut vp_bmp,
                                text_renderer,
                                scale,
                                cur_y,
                                right_edge,
                                "tog_lin_x11",
                                s.tog_linux_x11,
                                data.cfg.linux_prefer_x11,
                                SettingsAction::ToggleLinuxPreferX11,
                                &mut viewport_hits,
                            );
                            cur_y = self.draw_toggle_row(
                                &mut vp_bmp,
                                text_renderer,
                                scale,
                                cur_y,
                                right_edge,
                                "tog_lin_workarea",
                                s.tog_linux_workarea,
                                data.cfg.linux_respect_workarea,
                                SettingsAction::ToggleLinuxWorkarea,
                                &mut viewport_hits,
                            );
                        }
                    }
                }
                let _ = cur_y;
            }
            2 => {
                cur_y = self.draw_section_header(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.hdr_voice_tts,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_voice_tts",
                    s.tog_voice_tts,
                    data.cfg.voice_tts,
                    SettingsAction::ToggleVoiceTts,
                    &mut viewport_hits,
                );
                cur_y = self.draw_cycle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "cyc_tts_engine",
                    s.lbl_tts_engine,
                    &data.tts_engine_label,
                    SettingsAction::CycleTtsEngine(-1),
                    SettingsAction::CycleTtsEngine(1),
                    &mut viewport_hits,
                );
                cur_y = self.draw_cycle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "cyc_sakura_voice",
                    s.lbl_sakura_voice,
                    &data.sakura_voice_label,
                    SettingsAction::CycleSakuraVoice(-1),
                    SettingsAction::CycleSakuraVoice(1),
                    &mut viewport_hits,
                );
                cur_y = self.draw_cycle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "cyc_kero_voice",
                    s.lbl_kero_voice,
                    &data.kero_voice_label,
                    SettingsAction::CycleKeroVoice(-1),
                    SettingsAction::CycleKeroVoice(1),
                    &mut viewport_hits,
                );
                cur_y = self.draw_cycle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "cyc_kero_arch",
                    s.lbl_kero_arch,
                    &data.kero_archetype_label,
                    SettingsAction::CycleKeroArchetype(-1),
                    SettingsAction::CycleKeroArchetype(1),
                    &mut viewport_hits,
                );
                let _ = self.draw_info_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y + 4.0,
                    right_edge,
                    s.lbl_note,
                    s.voice_note_value,
                );
            }
            _ => {
                cur_y = self.draw_section_header(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.hdr_ai_tools,
                );
                cur_y = self.draw_cycle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "cyc_provider",
                    s.lbl_provider,
                    &data.provider_label,
                    SettingsAction::CycleProvider(-1),
                    SettingsAction::CycleProvider(1),
                    &mut viewport_hits,
                );
                cur_y = self.draw_info_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.lbl_model_api,
                    &format!(
                        "{} · HTTP :{} / SSTP :{}",
                        data.cfg.model, data.cfg.http_port, data.cfg.sstp_tcp_port
                    ),
                );
                let setup_btn_label = if data.local_ai_setup_busy {
                    s.btn_setup_local_ai_busy
                } else {
                    s.btn_setup_local_ai
                };
                cur_y = self.draw_action_button_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "btn_setup_local_ai",
                    setup_btn_label,
                    SettingsAction::SetupLocalAi,
                    &mut viewport_hits,
                );
                cur_y = self.draw_info_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.lbl_best_free_ai,
                    &data.recommended_local_ai_hint,
                );
                cur_y += 6.0;
                cur_y = self.draw_section_header(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.hdr_autotalk_news,
                );
                let autotalk_lbl = format_seconds(data.ui_lang, data.cfg.auto_talk_interval_sec);
                cur_y = self.draw_slider_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "sld_autotalk",
                    SliderKind::AutoTalkInterval,
                    data.cfg.auto_talk_interval_sec as f32,
                    s.sld_autotalk_title,
                    s.sld_autotalk_sub,
                    &autotalk_lbl,
                    &mut viewport_hits,
                );
                let weight_lbl = format!(
                    "{}%",
                    (data.cfg.interest_news_weight * 100.0).round() as i32
                );
                cur_y = self.draw_slider_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "sld_interest_weight",
                    SliderKind::InterestWeight,
                    data.cfg.interest_news_weight,
                    s.sld_weight_title,
                    s.sld_weight_sub,
                    &weight_lbl,
                    &mut viewport_hits,
                );
                cur_y = self.draw_info_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.lbl_saved_interests,
                    &data.interests_summary,
                );
                cur_y = self.draw_action_button_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "btn_clear_interests",
                    s.btn_clear_interests,
                    SettingsAction::ClearInterests,
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_tools",
                    s.tog_tools,
                    data.cfg.enable_tools,
                    SettingsAction::ToggleEnableTools,
                    &mut viewport_hits,
                );
                cur_y = self.draw_toggle_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "tog_clip",
                    s.tog_clipboard,
                    data.cfg.enable_clipboard_tool,
                    SettingsAction::ToggleEnableClipboard,
                    &mut viewport_hits,
                );

                cur_y += 6.0;
                cur_y = self.draw_section_header(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.hdr_storage,
                );
                cur_y = self.draw_info_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    s.lbl_json_path,
                    &data.config_path_display,
                );
                let _ = self.draw_dual_action_row(
                    &mut vp_bmp,
                    text_renderer,
                    scale,
                    cur_y,
                    right_edge,
                    "btn_open_cfg_dir",
                    s.btn_open_cfg_dir,
                    SettingsAction::OpenConfigFolder,
                    "btn_clear_hist",
                    s.btn_clear_hist,
                    SettingsAction::ClearHistory,
                    &mut viewport_hits,
                );
            }
        }

        // Viewport scrollbar
        if self.max_scroll_px > 0.5 && content_h > vp_h_log {
            let track_x = 502.0 * scale;
            let track_w = 5.0 * scale;
            let track_y0 = 4.0 * scale;
            let track_h = (vp_h_log - 8.0) * scale;
            vp_bmp.draw_aa_rounded_box(
                (track_x, track_y0, track_x + track_w, track_y0 + track_h),
                track_w * 0.5,
                [148, 163, 184, 50],
                [148, 163, 184, 50],
                0.0,
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            let frac_vis = (vp_h_log / content_h).clamp(0.12, 0.95);
            let thumb_h = (track_h * frac_vis).max(24.0 * scale);
            let frac_pos = (self.scroll_px / self.max_scroll_px).clamp(0.0, 1.0);
            let thumb_y0 = track_y0 + frac_pos * (track_h - thumb_h);
            vp_bmp.draw_aa_rounded_box(
                (track_x, thumb_y0, track_x + track_w, thumb_y0 + thumb_h),
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

        // Translate viewport hit rects into window logical coords clipped to viewport bounds
        for mut ht in viewport_hits {
            ht.rect.0 += vp_x0;
            ht.rect.1 = (ht.rect.1 + vp_y0).max(vp_y0);
            ht.rect.2 += vp_x0;
            ht.rect.3 = (ht.rect.3 + vp_y0).min(vp_y1);
            if let SettingsHitKind::Slider {
                ref mut track_x0,
                ref mut track_x1,
                ..
            } = ht.kind
            {
                *track_x0 += vp_x0;
                *track_x1 += vp_x0;
            }
            if ht.rect.3 > ht.rect.1 + 4.0 && ht.rect.2 > ht.rect.0 + 4.0 {
                self.hit_targets.push(ht);
            }
        }

        canvas.alpha_composite(
            &vp_bmp,
            (vp_x0 as f32 * scale).round() as i32,
            (vp_y0 as f32 * scale).round() as i32,
        );

        // Footer bar (y=426..470)
        canvas.fill_rect_alpha(
            (16.0 * scale) as i32,
            (426.0 * scale) as i32,
            ((w_log - 32.0) * scale) as i32,
            (1.0 * scale).max(1.0) as i32,
            90,
            125,
            195,
            110,
        );
        text_renderer.draw_inline_text(
            &mut canvas,
            s.footer_autosave,
            (18.0 * scale).round() as i32,
            (440.0 * scale).round() as i32,
            (16.0 * scale).round() as i32,
            (334.0 * scale).round() as i32,
            10.8 * scale,
            14.5 * scale,
            (147, 197, 253),
        );

        let rst_rect = (342.0, 433.0, 522.0, 461.0);
        let rst_hov = self.hovered_key == Some("btn_reset_defaults");
        let (r_a, r_b, r_brd) = if rst_hov {
            ([62, 92, 165, 235], [44, 68, 128, 235], [147, 197, 253, 210])
        } else {
            ([30, 44, 78, 225], [22, 34, 62, 225], [96, 142, 218, 155])
        };
        canvas.draw_aa_rounded_box(
            (
                rst_rect.0 as f32 * scale,
                rst_rect.1 as f32 * scale,
                rst_rect.2 as f32 * scale,
                rst_rect.3 as f32 * scale,
            ),
            8.0 * scale,
            r_a,
            r_b,
            1.0 * scale,
            r_brd,
            r_brd,
            0.0,
            0.0,
            [0, 0, 0, 0],
            0,
        );
        let rst_lbl = s.btn_reset_defaults;
        let rw = text_renderer.measure_inline_width(rst_lbl, 11.2 * scale) / scale;
        let rx = rst_rect.0 as f32 + (((rst_rect.2 - rst_rect.0) as f32 - rw) * 0.5).max(6.0);
        text_renderer.draw_inline_text(
            &mut canvas,
            rst_lbl,
            (rx * scale).round() as i32,
            ((rst_rect.1 as f32 + 7.5) * scale).round() as i32,
            ((rst_rect.0 as f32 + 4.0) * scale).round() as i32,
            ((rst_rect.2 as f32 - 4.0) * scale).round() as i32,
            11.2 * scale,
            14.5 * scale,
            (236, 244, 255),
        );
        self.hit_targets.push(SettingsHitTarget {
            key: "btn_reset_defaults",
            rect: rst_rect,
            kind: SettingsHitKind::Action(SettingsAction::ResetDefaults),
        });

        canvas
    }

    fn measure_content_height(&self) -> f32 {
        match self.active_tab {
            0 => 24.0 + 3.0 * 52.0 + 10.0 + 24.0 + 4.0 * 40.0 + 2.0 * 42.0 + 12.0,
            1 => {
                (24.0 + 42.0 + 5.0 * 40.0 + 42.0)
                    + (6.0 + 24.0 + 4.0 * 40.0 + 5.0 * 42.0)
                    + (6.0 + 3.0 * 24.0 + 7.0 * 40.0)
                    + 14.0
            }
            2 => 24.0 + 40.0 + 4.0 * 42.0 + 42.0 + 12.0,
            _ => 600.0,
        }
    }

    fn draw_section_header(
        &self,
        bmp: &mut RgbaBitmap,
        tr: &mut TextRenderer,
        scale: f32,
        y: f32,
        right_edge: f32,
        title: &str,
    ) -> f32 {
        let vy = y - self.scroll_px;
        if vy + 24.0 > 0.0 && vy < (bmp.height as f32) / scale {
            tr.draw_inline_text(
                bmp,
                title,
                (6.0 * scale).round() as i32,
                ((vy + 5.0) * scale).round() as i32,
                (4.0 * scale).round() as i32,
                ((right_edge - 4.0) * scale).round() as i32,
                11.0 * scale,
                14.5 * scale,
                (125, 195, 255),
            );
        }
        y + 24.0
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_slider_row(
        &self,
        bmp: &mut RgbaBitmap,
        tr: &mut TextRenderer,
        scale: f32,
        y: f32,
        right_edge: f32,
        key: &'static str,
        kind: SliderKind,
        value: f32,
        title: &str,
        subtitle: &str,
        value_text: &str,
        hits: &mut Vec<SettingsHitTarget>,
    ) -> f32 {
        let card_h = 46.0_f32;
        let vy = y - self.scroll_px;
        let vp_h = (bmp.height as f32) / scale;
        let track_x0 = 224.0_f32;
        let track_x1 = right_edge - 66.0;
        if vy + card_h > 0.0 && vy < vp_h {
            let hov = self.hovered_key == Some(key)
                || self.dragging_slider.is_some_and(|(k, _, _)| k == kind);
            let brd = if hov {
                [110, 175, 250, 195]
            } else {
                [70, 110, 185, 115]
            };
            bmp.draw_aa_rounded_box(
                (
                    2.0 * scale,
                    vy * scale,
                    right_edge * scale,
                    (vy + card_h) * scale,
                ),
                9.0 * scale,
                [24, 35, 64, 225],
                [18, 28, 52, 232],
                1.0 * scale,
                brd,
                brd,
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            tr.draw_inline_text(
                bmp,
                title,
                (12.0 * scale).round() as i32,
                ((vy + 7.0) * scale).round() as i32,
                (10.0 * scale).round() as i32,
                ((track_x0 - 8.0) * scale).round() as i32,
                11.6 * scale,
                15.0 * scale,
                (238, 246, 255),
            );
            tr.draw_inline_text(
                bmp,
                subtitle,
                (12.0 * scale).round() as i32,
                ((vy + 24.5) * scale).round() as i32,
                (10.0 * scale).round() as i32,
                ((track_x0 - 8.0) * scale).round() as i32,
                9.6 * scale,
                13.0 * scale,
                (142, 168, 212),
            );

            // Track background & filled bar
            let cy = vy + 23.0;
            bmp.draw_aa_rounded_box(
                (
                    track_x0 * scale,
                    (cy - 3.0) * scale,
                    track_x1 * scale,
                    (cy + 3.0) * scale,
                ),
                3.0 * scale,
                [42, 58, 96, 235],
                [35, 48, 82, 235],
                0.0,
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            let frac = kind.fraction_of(value);
            let knob_x = track_x0 + (track_x1 - track_x0) * frac;
            if knob_x > track_x0 + 3.0 {
                bmp.draw_aa_rounded_box(
                    (
                        track_x0 * scale,
                        (cy - 3.0) * scale,
                        knob_x * scale,
                        (cy + 3.0) * scale,
                    ),
                    3.0 * scale,
                    [96, 165, 250, 250],
                    [37, 99, 235, 250],
                    0.0,
                    [0, 0, 0, 0],
                    [0, 0, 0, 0],
                    0.0,
                    0.0,
                    [0, 0, 0, 0],
                    0,
                );
            }
            bmp.draw_aa_circle(
                knob_x * scale,
                cy * scale,
                7.2 * scale,
                [235, 246, 255, 255],
                [96, 165, 250, 255],
            );

            // Value readout badge
            let bx0 = track_x1 + 10.0;
            let bx1 = right_edge - 8.0;
            bmp.draw_aa_rounded_box(
                (
                    bx0 * scale,
                    (vy + 11.0) * scale,
                    bx1 * scale,
                    (vy + 35.0) * scale,
                ),
                6.0 * scale,
                [30, 48, 86, 235],
                [24, 38, 70, 235],
                0.8 * scale,
                [88, 138, 220, 155],
                [88, 138, 220, 155],
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            let vw = tr.measure_inline_width(value_text, 11.4 * scale) / scale;
            let vx = bx0 + (((bx1 - bx0) - vw) * 0.5).max(3.0);
            tr.draw_inline_text(
                bmp,
                value_text,
                (vx * scale).round() as i32,
                ((vy + 16.0) * scale).round() as i32,
                ((bx0 + 2.0) * scale).round() as i32,
                ((bx1 - 2.0) * scale).round() as i32,
                11.4 * scale,
                15.0 * scale,
                (186, 224, 255),
            );
            hits.push(SettingsHitTarget {
                key,
                rect: (
                    (track_x0 - 8.0) as f64,
                    (vy + 4.0) as f64,
                    (track_x1 + 8.0) as f64,
                    (vy + card_h - 4.0) as f64,
                ),
                kind: SettingsHitKind::Slider {
                    kind,
                    track_x0: track_x0 as f64,
                    track_x1: track_x1 as f64,
                },
            });
        }
        y + card_h + 6.0
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_toggle_row(
        &self,
        bmp: &mut RgbaBitmap,
        tr: &mut TextRenderer,
        scale: f32,
        y: f32,
        right_edge: f32,
        key: &'static str,
        label: &str,
        checked: bool,
        action: SettingsAction,
        hits: &mut Vec<SettingsHitTarget>,
    ) -> f32 {
        let card_h = 34.0_f32;
        let vy = y - self.scroll_px;
        let vp_h = (bmp.height as f32) / scale;
        if vy + card_h > 0.0 && vy < vp_h {
            let hov = self.hovered_key == Some(key);
            let (fill_a, fill_b, brd) = if hov {
                ([34, 52, 92, 232], [26, 40, 74, 232], [116, 178, 250, 200])
            } else {
                ([24, 35, 64, 225], [18, 28, 52, 232], [70, 110, 185, 110])
            };
            bmp.draw_aa_rounded_box(
                (
                    2.0 * scale,
                    vy * scale,
                    right_edge * scale,
                    (vy + card_h) * scale,
                ),
                8.0 * scale,
                fill_a,
                fill_b,
                1.0 * scale,
                brd,
                brd,
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            let sw_x1 = right_edge - 10.0;
            let sw_x0 = sw_x1 - 44.0;
            let sw_y0 = vy + 7.0;
            let sw_y1 = vy + card_h - 7.0;
            tr.draw_inline_text(
                bmp,
                label,
                (12.0 * scale).round() as i32,
                ((vy + 9.5) * scale).round() as i32,
                (10.0 * scale).round() as i32,
                ((sw_x0 - 10.0) * scale).round() as i32,
                11.8 * scale,
                15.0 * scale,
                (238, 245, 255),
            );
            let (sw_a, sw_b) = if checked {
                ([59, 130, 246, 250], [37, 99, 235, 250])
            } else {
                ([52, 65, 92, 235], [42, 52, 76, 235])
            };
            bmp.draw_aa_rounded_box(
                (sw_x0 * scale, sw_y0 * scale, sw_x1 * scale, sw_y1 * scale),
                10.0 * scale,
                sw_a,
                sw_b,
                0.8 * scale,
                [130, 180, 245, if checked { 190 } else { 70 }],
                [130, 180, 245, if checked { 190 } else { 70 }],
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            let knob_x = if checked { sw_x1 - 10.0 } else { sw_x0 + 10.0 };
            bmp.draw_aa_circle(
                knob_x * scale,
                ((sw_y0 + sw_y1) * 0.5) * scale,
                7.2 * scale,
                [255, 255, 255, 255],
                [215, 232, 255, 255],
            );
            hits.push(SettingsHitTarget {
                key,
                rect: (2.0, vy as f64, right_edge as f64, (vy + card_h) as f64),
                kind: SettingsHitKind::Action(action),
            });
        }
        y + card_h + 6.0
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_cycle_row(
        &self,
        bmp: &mut RgbaBitmap,
        tr: &mut TextRenderer,
        scale: f32,
        y: f32,
        right_edge: f32,
        key: &'static str,
        label: &str,
        value: &str,
        prev_act: SettingsAction,
        next_act: SettingsAction,
        hits: &mut Vec<SettingsHitTarget>,
    ) -> f32 {
        let card_h = 36.0_f32;
        let vy = y - self.scroll_px;
        let vp_h = (bmp.height as f32) / scale;
        if vy + card_h > 0.0 && vy < vp_h {
            bmp.draw_aa_rounded_box(
                (
                    2.0 * scale,
                    vy * scale,
                    right_edge * scale,
                    (vy + card_h) * scale,
                ),
                8.0 * scale,
                [24, 35, 64, 225],
                [18, 28, 52, 232],
                1.0 * scale,
                [70, 110, 185, 110],
                [70, 110, 185, 110],
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            let prev_rect = (176.0_f32, vy + 5.0, 202.0_f32, vy + card_h - 5.0);
            let next_rect = (right_edge - 34.0, vy + 5.0, right_edge - 8.0, vy + card_h - 5.0);
            let val_x0 = prev_rect.2 + 6.0;
            let val_x1 = next_rect.0 - 6.0;

            tr.draw_inline_text(
                bmp,
                label,
                (12.0 * scale).round() as i32,
                ((vy + 10.5) * scale).round() as i32,
                (10.0 * scale).round() as i32,
                ((prev_rect.0 - 6.0) * scale).round() as i32,
                11.6 * scale,
                15.0 * scale,
                (236, 244, 255),
            );

            let prev_key: &'static str = match key {
                "cyc_shell" => "cyc_shell_prev",
                "cyc_balloon" => "cyc_balloon_prev",
                "cyc_ui_lang" => "cyc_ui_lang_prev",
                "cyc_pomodoro_preset" => "cyc_pomodoro_preset_prev",
                "cyc_power_save" => "cyc_power_save_prev",
                "cyc_tts_engine" => "cyc_tts_engine_prev",
                "cyc_sakura_voice" => "cyc_sakura_voice_prev",
                "cyc_kero_voice" => "cyc_kero_voice_prev",
                "cyc_kero_arch" => "cyc_kero_arch_prev",
                "cyc_provider" => "cyc_provider_prev",
                _ => "cyc_autotalk_prev",
            };
            let next_key: &'static str = match key {
                "cyc_shell" => "cyc_shell_next",
                "cyc_balloon" => "cyc_balloon_next",
                "cyc_ui_lang" => "cyc_ui_lang_next",
                "cyc_pomodoro_preset" => "cyc_pomodoro_preset_next",
                "cyc_power_save" => "cyc_power_save_next",
                "cyc_tts_engine" => "cyc_tts_engine_next",
                "cyc_sakura_voice" => "cyc_sakura_voice_next",
                "cyc_kero_voice" => "cyc_kero_voice_next",
                "cyc_kero_arch" => "cyc_kero_arch_next",
                "cyc_provider" => "cyc_provider_next",
                _ => "cyc_autotalk_next",
            };

            for (btn_key, br, arrow, act) in [
                (prev_key, prev_rect, "◀", prev_act),
                (next_key, next_rect, "▶", next_act),
            ] {
                let hov = self.hovered_key == Some(btn_key);
                let (ba, bb, bbrd) = if hov {
                    ([62, 108, 212, 235], [40, 80, 180, 235], [147, 197, 253, 210])
                } else {
                    ([32, 48, 86, 235], [24, 38, 70, 235], [88, 138, 220, 150])
                };
                bmp.draw_aa_rounded_box(
                    (br.0 * scale, br.1 * scale, br.2 * scale, br.3 * scale),
                    6.0 * scale,
                    ba,
                    bb,
                    0.9 * scale,
                    bbrd,
                    bbrd,
                    0.0,
                    0.0,
                    [0, 0, 0, 0],
                    0,
                );
                let aw = tr.measure_inline_width(arrow, 10.8 * scale) / scale;
                let ax = br.0 + ((br.2 - br.0 - aw) * 0.5).max(2.0);
                tr.draw_inline_text(
                    bmp,
                    arrow,
                    (ax * scale).round() as i32,
                    ((br.1 + 6.0) * scale).round() as i32,
                    ((br.0 + 1.0) * scale).round() as i32,
                    ((br.2 - 1.0) * scale).round() as i32,
                    10.8 * scale,
                    14.0 * scale,
                    (236, 245, 255),
                );
                hits.push(SettingsHitTarget {
                    key: btn_key,
                    rect: (br.0 as f64, br.1 as f64, br.2 as f64, br.3 as f64),
                    kind: SettingsHitKind::Action(act),
                });
            }

            bmp.draw_aa_rounded_box(
                (
                    val_x0 * scale,
                    (vy + 5.0) * scale,
                    val_x1 * scale,
                    (vy + card_h - 5.0) * scale,
                ),
                6.0 * scale,
                [14, 22, 42, 235],
                [12, 18, 36, 235],
                0.8 * scale,
                [72, 112, 182, 130],
                [72, 112, 182, 130],
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            tr.draw_inline_text(
                bmp,
                value,
                ((val_x0 + 8.0) * scale).round() as i32,
                ((vy + 10.5) * scale).round() as i32,
                ((val_x0 + 6.0) * scale).round() as i32,
                ((val_x1 - 6.0) * scale).round() as i32,
                11.3 * scale,
                14.5 * scale,
                (198, 228, 255),
            );
        }
        y + card_h + 6.0
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_action_button_row(
        &self,
        bmp: &mut RgbaBitmap,
        tr: &mut TextRenderer,
        scale: f32,
        y: f32,
        right_edge: f32,
        key: &'static str,
        label: &str,
        action: SettingsAction,
        hits: &mut Vec<SettingsHitTarget>,
    ) -> f32 {
        let card_h = 36.0_f32;
        let vy = y - self.scroll_px;
        let vp_h = (bmp.height as f32) / scale;
        if vy + card_h > 0.0 && vy < vp_h {
            let hov = self.hovered_key == Some(key);
            let (ba, bb, brd) = if hov {
                ([56, 102, 204, 238], [38, 76, 168, 238], [147, 197, 253, 210])
            } else {
                ([30, 46, 84, 230], [22, 34, 66, 230], [88, 138, 220, 150])
            };
            bmp.draw_aa_rounded_box(
                (2.0 * scale, vy * scale, right_edge * scale, (vy + card_h) * scale),
                8.0 * scale,
                ba,
                bb,
                1.0 * scale,
                brd,
                brd,
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            let tw = tr.measure_inline_width(label, 11.6 * scale) / scale;
            let tx = 2.0 + (((right_edge - 2.0) - tw) * 0.5).max(8.0);
            tr.draw_inline_text(
                bmp,
                label,
                (tx * scale).round() as i32,
                ((vy + 10.5) * scale).round() as i32,
                (8.0 * scale).round() as i32,
                ((right_edge - 8.0) * scale).round() as i32,
                11.6 * scale,
                15.0 * scale,
                (236, 246, 255),
            );
            hits.push(SettingsHitTarget {
                key,
                rect: (2.0, vy as f64, right_edge as f64, (vy + card_h) as f64),
                kind: SettingsHitKind::Action(action),
            });
        }
        y + card_h + 6.0
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_dual_action_row(
        &self,
        bmp: &mut RgbaBitmap,
        tr: &mut TextRenderer,
        scale: f32,
        y: f32,
        right_edge: f32,
        key_l: &'static str,
        label_l: &str,
        act_l: SettingsAction,
        key_r: &'static str,
        label_r: &str,
        act_r: SettingsAction,
        hits: &mut Vec<SettingsHitTarget>,
    ) -> f32 {
        let card_h = 36.0_f32;
        let vy = y - self.scroll_px;
        let vp_h = (bmp.height as f32) / scale;
        if vy + card_h > 0.0 && vy < vp_h {
            let gap = 8.0_f32;
            let half = (right_edge - 2.0 - gap) * 0.5;
            for (k, lbl, act, x0, x1) in [
                (key_l, label_l, act_l, 2.0, 2.0 + half),
                (key_r, label_r, act_r, 2.0 + half + gap, right_edge),
            ] {
                let hov = self.hovered_key == Some(k);
                let (ba, bb, brd) = if hov {
                    ([56, 102, 204, 238], [38, 76, 168, 238], [147, 197, 253, 210])
                } else {
                    ([30, 46, 84, 230], [22, 34, 66, 230], [88, 138, 220, 150])
                };
                bmp.draw_aa_rounded_box(
                    (x0 * scale, vy * scale, x1 * scale, (vy + card_h) * scale),
                    8.0 * scale,
                    ba,
                    bb,
                    1.0 * scale,
                    brd,
                    brd,
                    0.0,
                    0.0,
                    [0, 0, 0, 0],
                    0,
                );
                let tw = tr.measure_inline_width(lbl, 11.4 * scale) / scale;
                let tx = x0 + (((x1 - x0) - tw) * 0.5).max(6.0);
                tr.draw_inline_text(
                    bmp,
                    lbl,
                    (tx * scale).round() as i32,
                    ((vy + 10.5) * scale).round() as i32,
                    ((x0 + 4.0) * scale).round() as i32,
                    ((x1 - 4.0) * scale).round() as i32,
                    11.4 * scale,
                    15.0 * scale,
                    (236, 246, 255),
                );
                hits.push(SettingsHitTarget {
                    key: k,
                    rect: (x0 as f64, vy as f64, x1 as f64, (vy + card_h) as f64),
                    kind: SettingsHitKind::Action(act),
                });
            }
        }
        y + card_h + 6.0
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_info_row(
        &self,
        bmp: &mut RgbaBitmap,
        tr: &mut TextRenderer,
        scale: f32,
        y: f32,
        right_edge: f32,
        label: &str,
        value: &str,
    ) -> f32 {
        let card_h = 30.0_f32;
        let vy = y - self.scroll_px;
        let vp_h = (bmp.height as f32) / scale;
        if vy + card_h > 0.0 && vy < vp_h {
            bmp.draw_aa_rounded_box(
                (2.0 * scale, vy * scale, right_edge * scale, (vy + card_h) * scale),
                7.0 * scale,
                [16, 25, 46, 215],
                [13, 20, 38, 220],
                0.8 * scale,
                [64, 96, 158, 95],
                [64, 96, 158, 95],
                0.0,
                0.0,
                [0, 0, 0, 0],
                0,
            );
            tr.draw_inline_text(
                bmp,
                label,
                (12.0 * scale).round() as i32,
                ((vy + 8.0) * scale).round() as i32,
                (10.0 * scale).round() as i32,
                (116.0 * scale).round() as i32,
                10.8 * scale,
                14.0 * scale,
                (148, 178, 224),
            );
            tr.draw_inline_text(
                bmp,
                value,
                (122.0 * scale).round() as i32,
                ((vy + 8.0) * scale).round() as i32,
                (120.0 * scale).round() as i32,
                ((right_edge - 8.0) * scale).round() as i32,
                10.8 * scale,
                14.0 * scale,
                (214, 232, 255),
            );
        }
        y + card_h + 6.0
    }
}
