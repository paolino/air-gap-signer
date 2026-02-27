#!/usr/bin/env python3
"""
Generate SVG circuit schematic for the SE050C1HQ1 breakout board.

Run:  python3 generate_schematic.py

No external libraries required.
Output: ../../docs/assets/se050-breakout-schematic.svg
"""

from __future__ import annotations

import os

# ---------------------------------------------------------------------------
# Canvas & style
# ---------------------------------------------------------------------------

W, H = 920, 660
STROKE = "#333"
STROKE_W = 2
FONT = "sans-serif"

# ---------------------------------------------------------------------------
# Layout coordinates
# ---------------------------------------------------------------------------

VCC_Y = 55  # VCC rail y

# J1 header
J1_BODY_X, J1_BODY_W = 20, 95
J1_PIN_X = J1_BODY_X + J1_BODY_W  # pin circle at body right edge
J1_STUB = J1_PIN_X + 40           # stub end (wire starts here)
J1_PINS: list[tuple[int, str, int]] = [
    (1, "VCC",   110),
    (2, "GND",   175),
    (3, "SDA",   260),
    (4, "SCL",   330),
    (5, "ENA",   400),
    (6, "RST_N", 460),
    (7, "VIN",   520),
    (8, "(nc)",  580),
]

# U1 SE050
U1_BODY_X, U1_BODY_W = 650, 190
U1_STUB = U1_BODY_X - 40  # stub end (wire ends here)
U1_PINS: list[tuple[int, str, int]] = [
    (18, "VCC",   130),
    (19, "GND",   195),
    (9,  "SDA",   260),
    (10, "SCL",   330),
    (11, "ENA",   400),
    (14, "RST_N", 460),
    (12, "VIN",   520),
]

# Passives x-positions
C1_X, C2_X = 270, 360
R1_X, R2_X = 455, 545

CAP_BOT = 145   # capacitor bottom (GND)
# Resistor bottoms match signal line y-coords
R1_BOT = 260    # SDA
R2_BOT = 330    # SCL

# VCC rail horizontal extent
VCC_X1 = J1_STUB + 40   # 195
VCC_X2 = U1_STUB - 10   # 600


# ---------------------------------------------------------------------------
# SVG primitives
# ---------------------------------------------------------------------------


def _attrs(**kw: object) -> str:
    parts = []
    for k, v in kw.items():
        name = k.replace("_", "-")
        parts.append(f'{name}="{v}"')
    return " ".join(parts)


def line(x1: float, y1: float, x2: float, y2: float) -> str:
    return f'  <line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}"/>\n'


def rect(x: float, y: float, w: float, h: float, **kw: object) -> str:
    return f'  <rect x="{x}" y="{y}" width="{w}" height="{h}" {_attrs(**kw)}/>\n'


def text(x: float, y: float, content: str, **kw: object) -> str:
    return f'  <text x="{x}" y="{y}" {_attrs(**kw)}>{content}</text>\n'


def circle(cx: float, cy: float, r: float, **kw: object) -> str:
    return f'  <circle cx="{cx}" cy="{cy}" r="{r}" {_attrs(**kw)}/>\n'


def polyline(pts: list[tuple[float, float]]) -> str:
    s = " ".join(f"{x},{y}" for x, y in pts)
    return f'  <polyline points="{s}"/>\n'


def junction(x: float, y: float) -> str:
    """Filled dot at wire junction."""
    return circle(x, y, 3.5, fill=STROKE, stroke="none")


# ---------------------------------------------------------------------------
# Schematic symbols
# ---------------------------------------------------------------------------


def resistor_v(x: float, y1: float, y2: float,
               designator: str, value: str) -> str:
    """Vertical resistor (US zigzag) from (x, y1) to (x, y2)."""
    body = 48
    mid = (y1 + y2) / 2
    bt, bb = mid - body / 2, mid + body / 2
    amp, n = 8, 6
    seg = body / n

    s = line(x, y1, x, bt)
    pts: list[tuple[float, float]] = [(x, bt)]
    for i in range(1, n):
        pts.append((x + amp * (1 if i % 2 else -1), bt + i * seg))
    pts.append((x, bb))
    s += polyline(pts)
    s += line(x, bb, x, y2)

    s += text(x + 14, mid - 5, designator,
              font_size="12", fill=STROKE, stroke="none")
    s += text(x + 14, mid + 11, value,
              font_size="11", fill="#666", stroke="none")
    return s


