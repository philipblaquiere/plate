//! HTTP API for the editor.
//!
//! | Method | Path                   | Body           | Returns                                   |
//! |--------|------------------------|----------------|-------------------------------------------|
//! | GET    | `/api/health`          |                | `{ ok, version }`                          |
//! | POST   | `/api/solve`           | `{ config }` or `{ source }` | `{ ok, layout, solver, timings }` or `{ ok: false, diagnostics }` |
//! | POST   | `/api/format`          | `{ source }`   | `{ ok, source }` or `{ ok: false, diagnostics }` |
//! | POST   | `/api/export/{format}` | `{ config }` or `{ source }` | file download (`dxf`, `svg`, `json`) |
//! | GET    | `/api/schema`          |                | the configuration JSON Schema             |
//! | GET    | `/api/files`           |                | `{ ok, root, files: [{ path, size }], folders }` |
//! | GET    | `/api/files/{path}`    |                | the file's text                            |
//! | PUT    | `/api/files/{path}`    | the text       | `{ ok }` (creates folders as needed)       |
//! | DELETE | `/api/files/{path}`    |                | `{ ok }`                                   |
//! | POST   | `/api/folders`         | `{ path }`     | `{ ok }` (a folder, empty to start with)   |
//! | DELETE | `/api/folders/{path}`  |                | `{ ok }` (only when no files are in it)    |
//! | POST   | `/api/rename`          | `{ from, to }` | `{ ok }`                                   |
//!
//! `config` is configuration data the browser already evaluated from Nickel; `source` is Nickel
//! text evaluated on the server. Both are validated here.
//!
//! Everything else is served from the built web app when `web_dir` exists.

pub mod engine;
pub mod workspace;

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use plate_core::Diagnostic;
use serde::Deserialize;
use serde_json::json;
use tower_http::services::{ServeDir, ServeFile};

#[derive(Debug, Deserialize)]
pub struct SourceRequest {
    pub source: String,
}

#[derive(Clone, Default)]
pub struct AppState {
    pub workspace: Option<Arc<workspace::Workspace>>,
}

pub fn app(web_dir: Option<PathBuf>) -> Router {
    app_with(web_dir, None)
}

/// The app, with a workspace folder for the editor's file browser.
pub fn app_with(web_dir: Option<PathBuf>, workspace: Option<workspace::Workspace>) -> Router {
    let state = AppState { workspace: workspace.map(Arc::new) };
    let api = Router::new()
        .route("/health", get(health))
        .route("/schema", get(schema))
        .route("/solve", post(solve))
        .route("/format", post(format))
        .route("/export/{format}", post(export))
        .route("/files", get(list_files))
        .route("/files/{*path}", get(read_file).put(write_file).delete(delete_file))
        .route("/rename", post(rename_file))
        .route("/folders", post(make_folder))
        .route("/folders/{*path}", axum::routing::delete(remove_folder))
        .with_state(state);

    let router = Router::new().nest("/api", api);
    match web_dir {
        Some(dir) if dir.join("index.html").exists() => {
            let index = dir.join("index.html");
            router.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)))
        }
        _ => router,
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

// ------------------------------------------------------------------ workspace files

type Ws = Arc<workspace::Workspace>;

fn ws_or_404(state: &AppState) -> Result<Ws, Box<Response>> {
    state.workspace.clone().ok_or_else(|| {
        Box::new(
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "ok": false, "error": "no workspace: start with `plate serve --workspace <dir>`" })),
            )
                .into_response(),
        )
    })
}

fn ws_error(err: workspace::WsError) -> Response {
    let status = match err {
        workspace::WsError::BadPath(_) => StatusCode::BAD_REQUEST,
        workspace::WsError::NotFound(_) => StatusCode::NOT_FOUND,
        workspace::WsError::Exists(_) | workspace::WsError::NotEmpty(_) => StatusCode::CONFLICT,
        workspace::WsError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, Json(json!({ "ok": false, "error": err.to_string() }))).into_response()
}

