# Air Gap Signer front board

The [secure project definition](secure-project.md) specifies the production trust boundary and release tests. The direct Pi wiring below belongs to the bench prototype.

This is the first schematic and fit draft for a Pi 3 Model B plug-on board. One PCB carries a 3.5-inch landscape display, physical controls and a soldered SE051C2. The Pi supplies power and hosts the transaction USB stick. The enclosure carries a camera for reading the phone’s QR codes. On the Pi 3B rig it plugs into the existing CSI camera socket through an internal ribbon, leaving the 40-pin board connection available for the front board.

[KiCad schematic](front-board.kicad_sch) · [1:1 fit drawing](fit.svg)

## Electrical design

The display candidate is Surenoo **STP0350A3-320480 NTP**, a 480 × 320 IPS panel in landscape with a 40-position, 0.5 mm flex. J2 follows its documented four-wire SPI pinout. The MSP3525 is the bench module for testing the screen size and ST7796-family driver. Confirm the bare panel's supplied revision, contact side, supply requirements and initialization with a sample before choosing J2's footprint. [Surenoo panel document](https://manuals.plus/surenoo/stp0350a3-320480-series-tft-lcd-panel-manual.pdf) · [MSP3525 bench module](https://lcdwiki.com/index.php?title=3.5inch_IPS_SPI_Module_ST7796)

| Function | Pi BCM GPIO |
|---|---|
| Display SPI MOSI, clock and CE0 | 10, 11, 8 |
| Display data/command, backlight PWM, reset | 25, 26, 24 |
| Navigation Up, Down, Back, Open | 17, 22, 27, 23 |
| Confirm, Mode | 5, 6 |
| SE051 I²C SDA, SCL | 2, 3 |

GPIO24 is the proposed display reset. The six active-low controls match the software at the checkout's base revision, where GPIO6 selects Mode. The navigation cluster can use discrete tactile switches or a four-way cap over them. Keep Confirm identifiable by touch. [Base button mapping](https://github.com/paolino/air-gap-signer/blob/4bd577d2685f8875e9c7e873d5bd20d3feed3cad/crates/signer-pi/src/buttons.rs#L15)

U1 uses NXP **SE051C2HQ1/Z01XD** in **SOT1969-1**. VIN, VCC, ENA and the unused ISO 7816 reset are held at 3.3 V. LA, LB, VSS and the thermal pad go to ground per the datasheet. The Pi supplies the I²C pull-ups for this draft. Probe pads expose 3.3 V, ground, SDA and SCL for bring-up. Check the footprint against the manufacturer's exact land pattern and test I²C and the absence of a contactless response at power-up on the assembled board. [NXP datasheet](https://www.nxp.com/docs/en/data-sheet/SE051.pdf) · [Package drawing](https://www.nxp.com/docs/en/package-information/SOT1969-1.pdf)

The panel drawing gives approximately **120 mA total** for the backlight. The schematic leaves the LED anode, three cathodes and GPIO26 PWM open pending selection of a current-limited driver powered from the Pi's 5 V rail. Verify the LED grouping and current on the actual panel. Complete that circuit and review backlight load, footprints and clearances before routing.

## Mechanical fit

The first assembly uses a **custom enclosure** with four nominal **8 mm feet** under the base. The **105 × 62 mm** front board reserves **84.52 × 55.26 mm** for the panel and **14 mm** for the controls. The Pi sits toward the USB/Ethernet and power edges so its connector faces reach the case openings. The proposed base cover gives service access to the underside microSD slot. [1:1 fit drawing](fit.svg) · [Pi 3B mechanical drawing](https://pip-assets.raspberrypi.com/categories/520-raspberry-pi-3-model-b/documents/RP-008335-DS-1-raspberry-pi-3-b-mechanical-drawing.pdf?disposition=inline)

**The boot microSD stays installed during ordinary use.** The owner can open the base after power-off to replace or reimage it. A spare carrying the same approved image is acceptable. The security design must let the user recognize the enrolled signer before entering a PIN and ensure that only approved signer software controls review and confirmation. These protections remain open engineering work on the Pi 3B prototype.

Mount the Pi to the base on its four standoffs and support the front board independently on enclosure rails or bosses. Fasten it from the back or the edges, clear of each plug path. The 40-pin connection mates directly through a socket stack with full pin engagement. Start with **20 mm from the top of the Pi PCB to the underside of the front PCB**, leaving **4 mm nominal** over the Pi's 16 mm USB stacks before subtracting any underside parts, solder or tolerance. Verify the socket height and keep protruding parts and fasteners out of that overhead clearance. Use removable front-board fixings so the Pi's internal CSI and DSI ribbon latches remain serviceable.

With the front board installed, insert and remove the actual transaction stick and plugs at all four USB-A ports, Ethernet, micro-USB power, HDMI and A/V. Include the plug shells, the Ethernet release tab, finger grip and cable bend room in the fit check. Power off, open the base and check that the owner can replace the card and refit the cover without disturbing the display flex. Check the screen flex fold and connector latch before fixing mounting holes. The drawing sets targets for this fit check. Final cutouts, socket stack and feet depend on the physical assembly.

## Camera and phone approval

Use the standard autofocus Raspberry Pi Camera Module 3 as the bench candidate. Its nominal envelope is 25 × 24 × 11.5 mm. Connect it to the Pi 3B camera socket with a standard 15-pin ribbon. Mount the lens facing out of the rear wall, opposite the power and HDMI edge, so the phone can present a code while the signer rests on its feet. Reserve space there for the module, connector and ribbon bend. The existing 105 × 62 mm front PCB dimensions refer to the display and controls. Size the rear camera pocket on a mock-up and confirm the field of view through the finished opening, USB/Ethernet plug clearance and access to the SD service cover. [Camera module specifications](https://www.raspberrypi.com/products/camera-module-3/) · [Raspberry Pi mechanical drawings](https://pip.raspberrypi.com/categories/1207-design-files)

On the bench, display a harmless QR code on the actual Android phone and read it through the camera with the intended case opening in place. Then scan the longest recognition and approval messages in both directions on the 240 × 240 prototype and the proposed 480 × 320 display. Check glare, handheld focus, scan distance and cancellation. If evidence spans several QR frames, bound their count and total size and verify the completely assembled message. [Security and acceptance criteria](secure-project.md)

## Bench checks before layout

Use the MSP3525 to render a transaction with full addresses, several outputs and native assets. Parameterize the signer's current 240 × 240 renderer, check panel colour and orientation, and measure when the updated review is visible relative to when Confirm is accepted. Check all six controls, then identify the secure element and verify a signature made with a disposable key. The KiCad schematic is an electrical draft. It has no routed PCB or fabrication files. [Renderer at the base revision](https://github.com/paolino/air-gap-signer/blob/4bd577d2685f8875e9c7e873d5bd20d3feed3cad/crates/signer-pi/src/display.rs#L12)
