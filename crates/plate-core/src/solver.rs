use crate::config::Config;
use crate::diagnostic::Diagnostic;
use crate::layout::Layout;

pub use crate::solve::{Solution, View};

#[derive(Debug, Clone, thiserror::Error)]
#[error("{0}")]
pub struct SolveError(pub String);

/// Turns a validated configuration into a layout.
///
/// Every solver, built-in or (later) user-supplied as a sandboxed WebAssembly component,
/// sits behind this trait. Solvers must be deterministic: same config, same layout.
pub trait Solver: Send + Sync {
    fn name(&self) -> &'static str;
    fn solve(&self, config: &Config) -> Result<Solution, SolveError> {
        self.solve_view(config, &View::Default)
    }

    /// Solve, showing a plan file (one without a `start`) in `view`. Plates ignore the view.
    fn solve_view(&self, config: &Config, view: &View) -> Result<Solution, SolveError>;
}

/// The built-in solver: places the starting geometry, then solves the plan's spaces level by
/// level with the `grid` and `spine_stack` engines (see [`crate::solve`]).
#[derive(Debug, Default, Clone, Copy)]
pub struct PlateSolver;

impl Solver for PlateSolver {
    fn name(&self) -> &'static str {
        "plate"
    }

    fn solve_view(&self, config: &Config, view: &View) -> Result<Solution, SolveError> {
        Ok(crate::solve::solve_view(config, view))
    }
}

impl Solution {
    pub fn new(layout: Layout, diagnostics: Vec<Diagnostic>, report: crate::layout::Report) -> Self {
        Solution { layout, diagnostics, report }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::ElementKind;
    use serde_json::json;

    fn solve(v: serde_json::Value) -> Solution {
        PlateSolver.solve(&Config::from_value(v).unwrap()).unwrap()
    }

    #[test]
    fn boundary_keeps_point_order_and_measures() {
        // Clockwise on purpose: the order must survive so edits map back to the file.
        let layout =
            solve(json!({ "start": { "mode": "boundary", "points": [[0, 0], [0, 10], [20, 10], [20, 0]] } }))
                .layout;
        let e = &layout.elements[0];
        assert_eq!(e.kind, ElementKind::Boundary);
        assert_eq!(e.points[1].y, 10.0);
        assert_eq!(e.area, Some(200.0));
        assert_eq!(e.length, 60.0);
        assert_eq!(e.source.as_deref(), Some("/start/points"));
    }

    #[test]
    fn spine_and_origin() {
        let spine =
            solve(json!({ "start": { "mode": "spine", "points": [[0, 0], [3, 4], [3, 10]] } })).layout;
        assert_eq!(spine.elements[0].kind, ElementKind::Spine);
        assert_eq!(spine.elements[0].length, 11.0);
        assert!(!spine.elements[0].closed);

        let origin = solve(json!({ "start": { "mode": "spine", "points": [[2, 3]] } })).layout;
        assert_eq!(origin.elements[0].kind, ElementKind::Origin);
        assert_eq!(origin.bounds.min_x, 2.0);
    }
}
