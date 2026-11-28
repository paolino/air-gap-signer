# STM32 signer bench prototype — bill of materials

Quantities are for **one STM32H573 signer**. Both the Raspberry Pi and STM32 can exercise the base signer with a host webcam. The first STM build uses assembled modules and a solderless breadboard. Use the Discovery kit screen for initial testing. The full optical bench adds a larger display and an STM-connected camera so the phone exchange can run with the Pi disconnected.

## Order or reuse for the first build

| Qty | Part | Role and purchasing notes |
|---:|---|---|
| 1 | **ST STM32H573I-DK** Discovery kit | Includes the STM32H573, 1.54-inch 240 × 240 LCD, USB programmer/debugger and MB1280 expansion adapter. Order the full kit. [ST kit](https://www.st.com/en/evaluation-tools/stm32h573i-dk.html) · [Kit contents and connectors](https://www.st.com/resource/en/user_manual/um3143-discovery-kit-with-stm32h573ii-mcu-stmicroelectronics.pdf) |
| 1 | **MikroE MIKROE-5392**, SE051 Plug&Trust Click | Reuse the SE051C2 module you showed, or order one if the Pi rigs need theirs simultaneously. The included MB1280 adapter has a matching mikroBUS socket at CN10/CN11. The Click uses 3.3 V logic and I²C. [Click board](https://www.mikroe.com/se051-plugtrust-click) · [Adapter manual, §2.5](https://www.st.com/resource/en/user_manual/dm00688718-stmod-fanout-expansion-board-for-stm32-discovery-kits-and-evaluation-boards-stmicroelectronics.pdf) |
| 1 | **Adafruit 504**, five-way navigation switch | Wire up, right, down and left. Leave the centre contact open. The through-hole switch fits a solderless breadboard with slight lead adjustment. [Switch](https://www.adafruit.com/product/504) |
| 2, plus 1 optional | **6 mm normally open tactile pushbuttons**, through-hole | Mode and Confirm. Use a third to mock up the Spare position. One pack covers all three. [Adafruit 367](https://www.adafruit.com/product/367) |
| 1 | Solderless breadboard, 2.54 mm pitch, 400 points or larger | Holds the controls and provides a common ground. Reuse if available. |
| 1 set | 2.54 mm jumper wires, male–male and male–female | Use male–male for breadboard and female sockets, and male–female for modules with fitted pin headers. A mixed set with at least ten of each is convenient. |
| 1 | USB **data** cable with USB-C at the board end | Use the ST-LINK connector for programming. Use the board’s user USB-C port (CN17) for the direct laptop signing protocol trial once the firmware supports it. Match the other end to the development computer. Reuse if available. |

## Display and camera options

| Qty | Evaluation part | What it tests |
|---:|---|---|
| 1 for the camera version, 0 for the base | **Arducam Mega B0401**, 5 MP autofocus SPI camera, supplied six-wire cable | Proposed STM camera. Its SPI interface can connect to the microcontroller and deliver image data for QR decoding. Verify the selected header, supply budget, firmware driver and successful decoding of the actual phone messages. Its 33 × 33 × 17 mm overall envelope requires a fresh fit check against the current panel drawing. [Arducam specifications](https://www.arducam.com/blog/product/presale-mega-5mp-color-rolling-shutter-camera-module-with-autofocus-lens-for-any-microcontroller/) |
| 1 | **LCDWIKI MSP3525**, 3.5-inch 480 × 320 non-touch SPI module with fitted 14-pin header | Tests the target review and QR display size. The module calls for a 5 V supply. Verify logic levels and current budget before wiring. Its 98 mm carrier is wider than the proposed 92 mm panel, so use it on the open bench. [Module and interface](https://lcdwiki.com/index.php?title=3.5inch_IPS_SPI_Module_ST7796) · [Mechanical drawing](https://www.lcdwiki.com/res/MSP3525_MSP3526/MSP3525_MSP3526_Specification_EN_V1.0.pdf) |

For the camera-free bench, use the development computer’s webcam to relay the phone’s codes over USB. Use the same panel display, phone app and controller approval checks for both versions.

Reuse the phone and development computer. The **SC1223** modules and Pi camera ribbons are optional for testing the one-hold optical variant on the Raspberry Pi rigs. The STM camera above is a separate engineering sample. The finished panel’s camera, LCD mounting and board layout are still under evaluation.

**Verification status.** The shared joystick input library has compiled for the STM32H573 processor target. This parts selection is based on manufacturer documentation. The assembled STM rig still needs a pin assignment, board drivers and a physical test. Start with joystick entry and SE051 communication on the kit screen. For the optical test, connect the external LCD and camera together, scan the longest phone approval and recognition messages in both directions, and check that display and camera activity preserve button input. Chip-enforced PIN retries and key-use authorization are separate hardware tests.
