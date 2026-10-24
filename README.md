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

Milestone 1 (this workspace) is the core and a command-line tool:

```
crates/
  winder-core    design files, wire table, coil physics, lay patterns, toolpath
  winder-gcode   GRBL post-processor and a verifier that reads the G-code back
  winder-cli     the `winder` command
designs/         example designs
```

```bash
cargo run -p winder-cli -- new strat -o my-strat.toml      # starter design
cargo run -p winder-cli -- sim designs/strat-vintage.toml  # DCR, L, resonance, toolpath summary
cargo run -p winder-cli -- gcode designs/strat-vintage.toml -o strat.nc
cargo run -p winder-cli -- import-recipes app/winder_recipes.json -o designs/imported
cargo run -p winder-cli -- machine                         # machine profile + GRBL settings
cargo test
```

How the G-code drives the machine: X is the spindle measured in turns
(`$100` = steps per revolution, 1600 at 1/8 step), Y is the traverse in mm
(`$101` = 200 for a T8 with 8 mm lead). Every `G1` line lays one turn as a
helix, and `G93` inverse-time feed makes each line's duration, and so the
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
