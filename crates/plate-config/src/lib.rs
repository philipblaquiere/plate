//! plate-config: turns a `.ncl` source string into checked configuration data, or into
//! diagnostics an editor can draw as squiggles.
//!
//! Nickel gives the config language its features (let bindings, functions, recursive
//! records for self-reference, `&` merging with priorities for cascading, imports). The
//! configuration format itself is defined once, as JSON Schema (`schema/plate.schema.json`),
//! and compiled to a Nickel contract ([`schema`]) that is applied to every file. Nickel then
//! checks values, fills in defaults and reports errors at their exact position.
//!
//! This crate compiles to WebAssembly (see `plate-wasm`), so all of this also runs in the
//! browser, before anything reaches the solver.

pub mod schema;
pub mod sources;

pub use sources::SourceSpan;

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::OnceLock;

use nickel_lang_core::{
    cache::{InputFormat, SourcePath},
    error::{Diagnostic as NickelDiagnostic, IntoDiagnostics, LabelStyle},
    eval::cache::CacheImpl,
    files::{FileId, Files},
    program::{Program, ProgramBuilder},
};
use plate_core::{Config, Diagnostic, Span};

/// The configuration format, as JSON Schema.
pub const SCHEMA_JSON: &str = include_str!("../../../schema/plate.schema.json");

/// The `plate` helper library, bound as `plate` in every file.
pub const STDLIB: &str = include_str!("../../../schema/plate.ncl");

/// Where files live inside Nickel. Paths must be absolute: imports are resolved relative to the
/// importing file, and in the browser there is no current directory to resolve against.
const ROOT: &str = "/workspace/";

/// The name a single source is evaluated under when it has no file of its own.
pub const MAIN_FILE: &str = "main.ncl";

/// The contract every main file is checked against, compiled to Nickel once per process: the
/// schema's root for a plate (a file with a `start`), `Plan` for a plan file.
pub fn compiled_schema() -> Result<&'static schema::CompiledSchema, String> {
    static COMPILED: OnceLock<Result<schema::CompiledSchema, String>> = OnceLock::new();
    COMPILED.get_or_init(|| schema::compile_file(schema_json()?, "Plan")).as_ref().map_err(Clone::clone)
}

/// The schema as parsed JSON.
pub fn schema_json() -> Result<&'static serde_json::Value, String> {
    static JSON: OnceLock<Result<serde_json::Value, String>> = OnceLock::new();
    JSON.get_or_init(|| {
        serde_json::from_str(SCHEMA_JSON).map_err(|e| format!("schema/plate.schema.json: {e}"))
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// A set of files to evaluate: the plate file and everything it may import, by path relative to
/// the workspace (`hotel.ncl`, `plans/hotel-room.ncl`).
#[derive(Debug, Clone, Copy)]
pub struct Workspace<'a> {
    pub main: &'a str,
    pub files: &'a BTreeMap<String, String>,
}

/// A user file as Nickel sees it: wrapped so the `plate` library is in scope (and, for the main
/// file, so the schema contract applies to it):
///
/// ```text
/// let plate = <schema/plate.ncl> in
/// let __plate_contract = <compiled schema> in      (main file only)
/// (
/// <user source>
/// ) | __plate_contract                             (main file only)
/// ```
///
/// Nickel only accepts extra contracts from files on disk, which the browser doesn't have, so
/// the contract is applied in the program text instead. Positions are mapped back afterwards.
#[derive(Debug, Clone, Copy)]
struct Mapped {
    start: usize,
    end: usize,
    /// False when the range was outside the user's source and got pinned to its end.
    inside: bool,
}

struct Wrapped<'a> {
    /// Path relative to the workspace.
    path: &'a str,
    /// Absolute path inside Nickel.
    virtual_path: String,
    text: String,
    /// The user's source.
    source: &'a str,
    /// Byte offset of the user's source inside `text`.
    offset: usize,
}

impl<'a> Wrapped<'a> {
    fn new(path: &'a str, source: &'a str, contract: Option<&str>) -> Self {
        let (prefix, suffix) = match contract {
            Some(contract) => (
                format!("let plate = {STDLIB} in\nlet __plate_contract = {contract} in\n(\n"),
                "\n) | __plate_contract\n",
            ),
            None => (format!("let plate = {STDLIB} in\n"), "\n"),
        };
        let text = format!("{prefix}{source}{suffix}");
        Wrapped { path, virtual_path: format!("{ROOT}{path}"), text, source, offset: prefix.len() }
    }

    /// Map a byte range in the wrapped text to the user's source. Ranges inside the generated
    /// prefix don't belong to the user. Ranges after the source (e.g. "unexpected end of
    /// input", or the contract annotation itself) are pinned to its end and flagged.
    fn map(&self, start: usize, end: usize) -> Option<Mapped> {
        if start < self.offset {
            return None;
        }
        let len = self.source.len();
        let s = (start - self.offset).min(len);
        let e = end.saturating_sub(self.offset).clamp(s, len);
        Some(Mapped { start: s, end: e, inside: start - self.offset <= len })
    }
}

