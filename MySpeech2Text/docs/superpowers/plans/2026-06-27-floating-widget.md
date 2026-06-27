# Floating Recording Widget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an always-on-top floating widget that shows dictation state at a glance and offers double-click-to-record, drag, position memory, visibility toggle, and click-through while active — without ever stealing focus from the app being dictated into.

**Architecture:** A second Tauri window (`label: "widget"`) with its own Vite HTML entry (`widget.html` → `src/widget.tsx` → `src/Widget.tsx`). The widget derives all state from existing global events (`recording`, `status`, `transcription`) — no new state events. Win32 extended styles make it non-activating. New Tauri commands (`toggle_recording`, `set_widget_visible`) and a shared `toggle_recording_impl` back the double-click and visibility features.

**Tech Stack:** Tauri 2.11, Rust, React 19 + TypeScript, Tailwind v4, Framer Motion, `windows` crate 0.56 (Win32 FFI), `tauri-plugin-store`.

## Global Constraints

- Target platform: Windows 10/11 (Win32 FFI gated `#[cfg(windows)]`).
- Tauri `2.x`, React `^19`, store plugin already present.
- The widget must NEVER take focus (injection targets the previously focused app).
- Widget preferences are stored as **separate top-level keys** in `settings.json`
  (`widgetVisible`, `widgetPosition`) — NOT inside the `settings` object — so the
  Settings save flow never clobbers them.
- Default widget visibility on first run: **visible**.
- Custom Tauri commands need no capability entry; only core/plugin commands do.
- Match existing code style; tests use `#[cfg(test)] mod tests` + `cargo test`
  (see `src/dictionary.rs`).

---

## File Structure

- `package.json` — add `framer-motion`.
- `src-tauri/Cargo.toml` — add `windows` (target-gated to Windows).
- `vite.config.ts` — multi-entry build (`main`, `widget`).
- `widget.html` (new) — widget window HTML entry.
- `src/widget.tsx` (new) — React root for the widget.
- `src/Widget.tsx` (new) — widget component (state, visuals, interactions).
- `src-tauri/tauri.conf.json` — add the `widget` window.
- `src-tauri/capabilities/default.json` — grant the widget window its permissions.
- `src-tauri/src/lib.rs` — widget setup (position restore, no-activate styles,
  visibility), shared `toggle_recording_impl`, new commands, tray menu item.
- `src/App.tsx` — "Show widget" checkbox + `widget-visible` listener.

---

## Task 1: Scaffold the widget window (static dot)

**Files:**
- Modify: `package.json` (add dependency)
- Modify: `vite.config.ts`
- Create: `widget.html`
- Create: `src/widget.tsx`
- Create: `src/Widget.tsx`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`

**Interfaces:**
- Produces: a transparent, always-on-top `widget` window that renders a static
  centered dot. Later tasks add behavior to `src/Widget.tsx`.

- [ ] **Step 1: Add framer-motion**

Run: `npm install framer-motion`
Expected: `framer-motion` appears under `dependencies` in `package.json`.

- [ ] **Step 2: Configure Vite multi-entry build**

Edit `vite.config.ts`. Add a `build` block inside the returned config object
(after the `server` block):

```ts
  build: {
    rollupOptions: {
      input: {
        main: "index.html",
        widget: "widget.html",
      },
    },
  },
```

- [ ] **Step 3: Create `widget.html`**

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Voice Dictation — Widget</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/widget.tsx"></script>
  </body>
</html>
```

- [ ] **Step 4: Create `src/widget.tsx`**

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import Widget from "./Widget";
import "./App.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Widget />
  </React.StrictMode>,
);
```

- [ ] **Step 5: Create `src/Widget.tsx` (static placeholder)**

```tsx
function Widget() {
  return (
    <div
      data-tauri-drag-region
      className="w-screen h-screen flex items-center justify-center bg-transparent cursor-pointer select-none"
    >
      <div className="w-6 h-6 rounded-full bg-neutral-400" />
    </div>
  );
}

export default Widget;
```

- [ ] **Step 6: Add the widget window to `tauri.conf.json`**

In `app.windows`, add a second entry after the `settings` window object:

```json
      {
        "label": "widget",
        "url": "widget.html",
        "width": 80,
        "height": 80,
        "decorations": false,
        "transparent": true,
        "alwaysOnTop": true,
        "skipTaskbar": true,
        "resizable": false,
        "maximizable": false,
        "minimizable": false,
        "shadow": false,
        "focus": false,
        "visible": false
      }
