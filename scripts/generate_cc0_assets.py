#!/usr/bin/env python3
"""Generate clean CC0-1.0 HiDPI (@2x) desktop mascot, balloon, and icon assets.

Uses Pillow exclusively (no numpy required) to:
1. Extract crisp, halo-free RGBA sprites for ``neo_default`` from optional
   ``--raw-dir`` magenta-keyed renders via flood-fill + 3px chroma despill +
   feathered face grafting onto pixel-locked full-body bases (with full feet).
2. Procedurally render modern frosted-glass HiDPI (@2x) balloons
   (``clean_default``) with soft Gaussian shadows and seamless side tails.
3. Composite the twin-mascot rounded emblem ``assets/icon.png``.
"""

from __future__ import annotations

import argparse
from collections import deque
from pathlib import Path
from typing import Iterable

from PIL import Image, ImageChops, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
SHELL_DIR = ROOT / "assets" / "shells" / "neo_default"
BALLOON_DIR = ROOT / "assets" / "balloons" / "clean_default"
ICON_PATH = ROOT / "assets" / "icon.png"

CC0_NOTICE = """\
CC0 1.0 Universal (Public Domain Dedication)

All artwork, surface graphics, balloon frames, and desktop mascot definitions
in this directory were originally created for the AI Ukagaka project and
are dedicated cleanly to the public domain under CC0-1.0:
https://creativecommons.org/publicdomain/zero/1.0/
"""

# Sakura canvas: 400 x 960 px (@2x -> 200 x 480 pt logical)
SAKURA_CANVAS = (400, 960)
# Unyu canvas: 400 x 500 px (@2x -> 200 x 250 pt logical)
UNYU_CANVAS = (400, 500)


def _find_raw_file(raw_dir: Path, prefix: str) -> Path:
    matches = sorted(raw_dir.glob(f"{prefix}*.jpg")) + sorted(raw_dir.glob(f"{prefix}*.png"))
    if not matches:
        raise FileNotFoundError(f"Missing raw asset '{prefix}*' in {raw_dir}")
    return matches[-1]


def _extract_magenta_rgba(im: Image.Image) -> Image.Image:
    """Remove #FF00FF backdrop cleanly via connected flood-fill, spill unmix, and dust prune."""
    rgb = im.convert("RGB")
    w, h = rgb.size
    r, g, b = rgb.split()
    min_rb = ImageChops.darker(r, b)
    mag_ex = ImageChops.subtract(min_rb, g)

    # High-confidence magenta seeds catch exterior and closed inner gaps safely (skin blush has B <= G).
    seed = ImageChops.darker(
        ImageChops.darker(
            r.point(lambda v: 255 if v >= 180 else 0),
            b.point(lambda v: 255 if v >= 180 else 0),
        ),
        mag_ex.point(lambda v: 255 if v >= 105 else 0),
    )
    cand = ImageChops.darker(
        ImageChops.darker(
            r.point(lambda v: 255 if v >= 135 else 0),
            b.point(lambda v: 255 if v >= 135 else 0),
        ),
        mag_ex.point(lambda v: 255 if v >= 45 else 0),
    )

    seed_bytes = seed.tobytes()
    cand_bytes = cand.tobytes()
    ext = bytearray(seed_bytes)

    # Seed BFS only from outer border candidates or seed boundary pixels touching candidates.
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

    ext_img = Image.frombytes("L", (w, h), bytes(ext))
    near_ext = ext_img.filter(ImageFilter.MaxFilter(7))  # 3px halo neighborhood
    ring = ImageChops.subtract(near_ext, ext_img)

    # Spill ramp & channel despill only inside the narrow 3px boundary ring.
    spill = ImageChops.darker(mag_ex.point(lambda v: max(0, v - 10)), near_ext)
    ring_alpha = spill.point(lambda e: int(round(255.0 * max(0.0, min(1.0, 1.0 - e / 195.0)))))
    despill_amt = spill.point(lambda e: min(255, int(round(0.85 * e))))

    fg_interior = ImageChops.invert(near_ext)
    alpha = ImageChops.lighter(fg_interior, ImageChops.darker(ring, ring_alpha))

    # Remove faint residual dark/magenta dust speckles (< 80px connected components).
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
            if len(comp) < 80:
                for px in comp:
                    a_bytes[px] = 0
    alpha = Image.frombytes("L", (w, h), bytes(a_bytes))

    r_clean = ImageChops.subtract(r, despill_amt)
    b_clean = ImageChops.subtract(b, despill_amt)
    return Image.merge("RGBA", (r_clean, g, b_clean, alpha))


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


