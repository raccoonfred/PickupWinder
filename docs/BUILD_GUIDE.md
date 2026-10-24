# Build Guide — Physical Assembly

This is the workbench guide for building the winder's physical machine.
Pair it with `HARDWARE.md` (wiring) and the firmware/app in the repo.

The machine has four functional parts on one baseboard:
1. **Spindle** — spins the bobbin and counts turns.
2. **Traverse** — moves the wire guide side to side so wire lays evenly.
3. **Wire path** — spool, tensioner, guide eyelet.
4. **Electronics** — Uno + CNC shield + PSU in a box.

Build order below is the order that causes the fewest do-overs.

---

## Tools you'll need
- Drill + bits, screwdrivers, hex keys
- Hot glue gun and/or two-part epoxy
- Calipers or a ruler, a square (alignment matters here)
- Soldering iron (for motor/power leads if not pre-terminated)
- Multimeter (for setting driver Vref and measuring finished coils)

---

## 1. The baseboard

Start with a rigid flat base — a piece of 12–18mm plywood or MDF,
roughly 40 x 25 cm. Everything mounts to this. Rigidity matters: any
flex while winding shows up as uneven tension.

- Sand the top, optionally seal it so wire offcuts brush off easily.
- Draw a centerline down the long axis. The spindle axis and the
  traverse lead screw both run parallel to this line.

---

## 2. Spindle assembly

This is the rotating side that holds and spins the pickup bobbin.

**Mount the spindle motor.**
- Use an L-bracket (aluminium or steel) bolted to the baseboard.
- The motor shaft must sit at a fixed, comfortable working height —
  about 6–8 cm above the board is typical. If your bracket raises it,
  raise the traverse to match (see step 3).
- Bolt the motor to the bracket through its four front holes (M3).

**Build the chuck / faceplate.**
The chuck is what grips the pickup so it spins true.
- Cut a disc of plywood or acrylic ~50–60mm diameter. This is the
  faceplate.
- Bore the center to fit a 5mm shaft coupler or bolt it to a coupler hub.
- On the face, mount a simple clamp: two pieces of wood/acrylic with a
  thumbscrew, or a foam-faced plate that presses the bobbin against the
  faceplate. The pickup's flatwork bobbin sits between them.
- The clamp must hold the bobbin **centered and square** — if it wobbles,
  the coil winds unevenly. Spin it by hand and check for runout before
  going further.

**Couple it to the motor.**
- 5mm-to-5mm flexible coupler from the motor shaft to the faceplate hub.
- A flexible coupler tolerates tiny misalignment; a rigid one does not.

---

## 3. Traverse assembly

This moves the wire guide back and forth so the wire doesn't pile up in
one spot. It runs **parallel** to the spindle axis.

**Lay out the rail.**
- Two parallel elements: a **T8 lead screw** (driven) and a smooth
  **8mm guide rod** (just for support). Space them ~40–50mm apart.
- Mount both on small end-blocks (3D-printed, wood, or metal angle) at
  each end of the board. The lead screw needs a bearing or bushing at the
  far end so it spins freely; the near end couples to the motor.

**Mount the traverse motor.**
- Second NEMA 17, same bracket style, at one end of the rail.
- Couple its shaft to the lead screw with a 5mm-to-8mm flexible coupler.

**Build the carriage.**
- A small block that rides the guide rod on an LM8UU linear bearing and
  threads onto the lead screw via the brass T8 nut.
- As the screw turns, the carriage slides. That's the whole mechanism.
- On top of the carriage, mount a vertical post holding the **ceramic
  wire-guide eyelet**.

**Critical alignment.**
- The eyelet must sit at the **same height as the center of the bobbin**,
  and the carriage's travel must be **parallel to the bobbin face**.
- If the guide is too high/low or angled, wire piles toward one flange.
- The traverse window in software (`bobbin width`) only works if the
  mechanical travel is straight and centered.

---

## 4. Wire path

**Spool holder.**
- Mount the magnet-wire spool on a vertical post (a dowel or bolt) so it
  unspools freely without tipping. Position it behind/above the tensioner.

**Tensioner.**
- The most important non-motor part. Too loose = sloppy coil; too tight =
  snapped 42 AWG wire and a dead pickup.
- Simplest reliable design: two felt pads the wire passes between, with a
  screw that adjusts how hard they pinch. A spring-loaded version is even
  better.
- Route: spool → tensioner → guide eyelet on the carriage → bobbin.

**Thread test.**
- Before any motor runs, hand-feed wire along the full path. It should
  pull smoothly with light, even resistance, no snags.

---

## 5. Electronics

Put the Uno + CNC shield + power supply in a small box (project box,
3D-printed enclosure, or even a sturdy plastic container) mounted at one
corner of the board, away from the wire path so stray wire can't reach it.

- Wire per `HARDWARE.md`: drivers in X (spindle) and Y (traverse) slots,
  motors to the X/Y screw terminals, 12V to the power terminal.
- Set each driver's Vref **before** powering motors.
- Run the USB cable out to your computer.
- Strain-relief the cables so tugging the USB doesn't move the board.

---

## 6. First-run checklist

Before winding a real pickup, dry-run with no wire:

1. Flash firmware, open the app, connect.
2. Set a small test: 50 turns, your real bobbin width, low RPM (~150).
3. Hit START. Watch:
   - Spindle turns the right way and counts up.
   - Traverse moves across the full width and **reverses at both ends**.
   - Carriage travel stays parallel and centered on the bobbin.
4. STOP. Adjust the `bobbin width` and carriage end-blocks until the
   traverse exactly covers the bobbin's wind window, no more.
5. Increase RPM gradually and confirm nothing vibrates loose.

Only then thread wire and wind a real coil — start at low RPM, watch the
first layer lay down evenly, then let it run.

---

## 7. Finishing a wound pickup

- Measure DC resistance across the coil with a multimeter and log it in
  the app. This is how you build reliable recipes.
- Pot the coil in wax (~65°C paraffin + beeswax) to kill microphonic
  squeal — important for high-gain use. Never overheat; keep it gentle.

---

## Common problems

| Symptom | Likely cause |
|---|---|
| Wire piles toward one flange | Guide eyelet not centered / travel not parallel |
| Wire snaps mid-wind | Tension too high, or a snag in the path |
| Loose, sloppy coil | Tension too low |
| Turn count drifts | RPM too high for the wire; slow down |
| Driver gets very hot / motor stutters | Vref set wrong — re-set it |
| Coil squeals through amp | Needs wax potting |
