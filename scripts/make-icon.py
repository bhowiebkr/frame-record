"""Regenerate the small vector-style app icon. Development only; requires Pillow."""
from pathlib import Path
from PIL import Image, ImageDraw

root = Path(__file__).resolve().parent.parent / "assets"
root.mkdir(exist_ok=True)
paths = [
    [(58, 104), (58, 68), (94, 68)],
    [(162, 68), (198, 68), (198, 104)],
    [(58, 152), (58, 188), (94, 188)],
    [(162, 188), (198, 188), (198, 152)],
]
svg_paths = "\n".join(
    f'<path d="M{x[0][0]} {x[0][1]} L{x[1][0]} {x[1][1]} L{x[2][0]} {x[2][1]}"/>'
    for x in paths
)
(root / "app-icon.svg").write_text(f'''<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">
<rect x="8" y="8" width="240" height="240" rx="52" fill="#181818" stroke="#303030" stroke-width="2"/>
<g fill="none" stroke="#0078d4" stroke-width="14" stroke-linecap="square" stroke-linejoin="miter">{svg_paths}</g>
<circle cx="128" cy="128" r="23" fill="#e85d54"/>
</svg>
''', encoding="utf-8")

scale = 4
image = Image.new("RGBA", (256 * scale, 256 * scale))
draw = ImageDraw.Draw(image)
draw.rounded_rectangle([8*scale, 8*scale, 248*scale, 248*scale], radius=52*scale,
                       fill="#181818", outline="#303030", width=2*scale)
for points in paths:
    draw.line([(x*scale, y*scale) for x, y in points], fill="#0078d4", width=14*scale)
draw.ellipse([105*scale, 105*scale, 151*scale, 151*scale], fill="#e85d54")
image = image.resize((256, 256), Image.Resampling.LANCZOS)
image.save(root / "app-icon.png")
image.save(root / "app-icon.ico", sizes=[(s, s) for s in (16, 20, 24, 32, 40, 48, 64, 128, 256)])
