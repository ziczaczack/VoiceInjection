# MySpeech2Text 🎙️

> Privacy-first voice dictation desktop app — hold a hotkey, speak, and the transcribed text is injected into whatever window has focus. Bilingual (Chinese + English, including code-switching), on-device by default. Built with Tauri 2 + Rust + React.

Inspired by BridgeVoice. The goal is a fast, universal dictation tool: hold-to-talk
→ on-device transcription → text injected into the focused app (IDE, browser,
terminal, chat) at sub-second latency. Cloud transcription is opt-in; no
subscription is required for daily use.

## Goals & non-goals

**Goals**
- Free / low-cost — no subscription for daily use
- Privacy-first — default to on-device transcription, cloud opt-in
- Bilingual — Chinese + English, including code-switching
- Universal — inject into any focused app
- Fast — sub-second latency from speech end to text appearing

**Non-goals (v1):** mobile apps · real-time streaming (chunked is fine) ·
multi-user / cloud sync · custom voice training.

## Tech stack

| Layer            | Choice |
|------------------|--------|
| App framework    | Tauri 2 (~10 MB binary, native perf) |
| Backend          | Rust (direct Win32 API access) |
| Frontend         | React + TypeScript + Tailwind CSS v4 (Vite) |
| Local STT        | whisper.cpp (via `whisper-rs`) |
| Cloud STT (opt)  | Groq Whisper API |
| Audio capture    | `cpal` crate |
| Global hotkey    | `tauri-plugin-global-shortcut` |
| Text injection   | `enigo` crate (Win32 `SendInput`) — clipboard paste as fallback |
| Persistence      | `tauri-plugin-store` (settings, dictionary, history) |

**Target platform:** Windows 10/11 (x64) for v1; Linux/macOS later (Tauri makes it cheap).

## Development

```bash
npm install
npm run tauri dev          # run the desktop app in dev mode
```

| Command              | Description |
|----------------------|-------------|
| `npm run dev`        | Vite dev server (frontend only) |
| `npm run build`      | Type-check + build frontend |
| `npm run typecheck`  | `tsc --noEmit` |
| `npm run tauri dev`  | Run the full Tauri desktop app |
| `npm run tauri build`| Produce a packaged desktop binary |

See `docs/toolchain-setup.md` for the Rust/whisper toolchain setup.

## Rust modules (`src-tauri/src`)

| File              | Role |
|-------------------|------|
| `audio.rs`        | Microphone capture (`cpal`) |
| `transcribe/`     | Transcription — `local.rs` (whisper.cpp), `cloud.rs` (Groq), `model.rs`, `mod.rs` |
| `inject.rs`       | Text injection into the focused window |
| `dictionary.rs`   | Custom dictionary (term replacements) |
| `history.rs`      | Transcription history |
| `lib.rs`/`main.rs`| Tauri app wiring + entry point |

## Status & next step

The full end-to-end Windows path works: hold the hotkey → capture audio → transcribe
(local whisper.cpp **or** cloud Groq, with auto-fallback) → apply the custom
dictionary → inject text into the focused app. Model download/management, transcription
history, autostart, and the system tray are all in place.

**Next:** a floating always-on-top recording widget (Stage 5) and first-run onboarding
+ auto-updater (Stage 8). Smaller gaps: SHA256 verification of downloaded models,
surfacing GPU/CUDA status, and history extras (stats, export, re-inject).

The full development guide (goals, tech rationale, milestones) is in `PROJECT.md`.
