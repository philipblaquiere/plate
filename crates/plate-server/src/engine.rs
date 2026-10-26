//! The full pipeline in one place: configuration in, layout (or diagnostics) out.
//!
//! The editor evaluates Nickel in the browser and sends checked configuration data
//! ([`Input::Config`]); the CLI and other tools can still send Nickel source ([`Input::Source`]).
//! Either way the data is validated again here: the server never trusts a client.

use std::time::Instant;

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
    run_input(Input::Files { main: main.to_owned(), files, view: View::Default, seed })
}

pub fn run_input(input: Input) -> Result<Solved, Vec<Diagnostic>> {
    let t0 = Instant::now();
    let (config, sources, view) = match input {
        Input::Source { source } => {
            let (config, evaluation) = plate_config::check(&source)?;
            (config, Some(evaluation.sources), View::Default)
        }
        Input::Files { main, files, view, seed } => {
            let (mut config, evaluation) =
                plate_config::check_files(plate_config::Workspace { main: &main, files: &files })?;
            // A plan has no name of its own: call it after its file.
            if config.is_plan() {
                let stem = main.rsplit('/').next().unwrap_or(&main).trim_end_matches(".ncl");
                config.name = stem.to_owned();
            }
            if let Some(seed) = seed {
                config.reseed(seed);
            }
            (config, Some(evaluation.sources), view)
        }
        Input::Config { config, view } => {
            let config = Config::from_value(config).map_err(|err| {
                let path = err.path.join(".");
                let message = if path.is_empty() { err.message } else { format!("{path}: {}", err.message) };
                vec![Diagnostic::error(message)]
            })?;
            (config, None, view)
        }
    };
    let t1 = Instant::now();
    let solver = solver();
    let solution = solver.solve_view(&config, &view).map_err(|e| vec![Diagnostic::error(e.to_string())])?;
    let t2 = Instant::now();
    // With the source at hand, point solver warnings at the text they concern.
    let mut diagnostics = solution.diagnostics;
    if let Some(sources) = &sources {
        for d in &mut diagnostics {
            if let Some(pointer) = &d.pointer {
                if let Some(s) = plate_config::sources::lookup(sources, pointer) {
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
        timings: Timings { eval_ms: ms(t1 - t0), solve_ms: ms(t2 - t1) },
    })
}

fn ms(d: std::time::Duration) -> f64 {
    (d.as_secs_f64() * 1000.0 * 100.0).round() / 100.0
}