```

- [ ] **Step 7: Grant the widget window permissions in `capabilities/default.json`**

Change `"windows": ["settings"]` to `"windows": ["settings", "widget"]`, and add
these entries to the `permissions` array:

```json
    "core:window:allow-start-dragging",
    "core:window:allow-set-position",
    "core:window:allow-set-ignore-cursor-events"
```

- [ ] **Step 8: Verify frontend types and Rust compile**

Run: `npm run typecheck`
Expected: no errors.

Run: `cd src-tauri && cargo check`
Expected: `Finished` with no errors.

- [ ] **Step 9: Manual check — widget renders**

Run: `npm run tauri dev`. Temporarily nothing shows the widget yet (visible:false).
To eyeball it now, in `tauri.conf.json` set the widget `"visible": true`
temporarily, relaunch, confirm a small transparent always-on-top window with a
grey dot appears, then set it back to `"visible": false`.
Expected: borderless ~80px dot window, always on top, no taskbar entry.

- [ ] **Step 10: Commit**

```bash
git add package.json package-lock.json vite.config.ts widget.html src/widget.tsx src/Widget.tsx src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "feat: scaffold floating widget window"
```

---

## Task 2: Make the widget non-activating (Win32)

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: the `widget` window from Task 1.
- Produces: `fn apply_no_activate(window: &tauri::WebviewWindow)` applied in
  `setup()`; the widget no longer steals focus when clicked.

- [ ] **Step 1: Add the `windows` crate (Windows-only)**

In `src-tauri/Cargo.toml`, add a target-specific dependency section (after the
existing `[dependencies]` block):

```toml
[target.'cfg(windows)'.dependencies]
windows = { version = "0.56", features = [
  "Win32_Foundation",
  "Win32_UI_WindowsAndMessaging",
] }
```

- [ ] **Step 2: Add the no-activate helper in `lib.rs`**

Add near the other free functions (e.g. after `update_tray_tooltip`):

```rust
#[cfg(windows)]
fn apply_no_activate(window: &tauri::WebviewWindow) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    let hwnd = match window.hwnd() {
        Ok(h) => h,
        Err(e) => {
            log::warn!("widget hwnd unavailable: {e}");
            return;
        }
    };
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let new = ex | (WS_EX_NOACTIVATE.0 as isize) | (WS_EX_TOOLWINDOW.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);
    }
}

#[cfg(not(windows))]
fn apply_no_activate(_window: &tauri::WebviewWindow) {}
```

- [ ] **Step 3: Call it in `setup()`**

In the `.setup(|app| { ... })` closure, after the tray is built and before the
final `Ok(())`, add:

```rust
            if let Some(widget) = app.get_webview_window("widget") {
                apply_no_activate(&widget);
            } else {
                log::warn!("widget window not found at setup");
            }
```

- [ ] **Step 4: Verify compile**

Run: `cd src-tauri && cargo check`
Expected: `Finished`, no errors. (If `hwnd()` type mismatches, confirm the
`windows` crate version resolves to the same one Tauri uses via
`cargo tree -i windows`.)

- [ ] **Step 5: Manual focus check**

Temporarily set the widget `"visible": true`. Run `npm run tauri dev`. Put the
text caret in another app (e.g. Notepad), click the widget.
Expected: the caret/focus stays in Notepad — the widget does not activate. Revert
`visible` to `false`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs
git commit -m "feat: make floating widget non-activating (WS_EX_NOACTIVATE)"
```

---

## Task 3: Recording-state visuals (Framer Motion)

**Files:**
- Modify: `src/Widget.tsx`

**Interfaces:**
- Consumes: existing global events `recording` (bool), `status` (string),
  `transcription` (string).
- Produces: a `WidgetState = "idle" | "recording" | "transcribing" | "done"`
  reflected visually; later tasks read this same state for click-through.

- [ ] **Step 1: Implement state derivation + animated visuals**

Replace the contents of `src/Widget.tsx` with:

