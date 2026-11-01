#!/usr/bin/env python3
"""Generate the BKG office pixel sprites in SNES / MUNDER-DIFFLIN style.

The two reference images are 16-bit era office scenes: every object has a
near-black navy outline, a 3-4 step shading ramp (shadow / mid / light /
specular), a warm highlight from the lamps and a cyan rim light from the
windows. This generator reproduces that with explicit pixel work -- not
flat colour fields:

  * desks 64x48, agents 24x32 (3/4 view), rack 40x64, plant 32x40
  * screens: vertical gradient + deterministic neon code bars + glow halo
  * wood/metal/plastic: horizontal shading ramps, edge highlights, shadows
  * characters: hair with highlight strand, eyes with specular, shirt
    ramps, headset/courier/celebration props per harness sprite

Palette (from the references):
  outline #04101f, floor #10243d/#132a47, panel #0a1a30, rim #35c4ff,
  light cyan #7fdcff, green #39ff6a, amber #ffb000, blue #3fa9ff,
  red #ff4d4d, pink #ff5ca8, violet #b78cff, wood #d8b06a/#8a5a2b.

Status glow is NOT baked in: pages colour it from the employee status.
Pivot: bottom-center (web-dist/assets/manifest.json).

Usage: python3 scripts/gen_pixel_sprites.py [outdir]
Default outdir: web-dist/assets/sprites
"""
import os
import sys

from PIL import Image, ImageDraw

O = (4, 16, 31, 255)              # outline #04101f
NAVY = (16, 36, 61, 255)
NAVY2 = (19, 42, 71, 255)
CYAN = (53, 196, 255, 255)
CYAN_LT = (127, 220, 255, 255)
GREEN = (57, 255, 106, 255)
AMBER = (255, 176, 0, 255)
BLUE = (63, 169, 255, 255)
RED = (255, 77, 77, 255)
PINK = (255, 92, 168, 255)
VIOLET = (183, 140, 255, 255)
CREAM = (255, 253, 247, 255)
CODE = [GREEN, PINK, CYAN_LT, VIOLET, AMBER]
SKINS = [(242, 199, 155, 255), (198, 145, 100, 255), (140, 95, 60, 255)]


def img(w, h):
    return Image.new("RGBA", (w, h), (0, 0, 0, 0))


def R(d, box, fill):
    d.rectangle(box, fill=fill)


def RO(d, box, fill):
    d.rectangle(box, fill=fill, outline=O)


def ramp(d, box, colors):
    """Horizontal shading ramp: colors left(lit) -> right(shadow)."""
    x0, y0, x1, y1 = box
    w = max(1, x1 - x0 + 1)
    n = len(colors)
    for i, c in enumerate(colors):
        xa = x0 + i * w // n
        xb = x0 + (i + 1) * w // n - 1
        if xb >= xa:
            R(d, [xa, y0, xb, y1], c)


