import { useEffect, useRef, useState } from "react";
import { load, Store } from "@tauri-apps/plugin-store";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { DEFAULT_SETTINGS, Settings } from "./types";
import "./App.css";

const STORE_FILE = "settings.json";

function App() {
  const [store, setStore] = useState<Store | null>(null);
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [savedAt, setSavedAt] = useState<string>("");

  const [isRecording, setIsRecording] = useState(false);
  const [status, setStatus] = useState<string>("idle");
  const [lastTranscription, setLastTranscription] = useState<string>("");
  const [testing, setTesting] = useState(false);

  const unlistens = useRef<UnlistenFn[]>([]);

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
        }),
      );
    })();

    return () => {
      unlistens.current.forEach((un) => un());
      unlistens.current = [];
    };
  }, []);

  async function update<K extends keyof Settings>(key: K, value: Settings[K]) {
    const next = { ...settings, [key]: value };
    setSettings(next);
    if (store) {
      await store.set("settings", next);
      await store.save();
      setSavedAt(new Date().toLocaleTimeString());
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

          <Field label="Transcription mode" hint="Cloud uses Groq Whisper. Local will use whisper.cpp once Stage 4 lands.">
            <select
              className="select"
              value={settings.mode}
              onChange={(e) => update("mode", e.target.value as Settings["mode"])}
            >
              <option value="cloud">Cloud (Groq)</option>
              <option value="local" disabled>Local — coming in Stage 4</option>
              <option value="auto" disabled>Auto — coming in Stage 4</option>
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

          <Field label="Injection strategy" hint="Wired in Stage 3.">
            <select
              className="select"
              value={settings.injectStrategy}
              onChange={(e) => update("injectStrategy", e.target.value as Settings["injectStrategy"])}
            >
              <option value="clipboard">Clipboard + paste</option>
              <option value="keystroke">Direct keystroke</option>
            </select>
          </Field>
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
