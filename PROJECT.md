# Voice Dictation Desktop App — Development Guide

A privacy-first voice-to-text desktop app for Windows, inspired by BridgeVoice. Hold a hotkey, speak, and have transcribed text injected into whatever app is focused.

---

## Project Overview

### Goals
- **Free / low-cost**: No subscription dependencies for daily use
- **Privacy-first**: Default to on-device transcription, cloud is opt-in
- **Bilingual**: Support Chinese + English (including code-switching)
- **Universal**: Inject text into any focused app (IDE, browser, terminal, chat)
- **Fast**: Sub-second latency from speech end to text appearing

### Target Platform
- **Primary**: Windows 10/11 (x64)
- **Secondary**: Linux/macOS later (Tauri makes this cheap)

### Non-goals (for v1)
- Mobile apps
- Real-time streaming transcription (chunked is fine)
- Multi-user / cloud sync
- Custom voice training

---

## Tech Stack Decision

| Layer | Choice | Why |
|---|---|---|
| App framework | **Tauri 2.0** | ~10MB binary, native performance, BridgeVoice itself uses this |
| Backend language | **Rust** | First-class Tauri support, direct Win32 API access |
| Frontend | **React + TypeScript + Tailwind** | Standard, fast iteration |
| Local STT | **whisper.cpp** via `whisper-rs` crate | C++ optimized, CUDA/Vulkan accel on Windows |
| Cloud STT (optional) | **Groq Whisper API** | 228x realtime, free tier sufficient for personal use |
| Audio capture | **`cpal` crate** | Rust standard for cross-platform audio |
| Global hotkey | **`tauri-plugin-global-shortcut`** | Official plugin |
| Text injection | **`enigo` crate** | Wraps Win32 `SendInput`, cross-platform |
| State persistence | **`tauri-plugin-store`** | Settings, dictionary, history |

### Why these specific choices over alternatives

- **Tauri over Electron**: 10x smaller bundles, native perf, Rust gives us safe Win32 API access
- **whisper.cpp over Python whisper**: No Python dependency to ship; users get a single .exe
- **whisper-large-v3-turbo over large-v3**: 2x faster, ~$0.04/hr vs $0.111/hr, accuracy difference negligible for dictation
- **enigo over raw Win32**: Saves ~200 lines of unsafe FFI; falls back gracefully
- **cpal over portaudio**: Pure Rust, no extra binary deps

---

## Repository Structure

```
voice-dictation/
├── README.md
├── PROJECT_GUIDE.md          ← this file
├── .gitignore
├── package.json              ← frontend deps
├── tsconfig.json
├── vite.config.ts
├── tailwind.config.js
├── index.html
├── src/                      ← React frontend
│   ├── main.tsx
│   ├── App.tsx
│   ├── components/
│   │   ├── Widget.tsx        ← floating recording widget
│   │   ├── Settings.tsx
│   │   ├── History.tsx
│   │   └── Dictionary.tsx
│   ├── hooks/
│   │   └── useTauri.ts
│   └── types.ts
├── src-tauri/                ← Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── build.rs
│   ├── src/
│   │   ├── main.rs           ← entry, window setup
│   │   ├── audio.rs          ← cpal capture
│   │   ├── transcribe/
│   │   │   ├── mod.rs
│   │   │   ├── local.rs      ← whisper.cpp via whisper-rs
│   │   │   └── cloud.rs      ← Groq API
│   │   ├── inject.rs         ← enigo text injection
│   │   ├── hotkey.rs         ← global shortcut handling
│   │   ├── dictionary.rs     ← post-processing replacements
│   │   ├── history.rs        ← transcription log
│   │   └── settings.rs       ← persisted config
│   └── models/               ← whisper .bin files (gitignored)
└── prototypes/
    └── voice_to_text_prototype.py   ← Stage 0 Python validator
```

---

## Stage 0 — Validate Transcription Quality

**Goal**: Confirm Whisper handles your specific speech patterns (Chinese + English code-switching) before investing in a full app.

**Time**: 30 minutes
**Output**: Decision on whether to proceed and which model to default to

