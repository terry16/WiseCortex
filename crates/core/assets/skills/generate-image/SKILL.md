---
name: generate-image
description: Use when the user wants to generate/produce an image file — 图片/图标/logo/海报/封面/示意图/流程图/架构图/信息图/插画/数据图表(chart)/SVG/PNG/JPG. WiseCortex 没有文生图模型，本技能教你用「写代码 → 渲染成位图」的方式产出 PNG/JPG（或矢量 SVG），并自动挑选机器上效果最好的渲染工具。不适用于照片级/绘画级写实图（那需要外部文生图 API）。
---

# generate-image

## Overview

WiseCortex **没有内置文生图模型**（没有 DALL·E / Stable Diffusion 这类工具）。要"生成图片"，正确做法是 **让模型写出图形代码（HTML+CSS 或 SVG 或绘图脚本）→ 用渲染器栅格化成 PNG/JPG**。

能做好的：图标、logo、海报/封面、卡片、示意图、流程图/架构图、信息图(infographic)、几何/算法插画、**数据图表**。
做不了：照片级、绘画级写实图（需外部文生图 API，本技能不涉及）。

## 决策第一步：用什么来源画

| 你要的图 | 用什么写 | 理由 |
|---|---|---|
| 海报 / 封面 / 卡片 / 信息图 / 插画 / 复杂排版 | **HTML + CSS**（写成 `.html`） | 模型最擅长，能用 flex 布局、渐变、阴影、滤镜、Web 字体，表达力最强 |
| 图标 / logo / 简单示意图 / 几何图形 | **SVG**（写成 `.svg`） | 矢量、干净、可无损放大 |
| 折线 / 柱状 / 散点 / 饼图 / 热力图等**数据图表** | **matplotlib**（Python） | 图表专业、坐标轴/图例处理最稳 |

> 写出来的 `.html` / `.svg` 在 WiseCortex 右侧预览面板可直接预览；但**最终交付物应是栅格化后的 PNG/JPG 文件**（除非用户只要矢量 SVG）。

## 决策第二步：选渲染器（按效果从高到低，挑机器上**已装**的第一个）

先探测可用工具（Linux/macOS 用 `command -v`，Windows 用 `where`）：

```bash
for t in chromium chromium-browser google-chrome resvg rsvg-convert cairosvg magick convert; do
  command -v "$t" >/dev/null 2>&1 && echo "available: $t"
done
python3 -c "import playwright" 2>/dev/null && echo "available: playwright(py)"
python3 -c "import cairosvg" 2>/dev/null && echo "available: cairosvg(py)"
python3 -c "import matplotlib" 2>/dev/null && echo "available: matplotlib"
```

**优先级（保真度由高到低）：**

### 1. 无头浏览器（效果最好，HTML 与 SVG 都能渲染）

最忠实的渲染器：渐变、阴影、滤镜、Web 字体、完整 CSS 全部正确。**HTML 来源首选这条。**

Chromium / Chrome 直接截图（无需 node）：
```bash
# 注意 --window-size 决定画布尺寸；2x 更清晰
chromium --headless --no-sandbox --hide-scrollbars \
  --force-device-scale-factor=2 --window-size=1200,630 \
  --screenshot=out.png --default-background-color=00000000 \
  "file://$(pwd)/poster.html"
# 命令名可能是 chromium-browser / google-chrome，按探测结果替换
```

Playwright（若已装，控制更精细，可截某个元素、设视口/缩放）：
```bash
python3 - <<'PY'
from playwright.sync_api import sync_playwright
import pathlib
with sync_playwright() as p:
    b = p.chromium.launch()
    pg = b.new_page(viewport={"width":1200,"height":630}, device_scale_factor=2)
    pg.goto("file://" + str(pathlib.Path("poster.html").resolve()))
    pg.screenshot(path="out.png")  # 整页；截元素用 pg.locator("#card").screenshot(...)
    b.close()
PY
```

