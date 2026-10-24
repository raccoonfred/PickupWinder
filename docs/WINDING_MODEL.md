# How the winding model works

This is how the Rust app decides where each turn of wire goes, and why. It
follows how real pickup winders behave. The first version of the model did
not.

## What was wrong before

The first model set every pattern as a pitch in wire diameters per turn. It
then cut the coil at a single place and let every turn roll into the lowest
groove within half a wire of the guide. That makes any pattern pack almost
perfectly. Wire that crosses at an angle can't do that, so the old model
showed a much tidier, denser coil than a real machine winds. It also had no
idea that a pickup bobbin is a long, narrow rectangle.

## How real machines lay wire

- **Turns per pass (TPL).** A winder is set by how many turns the spindle
  makes while the guide crosses the window once. On geared and cam machines,
  such as the Leesona 102 Gibson used for PAFs, that ratio is the gear set.
  Pickup makers quote 50 to 200 TPL: humbuckers 50 to 100 (Burstbuckers are
  reported at 64), Strats up to about 200. The pitch follows from it:
  window ÷ TPL. Below about 155 TPL on a Strat (91 on a PAF), the pitch is
  wider than one wire. Each pass then crosses the one below in a helix
  instead of lying in its grooves.
- **Perfect layers are a special motion.** Orthocyclic winding, with each
  layer in the grooves of the one below, needs the guide to hold still for
  most of the turn and step one wire in a short step zone. On a rectangular
  form the step goes on one side (US 3,989,200). At each flange the wire
  climbs half a wire, and the next layer has the same turn count, offset by
  half a wire. The best possible fill is 90.7 %.
- **Crossed winding packs looser.** Published fill factors are about 79 %
  for helical, 73 to 80 % for wild winding and 90.7 % for orthocyclic. At a
  crossing the upper wire rides on top of the lower one for a long stretch,
  because the crossing angle is shallow.
- **Wire lags the guide.** The wire leaves the guide's eye and meets the
  coil a free span later. For every millimetre wound, the lay point closes
  1/span of the gap. Reversals round off and steps land late, so the guide
  offset matters for perfect layers.
- **A pickup bobbin is not round.** The wire is pressed onto the coil only
  where it wraps the short ends. Across the long sides it runs straight from
  one contact to the next, so the coil bulges there.
- **Scatter** is irregular traverse speed and reversal points: hand guiding
  on a Schatten-style machine, or a new pitch per layer on a CNC machine
  (as in the volfovich firmware).

## What the model does now

### Patterns (design file version 2)

| kind | what the guide does | set by |
| --- | --- | --- |
| `layer` | Holds still, then steps one wire in the step zone each turn. At a flange it climbs half a wire, and the next layer comes back in the grooves. | `step_fraction`. The default is the first short end, which is the turn's start. |
| `helix` | Moves steadily and reverses at the window edges, like a geared or cam machine. | `tpl` |
| `scatter` | Each pass draws its own TPL and its own reversal points from a seeded stream, so a wind can be repeated. | `tpl_min`, `tpl_max`, `reversal_jitter_mm`, `seed` |

Version 1 files still open. Their pitch is converted to the TPL that gives
the same guide motion over that section's window.

Recipes imported from the Python app now keep their `pass` value, because
that was turns per pass all along.

### The lay (`winder_core::lay`)

Each turn is followed through 16 stations around the core, and each station
keeps its own cross-section.

1. The guide position comes from the toolpath at that station's moment in
   the turn.
2. **Lag:** the lay point moves toward the guide by `1 − e^(−ds/span)`,
   where `ds` is the wire wound since the last station and `span` is
   `[lay] free_span_mm`. The default is 25 mm.
3. **Short-end stations:** the wire rolls into the lowest groove within half
   a wire.
4. **Long-side stations:** the wire runs straight between the two end
   contacts on either side, and can only be nudged a quarter wire.
5. A neighbour touching only the wire's flank (more than 0.95 OD off to the
   side) pushes the wire but doesn't hold it up. Without this rule, a pitch a
   hair under one wire would climb the previous turn forever.

