#!/usr/bin/env python3
"""Generate localized HD Live2D motion & real AivisSpeech TTS demo videos for NanikaAi.

Synthesizes 3 character voice lines headlessly via AivisSpeech Engine (127.0.0.1:10101)
without playing audio through host speakers, renders synchronized 24+7 multi-part Live2D
motion + RMS-driven lip-sync once per frame, and encodes localized editions
(docs/media/nanikaai_intro_{ja,en,ko}.mp4 + *_poster.png) using ffmpeg.
"""

from __future__ import annotations

import argparse
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


def _split_script_runs(text: str) -> list[tuple[bool, str]]:
    if not text:
        return []
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
    return runs


def measure_mixed_text(d: ImageDraw.ImageDraw, text: str, size: int, bold: bool = False) -> float:
    total = 0.0
    for is_ko, chunk in _split_script_runs(text):
        font = get_font(size, bold=bold, script="ko" if is_ko else "jp")
        total += float(d.textlength(chunk, font=font))
    return total


def draw_mixed_text(
    d: ImageDraw.ImageDraw,
    xy: tuple[int, int],
    text: str,
    size: int,
    fill: tuple[int, int, int, int],
    bold: bool = False,
) -> float:
    runs = _split_script_runs(text)
    if not runs:
        return 0.0
    x, y = float(xy[0]), xy[1]
    start_x = x
    for is_ko, chunk in runs:
        font = get_font(size, bold=bold, script="ko" if is_ko else "jp")
        y_off = 2 if (is_ko and len(runs) > 1) else 0
        d.text((int(round(x)), y + y_off), chunk, font=font, fill=fill)
        x += float(d.textlength(chunk, font=font))
    return x - start_x


def draw_centered_mixed_text(
    d: ImageDraw.ImageDraw,
    box: tuple[int, int, int, int],
    text: str,
    size: int,
    fill: tuple[int, int, int, int],
    bold: bool = False,
) -> None:
    bx0, by0, bx1, by1 = box
    tw = measure_mixed_text(d, text, size, bold=bold)
    tx = int(round(bx0 + ((bx1 - bx0) - tw) * 0.5))
    ty = int(round(by0 + ((by1 - by0) - size) * 0.5)) - 1
    draw_mixed_text(d, (tx, ty), text, size, fill, bold=bold)


