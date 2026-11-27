use crate::gui::TextRenderer;
use crate::i18n::UiLang;
use crate::shell_parser::RgbaBitmap;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, OnceLock};

pub const DIARY_LOGICAL_W: u32 = 720;
pub const DIARY_LOGICAL_H: u32 = 470;
pub const DIARY_MIN_W: u32 = 520;
pub const DIARY_MAX_W: u32 = 1280;
pub const DIARY_MIN_H: u32 = 340;
pub const DIARY_MAX_H: u32 = 860;
/// Hit-test rect for the top-right close button at default size (logical coords).
pub const DIARY_CLOSE_RECT: (f32, f32, f32, f32) = (668.0, 26.0, 26.0, 26.0);

/// Clamp diary logical dimensions into a safe, readable open-book range.
pub fn clamp_diary_size(w: u32, h: u32) -> (u32, u32) {
    (w.clamp(DIARY_MIN_W, DIARY_MAX_W), h.clamp(DIARY_MIN_H, DIARY_MAX_H))
}

/// Dynamic hit-test rect for the top-right close button `(x, y, w, h)` in logical coords.
pub fn diary_close_rect(logical_w: u32, _logical_h: u32) -> (f32, f32, f32, f32) {
    let (lw, _) = clamp_diary_size(logical_w, DIARY_LOGICAL_H);
    ((lw as f32 - 52.0).max(200.0), 26.0, 26.0, 26.0)
}

/// Dynamic hit-test rect for the bottom-right corner resize grip `(x, y, w, h)` in logical coords.
pub fn diary_resize_grip_rect(logical_w: u32, logical_h: u32) -> (f32, f32, f32, f32) {
    let (lw, lh) = clamp_diary_size(logical_w, logical_h);
    (lw as f32 - 44.0, lh as f32 - 44.0, 34.0, 34.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiaryTaskStatus {
    Pending,
    Running,
    Done,
    Failed,
}

impl DiaryTaskStatus {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "running" | "in_progress" | "active" | "実行中" | "진행중" | "실행중" => {
                DiaryTaskStatus::Running
            }
            "done" | "completed" | "pass" | "ok" | "完了" | "완료" => DiaryTaskStatus::Done,
            "failed" | "error" | "fail" | "失敗" | "실패" => DiaryTaskStatus::Failed,
            _ => DiaryTaskStatus::Pending,
        }
    }

    pub fn checkbox(self) -> &'static str {
        match self {
            DiaryTaskStatus::Pending => "[ ]",
            DiaryTaskStatus::Running => "[▶]",
            DiaryTaskStatus::Done => "[✓]",
            DiaryTaskStatus::Failed => "[✗]",
        }
    }

    pub fn ink_color(self) -> [u8; 4] {
        match self {
            DiaryTaskStatus::Pending => [95, 86, 78, 255],
            DiaryTaskStatus::Running => [176, 98, 18, 255],
            DiaryTaskStatus::Done => [42, 118, 68, 255],
            DiaryTaskStatus::Failed => [178, 48, 48, 255],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiaryTask {
    pub index: usize,
    pub title: String,
    pub status: DiaryTaskStatus,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDiary {
    pub title: String,
    pub updated_at: String,
    pub tasks: Vec<DiaryTask>,
}

impl Default for TaskDiary {
    fn default() -> Self {
        Self {
            title: "Task Diary".to_string(),
            updated_at: Local::now().format("%Y-%m-%d %H:%M").to_string(),
            tasks: Vec::new(),
        }
    }
}

impl TaskDiary {
    pub fn completed_count(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| t.status == DiaryTaskStatus::Done)
            .count()
    }

    pub fn progress_percent(&self) -> u32 {
        if self.tasks.is_empty() {
            return 0;
        }
        ((self.completed_count() as f64 / self.tasks.len() as f64) * 100.0).round() as u32
    }

    pub fn progress_bar(&self) -> String {
        format_progress_bar(self.completed_count(), self.tasks.len(), 10)
    }
}

/// Format a character progress bar where `□` gradually fills with `■`, e.g. `■■■■■□□□□□ 50% (2/4)`.
pub fn format_progress_bar(done: usize, total: usize, width: usize) -> String {
    let w = width.clamp(4, 24);
    if total == 0 {
        return format!("{} 0% (0/0)", "□".repeat(w));
    }
    let clamped_done = done.min(total);
    let filled = ((clamped_done as f64 / total as f64) * (w as f64)).round() as usize;
    let filled = filled.min(w);
    let empty = w - filled;
    let pct = ((clamped_done as f64 / total as f64) * 100.0).round() as u32;
    format!(
        "{}{} {}% ({}/{})",
        "■".repeat(filled),
        "□".repeat(empty),
        pct,
        clamped_done,
        total
    )
}

type DiaryChangeCallback = Arc<dyn Fn(bool) + Send + Sync>;

fn global_diary_slot() -> &'static Mutex<TaskDiary> {
    static SLOT: OnceLock<Mutex<TaskDiary>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(TaskDiary::default()))
}

