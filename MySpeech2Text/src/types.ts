export type TranscribeMode = "local" | "cloud" | "auto";

export type CloudModel = "whisper-large-v3" | "whisper-large-v3-turbo";

export type InjectStrategy = "clipboard" | "keystroke";

export interface Settings {
  mode: TranscribeMode;
  cloudModel: CloudModel;
  groqApiKey: string;
  hotkey: string;
  injectStrategy: InjectStrategy;
  language: "auto" | "zh" | "en";
}

export const DEFAULT_SETTINGS: Settings = {
  mode: "cloud",
  cloudModel: "whisper-large-v3-turbo",
  groqApiKey: "",
  hotkey: "Alt+Space",
  injectStrategy: "clipboard",
  language: "auto",
};
