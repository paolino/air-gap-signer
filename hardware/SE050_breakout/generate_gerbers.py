#!/usr/bin/env python3
"""
Generate RS-274X Gerber files and Excellon drill file for an SE050C1HQ1
breakout board (20x20 mm, QFN-20 + passives + 8-pin header).

Run:  nix run nixpkgs#python3 -- generate_gerbers.py

No external libraries required.
"""

from __future__ import annotations

import os

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

BOARD_W = 20.0  # mm
BOARD_H = 20.0  # mm

# Gerber coordinate format: FSLAX36Y36 -> 1 unit = 1e-6 mm
SCALE = 1_000_000


def coord(mm: float) -> str:
    """Convert mm to Gerber integer string (FSLAX36Y36)."""
    return str(round(mm * SCALE))


# ---------------------------------------------------------------------------
# Aperture definitions
# ---------------------------------------------------------------------------

# Pad geometries (mm)
QFN_PAD_W = 0.24
QFN_PAD_H = 0.70
QFN_EP_W = 1.70
QFN_EP_H = 1.70
P0402_W = 0.50
P0402_H = 0.60
TH_PAD_D = 1.60
VIA_PAD_D = 0.60
TRACE_W = 0.20
OUTLINE_W = 0.05
SILK_W = 0.15
MOUNTING_RING_D = 3.2
PIN1_DOT_D = 0.30

SMASK_EXPAND = 0.05

# D-code assignments
AP_QFN_VERT = "D10"
AP_QFN_HORIZ = "D11"
AP_QFN_EP = "D12"
AP_0402 = "D13"
AP_TH = "D14"
AP_VIA = "D15"
AP_TRACE = "D16"
AP_OUTLINE = "D17"
AP_SILK = "D18"
AP_PIN1 = "D19"
AP_MOUNT = "D20"
AP_SMASK_QFN_V = "D21"
AP_SMASK_QFN_H = "D22"
AP_SMASK_EP = "D23"
AP_SMASK_0402 = "D24"
AP_SMASK_TH = "D25"
AP_SMASK_VIA = "D26"
AP_GND_POUR = "D27"

APERTURE_DEFS: dict[str, str] = {
    AP_QFN_VERT: f"%ADD10R,{QFN_PAD_W:.4f}X{QFN_PAD_H:.4f}*%",
    AP_QFN_HORIZ: f"%ADD11R,{QFN_PAD_H:.4f}X{QFN_PAD_W:.4f}*%",
    AP_QFN_EP: f"%ADD12R,{QFN_EP_W:.4f}X{QFN_EP_H:.4f}*%",
    AP_0402: f"%ADD13R,{P0402_W:.4f}X{P0402_H:.4f}*%",
    AP_TH: f"%ADD14C,{TH_PAD_D:.4f}*%",
    AP_VIA: f"%ADD15C,{VIA_PAD_D:.4f}*%",
    AP_TRACE: f"%ADD16C,{TRACE_W:.4f}*%",
    AP_OUTLINE: f"%ADD17C,{OUTLINE_W:.4f}*%",
    AP_SILK: f"%ADD18C,{SILK_W:.4f}*%",
    AP_PIN1: f"%ADD19C,{PIN1_DOT_D:.4f}*%",
    AP_MOUNT: f"%ADD20C,{MOUNTING_RING_D:.4f}*%",
    AP_SMASK_QFN_V: (
        f"%ADD21R,{QFN_PAD_W + 2 * SMASK_EXPAND:.4f}"
        f"X{QFN_PAD_H + 2 * SMASK_EXPAND:.4f}*%"
    ),
    AP_SMASK_QFN_H: (
        f"%ADD22R,{QFN_PAD_H + 2 * SMASK_EXPAND:.4f}"
        f"X{QFN_PAD_W + 2 * SMASK_EXPAND:.4f}*%"
    ),
    AP_SMASK_EP: (
        f"%ADD23R,{QFN_EP_W + 2 * SMASK_EXPAND:.4f}"
        f"X{QFN_EP_H + 2 * SMASK_EXPAND:.4f}*%"
    ),
    AP_SMASK_0402: (
        f"%ADD24R,{P0402_W + 2 * SMASK_EXPAND:.4f}"
        f"X{P0402_H + 2 * SMASK_EXPAND:.4f}*%"
    ),
    AP_SMASK_TH: f"%ADD25C,{TH_PAD_D + 2 * SMASK_EXPAND:.4f}*%",
    AP_SMASK_VIA: f"%ADD26C,{VIA_PAD_D + 2 * SMASK_EXPAND:.4f}*%",
    AP_GND_POUR: f"%ADD27R,{BOARD_W - 1.0:.4f}X{BOARD_H - 1.0:.4f}*%",
}


# ---------------------------------------------------------------------------
# Gerber file helpers
# ---------------------------------------------------------------------------


