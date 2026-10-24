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
