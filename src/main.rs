mod api;
mod biz;
mod catalog;
mod jobs;
mod lineage;
mod manual;
mod mybi;
mod naming;
mod secrets;
mod store;
mod sync;
mod transfer;

use axum::{http::{StatusCode, Uri, header}, response::{IntoResponse, Response}};
use std::{path::PathBuf, sync::Arc, time::Duration};

pub struct AppState {
    pub db: store::Db,
    pub sync_lock: tokio::sync::Mutex<()>,
    pub schedule_changed: tokio::sync::Notify,
    pub data_dir: PathBuf,
}

#[derive(rust_embed::Embed)]
#[folder = "web/dist"]
struct Assets;

async fn assets(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    // Unknown paths fall back to index.html so front-end routes survive a reload.
    let (path, file) = match Assets::get(path) { Some(file) => (path, file), None => match Assets::get("index.html") { Some(file) => ("index.html", file), None => return StatusCode::NOT_FOUND.into_response() } };
    ([(header::CONTENT_TYPE, mime_guess::from_path(path).first_or_octet_stream().to_string())], file.data).into_response()
}

fn data_dir() -> PathBuf {
    let dir = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|p| p.join("data"))).unwrap_or_else(|| PathBuf::from("data"));
    std::fs::create_dir_all(&dir).expect("create data directory");
    dir
}

fn port(dir: &std::path::Path) -> u16 {
    let path = dir.join("config.json");
    let config: serde_json::Value = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_else(|| {
        let default = serde_json::json!({ "port": 8100 });
        std::fs::write(&path, serde_json::to_vec_pretty(&default).unwrap()).ok();
        default
    });
    config["port"].as_u64().and_then(|p| u16::try_from(p).ok()).unwrap_or(8100)
}

fn open_browser(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(not(windows))]
    let _ = url;
}

fn main() {
    // SQL parsing of large view definitions recurses deeply; the default 2 MB worker stack is not enough.
    tokio::runtime::Builder::new_multi_thread().enable_all().thread_stack_size(32 << 20).build().expect("tokio runtime").block_on(run());
}

async fn run() {
    let dir = data_dir();
    let port = port(&dir);
    let url = format!("http://127.0.0.1:{port}/");
    let browser = !std::env::args().any(|a| a == "--no-browser");
    let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            println!("MyMeta 已在运行：{url}");
            if browser { open_browser(&url); }
            return;
        }
        Err(error) => panic!("监听 {url} 失败：{error}"),
    };
    let db = store::open(&dir.join("mymeta.db")).expect("open metadata database");
    db.execute("UPDATE sync_run SET status = 'error', error = 'interrupted' WHERE status = 'running'", []).ok();
    transfer::seed_metrics(&db);
    naming::refresh(&db, None).ok();
    let state = Arc::new(AppState { db: db.into(), sync_lock: Default::default(), schedule_changed: Default::default(), data_dir: dir.clone() });
    tokio::spawn(jobs::scheduler(state.clone()));

    let scheduler = state.clone();
    tokio::spawn(async move {
        let mut trigger = "startup";
        loop {
            sync::run(&scheduler, None, trigger).await;
            trigger = "schedule";
            let minutes = sync::interval_minutes(&scheduler.db.lock().unwrap());
            // Changing settings restarts the wait, so a new interval applies right away.
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(minutes * 60)) => break,
                    _ = scheduler.schedule_changed.notified() => {
                        let updated = sync::interval_minutes(&scheduler.db.lock().unwrap());
                        if updated != minutes { break; }
                    }
                }
            }
        }
    });

    println!("MyMeta 运行中：{url}（关闭此窗口即停止服务）");
    if browser { open_browser(&url); }
    let app = api::router().fallback(assets).layer(axum::extract::DefaultBodyLimit::max(64 << 20)).with_state(state);
    axum::serve(listener, app).await.expect("serve");
}
