#!/usr/bin/env python3
"""
Pickup Winder Control
=====================
Cross-platform desktop app (macOS / Windows / Linux) for controlling
an Arduino-based CNC guitar pickup winder.

Talks to the Uno over USB serial. The Uno handles real-time motion;
this app handles recipes, parameters, live monitoring, and logging.

SETUP
-----
1. Install Python 3 (python.org). tkinter is included on Win/macOS.
2. Install the serial library:
       pip install pyserial
3. Plug in the Uno, run:
       python winder_control.py
4. Pick the serial port from the dropdown and click Connect.

Recipes are saved to  winder_recipes.json  next to this file.
Wind history is appended to  winder_log.csv.
"""

import json
import os
import csv
import threading
import time
from datetime import datetime
import tkinter as tk
from tkinter import ttk, messagebox

try:
    import serial
    import serial.tools.list_ports
    HAVE_SERIAL = True
except ImportError:
    HAVE_SERIAL = False

BAUD = 115200
HERE = os.path.dirname(os.path.abspath(__file__))
RECIPE_FILE = os.path.join(HERE, "winder_recipes.json")
LOG_FILE = os.path.join(HERE, "winder_log.csv")

# Magnet wire properties for the DCR simulator.
# ohm_per_m: DC resistance of the bare copper conductor, derived from
#   copper resistivity (1.724e-8 ohm.m at 20C) and the nominal bare
#   diameter for each AWG.
# od_mm: nominal insulated (single-build) outer diameter, used to model
#   coil buildup so the average turn length grows with turn count.
WIRE_PROPS = {
    "42 AWG": {"ohm_per_m": 5.44, "od_mm": 0.071},
    "43 AWG": {"ohm_per_m": 6.85, "od_mm": 0.064},
    "44 AWG": {"ohm_per_m": 8.61, "od_mm": 0.058},
    "Other":  {"ohm_per_m": 5.44, "od_mm": 0.071},
}


def simulate_coil(turns, win_h, core_w, core_l, wire_props,
                  pack=1.0, scatter=0.0, tension_g=None, temp_c=20.0):
    """Layer-by-layer coil physics with pack factor, scatter, wire tension and
    temperature.

    Returns dict: dcr_k, length_m, layers, build_mm, turns_per_layer,
                  cap_pf, resonance_khz, eff_pitch_factor, inductance_h,
                  q_factor, break_risk, eff_pack, temp_c.

    Base model (validated): turns per layer = channel height / wire OD; layers
    pack radially at OD*sqrt(3)/2 (hex); each layer is longer; DCR = length*ohm.

    pack (0.6-1.0): how tightly turns pack into each layer. 1.0 = ideal hex.

    scatter (0-100): controlled wind irregularity. Loosens the lay slightly AND
      lowers self-capacitance, raising the resonant peak (the bright vintage
      tone).

    tension_g: wire tension in grams. When given, it DRIVES the effective pack
      (more tension -> tighter pack, slightly compressed enamel -> a denser,
      smaller coil with marginally higher turns/layer) up to a sensible limit,
      and sets a break-risk indicator (tension approaching the wire's breaking
      strength). If None, the manual `pack` is used directly.

    temp_c: coil temperature for the DCR estimate. Copper's resistivity rises
      ~0.393%/degC, so a coil measured warm reads higher than at 20C. This makes
      the predicted DCR match a meter at the actual measurement temperature.

    Validated (scatter=0, pack=1, 20C): Strat 16x46 ch11 7500t->5.4k,
      10866t->8.1k/71 layers; PAF 16x52 ch6.5 5250t->4.2k.
    """
    import math
    od = wire_props["od_mm"]
    ohm = wire_props["ohm_per_m"]
    empty = {"dcr_k": 0.0, "length_m": 0.0, "layers": 0, "build_mm": 0.0,
             "turns_per_layer": 0, "cap_pf": 0.0, "resonance_khz": 0.0,
             "eff_pitch_factor": 1.0, "inductance_h": 0.0, "q_factor": 0.0,
             "break_risk": 0.0, "eff_pack": pack, "temp_c": temp_c}
    if turns <= 0 or win_h <= 0 or od <= 0:
        return empty

    s = max(0.0, min(100.0, scatter)) / 100.0

    # ---- wire tension -> effective pack + break risk ----
    # Bare-copper diameter (enamel build is ~10-15% of OD on fine magnet wire).
    cu_d = od * 0.86
    break_risk = 0.0
    if tension_g is not None and tension_g > 0:
        # Breaking strength of soft copper ~ 220 MPa. Force at break (grams-force)
        # = 220e6 * area / 9.81. For 42 AWG (~0.064mm Cu) that's ~70 g, matching
        # the real "wind 42 AWG under ~30-50 g" practice.
        area_m2_wire = math.pi * (cu_d / 2 * 1e-3) ** 2
        break_force_g = 220e6 * area_m2_wire / 9.81 * 1000.0
        break_risk = max(0.0, min(1.0, tension_g / max(break_force_g, 1e-6)))
        # tension tightens the pack: 0 g -> loose (0.80), high (near break) ->
        # tight (~0.98), with mild enamel compression at the top end.
        eff_pack = 0.80 + 0.18 * min(1.0, tension_g / (0.6 * break_force_g))
        eff_pack = max(0.6, min(0.99, eff_pack))
    else:
        eff_pack = max(0.5, min(1.0, pack))

    # ---- geometry: layers, length, build ----
    loosen = (1.0 / eff_pack) * (1.0 + 0.12 * s)
    tpl = max(1.0, (win_h / od) / loosen)
    pitch = od * math.sqrt(3) / 2.0 * loosen

    base_perim = 2 * core_w + 2 * core_l
    remaining = turns
    layer = 0
    length_mm = 0.0
    while remaining > 0 and layer < 500000:
        n = min(tpl, remaining)
        b = layer * pitch
        perim = base_perim + 2 * math.pi * (b + pitch / 2.0)
        length_mm += n * perim
        remaining -= n
        layer += 1
    length_m = length_mm / 1000.0

    # ---- DCR with temperature coefficient of copper ----
    alpha = 0.00393                                  # per degC, annealed copper
    temp_factor = 1.0 + alpha * (temp_c - 20.0)
    dcr_k = length_m * ohm / 1000.0 * temp_factor
    build_mm = layer * pitch

    # ---- proper inductance (multilayer air-cored coil, Wheeler) ----
    # Mean coil radius and the rectangular cross-section of the winding pack.
    mean_perim_mm = base_perim + math.pi * build_mm
    a_mm = mean_perim_mm / (2 * math.pi)             # equivalent mean radius
    a_in = a_mm / 25.4
    b_in = win_h / 25.4                              # coil "length" (axial = win_h)
    c_in = build_mm / 25.4                           # winding depth (radial build)
    # Wheeler's multilayer formula (microhenries), N total turns:
    #   L(uH) = 0.8 * a^2 * N^2 / (6a + 9b + 10c)   [a,b,c in inches]
    denom = (6 * a_in + 9 * b_in + 10 * c_in)
    induct_uh = (0.8 * a_in ** 2 * turns ** 2) / denom if denom > 0 else 0.0
    # Wheeler assumes an air core. A real single-coil's open magnetic path and
    # the steel pole pieces give a LOWER effective inductance than the air-core
    # formula predicts; a 0.62 calibration lands a Strat-geometry 8000t coil at
    # ~2.2 H, matching published measurements. (Pole-piece/magnet effects on a
    # finished, mounted pickup shift this further; this is the bare-coil value.)
    induct_uh *= 0.62
    inductance_h = induct_uh * 1e-6

    # ---- self-capacitance (lumped, lowered by scatter) ----
    cap_pf = (turns * mean_perim_mm) * 2.2e-4 * (1.0 - 0.35 * s)
    cap_pf = max(1.0, cap_pf)

    # ---- RLC resonant peak and Q ----
    if inductance_h > 0 and cap_pf > 0:
        res_hz = 1.0 / (2 * math.pi * math.sqrt(inductance_h * cap_pf * 1e-12))
        # Q of the resonant peak: Q = (1/R) * sqrt(L/C). R is the coil DCR (ohms).
        R = max(1.0, dcr_k * 1000.0)
        q_factor = (1.0 / R) * math.sqrt(inductance_h / (cap_pf * 1e-12))
    else:
        res_hz, q_factor = 0.0, 0.0
    resonance_khz = res_hz / 1000.0

    return {
        "dcr_k": dcr_k,
        "length_m": length_m,
        "layers": layer,
        "build_mm": build_mm,
        "turns_per_layer": tpl,
        "cap_pf": cap_pf,
        "resonance_khz": resonance_khz,
        "eff_pitch_factor": loosen,
        "inductance_h": inductance_h,
        "q_factor": q_factor,
        "break_risk": break_risk,
        "eff_pack": eff_pack,
        "temp_c": temp_c,
    }


