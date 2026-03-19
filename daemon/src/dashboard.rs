//! Dashboard serving — embeds the React dashboard at compile time via rust-embed,
//! with fallback to serving from disk.

use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::Embed;

/// Embedded dashboard files from `dashboard/dist/`.
/// In debug mode, files are read from disk (hot-reload friendly).
/// In release mode, files are baked into the binary.
#[derive(Embed)]
#[folder = "../dashboard/dist/"]
struct DashboardAssets;

/// Serve a dashboard static file by path.
/// Returns the file with correct content-type, or 404.
pub async fn serve_dashboard(axum::extract::Path(path): axum::extract::Path<String>) -> Response {
    serve_embedded_file(&path)
}

/// Serve the dashboard index.html (SPA entry point).
pub async fn serve_index() -> Response {
    serve_embedded_file("index.html")
}

/// Look up a file in embedded assets and return with proper content type.
fn serve_embedded_file(path: &str) -> Response {
    match DashboardAssets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                file.data.to_vec(),
            )
                .into_response()
        }
        None => {
            // SPA fallback: serve index.html for unmatched paths
            // (lets React Router handle client-side routing)
            match DashboardAssets::get("index.html") {
                Some(file) => Html(String::from_utf8_lossy(&file.data).to_string()).into_response(),
                None => (StatusCode::NOT_FOUND, "Dashboard not found").into_response(),
            }
        }
    }
}
