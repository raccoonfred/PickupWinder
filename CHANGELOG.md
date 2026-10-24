# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- **The editor now looks and moves like Fusion 360.** It has a light theme,
  a toolbar of CREATE / INSPECT / MAKE groups and a floating Browser. Edit
  dialogs have OK and Cancel. The timeline shows the sections sized by
  turns, with playback, scrubbing and a right-click menu. Properties floats
  on the right.
- **3D navigation and the ViewCube work as in Fusion and Autodesk.**
  - Scroll zooms at the pointer, middle-drag pans and Shift + middle-drag
    orbits.
  - The navigation bar has Orbit, Pan, Zoom, Fit and Home, and switches
    between perspective and orthographic.
  - The ViewCube turns with the model and has 26 clickable views (faces,
    edges and corners) with highlight on hover. It also has a draggable
    compass ring, a home button and arrows to the next face. Moves are
    animated.
- **The coil is shown in 3D.** The editor's main view now draws the bobbin
  and the whole wire as one continuous copper strand, every turn where the
  lay model put it, with OpenGL and a depth buffer. Orbit, pan and zoom down
  to single wires; playback shows the newest turns and the guide with its
  free span. The dot cross-section is now a secondary view.
- **Winding model rebuilt to match real machines** (see
  `docs/WINDING_MODEL.md`). Patterns are now set by turns per pass, as
  geared, cam and CNC pickup winders are: `layer` (perfect layers, with a
  step zone on the short end and a half-wire climb at each flange), `helix`
  (fixed traverse ratio) and `scatter` (random turns per pass and reversal
  points per pass, seeded). Design files move to version 2; version 1 files
  are converted on load. Python recipes keep their `pass` value.
- The lay follows each turn through 16 stations around the core, with wire
  lag behind the guide (`[lay] free_span_mm`). Wire settles into grooves on
  the short ends and spans straight across the long sides. Crossed helices
  land at the published 77-79 % fill and perfect layers at about 90 %.
- Self-capacitance is now worked out from which turns touch which, and
  inductance from the laid build. The app and `winder sim` show these
  alongside long-side and short-end build and the fill factor.
- `winder machine` lists GRBL acceleration and junction settings, so the
  spindle keeps its speed through the guide's per-turn steps.

### Added
- **Rust port, milestone 2: desktop design editor (`winder-app`).** An egui
  app that edits a design and redraws it as you type: cross-section of every
  turn (by turn order or section, with hover), lay plot, and build profile
  against the bobbin limit, with scrub and 1x-1000x playback. It shows DCR
  from the actual lay next to the layer-model estimate, and exports
  verified G-code. It also opens and saves designs, imports Python recipes
  and loads a machine profile.
- **Cross-section from the toolpath** (`winder_core::lay`). Each turn comes
  to rest in the lowest groove near the guide, so a layer pattern nests
  (agrees with the validated layer model within 2 % on DCR) while scatter,
  dwell and insets change the build and the DCR. Also available as
  `winder preview` (SVG) and in `winder sim`.
- **Rust port, milestone 1: design core, G-code and CLI.** A Cargo workspace
  (`winder-core`, `winder-gcode`, `winder-cli`). Coils are TOML design files:
  bobbin, wire, speed/ramps, and an ordered list of winding sections, each
  with a Layer or seeded Scatter pattern, lay-window insets, dwell turns at
  reversals and optional stop points (e.g. for a tap). The coil physics is
  ported from `simulate_coil` and pinned to its validated numbers. One
  toolpath feeds both the estimate and a GRBL 1.1 post-processor (spindle =
  X in turns, traverse = Y in mm, `G93` inverse-time feed, `G38.2` traverse
  homing); every program is read back and verified. `winder import-recipes`
  converts the Python app's recipes and lists what could not carry over.

### Changed
- **DCR simulator rebuilt on real winding physics (layer-by-layer).** The
  previous model used a single average turn length, which got one DCR
  number right but the winding behavior wrong. The new `simulate_coil`
  follows actual coil engineering: turns per layer = channel height / wire
  OD, layers pack radially at OD x 0.866 (hexagonal close packing), and
  each successive layer sits at a larger perimeter so its turns are
  longer — total wire length is summed layer by layer. Validated against
  published pickups across multiple turn counts and pickup shapes
  simultaneously (DCR, wire length, and layer count all match): Strat
  16x46mm ch11mm gives 7500t->5.4k, 8000t->5.8k, 10866t->8.1k/71 layers/
  4904ft; PAF 16x52mm ch6.5mm gives 5250t->4.2k/coil. Inputs are now
  channel height + core footprint (the packing factor is gone — hex
  packing is built in). The live coil visualizer uses the same engine.

### Added
- **Wire tension, temperature, and proper inductance/RLC resonance.** Three
  real-physics additions. Wire tension (g) now drives how tightly turns pack
  (replacing a manual guess) and raises a break-risk warning as tension nears
  the wire's breaking strength (~66 g for 42 AWG) \u2014 matching the real 30-50 g
  practice. Measurement temperature applies copper's +0.393%/\u00b0C coefficient so
  predicted DCR matches a meter at the actual room/coil temperature. Inductance
  is computed properly (Wheeler multilayer, calibrated so a Strat-geometry
  8000-turn coil lands at ~2.2 H) and combined with self-capacitance into the
  real RLC resonant peak (~7 kHz for a Strat) and Q factor \u2014 the quantitative
  bright/dark character, which now makes the scatter parameter measurable. Base
  DCR validation (pack 1, scatter 0, 20\u00b0C) still matches published pickups
  exactly.