# Coil geometry presets for the layer-by-layer DCR simulator. Three real,
# measurable numbers define a coil — a single-coil and a humbucker are
# different SHAPES, not just sizes:
#   win_h  = winding channel height between the flanges (mm). The guide
#            traverses this; it sets turns per layer. Strat ~11, PAF ~6.5.
#   win_d  = radial depth available before the bobbin is full (mm)
#   core_w x core_l = inner footprint (mm) the wire wraps (an oval)
# Validated against published pickups (alexkenis, planetz, mylespaul):
#   Strat 16x46 ch11: 7500t->5.4k, 8000t->5.8k, 10866t->8.1k/71 layers
#   PAF   16x52 ch6.5: 5250t->4.2k/coil
COIL_PRESETS = {
    "Strat single-coil":   {"win_h": 11.0, "win_d": 6.0,  "core_w": 16.0, "core_l": 46.0},
    "Tele bridge":         {"win_h": 11.0, "win_d": 6.0,  "core_w": 16.0, "core_l": 48.0},
    "Tele neck":           {"win_h": 10.0, "win_d": 6.0,  "core_w": 14.0, "core_l": 42.0},
    "PAF humbucker (coil)":{"win_h": 6.5,  "win_d": 11.0, "core_w": 16.0, "core_l": 52.0},
    "P-90 (soapbar)":      {"win_h": 8.0,  "win_d": 13.0, "core_w": 18.0, "core_l": 62.0},
}

DEFAULT_RECIPES = {
    "Vintage Single-Coil": {
        "turns": 8000, "width": 11.0, "rpm": 420, "pass": 110,
        "jitter": 12, "wire": "42 AWG",
        "win_h": 11.0, "win_d": 6.0, "core_w": 16.0, "core_l": 46.0,
    },
    "Hot Single-Coil": {
        "turns": 9000, "width": 11.0, "rpm": 420, "pass": 105,
        "jitter": 8, "wire": "42 AWG",
        "win_h": 11.0, "win_d": 6.0, "core_w": 16.0, "core_l": 46.0,
    },
    "PAF Humbucker (per coil)": {
        "turns": 5000, "width": 6.5, "rpm": 450, "pass": 90,
        "jitter": 15, "wire": "42 AWG",
        "win_h": 6.5, "win_d": 11.0, "core_w": 16.0, "core_l": 52.0,
    },
    "Hot Humbucker (per coil)": {
        "turns": 5500, "width": 6.5, "rpm": 450, "pass": 85,
        "jitter": 6, "wire": "43 AWG",
        "win_h": 6.5, "win_d": 11.0, "core_w": 16.0, "core_l": 52.0,
    },
}


