# The winding machine (rev A)

A two-axis CNC coil winder that runs the G-code the Rust app bakes, on stock
GRBL 1.1 with an Arduino Uno and a CNC Shield v3. This page is the design;
the priced parts list is in [`PARTS.csv`](PARTS.csv). The older
[`BUILD_GUIDE.md`](BUILD_GUIDE.md) describes the hand-built Python-era
machine; where the two differ, follow this page.

```
                 overhead arm (2020)
   felt tensioner ──┬──── dancer ─────┐
   pigtail ─┐       │                 │ wire drops to the guide
            │                         │
   mast     │                         ▼     2020 column + MGN12 rail
   (2020)   spool   ┌──────┐    ┌── pigtail ──┐   T8 x 8 lead screw
            (upright)│bobbin│◄───eyelet  arm  ├─ carriage (Y, mm)
                    ├──────┤                  │
                    │platen│  (X, turns)      │
                    └──┬───┘                  │
              deck ────┼────                  │
                       │ coupler    homing switch (probe, Y0)
                   spindle NEMA 17       traverse NEMA 17
   ─────────────────── 18 mm ply base ─────────────────────
```

## How it maps to the software

| App / G-code              | Machine                                                          |
|---------------------------|------------------------------------------------------------------|
| `X` (turns)               | Spindle stepper, CNC Shield **X** slot. `$100=1600`: 200 steps × 1/8 microstep, so `X1` is one turn. |
| `Y` (mm)                  | Traverse stepper, CNC Shield **Y** slot. `$101=200`: 1600 steps per turn over the T8 screw's 8 mm lead. Positive Y moves the guide **up**, away from the bottom flange. |
| `G93` feed                | Inverse time: each `G1` takes `1/F` minutes, so every helix lands at the designed RPM. |
| `G38.2 Y-20 F120`         | The carriage drives down onto a microswitch wired to the probe input (A5, labelled SCL on the shield). Stock GRBL, no recompile. |
| `G92 X0 Y0`               | Y0 is where the switch trips. Set the switch bracket so that is the eyelet level with the inner face of the bottom flange, or measure the gap and put it in the machine profile's `flange_offset_mm`. |
| `M0`                      | Program pause. Resume from the green button (A2) or the app.    |
| CW / CCW                  | The app flips the sign of X. Seen from above, a CW design must turn the platen clockwise; flip `$3` bit 0 if it doesn't. |

The default `MachineProfile` in `winder-gcode` already describes this build,
so `winder machine` prints the right settings and nothing needs editing.

### GRBL settings

```
$100=1600   ; X steps per turn
$101=200    ; Y steps per mm
$110=1000   ; X max rate, turns/min
$111=600    ; Y max rate, mm/min
$120=200    ; X accel, turns/s^2
$121=200    ; Y accel, mm/s^2
$11=0.02    ; junction deviation
$1=255      ; keep drivers on, so a pause holds tension
$3=0        ; direction invert mask: set bit 0 (X) or 1 (Y) if an axis runs backwards
$6=0        ; probe switch normally open, closing to GND
$20=0 $21=0 $22=0   ; no soft/hard limits or homing cycle: homing is the probe move
```

1000 RPM is 26.7 kHz of step pulses, inside the Uno's ~30 kHz. A NEMA 17
on an A4988 keeps its torque that fast only on **24 V**; on 12 V expect the
spindle to stall somewhere above 600 RPM.

## Mechanical layout

All dimensions in mm. Origin on the spindle axis at the top of the base.

### Base
- 18 mm birch plywood, 380 × 260, four rubber feet. A flat, stiff base keeps
  the spindle and the rail parallel.

### Spindle (vertical)
- **Deck:** 110 × 110 × 6 aluminium plate (or 8 mm ply) on four M4 × 84
  standoffs. Top face at 90.
- **Motor:** NEMA 17 hanging under the deck on four M3 × 20 spacers, shaft up.
- **Shaft:** 8 mm hardened rod through a 608ZZ pressed into the deck and a
  KFL08 flange bearing bolted on top. The motor drives it through a 5 → 8
  flexible coupler in the spacer gap. The two bearings take the wire pull, so
  the motor's own bearings only see torque and the platen runs true.
- **Platen:** 70 × 6 aluminium disc on an 8 mm rigid flange coupling, face at
  114. Four M3 tapped holes on a 44 mm circle.
