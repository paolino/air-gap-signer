# Electronics Design Assistant

You are helping with PCB design for the air-gap-signer project. Use this reference when working on hardware files in `hardware/`.

## SE050 Secure Element Reference

### IC: SE050C1HQ1/Z01SCZ (NXP EdgeLock SE050)
- Package: HX2QFN20 (3x3mm body, 20-pin QFN)
- Datasheet: NXP SE050 (document SE050)
- Reference designs: mimok/se050-breakout, NXP OM-SE050ARD

### QFN20 Pin Numbering Convention
Standard QFN numbering (counter-clockwise from pin 1 dot, bottom-left):
- **Bottom** (L→R): pins 1-5
- **Left** (B→T): pins 6-10
- **Top** (R→L): pins 11-15
- **Right** (T→B): pins 16-20

### SE050 Pin Assignments (verified against datasheet)
| Pin | Name   | Function                |
|-----|--------|-------------------------|
| 9   | SDA    | I2C data (left side)    |
| 10  | SCL    | I2C clock (left side)   |
| 11  | ENA    | Enable (top side)       |
| 12  | VIN    | Voltage input (top side)|
| 14  | RST_N  | Reset, active low (top) |
| 18  | VCC    | Power supply (right)    |
| 19  | GND    | Ground (right side)     |
| 21  | EP     | Exposed pad (GND)       |

### Breakout Board J1 Header Pinout
| J1 Pin | Signal | Notes              |
|--------|--------|--------------------|
| 1      | VCC    | 1.8V or 3.3V      |
| 2      | GND    |                    |
| 3      | SDA    | 4.7k pull-up to VCC|
| 4      | SCL    | 4.7k pull-up to VCC|
| 5      | ENA    | Active high        |
| 6      | RST_N  | Active low         |
| 7      | VIN    | Voltage input      |
| 8      | (unused)|                   |

## PCB Design Patterns

### Gerber Generation
- Files are in `hardware/SE050_breakout/`
- Generator: `generate_gerbers.py` (pure Python, no dependencies)
- Build command: `just gerbers`
- Coordinate format: FSLAX36Y36 (1 unit = 1e-6 mm)
- Visual verification: upload to https://tracespace.io/view/

### QFN Pad Layout Calculations
For a QFN with N pads per side, pitch P, center offset D:
```
Bottom: x = cx + (i - N//2) * P,  y = cy - D
Left:   x = cx - D,               y = cy + (i - N//2) * P
Top:    x = cx - (i - N//2) * P,  y = cy + D
Right:  x = cx + D,               y = cy - (i - N//2) * P
```

### Assembly File Conventions
- BOM: `*_BOM.csv` — Designator, Quantity, Value, Package, Description, Manufacturer, MPN
- CPL: `*_CPL.csv` — Designator, Mid X (mm), Mid Y (mm), Layer, Rotation
- Gerber zip: `*_gerbers.zip` containing .GTL, .GBL, .GTS, .GBS, .GTO, .GKO, .DRL

## Common Pitfalls
- Always cross-reference IC pin assignments against the datasheet, not just schematic symbols
- QFN pin 1 is typically bottom-left with counter-clockwise numbering
- Exposed pad (EP) is usually GND — connect with thermal vias to ground pour
- I2C lines (SDA/SCL) need pull-up resistors (typically 4.7k for 100kHz/400kHz)
