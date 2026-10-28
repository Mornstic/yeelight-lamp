"""Generate NSIS header/sidebar bitmaps from the app icon."""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ICON = ROOT / "src-tauri" / "icons" / "icon.png"
OUT = ROOT / "src-tauri" / "icons"
FONT = Path("C:/Windows/Fonts/msyhbd.ttc")
FONT_LIGHT = Path("C:/Windows/Fonts/msyh.ttc")

S = 4


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


def text(draw, xy, content, fnt, fill, anchor="la"):
    draw.text(xy, content, font=fnt, fill=fill, anchor=anchor)


def icon_layer(px):
    icon = Image.open(ICON).convert("RGBA")
    return icon.resize((px, px), Image.Resampling.LANCZOS)


def header():
    w, h = 150 * S, 57 * S
    img = vgrad(
        (w, h),
        [(0.0, (32, 36, 44, 255)), (0.55, (16, 19, 25, 255)), (1.0, (9, 11, 15, 255))],
    )
    img = Image.alpha_composite(img, glow((w, h), (300, -160, 700, 120), (255, 190, 130, 52), 70 * S))
    img = Image.alpha_composite(img, glow((w, h), (-120, 60, 180, 260), (84, 122, 178, 26), 60 * S))

    icon = icon_layer(42 * S)
    img.alpha_composite(icon, (10 * S, 8 * S))

    draw = ImageDraw.Draw(img)
    text(draw, (61 * S, 10 * S), "Yeelight", font(FONT, 15 * S), (246, 246, 248, 255))
    text(draw, (61 * S, 32 * S), "屏幕挂灯", font(FONT_LIGHT, 10 * S), (198, 200, 206, 255))

    line = Image.new("RGBA", (w, 3 * S), (0, 0, 0, 0))
    ImageDraw.Draw(line).rectangle((0, 0, w, 3 * S), fill=(255, 196, 140, 40))
    img.alpha_composite(line, (0, h - 3 * S))

    return img.convert("RGB").resize((150, 57), Image.Resampling.LANCZOS)


def sidebar():
    w, h = 164 * S, 314 * S
    img = vgrad(
        (w, h),
        [(0.0, (25, 28, 35, 255)), (0.42, (14, 16, 21, 255)), (1.0, (7, 8, 11, 255))],
    )
    img = Image.alpha_composite(img, glow((w, h), (-70 * S, -60 * S, 230 * S, 120 * S), (255, 188, 128, 44), 44 * S))
    img = Image.alpha_composite(img, glow((w, h), (-80 * S, 210 * S, 110 * S, 360 * S), (84, 122, 178, 24), 46 * S))

    icon = icon_layer(102 * S)
    img.alpha_composite(icon, ((w - icon.width) // 2, 38 * S))

    draw = ImageDraw.Draw(img)
    text(draw, (w // 2, 160 * S), "Yeelight", font(FONT, 20 * S), (246, 246, 248, 255), "ma")
    text(draw, (w // 2, 188 * S), "屏幕挂灯", font(FONT_LIGHT, 12 * S), (190, 192, 198, 255), "ma")

    divider = Image.new("RGBA", (w, 2 * S), (0, 0, 0, 0))
    ImageDraw.Draw(divider).rectangle((70 * S, 0, 94 * S, 2 * S), fill=(255, 255, 255, 34))
    img.alpha_composite(divider, (0, 212 * S))

    label = "TUBE PLUS2"
    fnt = font(FONT, 5 * S)
    widths = [draw.textlength(ch, font=fnt) for ch in label]
    gap = 2.6 * S
    total = sum(widths) + gap * (len(label) - 1)
    x = (w - total) / 2
    for ch, cw in zip(label, widths):
        draw.text((x, 224 * S), ch, font=fnt, fill=(200, 162, 122, 255))
        x += cw + gap

    text(draw, (w // 2, 270 * S), "局域网直连 · 断网可用", font(FONT_LIGHT, 8 * S), (170, 173, 180, 255), "ma")
    text(draw, (w // 2, 291 * S), "v0.1.1", font(FONT_LIGHT, 7 * S), (126, 129, 136, 255), "ma")

    rim = Image.new("RGBA", (w, 2 * S), (0, 0, 0, 0))
    ImageDraw.Draw(rim).rectangle((0, 0, w, 2 * S), fill=(255, 255, 255, 22))
    img.alpha_composite(rim, (0, 0))

    return img.convert("RGB").resize((164, 314), Image.Resampling.LANCZOS)


def main():
    header().save(OUT / "installer-header.bmp", "BMP")
    sidebar().save(OUT / "installer-sidebar.bmp", "BMP")
    print("wrote installer-header.bmp (150x57) and installer-sidebar.bmp (164x314)")


if __name__ == "__main__":
    main()