async fn list_files(State(state): State<AppState>) -> Response {
    let ws = match ws_or_404(&state) {
        Ok(ws) => ws,
        Err(r) => return *r,
    };
    let root = ws.root().display().to_string();
    match blocking(move || Ok::<_, std::io::Error>((ws.list()?, ws.folders()?))).await {
        Ok((files, folders)) => {
            Json(json!({ "ok": true, "root": root, "files": files, "folders": folders })).into_response()
        }
        Err(e) => ws_error(workspace::WsError::Io(e)),
    }
}

#[derive(Debug, Deserialize)]
pub struct FolderRequest {
    pub path: String,
}

async fn make_folder(State(state): State<AppState>, Json(req): Json<FolderRequest>) -> Response {
    let ws = match ws_or_404(&state) {
        Ok(ws) => ws,
        Err(r) => return *r,
    };
    match blocking(move || ws.make_folder(&req.path)).await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => ws_error(e),
    }
}

async fn remove_folder(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    let ws = match ws_or_404(&state) {
        Ok(ws) => ws,
        Err(r) => return *r,
    };
    match blocking(move || ws.remove_folder(&path)).await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => ws_error(e),
    }
}

async fn read_file(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    let ws = match ws_or_404(&state) {
        Ok(ws) => ws,
        Err(r) => return *r,
    };
    match blocking(move || ws.read(&path)).await {
        Ok(text) => ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response(),
        Err(e) => ws_error(e),
    }
}

async fn write_file(State(state): State<AppState>, Path(path): Path<String>, body: String) -> Response {
    let ws = match ws_or_404(&state) {
        Ok(ws) => ws,
        Err(r) => return *r,
    };
    match blocking(move || ws.write(&path, &body)).await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => ws_error(e),
    }
}

async fn delete_file(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    let ws = match ws_or_404(&state) {
        Ok(ws) => ws,
        Err(r) => return *r,
    };
    match blocking(move || ws.delete(&path)).await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => ws_error(e),
    }
}

#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    pub from: String,
    pub to: String,
}

async fn rename_file(State(state): State<AppState>, Json(req): Json<RenameRequest>) -> Response {
    let ws = match ws_or_404(&state) {
        Ok(ws) => ws,
        Err(r) => return *r,
    };
    match blocking(move || ws.rename(&req.from, &req.to)).await {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => ws_error(e),
    }
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
        let ws = workspace::Workspace::open(&dir).unwrap();
        let app = || app_with(None, Some(ws.clone()));
        let send = |req: Request<Body>| async move {
            let res = app().oneshot(req).await.unwrap();
            let status = res.status();
            (status, String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap())
        };

        let (status, body) = send(Request::get("/api/files").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains("\"plans/guest-room.ncl\"") && body.contains("\"scenarios/mansion\""),
            "{body}"
        );

        let (status, _) =
            send(Request::put("/api/files/plans/a.ncl").body(Body::from("{ spaces = {} }")).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        let (_, body) = send(Request::get("/api/files/plans/a.ncl").body(Body::empty()).unwrap()).await;
        assert_eq!(body, "{ spaces = {} }");

        let rename = serde_json::to_vec(&json!({ "from": "plans/a.ncl", "to": "plans/b.ncl" })).unwrap();
        let (status, _) = send(
            Request::post("/api/rename")
                .header("content-type", "application/json")
                .body(Body::from(rename))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(Request::delete("/api/files/plans/b.ncl").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = send(Request::get("/api/files/plans/b.ncl").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = send(Request::get("/api/files/..%2Fsecret.ncl").body(Body::empty()).unwrap()).await;
        assert!(status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND, "{status}");

        // Folders: made empty, listed, removed only once empty.
        let folder = serde_json::to_vec(&json!({ "path": "studies/early" })).unwrap();
        let (status, _) = send(
            Request::post("/api/folders")
                .header("content-type", "application/json")
                .body(Body::from(folder))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (_, body) = send(Request::get("/api/files").body(Body::empty()).unwrap()).await;
        assert!(body.contains("\"studies/early\""), "{body}");
        let (status, _) = send(Request::delete("/api/folders/plans").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = send(Request::delete("/api/folders/studies").body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
