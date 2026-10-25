# The winding machine (rev C)

A two-axis CNC coil winder that runs the G-code the Rust app bakes, on stock
GRBL 1.1 with an Arduino Uno and a CNC Shield v3. It lies flat on the bench
like a small lathe: the bobbin spins on a horizontal spindle and the guide
carriage runs along the base beside it. As on commercial and DIY CNC pickup
winders, nothing stands tall: the wire leaves a spool on the bench and runs
level and straight through the tensioner and the guide to the coil, at
spindle height. A flange finder on the guide measures
each bobbin on the machine before winding. The priced parts list is in
[`PARTS.csv`](PARTS.csv). The older [`BUILD_GUIDE.md`](BUILD_GUIDE.md)
describes the hand-built Python-era machine; where the two differ, follow
this page.

```
 top view (operator at the bottom)

   ┌──────────────────────────────────────────────────────────┐
   │        felt tensioner ─ dancer ─┐     spool + pigtail post │
   │                                 │ wire, level, 80 high     │
   │ end    ════════ T8 x 8 lead screw ═════════  carriage     │
   │ brg    ════════ MGN12 rail on a 2020 bed ═══ NEMA 17 (Y)  │
   │   switch ▐▌ carriage: pigtail, cross-slide, arm, eyelet,  │
   │             flange finder, laser                          │
   │                   │ eyelet, 48 mm behind the axis          │
   │ NEMA 17 ═ coupler ═ KP08 ══ KP08 ═ platen │bobbin│  ─► +Y │
   │ (X)        headstock                                      │
   │ [electronics box]                                         │
   └──────────────────────────────────────────────────────────┘
          [foot pedal]
```

## How it maps to the software