The quarter-wire nudge is the one fitted constant. It is set so that crossed
helices land in the published 73 to 80 % fill range. Perfect layers come out
at about 90 % with no tuning.

### What comes out of it

- **DCR and wire length** from every turn's actual height all the way round.
- **Build** mid long side (where the cover goes), mid short end, and the
  deepest point anywhere.
- **Fill factor.**
- **Self-capacitance** from which turns touch which. With the voltage rising
  evenly from turn 1 to turn N, two touching wires Δn turns apart store
  energy in proportion to (Δn/N)². Summing that energy over every
  neighbouring pair at every station gives C = Σ c·ℓ·(Δn/N)². Here `c` is
  the capacitance per length of two parallel enamelled wires (effective
  permittivity 2.5), and only wires within 1.2 OD of each other count.
- **Inductance** from Wheeler's formula on the laid coil's mean build, using
  the same calibration as before. **Resonance** and **Q** follow from it.

The old layer model is still shown as a cross-check. It assumes perfect
layers.

## Validation

Run `cargo run --release -p winder-core --example patterns` to print this
table. These are 42 AWG coils with a 25 mm free span:

| Strat, 8000 turns | kΩ | build, long side (mm) | build, short end (mm) | fill | C (pF) | unloaded peak (kHz) |
| --- | --- | --- | --- | --- | --- | --- |
| perfect layer | 5.84 | 3.26 | 3.27 | 90 % | 129 | 9.5 |
| helix 123 TPL | 5.89 | 3.81 | 3.66 | 79 % | 83 | 11.8 |
| helix 92 TPL | 5.91 | 4.87 | 3.42 | 77 % | 84 | 11.7 |
| helix 62 TPL | 6.07 | 5.98 | 4.47 | 59 % | 69 | 12.9 |
| scatter 62-139 TPL | 5.98 | 4.87 | 4.17 | 68 % | 134 | 9.3 |
| `designs/strat-vintage.toml` | 5.95 | 4.54 | 4.12 | 70 % | 137 | 9.2, at 2.19 H |

What this agrees with:

- **Fill factors** match the published ranges: about 90 % for perfect
  layers and 77 to 79 % for crossed helices.
- **Perfect layers** match the old layer model within 1 % on DCR. The old
  model was validated against published Strat and PAF coils.
- **The vintage Strat design** gives about 5.9 kΩ, 2.2 H and 140 pF, with an
  unloaded peak near 9 kHz. That is in the range people measure on
  vintage-style Strat pickups.
- **Lower TPL lowers capacitance** in the model, as winders report.
  Crossing passes put turns that are close in number next to each other.

What it does not settle:

- **Scatter and capacitance.** Scatter is widely said to lower capacitance.
  In this model, a wide random TPL range packs looser but lets wires fall
  through to passes further back. Those wires are many turns apart, so C
  ends up about the same as a perfect layer, not lower. Forum threads assert
  the effect but publish no measurements. Measure your own coils to settle
  it.
- **Absolute L and C** carry the old calibration, so treat them as
  estimates until you log measured coils.

## Machine consequences

A perfect layer starts and stops the guide every turn. GRBL's default
acceleration of 10 units/s² would slow the spindle to about 100 RPM at
each of those corners. `winder machine` now lists `$120=200`, `$121=200`
and `$11=0.02`, which keep the junction speed above the winding speed.

## Sources

- Schatten Design, WinderTraverse manual. The traverse is independent of
  the spindle, set by knobs, and scatter comes from varying the speed.
- Music Electronics Forum threads "Turns per layer", "Leesona 102" and "man
  vs machine distributed capacitance".
- Guitar-list, "Pickup winding mechanism".
- Schuyler Dean, on scatter winding.
- Wikipedia, "Coil winding technology". Covers orthocyclic, helical and wild
  winding, their fill factors, and the step zone.
- US 3,989,200, perfect-layer winding on non-circular forms: the crossover
  side, the guide holding still and the half-wire offset.
- US 10,569,987 B2: wire slackening at reversals, and peaks and valleys at
  the flanges.
- volfovich/pickup-winder, `TRAVERSE.md`: per-layer pitch scatter and end
  scatter.
