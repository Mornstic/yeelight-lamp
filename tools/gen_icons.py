"""Generate the app icon and the three tray states from one geometry.

Run with Python + Pillow:
    python tools/gen_icons.py

All art is drawn at 8x and downsampled with LANCZOS, so the small sizes keep
clean edges. Tray art is authored on a 64-unit grid and exported at 32 px,
which is pixel-exact at 200% DPI and a clean 2:1 downsample at 100% DPI.
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "icons"
PREVIEW = ROOT / "design" / "screenshots" / "icons-preview.png"

LAMP = (255, 215, 171)


def gradient(size: int, stops: list[tuple[float, tuple[int, int, int, int]]]) -> Image.Image:
    """Vertical RGBA gradient with linear interpolation between stops."""
    h = size
    strip = Image.new("RGBA", (1, h))
    for y in range(h):
        t = y / max(1, h - 1)
        lo, hi = stops[0], stops[-1]
        for i in range(len(stops) - 1):
            if stops[i][0] <= t <= stops[i + 1][0]:
                lo, hi = stops[i], stops[i + 1]
                break
        span = max(1e-6, hi[0] - lo[0])
        f = min(1.0, max(0.0, (t - lo[0]) / span))
        color = tuple(round(lo[1][c] + (hi[1][c] - lo[1][c]) * f) for c in range(4))
        strip.putpixel((0, y), color)
    return strip.resize((size, size), Image.NEAREST)


def mask(size: int, box: tuple[float, float, float, float], radius: float) -> Image.Image:
    m = Image.new("L", (size, size), 0)
    ImageDraw.Draw(m).rounded_rectangle(box, radius=radius, fill=255)
    return m


def ellipse_mask(size: int, box: tuple[float, float, float, float]) -> Image.Image:
    m = Image.new("L", (size, size), 0)
    ImageDraw.Draw(m).ellipse(box, fill=255)
    return m


def polygon_mask(size: int, points: list[tuple[float, float]]) -> Image.Image:
    m = Image.new("L", (size, size), 0)
    ImageDraw.Draw(m).polygon(points, fill=255)
    return m


def blur(m: Image.Image, radius: float) -> Image.Image:
    return m.filter(ImageFilter.GaussianBlur(radius))


def union(*masks: Image.Image) -> Image.Image:
    out = masks[0]
    for item in masks[1:]:
        out = ImageChops.lighter(out, item)
    return out


def paint(base: Image.Image, layer: Image.Image) -> Image.Image:
    return Image.alpha_composite(base, layer)


def clipped(layer: Image.Image, m: Image.Image) -> Image.Image:
    return Image.composite(layer, Image.new("RGBA", layer.size, (0, 0, 0, 0)), m)


def tint(m: Image.Image, color: tuple[int, int, int, int]) -> Image.Image:
    layer = Image.new("RGBA", m.size, color[:3] + (0,))
    layer.putalpha(m.point(lambda v: v * color[3] // 255))
    return layer


def warm_glow(size: int, box, radius: float, spread: float, color) -> Image.Image:
    return blur(tint(mask(size, box, radius), color), spread)


def app_icon(px: int = 1024) -> Image.Image:
    ss = 4
    n = px * ss
    k = n / 1024
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))

    # ---- glass slab -------------------------------------------------------
    body = mask(n, (0, 0, n - 1, n - 1), 228 * k)
    base = gradient(
        n,
        [(0.0, (40, 45, 56, 255)), (0.42, (16, 19, 25, 255)), (1.0, (6, 8, 11, 255))],
    )
    img = paint(img, clipped(base, body))

    ambience = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    ImageDraw.Draw(ambience).ellipse((220 * k, 100 * k, 804 * k, 400 * k), fill=(255, 192, 132, 13))
    ambience = blur(ambience, 130 * k)
    img = paint(img, clipped(ambience, body))

    cool = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    ImageDraw.Draw(cool).ellipse((420 * k, 860 * k, 1220 * k, 1360 * k), fill=(84, 122, 178, 16))
    cool = blur(cool, 140 * k)
    img = paint(img, clipped(cool, body))

    sheen = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    ImageDraw.Draw(sheen).ellipse((-260 * k, -560 * k, 1284 * k, 300 * k), fill=(255, 255, 255, 6))
    sheen = blur(sheen, 60 * k)
    img = paint(img, clipped(sheen, body))

    rim = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    ImageDraw.Draw(rim).rounded_rectangle(
        (1.6 * k, 1.6 * k, n - 1.6 * k, n - 1.6 * k),
        radius=227 * k,
        outline=(255, 255, 255, 44),
        width=max(1, round(2.4 * k)),
    )
    img = paint(img, rim)
    top_gloss = warm_glow(n, (24 * k, 8 * k, n - 24 * k, 170 * k), 150 * k, 36 * k, (255, 255, 255, 12))
    img = paint(img, clipped(top_gloss, body))

    # ---- monitor ----------------------------------------------------------
    mon_box = (132 * k, 292 * k, 892 * k, 762 * k)
    screen_box = (150 * k, 310 * k, 874 * k, 744 * k)
    mon = mask(n, mon_box, 46 * k)
    screen = mask(n, screen_box, 31 * k)

    img = paint(img, clipped(gradient(n, [(0.285, (34, 39, 48, 255)), (0.744, (9, 11, 15, 255))]), mon))
    img = paint(img, clipped(gradient(n, [(0.303, (8, 10, 14, 255)), (0.727, (2, 3, 5, 255))]), screen))

    # Light wash on the glass: soft, top-weighted, no hard cone.
    wash = gradient(
        n,
        [(0.303, (255, 232, 206, 22)), (0.355, (255, 220, 188, 11)), (0.470, (255, 210, 172, 3)), (0.585, (255, 208, 170, 0))],
    )
    img = paint(img, clipped(wash, screen))
    hotspot = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    ImageDraw.Draw(hotspot).ellipse((320 * k, 300 * k, 704 * k, 408 * k), fill=(255, 238, 216, 20))
    img = paint(img, clipped(blur(hotspot, 36 * k), screen))

    # Faint diagonal reflection across the glass.
    refl = polygon_mask(
        n,
        [(206 * k, 480 * k), (360 * k, 480 * k), (540 * k, 744 * k), (386 * k, 744 * k)],
    )
    img = paint(img, clipped(tint(blur(refl, 40 * k), (255, 255, 255, 6)), screen))

    # Vignette keeps the lower glass deep instead of milky.
    img = paint(img, clipped(gradient(n, [(0.520, (0, 0, 0, 0)), (0.727, (0, 0, 0, 92))]), screen))

    bezel = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    bd = ImageDraw.Draw(bezel)
    bd.rounded_rectangle(mon_box, radius=46 * k, outline=(255, 255, 255, 52), width=max(1, round(6 * k)))
    bd.rounded_rectangle(
        (142 * k, 302 * k, 882 * k, 752 * k),
        radius=39 * k,
        outline=(255, 255, 255, 12),
        width=max(1, round(2 * k)),
    )
    img = paint(img, bezel)
    inner = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    ImageDraw.Draw(inner).rounded_rectangle(
        (150 * k, 310 * k, 874 * k, 744 * k), radius=31 * k, outline=(0, 0, 0, 110), width=max(1, round(3 * k))
    )
    img = paint(img, inner)
    # The tube lights the monitor's top edge; let that warm rim show outside the bar.
    top_lit = warm_glow(n, (140 * k, 288 * k, 884 * k, 302 * k), 6 * k, 3 * k, (255, 228, 196, 64))
    img = paint(img, clipped(top_lit, mon))
    edge_light = warm_glow(n, (196 * k, 298 * k, 828 * k, 342 * k), 24 * k, 10 * k, (255, 232, 200, 30))
    img = paint(img, clipped(edge_light, screen))

    # ---- stand ------------------------------------------------------------
    neck = mask(n, (492 * k, 756 * k, 532 * k, 830 * k), 10 * k)
    foot = mask(n, (344 * k, 820 * k, 680 * k, 878 * k), 28 * k)
    shadow = blur(tint(mask(n, (326 * k, 862 * k, 698 * k, 912 * k), 26 * k), (0, 0, 0, 110)), 26 * k)
    img = paint(img, shadow)
    img = paint(img, clipped(gradient(n, [(0.738, (60, 68, 83, 255)), (0.857, (19, 22, 29, 255))]), union(neck, foot)))
    img = paint(img, tint(blur(mask(n, (372 * k, 824 * k, 652 * k, 838 * k), 7 * k), 6 * k), (255, 255, 255, 26)))

    # ---- screen bar -------------------------------------------------------
    # Rests directly on the monitor's top edge, like the real clamp-on tube.
    bar_box = (210 * k, 220 * k, 814 * k, 288 * k)
    bar = mask(n, bar_box, 34 * k)
    # Keep the halo off the glass so the screen stays deep; the panel gets its
    # own controlled reflection instead.
    outside = blur(ImageChops.invert(screen), 2 * k)
    img = paint(img, clipped(warm_glow(n, (170 * k, 182 * k, 854 * k, 330 * k), 56 * k, 34 * k, (255, 198, 140, 80)), outside))
    img = paint(img, clipped(warm_glow(n, (106 * k, 124 * k, 918 * k, 404 * k), 92 * k, 86 * k, (255, 190, 128, 26)), outside))

    bar_grad = gradient(
        n,
        [
            (0.2148, (255, 253, 248, 255)),
            (0.2266, (255, 247, 233, 255)),
            (0.2422, (255, 228, 196, 255)),
            (0.2617, (255, 201, 151, 255)),
            (0.2812, (255, 186, 130, 255)),
        ],
    )
    contact = blur(tint(mask(n, (210 * k, 282 * k, 814 * k, 298 * k), 6 * k), (8, 5, 3, 66)), 5 * k)
    img = paint(img, contact)
    img = paint(img, clipped(bar_grad, bar))
    # Slightly darker moulded end caps keep it a product, not a pill.
    caps = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    cd = ImageDraw.Draw(caps)
    cd.rounded_rectangle((214 * k, 222 * k, 238 * k, 286 * k), radius=12 * k, fill=(58, 34, 16, 26))
    cd.rounded_rectangle((786 * k, 222 * k, 810 * k, 286 * k), radius=12 * k, fill=(58, 34, 16, 26))
    img = paint(img, clipped(blur(caps, 8 * k), bar))
    seams = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    sd = ImageDraw.Draw(seams)
    sd.rounded_rectangle((238.6 * k, 224 * k, 240.2 * k, 284 * k), radius=1 * k, fill=(84, 50, 26, 52))
    sd.rounded_rectangle((783.8 * k, 224 * k, 785.4 * k, 284 * k), radius=1 * k, fill=(84, 50, 26, 52))
    img = paint(img, clipped(blur(seams, 1.2 * k), bar))
    spec = mask(n, (234 * k, 227 * k, 790 * k, 243 * k), 8 * k)
    img = paint(img, clipped(tint(blur(spec, 4 * k), (255, 255, 255, 172)), bar))
    under = mask(n, (224 * k, 268 * k, 800 * k, 292 * k), 10 * k)
    img = paint(img, clipped(tint(blur(under, 7 * k), (255, 158, 92, 68)), bar))

    return img.resize((px, px), Image.LANCZOS)


def tray_icon(mode: str, px: int = 32, light: bool = False) -> Image.Image:
    """Bold, low-detail glyph that survives a 16 px tray.

    The bar and its light beam are the whole story: warm = lit, neutral = off,
    coral dot = unreachable. Light taskbars get deep ink instead of pale ink.
    """
    ss = 16
    n = px * ss * 2
    k = n / 64
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))

    on = mode == "on"
    offline = mode == "offline"
    bar_box = (8 * k, 16 * k, 56 * k, 28 * k)
    bar = mask(n, bar_box, 6 * k)

    if on:
        beam = polygon_mask(n, [(14 * k, 30 * k), (50 * k, 30 * k), (59 * k, 51 * k), (5 * k, 51 * k)])
        if light:
            fade = gradient(n, [(0.469, (214, 132, 58, 96)), (0.640, (214, 132, 58, 42)), (0.797, (214, 132, 58, 0))])
            bar_hi, bar_lo = (242, 176, 106, 255), (201, 122, 44, 255)
        else:
            fade = gradient(n, [(0.469, (255, 200, 146, 168)), (0.640, (255, 194, 136, 74)), (0.797, (255, 188, 126, 0))])
            img = paint(img, warm_glow(n, (10 * k, 10 * k, 54 * k, 36 * k), 8 * k, 5 * k, (255, 190, 128, 130)))
            bar_hi, bar_lo = (255, 246, 228, 255), (255, 194, 138, 255)
        img = paint(img, clipped(fade, blur(beam, 3.2 * k)))
    elif offline:
        if light:
            bar_hi, bar_lo = (134, 140, 150, 255), (118, 124, 134, 255)
        else:
            bar_hi, bar_lo = (142, 147, 156, 255), (128, 133, 142, 255)
    else:
        if light:
            bar_hi, bar_lo = (88, 94, 104, 255), (70, 76, 86, 255)
        else:
            bar_hi, bar_lo = (200, 204, 212, 255), (176, 181, 190, 255)

    img = paint(img, clipped(gradient(n, [(0.250, bar_hi), (0.4375, bar_lo)]), bar))

    if offline:
        # A status dot reads better than a slash at 16 px.
        badge = Image.new("RGBA", (n, n), (0, 0, 0, 0))
        ImageDraw.Draw(badge).ellipse((43 * k, 41 * k, 59 * k, 57 * k), fill=(214, 96, 70, 255))
        img = paint(img, badge)

    return img.resize((px, px), Image.LANCZOS)


def add_tile(sheet: Image.Image, art: Image.Image, x: int, y: int, size: int, dark: bool) -> None:
    tile = Image.new("RGBA", (size, size), (24, 27, 32, 255) if dark else (236, 238, 242, 255))
    icon = art if art.size == (size, size) else art.resize((size, size), Image.LANCZOS)
    tile.alpha_composite(icon)
    sheet.paste(tile, (x, y), tile)


def font(size: int) -> ImageFont.FreeTypeFont:
    try:
        return ImageFont.truetype("C:/Windows/Fonts/segoeui.ttf", size)
    except OSError:
        return ImageFont.load_default()


def preview(app: Image.Image, dark: dict[str, Image.Image], light: dict[str, Image.Image]) -> Image.Image:
    sheet = Image.new("RGBA", (1180, 720), (15, 17, 21, 255))
    draw = ImageDraw.Draw(sheet)
    label, dim = font(13), (138, 145, 158, 255)

    draw.text((32, 18), "APP ICON  ·  256 160 96 64 48 32 16", font=label, fill=dim)
    x, y = 32, 40
    for size in (256, 160, 96, 64, 48, 32, 16):
        add_tile(sheet, app, x, y, size, True)
        x += size + 22

    for row, (name, arts, dark_bg) in enumerate(
        (("DARK TASKBAR", dark, True), ("LIGHT TASKBAR", light, False))
    ):
        y = 330 + row * 190
        draw.text((32, y), f"TRAY · {name} · 32 px / 16 px zoom / 16 px native", font=label, fill=dim)
        strip = Image.new("RGBA", (1116, 146), (24, 27, 32, 255) if dark_bg else (243, 243, 243, 255))
        sheet.paste(strip, (32, y + 22))
        x = 52
        for mode, art in arts.items():
            add_tile(sheet, art, x, y + 52, 32, dark_bg)
            zoom = art.resize((16, 16), Image.LANCZOS).resize((128, 128), Image.NEAREST)
            sheet.paste(zoom, (x + 52, y + 28), zoom)
            small = art.resize((16, 16), Image.LANCZOS)
            tile = Image.new("RGBA", (16, 16), (24, 27, 32, 255) if dark_bg else (243, 243, 243, 255))
            tile.alpha_composite(small)
            sheet.paste(tile, (x + 192, y + 78), tile)
            label_color = (150, 156, 166, 255) if dark_bg else (86, 92, 102, 255)
            draw.text((x, y + 148), mode, font=label, fill=label_color)
            x += 340
    return sheet


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    master = app_icon(1024)
    master.save(OUT / "icon.png")
    master.save(
        OUT / "icon.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    dark = {mode: tray_icon(mode) for mode in ("on", "off", "offline")}
    light = {mode: tray_icon(mode, light=True) for mode in ("on", "off", "offline")}
    for mode, image in dark.items():
        image.save(OUT / f"tray_{mode}.png")
        light[mode].save(OUT / f"tray_{mode}_light.png")
    PREVIEW.parent.mkdir(parents=True, exist_ok=True)
    preview(master, dark, light).save(PREVIEW)
    print("icons written to", OUT)
    print("preview written to", PREVIEW)


if __name__ == "__main__":
    main()
