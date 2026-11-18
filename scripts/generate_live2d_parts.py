#!/usr/bin/env python3
"""Generate complete unoccluded CC0-1.0 Live2D part PNGs and live2d.json for neo_default.

Combines Visible-Pixel Lock (preserving exact surface0.png silhouettes & line-art)
with Occluder-Bounded Amodal Completion from aligned See-Through & clean repair plates
(`assets/live2d/neo_default/plates/*.png`) so every separated layer (`head_base`,
`crown_hair`, `hair_back_l/r`, `neck`, `upper_torso`, `forearm_front`, `chest_bow`,
`coat_flap_l/r`, `skirt`, `leg_l/r`, `arm_r`) is a complete closed silhouette without
cutouts, missing bites, or leftover foreground shadows.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import cv2
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
from skimage.filters import sato
from skimage.graph import route_through_array

ROOT = Path(__file__).resolve().parent.parent
SHELL_DIR = ROOT / "assets" / "shells" / "neo_default"
OUT_DIR = ROOT / "assets" / "live2d" / "neo_default"
PARTS_DIR = OUT_DIR / "parts"
PLATES_DIR = OUT_DIR / "plates"

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


def match_rgb_to_ref(
    plate_rgb: np.ndarray,
    ref_rgb: np.ndarray,
    anchor_mask: np.ndarray,
) -> np.ndarray:
    """Match channel mean & scale of a generated plate to `ref_rgb` over shared visible anchors."""
    if int(anchor_mask.sum()) < 200:
        return plate_rgb
    p_vals = plate_rgb[anchor_mask].astype(np.float32)
    r_vals = ref_rgb[anchor_mask].astype(np.float32)
    out = plate_rgb.astype(np.float32).copy()
    for c in range(3):
        pm, ps = float(p_vals[:, c].mean()), max(float(p_vals[:, c].std()), 1.0)
        rm, rs = float(r_vals[:, c].mean()), max(float(r_vals[:, c].std()), 1.0)
        gain = np.clip(rs / ps, 0.78, 1.28)
        out[:, :, c] = (out[:, :, c] - pm) * gain + rm
    return np.clip(np.round(out), 0, 255).astype(np.uint8)


def decontaminate_outer_aa(
    rgba: np.ndarray,
    solid_threshold: int = 230,
    scope_mask: np.ndarray | None = None,
) -> np.ndarray:
    """Strip bright white/sky background fringe strictly along outer cuts; skip interior fades."""
    if scope_mask is not None:
        return rgba.copy()
    out = rgba.copy()
    a = out[:, :, 3]
    solid = a >= solid_threshold
    edge = (a > 0) & (a < solid_threshold)
    if scope_mask is not None:
        edge &= scope_mask
    if not np.any(solid) or not np.any(edge):
        return out
    _, (ni, nj) = distance_transform_edt(~solid, return_indices=True)
    luma = (
        0.299 * out[:, :, 0].astype(np.float32)
        + 0.587 * out[:, :, 1].astype(np.float32)
        + 0.114 * out[:, :, 2].astype(np.float32)
    )
    halo = edge & (luma > 185.0)
    for c in range(3):
        nearest_c = out[:, :, c][ni, nj].astype(np.float32)
        orig_c = out[:, :, c].astype(np.float32)
        blended = np.clip(np.round(0.25 * orig_c + 0.75 * nearest_c), 0, 255).astype(np.uint8)
        out[:, :, c][halo] = blended[halo]
    return out


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


def crisp_mask_weight(mask: np.ndarray, outer_allow: np.ndarray, feather: float = 0.45) -> np.ndarray:
    """Keep 100% opacity inside `mask` while feathering only the outer anti-aliased edge into `outer_allow`."""
    w = gaussian_filter(mask.astype(np.float32), sigma=feather) if feather > 0.0 else mask.astype(np.float32)
    return np.clip(np.maximum(mask.astype(np.float32), np.where(outer_allow, w, 0.0)), 0.0, 1.0)


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


def build_strand_seam_cost(s0: np.ndarray, is_skin: np.ndarray) -> np.ndarray:
    """Build Sato black-ridge + luminance cost map so Livewire cuts hug dark ink hair creases."""
    h, w = s0.shape[:2]
    yg = y_grid(h, w)
    rgb_f = s0[:, :, :3].astype(np.float32)
    r_f, g_f, b_f = rgb_f[:, :, 0], rgb_f[:, :, 1], rgb_f[:, :, 2]
    opaque = s0[:, :, 3] > 16
    luma = 0.299 * r_f + 0.587 * g_f + 0.114 * b_f
    is_hi = opaque & (yg <= 265) & (luma > 202) & (b_f > r_f + 5.0)
    ridge = sato(luma / 255.0, sigmas=[1.0, 1.8], black_ridges=True)
    ridge = ridge / max(float(ridge.max()), 1e-6)
    return (
        0.04
        + 1.8 * np.clip((luma - 85.0) / 115.0, 0.0, 1.5)
        + 2.2 * (1.0 - np.clip(ridge * 3.5, 0.0, 1.0))
        + 80.0 * is_hi.astype(np.float32)
        + 120.0 * is_skin.astype(np.float32)
        + 40.0 * (~opaque).astype(np.float32)
    )


def trace_strand_seam(
    seam_cost: np.ndarray,
    waypoints: list[tuple[int, int]],
    pad: int = 12,
    smooth_win: int = 5,
) -> list[tuple[float, float]]:
    """Trace an 8-connected chord-guided smooth curve along drawn dark hair strand valleys."""
    h, w = seam_cost.shape
    pts: list[tuple[float, float]] = []
    for (x0, y0), (x1, y1) in zip(waypoints[:-1], waypoints[1:]):
        ry0, ry1 = max(0, min(y0, y1) - pad), min(h, max(y0, y1) + pad + 1)
        rx0, rx1 = max(0, min(x0, x1) - pad), min(w, max(x0, x1) + pad + 1)
        sub_c = seam_cost[ry0:ry1, rx0:rx1].copy()
        yy, xx = np.mgrid[ry0:ry1, rx0:rx1].astype(np.float32)
        dx, dy = float(x1 - x0), float(y1 - y0)
        seg_len2 = max(dx * dx + dy * dy, 1.0)
        t = np.clip(((xx - x0) * dx + (yy - y0) * dy) / seg_len2, 0.0, 1.0)
        dist = np.hypot(xx - (x0 + t * dx), yy - (y0 + t * dy))
        sub_c = sub_c + 0.18 * dist
        path, _ = route_through_array(
            sub_c,
            (y0 - ry0, x0 - rx0),
            (y1 - ry0, x1 - rx0),
            fully_connected=True,
            geometric=True,
        )
        seg = [(float(px + rx0), float(py + ry0)) for py, px in path]
        if pts:
            seg = seg[1:]
        pts.extend(seg)
    if smooth_win >= 3 and len(pts) > smooth_win * 2:
        arr = np.array(pts, dtype=np.float32)
        k = smooth_win // 2
        sm = arr.copy()
        for i in range(k, len(arr) - k):
            sm[i] = arr[i - k : i + k + 1].mean(axis=0)
        pts = [(float(x), float(y)) for x, y in sm]
    return pts


def build_sakura_parts() -> list[dict]:
    s0 = load_rgba(SHELL_DIR / "surface0.png")
    h, w = s0.shape[:2]
    alpha = s0[:, :, 3]
    opaque = alpha > 16
    solid_inner = binary_erosion(opaque, iterations=1)
    solid_inner2 = binary_erosion(opaque, iterations=2)
    solid_full = binary_erosion(alpha >= 245, iterations=1)
    rgb = s0[:, :, :3].astype(np.int16)
    r, g, b = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]
    xg, yg = x_grid(h, w), y_grid(h, w)

    # Load offline aligned completion plates (generated once from See-Through + image-generator)
    p_back_hair = load_rgba(PLATES_DIR / "plate_back_hair.png")
    st_back_hair = load_rgba(PLATES_DIR / "st_back_hair.png")
    p_bald_head = load_rgba(PLATES_DIR / "plate_bald_head.png")
    st_face = load_rgba(PLATES_DIR / "st_face.png")
    st_ear_l = load_rgba(PLATES_DIR / "st_ear_l.png")
    st_ear_r = load_rgba(PLATES_DIR / "st_ear_r.png")
    st_neck = load_rgba(PLATES_DIR / "st_neck.png")
    p_coat_free = load_rgba(PLATES_DIR / "plate_coat_bow_free.png")
    p_torso_fit = load_rgba(PLATES_DIR / "plate_torso_bodice.png")
    st_topwear = load_rgba(PLATES_DIR / "st_topwear.png")
    p_skirt_full = load_rgba(PLATES_DIR / "plate_skirt_full.png")
    st_arm_r = load_rgba(PLATES_DIR / "st_arm_r.png")
    st_forearm = load_rgba(PLATES_DIR / "st_forearm_front.png")
    st_legwear = load_rgba(PLATES_DIR / "st_legwear.png")

    # Interior color predicates
    is_pure_hair = opaque & (b >= r + 22) & (b >= 155) & (r >= 88) & (g >= 95) & (yg <= 415)
    is_hair_like = opaque & (yg <= 415) & ((b > r + 10) | ((r < 118) & (b < 148)))
    is_pure_navy_skirt = opaque & (b >= r + 24) & (b <= 218) & (r >= 76) & (r <= 160) & (g >= 78) & (g <= 170) & (yg >= 662)
    is_yellow_ribbon = opaque & (r >= 195) & (g >= 188) & (b >= 105) & (b <= 195) & (r - b >= 28)
    is_pure_white_coat = opaque & (r >= 224) & (g >= 226) & (b >= 232) & (b >= r - 2) & (b - r <= 24)
    is_skin = opaque & (r >= 208) & (g >= 160) & (r >= b + 12)
    is_cyan_trim = opaque & (g >= 182) & (b >= 205) & (r <= 192) & ((g - r) >= 22)
    is_dark_crisp = opaque & (r < 125) & (g < 130) & (b < 170)

    seam_cost = build_strand_seam_cost(s0, is_skin)

    # ── 1) Ahoge (z=80) ─────────────────────────────────────────────────
    ah_up = opaque.copy()
    ah_up[123:, :] = False
    lbl_a, _ = label(ah_up)
    ah_cc = lbl_a == lbl_a[80, 305]
    ah_stem_l = trace_strand_seam(seam_cost, [(300, 122), (308, 130), (316, 136)])
    ah_stem_r = trace_strand_seam(seam_cost, [(316, 136), (318, 129), (314, 122)])
    ah_stem_poly = poly_mask((h, w), ah_stem_l + ah_stem_r)
    ah_mask = opaque & (ah_cc | (ah_stem_poly & solid_inner2 & (yg >= 123)))
    ah_outer_allow = opaque & (yg < 123) & (xg <= 380)
    save_rgba(extract_layer(s0, crisp_mask_weight(ah_mask, ah_outer_allow, 0.35)), PARTS_DIR / "sakura_ahoge.png")

    # ── 2) Hair Bangs Center (z=69) & Wing L/R (z=66, 67) ───────────────
    # Strictly curve-cut along drawn strand valleys from crown whorl (312,134) down to pointed tips!
    whorl_fade = np.clip((np.hypot(xg.astype(np.float32) - 312.0, yg.astype(np.float32) - 133.0) - 3.0) / 7.0, 0.0, 1.0)
    eye_lash_excl_r = ellipse_mask((h, w), 362.0, 278.0, 23.0, 32.0) | (opaque & (xg >= 380) & (xg <= 390) & (yg >= 274) & (yg <= 292) & (~is_pure_hair))

    bc_l = trace_strand_seam(
        seam_cost,
        [(312, 134), (295, 150), (286, 172), (279, 195), (275, 218), (276, 238), (281, 254), (291, 270), (296, 272), (304, 270), (318, 281)],
        pad=10,
    )
    bc_r = trace_strand_seam(
        seam_cost,
        [(318, 281), (328, 270), (334, 250), (338, 224), (346, 248), (358, 272), (355, 244), (348, 215), (341, 185), (330, 156), (312, 134)],
        pad=10,
    )
    bang_center_poly = poly_mask((h, w), bc_l + bc_r)
    bang_center_mask = binary_fill_holes(opaque & bang_center_poly & (~is_skin) & (~eye_lash_excl_r)) & bang_center_poly & (~is_skin) & (~eye_lash_excl_r) & solid_full
    bang_center_w = crisp_mask_weight(bang_center_mask, solid_full & bang_center_poly & (~is_skin), 0.35) * whorl_fade
    save_rgba(decontaminate_outer_aa(extract_layer(s0, bang_center_w), scope_mask=solid_inner), PARTS_DIR / "sakura_hair_bangs_center.png")

    chin_pts = [
        (262, 334), (264, 338), (268, 345), (272, 349),
        (276, 353), (280, 356), (284, 358), (288, 361), (292, 363), (296, 365),
        (300, 367), (304, 369), (308, 371), (312, 372), (316, 374), (320, 374),
        (324, 373), (328, 372), (332, 370), (336, 369), (340, 368), (344, 366),
        (348, 365), (352, 363), (356, 361), (360, 359), (364, 357), (368, 355),
        (372, 352), (376, 349), (380, 344), (384, 329), (390, 320),
    ]
    chin_y = np.interp(xg, [p[0] for p in chin_pts], [p[1] for p in chin_pts])
    cheek_jaw_zone = (xg >= 256) & (xg <= 390) & (yg >= 280) & (yg <= 375)
    is_blue_hair_strict = is_pure_hair | ((b >= r + 14) & (r <= 195))
    is_pale_cheek_skin_bleed = (r >= 192) & (g >= 165) & ((b - r) < 10)
    left_jaw_ink_owner = (xg >= 275) & (xg <= 322) & (yg >= 352) & (yg <= chin_y + 0.5) & (r >= 125)
    hair_cheek_clean = (~(cheek_jaw_zone & is_pale_cheek_skin_bleed)) & (~left_jaw_ink_owner)

    def _clean_hair_cheek(rgba: np.ndarray) -> np.ndarray:
        out = rgba.copy()
        out[(out[:, :, 3] > 0) & cheek_jaw_zone & (~hair_cheek_clean)] = 0
        return out

    lw_seam = trace_strand_seam(
        seam_cost,
        [
            (308, 134), (275, 156), (260, 178), (253, 202), (249, 228), (246, 258), (249, 286), (266, 314),
            (245, 306), (232, 294), (221, 278), (214, 256), (212, 238), (218, 218),
            (207, 242), (202, 265), (206, 285),
        ],
        pad=7,
    )
    wing_l_poly = poly_mask((h, w), lw_seam + [(185.0, 285.0), (185.0, 128.0), (308.0, 128.0)])
    wing_l_outer = (yg >= 134) & (yg <= 286) & (xg <= 238)
    wing_l_mask = binary_fill_holes(opaque & wing_l_poly & (~is_skin)) & wing_l_poly & (~is_skin) & hair_cheek_clean & (wing_l_outer | solid_full)
    wing_l_w = np.where(hair_cheek_clean, crisp_mask_weight(wing_l_mask, opaque & wing_l_poly & hair_cheek_clean & (wing_l_outer | solid_full), 0.35), 0.0) * whorl_fade
    save_rgba(_clean_hair_cheek(decontaminate_outer_aa(extract_layer(s0, wing_l_w), scope_mask=solid_inner)), PARTS_DIR / "sakura_hair_wing_l.png")

    rw_seam = trace_strand_seam(
        seam_cost,
        [
            (316, 134), (330, 156), (341, 185), (348, 215), (355, 244), (370, 258), (382, 272), (386, 286), (376, 304),
            (392, 294), (401, 272), (404, 244), (398, 222),
            (412, 244), (427, 268), (444, 280), (464, 276),
            (452, 260), (465, 266), (482, 270), (497, 266),
        ],
        pad=7,
    )
    wing_r_poly = poly_mask((h, w), rw_seam + [(508.0, 266.0), (508.0, 128.0), (316.0, 128.0)])
    wing_r_outer = (yg >= 134) & (yg <= 280) & (xg >= 425)
    wing_r_mask = binary_fill_holes(opaque & wing_r_poly & (~is_skin) & (~eye_lash_excl_r)) & wing_r_poly & (~is_skin) & (~eye_lash_excl_r) & hair_cheek_clean & (wing_r_outer | solid_full)
    wing_r_w = np.where(hair_cheek_clean, crisp_mask_weight(wing_r_mask, opaque & wing_r_poly & (~eye_lash_excl_r) & hair_cheek_clean & (wing_r_outer | solid_full), 0.35), 0.0) * whorl_fade
    save_rgba(_clean_hair_cheek(decontaminate_outer_aa(extract_layer(s0, wing_r_w), scope_mask=solid_inner)), PARTS_DIR / "sakura_hair_wing_r.png")

    # ── 3) Lower Wavy Curls L (z=63, over ears/head) & R (z=64) ─────────
    # Separate inner eye/mouth inpaint guard (head_base) from wider cheek guard (curls) to avoid chin shading steps.
    eye_and_cheek_guard = binary_dilation(
        ((ellipse_mask((h, w), 278.0, 280.0, 22.0, 32.0) | ellipse_mask((h, w), 366.0, 279.0, 21.0, 32.0)) & (yg <= 300))
        | poly_mask((h, w), [(263, 236), (382, 236), (366, 322), (342, 346), (298, 348), (268, 322)]),
        iterations=1,
    )
    curl_cheek_guard = eye_and_cheek_guard | (
        poly_mask((h, w), [(265, 240), (377, 240), (372, 312), (364, 348), (344, 362), (322, 370), (298, 360), (274, 342), (265, 316)])
        & (~is_blue_hair_strict)
        & (~((r < 92) & (g < 80) & (b < 148)))
    )

    real_collar_yellow = (yg >= 380) & ((xg <= 316) | (xg >= 375)) & is_yellow_ribbon & (~is_skin)
    collar_guard = (
        real_collar_yellow
        | (opaque & (yg >= 382) & (r >= 185) & (g >= 180) & (r >= b + 12))
        | (opaque & (yg >= 384) & is_pure_white_coat)
        | is_skin
        | curl_cheek_guard
    )

    cl_seam = trace_strand_seam(
        seam_cost,
        [(204, 262), (214, 212), (234, 186), (254, 206), (252, 248), (250, 290), (245, 310), (255, 330), (268, 350), (316, 374), (316, 398), (252, 404)],
        pad=8,
    )
    curl_l_poly = poly_mask((h, w), cl_seam + [(188.0, 404.0), (188.0, 262.0)]) | (ellipse_mask((h, w), 236.0, 232.0, 28.0, 48.0) & solid_full)
    wing_l_aa_owned = wing_l_mask & (~solid_full) & wing_l_outer
    curl_l_mask = opaque & curl_l_poly & (~collar_guard) & hair_cheek_clean & (is_hair_like | is_dark_crisp | (xg <= 265)) & (~wing_l_aa_owned)
    curl_l_outer = opaque & curl_l_poly & (~collar_guard) & hair_cheek_clean & (~wing_l_aa_owned) & solid_full
    curl_l_w = np.where(hair_cheek_clean, crisp_mask_weight(curl_l_mask, curl_l_outer, 0.35), 0.0)
    save_rgba(_clean_hair_cheek(decontaminate_outer_aa(extract_layer(s0, curl_l_w), scope_mask=solid_inner)), PARTS_DIR / "sakura_hair_curl_l.png")

    cr_seam = trace_strand_seam(
        seam_cost,
        [(472, 256), (452, 212), (426, 188), (402, 208), (400, 248), (392, 285), (378, 306), (374, 326), (372, 346), (370, 370), (378, 388), (402, 402)],
        pad=8,
    )
    curl_r_poly = poly_mask((h, w), cr_seam + [(515.0, 404.0), (515.0, 256.0)]) | (ellipse_mask((h, w), 426.0, 236.0, 32.0, 48.0) & solid_full)
    eye_corner_r_guard = ellipse_mask((h, w), 365.0, 278.0, 26.0, 23.0) | ((xg >= 350) & (xg <= 390) & (yg >= 260) & (yg <= 300))
    temple_r_sliver = (xg >= 383) & (xg <= 390) & (yg >= 260) & (yg <= 300) & is_pure_hair
    wing_r_aa_owned = wing_r_mask & (~solid_full) & wing_r_outer
    curl_r_mask = opaque & curl_r_poly & (~collar_guard) & hair_cheek_clean & (is_hair_like | is_dark_crisp | (xg >= 380)) & (~wing_r_aa_owned) & (~eye_corner_r_guard)
    curl_r_outer = opaque & curl_r_poly & (~collar_guard) & hair_cheek_clean & (~wing_r_aa_owned) & solid_full & (~eye_corner_r_guard)
    curl_r_w = np.where(hair_cheek_clean, crisp_mask_weight(curl_r_mask, curl_r_outer, 0.35), 0.0)
    save_rgba(_clean_hair_cheek(decontaminate_outer_aa(extract_layer(s0, curl_r_w), scope_mask=solid_inner)), PARTS_DIR / "sakura_hair_curl_r.png")

    # ── 4) Complete Round Bald Oval Head + Ears (`head_base`, z=58) ─────
    # Solid lower-face jawline down to `chin_y` with full un-notched ears, unbroken dark chin ink, and zero halo/shadow step.
    st_ears_bin = ((st_ear_l[:, :, 3] > 110) | (st_ear_r[:, :, 3] > 110))
    st_head_alpha = np.maximum(st_face[:, :, 3], np.maximum(st_ear_l[:, :, 3], st_ear_r[:, :, 3]))
    st_head_bin = binary_fill_holes(st_head_alpha > 110)
    pa_head_bin = p_bald_head[:, :, 3] > 140
    left_ear_jaw_bridge = poly_mask((h, w), [(250, 295), (266, 295), (266, 338), (255, 326)]) & solid_full & (curl_l_mask | st_head_bin)
    below_chin_neck = (xg >= 262) & (xg <= 386) & (yg > chin_y + 0.5) & (~st_ears_bin) & (~left_ear_jaw_bridge)
    jaw_solid_poly = poly_mask((h, w), [(258, 250), (388, 250)] + list(reversed(chin_pts))) & solid_full & (~below_chin_neck)
    vis_face_skin = is_skin & (yg >= 250) & (yg <= chin_y + 0.5) & (xg >= 255) & (xg <= 395) & solid_full & (~below_chin_neck)
    bald_dome_oval = ellipse_mask((h, w), 322.0, 242.0, 62.0, 82.0) & solid_full
    head_shape_core = (st_head_bin | vis_face_skin | jaw_solid_poly | left_ear_jaw_bridge | (pa_head_bin & bald_dome_oval)) & (~below_chin_neck)
    ear_under_curls = ((st_ear_l[:, :, 3] > 128) | (st_ear_r[:, :, 3] > 128)) & (curl_l_mask | curl_r_mask) & solid_full & (~below_chin_neck)
    head_max_env = (
        bald_dome_oval
        | jaw_solid_poly
        | left_ear_jaw_bridge
        | vis_face_skin
        | ear_under_curls
        | (st_ears_bin & solid_full)
    ) & (~below_chin_neck)
    head_oval = binary_fill_holes(head_shape_core) & head_max_env & solid_full & (~below_chin_neck)

    pa_head_matched = match_rgb_to_ref(p_bald_head[:, :, :3], s0[:, :, :3], head_oval & is_skin & (~eye_and_cheek_guard))
    chin_ink_keep = (r < 128) & (g < 88) & (yg >= chin_y - 3.5) & (yg <= chin_y + 0.5) & (xg >= 262) & (xg <= 384) & (~is_pure_hair)
    s0_jaw_lock = (
        head_oval
        & opaque
        & (((is_skin | ((r >= b - 2) & (yg >= 290))) & (~(b >= r + 8))) | (is_dark_crisp & (~is_pure_hair)) | chin_ink_keep)
        & (~eye_and_cheek_guard)
        & (~is_pure_hair)
        & (yg >= 265)
    )
    forehead_skin_seed = (
        head_oval
        & is_skin
        & (~binary_dilation(eye_and_cheek_guard, iterations=2))
        & (~binary_dilation(is_dark_crisp, iterations=2))
        & (~is_pure_hair)
        & (yg >= 225)
        & (yg <= 355)
    )
    smooth_skin_fill = diffuse_from_known(s0[:, :, :3], head_oval & (~s0_jaw_lock), forehead_skin_seed, iters=180)
    dome_mix = np.clip((255.0 - yg.astype(np.float32)) / 50.0, 0.0, 0.65)[:, :, None]
    pa_valid = ((p_bald_head[:, :, 3] > 180) & (p_bald_head[:, :, 0] > 80))[:, :, None]
    dome_rgb = np.where(
        pa_valid,
        np.clip(np.round(smooth_skin_fill.astype(np.float32) * (1.0 - dome_mix) + pa_head_matched.astype(np.float32) * dome_mix), 0, 255).astype(np.uint8),
        smooth_skin_fill,
    )
    ear_mask = ((st_ear_l[:, :, 3] > 128) | (st_ear_r[:, :, 3] > 128)) & head_oval
    st_ears_rgb = np.where((st_ear_l[:, :, 3] >= st_ear_r[:, :, 3])[:, :, None], st_ear_l[:, :, :3], st_ear_r[:, :, :3])
    head_rgb = np.where(s0_jaw_lock[:, :, None], s0[:, :, :3], np.where(ear_mask[:, :, None] & (~s0_jaw_lock[:, :, None]), st_ears_rgb, dome_rgb))
    ear_dark_ink = (ear_mask | (xg <= 262) | (xg >= 376)) & (head_rgb[:, :, 0] < 115) & (head_rgb[:, :, 1] < 85)
    # Scrub corrupt near-black [0,0,0], neck-shadow leak on right chin, or pale/gray jawline fringe while preserving dark ear/chin outlines
    hr_i = head_rgb[:, :, 0].astype(np.int16)
    hg_i = head_rgb[:, :, 1].astype(np.int16)
    hb_i = head_rgb[:, :, 2].astype(np.int16)
    right_chin_shadow_leak = (xg >= 316) & (yg >= 340) & (~chin_ink_keep) & (hg_i < 192) & ((hr_i - hg_i) >= 48)
    jaw_zone_hb = (yg >= 325) & (yg >= chin_y - 4.0) & (xg >= 262) & (xg <= 382)
    head_white_fringe = head_oval & (~chin_ink_keep) & (~ear_dark_ink) & (
        ((~s0_jaw_lock) & (yg <= 265) & (hr_i >= 215) & (hb_i >= hr_i - 2))
        | (jaw_zone_hb & (((hr_i >= 165) & ((hr_i - hb_i) < 22)) | right_chin_shadow_leak))
    )
    head_dark_leak = (head_oval & (~chin_ink_keep) & (~ear_dark_ink) & (hr_i < 35)) | head_white_fringe
    head_warm_ref = (
        head_oval
        & (~chin_ink_keep)
        & (~ear_dark_ink)
        & (~right_chin_shadow_leak)
        & (hr_i >= 220)
        & (hg_i >= 180)
        & ((hr_i - hb_i) >= 24)
        & ((hr_i - hg_i) >= 10)
        & (~head_white_fringe)
    )
    if np.any(head_dark_leak) and np.any(head_warm_ref):
        _, (ni, nj) = distance_transform_edt(~head_warm_ref, return_indices=True)
        for c in range(3):
            head_rgb[:, :, c][head_dark_leak] = head_rgb[:, :, c][ni, nj][head_dark_leak]

    head_a = crisp_mask_weight(head_oval, head_oval | solid_full, 0.35) * 255.0
    head_a_u8 = np.clip(np.round(head_a), 0, 255).astype(np.uint8)
    # Propagate clean interior head_rgb (chin ink / skin) into 1px outer AA feather so raw s0 background/halo never bleeds
    outer_aa_hb = (head_a_u8 > 0) & (~head_oval)
    if np.any(outer_aa_hb) and np.any(head_oval):
        _, (ni, nj) = distance_transform_edt(~head_oval, return_indices=True)
        for c in range(3):
            head_rgb[:, :, c][outer_aa_hb] = head_rgb[:, :, c][ni, nj][outer_aa_hb]

    sakura_head_base = np.zeros_like(s0)
    sakura_head_base[:, :, :3] = head_rgb
    sakura_head_base[:, :, 3] = head_a_u8
    sakura_head_base[sakura_head_base[:, :, 3] == 0, :3] = 0
    save_rgba(decontaminate_outer_aa(sakura_head_base, scope_mask=solid_inner), PARTS_DIR / "sakura_head_base.png")

    eye_brow_mouth_keep = (
        ellipse_mask((h, w), 278.0, 274.0, 41.0, 36.0)
        | ellipse_mask((h, w), 366.0, 272.0, 41.0, 36.0)
        | poly_mask((h, w), [(254, 228), (294, 228), (294, 248), (254, 248)])
        | poly_mask((h, w), [(334, 228), (386, 228), (386, 250), (334, 250)])
        | poly_mask((h, w), [(296, 305), (344, 305), (344, 348), (296, 348)])
    )

    # ── 5) Crown Scalp Hair Cap (`crown_hair`, z=61, underneath curls/bangs/wings) ──
    # Smooth cosine scalp arch + elliptical forehead cutout: zero straight horizontal or vertical cuts!
    eye_clear_for_crown = (
        ellipse_mask((h, w), 278.0, 278.0, 32.0, 34.0)
        | ellipse_mask((h, w), 366.0, 276.0, 28.0, 34.0)
    ) & (~((yg <= 256) & is_pure_hair)) & (~temple_r_sliver)
    arch_y = 218.0 + 68.0 * (1.0 - np.cos(np.clip((xg.astype(np.float32) - 318.0) / 145.0, -1.0, 1.0) * (np.pi / 2.0)))
    forehead_arch_cut = ellipse_mask((h, w), 320.0, 298.0, 52.0, 80.0)
    crown_raw_mask = (
        opaque
        & (yg >= 122)
        & (yg.astype(np.float32) <= arch_y)
        & (~ah_cc)
        & (~forehead_arch_cut)
        & (~eye_and_cheek_guard)
        & (~eye_clear_for_crown)
        & (~is_skin)
        & (is_hair_like | solid_inner)
        & (xg >= 198)
        & (xg <= 462)
    ) | (opaque & temple_r_sliver)
    front_fringes_opaque = wing_l_mask | wing_r_mask | bang_center_mask | curl_l_mask | curl_r_mask
    fg_outer_aa = (~solid_full) & (wing_l_mask | wing_r_mask | curl_l_mask | curl_r_mask | bang_center_mask)
    crown_vis = crown_raw_mask & (~front_fringes_opaque) & (~fg_outer_aa) & (~is_skin)
    crown_fill = crown_raw_mask & (~crown_vis) & solid_full
    crown_mask = crown_vis | crown_fill
    crown_known = crown_mask & is_pure_hair & (~front_fringes_opaque) & (~binary_dilation(is_skin, iterations=2)) & (yg >= 127) & (yg <= 265)
    crown_rgb = diffuse_from_known(s0[:, :, :3], crown_fill, crown_known, iters=120)
    sakura_crown = np.zeros_like(s0)
    sakura_crown[:, :, :3] = np.where(crown_vis[:, :, None], s0[:, :, :3], crown_rgb)
    crown_w = crisp_mask_weight(crown_mask, crown_vis | solid_full, 0.35)
    sakura_crown[:, :, 3] = np.clip(
        np.round(np.where(crown_vis, s0[:, :, 3].astype(np.float32), np.where(crown_fill, 255.0 * crown_w, 0.0))),
        0,
        255,
    ).astype(np.uint8)
    sakura_crown[sakura_crown[:, :, 3] == 0, :3] = 0
    save_rgba(decontaminate_outer_aa(sakura_crown, scope_mask=solid_inner), PARTS_DIR / "sakura_crown_hair.png")

    # ── 6) Complete Rounded Back Hair L/R (`hair_back_l/r`, z=14, 16) ──
    # Rounded See-Through cranium top + continuous curtain behind chin/neck/curls + curved S-wave L/R blend
    st_bh_mask = st_back_hair[:, :, 3] > 64
    white_coat_collar = (yg >= 382) & is_pure_white_coat
    hb_outer_s0 = opaque & (yg >= 123) & (yg <= 405) & is_hair_like & (~real_collar_yellow) & (~is_skin) & (~eye_and_cheek_guard)
    hb_curtain = poly_mask((h, w), [(230, 210), (428, 210), (428, 402), (276, 402), (230, 378)]) & solid_full & (~real_collar_yellow) & (~white_coat_collar)
    hb_inner_tuck = (st_bh_mask & solid_inner & (yg >= 125) & (yg <= 406) & (~((xg <= 275) & (yg > 378 + (xg - 230) * 24.0 / 46.0)))) | hb_curtain
    hb_full_shape = binary_fill_holes(hb_outer_s0 | hb_inner_tuck) & opaque & (yg >= 123) & (yg <= 406) & (~real_collar_yellow) & (~white_coat_collar)

    hb_owns_outer = hb_outer_s0 & (~curl_l_mask) & (~curl_r_mask) & (~crown_mask) & (~wing_l_mask) & (~wing_r_mask) & (~bang_center_mask) & (~((yg >= 382) & (~solid_full) & (xg >= 215) & (xg <= 440)))
    hb_fill_only = hb_full_shape & (~hb_owns_outer) & solid_full

    pb_matched = match_rgb_to_ref(p_back_hair[:, :, :3], st_back_hair[:, :, :3], (p_back_hair[:, :, 3] > 180) & st_bh_mask)
    pb_inside = (p_back_hair[:, :, 3] > 180) & hb_fill_only
    s0_hair_src = (curl_l_mask | curl_r_mask | wing_l_mask | wing_r_mask | crown_mask | bang_center_mask) & is_pure_hair & solid_full
    hb_inner_rgb = np.where(s0_hair_src[:, :, None], s0[:, :, :3], np.where(pb_inside[:, :, None], pb_matched, st_back_hair[:, :, :3]))
    hb_blue_valid = (hb_inner_rgb[:, :, 2].astype(np.int16) >= hb_inner_rgb[:, :, 0].astype(np.int16) + 18) & (hb_inner_rgb[:, :, 0] <= 165)
    hb_known_seed = hb_fill_only & (st_bh_mask | (p_back_hair[:, :, 3] > 180) | s0_hair_src) & hb_blue_valid
    hb_inner_rgb = diffuse_from_known(hb_inner_rgb, hb_fill_only & (~hb_known_seed), hb_known_seed, iters=120)
    center_ao = 1.0 - 0.14 * np.exp(-(((xg.astype(np.float32) - 332.0) / 72.0) ** 2 + ((yg.astype(np.float32) - 275.0) / 85.0) ** 2))[:, :, None]
    hb_inner_shaded = np.clip(np.round(hb_inner_rgb.astype(np.float32) * center_ao), 0, 255).astype(np.uint8)

    hb_rgb = np.where(hb_owns_outer[:, :, None], s0[:, :, :3], hb_inner_shaded)
    x_split = 332.0 + 14.0 * np.sin((yg.astype(np.float32) - 130.0) / 68.0)
    split_l = np.clip((x_split + 18.0 - xg.astype(np.float32)) / 36.0, 0.0, 1.0)
    split_r = np.clip((xg.astype(np.float32) - (x_split - 18.0)) / 36.0, 0.0, 1.0)
    hb_w = crisp_mask_weight(hb_fill_only, hb_owns_outer & solid_full, 0.35)
    hb_base_a = np.where(hb_owns_outer, s0[:, :, 3].astype(np.float32), np.where(hb_fill_only, 255.0 * hb_w, 0.0))

    hb_l = np.zeros_like(s0)
    hb_l[:, :, :3] = hb_rgb
    hb_l[:, :, 3] = np.clip(np.round(hb_base_a * split_l), 0, 255).astype(np.uint8)
    hb_r = np.zeros_like(s0)
    hb_r[:, :, :3] = hb_rgb
    hb_r[:, :, 3] = np.clip(np.round(hb_base_a * split_r), 0, 255).astype(np.uint8)
    save_rgba(decontaminate_outer_aa(hb_l, scope_mask=solid_inner), PARTS_DIR / "sakura_hair_back_l.png")
    save_rgba(decontaminate_outer_aa(hb_r, scope_mask=solid_inner), PARTS_DIR / "sakura_hair_back_r.png")

    # ── 7) Pure Hair-Free Face Features (`face_features`, z=60) + Masks ──
    face_poly = poly_mask(
        (h, w),
        [(250, 222), (393, 222), (392, 304), (368, 322), (344, 346), (320, 350), (298, 346), (274, 322), (250, 302)],
    )
    temple_hair_guard = is_pure_hair | bang_center_mask | wing_l_mask | wing_r_mask | curl_l_mask | curl_r_mask | crown_mask
    brow_hair_strip = (yg <= 257) & (bang_center_mask | crown_mask | wing_l_mask | wing_r_mask) & (b >= r + 10) & (b >= 155) & (r >= 80)
    face_hair_excl = (
        binary_dilation(temple_hair_guard & (~eye_brow_mouth_keep) & (~is_skin), iterations=1)
        | temple_r_sliver
        | brow_hair_strip
    )
    brow_preserve = (yg >= 232) & (yg <= 248) & ((xg <= 284) | (xg >= 334)) & (~bang_center_mask)
    excl_all = (binary_erosion(bang_center_mask | wing_l_mask | wing_r_mask | crown_mask, iterations=1) | (face_hair_excl & (yg <= 238))) & (~brow_preserve) & (~is_skin) & (~eye_brow_mouth_keep)

    bang_excl_rgba = np.zeros_like(s0)
    bang_excl_rgba[:, :, :3] = 255
    bang_excl_rgba[:, :, 3] = np.clip(np.round(crisp_mask_weight(excl_all, face_hair_excl & (~is_skin), 0.35) * 255.0), 0, 255).astype(np.uint8)
    save_rgba(bang_excl_rgba, PARTS_DIR / "sakura_bang_center_excl.png")

    face_hair_excl_eff = (face_hair_excl & (yg >= 235) & (~eye_brow_mouth_keep)) | temple_r_sliver | brow_hair_strip
    eye_ink_lock = eye_brow_mouth_keep & opaque & (~is_pure_hair) & (~temple_r_sliver) & (~brow_hair_strip)
    face_region_w = np.where(
        eye_ink_lock,
        soft_weight(face_poly, 1.8),
        soft_weight(face_poly, 1.8) * (1.0 - np.clip(soft_weight(face_hair_excl_eff, 0.7), 0.0, 1.0)),
    )
    face_mask_f = face_region_w * (1.0 - bang_excl_rgba[:, :, 3].astype(np.float32) / 255.0)
    face_rgba = extract_layer(s0, face_mask_f)
    # Strip any pale/gray cheek-border halo pixels below eyes outside mouth box
    fr_i = face_rgba[:, :, 0].astype(np.int16)
    fb_i = face_rgba[:, :, 2].astype(np.int16)
    mouth_box = (xg >= 302) & (xg <= 340) & (yg >= 308) & (yg <= 346)
    face_cheek_halo = (face_rgba[:, :, 3] > 0) & (~eye_brow_mouth_keep) & (~mouth_box) & (yg >= 310) & (fr_i >= 135) & ((fr_i - fb_i) < 22)
    face_rgba[face_cheek_halo] = 0
    save_rgba(face_rgba, PARTS_DIR / "sakura_face_features.png")

    fmask_rgba = np.zeros_like(s0)
    fmask_rgba[:, :, :3] = 255
    fmask_rgba[:, :, 3] = np.clip(np.round(face_region_w * 255.0), 0, 255).astype(np.uint8)
    save_rgba(fmask_rgba, PARTS_DIR / "sakura_face_mask.png")

    save_rgba(s0, PARTS_DIR / "sakura_hairpin_ref.png")
    save_rgba(np.zeros_like(s0), PARTS_DIR / "sakura_hairpin_slot.png")

    # ── 8) Anatomically Complete Rounded Neck (`sakura_neck.png`, z=38) ─
    # Combines ST 05_neck rounded upper mushroom dome & wide collarbone base with Visible-Pixel Lock on s0 throat
    # Guarantees [290..318, 360..390] stays strictly alpha=0 (regression guard).
    st_neck_shape = (st_neck[:, :, 3] > 110) & (xg >= 306) & (yg >= 332) & (yg <= 448)
    # Smoothly trim any intrusion into left chin guard [290..318, 360..390]
    left_chin_guard = (xg >= 289) & (xg <= 318) & (yg >= 359) & (yg <= 390)
    neck_shape_raw = (
        binary_fill_holes(st_neck_shape | poly_mask((h, w), [(324, 344), (360, 344), (363, 376), (372, 412), (350, 442), (314, 434), (318, 392)]))
        & (~left_chin_guard)
        & (~(opaque & (b >= r + 8) & (yg <= 395) & (~curl_r_mask) & (~curl_l_mask)))
    )
    is_warm_throat_skin = opaque & (r >= 195) & (g >= 140) & (r >= g + 10) & (r >= b + 12)
    is_neutral_white_px = (r >= 135) & ((r - b) < 22)
    neck_vis = (
        neck_shape_raw
        & opaque
        & (~head_oval)
        & (yg > chin_y)
        & (yg <= 408)
        & (xg >= 308)
        & (xg <= 370)
        & (~is_yellow_ribbon)
        & (~is_pure_white_coat)
        & (~is_neutral_white_px)
        & (~is_pure_hair)
        & (~(b >= r + 8))
        & (~curl_l_mask)
        & (~curl_r_mask)
    )
    neck_shape = (neck_vis | (neck_shape_raw & solid_full & (yg <= 418) & (xg >= 308) & (xg <= 372))) & (~left_chin_guard)
    neck_skin_seed = neck_vis & is_warm_throat_skin & (~binary_dilation(is_dark_crisp, iterations=1))
    neck_fill_rgb = diffuse_from_known(s0[:, :, :3], neck_shape & (~neck_vis), neck_skin_seed, iters=120)
    neck_rgb = np.where(neck_vis[:, :, None], s0[:, :, :3], neck_fill_rgb)
    neck_a = np.clip(
        np.round(np.where(neck_vis, s0[:, :, 3].astype(np.float32), crisp_mask_weight(neck_shape, (neck_vis | solid_full) & (~left_chin_guard), 0.35) * 255.0)),
        0,
        255,
    ).astype(np.uint8)
    neck_outer_aa = (neck_a > 0) & (~neck_shape) & (~neck_vis)
    if np.any(neck_outer_aa) and np.any(neck_shape):
        _, (ni, nj) = distance_transform_edt(~neck_shape, return_indices=True)
        for c in range(3):
            neck_rgb[:, :, c][neck_outer_aa] = neck_rgb[:, :, c][ni, nj][neck_outer_aa]
    sakura_neck = np.zeros_like(s0)
    sakura_neck[:, :, :3] = neck_rgb
    sakura_neck[:, :, 3] = neck_a
    sakura_neck[sakura_neck[:, :, 3] == 0, :3] = 0
    save_rgba(decontaminate_outer_aa(sakura_neck, scope_mask=solid_inner), PARTS_DIR / "sakura_neck.png")

    # Helper masks for outfit, bows, arms, and buttons
    btn_guard = (
        poly_mask((h, w), [(259, 575), (277, 575), (277, 595), (259, 595)])
        | poly_mask((h, w), [(321, 578), (343, 578), (343, 598), (321, 598)])
        | poly_mask((h, w), [(242, 649), (260, 649), (260, 669), (242, 669)])
        | poly_mask((h, w), [(319, 658), (338, 658), (338, 678), (319, 678)])
    )
    hand_front_skin = is_skin & (yg >= 442) & (yg <= 506) & (xg <= 300)

    # ── 9) Back Yellow Sailor Collar (`sakura_collar_back.png`, z=24) ────
    collar_b_poly = poly_mask((h, w), [(450, 444), (484, 444), (484, 506), (452, 506)])
    cb_vis = opaque & collar_b_poly & (is_yellow_ribbon | (binary_dilation(is_yellow_ribbon, iterations=2) & is_dark_crisp))
    cb_tuck_zone = poly_mask((h, w), [(436, 442), (460, 442), (464, 502), (446, 502)]) & (~cb_vis) & solid_full
    cb_all = cb_vis | cb_tuck_zone
    cb_rgb = diffuse_from_known(s0[:, :, :3], cb_tuck_zone, cb_vis & is_yellow_ribbon, iters=90)
    cb_out = np.zeros_like(s0)
    cb_out[:, :, :3] = np.where(cb_vis[:, :, None], s0[:, :, :3], cb_rgb)
    cb_out[:, :, 3] = np.clip(np.round(np.where(cb_tuck_zone, 255.0, s0[:, :, 3].astype(np.float32)) * crisp_mask_weight(cb_all, cb_vis | solid_full, 0.35)), 0, 255).astype(np.uint8)
    cb_out[cb_out[:, :, 3] == 0, :3] = 0
    save_rgba(decontaminate_outer_aa(cb_out, scope_mask=solid_inner), PARTS_DIR / "sakura_collar_back.png")

    # ── 10) Complete Symmetrical Chest Bow (`chest_bow`, z=50) ───────────
    bow_c_poly = poly_mask(
        (h, w),
        [(238, 460), (358, 460), (358, 512), (348, 550), (320, 574), (272, 574), (238, 534), (238, 480)],
    )
    pc_r, pc_g, pc_b, pc_a = p_coat_free[:, :, 0].astype(np.int16), p_coat_free[:, :, 1].astype(np.int16), p_coat_free[:, :, 2].astype(np.int16), p_coat_free[:, :, 3]
    pc_purple = (pc_a > 128) & bow_c_poly & (pc_b >= pc_r + 18) & (pc_b >= pc_g + 12) & (pc_r <= 185)
    lbl_pc, _ = label(binary_dilation(pc_purple, iterations=1))
    pc_main_l = lbl_pc[495, 300]
    pc_bow_fill = ((lbl_pc == pc_main_l) & pc_purple) if pc_main_l > 0 else pc_purple
    pc_dark = (pc_a > 128) & (pc_r < 125) & (pc_g < 130) & (pc_b < 170) & bow_c_poly
    pc_bow_mask = binary_fill_holes(pc_bow_fill | (binary_dilation(pc_bow_fill, iterations=1) & pc_dark))

    s0_bow_purple = opaque & bow_c_poly & (b >= r + 18) & (b >= g + 12) & (~is_cyan_trim) & (~is_skin)
    lbl_b, _ = label(binary_dilation(s0_bow_purple, iterations=1))
    main_lbl = lbl_b[495, 300]
    s0_bow_main = ((lbl_b == main_lbl) & s0_bow_purple) if main_lbl > 0 else s0_bow_purple
    s0_bow_vis = binary_fill_holes(s0_bow_main | (binary_dilation(s0_bow_main, iterations=1) & is_dark_crisp & bow_c_poly)) & (~binary_dilation(hand_front_skin, iterations=2))

    pc_bow_matched = match_rgb_to_ref(p_coat_free[:, :, :3], s0[:, :, :3], s0_bow_vis & s0_bow_purple)
    full_bow_mask = binary_fill_holes(pc_bow_mask | s0_bow_vis) & bow_c_poly & solid_inner & (~is_pure_white_coat)
    use_s0_bow = s0_bow_vis & opaque
    cbow_out = np.zeros_like(s0)
    cbow_out[:, :, :3] = np.where(use_s0_bow[:, :, None], s0[:, :, :3], pc_bow_matched)
    cbow_src_a = np.where(use_s0_bow, s0[:, :, 3].astype(np.float32), p_coat_free[:, :, 3].astype(np.float32))
    cbow_w = crisp_mask_weight(full_bow_mask, bow_c_poly & solid_inner & (~is_pure_white_coat), 0.35)
    cbow_out[:, :, 3] = np.clip(np.round(cbow_src_a * cbow_w), 0, 255).astype(np.uint8)
    cbow_out[cbow_out[:, :, 3] == 0, :3] = 0
    save_rgba(decontaminate_outer_aa(cbow_out, scope_mask=solid_inner), PARTS_DIR / "sakura_chest_bow.png")

    # ── 11) Complete Waist Bow (`waist_bow`, z=54) ──────────────────────
    bow_w_poly = poly_mask((h, w), [(320, 582), (426, 580), (434, 650), (416, 684), (344, 686), (336, 640), (320, 616)])
    wbow_yel = opaque & bow_w_poly & (r >= 190) & (g >= 182) & (b <= 212) & ((r - b) >= 20)
    lbl_w, _ = label(binary_dilation(wbow_yel, iterations=2))
    wmain_lbl = lbl_w[615, 375]
    wbow_main = (lbl_w == wmain_lbl) if wmain_lbl > 0 else wbow_yel
    wbow_filled = binary_fill_holes(wbow_main) & bow_w_poly & (~btn_guard)
    wbow_mask = binary_fill_holes(wbow_filled | (binary_dilation(wbow_filled, iterations=3) & is_dark_crisp & bow_w_poly & (~btn_guard) & (~is_pure_white_coat))) & (~btn_guard)
    wbow_w = crisp_mask_weight(wbow_mask, opaque & bow_w_poly & (~btn_guard) & (~is_pure_white_coat), 0.35)
    save_rgba(decontaminate_outer_aa(extract_layer(s0, wbow_w), scope_mask=solid_inner), PARTS_DIR / "sakura_waist_bow.png")

    # ── 12) Raised Right Forearm (`forearm_front`, z=52 — IN FRONT of chest_bow!) ──
    raised_arm_poly = poly_mask(
        (h, w),
        [(168, 442), (242, 434), (294, 464), (294, 514), (242, 540), (222, 608), (168, 608)],
    )
    arm_f_diff = (
        np.max(np.abs(s0[:, :, :3].astype(np.int16) - p_coat_free[:, :, :3].astype(np.int16)), axis=2) > 26
    ) | hand_front_skin
    front_collar_top_l = opaque & (yg <= 454) & (xg >= 220) & (xg <= 316) & is_yellow_ribbon & (~hand_front_skin)
    arm_f_seed = opaque & raised_arm_poly & (arm_f_diff | (xg <= 212)) & (~front_collar_top_l) & (yg >= 434)
    st_f_mask = (st_forearm[:, :, 3] > 140) & (yg >= 440) & (yg <= 609) & (xg <= 293) & solid_inner
    is_purple_bow_rgb = opaque & (b >= r + 18) & (b >= g + 12) & (r <= 185) & (xg >= 235) & (yg >= 464)
    bow_right_of_arm = (
        (s0_bow_vis & is_purple_bow_rgb)
        | (binary_dilation(s0_bow_vis & is_purple_bow_rgb, iterations=1) & (~binary_dilation(hand_front_skin, iterations=3)) & (~is_pure_white_coat) & (xg >= 248) & (yg >= 466))
    )
    arm_f_core = binary_fill_holes((arm_f_seed & (~bow_right_of_arm)) | (st_f_mask & (~front_collar_top_l) & (~bow_right_of_arm))) & raised_arm_poly & (~front_collar_top_l) & (~bow_right_of_arm) & opaque
    shoulder_l_fade = np.where(solid_full & (xg >= 241), np.clip((yg.astype(np.float32) - 432.0) / 8.0, 0.0, 1.0), 1.0)
    arm_f_w = crisp_mask_weight(arm_f_core, opaque & raised_arm_poly & (~front_collar_top_l) & (~bow_right_of_arm) & (yg >= 434), 0.35) * shoulder_l_fade
    forearm_out = extract_layer(s0, arm_f_w)
    save_rgba(decontaminate_outer_aa(forearm_out, scope_mask=solid_inner), PARTS_DIR / "sakura_forearm_front.png")

    # ── 13) Right Hanging Arm + Sleeve + Hand (`arm_r`, z=44) ──────────
    front_collar_guard = binary_dilation(opaque & (yg <= 454) & (xg <= 438) & is_yellow_ribbon, iterations=2)
    arm_r_poly = poly_mask(
        (h, w),
        [
            (396, 412),
            (462, 412),
            (544, 690),
            (546, 856),
            (480, 856),
            (464, 794),
            (466, 750),
            (455, 740),
            (447, 730),
            (438, 720),
            (432, 690),
            (422, 650),
            (394, 585),
            (394, 488),
        ],
    )
    hand_r_skin = opaque & (yg >= 742) & (xg >= 458) & arm_r_poly & is_skin
    hand_r_dil = binary_dilation(hand_r_skin, iterations=3) | ((xg >= 490) & binary_dilation(hand_r_skin, iterations=5))
    hand_r_solid = binary_fill_holes(hand_r_dil & opaque & arm_r_poly & (is_skin | is_dark_crisp | (r >= 195) | (g < 85)))
    arm_sleeve_vis = opaque & arm_r_poly & ((yg <= 752) | ((xg >= 464) & (yg <= 758))) & (~front_collar_guard) & (~cb_vis) & (~wbow_mask)
    arm_r_st_cap = (st_arm_r[:, :, 3] > 140) & (yg >= 416) & (yg <= 465) & arm_r_poly & (~cb_vis) & solid_inner
    arm_r_mask = (arm_sleeve_vis | hand_r_solid | arm_r_st_cap) & (~cb_vis) & (~wbow_mask)
    arm_r_vis = arm_sleeve_vis | hand_r_solid
    st_arm_matched = match_rgb_to_ref(st_arm_r[:, :, :3], s0[:, :, :3], arm_r_vis & (st_arm_r[:, :, 3] > 200))
    arm_r_out = np.zeros_like(s0)
    arm_r_out[:, :, :3] = np.where(arm_r_vis[:, :, None], s0[:, :, :3], st_arm_matched)
    arm_r_src_a = np.where(arm_r_vis, s0[:, :, 3].astype(np.float32), np.where(arm_r_st_cap, st_arm_r[:, :, 3].astype(np.float32), 0.0))
    arm_r_w = crisp_mask_weight(arm_r_mask, opaque & arm_r_poly & (~cb_vis) & ((yg <= 756) | hand_r_solid | arm_r_st_cap), 0.35)
    arm_r_out[:, :, 3] = np.clip(np.round(arm_r_src_a * arm_r_w), 0, 255).astype(np.uint8)
    arm_r_out[arm_r_out[:, :, 3] == 0, :3] = 0
    save_rgba(decontaminate_outer_aa(arm_r_out, scope_mask=solid_inner), PARTS_DIR / "sakura_arm_r.png")

    # ── 14) Anatomically Complete Armless Sailor Bodice (`upper_torso`, z=40) ──
    torso_poly = poly_mask(
        (h, w),
        [
            (182, 392),
            (232, 380),
            (258, 378),
            (322, 378),
            (322, 405),
            (356, 405),
            (369, 378),
            (412, 382),
            (448, 398),
            (456, 434),
            (420, 488),
            (414, 588),
            (418, 628),
            (172, 628),
        ],
    )
    hb_vis_only = hb_owns_outer & (~curl_l_mask) & (~curl_r_mask)
    shoulder_hair_excl = curl_l_mask | curl_r_mask | hb_vis_only | (
        (yg <= 396) & (~real_collar_yellow) & (~is_pure_white_coat) & is_pure_hair
    )
    collar_top_pts = [
        (180, 404), (225, 400), (260, 398), (275, 395), (290, 391),
        (305, 385), (315, 381), (320, 381), (368, 381), (375, 389),
        (385, 397), (398, 400), (415, 398), (430, 403), (456, 410),
    ]
    collar_top_y = np.interp(xg, [p[0] for p in collar_top_pts], [p[1] for p in collar_top_pts])
    vneck_skin_only = neck_vis & is_warm_throat_skin & (xg >= 310) & (xg <= 366) & (yg <= 406)
    curl_notch_white = (yg < collar_top_y) & ((xg <= 316) | (xg >= 370)) & (~is_yellow_ribbon)
    shoulder_hair_excl = shoulder_hair_excl | curl_notch_white
    pc_torso_ok = (p_coat_free[:, :, 3] > 140) & torso_poly & (yg >= collar_top_y) & (~vneck_skin_only) & (~arm_r_mask) & opaque & (~hb_vis_only)
    st_bodice_env = (st_topwear[:, :, 3] > 128) & torso_poly & (yg >= collar_top_y) & (~vneck_skin_only) & solid_inner & (~hb_vis_only)
    torso_base_mask = binary_fill_holes(pc_torso_ok | (opaque & torso_poly & (yg >= collar_top_y) & (~vneck_skin_only) & (~arm_r_mask) & (~arm_f_core) & (~hb_vis_only)) | (st_bodice_env & (yg >= 384) & (yg <= 622))) & torso_poly & (yg >= collar_top_y) & (~vneck_skin_only) & (~arm_r_mask) & (~cb_vis) & (~hb_vis_only) & opaque
    torso_collar_tuck = (
        torso_poly
        & (yg >= collar_top_y)
        & (yg <= 412)
        & (((xg >= 245) & (xg <= 318)) | ((xg >= 368) & (xg <= 424)))
        & (~vneck_skin_only)
        & (~neck_vis)
        & (~hb_vis_only)
        & solid_full
    )

    torso_rgb = match_rgb_to_ref(p_coat_free[:, :, :3], s0[:, :, :3], opaque & torso_poly & is_pure_white_coat & (~arm_f_core) & (~full_bow_mask))
    s0_torso_vis = opaque & torso_poly & (~vneck_skin_only) & (~arm_f_core) & (~full_bow_mask) & (~wbow_mask) & (~arm_r_mask) & (~shoulder_hair_excl) & (~((~solid_full) & (yg >= 580) & wbow_mask))
    torso_mask = s0_torso_vis | ((torso_base_mask | torso_collar_tuck) & solid_full)
    torso_rgb = np.where(s0_torso_vis[:, :, None], s0[:, :, :3], torso_rgb)
    true_collar_yellow = opaque & torso_poly & (yg >= 384) & (yg <= 440) & ((xg <= 322) | (xg >= 368)) & is_yellow_ribbon & (g >= r - 18)
    collar_inpaint = torso_mask & (~s0_torso_vis) & (yg <= 415) & (is_hair_like | (~true_collar_yellow) | torso_collar_tuck)
    collar_known = (
        true_collar_yellow
        & (~shoulder_hair_excl)
        & (~binary_dilation(is_hair_like, iterations=1))
    )
    torso_rgb = diffuse_from_known(torso_rgb, collar_inpaint, collar_known, iters=120)
    # Remove any leaked blue hair or white curl-notch fringe along upper collar (y<=406) without altering visible purple collar stripes
    torso_white_collar_leak = torso_mask & (yg <= 406) & ((xg <= 316) | (xg >= 368)) & (torso_rgb[:, :, 0] >= 215) & (torso_rgb[:, :, 1] >= 215) & (torso_rgb[:, :, 2] >= 208) & ((torso_rgb[:, :, 0].astype(np.int16) - torso_rgb[:, :, 2].astype(np.int16)) < 22)
    torso_blue_leak = (torso_mask & (~s0_torso_vis) & (torso_rgb[:, :, 2].astype(np.int16) >= torso_rgb[:, :, 0].astype(np.int16) + 15) & (yg <= 415)) | torso_white_collar_leak
    torso_collar_seed = torso_mask & (yg >= 384) & (yg <= 435) & (torso_rgb[:, :, 0].astype(np.int16) >= 195) & (torso_rgb[:, :, 1].astype(np.int16) >= 188) & ((torso_rgb[:, :, 0].astype(np.int16) - torso_rgb[:, :, 2].astype(np.int16)) >= 25) & (~torso_blue_leak)
    torso_rgb = diffuse_from_known(torso_rgb, torso_blue_leak, torso_collar_seed, iters=90)
    bow_under = full_bow_mask & torso_mask & (~btn_guard) & (yg >= 464) & (p_torso_fit[:, :, 3] > 180)
    pa_torso_matched = match_rgb_to_ref(p_torso_fit[:, :, :3], torso_rgb, (yg >= 520) & (yg <= 610) & (~full_bow_mask) & (p_torso_fit[:, :, 3] > 200) & torso_mask)
    bow_blend = soft_weight(binary_erosion(full_bow_mask, iterations=1), 2.5)[:, :, None] * bow_under[:, :, None].astype(np.float32)
    torso_rgb = np.clip(np.round(torso_rgb.astype(np.float32) * (1.0 - bow_blend) + pa_torso_matched.astype(np.float32) * bow_blend), 0, 255).astype(np.uint8)
    torso_leftover_hole = full_bow_mask & torso_mask & (~bow_under) & (~s0_torso_vis) & (yg >= 464)
    coat_known_t = torso_mask & is_pure_white_coat & (~full_bow_mask) & (~arm_f_core) & (yg >= 455)
    torso_rgb = diffuse_from_known(torso_rgb, torso_leftover_hole, coat_known_t, iters=120)

    waist_bot_w = np.clip((626.0 - yg.astype(np.float32)) / 12.0, 0.0, 1.0)
    torso_src_a = np.where(s0_torso_vis, s0[:, :, 3].astype(np.float32), np.where(torso_mask, 255.0, 0.0))
    torso_w = crisp_mask_weight(torso_mask, s0_torso_vis | solid_full, 0.35) * waist_bot_w
    sakura_torso = np.zeros_like(s0)
    sakura_torso[:, :, :3] = torso_rgb
    sakura_torso[:, :, 3] = np.clip(np.round(torso_src_a * torso_w), 0, 255).astype(np.uint8)
    sakura_torso[sakura_torso[:, :, 3] == 0, :3] = 0
    save_rgba(decontaminate_outer_aa(sakura_torso, scope_mask=solid_inner), PARTS_DIR / "sakura_upper_torso.png")

    # ── 15) Complete Unbitten Pleated Skirt (`sakura_skirt.png`, z=28) ──
    is_thigh_skin = opaque & (yg >= 845) & (r >= 210) & (g >= 165) & (r >= b + 14)
    skirt_comp_cand = (
        opaque & (yg >= 677) & (yg <= 884) & (~arm_r_mask) & (~btn_guard)
        & (b >= r + 24) & (r >= 48) & (r <= 165) & (g >= 54) & (g <= 172) & (b <= 222)
    )
    lbl_sk, n_sk = label(skirt_comp_cand)
    sk_vis = np.zeros((h, w), dtype=bool)
    for k in range(1, n_sk + 1):
        comp = lbl_sk == k
        if int(comp.sum()) >= 40:
            sk_vis |= comp
    sk_vis_full = binary_fill_holes(
        sk_vis
        | (binary_dilation(sk_vis, iterations=6) & (is_dark_crisp | ((yg >= 855) & (b >= r - 4) & (r <= 200))) & (yg >= 677) & (~arm_r_mask) & (~is_thigh_skin))
    ) | (binary_dilation(sk_vis, iterations=9) & opaque & (yg >= 840) & (yg <= 884) & (~arm_r_mask) & (~is_thigh_skin))

    pd_r, pd_g, pd_b, pd_a = p_skirt_full[:, :, 0].astype(np.int16), p_skirt_full[:, :, 1].astype(np.int16), p_skirt_full[:, :, 2].astype(np.int16), p_skirt_full[:, :, 3]
    pd_navy = (pd_a > 128) & (yg >= 602) & (yg <= 885) & (pd_b >= pd_r + 18) & (pd_b <= 230)
    lbl_pd, _ = label(binary_dilation(pd_navy, iterations=1))
    pd_main = (lbl_pd == lbl_pd[780, 300]) if lbl_pd[780, 300] > 0 else pd_navy
    pd_skirt_mask = binary_fill_holes(pd_main | (binary_dilation(pd_main, iterations=2) & (pd_r < 120) & (pd_g < 120) & (pd_b < 170) & (yg >= 602) & (yg <= 885)))

    coat_waist_guard = binary_erosion(opaque & (yg >= 600) & (yg <= 884) & (~is_thigh_skin), iterations=2)
    skirt_full_mask = sk_vis_full | (pd_skirt_mask & coat_waist_guard)
    pd_matched = match_rgb_to_ref(p_skirt_full[:, :, :3], s0[:, :, :3], sk_vis & pd_navy)
    sakura_skirt = np.zeros_like(s0)
    sakura_skirt[:, :, :3] = np.where(sk_vis_full[:, :, None], s0[:, :, :3], pd_matched)
    sk_src_a = np.where(sk_vis_full, s0[:, :, 3].astype(np.float32), 255.0)
    sk_w = crisp_mask_weight(skirt_full_mask, sk_vis_full, 0.35)
    sakura_skirt[:, :, 3] = np.clip(np.round(sk_src_a * sk_w), 0, 255).astype(np.uint8)
    sakura_skirt[sakura_skirt[:, :, 3] == 0, :3] = 0
    save_rgba(decontaminate_outer_aa(sakura_skirt, scope_mask=solid_inner), PARTS_DIR / "sakura_skirt.png")

    # ── 16) Clean Yellow-Bow-Free Coat Flaps L (`z=36`) & R (`z=34`) ────
    coat_vis_area = (
        (opaque & (yg >= 596) & (yg <= 816) & (~sk_vis_full) & (~arm_r_mask) & (~arm_f_core) & (~is_thigh_skin) & (~(wbow_mask & (~solid_full))))
        | (btn_guard & opaque & (yg >= 596))
    )
    coat_outer_trim = (~((yg <= 612) & (~solid_full))) & (~(sk_vis_full & (~solid_full)))
    x_mid_slit = 358.0 + 0.13 * np.maximum(yg.astype(np.float32) - 678.0, 0.0)

    pc_coat_matched = match_rgb_to_ref(p_coat_free[:, :, :3], s0[:, :, :3], coat_vis_area & is_pure_white_coat & (~wbow_mask))
    wbow_clean_patch = binary_dilation(wbow_mask, iterations=2) & (p_coat_free[:, :, 3] > 140) & solid_full & (~sk_vis_full)

    flap_l_mask = (coat_vis_area | wbow_clean_patch) & coat_outer_trim & (xg.astype(np.float32) <= x_mid_slit) & (xg >= 134)
    use_s0_flap_l = coat_vis_area & (~wbow_mask)
    flap_l_out = np.zeros_like(s0)
    flap_l_out[:, :, :3] = np.where(use_s0_flap_l[:, :, None], s0[:, :, :3], pc_coat_matched)
    flap_l_src_a = np.where(use_s0_flap_l, s0[:, :, 3].astype(np.float32), 255.0)
    flap_l_w = crisp_mask_weight(flap_l_mask, (use_s0_flap_l & coat_outer_trim & (xg.astype(np.float32) <= x_mid_slit + 1.0)) | solid_full, 0.35)
    flap_l_out[:, :, 3] = np.clip(np.round(flap_l_src_a * flap_l_w), 0, 255).astype(np.uint8)
    flap_l_out[flap_l_out[:, :, 3] == 0, :3] = 0
    save_rgba(decontaminate_outer_aa(flap_l_out, scope_mask=solid_inner), PARTS_DIR / "sakura_coat_flap_l.png")

    tuck_ext = np.where(yg <= 676, 16.0, 0.0)
    flap_r_base = (coat_vis_area | wbow_clean_patch) & coat_outer_trim & (xg.astype(np.float32) >= x_mid_slit - tuck_ext) & (xg <= 478)
    flap_r_base &= ~((yg <= 676) & (xg.astype(np.float32) < x_mid_slit) & (~solid_inner))
    arm_r_below = arm_r_mask & (yg >= 445)
    flap_r_under_arm = (
        solid_full & arm_r_below & binary_dilation(flap_r_base & (yg >= 650) & (yg <= 758), iterations=7) & (yg >= 650) & (yg <= 758) & (~wbow_mask)
    )
    flap_r_mask = flap_r_base | flap_r_under_arm
    use_s0_flap_r = coat_vis_area & (~wbow_mask) & (~flap_r_under_arm) & (xg.astype(np.float32) >= x_mid_slit)
    coat_known_r = flap_r_base & is_pure_white_coat & (~wbow_mask) & (~flap_r_under_arm)
    under_arm_rgb = diffuse_from_known(pc_coat_matched, flap_r_under_arm, coat_known_r, iters=120)
    flap_r_rgb = np.where(use_s0_flap_r[:, :, None], s0[:, :, :3], np.where(flap_r_under_arm[:, :, None], under_arm_rgb, pc_coat_matched))
    flap_r_src_a = np.where(use_s0_flap_r, s0[:, :, 3].astype(np.float32), np.where(flap_r_mask, 255.0, 0.0))
    flap_r_w = crisp_mask_weight(flap_r_mask, (use_s0_flap_r & coat_outer_trim & (xg.astype(np.float32) >= x_mid_slit - tuck_ext - 1.0)) | solid_full, 0.35)
    flap_r_out = np.zeros_like(s0)
    flap_r_out[:, :, :3] = flap_r_rgb
    flap_r_out[:, :, 3] = np.clip(np.round(flap_r_src_a * flap_r_w), 0, 255).astype(np.uint8)
    flap_r_out[flap_r_out[:, :, 3] == 0, :3] = 0
    save_rgba(decontaminate_outer_aa(flap_r_out, scope_mask=solid_inner), PARTS_DIR / "sakura_coat_flap_r.png")

    # ── 17) Complete Legs + Frilly Ribbon Socks + Mary Jane Shoes (`z=18, 19`) ──
    legs_zone = opaque & (yg >= 862)
    skirt_hem_overlap = (yg <= 883) & sk_vis_full
    legs_clean = legs_zone & (~skirt_hem_overlap)
    st_thigh_env = (st_legwear[:, :, 3] > 140) & (yg >= 752) & (yg <= 885) & (xg >= 220) & (xg <= 415) & solid_inner
    thigh_extend = st_thigh_env & (~legs_clean) & (yg <= 885)
    thigh_seed = legs_clean & (yg >= 878) & (yg <= 945) & (r >= 225) & (g >= 180) & (r >= b + 20)
    legs_rgb = diffuse_from_known(s0[:, :, :3], thigh_extend, thigh_seed, iters=140)
    legs_all_mask = legs_clean | thigh_extend

    x_sep = np.where(yg < 995, 323.0, 325.0 + 0.10 * np.maximum(yg.astype(np.float32) - 995.0, 0.0))
    leg_base_w = crisp_mask_weight(legs_all_mask, opaque | thigh_extend, 0.35)
    mask_l_f = leg_base_w * np.clip((x_sep + 10.0 - xg.astype(np.float32)) / 4.0, 0.0, 1.0) * np.where(xg.astype(np.float32) >= x_sep - 2.0, solid_full.astype(np.float32), 1.0)
    mask_r_f = np.clip((xg.astype(np.float32) - (x_sep - 4.0)) / 4.0, 0.0, 1.0) * leg_base_w

    legs_full = np.zeros_like(s0)
    legs_full[:, :, :3] = legs_rgb
    legs_full[:, :, 3] = np.where(legs_clean, s0[:, :, 3], np.where(thigh_extend, st_legwear[:, :, 3], 0))
    save_rgba(decontaminate_outer_aa(extract_layer(legs_full, mask_l_f), scope_mask=solid_inner), PARTS_DIR / "sakura_leg_l.png")
    save_rgba(decontaminate_outer_aa(extract_layer(legs_full, mask_r_f), scope_mask=solid_inner), PARTS_DIR / "sakura_leg_r.png")

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
            "id": "forearm_front",
            "parent": "upper_torso",
            "texture": "parts/sakura_forearm_front.png",
            "z": 52,
            "pivot_px": [212.0, 442.0],
            "role": "arm",
            "parallax": [0.16, 0.12],
            "rot_deg": 1.6,
            "bend_px": 2.2,
            "breath_px": [0.0, -1.2],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 8.0,
            "spring_zeta": 0.44,
            "phase_offset": 0.15,
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
            "z": 63,
            "pivot_px": [246.0, 188.0],
            "role": "hair_side_l",
            "parallax": [0.40, 0.20],
            "rot_deg": 3.6,
            "bend_px": 6.2,
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
            "pivot_px": [412.0, 188.0],
            "role": "hair_side_r",
            "parallax": [0.40, 0.20],
            "rot_deg": 3.8,
            "bend_px": 6.4,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 9.8,
            "spring_zeta": 0.34,
            "phase_offset": 0.72,
        },
        {
            "id": "crown_hair",
            "parent": "head_base",
            "texture": "parts/sakura_crown_hair.png",
            "z": 61,
            "pivot_px": [330.0, 170.0],
            "role": "head",
            "parallax": [0.0, 0.0],
            "rot_deg": 0.0,
            "bend_px": 0.0,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
        },
        {
            "id": "hair_wing_l",
            "parent": "head_base",
            "texture": "parts/sakura_hair_wing_l.png",
            "z": 66,
            "pivot_px": [266.0, 166.0],
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
            "pivot_px": [392.0, 156.0],
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
            "pivot_px": [314.0, 140.0],
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
            "pivot_px": [312.0, 130.0],
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
    lens_col = (30, 36, 62, 102)
    glare_col = (150, 170, 222, 118)

    lx, ly, lr_out, lr_in = 180.0, 201.5, 27.6, 22.8
    rx, ry, rr_out, rr_in = 277.5, 195.0, 26.8, 22.2

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

    # 1) Tuft (z=20) with smooth inner root overlap so swaying tuft never peels away from dome
    tuft_poly = poly_mask((h, w), [(10, 26), (114, 26), (112, 66), (102, 102), (92, 132), (10, 148)])
    tuft_mask = opaque & tuft_poly
    save_rgba(extract_layer(u10, soft_weight(tuft_mask, 0.8)), PARTS_DIR / "unyu_tuft.png")

    # 2) Clean Spotless Round Mochi Body (z=10) with continuous left curve behind tuft
    interior_body = binary_erosion(opaque, iterations=3)
    interior_1px = binary_erosion(opaque, iterations=1) & (~((r < 80) & (g < 80) & (b < 140)))
    gloss_preserve = poly_mask((h, w), [(220, 45), (365, 45), (365, 118), (220, 118)])
    eyebrow_erase = poly_mask((h, w), [(132, 112), (314, 112), (314, 150), (132, 150)]) & binary_dilation(r < 105, iterations=2)
    face_wipe = poly_mask((h, w), [(104, 148), (322, 148), (322, 260), (104, 260)])
    paw_wipe = poly_mask((h, w), [(116, 232), (212, 232), (212, 330), (116, 330)]) | poly_mask(
        (h, w), [(246, 226), (340, 226), (340, 326), (246, 326)]
    )
    temple_l_wipe = poly_mask((h, w), [(32, 160), (156, 186), (156, 206), (30, 184)])
    temple_r_wipe = poly_mask((h, w), [(298, 184), (344, 144), (350, 166), (304, 201)])
    temple_erase = (temple_l_wipe | temple_r_wipe) & binary_dilation((~is_pure_periwinkle) & (r < 135), iterations=2)
    blush_body_wipe = ellipse_mask((h, w), 318.0, 202.0, 34.0, 24.0) & binary_dilation(is_pink_blush, iterations=3)

    body_erase = (((eyebrow_erase | face_wipe | paw_wipe | temple_erase) & interior_body) | (blush_body_wipe & interior_1px)) & (~gloss_preserve)
    pure_periwinkle_core = binary_erosion(is_pure_periwinkle, iterations=2)
    not_right_rim = ~((xg >= 272) & (yg <= 212) & (r >= 160))
    body_known = binary_erosion(opaque, iterations=3) & pure_periwinkle_core & not_right_rim & (~body_erase) & (~tuft_mask)
    body_rgb = diffuse_from_known(u10[:, :, :3], body_erase, body_known, iters=220)

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

    # Smooth rounded top-left cranium behind tuft (replaces angular diagonal slice at y=48..148)
    dome_pts_l = [
        (134.0, 52.0),
        (110.0, 62.0),
        (90.0, 82.0),
        (74.0, 108.0),
        (60.0, 134.0),
        (51.0, 154.0),
    ]
    smooth_dome_cut = poly_mask((h, w), [(4, 20), (134, 20)] + dome_pts_l + [(4, 156)])
    patch_l = (yg >= 164) & (yg <= 196) & (xg <= 64)
    patch_r = (yg >= 142) & (yg <= 178) & (xg >= 316)
    body_mask = (opaque | (xg >= 90) & (xg <= 140) & (yg >= 52) & (yg <= 145)) & (~smooth_dome_cut)
    body_mask = body_mask & (opaque | tuft_mask | poly_mask((h, w), [(134, 52), (134, 154), (51, 154)] + dome_pts_l))
    body_mask = np.where(patch_l, xg.astype(np.float32) >= fit_x_l, body_mask)
    body_mask = np.where(patch_r, xg.astype(np.float32) <= fit_x_r, body_mask)

    body_rgb = diffuse_from_known(
        body_rgb,
        body_mask & (patch_l | patch_r | ((~opaque) & tuft_mask) | ((yg <= 156) & (xg <= 136) & (~pure_periwinkle_core))) & (~pure_periwinkle_core),
        body_known,
        iters=140,
    )

    unyu_body = np.zeros_like(u10)
    unyu_body[:, :, :3] = body_rgb
    unyu_body[:, :, 3] = np.clip(np.round(soft_weight(body_mask, 0.8) * 255.0), 0, 255).astype(np.uint8)
    contour_im = Image.fromarray(unyu_body)
    draw_c = ImageDraw.Draw(contour_im)
    draw_c.line(dome_pts_l, fill=(22, 22, 82, 255), width=4, joint="curve")
    pts_l = [(float(poly_l(yy_f)) + 1.8, float(yy_f)) for yy_f in range(160, 201, 3)]
    pts_r = [(float(poly_r(yy_f)) - 1.8, float(yy_f)) for yy_f in range(138, 183, 3)]
    draw_c.line(pts_l, fill=(22, 22, 82, 255), width=4, joint="curve")
    draw_c.line(pts_r, fill=(22, 22, 82, 255), width=4, joint="curve")
    unyu_body = np.array(contour_im, dtype=np.uint8)
    unyu_body[:, :, 3][~body_mask] = 0
    unyu_body[unyu_body[:, :, 3] == 0, :3] = 0
    save_rgba(unyu_body, PARTS_DIR / "unyu_body.png")

    paw_l, paw_r = _render_unyu_paws_4x((h, w), unyu_body)
    save_rgba(paw_l, PARTS_DIR / "unyu_paw_l.png")
    save_rgba(paw_r, PARTS_DIR / "unyu_paw_r.png")
    unyu_glasses = _render_unyu_glasses_4x((h, w))
    save_rgba(unyu_glasses, PARTS_DIR / "unyu_glasses.png")

    def clean_base_for(expr_arr: np.ndarray, mode: str) -> np.ndarray:
        out = unyu_body.copy()
        out[tuft_mask] = u10[tuft_mask]
        er = expr_arr[:, :, 0].astype(np.int16)
        eb = expr_arr[:, :, 2].astype(np.int16)

        brow_l = poly_mask((h, w), [(142, 116), (195, 116), (195, 149), (142, 149)])
        brow_r = poly_mask((h, w), [(252, 112), (303, 112), (303, 149), (252, 149)])
        brow_mask = (brow_l | brow_r) & binary_dilation((er < 115) & (eb < 155), iterations=1)
        out[brow_mask] = expr_arr[brow_mask]

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

        out = _paint_blush_ellipse(out, 140.0, 205.0, 22.5, 10.5)
        out = _paint_blush_ellipse(out, 308.0, 196.0, 20.5, 9.8)
        return out

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

    face_patch_box = poly_mask((h, w), [(106, 106), (332, 106), (332, 260), (106, 260)])
    interior_face = binary_erosion(opaque, iterations=10)
    face_diff = face_patch_box & interior_face & (
        np.max(np.abs(clean_s10[:, :, :3].astype(np.int16) - unyu_body[:, :, :3].astype(np.int16)), axis=2) > 16
    )
    unyu_face = extract_layer(clean_s10, soft_weight(face_diff, 0.7))
    save_rgba(unyu_face, PARTS_DIR / "unyu_face.png")

    save_rgba(np.zeros_like(u10), PARTS_DIR / "unyu_crown_slot.png")

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
