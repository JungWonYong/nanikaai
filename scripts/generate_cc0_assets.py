#!/usr/bin/env python3
"""Generate clean CC0-1.0 HiDPI (@2x) desktop mascot, balloon, and icon assets.

Uses Pillow exclusively (no numpy required) to:
1. Extract crisp, halo-free RGBA sprites for ``neo_default`` from optional
   ``--raw-dir`` chroma-keyed renders (#00FF00 green or #FF00FF magenta) via
   connected flood-fill + 3px chroma despill + feathered face grafting onto
   pixel-locked full-body bases (with uncut feet/base), plus true classic
   periwinkle (#94A3FF) palette calibration for Unyu (\1) preserving 2.5D depth.
2. Procedurally render modern frosted-glass HiDPI (@2x) balloons
   (``clean_default``) with soft Gaussian shadows and seamless side tails.
3. Composite the twin-mascot rounded emblem ``assets/icon.png``.
"""

from __future__ import annotations

import argparse
from collections import deque
from pathlib import Path
import statistics
from typing import Iterable

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
SHELL_DIR = ROOT / "assets" / "shells" / "neo_default"
BALLOON_DIR = ROOT / "assets" / "balloons" / "clean_default"
ICON_PATH = ROOT / "assets" / "icon.png"

CC0_NOTICE = """\
CC0 1.0 Universal (Public Domain Dedication)

All artwork, surface graphics, balloon frames, and desktop mascot definitions
in this directory were originally created for the 何か藍 (NanikaAi) project and
are dedicated cleanly to the public domain under CC0-1.0:
https://creativecommons.org/publicdomain/zero/1.0/
"""

# Sakura canvas: 660 x 1350 px (@2x -> 330 x 675 pt logical, 1.5x balanced with Unyu)
SAKURA_CANVAS = (660, 1350)
SAKURA_TARGET_H = 1260
SAKURA_BOTTOM_PAD = 36
# Unyu canvas: 380 x 420 px (@2x -> 190 x 210 pt logical)
UNYU_CANVAS = (380, 420)


def _find_raw_file(raw_dir: Path, prefix: str) -> Path:
    matches = (
        sorted(raw_dir.glob(f"{prefix}*.jpg"))
        + sorted(raw_dir.glob(f"{prefix}*.jpeg"))
        + sorted(raw_dir.glob(f"{prefix}*.png"))
    )
    if not matches:
        raise FileNotFoundError(f"Missing raw asset '{prefix}*' in {raw_dir}")
    return matches[-1]


def _detect_chroma_mode(rgb: Image.Image) -> str:
    """Detect #00FF00 green screen vs #FF00FF magenta screen from corner pixels."""
    w, h = rgb.size
    corners = [rgb.getpixel((4, 4)), rgb.getpixel((w - 5, 4)), rgb.getpixel((4, h - 5)), rgb.getpixel((w - 5, h - 5))]
    green_score = sum(max(0, g - max(r, b)) for r, g, b in corners)
    magenta_score = sum(max(0, min(r, b) - g) for r, g, b in corners)
    return "green" if green_score >= magenta_score else "magenta"


def _prune_small_alpha_components(alpha: Image.Image, w: int, h: int, min_px: int = 80) -> Image.Image:
    a_bytes = bytearray(alpha.tobytes())
    visited = bytearray(w * h)
    for i in range(w * h):
        if a_bytes[i] > 24 and not visited[i]:
            comp = [i]
            visited[i] = 1
            head = 0
            while head < len(comp):
                cur = comp[head]
                head += 1
                cx = cur % w
                cy = cur // w
                for nb in (
                    cur - 1 if cx > 0 else -1,
                    cur + 1 if cx + 1 < w else -1,
                    cur - w if cy > 0 else -1,
                    cur + w if cy + 1 < h else -1,
                ):
                    if nb >= 0 and not visited[nb] and a_bytes[nb] > 24:
                        visited[nb] = 1
                        comp.append(nb)
            if len(comp) < min_px:
                for px in comp:
                    a_bytes[px] = 0
    return Image.frombytes("L", (w, h), bytes(a_bytes))


def _flood_fill_exterior(seed: Image.Image, cand: Image.Image, w: int, h: int) -> Image.Image:
    seed_bytes = seed.tobytes()
    cand_bytes = cand.tobytes()
    ext = bytearray(seed_bytes)

    q: deque[int] = deque()
    for x in range(w):
        for y in (0, h - 1):
            idx = y * w + x
            if cand_bytes[idx] and not ext[idx]:
                ext[idx] = 255
                q.append(idx)
    for y in range(h):
        for x in (0, w - 1):
            idx = y * w + x
            if cand_bytes[idx] and not ext[idx]:
                ext[idx] = 255
                q.append(idx)

    for idx in range(w * h):
        if ext[idx] == 255:
            x = idx % w
            y = idx // w
            if (
                (x > 0 and cand_bytes[idx - 1] and not ext[idx - 1])
                or (x + 1 < w and cand_bytes[idx + 1] and not ext[idx + 1])
                or (y > 0 and cand_bytes[idx - w] and not ext[idx - w])
                or (y + 1 < h and cand_bytes[idx + w] and not ext[idx + w])
            ):
                q.append(idx)

    while q:
        idx = q.popleft()
        x = idx % w
        y = idx // w
        if x > 0 and cand_bytes[idx - 1] and not ext[idx - 1]:
            ext[idx - 1] = 255
            q.append(idx - 1)
        if x + 1 < w and cand_bytes[idx + 1] and not ext[idx + 1]:
            ext[idx + 1] = 255
            q.append(idx + 1)
        if y > 0 and cand_bytes[idx - w] and not ext[idx - w]:
            ext[idx - w] = 255
            q.append(idx - w)
        if y + 1 < h and cand_bytes[idx + w] and not ext[idx + w]:
            ext[idx + w] = 255
            q.append(idx + w)

    return Image.frombytes("L", (w, h), bytes(ext))