fn global_callback_slot() -> &'static Mutex<Option<DiaryChangeCallback>> {
    static CB: OnceLock<Mutex<Option<DiaryChangeCallback>>> = OnceLock::new();
    CB.get_or_init(|| Mutex::new(None))
}

/// Register a UI notification callback invoked whenever the diary is updated (`open_window: bool`).
pub fn set_diary_change_callback(cb: DiaryChangeCallback) {
    if let Ok(mut guard) = global_callback_slot().lock() {
        *guard = Some(cb);
    }
}

fn notify_diary_changed(open_window: bool) {
    let cb = global_callback_slot()
        .lock()
        .ok()
        .and_then(|g| g.clone());
    if let Some(f) = cb {
        f(open_window);
    }
}

/// Snapshot current diary state.
pub fn current_diary() -> TaskDiary {
    global_diary_slot()
        .lock()
        .map(|d| d.clone())
        .unwrap_or_default()
}

/// Initialize a new multi-task plan in the diary and open the Diary window.
pub fn plan_diary_tasks(title: &str, task_titles: &[String]) -> TaskDiary {
    let now = Local::now().format("%Y-%m-%d %H:%M").to_string();
    let clean_title = if title.trim().is_empty() {
        "Task Diary".to_string()
    } else {
        title.trim().chars().take(48).collect()
    };
    let tasks: Vec<DiaryTask> = task_titles
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .take(12)
        .enumerate()
        .map(|(i, t)| DiaryTask {
            index: i + 1,
            title: t.chars().take(60).collect(),
            status: DiaryTaskStatus::Pending,
            result: String::new(),
        })
        .collect();
    let next = TaskDiary {
        title: clean_title,
        updated_at: now,
        tasks,
    };
    if let Ok(mut guard) = global_diary_slot().lock() {
        *guard = next.clone();
    }
    notify_diary_changed(true);
    next
}

/// Update an existing 1-based task's status and execution result (appends a new task if index > len).
pub fn update_diary_task(
    task_index: usize,
    status: DiaryTaskStatus,
    result: &str,
    fallback_title: Option<&str>,
) -> TaskDiary {
    let now = Local::now().format("%Y-%m-%d %H:%M").to_string();
    let updated = {
        let mut guard = global_diary_slot().lock().unwrap_or_else(|e| e.into_inner());
        guard.updated_at = now;
        let clean_res: String = result.trim().chars().take(120).collect();
        if task_index >= 1 && task_index <= guard.tasks.len() {
            let t = &mut guard.tasks[task_index - 1];
            t.status = status;
            if !clean_res.is_empty() {
                t.result = clean_res;
            }
        } else if guard.tasks.len() < 12 {
            let idx = guard.tasks.len() + 1;
            let title = fallback_title
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("Task")
                .chars()
                .take(60)
                .collect();
            guard.tasks.push(DiaryTask {
                index: idx,
                title,
                status,
                result: clean_res,
            });
        }
        guard.clone()
    };
    notify_diary_changed(true);
    updated
}

