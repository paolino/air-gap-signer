# SE050C1HQ1 Breakout Board Gerber Generator

## Context
Generate a self-contained Python script that outputs complete RS-274X Gerber files and an Excellon drill file for an SE050C1HQ1 breakout board. Run via `just gerbers`. No external libraries.

## Output files
1. `SE050_breakout.GTL` — Top copper (QFN pads, 0402 pads, TH pads, traces)
2. `SE050_breakout.GBL` — Bottom copper (ground pour)
3. `SE050_breakout.GTS` — Top soldermask (pad openings)
4. `SE050_breakout.GBS` — Bottom soldermask
5. `SE050_breakout.GTO` — Top silkscreen (outlines, labels, pin 1 dot)
6. `SE050_breakout.GKO` — Board outline (20x20mm)
7. `SE050_breakout.DRL` — Excellon drill file

## Component placement (all coords in mm, origin bottom-left)

| Component | Position (center) | Notes |
|-----------|-------------------|-------|
| SE050 (U1) | (10.0, 10.0) | HX2QFN20, 3x3mm body |
| C1 100nF 0402 | (7.0, 10.4) | Decoupling, near VCC pin |
| C2 10uF 0402 | (7.0, 9.0) | Bulk cap |
| R1 4.7k 0402 | (9.2, 7.5) | SDA pull-up |
| R2 4.7k 0402 | (10.6, 7.5) | SCL pull-up |
| J1 pin 1 | (1.11, 3.0) | 8-pin header, 2.54mm pitch: VCC, GND, SDA, SCL, ENA, RST_N, VIN, (unused) |
| J1 pin 8 | (18.89, 3.0) | Last header pin |
| M1-M4 | (2,2), (18,2), (2,18), (18,18) | 2.2mm NPTH mounting holes |

## QFN20 pad layout (relative to IC center)
- 5 pads per side, 0.4mm pitch
- Peripheral pad: 0.24mm x 0.70mm
- Pad center 1.40mm from IC center
- EP (exposed pad): 1.70mm x 1.70mm
- Pin numbering: bottom L→R (1-5), left B→T (6-10), top R→L (11-15), right T→B (16-20)

## Routing strategy
- Top copper: all signal traces (0.2mm width), component pads
- Bottom copper: ground pour (flood fill approximated as large rectangle)
- Vias for GND connections from top to bottom (0.6mm pad, 0.3mm drill)
- Trace routing: Manhattan style (horizontal + vertical segments)

## Net connections
- **VCC**: J1.1 → C2.1 → C1.1 → U1.pin18 (+ R1, R2 pull-up ends)
- **GND**: J1.2 → C2.2 → C1.2 → U1.pin19 → U1.EP (via to bottom GND pour)
- **SDA**: J1.3 → R1.1 → U1.pin9; R1.2 → VCC
- **SCL**: J1.4 → R2.1 → U1.pin10; R2.2 → VCC
- **ENA**: J1.5 → U1.pin11
- **RST_N**: J1.6 → U1.pin14
- **VIN**: J1.7 → U1.pin12

## Assembly files

The script must also generate two CSV files for PCB assembly:

### BOM (`SE050_breakout_BOM.csv`)
Columns: `Designator,Quantity,Value,Package,Description,Manufacturer,Manufacturer Part Number`

| Designator | Qty | Value | Package | Description | Manufacturer | MPN |
|---|---|---|---|---|---|---|
| U1 | 1 | SE050C1HQ1 | HX2QFN20 | Secure Element, EdgeLock SE050 | NXP | SE050C1HQ1/Z01SCZ |
| C1 | 1 | 100nF | 0402 | MLCC Capacitor 100nF 16V X5R | | |
| C2 | 1 | 10uF | 0402 | MLCC Capacitor 10uF 6.3V X5R | | |
| R1 | 1 | 4.7k | 0402 | Chip Resistor 4.7kOhm 1% | | |
| R2 | 1 | 4.7k | 0402 | Chip Resistor 4.7kOhm 1% | | |
| J1 | 1 | 8-pin header | 2.54mm pitch | Pin Header 1x8 2.54mm Through-Hole | | |

### Centroid / Pick-and-Place (`SE050_breakout_CPL.csv`)
Columns: `Designator,Mid X (mm),Mid Y (mm),Layer,Rotation`

Positions taken from the component placement table above. All components on Top layer, 0° rotation.

## Script structure
Single file `generate_gerbers.py`:
1. Helper: `coord(mm)` → integer string (×1,000,000 for FSLAX36Y36)
2. Helper: `gerber_header(file_attr)` → RS-274X header string
3. Define aperture table (circles, rectangles, obrounds for each pad type)
4. Define all component pad positions (absolute mm)
5. Define trace routes as polyline segments
6. Generate each layer by writing pads (D03 flash) + traces (D01/D02)
7. Write 7 output files + 2 assembly CSVs (BOM and CPL)

## Verification
- Run: `just gerbers`
- Check each file starts with `%FSLAX` and ends with `M02*`
- Check drill file has Excellon header with `M48` and tool definitions
- Visual: upload to https://tracespace.io/view/