### Tasks
1. Get a free Groq API key from https://console.groq.com/keys
2. Run the Python prototype (`prototypes/voice_to_text_prototype.py`)
3. Record three test samples:
   - Pure Chinese conversational
   - Chinese + English with technical terms ("我等下要 commit 然后 deploy")
   - Chinese + English longer mix ("这个 function 的 performance 不太行，我打算 refactor 一下")
4. Try both `whisper-large-v3` and `whisper-large-v3-turbo`, compare accuracy
5. Note any consistently mis-transcribed words → these go into the custom dictionary later

### Decision Points
- ✅ **>90% accuracy on samples** → proceed to Stage 1
- ⚠️ **Technical terms consistently wrong** → still proceed; plan for dictionary post-processing
- ❌ **Code-switching breaks transcription** → consider segmenting by language detection or lock to single language

### Validation Criteria
- [ ] Latency from end-of-speech to text return is <1s for 5-10s utterances
- [ ] At least one model variant produces acceptable accuracy
- [ ] Decided which model to ship as default

---

## Stage 1 — Tauri Project Skeleton

**Goal**: Get a Tauri app running with a system tray icon and a settings window. No transcription yet.

**Time**: 2-3 hours
**Output**: A buildable, runnable Tauri app

### Prerequisites
- Install Rust: https://rustup.rs
- Install Node.js 20+: https://nodejs.org
- Install Visual Studio Build Tools (C++ workload) on Windows
- Install Tauri prerequisites: https://tauri.app/start/prerequisites/

### Tasks
1. Scaffold project: `npm create tauri-app@latest voice-dictation -- --template react-ts`
2. Add core Rust deps in `src-tauri/Cargo.toml`:
   ```toml
   tauri = { version = "2", features = ["tray-icon"] }
   tauri-plugin-global-shortcut = "2"
   tauri-plugin-store = "2"
   tauri-plugin-clipboard-manager = "2"
   serde = { version = "1", features = ["derive"] }
   serde_json = "1"
   tokio = { version = "1", features = ["full"] }
   anyhow = "1"
   thiserror = "1"
   ```
3. Set up Tailwind in the React frontend
4. Create system tray with menu: Start/Stop, Settings, History, Quit
5. Create a Settings window (hidden by default, shown via tray)
6. Wire up `tauri-plugin-store` for persisting config to `%APPDATA%\voice-dictation\config.json`

### Validation Criteria
- [ ] `npm run tauri dev` launches the app successfully
- [ ] System tray icon appears on Windows
- [ ] Tray menu opens Settings window
- [ ] Settings window persists changes between app restarts
- [ ] `npm run tauri build` produces a working `.msi` installer

---

## Stage 2 — Audio Capture + Cloud Transcription

**Goal**: Record audio while a hotkey is held, send to Groq, get text back. No injection yet — just log to console.

**Time**: 4-6 hours
**Output**: Working dictation pipeline (cloud-only)

### Tasks

**Audio capture (`src-tauri/src/audio.rs`)**
- Use `cpal` to capture from default input device
- Buffer mono PCM at 16kHz (Whisper resamples anyway, save bandwidth)
- Expose `start_recording()` / `stop_recording() -> Vec<i16>` commands
- Handle device disconnect gracefully

**Hotkey (`src-tauri/src/hotkey.rs`)**
- Use `tauri-plugin-global-shortcut`
- Default: hold `Right Alt` to record (configurable)
- Critical: distinguish `keydown` (start recording) vs `keyup` (stop & transcribe)
- Edge case: hotkey already held when app starts → ignore until release

**Cloud transcription (`src-tauri/src/transcribe/cloud.rs`)**
- POST audio to `https://api.groq.com/openai/v1/audio/transcriptions`
- Multipart form: `file` (WAV bytes), `model`, `temperature=0.0`, optional `language`
- Use `reqwest` crate with `multipart` feature
- Store API key in `tauri-plugin-store` (encrypted via OS keyring later)
- Handle rate limit (429) with exponential backoff

**Frontend feedback**
- Tray icon changes color when recording (red dot indicator)
- Optional toast notification when transcription completes

