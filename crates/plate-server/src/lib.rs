//! HTTP API for the editor.
//!
//! | Method | Path                   | Body           | Returns                                   |
//! |--------|------------------------|----------------|-------------------------------------------|
//! | GET    | `/api/health`          |                | `{ ok, version }`                          |
//! | POST   | `/api/solve`           | `{ config }` or `{ source }` | `{ ok, layout, solver, timings }` or `{ ok: false, diagnostics }` |
//! | POST   | `/api/format`          | `{ source }`   | `{ ok, source }` or `{ ok: false, diagnostics }` |
//! | POST   | `/api/export/{format}` | `{ config }` or `{ source }` | file download (`dxf`, `svg`, `json`) |
//! | GET    | `/api/schema`          |                | the configuration JSON Schema             |
//!
//! Projects, the folders at the top of the workspace:
//!
//! | Method | Path                                | Body / query     | Returns                     |
//! |--------|-------------------------------------|------------------|-----------------------------|
//! | GET    | `/api/projects`                     |                  | `{ ok, root, projects, examples }` |
//! | POST   | `/api/projects`                     | `{ name }`, or `{ example }` | `{ ok, name, main }` |
//! | POST   | `/api/projects/import`              | a zip; `?name=`  | `{ ok, name, main }`        |
//! | POST   | `/api/trash/{id}/restore`           |                  | `{ ok, name }`              |
//! | GET    | `/api/examples/{name}/thumbnail`    |                  | an SVG                      |
//! | GET    | `/api/projects/{p}`                 |                  | `{ ok, root, files: [{ path, text }], folders }` |
//! | DELETE | `/api/projects/{p}`                 |                  | `{ ok, trash }` (to the trash) |
//! | POST   | `/api/projects/{p}/move`            | `{ to }`         | `{ ok }`                    |
//! | POST   | `/api/projects/{p}/duplicate`       | `{ to }`         | `{ ok, name }`              |
//! | GET    | `/api/projects/{p}/export`          |                  | a zip                       |
//! | GET    | `/api/projects/{p}/thumbnail`       | `?main=`         | an SVG                      |
//!
//! Inside a project (paths are relative to it):
//!
//! | Method | Path                                | Body           | Returns                       |
//! |--------|-------------------------------------|----------------|-------------------------------|
//! | GET    | `/api/projects/{p}/files/{path}`    |                | the file's text               |
//! | PUT    | `/api/projects/{p}/files/{path}`    | the text       | `{ ok }` (makes folders as needed) |
//! | DELETE | `/api/projects/{p}/files/{path}`    |                | `{ ok }`                      |
//! | POST   | `/api/projects/{p}/folders`         | `{ path }`     | `{ ok }` (empty to start with) |
//! | DELETE | `/api/projects/{p}/folders/{path}`  |                | `{ ok }` (only when no files are in it) |
//! | POST   | `/api/projects/{p}/rename`          | `{ from, to }` | `{ ok }`                      |
//!
//! `config` is configuration data the browser already evaluated from Nickel; `source` is Nickel
//! text evaluated on the server. Both are validated here.
//!
//! Everything else is served from the built web app when `web_dir` exists.

pub mod engine;
pub mod projects;
pub mod thumbnail;
pub mod workspace;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use plate_core::Diagnostic;
use serde::Deserialize;
use serde_json::json;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::{SetResponseHeader, SetResponseHeaderLayer};

#[derive(Debug, Deserialize)]
pub struct SourceRequest {
    pub source: String,
}

#[derive(Clone, Default)]
pub struct AppState {
    pub projects: Option<Arc<projects::Projects>>,
    pub thumbnails: Arc<Mutex<thumbnail::Cache>>,
}

pub fn app(web_dir: Option<PathBuf>) -> Router {
    app_with(web_dir, None)
}