| App / G-code       | Machine                                                                 |
|--------------------|-------------------------------------------------------------------------|
| `X` (turns)        | Spindle stepper, CNC Shield **X** slot. `$100=1600`: 200 steps × 1/8 microstep, so `X1` is one turn. |
| `Y` (mm)           | Carriage stepper, CNC Shield **Y** slot. `$101=200`: 1600 steps per turn over the T8 screw's 8 mm lead. Positive Y moves the guide **away from the platen**. |
| `G93` feed         | Inverse time: each `G1` takes `1/F` minutes, so every helix lands at the designed RPM. |
| `G38.2 Y-20 F120`  | The carriage runs toward the headstock onto a microswitch on the probe input (A5, labelled SCL on the shield). Stock GRBL, no recompile. |
| `G92 X0 Y0`        | Y0 is where the switch trips, about 10 mm short of the near flange's inner face. |
| `flange_offset_mm` | Switch to the near flange's inner face. The default profile now says 10.0; `winder calibrate` measures it for each bobbin type. |
| `M0`               | Program pause. Resume from the green button (A2) or the app.            |
| `M8` / `M9`        | Flood-coolant output (A3): drops and lifts the flange feeler's solenoid. Only the flange finder uses it. |
| `M3 S1000` / `M5`  | Spindle PWM (D11) at full duty is a steady 5 V that powers the index sensor, only while the flange finder looks for it. |
| CW / CCW           | The app flips the sign of X. Seen from the bobbin's top (looking at the platen), a CW design turns the platen clockwise; flip `$3` bit 0 if it doesn't. |

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
$6=0        ; probe switches normally open, closing to GND
$20=0 $21=0 $22=0   ; no soft/hard limits or homing cycle: homing is the probe move
```

1000 RPM is 26.7 kHz of step pulses, inside the Uno's ~30 kHz. A NEMA 17
on an A4988 keeps its torque that fast only on **24 V**; on 12 V expect the
spindle to stall somewhere above 600 RPM.

## Calibrating around the bobbin

Each bobbin type sits a little differently on its nest, and real flatwork is
never exactly the drawing size. If the guide's turn-round points are off by
a few tenths, wire piles up against one flange or leaves a gap. These parts
measure the bobbin on the machine:

| System             | What it does                                                        |
|--------------------|---------------------------------------------------------------------|
| **Flange finder**  | A 0.5 mm spring-steel blade (a feeler-gauge leaf) on a D2F-L microswitch, hinged under the guide eyelet, in line with it along the traverse. Wired to the probe pin in parallel with the homing switch. A small 12 V push solenoid (on `M8`) swings it down into the window, where it touches each flange's inner face; a return spring lifts it on `M9`. |
| **Index sensor**   | A 3144 Hall switch over the platen and a magnet in the nest, in line with the bobbin's length. The flange finder spins the platen until the sensor trips, then a quarter turn more, so an end of the bobbin faces the guide. Its output is on the probe pin too; it is powered from D11 so it is only live during that search. |
| **Guide cross-slide** | A 40 × 40 mm manual stage with a 13 mm micrometer head under the guide post. Sets the eyelet's distance from the coil in 0.01 mm steps (48 mm from the axis nominal). |
| **Height clamp and scale** | The guide arm clamps to the post with a steel rule beside it. Set the eyelet level with the spindle axis; read the scale to repeat it. |
| **Laser line**     | A 5 V red line module on the post marks where the wire lands. Use it to check the eyelet lines up with each flange face and to watch the lay. |
| **Turn counter**   | An A3144 Hall sensor over the platen rim, a magnet in the platen, and an LCD pulse counter. It runs on its own, so if it ever disagrees with GRBL's turn count, the spindle lost steps. |
| **Alignment tools** | An 8 mm ground test bar held in the platen and a 0.01 mm dial indicator on the carriage. Sweep the carriage along the bar to set the rail parallel to the spindle (aim under 0.02 mm over 50 mm), and spin the platen against the indicator to check runout. |
| **Tension gauge**  | A 0-100 g pocket spring scale hooked on the wire at the guide while the spindle turns slowly. Set the felt tensioner to the design's `tension_g`. |

### The flange-finder routine

```
winder calibrate strat.toml                 # writes strat-flanges.nc
```

Send `strat-flanges.nc` with the bobbin on its nest; it needs no hands. It
homes, moves the guide to the middle of the window, spins the platen to the
index and a quarter turn on so a bobbin end faces the guide, and drops the
feeler. It touches the near flange, then the far one, slowly (30 mm/min),
lifts the feeler and parks. GRBL prints four lines like
`[PRB:0.000,6.250,0.000:1]` (switch, index, near flange, far flange). Then:

```
winder calibrate strat.toml --probes "<paste GRBL's output>"
#   flange offset 10.120 mm (was 10.000)
#   window 11.000 mm (design says 11.000)
#   wrote strat-machine.toml
winder gcode strat.toml -m strat-machine.toml
```

The feeler's middle is in line with the eyelet, so it stops half a blade
inside each flange: offset = near − switch − t/2 and window = far − near + t.
If the window differs from the design by more than 0.1 mm the command says
so; put the measured value in the design's `channel_height_mm`. Keep one
machine profile per bobbin type (`strat-machine.toml`, `paf-machine.toml`).

Built without the solenoid and index sensor, use `winder calibrate
--manual`: the program stops (`M0`) for you to turn the bobbin and swing the
feeler in by hand, then again to swing it out, and reports three lines.

The search starts from the profile's `flange_offset_mm`, so before the first
run set it to within 2 mm with a ruler. A touch that lands on the homing
switch instead of a flange is rejected.

## Mechanical layout

All dimensions in mm. Origin on the spindle axis at the platen face; x runs
along the axis toward the bobbin, z toward the operator.

### Base
- 18 mm birch plywood, 480 × 340, four rubber feet.

### Headstock (spindle)
- **Frame:** two 100 × 65 × 6 aluminium side plates and a top plate (or a
  stack of 18 mm ply), spindle axis 80 above the base. Low enough that the
  guide reaches it on a short post, high enough to clear a P-90's ears.
- **Bearings:** two KP08 pillow blocks 70 apart on the top plate, 8 mm
  hardened shaft. They take the wire pull and keep the platen true.
- **Motor:** NEMA 17 on a steel L-bracket behind the headstock, 5 → 8
  flexible coupler to the shaft.
- **Platen:** 70 × 6 aluminium disc on an 8 mm rigid flange coupling, four M3
  tapped holes on a 44 mm circle, a 6 × 2 mm magnet in its rim for the turn
  counter.
- **Bobbin nests:** one printed PETG nest per bobbin type (Strat, Tele
  bridge, Tele neck, PAF, P-90). The bottom flange drops into a pocket 0.2 mm
  larger than the flange; an M4 knob through the centre pole hole holds it.

Inertia check: the platen is about 60 g, 3.7e-5 kg·m²; at `$120=200`
turns/s² that needs about 0.05 N·m, a fraction of what a 1.7 A NEMA 17 has
at 24 V.

### Carriage (traverse)
- **Bed:** 2020 extrusion, 250 long, on two 7 mm pads on the base, 95 behind
  the spindle axis and parallel to it. Rail top at 35. Slotted holes let you tram it with the test bar.
- **Rail:** MGN12H, 250 long, on top of the bed.
- **Drive:** NEMA 17 at the far end, 5 → 8 coupler, T8 × 8 (4-start) lead
  screw 30 further back, KFL08 end bearing at the headstock end,
  anti-backlash nut.
- **Carriage:** 70 × 60 × 6 plate on the rail block and the nut bracket,
  carrying the cross-slide, a short 15 × 15 post (26 tall), the height clamp
  and a 3 × 15 flat-bar arm to the ceramic eyelet (1-2 mm bore) at axis
  height, 48 mm behind the axis. A ceramic pigtail behind the post, also at
  axis height, takes the wire arriving level from the tensioner, so pigtail,
  eyelet and coil are in one straight line.
- **Stroke:** 150. The program only uses the window (a P-90 is about 9.5),
  the rest lets you park the guide clear while changing bobbins.
- **Homing switch:** KW12 microswitch, normally open, on a slotted bracket at
  the headstock end, tripping about 10 mm before the eyelet reaches the near
  flange. The probe move only goes 20 mm, so park the guide within 20 mm of
  the switch before a program starts.

### Wire path
Everything sits on the base behind the carriage, and from the tensioner on
the wire stays at spindle height (80):

1. **Spool** standing upright on a 10 mm dowel behind the carriage; the wire
   peels off over the top end, so the spool never spins or overruns.
2. **Pigtail** on a 150 mm post with a short arm over the spool, about 65
   above the spool top, enough for the balloon to form. This is the tallest
   part of the machine.
3. **Felt tensioner** on a 92 mm post in line with the middle of the winding
   window: two felt pads squeezed by a spring and an M5 thumb screw. Start
   around 30-40 g for 42 AWG and check with the gauge.
4. **Dancer:** a spring-loaded arm on a second 92 mm post, with a ceramic
   eyelet at spindle height, that soaks up the jerk as the wire leaves the
   spool. Two microswitches at the ends of its travel are wired to Hold (see
   below).
5. **Straight on** through the carriage's rear pigtail and the guide eyelet
   onto the coil, on a tangent. The carriage only moves across the 6-12 mm
   window while winding, so the wire's angle from the dancer changes by
   well under 10 degrees.

### Keeping the wire whole
42 AWG plain enamel copper breaks at about 70 g; 43 and 44 AWG at about 55
and 45 g. The design presets wind at half that, and the machine is built so
the pull at the coil stays close to the set value:

- **Set tension** with the spring scale, 30-40 g for 42 AWG. `winder sim`,
  `winder gcode` and the editor warn once a design's `tension_g` is over 70 %
  of the wire's breaking force (about 49 g for 42 AWG): guide friction and
  spool jerk add to the set value, so that is where breaks start.
- **No spool inertia:** the spool stands still and the wire peels over its
  end, so starting and stopping the spindle never yanks a heavy spool round.
- **Dancer** soaks up the surges from the balloon and from the guide turning
  round.
- **Soft starts:** each design ramps the spindle up and down over
  `ramp_turns` (50 on the Strat preset, from 60 RPM), and GRBL's
  acceleration limit (`$120`) eases every stop and feed hold.
- **Smooth path:** only polished ceramic touches the wire, and from the
  tensioner on it runs straight and level, with no sharp corners.
- **Snag or break:** the dancer's two end switches sit on Hold, in parallel
  with the pedal. A snag pulls the arm hard to one stop; a break lets the
  spring throw it to the other. Either one feed-holds GRBL, so the spindle
  decelerates and stops with the wire still under tension (`$1=255` keeps
  the motors energised). Splice or clear it and press resume.

### What is automatic
Homing, flange calibration (orienting the bobbin, the feeler, both touches)
and the whole wind with its ramps, pattern, turn count and pauses run on
their own. By hand, as on commercial winders: mounting the bobbin, threading
the wire and wrapping the start lead on its eyelet, and cutting, taping and
soldering the leads afterwards. Once per bobbin type you paste the flange
finder's result into `winder calibrate --probes` and set the eyelet with the
height clamp and cross-slide; once per build you tram the rail, set driver
current and set the tension. When the app streams to GRBL itself (milestone
3) it can read the probe results directly.

### Electronics
- Uno + CNC Shield v3 + two A4988 (spare in the drawer) in an ABS box at the
  front left, with a 40 mm fan and the turn counter's display in the front.
- **Microstep jumpers: MS1 and MS2 fitted, MS3 off** for 1/8 step. (All
  three fitted would be 1/16 on an A4988 and every turn would come out half
  size.) The same two jumpers give 1/8 on a DRV8825.
- **Vref** with the motor unplugged: about 0.6 V on the spindle driver,
  0.4 V on the carriage (A4988 with 0.1 Ω sense resistors: I = Vref × 1.25).
- 24 V 5 A enclosed supply to the shield's power terminal. Fit a mains
  switch.

| Shield pin       | Uno pin | Connect                                              |
|------------------|---------|------------------------------------------------------|
| X motor header   | D2, D5  | spindle motor                                        |
| Y motor header   | D3, D6  | carriage motor                                       |
| Abort            | A0      | E-stop, normally-open contact, to GND                |
| Hold             | A1      | foot pedal and both dancer end switches (all NO, in parallel), to GND |
| Resume           | A2      | green button (momentary NO), to GND                  |
| CoolEn           | A3      | gate of a logic-level MOSFET (IRLZ44N, 10 k pull-down) switching the 12 V feeler solenoid, with a flyback diode |
| SCL              | A5      | homing switch, flange feeler and index Hall output, all open-drain/NO, in parallel, to GND |
| SpnEn (Z+/Z-)    | D11     | index Hall sensor's supply (the pin sources about 20 mA; the 3144 draws under 10) |
| 5V / GND         | -       | turn counter (Hall sensor and display)               |
| 12-36 V terminal | -       | 24 V supply; a small 24 → 12 V buck feeds the solenoid |

## Bring-up

1. Flash GRBL 1.1h, send the settings above, then `$$` to check them.
2. Jog `X1` with no bobbin: the platen makes exactly one turn and the turn
   counter shows 1. If it turns the wrong way, set `$3` bit 0.
3. Jog `Y5`: the carriage moves 5.00 mm away from the headstock (measure with
   calipers). If it goes the other way, set `$3` bit 1. If it moves 10 mm,
   the jumpers are at 1/16.
4. Put the test bar in the platen and the dial indicator on the carriage.
   Sweep 50 mm along the bar; shim or slide the bed until it reads under
   0.02 mm. Spin the platen and check runout at the flange radius.
5. Send `G38.2 Y-20 F120` with the guide a few mm from the switch: it stops
   on the switch and GRBL prints a `[PRB:...]` line. If it errors at once,
   a switch is wired normally-closed or `$6` is wrong. Press the feeler by
   hand during a slow `G38.2`: it must stop the move too. Send `M8` and
   `M9`: the feeler drops and lifts. Pull the dancer to each stop while
   jogging slowly: GRBL must go to Hold.
6. Set the eyelet level with the axis (height clamp) and 48 mm behind it
   (cross-slide), with the laser line on the flange faces as a check.
7. Run the flange finder for each bobbin type you wind.
8. Thread the wire, set the tension with the gauge, wind a scrap coil, and
   compare its DC resistance with the app's estimate and its turns with the
   counter.

## Cheaper build

About 870 lei instead of about 1,160 (about 750 if you borrow the dial
indicator and the spring scale): two 8 mm rods and LM8UU bearings in place
of the MGN12 rail, PETG for the headstock frame, carriage plate and brackets,
and a dead 3D printer for both steppers, the lead screw and the supply. Keep
the flange finder, the cross-slide, the ceramic guides and the two spindle
bearings; they are what make the coil accurate. To save another 38 lei, leave out the
feeler solenoid and index sensor and calibrate with `--manual`; keep the
dancer switches, they are what saves a coil when the wire snags.