def gerber_header(file_attr: str) -> str:
    lines = [
        "%FSLAX36Y36*%",
        "%MOMM*%",
        f"%TF.FileFunction,{file_attr}*%",
        "%TF.FilePolarity,Positive*%",
    ]
    return "\n".join(lines) + "\n"


def gerber_apertures(codes: list[str]) -> str:
    return "\n".join(APERTURE_DEFS[c] for c in codes if c in APERTURE_DEFS) + "\n"


def flash(aperture: str, x: float, y: float) -> str:
    return f"{aperture}*\nX{coord(x)}Y{coord(y)}D03*\n"


def move_to(x: float, y: float) -> str:
    return f"X{coord(x)}Y{coord(y)}D02*\n"


def draw_to(x: float, y: float) -> str:
    return f"X{coord(x)}Y{coord(y)}D01*\n"


def select_aperture(ap: str) -> str:
    return f"{ap}*\n"


def gerber_footer() -> str:
    return "M02*\n"


# ---------------------------------------------------------------------------
# Component pad positions (absolute, mm)
# ---------------------------------------------------------------------------

IC_X, IC_Y = 10.0, 10.0

PITCH = 0.4
PAD_OFFSET = 1.40


def qfn_pads() -> list[tuple[float, float, str, str, int]]:
    """Return [(x, y, copper_aperture, mask_aperture, pin_number)]."""
    pads: list[tuple[float, float, str, str, int]] = []
    pin = 1
    # Bottom side: L->R
    for i in range(5):
        x = IC_X + (i - 2) * PITCH
        y = IC_Y - PAD_OFFSET
        pads.append((x, y, AP_QFN_VERT, AP_SMASK_QFN_V, pin))
        pin += 1
    # Left side: B->T
    for i in range(5):
        x = IC_X - PAD_OFFSET
        y = IC_Y + (i - 2) * PITCH
        pads.append((x, y, AP_QFN_HORIZ, AP_SMASK_QFN_H, pin))
        pin += 1
    # Top side: R->L
    for i in range(5):
        x = IC_X - (i - 2) * PITCH
        y = IC_Y + PAD_OFFSET
        pads.append((x, y, AP_QFN_VERT, AP_SMASK_QFN_V, pin))
        pin += 1
    # Right side: T->B
    for i in range(5):
        x = IC_X + PAD_OFFSET
        y = IC_Y - (i - 2) * PITCH
        pads.append((x, y, AP_QFN_HORIZ, AP_SMASK_QFN_H, pin))
        pin += 1
    return pads


QFN = qfn_pads()
QFN_BY_PIN: dict[int, tuple[float, float]] = {
    pin: (x, y) for x, y, _a, _m, pin in QFN
}
EP_POS = (IC_X, IC_Y)

# 0402 passives: two pads separated by 0.70 mm center-to-center
P0402_SPAN = 0.70


def p0402_pads(
    cx: float, cy: float, horizontal: bool = True
) -> tuple[tuple[float, float], tuple[float, float]]:
    half = P0402_SPAN / 2
    if horizontal:
        return ((cx - half, cy), (cx + half, cy))
    return ((cx, cy - half), (cx, cy + half))


C1_CENTER = (7.0, 10.4)
C2_CENTER = (7.0, 9.0)
R1_CENTER = (9.2, 7.5)
R2_CENTER = (10.6, 7.5)

C1_P1, C1_P2 = p0402_pads(*C1_CENTER)
C2_P1, C2_P2 = p0402_pads(*C2_CENTER)
R1_P1, R1_P2 = p0402_pads(*R1_CENTER)
R2_P1, R2_P2 = p0402_pads(*R2_CENTER)

# 8-pin header J1
J1_X_START = 1.11
J1_Y = 3.0
J1_PITCH = 2.54
J1_PINS: list[tuple[float, float]] = [
    (J1_X_START + i * J1_PITCH, J1_Y) for i in range(8)
]

# Mounting holes (NPTH)
MOUNT_HOLES = [(2.0, 2.0), (18.0, 2.0), (2.0, 18.0), (18.0, 18.0)]
MOUNT_DRILL = 2.2

# GND vias near EP
GND_VIAS = [
    (IC_X - 0.5, IC_Y - 0.5),
    (IC_X + 0.5, IC_Y - 0.5),
    (IC_X - 0.5, IC_Y + 0.5),
    (IC_X + 0.5, IC_Y + 0.5),
]
VIA_DRILL = 0.3
TH_DRILL = 1.0


# ---------------------------------------------------------------------------
# Trace routing (Manhattan style)
# ---------------------------------------------------------------------------


def manhattan(
    x0: float, y0: float, x1: float, y1: float
) -> list[tuple[float, float]]:
    return [(x0, y0), (x1, y0), (x1, y1)]