def _extract_chroma_rgba(im: Image.Image) -> Image.Image:
    """Remove #00FF00 green or #FF00FF magenta backdrop via flood-fill, 3px despill, and dust prune."""
    rgb = im.convert("RGB")
    w, h = rgb.size
    r, g, b = rgb.split()
    mode = _detect_chroma_mode(rgb)

    if mode == "green":
        max_rb = ImageChops.lighter(r, b)
        chroma_ex = ImageChops.subtract(g, max_rb)
        seed = ImageChops.darker(
            g.point(lambda v: 255 if v >= 135 else 0),
            chroma_ex.point(lambda v: 255 if v >= 48 else 0),
        )
        cand = ImageChops.darker(
            g.point(lambda v: 255 if v >= 90 else 0),
            chroma_ex.point(lambda v: 255 if v >= 24 else 0),
        )
    else:
        min_rb = ImageChops.darker(r, b)
        chroma_ex = ImageChops.subtract(min_rb, g)
        seed = ImageChops.darker(
            ImageChops.darker(
                r.point(lambda v: 255 if v >= 180 else 0),
                b.point(lambda v: 255 if v >= 180 else 0),
            ),
            chroma_ex.point(lambda v: 255 if v >= 105 else 0),
        )
        cand = ImageChops.darker(
            ImageChops.darker(
                r.point(lambda v: 255 if v >= 135 else 0),
                b.point(lambda v: 255 if v >= 135 else 0),
            ),
            chroma_ex.point(lambda v: 255 if v >= 45 else 0),
        )

    ext_img = _flood_fill_exterior(seed, cand, w, h)
    near_ext = ext_img.filter(ImageFilter.MaxFilter(7))  # 3px halo neighborhood
    ring = ImageChops.subtract(near_ext, ext_img)

    spill = ImageChops.darker(chroma_ex.point(lambda v: max(0, v - 8)), near_ext)
    ring_alpha = spill.point(lambda e: int(round(255.0 * max(0.0, min(1.0, 1.0 - e / 180.0)))))
    despill_amt = spill.point(lambda e: min(255, int(round(0.9 * e))))

    fg_interior = ImageChops.invert(near_ext)
    alpha = ImageChops.lighter(fg_interior, ImageChops.darker(ring, ring_alpha))
    alpha = _prune_small_alpha_components(alpha, w, h, min_px=80)

    if mode == "green":
        g_clean = ImageChops.subtract(g, despill_amt)
        return Image.merge("RGBA", (r, g_clean, b, alpha))
    r_clean = ImageChops.subtract(r, despill_amt)
    b_clean = ImageChops.subtract(b, despill_amt)
    return Image.merge("RGBA", (r_clean, g, b_clean, alpha))


def _calibrate_unyu_periwinkle(rgba: Image.Image) -> Image.Image:
    """Calibrate Unyu's periwinkle body color to classic #94A3FF (148, 163, 255) while preserving 2.5D shading."""
    buf = bytearray(rgba.tobytes())
    n = len(buf)
    rs: list[int] = []
    gs: list[int] = []
    bs: list[int] = []

    for i in range(0, n, 4):
        r, g, b, a = buf[i], buf[i + 1], buf[i + 2], buf[i + 3]
        if a > 220 and max(r, g, b) > 125 and b > g + 20 and b > r + 35:
            rs.append(r)
            gs.append(g)
            bs.append(b)

    if not rs:
        return rgba

    med_r = statistics.median(rs)
    med_g = statistics.median(gs)
    med_b = statistics.median(bs)

    target_r, target_g, target_b = 148.0, 163.0, 255.0
    if not (138 <= med_r <= 158 and 154 <= med_g <= 174 and 244 <= med_b <= 255):
        dr = target_r - med_r
        dg = target_g - med_g
        db = target_b - med_b
        for i in range(0, n, 4):
            r, g, b, a = buf[i], buf[i + 1], buf[i + 2], buf[i + 3]
            if a <= 40:
                continue
            # Smooth weight selecting ONLY periwinkle blue body pixels (excludes outlines, glasses, eyes, pink blush)
            w_blue = max(0.0, min(1.0, (b - r - 20.0) / 45.0)) * max(0.0, min(1.0, (b - g - 12.0) / 30.0))
            w_lum = max(0.0, min(1.0, (b - 135.0) / 45.0))
            # Softly preserve specular bright highlight dome while keeping midtones/shadows true
            w_hi = max(0.30, min(1.0, (240.0 - (r + g) * 0.5) / 55.0))
            w = w_blue * w_lum * w_hi
            if w > 0.01:
                buf[i] = max(0, min(255, int(round(r + dr * w))))
                buf[i + 1] = max(0, min(255, int(round(g + dg * w))))
                buf[i + 2] = max(0, min(255, int(round(b + db * w))))

    # Re-verify calibrated median
    rs2, gs2, bs2 = [], [], []
    for i in range(0, n, 4):
        r, g, b, a = buf[i], buf[i + 1], buf[i + 2], buf[i + 3]
        if a > 220 and max(r, g, b) > 125 and b > g + 20 and b > r + 35:
            rs2.append(r)
            gs2.append(g)
            bs2.append(b)
    if rs2:
        m_r, m_g, m_b = statistics.median(rs2), statistics.median(gs2), statistics.median(bs2)
        if not (138 <= m_r <= 158 and 154 <= m_g <= 174 and 244 <= m_b <= 255):
            raise RuntimeError(
                f"Unyu calibrated median RGB ({m_r:.1f}, {m_g:.1f}, {m_b:.1f}) out of periwinkle bounds"
            )
    return Image.frombytes("RGBA", rgba.size, bytes(buf))