def capacitor_v(x: float, y1: float, y2: float,
                designator: str, value: str) -> str:
    """Vertical capacitor from (x, y1) to (x, y2)."""
    mid = (y1 + y2) / 2
    gap, pw = 5, 10

    s = line(x, y1, x, mid - gap)
    s += line(x - pw, mid - gap, x + pw, mid - gap)
    s += line(x - pw, mid + gap, x + pw, mid + gap)
    s += line(x, mid + gap, x, y2)

    s += text(x + 16, mid - 3, designator,
              font_size="12", fill=STROKE, stroke="none")
    s += text(x + 16, mid + 13, value,
              font_size="11", fill="#666", stroke="none")
    return s


def vcc_symbol(x: float, y: float) -> str:
    """VCC power symbol: bar + upward stub + label."""
    s = line(x, y, x, y - 12)
    s += line(x - 10, y - 12, x + 10, y - 12)
    s += text(x, y - 18, "VCC",
              font_size="11", text_anchor="middle",
              fill=STROKE, stroke="none")
    return s


def gnd_symbol(x: float, y: float) -> str:
    """GND symbol: three decreasing horizontal bars."""
    s = line(x, y, x, y + 8)
    s += line(x - 10, y + 8, x + 10, y + 8)
    s += line(x - 6, y + 13, x + 6, y + 13)
    s += line(x - 2, y + 18, x + 2, y + 18)
    return s


# ---------------------------------------------------------------------------
# Component drawings
# ---------------------------------------------------------------------------


def draw_j1() -> str:
    """J1: 8-pin header connector."""
    y_top = J1_PINS[0][2] - 25
    y_bot = J1_PINS[-1][2] + 25
    s = ""

    # Body
    s += rect(J1_BODY_X, y_top, J1_BODY_W, y_bot - y_top,
              fill="#f8f8f8", stroke=STROKE, stroke_width="2")

    # Component label
    s += text(J1_BODY_X + J1_BODY_W / 2, y_top - 8, "J1",
              font_size="14", font_weight="bold",
              text_anchor="middle", fill=STROKE, stroke="none")
    s += text(J1_BODY_X + J1_BODY_W / 2, y_top - 22, "8-Pin Header",
              font_size="10", text_anchor="middle",
              fill="#999", stroke="none")

    for pin_num, pin_name, y in J1_PINS:
        # Pin label inside body
        s += text(J1_BODY_X + 10, y + 5,
                  f"{pin_num}",
                  font_size="10", fill="#999", stroke="none")
        s += text(J1_BODY_X + 25, y + 5,
                  pin_name,
                  font_size="11", fill=STROKE, stroke="none")
        # Pin circle at body edge
        s += circle(J1_PIN_X, y, 4,
                    fill="white", stroke=STROKE, stroke_width="1.5")
        # Stub rightward (skip nc pin)
        if pin_name != "(nc)":
            s += line(J1_PIN_X + 4, y, J1_STUB, y)

    return s


def draw_u1() -> str:
    """U1: SE050C1HQ1 (QFN-20)."""
    y_top = U1_PINS[0][2] - 40
    y_bot = U1_PINS[-1][2] + 40
    s = ""

    # Body
    s += rect(U1_BODY_X, y_top, U1_BODY_W, y_bot - y_top,
              fill="#f0f0f0", stroke=STROKE, stroke_width="2")

    # Labels inside body (centered)
    cx = U1_BODY_X + U1_BODY_W / 2
    s += text(cx, y_top + 22, "U1",
              font_size="14", font_weight="bold",
              text_anchor="middle", fill=STROKE, stroke="none")
    s += text(cx, y_top + 39, "SE050C1HQ1",
              font_size="11", text_anchor="middle",
              fill="#666", stroke="none")
    s += text(cx, y_top + 54, "QFN-20",
              font_size="10", text_anchor="middle",
              fill="#999", stroke="none")

    for pin_num, pin_name, y in U1_PINS:
        # Stub leftward
        s += line(U1_BODY_X, y, U1_STUB, y)
        # Pin number near stub end
        s += text(U1_STUB + 3, y + 4,
                  str(pin_num),
                  font_size="10", fill="#999", stroke="none")
        # Pin name inside body
        s += text(U1_BODY_X + 30, y + 5,
                  pin_name,
                  font_size="11", fill=STROKE, stroke="none")

    return s


# ---------------------------------------------------------------------------
# Wire routing
# ---------------------------------------------------------------------------


