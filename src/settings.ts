import { invoke, isTauri } from "@tauri-apps/api/core";

export interface Settings {
  proxy: string;
  autoTranslate: boolean;
  autoSpeak: boolean;
  langPair: string;
}

export const DEFAULTS: Settings = {
  proxy: "", autoTranslate: true, autoSpeak: false, langPair: "auto|zh-CN",
};

let pendingSave: Promise<void> = Promise.resolve();

export async function loadSettings(): Promise<Settings> {
  if (isTauri()) return invoke<Settings>("load_settings");
  try {
    const raw = JSON.parse(localStorage.getItem("translater.settings") ?? "{}");
    return { ...DEFAULTS, ...raw };
  } catch { return { ...DEFAULTS }; }
}

export function saveSettings(settings: Settings): Promise<void> {
  const snapshot = { ...settings };
  const task = pendingSave.catch(() => {}).then(async () => {
    if (isTauri()) await invoke("save_settings", { settings: snapshot });
    else localStorage.setItem("translater.settings", JSON.stringify(snapshot));
  });
  pendingSave = task;
  return task;
}