```tsx
import { useEffect, useRef, useState } from "react";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { motion } from "framer-motion";

type WidgetState = "idle" | "recording" | "transcribing" | "done";

function Widget() {
  const [state, setState] = useState<WidgetState>("idle");
  const doneTimer = useRef<number | null>(null);

  useEffect(() => {
    const unlistens: UnlistenFn[] = [];
    (async () => {
      unlistens.push(
        await listen<boolean>("recording", (e) => {
          if (e.payload) setState("recording");
        }),
      );
      unlistens.push(
        await listen<string>("status", (e) => {
          const s = e.payload;
          if (s.startsWith("transcribing") || s.startsWith("injecting")) {
            setState("transcribing");
          } else if (
            s.startsWith("silence") ||
            s.startsWith("too short") ||
            s.startsWith("error") ||
            s.includes("failed")
          ) {
            setState("idle");
          }
        }),
      );
      unlistens.push(
        await listen<string>("transcription", () => {
          setState("done");
          if (doneTimer.current) window.clearTimeout(doneTimer.current);
          doneTimer.current = window.setTimeout(() => setState("idle"), 1000);
        }),
      );
    })();
    return () => {
      unlistens.forEach((un) => un());
      if (doneTimer.current) window.clearTimeout(doneTimer.current);
    };
  }, []);

  return (
    <div
      data-tauri-drag-region
      className="w-screen h-screen flex items-center justify-center bg-transparent cursor-pointer select-none"
    >
      <Indicator state={state} />
    </div>
  );
}

function Indicator({ state }: { state: WidgetState }) {
  if (state === "transcribing") {
    return (
      <motion.div
        className="w-6 h-6 rounded-full border-2 border-blue-500 border-t-transparent"
        animate={{ rotate: 360 }}
        transition={{ repeat: Infinity, ease: "linear", duration: 0.8 }}
      />
    );
  }
  if (state === "done") {
    return (
      <motion.div
        className="w-7 h-7 rounded-full bg-green-500 flex items-center justify-center text-white text-sm"
        initial={{ scale: 0.6, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
      >
        ✓
      </motion.div>
    );
  }
  const recording = state === "recording";
  return (
    <motion.div
      className={`rounded-full ${recording ? "bg-red-500" : "bg-neutral-400"}`}
      style={{ width: 24, height: 24 }}
      animate={
        recording
          ? { scale: [1, 1.25, 1], opacity: [0.85, 1, 0.85] }
          : { scale: [1, 1.08, 1], opacity: [0.6, 0.8, 0.6] }
      }
      transition={{ repeat: Infinity, duration: recording ? 0.9 : 2.2, ease: "easeInOut" }}
    />
  );
}

export default Widget;
```

- [ ] **Step 2: Verify types**

Run: `npm run typecheck`
Expected: no errors.

- [ ] **Step 3: Manual state check**

Temporarily set the widget `"visible": true`, run `npm run tauri dev`. Hold the
hotkey: widget turns red and pulses; on release it shows a spinner, then a brief
check, then returns to the idle grey pulse. Revert `visible` to `false`.

- [ ] **Step 4: Commit**

```bash
git add src/Widget.tsx
git commit -m "feat: animate widget recording states"
```

---

## Task 4: Click-through while active

**Files:**
- Modify: `src/Widget.tsx`

**Interfaces:**
- Consumes: `WidgetState` from Task 3.
- Produces: cursor events ignored while `recording`/`transcribing`, restored
  otherwise.

- [ ] **Step 1: Toggle ignore-cursor-events on state change**

In `src/Widget.tsx`, add the import:

```tsx
import { getCurrentWindow } from "@tauri-apps/api/window";
```

Then add this effect inside `Widget`, after the existing `useEffect`:

```tsx
  useEffect(() => {
    const passThrough = state === "recording" || state === "transcribing";
    getCurrentWindow()
      .setIgnoreCursorEvents(passThrough)
      .catch((e) => console.warn("setIgnoreCursorEvents failed", e));
  }, [state]);
```

- [ ] **Step 2: Verify types**

Run: `npm run typecheck`
Expected: no errors.

- [ ] **Step 3: Manual pass-through check**

Temporarily set widget `"visible": true`. Position the widget over a clickable
element in another window. While idle, the widget blocks/handles clicks; while
holding the hotkey (recording), clicking through the widget reaches the window
underneath. Revert `visible` to `false`.

