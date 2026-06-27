# Stage 5 — Floating Recording Widget — Design

**Date:** 2026-06-27
**Status:** Approved (design)
**Scope:** Implements PROJECT.md "Stage 5 — Floating Widget + Visual Polish".

## Goal

Add an always-on-top floating widget that shows the current dictation state at a
glance and offers an alternative to the hotkey. It must never interfere with the
app the user is dictating into.

## Decisions (from brainstorming)

- **Window/frontend architecture:** separate HTML entry point for the widget.
- **Animation:** Framer Motion.
- **Interactions shipped in v1:** drag + position memory, double-click to toggle
  recording, visibility toggle (tray + settings), and click-through.
- **Click-through behavior:** interactive when idle (drag + double-click work);
  click-through only while recording/transcribing. (Resolved against the literal
  "click-through when idle", which would have cancelled drag and double-click.)

## Architecture

### Widget window

A second Tauri window declared in `src-tauri/tauri.conf.json`:

- `label: "widget"`
- `width: 80`, `height: 80`
- `decorations: false`, `transparent: true`
- `alwaysOnTop: true`, `skipTaskbar: true`
- `resizable: false`, `maximizable: false`, `minimizable: false`
- `shadow: false`
- `visible: false` — shown/hidden in `setup()` based on the persisted
  `widgetVisible` preference (default visible).
- Loads `widget.html`.

**No-activate / no focus-steal (critical):** if the widget takes focus when
clicked, text injection would target the widget's own webview instead of the
user's real app. In `setup()`, after the window exists, apply the Win32 extended
window styles `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` to the widget HWND (via the
window's raw handle). The widget still receives mouse messages (click, double-
click, drag) but never activates, so the previously focused app remains the
injection target. This is Windows-only code, gated to `#[cfg(windows)]`.

### Frontend entry

- `widget.html` (new) → `src/widget.tsx` (new) → `src/Widget.tsx` (new component).
- `vite.config.ts`: add `build.rollupOptions.input` with two entries —
  `main: index.html` and `widget: widget.html`. Dev server serves both routes.
- The widget shares `src/App.css` (Tailwind) but does **not** import the settings
  `App` component, keeping its bundle minimal.

## State & visuals

The widget derives its state purely from existing global events emitted by the
backend — **no new events are required for state**:

- `recording` (bool) — already emitted on start/stop.
- `status` (string) — already emitted: "transcribing (...)", "injecting...",
  "done", "silence — discarded", etc.
- `transcription` (string) — already emitted when text is ready.

Derived states:

| State        | Trigger                                                        | Visual                         |
|--------------|---------------------------------------------------------------|--------------------------------|
| idle         | default; after `done`/`transcription`/discard                 | soft pulsing dot               |
| recording    | `recording == true`                                           | red breathing/pulsing dot      |
| transcribing | `recording == false` and `status` starts with "transcribing" or "injecting" | spinner    |
| done         | `transcription` received (brief), then → idle after ~1s       | brief check mark, then idle    |

Framer Motion drives the pulse and spinner transitions.

## Interactions

### Drag + position memory

- Root element carries `data-tauri-drag-region` so the whole widget is draggable.
- The widget subscribes to its own window `onMoved` event, debounced ~300ms, and
  persists `{ x, y }` to the existing settings store under key `widgetPosition`.
- On `setup()`, the backend reads `widgetPosition` and sets the widget's position
  before showing it. Position is clamped to the visible work area so a removed
  monitor can't strand the widget off-screen. (Full multi-monitor robustness is
  acceptable to keep basic for v1.)

### Double-click to toggle recording

- `onDoubleClick` on the widget calls a new `toggle_recording` Tauri command.
- Refactor: the start/stop logic currently inlined in the tray "toggle_record"
  menu handler is extracted into a shared function (e.g. `toggle_recording_impl`)
  used by the tray menu, the new command, and kept consistent with `handle_shortcut`.
- Because the window is `WS_EX_NOACTIVATE`, the double-click does not steal focus.

### Visibility toggle

- New command `set_widget_visible(visible: bool)`:
  - shows/hides the `widget` window,
  - writes `widgetVisible` into the settings store,
  - emits a `widget-visible` (bool) event so the Settings checkbox stays in sync.
- Tray menu: new item "Show/Hide Widget" that flips current visibility via the
  command.
- Settings UI: a new checkbox bound to `widgetVisible`, listening to the
  `widget-visible` event to reflect changes made from the tray.
- `setup()` reads `widgetVisible` (default `true`) and shows/hides accordingly.

### Click-through (interactive when idle, pass-through when active)

- When the derived state becomes **recording** or **transcribing**, the widget
  calls `getCurrentWindow().setIgnoreCursorEvents(true)` so clicks pass through to
  the app being dictated into.
- When the state returns to **idle**/**done**, it calls
  `setIgnoreCursorEvents(false)` so drag and double-click work again.

## Backend changes summary (`src-tauri`)

- `tauri.conf.json`: add the `widget` window.
- `lib.rs`:
  - Extract shared `toggle_recording_impl(app)` from the tray handler.
  - New commands: `toggle_recording`, `set_widget_visible`; register in
    `invoke_handler`.
  - In `setup()`: restore widget position from `widgetPosition`, apply the
    `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` styles (`#[cfg(windows)]`), and apply the
    `widgetVisible` preference.
  - Tray menu: add "Show/Hide Widget" item and handler.
- Capabilities (`capabilities/default.json`): add `"widget"` to the `windows`
  list and any window permissions the widget needs
  (`core:window:allow-set-ignore-cursor-events`, `allow-set-position`,
  `allow-start-dragging`, `allow-show`, `allow-hide`, plus `event` + `store`
  defaults already present).

## Frontend changes summary (`src`)

- New `widget.html`, `src/widget.tsx`, `src/Widget.tsx`.
- `vite.config.ts`: multi-entry `rollupOptions.input`.
- `src/App.tsx`: add the "Show widget" checkbox + `widget-visible` listener.
- `package.json`: add `framer-motion`.
- `src/types.ts`: add `widgetVisible` (and `widgetPosition` if typed) to settings.

## Error handling & edge cases

- Widget window missing (e.g. failed to create): commands log a warning and no-op
  rather than panicking.
- Stored position off-screen / monitor removed: clamp into the primary work area.
- Rapid double-clicks while a transcription is in flight: `toggle_recording_impl`
  relies on `recorder.is_running()` as the single source of truth, matching
  existing tray behavior.
- `setIgnoreCursorEvents` failures: logged, non-fatal (widget stays interactive).

## Testing / validation (from PROJECT.md Stage 5 checklist)

- [ ] Widget stays on top over other windows.
- [ ] Position persists across restarts.
- [ ] Double-click recording works alongside the hotkey, without stealing focus
      from the injection target.
- [ ] Visual states (idle / recording / transcribing) are unambiguous.
- [ ] While recording/transcribing, clicks pass through to the underlying app.
- [ ] Visibility toggle works from both tray and Settings and stays in sync.

## Out of scope (defer to v1.1)

- Multi-monitor display-config-change tracking beyond basic on-screen clamping.
- Snapping/edge docking.
- Per-monitor DPI-specific sizing.