/// The wrapped files, and which Nickel file is which.
struct Set<'a> {
    files: Vec<Wrapped<'a>>,
}

impl Set<'_> {
    fn find(&self, nickel_files: &Files, id: FileId) -> Option<&Wrapped<'_>> {
        let name = nickel_files.name(id);
        let text = nickel_files.source(id);
        self.files.iter().find(|w| name == OsStr::new(&w.virtual_path) && text == w.text)
    }
}

/// The evaluated configuration and where each of its values comes from in the source.
#[derive(Debug, Clone)]
pub struct Evaluation {
    pub value: serde_json::Value,
    pub sources: Vec<SourceSpan>,
    /// What the file is: a plate, a plan, or a library (a service, a set of assemblies).
    pub kind: schema::FileKind,
}

/// Evaluate Nickel source, check it against the schema, and export it as plain JSON data.
/// Defaults declared in the schema are filled in.
pub fn evaluate(source: &str) -> Result<Evaluation, Vec<Diagnostic>> {
    let files = BTreeMap::from([(MAIN_FILE.to_owned(), source.to_owned())]);
    evaluate_files(Workspace { main: MAIN_FILE, files: &files })
}

/// Evaluate the main file of a workspace (which may import the others). A plate (a value with a
/// `start`) is checked as a plate; anything else as a plan, which is shown on its own.
pub fn evaluate_files(ws: Workspace) -> Result<Evaluation, Vec<Diagnostic>> {
    let main_source = ws
        .files
        .get(ws.main)
        .ok_or_else(|| vec![Diagnostic::error(format!("{} isn't in the workspace", ws.main))])?;
    let compiled = compiled_schema()
        .map_err(|e| vec![Diagnostic::error(format!("the plate schema is invalid: {e}"))])?;
    let mut set = Set { files: vec![Wrapped::new(ws.main, main_source, Some(&compiled.nickel))] };
    for (path, source) in ws.files {
        if path != ws.main {
            set.files.push(Wrapped::new(path, source, None));
        }
    }
    let mut evaluation = eval_set(&set)?;
    evaluation.kind = schema::FileKind::of(&evaluation.value);
    if let Ok(json) = schema_json() {
        match evaluation.kind.definition() {
            None => schema::apply_defaults(json, &mut evaluation.value),
            Some(def) => schema::apply_defaults_def(json, def, &mut evaluation.value),
        }
    }
    Ok(evaluation)
}

/// Evaluate Nickel source without the schema. Mostly useful for tests and tooling.
pub fn evaluate_unchecked(source: &str) -> Result<serde_json::Value, Vec<Diagnostic>> {
    let set = Set {
        files: vec![Wrapped {
            path: MAIN_FILE,
            virtual_path: format!("{ROOT}{MAIN_FILE}"),
            text: source.to_owned(),
            source,
            offset: 0,
        }],
    };
    eval_set(&set).map(|e| e.value)
}

/// Evaluate, then check the data against the engine's types and geometric rules (repeated
/// points, a boundary crossing itself, …). Every error carries the position of the offending
/// value. This is what the browser runs on every edit.
pub fn check(source: &str) -> Result<(Config, Evaluation), Vec<Diagnostic>> {
    let files = BTreeMap::from([(MAIN_FILE.to_owned(), source.to_owned())]);
    check_files(Workspace { main: MAIN_FILE, files: &files })
}

/// [`check`] for the main file of a workspace.
pub fn check_files(ws: Workspace) -> Result<(Config, Evaluation), Vec<Diagnostic>> {
    let evaluation = evaluate_files(ws)?;
    // A library is checked as what it is: a service's placements must name its parts, say.
    let library = match evaluation.kind {
        schema::FileKind::Service => {
            serde_json::from_value::<plate_core::service::Service>(evaluation.value.clone())
                .map_err(|e| ("".to_owned(), e.to_string()))
                .and_then(|s| s.validate(""))
        }
        schema::FileKind::Assemblies => serde_json::from_value::<
            BTreeMap<String, plate_core::assembly::Assembly>,
        >(evaluation.value.clone())
        .map_err(|e| ("".to_owned(), e.to_string()))
        .and_then(|all| {
            all.iter().try_for_each(|(k, a)| a.validate(&format!("/{}", plate_core::plan::escape_pointer(k))))
        }),
        _ => Ok(()),
    };
    if let Err((pointer, message)) = library {
        let path: Vec<String> = pointer.split('/').filter(|s| !s.is_empty()).map(str::to_owned).collect();
        let mut d = Diagnostic::error(describe_path(&path, &message));
        if let Some(s) = sources::lookup(&evaluation.sources, &pointer) {
            d = d.with_span(Some(s.span)).in_file(s.file.clone());
        }
        return Err(vec![d]);
    }
    // A library has no plate fields: its keys are its own (an assembly can be called `name`).
    let value = match evaluation.kind {
        schema::FileKind::Service | schema::FileKind::Assemblies => serde_json::json!({}),
        _ => evaluation.value.clone(),
    };
    match Config::from_value(value) {
        Ok(config) => Ok((config, evaluation)),
        Err(err) => {
            let main = &ws.files[ws.main];
            let located = sources::lookup(&evaluation.sources, &err.pointer())
                .map(|s| (s.file.clone(), s.span))
                .or_else(|| {
                    locate(main, &err.path, &err.message)
                        .map(|(a, b)| (ws.main.to_owned(), Span::from_offsets(main, a, b)))
                });
            let mut d = Diagnostic::error(describe_path(&err.path, &err.message));
            if let Some((file, span)) = located {
                d = d.with_span(Some(span)).in_file(file);
            }
            Err(vec![d])
        }
    }
}