### API Key Handling — Important
Don't ship your Groq key. On first run, prompt the user in Settings to paste their own key. Store via `tauri-plugin-store` for now; migrate to OS keyring (`keyring` crate) before any public release.

### Validation Criteria
- [ ] Hold-to-record works reliably; release triggers transcription
- [ ] Audio sounds correct when saved as a debug `.wav`
- [ ] Groq returns text within 1s for 5-10s clips
- [ ] Rate limit errors show a clear message in the UI
- [ ] Test all 3 language scenarios from Stage 0 — quality should match Python prototype

---

## Stage 3 — Text Injection

**Goal**: Transcribed text is automatically typed into the currently focused application.

**Time**: 3-4 hours
**Output**: End-to-end dictation works in real apps (VS Code, browser, Slack, Notepad)

### Tasks

**Injection module (`src-tauri/src/inject.rs`)**
- Use `enigo` crate's `Keyboard` trait
- Strategy A (default): clipboard + paste (`Ctrl+V`)
  - Save current clipboard → set transcribed text → send Ctrl+V → restore original clipboard after 200ms
  - Pros: handles Unicode (Chinese chars) reliably, fast for long text
  - Cons: requires brief clipboard takeover
- Strategy B (fallback): direct keystroke simulation via `enigo.text()`
  - Pros: no clipboard side effects
  - Cons: slow for long text, some apps drop fast keystrokes

**Make injection method configurable in Settings.**

