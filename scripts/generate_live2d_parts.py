#!/usr/bin/env python3
"""Generate anatomical CC0-1.0 multi-layer Live2D part PNGs and sidecar live2d.json for neo_default.

Uses strict Neumann-boundary Laplace diffusion (preventing adjacent white/outline bleed)
to separate Sakura (\0) into anatomically articulated joints (neck, C1 skull head,
floating facial features, multi-layer bangs/curls/back hair, swinging coat flaps/collar/arm,
and redesigned cute frilly ribbon socks + feminine Mary Jane dress shoes) and Unyu (\1) into
unbroken expression bases (`surface_overrides`), flapping paws, and crisp 40%-translucent
sunglasses floating independently on top.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import (
    binary_dilation,
    binary_erosion,
    binary_fill_holes,
    distance_transform_edt,
    gaussian_filter,
    label,
)

ROOT = Path(__file__).resolve().parent.parent
SHELL_DIR = ROOT / "assets" / "shells" / "neo_default"
OUT_DIR = ROOT / "assets" / "live2d" / "neo_default"
PARTS_DIR = OUT_DIR / "parts"

CC0_NOTICE = """\
CC0 1.0 Universal (Public Domain Dedication)

All layered character part images, alpha masks, and live2d.json sidecar definitions
in this directory (`assets/live2d/neo_default`) were created for the 何か藍 (NanikaAi)
project and are dedicated cleanly to the public domain under CC0-1.0:
https://creativecommons.org/publicdomain/zero/1.0/
"""


def load_rgba(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGBA"), dtype=np.uint8)


def save_rgba(arr: np.ndarray, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    out = np.array(arr, dtype=np.uint8, copy=True)
    out[out[:, :, 3] == 0, :3] = 0
    Image.fromarray(out).save(path)


def diffuse_from_known(
    rgb: np.ndarray,
    fill_mask: np.ndarray,
    known_mask: np.ndarray,
    iters: int = 160,
) -> np.ndarray:
    """Strict Neumann-boundary Laplace diffusion strictly from `known_mask` into `fill_mask`."""
    work = rgb.astype(np.float32).copy()
    if not np.any(fill_mask) or not np.any(known_mask):
        return np.clip(np.round(work), 0, 255).astype(np.uint8)

    _, (ni, nj) = distance_transform_edt(~known_mask, return_indices=True)
    for c in range(3):
        init_c = work[:, :, c][ni, nj]
        work[:, :, c][fill_mask] = init_c[fill_mask]

    valid = (known_mask | fill_mask).astype(np.float32)
    ys, xs = np.where(fill_mask)
    y0, y1 = max(1, int(ys.min()) - 2), min(rgb.shape[0] - 1, int(ys.max()) + 3)
    x0, x1 = max(1, int(xs.min()) - 2), min(rgb.shape[1] - 1, int(xs.max()) + 3)
    sub_fill = fill_mask[y0:y1, x0:x1]

    vn = valid[y0 - 1 : y1 - 1, x0:x1, None]
    vs = valid[y0 + 1 : y1 + 1, x0:x1, None]
    vw = valid[y0:y1, x0 - 1 : x1 - 1, None]
    ve = valid[y0:y1, x0 + 1 : x1 + 1, None]
    vsum = np.maximum(vn + vs + vw + ve, 1.0)

    for _ in range(iters):
        acc = (
            work[y0 - 1 : y1 - 1, x0:x1] * vn
            + work[y0 + 1 : y1 + 1, x0:x1] * vs
            + work[y0:y1, x0 - 1 : x1 - 1] * vw
            + work[y0:y1, x0 + 1 : x1 + 1] * ve
        ) / vsum
        sub = work[y0:y1, x0:x1]
        sub[sub_fill] = acc[sub_fill]

    return np.clip(np.round(work), 0, 255).astype(np.uint8)


def x_grid(h: int, w: int) -> np.ndarray:
    return np.broadcast_to(np.arange(w, dtype=np.int32)[None, :], (h, w))


def y_grid(h: int, w: int) -> np.ndarray:
    return np.broadcast_to(np.arange(h, dtype=np.int32)[:, None], (h, w))


def poly_mask(shape: tuple[int, int], pts: list[tuple[float, float]]) -> np.ndarray:
    h, w = shape
    im = Image.new("L", (w, h), 0)
    ImageDraw.Draw(im).polygon(pts, fill=255)
    return np.array(im, dtype=np.uint8) > 127


def ellipse_mask(shape: tuple[int, int], cx: float, cy: float, rx: float, ry: float) -> np.ndarray:
    h, w = shape
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    return ((xx - cx) / max(rx, 1.0)) ** 2 + ((yy - cy) / max(ry, 1.0)) ** 2 <= 1.0


def soft_weight(mask: np.ndarray, feather: float = 1.0) -> np.ndarray:
    if feather <= 0.0:
        return mask.astype(np.float32)
    return np.clip(gaussian_filter(mask.astype(np.float32), sigma=feather), 0.0, 1.0)


def extract_layer(src: np.ndarray, mask_f: np.ndarray) -> np.ndarray:
    out = src.copy()
    w = np.clip(mask_f, 0.0, 1.0)
    out[:, :, 3] = np.clip(np.round(out[:, :, 3].astype(np.float32) * w), 0, 255).astype(np.uint8)
    out[out[:, :, 3] == 0, :3] = 0
    return out


def alpha_composite_np(dst: np.ndarray, src: np.ndarray) -> np.ndarray:
    """Exact straight-RGBA alpha composition of `src` over `dst`."""
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


def build_sakura_parts() -> list[dict]:
    s0 = load_rgba(SHELL_DIR / "surface0.png")
    h, w = s0.shape[:2]
    alpha = s0[:, :, 3]
    opaque = alpha > 16
    rgb = s0[:, :, :3].astype(np.int16)
    r, g, b = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]
    xg, yg = x_grid(h, w), y_grid(h, w)

    # Interior color predicates (strict separation so coat white, dark line-art, and bows stay isolated)
    is_pure_hair = opaque & (b >= r + 22) & (b >= 155) & (r >= 92) & (g >= 115) & (yg <= 415)
    is_hair_like = opaque & (yg <= 415) & ((b > r + 10) | ((r < 118) & (b < 148)))
    is_pure_navy_skirt = opaque & (b >= r + 24) & (b <= 218) & (r >= 76) & (r <= 160) & (g >= 78) & (g <= 170) & (yg >= 662)
    is_yellow_ribbon = opaque & (r >= 195) & (g >= 188) & (b >= 105) & (b <= 195) & (r - b >= 28)
    # Strict saturated periwinkle bow predicate (never matches white coat cool shading!)
    is_purple_bow = opaque & (b >= r + 26) & (b - g >= 20) & (r <= 178) & (g <= 175) & (b <= 238)
    is_pure_white_coat = opaque & (r >= 224) & (g >= 226) & (b >= 232) & (b >= r - 2) & (b - r <= 24)
    is_skin = opaque & (r >= 208) & (g >= 160) & (r >= b + 12)
    is_dark_line = opaque & (r < 110) & (g < 115) & (b < 155)

    # 1) Ahoge (z=80) strictly above crown dome arc
    dome_cut_y = 123.5 + 0.0045 * ((xg - 318.0) ** 2)
    ahoge_region = opaque & (yg <= dome_cut_y) & (xg >= 272) & (xg <= 388) & (yg >= 44)
    save_rgba(extract_layer(s0, soft_weight(ahoge_region, 0.7)), PARTS_DIR / "sakura_ahoge.png")

    # 2) Central forehead bang lock (z=69) hanging between eyebrows
    bang_center_poly = poly_mask(
        (h, w),
        [
            (286, 146),
            (338, 146),
            (346, 198),
            (340, 248),
            (329, 272),
            (318, 281),
            (296, 268),
            (277, 246),
            (274, 198),
        ],
    )
    bang_center_mask = opaque & bang_center_poly & is_hair_like & (~is_skin)
    top_fade_bc = np.clip((yg.astype(np.float32) - 146.0) / 20.0, 0.0, 1.0)
    bang_center_w = np.clip(soft_weight(bang_center_mask, 0.7) * top_fade_bc, 0.0, 1.0)
    save_rgba(extract_layer(s0, bang_center_w), PARTS_DIR / "sakura_hair_bangs_center.png")

    bang_excl_rgba = np.zeros_like(s0)
    bang_excl_rgba[:, :, :3] = 255
    bang_excl_rgba[:, :, 3] = np.clip(
        np.round(soft_weight(binary_erosion(bang_center_mask, iterations=2), 0.7) * 255.0),
        0,
        255,
    ).astype(np.uint8)
    save_rgba(bang_excl_rgba, PARTS_DIR / "sakura_bang_center_excl.png")

    # 3) Outer upper temple wings (z=66, z=67)
    wing_l_poly = poly_mask((h, w), [(185, 165), (242, 165), (246, 232), (225, 294), (185, 294)])
    wing_r_poly = poly_mask((h, w), [(424, 170), (506, 170), (506, 304), (432, 304), (414, 242)])
    wing_l_mask = opaque & wing_l_poly & is_hair_like & (~is_skin)
    wing_r_mask = opaque & wing_r_poly & is_hair_like & (~is_skin)
    wl_top = np.clip((yg.astype(np.float32) - 165.0) / 22.0, 0.0, 1.0)
    wr_top = np.clip((yg.astype(np.float32) - 170.0) / 22.0, 0.0, 1.0)
    wl_in = np.clip((244.0 - xg.astype(np.float32)) / 14.0, 0.0, 1.0)
    wr_in = np.clip((xg.astype(np.float32) - 418.0) / 14.0, 0.0, 1.0)
    wing_l_w = np.clip(soft_weight(wing_l_mask, 0.7) * wl_top * wl_in, 0.0, 1.0)
    wing_r_w = np.clip(soft_weight(wing_r_mask, 0.7) * wr_top * wr_in, 0.0, 1.0)
    save_rgba(extract_layer(s0, wing_l_w), PARTS_DIR / "sakura_hair_wing_l.png")
    save_rgba(extract_layer(s0, wing_r_w), PARTS_DIR / "sakura_hair_wing_r.png")

    # 4) Lower cyan-tipped wavy curls:
    # - hair_curl_l at z=56 (BEHIND head_base z=58 so crisp cheek jawline renders cleanly on top!)
    # - hair_curl_r at z=64 (IN FRONT on right jaw, strictly guarded outside eye/lash/cheek bbox!)
    eye_and_cheek_guard = binary_dilation(
        ellipse_mask((h, w), 275.0, 282.0, 34.0, 42.0)
        | ellipse_mask((h, w), 367.0, 282.0, 34.0, 42.0)
        | poly_mask((h, w), [(252, 220), (392, 220), (386, 342), (346, 366), (294, 364), (258, 332)]),
        iterations=2,
    )
    collar_guard = is_yellow_ribbon | is_pure_white_coat | is_skin | eye_and_cheek_guard
    curl_l_poly = poly_mask((h, w), [(188, 240), (249, 240), (250, 306), (264, 340), (300, 405), (188, 405)])
    curl_r_poly = poly_mask((h, w), [(396, 238), (488, 238), (488, 406), (358, 406), (378, 348), (394, 308)])
    curl_l_raw = (curl_l_poly & opaque & is_hair_like & (~collar_guard)).astype(np.float32)
    curl_r_raw = (curl_r_poly & opaque & is_hair_like & (~collar_guard)).astype(np.float32)
    cl_top = np.clip((yg.astype(np.float32) - 240.0) / 26.0, 0.0, 1.0)
    cr_top = np.clip((yg.astype(np.float32) - 238.0) / 26.0, 0.0, 1.0)
    cr_in = np.where(yg < 312, np.clip((xg.astype(np.float32) - 392.0) / 8.0, 0.0, 1.0), 1.0)
    curl_l_w = np.clip(gaussian_filter(curl_l_raw, 0.7) * cl_top, 0.0, 1.0)
    curl_r_w = np.clip(gaussian_filter(curl_r_raw, 0.7) * cr_top * cr_in, 0.0, 1.0)
    save_rgba(extract_layer(s0, curl_l_w), PARTS_DIR / "sakura_hair_curl_l.png")
    save_rgba(extract_layer(s0, curl_r_w), PARTS_DIR / "sakura_hair_curl_r.png")

    # 5) Back hair behind neck/ears (left & right split for natural stereo parallax, z=14, z=16)
    hb_zone = poly_mask((h, w), [(208, 248), (468, 248), (470, 396), (416, 406), (238, 406), (206, 386)])
    hb_shape = hb_zone & (
        binary_dilation(opaque & is_hair_like & (yg >= 256), iterations=2)
        | poly_mask((h, w), [(244, 272), (410, 272), (414, 402), (242, 402)])
    )
    hb_known = hb_zone & is_pure_hair & ((xg <= 264) | (xg >= 388))
    hb_rgb = diffuse_from_known(s0[:, :, :3], hb_shape & (~hb_known), hb_known, iters=140)
    hb_full = np.zeros_like(s0)
    hb_full[:, :, :3] = hb_rgb
    hb_alpha = soft_weight(hb_shape, 1.0)
    split_l = np.clip((332.0 - xg.astype(np.float32) + 8.0) / 16.0, 0.0, 1.0)
    split_r = 1.0 - split_l
    hb_l = hb_full.copy()
    hb_l[:, :, 3] = np.clip(np.round(hb_alpha * split_l * 255.0), 0, 255).astype(np.uint8)
    hb_r = hb_full.copy()
    hb_r[:, :, 3] = np.clip(np.round(hb_alpha * split_r * 255.0), 0, 255).astype(np.uint8)
    save_rgba(hb_l, PARTS_DIR / "sakura_hair_back_l.png")
    save_rgba(hb_r, PARTS_DIR / "sakura_hair_back_r.png")

    # 6) Anatomical Head Base (z=58):
    # - Strictly follows the crisp dark jawline `(256,332)->(272,350)->(292,363)->(318,374)->(345,367)->(365,355)->(376,342)`
    #   with ZERO lower neck skin!
    # - Smooth curved outer boundary (ZERO rectangular holes!) gently tucked behind wing_l/wing_r/curl_r
    # - Complete skin inpaint under eyes, eyebrows, blush/mouth, and lower central bang lock
    jaw_poly = poly_mask(
        (h, w),
        [
            (196, 118),
            (462, 118),
            (456, 238),
            (416, 254),
            (388, 286),
            (380, 338),
            (366, 357),
            (345, 369),
            (320, 376),
            (292, 365),
            (272, 351),
            (256, 332),
            (242, 278),
            (226, 250),
            (196, 240),
        ],
    )
    outer_wing_trim_l = poly_mask((h, w), [(170, 165), (228, 215), (222, 315), (170, 315)])
    outer_wing_trim_r = poly_mask((h, w), [(428, 215), (510, 175), (510, 325), (422, 325)])
    head_base_mask = opaque & jaw_poly & (~(yg < 122)) & (~ahoge_region) & (~outer_wing_trim_l) & (~outer_wing_trim_r)

    eye_l_wipe = ellipse_mask((h, w), 278.0, 276.0, 37.0, 38.0)
    eye_r_wipe = ellipse_mask((h, w), 366.0, 274.0, 37.0, 38.0)
    brows_wipe = poly_mask((h, w), [(252, 224), (300, 224), (300, 246), (252, 246)]) | poly_mask(
        (h, w), [(338, 224), (390, 224), (390, 246), (338, 246)]
    )
    nose_mouth_wipe = poly_mask((h, w), [(296, 296), (346, 296), (346, 352), (296, 352)])
    # Exclude tiny isolated dark lash pixels inside face interior
    face_int_poly = poly_mask((h, w), [(252, 224), (390, 224), (382, 344), (320, 358), (258, 342)])
    stray_dark_face = face_int_poly & is_dark_line & (yg >= 224) & (yg <= 346)
    curved_hairline = 226.0 + 0.008 * ((xg.astype(np.float32) - 312.0) ** 2)
    bang_lower_wipe = binary_dilation(bang_center_mask, iterations=4) & (yg >= curved_hairline)
    face_skin_wipe = (eye_l_wipe | eye_r_wipe | brows_wipe | nose_mouth_wipe | stray_dark_face | bang_lower_wipe) & poly_mask(
        (h, w), [(244, 190), (398, 190), (384, 354), (320, 366), (256, 344)]
    )
    forehead_skin_known = (
        jaw_poly
        & is_skin
        & (~binary_dilation(face_skin_wipe, iterations=3))
        & (~binary_dilation(is_dark_line, iterations=2))
        & (~binary_dilation(bang_center_mask, iterations=3))
        & (yg >= 220)
        & (yg <= 362)
    )
    bang_crown_wipe = binary_dilation(bang_center_mask, iterations=3) & (yg < curved_hairline) & jaw_poly
    bang_crown_known = jaw_poly & is_pure_hair & (~bang_crown_wipe) & (~bang_center_mask) & (yg >= 124) & (yg <= 212)

    head_rgb = diffuse_from_known(s0[:, :, :3], face_skin_wipe, forehead_skin_known, iters=180)
    head_rgb = diffuse_from_known(head_rgb, bang_crown_wipe, bang_crown_known, iters=110)

    sakura_head = s0.copy()
    sakura_head[:, :, :3] = head_rgb
    sakura_head = extract_layer(sakura_head, soft_weight(head_base_mask, 0.7))
    save_rgba(sakura_head, PARTS_DIR / "sakura_head_base.png")

    # 7) Floating 2.5D Face Features rest texture + smooth AlphaMask + Pin ref
    face_poly = poly_mask(
        (h, w),
        [
            (248, 212),
            (396, 212),
            (396, 306),
            (380, 336),
            (356, 354),
            (320, 363),
            (284, 352),
            (262, 332),
            (250, 302),
        ],
    )
    face_mask_f = soft_weight(face_poly, 3.2) * (
        1.0 - np.clip(soft_weight(binary_erosion(bang_center_mask, iterations=2), 0.7), 0.0, 1.0)
    )
    sakura_face = extract_layer(s0, face_mask_f)
    save_rgba(sakura_face, PARTS_DIR / "sakura_face_features.png")

    fmask_rgba = np.zeros_like(s0)
    fmask_rgba[:, :, :3] = 255
    fmask_rgba[:, :, 3] = np.clip(np.round(soft_weight(face_poly, 3.2) * 255.0), 0, 255).astype(np.uint8)
    save_rgba(fmask_rgba, PARTS_DIR / "sakura_face_mask.png")

    save_rgba(s0, PARTS_DIR / "sakura_hairpin_ref.png")
    save_rgba(np.zeros_like(s0), PARTS_DIR / "sakura_hairpin_slot.png")

    # 8) Neck column (z=38) between jawline y=336 and sailor collar V-lapels y=424
    neck_poly = poly_mask((h, w), [(296, 336), (376, 336), (374, 424), (294, 424)])
    neck_skin_known = neck_poly & is_skin & (~binary_dilation(is_dark_line, iterations=1)) & (yg >= 372) & (yg <= 416)
    neck_fill = neck_poly & (~neck_skin_known)
    neck_rgb = diffuse_from_known(s0[:, :, :3], neck_fill, neck_skin_known, iters=110)
    submental_shade = np.clip((380.0 - yg.astype(np.float32)) / 32.0, 0.0, 1.0) * 0.08
    neck_rgb_f = neck_rgb.astype(np.float32)
    neck_rgb_f[:, :, 0] *= 1.0 - submental_shade * 0.45
    neck_rgb_f[:, :, 1] *= 1.0 - submental_shade * 0.85
    neck_rgb_f[:, :, 2] *= 1.0 - submental_shade * 0.65
    neck_rgb = np.clip(np.round(neck_rgb_f), 0, 255).astype(np.uint8)
    neck_shape = poly_mask((h, w), [(306, 344), (368, 344), (366, 422), (304, 422)])
    sakura_neck = np.zeros_like(s0)
    sakura_neck[:, :, :3] = neck_rgb
    sakura_neck[:, :, 3] = np.clip(np.round(soft_weight(neck_shape, 1.0) * 255.0), 0, 255).astype(np.uint8)
    save_rgba(sakura_neck, PARTS_DIR / "sakura_neck.png")

    # 9) Back Yellow Sailor Collar corner (z=24, x=436..482, y=420..505)
    collar_b_poly = poly_mask((h, w), [(424, 420), (484, 420), (484, 506), (426, 506)])
    cb_mask = opaque & collar_b_poly & (is_yellow_ribbon | (binary_dilation(is_yellow_ribbon, iterations=3) & is_dark_line))
    cb_fade_in = np.clip((xg.astype(np.float32) - 424.0) / 14.0, 0.0, 1.0)
    save_rgba(extract_layer(s0, soft_weight(cb_mask, 0.7) * cb_fade_in), PARTS_DIR / "sakura_collar_back.png")

    # 10) Chest & Waist Ribbons (z=50, z=54) — connected-component isolated so sleeve cuff & cyan lapel never leak!
    bow_c_zone = poly_mask((h, w), [(244, 466), (362, 466), (362, 572), (244, 572)])
    bow_c_mask_guard = ~(
        ((xg <= 243) & (yg >= 486))
        | ((xg >= 354) & (yg >= 506))
        | ((xg >= 347) & (yg >= 546))
        | ((g >= 175) & (b >= 205) & (r <= 185))
    )
    bow_seeds_raw = bow_c_zone & is_purple_bow & bow_c_mask_guard
    lbl_b, _ = label(binary_dilation(bow_seeds_raw, iterations=1))
    main_lbl = lbl_b[495, 300]
    bow_main = (lbl_b == main_lbl) if main_lbl > 0 else bow_seeds_raw
    bow_main_filled = binary_fill_holes(bow_main) & bow_c_zone & (~is_skin)
    bow_core = binary_dilation(bow_main_filled, iterations=2) & bow_c_zone & bow_c_mask_guard & opaque & (~is_skin)
    # Remove any outer light coat halo (not adjacent white glint enclosed by bow)
    bow_outside_white = is_pure_white_coat & (~binary_fill_holes(bow_main | (is_dark_line & binary_dilation(bow_main, iterations=2))))
    bow_core = bow_core & (~bow_outside_white)
    save_rgba(extract_layer(s0, soft_weight(bow_core, 0.7)), PARTS_DIR / "sakura_chest_bow.png")

    bow_w_zone = poly_mask((h, w), [(314, 578), (434, 578), (434, 688), (314, 688)])
    wbow_core = binary_dilation(bow_w_zone & is_yellow_ribbon, iterations=3) & bow_w_zone & opaque & (~is_pure_white_coat) & (~is_pure_navy_skirt)
    save_rgba(extract_layer(s0, soft_weight(wbow_core, 0.7)), PARTS_DIR / "sakura_waist_bow.png")

    # 11) Right Hanging Arm + Sleeve + Hand (z=44) — cleanly excluding thumb-adjacent skirt & back collar!
    arm_r_poly = poly_mask(
        (h, w),
        [
            (402, 422),
            (456, 422),
            (536, 690),
            (536, 826),
            (494, 824),
            (484, 804),
            (474, 776),
            (462, 746),
            (448, 732),
            (422, 650),
            (404, 585),
            (396, 488),
        ],
    )
    arm_r_mask = (
        opaque
        & arm_r_poly
        & (~wbow_core)
        & (~cb_mask)
        & (~is_pure_navy_skirt)
        & (~poly_mask((h, w), [(480, 808), (505, 808), (505, 830), (480, 830)]))
    )
    arm_top_fade = np.clip((yg.astype(np.float32) - 422.0) / 16.0, 0.0, 1.0)
    save_rgba(extract_layer(s0, soft_weight(arm_r_mask, 0.7) * arm_top_fade), PARTS_DIR / "sakura_arm_r.png")

    # 12) Pleated Navy Skirt (z=28) — ZERO coat outline/pocket/button seeds!
    hand_r_zone = poly_mask((h, w), [(456, 720), (542, 720), (542, 836), (456, 836)])
    skirt_vis_poly = poly_mask((h, w), [(120, 680), (476, 680), (492, 845), (365, 886), (225, 884), (118, 805)])
    # Strict interior navy skirt pixels excluding white coat, cyan pocket trim (g>=180), buttons, and yellow ribbon!
    coat_trim_guard = binary_dilation(is_pure_white_coat | is_yellow_ribbon | is_skin | ((g >= 180) & (b >= 210)), iterations=5)
    skirt_seed = (
        opaque
        & skirt_vis_poly
        & (~hand_r_zone)
        & (~arm_r_mask)
        & (~coat_trim_guard)
        & (b >= r + 24)
        & (b <= 216)
        & (r >= 78)
        & (g >= 80)
        & (r <= 155)
        & (g <= 165)
    )
    skirt_pleat_lines = (
        opaque
        & skirt_vis_poly
        & (~hand_r_zone)
        & (~arm_r_mask)
        & (~coat_trim_guard)
        & (binary_dilation(skirt_seed, iterations=4))
    )
    skirt_vis_all = skirt_seed | skirt_pleat_lines
    skirt_known = skirt_seed & is_pure_navy_skirt
    dist_skirt = distance_transform_edt(~skirt_vis_all)
    skirt_fill = (
        opaque
        & skirt_vis_poly
        & (yg >= 654)
        & (yg <= 880)
        & (xg <= 474)
        & (dist_skirt <= 16.0)
        & (~skirt_vis_all)
        & (~(opaque & (yg >= 868)))
    )
    skirt_rgb = diffuse_from_known(s0[:, :, :3], skirt_fill, skirt_known, iters=180)
    skirt_mask = (skirt_vis_all | skirt_fill) & (~(opaque & (yg >= 875)))
    sakura_skirt = np.zeros_like(s0)
    sakura_skirt[:, :, :3] = skirt_rgb
    sakura_skirt[:, :, 3] = np.clip(np.round(soft_weight(skirt_mask, 0.7) * 255.0), 0, 255).astype(np.uint8)
    save_rgba(sakura_skirt, PARTS_DIR / "sakura_skirt.png")

    # 13) Coat Flaps (L z=36, R z=34) split cleanly at waist y=624 (well below upper buttons y=593, well above lower buttons y=654!)
    coat_all = opaque & (yg >= 604) & (yg <= 812) & (~skirt_vis_all) & (~hand_r_zone) & (~arm_r_mask)
    navy_slit = (yg >= 662) & (b >= r + 18) & (r <= 170) & (b <= 218)
    coat_all &= ~navy_slit

    x_slit = 356.0 - 0.14 * np.maximum(yg.astype(np.float32) - 600.0, 0.0)
    flap_l_raw = coat_all & (xg.astype(np.float32) <= x_slit + 2.0) & (xg >= 136)
    flap_r_raw = coat_all & (xg.astype(np.float32) >= x_slit - 2.0) & (xg <= 470)

    coat_known_r = flap_r_raw & is_pure_white_coat & (~wbow_core)
    wbow_wipe_r = flap_r_raw & binary_dilation(wbow_core, iterations=4)
    flap_r_rgb = diffuse_from_known(s0[:, :, :3], wbow_wipe_r, coat_known_r, iters=120)

    waist_tuck_w = np.clip((yg.astype(np.float32) - 604.0) / 18.0, 0.0, 1.0)
    save_rgba(extract_layer(s0, soft_weight(flap_l_raw, 0.7) * waist_tuck_w), PARTS_DIR / "sakura_coat_flap_l.png")
    flap_r_full = s0.copy()
    flap_r_full[:, :, :3] = flap_r_rgb
    save_rgba(extract_layer(flap_r_full, soft_weight(flap_r_raw, 0.7) * waist_tuck_w), PARTS_DIR / "sakura_coat_flap_r.png")

    # 14) Upper Torso (z=40): collar + chest bodice + BOTH upper buttons (y<=593) + bent left arm (ZERO hair crumbs!)
    torso_poly = poly_mask(
        (h, w),
        [
            (166, 396),
            (256, 394),
            (305, 388),
            (306, 408),
            (366, 408),
            (368, 388),
            (416, 394),
            (448, 404),
            (448, 434),
            (402, 438),
            (412, 588),
            (420, 634),
            (166, 634),
        ],
    )
    torso_mask = opaque & torso_poly & (~cb_mask) & (~arm_r_mask)
    # Only keep collar pixels below shoulder top curve; remove hair crumbs on top edge
    hair_scrap_top = torso_mask & is_hair_like & (yg <= 406)
    torso_mask &= ~hair_scrap_top

    # Wipe purple chest bow & yellow waist bow + expand strictly over bow contours without touching sleeve cuff or cyan lapel!
    hand_skin = is_skin & (yg <= 504) & (xg <= 336)
    wbow_wipe_box = poly_mask((h, w), [(348, 584), (426, 584), (426, 642), (348, 642)]) | poly_mask(
        (h, w), [(322, 598), (426, 598), (426, 642), (322, 642)]
    )
    bow_wipe_t = (
        torso_mask
        & (
            binary_dilation(bow_core, iterations=4)
            | binary_dilation(wbow_core | wbow_wipe_box, iterations=5)
        )
        & (~hand_skin)
        & (yg >= 468)
    )
    coat_known_t = (
        torso_mask
        & is_pure_white_coat
        & (~bow_wipe_t)
        & (~binary_dilation(is_dark_line | hand_skin | bow_core | wbow_core | wbow_wipe_box, iterations=2))
        & (yg >= 462)
    )
    torso_rgb = diffuse_from_known(s0[:, :, :3], bow_wipe_t, coat_known_t, iters=150)

    # Waist bottom fade y=616..632 (between upper buttons at y=585 and lower buttons at y=660!)
    waist_bot_w = np.clip((632.0 - yg.astype(np.float32)) / 16.0, 0.0, 1.0)
    sakura_torso = s0.copy()
    sakura_torso[:, :, :3] = torso_rgb
    sakura_torso = extract_layer(sakura_torso, soft_weight(torso_mask, 0.7) * waist_bot_w)
    save_rgba(sakura_torso, PARTS_DIR / "sakura_upper_torso.png")

    # 15) Cute Legs + Frilly Ribbon Socks + Feminine Mary Jane Dress Shoes (split into leg_l z=18 & leg_r z=19)
    legs_zone = opaque & (yg >= 862)
    skirt_fringe = (yg <= 885) & (b > r + 14) & (r < 175)
    legs_clean = legs_zone & (~skirt_fringe)
    thigh_env = poly_mask((h, w), [(234, 836), (400, 836), (402, 886), (236, 886)])
    thigh_extend = thigh_env & (~legs_clean) & (yg <= 886)
    thigh_seed = legs_clean & (yg >= 878) & (yg <= 935) & (r >= 225) & (g >= 180) & (r >= b + 20)
    legs_rgb = diffuse_from_known(s0[:, :, :3], thigh_extend, thigh_seed, iters=100)
    legs_all_mask = legs_clean | thigh_extend

    x_sep = np.where(yg < 995, 323.0, 325.0 + 0.10 * np.maximum(yg.astype(np.float32) - 995.0, 0.0))
    blend_w = np.where(yg < 995, 8.0, 2.0)
    mask_l_f = np.clip((x_sep - xg.astype(np.float32) + blend_w * 0.5) / blend_w, 0.0, 1.0) * soft_weight(legs_all_mask, 0.7)
    mask_r_f = (1.0 - np.clip((x_sep - xg.astype(np.float32) + blend_w * 0.5) / blend_w, 0.0, 1.0)) * soft_weight(
        legs_all_mask, 0.7
    )

    legs_full = s0.copy()
    legs_full[:, :, :3] = legs_rgb
    save_rgba(extract_layer(legs_full, mask_l_f), PARTS_DIR / "sakura_leg_l.png")
    save_rgba(extract_layer(legs_full, mask_r_f), PARTS_DIR / "sakura_leg_r.png")

    return [
        {
            "id": "hair_back_l",
            "parent": "head_base",
            "texture": "parts/sakura_hair_back_l.png",
            "z": 14,
            "pivot_px": [286.0, 252.0],
            "role": "hair_back",
            "parallax": [-0.35, -0.15],
            "rot_deg": 2.4,
            "bend_px": 5.5,
            "breath_px": [0.0, 0.6],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 8.4,
            "spring_zeta": 0.40,
            "phase_offset": 0.20,
        },
        {
            "id": "hair_back_r",
            "parent": "head_base",
            "texture": "parts/sakura_hair_back_r.png",
            "z": 16,
            "pivot_px": [374.0, 252.0],
            "role": "hair_back",
            "parallax": [-0.35, -0.15],
            "rot_deg": 2.6,
            "bend_px": 5.5,
            "breath_px": [0.0, 0.6],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 8.8,
            "spring_zeta": 0.40,
            "phase_offset": 0.65,
        },
        {
            "id": "leg_l",
            "parent": None,
            "texture": "parts/sakura_leg_l.png",
            "z": 18,
            "pivot_px": [305.0, 1308.0],
            "role": "planted",
            "parallax": [0.0, 0.0],
            "rot_deg": 0.0,
            "bend_px": 0.0,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.002, -0.002],
        },
        {
            "id": "leg_r",
            "parent": None,
            "texture": "parts/sakura_leg_r.png",
            "z": 19,
            "pivot_px": [428.0, 1298.0],
            "role": "planted",
            "parallax": [0.0, 0.0],
            "rot_deg": 0.0,
            "bend_px": 0.0,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.002, -0.002],
        },
        {
            "id": "collar_back",
            "parent": "upper_torso",
            "texture": "parts/sakura_collar_back.png",
            "z": 24,
            "pivot_px": [442.0, 424.0],
            "role": "collar",
            "parallax": [0.20, 0.10],
            "rot_deg": 2.6,
            "bend_px": 3.0,
            "breath_px": [0.0, -0.6],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 9.2,
            "spring_zeta": 0.38,
            "phase_offset": 0.40,
        },
        {
            "id": "skirt",
            "parent": "upper_torso",
            "texture": "parts/sakura_skirt.png",
            "z": 28,
            "pivot_px": [324.0, 658.0],
            "role": "skirt",
            "parallax": [0.24, 0.0],
            "rot_deg": 2.8,
            "bend_px": 8.0,
            "breath_px": [0.0, 0.4],
            "scale_breath": [0.005, 0.0],
            "spring_omega": 6.6,
            "spring_zeta": 0.42,
        },
        {
            "id": "coat_flap_r",
            "parent": "upper_torso",
            "texture": "parts/sakura_coat_flap_r.png",
            "z": 34,
            "pivot_px": [374.0, 615.0],
            "role": "coat_hem",
            "parallax": [0.22, 0.0],
            "rot_deg": 3.0,
            "bend_px": 7.5,
            "breath_px": [0.0, 0.2],
            "scale_breath": [0.004, 0.0],
            "spring_omega": 7.0,
            "spring_zeta": 0.44,
            "phase_offset": 0.28,
        },
        {
            "id": "coat_flap_l",
            "parent": "upper_torso",
            "texture": "parts/sakura_coat_flap_l.png",
            "z": 36,
            "pivot_px": [305.0, 615.0],
            "role": "coat_hem",
            "parallax": [0.22, 0.0],
            "rot_deg": 2.8,
            "bend_px": 7.2,
            "breath_px": [0.0, 0.2],
            "scale_breath": [0.004, 0.0],
            "spring_omega": 6.8,
            "spring_zeta": 0.44,
        },
        {
            "id": "neck",
            "parent": "upper_torso",
            "texture": "parts/sakura_neck.png",
            "z": 38,
            "pivot_px": [338.0, 416.0],
            "role": "neck",
            "parallax": [0.15, 0.0],
            "rot_deg": 1.6,
            "bend_px": 0.0,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
        },
        {
            "id": "upper_torso",
            "parent": None,
            "texture": "parts/sakura_upper_torso.png",
            "z": 40,
            "pivot_px": [322.0, 622.0],
            "role": "torso",
            "parallax": [0.20, 0.12],
            "rot_deg": 1.4,
            "bend_px": 0.0,
            "breath_px": [0.0, -2.6],
            "scale_breath": [0.011, 0.008],
        },
        {
            "id": "arm_r",
            "parent": "upper_torso",
            "texture": "parts/sakura_arm_r.png",
            "z": 44,
            "pivot_px": [414.0, 434.0],
            "role": "arm",
            "parallax": [0.18, 0.10],
            "rot_deg": 2.8,
            "bend_px": 4.2,
            "breath_px": [0.0, -0.8],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 7.4,
            "spring_zeta": 0.42,
            "phase_offset": 0.55,
        },
        {
            "id": "chest_bow",
            "parent": "upper_torso",
            "texture": "parts/sakura_chest_bow.png",
            "z": 50,
            "pivot_px": [298.0, 476.0],
            "role": "ribbon",
            "parallax": [0.35, -0.18],
            "rot_deg": 2.4,
            "bend_px": 3.8,
            "breath_px": [0.0, -1.6],
            "scale_breath": [0.005, 0.0],
            "spring_omega": 9.8,
            "spring_zeta": 0.36,
            "phase_offset": 0.25,
        },
        {
            "id": "waist_bow",
            "parent": "coat_flap_r",
            "texture": "parts/sakura_waist_bow.png",
            "z": 54,
            "pivot_px": [374.0, 604.0],
            "role": "ribbon",
            "parallax": [0.32, 0.10],
            "rot_deg": 3.0,
            "bend_px": 4.8,
            "breath_px": [0.0, -0.6],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 8.4,
            "spring_zeta": 0.38,
            "phase_offset": 0.85,
        },
        {
            "id": "hair_curl_l",
            "parent": "head_base",
            "texture": "parts/sakura_hair_curl_l.png",
            "z": 56,
            "pivot_px": [232.0, 242.0],
            "role": "hair_side_l",
            "parallax": [0.40, 0.20],
            "rot_deg": 4.4,
            "bend_px": 7.5,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 9.2,
            "spring_zeta": 0.34,
            "phase_offset": 0.12,
        },
        {
            "id": "head_base",
            "parent": "neck",
            "texture": "parts/sakura_head_base.png",
            "z": 58,
            "pivot_px": [334.0, 354.0],
            "role": "head",
            "parallax": [0.35, 0.25],
            "rot_deg": 4.2,
            "bend_px": 0.0,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
        },
        {
            "id": "face_features",
            "parent": "head_base",
            "texture": "parts/sakura_face_features.png",
            "z": 60,
            "pivot_px": [322.0, 286.0],
            "role": "face_features",
            "parallax": [0.80, 0.65],
            "rot_deg": 0.0,
            "bend_px": 0.0,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "dynamic_patch": {
                "rect_px": [232, 198, 412, 366],
                "mode": "alpha_mask",
                "mask_texture": "parts/sakura_face_mask.png",
                "exclude_mask_texture": "parts/sakura_bang_center_excl.png",
            },
        },
        {
            "id": "hair_curl_r",
            "parent": "head_base",
            "texture": "parts/sakura_hair_curl_r.png",
            "z": 64,
            "pivot_px": [420.0, 236.0],
            "role": "hair_side_r",
            "parallax": [0.40, 0.20],
            "rot_deg": 4.6,
            "bend_px": 7.8,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 9.8,
            "spring_zeta": 0.34,
            "phase_offset": 0.72,
        },
        {
            "id": "hair_wing_l",
            "parent": "head_base",
            "texture": "parts/sakura_hair_wing_l.png",
            "z": 66,
            "pivot_px": [232.0, 162.0],
            "role": "hair_bangs",
            "parallax": [0.50, 0.30],
            "rot_deg": 3.4,
            "bend_px": 5.0,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 10.6,
            "spring_zeta": 0.34,
            "phase_offset": 0.18,
        },
        {
            "id": "hair_wing_r",
            "parent": "head_base",
            "texture": "parts/sakura_hair_wing_r.png",
            "z": 67,
            "pivot_px": [426.0, 168.0],
            "role": "hair_bangs",
            "parallax": [0.50, 0.30],
            "rot_deg": 3.4,
            "bend_px": 5.2,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 10.9,
            "spring_zeta": 0.34,
            "phase_offset": 0.58,
        },
        {
            "id": "hair_bangs_center",
            "parent": "head_base",
            "texture": "parts/sakura_hair_bangs_center.png",
            "z": 69,
            "pivot_px": [314.0, 148.0],
            "role": "hair_bangs",
            "parallax": [0.90, 0.65],
            "rot_deg": 3.6,
            "bend_px": 4.2,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 11.4,
            "spring_zeta": 0.32,
        },
        {
            "id": "star_hairpin",
            "parent": "head_base",
            "texture": "parts/sakura_hairpin_slot.png",
            "z": 75,
            "pivot_px": [242.0, 165.0],
            "role": "accessory",
            "parallax": [0.75, 0.55],
            "rot_deg": 2.2,
            "bend_px": 0.0,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "dynamic_patch": {
                "rect_px": [200, 130, 282, 200],
                "mode": "diff_overlay",
                "ref_texture": "parts/sakura_hairpin_ref.png",
                "threshold": 18,
            },
        },
        {
            "id": "ahoge",
            "parent": "head_base",
            "texture": "parts/sakura_ahoge.png",
            "z": 80,
            "pivot_px": [314.0, 125.0],
            "role": "ahoge",
            "parallax": [0.22, -0.18],
            "rot_deg": 7.0,
            "bend_px": 10.8,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 15.2,
            "spring_zeta": 0.24,
        },
    ]


def _draw_ss_circle(draw: ImageDraw.ImageDraw, cx: float, cy: float, r: float, ss: int, **kwargs) -> None:
    draw.ellipse([(cx - r) * ss, (cy - r) * ss, (cx + r) * ss, (cy + r) * ss], **kwargs)


def _paint_blush_ellipse(arr: np.ndarray, cx: float, cy: float, rx: float, ry: float) -> np.ndarray:
    """Paint clean soft-edged coral-pink Unyu cheek blush in straight-RGB space."""
    h, w = arr.shape[:2]
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    dist = np.sqrt(((xx - cx) / rx) ** 2 + ((yy - cy) / ry) ** 2)
    w_blush = np.clip((1.05 - dist) / 0.18, 0.0, 1.0)
    out = arr.astype(np.float32).copy()
    blush_col = np.array([255.0, 138.0, 146.0], dtype=np.float32)
    for c in range(3):
        out[:, :, c] = out[:, :, c] * (1.0 - w_blush) + blush_col[c] * w_blush
    return np.clip(np.round(out), 0, 255).astype(np.uint8)


def _render_unyu_glasses_4x(shape: tuple[int, int]) -> np.ndarray:
    """Render 4x-supersampled standalone Unyu sunglasses with crisp frames and 40%-translucent lenses."""
    h, w = shape
    ss = 4
    cw, ch = w * ss, h * ss
    im = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))

    frame_col = (27, 29, 62, 255)
    # 40% translucent charcoal-indigo lenses (102/255 = 40.0%)
    lens_col = (30, 36, 62, 102)
    glare_col = (150, 170, 222, 118)

    lx, ly, lr_out, lr_in = 180.0, 201.5, 27.6, 22.8
    rx, ry, rr_out, rr_in = 277.5, 195.0, 26.8, 22.2

    # 1. Left & right lenses (separate glare layer clipped strictly to each circular lens)
    lens_layer = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
    ld = ImageDraw.Draw(lens_layer)
    _draw_ss_circle(ld, lx, ly, lr_in, ss, fill=lens_col)
    _draw_ss_circle(ld, rx, ry, rr_in, ss, fill=lens_col)

    glare_raw = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glare_raw)
    gd.line([(162 * ss, 218 * ss), (192 * ss, 182 * ss)], fill=glare_col, width=7 * ss)
    gd.line([(176 * ss, 222 * ss), (202 * ss, 192 * ss)], fill=(150, 170, 222, 85), width=3 * ss)
    gd.line([(260 * ss, 212 * ss), (288 * ss, 178 * ss)], fill=glare_col, width=7 * ss)
    gd.line([(274 * ss, 216 * ss), (298 * ss, 187 * ss)], fill=(150, 170, 222, 85), width=3 * ss)

    lens_clip = Image.new("L", (cw, ch), 0)
    lcd = ImageDraw.Draw(lens_clip)
    _draw_ss_circle(lcd, lx, ly, lr_in - 0.3, ss, fill=255)
    _draw_ss_circle(lcd, rx, ry, rr_in - 0.3, ss, fill=255)
    glare_clipped = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
    glare_clipped.paste(glare_raw, (0, 0), lens_clip)
    lens_layer = Image.alpha_composite(lens_layer, glare_clipped)
    im = Image.alpha_composite(im, lens_layer)

    # 2. Crisp 100% opaque bridge, temples & circular rims
    d = ImageDraw.Draw(im)
    d.line([(39 * ss, 173 * ss), (49 * ss, 168 * ss), (153 * ss, 196 * ss)], fill=frame_col, width=4 * ss, joint="curve")
    d.line([(303 * ss, 192 * ss), (332 * ss, 155 * ss)], fill=frame_col, width=4 * ss)
    bridge_pts = []
    for k in range(17):
        t = k / 16.0
        bx = 205.5 + (252.5 - 205.5) * t
        by = 195.5 - 7.5 * np.sin(np.pi * t) - 2.0 * t
        bridge_pts.append((bx * ss, by * ss))
    d.line(bridge_pts, fill=frame_col, width=4 * ss, joint="curve")

    d.ellipse([149 * ss, 194 * ss, 157 * ss, 202 * ss], fill=frame_col)
    d.ellipse([299 * ss, 188 * ss, 307 * ss, 196 * ss], fill=frame_col)

    d.ellipse(
        [(lx - lr_out) * ss, (ly - lr_out) * ss, (lx + lr_out) * ss, (ly + lr_out) * ss],
        outline=frame_col,
        width=int(round((lr_out - lr_in) * ss)),
    )
    d.ellipse(
        [(rx - rr_out) * ss, (ry - rr_out) * ss, (rx + rr_out) * ss, (ry + rr_out) * ss],
        outline=frame_col,
        width=int(round((rr_out - rr_in) * ss)),
    )

    down = im.resize((w, h), Image.Resampling.LANCZOS)
    return np.array(down, dtype=np.uint8)


def _render_unyu_paws_4x(shape: tuple[int, int], body_rgba: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Render 4x-supersampled crisp smooth curved Unyu paws using exact underlying `unyu_body` gradient."""
    h, w = shape
    ss = 4
    cw, ch = w * ss, h * ss
    outline_col = (22, 22, 82, 255)
    body_up = np.array(
        Image.fromarray(body_rgba).resize((cw, ch), Image.Resampling.BILINEAR),
        dtype=np.uint8,
    )

    def make_paw(is_right: bool) -> np.ndarray:
        stroke_im = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
        fill_mask = Image.new("L", (cw, ch), 0)
        fd = ImageDraw.Draw(fill_mask)
        sd = ImageDraw.Draw(stroke_im)
        if not is_right:
            poly = [
                (156 * ss, 244 * ss),
                (182 * ss, 266 * ss),
                (196 * ss, 290 * ss),
                (195 * ss, 305 * ss),
                (182 * ss, 313 * ss),
                (160 * ss, 310 * ss),
                (130 * ss, 294 * ss),
                (130 * ss, 260 * ss),
            ]
            fd.polygon(poly, fill=255)
            sd.line(
                [
                    (164 * ss, 248 * ss),
                    (184 * ss, 268 * ss),
                    (195 * ss, 289 * ss),
                    (195 * ss, 304 * ss),
                    (182 * ss, 312 * ss),
                    (162 * ss, 310 * ss),
                    (132 * ss, 294 * ss),
                ],
                fill=outline_col,
                width=4 * ss,
                joint="curve",
            )
            sd.line([(179 * ss, 292 * ss), (191 * ss, 304 * ss)], fill=outline_col, width=3 * ss)
            sd.line([(169 * ss, 297 * ss), (182 * ss, 310 * ss)], fill=outline_col, width=3 * ss)
        else:
            poly = [
                (304 * ss, 238 * ss),
                (326 * ss, 256 * ss),
                (324 * ss, 286 * ss),
                (298 * ss, 304 * ss),
                (275 * ss, 308 * ss),
                (260 * ss, 300 * ss),
                (262 * ss, 280 * ss),
                (282 * ss, 252 * ss),
            ]
            fd.polygon(poly, fill=255)
            sd.line(
                [
                    (296 * ss, 240 * ss),
                    (276 * ss, 262 * ss),
                    (263 * ss, 283 * ss),
                    (261 * ss, 298 * ss),
                    (272 * ss, 306 * ss),
                    (292 * ss, 305 * ss),
                    (322 * ss, 287 * ss),
                ],
                fill=outline_col,
                width=4 * ss,
                joint="curve",
            )
            sd.line([(277 * ss, 288 * ss), (266 * ss, 299 * ss)], fill=outline_col, width=3 * ss)
            sd.line([(287 * ss, 293 * ss), (275 * ss, 304 * ss)], fill=outline_col, width=3 * ss)

        fill_arr = np.array(fill_mask, dtype=np.uint8)
        yy_s, xx_s = np.mgrid[0:ch, 0:cw].astype(np.float32) / float(ss)
        if not is_right:
            join_s = np.clip((yy_s - 252.0 + (xx_s - 138.0) * 0.45) / 24.0, 0.0, 1.0)
        else:
            join_s = np.clip((yy_s - 248.0 + (320.0 - xx_s) * 0.45) / 24.0, 0.0, 1.0)

        paw_hi = body_up.copy()
        paw_hi[:, :, 3] = np.clip(np.round(fill_arr.astype(np.float32) * join_s), 0, 255).astype(np.uint8)
        merged_hi = Image.alpha_composite(Image.fromarray(paw_hi), stroke_im)
        down = np.array(merged_hi.resize((w, h), Image.Resampling.LANCZOS), dtype=np.uint8)
        down[down[:, :, 3] == 0, :3] = 0
        return down

    return make_paw(False), make_paw(True)