/// The app, with a workspace folder of projects for the editor's file browser.
pub fn app_with(web_dir: Option<PathBuf>, projects: Option<projects::Projects>) -> Router {
    let state = AppState { projects: projects.map(Arc::new), thumbnails: Default::default() };
    let api = Router::new()
        .route("/health", get(health))
        .route("/schema", get(schema))
        .route("/solve", post(solve))
        .route("/format", post(format))
        .route("/export/{format}", post(export))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/import", post(import_project).layer(DefaultBodyLimit::max(MAX_ZIP)))
        .route("/trash/{id}/restore", post(restore_project))
        .route("/examples/{name}/thumbnail", get(example_thumbnail))
        .route("/projects/{project}", get(read_project).delete(trash_project))
        .route("/projects/{project}/move", post(move_project))
        .route("/projects/{project}/duplicate", post(duplicate_project))
        .route("/projects/{project}/export", get(export_project))
        .route("/projects/{project}/thumbnail", get(project_thumbnail))
        .route("/projects/{project}/files/{*path}", get(read_file).put(write_file).delete(delete_file))
        .route("/projects/{project}/rename", post(rename_file))
        .route("/projects/{project}/folders", post(make_folder))
        .route("/projects/{project}/folders/{*path}", axum::routing::delete(remove_folder))
        // Not the web app's page: a page from an older build would take that for an answer.
        .fallback(|| async {
            (StatusCode::NOT_FOUND, Json(json!({ "ok": false, "error": "no such API route" })))
        })
        .with_state(state);

    // Nothing the API says is to be kept, unless it says otherwise (thumbnails do).
    let api = api.layer(SetResponseHeaderLayer::if_not_present(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    ));
    let router = Router::new().nest("/api", api);
    match web_dir {
        Some(dir) if dir.join("index.html").exists() => {
            let index = dir.join("index.html");
            // Always ask whether the page is current (an unchanged file costs a 304): left to
            // guess, a browser keeps an old page for days, and it calls routes long gone.
            let web = SetResponseHeader::overriding(
                ServeDir::new(dir).fallback(ServeFile::new(index)),
                header::CACHE_CONTROL,
                header::HeaderValue::from_static("no-cache"),
            );
            router.fallback_service(web)
        }
        _ => router.fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                "This is the plate API. In development the app is at http://localhost:5173. `npm start` serves it here.",
            )
        }),
    }
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
}

async fn schema() -> Response {
    ([(header::CONTENT_TYPE, "application/schema+json")], plate_config::SCHEMA_JSON).into_response()
}

/// Nickel evaluation is CPU-bound, so it runs off the async workers.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.expect("evaluation task panicked")
}

async fn solve(Json(input): Json<engine::Input>) -> Json<serde_json::Value> {
    let result = blocking(move || engine::run_input(input)).await;
    Json(match result {
        Ok(solved) => json!({
            "ok": true,
            "layout": solved.layout,
            "solver": solved.solver,
            "timings": solved.timings,
            "diagnostics": solved.diagnostics,
            "report": solved.report,
        }),
        Err(diagnostics) => json!({ "ok": false, "diagnostics": diagnostics }),
    })
}

async fn format(Json(req): Json<SourceRequest>) -> Json<serde_json::Value> {
    let result = blocking(move || plate_config::format(&req.source)).await;
    Json(match result {
        Ok(source) => json!({ "ok": true, "source": source }),
        Err(diagnostic) => json!({ "ok": false, "diagnostics": [diagnostic] }),
    })
}

async fn export(Path(format): Path<String>, Json(input): Json<engine::Input>) -> Response {
    let Some(format) = plate_export::Format::parse(&format) else {
        return error_response(
            StatusCode::NOT_FOUND,
            vec![Diagnostic::error(format!("unknown export format `{format}`; use dxf, svg or json"))],
        );
    };
    let result = blocking(move || {
        let solved = engine::run_input(input)?;
        let bytes = plate_export::export(&solved.layout, format)
            .map_err(|e| vec![Diagnostic::error(e.to_string())])?;
        Ok::<_, Vec<Diagnostic>>((plate_export::file_stem(&solved.layout), bytes))
    })
    .await;

    match result {
        Ok((stem, bytes)) => {
            let disposition = format!("attachment; filename=\"{stem}.{}\"", format.extension());
            (
                [
                    (header::CONTENT_TYPE, format.mime().to_owned()),
                    (header::CONTENT_DISPOSITION, disposition),
                ],
                bytes,
            )
                .into_response()
        }
        Err(diagnostics) => error_response(StatusCode::UNPROCESSABLE_ENTITY, diagnostics),
    }
}

// ------------------------------------------------------------------ projects

/// The largest project zip accepted.
const MAX_ZIP: usize = 32 * 1024 * 1024;

type Ps = Arc<projects::Projects>;

fn no_workspace() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "ok": false, "error": "no workspace: start with `plate serve --workspace <dir>`" })),
    )
        .into_response()
}

fn ws_error(err: workspace::WsError) -> Response {
    let status = match err {
        workspace::WsError::BadPath(_) | workspace::WsError::BadName(_) => StatusCode::BAD_REQUEST,
        workspace::WsError::NotFound(_) => StatusCode::NOT_FOUND,
        workspace::WsError::Exists(_) | workspace::WsError::NotEmpty(_) => StatusCode::CONFLICT,
        workspace::WsError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "ok": false, "error": err.to_string() }))).into_response()
}