- **Real machine model grounded in how actual winders work.** Researched
  production and DIY pickup winders (orthocyclic/perfect-layer winding, traverse
  coupling, scatter randomizing) and rebuilt the motion around it. New real
  parameters: traverse lead (mm/rev, couples spindle to traverse so ~1 turn =
  1 wire width), turns-per-layer scatter range (\u00b1, the hand/scatter-wind
  randomizer), wind direction (CW/CCW), and ramp up/down (soft start/stop to
  protect the wire). A single `machine_state(turns)` function is now the one
  source of truth for spindle angle, traverse position, layer and direction, so
  the spindle, carriage, wire feed and copper can never drift apart.
- **Fixed spindle motion (no more teleport) + RPM-paced animation.** The spindle
  now rotates by the fractional turn (smoothly, during animation) instead of
  jumping; the animation advances fractional turns per frame paced by RPM.
- **Scrub controls + debug overlay.** Pause, Reset, and \u2192Target buttons, a live
  scrub readout, and a Debug toggle that overlays the raw machine state (layer,
  in-layer, turns/layer, spindle angle, build, DCR, capacitance, resonance).
- **Pack factor and scatter parameters with a real model.** Two new winding
  inputs that affect the physics, not just the picture. Pack factor (0.6-1.0)
  sets how tightly turns sit: a looser pack gives a taller build, more layers,
  and higher DCR for the same turns. Scatter amount (0-100) models scatter
  winding: it loosens the lay slightly AND lowers self-capacitance, raising the
  resonant peak (the brighter, more open vintage tone). The estimate now reports
  self-capacitance (pF) and resonant peak (kHz); a scatter seed gives a
  repeatable irregular pattern. Base model (pack 1.0, scatter 0) still validates
  exactly against published pickups.
- **Parameter-driven coil visualizer (front view).** The live visualizer now
  renders the bobbin face from the chosen parameters: each turn is a
  horizontal line (the wire wrapping the core), turns fill the channel height
  to complete a layer, and successive layers alternate winding direction
  (bottom-to-top, then top-to-bottom) and stack outward toward the viewer.
  Shows bobbin dimensions (length and channel height) and a live layer
  counter (current / total). Editing any geometry, wire gauge, or turn count
  updates the drawing immediately. "Simulate wind" runs the scrub all the way
  to a full bobbin build.
- **Live coil visualizer.** Side-view cross-section showing the guide
  traversing the channel and layers stacking outward from the core, driven
  live by the Uno's turn count during a real wind, plus a "Simulate wind"
  button and scrub slider for offline use. Same layer-by-layer engine as
  the DCR simulator, so picture and numbers agree.
- **Traverse calibration tools.** App has a Calibration section: jog the
  carriage a known distance, enter the measured travel, and it computes
  and applies the corrected lead-screw value — then saves it to the
  Uno's EEPROM so it survives power-off. Firmware adds `JOG`,
  `SETLEADSCREW`, `SAVECAL`, `GETCAL`; calibration loads on startup.
- `docs/BUILD_GUIDE.md` — full physical assembly guide (baseboard,
  spindle, traverse, wire path, electronics, first-run checklist,
  troubleshooting).
- `docs/HARDWARE.md` — CNC Shield v3 wiring option, parts list, and
  lei budget (~400 lei total).

### Planned
- Optional layer-based traverse mode (turns-per-layer instead of per-pass)
- Live DC-resistance entry to refine recipe estimates over time

## [1.0.0] - 2026-06-14

First working release: customizable desktop control with an Arduino Uno
handling real-time motion and turn counting.

### Added
- **Desktop app** (`app/winder_control.py`), cross-platform (macOS / Windows / Linux):
  - Serial connection with port auto-detection and connect/disconnect.
  - Recipe system: save, load, update, delete; persisted to `winder_recipes.json`.
    Ships with four seed recipes (vintage SC, hot SC, PAF, hot humbucker).
  - Full parameter control: turns, bobbin width, RPM, turns-per-pass,
    scatter jitter, bobbin diameter, wire gauge.
  - Live readout: turn counter, state, progress bar, console.
  - Estimated DC resistance and wire length from turns + gauge.
  - Wind logging to `winder_log.csv`.
- **Uno firmware** (`firmware/winder_firmware.ino`):
  - Serial command executor at 115200 baud (`SET`, `START`, `STOP`,
    `PAUSE`, `ZERO`, `PING`).
  - Coordinated spindle + traverse stepping with exact turn counting.
  - Scatter-wind jitter for hand-wound character.
  - Responsive to STOP/PAUSE mid-turn.
- **Docs**: `HARDWARE.md` (wiring, parts, tension/potting tips),
  `PROTOCOL.md` (serial command reference).
- Repo scaffolding: `README.md`, `requirements.txt`, `.gitignore`.

[Unreleased]: https://github.com/YOUR_USERNAME/pickup-winder/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/YOUR_USERNAME/pickup-winder/releases/tag/v1.0.0