/// Is this file a plate (it has a `start`), as opposed to a plan or a library to import?
pub fn is_plate(source: &str) -> bool {
    source.lines().any(|line| {
        let line = line.trim_start();
        line.strip_prefix("start").is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

/// Evaluate and validate against the plate schema.
pub fn load(source: &str) -> Result<Config, Vec<Diagnostic>> {
    check(source).map(|(config, _)| config)
}

/// `start.points[2]: message`, or just the message for messages that already name the place.
fn describe_path(path: &[String], message: &str) -> String {
    let mut out = String::new();
    for seg in path {
        if seg.parse::<usize>().is_ok() {
            out.push_str(&format!("[{seg}]"));
        } else {
            if !out.is_empty() {
                out.push('.');
            }
            out.push_str(seg);
        }
    }
    if out.is_empty() {
        message.to_owned()
    } else {
        format!("{out}: {message}")
    }
}

fn eval_set(set: &Set) -> Result<Evaluation, Vec<Diagnostic>> {
    let main = &set.files[0];
    let mut program: Program<CacheImpl> = ProgramBuilder::new()
        .add_source_string(main.text.clone(), main.virtual_path.clone())
        .build()
        .map_err(|err| vec![Diagnostic::error(err.to_string())])?;

    let fail = |program: &Program<CacheImpl>, err: nickel_lang_core::error::Error| {
        let mut files = program.files();
        let diagnostics = err.into_diagnostics(&mut files);
        convert_all(&files, diagnostics, set)
    };

    // The other files become in-memory sources that `import` finds by path, without a file
    // system (the browser has none). They're registered once the main file is compiled (that's
    // when the cache can be reached) and before imports are resolved.
    if let Err(err) = program.compile() {
        return Err(fail(&program, err));
    }
    let extra: Vec<(PathBuf, String)> =
        set.files[1..].iter().map(|w| (PathBuf::from(&w.virtual_path), w.text.clone())).collect();
    program
        .custom_transform(0, |cache, _, value| {
            for (path, text) in &extra {
                cache.sources.add_string(SourcePath::Path(path.clone(), InputFormat::Nickel), text.clone());
            }
            Ok::<_, std::convert::Infallible>(value)
        })
        .map_err(|_| vec![Diagnostic::error("couldn't register the workspace files")])?;

    let value = match program.eval_full_for_export() {
        Ok(value) => value,
        Err(err) => return Err(fail(&program, err)),
    };

    let files = program.files();
    let map = |id: FileId, a: usize, b: usize| -> Option<(usize, usize, usize)> {
        let w = set.find(&files, id)?;
        let index = set.files.iter().position(|x| std::ptr::eq(x, w))?;
        w.map(a, b).filter(|m| m.inside).map(|m| (index, m.start, m.end))
    };
    let user: Vec<(&str, &str)> = set.files.iter().map(|w| (w.path, w.source)).collect();
    let mut walker = sources::Walker { table: program.pos_table(), map: &map, files: &user, out: Vec::new() };
    walker.walk(&value, "", 0);
    let sources = walker.out;

    let value = serde_json::to_value(&value).map_err(|err| {
        vec![Diagnostic::error(format!("the configuration can't be exported as data: {err}"))]
    })?;
    Ok(Evaluation { value, sources, kind: schema::FileKind::Plan })
}

/// Reformat Nickel source with the official formatter.
#[cfg(feature = "format")]
pub fn format(source: &str) -> Result<String, Diagnostic> {
    let mut out = Vec::new();
    nickel_lang_core::format::format(std::io::Cursor::new(source.as_bytes()), &mut out)
        .map_err(|err| Diagnostic::error(format!("can't format this file: {err:?}")))?;
    String::from_utf8(out).map_err(|err| Diagnostic::error(err.to_string()))
}

fn convert_all(files: &Files, diagnostics: Vec<NickelDiagnostic<FileId>>, set: &Set) -> Vec<Diagnostic> {
    // Nickel emits one diagnostic per call-stack frame; frames without a message are context.
    let mut out: Vec<Diagnostic> =
        diagnostics.into_iter().filter(|d| !d.message.is_empty()).map(|d| convert(files, d, set)).collect();
    if out.is_empty() {
        out.push(Diagnostic::error("evaluation failed"));
    }
    out
}

fn convert(files: &Files, diagnostic: NickelDiagnostic<FileId>, set: &Set) -> Diagnostic {
    // A label's range in one of the user's files, if it points there.
    let user_range = |l: &nickel_lang_core::error::Label<FileId>| -> Option<(usize, Mapped)> {
        let w = set.find(files, l.file_id)?;
        let index = set.files.iter().position(|x| std::ptr::eq(x, w))?;
        w.map(l.range.start, l.range.end).map(|m| (index, m))
    };

    // Prefer a primary label inside the user's files, then any label inside them, then a label
    // pinned to the end of one (e.g. a missing closing brace).
    let labels = &diagnostic.labels;
    let mapped: Vec<(LabelStyle, usize, Mapped)> =
        labels.iter().filter_map(|l| user_range(l).map(|(i, m)| (l.style, i, m))).collect();
    let found = mapped
        .iter()
        .find(|(style, _, m)| *style == LabelStyle::Primary && m.inside)
        .or_else(|| mapped.iter().find(|(_, _, m)| m.inside))
        .or_else(|| mapped.first())
        .map(|(_, i, m)| (*i, *m));

    let message = tidy_message(&diagnostic.message);
    let index = found.map_or(0, |(i, _)| i);
    let file = &set.files[index];
    let range = narrow(&message, file.source, found.map(|(_, m)| m));

    let mut out = Diagnostic::error(message)
        .with_span(range.map(|m| Span::from_offsets(file.source, m.start, m.end)))
        .in_file(file.path);

    // Generic pointers ("expected type", "applied to this expression") and labels inside the
    // generated contract code mean nothing to the user; keep real hints.
    const GENERIC: &[&str] =
        &["expected type", "applied to this expression", "evaluated to this expression", "bound here"];
    for l in labels {
        let useful = !l.message.is_empty() && !GENERIC.contains(&l.message.as_str());
        if useful && user_range(l).is_some_and(|(_, m)| m.inside) {
            out = out.with_note(l.message.clone());
        }
    }
    for note in diagnostic.notes {
        if !note.starts_with("The record contract might also be too strict") {
            out = out.with_note(note);
        }
    }
    out
}

/// Point at the most specific text the message names: the extra field in "extra field
/// `colour`", the tag in "unknown mode 'blob", or the record missing a field.
fn narrow(message: &str, source: &str, range: Option<Mapped>) -> Option<Mapped> {
    let quoted = |prefix: &str, close: char| {
        message.split(prefix).nth(1).and_then(|r| r.split(close).next()).filter(|s| !s.is_empty())
    };
    let within = |m: Option<Mapped>| match m {
        Some(m) if m.inside => (m.start, m.end),
        _ => (0, source.len()),
    };
    let found = |start: usize, hit: Option<(usize, usize)>| {
        hit.map(|(a, b)| Mapped { start: start + a, end: start + b, inside: true })
    };

    if let Some(field) = quoted("extra field `", '`') {
        let (a, b) = within(range);
        return found(a, find_field(&source[a..b], field, 0)).or(range);
    }
    // "unknown mode 'blob: …" (from a tagged union in the schema)
    let unknown_tag = message
        .split("unknown ")
        .nth(1)
        .and_then(|r| r.split_once(" '"))
        .and_then(|(_, r)| r.split(':').next())
        .filter(|t| !t.is_empty());
    if let Some(tag) = unknown_tag {
        let (a, b) = within(range);
        let needle = format!("'{tag}");
        return found(a, source[a..b].find(&needle).map(|i| (i, i + needle.len()))).or(range);
    }
    if range.is_none() && message.starts_with("missing definition for `") {
        // Nickel doesn't say which record; the top-level opening brace is the best guess.
        return source.find('{').map(|i| Mapped { start: i, end: i + 1, inside: true });
    }
    range
}

/// Nickel reports contract failures over two lines: "contract broken by the value of `size`"
/// then the contract's own message. Fold that into "size: <message>".
fn tidy_message(message: &str) -> String {
    // Imports are resolved inside a virtual folder; its name means nothing to the user.
    let message = &message.replace(&format!(" (looked in [{}])", ROOT.trim_end_matches('/')), "");
    let mut lines = message.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next().unwrap_or_default();
    let rest: Vec<&str> = lines.collect();
    if rest.is_empty() {
        return first.to_owned();
    }
    let detail = rest.join(" ");
    if let Some(field) =
        first.strip_prefix("contract broken by the value of `").and_then(|r| r.strip_suffix('`'))
    {
        format!("{field}: {detail}")
    } else if first.starts_with("contract broken by") {
        detail
    } else {
        format!("{first}: {detail}")
    }
}

/// Best-effort location of a schema error in the source text, so it gets a squiggle too.
///
/// Walks the field path (e.g. `outline.size`) as deep as it can be found. If the message quotes
/// the offending string value (`can't read "30 yd"`), the literal itself is preferred.
fn locate(source: &str, path: &[String], message: &str) -> Option<(usize, usize)> {
    let mut from = 0;
    let mut found = None;
    for key in path {
        match find_field(source, key, from) {
            Some((start, end)) => {
                found = Some((start, end));
                from = start;
            }
            None => break,
        }
    }
    let quoted = message.split('"').nth(1).filter(|q| !q.is_empty()).map(|q| format!("\"{q}\""));
    if let Some(literal) = quoted {
        let hit = source[from..].find(&literal).map(|i| i + from).or_else(|| source.find(&literal));
        if let Some(start) = hit {
            return Some((start, start + literal.len()));
        }
    }
    found
}

/// Find `key` used as a field name (`key =` or `key.`) at or after `from`.
fn find_field(source: &str, key: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut search = from;
    while let Some(rel) = source.get(search..)?.find(key) {
        let start = search + rel;
        let end = start + key.len();
        let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
        let rest = source[end..].trim_start();
        let after_ok = rest.starts_with('=') && !rest.starts_with("==")
            || rest.starts_with('.')
            || rest.starts_with('|');
        if before_ok && after_ok {
            return Some((start, end));
        }
        search = end;
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'\''
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDARY: &str = r#"
let w = 30 in
{
  name = "Typical floor",
  units = 'm,
  start = {
    mode = 'boundary,
    points = [
      [0, 0],
      [w, 0],
      [w, 20],
      [0, 20],
    ],
  },
}
"#;

    /// (line, start col, end col, message) of the first diagnostic.
    fn squiggle(src: &str) -> (u32, u32, u32, String) {
        let errs = check(src).unwrap_err();
        let span = errs[0].span.unwrap_or_else(|| panic!("no span: {errs:?}"));
        assert_eq!(span.start_line, span.end_line, "{errs:?}");
        (span.start_line, span.start_col, span.end_col, errs[0].message.clone())
    }

    #[test]
    fn evaluates_a_boundary() {
        let cfg = load(BOUNDARY).unwrap();
        assert_eq!(cfg.name, "Typical floor");
        assert_eq!(cfg.start_points()[2], plate_core::Point::new(30.0, 20.0));
    }

    #[test]
    fn supports_self_reference_and_merging() {
        let src = r#"
let base = { name | default = "Base", start = { mode = 'spine, points | default = [[0, 0]] } } in
base & { name = "Override", start.points = base.start.points @ [[10, 0]] }
"#;
        let cfg = load(src).unwrap();
        assert_eq!(cfg.name, "Override");
        assert_eq!(cfg.start_points().len(), 2);
    }

    #[test]
    fn syntax_error_has_span() {
        let errs = evaluate("{ start = { mode = 'spine, points = } }").unwrap_err();
        assert!(errs[0].span.is_some(), "{errs:?}");
    }

    #[test]
    fn unbound_identifier_points_at_it() {
        let errs = evaluate("{\n  start = { mode = 'spine, points = [[0, sid]] },\n}").unwrap_err();
        let span = errs[0].span.expect("span");
        assert_eq!((span.start_line, span.start_col), (2, 42), "{errs:?}");
    }

    #[test]
    fn schema_errors_point_at_the_exact_value() {
        // A bad coordinate inside a point.
        let (line, a, b, msg) =
            squiggle("{\n  start = {\n    mode = 'spine,\n    points = [[0, 0], [1, \"2 yd\"]],\n  },\n}");
        assert_eq!((line, a, b), (4, 27, 33), "{msg}");
        assert!(msg.contains("expected a length"), "{msg}");

        // A point with three coordinates.
        let (line, a, b, msg) =
            squiggle("{\n  start = {\n    mode = 'spine,\n    points = [[0, 0, 0]],\n  },\n}");
        assert_eq!((line, a, b), (4, 15, 24), "{msg}");
        assert!(msg.contains("expected a point: [x, y]"), "{msg}");

        let (line, a, b, msg) =
            squiggle("{\n  start = { mode = 'spine, points = [[0, 0]] },\n  units = 'yd,\n}");
        assert_eq!((line, a, b), (3, 11, 14), "{msg}");
        assert_eq!(msg, "units: expected one of 'mm, 'cm, 'm, 'in, 'ft");
    }

    #[test]
    fn too_few_boundary_points() {
        let (line, _, _, msg) =
            squiggle("{\n  start = {\n    mode = 'boundary,\n    points = [[0, 0], [1, 0]],\n  },\n}");
        assert_eq!(line, 4, "{msg}");
        assert!(msg.contains("at least 3 points"), "{msg}");
    }

    #[test]
    fn typos_and_unknown_modes_point_at_the_name() {
        let (line, a, b, msg) = squiggle("{\n  strat = { mode = 'spine, points = [[0, 0]] },\n}");
        assert_eq!((line, a, b), (2, 3, 8), "{msg}");
        let (line, a, b, msg) = squiggle("{\n  start = { mode = 'blob, points = [[0, 0]] },\n}");
        assert_eq!((line, a, b), (2, 20, 25), "{msg}");
        assert!(msg.contains("one of 'boundary, 'spine"), "{msg}");
    }

    #[test]
    fn geometric_errors_point_at_the_offending_point() {
        // A bow tie: the error is reported on the 3rd point.
        let src = "{\n  start = {\n    mode = 'boundary,\n    points = [\n      [0, 0],\n      [10, 10],\n      [10, 0],\n      [0, 10],\n    ],\n  },\n}";
        let (line, a, b, msg) = squiggle(src);
        assert_eq!((line, a, b), (7, 7, 14), "{msg}");
        assert!(msg.contains("crosses itself"), "{msg}");
    }

    #[test]
    fn sources_map_values_to_their_text() {
        let (_, evaluation) = check(BOUNDARY).unwrap();
        let find =
            |p: &str| evaluation.sources.iter().find(|s| s.pointer == p).unwrap_or_else(|| panic!("{p}"));
        // A literal inside the array.
        let y = find("/start/points/2/1");
        assert!(y.literal);
        assert_eq!((y.span.start_line, y.span.start_col, y.span.end_col), (11, 11, 13));
        // A value that comes from a let binding points at the binding's literal.
        let x = find("/start/points/1/0");
        assert!(x.literal);
        assert_eq!((x.span.start_line, x.span.start_col), (2, 9));
        // The array literal itself, for inserting and removing points.
        let points = find("/start/points");
        assert_eq!(points.kind, "array");
        assert!(points.literal);
        assert_eq!(points.span.start_line, 8);
    }

    #[test]
    fn schema_defaults_are_filled_in_without_clashing_with_user_defaults() {
        let v = evaluate("{ start = { mode = 'spine, points = [[0, 0]] } }").unwrap().value;
        assert_eq!(v["name"], "Untitled plate");
        assert_eq!(v["system"], "metric");

        let v = evaluate("{ name | default = \"Mine\", start = { mode = 'spine, points = [[0, 0]] } }")
            .unwrap()
            .value;
        assert_eq!(v["name"], "Mine");
    }

    /// Every `.ncl` file under `examples/`, by path relative to it (`plans/wing.ncl`).
    fn examples() -> BTreeMap<String, String> {
        fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut BTreeMap<String, String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else if path.extension().is_some_and(|e| e == "ncl") {
                    let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
                    out.insert(rel, std::fs::read_to_string(&path).unwrap());
                }
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let mut files = BTreeMap::new();
        walk(&dir, &dir, &mut files);
        files
    }

    #[test]
    fn every_example_passes_the_schema_and_the_rust_types() {
        let files = examples();
        let mut count = 0;
        // Plate files are checked as plates (with whatever they import); plans are imported.
        for name in files.iter().filter(|(_, s)| is_plate(s)).map(|(n, _)| n) {
            count += 1;
            let (config, evaluation) = check_files(Workspace { main: name, files: &files })
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
            // Every example meets every requirement at the seed it ships with, with no floor
            // left over (preferences may be missed: the report says so).
            let solved = plate_core::solve::solve(&config);
            let found: Vec<&str> = solved
                .diagnostics
                .iter()
                .filter(|d| d.code != Some(plate_core::diagnostic::Code::Preference))
                .map(|d| d.message.as_str())
                .collect();
            assert!(solved.report.unmet == 0 && solved.report.leftover < 1e-6, "{name}: {found:?}");
            assert!(solved.report.status != plate_core::layout::Status::Incomplete, "{name}");
            // The data also passes a standard JSON Schema validator, which keeps the schema
            // honest JSON Schema rather than something only our compiler reads.
            let validator = jsonschema::validator_for(schema_json().unwrap()).unwrap();
            if let Err(e) = validator.validate(&evaluation.value) {
                panic!("{name}: {e}");
            }
        }
        assert!(count >= 3);
        assert!(files.keys().any(|k| !is_plate(&files[k])), "an example plan file");

        // Plan files are checked on their own too (they're previewed), against `Plan`.
        let schema = schema_json().unwrap();
        let plan_schema = serde_json::json!({ "$ref": "#/$defs/Plan", "$defs": schema["$defs"].clone() });
        let validator = jsonschema::validator_for(&plan_schema).unwrap();
        let mut libraries = 0;
        for name in files.iter().filter(|(_, s)| !is_plate(s)).map(|(n, _)| n) {
            let (config, evaluation) = check_files(Workspace { main: name, files: &files })
                .unwrap_or_else(|e| panic!("{name}: {e:?}"));
            // A library (a service, a set of assemblies) is checked against its own definition.
            if let Some(def @ ("Service" | "Assemblies")) = evaluation.kind.definition() {
                libraries += 1;
                let def_schema =
                    serde_json::json!({ "$ref": format!("#/$defs/{def}"), "$defs": schema["$defs"].clone() });
                let v = jsonschema::validator_for(&def_schema).unwrap();
                if let Err(e) = v.validate(&evaluation.value) {
                    panic!("{name}: {e}");
                }
                continue;
            }
            assert!(config.is_plan(), "{name}");
            if let Err(e) = validator.validate(&evaluation.value) {
                panic!("{name}: {e}");
            }
            // Every preview it offers solves without complaint.
            let cases = config.preview.as_ref().map_or(1, |p| p.cases().len());
            for i in 0..cases {
                let solved = plate_core::solve::solve_view(&config, &plate_core::View::Case(i));
                let found: Vec<&str> = solved
                    .diagnostics
                    .iter()
                    .filter(|d| d.code != Some(plate_core::diagnostic::Code::Preference))
                    .map(|d| d.message.as_str())
                    .collect();
                assert!(
                    solved.report.unmet == 0 && solved.report.leftover < 1e-6,
                    "{name}, preview {i}: {found:?}"
                );
            }
        }
        assert!(libraries >= 4, "example services and assemblies");
    }

    #[test]
    fn library_files_are_told_by_their_shape() {
        use schema::FileKind;
        let files = examples();
        let kind = |name: &str| check_files(Workspace { main: name, files: &files }).unwrap().1.kind;
        assert_eq!(kind("build/services/lighting.ncl"), FileKind::Service);
        assert_eq!(kind("build/codes/nec.ncl"), FileKind::Service);
        assert_eq!(kind("build/assemblies/wood-frame.ncl"), FileKind::Assemblies);
        assert_eq!(kind("plans/flat.ncl"), FileKind::Plan);
        assert_eq!(kind("5-duplex.ncl"), FileKind::Plate);
        // A service's placement naming a part it doesn't have is caught in the file itself.
        let bad = "{ parts = { light = {} }, place = [plate.over \"lamp\" \"#room\"] }";
        let files = BTreeMap::from([("s.ncl".to_owned(), bad.to_owned())]);
        let err = check_files(Workspace { main: "s.ncl", files: &files }).unwrap_err();
        assert!(err[0].message.contains("no part named \"lamp\""), "{:?}", err[0].message);
        assert!(err[0].span.is_some());
    }

    #[test]
    fn a_plan_file_is_the_same_plan_a_plate_imports() {
        let files = examples();
        let (hotel, _) = check_files(Workspace { main: "3-hotel.ncl", files: &files }).unwrap();
        let (wing, _) = check_files(Workspace { main: "plans/wing.ncl", files: &files }).unwrap();
        let imported = hotel.spaces["wing"].plan.as_ref().unwrap();
        // Same fingerprint: same seed, so a preview in a plate's slot is that plate's solution.
        assert_eq!(imported.fingerprint(), wing.plan().fingerprint());
    }

    #[test]
    fn the_symbol_enum_is_the_catalog() {
        let schema = schema_json().unwrap();
        let symbol = &schema["$defs"]["Symbol"];
        let mut names: Vec<&str> =
            symbol["enum"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        names.sort_unstable();
        let catalog = plate_core::symbols::catalog();
        // The catalog is kept in a sorted map.
        let known: Vec<&str> = catalog.keys().map(String::as_str).collect();
        assert_eq!(names, known, "schema/symbols.json and the schema's Symbol enum");
        for (name, def) in catalog {
            let doc = symbol["x-enumDescriptions"][name].as_str().unwrap_or_default();
            assert!(doc.starts_with(&def.label), "{name}: described as {doc:?}, labelled {:?}", def.label);
        }
    }

    #[test]
    fn schema_length_pattern_matches_the_rust_parser() {
        let schema = schema_json().unwrap();
        let pattern = schema["$defs"]["Length"]["anyOf"][1]["pattern"].as_str().unwrap();
        let re = regex::Regex::new(pattern).unwrap();
        let cases = [
            "30",
            "2.5",
            "98'-6\"",
            "98'-6",
            "98' 6\"",
            "98'6\"",
            "30'",
            "6 1/2\"",
            "1/2\"",
            "10' 3 3/4\"",
            "12 ft 6 in",
            "12ft",
            "2 feet 1 inch",
            "98′-6″",
            "-2'-6\"",
            "9.5 m",
            "950mm",
            "12 cm",
            "−1.2 m",
            "98'-6 1/2",
            "",
            "abc",
            "30 yd",
            "6\" 30'",
            "30 30",
            "m",
            "30'-6\"x",
            "1 m 20 cm",
            "30 M",
            "30''",
        ];
        for case in cases {
            let parsed = plate_core::Length::parse(case).is_ok();
            assert_eq!(re.is_match(case), parsed, "pattern and parser disagree on {case:?}");
        }
    }

    fn workspace(files: &[(&str, &str)]) -> BTreeMap<String, String> {
        files.iter().map(|(p, s)| (p.to_string(), s.to_string())).collect()
    }

    const ROOM: &str = "# a plan in its own file\n{\n  spaces = { sleep = { area.min = 14 } },\n  rules = [plate.window \"sleep\"],\n}\n";
    const HOTEL: &str = "let room_plan = import \"plans/room.ncl\" in\n{\n  start = { mode = 'spine, points = [[0, 0], [30, 0]] },\n  spaces = {\n    corridor = { kind = 'circulation, width = 1.8 },\n    room = { count.min = 1, plan = room_plan },\n  },\n  rules = [plate.along \"room\" \"corridor\"],\n}\n";

    #[test]
    fn an_assembly_can_be_called_what_a_plate_field_is() {
        let (_, evaluation) =
            check(r#"{ name = { thickness = "100 mm" }, seed = { thickness = 0.1 } }"#).unwrap();
        assert_eq!(evaluation.kind, schema::FileKind::Assemblies);
    }

    #[test]
    fn imports_plans_from_other_files() {
        let files = workspace(&[("hotel.ncl", HOTEL), ("plans/room.ncl", ROOM)]);
        let (config, evaluation) = check_files(Workspace { main: "hotel.ncl", files: &files }).unwrap();
        let plan = config.spaces["room"].plan.as_ref().unwrap();
        assert!(plan.spaces.contains_key("sleep"));
        // Values from the imported file point into it, with its own line numbers.
        let area = evaluation
            .sources
            .iter()
            .find(|s| s.pointer == "/spaces/room/plan/spaces/sleep/area/min")
            .unwrap();
        assert_eq!(area.file, "plans/room.ncl");
        assert_eq!((area.span.start_line, area.span.start_col), (3, 35));
    }

    #[test]
    fn errors_in_imported_files_point_into_them() {
        let broken = ROOM.replace("area.min = 14", "area.min = \"lots\"");
        let files = workspace(&[("hotel.ncl", HOTEL), ("plans/room.ncl", &broken)]);
        let errs = check_files(Workspace { main: "hotel.ncl", files: &files }).unwrap_err();
        assert_eq!(errs[0].file.as_deref(), Some("plans/room.ncl"), "{errs:?}");
        assert_eq!(errs[0].span.unwrap().start_line, 3, "{errs:?}");

        let typo = ROOM.replace("rules", "rulez");
        let files = workspace(&[("hotel.ncl", HOTEL), ("plans/room.ncl", &typo)]);
        let errs = check_files(Workspace { main: "hotel.ncl", files: &files }).unwrap_err();
        assert!(errs[0].message.contains("rulez"), "{errs:?}");
    }

    #[test]
    fn plan_files_are_checked_as_plans() {
        let files = workspace(&[("hotel.ncl", HOTEL), ("plans/room.ncl", ROOM)]);
        let (config, evaluation) = check_files(Workspace { main: "plans/room.ncl", files: &files }).unwrap();
        assert!(config.is_plan());
        assert!(config.spaces.contains_key("sleep"));
        let area = evaluation.sources.iter().find(|s| s.pointer == "/spaces/sleep/area/min").unwrap();
        assert_eq!((area.file.as_str(), area.span.start_line), ("plans/room.ncl", 3));

        let typo = ROOM.replace("rules", "rulez");
        let files = workspace(&[("plans/room.ncl", &typo)]);
        let errs = check_files(Workspace { main: "plans/room.ncl", files: &files }).unwrap_err();
        assert!(errs[0].message.contains("rulez"), "{errs:?}");
        assert_eq!(errs[0].span.unwrap().start_line, 4, "{errs:?}");

        let preview = ROOM.replace("rules =", "preview = { width = \"wide\" },\n  rules =");
        let files = workspace(&[("plans/room.ncl", &preview)]);
        let errs = check_files(Workspace { main: "plans/room.ncl", files: &files }).unwrap_err();
        assert!(errs[0].message.contains("width"), "{errs:?}");
        assert_eq!(errs[0].span.unwrap().start_line, 4, "{errs:?}");

        let flat = ROOM.replace("rules =", "preview = [{ width = 0, depth = 8 }],\n  rules =");
        let files = workspace(&[("plans/room.ncl", &flat)]);
        let errs = check_files(Workspace { main: "plans/room.ncl", files: &files }).unwrap_err();
        assert!(errs[0].message.contains("more than 0"), "{errs:?}");
        assert_eq!(errs[0].span.unwrap().start_line, 4, "{errs:?}");
    }

    #[test]
    fn missing_imports_point_at_the_import() {
        let files = workspace(&[("hotel.ncl", HOTEL)]);
        let errs = check_files(Workspace { main: "hotel.ncl", files: &files }).unwrap_err();
        assert_eq!(errs[0].file.as_deref(), Some("hotel.ncl"), "{errs:?}");
        assert_eq!(errs[0].span.unwrap().start_line, 1, "{errs:?}");
        assert!(errs[0].message.contains("room.ncl"), "{errs:?}");
    }

    #[cfg(feature = "format")]
    #[test]
    fn formats_source() {
        let out = format("{start={mode='spine,points=[[0,0]]}}").unwrap();
        assert!(out.contains("mode = 'spine"), "{out}");
    }
}
