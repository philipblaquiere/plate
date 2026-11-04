//! The full pipeline in one place: configuration in, layout (or diagnostics) out.
//!
//! The editor evaluates Nickel in the browser and sends checked configuration data
//! ([`Input::Config`]); the CLI and other tools can still send Nickel source ([`Input::Source`]).
//! Either way the data is validated again here: the server never trusts a client.

use std::time::{Duration, Instant};

use plate_core::{Config, Diagnostic, Layout, PlateSolver, Solver, View};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    pub eval_ms: f64,
    pub solve_ms: f64,
}

pub struct Solved {
    pub layout: Layout,
    /// Warnings from the solver (rules it couldn't meet), with JSON pointers into the config.
    pub diagnostics: Vec<Diagnostic>,
    /// Perfect, solved or incomplete, and why.
    pub report: plate_core::layout::Report,
    pub solver: &'static str,
    pub timings: Timings,
}

/// The solver used for every request. When user-supplied solvers arrive, this becomes a
/// lookup by the config's `solver` field into a registry of sandboxed components.
pub fn solver() -> &'static dyn Solver {
    &PlateSolver
}

/// What a request hands the engine.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum Input {
    /// Configuration data, already evaluated from Nickel (by the browser). `view` says what a
    /// plan file (no `start`) is shown in.
    Config {
        config: serde_json::Value,
        #[serde(default)]
        view: View,
    },
    /// A workspace: the plate (or plan) file and the files it may import, by relative path.
    Files {
        main: String,
        files: std::collections::BTreeMap<String, String>,
        #[serde(default)]
        view: View,
        /// Solve with this seed instead of the file's (to try others without editing it).
        #[serde(default)]
        seed: Option<u64>,
    },
    /// Nickel source, evaluated here.
    Source { source: String },
}

pub fn run(source: &str) -> Result<Solved, Vec<Diagnostic>> {
    run_input(Input::Source { source: source.to_owned() })
}

/// Solve the plate `main` of a workspace (`files` by relative path).
pub fn run_files(
    main: &str,
    files: std::collections::BTreeMap<String, String>,
    seed: Option<u64>,
) -> Result<Solved, Vec<Diagnostic>> {
    check_files(main, &files)?.solve(View::Default, seed)
}

/// A workspace's main file, evaluated and checked. What it evaluated to can be read (is it a
/// library?) and solved without evaluating it again.
pub struct Checked {
    pub config: Config,
    pub evaluation: plate_config::Evaluation,
    eval: Duration,
}

pub fn check_files(
    main: &str,
    files: &std::collections::BTreeMap<String, String>,
) -> Result<Checked, Vec<Diagnostic>> {
    let t0 = Instant::now();
    let (mut config, evaluation) = plate_config::check_files(plate_config::Workspace { main, files })?;
    // A plan has no name of its own: call it after its file.
    if config.is_plan() {
        let stem = main.rsplit('/').next().unwrap_or(main).trim_end_matches(".ncl");
        config.name = stem.to_owned();
    }
    Ok(Checked { config, evaluation, eval: t0.elapsed() })
}

impl Checked {
    /// Solve it, with `seed` instead of the file's if there is one.
    pub fn solve(mut self, view: View, seed: Option<u64>) -> Result<Solved, Vec<Diagnostic>> {
        if let Some(seed) = seed {
            self.config.reseed(seed);
        }
        solve(&self.config, Some(&self.evaluation), &view, self.eval)
    }
}

pub fn run_input(input: Input) -> Result<Solved, Vec<Diagnostic>> {
    let t0 = Instant::now();
    match input {
        Input::Source { source } => {
            let (config, evaluation) = plate_config::check(&source)?;
            solve(&config, Some(&evaluation), &View::Default, t0.elapsed())
        }
        Input::Files { main, files, view, seed } => check_files(&main, &files)?.solve(view, seed),
        Input::Config { config, view } => {
            let config = Config::from_value(config).map_err(|err| {
                let path = err.path.join(".");
                let message = if path.is_empty() { err.message } else { format!("{path}: {}", err.message) };
                vec![Diagnostic::error(message)]
            })?;
            solve(&config, None, &view, t0.elapsed())
        }
    }
}

fn solve(
    config: &Config,
    evaluation: Option<&plate_config::Evaluation>,
    view: &View,
    eval: Duration,
) -> Result<Solved, Vec<Diagnostic>> {
    let t1 = Instant::now();
    let solver = solver();
    let solution = solver.solve_view(config, view).map_err(|e| vec![Diagnostic::error(e.to_string())])?;
    let solve_time = t1.elapsed();
    // With the source at hand, point solver warnings at the text they concern.
    let mut diagnostics = solution.diagnostics;
    if let Some(evaluation) = evaluation {
        for d in &mut diagnostics {
            if let Some(pointer) = &d.pointer {
                if let Some(s) = plate_config::sources::lookup(&evaluation.sources, pointer) {
                    d.span = Some(s.span);
                    d.file = Some(s.file.clone());
                }
            }
        }
    }
    Ok(Solved {
        layout: solution.layout,
        diagnostics,
        report: solution.report,
        solver: solver.name(),
        timings: Timings { eval_ms: ms(eval), solve_ms: ms(solve_time) },
    })
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1000.0 * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_once_solves_like_a_run_and_names_a_plan_after_its_file() {
        let source = "{ spaces = { room = { area = 12 } } }";
        let files = std::collections::BTreeMap::from([("plans/room.ncl".to_owned(), source.to_owned())]);
        let checked = check_files("plans/room.ncl", &files).unwrap();
        assert_eq!(checked.config.name, "room");
        let once = checked.solve(View::Default, Some(3)).unwrap();
        let run = run_files("plans/room.ncl", files, Some(3)).unwrap();
        assert_eq!(serde_json::to_string(&once.layout).unwrap(), serde_json::to_string(&run.layout).unwrap());
    }
}