**Edge cases to handle**
- App loses focus during transcription → still type into wherever focus moved (BridgeVoice does this; it's the expected behavior)
- Empty transcription (silence detected) → no-op, no clipboard touch
- Very long text (>500 chars) → always use clipboard strategy regardless of setting

**Test matrix**
Verify injection works in all of:
- VS Code (multi-line)
- Chrome address bar + a Google Docs document
- Windows Terminal / PowerShell
- Slack desktop
- Discord
- Notepad
- A native Win32 app (e.g. Calculator's input field)

### Validation Criteria
- [ ] Text appears in focused app within 200ms of transcription return
- [ ] Chinese characters render correctly (not as `???`)
- [ ] Clipboard is restored to original content after injection
- [ ] No keyboard layout issues (test with US, UK, and any IME you use)

---

## Stage 4 — Local Transcription with whisper.cpp

**Goal**: Default to on-device transcription. Cloud becomes opt-in.

**Time**: 6-8 hours (this is the trickiest stage)
**Output**: Privacy-first dictation that works offline

### Tasks

**Add `whisper-rs` dependency**
```toml
whisper-rs = { version = "0.13", features = ["cuda"] }  # or "vulkan" for non-NVIDIA
```

**Model management**
- Model files (`ggml-base.bin`, `ggml-small.bin`, etc.) are 100MB-3GB each
- Don't bundle in installer — download on first run from Hugging Face
- URLs follow pattern: `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{size}.bin`
- Recommended models for Windows + bilingual (zh/en):
  - `ggml-small.bin` (466MB) — fast, decent accuracy, default
  - `ggml-medium.bin` (1.5GB) — better accuracy, slower
  - `ggml-large-v3-turbo.bin` (1.6GB) — best balance for high-end machines
- Show download progress in Settings UI
- Verify SHA256 after download

**GPU detection**
- On startup, detect: CUDA (NVIDIA), Vulkan (anything else), or CPU fallback
- Surface this in Settings so user knows why it's slow
- whisper-rs feature flags are compile-time — ship two binaries or use `vulkan` (works almost everywhere)

**Local inference (`src-tauri/src/transcribe/local.rs`)**
- Load model once on app startup, keep `WhisperContext` in app state
- Set `WhisperFullParams`:
  - `language = "auto"` for code-switching, or let user lock it
  - `n_threads = num_cpus / 2`
  - `translate = false`
  - `no_context = true` (no need for prior context in dictation)
- Run inference on a tokio blocking thread (whisper-rs is sync)
- Stream tokens to UI if possible (whisper-rs supports callbacks)

**Mode switching**
- Settings toggle: Local / Cloud / Auto
  - Auto = Local first, fall back to Cloud if local fails or model not downloaded

### Performance Reality Check
On Windows without GPU:
- `tiny`: real-time on any CPU
- `base`: real-time on modern CPU
- `small`: ~2x real-time on i5/Ryzen 5
- `medium`: 5-10x real-time, painful for dictation

With CUDA on RTX 3060+:
- `large-v3-turbo`: <1x real-time, comfortable for dictation
- All smaller models: instant

If user has weak hardware, default to `base` and warn if they pick larger models.

### Validation Criteria
- [ ] Model downloads with progress UI, resumes on interrupt
- [ ] Transcription works with no internet connection
- [ ] Latency on user's machine is acceptable (configure default model accordingly)
- [ ] Cloud fallback kicks in if local fails
- [ ] Chinese+English code-switching still works locally

---

## Stage 5 — Floating Widget + Visual Polish

**Goal**: Always-on-top recording indicator the user can drag and double-click.

**Time**: 3-4 hours

### Tasks
- Create a second Tauri window: borderless, transparent, always-on-top, ~80x80px
- Show recording state visually (idle pulse / red dot when recording / spinner when transcribing)
- Make it draggable (`data-tauri-drag-region` on the React side)
- Position memory: save last position, restore on launch
- Double-click to toggle recording (alternative to hotkey)
- Toggle widget visibility from tray menu and Settings

### Design notes
- Look at BridgeVoice's widget for reference — minimal, glanceable, doesn't get in the way
- Use Tailwind + Framer Motion for the pulse animation
- Test against multi-monitor setups: widget should stay on its monitor when display config changes

### Validation Criteria
- [ ] Widget stays on top over fullscreen apps where possible
- [ ] Position persists across restarts
- [ ] Double-click recording works alongside hotkey
- [ ] Visual states are unambiguous

---

## Stage 6 — Custom Dictionary

**Goal**: Auto-correct words Whisper consistently mis-hears (especially technical jargon and proper nouns).

**Time**: 2-3 hours

### Tasks
- Settings UI: a table of `{ from, to }` replacement pairs
- Examples: `react js` → `React.js`, `type script` → `TypeScript`, `pee r` → `PR`, proper nouns
- Implement as plain string replacement on transcription output (case-insensitive option)
- Quick-add from history: right-click any past transcription word → "Always replace with..."
- Persist via `tauri-plugin-store`
- Apply replacements **before** clipboard injection

### Implementation note
Build a starter dictionary of common dev/tech terms Whisper gets wrong (framework names, CLI tools, abbreviations) and ship it as a default, opt-in toggle.

### Validation Criteria
- [ ] Replacements apply correctly across the 3 test scenarios from Stage 0
- [ ] No regression on already-correct text
- [ ] Order of rules is deterministic and predictable

---

## Stage 7 — Transcription History

**Goal**: Local searchable log of all transcriptions for review and dictionary-building.

**Time**: 3-4 hours

### Tasks
- Store each transcription with: timestamp, model used, duration, text, language detected
- Backend: SQLite via `rusqlite` crate, stored at `%APPDATA%\voice-dictation\history.db`
- Frontend: paginated list with full-text search
- Actions per row: copy, re-inject, delete, "add to dictionary"
- Bulk: clear all, export to JSON/Markdown
- Stats panel: total words dictated, total time saved (assume 40 WPM typing baseline), most-used hours

### Privacy
- All local. Never send history anywhere.
- Settings option: auto-delete entries older than N days

### Validation Criteria
- [ ] History survives app restarts
- [ ] Search returns results in <100ms even with 10k entries
- [ ] No history written if user disables it in Settings

---

## Stage 8 — Distribution & Polish

**Goal**: Shippable installer.

**Time**: 4-6 hours

### Tasks
- Configure `tauri.conf.json` for Windows MSI/NSIS bundles
- Add app icon (`.ico`) + tray icon variants (light/dark)
- Code-sign the binary if you have a certificate (optional but reduces SmartScreen warnings)
- Auto-updater via `tauri-plugin-updater` (point at GitHub Releases)
- First-run onboarding window:
  1. Welcome
  2. Pick transcription mode (local / cloud / auto)
  3. If local: pick model size, download
  4. If cloud: paste Groq API key
  5. Set hotkey
  6. Test recording → confirms everything works
- Crash reporting: log panics to `%APPDATA%\voice-dictation\logs\` (don't send anywhere without consent)

### Validation Criteria
- [ ] Fresh install on a clean Windows VM works end-to-end
- [ ] Onboarding completes in <2 minutes
- [ ] Auto-updater delivers a v0.1.1 to a v0.1.0 install
- [ ] Uninstaller removes everything except user data (history/settings)

---

## Known Pitfalls & How to Avoid Them

### Windows-specific gotchas
1. **Push-to-talk in packaged builds**: BridgeVoice publicly logged this bug — global keyboard hooks behave differently in NSIS-packaged builds vs dev. Test the packaged `.exe` early and often, not just `tauri dev`.
2. **Microphone permission**: Windows 10/11 will silently return empty audio if mic permission isn't granted. Detect this on first record attempt and link to Settings.
3. **AV false positives**: Apps that hook the keyboard often trigger antivirus warnings. Code signing helps but isn't a full fix. Document this in your README.
4. **High DPI**: Test the floating widget on a 4K display at 200% scaling.

### Whisper-specific gotchas
1. **Very short audio (<300ms)**: Whisper hallucinates. Reject these client-side before sending.
2. **Silence at start**: Pre-buffer ~200ms before the hotkey is pressed so you don't clip the first syllable. (Hard mode — defer this to v1.1.)
3. **Hallucination on noise**: Whisper sometimes outputs phrases like "Thanks for watching!" on background noise. Add a basic noise gate before sending.
4. **Language locking**: For pure-English dictation users, locking `language="en"` improves accuracy notably. Make this configurable.

### Code-switching reality
Whisper detects one primary language per chunk. For Chinese+English mix:
- Generally handles well (lots of training data for both)
- Technical English terms inside Chinese sentences may get romanized or mis-segmented
- The custom dictionary is your safety net — don't over-engineer the model layer; over-invest in good post-processing rules

---

## Useful Commands Reference

```bash
# Development
npm run tauri dev                  # Run in dev mode
npm run tauri build                # Production build → src-tauri/target/release/bundle/

# Rust
cd src-tauri && cargo check        # Fast type-check without building
cd src-tauri && cargo clippy       # Lints
cd src-tauri && cargo test         # Run Rust tests

# Frontend
npm run dev                        # Vite-only dev (faster for UI iteration)
npm run typecheck                  # TS check without build

# Whisper model download (manual, for local dev)
curl -L -o src-tauri/models/ggml-small.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin
```

---

## References

- BridgeVoice (the inspiration): https://www.bridgemind.ai/products/bridgevoice
- BridgeVoice docs (study their UX patterns): https://docs.bridgemind.ai/docs/bridgevoice
- Tauri 2.0 docs: https://tauri.app/
- whisper.cpp: https://github.com/ggerganov/whisper.cpp
- whisper-rs: https://github.com/tazz4843/whisper-rs
- Groq Whisper API docs: https://console.groq.com/docs/speech-to-text
- Groq free tier limits: 2,000 requests/day, 7,200 audio seconds/hour
- enigo (text injection): https://github.com/enigo-rs/enigo
- cpal (audio): https://github.com/RustAudio/cpal

---

## Working with Claude Code on This Project

When delegating to Claude Code, give it focused, scoped tasks per stage. Good prompts look like:

> "Implement Stage 2 audio capture only. Read PROJECT_GUIDE.md section 'Stage 2'. Use cpal as specified. Don't touch transcription or injection yet — just expose `start_recording` and `stop_recording` Tauri commands that return raw PCM samples. Write a unit test that captures 1 second of silence and verifies the buffer is the expected size."

Bad prompts (avoid):
> "Build the whole app from PROJECT_GUIDE.md"

Stage-by-stage with explicit validation criteria works much better — Claude Code can self-check against the checklist at the end of each stage.

### Suggested workflow
1. Tell Claude Code which stage you're on
2. Have it read this guide and the relevant prototype/existing code
3. Implement just that stage
4. Run the validation checklist together
5. Commit before moving on — keeps each stage as a clean rollback point