import { useCallback, useEffect, useRef, useState } from "react";
import { load, Store } from "@tauri-apps/plugin-store";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import {
  DEFAULT_SETTINGS,
  DictRule,
  DownloadEvent,
  HistoryEntry,
  HistoryPage,
  LocalModelStatus,
  Settings,
} from "./types";
import "./App.css";


const STORE_FILE = "settings.json";

interface DownloadState {
  downloaded: number;
  total: number | null;
}

function App() {
  const [store, setStore] = useState<Store | null>(null);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [savedAt, setSavedAt] = useState<string>("");

  const [isRecording, setIsRecording] = useState(false);
  const [autostart, setAutostart] = useState(false);
  const [status, setStatus] = useState<string>("idle");
  const [lastTranscription, setLastTranscription] = useState<string>("");
  const [testing, setTesting] = useState(false);

  const [models, setModels] = useState<LocalModelStatus[]>([]);
  const [downloads, setDownloads] = useState<Record<string, DownloadState>>({});

  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [historyTotal, setHistoryTotal] = useState<number>(0);
  const [historyQuery, setHistoryQuery] = useState<string>("");
  const [historyOffset, setHistoryOffset] = useState<number>(0);
  const PAGE_SIZE = 20;

  const unlistens = useRef<UnlistenFn[]>([]);

  const refreshModels = useCallback(async () => {
    try {
      const list = await invoke<LocalModelStatus[]>("list_local_models");
      setModels(list);
    } catch (e) {
      console.warn("list_local_models failed", e);
    }
  }, []);

  const refreshHistory = useCallback(
    async (q: string, offset: number) => {
      try {
        const page = await invoke<HistoryPage>("list_history", {
          query: q || null,
          limit: PAGE_SIZE,
          offset,
        });
        setHistory(page.entries);
        setHistoryTotal(page.total);
      } catch (e) {
        console.warn("list_history failed", e);
      }
    },
    [],
  );

  useEffect(() => {
    (async () => {
      const s = await load(STORE_FILE, { defaults: {} });
      const persisted = (await s.get<Partial<Settings>>("settings")) ?? {};
      setSettings({ ...DEFAULT_SETTINGS, ...persisted });
      setStore(s);

      unlistens.current.push(
        await listen<boolean>("recording", (e) => setIsRecording(e.payload)),
      );
      unlistens.current.push(
        await listen<string>("status", (e) => setStatus(e.payload)),
      );
      unlistens.current.push(
        await listen<string>("transcription", (e) => {
          setLastTranscription(e.payload);
          setStatus("done");
          refreshHistory("", 0);
          setHistoryQuery("");
          setHistoryOffset(0);
        }),
      );
      unlistens.current.push(
        await listen<DownloadEvent>("model-download", (e) => {
          const ev = e.payload;
          setDownloads((prev) => {
            const next = { ...prev };
            switch (ev.kind) {
              case "started":
                next[ev.name] = { downloaded: 0, total: ev.total };
                break;
              case "progress":
                next[ev.name] = { downloaded: ev.downloaded, total: ev.total };
                break;
              case "done":
              case "failed":
              case "canceled":
                delete next[ev.name];
                break;
            }
            return next;
          });
          if (ev.kind === "done" || ev.kind === "failed" || ev.kind === "canceled") {
            refreshModels();
            if (ev.kind === "failed") setStatus(`download failed: ${ev.error}`);
            if (ev.kind === "done") setStatus(`downloaded ${ev.name}`);
          }
        }),
      );

      await refreshModels();
      await refreshHistory("", 0);

      try {
        setAutostart(await isEnabled());
      } catch (e) {
        console.warn("autostart isEnabled failed", e);
      }
    })();

    return () => {
      unlistens.current.forEach((un) => un());
      unlistens.current = [];
    };
  }, [refreshModels, refreshHistory]);

  useEffect(() => {
    const t = setTimeout(() => {
      refreshHistory(historyQuery, historyOffset);
    }, 150);
    return () => clearTimeout(t);
  }, [historyQuery, historyOffset, refreshHistory]);

  async function update<K extends keyof Settings>(key: K, value: Settings[K]) {
    const next = { ...settings, [key]: value };
    setSettings(next);
    if (store) {
      await store.set("settings", next);
      await store.save();
      setSavedAt(new Date().toLocaleTimeString());
    }
  }

  async function toggleAutostart(on: boolean) {
    try {
      if (on) await enable();
      else await disable();
      setAutostart(await isEnabled());
    } catch (e) {
      setStatus(`autostart error: ${String(e)}`);
    }
  }

  async function runTestRecord() {
    setTesting(true);
    setStatus("recording 3s...");
    try {
      const text = await invoke<string>("test_record_3s");
      if (text) setLastTranscription(text);
    } catch (e) {
      setStatus(`error: ${String(e)}`);
    } finally {
      setTesting(false);
    }
  }

  async function startDownload(name: string) {
    try {
      setDownloads((p) => ({ ...p, [name]: { downloaded: 0, total: null } }));
      await invoke("download_model", { name });
    } catch (e) {
      setStatus(`download error: ${String(e)}`);
      setDownloads((p) => {
        const n = { ...p };
        delete n[name];
        return n;
      });
    }
  }

  async function cancelDownload(name: string) {
    try {
      await invoke("cancel_model_download", { name });
    } catch (e) {
      console.warn("cancel failed", e);
    }
  }

  async function deleteModel(name: string) {
    if (!confirm(`Delete local model "${name}"?`)) return;
    try {
      await invoke("delete_local_model", { name });
      await refreshModels();
    } catch (e) {
      setStatus(`delete error: ${String(e)}`);
    }
  }

  function addRule() {
    const next: DictRule[] = [
      ...settings.dictionary,
      { from: "", to: "", caseInsensitive: true, enabled: true },
    ];
    update("dictionary", next);
  }

  function updateRule(index: number, patch: Partial<DictRule>) {
    const next = settings.dictionary.map((r, i) => (i === index ? { ...r, ...patch } : r));
    update("dictionary", next);
  }

  function removeRule(index: number) {
    const next = settings.dictionary.filter((_, i) => i !== index);
    update("dictionary", next);
  }

  async function deleteHistoryEntry(id: number) {
    try {
      await invoke("delete_history_entry", { id });
      await refreshHistory(historyQuery, historyOffset);
    } catch (e) {
      setStatus(`delete error: ${String(e)}`);
    }
  }

  async function clearAllHistory() {
    if (!confirm("Clear all transcription history? This cannot be undone.")) return;
    try {
      await invoke("clear_history");
      setHistoryOffset(0);
      await refreshHistory("", 0);
    } catch (e) {
      setStatus(`clear error: ${String(e)}`);
    }
  }

  async function copyToClipboard(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      setStatus("copied to clipboard");
    } catch (e) {
      setStatus(`copy error: ${String(e)}`);
    }
  }

  function formatTime(unix: number): string {
    const d = new Date(unix * 1000);
    const today = new Date();
    const sameDay =
      d.getFullYear() === today.getFullYear() &&
      d.getMonth() === today.getMonth() &&
      d.getDate() === today.getDate();
    if (sameDay) {
      return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    }
    return d.toLocaleString([], {
      month: "short",
      day: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  return (
    <main className="min-h-screen bg-neutral-50 dark:bg-neutral-900 text-neutral-900 dark:text-neutral-100 p-8">
      <div className="max-w-2xl mx-auto space-y-6">
        <header>
          <h1 className="text-2xl font-semibold">Voice Dictation</h1>
          <p className="text-sm text-neutral-500 mt-1">
            Privacy-first voice-to-text. Hold <kbd className="kbd">{settings.hotkey}</kbd> and speak.
          </p>
        </header>

        <section className="bg-white dark:bg-neutral-800 rounded-lg p-6 shadow-sm">
          <div className="flex items-center gap-3 mb-3">
            <span
              className={`inline-block w-3 h-3 rounded-full ${
                isRecording ? "bg-red-500 animate-pulse" : "bg-neutral-400"
              }`}
            />
            <span className="text-sm font-medium">
              {isRecording ? "Recording" : "Idle"}
            </span>
            <span className="text-xs text-neutral-500 ml-auto">{status}</span>
          </div>

          <button
            onClick={runTestRecord}
            disabled={testing}
            className="px-4 py-2 rounded-md bg-blue-600 hover:bg-blue-700 disabled:bg-neutral-400 text-white text-sm font-medium"
          >
            {testing ? "Recording 3s..." : "Test record (3s)"}
          </button>

          <div className="mt-4">
            <div className="text-xs uppercase text-neutral-500 mb-1">Last transcription</div>
            <div className="min-h-[3rem] p-3 rounded-md bg-neutral-100 dark:bg-neutral-900 text-sm whitespace-pre-wrap">
              {lastTranscription || <span className="text-neutral-400">—</span>}
            </div>
          </div>
        </section>

        <section className="space-y-5 bg-white dark:bg-neutral-800 rounded-lg p-6 shadow-sm">
          <h2 className="text-lg font-semibold">Settings</h2>

          <Field label="Transcription mode" hint="Cloud uses Groq Whisper. Local runs on your GPU. Auto tries cloud first and falls back to local on failure.">
            <select
              className="select"
              value={settings.mode}
              onChange={(e) => update("mode", e.target.value as Settings["mode"])}
            >
              <option value="cloud">Cloud (Groq)</option>
              <option value="local">Local (whisper.cpp + CUDA)</option>
              <option value="auto">Auto (cloud, fall back to local)</option>
            </select>
          </Field>

          <Field label="Cloud model" hint="turbo is faster and cheaper; large-v3 is slightly more accurate.">
            <select
              className="select"
              value={settings.cloudModel}
              onChange={(e) => update("cloudModel", e.target.value as Settings["cloudModel"])}
            >
              <option value="whisper-large-v3-turbo">whisper-large-v3-turbo (default)</option>
              <option value="whisper-large-v3">whisper-large-v3</option>
            </select>
          </Field>

          <Field label="Groq API key" hint="Get one at console.groq.com/keys. Stored locally only.">
            <input
              type="password"
              placeholder="gsk_..."
              className="input"
              value={settings.groqApiKey}
              onChange={(e) => update("groqApiKey", e.target.value)}
            />
          </Field>

          <Field label="Active local model" hint="Used for local and auto-fallback. Download below first.">
            <select
              className="select"
              value={settings.localModel}
              onChange={(e) => update("localModel", e.target.value)}
            >
              {models.map((m) => (
                <option key={m.name} value={m.name}>
                  {m.display} {m.installed ? "✓" : "(not downloaded)"}
                </option>
              ))}
            </select>
          </Field>

          <Field label="Language" hint="Auto handles Chinese + English code-switching. Lock if you only ever dictate in one.">
            <select
              className="select"
              value={settings.language}
              onChange={(e) => update("language", e.target.value as Settings["language"])}
            >
              <option value="auto">Auto (zh + en)</option>
              <option value="zh">Chinese only</option>
              <option value="en">English only</option>
            </select>
          </Field>

          <Field label="Hotkey" hint="Held to record. Restart app to apply changes.">
            <input
              type="text"
              className="input"
              value={settings.hotkey}
              onChange={(e) => update("hotkey", e.target.value)}
            />
          </Field>

          <Field label="Injection strategy" hint="Clipboard paste is reliable for Unicode and long text; direct keystroke avoids touching the clipboard. Long text always uses clipboard.">
            <select
              className="select"
              value={settings.injectStrategy}
              onChange={(e) => update("injectStrategy", e.target.value as Settings["injectStrategy"])}
            >
              <option value="clipboard">Clipboard + paste</option>
              <option value="keystroke">Direct keystroke</option>
            </select>
          </Field>

          <Field label="Start on login" hint="Launch Voice Dictation automatically (minimized to tray) when you sign in to Windows.">
            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={autostart}
                onChange={(e) => toggleAutostart(e.target.checked)}
              />
              <span>{autostart ? "Enabled" : "Disabled"}</span>
            </label>
          </Field>
        </section>

        <section className="space-y-3 bg-white dark:bg-neutral-800 rounded-lg p-6 shadow-sm">
          <div className="flex items-baseline justify-between">
            <h2 className="text-lg font-semibold">Local models</h2>
            <span className="text-xs text-neutral-500">stored in app data dir</span>
          </div>
          <div className="space-y-2">
            {models.map((m) => {
              const dl = downloads[m.name];
              const pct = dl && dl.total ? Math.round((dl.downloaded / dl.total) * 100) : null;
              return (
                <div key={m.name} className="border border-neutral-200 dark:border-neutral-700 rounded-md p-3">
                  <div className="flex items-center gap-2">
                    <div className="flex-1">
                      <div className="text-sm font-medium">
                        {m.display}
                        {m.recommended && (
                          <span className="ml-2 text-xs px-1.5 py-0.5 rounded bg-blue-100 dark:bg-blue-900 text-blue-700 dark:text-blue-300">
                            recommended
                          </span>
                        )}
                      </div>
                      <div className="text-xs text-neutral-500">
                        {m.installed
                          ? `installed — ${(m.sizeOnDisk / 1_048_576).toFixed(1)} MB on disk`
                          : `${m.sizeMb} MB — not downloaded`}
                      </div>
                    </div>
                    {dl ? (
                      <button
                        onClick={() => cancelDownload(m.name)}
                        className="px-3 py-1 text-xs rounded bg-neutral-200 dark:bg-neutral-700 hover:bg-neutral-300"
                      >
                        Cancel
                      </button>
                    ) : m.installed ? (
                      <button
                        onClick={() => deleteModel(m.name)}
                        className="px-3 py-1 text-xs rounded bg-red-600 hover:bg-red-700 text-white"
                      >
                        Delete
                      </button>
                    ) : (
                      <button
                        onClick={() => startDownload(m.name)}
                        className="px-3 py-1 text-xs rounded bg-blue-600 hover:bg-blue-700 text-white"
                      >
                        Download
                      </button>
                    )}
                  </div>
                  {dl && (
                    <div className="mt-2">
                      <div className="h-1.5 rounded bg-neutral-200 dark:bg-neutral-700 overflow-hidden">
                        <div
                          className="h-full bg-blue-600 transition-all"
                          style={{ width: pct !== null ? `${pct}%` : "50%" }}
                        />
                      </div>
                      <div className="text-xs text-neutral-500 mt-1">
                        {pct !== null
                          ? `${pct}% — ${(dl.downloaded / 1_048_576).toFixed(1)} / ${((dl.total ?? 0) / 1_048_576).toFixed(1)} MB`
                          : `${(dl.downloaded / 1_048_576).toFixed(1)} MB downloaded`}
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
            {models.length === 0 && (
              <div className="text-sm text-neutral-500">Loading model list...</div>
            )}
          </div>
        </section>

        <section className="space-y-3 bg-white dark:bg-neutral-800 rounded-lg p-6 shadow-sm">
          <div className="flex items-baseline justify-between">
            <h2 className="text-lg font-semibold">Dictionary</h2>
            <span className="text-xs text-neutral-500">applied to every transcription</span>
          </div>
          <p className="text-xs text-neutral-500">
            Find &amp; replace pairs for words Whisper consistently mis-hears (tech jargon, proper nouns, abbreviations).
            Examples: <code>react js</code> → <code>React.js</code>, <code>type script</code> → <code>TypeScript</code>.
          </p>
          <div className="space-y-2">
            {settings.dictionary.map((rule, i) => (
              <div
                key={i}
                className="flex items-center gap-2 border border-neutral-200 dark:border-neutral-700 rounded-md p-2"
              >
                <input
                  type="checkbox"
                  checked={rule.enabled}
                  onChange={(e) => updateRule(i, { enabled: e.target.checked })}
                  title="Enabled"
                  className="shrink-0"
                />
                <input
                  type="text"
                  value={rule.from}
                  placeholder="heard as..."
                  onChange={(e) => updateRule(i, { from: e.target.value })}
                  className="flex-1 px-2 py-1 text-sm rounded border border-neutral-300 dark:border-neutral-600 bg-white dark:bg-neutral-900"
                />
                <span className="text-neutral-400 text-sm">→</span>
                <input
                  type="text"
                  value={rule.to}
                  placeholder="replace with..."
                  onChange={(e) => updateRule(i, { to: e.target.value })}
                  className="flex-1 px-2 py-1 text-sm rounded border border-neutral-300 dark:border-neutral-600 bg-white dark:bg-neutral-900"
                />
                <label className="flex items-center gap-1 text-xs text-neutral-600 dark:text-neutral-400 shrink-0">
                  <input
                    type="checkbox"
                    checked={rule.caseInsensitive}
                    onChange={(e) => updateRule(i, { caseInsensitive: e.target.checked })}
                  />
                  CI
                </label>
                <button
                  onClick={() => removeRule(i)}
                  className="px-2 py-1 text-xs rounded bg-red-600 hover:bg-red-700 text-white shrink-0"
                  title="Delete rule"
                >
                  ✕
                </button>
              </div>
            ))}
            {settings.dictionary.length === 0 && (
              <div className="text-sm text-neutral-500">No rules yet. Add one below.</div>
            )}
          </div>
          <button
            onClick={addRule}
            className="px-3 py-1 text-xs rounded bg-blue-600 hover:bg-blue-700 text-white"
          >
            + Add rule
          </button>
        </section>

        <section className="space-y-3 bg-white dark:bg-neutral-800 rounded-lg p-6 shadow-sm">
          <div className="flex items-baseline justify-between">
            <h2 className="text-lg font-semibold">History</h2>
            <span className="text-xs text-neutral-500">
              {historyTotal} entr{historyTotal === 1 ? "y" : "ies"}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <input
              type="text"
              value={historyQuery}
              placeholder="Search transcriptions..."
              onChange={(e) => {
                setHistoryQuery(e.target.value);
                setHistoryOffset(0);
              }}
              className="flex-1 px-3 py-1.5 text-sm rounded border border-neutral-300 dark:border-neutral-600 bg-white dark:bg-neutral-900"
            />
            <button
              onClick={clearAllHistory}
              disabled={historyTotal === 0}
              className="px-3 py-1.5 text-xs rounded bg-red-600 hover:bg-red-700 text-white disabled:opacity-40"
            >
              Clear all
            </button>
          </div>
          <div className="space-y-2">
            {history.map((h) => (
              <div
                key={h.id}
                className="border border-neutral-200 dark:border-neutral-700 rounded-md p-3"
              >
                <div className="flex items-center gap-2 text-xs text-neutral-500 mb-1">
                  <span>{formatTime(h.createdAt)}</span>
                  <span>·</span>
                  <span>
                    {h.mode} · {h.model}
                  </span>
                  {h.language && (
                    <>
                      <span>·</span>
                      <span>{h.language}</span>
                    </>
                  )}
                  <span>·</span>
                  <span>{h.durationSecs.toFixed(1)}s</span>
                  <div className="ml-auto flex items-center gap-1">
                    <button
                      onClick={() => copyToClipboard(h.text)}
                      className="px-2 py-0.5 rounded bg-neutral-200 dark:bg-neutral-700 hover:bg-neutral-300"
                    >
                      Copy
                    </button>
                    <button
                      onClick={() => deleteHistoryEntry(h.id)}
                      className="px-2 py-0.5 rounded bg-red-600 hover:bg-red-700 text-white"
                    >
                      ✕
                    </button>
                  </div>
                </div>
                <div className="text-sm whitespace-pre-wrap break-words">{h.text}</div>
              </div>
            ))}
            {history.length === 0 && (
              <div className="text-sm text-neutral-500">
                {historyQuery ? "No matches." : "No transcriptions yet."}
              </div>
            )}
          </div>
          {historyTotal > PAGE_SIZE && (
            <div className="flex items-center justify-between text-xs">
              <button
                onClick={() => setHistoryOffset(Math.max(0, historyOffset - PAGE_SIZE))}
                disabled={historyOffset === 0}
                className="px-3 py-1 rounded bg-neutral-200 dark:bg-neutral-700 hover:bg-neutral-300 disabled:opacity-40"
              >
                ← Prev
              </button>
              <span className="text-neutral-500">
                {historyOffset + 1}–{Math.min(historyOffset + PAGE_SIZE, historyTotal)} of {historyTotal}
              </span>
              <button
                onClick={() => setHistoryOffset(historyOffset + PAGE_SIZE)}
                disabled={historyOffset + PAGE_SIZE >= historyTotal}
                className="px-3 py-1 rounded bg-neutral-200 dark:bg-neutral-700 hover:bg-neutral-300 disabled:opacity-40"
              >
                Next →
              </button>
            </div>
          )}
        </section>

        <footer className="text-xs text-neutral-500">
          {savedAt ? `Settings saved at ${savedAt}` : "Changes save automatically."}
        </footer>
      </div>
    </main>
  );
}

function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <div className="text-sm font-medium mb-1">{label}</div>
      {children}
      {hint && <div className="text-xs text-neutral-500 mt-1">{hint}</div>}
    </label>
  );
}

export default App;