def _align_raw_rgb(base_rgb: Image.Image, expr_rgb: Image.Image, anchor_box: tuple[int, int, int, int]) -> Image.Image:
    if expr_rgb.size != base_rgb.size:
        expr_rgb = expr_rgb.resize(base_rgb.size, Image.Resampling.LANCZOS)
    x0, y0, x1, y1 = anchor_box
    p0 = base_rgb.crop((x0, y0, x1, y1))
    best_score = 1 << 60
    best_offset = (0, 0)
    for dx in range(-4, 5):
        for dy in range(-4, 5):
            p1 = expr_rgb.crop((x0 + dx, y0 + dy, x1 + dx, y1 + dy))
            diff = ImageChops.difference(p0, p1)
            score = sum(diff.tobytes()[::3])
            if score < best_score:
                best_score = score
                best_offset = (dx, dy)
    dx, dy = best_offset
    if dx == 0 and dy == 0:
        return expr_rgb
    return expr_rgb.transform(base_rgb.size, Image.Transform.AFFINE, (1, 0, dx, 0, 1, dy), Image.Resampling.BILINEAR)


def _feathered_ellipse_mask(
    size: tuple[int, int],
    box: tuple[int, int, int, int],
    blur_radius: float,
) -> Image.Image:
    mask = Image.new("L", size, 0)
    draw = ImageDraw.Draw(mask)
    draw.ellipse(box, fill=255)
    if blur_radius > 0:
        mask = mask.filter(ImageFilter.GaussianBlur(radius=blur_radius))
    return mask


def _feathered_ellipses_mask(
    size: tuple[int, int],
    boxes: list[tuple[int, int, int, int]],
    blur_radius: float,
) -> Image.Image:
    mask = Image.new("L", size, 0)
    draw = ImageDraw.Draw(mask)
    for box in boxes:
        draw.ellipse(box, fill=255)
    if blur_radius > 0:
        mask = mask.filter(ImageFilter.GaussianBlur(radius=blur_radius))
    return mask


def _feathered_rounded_rects_mask(
    size: tuple[int, int],
    boxes: list[tuple[int, int, int, int]],
    radius: int,
    blur_radius: float,
) -> Image.Image:
    """Create a grayscale ('L') mask from rounded rectangles with Gaussian feathering."""
    mask = Image.new("L", size, 0)
    draw = ImageDraw.Draw(mask)
    for box in boxes:
        draw.rounded_rectangle(box, radius=radius, fill=255)
    if blur_radius > 0:
        mask = mask.filter(ImageFilter.GaussianBlur(radius=blur_radius))
    return mask


def _compute_placement(
    bbox: tuple[int, int, int, int],
    canvas_size: tuple[int, int],
    target_h: int,
    bottom_pad: int,
) -> tuple[tuple[int, int], tuple[int, int]]:
    """Compute scaled size (tw, th) and canvas offset (px, py) from a source bbox."""
    cw, ch = canvas_size
    x0, y0, x1, y1 = bbox
    bw, bh = x1 - x0, y1 - y0
    scale = target_h / float(bh)
    tw = max(1, int(round(bw * scale)))
    th = max(1, int(round(bh * scale)))
    px = (cw - tw) // 2
    py = ch - bottom_pad - th
    return (tw, th), (px, py)


def _frame_to_canvas(
    rgba: Image.Image,
    bbox: tuple[int, int, int, int],
    canvas_size: tuple[int, int],
    target_h: int,
    bottom_pad: int,
) -> tuple[Image.Image, float, tuple[int, int]]:
    x0, y0, x1, y1 = bbox
    cropped = rgba.crop((x0, y0, x1, y1))
    (tw, th), (px, py) = _compute_placement(bbox, canvas_size, target_h, bottom_pad)
    scale = target_h / float(max(1, y1 - y0))
    scaled = cropped.resize((tw, th), Image.Resampling.LANCZOS)
    out = Image.new("RGBA", canvas_size, (0, 0, 0, 0))
    # Direct unmasked paste preserves straight-RGBA RGB at feathered edges (no black halo).
    out.paste(scaled, (px, py))
    return out, scale, (px, py)


