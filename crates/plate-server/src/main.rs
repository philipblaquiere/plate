//! `plate` command line.
//!
//! ```text
//! plate serve [--port 7878] [--web web/dist] [--workspace workspace]
//!                                               start the API, the web app and the file browser
//! plate check  <file.ncl> [-v] [--seed N]       solve and print diagnostics (imports resolve
//!                                               against the file's folder); --seed tries another
//!       [--seeds 1..10]                         one line per seed (a sweep), and a tally
//!       [--json]                                the report, counts, timings and diagnostics as JSON
//!       [--strict]                              exit 1 unless every requirement is met
//!       [--case N | --slot slot.json | --in plate.ncl:space]
//!                                               a plan file: solve preview case N, or in a slot
//!                                               (`plate slot`), or in the slot a plate gives a space
//! plate slot   <plate.ncl> <space> [--seed N]   print the slot a plate gave a space (JSON)
//! plate export <file.ncl> <dxf|svg|json> [-o out] [--seed N] [--case N | --slot … | --in …]
//! plate eval   <file.ncl>                       print the checked configuration data (JSON)
//! plate schema                                  print the Nickel contract generated from the schema
//! ```

use std::{path::PathBuf, process::ExitCode};

use plate_core::{Diagnostic, Severity};

const USAGE: &str = "usage:
  plate serve [--port 7878] [--web web/dist] [--workspace workspace]
  plate check <file.ncl> [-v] [--seed N | --seeds 1..10] [--json] [--strict]
              [--case N | --slot slot.json | --in plate.ncl:space]
  plate slot <plate.ncl> <space> [--seed N]
  plate export <file.ncl> <dxf|svg|json> [-o <out>] [--seed N] [--case N | --slot … | --in …]
  plate eval <file.ncl>
  plate schema";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("serve") => serve(args.get(1..).unwrap_or_default()),
        Some("check") => check(&args[1..]),
        Some("slot") => slot(&args[1..]),
        Some("export") => export(&args[1..]),
        Some("eval") => eval(&args[1..]),
        Some("schema") => plate_config::compiled_schema().map(|c| println!("{}", c.nickel)),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn serve(args: &[String]) -> Result<(), String> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "plate=info,plate_server=info,tower_http=warn".into()),
        )
        .init();

    let port: u16 = flag(args, "--port")
        .or_else(|| std::env::var("PLATE_PORT").ok())
        .map(|p| p.parse().map_err(|_| format!("invalid port `{p}`")))
        .transpose()?
        .unwrap_or(7878);
    let web_dir = flag(args, "--web").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("web/dist"));
    let ws_dir = flag(args, "--workspace")
        .or_else(|| std::env::var("PLATE_WORKSPACE").ok())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("workspace"));
    let projects = plate_server::projects::Projects::open(&ws_dir)
        .map_err(|e| format!("can't open workspace {}: {e}", ws_dir.display()))?;

    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let has_web = web_dir.join("index.html").exists();
        let root = projects.root().canonicalize().unwrap_or_else(|_| projects.root().to_owned());
        let app = plate_server::app_with(Some(web_dir.clone()), Some(projects))
            .layer(tower_http::trace::TraceLayer::new_for_http());
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let listener =
            tokio::net::TcpListener::bind(addr).await.map_err(|e| format!("can't bind {addr}: {e}"))?;
        tracing::info!("plate API listening on http://{addr}/api");
        tracing::info!("workspace: {}", root.display());
        if has_web {
            tracing::info!("serving web app from {} at http://{addr}", web_dir.display());
        }
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
            .map_err(|e| e.to_string())
    })
}

