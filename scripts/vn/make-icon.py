"""Draws the fork's icon: assets/icons/videoedit.ico and videoedit_256.png."""
from pathlib import Path

from PIL import Image, ImageDraw

SIZE = 1024
OUT = Path(__file__).resolve().parents[2] / "assets" / "icons"


def draw() -> Image.Image:
    top, bottom = (139, 92, 246), (14, 116, 144)  # violet to teal
    gradient = Image.new("RGBA", (SIZE, SIZE))
    pixels = gradient.load()
    for y in range(SIZE):
        for x in range(SIZE):
            t = (x + y) / (2 * SIZE)
            pixels[x, y] = tuple(round(a + (b - a) * t) for a, b in zip(top, bottom)) + (255,)
    mask = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(mask).rounded_rectangle((40, 40, SIZE - 40, SIZE - 40), radius=220, fill=255)
    icon = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    icon.paste(gradient, mask=mask)
    pen = ImageDraw.Draw(icon)
    # Play triangle.
    pen.polygon([(370, 230), (370, 610), (690, 420)], fill="white")
    # Timeline: two lanes of clips with a cut between them.
    pen.rounded_rectangle((200, 690, 500, 760), radius=30, fill="white")
    pen.rounded_rectangle((540, 690, 824, 760), radius=30, fill=(255, 255, 255, 170))
    pen.rounded_rectangle((200, 800, 380, 860), radius=28, fill=(255, 255, 255, 170))
    pen.rounded_rectangle((420, 800, 824, 860), radius=28, fill="white")
    return icon


def main() -> None:
    icon = draw()
    icon.resize((256, 256), Image.LANCZOS).save(OUT / "videoedit_256.png")
    icon.save(OUT / "videoedit.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])


if __name__ == "__main__":
    main()