def _frame_mask_to_canvas(
    mask_l: Image.Image,
    bbox: tuple[int, int, int, int],
    canvas_size: tuple[int, int],
    target_h: int,
    bottom_pad: int,
) -> Image.Image:
    """Frame an 'L' mask using the exact placement geometry as _frame_to_canvas."""
    cropped = mask_l.crop(bbox)
    (tw, th), (px, py) = _compute_placement(bbox, canvas_size, target_h, bottom_pad)
    scaled = cropped.resize((tw, th), Image.Resampling.BILINEAR)
    out = Image.new("L", canvas_size, 0)
    out.paste(scaled, (px, py))
    return out


def _make_canvas_overlay(full_canvas_rgba: Image.Image, canvas_mask_l: Image.Image) -> Image.Image:
    """Build a straight-RGBA overlay directly in canvas space to eliminate alpha edge ringing."""
    r, g, b, base_a = full_canvas_rgba.split()
    eff_a = ImageChops.multiply(base_a, canvas_mask_l)
    # Zero RGB strictly where alpha == 0 for clean compression; keep pure unpremultiplied RGB where a > 0.
    zero = Image.new("L", full_canvas_rgba.size, 0)
    nz = eff_a.point(lambda v: 255 if v > 0 else 0)
    return Image.merge(
        "RGBA",
        (
            Image.composite(r, zero, nz),
            Image.composite(g, zero, nz),
            Image.composite(b, zero, nz),
            eff_a,
        ),
    )


def _match_skin_patch(
    src_rgb: Image.Image,
    ref_rgb: Image.Image,
    sample_box: tuple[int, int, int, int],
) -> Image.Image:
    """Shift light peach skin tone of src_rgb toward ref_rgb within sample_box."""
    x0, y0, x1, y1 = sample_box
    src_px = [src_rgb.getpixel((x, y)) for y in range(y0, y1) for x in range(x0, x1)]
    ref_px = [ref_rgb.getpixel((x, y)) for y in range(y0, y1) for x in range(x0, x1)]
    src_skin = [p for p in src_px if p[0] > 210 and p[0] >= p[2] + 5]
    ref_skin = [p for p in ref_px if p[0] > 210 and p[0] >= p[2] + 5]
    if not src_skin or not ref_skin:
        return src_rgb
    dr = round(statistics.median(p[0] for p in ref_skin) - statistics.median(p[0] for p in src_skin))
    dg = round(statistics.median(p[1] for p in ref_skin) - statistics.median(p[1] for p in src_skin))
    db = round(statistics.median(p[2] for p in ref_skin) - statistics.median(p[2] for p in src_skin))
    out = src_rgb.copy()
    px = out.load()
    w, h = out.size
    for y in range(h):
        for x in range(w):
            r, g, b = px[x, y]
            if r > 205 and r >= b + 4:
                px[x, y] = (
                    max(0, min(255, r + dr)),
                    max(0, min(255, g + dg)),
                    max(0, min(255, b + db)),
                )
    return out