/// The file and every `.ncl` file it imports (following imports, `..` included), keyed by path
/// relative to the folder they all share, so imports resolve as they do in the app.
fn with_siblings(path: &str) -> Result<Files, String> {
    use std::path::{Path, PathBuf};
    let start = std::fs::canonicalize(path).map_err(|e| format!("can't read {path}: {e}"))?;
    let mut found: Vec<(PathBuf, String)> = Vec::new();
    let mut todo = vec![start.clone()];
    while let Some(file) = todo.pop() {
        if found.iter().any(|(p, _)| *p == file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        let dir = file.parent().unwrap_or(Path::new("/")).to_path_buf();
        for (k, _) in text.match_indices("import \"") {
            let rest = &text[k + 8..];
            if let Some(end) = rest.find('"') {
                if let Ok(p) = std::fs::canonicalize(import_target(&dir, &rest[..end])) {
                    todo.push(p);
                }
            }
        }
        found.push((file, text));
    }
    // The deepest folder holding all of them.
    let mut root = start.parent().unwrap_or(Path::new("/")).to_path_buf();
    while !found.iter().all(|(p, _)| p.starts_with(&root)) {
        if !root.pop() {
            break;
        }
    }
    // Other files in the main file's folder too, as before (a plan opened on its own).
    let siblings = plate_server::workspace::Workspace::at(start.parent().unwrap_or(Path::new("/")))
        .sources()
        .unwrap_or_default();
    let rel = |p: &Path| p.strip_prefix(&root).unwrap_or(p).to_string_lossy().replace('\\', "/");
    let main_dir = rel(start.parent().unwrap_or(Path::new("/")));
    let mut files = std::collections::BTreeMap::new();
    for (name, text) in siblings {
        let key = if main_dir.is_empty() { name } else { format!("{main_dir}/{name}") };
        files.insert(key, text);
    }
    for (p, text) in found {
        files.insert(rel(&p), text);
    }
    Ok(Files { main: rel(&start), files, root })
}

/// Where an import points, from the importing file's folder. Walked part by part because
/// `canonicalize` gives `\\?\C:\…` paths on Windows, where `/` and `..` aren't understood.
fn import_target(dir: &std::path::Path, spec: &str) -> PathBuf {
    let mut out = dir.to_path_buf();
    for part in spec.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            part => out.push(part),
        }
    }
    out
}

/// A file to solve, the files it can import (relative to `root`), and that folder.
struct Files {
    main: String,
    files: std::collections::BTreeMap<String, String>,
    root: PathBuf,
}

/// Solve a file, with `--seed N` in `args` overriding its seed. Also gives back where its
/// imports live, to name them in diagnostics.
fn run_file(path: &str, args: &[String]) -> Result<(plate_server::engine::Solved, PathBuf), String> {
    let seed = seed_flag(args)?;
    let Files { main, files, root } = with_siblings(path)?;
    plate_server::engine::run_files(&main, files, seed)
        .map(|s| (s, root.clone()))
        .map_err(|d| print_diagnostics(path, &root, &d))
}

fn seed_flag(args: &[String]) -> Result<Option<u64>, String> {
    flag(args, "--seed").map(|s| s.parse::<u64>().map_err(|_| format!("invalid seed `{s}`"))).transpose()
}

