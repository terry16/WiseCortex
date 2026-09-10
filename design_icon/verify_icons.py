import os

from PIL import Image

ROOT = r"D:\apps\wisecortex"
files = [
    "web/public/apple-touch-icon.png",
    "web/public/favicon.ico",
    "desktop/src-tauri/icons/icon.png",
    "desktop/src-tauri/icons/icon.ico",
    "desktop/src-tauri/icons/icon.icns",
]
for rel in files:
    p = os.path.join(ROOT, rel.replace("/", os.sep))
    im = Image.open(p)
    extra = ""
    if im.format == "ICO":
        extra = " sizes=" + str(sorted(im.ico.sizes()))
    print(f"{rel:42} {im.format:5} {im.mode:5} {im.size}{extra}")

# 抽查 16px 是否有足够墨色（避免生成出一片糊）
ico = Image.open(os.path.join(ROOT, r"web\public\favicon.ico"))
ico.size = (16, 16)
small = ico.convert("RGBA").resize((16, 16), Image.LANCZOS)
rgba = small.load()
light = sum(
    1
    for y in range(16)
    for x in range(16)
    if rgba[x, y][3] > 100 and min(rgba[x, y][0], rgba[x, y][1], rgba[x, y][2]) > 190
)
print(f"\nfavicon 16px 高亮像素(墨白节点): {light}/256 = {light / 256:.0%}")
print("低于 15% 说明图形太细碎，小尺寸会糊——见 README。")
