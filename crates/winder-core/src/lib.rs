//! Core of the pickup winder: what a coil is, how its wire is laid, and what
//! it will measure.
//!
//! The flow is one-way and has a single source of truth:
//!
//! ```text
//! Design (bobbin + wire + sections) --generate--> Toolpath --> G-code (winder-gcode)
//!                                                     \-----> estimates, preview
//! ```
//!
//! Nothing here touches the GUI, files or the serial port, so all of it runs
//! in tests and from the CLI.

pub mod auto;
pub mod bobbin;
pub mod design;
pub mod lay;
pub mod physics;
pub mod recipe;
pub mod rng;
pub mod toolpath;
pub mod wire;

pub use auto::Auto;
pub use bobbin::{Bobbin, Detail, Style};
pub use design::{Design, DesignError, Direction, Pattern, Section};
pub use lay::{CrossSection, Laid, LaySettings};
pub use physics::{Estimate, EstimateInput, estimate};
pub use toolpath::{Move, Step, Toolpath, generate};
pub use wire::WireSpec;