def manhattan_vert_first(
    x0: float, y0: float, x1: float, y1: float
) -> list[tuple[float, float]]:
    return [(x0, y0), (x0, y1), (x1, y1)]


TRACES: list[list[tuple[float, float]]] = []

# VCC net
TRACES.append(manhattan_vert_first(*J1_PINS[0], *C2_P1))
TRACES.append(manhattan_vert_first(*C2_P1, *C1_P1))
TRACES.append(manhattan(*C1_P1, *QFN_BY_PIN[18]))
TRACES.append(manhattan(*R1_P2, *C1_P1))
TRACES.append(manhattan(*R2_P2, *R1_P2))

# GND net
TRACES.append(manhattan_vert_first(*J1_PINS[1], *C2_P2))
TRACES.append(manhattan_vert_first(*C2_P2, *C1_P2))
TRACES.append(manhattan(*C1_P2, *QFN_BY_PIN[19]))
TRACES.append([(QFN_BY_PIN[19][0], QFN_BY_PIN[19][1]),
               (IC_X, QFN_BY_PIN[19][1]), (IC_X, IC_Y)])

# SDA net
TRACES.append(manhattan_vert_first(*J1_PINS[2], *R1_P1))
TRACES.append(manhattan(*R1_P1, *QFN_BY_PIN[9]))

# SCL net
TRACES.append(manhattan_vert_first(*J1_PINS[3], *R2_P1))
TRACES.append(manhattan(*R2_P1, *QFN_BY_PIN[10]))

# ENA net
TRACES.append(manhattan_vert_first(*J1_PINS[4], *QFN_BY_PIN[11]))

# RST_N net
TRACES.append(manhattan_vert_first(*J1_PINS[5], *QFN_BY_PIN[14]))

# VIN net
TRACES.append(manhattan_vert_first(*J1_PINS[6], *QFN_BY_PIN[12]))


# ---------------------------------------------------------------------------
# Layer generators
# ---------------------------------------------------------------------------


def gen_top_copper() -> str:
    out = gerber_header("Copper,L1,Top")
    out += gerber_apertures([
        AP_QFN_VERT, AP_QFN_HORIZ, AP_QFN_EP, AP_0402,
        AP_TH, AP_VIA, AP_TRACE,
    ])
    for x, y, ap, _m, _pin in QFN:
        out += flash(ap, x, y)
    out += flash(AP_QFN_EP, *EP_POS)
    for p1, p2 in [(C1_P1, C1_P2), (C2_P1, C2_P2),
                    (R1_P1, R1_P2), (R2_P1, R2_P2)]:
        out += flash(AP_0402, *p1)
        out += flash(AP_0402, *p2)
    for px, py in J1_PINS:
        out += flash(AP_TH, px, py)
    for vx, vy in GND_VIAS:
        out += flash(AP_VIA, vx, vy)
    out += select_aperture(AP_TRACE)
    for route in TRACES:
        if len(route) < 2:
            continue
        out += move_to(*route[0])
        for pt in route[1:]:
            out += draw_to(*pt)
    out += gerber_footer()
    return out


def gen_bottom_copper() -> str:
    out = gerber_header("Copper,L2,Bot")
    out += gerber_apertures([AP_GND_POUR, AP_VIA])
    out += flash(AP_GND_POUR, BOARD_W / 2, BOARD_H / 2)
    for vx, vy in GND_VIAS:
        out += flash(AP_VIA, vx, vy)
    out += gerber_footer()
    return out


def gen_top_soldermask() -> str:
    out = gerber_header("Soldermask,Top")
    out += gerber_apertures([
        AP_SMASK_QFN_V, AP_SMASK_QFN_H, AP_SMASK_EP,
        AP_SMASK_0402, AP_SMASK_TH, AP_SMASK_VIA,
    ])
    for x, y, _a, mask_ap, _pin in QFN:
        out += flash(mask_ap, x, y)
    out += flash(AP_SMASK_EP, *EP_POS)
    for p1, p2 in [(C1_P1, C1_P2), (C2_P1, C2_P2),
                    (R1_P1, R1_P2), (R2_P1, R2_P2)]:
        out += flash(AP_SMASK_0402, *p1)
        out += flash(AP_SMASK_0402, *p2)
    for px, py in J1_PINS:
        out += flash(AP_SMASK_TH, px, py)
    for vx, vy in GND_VIAS:
        out += flash(AP_SMASK_VIA, vx, vy)
    out += gerber_footer()
    return out


def gen_bottom_soldermask() -> str:
    out = gerber_header("Soldermask,Bot")
    out += gerber_apertures([AP_SMASK_VIA, AP_SMASK_TH])
    for vx, vy in GND_VIAS:
        out += flash(AP_SMASK_VIA, vx, vy)
    for px, py in J1_PINS:
        out += flash(AP_SMASK_TH, px, py)
    out += gerber_footer()
    return out