- [ ] **Step 4: Commit**

```bash
git add src/Widget.tsx
git commit -m "feat: widget click-through while recording/transcribing"
```

---

## Task 5: Double-click to toggle recording

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/Widget.tsx`

**Interfaces:**
- Produces: Tauri command `toggle_recording` and shared
  `fn toggle_recording_impl(app: &AppHandle)`; the tray "toggle_record" handler
  and the widget double-click both use them.

- [ ] **Step 1: Extract shared toggle logic in `lib.rs`**

Add this function near `start_recording`:

```rust
fn toggle_recording_impl(app: &AppHandle) {
    let recorder = app.state::<AppState>().recorder.clone();
    if recorder.is_running() {
        let app_clone = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = stop_and_transcribe(app_clone.clone(), true).await {
                let _ = app_clone.emit("status", format!("transcription failed: {e:#}"));
                log::error!("transcribe error: {e:#}");
            }
        });
    } else if let Err(e) = start_recording(app) {
        let _ = app.emit("status", format!("error starting: {e:#}"));
        log::error!("start error: {e:#}");
    }
}
```

- [ ] **Step 2: Add the `toggle_recording` command**

Add with the other `#[tauri::command]` functions:

```rust
#[tauri::command]
fn toggle_recording(app: AppHandle) {
    toggle_recording_impl(&app);
}
```

- [ ] **Step 3: Use the shared fn in the tray handler**

In the tray `on_menu_event` closure, replace the existing `"toggle_record"` arm
body with:

```rust
                    "toggle_record" => {
                        toggle_recording_impl(app);
                    }
```

- [ ] **Step 4: Register the command**

Add `toggle_recording` to the `tauri::generate_handler![...]` list.

- [ ] **Step 5: Wire double-click in the widget**

In `src/Widget.tsx`, add the import:

```tsx
import { invoke } from "@tauri-apps/api/core";
```

Add an `onDoubleClick` to the root `div`:

```tsx
      onDoubleClick={() =>
        invoke("toggle_recording").catch((e) => console.warn("toggle failed", e))
      }
```

- [ ] **Step 6: Verify compile + types**

Run: `cd src-tauri && cargo check`
Expected: `Finished`, no errors.

Run: `npm run typecheck`
Expected: no errors.

- [ ] **Step 7: Manual double-click check**

Temporarily set widget `"visible": true`, run `npm run tauri dev`. Double-click
the widget: recording starts (red pulse). Double-click again: it stops and
transcribes. Confirm focus stays in the other app throughout. Revert `visible`.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/lib.rs src/Widget.tsx
git commit -m "feat: double-click widget to toggle recording"
```

---

## Task 6: Drag + position memory

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/Widget.tsx`

**Interfaces:**
- Consumes: store file `settings.json`, top-level key `widgetPosition`
  (`{ x: number, y: number }`).
- Produces: `fn clamp_position(x, y, w, h, area_w, area_h) -> (i32, i32)` (unit
  tested); widget persists its position and `setup()` restores it.

- [ ] **Step 1: Write the failing clamp test**

