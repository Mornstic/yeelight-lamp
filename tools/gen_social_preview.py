"""Generate the GitHub social preview image (1280x640)."""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ICON = ROOT / "src-tauri" / "icons" / "icon.png"
PANEL = ROOT / "docs" / "screenshots" / "panel-on.png"
OUT = ROOT / "docs" / "screenshots" / "social-preview.png"
FONT_BOLD = Path("C:/Windows/Fonts/msyhbd.ttc")
FONT_LIGHT = Path("C:/Windows/Fonts/msyh.ttc")

W, H = 1280, 640
S = 2


def vgrad(size, stops):
    w, h = size
    column = Image.new("RGBA", (1, h))
    for y in range(h):
        t = y / max(h - 1, 1)
        for i in range(len(stops) - 1):
            p0, c0 = stops[i]
            p1, c1 = stops[i + 1]
            if p0 <= t <= p1:
                k = (t - p0) / (p1 - p0) if p1 > p0 else 0.0
                column.putpixel(
                    (0, y),
                    tuple(round(c0[j] + (c1[j] - c0[j]) * k) for j in range(4)),
                )
                break
        else:
            column.putpixel((0, y), stops[-1][1])
    return column.resize((w, h), Image.Resampling.NEAREST)


def glow(size, box, color, radius):
    layer = Image.new("RGBA", size, (0, 0, 0, 0))
    ImageDraw.Draw(layer).ellipse(box, fill=color)
    return layer.filter(ImageFilter.GaussianBlur(radius))


def font(path, size):
    return ImageFont.truetype(str(path), size)


def tracked_text(draw, xy, content, fnt, fill, tracking=0):
    x, y = xy
    for ch in content:
        draw.text((x, y), ch, font=fnt, fill=fill)
        x += draw.textlength(ch, font=fnt) + tracking
    return x


def chip(img, xy, label, fnt):
    x, y = xy
    width = ImageDraw.Draw(img).textlength(label, font=fnt) + 44 * S
    height = 42 * S

    layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    draw.rounded_rectangle(
        (x, y, x + width, y + height),
        radius=21 * S,
        fill=(255, 255, 255, 10),
        outline=(255, 255, 255, 26),
        width=1 * S,
    )
    draw.text((x + 22 * S, y + 10 * S), label, font=fnt, fill=(208, 211, 218, 255))
    img.alpha_composite(layer)
    return x + width + 12 * S


def panel_layer():
    panel = Image.open(PANEL).convert("RGB")
    panel = panel.resize((390 * S, 560 * S), Image.Resampling.LANCZOS)

    width, height = panel.size
    mask = Image.new("L", (width, height), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, width, height), radius=30 * S, fill=255)
    panel = panel.convert("RGBA")
    panel.putalpha(mask)
    return panel


def main():
    size = (W * S, H * S)
    img = vgrad(
        size,
        [
            (0.0, (28, 32, 40, 255)),
            (0.48, (15, 18, 24, 255)),
            (1.0, (7, 9, 13, 255)),
        ],
    )
    img = Image.alpha_composite(
        img, glow(size, (-160 * S, -120 * S, 620 * S, 430 * S), (255, 188, 126, 50), 110 * S)
    )
    img = Image.alpha_composite(
        img, glow(size, (820 * S, 260 * S, 1480 * S, 760 * S), (74, 110, 166, 32), 120 * S)
    )

    rim = Image.new("RGBA", size, (0, 0, 0, 0))
    ImageDraw.Draw(rim).rectangle((0, 0, size[0], 2 * S), fill=(255, 255, 255, 24))
    img = Image.alpha_composite(img, rim)

    icon = Image.open(ICON).convert("RGBA").resize((112 * S, 112 * S), Image.Resampling.LANCZOS)
    img.alpha_composite(icon, (72 * S, 64 * S))

    draw = ImageDraw.Draw(img)
    eyebrow_font = font(FONT_BOLD, 15 * S)
    tracked_text(
        draw,
        (76 * S, 218 * S),
        "YEELIGHT · TUBE PLUS2",
        eyebrow_font,
        (203, 164, 124, 255),
        2.6 * S,
    )

    draw.text((72 * S, 246 * S), "屏幕挂灯", font=font(FONT_BOLD, 78 * S), fill=(246, 247, 249, 255))
    draw.text(
        (76 * S, 360 * S),
        "局域网直连 · 不经过云端",
        font=font(FONT_LIGHT, 28 * S),
        fill=(198, 202, 210, 255),
    )

    chip_font = font(FONT_LIGHT, 18 * S)
    x = 76 * S
    for label in ("无级调光", "四档场景", "自动补光"):
        x = chip(img, (x, 428 * S), label, chip_font)

    draw.text(
        (76 * S, 566 * S),
        "Windows 10 / 11   ·   Tauri 2   ·   Rust",
        font=font(FONT_LIGHT, 18 * S),
        fill=(136, 141, 151, 255),
    )

    panel = panel_layer()
    px = W * S - 72 * S - panel.width
    py = 40 * S

    shadow = Image.new("RGBA", size, (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        (px + 10 * S, py + 22 * S, px + panel.width + 10 * S, py + panel.height + 22 * S),
        radius=32 * S,
        fill=(0, 0, 0, 165),
    )
    img = Image.alpha_composite(img, shadow.filter(ImageFilter.GaussianBlur(24 * S)))
    img = Image.alpha_composite(img, glow(size, (px - 90 * S, py + 210 * S, px + 360 * S, py + 600 * S), (255, 184, 120, 38), 110 * S))
    img.alpha_composite(panel, (px, py))

    border = Image.new("RGBA", size, (0, 0, 0, 0))
    ImageDraw.Draw(border).rounded_rectangle(
        (px, py, px + panel.width, py + panel.height),
        radius=30 * S,
        outline=(255, 255, 255, 28),
        width=2 * S,
    )
    img = Image.alpha_composite(img, border)

    img = img.convert("RGB").resize((W, H), Image.Resampling.LANCZOS)
    img.save(OUT, "PNG", optimize=True)
    print(f"wrote {OUT.relative_to(ROOT)} ({OUT.stat().st_size / 1024:.1f} KB)")


if __name__ == "__main__":
    main()
