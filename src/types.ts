export type TranscribeMode = "local" | "cloud" | "auto";

export type CloudModel = "whisper-large-v3" | "whisper-large-v3-turbo";

export type InjectStrategy = "clipboard" | "keystroke";

export interface DictRule {
  from: string;
  to: string;
  caseInsensitive: boolean;
  enabled: boolean;
}

export interface Settings {
  mode: TranscribeMode;
  cloudModel: CloudModel;
  localModel: string;
  groqApiKey: string;
  hotkey: string;
  injectStrategy: InjectStrategy;
  language: "auto" | "zh" | "en";
  dictionary: DictRule[];
}

export const DEFAULT_SETTINGS: Settings = {
  mode: "cloud",
  cloudModel: "whisper-large-v3-turbo",
  localModel: "large-v3-turbo-q5_0",
  groqApiKey: "",
  hotkey: "Alt+Space",
  injectStrategy: "clipboard",
  language: "auto",
  dictionary: [],
};

export interface LocalModelStatus {
  name: string;
  display: string;
  filename: string;
  sizeMb: number;
  recommended: boolean;
  installed: boolean;
  sizeOnDisk: number;
}

export type DownloadEvent =
  | { kind: "started"; name: string; total: number | null }
  | { kind: "progress"; name: string; downloaded: number; total: number | null }
  | { kind: "done"; name: string }
  | { kind: "failed"; name: string; error: string }
  | { kind: "canceled"; name: string };

export interface HistoryEntry {
  id: number;
  createdAt: number;
  mode: string;
  model: string;
  language: string | null;
  durationSecs: number;
  text: string;
}

export interface HistoryPage {
  total: number;
  entries: HistoryEntry[];
}
