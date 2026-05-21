use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::fs;
use tokio::io::AsyncWriteExt;

const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub name: &'static str,
    pub display: &'static str,
    pub filename: &'static str,
    pub size_mb: u32,
    pub recommended: bool,
}

pub const KNOWN_MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "tiny",
        display: "tiny (multi, 75 MB) — fastest, lowest quality",
        filename: "ggml-tiny.bin",
        size_mb: 75,
        recommended: false,
    },
    ModelInfo {
        name: "base",
        display: "base (multi, 142 MB)",
        filename: "ggml-base.bin",
        size_mb: 142,
        recommended: false,
    },
    ModelInfo {
        name: "small",
        display: "small (multi, 466 MB)",
        filename: "ggml-small.bin",
        size_mb: 466,
        recommended: false,
    },
    ModelInfo {
        name: "medium-q5_0",
        display: "medium q5_0 (multi, 539 MB) — good balance",
        filename: "ggml-medium-q5_0.bin",
        size_mb: 539,
        recommended: false,
    },
    ModelInfo {
        name: "large-v3-turbo-q5_0",
        display: "large-v3-turbo q5_0 (multi, 574 MB) — best for RTX 4060",
        filename: "ggml-large-v3-turbo-q5_0.bin",
        size_mb: 574,
        recommended: true,
    },
];

pub fn find(name: &str) -> Option<&'static ModelInfo> {
    KNOWN_MODELS.iter().find(|m| m.name == name)
}

pub fn models_dir(app: &AppHandle) -> Result<PathBuf> {
    let base = app
        .path()
        .app_data_dir()
        .context("could not resolve app_data_dir")?;
    let dir = base.join("models");
    Ok(dir)
}

pub fn model_path(app: &AppHandle, name: &str) -> Result<PathBuf> {
    let info = find(name).ok_or_else(|| anyhow!("unknown model: {name}"))?;
    Ok(models_dir(app)?.join(info.filename))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelStatus {
    pub name: &'static str,
    pub display: &'static str,
    pub filename: &'static str,
    pub size_mb: u32,
    pub recommended: bool,
    pub installed: bool,
    pub size_on_disk: u64,
}

pub fn list_local(app: &AppHandle) -> Result<Vec<LocalModelStatus>> {
    let dir = models_dir(app)?;
    let mut out = Vec::with_capacity(KNOWN_MODELS.len());
    for m in KNOWN_MODELS {
        let path = dir.join(m.filename);
        let (installed, size) = match std::fs::metadata(&path) {
            Ok(md) => (md.is_file() && md.len() > 0, md.len()),
            Err(_) => (false, 0),
        };
        out.push(LocalModelStatus {
            name: m.name,
            display: m.display,
            filename: m.filename,
            size_mb: m.size_mb,
            recommended: m.recommended,
            installed,
            size_on_disk: size,
        });
    }
    Ok(out)
}

pub fn delete_local(app: &AppHandle, name: &str) -> Result<()> {
    let path = model_path(app, name)?;
    if path.exists() {
        std::fs::remove_file(&path).with_context(|| format!("removing {path:?}"))?;
    }
    Ok(())
}

#[derive(Default)]
pub struct DownloadRegistry {
    inner: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl DownloadRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn start(&self, name: &str) -> Result<Arc<AtomicBool>> {
        let mut g = self.inner.lock().unwrap();
        if g.contains_key(name) {
            return Err(anyhow!("download already in progress for {name}"));
        }
        let token = Arc::new(AtomicBool::new(false));
        g.insert(name.to_string(), token.clone());
        Ok(token)
    }

    fn finish(&self, name: &str) {
        self.inner.lock().unwrap().remove(name);
    }

    pub fn cancel(&self, name: &str) -> bool {
        if let Some(t) = self.inner.lock().unwrap().get(name) {
            t.store(true, Ordering::SeqCst);
            true
        } else {
            false
        }
    }

    pub fn is_active(&self, name: &str) -> bool {
        self.inner.lock().unwrap().contains_key(name)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum DownloadEvent<'a> {
    Started { name: &'a str, total: Option<u64> },
    Progress { name: &'a str, downloaded: u64, total: Option<u64> },
    Done { name: &'a str },
    Failed { name: &'a str, error: String },
    Canceled { name: &'a str },
}

pub async fn download(app: AppHandle, registry: Arc<DownloadRegistry>, name: String) -> Result<()> {
    let info = find(&name)
        .ok_or_else(|| anyhow!("unknown model: {name}"))?
        .clone();
    let cancel = registry.start(&name)?;

    let result = do_download(&app, &info, cancel.clone()).await;
    registry.finish(&name);

    match &result {
        Ok(()) => {
            let _ = app.emit("model-download", DownloadEvent::Done { name: &name });
        }
        Err(e) => {
            if cancel.load(Ordering::SeqCst) {
                let _ = app.emit("model-download", DownloadEvent::Canceled { name: &name });
            } else {
                let _ = app.emit(
                    "model-download",
                    DownloadEvent::Failed {
                        name: &name,
                        error: format!("{e:#}"),
                    },
                );
            }
        }
    }
    result
}

async fn do_download(
    app: &AppHandle,
    info: &ModelInfo,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let dir = models_dir(app)?;
    fs::create_dir_all(&dir).await.context("creating models dir")?;
    let final_path = dir.join(info.filename);
    let tmp_path = dir.join(format!("{}.part", info.filename));

    let url = format!("{HF_BASE}/{}", info.filename);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(900))
        .build()?;
    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("HTTP {} from {url}", resp.status()));
    }
    let total = resp.content_length();

    let _ = app.emit(
        "model-download",
        DownloadEvent::Started {
            name: info.name,
            total,
        },
    );

    let mut file = fs::File::create(&tmp_path)
        .await
        .with_context(|| format!("create {tmp_path:?}"))?;
    let mut downloaded: u64 = 0;
    let mut last_emit: u64 = 0;
    let mut stream = resp.bytes_stream();

    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            drop(file);
            let _ = fs::remove_file(&tmp_path).await;
            return Err(anyhow!("download canceled"));
        }
        let bytes = chunk.context("reading chunk")?;
        file.write_all(&bytes).await.context("writing chunk")?;
        downloaded += bytes.len() as u64;
        if downloaded - last_emit > 256 * 1024 {
            last_emit = downloaded;
            let _ = app.emit(
                "model-download",
                DownloadEvent::Progress {
                    name: info.name,
                    downloaded,
                    total,
                },
            );
        }
    }
    file.flush().await.ok();
    drop(file);

    fs::rename(&tmp_path, &final_path)
        .await
        .with_context(|| format!("rename {tmp_path:?} -> {final_path:?}"))?;
    Ok(())
}