In `src-tauri/src/lib.rs`, add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::clamp_position;

    #[test]
    fn keeps_on_screen_position_unchanged() {
        assert_eq!(clamp_position(100, 100, 80, 80, 1920, 1080), (100, 100));
    }

    #[test]
    fn clamps_off_right_and_bottom() {
        assert_eq!(clamp_position(2000, 2000, 80, 80, 1920, 1080), (1840, 1000));
    }

    #[test]
    fn clamps_negative() {
        assert_eq!(clamp_position(-50, -30, 80, 80, 1920, 1080), (0, 0));
    }
}
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd src-tauri && cargo test clamp 2>&1 | head -20`
Expected: FAIL — `cannot find function clamp_position`.

- [ ] **Step 3: Implement `clamp_position`**

Add near the other free functions in `lib.rs`:

```rust
fn clamp_position(x: i32, y: i32, w: i32, h: i32, area_w: i32, area_h: i32) -> (i32, i32) {
    let max_x = (area_w - w).max(0);
    let max_y = (area_h - h).max(0);
    (x.clamp(0, max_x), y.clamp(0, max_y))
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd src-tauri && cargo test clamp 2>&1 | tail -10`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 5: Restore widget position in `setup()`**

In the `setup()` closure, where the widget window is fetched (Task 2 added an
`if let Some(widget) = ...` block), extend it to restore position before any
later show:

```rust
            if let Some(widget) = app.get_webview_window("widget") {
                apply_no_activate(&widget);
                if let Ok(store) = app.store(STORE_FILE) {
                    if let Some(pos) = store.get("widgetPosition") {
                        if let (Some(x), Some(y)) = (
                            pos.get("x").and_then(|v| v.as_i64()),
                            pos.get("y").and_then(|v| v.as_i64()),
                        ) {
                            if let Ok(Some(mon)) = widget.primary_monitor() {
                                let size = mon.size();
                                let (cx, cy) = clamp_position(
                                    x as i32, y as i32, 80, 80,
                                    size.width as i32, size.height as i32,
                                );
                                let _ = widget.set_position(tauri::PhysicalPosition::new(cx, cy));
                            } else {
                                let _ = widget.set_position(tauri::PhysicalPosition::new(
                                    x as i32, y as i32,
                                ));
                            }
                        }
                    }
                }
            } else {
                log::warn!("widget window not found at setup");
            }
```

- [ ] **Step 6: Persist position from the widget on move**

In `src/Widget.tsx`, add imports:

```tsx
import { load } from "@tauri-apps/plugin-store";
```

Add this effect inside `Widget`:

```tsx
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let timer: number | null = null;
    (async () => {
      const win = getCurrentWindow();
      unlisten = await win.onMoved(({ payload }) => {
        if (timer) window.clearTimeout(timer);
        timer = window.setTimeout(async () => {
          try {
            const store = await load("settings.json", { defaults: {} });
            await store.set("widgetPosition", { x: payload.x, y: payload.y });
            await store.save();
          } catch (e) {
            console.warn("save widget position failed", e);
          }
        }, 300);
      });
    })();
    return () => {
      if (unlisten) unlisten();
      if (timer) window.clearTimeout(timer);
    };
  }, []);
```

- [ ] **Step 7: Verify compile + types**

Run: `cd src-tauri && cargo test clamp 2>&1 | tail -5` → `ok`.
Run: `npm run typecheck` → no errors.

- [ ] **Step 8: Manual drag + persistence check**

Temporarily set widget `"visible": true`. Run `npm run tauri dev`. Drag the
widget to a new spot, quit, relaunch.
Expected: the widget reappears where you left it. Revert `visible` to `false`.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/lib.rs src/Widget.tsx
git commit -m "feat: widget drag and position memory"
```

---

## Task 7: Visibility toggle (tray + settings)

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: store key `widgetVisible` (bool, default `true`).
- Produces: command `set_widget_visible(app, visible: bool)` that shows/hides the
  widget, persists `widgetVisible`, and emits a `widget-visible` (bool) event;
  tray menu item `toggle_widget`; Settings checkbox.

- [ ] **Step 1: Add the `set_widget_visible` command in `lib.rs`**

```rust
#[tauri::command]
fn set_widget_visible(app: AppHandle, visible: bool) {
    set_widget_visible_impl(&app, visible);
}

fn set_widget_visible_impl(app: &AppHandle, visible: bool) {
    if let Some(widget) = app.get_webview_window("widget") {
        if visible {
            let _ = widget.show();
        } else {
            let _ = widget.hide();
        }
    }
    if let Ok(store) = app.store(STORE_FILE) {
        store.set("widgetVisible", serde_json::json!(visible));
        let _ = store.save();
    }
    let _ = app.emit("widget-visible", visible);
}
```

- [ ] **Step 2: Apply persisted visibility in `setup()`**

At the end of the widget `if let Some(widget)` block from Task 6 (after position
restore, still inside the block), add:

```rust
                let visible = app
                    .store(STORE_FILE)
                    .ok()
                    .and_then(|s| s.get("widgetVisible"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true);
                if visible {
                    let _ = widget.show();
                } else {
                    let _ = widget.hide();
                }
```

(Note: the widget window is configured `"visible": false`, so this is the single
place visibility is decided on launch.)

- [ ] **Step 3: Add the tray menu item**

In `setup()`, where the menu items are built, add after `history_item`:

```rust
            let toggle_widget =
                MenuItem::with_id(app, "toggle_widget", "Show/Hide Widget", true, None::<&str>)?;
```

