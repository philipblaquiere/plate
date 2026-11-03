//! plate-core: the engine's domain model.
//!
//! The flow is deliberately linear so every stage can be swapped out later:
//!
//! ```text
//! source (.ncl) --plate-config--> serde_json::Value --Config::from_value--> Config
//!     --Solver::solve--> Layout --plate-export--> DXF / SVG / JSON
//! ```
//!
//! Nothing in this crate knows about Nickel, HTTP or file formats, which keeps
//! it usable from a server, a CLI, or a future WebAssembly build.

pub mod assembly;
pub mod config;
pub mod diagnostic;
pub mod geometry;
pub mod layout;
pub mod plan;
pub mod service;
pub mod solve;
pub mod solver;
pub mod symbols;
pub mod units;

pub use config::{Config, ConfigError, EdgeRoles, PointInput, Start};
pub use diagnostic::{Diagnostic, Severity, Span};
pub use geometry::{path_length, segments_cross, signed_area, Bounds, Point, Polygon};
pub use layout::{Element, ElementKind, Layout, PortInfo, SlotInfo};
pub use plan::SpaceKind;
pub use solver::{PlateSolver, Solution, SolveError, Solver, View};
pub use units::{Length, System, Units};