def _wav_bytes_to_pcm(wav_bytes: bytes) -> np.ndarray:
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
        target_len = max(1, int(round(len(samples) * SAMPLE_RATE / sr)))
        old_idx = np.linspace(0.0, len(samples) - 1, num=len(samples))
        new_idx = np.linspace(0.0, len(samples) - 1, num=target_len)
        samples = np.interp(new_idx, old_idx, samples).astype(np.float32)

    fade_n = min(int(0.012 * SAMPLE_RATE), len(samples) // 4)
    if fade_n > 0:
        ramp = np.linspace(0.0, 1.0, fade_n, dtype=np.float32)
        samples[:fade_n] *= ramp
        samples[-fade_n:] *= ramp[::-1]
    return samples


def synth_aivis_wav(speaker_id: int, text: str, out_wav: Path) -> np.ndarray:
    """Call local AivisSpeech Engine headlessly (or reuse cached WAV if unreachable)."""
    try:
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
        return _wav_bytes_to_pcm(wav_bytes)
    except Exception as exc:
        if out_wav.is_file():
            print(f"  [fallback] AivisSpeech unreachable ({exc}); reusing cached {out_wav.name}")
            return _wav_bytes_to_pcm(out_wav.read_bytes())
        raise


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


def wrap_lines(draw: ImageDraw.ImageDraw, text: str, size: int, bold: bool, max_w: int) -> list[str]:
    lines: list[str] = []
    cur = ""
    for ch in text:
        cand = cur + ch
        if measure_mixed_text(draw, cand, size, bold=bold) <= max_w or not cur:
            cur = cand
        else:
            # Break neatly at spaces for spaced languages (EN/KO)
            if " " in cur and not ("\u3000" <= ch <= "\u9fff"):
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


SUPPORTED_LANGS = ("ja", "en", "ko")

LOCALE_CONFIG: dict[str, dict] = {
    "ja": {
        "header_sub": "100% Safe Rust  ·  24+7層 Live2D  ·  ヘッドレス AivisSpeech 音声",
        "speakers": {0: "さくら (\\0)", 1: "うにゅう (\\1)"},
        "voices": {
            "clip1": "まお (ノーマル)",
            "clip2": "らせつん (標準)",
            "clip3": "コハク (ノーマル)",
        },
        "dialogues": {
            "clip1": "こんにちは！わたしは何か藍のさくら。マルチパーツ Live2D で、なめらかに動いてお話しできるよ！",
            "clip2": "わしは相棒のうにゅうや！右クリックメニューから、いつでも好きな声色に切り替えられるで！",
            "clip3": "声色をコハクに切り替えたよ！これからよろしくね！",
        },
        "voice_prefix": "声: ",
        "menu_header_voice": "[♪ 声色選択 · AivisSpeech (:10101)]",
        "menu_sak_auto": "(\\0) 声: 自動 (まお)",
        "menu_sak_mao": "(\\0) まお (あまあま)",
        "menu_sak_kohaku": "(\\0) コハク (ノーマル)",
        "menu_sak_kohaku2": "(\\0) コハク (あまあま)",
        "menu_ker_auto": "(\\1) 声: 自動 (らせつん)",
        "menu_ker_joy": "(\\1) らせつん (喜び)",
        "menu_header_engine": "[音声エンジン · TTS Engine]",
        "menu_eng_auto": "自動 (AivisSpeech→VOICEVOX→OS)",
        "card_title_switched": "★ 声色切り替え完了！",
        "card_title_default": "★ Live2D ＆ 音声機能ハイライト",
        "bullets": [
            "右クリック [声色選択] から即座に切替",
            "さくら(\\0) 24層・うにゅう(\\1) 7層独立リグ",
            "音声波形同期リップシンク・自然な瞬き",
            "AivisSpeech / VOICEVOX 自動起動＆保存",
        ],
        "active_prefix": "現在の選択: ",
        "status_title": "★ リアルタイム状態",
        "status_keys": ("エンジン", "さくら", "うにゅう", "装飾", "表情"),
        "acc_on": "★ 星ピン・王冠 ON",
        "acc_off": "標準衣装",
        "expr_switched": "笑顔 (s5) + 発話",
        "expr_default": "通常 + 発話リップシンク",
        "input_placeholder": "▶ Communicate · さくらをダブルクリック、またはここから入力...",
        "send_label": "↑ 送信",
        "dock_badge_top": "JA",
        "dock_badge_sub": "解説",
        "dock_notes": {
            "clip1": "24+7層マルチパーツ Live2D の滑らかな揺れと音声同期リップシンク",
            "clip2": "右クリックメニューから AivisSpeech 声色をリアルタイム切替",
            "clip3": "声色「コハク」へ切替完了・アクセサリ着せ替え (bindgroup) も連動",
        },
    },
    "en": {
        "header_sub": "100% Safe Rust  ·  24+7 Layer Live2D  ·  Headless AivisSpeech TTS",
        "speakers": {0: "Sakura (\\0)", 1: "Unyu (\\1)"},
        "voices": {
            "clip1": "Mao (Normal)",
            "clip2": "Rasetsun (Std)",
            "clip3": "Kohaku (Normal)",
        },
        "dialogues": {
            "clip1": "Hi! I'm Sakura from NanikaAi — smooth 24+7 multi-part Live2D motion and real-time voice!",
            "clip2": "I'm partner Unyu! Switch character voices anytime right from the context menu!",
            "clip3": "Switched Sakura's voice to Kohaku! Nice to meet you!",
        },
        "voice_prefix": "Voice: ",
        "menu_header_voice": "[♪ Voice Selection · AivisSpeech (:10101)]",
        "menu_sak_auto": "(\\0) Voice: Auto (Mao)",
        "menu_sak_mao": "(\\0) Mao (Sweet)",
        "menu_sak_kohaku": "(\\0) Kohaku (Normal)",
        "menu_sak_kohaku2": "(\\0) Kohaku (Sweet)",
        "menu_ker_auto": "(\\1) Voice: Auto (Rasetsun)",
        "menu_ker_joy": "(\\1) Rasetsun (Happy)",
        "menu_header_engine": "[TTS Engine]",
        "menu_eng_auto": "Auto (AivisSpeech→VOICEVOX→OS)",
        "card_title_switched": "★ Voice Switched!",
        "card_title_default": "★ Live2D & Voice Highlights",
        "bullets": [
            "Switch voices live from right-click [Voice]",
            "Sakura (24 layers) & Unyu (7 layers) rig",
            "Audio-synced lip-sync & natural blinking",
            "Headless AivisSpeech / VOICEVOX auto-run",
        ],
        "active_prefix": "Active: ",
        "status_title": "★ Live Status",
        "status_keys": ("Engine", "Sakura", "Unyu", "Dress-Up", "Face"),
        "acc_on": "★ Star Pin & Crown",
        "acc_off": "Default Outfit",
        "expr_switched": "Smile (s5) + Sync",
        "expr_default": "Idle + Lip-Sync",
        "input_placeholder": "▶ Communicate · Double-click Sakura or type a message here...",
        "send_label": "↑ Send",
        "dock_badge_top": "EN",
        "dock_badge_sub": "VOICE",
        "dock_notes": {
            "clip1": "AivisSpeech Japanese Character Voice (Mao) · 24+7 Multi-Part Live2D Motion",
            "clip2": "AivisSpeech Voice (Rasetsun) · Live Right-Click Voice Switching Demo",
            "clip3": "Switched to Kohaku Voice + Live bindgroup Accessories (Star Hairpin & Crown)",
        },
    },
    "ko": {
        "header_sub": "100% Safe Rust  ·  24+7 레이어 Live2D  ·  헤드리스 AivisSpeech 음성",
        "speakers": {0: "사쿠라 (\\0)", 1: "우뉴 (\\1)"},
        "voices": {
            "clip1": "마오 (노멀)",
            "clip2": "라세츤 (표준)",
            "clip3": "코하쿠 (노멀)",
        },
        "dialogues": {
            "clip1": "안녕하세요! 나니카아이의 사쿠라예요. 24+7 멀티파츠 Live2D로 부드럽게 움직이며 대화할 수 있어요!",
            "clip2": "나는 파트너 우뉴야! 우클릭 메뉴에서 언제든 원하는 목소리로 바꿀 수 있어!",
            "clip3": "목소리를 코하쿠로 바꿨어요! 앞으로 잘 부탁해요!",
        },
        "voice_prefix": "음성: ",
        "menu_header_voice": "[♪ 목소리 선택 · AivisSpeech (:10101)]",
        "menu_sak_auto": "(\\0) 목소리: 자동 (마오)",
        "menu_sak_mao": "(\\0) 마오 (달콤)",
        "menu_sak_kohaku": "(\\0) 코하쿠 (노멀)",
        "menu_sak_kohaku2": "(\\0) 코하쿠 (달콤)",
        "menu_ker_auto": "(\\1) 목소리: 자동 (라세츤)",
        "menu_ker_joy": "(\\1) 라세츤 (기쁨)",
        "menu_header_engine": "[음성 엔진 · TTS Engine]",
        "menu_eng_auto": "자동 (AivisSpeech→VOICEVOX→OS)",
        "card_title_switched": "★ 목소리 전환 완료!",
        "card_title_default": "★ Live2D & 음성 기능 하이라이트",
        "bullets": [
            "우클릭 [목소리 선택]에서 즉시 전환",
            "사쿠라(\\0) 24파츠 · 우뉴(\\1) 7파츠 모션",
            "음성 파형 동기화 립싱크 · 자연스러운 눈깜빡임",
            "AivisSpeech / VOICEVOX 자동 구동 & 저장",
        ],
        "active_prefix": "현재 선택: ",
        "status_title": "★ 실시간 상태",
        "status_keys": ("엔진", "사쿠라", "우뉴", "장식", "표정"),
        "acc_on": "★ 별핀 · 왕관 ON",
        "acc_off": "기본 복장",
        "expr_switched": "미소 (s5) + 립싱크",
        "expr_default": "기본 + 대화 립싱크",
        "input_placeholder": "▶ Communicate · 사쿠라를 더블클릭하거나 여기에 말을 걸어 보세요...",
        "send_label": "↑ 전송",
        "dock_badge_top": "KO",
        "dock_badge_sub": "안내",
        "dock_notes": {
            "clip1": "AivisSpeech 캐릭터 음성(마오) · 24+7 멀티파츠 Live2D 스웨이 & 립싱크",
            "clip2": "AivisSpeech 캐릭터 음성(라세츤) · 우클릭 메뉴 실시간 목소리 전환 시연",
            "clip3": "코하쿠 목소리 전환 완료 · 별핀/왕관 액세서리(bindgroup) 동시 적용",
        },
    },
}


def render_ui_overlay(
    lang: str,
    t: float,
    fi: int,
    n_frames: int,
    rms: float,
    active_clip: VoiceClip | None,
    display_clip: VoiceClip,
    clips: list[VoiceClip],
    menu_start_t: float,
    switch_click_t: float,
    menu_end_t: float,
    show_acc: bool,
) -> Image.Image:
    loc = LOCALE_CONFIG[lang]
    overlay = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(overlay)

    # ── Top Header Bar ───────────────────────────────────────────────
    d.rounded_rectangle([18, 12, W - 18, 60], radius=14, fill=(18, 26, 46, 224), outline=(88, 142, 238, 120), width=1)
    draw_mixed_text(d, (34, 21), "★ 何か藍 (NanikaAi) v0.1.10", 17, (242, 247, 255, 255), bold=True)
    draw_mixed_text(d, (292, 24), loc["header_sub"], 12, (156, 184, 230, 230), bold=False)

    switched = t >= switch_click_t
    if display_clip.speaker_scope == 1:
        cur_voice = loc["voices"]["clip2"]
    else:
        cur_voice = loc["voices"]["clip3"] if switched else loc["voices"]["clip1"]
    spk_scope = display_clip.speaker_scope if active_clip is None else active_clip.speaker_scope
    cur_spk = loc["speakers"][spk_scope]

    pill_x0 = 706
    d.rounded_rectangle([pill_x0, 17, W - 26, 55], radius=11, fill=(29, 44, 78, 235), outline=(96, 165, 250, 180), width=1)
    draw_speaker_icon(d, pill_x0 + 12, 28, (125, 211, 252, 255), active=rms > 0.05)
    pill_font_size = 13 if lang == "en" else 14
    draw_mixed_text(d, (pill_x0 + 38, 27 if lang == "en" else 26), f"{cur_spk} · {cur_voice}", pill_font_size, (240, 248, 255, 255), bold=True)

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
    balloon_spk = loc["speakers"][display_clip.speaker_scope]
    spk_badge_txt = f"★ {balloon_spk}"
    spk_badge_w = max(126, int(math.ceil(measure_mixed_text(d, spk_badge_txt, 13, bold=True))) + 24)
    d.rounded_rectangle([bx0 + 16, by0 + 12, bx0 + 16 + spk_badge_w, by0 + 36], radius=9, fill=(219, 234, 254, 255), outline=(96, 165, 250, 180), width=1)
    draw_mixed_text(d, (bx0 + 28, by0 + 17), spk_badge_txt, 13, (30, 64, 155, 255), bold=True)
    draw_mixed_text(d, (bx0 + 28 + spk_badge_w, by0 + 18), f"{loc['voice_prefix']}{cur_voice}", 12, (71, 85, 105, 255), bold=False)

    full_dlg = loc["dialogues"][display_clip.clip_id]
    if t < display_clip.start_sec:
        vis_chars = 0
    elif t <= display_clip.end_sec:
        prog = min(1.0, (t - display_clip.start_sec) / max(0.35, (display_clip.end_sec - display_clip.start_sec) * 0.88))
        vis_chars = max(1, int(round(len(full_dlg) * prog)))
    else:
        vis_chars = len(full_dlg)

    shown_dlg = full_dlg[:vis_chars]
    dlg_lines = wrap_lines(d, shown_dlg, 18, True, (bx1 - bx0) - 40)
    for li, line in enumerate(dlg_lines[:4]):
        draw_mixed_text(d, (bx0 + 20, by0 + 50 + li * 28), line, 18, (19, 28, 52, 255), bold=True)

    # ── Right-Click Voice Selection Menu Card (Shows Live Switch) ───
    mx0, my0, mx1, my1 = 592, 254, 984, 530
    menu_active = menu_start_t <= t <= menu_end_t
    if menu_active:
        d.rounded_rectangle([mx0, my0, mx1, my1], radius=13, fill=(20, 27, 44, 246), outline=(96, 156, 248, 225), width=2)
        menu_items = [
            ("hdr", loc["menu_header_voice"]),
            ("sak_auto", f"{'✓' if not switched else ' '} {loc['menu_sak_auto']}"),
            ("sak_mao", f"   {loc['menu_sak_mao']}"),
            ("sak_kohaku", f"{'✓' if switched else ' '} {loc['menu_sak_kohaku']}"),
            ("sak_kohaku2", f"   {loc['menu_sak_kohaku2']}"),
            ("ker_auto", f"✓ {loc['menu_ker_auto']}"),
            ("ker_joy", f"   {loc['menu_ker_joy']}"),
            ("hdr", loc["menu_header_engine"]),
            ("eng_auto", f"✓ {loc['menu_eng_auto']}"),
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
        card_title = loc["card_title_switched"] if switched else loc["card_title_default"]
        draw_mixed_text(d, (mx0 + 18, my0 + 18), card_title, 14, (147, 205, 255, 255), bold=True)
        bullets = [( "✓", txt) for txt in loc["bullets"]] + [
            ("✓", f"{loc['active_prefix']}{cur_spk} → {cur_voice}"),
        ]
        for bi, (mk, txt) in enumerate(bullets):
            by_row = my0 + 52 + bi * 38
            d.rounded_rectangle([mx0 + 14, by_row, mx1 - 14, by_row + 30], radius=7, fill=(28, 41, 68, 200))
            draw_mixed_text(d, (mx0 + 24, by_row + 7), f"{mk}  {txt}", 13, (228, 238, 255, 255), bold=False)

    # ── Rightmost Spec Card (x=996..W-24) ───────────────────────────
    sx0, sx1 = 996, W - 24
    d.rounded_rectangle([sx0, my0, sx1, my1], radius=13, fill=(20, 29, 48, 215), outline=(74, 118, 200, 145), width=1)
    draw_mixed_text(d, (sx0 + 14, my0 + 18), loc["status_title"], 14, (147, 205, 255, 255), bold=True)
    k0, k1, k2, k3, k4 = loc["status_keys"]
    status_items = [
        (k0, "AivisSpeech (:10101)"),
        (k1, loc["voices"]["clip3"] if switched else loc["voices"]["clip1"]),
        (k2, loc["voices"]["clip2"]),
        (k3, loc["acc_on"] if show_acc else loc["acc_off"]),
        (k4, loc["expr_switched"] if switched else loc["expr_default"]),
    ]
    for si_idx, (k_lbl, v_lbl) in enumerate(status_items):
        sy_r = my0 + 52 + si_idx * 40
        draw_mixed_text(d, (sx0 + 14, sy_r), k_lbl, 12, (125, 178, 255, 235), bold=True)
        draw_mixed_text(d, (sx0 + 14, sy_r + 16), v_lbl, 13, (240, 246, 255, 255), bold=False)

    # ── Siri-style Communicate Input Bar Preview ────────────────────
    cb0, cy0, cb1, cy1 = 592, 542, W - 24, 588
    d.rounded_rectangle([cb0, cy0, cb1, cy1], radius=13, fill=(21, 31, 56, 236), outline=(59, 130, 246, 215), width=2)
    draw_mixed_text(d, (cb0 + 18, cy0 + 14), loc["input_placeholder"], 13, (195, 216, 248, 225), bold=False)
    send_box = (cb1 - 76, cy0 + 7, cb1 - 10, cy1 - 7)
    d.rounded_rectangle(list(send_box), radius=9, fill=(37, 99, 235, 240))
    draw_centered_mixed_text(d, send_box, loc["send_label"], 12, (255, 255, 255, 255), bold=True)

    # ── Bottom Localized Dialogue & Feature Note Dock + Progress Bar ─
    d.rounded_rectangle([18, 608, W - 18, H - 14], radius=14, fill=(11, 17, 32, 238), outline=(79, 140, 255, 165), width=1)
    top_badge = (32, 621, 84, 643)
    d.rounded_rectangle(list(top_badge), radius=5, fill=(37, 99, 235, 235))
    draw_centered_mixed_text(d, top_badge, loc["dock_badge_top"], 12, (255, 255, 255, 255), bold=True)
    dock_top_size = 17 if lang in ("en", "ko") else 18
    draw_mixed_text(d, (94, 621), full_dlg, dock_top_size, (245, 250, 255, 255), bold=True)

    sub_badge = (32, 653, 84, 675)
    d.rounded_rectangle(list(sub_badge), radius=5, fill=(51, 65, 95, 235))
    draw_centered_mixed_text(d, sub_badge, loc["dock_badge_sub"], 11, (203, 222, 252, 255), bold=True)
    draw_mixed_text(d, (94, 655), loc["dock_notes"][display_clip.clip_id], 14, (168, 198, 246, 245), bold=False)

    px0, py0, px1, py1 = 34, H - 28, W - 34, H - 22
    d.rounded_rectangle([px0, py0, px1, py1], radius=3, fill=(40, 56, 88, 200))
    fill_w = max(6, int(round((px1 - px0) * (fi / max(1, n_frames - 1)))))
    d.rounded_rectangle([px0, py0, px0 + fill_w, py1], radius=3, fill=(79, 162, 255, 255))

    return overlay


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Generate localized NanikaAi HD intro demo MP4 + poster images.")
    parser.add_argument(
        "--langs",
        default="ja,en,ko",
        help="Comma-separated language codes to render (subset of: ja,en,ko). Default: ja,en,ko",
    )
    parser.add_argument(
        "--audit-dir",
        default="",
        help="Optional directory to write 1 closed-menu highlight card audit PNG per language at t=4.0s.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    langs = [x.strip().lower() for x in args.langs.split(",") if x.strip()]
    if not langs:
        raise ValueError("No languages specified via --langs")
    for lang in langs:
        if lang not in SUPPORTED_LANGS:
            raise ValueError(f"Unsupported language '{lang}'. Expected subset of {SUPPORTED_LANGS}")
    audit_dir = Path(args.audit_dir) if args.audit_dir else None
    if audit_dir is not None:
        audit_dir.mkdir(parents=True, exist_ok=True)

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

    menu_start_t = clips[1].start_sec + 0.85
    switch_click_t = clips[1].end_sec + menu_switch_sec * 0.52
    menu_end_t = clips[2].start_sec + 1.75

    poster_target_frame = int(round((clips[2].start_sec + 1.05) * FPS))
    audit_target_frame = int(round(4.0 * FPS))

    # Launch one ffmpeg pipe per language so Live2D rendering runs only once per frame
    procs: dict[str, tuple[Path, subprocess.Popen[bytes]]] = {}
    for lang in langs:
        mp4_path = MEDIA_DIR / f"nanikaai_intro_{lang}.mp4"
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
        proc = subprocess.Popen(ffmpeg_cmd, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        assert proc.stdin is not None
        procs[lang] = (mp4_path, proc)

    print(f"Rendering synchronized Live2D once per frame for editions {langs}...")
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

        base_canvas = bg_rgba.copy()
        sak_resized = sak_img.resize((272, 560), Image.Resampling.LANCZOS)
        base_canvas.alpha_composite(sak_resized, (62, 62))
        uny_resized = uny_img.resize((220, 232), Image.Resampling.LANCZOS)
        base_canvas.alpha_composite(uny_resized, (342, 368))

        for lang in langs:
            overlay = render_ui_overlay(
                lang=lang,
                t=t,
                fi=fi,
                n_frames=n_frames,
                rms=rms,
                active_clip=active_clip,
                display_clip=display_clip,
                clips=clips,
                menu_start_t=menu_start_t,
                switch_click_t=switch_click_t,
                menu_end_t=menu_end_t,
                show_acc=show_acc,
            )
            frame_canvas = base_canvas.copy()
            frame_canvas.alpha_composite(overlay)
            rgb_img = frame_canvas.convert("RGB")
            mp4_path, proc = procs[lang]
            assert proc.stdin is not None
            proc.stdin.write(rgb_img.tobytes())

            if fi == poster_target_frame:
                poster_path = MEDIA_DIR / f"nanikaai_intro_{lang}_poster.png"
                rgb_img.save(poster_path, optimize=True)
            if audit_dir is not None and fi == audit_target_frame:
                rgb_img.save(audit_dir / f"audit_intro_{lang}_card.png", optimize=True)

    for lang in langs:
        mp4_path, proc = procs[lang]
        assert proc.stdin is not None
        proc.stdin.close()
        stderr_out = proc.stderr.read().decode("utf-8", errors="replace") if proc.stderr else ""
        rc = proc.wait()
        if rc != 0:
            raise RuntimeError(f"ffmpeg ({lang}) exited with code {rc}:\n{stderr_out}")
        poster_path = MEDIA_DIR / f"nanikaai_intro_{lang}_poster.png"
        print(f"SAVED [{lang}] MP4:    {mp4_path} ({mp4_path.stat().st_size // 1024} KB)")
        print(f"SAVED [{lang}] POSTER: {poster_path}")


if __name__ == "__main__":
    main()