Update the `Menu::with_items` call to include it:

```rust
            let menu = Menu::with_items(
                app,
                &[&toggle_record, &settings, &history_item, &toggle_widget, &quit],
            )?;
```

- [ ] **Step 4: Handle the tray item**

In the tray `on_menu_event` match, add an arm:

```rust
                    "toggle_widget" => {
                        let currently = app
                            .get_webview_window("widget")
                            .map(|w| w.is_visible().unwrap_or(false))
                            .unwrap_or(false);
                        set_widget_visible_impl(app, !currently);
                    }
```

- [ ] **Step 5: Register the command**

Add `set_widget_visible` to `tauri::generate_handler![...]`.

- [ ] **Step 6: Verify Rust compile**

Run: `cd src-tauri && cargo check`
Expected: `Finished`, no errors.

- [ ] **Step 7: Add the Settings checkbox in `App.tsx`**

After the `autostart` state declaration (`const [autostart, ...]`), add:

```tsx
  const [widgetVisible, setWidgetVisible] = useState(true);
```

Inside the main init `useEffect` (where other listeners are pushed), add a
listener and load the initial value:

```tsx
      unlistens.current.push(
        await listen<boolean>("widget-visible", (e) => setWidgetVisible(e.payload)),
      );
      try {
        const v = await s.get<boolean>("widgetVisible");
        setWidgetVisible(v ?? true);
      } catch (e) {
        console.warn("read widgetVisible failed", e);
      }
```

Add a handler near `toggleAutostart`:

```tsx
  async function toggleWidget(on: boolean) {
    setWidgetVisible(on);
    try {
      await invoke("set_widget_visible", { visible: on });
    } catch (e) {
      setStatus(`widget error: ${String(e)}`);
    }
  }
```

Add a new `Field` in the Settings section (e.g. right after the "Start on login"
field):

```tsx
          <Field label="Floating widget" hint="Always-on-top recording indicator. Drag to move, double-click to record.">
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={widgetVisible}
                onChange={(e) => toggleWidget(e.target.checked)}
              />
              <span>{widgetVisible ? "Shown" : "Hidden"}</span>
            </label>
          </Field>
```

- [ ] **Step 8: Verify types**

Run: `npm run typecheck`
Expected: no errors.

- [ ] **Step 9: Manual visibility check**

Run `npm run tauri dev`. The widget shows by default. Toggle "Show/Hide Widget"
from the tray → widget hides/shows and the Settings checkbox updates to match.
Toggle the Settings checkbox → widget hides/shows. Quit and relaunch → last
visibility state is restored.

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/lib.rs src/App.tsx
git commit -m "feat: widget visibility toggle from tray and settings"
```

---

## Final validation (Stage 5 checklist from PROJECT.md)

After Task 7, run a full `npm run tauri dev` pass and confirm:

- [ ] Widget stays on top of other windows.
- [ ] Position persists across restarts.
- [ ] Double-click recording works alongside the hotkey, without stealing focus
      from the injection target (text injects into the real app, not the widget).
- [ ] Idle / recording / transcribing visual states are unambiguous.
- [ ] While recording/transcribing, clicks pass through to the app underneath.
- [ ] Visibility toggle works from both tray and Settings and stays in sync.
- [ ] `cd src-tauri && cargo test` passes (clamp tests).
- [ ] `npm run typecheck` is clean.

Then update `README.md` "Status & next step" to mark Stage 5 done and point at the
remaining work (Stage 8 onboarding/updater, model SHA256, history extras), and
commit.

---

## Notes / risk areas

- **`windows` crate version unification (Task 2):** Tauri 2.11 already pulls
  `windows` 0.56; specifying `0.56` lets `window.hwnd()`'s `HWND` type match. If
  `cargo check` complains about mismatched `HWND`, run `cargo tree -i windows`
  and align the version to Tauri's.
- **Manual validation:** window/FFI/animation behavior isn't unit-testable here;
  only `clamp_position` is. All other tasks gate on `cargo check`/`typecheck` plus
  the manual steps. This is intentional, not an omission.
- **Temporary `visible: true`:** several manual checks flip the widget visible
  before Task 7 wires real visibility control. Always revert to `false` after, as
  Task 7 + the store key become the source of truth.
