# Hardware

> For the CNC machine that runs the Rust app's G-code (GRBL 1.1), see
> [`MACHINE.md`](MACHINE.md) and [`PARTS.csv`](PARTS.csv). This page covers
> the original Python-app build.

## Parts

| Part                         | Spec / note                          |
|------------------------------|--------------------------------------|
| Arduino Uno                  | the controller                       |
| 2x stepper driver            | A4988 or DRV8825                     |
| 2x NEMA 17 stepper           | spindle + traverse                   |
| 12V or 24V power supply      | 3-5A; 24V for spindle speeds above ~600 RPM |
| Lead screw + nut             | T8, 8mm pitch (traverse)             |
| Linear rail or rods          | traverse carriage guide              |
| Faceplate / chuck            | holds the bobbin on the spindle      |
| Ceramic wire eyelet          | wire guide on the carriage           |
| Felt pad + screw             | wire tensioner                       |
| Magnet wire                  | 42 AWG (vintage) / 43 AWG (hot)      |

## Recommended: use a CNC Shield v3

Instead of wiring loose A4988 drivers, use an Arduino CNC Shield v3. It
plugs onto the Uno, the two A4988 drivers slot into the X and Y
positions, and motor/power connections are screw terminals. The firmware
pins already match the shield's X/Y mapping, so no code change is needed.
For 1/8 step fit the MS1 and MS2 jumpers under each driver and leave MS3
off. (All three fitted is 1/16 on an A4988 and 1/32 on a DRV8825.)

## Parts list & budget (lei, approximate)

| Part | Qty | ~Price | Notes |
|------|-----|--------|-------|
| Arduino Uno | 1 | (have it) | the controller |
| CNC Shield v3 | 1 | ~30 | holds drivers, breaks out pins |
| A4988 driver | 2 (+1 spare) | ~8 ea | they die easily; buy a spare |
| NEMA 17 stepper (1.7A) | 2 | ~45 ea | spindle + traverse |
| 12V 5A power supply | 1 | ~45 | |
| T8 lead screw + nut | 1 | ~25 | traverse motion |
| 8mm rod + LM8UU bearings | 1 set | ~35 | carriage guide |
| 5-8mm flexible couplers | 2 | ~6 ea | motor to screw/faceplate |
| Magnet wire 42 AWG (~0.5kg) | 1 | ~80-120 | the consumable |
| Ceramic wire eyelet | 1 | ~5 | |
| Jumpers, small hardware | - | ~15 | |
| Plywood / acrylic for frame | - | ~30 | scrap works |
| **Total** | | **~400 lei** | core (no wire) ~270 lei |

Prices are typical Romanian shop estimates (Optimus Digital, Sigma
Nortec); build the cart online for exact live totals. NEMA 17s vary
~40-90 lei by torque/seller. Magnet wire is the recurring cost. You can
trim by salvaging a stepper and PSU from a dead 3D printer.

## Wiring (Uno -> drivers, if NOT using the shield)

| Uno pin | Connects to            |
|---------|------------------------|
| D2      | Spindle driver STEP    |
| D3      | Spindle driver DIR     |
| D4      | Traverse driver STEP   |
| D5      | Traverse driver DIR    |
| D6      | Both drivers ENABLE    |
| GND     | Driver logic GND + PSU GND (common) |

- Motor power comes from the **12V supply through the drivers**, never
  from the Uno. Only logic pins touch the Uno. **Share grounds.**
- Set the driver microstep jumpers to **1/8** (matches firmware default).
- **Set each driver's Vref current limit BEFORE connecting motors.**
  Start ~0.4-0.5V for NEMA 17s and tune up. Skipping this cooks the chip.

## Mechanical constants (must match firmware)

At the top of `winder_firmware.ino`:
```cpp
const int   microstep    = 8;     // driver microstep setting
const float leadScrewMM  = 8.0;   // mm travel per traverse-motor rev
```
If your hardware differs, change these two and everything recalculates.

## Tension & winding tips
- Tension is everything: too loose = sloppy coil, too tight = broken
  wire and a dead pickup. Tune the felt-pad screw.
- Scatter winding (the `JITTER` setting) mimics hand-winding for a
  warmer tone. Tight machine winds are brighter and more compressed.
- Pot the finished pickup in wax (~65C paraffin+beeswax) to kill
  microphonic squeal — important for high-gain use.
