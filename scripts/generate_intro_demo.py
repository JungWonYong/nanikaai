#!/usr/bin/env python3
"""Generate an HD Live2D motion & real AivisSpeech TTS voice showcase video for NanikaAi.

Synthesizes 3 character voice lines headlessly via AivisSpeech Engine (127.0.0.1:10101)
without playing audio through host speakers, renders synchronized 24+7 multi-part Live2D
motion + RMS-driven lip-sync + speech balloons + right-click [声色選択] menu demo,
and encodes docs/media/nanikaai_intro_demo.mp4, nanikaai_intro_poster.png, and
nanikaai_intro_preview.gif using ffmpeg.
"""

from __future__ import annotations

import io
import json
import math
import subprocess
import tempfile
import urllib.parse
import urllib.request
import wave
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy.ndimage import binary_erosion, gaussian_filter

ROOT = Path(__file__).resolve().parent.parent
SHELL_DIR = ROOT / "assets" / "shells" / "neo_default"
L2D_DIR = ROOT / "assets" / "live2d" / "neo_default"
PARTS_DIR = L2D_DIR / "parts"
MEDIA_DIR = ROOT / "docs" / "media"

AIVIS_URL = "http://127.0.0.1:10101"
FPS = 30
SAMPLE_RATE = 44100
W, H = 1280, 720


@dataclass
class VoiceClip:
    clip_id: str
    speaker_scope: int  # 0 = Sakura, 1 = Unyu
    speaker_id: int
    speaker_name: str
    voice_label: str
    jp_text: str
    ko_sub: str
    en_sub: str
    sakura_expr: str
    unyu_expr: str
    accessories: bool
    wav_path: Path | None = None
    pcm: np.ndarray | None = None  # float32 mono [-1, 1] at SAMPLE_RATE
    start_sec: float = 0.0
    end_sec: float = 0.0


def find_font_paths() -> tuple[tuple[str, int, int], tuple[str, int, int]]:
    jp_candidates = [
        ("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", 0, 0),
        ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0, 0),
        ("/System/Library/Fonts/Supplemental/Arial Unicode.ttf", 0, 0),
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0, 0),
    ]
    jp_bold_path = Path("/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc")
    ko_candidates = [
        ("/System/Library/Fonts/AppleSDGothicNeo.ttc", 0, 6),
        ("/System/Library/Fonts/Supplemental/Arial Unicode.ttf", 0, 0),
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0, 0),
    ]
    jp_info = next((c for c in jp_candidates if Path(c[0]).is_file()), None)
    ko_info = next((c for c in ko_candidates if Path(c[0]).is_file()), None)
    if jp_info is None or ko_info is None:
        raise FileNotFoundError("Suitable Japanese/Korean system fonts not found")
    jp_pair = (jp_info[0], jp_info[1], str(jp_bold_path) if jp_bold_path.is_file() else jp_info[0])
    # Store (reg_path, reg_idx, bold_path, bold_idx) conceptually via helper maps
    return (jp_pair[0], 0, 0), ko_info


_JP_REG_PATH = next(
    (p for p in ("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", "/System/Library/Fonts/Hiragino Sans GB.ttc", "/System/Library/Fonts/Supplemental/Arial Unicode.ttf") if Path(p).is_file()),
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
)
_JP_BOLD_PATH = (
    "/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc"
    if Path("/System/Library/Fonts/ヒラギノ角ゴシック W6.ttc").is_file()
    else _JP_REG_PATH
)
_KO_PATH = next(
    (p for p in ("/System/Library/Fonts/AppleSDGothicNeo.ttc", "/System/Library/Fonts/Supplemental/Arial Unicode.ttf") if Path(p).is_file()),
    _JP_REG_PATH,
)
_FONT_CACHE: dict[tuple[str, int, bool], ImageFont.FreeTypeFont] = {}


def get_font(size: int, bold: bool = False, script: str = "jp") -> ImageFont.FreeTypeFont:
    key = (script, size, bold)
    if key in _FONT_CACHE:
        return _FONT_CACHE[key]
    if script == "ko":
        idx = 6 if (bold and _KO_PATH.endswith("AppleSDGothicNeo.ttc")) else 0
        try:
            f = ImageFont.truetype(_KO_PATH, size, index=idx)
        except Exception:
            f = ImageFont.truetype(_KO_PATH, size, index=0)
    else:
        path = _JP_BOLD_PATH if bold else _JP_REG_PATH
        f = ImageFont.truetype(path, size, index=0)
    _FONT_CACHE[key] = f
    return f


def _is_hangul(ch: str) -> bool:
    o = ord(ch)
    return (0xAC00 <= o <= 0xD7AF) or (0x1100 <= o <= 0x11FF) or (0x3130 <= o <= 0x318F)


def draw_mixed_text(
    d: ImageDraw.ImageDraw,
    xy: tuple[int, int],
    text: str,
    size: int,
    fill: tuple[int, int, int, int],
    bold: bool = False,
) -> None:
    if not text:
        return
    x, y = float(xy[0]), xy[1]
    runs: list[tuple[bool, str]] = []
    cur_ko = _is_hangul(text[0])
    buf = [text[0]]
    for ch in text[1:]:
        is_ko = _is_hangul(ch)
        if is_ko == cur_ko or ch == " ":
            buf.append(ch)
        else:
            runs.append((cur_ko, "".join(buf)))
            cur_ko = is_ko
            buf = [ch]
    runs.append((cur_ko, "".join(buf)))

    for is_ko, chunk in runs:
        font = get_font(size, bold=bold, script="ko" if is_ko else "jp")
        y_off = 2 if (is_ko and len(runs) > 1) else 0
        d.text((int(round(x)), y + y_off), chunk, font=font, fill=fill)
        x += float(d.textlength(chunk, font=font))


