"""把 design_icon/icon.svg 渲染成各平台需要的位图，覆盖 web/ 与 desktop/ 下的图标产物。

用法：
    python design_icon/build_icons.py
    python design_icon/verify_icons.py   # 核对格式/尺寸，并量 16px 的墨色占比

本机没有 cairosvg/ImageMagick，改用 Edge 无头截图拿到 1024 的高清底图，
再用 Pillow 逐级缩放（LANCZOS）生成 ico / icns / png。

注意：Edge 截图必须给足 --virtual-time-budget 并在之后 sleep，
否则文件还没落盘就返回（踩过这个坑）。
"""

import os
import shutil
import subprocess
import sys
import time

from PIL import Image

EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
ROOT = r"D:\apps\wisecortex"
DESIGN = os.path.join(ROOT, "design_icon")
SRC_SVG = os.path.join(DESIGN, "icon.svg")
BASE = os.path.join(DESIGN, "_render_1024.png")

# 用一个纯净页面渲染，避免任何 body 边距混进截图
HTML = os.path.join(DESIGN, "_render.html")
with open(HTML, "w", encoding="utf-8") as f:
    f.write(
        '<!doctype html><meta charset="utf-8">'
        "<style>html,body{margin:0;padding:0;background:transparent}"
        "img{display:block;width:1024px;height:1024px}</style>"
        '<img src="icon.svg">'
    )

subprocess.run(
    [
        EDGE,
        "--headless=new",
        "--disable-gpu",
        "--virtual-time-budget=4000",
        "--default-background-color=00000000",  # 透明背景
        f"--screenshot={BASE}",
        "--window-size=1024,1024",
        "file:///" + HTML.replace("\\", "/"),
    ],
    check=False,
)
time.sleep(6)

if not os.path.exists(BASE):
    sys.exit("渲染失败：没有生成 " + BASE)

base = Image.open(BASE).convert("RGBA")
print("base:", base.size)


def resized(n: int) -> Image.Image:
    return base.resize((n, n), Image.LANCZOS)


targets = []

# --- web/public ---
web = os.path.join(ROOT, "web", "public")
resized(180).save(os.path.join(web, "apple-touch-icon.png"))
targets.append("web/public/apple-touch-icon.png")

ico_sizes = [16, 24, 32, 48, 64, 128, 256]
resized(256).save(
    os.path.join(web, "favicon.ico"), sizes=[(s, s) for s in ico_sizes]
)
targets.append("web/public/favicon.ico")

shutil.copyfile(SRC_SVG, os.path.join(web, "icon.svg"))
targets.append("web/public/icon.svg")

# --- desktop/src-tauri/icons ---
ic = os.path.join(ROOT, "desktop", "src-tauri", "icons")
resized(512).save(os.path.join(ic, "icon.png"))
targets.append("desktop/src-tauri/icons/icon.png")

resized(256).save(os.path.join(ic, "icon.ico"), sizes=[(s, s) for s in ico_sizes])
targets.append("desktop/src-tauri/icons/icon.ico")

# icns：Pillow 要求最小 16 最大 1024，且需为方图
resized(1024).save(os.path.join(ic, "icon.icns"))
targets.append("desktop/src-tauri/icons/icon.icns")

print("\n written:")
for t in targets:
    p = os.path.join(ROOT, t.replace("/", os.sep))
    print(f"  {t}  {os.path.getsize(p) / 1024:.1f} KB")

# 清掉中间产物，保持目录干净（下次跑会重新生成）
base.close()
for junk in (HTML, BASE):
    if os.path.exists(junk):
        os.remove(junk)
