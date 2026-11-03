//! `plate` command line.
//!
//! ```text
//! plate serve [--port 7878] [--web web/dist] [--workspace workspace]
//!                                               start the API, the web app and the file browser
//! plate check  <file.ncl> [-v] [--seed N]       solve and print diagnostics (imports resolve
//!                                               against the file's folder); --seed tries another
//! plate export <file.ncl> <dxf|svg|json> [-o out] [--seed N]
//! plate eval   <file.ncl>                       print the checked configuration data (JSON)
//! plate schema                                  print the Nickel contract generated from the schema
//! ```

use std::{path::PathBuf, process::ExitCode};

use plate_core::{Diagnostic, Severity};

const USAGE: &str = "usage:
  plate serve [--port 7878] [--web web/dist] [--workspace workspace]
  plate check <file.ncl> [-v] [--seed N]
  plate export <file.ncl> <dxf|svg|json> [-o <out>] [--seed N]
  plate eval <file.ncl>
  plate schema";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None | Some("serve") => serve(args.get(1..).unwrap_or_default()),
        Some("check") => check(&args[1..]),
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
    let workspace = plate_server::workspace::Workspace::open(&ws_dir)
        .map_err(|e| format!("can't open workspace {}: {e}", ws_dir.display()))?;

    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    runtime.block_on(async move {
        let has_web = web_dir.join("index.html").exists();
        let root = workspace.root().canonicalize().unwrap_or_else(|_| workspace.root().to_owned());
        let app = plate_server::app_with(Some(web_dir.clone()), Some(workspace))
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
                if let Ok(p) = std::fs::canonicalize(dir.join(&rest[..end])) {
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

/// A file to solve, the files it can import (relative to `root`), and that folder.
struct Files {
    main: String,
    files: std::collections::BTreeMap<String, String>,
    root: PathBuf,
}

/// Solve a file, with `--seed N` in `args` overriding its seed. Also gives back where its
/// imports live, to name them in diagnostics.
fn run_file(path: &str, args: &[String]) -> Result<(plate_server::engine::Solved, PathBuf), String> {
    let seed = flag(args, "--seed")
        .map(|s| s.parse::<u64>().map_err(|_| format!("invalid seed `{s}`")))
        .transpose()?;
    let Files { main, files, root } = with_siblings(path)?;
    plate_server::engine::run_files(&main, files, seed)
        .map(|s| (s, root.clone()))
        .map_err(|d| print_diagnostics(path, &root, &d))
}

fn print_diagnostics(path: &str, root: &std::path::Path, diagnostics: &[Diagnostic]) -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    diagnostics
        .iter()
        .map(|d| {
            let at = d.span.map(|s| format!(":{}:{}", s.start_line, s.start_col)).unwrap_or_default();
            // Diagnostics name the file they're in (an import, maybe), from where we are.
            let path = match &d.file {
                Some(f) => {
                    let full = root.join(f);
                    full.strip_prefix(&cwd).unwrap_or(&full).display().to_string()
                }
                None => path.to_owned(),
            };
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
    let Files { main, files, .. } = with_siblings(path)?;
    if let Ok((_, ev)) = plate_config::check_files(plate_config::Workspace { main: &main, files: &files }) {
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
                let names: Vec<&String> =
                    ev.value.as_object().map(|o| o.keys().collect()).unwrap_or_default();
                println!(
                    "ok: {} assemblies in \"{name}\": {}",
                    names.len(),
                    names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                );
                return Ok(());
            }
            _ => {}
        }
    }
    match run_file(path, args).map_err(|e| vec![plate_core::Diagnostic::error(e)]) {
        Ok((solved, root)) => {
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
                "{status}: \"{}\": {shape}; {} spaces, {} doors, {} windows (eval {} ms, solve {} ms)",
                layout.name,
                count(ElementKind::Space),
                count(ElementKind::Door),
                count(ElementKind::Window),
                solved.timings.eval_ms,
                solved.timings.solve_ms,
            );
            if args.iter().any(|a| a == "-v" || a == "--verbose") {
                for e in layout.elements.iter().filter(|e| e.kind == ElementKind::Space) {
                    let info = e.space.as_ref().expect("space info");
                    let typical = info.typical.as_ref().map(|t| format!(" [type {t}]")).unwrap_or_default();
                    println!("  {:<24} {:>8.2} {u}²{typical}", e.id, e.area.unwrap_or(0.0));
                }
            }
            if !solved.diagnostics.is_empty() {
                println!("{}", print_diagnostics(&args[0], &root, &solved.diagnostics));
            }
            Ok(())
        }
        Err(diags) => Err(diags.into_iter().map(|d| d.message).collect::<Vec<_>>().join("\n")),
    }
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
    let (solved, _) = run_file(path, args)?;
    let bytes = plate_export::export(&solved.layout, format).map_err(|e| e.to_string())?;
    let out = flag(args, "-o")
        .unwrap_or_else(|| format!("{}.{}", plate_export::file_stem(&solved.layout), format.extension()));
    std::fs::write(&out, bytes).map_err(|e| format!("can't write {out}: {e}"))?;
    println!("wrote {out}");
    Ok(())
}
