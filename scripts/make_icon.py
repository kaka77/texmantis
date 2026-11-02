#!/usr/bin/env python3
"""青刃 TexMantis 应用图标：刃中藏螳
   - 青色直刃（竹叶形刀身）+ 镡 + 缠纹柄
   - 刃身负空间镂出螳首，两只薄荷色眼睛微微不对称（发亮）
   - 触角破锋而出，右缘白刃高光
"""
from PIL import Image, ImageDraw, ImageChops
import os

S = 1024
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)

# ---- 徽章底：圆角方 + 深墨青渐变 ----
badge = Image.new("L", (S, S), 0)
bd = ImageDraw.Draw(badge)
bd.rounded_rectangle([20, 20, S - 20, S - 20], radius=210, fill=255)
grad = Image.new("RGBA", (S, S))
gd = ImageDraw.Draw(grad)
top, bot = (8, 44, 35), (2, 10, 8)
for y in range(S):
    t = y / S
    gd.line([(0, y), (S, y)], fill=tuple(int(top[i] + (bot[i] - top[i]) * t) for i in range(3)) + (255,))
img.paste(grad, (0, 0), badge)

# ---- 刀身几何 ----
T = (512, 60)        # 刀尖
B = (352, 848)       # 刀背底
E = (464, 848)       # 刀刃底（镡上沿）
P1 = (700, 440)      # 刃曲线控制点

def quad(p0, p1, p2, n=40):
    pts = []
    for i in range(n + 1):
        t = i / n
        x = (1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * p1[0] + t ** 2 * p2[0]
        y = (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * p1[1] + t ** 2 * p2[1]
        pts.append((x, y))
    return pts

edge_pts = quad(T, P1, E)                 # 刀刃（右缘曲线）
blade_pts = [T] + edge_pts + [(512, 872), B]  # + 弧形刀跟 + 刀背回尖

blade_mask = Image.new("L", (S, S), 0)
bm = ImageDraw.Draw(blade_mask)
bm.polygon(blade_pts, fill=255)

# 刀身渐变（翡翠 → 深翡）
blade_grad = Image.new("RGBA", (S, S))
bgd = ImageDraw.Draw(blade_grad)
ct, cb = (52, 211, 153), (6, 128, 96)
for y in range(S):
    t = max(0.0, min(1.0, (y - T[1]) / (872 - T[1])))
    bgd.line([(0, y), (S, y)], fill=tuple(int(ct[i] + (cb[i] - ct[i]) * t) for i in range(3)) + (255,))

# ---- 负空间：刃中螳首 ----
cutout = Image.new("L", (S, S), 0)
cd = ImageDraw.Draw(cutout)
cd.polygon([(512, 196), (576, 436), (512, 516), (448, 436)], fill=255)  # 螳首剪影
final_mask = ImageChops.subtract(blade_mask, cutout)
img.paste(blade_grad, (0, 0), final_mask)

# ---- 螳眼：亮薄荷，微不对称，带高光（盯着你） ----
d.ellipse([487 - 17, 400 - 17, 487 + 17, 400 + 17], fill=(185, 251, 217, 255))
d.ellipse([537 - 19, 403 - 19, 537 + 19, 403 + 19], fill=(185, 251, 217, 255))
d.ellipse([487 - 6, 400 - 8, 487 + 3, 400 + 1], fill=(4, 40, 30, 255))
d.ellipse([537 - 7, 403 - 9, 537 + 4, 403 + 2], fill=(4, 40, 30, 255))
d.ellipse([491, 388, 499, 396], fill=(255, 255, 255, 230))
d.ellipse([543, 390, 552, 399], fill=(255, 255, 255, 230))

# ---- 白刃高光（右缘） ----
glint = [(x + 7, y + 2) for x, y in edge_pts[10:34]]
d.line(glint, fill=(240, 253, 250, 210), width=9, joint="curve")

# ---- 触角破锋而出 ----
d.line([(480, 150), (340, 44)], fill=(110, 231, 183, 255), width=15, joint="curve")
d.line([(546, 150), (686, 44)], fill=(110, 231, 183, 255), width=15, joint="curve")
d.ellipse([322, 30, 358, 66], fill=(110, 231, 183, 255))
d.ellipse([668, 30, 704, 66], fill=(110, 231, 183, 255))

# ---- 镡（护手）与缠纹刀柄 ----
d.rounded_rectangle([388, 866, 636, 912], radius=18, fill=(14, 74, 57, 255))
d.rounded_rectangle([462, 912, 562, 986], radius=14, fill=(16, 63, 51, 255))
for i, y0 in enumerate([928, 956]):
    d.line([(462 + 8, y0), (562 - 8, y0 + 14)], fill=(10, 41, 33, 255), width=10)
d.ellipse([498, 992, 526, 1010], fill=(110, 231, 183, 255))

# ---- 导出多尺寸 ----
out_dir = "src-tauri/icons"
os.makedirs(out_dir, exist_ok=True)
img.save(os.path.join(out_dir, "icon.ico"),
         sizes=[(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (24, 24), (16, 16)],
         bitmap_format="bmp")
img.resize((256, 256), Image.LANCZOS).save(os.path.join(out_dir, "icon.png"))
img.resize((128, 128), Image.LANCZOS).save(os.path.join(out_dir, "128x128.png"))
img.resize((32, 32), Image.LANCZOS).save(os.path.join(out_dir, "32x32.png"))
two_x = Image.new("RGBA", (256, 256), (0, 0, 0, 0))
two_x.paste(img.resize((128, 128), Image.LANCZOS), (0, 0))
two_x.save(os.path.join(out_dir, "128x128@2x.png"))
print("刃中藏螳 icon generated")
