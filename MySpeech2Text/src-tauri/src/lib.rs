mod audio;
mod inject;
mod transcribe;

use std::sync::Arc;

use serde::Deserialize;
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

const TRAY_ID: &str = "main-tray";
const STORE_FILE: &str = "settings.json";
const STORE_KEY: &str = "settings";

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AppSettings {
    #[serde(default)]
    groq_api_key: String,
    #[serde(default = "default_model")]
    cloud_model: String,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default = "default_hotkey")]
    hotkey: String,
    #[serde(default = "default_inject_strategy")]
    inject_strategy: String,
}

fn default_model() -> String {
    "whisper-large-v3-turbo".into()
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
            cloud_model: default_model(),
            language: default_language(),
            hotkey: default_hotkey(),
            inject_strategy: default_inject_strategy(),
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
}

#[tauri::command]
async fn test_record_3s(app: AppHandle) -> Result<String, String> {
    start_recording(&app).map_err(|e| format!("{e:#}"))?;
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    stop_and_transcribe(app, false)
        .await
        .map_err(|e| format!("{e:#}"))
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

    let wav = audio::write_wav(&audio)?;
    let settings = read_settings(&app);
    let lang_owned = settings.language.clone();
    let lang: Option<&str> = if lang_owned == "auto" || lang_owned.is_empty() {
        None
    } else {
        Some(lang_owned.as_str())
    };

    let _ = app.emit("status", format!("transcribing ({dur:.1}s)..."));
    let text = transcribe::cloud::transcribe(transcribe::cloud::GroqRequest {
        api_key: &settings.groq_api_key,
        model: &settings.cloud_model,
        language: lang,
        wav_bytes: wav,
    })
    .await?;

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
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(handle_shortcut)
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(AppState {
            recorder: Arc::new(Recorder::spawn()),
        })
        .invoke_handler(tauri::generate_handler![test_record_3s])
        .setup(|app| {
            let toggle_record = MenuItem::with_id(
                app, "toggle_record", "Toggle Recording", true, None::<&str>,
            )?;
            let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let history = MenuItem::with_id(app, "history", "History", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle_record, &settings, &history, &quit])?;

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