/// Run a job on the projects off the async workers, as JSON (`ok` is added to what it gives).
async fn with_projects<T: serde::Serialize + Send + 'static>(
    state: &AppState,
    job: impl FnOnce(&Ps) -> Result<T, workspace::WsError> + Send + 'static,
) -> Response {
    let Some(projects) = state.projects.clone() else { return no_workspace() };
    match blocking(move || job(&projects)).await {
        Ok(value) => {
            let mut body = serde_json::to_value(value).unwrap_or_else(|_| json!({}));
            if !body.is_object() {
                body = json!({});
            }
            body["ok"] = json!(true);
            Json(body).into_response()
        }
        Err(e) => ws_error(e),
    }
}

async fn list_projects(State(state): State<AppState>) -> Response {
    with_projects(&state, |ps| {
        let examples: Vec<_> = projects::EXAMPLES
            .iter()
            .map(|e| {
                json!({ "name": e.name, "title": e.title, "blurb": e.blurb, "main": e.main, "files": e.files.len() })
            })
            .collect();
        Ok(json!({
            "root": ps.root().display().to_string(),
            "projects": ps.list()?,
            "examples": examples,
        }))
    })
    .await
}

#[derive(Debug, Deserialize)]
pub struct CreateProject {
    #[serde(default)]
    pub name: Option<String>,
    /// Open this example instead of making an empty project.
    #[serde(default)]
    pub example: Option<String>,
}

async fn create_project(State(state): State<AppState>, Json(req): Json<CreateProject>) -> Response {
    with_projects(&state, move |ps| {
        if let Some(example) = &req.example {
            let found = projects::Example::named(example)
                .ok_or_else(|| workspace::WsError::NotFound(example.clone()))?;
            let name = ps.open_example(found)?;
            return Ok(json!({ "name": name, "main": found.main }));
        }
        let name = req.name.unwrap_or_default();
        ps.create(&name)?;
        Ok(json!({ "name": name, "main": null }))
    })
    .await
}

#[derive(Debug, Deserialize)]
pub struct ImportQuery {
    pub name: String,
}

async fn import_project(
    State(state): State<AppState>,
    Query(query): Query<ImportQuery>,
    body: Bytes,
) -> Response {
    with_projects(&state, move |ps| {
        let name = ps.import_zip(&query.name, &body)?;
        let info = ps.info(&name)?;
        Ok(json!({ "name": name, "main": info.main }))
    })
    .await
}

async fn restore_project(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    with_projects(&state, move |ps| Ok(json!({ "name": ps.restore(&id)? }))).await
}

/// Everything in a project in one answer: opening one takes one request, however many files.
async fn read_project(State(state): State<AppState>, Path(project): Path<String>) -> Response {
    with_projects(&state, move |ps| {
        let ws = ps.get(&project)?;
        let files: Vec<_> =
            ws.sources()?.into_iter().map(|(path, text)| json!({ "path": path, "text": text })).collect();
        Ok(json!({ "root": ws.root().display().to_string(), "files": files, "folders": ws.folders()? }))
    })
    .await
}

async fn trash_project(State(state): State<AppState>, Path(project): Path<String>) -> Response {
    with_projects(&state, move |ps| Ok(json!({ "trash": ps.trash(&project)? }))).await
}

#[derive(Debug, Deserialize)]
pub struct ProjectTo {
    pub to: String,
}

async fn move_project(
    State(state): State<AppState>,
    Path(project): Path<String>,
    Json(req): Json<ProjectTo>,
) -> Response {
    with_projects(&state, move |ps| ps.rename(&project, &req.to).map(|()| json!({}))).await
}

async fn duplicate_project(
    State(state): State<AppState>,
    Path(project): Path<String>,
    Json(req): Json<ProjectTo>,
) -> Response {
    with_projects(&state, move |ps| Ok(json!({ "name": ps.duplicate(&project, &req.to)? }))).await
}

async fn export_project(State(state): State<AppState>, Path(project): Path<String>) -> Response {
    let Some(ps) = state.projects.clone() else { return no_workspace() };
    let name = project.clone();
    match blocking(move || ps.export_zip(&project)).await {
        Ok(bytes) => {
            // Only letters and the like get this far (see `check_name`), so the name is safe here.
            let disposition = format!("attachment; filename=\"{name}.zip\"");
            (
                [
                    (header::CONTENT_TYPE, "application/zip".to_owned()),
                    (header::CONTENT_DISPOSITION, disposition),
                ],
                bytes,
            )
                .into_response()
        }
        Err(e) => ws_error(e),
    }
}

