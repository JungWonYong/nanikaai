#!/usr/bin/env python3
"""Generate clean CC0-1.0 multi-layer Live2D part PNGs and sidecar live2d.json for neo_default.

Segments the bundled @2x character sheets (`surface0.png` Sakura and `surface10.png` Unyu)
into anatomically independent RGBA layers with smooth Laplace-diffused background inpainting
behind occluded regions so hierarchical bone + damped-spring animations never expose gaps.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw
from scipy.ndimage import binary_dilation, binary_erosion, distance_transform_edt, gaussian_filter

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
    Image.fromarray(arr.astype(np.uint8), "RGBA").save(path)


def inpaint_biharmonic_like(rgb: np.ndarray, fill_mask: np.ndarray, known_mask: np.ndarray, iters: int = 120) -> np.ndarray:
    """Smoothly diffuse RGB strictly from known_mask pixels into fill_mask pixels via seeded Laplace relaxation."""
    work = rgb.astype(np.float32).copy()
    if not np.any(fill_mask) or not np.any(known_mask):
        return np.clip(np.round(work), 0, 255).astype(np.uint8)

    _, (ni, nj) = distance_transform_edt(~known_mask, return_indices=True)
    for c in range(3):
        init_c = work[:, :, c][ni, nj]
        work[:, :, c][fill_mask] = init_c[fill_mask]

    ys, xs = np.where(fill_mask)
    y0, y1 = max(1, int(ys.min()) - 2), min(rgb.shape[0] - 1, int(ys.max()) + 3)
    x0, x1 = max(1, int(xs.min()) - 2), min(rgb.shape[1] - 1, int(xs.max()) + 3)
    sub_fill = fill_mask[y0:y1, x0:x1]
    sub = work[y0:y1, x0:x1]

    for _ in range(iters):
        avg = 0.25 * (
            work[y0 - 1 : y1 - 1, x0:x1]
            + work[y0 + 1 : y1 + 1, x0:x1]
            + work[y0:y1, x0 - 1 : x1 - 1]
            + work[y0:y1, x0 + 1 : x1 + 1]
        )
        sub[sub_fill] = avg[sub_fill]

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


def build_sakura_parts() -> list[dict]:
    s0 = load_rgba(SHELL_DIR / "surface0.png")
    h, w = s0.shape[:2]
    alpha = s0[:, :, 3]
    opaque = alpha > 16
    rgb = s0[:, :, :3].astype(np.int16)
    r, g, b = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]
    xg, yg = x_grid(h, w), y_grid(h, w)

    # Pure color interior predicates (avoid dark line art r<95 bleeding into inpaint fills!)
    is_pure_hair = opaque & (b >= r + 22) & (b >= 155) & (r >= 95) & (g >= 120)
    is_pure_navy_skirt = opaque & (b >= r + 26) & (b <= 215) & (r >= 78) & (r <= 155) & (g >= 80) & (g <= 165) & (yg >= 665)
    is_yellow_ribbon = opaque & (r >= 195) & (g >= 188) & (b >= 110) & (b <= 192) & (r - b >= 32)
    is_purple_bow = opaque & (b >= r + 18) & (r >= 108) & (r <= 215) & (g >= 108) & (g <= 212)
    is_pure_white_coat = opaque & (r >= 224) & (g >= 226) & (b >= 232) & (b - r <= 24)
    is_skin = opaque & (r >= 205) & (g >= 158) & (r >= b + 12)

    # 1) Ahoge: strictly above crown dome curve so zero dark dome line detaches
    dome_cut_y = 123.5 + 0.0045 * ((xg - 318.0) ** 2)
    ahoge_region = opaque & (yg <= dome_cut_y) & (xg >= 272) & (xg <= 388) & (yg >= 44)
    sakura_ahoge = extract_layer(s0, soft_weight(ahoge_region, 0.7))
    save_rgba(sakura_ahoge, PARTS_DIR / "sakura_ahoge.png")

    # 2) Side hair curls (outer floating curl tips below y=245, softly feathered along top attachment)
    # Kept cleanly outside the head dome interior so NO black carve scars appear!
    hair_like = opaque & (yg <= 415) & ((b > r + 10) | ((r < 115) & (b < 145))) & (~is_skin)
    curl_l_poly = poly_mask((h, w), [(170, 240), (236, 240), (234, 320), (252, 412), (170, 412)])
    curl_r_poly = poly_mask((h, w), [(412, 238), (522, 238), (522, 412), (402, 412), (414, 320)])
    collar_guard = is_yellow_ribbon | is_pure_white_coat | is_skin
    curl_l_raw = (curl_l_poly & opaque & hair_like & (~collar_guard)).astype(np.float32)
    curl_r_raw = (curl_r_poly & opaque & hair_like & (~collar_guard)).astype(np.float32)
    # Smooth 30px vertical ramp at attachment (y=240..272) + smooth 14px inner ramp toward cheek
    top_ramp_l = np.clip((yg.astype(np.float32) - 240.0) / 30.0, 0.0, 1.0)
    top_ramp_r = np.clip((yg.astype(np.float32) - 238.0) / 30.0, 0.0, 1.0)
    inner_ramp_l = np.clip((236.0 - xg.astype(np.float32)) / 14.0, 0.0, 1.0)
    inner_ramp_r = np.clip((xg.astype(np.float32) - 410.0) / 14.0, 0.0, 1.0)
    curl_l_w = np.clip(gaussian_filter(curl_l_raw, 0.8) * np.minimum(top_ramp_l, np.maximum(inner_ramp_l, (yg > 315).astype(np.float32))), 0.0, 1.0)
    curl_r_w = np.clip(gaussian_filter(curl_r_raw, 0.8) * np.minimum(top_ramp_r, np.maximum(inner_ramp_r, (yg > 315).astype(np.float32))), 0.0, 1.0)
    save_rgba(extract_layer(s0, curl_l_w), PARTS_DIR / "sakura_hair_side_l.png")
    save_rgba(extract_layer(s0, curl_r_w), PARTS_DIR / "sakura_hair_side_r.png")

    # 3) Chest purple bow (x: 234..370, y: 462..578)
    bow_c_zone = poly_mask((h, w), [(234, 464), (270, 456), (368, 460), (370, 578), (234, 578)])
    bow_seed = bow_c_zone & is_purple_bow
    bow_core = binary_dilation(bow_seed, iterations=3) & bow_c_zone & opaque & (~is_skin) & (~is_pure_white_coat)
    save_rgba(extract_layer(s0, soft_weight(bow_core, 0.7)), PARTS_DIR / "sakura_chest_bow.png")

    # 4) Waist yellow bow (x: 314..434, y: 578..688)
    bow_w_zone = poly_mask((h, w), [(314, 578), (434, 578), (434, 688), (314, 688)])
    wbow_seed = bow_w_zone & is_yellow_ribbon
    wbow_core = binary_dilation(wbow_seed, iterations=3) & bow_w_zone & opaque & (~is_pure_white_coat) & (~is_pure_navy_skirt)
    save_rgba(extract_layer(s0, soft_weight(wbow_core, 0.7)), PARTS_DIR / "sakura_waist_bow.png")

    # 5) Head dome + face + bangs:
    # Keep full intact head & face! Only exclude ahoge (y < dome_cut_y) and chin neck/collar!
    # Inside outermost lower curls (x < 224 or x > 424 below y=275), fill smoothly with pure hair blue & fade outer overhang!
    chin_poly = poly_mask(
        (h, w),
        [
            (180, 118),
            (490, 118),
            (490, 398),
            (416, 405),
            (382, 378),
            (350, 386),
            (322, 389),
            (292, 384),
            (265, 372),
            (225, 404),
            (180, 402),
        ],
    )
    head_base_mask = opaque & chin_poly & (~(yg < 122))
    side_l_core = curl_l_w > 0.70
    side_r_core = curl_r_w > 0.70
    # Smooth diagonal-curved trim removing only outer dangling curls below the temple (never a horizontal shelf cut!)
    outer_l_wing = poly_mask((h, w), [(165, 240), (222, 286), (226, 410), (165, 410)])
    outer_r_wing = poly_mask((h, w), [(424, 286), (525, 240), (525, 410), (420, 410)])
    wing_keep_w = np.clip(1.0 - soft_weight(outer_l_wing | outer_r_wing, 3.2), 0.0, 1.0)
    head_alpha_mask = head_base_mask & (~(outer_l_wing | outer_r_wing))
    # Softly fill the narrow overlap behind curls using pure bright hair blue
    overlap_l = head_base_mask & poly_mask((h, w), [(208, 255), (242, 255), (242, 392), (208, 392)]) & (~is_skin)
    overlap_r = head_base_mask & poly_mask((h, w), [(406, 255), (442, 255), (442, 392), (406, 392)]) & (~is_skin)
    head_fill = overlap_l | overlap_r
    head_known = head_base_mask & is_pure_hair & (~binary_dilation(head_fill | side_l_core | side_r_core, iterations=2))
    head_rgb = inpaint_biharmonic_like(s0[:, :, :3], head_fill, head_known, iters=75)
    sakura_head = s0.copy()
    sakura_head[:, :, :3] = head_rgb
    head_soft_alpha = soft_weight(head_base_mask, 0.8) * wing_keep_w
    sakura_head = extract_layer(sakura_head, head_soft_alpha)
    save_rgba(sakura_head, PARTS_DIR / "sakura_head.png")

    # sakura_head_mask.png: full face + bangs + upper dome + star clip zone (200..438, 130..394), excluding ahoge & side_l/side_r cores
    dyn_mask = np.zeros((h, w), dtype=np.uint8)
    face_allow = chin_poly & poly_mask((h, w), [(224, 130), (416, 130), (416, 392), (224, 392)])
    star_clip_zone = poly_mask((h, w), [(202, 134), (278, 134), (278, 196), (202, 196)])
    combined_allow = (face_allow | star_clip_zone) & (~ahoge_region) & (~side_l_core) & (~side_r_core)
    inside_head_or_clip = binary_dilation(head_alpha_mask, iterations=3) | star_clip_zone
    dyn_mask[combined_allow & inside_head_or_clip] = 255
    dyn_mask_rgba = np.zeros((h, w, 4), dtype=np.uint8)
    dyn_mask_rgba[:, :, :3] = 255
    dyn_mask_rgba[:, :, 3] = dyn_mask
    save_rgba(dyn_mask_rgba, PARTS_DIR / "sakura_head_mask.png")

    # 6) Back hair layer (behind neck/collar y=260..406): fill strictly from bright is_pure_hair only!
    hair_back_zone = poly_mask((h, w), [(205, 250), (475, 250), (475, 408), (205, 408)])
    hb_known = hair_back_zone & is_pure_hair & ((xg <= 265) | (xg >= 390))
    hb_shape = poly_mask((h, w), [(212, 260), (468, 260), (470, 395), (415, 405), (242, 405), (210, 385)]) & (binary_dilation(opaque & hair_like & (yg >= 260), iterations=2) | poly_mask((h, w), [(245, 275), (412, 275), (414, 402), (245, 402)]))
    hb_rgb = inpaint_biharmonic_like(s0[:, :, :3], hb_shape & (~hb_known), hb_known, iters=120)
    sakura_hair_back = np.zeros_like(s0)
    sakura_hair_back[:, :, :3] = hb_rgb
    sakura_hair_back[:, :, 3] = np.clip(np.round(soft_weight(hb_shape, 1.2) * 255.0), 0, 255).astype(np.uint8)
    sakura_hair_back[sakura_hair_back[:, :, 3] == 0, :3] = 0
    save_rgba(sakura_hair_back, PARTS_DIR / "sakura_hair_back.png")

    # 7) Legs + Socks + Boots (y >= 864, thigh top extended up to y=836 behind skirt)
    legs_zone = opaque & (yg >= 864)
    skirt_fringe = (yg <= 885) & (b > r + 14) & (r < 175)
    legs_clean = legs_zone & (~skirt_fringe)
    thigh_env = poly_mask((h, w), [(236, 836), (398, 836), (400, 886), (238, 886)])
    thigh_extend = thigh_env & (~legs_clean) & (yg <= 886)
    thigh_seed = legs_clean & (yg >= 868) & (yg <= 925) & is_skin
    legs_rgb = inpaint_biharmonic_like(s0[:, :, :3], thigh_extend, thigh_seed, iters=80)
    legs_final_mask = legs_clean | thigh_extend
    sakura_legs = np.zeros_like(s0)
    sakura_legs[:, :, :3] = legs_rgb
    sakura_legs[:, :, 3] = np.clip(np.round(soft_weight(legs_final_mask, 0.8) * 255.0), 0, 255).astype(np.uint8)
    sakura_legs[sakura_legs[:, :, 3] == 0, :3] = 0
    save_rgba(sakura_legs, PARTS_DIR / "sakura_legs_boots.png")

    # 8) Pleated Navy Skirt (z=30):
    # CRITICAL: stay strictly INSIDE opaque character boundary + never leak out left/right of coat flaps!
    hand_r_zone = poly_mask((h, w), [(464, 725), (540, 725), (540, 835), (464, 835)])
    skirt_vis_poly = poly_mask((h, w), [(120, 665), (498, 665), (506, 845), (365, 886), (225, 884), (118, 805)])
    skirt_seed = opaque & skirt_vis_poly & ((b >= r + 18) & (b <= 215) & (r <= 170) & (g <= 175)) & (~hand_r_zone)
    skirt_known = skirt_seed & is_pure_navy_skirt & (~binary_dilation(is_pure_white_coat | is_skin | is_yellow_ribbon, iterations=2))
    # Tuck ~20px up strictly INSIDE opaque coat pixels / under hanging right hand (NEVER outside opaque silhouette!)
    dist_from_vis = distance_transform_edt(~skirt_seed)
    skirt_fill = opaque & (yg >= 660) & (yg <= 880) & (dist_from_vis <= 20.0) & (~skirt_seed) & (~legs_zone)
    skirt_rgb = inpaint_biharmonic_like(s0[:, :, :3], skirt_fill, skirt_known, iters=120)
    skirt_mask = (skirt_seed | skirt_fill) & (~(legs_zone & (yg >= 875)))
    sakura_skirt = np.zeros_like(s0)
    sakura_skirt[:, :, :3] = skirt_rgb
    sakura_skirt[:, :, 3] = np.clip(np.round(soft_weight(skirt_mask, 0.7) * 255.0), 0, 255).astype(np.uint8)
    sakura_skirt[sakura_skirt[:, :, 3] == 0, :3] = 0
    save_rgba(sakura_skirt, PARTS_DIR / "sakura_skirt.png")

    # 9) Torso + Neck + Collar + White Coat + Arms + Hands (z=40):
    # Strictly fill under chest_bow from pure snowy white coat pixels (y >= 466) so NO skin peach leaks down!
    torso_zone = poly_mask((h, w), [(120, 368), (542, 368), (542, 835), (340, 840), (140, 760)])
    torso_base_mask = opaque & torso_zone & (~skirt_seed)
    navy_slit = (yg >= 668) & ((b >= r + 20) & (r <= 165) & (b <= 215)) & (~poly_mask((h, w), [(300, 380), (370, 380), (370, 580), (300, 580)]))
    torso_base_mask &= ~navy_slit

    bow_fill = torso_base_mask & binary_dilation(bow_core, iterations=4)
    wbow_fill = torso_base_mask & binary_dilation(wbow_core, iterations=4)
    chin_fill = torso_base_mask & ((yg >= 368) & (yg < 386))

    # Fill coat under bows strictly from pure white coat & collar-adjacent pixels (never skin above!)
    coat_known = torso_base_mask & (~bow_fill) & (~wbow_fill) & (~chin_fill) & is_pure_white_coat & (yg >= 475)
    skin_known = torso_base_mask & (~chin_fill) & is_skin & (yg >= 375) & (yg <= 415)
    torso_rgb = inpaint_biharmonic_like(s0[:, :, :3], bow_fill | wbow_fill, coat_known, iters=110)
    torso_rgb = inpaint_biharmonic_like(torso_rgb, chin_fill, skin_known, iters=60)

    sakura_torso = np.zeros_like(s0)
    sakura_torso[:, :, :3] = torso_rgb
    sakura_torso[:, :, 3] = np.clip(np.round(soft_weight(torso_base_mask, 0.7) * 255.0), 0, 255).astype(np.uint8)
    sakura_torso[sakura_torso[:, :, 3] == 0, :3] = 0
    save_rgba(sakura_torso, PARTS_DIR / "sakura_torso.png")

    return [
        {
            "id": "legs_boots",
            "parent": None,
            "texture": "parts/sakura_legs_boots.png",
            "z": 10,
            "pivot_px": [335.0, 1295.0],
            "role": "planted",
            "parallax": [0.0, 0.0],
            "rot_deg": 0.0,
            "bend_px": 0.0,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.002, -0.002],
        },
        {
            "id": "hair_back",
            "parent": "torso",
            "texture": "parts/sakura_hair_back.png",
            "z": 20,
            "pivot_px": [330.0, 240.0],
            "role": "hair_back",
            "parallax": [-0.28, -0.15],
            "rot_deg": 1.6,
            "bend_px": 5.5,
            "breath_px": [0.0, 1.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 8.4,
            "spring_zeta": 0.42,
            "phase_offset": 0.35,
        },
        {
            "id": "skirt",
            "parent": "torso",
            "texture": "parts/sakura_skirt.png",
            "z": 30,
            "pivot_px": [326.0, 678.0],
            "role": "skirt",
            "parallax": [0.22, 0.0],
            "rot_deg": 2.4,
            "bend_px": 7.5,
            "breath_px": [0.0, 0.4],
            "scale_breath": [0.004, 0.0],
            "spring_omega": 6.8,
            "spring_zeta": 0.44,
        },
        {
            "id": "torso",
            "parent": None,
            "texture": "parts/sakura_torso.png",
            "z": 40,
            "pivot_px": [322.0, 825.0],
            "role": "torso",
            "parallax": [0.25, 0.14],
            "rot_deg": 1.1,
            "bend_px": 0.0,
            "breath_px": [0.0, -3.2],
            "scale_breath": [0.011, 0.008],
        },
        {
            "id": "chest_bow",
            "parent": "torso",
            "texture": "parts/sakura_chest_bow.png",
            "z": 50,
            "pivot_px": [300.0, 488.0],
            "role": "ribbon",
            "parallax": [0.35, -0.20],
            "rot_deg": 2.2,
            "bend_px": 3.5,
            "breath_px": [0.0, -1.6],
            "scale_breath": [0.005, 0.0],
            "spring_omega": 9.8,
            "spring_zeta": 0.38,
            "phase_offset": 0.25,
        },
        {
            "id": "waist_bow",
            "parent": "torso",
            "texture": "parts/sakura_waist_bow.png",
            "z": 55,
            "pivot_px": [374.0, 608.0],
            "role": "ribbon",
            "parallax": [0.32, 0.10],
            "rot_deg": 2.8,
            "bend_px": 4.5,
            "breath_px": [0.0, -0.8],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 8.2,
            "spring_zeta": 0.40,
            "phase_offset": 0.85,
        },
        {
            "id": "head",
            "parent": "torso",
            "texture": "parts/sakura_head.png",
            "z": 60,
            "pivot_px": [328.0, 382.0],
            "role": "head",
            "parallax": [0.85, 0.48],
            "rot_deg": 2.8,
            "bend_px": 0.0,
            "breath_px": [0.0, -1.2],
            "scale_breath": [0.0, 0.0],
            "dynamic_patch": {
                "rect_px": [200, 130, 438, 394],
                "mode": "masked_copy",
                "mask_texture": "parts/sakura_head_mask.png",
            },
        },
        {
            "id": "hair_side_l",
            "parent": "head",
            "texture": "parts/sakura_hair_side_l.png",
            "z": 70,
            "pivot_px": [228.0, 248.0],
            "role": "hair_side_l",
            "parallax": [0.42, 0.22],
            "rot_deg": 4.2,
            "bend_px": 7.5,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 9.4,
            "spring_zeta": 0.35,
            "phase_offset": 0.12,
        },
        {
            "id": "hair_side_r",
            "parent": "head",
            "texture": "parts/sakura_hair_side_r.png",
            "z": 72,
            "pivot_px": [424.0, 244.0],
            "role": "hair_side_r",
            "parallax": [0.42, 0.22],
            "rot_deg": 4.4,
            "bend_px": 7.8,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 9.9,
            "spring_zeta": 0.34,
            "phase_offset": 0.72,
        },
        {
            "id": "ahoge",
            "parent": "head",
            "texture": "parts/sakura_ahoge.png",
            "z": 80,
            "pivot_px": [314.0, 128.0],
            "role": "ahoge",
            "parallax": [0.22, -0.18],
            "rot_deg": 6.8,
            "bend_px": 10.5,
            "breath_px": [0.0, 0.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 15.2,
            "spring_zeta": 0.24,
        },
    ]


def build_unyu_parts() -> list[dict]:
    u10 = load_rgba(SHELL_DIR / "surface10.png")
    h, w = u10.shape[:2]
    alpha = u10[:, :, 3]
    opaque = alpha > 16
    rgb = u10[:, :, :3].astype(np.int16)
    r, g, b = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]

    yg = y_grid(h, w)
    # Use 6px erosion for wiping temple ends on mochi_body; keep 18px away from outer border/gloss for diff_overlay (except blush)
    interior_wipe = binary_erosion(opaque, iterations=6)
    is_pure_periwinkle = opaque & (b >= r + 48) & (b >= 195) & (r >= 115) & (g >= 120) & ((r <= 190) | (b >= 235))
    is_pink_blush = opaque & (r >= 172) & (r > b + 12) & (yg >= 165) & (yg <= 238)
    interior_face = binary_erosion(opaque, iterations=18) | (binary_erosion(opaque, iterations=8) & is_pink_blush)

    # 1) Tuft: top-left 3 curved spikes above smooth dome curve
    tuft_poly = poly_mask((h, w), [(10, 26), (114, 26), (112, 66), (102, 102), (92, 132), (10, 148)])
    tuft_mask = opaque & tuft_poly
    save_rgba(extract_layer(u10, soft_weight(tuft_mask, 0.8)), PARTS_DIR / "unyu_tuft.png")

    # 2) Glasses: true measured centers (180.0, 201.5) & (277.5, 195.0), rim r=28.6, glass r=22.5
    lens_l = ellipse_mask((h, w), 180.0, 201.5, 28.6, 28.6)
    lens_r = ellipse_mask((h, w), 277.5, 195.0, 28.6, 28.6)
    bridge_band = poly_mask((h, w), [(200, 177), (256, 177), (256, 197), (200, 197)])
    temple_l_band = poly_mask((h, w), [(26, 162), (158, 184), (158, 208), (26, 195)])
    temple_r_band = poly_mask((h, w), [(298, 180), (342, 140), (344, 168), (298, 204)])
    not_skin_or_white = (r < 120) & (g < 120) & (b < 145)
    glasses_mask = (lens_l | lens_r | bridge_band | temple_l_band | temple_r_band) & opaque & (
        not_skin_or_white
        | ellipse_mask((h, w), 180.0, 201.5, 22.5, 22.5)
        | ellipse_mask((h, w), 277.5, 195.0, 22.5, 22.5)
    )
    unyu_glasses = extract_layer(u10, soft_weight(glasses_mask, 0.7))
    rim_l = lens_l & (~ellipse_mask((h, w), 180.0, 201.5, 22.5, 22.5))
    rim_r = lens_r & (~ellipse_mask((h, w), 277.5, 195.0, 22.5, 22.5))
    lens_glass = (ellipse_mask((h, w), 180.0, 201.5, 22.5, 22.5) | ellipse_mask((h, w), 277.5, 195.0, 22.5, 22.5)) & (~rim_l) & (~rim_r)
    unyu_glasses[:, :, 3][lens_glass] = 175
    unyu_glasses[:, :, :3][lens_glass] = [42, 46, 72]
    save_rgba(unyu_glasses, PARTS_DIR / "unyu_glasses.png")

    # Exclude glasses pixels AND outer body contour (~interior_face) while keeping full pink blush cheeks intact
    lens_l_excl = ellipse_mask((h, w), 180.0, 201.5, 30.8, 30.8) & (~poly_mask((h, w), [(208, 192), (252, 192), (252, 260), (208, 260)]))
    lens_r_excl = ellipse_mask((h, w), 277.5, 195.0, 30.8, 30.8) & (~poly_mask((h, w), [(208, 192), (252, 192), (252, 260), (208, 260)]))
    glasses_excl_mask = (
        binary_dilation(
            lens_l_excl
            | lens_r_excl
            | (bridge_band & not_skin_or_white)
            | (temple_l_band & not_skin_or_white)
            | (temple_r_band & not_skin_or_white),
            iterations=2,
        )
        & (~is_pink_blush)
    ) | (~interior_face)
    glasses_mask_img = np.zeros_like(u10)
    glasses_mask_img[:, :, :3] = 255
    glasses_mask_img[:, :, 3] = np.where(glasses_excl_mask, 255, 0).astype(np.uint8)
    save_rgba(glasses_mask_img, PARTS_DIR / "unyu_glasses_mask.png")

    # 3) Paws with soft feathered interior attachment
    paw_l_zone = poly_mask((h, w), [(122, 238), (204, 238), (204, 330), (122, 330)])
    paw_r_zone = poly_mask((h, w), [(250, 232), (336, 232), (336, 326), (250, 326)])
    paw_l_outline = paw_l_zone & (r < 95) & (b < 145)
    paw_r_outline = paw_r_zone & (r < 95) & (b < 145)
    paw_l_ell = ellipse_mask((h, w), 165.0, 290.0, 32.0, 28.0) & poly_mask((h, w), [(140, 240), (204, 282), (186, 324), (124, 296)])
    paw_r_ell = ellipse_mask((h, w), 292.0, 282.0, 32.0, 28.0) & poly_mask((h, w), [(296, 236), (334, 286), (298, 320), (254, 298)])
    paw_l_alpha = np.maximum(soft_weight(paw_l_outline, 0.7), soft_weight(paw_l_ell, 3.2) * 0.85)
    paw_r_alpha = np.maximum(soft_weight(paw_r_outline, 0.7), soft_weight(paw_r_ell, 3.2) * 0.85)
    save_rgba(extract_layer(u10, paw_l_alpha), PARTS_DIR / "unyu_paw_l.png")
    save_rgba(extract_layer(u10, paw_r_alpha), PARTS_DIR / "unyu_paw_r.png")

    # 4) Clean Mochi Body: wipe ALL face/paws/glasses/temples inside interior_wipe; protect upper-right shine bubble
    gloss_preserve = poly_mask((h, w), [(220, 45), (365, 45), (365, 106), (220, 106)])
    face_box = poly_mask((h, w), [(112, 106), (324, 106), (324, 260), (112, 260)])
    glasses_marks = lens_l | lens_r | (bridge_band & not_skin_or_white) | (temple_l_band & not_skin_or_white) | (temple_r_band & not_skin_or_white)
    raw_marks = interior_wipe & (~gloss_preserve) & (
        (face_box & ((~is_pure_periwinkle) | is_pink_blush))
        | glasses_marks
        | paw_l_outline
        | paw_r_outline
    )
    body_erase = binary_dilation(raw_marks, iterations=5) & interior_wipe & (~gloss_preserve)
    body_known = interior_wipe & is_pure_periwinkle & (~body_erase) & (~tuft_mask)
    body_rgb = inpaint_biharmonic_like(u10[:, :, :3], body_erase, body_known, iters=150)

    smooth_dome_cut = poly_mask((h, w), [(8, 20), (104, 20), (92, 74), (72, 132), (8, 148)])
    body_mask = opaque & (~smooth_dome_cut)
    unyu_body = np.zeros_like(u10)
    unyu_body[:, :, :3] = body_rgb
    unyu_body[:, :, 3] = np.clip(np.round(soft_weight(body_mask, 0.8) * 255.0), 0, 255).astype(np.uint8)
    contour_im = Image.fromarray(unyu_body, "RGBA")
    ImageDraw.Draw(contour_im).line([(103, 62), (91, 88), (74, 130)], fill=(22, 22, 82, 255), width=4)
    unyu_body = np.array(contour_im, dtype=np.uint8)
    unyu_body[:, :, 3][~body_mask] = 0
    unyu_body[unyu_body[:, :, 3] == 0, :3] = 0
    save_rgba(unyu_body, PARTS_DIR / "unyu_body.png")

    # 5) Clean Unyu Face rest frame (excluding outer body rim and top-right highlight!)
    face_patch_box = poly_mask((h, w), [(118, 106), (318, 106), (318, 258), (118, 258)])
    face_diff = (
        face_patch_box
        & interior_face
        & (np.max(np.abs(u10[:, :, :3].astype(np.int16) - unyu_body[:, :, :3].astype(np.int16)), axis=2) > 22)
        & (~glasses_excl_mask)
    )
    unyu_face = extract_layer(u10, soft_weight(face_diff, 0.7))
    save_rgba(unyu_face, PARTS_DIR / "unyu_face.png")

    save_rgba(u10, PARTS_DIR / "unyu_crown_ref.png")
    save_rgba(np.zeros_like(u10), PARTS_DIR / "unyu_crown_slot.png")

    return [
        {
            "id": "mochi_body",
            "parent": None,
            "texture": "parts/unyu_body.png",
            "z": 10,
            "pivot_px": [190.0, 386.0],
            "role": "mochi_body",
            "parallax": [0.35, 0.14],
            "rot_deg": 1.6,
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
            "rot_deg": 5.5,
            "bend_px": 7.5,
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
                "ref_texture": "parts/unyu_crown_ref.png",
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
                "rect_px": [118, 108, 316, 258],
                "mode": "diff_overlay",
                "ref_texture": "parts/unyu_body.png",
                "exclude_mask_texture": "parts/unyu_glasses_mask.png",
                "threshold": 24,
            },
        },
        {
            "id": "mochi_paw_l",
            "parent": "mochi_body",
            "texture": "parts/unyu_paw_l.png",
            "z": 40,
            "pivot_px": [146.0, 262.0],
            "role": "mochi_paws",
            "parallax": [0.25, 0.20],
            "rot_deg": 4.2,
            "bend_px": 3.0,
            "breath_px": [0.0, -1.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 10.5,
            "spring_zeta": 0.35,
            "phase_offset": 0.0,
        },
        {
            "id": "mochi_paw_r",
            "parent": "mochi_body",
            "texture": "parts/unyu_paw_r.png",
            "z": 42,
            "pivot_px": [306.0, 258.0],
            "role": "mochi_paws",
            "parallax": [0.25, 0.20],
            "rot_deg": -4.2,
            "bend_px": 3.0,
            "breath_px": [0.0, -1.0],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 10.8,
            "spring_zeta": 0.35,
            "phase_offset": 1.1,
        },
        {
            "id": "mochi_glasses",
            "parent": "mochi_face",
            "texture": "parts/unyu_glasses.png",
            "z": 50,
            "pivot_px": [228.0, 192.0],
            "role": "mochi_glasses",
            "parallax": [0.16, 0.12],
            "rot_deg": 1.2,
            "bend_px": 0.0,
            "breath_px": [0.0, 0.4],
            "scale_breath": [0.0, 0.0],
            "spring_omega": 14.0,
            "spring_zeta": 0.42,
        },
    ]


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate CC0-1.0 Live2D layered parts for neo_default.")
    parser.parse_args()

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    PARTS_DIR.mkdir(parents=True, exist_ok=True)
    (OUT_DIR / "LICENSE-CC0.txt").write_text(CC0_NOTICE, encoding="utf-8")

    sakura_parts = build_sakura_parts()
    unyu_parts = build_unyu_parts()

    manifest = {
        "schema_version": 1,
        "name": "neo_default Multi-Layer Live2D Rig (CC0-1.0)",
        "sakura": {
            "canvas_size": [660, 1350],
            "parts": sakura_parts,
        },
        "kero": {
            "canvas_size": [380, 420],
            "parts": unyu_parts,
        },
    }
    (OUT_DIR / "live2d.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )
    print(f"Generated {len(sakura_parts)} Sakura parts and {len(unyu_parts)} Unyu parts in {OUT_DIR}")


if __name__ == "__main__":
    main()