def build_unyu_parts(clean_sources: dict[str, np.ndarray] | None = None) -> tuple[list[dict], dict[str, str]]:
    if clean_sources and all(k in clean_sources for k in ("s10", "s11", "s12")):
        u10 = clean_sources["s10"]
        u11 = clean_sources["s11"]
        u12 = clean_sources["s12"]
    else:
        u10 = load_rgba(SHELL_DIR / "surface10.png")
        u11 = load_rgba(SHELL_DIR / "surface11.png")
        u12 = load_rgba(SHELL_DIR / "surface12.png")
    h, w = u10.shape[:2]
    alpha = u10[:, :, 3]
    opaque = alpha > 16
    rgb10 = u10[:, :, :3].astype(np.int16)
    r, g, b = rgb10[:, :, 0], rgb10[:, :, 1], rgb10[:, :, 2]
    yg, xg = y_grid(h, w), x_grid(h, w)

    is_pure_periwinkle = opaque & (b >= r + 48) & (b >= 195) & (r >= 115) & (g >= 120) & ((r <= 195) | (b >= 235))
    is_pink_blush = opaque & (r >= 165) & (r > b + 8) & (yg >= 165) & (yg <= 238)

    # 1) Tuft (z=20)
    tuft_poly = poly_mask((h, w), [(10, 26), (114, 26), (112, 66), (102, 102), (92, 132), (10, 148)])
    tuft_mask = opaque & tuft_poly
    save_rgba(extract_layer(u10, soft_weight(tuft_mask, 0.8)), PARTS_DIR / "unyu_tuft.png")

    # Clean Spotless Mochi Body (z=10):
    # First fit smooth monotonic body boundary through tuft shoulder -> waist and right dome -> waist
    # so temple cutouts & smooth dome behind tuft have 100% continuous anti-aliased #161854 border!
    interior_body = binary_erosion(opaque, iterations=3)
    interior_1px = binary_erosion(opaque, iterations=1) & (~((r < 80) & (g < 80) & (b < 140)))
    gloss_preserve = poly_mask((h, w), [(220, 45), (365, 45), (365, 118), (220, 118)])
    eyebrow_erase = poly_mask((h, w), [(132, 112), (314, 112), (314, 150), (132, 150)]) & binary_dilation(
        r < 105, iterations=2
    )
    face_wipe = poly_mask((h, w), [(104, 148), (322, 148), (322, 260), (104, 260)])
    paw_wipe = poly_mask((h, w), [(116, 232), (212, 232), (212, 330), (116, 330)]) | poly_mask(
        (h, w), [(246, 226), (340, 226), (340, 326), (246, 326)]
    )
    temple_l_wipe = poly_mask((h, w), [(32, 160), (156, 186), (156, 206), (30, 184)])
    temple_r_wipe = poly_mask((h, w), [(298, 184), (344, 144), (350, 166), (304, 201)])
    temple_erase = (temple_l_wipe | temple_r_wipe) & binary_dilation((~is_pure_periwinkle) & (r < 135), iterations=2)
    blush_body_wipe = ellipse_mask((h, w), 318.0, 202.0, 34.0, 24.0) & binary_dilation(is_pink_blush, iterations=3)

    body_erase = (
        ((eyebrow_erase | face_wipe | paw_wipe | temple_erase) & interior_body) | (blush_body_wipe & interior_1px)
    ) & (~gloss_preserve)
    pure_periwinkle_core = binary_erosion(is_pure_periwinkle, iterations=2)
    not_right_rim = ~((xg >= 272) & (yg <= 212) & (r >= 160))
    body_known = binary_erosion(opaque, iterations=3) & pure_periwinkle_core & not_right_rim & (~body_erase) & (~tuft_mask)
    body_rgb = diffuse_from_known(u10[:, :, :3], body_erase, body_known, iters=220)

    # Sample strictly on oval torso below tuft (y=150..162 and y=198..232 on left; y=124..140 and y=182..216 on right)
    border_l_band = (yg >= 146) & (yg <= 234) & (xg <= 75) & opaque
    ys_sample_l, xs_sample_l = [], []
    for yy_i in list(range(149, 164)) + list(range(196, 232)):
        row_xs = np.where(border_l_band[yy_i, :])[0]
        if len(row_xs):
            ys_sample_l.append(float(yy_i))
            xs_sample_l.append(float(row_xs[0]))
    poly_l = np.poly1d(np.polyfit(ys_sample_l, xs_sample_l, 2))
    fit_x_l = poly_l(yg.astype(np.float32))

    ys_sample_r, xs_sample_r = [], []
    border_r_band = (yg >= 120) & (yg <= 218) & (xg >= 300) & opaque
    for yy_i in list(range(122, 140)) + list(range(178, 214)):
        row_xs = np.where(border_r_band[yy_i, :])[0]
        if len(row_xs):
            ys_sample_r.append(float(yy_i))
            xs_sample_r.append(float(row_xs[-1]))
    poly_r = np.poly1d(np.polyfit(ys_sample_r, xs_sample_r, 2))
    fit_x_r = poly_r(yg.astype(np.float32))

    smooth_dome_cut = poly_mask((h, w), [(6, 20), (96, 20), (88, 66), (72, 118), (52, 152), (6, 152)])
    patch_l = (yg >= 164) & (yg <= 196) & (xg <= 64)
    patch_r = (yg >= 142) & (yg <= 178) & (xg >= 316)
    body_mask = opaque & (~smooth_dome_cut)
    body_mask = np.where(patch_l, xg.astype(np.float32) >= fit_x_l, body_mask)
    body_mask = np.where(patch_r, xg.astype(np.float32) <= fit_x_r, body_mask)

    body_rgb = diffuse_from_known(
        body_rgb,
        body_mask & (patch_l | patch_r) & (~pure_periwinkle_core),
        body_known,
        iters=120,
    )

    unyu_body = np.zeros_like(u10)
    unyu_body[:, :, :3] = body_rgb
    unyu_body[:, :, 3] = np.clip(np.round(soft_weight(body_mask, 0.8) * 255.0), 0, 255).astype(np.uint8)
    contour_im = Image.fromarray(unyu_body)
    draw_c = ImageDraw.Draw(contour_im)
    draw_c.line([(96, 56), (86, 86), (72, 118), (52, 150)], fill=(22, 22, 82, 255), width=4, joint="curve")
    pts_l = [(float(poly_l(yy_f)) + 1.8, float(yy_f)) for yy_f in range(160, 201, 3)]
    pts_r = [(float(poly_r(yy_f)) - 1.8, float(yy_f)) for yy_f in range(138, 183, 3)]
    draw_c.line(pts_l, fill=(22, 22, 82, 255), width=4, joint="curve")
    draw_c.line(pts_r, fill=(22, 22, 82, 255), width=4, joint="curve")
    unyu_body = np.array(contour_im, dtype=np.uint8)
    unyu_body[:, :, 3][~body_mask] = 0
    unyu_body[unyu_body[:, :, 3] == 0, :3] = 0
    save_rgba(unyu_body, PARTS_DIR / "unyu_body.png")

    # 3) 4x-supersampled clean standalone Paws (matching exact `unyu_body` gradient!) & 40%-Translucent Sunglasses
    paw_l, paw_r = _render_unyu_paws_4x((h, w), unyu_body)
    save_rgba(paw_l, PARTS_DIR / "unyu_paw_l.png")
    save_rgba(paw_r, PARTS_DIR / "unyu_paw_r.png")
    unyu_glasses = _render_unyu_glasses_4x((h, w))
    save_rgba(unyu_glasses, PARTS_DIR / "unyu_glasses.png")

    # 4) Clean sunglasses-free, paw-free expression bases (`unyu_clean_s10/11/12.png`)
    # Built directly on `unyu_body` so outer rim/gloss are 100% identical to `unyu_body.png`
    # and `mochi_face` DiffOverlay NEVER copies a single border/rim outline pixel!
    def clean_base_for(expr_arr: np.ndarray, mode: str) -> np.ndarray:
        out = unyu_body.copy()
        out[tuft_mask] = u10[tuft_mask]
        er = expr_arr[:, :, 0].astype(np.int16)
        eb = expr_arr[:, :, 2].astype(np.int16)

        # Graft crisp dark eyebrows without disturbing surrounding body shading/gloss
        brow_l = poly_mask((h, w), [(142, 116), (195, 116), (195, 149), (142, 149)])
        brow_r = poly_mask((h, w), [(252, 112), (303, 112), (303, 149), (252, 149)])
        brow_mask = (brow_l | brow_r) & binary_dilation((er < 115) & (eb < 155), iterations=1)
        out[brow_mask] = expr_arr[brow_mask]

        # Graft clean mouth
        if mode == "s11":
            m_box = ellipse_mask((h, w), 230.5, 226.5, 20.5, 23.0)
            mouth_keep = m_box & binary_dilation((er < 120) | (er > eb + 10), iterations=2)
        elif mode == "s10":
            m_box = poly_mask((h, w), [(214, 205), (249, 205), (249, 225), (214, 225)])
            mouth_keep = m_box & binary_dilation((er < 105) & (eb < 155), iterations=1)
        else:
            m_box = poly_mask((h, w), [(212, 204), (251, 204), (251, 248), (212, 248)])
            mouth_keep = m_box & binary_dilation((er < 105) | (er > eb + 10), iterations=1)
        out[mouth_keep] = expr_arr[mouth_keep]

        # Graft or draw eyes (100% free of glasses rim/lens smudge)
        if mode == "s10":
            is_lens_tint = (er >= 95) & (er <= 185) & (eb - er < 36)
            eye_boxes = (
                poly_mask((h, w), [(145, 148), (192, 148), (192, 171), (145, 171)])
                | poly_mask((h, w), [(151, 171), (186, 171), (186, 175), (151, 175)])
                | poly_mask((h, w), [(256, 145), (294, 145), (294, 170), (256, 170)])
            )
            eye_feat = eye_boxes & (~is_lens_tint) & binary_dilation((er < 115) | ((er > 195) & (eb > 195)), iterations=1)
            out[eye_feat] = expr_arr[eye_feat]
        elif mode == "s12":
            eye_boxes = (
                poly_mask((h, w), [(142, 155), (192, 155), (192, 177), (142, 177)])
                | poly_mask((h, w), [(256, 154), (301, 154), (301, 171), (256, 171)])
            )
            eye_feat = eye_boxes & binary_dilation(er < 95, iterations=1)
            out[eye_feat] = expr_arr[eye_feat]
        elif mode == "s11":
            ss = 4
            eye_im = Image.new("RGBA", (w * ss, h * ss), (0, 0, 0, 0))
            ed = ImageDraw.Draw(eye_im)
            navy_line = (22, 24, 84, 255)
            sclera_w = (255, 255, 255, 255)
            _draw_ss_circle(ed, 168.5, 175.0, 29.2, ss, fill=sclera_w, outline=navy_line, width=int(3.6 * ss))
            _draw_ss_circle(ed, 277.5, 169.0, 26.8, ss, fill=sclera_w, outline=navy_line, width=int(3.6 * ss))
            _draw_ss_circle(ed, 172.0, 174.5, 14.2, ss, fill=navy_line)
            _draw_ss_circle(ed, 275.5, 169.5, 13.0, ss, fill=navy_line)
            _draw_ss_circle(ed, 169.0, 170.5, 4.4, ss, fill=(255, 255, 255, 255))
            _draw_ss_circle(ed, 272.5, 165.5, 4.0, ss, fill=(255, 255, 255, 255))
            _draw_ss_circle(ed, 178.0, 181.0, 2.2, ss, fill=(255, 255, 255, 220))
            _draw_ss_circle(ed, 281.0, 176.0, 2.0, ss, fill=(255, 255, 255, 220))
            eyes_down = np.array(eye_im.resize((w, h), Image.Resampling.LANCZOS), dtype=np.uint8)
            out = alpha_composite_np(out, eyes_down)

        # Fresh crisp soft coral-pink blushes positioned cleanly beside eyes & outside mouth
        out = _paint_blush_ellipse(out, 140.0, 205.0, 22.5, 10.5)
        out = _paint_blush_ellipse(out, 308.0, 196.0, 20.5, 9.8)
        return out

    # Also build crisp glasses-free blink overlay (`surface111.png`) so SERIKO blink never stamps old sunglasses!
    ss = 4
    blink_im = Image.new("RGBA", (w * ss, h * ss), (0, 0, 0, 0))
    bd = ImageDraw.Draw(blink_im)
    navy_line = (22, 24, 84, 255)
    for arc_pts in (
        [(142, 168), (155, 174), (171, 175), (186, 170), (194, 163)],
        [(255, 165), (267, 170), (282, 170), (296, 165), (303, 159)],
    ):
        bd.line([(px * ss, py * ss) for px, py in arc_pts], fill=navy_line, width=int(5.2 * ss), joint="curve")
        for ex, ey in (arc_pts[0], arc_pts[-1]):
            _draw_ss_circle(bd, ex, ey, 2.5, ss, fill=navy_line)
    blink_arcs = np.array(blink_im.resize((w, h), Image.Resampling.LANCZOS), dtype=np.uint8)
    blink_comp = alpha_composite_np(unyu_body, blink_arcs)
    blink_mask_boxes = poly_mask((h, w), [(132, 152), (202, 152), (202, 186), (132, 186)]) | poly_mask(
        (h, w), [(248, 148), (312, 148), (312, 184), (248, 184)]
    )
    surface111 = extract_layer(blink_comp, soft_weight(blink_mask_boxes & body_mask, 1.4))
    save_rgba(surface111, SHELL_DIR / "surface111.png")

    clean_s10 = clean_base_for(u10, "s10")
    clean_s11 = clean_base_for(u11, "s11")
    clean_s12 = clean_base_for(u12, "s12")
    save_rgba(clean_s10, PARTS_DIR / "unyu_clean_s10.png")
    save_rgba(clean_s11, PARTS_DIR / "unyu_clean_s11.png")
    save_rgba(clean_s12, PARTS_DIR / "unyu_clean_s12.png")

    # 5) Clean Unyu Face rest frame (eyebrows/eyes/blush/`:3` mouth, ZERO glasses!)
    face_patch_box = poly_mask((h, w), [(106, 106), (332, 106), (332, 260), (106, 260)])
    interior_face = binary_erosion(opaque, iterations=10)
    face_diff = face_patch_box & interior_face & (
        np.max(np.abs(clean_s10[:, :, :3].astype(np.int16) - unyu_body[:, :, :3].astype(np.int16)), axis=2) > 16
    )
    unyu_face = extract_layer(clean_s10, soft_weight(face_diff, 0.7))
    save_rgba(unyu_face, PARTS_DIR / "unyu_face.png")

    save_rgba(np.zeros_like(u10), PARTS_DIR / "unyu_crown_slot.png")

    # 6) Overwrite static `surface10.png`, `surface11.png`, `surface12.png` with clean composited versions
    for fname, clean_src in (("surface10.png", clean_s10), ("surface11.png", clean_s11), ("surface12.png", clean_s12)):
        comp = alpha_composite_np(clean_src, paw_l)
        comp = alpha_composite_np(comp, paw_r)
        comp = alpha_composite_np(comp, unyu_glasses)
        save_rgba(comp, SHELL_DIR / fname)

    parts = [
        {
            "id": "mochi_body",
            "parent": None,
            "texture": "parts/unyu_body.png",
            "z": 10,
            "pivot_px": [190.0, 386.0],
            "role": "mochi_body",
            "parallax": [0.35, 0.14],
            "rot_deg": 1.8,
            "bend_px": 0.0,
            "breath_px": [0.0, -2.5],
            "scale_breath": [0.014, -0.008],
        },
        {
            "id": "mochi_tuft",
            "parent": "mochi_body",
            "texture": "parts/unyu_tuft.png",
            "z": 20,
            "pivot_px": [96.0, 102.0],
            "role": "mochi_tuft",
            "parallax": [0.22, -0.18],
            "rot_deg": 6.2,
            "bend_px": 8.0,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 13.5,
            "spring_zeta": 0.28,
        },
        {
            "id": "crown_accessory",
            "parent": "mochi_body",
            "texture": "parts/unyu_crown_slot.png",
            "z": 25,
            "pivot_px": [190.0, 62.0],
            "role": "accessory",
            "parallax": [0.42, 0.20],
            "rot_deg": 2.4,
            "bend_px": 0.0,
            "breath_px": [0.0, -1.5],
            "scale_breath": [0.0, 0.0],
            "dynamic_patch": {
                "rect_px": [140, 12, 240, 68],
                "mode": "diff_overlay",
                "ref_texture": "parts/unyu_clean_s10.png",
                "threshold": 18,
            },
        },
        {
            "id": "mochi_face",
            "parent": "mochi_body",
            "texture": "parts/unyu_face.png",
            "z": 30,
            "pivot_px": [216.0, 188.0],
            "role": "mochi_face",
            "parallax": [0.78, 0.42],
            "rot_deg": 2.0,
            "bend_px": 0.0,
            "breath_px": [0.0, -0.8],
            "scale_breath": [0.0, 0.0],
            "dynamic_patch": {
                "rect_px": [106, 106, 332, 260],
                "mode": "diff_overlay",
                "ref_texture": "parts/unyu_body.png",
                "threshold": 16,
            },
        },
        {
            "id": "mochi_paw_l",
            "parent": "mochi_body",
            "texture": "parts/unyu_paw_l.png",
            "z": 40,
            "pivot_px": [138.0, 256.0],
            "role": "mochi_paws",
            "parallax": [0.32, 0.24],
            "rot_deg": 9.5,
            "bend_px": 3.5,
            "breath_px": [0.0, -1.4],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 11.2,
            "spring_zeta": 0.32,
            "phase_offset": 0.0,
        },
        {
            "id": "mochi_paw_r",
            "parent": "mochi_body",
            "texture": "parts/unyu_paw_r.png",
            "z": 42,
            "pivot_px": [314.0, 252.0],
            "role": "mochi_paws",
            "parallax": [0.32, 0.24],
            "rot_deg": -9.5,
            "bend_px": 3.5,
            "breath_px": [0.0, -1.4],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 11.5,
            "spring_zeta": 0.32,
            "phase_offset": 0.85,
        },
        {
            "id": "mochi_glasses",
            "parent": "mochi_face",
            "texture": "parts/unyu_glasses.png",
            "z": 50,
            "pivot_px": [228.0, 194.0],
            "role": "mochi_glasses",
            "opacity": 1.0,
            "parallax": [0.48, 0.36],
            "rot_deg": 4.6,
            "bend_px": 0.0,
            "breath_px": [0.0, 0.4],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 14.0,
            "spring_zeta": 0.40,
        },
    ]

    overrides = {
        "10": "parts/unyu_clean_s10.png",
        "11": "parts/unyu_clean_s11.png",
        "12": "parts/unyu_clean_s12.png",
    }
    return parts, overrides


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate anatomical CC0-1.0 Live2D layered parts for neo_default.")
    parser.add_argument("--sakura-only", action="store_true", help="Regenerate only Sakura parts cleanly.")
    args = parser.parse_args()

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    PARTS_DIR.mkdir(parents=True, exist_ok=True)

    # Preserve existing clean Unyu bases before wiping so repeated runs remain 100% idempotent
    cached_clean: dict[str, np.ndarray] = {}
    for key, fname in (("s10", "unyu_clean_s10.png"), ("s11", "unyu_clean_s11.png"), ("s12", "unyu_clean_s12.png")):
        p = PARTS_DIR / fname
        if p.is_file():
            cached_clean[key] = load_rgba(p)

    if args.sakura_only:
        for old_png in PARTS_DIR.glob("sakura_*.png"):
            old_png.unlink()
    else:
        for old_png in PARTS_DIR.glob("*.png"):
            old_png.unlink()

    (OUT_DIR / "LICENSE-CC0.txt").write_text(CC0_NOTICE, encoding="utf-8")

    sakura_parts = build_sakura_parts()
    if args.sakura_only and (OUT_DIR / "live2d.json").is_file():
        existing_manifest = json.loads((OUT_DIR / "live2d.json").read_text(encoding="utf-8"))
        kero_block = existing_manifest["kero"]
        unyu_parts = kero_block["parts"]
    else:
        unyu_parts, unyu_overrides = build_unyu_parts(cached_clean or None)
        kero_block = {
            "canvas_size": [380, 420],
            "surface_overrides": unyu_overrides,
            "parts": unyu_parts,
        }

    manifest = {
        "schema_version": 1,
        "name": "neo_default Multi-Layer Anatomical Live2D Rig (CC0-1.0)",
        "sakura": {
            "canvas_size": [660, 1350],
            "parts": sakura_parts,
        },
        "kero": kero_block,
    }
    (OUT_DIR / "live2d.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    print(f"Generated {len(sakura_parts)} Sakura parts and {len(unyu_parts)} Unyu parts in {OUT_DIR}")


if __name__ == "__main__":
    main()