def draw_wires() -> str:
    s = ""

    # --- VCC rail ---
    s += line(VCC_X1, VCC_Y, VCC_X2, VCC_Y)
    s += vcc_symbol((VCC_X1 + VCC_X2) / 2, VCC_Y)

    # J1.1 VCC → rail
    s += line(J1_STUB, 110, VCC_X1, 110)
    s += line(VCC_X1, 110, VCC_X1, VCC_Y)
    s += junction(VCC_X1, VCC_Y)

    # U1.18 VCC → rail
    s += line(U1_STUB, 130, VCC_X2, 130)
    s += line(VCC_X2, 130, VCC_X2, VCC_Y)
    s += junction(VCC_X2, VCC_Y)

    # Component tops connect to VCC rail (junctions)
    for cx in (C1_X, C2_X, R1_X, R2_X):
        s += junction(cx, VCC_Y)

    # --- GND connections (each gets its own symbol) ---
    # J1.2
    s += line(J1_STUB, 175, VCC_X1, 175)
    s += gnd_symbol(VCC_X1, 175)
    # C1
    s += gnd_symbol(C1_X, CAP_BOT)
    # C2
    s += gnd_symbol(C2_X, CAP_BOT)
    # U1.19
    s += line(U1_STUB, 195, VCC_X2, 195)
    s += gnd_symbol(VCC_X2, 195)

    # --- SDA net: J1.3 → R1 junction → U1.9 ---
    s += line(J1_STUB, 260, U1_STUB, 260)
    s += junction(R1_X, 260)

    # --- SCL net: J1.4 → R2 junction → U1.10 ---
    s += line(J1_STUB, 330, U1_STUB, 330)
    s += junction(R2_X, 330)

    # --- ENA: J1.5 → U1.11 ---
    s += line(J1_STUB, 400, U1_STUB, 400)

    # --- RST_N: J1.6 → U1.14 ---
    s += line(J1_STUB, 460, U1_STUB, 460)

    # --- VIN: J1.7 → U1.12 ---
    s += line(J1_STUB, 520, U1_STUB, 520)

    return s


# ---------------------------------------------------------------------------
# Assemble SVG
# ---------------------------------------------------------------------------


def generate_svg() -> str:
    svg = f"""\
<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg"
     viewBox="0 0 {W} {H}" width="{W}" height="{H}"
     font-family="{FONT}"
     stroke="{STROKE}" stroke-width="{STROKE_W}"
     fill="none" stroke-linecap="round" stroke-linejoin="round">
  <rect width="{W}" height="{H}" fill="white" stroke="none"/>
"""

    # Wires (behind everything)
    svg += "\n  <!-- Wires -->\n"
    svg += draw_wires()

    # Components
    svg += "\n  <!-- J1 Header -->\n"
    svg += draw_j1()

    svg += "\n  <!-- U1 SE050 -->\n"
    svg += draw_u1()

    # Passives
    svg += "\n  <!-- C1 -->\n"
    svg += capacitor_v(C1_X, VCC_Y, CAP_BOT, "C1", "100nF")

    svg += "\n  <!-- C2 -->\n"
    svg += capacitor_v(C2_X, VCC_Y, CAP_BOT, "C2", u"10\u00b5F")

    svg += "\n  <!-- R1 (SDA pull-up) -->\n"
    svg += resistor_v(R1_X, VCC_Y, R1_BOT, "R1", u"4.7k\u2126")

    svg += "\n  <!-- R2 (SCL pull-up) -->\n"
    svg += resistor_v(R2_X, VCC_Y, R2_BOT, "R2", u"4.7k\u2126")

    # Title & datasheet reference
    svg += "\n  <!-- Title -->\n"
    svg += text(W / 2, H - 28,
                u"SE050 Breakout Board \u2014 Circuit Schematic",
                font_size="13", text_anchor="middle",
                fill="#999", stroke="none")
    DS_URL = "https://www.nxp.com/docs/en/data-sheet/SE050-DATASHEET.pdf"
    svg += (
        f'  <a href="{DS_URL}" target="_blank">'
        f'<text x="{W / 2}" y="{H - 10}" font-size="10" '
        f'text-anchor="middle" fill="#07c" stroke="none">'
        f"Datasheet: NXP SE050 (Rev\u00a03.8)</text></a>\n"
    )

    svg += "</svg>\n"
    return svg


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> None:
    svg = generate_svg()

    out_dir = os.path.join(os.path.dirname(__file__), "..", "..", "docs", "assets")
    os.makedirs(out_dir, exist_ok=True)
    out_path = os.path.join(out_dir, "se050-breakout-schematic.svg")

    with open(out_path, "w", encoding="utf-8") as f:
        f.write(svg)

    print(f"wrote {out_path}  ({len(svg)} bytes)")


if __name__ == "__main__":
    main()