fn print_diagnostics(path: &str, root: &std::path::Path, diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|d| {
            let at = d.span.map(|s| format!(":{}:{}", s.start_line, s.start_col)).unwrap_or_default();
            // Diagnostics name the file they're in (an import, maybe), from where we are.
            let path = diagnostic_path(path, root, d);
            let severity = match d.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::Info => "info",
            };
            let notes: String = d.notes.iter().map(|n| format!("\n  note: {n}")).collect();
            format!("{path}{at}: {severity}: {}{notes}", d.message)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn check(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or_else(|| USAGE.to_owned())?;
    // A library (a service, a set of assemblies) is checked, not drawn.
    let Files { main, files, root } = with_siblings(path)?;
    let checked =
        plate_server::engine::check_files(&main, &files).map_err(|d| print_diagnostics(path, &root, &d))?;
    let ev = &checked.evaluation;
    let name = main.rsplit('/').next().unwrap_or(&main).trim_end_matches(".ncl");
    let len = |k: &str| {
        ev.value
            .get(k)
            .map_or(0, |v| v.as_object().map_or_else(|| v.as_array().map_or(0, Vec::len), |o| o.len()))
    };
    match ev.kind {
        plate_config::schema::FileKind::Service
            if ev.value.as_object().is_some_and(|o| o.keys().all(|k| k == "params" || k == "label")) =>
        {
            let names: Vec<&String> =
                ev.value["params"].as_object().map(|o| o.keys().collect()).unwrap_or_default();
            println!(
                "ok: numbers for a service in \"{name}\": {}",
                names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            );
            return Ok(());
        }
        plate_config::schema::FileKind::Service => {
            println!(
                "ok: service \"{name}\": {} parts, {} placements, {} networks",
                len("parts"),
                len("place"),
                len("connect")
            );
            return Ok(());
        }
        plate_config::schema::FileKind::Assemblies => {
            let names: Vec<&String> = ev.value.as_object().map(|o| o.keys().collect()).unwrap_or_default();
            println!(
                "ok: {} assemblies in \"{name}\": {}",
                names.len(),
                names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            );
            return Ok(());
        }
        _ => {}
    }
    let json = args.iter().any(|a| a == "--json");
    let strict = args.iter().any(|a| a == "--strict");
    if let Some(spec) = flag(args, "--seeds") {
        return sweep(path, args, &spec, json, strict);
    }
    let seed = seed_flag(args)?;
    let (solved, root) = solve_once(path, args, Some(checked), seed)?;
    if json {
        let v = report_json(path, &root, &solved, seed);
        println!("{}", serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?);
    } else {
        print_report(path, &root, &solved, args.iter().any(|a| a == "-v" || a == "--verbose"));
    }
    if strict && !met(&solved) {
        return Err(format!("{path}: not every requirement is met"));
    }
    Ok(())
}

/// Every requirement met and no floor left over.
fn met(solved: &plate_server::engine::Solved) -> bool {
    solved.report.unmet == 0 && solved.report.leftover < 1e-6
}

/// What a plan file is solved in: `--case N` (a preview case), `--slot slot.json` (a slot saved
/// with `plate slot`) or `--in plate.ncl:space` (the slot that plate gives that space, solved
/// now at `seed`). Plates ignore it.
fn view_flag(args: &[String], seed: Option<u64>) -> Result<plate_core::View, String> {
    use plate_core::View;
    if let Some(n) = flag(args, "--case") {
        return n.parse().map(View::Case).map_err(|_| format!("invalid case `{n}`"));
    }
    if let Some(file) = flag(args, "--slot") {
        let text = std::fs::read_to_string(&file).map_err(|e| format!("can't read {file}: {e}"))?;
        return serde_json::from_str(&text).map(View::Slot).map_err(|e| format!("{file}: not a slot: {e}"));
    }
    if let Some(spec) = flag(args, "--in") {
        let (plate, space) =
            spec.rsplit_once(':').ok_or_else(|| format!("`--in {spec}`: expected plate.ncl:space"))?;
        return slot_of(plate, space, seed).map(View::Slot);
    }
    Ok(View::Default)
}

/// The slot a plate (solved at `seed`, or its own) gives one of its spaces.
fn slot_of(plate: &str, space: &str, seed: Option<u64>) -> Result<plate_core::layout::SlotInfo, String> {
    let (solved, _) =
        run_file(plate, &seed.map(|s| vec!["--seed".to_owned(), s.to_string()]).unwrap_or_default())?;
    let spaces: Vec<_> = solved
        .layout
        .elements
        .iter()
        .filter_map(|e| Some((e.id.as_str(), e.space.as_ref()?.slot.as_ref()?)))
        .collect();
    spaces.iter().find(|(id, _)| *id == space).map(|(_, slot)| (*slot).clone()).ok_or_else(|| {
        let ids: Vec<&str> = spaces.iter().map(|(id, _)| *id).collect();
        format!("{plate}: no space `{space}` with a plan; spaces with one: {}", ids.join(", "))
    })
}

/// Solve `path` (already checked, or checked now) in the view the flags ask for. With `--in`,
/// `seed` is the plate's: the plan is solved with the seed its slot carries, as in the app.
fn solve_once(
    path: &str,
    args: &[String],
    checked: Option<plate_server::engine::Checked>,
    seed: Option<u64>,
) -> Result<(plate_server::engine::Solved, PathBuf), String> {
    let Files { main, files, root } = with_siblings(path)?;
    let checked = match checked {
        Some(c) => c,
        None => plate_server::engine::check_files(&main, &files)
            .map_err(|d| print_diagnostics(path, &root, &d))?,
    };
    let view = view_flag(args, seed)?;
    let seed = if flag(args, "--in").is_some() { None } else { seed };
    let solved = checked.solve(view, seed).map_err(|d| print_diagnostics(path, &root, &d))?;
    Ok((solved, root))
}

/// `--seeds 1..10` (both ends included) or `--seeds 1,4,9`.
fn parse_seeds(spec: &str) -> Result<Vec<u64>, String> {
    let bad = || format!("invalid seeds `{spec}`: write 1..10 or 1,4,9");
    if let Some((a, b)) = spec.split_once("..") {
        let (a, b): (u64, u64) =
            (a.parse().map_err(|_| bad())?, b.trim_start_matches('=').parse().map_err(|_| bad())?);
        if a > b || b - a > 10_000 {
            return Err(bad());
        }
        return Ok((a..=b).collect());
    }
    spec.split(',').map(|s| s.trim().parse().map_err(|_| bad())).collect()
}

/// One line (or JSON object) per seed, and how many met every requirement.
fn sweep(path: &str, args: &[String], spec: &str, json: bool, strict: bool) -> Result<(), String> {
    let seeds = parse_seeds(spec)?;
    let mut rows = Vec::new();
    let mut good = 0;
    for &seed in &seeds {
        let (solved, root) = solve_once(path, args, None, Some(seed))?;
        good += usize::from(met(&solved));
        if json {
            rows.push(report_json(path, &root, &solved, Some(seed)));
        } else {
            let r = &solved.report;
            let u = solved.layout.units.symbol();
            println!(
                "seed {seed:>4}  {:<10} unmet {:<3} preferences {:<3} leftover {:>6.2} {u}²  {:>7.0} ms  {:>9} moves",
                format!("{:?}", r.status).to_lowercase(),
                r.unmet,
                r.preferences,
                r.leftover,
                solved.timings.solve_ms,
                solved.moves,
            );
        }
    }
    if json {
        let v = serde_json::json!({ "file": path, "seeds": seeds.len(), "met": good, "runs": rows });
        println!("{}", serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?);
    } else {
        println!("{good} of {} seeds meet every requirement with no floor left over", seeds.len());
    }
    if strict && good < seeds.len() {
        return Err(format!("{path}: {} of {} seeds fall short", seeds.len() - good, seeds.len()));
    }
    Ok(())
}

/// Where a diagnostic is, from where we are: the file it names (an import, maybe) or `path`.
fn diagnostic_path(path: &str, root: &std::path::Path, d: &Diagnostic) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    match &d.file {
        Some(f) => {
            let full = root.join(f);
            let shown = full.strip_prefix(&cwd).unwrap_or(&full).display().to_string();
            shown.strip_prefix(r"\\?\").map(str::to_owned).unwrap_or(shown)
        }
        None => path.to_owned(),
    }
}

/// `plate check --json`: everything an agent needs to judge a solve without reading text.
fn report_json(
    path: &str,
    root: &std::path::Path,
    solved: &plate_server::engine::Solved,
    seed: Option<u64>,
) -> serde_json::Value {
    use plate_core::layout::ElementKind;
    use serde_json::json;
    let layout = &solved.layout;
    let mut counts = serde_json::Map::new();
    for e in &layout.elements {
        let k =
            serde_json::to_value(e.kind).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        let n = counts.entry(k).or_insert(json!(0));
        *n = json!(n.as_u64().unwrap_or(0) + 1);
    }
    let spaces: Vec<_> = layout
        .elements
        .iter()
        .filter(|e| e.kind == ElementKind::Space)
        .map(|e| {
            let info = e.space.as_ref();
            json!({
                "id": e.id,
                "area": e.area,
                "typical": info.and_then(|i| i.typical.clone()),
                "bounds": plate_core::Bounds::from_points(&e.points),
            })
        })
        .collect();
    let diagnostics: Vec<_> = solved
        .diagnostics
        .iter()
        .map(|d| {
            json!({
                "severity": d.severity,
                "code": d.code,
                "message": d.message,
                "notes": d.notes,
                "file": diagnostic_path(path, root, d),
                "line": d.span.map(|s| s.start_line),
                "column": d.span.map(|s| s.start_col),
                "pointer": d.pointer,
            })
        })
        .collect();
    json!({
        "file": path,
        "name": layout.name,
        "seed": seed,
        "met": met(solved),
        "report": solved.report,
        "units": layout.units,
        "counts": counts,
        "spaces": spaces,
        "circuits": layout.circuits.len(),
        "timings": solved.timings,
        "moves": solved.moves,
        "diagnostics": diagnostics,
    })
}

/// `plate check`'s summary line, spaces with `-v`, then diagnostics.
fn print_report(path: &str, root: &std::path::Path, solved: &plate_server::engine::Solved, verbose: bool) {
    use plate_core::layout::ElementKind;
    let layout = &solved.layout;
    let u = layout.units.symbol();
    let count = |k: ElementKind| layout.elements.iter().filter(|e| e.kind == k).count();
    let start = &layout.elements[0];
    let shape = match start.kind {
        ElementKind::Origin => {
            format!("origin at ({:.2}, {:.2}) {u}", start.points[0].x, start.points[0].y)
        }
        ElementKind::Spine => format!("spine {:.2} {u} long", start.length),
        ElementKind::Slot => {
            let b = plate_core::Bounds::from_points(&start.points);
            format!("plan in a {:.2} × {:.2} {u} slot", b.width(), b.height())
        }
        _ => format!("boundary {:.2} {u}²", start.area.unwrap_or(0.0)),
    };
    let r = &solved.report;
    let mut status = format!("{:?}", r.status).to_lowercase();
    let mut why = Vec::new();
    if r.unmet > 0 {
        why.push(format!("{} unmet", r.unmet));
    }
    if r.preferences > 0 {
        why.push(format!("{} preferences missed", r.preferences));
    }
    if r.leftover > 1e-9 {
        why.push(format!("{:.2} {u}² left over", r.leftover));
    }
    if !why.is_empty() {
        status = format!("{status} ({})", why.join(", "));
    }
    println!(
        "{status}: \"{}\": {shape}; {} spaces, {} doors, {} windows (eval {} ms, solve {} ms, {} moves)",
        layout.name,
        count(ElementKind::Space),
        count(ElementKind::Door),
        count(ElementKind::Window),
        solved.timings.eval_ms,
        solved.timings.solve_ms,
        solved.moves,
    );
    if verbose {
        for e in layout.elements.iter().filter(|e| e.kind == ElementKind::Space) {
            let info = e.space.as_ref().expect("space info");
            let typical = info.typical.as_ref().map(|t| format!(" [type {t}]")).unwrap_or_default();
            println!("  {:<24} {:>8.2} {u}²{typical}", e.id, e.area.unwrap_or(0.0));
        }
    }
    if !solved.diagnostics.is_empty() {
        println!("{}", print_diagnostics(path, root, &solved.diagnostics));
    }
}

/// `plate slot plate.ncl space`: the slot a plate gives a space, to solve its plan file in
/// (`plate check plan.ncl --slot slot.json`) without solving the plate again.
fn slot(args: &[String]) -> Result<(), String> {
    let (plate, space) = match args {
        [plate, space, ..] => (plate, space),
        _ => return Err(USAGE.to_owned()),
    };
    let slot = slot_of(plate, space, seed_flag(args)?)?;
    println!("{}", serde_json::to_string_pretty(&slot).map_err(|e| e.to_string())?);
    Ok(())
}

fn eval(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or_else(|| USAGE.to_owned())?;
    let Files { main, files, root } = with_siblings(path)?;
    let (_, evaluation) = plate_config::check_files(plate_config::Workspace { main: &main, files: &files })
        .map_err(|d| print_diagnostics(path, &root, &d))?;
    println!("{}", serde_json::to_string_pretty(&evaluation.value).map_err(|e| e.to_string())?);
    Ok(())
}

fn export(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or_else(|| USAGE.to_owned())?;
    let format = args
        .get(1)
        .and_then(|f| plate_export::Format::parse(f))
        .ok_or_else(|| format!("expected an export format (dxf, svg, json)\n{USAGE}"))?;
    let (solved, _) = solve_once(path, args, None, seed_flag(args)?)?;
    let bytes = plate_export::export(&solved.layout, format).map_err(|e| e.to_string())?;
    let out = flag(args, "-o")
        .unwrap_or_else(|| format!("{}.{}", plate_export::file_stem(&solved.layout), format.extension()));
    std::fs::write(&out, bytes).map_err(|e| format!("can't write {out}: {e}"))?;
    println!("wrote {out}");
    Ok(())
}