// ------------------------------------------------------------------ thumbnails

#[derive(Debug, Deserialize)]
pub struct ThumbnailQuery {
    #[serde(default)]
    pub main: Option<String>,
}

async fn project_thumbnail(
    State(state): State<AppState>,
    Path(project): Path<String>,
    Query(query): Query<ThumbnailQuery>,
    headers: HeaderMap,
) -> Response {
    let Some(ps) = state.projects.clone() else { return no_workspace() };
    let files = {
        let name = project.clone();
        match blocking(move || ps.get(&name).and_then(|ws| Ok(ws.sources()?))).await {
            Ok(files) => files,
            Err(e) => return ws_error(e),
        }
    };
    let main =
        query.main.filter(|m| files.contains_key(m)).or_else(|| projects::main_plate(&project, &files));
    thumbnail_response(&state, main, files, &headers).await
}

async fn example_thumbnail(
    State(state): State<AppState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Response {
    let Some(example) = projects::Example::named(&name) else {
        return ws_error(workspace::WsError::NotFound(name));
    };
    thumbnail_response(&state, Some(example.main.to_owned()), example.sources(), &headers).await
}

/// The plate drawn small. Solving is the costly part, so it's done once per content: the browser
/// asks again with the tag it was given (a 304 costs no solve) and the server keeps what it drew.
async fn thumbnail_response(
    state: &AppState,
    main: Option<String>,
    files: std::collections::BTreeMap<String, String>,
    headers: &HeaderMap,
) -> Response {
    let Some(main) = main else { return StatusCode::NOT_FOUND.into_response() };
    let key = thumbnail::key(&main, &files);
    let tag = format!("\"{key:x}\"");
    if headers.get(header::IF_NONE_MATCH).is_some_and(|v| v.as_bytes() == tag.as_bytes()) {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    let cached = state.thumbnails.lock().ok().and_then(|c| c.get(key));
    let svg = match cached {
        Some(svg) => svg,
        None => {
            let drawn = blocking(move || thumbnail::draw(&main, files)).await;
            let Some(svg) = drawn else { return StatusCode::NOT_FOUND.into_response() };
            let svg: Arc<str> = svg.into();
            if let Ok(mut cache) = state.thumbnails.lock() {
                cache.put(key, svg.clone());
            }
            svg
        }
    };
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml".to_owned()),
            (header::CACHE_CONTROL, "no-cache".to_owned()),
            (header::ETAG, tag),
        ],
        svg.to_string(),
    )
        .into_response()
}

// ------------------------------------------------------------------ files in a project

/// Run a job on a project's folder off the async workers.
async fn in_project<T: Send + 'static>(
    state: &AppState,
    project: String,
    job: impl FnOnce(workspace::Workspace) -> Result<T, workspace::WsError> + Send + 'static,
) -> Result<T, Box<Response>> {
    let Some(projects) = state.projects.clone() else { return Err(Box::new(no_workspace())) };
    blocking(move || job(projects.get(&project)?)).await.map_err(|e| Box::new(ws_error(e)))
}

fn done(result: Result<(), Box<Response>>) -> Response {
    match result {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(response) => *response,
    }
}

#[derive(Debug, Deserialize)]
pub struct FolderRequest {
    pub path: String,
}

async fn make_folder(
    State(state): State<AppState>,
    Path(project): Path<String>,
    Json(req): Json<FolderRequest>,
) -> Response {
    done(in_project(&state, project, move |ws| ws.make_folder(&req.path)).await)
}

async fn remove_folder(
    State(state): State<AppState>,
    Path((project, path)): Path<(String, String)>,
) -> Response {
    done(in_project(&state, project, move |ws| ws.remove_folder(&path)).await)
}

async fn read_file(State(state): State<AppState>, Path((project, path)): Path<(String, String)>) -> Response {
    match in_project(&state, project, move |ws| ws.read(&path)).await {
        Ok(text) => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response(),
        Err(response) => *response,
    }
}

async fn write_file(
    State(state): State<AppState>,
    Path((project, path)): Path<(String, String)>,
    body: String,
) -> Response {
    done(in_project(&state, project, move |ws| ws.write(&path, &body)).await)
}