def _build_neo_shell_from_raw(raw_dir: Path) -> None:
    SHELL_DIR.mkdir(parents=True, exist_ok=True)

    # ---- Sakura (\0) pixel-locked base + feathered face expressions ----
    sak_base_raw = Image.open(_find_raw_file(raw_dir, "sakura_base")).convert("RGB")
    sak_base_rgba = _extract_chroma_rgba(sak_base_raw)
    sak_bbox = sak_base_rgba.getbbox()
    if sak_bbox is None:
        raise RuntimeError("Sakura base extraction produced empty alpha")

    # Face mask covers eyes + eyebrows + blush + mouth inside (768, 1376) raw frame
    sak_face_mask_raw = _feathered_ellipse_mask(sak_base_raw.size, (278, 206, 480, 368), blur_radius=7.0)
    # Two-pocket blink mask covers both eyeballs/lashes strictly below bangs & eyebrows and above cheeks
    sak_blink_mask_raw = _feathered_rounded_rects_mask(
        sak_base_raw.size,
        [(296, 248, 362, 302), (392, 246, 462, 300)],
        radius=16,
        blur_radius=4.0,
    )
    # Tight mouth mask sits strictly below nose (y=308) and cheek blush (y<=318) while covering 100% of joy/wink mouths
    sak_mouth_mask_raw = _feathered_rounded_rects_mask(
        sak_base_raw.size,
        [(348, 318, 408, 355)],
        radius=12,
        blur_radius=2.5,
    )

    def graft_sakura_face(expr_rgb: Image.Image) -> Image.Image:
        a0 = sak_base_rgba.getchannel("A")
        expr_rgba = _extract_chroma_rgba(expr_rgb)
        comp_rgb = Image.composite(expr_rgba.convert("RGB"), sak_base_rgba.convert("RGB"), sak_face_mask_raw)
        comp_a = Image.composite(expr_rgba.getchannel("A"), a0, sak_face_mask_raw)
        merged = Image.merge("RGBA", (*comp_rgb.split(), comp_a))
        framed, _, _ = _frame_to_canvas(merged, sak_bbox, SAKURA_CANVAS, target_h=SAKURA_TARGET_H, bottom_pad=SAKURA_BOTTOM_PAD)
        return framed

    def map_sakura_expr(prefix: str) -> Image.Image:
        expr_raw = Image.open(_find_raw_file(raw_dir, prefix)).convert("RGB")
        aligned = _align_raw_rgb(sak_base_raw, expr_raw, (240, 460, 520, 740))
        return graft_sakura_face(aligned)

    s0, _, _ = _frame_to_canvas(sak_base_rgba, sak_bbox, SAKURA_CANVAS, target_h=SAKURA_TARGET_H, bottom_pad=SAKURA_BOTTOM_PAD)
    s_shy = map_sakura_expr("sakura_shy")
    s_surp = map_sakura_expr("sakura_surprise")
    s_troub = map_sakura_expr("sakura_troubled")
    s_joy = map_sakura_expr("sakura_joy")
    s_closed = map_sakura_expr("sakura_closed")
    s_wink = map_sakura_expr("sakura_wink")

    for fname, img in {
        "surface0.png": s0,
        "surface1.png": s_shy,
        "surface2.png": s_surp,
        "surface3.png": s_troub,
        "surface4.png": s_troub,
        "surface5.png": s_joy,
        "surface6.png": s_closed,
        "surface7.png": s_troub,
        "surface8.png": s_wink,
    }.items():
        img.save(SHELL_DIR / fname)

    # Build canvas-space Sakura blink & mouth overlays (zero 50%-blend ghosting, zero black halo)
    sak_blink_canvas_mask = _frame_mask_to_canvas(
        sak_blink_mask_raw, sak_bbox, SAKURA_CANVAS, target_h=SAKURA_TARGET_H, bottom_pad=SAKURA_BOTTOM_PAD
    )
    sak_mouth_canvas_mask = _frame_mask_to_canvas(
        sak_mouth_mask_raw, sak_bbox, SAKURA_CANVAS, target_h=SAKURA_TARGET_H, bottom_pad=SAKURA_BOTTOM_PAD
    )
    s101 = _make_canvas_overlay(s_closed, sak_blink_canvas_mask)
    s101.save(SHELL_DIR / "surface100.png")
    s101.save(SHELL_DIR / "surface101.png")

    # Blush-matched closed-eye overlay (104) for shy surface1
    shy_raw = _align_raw_rgb(
        sak_base_raw,
        Image.open(_find_raw_file(raw_dir, "sakura_shy")).convert("RGB"),
        (240, 460, 520, 740),
    )
    closed_raw = _align_raw_rgb(
        sak_base_raw,
        Image.open(_find_raw_file(raw_dir, "sakura_closed")).convert("RGB"),
        (240, 460, 520, 740),
    )
    closed_shy_tone = _match_skin_patch(closed_raw, shy_raw, (320, 286, 440, 304))
    s_shy_closed = graft_sakura_face(Image.composite(closed_shy_tone, shy_raw, sak_blink_mask_raw))
    _make_canvas_overlay(s_shy_closed, sak_blink_canvas_mask).save(SHELL_DIR / "surface104.png")

    # Open speaking mouth (102) for closed-mouth poses & closed smile mouth (103) for open-mouth poses
    _make_canvas_overlay(s_wink, sak_mouth_canvas_mask).save(SHELL_DIR / "surface102.png")
    _make_canvas_overlay(s0, sak_mouth_canvas_mask).save(SHELL_DIR / "surface103.png")

    # ---- Unyu (\1) plump periwinkle base + eyes above low sunglasses & mouth graft ----
    unyu_base_raw = Image.open(_find_raw_file(raw_dir, "unyu_base")).convert("RGB")
    unyu_base_rgba = _calibrate_unyu_periwinkle(_extract_chroma_rgba(unyu_base_raw))
    unyu_bbox = unyu_base_rgba.getbbox()
    if unyu_bbox is None:
        raise RuntimeError("Unyu base extraction produced empty alpha")

    # Covers raised eyebrows/eyes above low round sunglasses AND mouth below bridge
    unyu_face_mask = _feathered_ellipse_mask(unyu_base_raw.size, (365, 295, 792, 608), blur_radius=10.0)
    # Rounded-rect blink masks covering both eyes completely while staying strictly above sunglasses rims
    unyu_blink_mask = _feathered_rounded_rects_mask(
        unyu_base_raw.size,
        [(378, 394, 546, 468), (622, 394, 778, 468)],
        radius=16,
        blur_radius=4.5,
    )
    # Two-zone mouth mask: narrow neck between lenses above y=544 + wider bell below rims to cover surprise 'O' 100%
    unyu_mouth_mask = _feathered_rounded_rects_mask(
        unyu_base_raw.size,
        [(556, 494, 646, 544), (546, 534, 654, 606)],
        radius=16,
        blur_radius=2.8,
    )

    def map_unyu_expr(prefix: str) -> Image.Image:
        expr_raw = Image.open(_find_raw_file(raw_dir, prefix)).convert("RGB")
        aligned = _align_raw_rgb(unyu_base_raw, expr_raw, (320, 620, 760, 880))
        expr_cal = _calibrate_unyu_periwinkle(_extract_chroma_rgba(aligned))
        comp_rgb = Image.composite(expr_cal.convert("RGB"), unyu_base_rgba.convert("RGB"), unyu_face_mask)
        comp_a = Image.composite(expr_cal.getchannel("A"), unyu_base_rgba.getchannel("A"), unyu_face_mask)
        merged = Image.merge("RGBA", (*comp_rgb.split(), comp_a))
        framed, _, _ = _frame_to_canvas(merged, unyu_bbox, UNYU_CANVAS, target_h=364, bottom_pad=24)
        return framed

    s10, _, _ = _frame_to_canvas(unyu_base_rgba, unyu_bbox, UNYU_CANVAS, target_h=364, bottom_pad=24)
    s11 = map_unyu_expr("unyu_surprise")
    s12 = map_unyu_expr("unyu_smug")
    s_u_closed = map_unyu_expr("unyu_closed")
    s10.save(SHELL_DIR / "surface10.png")
    s11.save(SHELL_DIR / "surface11.png")
    s12.save(SHELL_DIR / "surface12.png")

    # Build canvas-space Unyu blink (110/111) and open/closed mouth (112/113) overlays
    unyu_blink_canvas_mask = _frame_mask_to_canvas(
        unyu_blink_mask, unyu_bbox, UNYU_CANVAS, target_h=364, bottom_pad=24
    )
    unyu_mouth_canvas_mask = _frame_mask_to_canvas(
        unyu_mouth_mask, unyu_bbox, UNYU_CANVAS, target_h=364, bottom_pad=24
    )
    s111 = _make_canvas_overlay(s_u_closed, unyu_blink_canvas_mask)
    s111.save(SHELL_DIR / "surface110.png")
    s111.save(SHELL_DIR / "surface111.png")
    _make_canvas_overlay(s12, unyu_mouth_canvas_mask).save(SHELL_DIR / "surface112.png")
    _make_canvas_overlay(s10, unyu_mouth_canvas_mask).save(SHELL_DIR / "surface113.png")

    (SHELL_DIR / "LICENSE-CC0.txt").write_text(CC0_NOTICE, encoding="utf-8")
    (SHELL_DIR / "descript.txt").write_text(
        "charset,UTF-8\n"
        "type,shell\n"
        "name,Neo Sakura & Unyu (CC0 Default)\n"
        "sakura.name,사쿠라\n"
        "kero.name,우뉴\n"
        "display.scale,2\n"
        "sakura.balloon.offsetx,38\n"
        "sakura.balloon.offsety,68\n"
        "kero.balloon.offsetx,20\n"
        "kero.balloon.offsety,28\n",
        encoding="utf-8",
    )
    (SHELL_DIR / "surfaces.txt").write_text(
        "charset,UTF-8\n"
        "version,1\n\n"
        "surface0,1,2,3,4,5,6,7,8\n{\n"
        "collision0,188,78,465,213,Head\n"
        "collision1,237,215,420,372,Face\n"
        "collision2,177,374,462,597,Bust\n"
        "}\n\n"
        "surface0,3,4,5,7\n{\n"
        "animation0.interval,sometimes\n"
        "animation0.pattern0,overlay,101,110,0,0\n"
        "animation0.pattern1,overlay,-1,50,0,0\n"
        "}\n\n"
        "surface1\n{\n"
        "animation0.interval,sometimes\n"
        "animation0.pattern0,overlay,104,110,0,0\n"
        "animation0.pattern1,overlay,-1,50,0,0\n"
        "}\n\n"
        "surface10,11,12\n{\n"
        "collision0,48,32,340,188,Head\n"
        "collision1,32,189,356,396,Body\n"
        "}\n\n"
        "surface10\n{\n"
        "animation0.interval,sometimes\n"
        "animation0.pattern0,overlay,111,110,0,0\n"
        "animation0.pattern1,overlay,-1,50,0,0\n"
        "}\n\n"
        "surface0,1,3,4,6,7\n{\n"
        "animation1.interval,talk\n"
        "animation1.pattern0,overlay,102,115,0,0\n"
        "animation1.pattern1,overlay,-1,115,0,0\n"
        "}\n\n"
        "surface2,5,8\n{\n"
        "animation1.interval,talk\n"
        "animation1.pattern0,overlay,103,115,0,0\n"
        "animation1.pattern1,overlay,-1,115,0,0\n"
        "}\n\n"
        "surface10\n{\n"
        "animation1.interval,talk\n"
        "animation1.pattern0,overlay,112,115,0,0\n"
        "animation1.pattern1,overlay,-1,115,0,0\n"
        "}\n\n"
        "surface11,12\n{\n"
        "animation1.interval,talk\n"
        "animation1.pattern0,overlay,113,115,0,0\n"
        "animation1.pattern1,overlay,-1,115,0,0\n"
        "}\n",
        encoding="utf-8",
    )