- **Bobbin nests:** one printed PETG nest per bobbin type (Strat, Tele bridge,
  Tele neck, PAF, P-90, the app's presets). The bottom flange drops into a
  pocket 0.2 mm larger than the flange; an M4 knob through the centre pole
  hole, or a printed clip over the top flange, holds it down. Check runout by
  hand before winding: under 0.1 mm at the flange edge.

Inertia check: the platen is about 60 g, 3.7e-5 kg·m²; at `$120=200`
turns/s² that needs about 0.05 N·m, a fraction of what a 1.7 A NEMA 17 has
at 24 V.

### Traverse (vertical, parallel to the spindle)
- **Column:** 2020 extrusion, 300 tall, 150 from the spindle axis (behind and
  to the right, as in the app's 3D view), bolted to the base with two corner
  brackets.
- **Rail:** MGN12H, 200 long, on the column's face toward the spindle. Square
  it to the base with an engineer's square; any lean shows up as a lay that
  drifts across the window.
- **Drive:** NEMA 17 standing on the base beside the column, shaft up, 5 → 8
  coupler, T8 × 8 (4-start) lead screw to a KFL08 at the top bracket.
  Anti-backlash nut. The screw can back-drive under the carriage's weight when
  the motor is off; `$1=255` keeps the drivers on.
- **Carriage:** 60 × 50 × 5 plate joining the rail block and the screw nut.
- **Guide arm:** 3 × 15 aluminium flat bar, 56 long, ceramic eyelet (1-2 mm
  bore) at the tip, 58 from the spindle axis at the height of the wire. A
  ceramic pigtail at the arm root takes the wire coming down from above.
- **Stroke:** 120 mm usable, far more than the tallest bobbin (a P-90 is
  about 9.5 between flanges).
- **Homing switch:** KW12 microswitch, normally open, on a slotted bracket
  under the carriage. The carriage trips it at Y0. The probe move only goes
  20 mm, so park the guide within 20 mm above the switch before you start a
  program (the app's jog does this).

### Wire path
1. **Spool** standing upright on a 10 mm dowel; the wire peels off over the
   top end, so the spool never spins and never overruns.
2. **Pigtail** 300 above the spool on the overhead arm (high enough for the
   balloon to form).
3. **Felt tensioner:** two felt pads squeezed by a spring and an M5 thumb
   screw. Start around 30-40 g for 42 AWG (the design's `tension_g`).
4. **Dancer:** a spring-loaded arm with a ceramic eyelet that soaks up the
   jerk as the wire leaves the spool.
5. **Down** to the pigtail on the carriage, along the arm to the eyelet, and
   onto the coil on a tangent, as in the app's 3D view.

The overhead arm is a 250 mm piece of 2020 from the top of a 400 mm mast at
the back left.

### Electronics
- Uno + CNC Shield v3 + two A4988 (spare in the drawer) in an ABS box at the
  front left, with a 40 mm fan.
- **Microstep jumpers: MS1 and MS2 fitted, MS3 off** for 1/8 step. (All
  three fitted would be 1/16 on an A4988 and every turn would come out half
  size.) The same two jumpers give 1/8 on a DRV8825.
- **Vref** with the motor unplugged: about 0.6 V on the spindle driver,
  0.4 V on the traverse (A4988 with 0.1 Ω sense resistors: I = Vref × 1.25).
- 24 V 5 A enclosed supply to the shield's power terminal. Fit a mains
  switch.

| Shield pin       | Uno pin | Connect                                   |
|------------------|---------|-------------------------------------------|
| X motor header   | D2, D5  | spindle motor                             |
| Y motor header   | D3, D6  | traverse motor                            |
| Abort            | A0      | E-stop, normally-open contact, to GND     |
| Hold             | A1      | foot pedal (momentary NO), to GND         |
| Resume           | A2      | green button (momentary NO), to GND       |
| SCL              | A5      | homing switch (NO), to GND: GRBL's probe  |
| 12-36 V terminal | -       | 24 V supply                               |

## Bring-up

1. Flash GRBL 1.1h, send the settings above, then `$$` to check them.
2. With no bobbin, jog `X1` from the app: the platen makes exactly one turn,
   clockwise from above. If it's backwards, set `$3` bit 0.
3. Jog `Y5`: the guide rises 5.00 mm (measure with calipers). If it drops,
   set `$3` bit 1. If it moves 10 mm, the jumpers are at 1/16.
4. Send `G38.2 Y-20 F120` with the guide a few mm above the switch: it stops
   on the switch and GRBL reports the probe position. If it errors at once,
   the switch is wired normally-closed or `$6` is wrong.
5. Put a bobbin on, home, and look at the eyelet against the bottom flange's
   inner face. Slide the switch bracket until they line up, or measure the
   offset and put it in the profile as `flange_offset_mm`.
6. Run a 50-turn test design with no wire at 150 RPM and watch the guide
   cover the window and turn round at both flanges.
7. Thread the wire, wind a scrap coil, measure its DC resistance and compare
   it with the app's estimate.

## Cheaper build

About 450 lei instead of about 850: two 8 mm rods and LM8UU bearings in place
of the MGN12 rail, PETG for the deck, carriage, platen and brackets, and a dead
3D printer for both steppers, the lead screw and the supply. Keep the ceramic
guides and the two spindle bearings; they are what make the coil neat.