async fn delete_file(
    State(state): State<AppState>,
    Path((project, path)): Path<(String, String)>,
) -> Response {
    done(in_project(&state, project, move |ws| ws.delete(&path)).await)
}

#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    pub from: String,
    pub to: String,
}

async fn rename_file(
    State(state): State<AppState>,
    Path(project): Path<String>,
    Json(req): Json<RenameRequest>,
) -> Response {
    done(in_project(&state, project, move |ws| ws.rename(&req.from, &req.to)).await)
}

fn error_response(status: StatusCode, diagnostics: Vec<Diagnostic>) -> Response {
    (status, Json(json!({ "ok": false, "diagnostics": diagnostics }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const BOUNDARY: &str =
        "{ name = \"T\", start = { mode = 'boundary, points = [[0, 0], [30, 0], [30, 30]] } }";

    async fn post_json(uri: &str, source: &str) -> (StatusCode, Vec<u8>) {
        let body = serde_json::to_vec(&json!({ "source": source })).unwrap();
        let req =
            Request::post(uri).header("content-type", "application/json").body(Body::from(body)).unwrap();
        let res = app(None).oneshot(req).await.unwrap();
        let status = res.status();
        (status, res.into_body().collect().await.unwrap().to_bytes().to_vec())
    }

    #[tokio::test]
    async fn solve_returns_layout() {
        let (status, body) = post_json("/api/solve", BOUNDARY).await;
        assert_eq!(status, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["layout"]["elements"][0]["area"], 450.0);
    }

    #[tokio::test]
    async fn solve_reports_diagnostics() {
        let (_, body) = post_json("/api/solve", "{ start = }").await;
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["diagnostics"][0]["span"]["startLine"].is_number(), "{v}");
    }

    #[tokio::test]
    async fn solve_accepts_evaluated_config() {
        let body = serde_json::to_vec(
            &json!({ "config": { "start": { "mode": "spine", "points": [[0, 0], ["98'-6", 0]] }, "system": "imperial" } }),
        )
        .unwrap();
        let req = Request::post("/api/solve")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let res = app(None).oneshot(req).await.unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["layout"]["units"], "ft");
    }

    #[tokio::test]
    async fn solve_previews_a_plan_in_the_view_asked_for() {
        let plan = json!({
            "spaces": { "a": { "area": { "min": 10 } }, "b": { "area": { "min": 8 } } },
            "preview": [{ "width": 4, "depth": 6 }, { "name": "wide", "width": 8, "depth": 4 }],
        });
        let body = serde_json::to_vec(&json!({ "config": plan, "view": { "case": 1 } })).unwrap();
        let req = Request::post("/api/solve")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let res = app(None).oneshot(req).await.unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        let slot = &v["layout"]["elements"][0];
        assert_eq!(slot["kind"], "slot");
        assert_eq!(slot["source"], "/preview/1");
        assert_eq!(slot["area"], 32.0);
    }

    #[tokio::test]
    async fn solve_rejects_bad_config_data() {
        let body = serde_json::to_vec(
            &json!({ "config": { "start": { "mode": "spine", "points": [[0, "30 yd"]] } } }),
        )
        .unwrap();
        let req = Request::post("/api/solve")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        let res = app(None).oneshot(req).await.unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["ok"], false, "{v}");
    }

    #[tokio::test]
    async fn export_dxf() {
        let (status, body) = post_json("/api/export/dxf", BOUNDARY).await;
        assert_eq!(status, StatusCode::OK);
        assert!(String::from_utf8_lossy(&body).contains("LWPOLYLINE"));
    }

    #[tokio::test]
    async fn workspace_files_round_trip() {
        let dir = std::env::temp_dir().join(format!("plate-api-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ps = projects::Projects::open(&dir).unwrap();
        let app = || app_with(None, Some(ps.clone()));
        let send = |req: Request<Body>| async move {
            let res = app().oneshot(req).await.unwrap();
            let status = res.status();
            (
                status,
                String::from_utf8_lossy(&res.into_body().collect().await.unwrap().to_bytes()).into_owned(),
            )
        };
        let post = |uri: &str, body: serde_json::Value| {
            Request::post(uri)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap()
        };
        let get = |uri: &str| Request::get(uri).body(Body::empty()).unwrap();

        // An unknown API route says so, as JSON, even with a web app to fall back on.
        let web = std::env::temp_dir().join(format!("plate-web-{}", std::process::id()));
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("index.html"), "<!doctype html>").unwrap();
        let res = app_with(Some(web.clone()), None).oneshot(get("/api/files")).await.unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        assert!(res.headers()[header::CONTENT_TYPE].to_str().unwrap().contains("json"));
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-store", "API answers aren't kept");
        let res = app_with(Some(web.clone()), None).oneshot(get("/")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK, "the app's page is still served");
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache", "and never reused unchecked");
        let _ = std::fs::remove_dir_all(&web);

        // A new workspace holds nothing; the examples are on offer.
        let (status, body) = send(get("/api/projects")).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"projects\":[]") && body.contains("\"commercial\""), "{body}");

        // Opening an example copies it; opening a project takes one request.
        let (status, body) = send(post("/api/projects", json!({ "example": "commercial" }))).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("\"main\":\"hotel.ncl\""), "{body}");
        let (_, body) = send(get("/api/projects/commercial")).await;
        assert!(
            body.contains("\"hotel/guest-room.ncl\"") && body.contains("\"folders\":[\"hotel\""),
            "{body}"
        );

        // Thumbnails: drawn once per content, then a 304.
        let res = app().oneshot(get("/api/projects/commercial/thumbnail")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache", "thumbnails revalidate");
        let tag = res.headers()[header::ETAG].clone();
        let again = Request::get("/api/projects/commercial/thumbnail")
            .header(header::IF_NONE_MATCH, tag)
            .body(Body::empty())
            .unwrap();
        assert_eq!(app().oneshot(again).await.unwrap().status(), StatusCode::NOT_MODIFIED);
        let (status, svg) = send(get("/api/examples/home/thumbnail")).await;
        assert_eq!(status, StatusCode::OK);
        assert!(svg.starts_with("<svg"), "{svg}");

        // Files, inside a project.
        const P: &str = "/api/projects/commercial";
        let (status, _) =
            send(Request::put(format!("{P}/files/plans/a.ncl")).body(Body::from("{ spaces = {} }")).unwrap())
                .await;
        assert_eq!(status, StatusCode::OK);
        let (_, body) = send(get(&format!("{P}/files/plans/a.ncl"))).await;
        assert_eq!(body, "{ spaces = {} }");
        let (status, _) =
            send(post(&format!("{P}/rename"), json!({ "from": "plans/a.ncl", "to": "plans/b.ncl" }))).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) =
            send(Request::delete(format!("{P}/files/plans/b.ncl")).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(get(&format!("{P}/files/plans/b.ncl"))).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(get(&format!("{P}/files/..%2Fsecret.ncl"))).await;
        assert!(status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND, "{status}");
        let (status, _) = send(get("/api/projects/..%2F..%2Fetc/files/a.ncl")).await;
        assert!(status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND, "{status}");

        // Folders: made empty, listed, removed only once empty.
        let (status, _) = send(post(&format!("{P}/folders"), json!({ "path": "studies/early" }))).await;
        assert_eq!(status, StatusCode::OK);
        let (_, body) = send(get(P)).await;
        assert!(body.contains("\"studies/early\""), "{body}");
        let (status, _) =
            send(Request::delete(format!("{P}/folders/hotel")).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) =
            send(Request::delete(format!("{P}/folders/studies")).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);

        // Projects: rename, duplicate, trash and restore, zip and back.
        let (status, _) = send(post(&format!("{P}/move"), json!({ "to": "work" }))).await;
        assert_eq!(status, StatusCode::OK);
        let (_, body) = send(post("/api/projects/work/duplicate", json!({ "to": "work copy" }))).await;
        assert!(body.contains("\"work copy\""), "{body}");
        let (status, body) = send(post("/api/projects", json!({ "name": "work" }))).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        let (_, body) = send(Request::delete("/api/projects/work%20copy").body(Body::empty()).unwrap()).await;
        let trashed: serde_json::Value = serde_json::from_str(&body).unwrap();
        let id = trashed["trash"].as_str().unwrap().replace(' ', "%20");
        let (status, _) = send(post(&format!("/api/trash/{id}/restore"), json!({}))).await;
        assert_eq!(status, StatusCode::OK);
        assert!(ps.get("work copy").is_ok());
        let zipped = app().oneshot(get("/api/projects/work/export")).await.unwrap();
        assert_eq!(zipped.status(), StatusCode::OK);
        let bytes = zipped.into_body().collect().await.unwrap().to_bytes();
        let (status, body) = send(
            Request::post("/api/projects/import?name=unzipped").body(Body::from(bytes.to_vec())).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(ps.get("unzipped").unwrap().read("hotel/wing.ncl").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
