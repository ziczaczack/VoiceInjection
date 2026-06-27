mod audio;
mod dictionary;
mod history;
mod inject;
mod transcribe;

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState,
};
use tauri_plugin_store::StoreExt;

use crate::audio::Recorder;
use crate::transcribe::model::{DownloadRegistry, LocalModelStatus};

const TRAY_ID: &str = "main-tray";
const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "settings";

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    #[serde(default)]
    groq_api_key: String,
    #[serde(default = "default_mode")]
    mode: String,
    #[serde(default = "default_cloud_model")]
    cloud_model: String,
    #[serde(default = "default_local_model")]
    local_model: String,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default = "default_hotkey")]
    hotkey: String,
    #[serde(default = "default_inject_strategy")]
    inject_strategy: String,
    #[serde(default)]
    dictionary: Vec<dictionary::DictRule>,
}

fn default_mode() -> String {
    "cloud".into()
}
fn default_cloud_model() -> String {
    "whisper-large-v3-turbo".into()
}
fn default_local_model() -> String {
    "large-v3-turbo-q5_0".into()
}
fn default_language() -> String {
    "auto".into()
}
fn default_hotkey() -> String {
    "Alt+Space".into()
}
fn default_inject_strategy() -> String {
    "clipboard".into()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            groq_api_key: String::new(),
            mode: default_mode(),
            cloud_model: default_cloud_model(),
            local_model: default_local_model(),
            language: default_language(),
            hotkey: default_hotkey(),
            inject_strategy: default_inject_strategy(),
            dictionary: Vec::new(),
        }
    }
}

fn read_settings(app: &AppHandle) -> AppSettings {
    let store = match app.store(STORE_FILE) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("could not open store: {e}");
            return AppSettings::default();
        }
    };
    match store.get(STORE_KEY) {
        Some(v) => serde_json::from_value(v).unwrap_or_else(|e| {
            log::warn!("settings parse error: {e}");
            AppSettings::default()
        }),
        None => AppSettings::default(),
    }
}

