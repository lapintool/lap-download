import { reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";

export type LocaleInfo = {
  id: string;
  name: string;
};

export type LocaleFile = {
  id: string;
  name: string;
  strings: Record<string, string>;
};

const current = reactive<LocaleFile>({
  id: "zh",
  name: "中文",
  strings: {},
});

export function localeId(): string {
  return current.id;
}

export function t(key: string, vars?: Record<string, string | number>): string {
  void current.id;
  let text = current.strings[key] ?? key;
  if (vars) {
    for (const [name, value] of Object.entries(vars)) {
      text = text.replace(new RegExp(`\\{${name}\\}`, "g"), String(value));
    }
  }
  return text;
}

function applyDocumentLang(id: string) {
  document.documentElement.lang = id === "zh" ? "zh-CN" : id;
  try {
    localStorage.setItem("lapdw-locale", id);
  } catch {
    /* ignore */
  }
}

export async function loadLocale(id: string): Promise<LocaleFile> {
  const file = await invoke<LocaleFile>("load_ui_locale", { id });
  current.id = file.id;
  current.name = file.name;
  current.strings = file.strings;
  applyDocumentLang(file.id);
  return file;
}

export async function listLocales(): Promise<LocaleInfo[]> {
  return invoke<LocaleInfo[]>("list_ui_locales");
}