def shadow(d, cx, y, w):
    d.ellipse([cx - w // 2, y, cx + w // 2, y + 5], fill=(3, 10, 22, 110))


def code_bars(d, x, y, w, h, seed):
    rows = [(0.8, 0), (0.55, 1), (0.7, 2), (0.4, 3), (0.85, 4), (0.5, 1), (0.65, 0)]
    for i, (frac, ci) in enumerate(rows):
        yy = y + 2 + i * 3
        if yy > y + h - 2:
            break
        ln = max(2, int((w - 4) * frac))
        indent = 2 if i % 2 else 1
        d.line([(x + indent, yy), (x + indent + ln, yy)], fill=CODE[(ci + seed) % len(CODE)])
        R(d, [x + indent + ln + 1, yy, x + indent + ln + 1, yy], (20, 60, 90, 255))


def monitor(d, x, y, w, h, seed):
    # bezel with top highlight, gradient screen, stand. No rectangular glow
    # halo: at pixel scale its edges read as a flat slab, not light.
    RO(d, [x, y, x + w, y + h], (8, 18, 36, 255))
    R(d, [x + 1, y + 1, x + w - 1, y + 1], (70, 110, 150, 255))
    for i in range(h - 3):
        t = i / max(1, h - 4)
        c = (int(10 + 14 * (1 - t)), int(34 + 30 * (1 - t)), int(66 + 44 * (1 - t)), 255)
        R(d, [x + 2, y + 2 + i, x + w - 2, y + 2 + i], c)
    code_bars(d, x + 1, y + 1, w - 2, h - 2, seed)
    R(d, [x + w // 2, y + h + 1, x + w // 2 + 1, y + h + 3], O)
    R(d, [x + w // 2 - 3, y + h + 3, x + w // 2 + 4, y + h + 3], O)


def chair_front(d, cx, y, main=(43, 108, 176, 255)):
    dark = tuple(max(0, c - 50) for c in main[:3]) + (255,)
    lite = tuple(min(255, c + 45) for c in main[:3]) + (255,)
    ramp(d, [cx - 8, y, cx + 8, y + 10], [lite, main, dark])          # backrest
    R(d, [cx - 8, y, cx + 8, y], O); R(d, [cx - 8, y + 10, cx + 8, y + 10], O)
    R(d, [cx - 6, y + 2, cx - 2, y + 3], lite)
    ramp(d, [cx - 8, y + 11, cx + 8, y + 14], [main, dark])           # seat
    R(d, [cx - 1, y + 15, cx + 1, y + 18], O)
    R(d, [cx - 7, y + 19, cx + 7, y + 19], O)
    R(d, [cx - 7, y + 17, cx - 6, y + 19], O); R(d, [cx + 6, y + 17, cx + 7, y + 19], O)


def desk_body(d, top, edge, front):
    shadow(d, 32, 42, 56)
    ramp(d, [2, 22, 61, 29], top)                                     # desktop
    R(d, [2, 22, 61, 22], (255, 235, 190, 255))                       # lit edge
    ramp(d, [2, 30, 61, 33], edge)                                    # front edge
    ramp(d, [4, 34, 16, 43], front)                                   # drawer block
    R(d, [6, 37, 14, 37], O); R(d, [6, 40, 14, 40], O)
    R(d, [9, 36, 11, 36], CREAM); R(d, [9, 39, 11, 39], CREAM)
    R(d, [56, 34, 59, 43], front[1] if len(front) > 1 else front[0])  # leg
    R(d, [2, 29, 61, 29], O)


WOOD_TOP = [(238, 200, 130, 255), (216, 176, 106, 255), (196, 152, 84, 255), (160, 118, 62, 255)]
WOOD_EDGE = [(160, 118, 62, 255), (138, 90, 43, 255), (110, 68, 32, 255)]
WOOD_FRONT = [(150, 105, 58, 255), (120, 78, 40, 255)]


def mug(d, x, y):
    RO(d, [x, y, x + 4, y + 4], CREAM)
    R(d, [x + 1, y, x + 3, y], (255, 120, 60, 255))
    R(d, [x + 2, y - 3, x + 2, y - 2], (200, 220, 240, 140))          # steam


def tiny_plant(d, x, y):
    RO(d, [x, y + 4, x + 5, y + 8], (170, 90, 50, 255))
    for dx, dy, c in [(-1, 0, (50, 190, 95, 255)), (2, -3, (40, 160, 80, 255)), (5, 0, (60, 205, 105, 255)), (2, 2, (35, 140, 70, 255))]:
        d.ellipse([x + dx - 2, y + dy - 2, x + dx + 3, y + dy + 3], fill=c, outline=O)


def desk_dual_monitor():
    im = img(64, 48); d = ImageDraw.Draw(im)
    desk_body(d, WOOD_TOP, WOOD_EDGE, WOOD_FRONT)
    monitor(d, 5, 3, 22, 15, seed=1)
    monitor(d, 36, 3, 22, 15, seed=3)
    RO(d, [24, 25, 40, 28], (24, 28, 44, 255))                        # keyboard
    for kx in range(26, 39, 3):
        R(d, [kx, 26, kx, 26], CYAN_LT)
    R(d, [43, 26, 45, 27], (24, 28, 44, 255))                         # mouse
    mug(d, 57, 23)
    tiny_plant(d, 29, 17)
    chair_front(d, 32, 30)
    return im


def desk_snes():
    im = img(64, 48); d = ImageDraw.Draw(im)
    grey = [(200, 200, 212, 255), (178, 178, 195, 255), (150, 150, 172, 255), (120, 120, 145, 255)]
    desk_body(d, grey, [(130, 130, 155, 255), (105, 105, 130, 255), (85, 85, 108, 255)],
              [(140, 140, 162, 255), (110, 110, 135, 255)])
    monitor(d, 10, 2, 26, 16, seed=2)
    RO(d, [40, 24, 56, 29], (168, 168, 185, 255))                     # SNES console
    RO(d, [45, 19, 52, 24], (140, 60, 160, 255))                      # cartridge
    R(d, [46, 20, 51, 20], PINK); R(d, [46, 22, 49, 22], CYAN_LT)
    R(d, [42, 26, 45, 26], (90, 60, 160, 255)); R(d, [50, 26, 53, 26], (205, 60, 80, 255))
    d.line([(48, 29), (48, 31), (30, 31)], fill=O)                    # controller cable
    RO(d, [24, 30, 33, 32], (60, 55, 120, 255))                       # controller
    R(d, [26, 31, 26, 31], RED); R(d, [30, 31, 30, 31], GREEN)
    chair_front(d, 32, 30, main=(80, 70, 160, 255))
    return im


def desk_wood():
    im = img(64, 48); d = ImageDraw.Draw(im)
    desk_body(d, WOOD_TOP, WOOD_EDGE, WOOD_FRONT)
    # architect lamp (left), blueprint, books, mug
    R(d, [8, 20, 9, 28], O); d.line([(9, 20), (15, 12)], fill=O, width=2)
    RO(d, [13, 8, 19, 12], AMBER); R(d, [14, 11, 18, 11], (255, 230, 150, 255))
    RO(d, [24, 23, 44, 29], CREAM)                                    # blueprint
    d.line([(27, 26), (34, 24), (41, 27)], fill=(70, 130, 190, 255))
    R(d, [27, 24, 30, 24], CYAN); R(d, [36, 25, 39, 25], PINK)
    RO(d, [48, 24, 55, 27], (150, 50, 60, 255)); RO(d, [49, 21, 54, 24], (50, 90, 150, 255))
    R(d, [30, 24, 31, 27], AMBER)                                     # pencil
    mug(d, 57, 23)
    chair_front(d, 32, 30, main=(50, 150, 85, 255))
    return im


def server_rack():
    im = img(40, 64); d = ImageDraw.Draw(im)
    shadow(d, 20, 59, 34)
    ramp(d, [4, 2, 35, 58], [(28, 56, 96, 255), (18, 40, 72, 255), (10, 26, 50, 255)])
    R(d, [4, 2, 35, 3], CYAN)
    for i in range(6):
        y = 6 + i * 9
        RO(d, [6, y, 33, y + 7], (12, 28, 54, 255))
        R(d, [7, y + 1, 32, y + 1], (40, 80, 125, 255))
        for vx in range(9, 20, 3):
            R(d, [vx, y + 5, vx + 1, y + 5], (45, 95, 145, 255))      # vents
        R(d, [23, y + 3, 23, y + 3], GREEN if i % 3 else RED)
        R(d, [26, y + 3, 26, y + 3], AMBER if i % 2 else CYAN)
        R(d, [29, y + 3, 29, y + 3], GREEN)
        R(d, [29, y + 5, 32, y + 5], O)
    R(d, [6, 58, 33, 58], CYAN)
    return im


def cabinet():
    im = img(48, 40); d = ImageDraw.Draw(im)
    shadow(d, 24, 35, 42)
    ramp(d, [3, 6, 44, 34], [(96, 118, 148, 255), (70, 90, 118, 255), (50, 66, 92, 255)])
    R(d, [3, 6, 44, 6], CYAN_LT)
    RO(d, [30, 1, 36, 6], (150, 50, 60, 255)); RO(d, [36, 2, 42, 6], (50, 90, 150, 255))  # books on top
    for i in range(3):
        y = 9 + i * 8
        RO(d, [5, y, 42, y + 6], (84, 106, 134, 255))
        R(d, [6, y + 1, 41, y + 1], (130, 158, 190, 255))
        RO(d, [18, y + 2, 29, y + 4], CREAM)
        R(d, [20, y + 3, 27, y + 3], (110, 120, 140, 255))
        R(d, [8, y + 3, 10, y + 3], CYAN_LT)
    return im


def plant():
    im = img(32, 40); d = ImageDraw.Draw(im)
    shadow(d, 16, 35, 24)
    greens = [(50, 190, 95, 255), (40, 160, 80, 255), (30, 130, 65, 255), (65, 205, 110, 255)]
    for (x, y, s) in [(16, 8, 9), (8, 14, 7), (24, 13, 7), (12, 19, 6), (21, 19, 6), (16, 15, 7)]:
        c = greens[(x + y) % 4]
        d.ellipse([x - s // 2, y - s // 2, x + s // 2, y + s // 2], fill=c, outline=O)
        R(d, [x - 2, y - 2, x, y - 2], (120, 235, 150, 255))          # leaf highlight
    R(d, [15, 16, 16, 28], (26, 100, 52, 255))
    ramp(d, [9, 27, 22, 35], [(200, 115, 65, 255), (170, 90, 50, 255), (130, 65, 38, 255)])
    R(d, [8, 26, 23, 27], (225, 140, 80, 255))
    return im


def chair_sprite(main):
    im = img(32, 32); d = ImageDraw.Draw(im)
    shadow(d, 16, 27, 24)
    chair_front(d, 16, 4, main=main)
    return im


# --- agents: 24x32, 3/4 view -------------------------------------------

def agent(shirt, hair_c, skin, kind):
    im = img(24, 32); d = ImageDraw.Draw(im)
    shirt_d = tuple(max(0, c - 55) for c in shirt[:3]) + (255,)
    shirt_l = tuple(min(255, c + 40) for c in shirt[:3]) + (255,)
    hair_d = tuple(max(0, c - 35) for c in hair_c[:3]) + (255,)
    shadow(d, 12, 29, 18)
    if kind == "celebrating":
        d.line([(5, 18), (2, 9)], fill=shirt, width=3); d.line([(18, 18), (21, 9)], fill=shirt, width=3)
        R(d, [1, 6, 3, 8], skin); R(d, [20, 6, 22, 8], skin)
        for (x, y, c) in [(0, 2, PINK), (8, 0, AMBER), (16, 1, CYAN_LT), (22, 4, GREEN), (5, 4, VIOLET), (19, 0, PINK)]:
            R(d, [x, y, x + 1, y], c)
    # legs / shoes (walking = stride)
    if kind == "walking":
        R(d, [7, 25, 9, 29], (24, 30, 54, 255)); R(d, [14, 24, 16, 27], (24, 30, 54, 255))
        R(d, [6, 29, 10, 30], O); R(d, [14, 27, 18, 28], O)
    else:
        R(d, [7, 25, 10, 29], (24, 30, 54, 255)); R(d, [13, 25, 16, 29], (24, 30, 54, 255))
        R(d, [6, 29, 10, 30], O); R(d, [13, 29, 17, 30], O)
    # torso with ramp + collar
    ramp(d, [5, 17, 18, 25], [shirt_l, shirt, shirt_d])
    R(d, [5, 17, 18, 17], shirt_l); R(d, [10, 17, 13, 18], CREAM)
    R(d, [5, 24, 18, 25], (20, 26, 48, 255))                          # belt/waist
    # arms
    if kind == "typing":
        d.line([(5, 19), (8, 23)], fill=shirt_d, width=3); d.line([(18, 19), (15, 23)], fill=shirt_d, width=3)
        R(d, [7, 23, 9, 24], skin); R(d, [14, 23, 16, 24], skin)      # hands to keyboard
    elif kind == "walking":
        d.line([(5, 19), (3, 23)], fill=shirt_d, width=3); R(d, [2, 23, 4, 24], skin)
        RO(d, [17, 20, 23, 25], CREAM)                                # envelope in hand
        d.line([(17, 20), (23, 25)], fill=CYAN); d.line([(23, 20), (17, 25)], fill=CYAN)
    elif kind == "celebrating":
        R(d, [11, 21, 12, 22], AMBER)
    # head: face, hair with highlight strand, eyes w/ specular, mouth
    RO(d, [6, 6, 17, 16], skin)
    ramp(d, [6, 3, 17, 8], [tuple(min(255, c + 45) for c in hair_c[:3]) + (255,), hair_c, hair_d])
    R(d, [6, 8, 7, 11], hair_c); R(d, [16, 8, 17, 11], hair_c)        # side locks
    R(d, [8, 4, 11, 4], (255, 255, 255, 90))                          # hair shine
    R(d, [8, 12, 9, 13], O); R(d, [14, 12, 15, 13], O)                # eyes
    R(d, [9, 12, 9, 12], (255, 255, 255, 255)); R(d, [15, 12, 15, 12], (255, 255, 255, 255))
    R(d, [11, 15, 12, 15], (170, 90, 70, 255))                        # mouth
    if kind == "typing":                                              # headset + mic
        R(d, [5, 4, 18, 5], (26, 30, 44, 255))
        RO(d, [4, 8, 6, 12], (26, 30, 44, 255)); RO(d, [17, 8, 19, 12], (26, 30, 44, 255))
        d.line([(18, 12), (14, 15)], fill=(26, 30, 44, 255)); R(d, [13, 15, 14, 15], CYAN)
    if kind == "walking":                                             # courier cap
        ramp(d, [5, 2, 18, 5], [(150, 110, 220, 255), (120, 80, 200, 255), (95, 60, 165, 255)])
        R(d, [4, 5, 19, 5], (95, 60, 165, 255))
    return im


def floor_tile(base, speck):
    im = img(32, 32); d = ImageDraw.Draw(im)
    R(d, [0, 0, 31, 31], base)
    for (x, y) in [(5, 7), (24, 4), (12, 20), (27, 25), (8, 28), (19, 12)]:
        R(d, [x, y, x, y], speck)
    R(d, [0, 31, 31, 31], (8, 22, 42, 255)); R(d, [31, 0, 31, 31], (8, 22, 42, 255))
    R(d, [0, 0, 31, 0], (36, 72, 112, 255))
    return im


def wall_brick():
    im = img(32, 32); d = ImageDraw.Draw(im)
    R(d, [0, 0, 31, 31], (22, 42, 72, 255))
    for row in range(4):
        y = row * 8
        off = 8 if row % 2 else 0
        for x in range(-8, 32, 16):
            bx = x + off
            ramp(d, [bx + 1, y + 1, bx + 14, y + 6], [(38, 68, 108, 255), (28, 52, 86, 255), (20, 38, 66, 255)])
        R(d, [0, y + 7, 31, y + 7], O)
    R(d, [0, 0, 31, 1], CYAN)                                         # cyan rim light
    return im


def door_wood():
    im = img(40, 56); d = ImageDraw.Draw(im)
    shadow(d, 20, 51, 34)
    ramp(d, [4, 2, 35, 50], [(80, 55, 30, 255), (60, 40, 22, 255)])   # frame
    ramp(d, [8, 5, 31, 48], [(150, 100, 55, 255), (125, 80, 42, 255), (100, 62, 32, 255)])
    RO(d, [11, 8, 28, 22], (110, 70, 36, 255)); R(d, [12, 9, 27, 9], (185, 130, 75, 255))
    RO(d, [11, 27, 28, 44], (110, 70, 36, 255)); R(d, [12, 28, 27, 28], (185, 130, 75, 255))
    d.ellipse([27, 25, 30, 28], fill=AMBER, outline=O)
    R(d, [8, 5, 31, 5], CYAN_LT)
    return im


def status_dot(color):
    im = img(16, 16); d = ImageDraw.Draw(im)
    d.ellipse([1, 1, 14, 14], fill=color[:3] + (60,))                 # halo
    d.ellipse([3, 3, 12, 12], fill=color, outline=O)
    R(d, [6, 5, 7, 6], (255, 255, 255, 255))                          # specular
    return im


def envelope():
    im = img(20, 14); d = ImageDraw.Draw(im)
    RO(d, [1, 2, 18, 11], CREAM)
    d.line([(1, 2), (10, 8), (18, 2)], fill=CYAN)
    R(d, [2, 3, 4, 3], PINK)
    return im


SPRITES = {
    "floor_tile_00.png": lambda: floor_tile(NAVY, (28, 58, 98, 255)),
    "floor_tile_01.png": lambda: floor_tile(NAVY2, (32, 64, 106, 255)),
    "wall_brick_01.png": wall_brick,
    "door_wood_iso_01.png": door_wood,
    "desk_dual_monitor_01.png": desk_dual_monitor,
    "desk_snes_01.png": desk_snes,
    "server_rack_snes_01.png": server_rack,
    "cabinet_01.png": cabinet,
    "plant_snes_01.png": plant,
    "desk_wood_01.png": desk_wood,
    "chair_blue_01.png": lambda: chair_sprite((43, 108, 176, 255)),
    "chair_green_01.png": lambda: chair_sprite((50, 170, 90, 255)),
    "agent_typing_02.png": lambda: agent((43, 108, 176, 255), (58, 36, 21, 255), SKINS[0], "typing"),
    "agent_walking_right_01.png": lambda: agent((120, 80, 200, 255), (40, 28, 60, 255), SKINS[1], "walking"),
    "agent_celebrating_01.png": lambda: agent((40, 170, 90, 255), (90, 55, 25, 255), SKINS[2], "celebrating"),
    "status_green.png": lambda: status_dot(GREEN),
    "status_amber.png": lambda: status_dot(AMBER),
    "status_blue.png": lambda: status_dot(BLUE),
    "status_red.png": lambda: status_dot(RED),
    "envelope_01.png": envelope,
}


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    outdir = sys.argv[1] if len(sys.argv) > 1 else os.path.join(root, "web-dist", "assets", "sprites")
    os.makedirs(outdir, exist_ok=True)
    for name, make in SPRITES.items():
        path = os.path.join(outdir, name)
        make().save(path)
        print("wrote", path)


if __name__ == "__main__":
    main()
