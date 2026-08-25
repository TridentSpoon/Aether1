"""
Generate Cyberpunk / Cortana & Cephalon Simaris Robot Emoticon Icons.
Creates high-resolution holographic Robot Emoticon (🤖) variants:
- icon_cyan.png (Active / Standby - Cortana)
- icon_gold.png (Cephalon Simaris Sanctuary Amber)
- icon_green.png (Voice Listening)
- icon_purple.png (Thinking / Processing)
- icon_amber.png (Alert / Offline)
- icon.png (App launcher)
"""

import os
import math
from PIL import Image, ImageDraw, ImageFont, ImageFilter

ICON_DIR = os.path.join(os.path.dirname(__file__), "icons")
os.makedirs(ICON_DIR, exist_ok=True)

def generate_robot_emoticon_icon(filename: str, primary_color: tuple, glow_color: tuple, size: int = 128):
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    glow_img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    
    glow_draw = ImageDraw.Draw(glow_img)
    draw = ImageDraw.Draw(img)

    center = size // 2
    
    # Outer glowing frame
    margin = int(size * 0.12)
    head_box = [margin, int(size * 0.22), size - margin, size - int(size * 0.15)]
    corner_radius = int(size * 0.12)

    # 1. Glow Layer
    # Antenna glow
    glow_draw.line([(center, int(size * 0.22)), (center, int(size * 0.1))], fill=glow_color, width=int(size * 0.08))
    glow_draw.ellipse([center - int(size * 0.08), int(size * 0.04), center + int(size * 0.08), int(size * 0.16)], fill=glow_color)
    
    # Head box glow
    glow_draw.rounded_rectangle(head_box, radius=corner_radius, outline=glow_color, width=int(size * 0.08), fill=(glow_color[0], glow_color[1], glow_color[2], 50))
    
    # Ear dials glow
    ear_w = int(size * 0.06)
    ear_h = int(size * 0.18)
    glow_draw.rounded_rectangle([margin - ear_w, center - ear_h//2, margin, center + ear_h//2], radius=4, fill=glow_color)
    glow_draw.rounded_rectangle([size - margin, center - ear_h//2, size - margin + ear_w, center + ear_h//2], radius=4, fill=glow_color)

    glow_img = glow_img.filter(ImageFilter.GaussianBlur(radius=int(size * 0.06)))
    img = Image.alpha_composite(img, glow_img)

    # 2. Foreground Crisp Robot Drawing
    fg = ImageDraw.Draw(img)

    # Antenna
    fg.line([(center, int(size * 0.22)), (center, int(size * 0.1))], fill=primary_color, width=int(size * 0.04))
    fg.ellipse([center - int(size * 0.06), int(size * 0.04), center + int(size * 0.06), int(size * 0.14)], fill=(255, 255, 255, 250))

    # Ears / Side bolts
    fg.rounded_rectangle([margin - ear_w, center - ear_h//2, margin, center + ear_h//2], radius=3, fill=primary_color)
    fg.rounded_rectangle([size - margin, center - ear_h//2, size - margin + ear_w, center + ear_h//2], radius=3, fill=primary_color)

    # Head Chassis
    fg.rounded_rectangle(head_box, radius=corner_radius, outline=primary_color, width=int(size * 0.04), fill=(10, 15, 28, 230))

    # Eyes (glowing cyan / gold round lenses)
    eye_r = int(size * 0.09)
    eye_y = int(size * 0.44)
    left_eye_x = int(size * 0.35)
    right_eye_x = int(size * 0.65)

    # Left eye
    fg.ellipse([left_eye_x - eye_r, eye_y - eye_r, left_eye_x + eye_r, eye_y + eye_r], fill=primary_color)
    fg.ellipse([left_eye_x - eye_r//2, eye_y - eye_r//2, left_eye_x + eye_r//2, eye_y + eye_r//2], fill=(255, 255, 255, 250))

    # Right eye
    fg.ellipse([right_eye_x - eye_r, eye_y - eye_r, right_eye_x + eye_r, eye_y + eye_r], fill=primary_color)
    fg.ellipse([right_eye_x - eye_r//2, eye_y - eye_r//2, right_eye_x + eye_r//2, eye_y + eye_r//2], fill=(255, 255, 255, 250))

    # Futuristic HUD Mouth / Speaker Grille
    mouth_y = int(size * 0.68)
    mouth_w = int(size * 0.44)
    mouth_h = int(size * 0.09)
    mouth_box = [center - mouth_w//2, mouth_y, center + mouth_w//2, mouth_y + mouth_h]
    fg.rounded_rectangle(mouth_box, radius=3, outline=primary_color, width=int(size * 0.02), fill=(5, 10, 20, 200))
    
    # Mouth bars
    num_bars = 5
    bar_step = mouth_w / (num_bars + 1)
    for i in range(1, num_bars + 1):
        bx = int(center - mouth_w//2 + i * bar_step)
        fg.line([(bx, mouth_y + 2), (bx, mouth_y + mouth_h - 2)], fill=primary_color, width=2)

    out_path = os.path.join(ICON_DIR, filename)
    img.save(out_path, "PNG")
    print(f"Generated robot icon: {out_path}")
    return out_path

def generate_all_icons():
    # Cortana Cyan (Default)
    generate_robot_emoticon_icon("icon_cyan.png", (0, 240, 255, 255), (0, 180, 255, 180))
    # Cephalon Simaris Sanctuary Gold/Amber
    generate_robot_emoticon_icon("icon_gold.png", (255, 170, 0, 255), (255, 100, 0, 180))
    # Voice Listening Green
    generate_robot_emoticon_icon("icon_green.png", (0, 255, 170, 255), (0, 255, 120, 180))
    # Thinking Purple
    generate_robot_emoticon_icon("icon_purple.png", (180, 70, 255, 255), (140, 0, 255, 180))
    # Alert Amber / Offline
    generate_robot_emoticon_icon("icon_amber.png", (255, 80, 0, 255), (255, 50, 0, 180))
    # Standard app icon (high res)
    generate_robot_emoticon_icon("icon.png", (0, 240, 255, 255), (0, 180, 255, 180), size=256)

if __name__ == "__main__":
    generate_all_icons()
