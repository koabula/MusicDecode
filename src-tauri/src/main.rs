#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ncm;

use serde::Serialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::{Emitter, Manager};

#[derive(Clone, Default)]
struct Jobs {
    busy: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Update {
    path: String,
    status: String,
    progress: u8,
    message: String,
    output: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportItem {
    path: String,
    name: String,
    title: String,
    artist: String,
    format: String,
    duration: u64,
    bitrate: u64,
    error: Option<String>,
}

#[derive(Serialize)]
struct ImportResult {
    items: Vec<ImportItem>,
    skipped: usize,
}

#[derive(Default, Serialize)]
struct Summary {
    succeeded: usize,
    failed: usize,
    cancelled: usize,
    outputs: Vec<String>,
}

#[tauri::command]
async fn pick_files() -> Vec<String> {
    rfd::AsyncFileDialog::new()
        .set_title("添加 NCM 文件")
        .add_filter("网易云音乐文件", &["ncm"])
        .pick_files()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|f| f.path().to_string_lossy().into_owned())
        .collect()
}

#[tauri::command]
async fn pick_directory(title: String) -> Option<String> {
    rfd::AsyncFileDialog::new()
        .set_title(&title)
        .pick_folder()
        .await
        .map(|f| f.path().to_string_lossy().into_owned())
}

fn gather(path: &Path, recursive: bool, out: &mut Vec<PathBuf>, skipped: &mut usize) {
    if out.len() >= 5000 {
        *skipped += 1;
        return;
    }
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        *skipped += 1;
        return;
    };
    if meta.file_type().is_symlink() {
        *skipped += 1;
        return;
    }
    if meta.is_dir() {
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut children: Vec<_> =
                    entries.filter_map(Result::ok).map(|e| e.path()).collect();
                children.sort();
                for child in children {
                    if child.is_file() || recursive {
                        gather(&child, recursive, out, skipped);
                    }
                }
            }
            Err(_) => *skipped += 1,
        }
    } else if meta.is_file()
        && path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ncm"))
    {
        out.push(path.to_path_buf());
    } else {
        *skipped += 1;
    }
}

#[tauri::command]
async fn import_paths(paths: Vec<String>, recursive: bool) -> Result<ImportResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut files = Vec::new();
        let mut skipped = 0;
        for p in paths {
            gather(Path::new(&p), recursive, &mut files, &mut skipped);
        }
        let mut seen = HashSet::new();
        let mut items = Vec::new();
        for file in files {
            let file = std::fs::canonicalize(&file).unwrap_or(file);
            let path = ncm::display_path(&file);
            if !seen.insert(path.clone()) {
                skipped += 1;
                continue;
            }
            let name = file
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let fallback = file
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            match ncm::inspect(&file) {
                Ok(info) => items.push(ImportItem {
                    path,
                    name,
                    title: info.title.unwrap_or(fallback),
                    artist: info.artist.unwrap_or_default(),
                    format: info.format,
                    duration: info.duration,
                    bitrate: info.bitrate,
                    error: None,
                }),
                Err(e) => items.push(ImportItem {
                    path,
                    name,
                    title: fallback,
                    artist: String::new(),
                    format: "未知".into(),
                    duration: 0,
                    bitrate: 0,
                    error: Some(e),
                }),
            }
        }
        Ok(ImportResult { items, skipped })
    })
    .await
    .map_err(|e| e.to_string())?
}

struct BusyGuard(Arc<AtomicBool>);
impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[tauri::command]
async fn convert_batch(
    app: tauri::AppHandle,
    jobs: tauri::State<'_, Jobs>,
    paths: Vec<String>,
    output_dir: Option<String>,
    bitrate: u32,
) -> Result<Summary, String> {
    if ![192, 256, 320].contains(&bitrate) {
        return Err("不支持的 MP3 码率".into());
    }
    if paths.is_empty() || paths.len() > 5000 {
        return Err("请选择 1 至 5000 个文件".into());
    }
    if jobs
        .busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("已有转换任务正在进行".into());
    }
    jobs.cancel.store(false, Ordering::SeqCst);
    let jobs = jobs.inner().clone();
    let ffmpeg = resolve_ffmpeg().map_err(|e| {
        jobs.busy.store(false, Ordering::SeqCst);
        e
    })?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = BusyGuard(jobs.busy.clone());
        let mut summary = Summary::default();
        let mut seen = HashSet::new();
        for path in paths {
            if !seen.insert(path.to_lowercase()) {
                continue;
            }
            let notify = |status: &str, progress, message: &str, output: Option<String>| {
                let _ = app.emit(
                    "decode-progress",
                    Update {
                        path: path.clone(),
                        status: status.into(),
                        progress,
                        message: message.into(),
                        output,
                    },
                );
            };
            if jobs.cancel.load(Ordering::SeqCst) {
                summary.cancelled += 1;
                notify("cancelled", 0, "已取消", None);
                continue;
            }
            notify("working", 0, "正在读取", None);
            match ncm::convert(
                Path::new(&path),
                output_dir.as_deref().map(Path::new),
                bitrate,
                &ffmpeg,
                &jobs.cancel,
                |progress, msg| notify("working", progress, msg, None),
            ) {
                Ok(output) => {
                    let output = ncm::display_path(&output);
                    summary.succeeded += 1;
                    summary.outputs.push(output.clone());
                    notify("success", 100, "已完成", Some(output));
                }
                Err(e) => {
                    if jobs.cancel.load(Ordering::SeqCst) {
                        summary.cancelled += 1;
                        notify("cancelled", 0, "已取消", None);
                    } else {
                        summary.failed += 1;
                        notify("error", 0, &e, None);
                    }
                }
            }
        }
        Ok(summary)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn cancel_batch(jobs: tauri::State<'_, Jobs>) {
    jobs.cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn open_directory(path: String) -> Result<(), String> {
    let p = PathBuf::from(path);
    let p = if p.is_file() {
        p.parent().ok_or("找不到输出目录")?.to_path_buf()
    } else {
        p
    };
    if !p.is_dir() {
        return Err("输出目录尚未创建，请先完成转换".into());
    }
    #[cfg(windows)]
    let opener = "explorer.exe";
    #[cfg(target_os = "macos")]
    let opener = "/usr/bin/open";
    #[cfg(target_os = "linux")]
    let opener = "xdg-open";
    std::process::Command::new(opener)
        .arg(p)
        .spawn()
        .map_err(|e| format!("无法打开目录：{e}"))?;
    Ok(())
}

fn resolve_ffmpeg() -> Result<PathBuf, String> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let bundled = executable
        .parent()
        .ok_or("无法定位程序目录")?
        .join(format!("musicdecode-ffmpeg{suffix}"));
    if bundled.is_file() {
        return Ok(bundled);
    }
    #[cfg(debug_assertions)]
    {
        let development = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(format!(
                "musicdecode-ffmpeg-{}{suffix}",
                env!("MUSICDECODE_TARGET")
            ));
        if development.is_file() {
            return Ok(development);
        }
    }
    Err("找不到音频组件，请重新安装完整的 MusicDecode".into())
}

fn main() {
    tauri::Builder::default()
        .manage(Jobs::default())
        .invoke_handler(tauri::generate_handler![
            pick_files,
            pick_directory,
            import_paths,
            convert_batch,
            cancel_batch,
            open_directory
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<Jobs>().busy.load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.emit("close-during-job", ());
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("MusicDecode failed to start");
}