struct AppState {
    recorder: Arc<Recorder>,
    downloads: Arc<DownloadRegistry>,
    history: Arc<history::History>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryPage {
    total: i64,
    entries: Vec<history::Entry>,
}

#[tauri::command]
async fn test_record_3s(app: AppHandle) -> Result<String, String> {
    start_recording(&app).map_err(|e| format!("{e:#}"))?;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    stop_and_transcribe(app, false)
        .await
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn list_local_models(app: AppHandle) -> Result<Vec<LocalModelStatus>, String> {
    transcribe::model::list_local(&app).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn download_model(app: AppHandle, name: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let registry = state.downloads.clone();
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = transcribe::model::download(app_clone, registry, name).await {
            log::warn!("download failed: {e:#}");
        }
    });
    Ok(())
}

#[tauri::command]
fn cancel_model_download(app: AppHandle, name: String) -> bool {
    app.state::<AppState>().downloads.cancel(&name)
}

#[tauri::command]
fn list_history(
    app: AppHandle,
    query: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<HistoryPage, String> {
    let state = app.state::<AppState>();
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    let total = state
        .history
        .count(query.as_deref())
        .map_err(|e| format!("{e:#}"))?;
    let entries = state
        .history
        .list(query.as_deref(), limit, offset)
        .map_err(|e| format!("{e:#}"))?;
    Ok(HistoryPage { total, entries })
}

#[tauri::command]
fn delete_history_entry(app: AppHandle, id: i64) -> Result<(), String> {
    app.state::<AppState>()
        .history
        .delete(id)
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn clear_history(app: AppHandle) -> Result<(), String> {
    app.state::<AppState>()
        .history
        .clear()
        .map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn delete_local_model(app: AppHandle, name: String) -> Result<(), String> {
    transcribe::local::unload();
    transcribe::model::delete_local(&app, &name).map_err(|e| format!("{e:#}"))
}

fn start_recording(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    state.recorder.start()?;
    let _ = app.emit("recording", true);
    update_tray_tooltip(app, "Voice Dictation — recording...");
    Ok(())
}

async fn stop_and_transcribe(app: AppHandle, inject_text: bool) -> anyhow::Result<String> {
    let _ = app.emit("recording", false);
    update_tray_tooltip(&app, "Voice Dictation");

    let state = app.state::<AppState>();
    let audio = state.recorder.stop()?;
    let dur = audio::duration_secs(&audio);
    if dur < 0.3 {
        let _ = app.emit("status", "too short — discarded");
        return Ok(String::new());
    }
    let level = audio::rms(&audio.samples);
    if level < 0.003 {
        let _ = app.emit("status", "silence — discarded");
        return Ok(String::new());
    }

    let settings = read_settings(&app);
    let lang_owned = settings.language.clone();
    let lang_opt: Option<String> = if lang_owned == "auto" || lang_owned.is_empty() {
        None
    } else {
        Some(lang_owned)
    };

    let prompt_opt: Option<&str> = match lang_opt.as_deref() {
        Some("zh") => Some("以下是普通话的句子。"),
        Some("en") => None,
        _ => Some("以下是普通话的句子。"),
    };

    let _ = app.emit("status", format!("transcribing ({dur:.1}s)..."));
    let (raw, used_mode) = match settings.mode.as_str() {
        "local" => (
            run_local(&app, &settings, &audio, lang_opt.as_deref(), prompt_opt).await?,
            "local",
        ),
        "auto" => match run_cloud(&app, &settings, &audio, lang_opt.as_deref(), prompt_opt).await {
            Ok(t) => (t, "cloud"),
            Err(e) => {
                log::warn!("cloud failed in auto mode, falling back to local: {e:#}");
                let _ = app.emit("status", "cloud failed — trying local...");
                (
                    run_local(&app, &settings, &audio, lang_opt.as_deref(), prompt_opt).await?,
                    "local",
                )
            }
        },
        _ => (
            run_cloud(&app, &settings, &audio, lang_opt.as_deref(), prompt_opt).await?,
            "cloud",
        ),
    };
    let normalized = fast2s::convert(&raw);
    let text = dictionary::apply(&normalized, &settings.dictionary);

    if !text.is_empty() {
        let used_model = if used_mode == "cloud" {
            &settings.cloud_model
        } else {
            &settings.local_model
        };
        let lang_for_db = lang_opt.as_deref();
        if let Err(e) = app.state::<AppState>().history.insert(history::NewEntry {
            mode: used_mode,
            model: used_model,
            language: lang_for_db,
            duration_secs: dur as f64,
            text: &text,
        }) {
            log::warn!("history insert failed: {e:#}");
        }
    }

    let _ = app.emit("transcription", &text);

    if inject_text && !text.is_empty() {
        let _ = app.emit("status", "injecting...");
        let strategy = settings.inject_strategy.clone();
        let text_for_inject = text.clone();
        let join =
            tokio::task::spawn_blocking(move || inject::inject(&text_for_inject, &strategy)).await;
        match join {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                log::warn!("inject failed: {e:#}");
                let _ = app.emit("status", format!("inject failed: {e:#}"));
            }
            Err(e) => {
                log::warn!("inject task panicked: {e:#}");
                let _ = app.emit("status", format!("inject task panicked: {e}"));
            }
        }
    }

    let _ = app.emit("status", "done");
    log::info!("transcribed ({} chars): {}", text.len(), text);
    Ok(text)
}

async fn run_cloud(
    _app: &AppHandle,
    settings: &AppSettings,
    audio: &audio::RecordedAudio,
    language: Option<&str>,
    initial_prompt: Option<&str>,
) -> anyhow::Result<String> {
    let wav = audio::write_wav(audio)?;
    transcribe::cloud::transcribe(transcribe::cloud::GroqRequest {
        api_key: &settings.groq_api_key,
        model: &settings.cloud_model,
        language,
        initial_prompt,
        wav_bytes: wav,
    })
    .await
}

async fn run_local(
    app: &AppHandle,
    settings: &AppSettings,
    audio: &audio::RecordedAudio,
    language: Option<&str>,
    initial_prompt: Option<&str>,
) -> anyhow::Result<String> {
    let model_path = transcribe::model::model_path(app, &settings.local_model)?;
    if !model_path.exists() {
        return Err(anyhow::anyhow!(
            "local model '{}' not downloaded — go to Settings → Local Models",
            settings.local_model
        ));
    }
    let model_name = settings.local_model.clone();
    let samples = audio.samples.clone();
    let sample_rate = audio.sample_rate;
    let lang_owned = language.map(|s| s.to_string());
    let prompt_owned = initial_prompt.map(|s| s.to_string());
    let path_clone = model_path.clone();
    let text = tokio::task::spawn_blocking(move || {
        transcribe::local::transcribe(transcribe::local::LocalRequest {
            model_name: &model_name,
            model_path: &path_clone,
            samples: &samples,
            sample_rate,
            language: lang_owned.as_deref(),
            initial_prompt: prompt_owned.as_deref(),
        })
    })
    .await??;
    Ok(text)
}

fn update_tray_tooltip(app: &AppHandle, text: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}

fn handle_shortcut(app: &AppHandle, _sc: &Shortcut, event: ShortcutEvent) {
    match event.state() {
        ShortcutState::Pressed => {
            if let Err(e) = start_recording(app) {
                let _ = app.emit("status", format!("error starting: {e:#}"));
                log::error!("start error: {e:#}");
            }
        }
        ShortcutState::Released => {
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = stop_and_transcribe(app_clone.clone(), true).await {
                    let _ = app_clone.emit("status", format!("transcription failed: {e:#}"));
                    log::error!("transcribe error: {e:#}");
                }
            });
        }
    }
}

fn parse_hotkey(s: &str) -> Shortcut {
    s.parse::<Shortcut>().unwrap_or_else(|_| {
        log::warn!("could not parse hotkey '{s}', falling back to Alt+Space");
        Shortcut::new(Some(Modifiers::ALT), Code::Space)
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Must be the FIRST plugin registered. When a second launch happens, the
        // running instance gets the new args and focuses its window; the duplicate
        // process exits instead of spawning another tray icon.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("settings") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--minimized"])
                .build(),
        )
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(handle_shortcut)
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            test_record_3s,
            list_local_models,
            download_model,
            cancel_model_download,
            delete_local_model,
            list_history,
            delete_history_entry,
            clear_history
        ])
        .setup(|app| {
            let history_db_path = app
                .path()
                .app_data_dir()
                .expect("could not resolve app_data_dir")
                .join("history.db");
            let history_db =
                history::History::open(&history_db_path).expect("failed to open history db");

            app.manage(AppState {
                recorder: Arc::new(Recorder::spawn()),
                downloads: Arc::new(DownloadRegistry::new()),
                history: Arc::new(history_db),
            });

            let toggle_record =
                MenuItem::with_id(app, "toggle_record", "Toggle Recording", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let history_item = MenuItem::with_id(app, "history", "History", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle_record, &settings, &history_item, &quit])?;

            let _tray = TrayIconBuilder::with_id(TRAY_ID)
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Voice Dictation")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" | "history" => {
                        if let Some(window) = app.get_webview_window("settings") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "toggle_record" => {
                        let app_clone = app.clone();
                        let recorder = app.state::<AppState>().recorder.clone();
                        if recorder.is_running() {
                            tauri::async_runtime::spawn(async move {
                                let _ = stop_and_transcribe(app_clone, true).await;
                            });
                        } else {
                            let _ = start_recording(app);
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("settings") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            let settings = read_settings(&app.handle());
            let shortcut = parse_hotkey(&settings.hotkey);
            if let Err(e) = app.global_shortcut().register(shortcut) {
                log::error!("failed to register hotkey '{}': {e}", settings.hotkey);
            } else {
                log::info!("registered hotkey: {}", settings.hotkey);
            }

            // The window is hidden by default (see tauri.conf.json). On a manual
            // launch, surface the settings window so the user sees the app opened.
            // When autostarted at login (`--minimized`), stay quietly in the tray.
            let launched_minimized = std::env::args().any(|a| a == "--minimized");
            if !launched_minimized {
                if let Some(window) = app.get_webview_window("settings") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