def _draw_frosted_balloon(
    target_w: int,
    target_h: int,
    side: str,
    accent_start: tuple[int, int, int],
    accent_end: tuple[int, int, int],
    out_path: Path,
    comm: bool = False,
) -> None:
    """Render an anti-aliased frosted-glass speech balloon at 2x supersampling over @2x pixels."""
    ss = 2
    cw, ch = target_w * ss, target_h * ss
    pad = 16 * ss
    tail = 26 * ss if not comm else 0
    radius = 28 * ss

    x0 = pad + (tail if side == "right" else 0)
    x1 = cw - pad - (tail if side == "left" else 0)
    y0 = pad
    y1 = ch - pad

    shadow_layer = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow_layer)
    sd.rounded_rectangle([x0 + 3 * ss, y0 + 5 * ss, x1 + 3 * ss, y1 + 5 * ss], radius=radius, fill=(22, 34, 62, 58))
    if not comm:
        ty = min(y1 - 44 * ss, y0 + 68 * ss)
        if side == "left":
            tip = (x1 + tail - 2 * ss, ty + 4 * ss)
            sd.polygon([(x1 - 4 * ss, ty - 14 * ss), tip, (x1 - 4 * ss, ty + 20 * ss)], fill=(22, 34, 62, 50))
        else:
            tip = (x0 - tail + 2 * ss, ty + 4 * ss)
            sd.polygon([(x0 + 4 * ss, ty - 14 * ss), tip, (x0 + 4 * ss, ty + 20 * ss)], fill=(22, 34, 62, 50))
    shadow_layer = shadow_layer.filter(ImageFilter.GaussianBlur(radius=7 * ss))

    card = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
    d = ImageDraw.Draw(card)
    bg = (250, 252, 255, 244)
    border = (118, 150, 206, 240)
    inner_hi = (255, 255, 255, 210)
    stroke_w = 2 * ss

    d.rounded_rectangle([x0, y0, x1, y1], radius=radius, fill=bg, outline=border, width=stroke_w)
    d.rounded_rectangle(
        [x0 + stroke_w, y0 + stroke_w, x1 - stroke_w, y1 - stroke_w],
        radius=max(4, radius - stroke_w),
        outline=inner_hi,
        width=max(1, ss),
    )

    bar_x0 = x0 + 32 * ss
    bar_x1 = x1 - 32 * ss
    bar_y = y0 + 4 * ss
    bar_h = 4 * ss
    span = max(1, bar_x1 - bar_x0)
    steps = 48
    for i in range(steps):
        t = i / float(steps - 1)
        r_c = int(round(accent_start[0] + (accent_end[0] - accent_start[0]) * t))
        g_c = int(round(accent_start[1] + (accent_end[1] - accent_start[1]) * t))
        b_c = int(round(accent_start[2] + (accent_end[2] - accent_start[2]) * t))
        sx0 = bar_x0 + int(span * i / steps)
        sx1 = bar_x0 + int(span * (i + 1) / steps) + 1
        d.rectangle([sx0, bar_y, min(sx1, bar_x1), bar_y + bar_h], fill=(r_c, g_c, b_c, 225))

    if not comm:
        ty = min(y1 - 44 * ss, y0 + 68 * ss)
        if side == "left":
            tip = (x1 + tail - 4 * ss, ty)
            top_pt = (x1 - stroke_w, ty - 14 * ss)
            bot_pt = (x1 - stroke_w, ty + 14 * ss)
            d.polygon([top_pt, tip, bot_pt], fill=bg)
            d.line([(x1, ty - 14 * ss), tip, (x1, ty + 14 * ss)], fill=border, width=stroke_w)
        else:
            tip = (x0 - tail + 4 * ss, ty)
            top_pt = (x0 + stroke_w, ty - 14 * ss)
            bot_pt = (x0 + stroke_w, ty + 14 * ss)
            d.polygon([top_pt, tip, bot_pt], fill=bg)
            d.line([(x0, ty - 14 * ss), tip, (x0, ty + 14 * ss)], fill=border, width=stroke_w)

    combined = Image.alpha_composite(shadow_layer, card)
    combined.resize((target_w, target_h), Image.Resampling.LANCZOS).save(out_path)