class WinderApp:
    def __init__(self, root):
        self.root = root
        root.title("Pickup Winder Control")
        root.geometry("780x820")
        root.minsize(700, 560)

        self.ser = None
        self.reader_thread = None
        self.reading = False
        self.turns_done = 0
        self.state = "DISCONNECTED"

        self.recipes = self.load_recipes()
        self._build_ui()
        self.refresh_ports()
        # Draw once the window has laid out and the canvas has its real width.
        self.root.after(200, self.update_estimate)
        self.root.after(500, self.update_estimate)

    # ---------- persistence ----------
    def load_recipes(self):
        if os.path.exists(RECIPE_FILE):
            try:
                with open(RECIPE_FILE) as f:
                    return json.load(f)
            except Exception:
                pass
        # seed defaults on first run
        with open(RECIPE_FILE, "w") as f:
            json.dump(DEFAULT_RECIPES, f, indent=2)
        return dict(DEFAULT_RECIPES)

    def save_recipes(self):
        with open(RECIPE_FILE, "w") as f:
            json.dump(self.recipes, f, indent=2)

    # ---------- UI ----------
    def _build_ui(self):
        pad = {"padx": 8, "pady": 4}

        # Scrollable container so every section (including the coil visualizer
        # lower down) is reachable regardless of window height.
        outer = tk.Frame(self.root)
        outer.pack(fill="both", expand=True)
        canvas = tk.Canvas(outer, highlightthickness=0)
        vsb = ttk.Scrollbar(outer, orient="vertical", command=canvas.yview)
        canvas.configure(yscrollcommand=vsb.set)
        vsb.pack(side="right", fill="y")
        canvas.pack(side="left", fill="both", expand=True)

        body = ttk.Frame(canvas)
        body_id = canvas.create_window((0, 0), window=body, anchor="nw")

        def _on_body_config(e):
            canvas.configure(scrollregion=canvas.bbox("all"))
        body.bind("<Configure>", _on_body_config)

        def _on_canvas_config(e):
            canvas.itemconfigure(body_id, width=e.width)
        canvas.bind("<Configure>", _on_canvas_config)

        # Mouse-wheel scrolling (cross-platform)
        def _wheel(e):
            delta = -1 * (e.delta // 120) if e.delta else (1 if e.num == 5 else -1)
            canvas.yview_scroll(delta, "units")
        canvas.bind_all("<MouseWheel>", _wheel)
        canvas.bind_all("<Button-4>", _wheel)
        canvas.bind_all("<Button-5>", _wheel)

        # All sections are parented to `body` (the scrollable frame).
        parent = body

        # --- Connection bar ---
        conn = ttk.LabelFrame(parent, text="Connection")
        conn.pack(fill="x", **pad)
        ttk.Label(conn, text="Port:").grid(row=0, column=0, padx=6, pady=6)
        self.port_var = tk.StringVar()
        self.port_combo = ttk.Combobox(conn, textvariable=self.port_var, width=28, state="readonly")
        self.port_combo.grid(row=0, column=1, padx=6)
        ttk.Button(conn, text="Refresh", command=self.refresh_ports).grid(row=0, column=2, padx=6)
        self.connect_btn = ttk.Button(conn, text="Connect", command=self.toggle_connect)
        self.connect_btn.grid(row=0, column=3, padx=6)
        self.conn_lbl = ttk.Label(conn, text="Disconnected", foreground="#b00")
        self.conn_lbl.grid(row=0, column=4, padx=10)

        # --- Recipes ---
        rec = ttk.LabelFrame(parent, text="Recipes")
        rec.pack(fill="x", **pad)
        self.recipe_var = tk.StringVar()
        self.recipe_combo = ttk.Combobox(rec, textvariable=self.recipe_var, width=30, state="readonly")
        self.recipe_combo.grid(row=0, column=0, padx=6, pady=6)
        self.refresh_recipe_list()
        ttk.Button(rec, text="Load", command=self.load_selected_recipe).grid(row=0, column=1, padx=4)
        ttk.Button(rec, text="Save As New", command=self.save_as_recipe).grid(row=0, column=2, padx=4)
        ttk.Button(rec, text="Update", command=self.update_recipe).grid(row=0, column=3, padx=4)
        ttk.Button(rec, text="Delete", command=self.delete_recipe).grid(row=0, column=4, padx=4)

        # --- Parameters (left) + Visualizer (right) side by side ---
        mid = ttk.Frame(parent)
        mid.pack(fill="both", expand=True, **pad)
        mid.columnconfigure(0, weight=0)
        mid.columnconfigure(1, weight=1)
        mid.rowconfigure(0, weight=1)

        par = ttk.LabelFrame(mid, text="Wind Parameters")
        par.grid(row=0, column=0, sticky="nsew", padx=(0, 6))

        self.turns = self._param(par, 0, "Target turns", 8000, 0, 30000)
        self.win_h = self._param(par, 1, "Winding width (mm)", 11.0, 1, 40, is_float=True)
        self.rpm = self._param(par, 2, "Spindle RPM", 420, 50, 1200)
        self.ramp = self._param(par, 3, "Ramp up/down (turns)", 50, 0, 500)
        self.jitter = self._param(par, 4, "Scatter amount (0-100)", 12, 0, 100)

        # Coil-shape preset: fills the geometry fields below
        ttk.Label(par, text="Coil shape:").grid(row=5, column=0, sticky="e", padx=6, pady=4)
        self.coil_preset = tk.StringVar(value="Strat single-coil")
        cp = ttk.Combobox(par, textvariable=self.coil_preset, width=18, state="readonly",
                          values=list(COIL_PRESETS.keys()))
        cp.grid(row=5, column=1, sticky="w", padx=6)
        cp.bind("<<ComboboxSelected>>", self.apply_coil_preset)

        # Real geometry numbers that make the simulator accurate. Note: the
        # winding width (win_h) above is the span the guide traverses between
        # the flanges; turns-per-layer = winding width / wire OD.
        self.core_w = self._param(par, 6, "Core footprint width (mm)", 16.0, 1, 40, is_float=True)
        self.core_l = self._param(par, 7, "Core footprint length (mm)", 46.0, 1, 90, is_float=True)
        self.win_d = self._param(par, 8, "Build depth (mm)", 6.0, 0.5, 40, is_float=True)
        # Machine motion: lead screw and traverse coupling.
        self.lead = self._param(par, 9, "Traverse lead (mm/rev)", 1.0, 0.05, 8.0, is_float=True)
        # Winding character: pack, tension, scatter randomness, per-layer range.
        self.pack = self._param(par, 10, "Pack factor (0.6-1.0)", 0.92, 0.5, 1.0, is_float=True)
        self.tension = self._param(par, 11, "Wire tension (g)", 35, 0, 120)
        self.use_tension = tk.BooleanVar(value=True)
        ttk.Checkbutton(par, text="Tension drives pack",
                        variable=self.use_tension,
                        command=self.update_estimate)\
            .grid(row=12, column=1, sticky="w", padx=6)
        self.temp_c = self._param(par, 13, "Measure temp (\u00b0C)", 20, -10, 80)
        self.scatter_range = self._param(par, 14, "Turns/layer scatter (\u00b1)", 0, 0, 30)
        self.scatter_seed = self._param(par, 15, "Scatter seed", 1234, 0, 99999)

        # Wind direction
        ttk.Label(par, text="Wind direction:").grid(row=16, column=0, sticky="e", padx=6, pady=4)
        self.wind_dir = tk.StringVar(value="CW")
        wd = ttk.Combobox(par, textvariable=self.wind_dir, width=8, state="readonly",
                          values=["CW", "CCW"])
        wd.grid(row=16, column=1, sticky="w", padx=6)
        wd.bind("<<ComboboxSelected>>", lambda e: self.update_estimate())

        ttk.Label(par, text="Wire gauge:").grid(row=17, column=0, sticky="e", padx=6, pady=4)
        self.wire_var = tk.StringVar(value="42 AWG")
        wcb = ttk.Combobox(par, textvariable=self.wire_var, width=12, state="readonly",
                           values=list(WIRE_PROPS.keys()))
        wcb.grid(row=17, column=1, sticky="w", padx=6)
        wcb.bind("<<ComboboxSelected>>", lambda e: self.update_estimate())

        ttk.Button(par, text="Send Settings to Winder", command=self.send_settings)\
            .grid(row=18, column=0, columnspan=2, pady=8)
        self.est_lbl = ttk.Label(par, text="DCR: --  |  wire: --  |  layers: --",
                                 font=("Helvetica", 11, "bold"))
        self.est_lbl.grid(row=18, column=2, columnspan=2, sticky="w")
        self.est_lbl2 = ttk.Label(par, text="", foreground="#9fb0c4",
                                  font=("Helvetica", 10))
        self.est_lbl2.grid(row=19, column=2, columnspan=2, sticky="w")
        self.est_lbl3 = ttk.Label(par, text="", foreground="#9fb0c4",
                                  font=("Helvetica", 10))
        self.est_lbl3.grid(row=20, column=2, columnspan=2, sticky="w")
        ttk.Label(par, text="Wire tension drives how tightly turns pack (and the "
                            "break-risk warning for fine wire). Measure temp applies "
                            "copper's +0.39%/\u00b0C so predicted DCR matches your meter. "
                            "Inductance (Wheeler) with self-capacitance gives the RLC "
                            "resonant peak and Q \u2014 the real bright/dark character. "
                            "Verify against a measured coil.",
                  foreground="#888", wraplength=440, justify="left")\
            .grid(row=21, column=0, columnspan=4, sticky="w", padx=6)

        # --- Visualizer (right column of mid) ---
        viz = ttk.LabelFrame(mid, text="Coil Visualizer")
        viz.grid(row=0, column=1, sticky="nsew")
        self.coil_canvas = tk.Canvas(viz, height=300, width=460, bg="#1a1a1a",
                                     highlightthickness=0)
        self.coil_canvas.pack(fill="both", expand=True, padx=4, pady=4)
        self.coil_canvas.bind("<Configure>", lambda e: self.draw_coil(self._sim_turns))
        self._sim_turns = 0

        simrow = ttk.Frame(viz)
        simrow.pack(fill="x", padx=4, pady=2)
        ttk.Button(simrow, text="Simulate wind", command=self.simulate_wind)\
            .pack(side="left", padx=2)
        ttk.Button(simrow, text="Pause", command=self.sim_pause)\
            .pack(side="left", padx=2)
        ttk.Button(simrow, text="Reset", command=self.sim_reset)\
            .pack(side="left", padx=2)
        ttk.Button(simrow, text="\u2192 Target", command=self.scrub_to_target)\
            .pack(side="left", padx=2)
        self.debug_var = tk.BooleanVar(value=False)
        ttk.Checkbutton(simrow, text="Debug", variable=self.debug_var,
                        command=lambda: self.draw_coil(self._sim_turns))\
            .pack(side="left", padx=6)

        scrubrow = ttk.Frame(viz)
        scrubrow.pack(fill="x", padx=4, pady=2)
        ttk.Label(scrubrow, text="Scrub:").pack(side="left", padx=(4, 2))
        self.scrub = tk.IntVar(value=0)
        self.scrub_scale = ttk.Scale(scrubrow, from_=0, to=8000, variable=self.scrub,
                                     command=lambda v: self._scrub_cmd(v))
        self.scrub_scale.pack(side="left", fill="x", expand=True, padx=4)
        self.scrub_lbl = ttk.Label(scrubrow, text="0", width=7)
        self.scrub_lbl.pack(side="left", padx=4)

        # --- Controls ---
        ctl = ttk.LabelFrame(parent, text="Run")
        ctl.pack(fill="x", **pad)
        self.start_btn = ttk.Button(ctl, text="START", command=self.cmd_start, state="disabled")
        self.start_btn.grid(row=0, column=0, padx=8, pady=8)
        self.pause_btn = ttk.Button(ctl, text="PAUSE", command=self.cmd_pause, state="disabled")
        self.pause_btn.grid(row=0, column=1, padx=8)
        self.stop_btn = ttk.Button(ctl, text="STOP", command=self.cmd_stop, state="disabled")
        self.stop_btn.grid(row=0, column=2, padx=8)
        ttk.Button(ctl, text="Log This Wind", command=self.log_wind).grid(row=0, column=3, padx=8)

        # --- Calibration ---
        cal = ttk.LabelFrame(parent, text="Traverse Calibration")
        cal.pack(fill="x", **pad)
        ttk.Label(cal, text="Jog distance (mm):").grid(row=0, column=0, padx=6, pady=6, sticky="e")
        self.jog_mm = tk.DoubleVar(value=20.0)
        ttk.Entry(cal, textvariable=self.jog_mm, width=8).grid(row=0, column=1, padx=4, sticky="w")
        self.jog_btn = ttk.Button(cal, text="Jog Carriage", command=self.cmd_jog, state="disabled")
        self.jog_btn.grid(row=0, column=2, padx=6)
        ttk.Label(cal, text="Measured travel (mm):").grid(row=1, column=0, padx=6, pady=6, sticky="e")
        self.measured_mm = tk.DoubleVar(value=0.0)
        ttk.Entry(cal, textvariable=self.measured_mm, width=8).grid(row=1, column=1, padx=4, sticky="w")
        self.calc_btn = ttk.Button(cal, text="Calculate && Apply", command=self.apply_calibration, state="disabled")
        self.calc_btn.grid(row=1, column=2, padx=6)
        self.save_cal_btn = ttk.Button(cal, text="Save to Winder", command=self.save_calibration, state="disabled")
        self.save_cal_btn.grid(row=1, column=3, padx=6)
        self.cal_lbl = ttk.Label(cal, text="Lead screw: -- mm/rev", foreground="#666")
        self.cal_lbl.grid(row=0, column=3, columnspan=2, padx=10, sticky="w")
        self.lead_mm = 8.0

        # --- Live readout ---
        live = ttk.LabelFrame(parent, text="Live")
        live.pack(fill="x", **pad)
        self.turn_lbl = ttk.Label(live, text="0", font=("Helvetica", 40, "bold"))
        self.turn_lbl.pack(pady=4)
        self.state_lbl = ttk.Label(live, text="IDLE", font=("Helvetica", 12))
        self.state_lbl.pack()
        self.progress = ttk.Progressbar(live, length=600, maximum=100)
        self.progress.pack(pady=6)

        self.console = tk.Text(live, height=6, bg="#111", fg="#5f5", font=("Courier", 9))
        self.console.pack(fill="x", padx=4, pady=4)

    def _param(self, parent, row, label, default, lo, hi, is_float=False):
        ttk.Label(parent, text=label + ":").grid(row=row, column=0, sticky="e", padx=6, pady=3)
        var = tk.DoubleVar(value=default) if is_float else tk.IntVar(value=default)
        ent = ttk.Entry(parent, textvariable=var, width=12)
        ent.grid(row=row, column=1, sticky="w", padx=6)
        var.trace_add("write", lambda *a: self.update_estimate())
        return var

    # ---------- recipes ----------
    def refresh_recipe_list(self):
        self.recipe_combo["values"] = list(self.recipes.keys())
        if self.recipes:
            self.recipe_combo.current(0)

    def load_selected_recipe(self):
        name = self.recipe_var.get()
        if name not in self.recipes:
            return
        r = self.recipes[name]
        self.turns.set(r["turns"])
        # winding width: new key win_h, fall back to old width/win_w
        self.win_h.set(r.get("win_h", r.get("width", r.get("win_w", 11.0))))
        self.rpm.set(r["rpm"])
        self.ramp.set(r.get("ramp", 50))
        self.jitter.set(r.get("jitter", 12))
        self.core_w.set(r.get("core_w", 16.0))
        self.core_l.set(r.get("core_l", r.get("bobbin_len", 46.0)))
        self.win_d.set(r.get("win_d", 6.0))
        self.lead.set(r.get("lead", 1.0))
        self.pack.set(r.get("pack", 0.92))
        self.tension.set(r.get("tension", 35))
        self.use_tension.set(r.get("use_tension", True))
        self.temp_c.set(r.get("temp_c", 20))
        self.scatter_range.set(r.get("scatter_range", 0))
        self.scatter_seed.set(r.get("scatter_seed", 1234))
        self.wind_dir.set(r.get("wind_dir", "CW"))
        self.wire_var.set(r.get("wire", "42 AWG"))
        self.update_estimate()
        self.log(f"Loaded recipe: {name}")

    def _current_params(self):
        return {
            "turns": int(self.turns.get()),
            "win_h": float(self.win_h.get()),
            "rpm": int(self.rpm.get()), "ramp": int(self.ramp.get()),
            "jitter": int(self.jitter.get()), "wire": self.wire_var.get(),
            "core_w": float(self.core_w.get()),
            "core_l": float(self.core_l.get()), "win_d": float(self.win_d.get()),
            "lead": float(self.lead.get()), "pack": float(self.pack.get()),
            "tension": int(self.tension.get()),
            "use_tension": bool(self.use_tension.get()),
            "temp_c": int(self.temp_c.get()),
            "scatter_range": int(self.scatter_range.get()),
            "scatter_seed": int(self.scatter_seed.get()),
            "wind_dir": self.wind_dir.get(),
            # keep legacy keys so older code paths / CSV export still work
            "width": float(self.win_h.get()), "pass": 0,
        }

    def save_as_recipe(self):
        name = self._ask_name("Save recipe as:")
        if not name:
            return
        self.recipes[name] = self._current_params()
        self.save_recipes()
        self.refresh_recipe_list()
        self.recipe_var.set(name)
        self.log(f"Saved recipe: {name}")

    def update_recipe(self):
        name = self.recipe_var.get()
        if name not in self.recipes:
            return
        self.recipes[name] = self._current_params()
        self.save_recipes()
        self.log(f"Updated recipe: {name}")

    def delete_recipe(self):
        name = self.recipe_var.get()
        if name in self.recipes and messagebox.askyesno("Delete", f"Delete '{name}'?"):
            del self.recipes[name]
            self.save_recipes()
            self.refresh_recipe_list()

    def _ask_name(self, prompt):
        dlg = tk.Toplevel(self.root)
        dlg.title(prompt)
        dlg.transient(self.root)
        ttk.Label(dlg, text=prompt).pack(padx=12, pady=8)
        var = tk.StringVar()
        ent = ttk.Entry(dlg, textvariable=var, width=30)
        ent.pack(padx=12)
        ent.focus()
        result = {}
        def ok():
            result["name"] = var.get().strip()
            dlg.destroy()
        ttk.Button(dlg, text="OK", command=ok).pack(pady=8)
        ent.bind("<Return>", lambda e: ok())
        dlg.wait_window()
        return result.get("name")

    # ---------- estimates ----------
    def update_estimate(self, *a):
        try:
            turns = int(self.turns.get())
            win_h = float(self.win_h.get())
            core_w = float(self.core_w.get())
            core_l = float(self.core_l.get())
            win_d = float(self.win_d.get())
            pack = float(self.pack.get())
            scatter = float(self.jitter.get())
            tension = float(self.tension.get())
            use_tension = bool(self.use_tension.get())
            temp_c = float(self.temp_c.get())
            wire = self.wire_var.get()
        except (tk.TclError, ValueError):
            return
        props = WIRE_PROPS.get(wire, WIRE_PROPS["Other"])
        r = simulate_coil(turns, win_h, core_w, core_l, props,
                          pack=pack, scatter=scatter,
                          tension_g=(tension if use_tension else None),
                          temp_c=temp_c)
        txt = (f"DCR: {r['dcr_k']:.2f} k\u03a9  |  wire: {r['length_m']:.0f} m  |  "
               f"layers: {r['layers']}  |  build: {r['build_mm']:.1f} mm")
        if hasattr(self, "est_lbl"):
            self.est_lbl.config(text=txt)
        if hasattr(self, "est_lbl2"):
            self.est_lbl2.config(
                text=f"L: {r['inductance_h']:.2f} H  |  self-C: {r['cap_pf']:.0f} pF  |  "
                     f"resonant peak: {r['resonance_khz']:.1f} kHz  |  Q: {r['q_factor']:.1f}")
        if hasattr(self, "est_lbl3"):
            pack_src = (f"tension {int(tension)} g \u2192 pack {r['eff_pack']:.2f}"
                        if use_tension else f"pack {pack:.2f} (manual)")
            risk = r["break_risk"]
            if use_tension and risk > 0:
                risk_txt = f"  |  break risk: {risk*100:.0f}%"
                if risk >= 0.85:
                    risk_txt += " \u26a0 too high"
            else:
                risk_txt = ""
            self.est_lbl3.config(
                text=f"{pack_src}  |  {int(temp_c)}\u00b0C{risk_txt}",
                foreground=("#e07a5a" if (use_tension and risk >= 0.85) else "#9fb0c4"))
        # Keep the live visualizer in sync with the chosen parameters. The
        # scrub range tracks the TARGET turns (the wind ends there).
        if hasattr(self, "coil_canvas") and hasattr(self, "scrub_scale"):
            self.scrub_scale.configure(to=max(turns, 1))
            if self.state not in ("RUNNING", "PAUSED"):
                self.scrub.set(turns)
                self.draw_coil(turns)

    def apply_coil_preset(self, *a):
        preset = COIL_PRESETS.get(self.coil_preset.get())
        if not preset:
            return
        self.win_h.set(preset["win_h"])
        self.core_w.set(preset["core_w"])
        self.core_l.set(preset["core_l"])
        self.win_d.set(preset["win_d"])
        self.update_estimate()
        self.draw_coil(self._sim_turns)

    # ---------- coil visualizer ----------
    def _coil_params(self):
        """Read geometry inputs; return (win_h, core_w, core_l, win_d, props) or None."""
        try:
            win_h = float(self.win_h.get())
            core_w = float(self.core_w.get())
            core_l = float(self.core_l.get())
            win_d = float(self.win_d.get())
            wire = self.wire_var.get()
        except (tk.TclError, ValueError):
            return None
        props = WIRE_PROPS.get(wire, WIRE_PROPS["Other"])
        return win_h, core_w, core_l, win_d, props

    def machine_state(self, turns):
        """Single source of truth for the machine's mechanical state at a given
        turn count. Everything (draw + animation) reads from this so the
        spindle, traverse and copper can never disagree.

        Real winder model (from how actual machines are programmed):
          - turns per layer = winding width / wire OD (integer), optionally
            randomized +/- scatter_range per layer to mimic hand/scatter winds
          - the traverse advances ~1 wire-width per spindle revolution; on a
            rectangular bobbin most of that advance happens over a small part
            of the turn, but for a smooth on-screen guide we advance it
            continuously across the layer
          - each completed layer reverses traverse direction and the next layer
            staggers half a wire width
          - spindle angle = fractional part of the current turn * 360
        Returns dict: layer, in_layer, tpl_here, frac (0..1 across the width),
                      going_up (bool), spindle_deg, layers_done, total_layers_est
        """
        import random
        p = self._coil_params()
        if not p or turns < 0:
            return None
        win_h, core_w, core_l, win_d, props = p
        od = props["od_mm"]
        try:
            pack = float(self.pack.get())
            srange = int(self.scatter_range.get())
            seed = int(self.scatter_seed.get())
            wind_dir = self.wind_dir.get()
        except (tk.TclError, ValueError, AttributeError):
            pack, srange, seed, wind_dir = 1.0, 0, 1234, "CW"

        base_tpl = max(1, int(round((win_h / od) * pack)))
        rng = random.Random(seed)

        # Walk layers, drawing a (possibly scatter-randomized) turns-per-layer
        # for each, until we've accounted for `turns`.
        t_remaining = int(turns)
        layer = 0
        in_layer = 0
        tpl_here = base_tpl
        while t_remaining > 0:
            tpl_here = base_tpl
            if srange > 0:
                tpl_here = max(1, base_tpl + rng.randint(-srange, srange))
            if t_remaining <= tpl_here:
                in_layer = t_remaining
                t_remaining = 0
                break
            t_remaining -= tpl_here
            layer += 1
        if turns == 0:
            layer, in_layer, tpl_here = 0, 0, base_tpl

        frac = (in_layer / tpl_here) if tpl_here else 0.0
        # even layers wind one way, odd the other (reversal each layer)
        going_up = (layer % 2 == 0)
        if wind_dir == "CCW":
            going_up = not going_up
        spindle_deg = (turns - int(turns)) * 360.0 if isinstance(turns, float) else \
                      (turns % 1) * 360.0
        # for integer turns, derive a sub-turn angle from a fine fraction so the
        # spindle reads as spinning during animation (animation passes floats)
        return {
            "layer": layer, "in_layer": in_layer, "tpl_here": tpl_here,
            "base_tpl": base_tpl, "frac": frac, "going_up": going_up,
            "spindle_deg": spindle_deg, "od": od,
        }

    def _max_fill_turns(self, win_h, win_d, od):
        """Turns that completely fill the bobbin: turns/layer x layers that fit."""
        import math
        tpl = max(1, int(round(win_h / od)))
        pitch = od * math.sqrt(3) / 2.0
        layers = max(1, int(win_d / pitch))
        return tpl * layers

    def draw_coil(self, turns):
        """Front view of the bobbin, driven entirely by machine_state() so the
        spindle, traverse carriage, wire feed and copper can never disagree.
        Flanges bound the winding channel; the active layer fills with turn
        lines, completed layers accumulate with a half-wire stagger, the
        traverse rides a vertical rail in lock-step with the laid row, and the
        spindle rotates by the fractional turn. Accepts fractional turns during
        animation so the spindle spins smoothly. Scaled to fit the canvas."""
        import math, random
        tfloat = float(turns)
        turns_i = int(tfloat)
        self._sim_turns = turns_i
        c = self.coil_canvas
        c.delete("all")
        W = c.winfo_width(); H = c.winfo_height()
        if W < 50:
            W = max(int(c.cget("width")), 300)
        if H < 50:
            H = max(int(c.cget("height")), 200)
        p = self._coil_params()
        if not p:
            return
        win_h, core_w, core_l, win_d, props = p
        od = props["od_mm"]
        try:
            pack = float(self.pack.get())
            scatter = float(self.jitter.get())
            rpm = float(self.rpm.get())
            seed = int(self.scatter_seed.get())
            tension = float(self.tension.get())
            use_tension = bool(self.use_tension.get())
            temp_c = float(self.temp_c.get())
        except (tk.TclError, ValueError, AttributeError):
            pack, scatter, rpm, seed = 1.0, 0.0, 420.0, 1234
            tension, use_tension, temp_c = 35.0, True, 20.0
        sim = simulate_coil(turns_i, win_h, core_w, core_l, props,
                            pack=pack, scatter=scatter,
                            tension_g=(tension if use_tension else None),
                            temp_c=temp_c)
        st = self.machine_state(turns_i)
        if st is None:
            return
        base_tpl = st["base_tpl"]
        cur_layer = st["layer"]
        in_layer = st["in_layer"]
        tpl_here = st["tpl_here"]
        going_up = st["going_up"]
        cap_layers = max(1, self._max_fill_turns(win_h, win_d, od) // max(base_tpl, 1))
        layer_disp = cur_layer + (1 if in_layer > 0 else 0)
        jfrac = max(0.0, min(1.0, scatter / 100.0))
        rng = random.Random(seed)

        # ---- layout: bobbin rectangle PROPORTIONAL to the real dimensions ----
        # The drawn shape reflects core length (horizontal) x winding width
        # (vertical) so changing the bobbin dimensions changes the picture, not
        # just the labels. Fit the real aspect ratio into the available area.
        m_left, m_right, m_top, m_bot = 96, 120, 82, 50
        avail_w = max(60, W - m_left - m_right)
        avail_h = max(40, min(H - m_top - m_bot, 230))
        aspect = max(0.05, win_h / max(core_l, 0.1))   # height/width of the real bobbin
        # try filling width first, clamp height; else fill height
        bob_w = avail_w
        bob_h = bob_w * aspect
        if bob_h > avail_h:
            bob_h = avail_h
            bob_w = bob_h / aspect
        bob_w = max(60, min(avail_w, bob_w))
        bob_h = max(30, min(avail_h, bob_h))
        bx0 = m_left + (avail_w - bob_w) / 2
        by0 = m_top + (avail_h - bob_h) / 2
        bx1 = bx0 + bob_w
        by1 = by0 + bob_h
        inner_h = by1 - by0
        flange_th = max(8, 0.12 * bob_h)
        row_px = inner_h / max(base_tpl, 1)

        # Channel + flanges
        c.create_rectangle(bx0, by0, bx1, by1, fill="#241f1c", outline="#555")
        c.create_rectangle(bx0 - 10, by0 - flange_th, bx1 + 10, by0,
                           fill="#202020", outline="#000")
        c.create_rectangle(bx0 - 10, by1, bx1 + 10, by1 + flange_th,
                           fill="#202020", outline="#000")

        # ---- copper: completed layers + the active layer's turn-lines ----
        if cur_layer > 0 or in_layer > 0:
            frac_depth = min(1.0, sim["build_mm"] / win_d) if win_d else 0
            r0, g0, b0 = 0x5a, 0x2c, 0x12
            r1, g1, b1 = 0xC9, 0x40, 0x1A
            r = int(r0 + (r1 - r0) * frac_depth)
            g = int(g0 + (g1 - g0) * frac_depth)
            b = int(b0 + (b1 - b0) * frac_depth)
            c.create_rectangle(bx0 + 1, by0 + 1, bx1 - 1, by1 - 1,
                               fill=f"#{r:02x}{g:02x}{b:02x}", outline="")
            # one faint band per completed layer; alternate layers stagger half
            # a wire-width (the real close-pack nesting), with scatter jitter.
            bands = min(cur_layer, 48)
            for k in range(1, bands + 1):
                stagger = 0.5 * row_px if (k % 2) else 0.0
                dy = (rng.random() - 0.5) * row_px * jfrac
                yy = by0 + inner_h * k / (bands + 1) + stagger * 0.2 + dy
                c.create_line(bx0 + 2, yy, bx1 - 2, yy, fill="#8a4a1e", width=1)

        # ---- compute the guide's wandering lay path first ----
        # Each turn's lay position = its ideal row + a seeded hand-wander, so
        # the guide, the wire feed and the laid copper all reference the SAME
        # positions and stay in sync. The wander is a smooth drift + small
        # tremor scaled by the scatter amount (an unsteady hand).
        stagger_px = 0.5 * row_px if (cur_layer % 2) else 0.0
        def lay_y(turn_index_in_layer, slot_for_turn):
            base = by1 - (slot_for_turn + 0.5) * row_px + stagger_px
            if jfrac <= 0:
                return max(by0 + 1, min(by1 - 1, base))
            gt = (cur_layer * 10007 + turn_index_in_layer)   # global-ish turn id
            ph = gt * 0.7 + seed
            wander = (math.sin(ph * 0.13) + 0.5 * math.sin(ph * 0.41 + 1.3)
                      + 0.3 * math.sin(ph * 1.07 + 2.1)) / 1.8
            tremor = random.Random((gt * 2654435761) ^ seed).random() - 0.5
            off = (wander * 0.12 + tremor * 0.05) * inner_h * jfrac
            return max(by0 + 1, min(by1 - 1, base + off))

        # helix tilt: per revolution the guide advances ~1 wire-width across the
        # winding width, so each turn lays at a slight diagonal. The geometric
        # helix angle is set by traverse advance per rev (lead) vs the turn's
        # circumference; RPM sets how that advance is distributed through the
        # rotation, which we let modulate the on-screen slant (higher RPM ->
        # the advance is spread over more wire, shallower visible angle).
        try:
            lead = float(self.lead.get())
            rpm = float(self.rpm.get())
        except (tk.TclError, ValueError, AttributeError):
            lead, rpm = 1.0, 420.0
        helix = max(0.2, min(3.0, lead / max(od, 1e-3)))
        rpm_factor = max(0.4, min(1.6, 420.0 / max(rpm, 1.0)))  # faster -> shallower
        tilt_px = row_px * helix * rpm_factor

        # active layer turn-lines, drawn as connected angled segments that track
        # the wandering guide path (not flat stacked lines).
        n_lines = in_layer if in_layer > 0 else 0
        max_draw = 80
        step = max(1, int(n_lines) // max_draw)
        lw = max(1, int(row_px * step * 0.9))
        prev_pt = None
        for i in range(0, int(n_lines), step):
            slot = i if going_up else (base_tpl - 1 - i)
            y = lay_y(i, slot)
            # the wire enters at the left from the previous turn's height and
            # rises/falls to this turn's height at the right (the guide side):
            y_left = y + (tilt_px if going_up else -tilt_px) * 0.5
            y_right = y - (tilt_px if going_up else -tilt_px) * 0.5
            y_left = max(by0 + 1, min(by1 - 1, y_left))
            y_right = max(by0 + 1, min(by1 - 1, y_right))
            col = "#E5481F" if (i // step) % 2 == 0 else "#C9401A"
            c.create_line(bx0 + 2, y_left, bx1 - 2, y_right, fill=col, width=lw,
                          capstyle="round")
            # the guide feeds the wire in on the left side; track that endpoint
            prev_pt = (bx0 + 2, y_left)

        # ---- machine motion (all from machine_state, so it stays in sync) ----
        if turns_i > 0:
            # the guide sits at the CURRENT turn within the layer. Use the same
            # lay_y() the copper lines use, so the carriage, the wire feed and
            # the laid wire all reference one position and cannot disagree.
            active_turn = (in_layer - 1) if in_layer > 0 else (tpl_here - 1)
            active_turn = max(0, active_turn)
            slot = active_turn if going_up else (base_tpl - 1 - active_turn)
            slot = max(0, min(base_tpl, slot))
            cy_lay = lay_y(active_turn, slot)

            rail_x = bx0 - 30
            c.create_line(rail_x, by0, rail_x, by1, fill="#444", width=1)
            # wire's actual entry point on the coil (guide side / left end)
            lay_pt = prev_pt if prev_pt else (bx0 + 2, cy_lay)
            # carriage rides the rail at exactly the wire's lay height
            cy_car = lay_pt[1]
            c.create_rectangle(rail_x - 7, cy_car - 9, rail_x + 7, cy_car + 9,
                               fill="#888780", outline="#aaa")
            # wire runs from the guide across to its entry point on the copper
            c.create_line(rail_x + 7, cy_car, lay_pt[0], lay_pt[1],
                          fill="#E0995E", width=1)
            c.create_oval(lay_pt[0] - 3, lay_pt[1] - 3, lay_pt[0] + 3, lay_pt[1] + 3,
                          fill="#FF7A4D", outline="")
            c.create_text(rail_x, by0 - 8, anchor="s",
                          text=("\u25b2 up" if going_up else "\u25bc down"),
                          fill="#E0995E", font=("Helvetica", 8))

            # spindle: rotate by the fractional turn so it spins smoothly during
            # animation. When stepping many turns per frame (high RPM) a single
            # tick can't be tracked, so we draw faint motion-blur arcs to read as
            # a fast spin, with the sharp spokes on top.
            axis_y = by0 - flange_th - 22
            sub_turn = tfloat - math.floor(tfloat)
            ang = sub_turn * 2 * math.pi
            spin_cx = (bx0 + bx1) / 2
            rr = 10
            c.create_oval(spin_cx - rr, axis_y - rr, spin_cx + rr, axis_y + rr,
                          outline="#7a8aa0", width=2)
            per_frame = getattr(self, "_anim_per_frame", 1.0) \
                if getattr(self, "_sim_running", False) else 1.0
            if per_frame > 3:
                # blur ring: multiple faint spokes spread around the circle
                blur = min(10, int(per_frame))
                for kk in range(blur):
                    a = ang + (kk / blur) * 2 * math.pi
                    c.create_line(spin_cx, axis_y,
                                  spin_cx + rr * math.cos(a),
                                  axis_y + rr * math.sin(a),
                                  fill="#5a3a2a", width=1)
            # two sharp spokes so direction/rotation still reads
            for off in (0, math.pi):
                c.create_line(spin_cx, axis_y,
                              spin_cx + rr * math.cos(ang + off),
                              axis_y + rr * math.sin(ang + off),
                              fill="#FF7A4D", width=2)
            c.create_text(spin_cx + rr + 8, axis_y, anchor="w",
                          text=f"{int(rpm)} rpm", fill="#9fb0c4",
                          font=("Helvetica", 8))

        # ---- build bar ----
        bar_x = bx1 + 30; bar_w = 26
        bfrac = min(1.0, sim["build_mm"] / win_d) if win_d else 0
        over = sim["build_mm"] > win_d
        c.create_rectangle(bar_x, by0, bar_x + bar_w, by1, outline="#666", fill="#1a1a1a")
        fill_h = min(inner_h, bfrac * inner_h)
        if fill_h > 0.5:
            c.create_rectangle(bar_x + 1, by1 - fill_h, bar_x + bar_w - 1, by1 - 1,
                               fill=("#e07a5a" if over else "#D9A441"), outline="")
        c.create_text(bar_x + bar_w / 2, by0 - 6, anchor="s", text="build",
                      fill="#9fb0c4", font=("Helvetica", 8))
        c.create_text(bar_x + bar_w / 2, by1 + 12, anchor="n",
                      text=f"{sim['build_mm']:.1f}/{win_d:.1f}mm",
                      fill=("#e07a5a" if over else "#9fb0c4"), font=("Helvetica", 8))

        # ---- dimensions ----
        c.create_line(bx0 - 52, by0, bx0 - 52, by1, fill="#7a8aa0")
        c.create_text(bx0 - 56, (by0 + by1) / 2, anchor="e",
                      text=f"{win_h:.1f} mm", fill="#9fb0c4", font=("Helvetica", 9))
        c.create_line(bx0, by1 + flange_th + 14, bx1, by1 + flange_th + 14, fill="#7a8aa0")
        c.create_text((bx0 + bx1) / 2, by1 + flange_th + 26,
                      text=f"{core_l:.1f} mm length", fill="#9fb0c4", font=("Helvetica", 9))

        # ---- headline + footer ----
        scat_txt = ""
        try:
            if int(self.scatter_range.get()) > 0:
                scat_txt = f"  ·  scatter \u00b1{int(self.scatter_range.get())}/layer"
        except (tk.TclError, ValueError, AttributeError):
            pass
        c.create_text(W / 2, 16,
                      text=f"Layer {layer_disp} / {cap_layers}    "
                           f"{turns_i:,} turns    {sim['dcr_k']:.2f} k\u03a9",
                      fill="#eee", font=("Helvetica", 11, "bold"))
        c.create_text(W / 2, H - 10, fill=("#e07a5a" if over else "#999"),
                      text=f"{base_tpl} turns/layer  ·  {sim['length_m']:.0f} m wire"
                           + scat_txt
                           + ("  ·  EXCEEDS BUILD DEPTH" if over else ""),
                      font=("Helvetica", 9))

        # ---- break-risk banner (when winding above safe tension) ----
        if use_tension and sim.get("break_risk", 0) >= 0.85:
            c.create_text(W / 2, 32, fill="#e07a5a",
                          text=f"\u26a0 tension {int(tension)} g \u2014 "
                               f"break risk {sim['break_risk']*100:.0f}%",
                          font=("Helvetica", 9, "bold"))

        # ---- debug overlay ----
        if getattr(self, "debug_var", None) and self.debug_var.get():
            lines = [
                f"turns={turns_i}  layer={cur_layer}  in_layer={in_layer}",
                f"tpl_here={tpl_here}  base_tpl={base_tpl}",
                f"frac={st['frac']:.3f}  going_up={going_up}",
                f"spindle={(tfloat - int(tfloat)) * 360:.0f}\u00b0  od={od:.4f}mm  row_px={row_px:.1f}",
                f"build={sim['build_mm']:.2f}/{win_d:.1f}mm  layers={sim['layers']}",
                f"DCR={sim['dcr_k']:.3f}k  L={sim['length_m']:.1f}m",
                f"L={sim['inductance_h']:.2f}H  Q={sim['q_factor']:.1f}  "
                f"eff_pack={sim['eff_pack']:.2f}",
                f"cap={sim['cap_pf']:.0f}pF  res={sim['resonance_khz']:.1f}kHz  "
                f"brk={sim['break_risk']*100:.0f}%",
                f"running={getattr(self,'_sim_running',False)}  "
                f"paused={getattr(self,'_sim_paused',False)}",
            ]
            yy = by0 + 4
            for ln in lines:
                c.create_text(bx0 + 6, yy, anchor="nw", text=ln, fill="#7CFC00",
                              font=("Courier", 8))
                yy += 12

    # ---------- animation ----------
    def simulate_wind(self):
        """Animate the wind to target turns. RPM sets turns/sec (time-compressed
        but proportional). Spindle spins smoothly because we advance fractional
        turns per frame and pass them to draw_coil. Honors pause/stop."""
        try:
            target = int(self.turns.get())
            rpm = max(1.0, float(self.rpm.get()))
        except (tk.TclError, ValueError):
            return
        if target <= 0:
            return
        self.scrub_scale.configure(to=max(target, 1))
        self._sim_running = True
        self._sim_paused = False
        # RPM drives the real winding rate: turns/sec = rpm/60. We compress time
        # so the wind is watchable, then split that rate between turns-per-frame
        # and the frame delay. Crucially this stays PROPORTIONAL to RPM: higher
        # RPM => more turns added per second on screen. We keep turns-per-frame
        # modest so the spindle still visibly rotates, and shorten the frame
        # delay (down to a floor) as RPM rises, then add turns/frame beyond that.
        compress = 60.0                          # 1 real sec -> ~60 sim sec
        turns_per_sec = (rpm / 60.0) * compress  # on-screen turns/sec
        # choose a frame rate; faster wind -> shorter delay until a floor, then
        # grow turns/frame so very high RPM still speeds up.
        ideal_frame_ms = 1000.0 / max(1.0, turns_per_sec)   # if 1 turn/frame
        if ideal_frame_ms >= 16:
            frame_ms = min(120, int(ideal_frame_ms))
            per_frame = 1.0
        else:
            frame_ms = 16
            # keep RPM proportional: add turns/frame beyond the frame-rate floor.
            # The spindle shows a motion-blur ring when it's spinning faster than
            # the eye can track, so we don't need a trackable single tick.
            per_frame = max(1.0, turns_per_sec * (frame_ms / 1000.0))
        self._anim_per_frame = per_frame
        self._anim_total_s = target / max(turns_per_sec, 1e-6)
        self._sim_step(0.0, target, per_frame, frame_ms)

    def _sim_step(self, t, target, per_frame, frame_ms):
        if not getattr(self, "_sim_running", False):
            return
        if getattr(self, "_sim_paused", False):
            self.root.after(80, lambda: self._sim_step(t, target, per_frame, frame_ms))
            return
        t = min(target, t + per_frame)
        self.scrub.set(int(t))
        self.draw_coil(t)            # float -> spindle angle from sub-turn part
        if t < target:
            self.root.after(frame_ms,
                            lambda: self._sim_step(t, target, per_frame, frame_ms))
        else:
            self._sim_running = False

    def sim_pause(self):
        self._sim_paused = not getattr(self, "_sim_paused", False)

    def sim_reset(self):
        self._sim_running = False
        self._sim_paused = False
        self.scrub.set(0)
        self.draw_coil(0)

    def scrub_to_target(self):
        try:
            target = int(self.turns.get())
        except (tk.TclError, ValueError):
            return
        self._sim_running = False
        self.scrub.set(target)
        self.draw_coil(target)

    def _scrub_cmd(self, v):
        # Manual scrub interrupts any running animation and redraws at that turn
        self._sim_running = False
        t = int(float(v))
        if hasattr(self, "scrub_lbl"):
            self.scrub_lbl.config(text=f"{t:,}")
        self.draw_coil(t)


    # ---------- serial ----------
    def refresh_ports(self):
        if not HAVE_SERIAL:
            self.port_combo["values"] = ["pyserial not installed"]
            self.log("ERROR: run  pip install pyserial")
            return
        ports = [p.device for p in serial.tools.list_ports.comports()]
        self.port_combo["values"] = ports
        if ports:
            self.port_combo.current(0)

    def toggle_connect(self):
        if self.ser and self.ser.is_open:
            self.disconnect()
        else:
            self.connect()

    def connect(self):
        if not HAVE_SERIAL:
            messagebox.showerror("Missing library", "Run: pip install pyserial")
            return
        port = self.port_var.get()
        if not port:
            return
        try:
            self.ser = serial.Serial(port, BAUD, timeout=0.2)
            time.sleep(2)  # Uno resets on connect
            self.reading = True
            self.reader_thread = threading.Thread(target=self._reader, daemon=True)
            self.reader_thread.start()
            self.conn_lbl.config(text="Connected", foreground="#080")
            self.connect_btn.config(text="Disconnect")
            for b in (self.start_btn, self.pause_btn, self.stop_btn,
                      self.jog_btn, self.calc_btn, self.save_cal_btn):
                b.config(state="normal")
            self.send("PING")
            self.send("GETCAL")
            self.log(f"Connected to {port}")
        except Exception as e:
            messagebox.showerror("Connection failed", str(e))

    def disconnect(self):
        self.reading = False
        if self.ser:
            try:
                self.ser.close()
            except Exception:
                pass
        self.ser = None
        self.conn_lbl.config(text="Disconnected", foreground="#b00")
        self.connect_btn.config(text="Connect")
        for b in (self.start_btn, self.pause_btn, self.stop_btn,
                  self.jog_btn, self.calc_btn, self.save_cal_btn):
            b.config(state="disabled")

    def send(self, cmd):
        if self.ser and self.ser.is_open:
            self.ser.write((cmd + "\n").encode())
            self.log(f">> {cmd}")
        else:
            self.log("(not connected)")

    def _reader(self):
        buf = ""
        while self.reading and self.ser:
            try:
                data = self.ser.read(128).decode(errors="ignore")
            except Exception:
                break
            if not data:
                continue
            buf += data
            while "\n" in buf:
                line, buf = buf.split("\n", 1)
                line = line.strip()
                if line:
                    self.root.after(0, self._handle_line, line)

    def _handle_line(self, line):
        if line.startswith("TURNS:"):
            try:
                self.turns_done = int(line[6:])
                self.turn_lbl.config(text=str(self.turns_done))
                target = max(int(self.turns.get()), 1)
                self.progress["value"] = min(100, self.turns_done / target * 100)
                self.draw_coil(self.turns_done)   # live coil cross-section
            except ValueError:
                pass
        elif line.startswith("STATE:"):
            self.state = line[6:]
            self.state_lbl.config(text=self.state)
        elif line.startswith("CAL:"):
            try:
                parts = dict(p.split("=") for p in line[4:].split(","))
                self.lead_mm = float(parts["leadscrew"])
                self.cal_lbl.config(text=f"Lead screw: {self.lead_mm:.3f} mm/rev")
            except (ValueError, KeyError):
                self.log(f"<< {line}")
        else:
            self.log(f"<< {line}")

    # ---------- commands ----------
    def send_settings(self):
        p = self._current_params()
        self.send(f"SET TURNS {p['turns']}")
        self.send(f"SET WIDTH {p['win_h']}")
        self.send(f"SET RPM {p['rpm']}")
        self.send(f"SET LEAD {p['lead']}")
        self.send(f"SET RAMP {p['ramp']}")
        self.send(f"SET DIR {p['wind_dir']}")
        self.send(f"SET SCATTER {p['scatter_range']} {p['scatter_seed']}")
        self.send(f"SET JITTER {p['jitter']}")

    def cmd_start(self):
        self.send_settings()
        self.send("START")

    def cmd_pause(self):
        self.send("PAUSE")

    def cmd_stop(self):
        self.send("STOP")
        self.progress["value"] = 0

    # ---------- calibration ----------
    def cmd_jog(self):
        try:
            mm = float(self.jog_mm.get())
        except (tk.TclError, ValueError):
            return
        self.send(f"JOG {mm}")
        self.log(f"Jogged {mm} mm — now measure actual travel with calipers")

    def apply_calibration(self):
        try:
            commanded = float(self.jog_mm.get())
            measured = float(self.measured_mm.get())
        except (tk.TclError, ValueError):
            return
        if measured <= 0 or commanded <= 0:
            messagebox.showwarning("Calibration",
                                   "Enter the commanded and measured distances first.")
            return
        # If we asked for `commanded` and the carriage moved `measured`,
        # the true lead-screw value scales by measured/commanded.
        new_lead = self.lead_mm * (measured / commanded)
        self.send(f"SETLEADSCREW {new_lead:.4f}")
        self.send("GETCAL")
        self.log(f"Applied calibration: lead screw -> {new_lead:.4f} mm/rev "
                 f"(was {self.lead_mm:.4f}). Jog again to verify, then Save.")

    def save_calibration(self):
        self.send("SAVECAL")
        self.log("Calibration saved to winder (survives power-off).")

    # ---------- logging ----------
    def log_wind(self):
        p = self._current_params()
        row = [datetime.now().isoformat(timespec="seconds"),
               self.recipe_var.get(), self.turns_done, p["turns"],
               p["win_h"], p["rpm"], p["lead"], p["scatter_range"],
               p["jitter"], p["wire"]]
        new = not os.path.exists(LOG_FILE)
        with open(LOG_FILE, "a", newline="") as f:
            w = csv.writer(f)
            if new:
                w.writerow(["timestamp", "recipe", "turns_done", "target",
                            "winding_width_mm", "rpm", "lead_mm_rev",
                            "scatter_range", "jitter", "wire"])
            w.writerow(row)
        self.log(f"Logged wind: {self.turns_done} turns -> {LOG_FILE}")

    def log(self, msg):
        ts = datetime.now().strftime("%H:%M:%S")
        self.console.insert("end", f"[{ts}] {msg}\n")
        self.console.see("end")


def main():
    root = tk.Tk()
    app = WinderApp(root)
    root.protocol("WM_DELETE_WINDOW", lambda: (app.disconnect(), root.destroy()))
    root.mainloop()


if __name__ == "__main__":
    main()
