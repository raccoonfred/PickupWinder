//! G-code for the pickup winder.
//!
//! The spindle is driven as a linear axis measured in turns (GRBL X with
//! steps/mm = steps per spindle revolution) and the traverse as Y in mm.
//! Each `G1` line lays a helix; `G93` inverse-time feed makes every line's
//! duration, and so the RPM, exact.

pub mod calibrate;
pub mod grbl;
pub mod profile;
pub mod verify;

pub use calibrate::{CalibrateError, Calibration, flange_program, probe_ys, solve};
pub use grbl::{PostError, post_grbl};
pub use profile::{Homing, MachineProfile};
pub use verify::{Program, VerifyError, parse, verify};