def synth_aivis_wav(speaker_id: int, text: str, out_wav: Path) -> np.ndarray:
    """Call local AivisSpeech Engine headlessly to synthesize 44.1kHz mono float32 PCM."""
    q_url = f"{AIVIS_URL}/audio_query?text={urllib.parse.quote(text)}&speaker={speaker_id}"
    req_q = urllib.request.Request(q_url, data=b"", method="POST")
    with urllib.request.urlopen(req_q, timeout=15) as resp:
        query_bytes = resp.read()

    query_obj = json.loads(query_bytes.decode("utf-8"))
    query_obj["outputSamplingRate"] = SAMPLE_RATE
    query_obj["outputStereo"] = False
    query_obj["speedScale"] = 1.03

    s_url = f"{AIVIS_URL}/synthesis?speaker={speaker_id}"
    req_s = urllib.request.Request(
        s_url,
        data=json.dumps(query_obj).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req_s, timeout=30) as resp:
        wav_bytes = resp.read()

    out_wav.write_bytes(wav_bytes)
    with wave.open(io.BytesIO(wav_bytes), "rb") as wf:
        ch = wf.getnchannels()
        sw = wf.getsampwidth()
        sr = wf.getframerate()
        raw = wf.readframes(wf.getnframes())

    if sw != 2:
        raise ValueError(f"Expected 16-bit PCM WAV, got sampwidth={sw}")
    samples = np.frombuffer(raw, dtype=np.int16).astype(np.float32) / 32768.0
    if ch > 1:
        samples = samples.reshape(-1, ch).mean(axis=1)
    if sr != SAMPLE_RATE:
        # Linear resample if engine ignored outputSamplingRate
        target_len = max(1, int(round(len(samples) * SAMPLE_RATE / sr)))
        old_idx = np.linspace(0.0, len(samples) - 1, num=len(samples))
        new_idx = np.linspace(0.0, len(samples) - 1, num=target_len)
        samples = np.interp(new_idx, old_idx, samples).astype(np.float32)

    # Gentle fade in/out 12ms to eliminate clicks
    fade_n = min(int(0.012 * SAMPLE_RATE), len(samples) // 4)
    if fade_n > 0:
        ramp = np.linspace(0.0, 1.0, fade_n, dtype=np.float32)
        samples[:fade_n] *= ramp
        samples[-fade_n:] *= ramp[::-1]
    return samples


def load_rgba(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGBA"), dtype=np.uint8)


def alpha_composite_np(dst: np.ndarray, src: np.ndarray) -> np.ndarray:
    out = dst.astype(np.float32) / 255.0
    fg = src.astype(np.float32) / 255.0
    sa = fg[:, :, 3:4]
    da = out[:, :, 3:4]
    out_a = sa + da * (1.0 - sa)
    safe_a = np.maximum(out_a, 1e-6)
    out_rgb = (fg[:, :, :3] * sa + out[:, :, :3] * da * (1.0 - sa)) / safe_a
    res = np.zeros_like(dst)
    res[:, :, :3] = np.clip(np.round(out_rgb * 255.0), 0, 255).astype(np.uint8)
    res[:, :, 3] = np.clip(np.round(out_a[:, :, 0] * 255.0), 0, 255).astype(np.uint8)
    res[res[:, :, 3] == 0, :3] = 0
    return res


def extract_unyu_face(expr_arr: np.ndarray, body_arr: np.ndarray) -> np.ndarray:
    h, w = body_arr.shape[:2]
    yy, xx = np.mgrid[0:h, 0:w]
    box = (xx >= 106) & (xx <= 332) & (yy >= 106) & (yy <= 260)
    interior = binary_erosion(body_arr[:, :, 3] > 16, iterations=8)
    diff = box & interior & (
        np.max(np.abs(expr_arr[:, :, :3].astype(np.int16) - body_arr[:, :, :3].astype(np.int16)), axis=2) > 16
    )
    alpha = np.clip(gaussian_filter(diff.astype(np.float32), sigma=0.7), 0.0, 1.0)
    out = expr_arr.copy()
    out[:, :, 3] = np.clip(np.round(out[:, :, 3].astype(np.float32) * alpha), 0, 255).astype(np.uint8)
    out[out[:, :, 3] == 0, :3] = 0
    return out


def to_premul(rgba: np.ndarray) -> tuple[np.ndarray, tuple[int, int, int, int] | None]:
    ys, xs = np.where(rgba[:, :, 3] > 0)
    sf = rgba.astype(np.float32) / 255.0
    pm = np.concatenate([sf[:, :, :3] * sf[:, :, 3:4], sf[:, :, 3:4]], axis=2)
    if len(xs) == 0:
        return pm, None
    return pm, (int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1)


def over_pm_slice(
    dst_pm: np.ndarray,
    src_pm: np.ndarray,
    bbox: tuple[int, int, int, int],
    opacity: float = 1.0,
) -> None:
    x0, y0, x1, y1 = bbox
    sub_s = src_pm if opacity >= 0.999 else src_pm * opacity
    sub_d = dst_pm[y0:y1, x0:x1]
    sa = sub_s[:, :, 3:4]
    sub_d[:, :, :] = sub_s + sub_d * (1.0 - sa)


def mat_rot(pivot: tuple[float, float], angle_deg: float, dx: float = 0.0, dy: float = 0.0) -> np.ndarray:
    rad = math.radians(angle_deg)
    c, s = math.cos(rad), math.sin(rad)
    px, py = pivot
    t_to = np.array([[1, 0, -px], [0, 1, -py], [0, 0, 1]], dtype=np.float32)
    r_m = np.array([[c, -s, 0], [s, c, 0], [0, 0, 1]], dtype=np.float32)
    t_back = np.array([[1, 0, px + dx], [0, 1, py + dy], [0, 0, 1]], dtype=np.float32)
    return t_back @ r_m @ t_to


def warp_bbox_pm(
    src_pm: np.ndarray,
    src_bbox: tuple[int, int, int, int] | None,
    world_mat: np.ndarray,
    local_pivot_y: float = 0.0,
    bend_px: float = 0.0,
) -> tuple[np.ndarray, tuple[int, int, int, int]] | None:
    if src_bbox is None:
        return None
    h, w = src_pm.shape[:2]
    sx0, sy0, sx1, sy1 = src_bbox
    if np.allclose(world_mat, np.eye(3, dtype=np.float32), atol=1e-4) and abs(bend_px) < 1e-4:
        return src_pm[sy0:sy1, sx0:sx1], (sx0, sy0, sx1, sy1)

    corners = np.array(
        [[sx0, sy0, 1.0], [sx1, sy0, 1.0], [sx0, sy1, 1.0], [sx1, sy1, 1.0]],
        dtype=np.float32,
    ).T
    wc = world_mat @ corners
    pad = int(math.ceil(abs(bend_px))) + 6
    dx0 = max(0, int(math.floor(float(wc[0].min()))) - pad)
    dy0 = max(0, int(math.floor(float(wc[1].min()))) - pad)
    dx1 = min(w, int(math.ceil(float(wc[0].max()))) + pad)
    dy1 = min(h, int(math.ceil(float(wc[1].max()))) + pad)
    if dx1 <= dx0 or dy1 <= dy0:
        return None

    inv = np.linalg.inv(world_mat)
    yy, xx = np.mgrid[dy0:dy1, dx0:dx1].astype(np.float32)
    sx = inv[0, 0] * xx + inv[0, 1] * yy + inv[0, 2]
    sy = inv[1, 0] * xx + inv[1, 1] * yy + inv[1, 2]
    if abs(bend_px) > 1e-4:
        rel = np.clip(np.abs(sy - local_pivot_y) / 90.0, 0.0, 1.0)
        sx = sx - bend_px * (rel ** 2)

    x0 = np.floor(sx).astype(np.int32)
    y0 = np.floor(sy).astype(np.int32)
    fx = (sx - x0)[:, :, None]
    fy = (sy - y0)[:, :, None]
    valid = (x0 >= 0) & (x0 < w - 1) & (y0 >= 0) & (y0 < h - 1)
    x0c = np.clip(x0, 0, w - 2)
    y0c = np.clip(y0, 0, h - 2)
    p00 = src_pm[y0c, x0c]
    p10 = src_pm[y0c, x0c + 1]
    p01 = src_pm[y0c + 1, x0c]
    p11 = src_pm[y0c + 1, x0c + 1]
    mix = (p00 * (1.0 - fx) + p10 * fx) * (1.0 - fy) + (p01 * (1.0 - fx) + p11 * fx) * fy
    mix[~valid] = 0.0
    return mix, (dx0, dy0, dx1, dy1)


def pm_to_pil_rgba(pm: np.ndarray) -> Image.Image:
    a = np.clip(pm[:, :, 3:4], 0.0, 1.0)
    rgb = np.where(a > 1e-4, pm[:, :, :3] / np.maximum(a, 1e-4), 0.0)
    u8 = np.zeros(pm.shape, dtype=np.uint8)
    u8[:, :, :3] = np.clip(np.round(rgb * 255.0), 0, 255).astype(np.uint8)
    u8[:, :, 3:4] = np.clip(np.round(a * 255.0), 0, 255).astype(np.uint8)
    return Image.fromarray(u8, mode="RGBA")


def build_background() -> Image.Image:
    yy, xx = np.mgrid[0:H, 0:W].astype(np.float32)
    # Rich dark sapphire/indigo desktop gradient with warm spotlight behind mascots
    t_y = yy / H
    r = 12.0 + 14.0 * t_y + 18.0 * np.exp(-(((xx - 285.0) / 260.0) ** 2 + ((yy - 360.0) / 260.0) ** 2))
    g = 20.0 + 18.0 * t_y + 28.0 * np.exp(-(((xx - 285.0) / 260.0) ** 2 + ((yy - 360.0) / 260.0) ** 2))
    b = 38.0 + 26.0 * t_y + 52.0 * np.exp(-(((xx - 285.0) / 260.0) ** 2 + ((yy - 360.0) / 260.0) ** 2))

    # Secondary soft ambient halo behind UI cards
    glow2 = np.exp(-(((xx - 920.0) / 340.0) ** 2 + ((yy - 260.0) / 220.0) ** 2))
    r += 8.0 * glow2
    g += 16.0 * glow2
    b += 34.0 * glow2

    # Subtle UI dot pattern
    dots = ((xx.astype(np.int32) % 28 == 0) & (yy.astype(np.int32) % 28 == 0)).astype(np.float32)
    r += 7.0 * dots
    g += 10.0 * dots
    b += 16.0 * dots

    arr = np.stack([r, g, b], axis=2)
    arr = np.clip(np.round(arr), 0, 255).astype(np.uint8)
    return Image.fromarray(arr, mode="RGB").convert("RGBA")


def wrap_lines(draw: ImageDraw.ImageDraw, text: str, font: ImageFont.FreeTypeFont, max_w: int) -> list[str]:
    lines: list[str] = []
    cur = ""
    for ch in text:
        cand = cur + ch
        if draw.textlength(cand, font=font) <= max_w or not cur:
            cur = cand
        else:
            # Break neatly at trailing spaces if English
            if " " in cur and not ("\u3000" <= ch <= "\u9fff" or "\uac00" <= ch <= "\ud7af"):
                sp = cur.rfind(" ")
                if sp > len(cur) // 2:
                    lines.append(cur[:sp])
                    cur = cur[sp + 1 :] + ch
                    continue
            lines.append(cur)
            cur = ch
    if cur:
        lines.append(cur)
    return lines


def draw_speaker_icon(d: ImageDraw.ImageDraw, x: int, y: int, color: tuple[int, int, int, int], active: bool) -> None:
    # Crisp vector speaker + sound waves (no font emoji dependency)
    d.polygon([(x, y + 5), (x + 5, y + 5), (x + 9, y + 1), (x + 9, y + 15), (x + 5, y + 11), (x, y + 11)], fill=color)
    if active:
        d.arc([x + 9, y + 3, x + 15, y + 13], start=-55, end=55, fill=color, width=2)
        d.arc([x + 10, y + 0, x + 20, y + 16], start=-55, end=55, fill=color, width=2)


def draw_cursor_arrow(d: ImageDraw.ImageDraw, x: int, y: int, pressing: bool = False) -> None:
    if pressing:
        d.ellipse([x - 12, y - 12, x + 12, y + 12], outline=(147, 197, 253, 220), width=2)
    pts = [(x, y), (x, y + 20), (x + 5, y + 15), (x + 9, y + 23), (x + 12, y + 21), (x + 8, y + 14), (x + 14, y + 14)]
    d.polygon(pts, fill=(255, 255, 255, 248), outline=(15, 23, 42, 240))


def main() -> None:
    MEDIA_DIR.mkdir(parents=True, exist_ok=True)
    audio_dir = Path(tempfile.gettempdir()) / "nanikaai_demo_audio"
    audio_dir.mkdir(parents=True, exist_ok=True)

    clips = [
        VoiceClip(
            clip_id="clip1",
            speaker_scope=0,
            speaker_id=888753760,
            speaker_name="さくら (\\0)",
            voice_label="まお (ノーマル)",
            jp_text="こんにちは！わたしは何か藍のさくら。マルチパーツ Live2D で、なめらかに動いてお話しできるよ！",
            ko_sub="안녕하세요! 나니카아이의 사쿠라예요. 24+7 멀티파츠 Live2D로 부드럽게 움직이며 대화할 수 있어요!",
            en_sub="Hi! I'm Sakura from NanikaAi — smooth 24+7 multi-part Live2D motion & real-time voice!",
            sakura_expr="s0",
            unyu_expr="s10",
            accessories=False,
        ),
        VoiceClip(
            clip_id="clip2",
            speaker_scope=1,
            speaker_id=893625024,
            speaker_name="うにゅう (\\1)",
            voice_label="らせつん (標準)",
            jp_text="わしは相棒のうにゅうや！右クリックメニューから、いつでも好きな声色に切り替えられるで！",
            ko_sub="나는 파트너 우뉴야! 우클릭 메뉴 최상단에서 언제든 원하는 목소리로 바꿀 수 있어!",
            en_sub="I'm partner Unyu! Switch character voices anytime right from the top of the context menu!",
            sakura_expr="s0",
            unyu_expr="s11",
            accessories=False,
        ),
        VoiceClip(
            clip_id="clip3",
            speaker_scope=0,
            speaker_id=1878365376,
            speaker_name="さくら (\\0)",
            voice_label="コハク (ノーマル)",
            jp_text="声色をコハクに切り替えたよ！これからよろしくね！",
            ko_sub="목소리를 코하쿠(コハク)로 바꿨어요! 앞으로 잘 부탁해요!",
            en_sub="Switched Sakura's voice to Kohaku! Nice to meet you!",
            sakura_expr="s5",
            unyu_expr="s11",
            accessories=True,
        ),
    ]

    print("Synthesizing 3 headless AivisSpeech WAV clips...")
    lead_in_sec = 0.35
    pause_12_sec = 0.40
    menu_switch_sec = 0.65
    tail_sec = 0.65

    t_cursor = lead_in_sec
    for idx, clip in enumerate(clips):
        wav_p = audio_dir / f"{clip.clip_id}.wav"
        pcm = synth_aivis_wav(clip.speaker_id, clip.jp_text, wav_p)
        clip.wav_path = wav_p
        clip.pcm = pcm
        dur = len(pcm) / SAMPLE_RATE
        clip.start_sec = t_cursor
        clip.end_sec = t_cursor + dur
        print(f"  {clip.clip_id}: speaker={clip.speaker_id} ({clip.voice_label}) dur={dur:.2f}s [{clip.start_sec:.2f}..{clip.end_sec:.2f}]")
        t_cursor = clip.end_sec + (pause_12_sec if idx == 0 else (menu_switch_sec if idx == 1 else tail_sec))

    total_sec = t_cursor
    n_samples = int(math.ceil(total_sec * SAMPLE_RATE))
    master_mono = np.zeros(n_samples, dtype=np.float32)
    for clip in clips:
        assert clip.pcm is not None
        s0_idx = int(round(clip.start_sec * SAMPLE_RATE))
        e0_idx = min(n_samples, s0_idx + len(clip.pcm))
        master_mono[s0_idx:e0_idx] += clip.pcm[: e0_idx - s0_idx]

    peak = float(np.max(np.abs(master_mono)))
    if peak > 0.95:
        master_mono *= 0.92 / peak

    # Convert to stereo 16-bit PCM WAV with gentle binaural presence
    pcm_i16 = np.clip(np.round(master_mono * 32767.0), -32768, 32767).astype(np.int16)
    stereo_i16 = np.column_stack([pcm_i16, pcm_i16]).reshape(-1)
    master_wav = audio_dir / "master_intro.wav"
    with wave.open(str(master_wav), "wb") as wf:
        wf.setnchannels(2)
        wf.setsampwidth(2)
        wf.setframerate(SAMPLE_RATE)
        wf.writeframes(stereo_i16.tobytes())

    n_frames = int(math.ceil(total_sec * FPS))
    print(f"Total timeline: {total_sec:.2f}s ({n_frames} frames @ {FPS} fps)")

    # Per-frame RMS energy & 12-band visual bar heights
    sp_frame = SAMPLE_RATE // FPS
    frame_rms = np.zeros(n_frames, dtype=np.float32)
    for fi in range(n_frames):
        a = fi * sp_frame
        b = min(n_samples, (fi + 1) * sp_frame)
        if b > a:
            seg = master_mono[a:b]
            frame_rms[fi] = float(np.sqrt(np.mean(seg * seg)))
    rms_norm = np.clip(frame_rms / max(float(np.percentile(frame_rms[frame_rms > 0.005], 90)) if np.any(frame_rms > 0.005) else 0.1, 0.05), 0.0, 1.0)

    # Load Live2D rig + premultiplied parts once
    rig = json.loads((L2D_DIR / "live2d.json").read_text(encoding="utf-8"))
    s_parts = sorted(rig["sakura"]["parts"], key=lambda p: p["z"])
    k_parts = sorted(rig["kero"]["parts"], key=lambda p: p["z"])

    fmask = load_rgba(PARTS_DIR / "sakura_face_mask.png")[:, :, 3].astype(np.float32) / 255.0
    bexcl = load_rgba(PARTS_DIR / "sakura_bang_center_excl.png")[:, :, 3].astype(np.float32) / 255.0
    fh, fw = fmask.shape
    fyy, fxx = np.mgrid[0:fh, 0:fw].astype(np.float32)
    half_w_chin = 44.0 - 20.0 * np.clip((fyy - 314.0) / 38.0, 0.0, 1.0)
    chin_trim = np.where(
        fyy <= 314.0,
        1.0,
        np.clip((half_w_chin - np.abs(fxx - 322.0)) / 6.0, 0.0, 1.0) * np.clip((357.0 - fyy) / 4.0, 0.0, 1.0),
    )
    eye_brow_mouth_box = (
        (((fxx - 278.0) / 41.0) ** 2 + ((fyy - 274.0) / 36.0) ** 2 <= 1.0)
        | (((fxx - 366.0) / 41.0) ** 2 + ((fyy - 272.0) / 36.0) ** 2 <= 1.0)
        | ((fxx >= 302.0) & (fxx <= 340.0) & (fyy >= 308.0) & (fyy <= 346.0))
    )

    def face_patch_pm(si: np.ndarray) -> tuple[np.ndarray, tuple[int, int, int, int] | None]:
        out = si.copy()
        sr = si[:, :, 0].astype(np.int16)
        sb = si[:, :, 2].astype(np.int16)
        halo_excl = (fyy >= 310.0) & (~eye_brow_mouth_box) & (sr >= 135) & ((sr - sb) < 22)
        out[:, :, 3] = np.clip(np.round(si[:, :, 3].astype(np.float32) * fmask * (1.0 - bexcl) * chin_trim), 0, 255).astype(np.uint8)
        out[halo_excl, 3] = 0
        out[out[:, :, 3] == 0, :3] = 0
        return to_premul(out)

    s0 = load_rgba(SHELL_DIR / "surface0.png")
    s5 = load_rgba(SHELL_DIR / "surface5.png")
    s101 = load_rgba(SHELL_DIR / "surface101.png")
    s102 = load_rgba(SHELL_DIR / "surface102.png")
    s200 = load_rgba(SHELL_DIR / "surface200.png")

    u_body = load_rgba(PARTS_DIR / "unyu_body.png")
    u10_clean = load_rgba(PARTS_DIR / "unyu_clean_s10.png")
    u11_clean = load_rgba(PARTS_DIR / "unyu_clean_s11.png")
    u111 = load_rgba(SHELL_DIR / "surface111.png")
    u112 = load_rgba(SHELL_DIR / "surface112.png")
    u210 = load_rgba(SHELL_DIR / "surface210.png")

    loaded_s = {p["id"]: to_premul(load_rgba(L2D_DIR / p["texture"])) for p in s_parts}
    loaded_k = {p["id"]: to_premul(load_rgba(L2D_DIR / p["texture"])) for p in k_parts}
    star_pm = to_premul(s200)
    crown_pm = to_premul(u210)

    sak_faces = {
        "s0": face_patch_pm(s0),
        "s0_talk": face_patch_pm(alpha_composite_np(s0, s102)),
        "s0_blink": face_patch_pm(alpha_composite_np(s0, s101)),
        "s5": face_patch_pm(s5),
        "s5_talk": face_patch_pm(alpha_composite_np(s5, s102)),
        "s5_blink": face_patch_pm(alpha_composite_np(s5, s101)),
    }
    unyu_faces = {
        "s10": to_premul(extract_unyu_face(u10_clean, u_body)),
        "s10_talk": to_premul(extract_unyu_face(alpha_composite_np(u10_clean, u112), u_body)),
        "s10_blink": to_premul(extract_unyu_face(alpha_composite_np(u10_clean, u111), u_body)),
        "s11": to_premul(extract_unyu_face(u11_clean, u_body)),
        "s11_talk": to_premul(extract_unyu_face(alpha_composite_np(u11_clean, u112), u_body)),
        "s11_blink": to_premul(extract_unyu_face(alpha_composite_np(u11_clean, u111), u_body)),
    }

    def render_sakura_pil(face_key: str, sway: float, breath: float, show_star: bool) -> Image.Image:
        pm = np.zeros((1350, 660, 4), dtype=np.float32)
        b_scale = breath * 1.4
        m_torso = mat_rot((322.0, 622.0), sway * 1.4, dy=-2.6 * b_scale)
        m_neck = m_torso @ mat_rot((338.0, 416.0), sway * 1.6)
        m_head = m_neck @ mat_rot((334.0, 354.0), sway * 4.2)
        face_pm, face_bbox = sak_faces[face_key]
        for p in s_parts:
            pid = p["id"]
            if pid == "face_features":
                src_pm, src_bbox = face_pm, face_bbox
            elif pid == "star_hairpin":
                if not show_star:
                    continue
                src_pm, src_bbox = star_pm
            else:
                src_pm, src_bbox = loaded_s[pid]
            if src_bbox is None:
                continue
            piv = tuple(p["pivot_px"])
            bp = p.get("breath_px", [0.0, 0.0])
            if pid in {"leg_l", "leg_r"}:
                over_pm_slice(pm, src_pm[src_bbox[1] : src_bbox[3], src_bbox[0] : src_bbox[2]], src_bbox, p.get("opacity", 1.0))
                continue
            elif pid == "upper_torso":
                res = warp_bbox_pm(src_pm, src_bbox, m_torso)
            elif pid in {"collar_back", "arm_r", "forearm_front", "chest_bow", "skirt", "coat_flap_l", "coat_flap_r"}:
                wmat = m_torso @ mat_rot(piv, sway * p["rot_deg"], dx=bp[0] * b_scale, dy=bp[1] * b_scale)
                res = warp_bbox_pm(src_pm, src_bbox, wmat, piv[1], sway * p.get("bend_px", 0.0))
            elif pid == "waist_bow":
                m_flap = m_torso @ mat_rot((374.0, 615.0), sway * 3.0)
                wmat = m_flap @ mat_rot(piv, sway * p["rot_deg"], dy=bp[1] * b_scale)
                res = warp_bbox_pm(src_pm, src_bbox, wmat, piv[1], sway * p.get("bend_px", 0.0))
            elif pid == "neck":
                res = warp_bbox_pm(src_pm, src_bbox, m_neck)
            elif pid == "head_base":
                res = warp_bbox_pm(src_pm, src_bbox, m_head)
            elif pid == "face_features":
                wmat = m_head @ np.array([[1, 0, sway * 3.2], [0, 1, 0], [0, 0, 1]], dtype=np.float32)
                res = warp_bbox_pm(src_pm, src_bbox, wmat)
            else:
                wmat = m_head @ mat_rot(piv, sway * p["rot_deg"])
                res = warp_bbox_pm(src_pm, src_bbox, wmat, piv[1], sway * p.get("bend_px", 0.0))
            if res is not None:
                over_pm_slice(pm, res[0], res[1], p.get("opacity", 1.0))
        # Crop cleanly from hair top to pleated skirt/upper thigh
        return pm_to_pil_rgba(pm[42:1060, 82:576])

    def render_unyu_pil(face_key: str, sway: float, breath: float, hop: float, show_crown: bool) -> Image.Image:
        pm = np.zeros((420, 380, 4), dtype=np.float32)
        m_body = mat_rot((190.0, 386.0), sway * 1.8, dy=-1.8 * breath - hop)
        face_pm, face_bbox = unyu_faces[face_key]
        for p in k_parts:
            pid = p["id"]
            if pid == "mochi_face":
                src_pm, src_bbox = face_pm, face_bbox
            elif pid == "crown_accessory":
                if not show_crown:
                    continue
                src_pm, src_bbox = crown_pm
            else:
                src_pm, src_bbox = loaded_k[pid]
            if src_bbox is None:
                continue
            piv = tuple(p["pivot_px"])
            if pid == "mochi_body":
                res = warp_bbox_pm(src_pm, src_bbox, m_body)
            elif pid == "mochi_face":
                wmat = m_body @ mat_rot(piv, sway * p["rot_deg"], dx=sway * 1.5)
                res = warp_bbox_pm(src_pm, src_bbox, wmat)
            elif pid == "mochi_glasses":
                m_face = m_body @ mat_rot((216.0, 188.0), sway * 2.0, dx=sway * 1.5)
                wmat = m_face @ mat_rot(piv, sway * p["rot_deg"], dx=sway * 1.5, dy=-abs(sway) * 2.0 - 0.5 * hop)
                res = warp_bbox_pm(src_pm, src_bbox, wmat)
            else:
                wmat = m_body @ mat_rot(piv, sway * p["rot_deg"])
                res = warp_bbox_pm(src_pm, src_bbox, wmat, piv[1], sway * p.get("bend_px", 0.0))
            if res is not None:
                over_pm_slice(pm, res[0], res[1], p.get("opacity", 1.0))
        return pm_to_pil_rgba(pm[18:408, 6:374])

    bg_rgba = build_background()
    f_jp = get_font(18, bold=True, script="jp")
    f_sub_ko = get_font(19, bold=True, script="ko")
    f_sub_en = get_font(14, bold=False, script="jp")
    f_small = get_font(12, bold=True, script="jp")

    menu_start_t = clips[1].start_sec + 0.85
    switch_click_t = clips[1].end_sec + menu_switch_sec * 0.52
    menu_end_t = clips[2].start_sec + 1.75

    # Launch ffmpeg pipe to stream RGB24 raw frames directly + master WAV
    mp4_path = MEDIA_DIR / "nanikaai_intro_demo.mp4"
    ffmpeg_cmd = [
        "ffmpeg",
        "-y",
        "-f",
        "rawvideo",
        "-vcodec",
        "rawvideo",
        "-pix_fmt",
        "rgb24",
        "-s",
        f"{W}x{H}",
        "-r",
        str(FPS),
        "-i",
        "-",
        "-i",
        str(master_wav),
        "-c:v",
        "libx264",
        "-preset",
        "medium",
        "-crf",
        "19",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        "-ar",
        str(SAMPLE_RATE),
        "-movflags",
        "+faststart",
        "-shortest",
        str(mp4_path),
    ]

    poster_target_frame = int(round((clips[2].start_sec + 1.05) * FPS))
    proc = subprocess.Popen(ffmpeg_cmd, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    assert proc.stdin is not None

    print("Rendering synchronized Live2D + UI frames directly to H.264/AAC MP4...")
    for fi in range(n_frames):
        t = fi / FPS
        active_clip: VoiceClip | None = None
        display_clip: VoiceClip = clips[0]
        for c in clips:
            if c.start_sec <= t <= c.end_sec:
                active_clip = c
                display_clip = c
                break
            elif t > c.end_sec:
                display_clip = c

        rms = float(rms_norm[fi])
        is_speaking = active_clip is not None and rms > 0.08
        talk_gate = is_speaking and ((fi % 5) not in (4,))

        blink_cycle = t % 3.45
        is_blink = 2.85 <= blink_cycle <= 2.98

        sak_base = "s5" if t >= clips[2].start_sec - 0.12 else display_clip.sakura_expr
        if active_clip and active_clip.speaker_scope == 0 and talk_gate:
            sak_key = f"{sak_base}_talk"
        elif is_blink:
            sak_key = f"{sak_base}_blink"
        else:
            sak_key = sak_base

        unyu_base = "s11" if t >= clips[1].start_sec - 0.1 else display_clip.unyu_expr
        if active_clip and active_clip.speaker_scope == 1 and talk_gate:
            unyu_key = f"{unyu_base}_talk"
        elif is_blink and not (active_clip and active_clip.speaker_scope == 1):
            unyu_key = f"{unyu_base}_blink"
        else:
            unyu_key = unyu_base

        sway = math.sin(t * 2.15)
        breath = math.sin(t * 1.45)
        unyu_hop = max(0.0, math.sin(t * 7.5)) * 7.0 if (active_clip and active_clip.speaker_scope == 1 and talk_gate) else 0.0
        show_acc = t >= switch_click_t

        sak_img = render_sakura_pil(sak_key, sway * 0.95, breath, show_acc)
        uny_img = render_unyu_pil(unyu_key, sway * 0.85, breath, unyu_hop, show_acc)

        canvas = bg_rgba.copy()
        sak_resized = sak_img.resize((272, 560), Image.Resampling.LANCZOS)
        canvas.alpha_composite(sak_resized, (62, 62))
        uny_resized = uny_img.resize((220, 232), Image.Resampling.LANCZOS)
        canvas.alpha_composite(uny_resized, (342, 368))

        overlay = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        d = ImageDraw.Draw(overlay)

        # ── Top Header Bar ───────────────────────────────────────────────
        d.rounded_rectangle([18, 12, W - 18, 60], radius=14, fill=(18, 26, 46, 224), outline=(88, 142, 238, 120), width=1)
        draw_mixed_text(d, (34, 21), "★ 何か藍 (NanikaAi) v0.1.10", 17, (242, 247, 255, 255), bold=True)
        draw_mixed_text(d, (292, 24), "100% Safe Rust  ·  24+7 Layer Live2D  ·  Headless AivisSpeech TTS", 12, (156, 184, 230, 230), bold=False)

        if display_clip.speaker_scope == 1:
            cur_voice = clips[1].voice_label
        else:
            cur_voice = clips[2].voice_label if t >= switch_click_t else clips[0].voice_label
        cur_spk = display_clip.speaker_name if active_clip is None else active_clip.speaker_name
        pill_x0 = 748
        d.rounded_rectangle([pill_x0, 17, W - 26, 55], radius=11, fill=(29, 44, 78, 235), outline=(96, 165, 250, 180), width=1)
        draw_speaker_icon(d, pill_x0 + 12, 28, (125, 211, 252, 255), active=rms > 0.05)
        draw_mixed_text(d, (pill_x0 + 38, 26), f"{cur_spk} · {cur_voice}", 14, (240, 248, 255, 255), bold=True)

        bar_x0 = W - 150
        for bi in range(12):
            shape = 0.45 + 0.55 * abs(math.sin(t * 14.0 + bi * 0.72))
            bh = int(round(3 + 20 * rms * shape)) if rms > 0.03 else 3
            bx = bar_x0 + bi * 9
            by1 = 44
            by0 = by1 - bh
            col = (96, 215, 255, 255) if rms > 0.05 else (100, 125, 165, 140)
            d.rounded_rectangle([bx, by0, bx + 5, by1], radius=2, fill=col)

        # ── Speech Balloon (Clean Pearl Glassmorphism) ──────────────────
        bx0, by0, bx1, by1 = 592, 78, W - 24, 240
        d.rounded_rectangle([bx0, by0, bx1, by1], radius=16, fill=(247, 250, 255, 242), outline=(112, 162, 248, 215), width=2)
        d.rounded_rectangle([bx0 + 16, by0 + 12, bx0 + 150, by0 + 36], radius=9, fill=(219, 234, 254, 255), outline=(96, 165, 250, 180), width=1)
        draw_mixed_text(d, (bx0 + 26, by0 + 17), f"★ {display_clip.speaker_name}", 13, (30, 64, 155, 255), bold=True)
        draw_mixed_text(d, (bx0 + 162, by0 + 18), f"Voice: {cur_voice}", 12, (71, 85, 105, 255), bold=False)

        if t < display_clip.start_sec:
            vis_chars = 0
        elif t <= display_clip.end_sec:
            prog = min(1.0, (t - display_clip.start_sec) / max(0.35, (display_clip.end_sec - display_clip.start_sec) * 0.88))
            vis_chars = max(1, int(round(len(display_clip.jp_text) * prog)))
        else:
            vis_chars = len(display_clip.jp_text)

        shown_jp = display_clip.jp_text[:vis_chars]
        jp_lines = wrap_lines(d, shown_jp, f_jp, (bx1 - bx0) - 40)
        for li, line in enumerate(jp_lines[:4]):
            d.text((bx0 + 20, by0 + 50 + li * 28), line, font=f_jp, fill=(19, 28, 52, 255))

        # ── Right-Click Voice Selection Menu Card (Shows Live Switch) ───
        mx0, my0, mx1, my1 = 592, 254, 984, 530
        menu_active = menu_start_t <= t <= menu_end_t
        switched = t >= switch_click_t
        if menu_active:
            d.rounded_rectangle([mx0, my0, mx1, my1], radius=13, fill=(20, 27, 44, 246), outline=(96, 156, 248, 225), width=2)
            menu_items = [
                ("hdr", "[♪ 声色選択 · AivisSpeech (:10101)]"),
                ("sak_auto", f"{'✓' if not switched else ' '} (\\0) 声: 自動 (まお)"),
                ("sak_mao", "   (\\0) まお (あまあま)"),
                ("sak_kohaku", f"{'✓' if switched else ' '} (\\0) コハク (ノーマル)"),
                ("sak_kohaku2", "   (\\0) コハク (あまあま)"),
                ("ker_auto", "✓ (\\1) 声: 自動 (らせつん)"),
                ("ker_joy", "   (\\1) らせつん (喜び)"),
                ("hdr", "[音声エンジン · TTS Engine]"),
                ("eng_auto", "✓ 自動 (AivisSpeech→VOICEVOX→OS)"),
            ]
            hover_t = min(1.0, max(0.0, (t - (menu_start_t + 0.55)) / max(0.3, (switch_click_t - menu_start_t - 0.55))))
            hover_row = 3 if hover_t >= 0.72 else (2 if hover_t >= 0.35 else 1)
            row_h = 27
            for ri, (key, lbl) in enumerate(menu_items):
                ry = my0 + 12 + ri * row_h
                if key == "hdr":
                    draw_mixed_text(d, (mx0 + 14, ry + 6), lbl, 13, (125, 185, 255, 255), bold=True)
                    continue
                if ri == hover_row or (switched and key == "sak_kohaku"):
                    fill_box = (37, 99, 235, 232) if (switched and key == "sak_kohaku") else (52, 92, 185, 210)
                    d.rounded_rectangle([mx0 + 8, ry + 1, mx1 - 8, ry + row_h - 2], radius=6, fill=fill_box, outline=(147, 197, 253, 180), width=1)
                draw_mixed_text(d, (mx0 + 16, ry + 7), lbl, 13, (244, 248, 255, 255), bold=(switched and key == "sak_kohaku"))

            cy_start = my0 + 12 + 1 * row_h + 14
            cy_end = my0 + 12 + 3 * row_h + 14
            cx_pos = int(round(mx0 + 275 + 22 * math.sin(hover_t * math.pi)))
            cy_pos = int(round(cy_start + (cy_end - cy_start) * hover_t))
            pressing = abs(t - switch_click_t) <= 0.14
            draw_cursor_arrow(d, cx_pos, cy_pos, pressing=pressing)
        else:
            d.rounded_rectangle([mx0, my0, mx1, my1], radius=13, fill=(20, 29, 48, 215), outline=(74, 118, 200, 145), width=1)
            card_title = "★ 声色切り替え完了！" if switched else "★ Live2D ＆ 音声機能ハイライト"
            draw_mixed_text(d, (mx0 + 18, my0 + 18), card_title, 14, (147, 205, 255, 255), bold=True)
            bullets = [
                ("✓", "右クリック 최상단 [声色選択]에서 즉시 변경"),
                ("✓", "さくら(\\0) 24파츠 · 우뉴(\\1) 7파츠 독립 모션"),
                ("✓", "음성 리듬 동기화 립싱크 · 자연스러운 눈깜빡임"),
                ("✓", "AivisSpeech / VOICEVOX 자동 기동 & 저장"),
                ("✓", f"현재 선택: {cur_spk} → {cur_voice}"),
            ]
            for bi, (mk, txt) in enumerate(bullets):
                by_row = my0 + 52 + bi * 38
                d.rounded_rectangle([mx0 + 14, by_row, mx1 - 14, by_row + 30], radius=7, fill=(28, 41, 68, 200))
                draw_mixed_text(d, (mx0 + 24, by_row + 7), f"{mk}  {txt}", 13, (228, 238, 255, 255), bold=False)

        # ── Rightmost Spec Card (x=996..W-24) ───────────────────────────
        sx0, sx1 = 996, W - 24
        d.rounded_rectangle([sx0, my0, sx1, my1], radius=13, fill=(20, 29, 48, 215), outline=(74, 118, 200, 145), width=1)
        draw_mixed_text(d, (sx0 + 14, my0 + 18), "★ 실시간 상태", 14, (147, 205, 255, 255), bold=True)
        status_items = [
            ("엔진", "AivisSpeech (:10101)"),
            ("사쿠라", clips[2].voice_label if switched else clips[0].voice_label),
            ("우뉴", clips[1].voice_label),
            ("장식", "★ 머리핀 · 왕관 ON" if show_acc else "기본 복장"),
            ("표정", "미소 (s5) + 립싱크" if switched else "기본 + 대화 립싱크"),
        ]
        for si_idx, (k_lbl, v_lbl) in enumerate(status_items):
            sy_r = my0 + 52 + si_idx * 40
            draw_mixed_text(d, (sx0 + 14, sy_r), k_lbl, 12, (125, 178, 255, 235), bold=True)
            draw_mixed_text(d, (sx0 + 14, sy_r + 16), v_lbl, 13, (240, 246, 255, 255), bold=False)

        # ── Siri-style Communicate Input Bar Preview ────────────────────
        cb0, cy0, cb1, cy1 = 592, 542, W - 24, 588
        d.rounded_rectangle([cb0, cy0, cb1, cy1], radius=13, fill=(21, 31, 56, 236), outline=(59, 130, 246, 215), width=2)
        draw_mixed_text(d, (cb0 + 18, cy0 + 14), "▶ Communicate · 사쿠라를 더블클릭하거나 여기에 말을 걸어 보세요...", 13, (195, 216, 248, 225), bold=False)
        d.rounded_rectangle([cb1 - 68, cy0 + 7, cb1 - 10, cy1 - 7], radius=9, fill=(37, 99, 235, 240))
        d.text((cb1 - 52, cy0 + 15), "↑ 送信", font=f_small, fill=(255, 255, 255, 255))

        # ── Bottom Dual-Language Subtitle Dock & Progress Bar ───────────
        d.rounded_rectangle([18, 608, W - 18, H - 14], radius=14, fill=(11, 17, 32, 238), outline=(79, 140, 255, 165), width=1)
        d.rounded_rectangle([32, 621, 66, 643], radius=5, fill=(37, 99, 235, 235))
        d.text((40, 625), "KO", font=f_small, fill=(255, 255, 255, 255))
        d.text((76, 620), display_clip.ko_sub, font=f_sub_ko, fill=(245, 250, 255, 255))
        d.rounded_rectangle([32, 653, 66, 673], radius=5, fill=(51, 65, 95, 235))
        d.text((41, 656), "EN", font=f_small, fill=(203, 222, 252, 255))
        d.text((76, 654), display_clip.en_sub, font=f_sub_en, fill=(168, 198, 246, 245))

        px0, py0, px1, py1 = 34, H - 28, W - 34, H - 22
        d.rounded_rectangle([px0, py0, px1, py1], radius=3, fill=(40, 56, 88, 200))
        fill_w = max(6, int(round((px1 - px0) * (fi / max(1, n_frames - 1)))))
        d.rounded_rectangle([px0, py0, px0 + fill_w, py1], radius=3, fill=(79, 162, 255, 255))

        canvas.alpha_composite(overlay)
        rgb_img = canvas.convert("RGB")
        proc.stdin.write(rgb_img.tobytes())

        if fi == poster_target_frame:
            rgb_img.save(MEDIA_DIR / "nanikaai_intro_poster.png", optimize=True)

    proc.stdin.close()
    stderr_out = proc.stderr.read().decode("utf-8", errors="replace") if proc.stderr else ""
    rc = proc.wait()
    if rc != 0:
        raise RuntimeError(f"ffmpeg exited with code {rc}:\n{stderr_out}")

    gif_path = MEDIA_DIR / "nanikaai_intro_preview.gif"
    subprocess.run(
        [
            "ffmpeg",
            "-y",
            "-i",
            str(mp4_path),
            "-vf",
            "fps=8,scale=640:-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=128:stats_mode=diff[p];[s1][p]paletteuse=dither=none",
            str(gif_path),
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )

    print(f"SAVED MP4:    {mp4_path} ({mp4_path.stat().st_size // 1024} KB)")
    print(f"SAVED GIF:    {gif_path} ({gif_path.stat().st_size // 1024} KB)")
    print(f"SAVED POSTER: {MEDIA_DIR / 'nanikaai_intro_poster.png'}")


if __name__ == "__main__":
    main()