def _generate_balloons() -> None:
    BALLOON_DIR.mkdir(parents=True, exist_ok=True)
    sak_a0, sak_a1 = (44, 122, 245), (56, 208, 242)
    unyu_a0, unyu_a1 = (118, 134, 245), (148, 163, 255)

    _draw_frosted_balloon(720, 336, "left", sak_a0, sak_a1, BALLOON_DIR / "balloons0.png")
    _draw_frosted_balloon(720, 336, "right", sak_a0, sak_a1, BALLOON_DIR / "balloons1.png")
    _draw_frosted_balloon(780, 520, "left", sak_a0, sak_a1, BALLOON_DIR / "balloons2.png")
    _draw_frosted_balloon(780, 520, "right", sak_a0, sak_a1, BALLOON_DIR / "balloons3.png")
    _draw_frosted_balloon(660, 304, "left", unyu_a0, unyu_a1, BALLOON_DIR / "balloonk0.png")
    _draw_frosted_balloon(660, 304, "right", unyu_a0, unyu_a1, BALLOON_DIR / "balloonk1.png")
    _draw_frosted_balloon(720, 464, "left", unyu_a0, unyu_a1, BALLOON_DIR / "balloonk2.png")
    _draw_frosted_balloon(720, 464, "right", unyu_a0, unyu_a1, BALLOON_DIR / "balloonk3.png")
    _draw_frosted_balloon(720, 216, "left", sak_a0, sak_a1, BALLOON_DIR / "balloonc0.png", comm=True)

    (BALLOON_DIR / "LICENSE-CC0.txt").write_text(CC0_NOTICE, encoding="utf-8")
    (BALLOON_DIR / "descript.txt").write_text(
        "charset,UTF-8\n"
        "type,balloon\n"
        "name,Clean Glass (CC0 Default)\n"
        "display.scale,2\n"
        "origin.x,28\n"
        "origin.y,20\n"
        "wordwrappoint.x,-28\n"
        "validrect.bottom,-20\n"
        "font.height,14\n"
        "font.color.r,28\n"
        "font.color.g,36\n"
        "font.color.b,56\n"
        "anchor.font.color.r,28\n"
        "anchor.font.color.g,106\n"
        "anchor.font.color.b,224\n"
        "cursor.font.color.r,44\n"
        "cursor.font.color.g,132\n"
        "cursor.font.color.b,242\n"
        "sstpmessage.font.color.r,100\n"
        "sstpmessage.font.color.g,116\n"
        "sstpmessage.font.color.b,146\n"
        "sstpmessage.x,28\n"
        "sstpmessage.y,-18\n"
        "communicatebox.x,16\n"
        "communicatebox.y,14\n"
        "communicatebox.width,328\n"
        "communicatebox.height,24\n",
        encoding="utf-8",
    )


