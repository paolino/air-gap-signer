# Air Gap Signer front board

The [secure project definition](secure-project.md) specifies the production trust boundary and release tests. The direct Pi wiring below belongs to the bench prototype.

The product is a bolt-on signer panel. Both versions carry the display, controls, trusted controller and SE051C2 on a protected rear carrier. The base version receives phone QR messages through the host webcam and USB. The camera version can read the same messages directly while the phone is held over the panel. Both display outgoing codes on the panel and apply the same approval checks. A laptop connects directly with one USB data cable to power the panel, submit requests and receive results. Direct USB signing is a release requirement. A lower box is an optional host. An enclosure builder can mount the panel to a flat plate, a custom box or a bracket. The Pi 3B assembly is one bench host.

[Panel face](fit.svg) · [1:1 mounting template](mount.svg) · [Bench KiCad schematic](front-board.kicad_sch)

[Bill of materials for one STM32 bench prototype](stm32-prototype-bom.md)

## Electrical design

The display candidate is Surenoo **STP0350A3-320480 NTP**, a 480 × 320 IPS panel in landscape with a 40-position, 0.5 mm flex. This variant has no touch layer. J2 follows its documented four-wire SPI pinout. The MSP3525 is the bench module for testing the screen size and ST7796-family driver. Confirm the bare panel's supplied revision, contact side, supply requirements and initialization with a sample before choosing J2's footprint. [Surenoo panel document](https://manuals.plus/surenoo/stp0350a3-320480-series-tft-lcd-panel-manual.pdf) · [MSP3525 bench module](https://lcdwiki.com/index.php?title=3.5inch_IPS_SPI_Module_ST7796)

| Function | Pi BCM GPIO |
|---|---|
| Display SPI MOSI, clock and CE0 | 10, 11, 8 |
| Display data/command, backlight PWM, reset | 25, 26, 24 |
| Navigation Up, Down, Back, Open | 17, 22, 27, 23 |
| Confirm, Mode | 5, 6 |
| SE051 I²C SDA, SCL | 2, 3 |

GPIO24 is the proposed display reset. The six active-low controls match the software at the checkout's base revision, where GPIO6 selects Mode. Use the four joystick directions and leave the centre contact disconnected. The recovery keyboard selects a character with Confirm. Keep Confirm identifiable by touch. [Base button mapping](https://github.com/paolino/air-gap-signer/blob/4bd577d2685f8875e9c7e873d5bd20d3feed3cad/crates/signer-pi/src/buttons.rs#L15)

U1 uses NXP **SE051C2HQ1/Z01XD** in **SOT1969-1**. VIN, VCC, ENA and the unused ISO 7816 reset are held at 3.3 V. LA, LB, VSS and the thermal pad go to ground per the datasheet. The Pi supplies the I²C pull-ups for this draft. Probe pads expose 3.3 V, ground, SDA and SCL for bring-up. Check the footprint against the manufacturer's exact land pattern and test I²C and the absence of a contactless response at power-up on the assembled board. [NXP datasheet](https://www.nxp.com/docs/en/data-sheet/SE051.pdf) · [Package drawing](https://www.nxp.com/docs/en/package-information/SOT1969-1.pdf)

The panel drawing gives approximately **120 mA total** for the backlight. The schematic leaves the LED anode, three cathodes and GPIO26 PWM open pending selection of a current-limited driver powered from the Pi's 5 V rail. Verify the LED grouping and current on the actual panel. Complete that circuit and review backlight load, footprints and clearances before routing.

## Mechanical fit

The front panel uses **two rows in an 80 × 80 mm interface area**. The upper row is the 3.5-inch landscape display. Across the lower row, from left to right, are the joystick, optional spare button, front-facing camera, Mode and Confirm. The joystick and Confirm sit at the thumb edges. The spare and Mode button centres sit 18 mm to either side of the lens. The camera lens and active display share the horizontal centre of the 80 mm interface. Test button spacing on a full-size mock-up. The panel has a provisional 92 × 92 mm frame around its 80 × 80 mm interface. The visible 73.44 × 48.96 mm image fits in the upper row. Mount the lens and display behind the continuous rim.

### Mounting interface

The proposed rear carrier has four blind M4 threaded inserts on a **75 × 75 mm square**, centred on the panel. This adopts the common 75 mm monitor-bracket pattern. The outer frame is 92 mm square, so the hole centres are 8.5 and 83.5 mm from either edge. Load passes from the inserts into the carrier and frame. The panel can be installed and removed without opening the protected assembly. On a flat host plate, mark the four centres using the template and allow nominal 4.5 mm screw-clearance holes. Check the printed 50 mm scale before using it. [Printable template](mount.svg) · [Bracket pattern reference](https://www.ergotron.com/en-us/support/vesa-standard)

For release, check with an actual 75 mm bracket and a flat drilled plate. Publish the measured panel depth, rear connector position and cable clearance, mounting tolerances, screw engagement limits and suitable screw lengths for different plate thicknesses. Verify insertion stops short of the electronics and mounting loads leave the LCD and camera undisturbed. The template is for prototype layout pending these checks.

### USB interface for laptops and enclosure builders

The panel acts as a bus-powered USB device. Treat all messages from the host as untrusted. Display, PIN controls, authentication and wallet keys belong to the panel. On the camera-equipped model, camera acquisition must work with a host that has no camera socket. Select and validate an on-panel capture/decoder or bridge for the chosen sensor. The Pi CSI ribbon is a bench arrangement. Before release publish connector and mating part numbers, pinout, voltage and peak current, cable limits, and a versioned request/response description. Supply a working host client. Test a complete disposable-key signing exchange with the finished panel plugged directly into a laptop, including peak load with the display and camera operating. See the [release test](secure-project.md#evidence-required-for-release).

The Raspberry Pi and laptop should use the same public host protocol. Start with an unsigned request and a disposable-key test case, and verify the returned signature against the submitted bytes. The Pi’s existing USB-stick reader is a source of requests for this client.

### Try the panel controls on a Raspberry Pi

Use the existing 240 × 240 bonnet on a bench Pi with a disposable sequence. Finish or exit the current review first. The demo takes the framebuffer lock before it opens the GPIO lines. An ordinary transaction review interprets joystick movements as navigation immediately.

On a Linux build machine run `nix develop -c just build-pi`. Copy `target/aarch64-unknown-linux-musl/release/signer-pi-demo` to the 64-bit Pi and run `./signer-pi-demo --panel-trial` with access to the framebuffer and GPIO device. The first prompt asks for nine actions. Each action is one of the four directions followed by release. Cover the joystick, enter the sequence, then repeat it when prompted. Mode restarts the current entry. A mismatch asks for another attempt. A match opens one built-in sample review, which ends on confirmation or rejection. The PIN screen stays still during entry. The Pi holds the disposable sequence in memory for the trial.

The Pi reads debounced switches and passes their levels to the same [Rust PIN entry library](../../crates/panel-input/src/lib.rs) built for the proposed STM32H573. That library consumes a neutral position, the four directions, handles ambiguous inputs and cancellation, and emits a completed sequence. It owns no GPIO, screen, clock or credential store. The Pi trial compares disposable sequences in memory. The production controller must submit the sequence to the secure element under the policy in [issue #39](https://github.com/paolino/air-gap-signer/issues/39).

Run `nix develop -c just build-panel-input` to compile the library for bare-metal Cortex-M33. On the first controller board, replay the same switch traces and compare results, then measure real switch bounce and check that display redraw and camera load do not lose input. The controller still needs its own board drivers and bootable firmware. This trial exercises the front-panel interaction on Linux. The production controller, paired phone protocol, chip-enforced retry policy and signing operation are outside this bench trial.

### Pi 3B bench installation

Feet, Pi ports and microSD service access below describe the existing bench box. Host enclosure builders choose their own compute board, ports, storage and supports. The bench card stays installed during normal use.

Mount the Pi to the base on its four standoffs and support the front board independently on enclosure rails or bosses. Fasten it from the back or the edges, clear of each plug path. The 40-pin connection mates directly through a socket stack with full pin engagement. Start with **20 mm from the top of the Pi PCB to the underside of the front PCB**, leaving **4 mm nominal** over the Pi's 16 mm USB stacks before subtracting any underside parts, solder or tolerance. Verify the socket height and keep protruding parts and fasteners out of that overhead clearance. Use removable front-board fixings so the Pi's internal CSI and DSI ribbon latches remain serviceable.

With the front board installed, insert and remove the actual transaction stick and plugs at all four USB-A ports, Ethernet, micro-USB power, HDMI and A/V. Include the plug shells, the Ethernet release tab, finger grip and cable bend room in the fit check. Power off, open the base and check that the owner can replace the card and refit the cover without disturbing the display flex. Check the screen flex fold and connector latch before fixing mounting holes. The drawing sets targets for this fit check. Final cutouts, socket stack and feet depend on the physical assembly.

## Camera and phone approval

Use the standard autofocus Raspberry Pi Camera Module 3 as the bench candidate. Its nominal envelope is 25 × 24 × 11.5 mm. Connect it to the Pi 3B camera socket with a standard 15-pin ribbon. Mount the camera to the inside of the removable front panel, with its lens facing the same direction as the display. Place the module behind the lower-row opening between the joystick and buttons with 2.74 mm nominal plan clearance to the hidden LCD outline. Mount it from inside on bosses tied into the surrounding lid. Keep a continuous outer rim and recess the lens behind the front surface. Choose the bore and recess depth from the module drawing and check the field of view and autofocus travel on the assembled lid. Keep the CSI ribbon and camera body clear of the controls, LCD flex, Pi header and standoffs. Prototype the lid in the selected material and check for flex or cracking around the lens and switch openings under button presses, corner loading and handling. The drawing establishes component positions in plan. Material, wall sections, ribs, lens aperture and assembled strength require physical verification. [Camera module specifications](https://www.raspberrypi.com/products/camera-module-3/) · [Raspberry Pi mechanical drawings](https://pip.raspberrypi.com/categories/1207-design-files)

On the bench, display a harmless QR code on the actual Android phone and read it through the camera with the intended case opening in place. Then scan the longest recognition and approval messages in both directions on the 240 × 240 prototype and the proposed 480 × 320 display. For a face-to-face exchange, use the phone’s front camera and hold its top edge toward the signer display, with the phone’s on-screen QR facing the signer lens below. Test both scan directions from one hold with the intended phone. Vary distance, lateral offset and tilt, and record the range that works with glare and handheld motion. The standard camera has a nominal near focus of about 10 cm. Actual decoding distance also depends on the phone camera and displayed code size. Check cancellation. [Camera focus specifications](https://www.raspberrypi.com/documentation/accessories/camera.html) If evidence spans several QR frames, bound their count and total size and verify the completely assembled message. [Security and acceptance criteria](secure-project.md)

## Bench checks before layout

Use the MSP3525 to render a transaction with full addresses, several outputs and native assets. Parameterize the signer's current 240 × 240 renderer, check panel colour and orientation, and measure when the updated review is visible relative to when Confirm is accepted. Check all six controls, then identify the secure element and verify a signature made with a disposable key. The KiCad schematic is an electrical draft. It has no routed PCB or fabrication files. [Renderer at the base revision](https://github.com/paolino/air-gap-signer/blob/4bd577d2685f8875e9c7e873d5bd20d3feed3cad/crates/signer-pi/src/display.rs#L12)