/// Clear all diary entries.
pub fn clear_diary() -> TaskDiary {
    let next = TaskDiary::default();
    if let Ok(mut guard) = global_diary_slot().lock() {
        *guard = next.clone();
    }
    notify_diary_changed(false);
    next
}

// ---------------------------------------------------------------------------
// Illustrated Open-Book Paper Diary Renderer (100% Safe Rust, HiDPI RGBA)
// ---------------------------------------------------------------------------

fn blend_px(buf: &mut [u8], w: u32, h: u32, x: i32, y: i32, c: [u8; 4]) {
    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 || c[3] == 0 {
        return;
    }
    let idx = ((y as u32 * w + x as u32) * 4) as usize;
    let sa = c[3] as f32 / 255.0;
    let da = buf[idx + 3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0001 {
        return;
    }
    for ch in 0..3 {
        let sc = c[ch] as f32 / 255.0;
        let dc = buf[idx + ch] as f32 / 255.0;
        let oc = (sc * sa + dc * da * (1.0 - sa)) / out_a;
        buf[idx + ch] = (oc * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    buf[idx + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

fn fill_rect(buf: &mut [u8], w: u32, h: u32, x: i32, y: i32, rw: i32, rh: i32, c: [u8; 4]) {
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + rw).min(w as i32);
    let y1 = (y + rh).min(h as i32);
    for py in y0..y1 {
        for px in x0..x1 {
            blend_px(buf, w, h, px, py, c);
        }
    }
}

fn fill_rounded_rect(
    buf: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    rw: i32,
    rh: i32,
    radius: i32,
    c: [u8; 4],
) {
    let r = radius.min(rw / 2).min(rh / 2).max(0);
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + rw).min(w as i32);
    let y1 = (y + rh).min(h as i32);
    for py in y0..y1 {
        for px in x0..x1 {
            let lx = px - x;
            let ly = py - y;
            let inside = if lx < r && ly < r {
                let dx = r - lx;
                let dy = r - ly;
                dx * dx + dy * dy <= r * r
            } else if lx >= rw - r && ly < r {
                let dx = lx - (rw - r - 1);
                let dy = r - ly;
                dx * dx + dy * dy <= r * r
            } else if lx < r && ly >= rh - r {
                let dx = r - lx;
                let dy = ly - (rh - r - 1);
                dx * dx + dy * dy <= r * r
            } else if lx >= rw - r && ly >= rh - r {
                let dx = lx - (rw - r - 1);
                let dy = ly - (rh - r - 1);
                dx * dx + dy * dy <= r * r
            } else {
                true
            };
            if inside {
                blend_px(buf, w, h, px, py, c);
            }
        }
    }
}

fn draw_text_line(
    canvas: &mut RgbaBitmap,
    text_renderer: &mut TextRenderer,
    text: &str,
    x: i32,
    y: i32,
    max_w: f32,
    font_px: f32,
    color: [u8; 4],
) {
    if text.trim().is_empty() {
        return;
    }
    let clip_x1 = (x + max_w.round() as i32).min(canvas.width as i32);
    text_renderer.draw_inline_text(
        canvas,
        text,
        x,
        y,
        x.max(0),
        clip_x1.max(x + 16),
        font_px,
        font_px * 1.28,
        (color[0], color[1], color[2]),
    );
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Render an illustrated two-page open paper diary at default size (`720x470`).
pub fn render_diary_canvas(
    diary: &TaskDiary,
    text_renderer: &mut TextRenderer,
    scale: f32,
    lang: UiLang,
) -> RgbaBitmap {
    render_diary_canvas_sized(
        diary,
        text_renderer,
        DIARY_LOGICAL_W,
        DIARY_LOGICAL_H,
        scale,
        lang,
    )
}

/// Render an illustrated two-page open paper diary at arbitrary logical dimensions `(logical_w, logical_h)`:
/// - Hardcover leather border + stacked paper edges + center spine shadow + bookmark ribbon
/// - Ruled notebook lines on both pages + bottom-right corner resize grip
/// - Left page: Task list with checkboxes (`[ ]`/`[▶]`/`[✓]`/`[✗]`) & `■`/`□` progress bar
/// - Right page: Per-task execution results
pub fn render_diary_canvas_sized(
    diary: &TaskDiary,
    text_renderer: &mut TextRenderer,
    logical_w: u32,
    logical_h: u32,
    scale: f32,
    lang: UiLang,
) -> RgbaBitmap {
    let (lw_u, lh_u) = clamp_diary_size(logical_w, logical_h);
    let lw = lw_u as f32;
    let lh = lh_u as f32;
    let s = scale.clamp(1.0, 3.0);
    let sc = |v: f32| -> i32 { (v * s).round() as i32 };
    let w = (lw * s).round() as u32;
    let h = (lh * s).round() as u32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];

    let cover_w = lw - 20.0;
    let cover_h = lh - 22.0;
    let spine = (lw * 0.5).round();
    let page_h = (lh - 54.0).max(200.0);
    let left_page_w = (spine - 28.0).max(180.0);
    let right_page_w = (lw - 28.0 - spine).max(180.0);
    let left_rule_x = 42.0;
    let left_rule_w = (spine - 60.0).max(140.0);
    let right_rule_x = spine + 18.0;
    let right_rule_w = (lw - 42.0 - right_rule_x).max(140.0);
    let left_margin_x = 54.0;
    let right_margin_x = spine + 30.0;
    let left_text_x = 58.0;
    let right_text_x = spine + 34.0;
    let left_text_w = (spine - 78.0).max(140.0);
    let right_text_w = (lw - 44.0 - right_text_x).max(140.0);

    // 1. Soft outer drop shadow
    for i in (1..=6).rev() {
        let pad = i as f32 * 2.0;
        let alpha = (18 - i * 2).max(2) as u8;
        fill_rounded_rect(
            &mut rgba,
            w,
            h,
            sc(10.0 - pad * 0.4),
            sc(14.0 - pad * 0.2),
            sc(cover_w + pad * 0.8),
            sc((cover_h - 2.0) + pad * 0.8),
            sc(16.0),
            [28, 20, 14, alpha],
        );
    }

    // 2. Hardcover warm chestnut leather book cover + stitched inner frame
    fill_rounded_rect(
        &mut rgba,
        w,
        h,
        sc(10.0),
        sc(10.0),
        sc(cover_w),
        sc(cover_h),
        sc(14.0),
        [106, 72, 54, 255],
    );
    fill_rounded_rect(
        &mut rgba,
        w,
        h,
        sc(14.0),
        sc(14.0),
        sc(cover_w - 8.0),
        sc(cover_h - 8.0),
        sc(11.0),
        [136, 95, 70, 255],
    );
    fill_rounded_rect(
        &mut rgba,
        w,
        h,
        sc(17.0),
        sc(17.0),
        sc(cover_w - 14.0),
        sc(cover_h - 14.0),
        sc(9.0),
        [114, 78, 58, 255],
    );

    // 3. Stacked paper edges (3D thickness along outer left/right/bottom)
    fill_rounded_rect(
        &mut rgba,
        w,
        h,
        sc(22.0),
        sc(22.0),
        sc(lw - 44.0),
        sc(lh - 46.0),
        sc(6.0),
        [218, 207, 188, 255],
    );
    fill_rounded_rect(
        &mut rgba,
        w,
        h,
        sc(25.0),
        sc(21.0),
        sc(lw - 50.0),
        sc(lh - 50.0),
        sc(5.0),
        [235, 226, 209, 255],
    );

    // 4. Open two-page cream paper spread
    // Left page
    fill_rounded_rect(
        &mut rgba,
        w,
        h,
        sc(28.0),
        sc(20.0),
        sc(left_page_w),
        sc(page_h),
        sc(4.0),
        [252, 247, 236, 255],
    );
    // Right page
    fill_rounded_rect(
        &mut rgba,
        w,
        h,
        sc(spine),
        sc(20.0),
        sc(right_page_w),
        sc(page_h),
        sc(4.0),
        [250, 244, 232, 255],
    );

    // 5. Center spine crease & realistic page-gutter shading
    let spine_x = sc(spine);
    let shade_span = sc(24.0).max(6);
    for dx in -shade_span..=shade_span {
        let dist = (dx.abs() as f32) / (shade_span as f32);
        let alpha = ((1.0 - dist).powf(2.2) * 58.0).round() as u8;
        fill_rect(
            &mut rgba,
            w,
            h,
            spine_x + dx,
            sc(20.0),
            1,
            sc(page_h),
            [92, 74, 58, alpha],
        );
    }
    // Crisp center binding line
    fill_rect(
        &mut rgba,
        w,
        h,
        spine_x,
        sc(20.0),
        sc(1.5).max(1),
        sc(page_h),
        [128, 106, 86, 175],
    );

    // 6. Cute coral-red bookmark ribbon at top center & peeking at bottom
    fill_rect(
        &mut rgba,
        w,
        h,
        sc(spine - 6.0),
        sc(10.0),
        sc(12.0),
        sc(26.0),
        [204, 72, 72, 255],
    );
    fill_rect(
        &mut rgba,
        w,
        h,
        sc(spine - 6.0),
        sc(lh - 34.0),
        sc(12.0),
        sc(24.0),
        [204, 72, 72, 255],
    );
    // Ribbon notch at bottom
    fill_rect(
        &mut rgba,
        w,
        h,
        sc(spine - 2.0),
        sc(lh - 16.0),
        sc(4.0),
        sc(6.0),
        [106, 72, 54, 255],
    );

    // 7. Horizontal ruled notebook lines on BOTH pages + subtle coral margin line
    let line_start_y = 92.0;
    let avail_h = (page_h - (line_start_y - 20.0) - 10.0).max(120.0);
    let line_step = (avail_h / 12.0).clamp(22.0, 38.0);
    let num_lines = ((avail_h / line_step).floor() as usize).clamp(6, 16);
    for i in 0..num_lines {
        let y = sc(line_start_y + i as f32 * line_step);
        // Left page ruled line
        fill_rect(
            &mut rgba,
            w,
            h,
            sc(left_rule_x),
            y,
            sc(left_rule_w),
            sc(1.0).max(1),
            [186, 206, 222, 165],
        );
        // Right page ruled line
        fill_rect(
            &mut rgba,
            w,
            h,
            sc(right_rule_x),
            y,
            sc(right_rule_w),
            sc(1.0).max(1),
            [186, 206, 222, 165],
        );
    }
    // Header double rule on both pages
    fill_rect(
        &mut rgba,
        w,
        h,
        sc(left_rule_x),
        sc(62.0),
        sc(left_rule_w),
        sc(1.5).max(1),
        [182, 152, 128, 190],
    );
    fill_rect(
        &mut rgba,
        w,
        h,
        sc(right_rule_x),
        sc(62.0),
        sc(right_rule_w),
        sc(1.5).max(1),
        [182, 152, 128, 190],
    );
    // Vertical pink margin line on left & right pages
    fill_rect(
        &mut rgba,
        w,
        h,
        sc(left_margin_x),
        sc(24.0),
        sc(1.0).max(1),
        sc(page_h - 10.0),
        [228, 162, 162, 130],
    );
    fill_rect(
        &mut rgba,
        w,
        h,
        sc(right_margin_x),
        sc(24.0),
        sc(1.0).max(1),
        sc(page_h - 10.0),
        [228, 162, 162, 130],
    );

    // 8. Close (`×`) button on top-right corner of right page
    let (cx, cy, cw, ch) = diary_close_rect(lw_u, lh_u);
    fill_rounded_rect(
        &mut rgba,
        w,
        h,
        sc(cx),
        sc(cy),
        sc(cw),
        sc(ch),
        sc(6.0),
        [232, 218, 200, 235],
    );

    // 8b. Bottom-right diagonal resize grip (3 subtle sepia hatch lines)
    let grip_br_x = lw - 32.0;
    let grip_br_y = lh - 38.0;
    for g in 0..3 {
        let len = 5.0 + g as f32 * 4.5;
        let steps = (len * s * 1.4).round().max(4.0) as i32;
        for st in 0..=steps {
            let t = st as f32 / steps as f32;
            let px = sc(grip_br_x - len * t);
            let py = sc(grip_br_y - len * (1.0 - t));
            fill_rect(&mut rgba, w, h, px, py, sc(1.4).max(1), sc(1.4).max(1), [152, 124, 98, 195]);
        }
    }

    let mut canvas = RgbaBitmap {
        width: w,
        height: h,
        data: rgba,
    };

    // Localized headers
    let (left_hdr, right_hdr, empty_left, empty_right) = match lang {
        UiLang::Ko => (
            "태스크 일람",
            "실행 결과",
            "진행 중인 다중 태스크가 없습니다.",
            "태스크가 실행되면 결과가 기록됩니다.",
        ),
        UiLang::Ja => (
            "タスク一覧",
            "実行結果",
            "実行中のマルチタスクはありません。",
            "タスクを実行すると結果が記録されます。",
        ),
        UiLang::En => (
            "Task List",
            "Execution Results",
            "No active multi-step tasks yet.",
            "Results appear here as tasks run.",
        ),
    };

    let font_boost = (lw / DIARY_LOGICAL_W as f32).clamp(0.90, 1.18);
    let title_max_chars = ((left_text_w / (12.8 * font_boost)).floor() as usize).clamp(12, 48);
    let task_max_chars = ((left_text_w / (12.4 * font_boost)).floor() as usize).clamp(14, 52);
    let result_max_chars = ((right_text_w / (11.6 * font_boost)).floor() as usize).clamp(16, 56);

    // Left page header: Diary title & left section label
    let title_disp = truncate_chars(&diary.title, title_max_chars);
    draw_text_line(
        &mut canvas,
        text_renderer,
        &format!("📓 {left_hdr} · {title_disp}"),
        sc(left_text_x),
        sc(32.0),
        left_text_w * s,
        14.0 * font_boost * s,
        [68, 46, 34, 255],
    );

    // Right page header: Execution Results + timestamp + close glyph
    let right_hdr_w = (cx - right_text_x - 8.0).max(120.0);
    draw_text_line(
        &mut canvas,
        text_renderer,
        &format!("📝 {right_hdr}  {}", diary.updated_at),
        sc(right_text_x),
        sc(32.0),
        right_hdr_w * s,
        13.5 * font_boost * s,
        [68, 46, 34, 255],
    );
    draw_text_line(
        &mut canvas,
        text_renderer,
        "×",
        sc(cx + 7.0),
        sc(cy + 2.0),
        20.0 * s,
        16.0 * s,
        [110, 74, 56, 255],
    );

    // Progress bar row on Left page (directly under header line, using ■ and □)
    let bar_str = diary.progress_bar();
    draw_text_line(
        &mut canvas,
        text_renderer,
        &bar_str,
        sc(left_text_x),
        sc(68.0),
        left_text_w * s,
        13.0 * font_boost * s,
        [46, 98, 72, 255],
    );

    // Right page sub-summary
    let done_n = diary.completed_count();
    let total_n = diary.tasks.len();
    let right_sub = match lang {
        UiLang::Ko => format!("완료 {done_n} / 전체 {total_n}건"),
        UiLang::Ja => format!("完了 {done_n} / 全 {total_n}件"),
        UiLang::En => format!("Completed {done_n} / {total_n}"),
    };
    draw_text_line(
        &mut canvas,
        text_renderer,
        &right_sub,
        sc(right_text_x),
        sc(68.0),
        right_text_w * s,
        12.5 * font_boost * s,
        [110, 92, 76, 255],
    );

    // 9. Render Tasks (Left Page) & Execution Results (Right Page) aligned to ruled lines!
    let max_visible_tasks = num_lines.saturating_sub(1).clamp(5, 12);
    if diary.tasks.is_empty() {
        draw_text_line(
            &mut canvas,
            text_renderer,
            empty_left,
            sc(left_text_x),
            sc(100.0),
            left_text_w * s,
            13.0 * font_boost * s,
            [130, 118, 106, 255],
        );
        draw_text_line(
            &mut canvas,
            text_renderer,
            empty_right,
            sc(right_text_x),
            sc(100.0),
            right_text_w * s,
            13.0 * font_boost * s,
            [130, 118, 106, 255],
        );
    } else {
        for (i, task) in diary.tasks.iter().take(max_visible_tasks).enumerate() {
            let row_y = line_start_y + i as f32 * line_step + 4.0;
            // Highlight currently running row with warm highlighter tint
            if task.status == DiaryTaskStatus::Running {
                fill_rect(
                    &mut canvas.data,
                    w,
                    h,
                    sc(left_text_x - 2.0),
                    sc(row_y - 2.0),
                    sc(left_text_w + 2.0),
                    sc((line_step - 5.0).max(18.0)),
                    [255, 232, 166, 130],
                );
                fill_rect(
                    &mut canvas.data,
                    w,
                    h,
                    sc(right_text_x - 2.0),
                    sc(row_y - 2.0),
                    sc(right_text_w + 2.0),
                    sc((line_step - 5.0).max(18.0)),
                    [255, 232, 166, 100],
                );
            }
            let left_line = format!(
                "{} {}. {}",
                task.status.checkbox(),
                task.index,
                truncate_chars(&task.title, task_max_chars)
            );
            draw_text_line(
                &mut canvas,
                text_renderer,
                &left_line,
                sc(left_text_x),
                sc(row_y),
                left_text_w * s,
                12.5 * font_boost * s,
                task.status.ink_color(),
            );

            let res_text = if !task.result.is_empty() {
                format!("{}. {}", task.index, truncate_chars(&task.result, result_max_chars))
            } else {
                match (lang, task.status) {
                    (UiLang::Ko, DiaryTaskStatus::Running) => format!("{}. 실행 중…", task.index),
                    (UiLang::Ja, DiaryTaskStatus::Running) => format!("{}. 実行中…", task.index),
                    (UiLang::En, DiaryTaskStatus::Running) => format!("{}. Running…", task.index),
                    (UiLang::Ko, DiaryTaskStatus::Done) => format!("{}. 완료", task.index),
                    (UiLang::Ja, DiaryTaskStatus::Done) => format!("{}. 完了", task.index),
                    (UiLang::En, DiaryTaskStatus::Done) => format!("{}. Done", task.index),
                    _ => format!("{}. —", task.index),
                }
            };
            draw_text_line(
                &mut canvas,
                text_renderer,
                &res_text,
                sc(right_text_x),
                sc(row_y),
                right_text_w * s,
                12.5 * font_boost * s,
                if task.result.is_empty() && task.status == DiaryTaskStatus::Pending {
                    [156, 144, 132, 255]
                } else {
                    task.status.ink_color()
                },
            );
        }
    }

    canvas
}
