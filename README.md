# Pickup Winder

A CNC guitar pickup winder controlled from your computer.

An Arduino Uno does the precise real-time work (stepping the motors,
counting every turn). A desktop app on your Mac or PC handles the
customizable side: recipes, wind parameters, scatter control, live
monitoring, and a build log. The two talk over the USB cable.

```
  [ Mac / Windows ]            [ Arduino Uno ]            [ Winder ]
  winder_control.py  <--USB-->  firmware  ---STEP/DIR-->  2x stepper
  (recipes, GUI)      serial    (motion+count)            motors
```

## What's in here

```
pickup-winder/
├── app/
│   └── winder_control.py     desktop control app (Python)
├── firmware/
│   └── winder_firmware.ino   Arduino Uno sketch
├── docs/
│   ├── HARDWARE.md           wiring + parts + budget
│   ├── BUILD_GUIDE.md        physical assembly
│   └── PROTOCOL.md           serial command reference
├── CHANGELOG.md
├── requirements.txt
└── README.md
```

## Quick start

### 1. Get the code
```bash
git clone https://github.com/YOUR_USERNAME/pickup-winder.git
cd pickup-winder
```

### 2. Flash the Uno
1. Open `firmware/winder_firmware.ino` in the Arduino IDE.
2. Install the **AccelStepper** library (Tools → Manage Libraries).
3. Select board "Arduino Uno" and the right port, click Upload.

### 3. Run the app
You need Python 3 (from python.org). Then:

```bash
pip install -r requirements.txt
python app/winder_control.py
```

On macOS you may need `python3` / `pip3` instead of `python` / `pip`.

In the app: pick the Uno's serial port, click **Connect**, load or
build a recipe, click **START**.

## Rust port (in progress)

The program is being rebuilt in Rust as a small CAD/CAM tool: you design
the coil (bobbin, wire, and a stack of winding sections that say how each
layer is laid), it computes one toolpath, and that toolpath drives the
estimates, the preview and the G-code. The Uno will run stock **GRBL 1.1**
on the same CNC Shield instead of the custom firmware.

```
crates/
  winder-core    design files, wire table, coil physics, lay patterns,
                 toolpath, and the cross-section built from it
  winder-gcode   GRBL post-processor and a verifier that reads the G-code back
  winder-cli     the `winder` command
  winder-app     the desktop design editor
designs/         example designs
```

### Design editor

```bash
cargo run --release -p winder-app -- designs/strat-vintage.toml
```

Edit the bobbin, wire, speed and the stack of winding sections on the left.
The middle shows the result, recomputed as you type:

- **Cross-section**: every turn where it lands, cut through the middle of a
  long side or through a short end, coloured by turn order or by section.
  Hover a wire to see its turn number. Wire is pressed into its grooves on
  the short ends and spans straight across the long sides, so crossed
  patterns bulge there, as real coils do.
- **Lay plot**: guide position against turn number, and where the wire
  actually landed (it lags the guide), with section bands and stop points.
- **Build profile**: the top of the coil across the channel, against the
  bobbin's build limit.

Sections are set the way real winders are, by **turns per pass**: `layer`
(perfect layers, the guide steps one wire per turn), `helix` (a fixed
traverse ratio, like a geared or cam machine) and `scatter` (a new turns per
pass and reversal point for every pass). See
[docs/WINDING_MODEL.md](docs/WINDING_MODEL.md) for how the lay is worked
out and what it was checked against.

Scrub or play the wind (1× to 1000× real speed). The right panel shows what
the laid coil measures: DCR, wire length, build on the long side and short
end, fill factor, inductance, self-capacitance, resonance and Q, plus the
layer model's DCR as a cross-check, and run time. **Export G-code** bakes the
program, reads it back to check it, then saves it. The File menu opens and
saves designs (Ctrl+S), imports the Python app's recipes and loads a machine
profile.

On Linux the app needs `libxkbcommon-x11` (X11) or a Wayland session.

### Command line

```bash
cargo run -p winder-cli -- new strat -o my-strat.toml      # starter design
cargo run -p winder-cli -- sim designs/strat-vintage.toml  # DCR, L, resonance, toolpath summary
cargo run -p winder-cli -- preview designs/strat-vintage.toml  # cross-section as SVG
cargo run -p winder-cli -- gcode designs/strat-vintage.toml -o strat.nc
cargo run -p winder-cli -- import-recipes app/winder_recipes.json -o designs/imported
cargo run -p winder-cli -- machine                         # machine profile + GRBL settings
cargo test
```

How the G-code drives the machine: X is the spindle measured in turns
(`$100` = steps per revolution, 1600 at 1/8 step), Y is the traverse in mm
(`$101` = 200 for a T8 with 8 mm lead). Every `G1` line lays at most one
turn (a perfect layer uses two: the step, then the hold), and `G93`
inverse-time feed makes each line's duration, and so the
RPM, exact. The program finds the traverse switch with `G38.2` on GRBL's
probe input, then measures from there (`flange_offset_mm` in the machine
profile). Every generated file is parsed back and checked: turn count,
guide inside the lay window, stops, and run time.

The Python app and the Arduino sketch below stay as they are until the
Rust app can drive the machine.

## Notes
- The app must run locally — it needs USB access to the Uno, so it
  can't run "in the cloud."
- Resistance estimates are approximate. Measure the finished coil with
  a multimeter and log the real value to build reliable recipes.
- Mechanical constants (microstep, lead-screw pitch) live at the top of
  the firmware. Adjust them to match your hardware.

See `docs/HARDWARE.md` for wiring and `docs/PROTOCOL.md` for the command set.
Physical assembly is in `docs/BUILD_GUIDE.md`.
Version history is in `CHANGELOG.md`.