def _frame_to_canvas(
    rgba: Image.Image,
    bbox: tuple[int, int, int, int],
    canvas_size: tuple[int, int],
    target_h: int,
    bottom_pad: int,
) -> tuple[Image.Image, float, tuple[int, int]]:
    cw, ch = canvas_size
    x0, y0, x1, y1 = bbox
    cropped = rgba.crop((x0, y0, x1, y1))
    bw, bh = cropped.size
    scale = target_h / float(bh)
    tw = max(1, int(round(bw * scale)))
    th = max(1, int(round(bh * scale)))
    scaled = cropped.resize((tw, th), Image.Resampling.LANCZOS)
    out = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
    px = (cw - tw) // 2
    py = ch - bottom_pad - th
    out.paste(scaled, (px, py), scaled)
    return out, scale, (px, py)


def _build_neo_shell_from_raw(raw_dir: Path) -> None:
    SHELL_DIR.mkdir(parents=True, exist_ok=True)

    # ---- Sakura (\0) pixel-locked base + feathered face expressions ----
    sak_base_raw = Image.open(_find_raw_file(raw_dir, "sakura_base_")).convert("RGB")
    sak_base_rgba = _extract_magenta_rgba(sak_base_raw)
    sak_bbox = sak_base_rgba.getbbox()
    if sak_bbox is None:
        raise RuntimeError("Sakura base extraction produced empty alpha")

    sak_face_mask_raw = _feathered_ellipse_mask(sak_base_raw.size, (306, 148, 436, 276), blur_radius=6.0)
    sak_blink_mask_raw = _feathered_ellipse_mask(sak_base_raw.size, (320, 162, 422, 222), blur_radius=3.5)

    def map_sakura_expr(prefix: str) -> Image.Image:
        expr_raw = Image.open(_find_raw_file(raw_dir, prefix)).convert("RGB")
        aligned = _align_raw_rgb(sak_base_raw, expr_raw, (240, 80, 500, 170))
        # Apply expression cleanly over base canvas (RGB preserved inside alpha of base, plus local RGBA crop)
        r0, g0, b0, a0 = sak_base_rgba.split()
        expr_rgba = _extract_magenta_rgba(aligned)
        comp_rgb = Image.composite(expr_rgba.convert("RGB"), sak_base_rgba.convert("RGB"), sak_face_mask_raw)
        comp_a = Image.composite(expr_rgba.getchannel("A"), a0, sak_face_mask_raw)
        merged = Image.merge("RGBA", (*comp_rgb.split(), comp_a))
        framed, _, _ = _frame_to_canvas(merged, sak_bbox, SAKURA_CANVAS, target_h=916, bottom_pad=20)
        return framed

    s0, _, _ = _frame_to_canvas(sak_base_rgba, sak_bbox, SAKURA_CANVAS, target_h=916, bottom_pad=20)
    s_shy = map_sakura_expr("sakura_shy_")
    s_surp = map_sakura_expr("sakura_surprise_")
    s_troub = map_sakura_expr("sakura_troubled_")
    s_joy = map_sakura_expr("sakura_joy_")
    s_closed = map_sakura_expr("sakura_closed_")
    s_wink = map_sakura_expr("sakura_wink_")

    surfaces = {
        "surface0.png": s0,
        "surface1.png": s_shy,
        "surface2.png": s_surp,
        "surface3.png": s_troub,
        "surface4.png": s_troub,
        "surface5.png": s_joy,
        "surface6.png": s_closed,
        "surface7.png": s_troub,
        "surface8.png": s_wink,
    }
    for fname, img in surfaces.items():
        img.save(SHELL_DIR / fname)

    # SERIKO eye-blink overlays (100 = half blink, 101 = closed eyes only)
    closed_raw = _align_raw_rgb(
        sak_base_raw,
        Image.open(_find_raw_file(raw_dir, "sakura_closed_")).convert("RGB"),
        (240, 80, 500, 170),
    )
    closed_rgb_rgba = Image.merge("RGBA", (*closed_raw.split(), sak_blink_mask_raw))
    half_rgb = Image.blend(sak_base_raw, closed_raw, 0.5)
    half_rgb_rgba = Image.merge("RGBA", (*half_rgb.split(), sak_blink_mask_raw))

    s101, _, _ = _frame_to_canvas(closed_rgb_rgba, sak_bbox, SAKURA_CANVAS, target_h=916, bottom_pad=20)
    s100, _, _ = _frame_to_canvas(half_rgb_rgba, sak_bbox, SAKURA_CANVAS, target_h=916, bottom_pad=20)
    s100.save(SHELL_DIR / "surface100.png")
    s101.save(SHELL_DIR / "surface101.png")

    # ---- Unyu (\1) pixel-locked base + sunglasses-top eyes & mouth graft ----
    unyu_base_raw = Image.open(_find_raw_file(raw_dir, "unyu_base_")).convert("RGB")
    unyu_base_rgba = _extract_magenta_rgba(unyu_base_raw)
    unyu_bbox = unyu_base_rgba.getbbox()
    if unyu_bbox is None:
        raise RuntimeError("Unyu base extraction produced empty alpha")

    # Covers both eyes clearly visible ABOVE low round sunglasses AND mouth expression zone
    unyu_face_mask = _feathered_ellipse_mask(unyu_base_raw.size, (346, 474, 704, 706), blur_radius=7.0)

    def map_unyu_expr(prefix: str) -> Image.Image:
        expr_raw = Image.open(_find_raw_file(raw_dir, prefix)).convert("RGB")
        aligned = _align_raw_rgb(unyu_base_raw, expr_raw, (200, 720, 680, 1080))
        a0 = unyu_base_rgba.getchannel("A")
        expr_rgba = _extract_magenta_rgba(aligned)
        comp_rgb = Image.composite(expr_rgba.convert("RGB"), unyu_base_rgba.convert("RGB"), unyu_face_mask)
        comp_a = Image.composite(expr_rgba.getchannel("A"), a0, unyu_face_mask)
        merged = Image.merge("RGBA", (*comp_rgb.split(), comp_a))
        framed, _, _ = _frame_to_canvas(merged, unyu_bbox, UNYU_CANVAS, target_h=440, bottom_pad=20)
        return framed

    s10, _, _ = _frame_to_canvas(unyu_base_rgba, unyu_bbox, UNYU_CANVAS, target_h=440, bottom_pad=20)
    s10.save(SHELL_DIR / "surface10.png")
    map_unyu_expr("unyu_surprise_").save(SHELL_DIR / "surface11.png")
    map_unyu_expr("unyu_smug_").save(SHELL_DIR / "surface12.png")

    (SHELL_DIR / "LICENSE-CC0.txt").write_text(CC0_NOTICE, encoding="utf-8")
    (SHELL_DIR / "descript.txt").write_text(
        "charset,UTF-8\n"
        "type,shell\n"
        "name,Neo Sakura & Unyu (CC0 Default)\n"
        "sakura.name,사쿠라\n"
        "kero.name,우뉴\n"
        "display.scale,2\n"
        "sakura.balloon.offsetx,24\n"
        "sakura.balloon.offsety,44\n"
        "kero.balloon.offsetx,20\n"
        "kero.balloon.offsety,32\n",
        encoding="utf-8",
    )
    (SHELL_DIR / "surfaces.txt").write_text(
        "charset,UTF-8\n"
        "version,1\n\n"
        "surface0,1,2,3,4,5,6,7,8\n{\n"
        "collision0,124,24,274,132,Head\n"
        "collision1,142,133,246,204,Face\n"
        "collision2,126,205,274,410,Bust\n"
        "}\n\n"
        "surface0,1,2,3,4\n{\n"
        "animation0.interval,sometimes\n"
        "animation0.pattern0,overlay,100,60,0,0\n"
        "animation0.pattern1,overlay,101,80,0,0\n"
        "animation0.pattern2,overlay,100,60,0,0\n"
        "animation0.pattern3,overlay,-1,50,0,0\n"
        "}\n\n"
        "surface10,11,12\n{\n"
        "collision0,88,44,316,264,Head\n"
        "collision1,96,265,308,478,Body\n"
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

    # Outer bordered body
    d.rounded_rectangle([x0, y0, x1, y1], radius=radius, fill=bg, outline=border, width=stroke_w)
    # Inner subtle bright rim
    d.rounded_rectangle(
        [x0 + stroke_w, y0 + stroke_w, x1 - stroke_w, y1 - stroke_w],
        radius=max(4, radius - stroke_w),
        outline=inner_hi,
        width=max(1, ss),
    )

    # Top accent pill with smooth RGB interpolation
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

    # Seamless pointer tail pointing directly toward character
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
    unyu_a0, unyu_a1 = (72, 114, 212), (118, 168, 248)

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
    d = ImageDraw.Draw(badge)

    # Diagonal sapphire-to-cyan squircle badge
    pad = 26 * ss
    radius = 108 * ss
    mask = Image.new("L", (cw, cw), 0)
    ImageDraw.Draw(mask).rounded_rectangle([pad, pad, cw - pad, cw - pad], radius=radius, fill=255)

    grad = Image.new("RGBA", (cw, cw), (0, 0, 0, 255))
    gd = ImageDraw.Draw(grad)
    c0, c1 = (22, 46, 108), (44, 152, 232)
    for y in range(cw):
        t = y / float(cw - 1)
        rc = int(round(c0[0] + (c1[0] - c0[0]) * t))
        gc = int(round(c0[1] + (c1[1] - c0[1]) * t))
        bc = int(round(c0[2] + (c1[2] - c0[2]) * t))
        gd.line([(0, y), (cw, y)], fill=(rc, gc, bc, 255))

    badge.paste(grad, (0, 0), mask)

    # Soft cyan halo glow
    glow = Image.new("RGBA", (cw, cw), (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse([120 * ss, 80 * ss, 440 * ss, 400 * ss], fill=(120, 230, 255, 70))
    glow = glow.filter(ImageFilter.GaussianBlur(radius=38 * ss))
    badge = Image.alpha_composite(badge, glow)

    # Clip duo bust portraits neatly inside rounded squircle
    content = Image.new("RGBA", (cw, cw), (0, 0, 0, 0))
    sak_path = SHELL_DIR / "surface0.png"
    unyu_path = SHELL_DIR / "surface10.png"
    if sak_path.exists() and unyu_path.exists():
        sak = Image.open(sak_path).convert("RGBA")
        # Crop upper half bust + face from 400x960
        sak_bust = sak.crop((48, 8, 352, 410)).resize((460 * ss // 2 * 2, 608 * ss // 2 * 2), Image.Resampling.LANCZOS)
        # Scale nicely inside badge
        sak_bust = sak.crop((54, 12, 346, 360)).resize((int(cw * 0.72), int(cw * 0.85)), Image.Resampling.LANCZOS)
        content.paste(sak_bust, (int(cw * 0.25), int(cw * 0.14)), sak_bust)

        unyu = Image.open(unyu_path).convert("RGBA")
        unyu_bust = unyu.crop((20, 12, 380, 340)).resize((int(cw * 0.56), int(cw * 0.51)), Image.Resampling.LANCZOS)
        content.paste(unyu_bust, (int(cw * 0.05), int(cw * 0.45)), unyu_bust)

    # Apply rounded mask clip and outer cyan rim
    clipped = Image.new("RGBA", (cw, cw), (0, 0, 0, 0))
    clipped.paste(content, (0, 0), mask)
    badge = Image.alpha_composite(badge, clipped)
    ImageDraw.Draw(badge).rounded_rectangle(
        [pad, pad, cw - pad, cw - pad],
        radius=radius,
        outline=(140, 224, 255, 235),
        width=9 * ss,
    )
    badge.resize((size, size), Image.Resampling.LANCZOS).save(ICON_PATH)


def main(argv: Iterable[str] | None = None) -> None:
    parser = argparse.ArgumentParser(description="Generate CC0-1.0 Neo Ukagaka assets.")
    parser.add_argument(
        "--raw-dir",
        type=Path,
        default=None,
        help="Optional directory containing raw #FF00FF Sakura & Unyu base/expression renders.",
    )
    args = parser.parse_args(list(argv) if argv is not None else None)

    if args.raw_dir is not None:
        _build_neo_shell_from_raw(args.raw_dir)
    _generate_balloons()
    _generate_app_icon()
    print(f"Generated CC0 assets cleanly in {SHELL_DIR} and {BALLOON_DIR}")


if __name__ == "__main__":
    main()