def _generate_app_icon() -> None:
    size = 512
    ss = 2
    cw = size * ss
    badge = Image.new("RGBA", (cw, cw), (0, 0, 0, 0))

    pad = 26 * ss
    radius = 108 * ss
    mask = Image.new("L", (cw, cw), 0)
    ImageDraw.Draw(mask).rounded_rectangle([pad, pad, cw - pad, cw - pad], radius=radius, fill=255)

    grad = Image.new("RGBA", (cw, cw), (0, 0, 0, 255))
    gd = ImageDraw.Draw(grad)
    c0, c1 = (24, 48, 120), (54, 150, 232)
    for y in range(cw):
        t = y / float(cw - 1)
        rc = int(round(c0[0] + (c1[0] - c0[0]) * t))
        gc = int(round(c0[1] + (c1[1] - c0[1]) * t))
        bc = int(round(c0[2] + (c1[2] - c0[2]) * t))
        gd.line([(0, y), (cw, y)], fill=(rc, gc, bc, 255))

    badge.paste(grad, (0, 0), mask)

    glow = Image.new("RGBA", (cw, cw), (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse([120 * ss, 80 * ss, 440 * ss, 400 * ss], fill=(140, 224, 255, 72))
    glow = glow.filter(ImageFilter.GaussianBlur(radius=38 * ss))
    badge = Image.alpha_composite(badge, glow)

    content = Image.new("RGBA", (cw, cw), (0, 0, 0, 0))
    sak_path = SHELL_DIR / "surface0.png"
    unyu_path = SHELL_DIR / "surface10.png"
    if sak_path.exists() and unyu_path.exists():
        sak = Image.open(sak_path).convert("RGBA")
        sw, sh = sak.size
        # Crop upper half bust + cute head/ahoge proportionally for any canvas scale
        sak_bust = sak.crop((int(sw * 0.155), int(sh * 0.013), int(sw * 0.855), int(sh * 0.444))).resize(
            (int(cw * 0.72), int(cw * 0.90)), Image.Resampling.LANCZOS
        )
        content.paste(sak_bust, (int(cw * 0.27), int(cw * 0.08)), sak_bust)

        unyu = Image.open(unyu_path).convert("RGBA")
        unyu_bust = unyu.crop((8, 16, 372, 396)).resize((int(cw * 0.56), int(cw * 0.58)), Image.Resampling.LANCZOS)
        content.paste(unyu_bust, (int(cw * 0.04), int(cw * 0.42)), unyu_bust)

    clipped = Image.new("RGBA", (cw, cw), (0, 0, 0, 0))
    clipped.paste(content, (0, 0), mask)
    badge = Image.alpha_composite(badge, clipped)
    ImageDraw.Draw(badge).rounded_rectangle(
        [pad, pad, cw - pad, cw - pad],
        radius=radius,
        outline=(152, 228, 255, 235),
        width=9 * ss,
    )
    badge.resize((size, size), Image.Resampling.LANCZOS).save(ICON_PATH)


def main(argv: Iterable[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description="Generate CC0-1.0 何か藍 (NanikaAi) assets.")
    parser.add_argument(
        "--raw-dir",
        type=Path,
        default=None,
        help="Optional directory containing raw chroma-keyed Sakura & Unyu base/expression renders.",
    )
    args = parser.parse_args(list(argv) if argv is not None else None)

    if args.raw_dir is not None:
        _build_neo_shell_from_raw(args.raw_dir)
    _generate_balloons()
    _generate_app_icon()
    print(f"Generated CC0 assets cleanly in {SHELL_DIR} and {BALLOON_DIR}")


if __name__ == "__main__":
    main()