def gen_top_silkscreen() -> str:
    out = gerber_header("Legend,Top")
    out += gerber_apertures([AP_SILK, AP_PIN1])
    # IC body outline (3x3 mm)
    bx0, by0 = IC_X - 1.5, IC_Y - 1.5
    bx1, by1 = IC_X + 1.5, IC_Y + 1.5
    out += select_aperture(AP_SILK)
    out += move_to(bx0, by0)
    out += draw_to(bx1, by0)
    out += draw_to(bx1, by1)
    out += draw_to(bx0, by1)
    out += draw_to(bx0, by0)
    # Pin-1 dot
    out += flash(AP_PIN1, bx0 - 0.4, by0 - 0.4)
    # 0402 outlines
    for cx, cy in [C1_CENTER, C2_CENTER, R1_CENTER, R2_CENTER]:
        out += select_aperture(AP_SILK)
        out += move_to(cx - 0.55, cy - 0.35)
        out += draw_to(cx + 0.55, cy - 0.35)
        out += draw_to(cx + 0.55, cy + 0.35)
        out += draw_to(cx - 0.55, cy + 0.35)
        out += draw_to(cx - 0.55, cy - 0.35)
    # Header outline
    hx0 = J1_PINS[0][0] - 1.5
    hx1 = J1_PINS[7][0] + 1.5
    hy0 = J1_Y - 1.5
    hy1 = J1_Y + 1.5
    out += select_aperture(AP_SILK)
    out += move_to(hx0, hy0)
    out += draw_to(hx1, hy0)
    out += draw_to(hx1, hy1)
    out += draw_to(hx0, hy1)
    out += draw_to(hx0, hy0)
    # U1 label cross
    out += select_aperture(AP_SILK)
    out += move_to(IC_X - 0.3, IC_Y + 2.0)
    out += draw_to(IC_X + 0.3, IC_Y + 2.0)
    out += move_to(IC_X, IC_Y + 1.7)
    out += draw_to(IC_X, IC_Y + 2.3)
    out += gerber_footer()
    return out


def gen_board_outline() -> str:
    out = gerber_header("Profile,NP")
    out += gerber_apertures([AP_OUTLINE])
    out += select_aperture(AP_OUTLINE)
    out += move_to(0.0, 0.0)
    out += draw_to(BOARD_W, 0.0)
    out += draw_to(BOARD_W, BOARD_H)
    out += draw_to(0.0, BOARD_H)
    out += draw_to(0.0, 0.0)
    out += gerber_footer()
    return out


# ---------------------------------------------------------------------------
# Excellon drill file
# ---------------------------------------------------------------------------


def gen_drill() -> str:
    lines: list[str] = [
        "M48",
        ";DRILL file",
        ";FORMAT={-:-/ absolute / metric / decimal}",
        "FMAT,2",
        "METRIC,TZ",
        f"T1C{VIA_DRILL:.3f}",
        f"T2C{TH_DRILL:.3f}",
        f"T3C{MOUNT_DRILL:.3f}",
        "%",
    ]
    lines.append("T1")
    for vx, vy in GND_VIAS:
        lines.append(f"X{vx:.4f}Y{vy:.4f}")
    lines.append("T2")
    for px, py in J1_PINS:
        lines.append(f"X{px:.4f}Y{py:.4f}")
    lines.append("T3")
    for mx, my in MOUNT_HOLES:
        lines.append(f"X{mx:.4f}Y{my:.4f}")
    lines.append("M30")
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

FILES = {
    "SE050_breakout.GTL": gen_top_copper,
    "SE050_breakout.GBL": gen_bottom_copper,
    "SE050_breakout.GTS": gen_top_soldermask,
    "SE050_breakout.GBS": gen_bottom_soldermask,
    "SE050_breakout.GTO": gen_top_silkscreen,
    "SE050_breakout.GKO": gen_board_outline,
    "SE050_breakout.DRL": gen_drill,
}


def main() -> None:
    for fname, generator in FILES.items():
        content = generator()
        with open(fname, "w") as f:
            f.write(content)
        print(f"wrote {fname}  ({len(content)} bytes)")

    print("\n--- sanity checks ---")
    ok = True
    for fname in FILES:
        with open(fname) as f:
            data = f.read()
        if fname.endswith(".DRL"):
            if not data.startswith("M48"):
                print(f"FAIL: {fname} does not start with M48")
                ok = False
            if "M30" not in data:
                print(f"FAIL: {fname} missing M30")
                ok = False
        else:
            if not data.startswith("%FSLAX"):
                print(f"FAIL: {fname} does not start with %FSLAX")
                ok = False
            if not data.rstrip().endswith("M02*"):
                print(f"FAIL: {fname} does not end with M02*")
                ok = False
    if ok:
        print("All checks passed.")


if __name__ == "__main__":
    main()