### 2. resvg（SVG→PNG，轻量、SVG 保真度高）
```bash
resvg --zoom 2 icon.svg out.png          # 二进制版
npx -y @resvg/resvg-js-cli icon.svg out.png   # 或 node 版
```

### 3. rsvg-convert（librsvg，SVG→PNG/PDF）
```bash
rsvg-convert -z 2 -o out.png icon.svg
```

### 4. cairosvg（Python，SVG→PNG）
```bash
python3 -c "import cairosvg; cairosvg.svg2png(url='icon.svg', write_to='out.png', scale=2)"
```

### 5. ImageMagick（SVG 渲染弱，但 PNG→JPG / 缩放裁剪很好用，常作末端转换）
```bash
magick icon.svg -density 200 out.png      # 或旧版命令名 convert
```

### 6. matplotlib（数据图表专用）
```bash
python3 - <<'PY'
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
fig, ax = plt.subplots(figsize=(8,4.5), dpi=150)
ax.plot([1,2,3,4],[10,25,18,40], marker="o")
ax.set_title("示例"); ax.grid(True, alpha=.3)
fig.tight_layout(); fig.savefig("out.png")   # 直接存 out.jpg 也行
PY
```

## 要 JPG 时

多数 SVG 渲染器只出 PNG。先渲成 PNG，再转 JPG（JPG 不透明，需铺白底）：

```bash
magick out.png -background white -flatten -quality 92 out.jpg
# 或 Python(Pillow):
python3 -c "from PIL import Image; im=Image.open('out.png').convert('RGB'); im.save('out.jpg', quality=92)"
```
matplotlib / 浏览器截图可直接指定 `.jpg` 输出，无需二次转换。

## 清晰度

位图会糊的根因是分辨率不够。务必用 **2x**：浏览器 `--force-device-scale-factor=2` / Playwright `device_scale_factor=2`；resvg/rsvg `-z 2`；cairosvg `scale=2`；ImageMagick `-density 200`；matplotlib `dpi=150` 以上。

## 标准流程

1. 按"决策第一步"选来源；把图形代码写成 `.html` / `.svg`（用 write_file），落在工作目录。
2. 探测渲染器，按优先级选已装的第一个。
3. 渲染成 PNG（2x），需要 JPG 再转一道。
4. 告诉用户最终文件名。`.html` / `.svg` 可在右侧预览；PNG/JPG 是交付物。

## 一个都没装时：自己装（你有 shell）

**别停下来问用户、也别放弃。** 你有 shell 工具，直接把需要的渲染器装上再继续。
原则：**按本次任务的来源类型，装能满足需求里最轻的那个**，别无脑装 Chromium（它 ~400MB，很重）。

| 本次要画 | 自己装（择一，从轻到重） |
|---|---|
| 图标 / logo / SVG | `pip install cairosvg` ｜ `apt-get install -y librsvg2-bin`（得 `rsvg-convert`）｜ `npm i -g @resvg/resvg-js-cli` |
| 数据图表 | `pip install matplotlib` |
| 海报 / 信息图 / 富排版 HTML | 优先 `pip install playwright && playwright install --with-deps chromium`；或 `apt-get install -y chromium-browser`（或 `chromium`） |
| 已有 PNG 要转 JPG / 缩放 | `pip install pillow` ｜ `apt-get install -y imagemagick` |

装的时候：

- Linux 上若非 root 且命令需要权限，加 `sudo`（如 `sudo apt-get install -y ...`）；`apt` 前最好先 `apt-get update`。
- 装完用前面的探测命令确认可用，再继续渲染。
- **只有在真装不动时**（无网络、无权限、包源不可用）才停下，向用户说明卡在哪、建议手动执行哪条命令。

> 经验：图标/示意图类优先 `cairosvg`（一条 `pip` 最省事）；只有用户明确要海报/信息图这种富排版 HTML 渲染，才值得装 Chromium。
