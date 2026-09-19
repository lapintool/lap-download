<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import TitleBar from "./components/TitleBar.vue";
import { listLocales, loadLocale, localeId, t, type LocaleInfo } from "./i18n";
import logo from "./assets/logo.png";

const SAVE_ICON = `<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M5 4.5h11.2L19.5 8v11.5H5z"/><path d="M8 4.5v5.2h8V4.5"/><path d="M8 19.5v-5.4h8v5.4"/></svg>`;
const FOLDER_ICON = `<svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M3.8 7.2h6.1l1.7 1.8H20.2v9.2H3.8z"/><path d="M3.8 7.2V5.8h5.2l1.2 1.4"/></svg>`;

type TaskStatus =
  | "queued"
  | "running"
  | "completed"
  | "skipped"
  | "failed"
  | "cancelled";

interface TaskRecord {
  id: string;
  url: string;
  savePath: string;
  sha256: string;
  status: TaskStatus;
  downloaded: number;
  total: number;
  speedBps: number;
  message: string;
  error: string;
}

interface Settings {
  theme: string;
  locale: string;
  zoom: number;
  hfToken: string;
  token: string;
  cookie: string;
  useProxy: boolean;
  proxy: string;
  threads: number;
  saveDir: string;
}

const urlText = ref("");
const tasks = ref<TaskRecord[]>([]);
const settings = reactive<Settings>({
  theme: "dark",
  locale: "zh",
  zoom: 1,
  hfToken: "",
  token: "",
  cookie: "",
  useProxy: false,
  proxy: "http://127.0.0.1:6990",
  threads: 16,
  saveDir: "",
});
const saveDirLabel = ref("");
const busy = ref(false);
const error = ref("");
const settingsDirty = ref(false);
const locales = ref<LocaleInfo[]>([]);

let unlistenUpdated: UnlistenFn | undefined;
let unlistenMoved: UnlistenFn | undefined;
let unlistenResized: UnlistenFn | undefined;
let saveTimer: number | undefined;

const runningCount = computed(
  () => tasks.value.filter((t) => t.status === "running" || t.status === "queued").length,
);

function applyTheme(theme: string) {
  const id = theme === "light" ? "light" : "dark";
  document.documentElement.dataset.theme = id;
  try {
    localStorage.setItem("lapdw-theme", id);
  } catch {
    /* ignore */
  }
}

function formatBytes(n: number): string {
  if (!n || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

function formatSpeed(bps: number): string {
  if (!bps) return "—";
  return `${formatBytes(bps)}/s`;
}

function percent(task: TaskRecord): number {
  if (!task.total) return task.status === "completed" || task.status === "skipped" ? 100 : 0;
  return Math.min(100, Math.round((task.downloaded / task.total) * 1000) / 10);
}

function progressStyle(task: TaskRecord): Record<string, string> {
  return { "--ls-progress": `${percent(task)}%` };
}

function statusLabel(status: TaskStatus): string {
  return t(`status.${status}`);
}

async function persistPatch(patch: Partial<Settings>) {
  const s = await invoke<Settings>("update_settings", { patch });
  Object.assign(settings, s);
  return s;
}

async function refreshTasks() {
  tasks.value = await invoke<TaskRecord[]>("list_tasks");
}

async function loadSettings() {
  const s = await invoke<Settings>("get_settings");
  Object.assign(settings, s);
  applyTheme(s.theme);
  locales.value = await listLocales();
  await loadLocale(s.locale || "zh");
  settings.locale = localeId();
  saveDirLabel.value = await invoke<string>("resolve_save_dir");
}

async function saveSettings() {
  busy.value = true;
  error.value = "";
  try {
    const s = await persistPatch({
      hfToken: settings.hfToken,
      token: settings.token,
      cookie: settings.cookie,
      useProxy: settings.useProxy,
      proxy: settings.proxy,
      threads: Number(settings.threads) || 16,
      saveDir: settings.saveDir,
    });
    applyTheme(s.theme);
    saveDirLabel.value = await invoke<string>("resolve_save_dir");
    settingsDirty.value = false;
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

function markDirty() {
  settingsDirty.value = true;
}

async function toggleTheme() {
  const next = settings.theme === "light" ? "dark" : "light";
  settings.theme = next;
  applyTheme(next);
  try {
    await persistPatch({ theme: next });
  } catch (e) {
    error.value = String(e);
  }
}

async function cycleLocale() {
  const list = locales.value.length ? locales.value : await listLocales();
  locales.value = list;
  const ids = list.map((item) => item.id);
  const current = localeId();
  const idx = ids.indexOf(current);
  const next = ids.length ? ids[(idx + 1) % ids.length] : current === "zh" ? "en" : "zh";
  await loadLocale(next);
  settings.locale = localeId();
  try {
    await persistPatch({ locale: settings.locale });
  } catch (e) {
    error.value = String(e);
  }
}

async function pickDir() {
  const dir = await invoke<string | null>("pick_save_dir", { title: t("settings.pickDir") });
  if (dir) {
    settings.saveDir = dir;
    settingsDirty.value = true;
  }
}

async function addAndStart() {
  error.value = "";
  const text = urlText.value.trim();
  if (!text) {
    error.value = t("error.needUrl");
    return;
  }
  if (settingsDirty.value) {
    await saveSettings();
  }
  busy.value = true;
  try {
    await invoke("add_urls", { urls: [text] });
    urlText.value = "";
    await invoke("start_downloads");
    await refreshTasks();
  } catch (e) {
    error.value = String(e);
  } finally {
    busy.value = false;
  }
}

async function startAll() {
  if (settingsDirty.value) {
    await saveSettings();
  }
  await invoke("start_downloads");
}

async function cancelTask(id: string) {
  await invoke("cancel_task", { id });
}

async function removeTask(id: string) {
  await invoke("remove_task", { id });
}

async function clearFinished() {
  await invoke("clear_finished");
}

async function openSaveDir() {
  const dir = await invoke<string>("resolve_save_dir");
  await invoke("open_path", { path: dir });
}

async function openTaskPath(path: string) {
  if (!path) return;
  await invoke("open_path", { path });
}

function scheduleSaveWindow() {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    void invoke("save_window_state_cmd");
  }, 1000);
}

onMounted(async () => {
  await loadSettings();
  await refreshTasks();
  unlistenUpdated = await listen<TaskRecord[]>("download-updated", (ev) => {
    tasks.value = ev.payload;
  });
  const win = getCurrentWindow();
  unlistenMoved = await win.onMoved(() => scheduleSaveWindow());
  unlistenResized = await win.onResized(() => scheduleSaveWindow());
});

onUnmounted(() => {
  unlistenUpdated?.();
  unlistenMoved?.();
  unlistenResized?.();
  window.clearTimeout(saveTimer);
});
</script>

<template>
  <div class="app-shell">
    <TitleBar :theme="settings.theme" :locale-id="settings.locale" @toggle-theme="toggleTheme" @cycle-locale="cycleLocale" />

    <div class="main">
      <section class="workspace">
        <div class="compose">
          <textarea
            class="ls-input"
            v-model="urlText"
            :placeholder="t('compose.placeholder')"
            @keydown.ctrl.enter.prevent="addAndStart"
          />
          <div class="compose-actions">
            <button class="ls-btn green no-caps" :disabled="busy" @click="addAndStart">
              {{ t("action.addStart") }}
            </button>
            <button class="ls-btn cyan no-caps" :disabled="busy" @click="startAll">
              {{ t("action.startQueue") }}
            </button>
            <button class="ls-btn red no-caps" @click="clearFinished">{{ t("action.clearDone") }}</button>
          </div>
        </div>

        <div class="task-list ls-scroll">
          <p v-if="error" class="err">{{ error }}</p>
          <div v-if="tasks.length === 0" class="empty">
            <img class="empty-logo" :src="logo" alt="" />
            <p class="empty-caption">{{ t("empty.tasks") }}</p>
          </div>
          <div v-for="task in tasks" :key="task.id" class="task-row">
            <div class="task-main">
              <div class="task-url" :title="task.url">{{ task.url }}</div>
              <div class="task-meta">
                <span>{{ statusLabel(task.status) }}</span>
                <span v-if="task.total">
                  {{ formatBytes(task.downloaded) }} / {{ formatBytes(task.total) }}
                  ({{ percent(task) }}%)
                </span>
                <span v-else-if="task.downloaded">{{ formatBytes(task.downloaded) }}</span>
                <span>{{ formatSpeed(task.speedBps) }}</span>
                <span v-if="task.message">{{ task.message }}</span>
                <span v-if="task.error" class="err">{{ task.error }}</span>
              </div>
              <div
                class="ls-progress blue rounded"
                role="progressbar"
                :aria-valuenow="percent(task)"
                aria-valuemin="0"
                aria-valuemax="100"
                :style="progressStyle(task)"
              />
            </div>
            <div class="task-actions">
              <button
                v-if="task.status === 'running' || task.status === 'queued'"
                class="ls-btn dense no-caps"
                @click="cancelTask(task.id)"
              >
                {{ t("action.cancel") }}
              </button>
              <button
                v-if="task.savePath"
                class="ls-btn dense no-caps"
                @click="openTaskPath(task.savePath)"
              >
                {{ t("action.open") }}
              </button>
              <button class="ls-btn dense no-caps" @click="removeTask(task.id)">
                {{ t("action.remove") }}
              </button>
            </div>
          </div>
        </div>
      </section>

      <aside class="side ls-scroll">
        <div class="side-head">
          <h2>{{ t("settings.title") }}</h2>
          <button
            class="ls-btn blue no-caps"
            :disabled="busy || !settingsDirty"
            @click="saveSettings"
          >
            <span class="ls-icon" v-html="SAVE_ICON" />
            {{ t("settings.save") }}
          </button>
        </div>
        <p class="hint">{{ t("settings.hint") }}</p>

        <div class="field-stack">
          <label class="ls-field">
            <span class="label">{{ t("settings.hfToken") }}</span>
            <input
              class="ls-input"
              type="password"
              v-model="settings.hfToken"
              placeholder="hf_..."
              @input="markDirty"
            />
          </label>

          <label class="ls-field">
            <span class="label">{{ t("settings.token") }}</span>
            <input
              class="ls-input"
              type="password"
              v-model="settings.token"
              placeholder="Bearer token"
              @input="markDirty"
            />
          </label>

          <label class="ls-field">
            <span class="label">{{ t("settings.cookie") }}</span>
            <input
              class="ls-input"
              type="text"
              v-model="settings.cookie"
              placeholder="name=value; ..."
              @input="markDirty"
            />
          </label>

          <label class="ls-field">
            <span class="label">{{ t("settings.threads") }}</span>
            <input
              class="ls-input"
              type="number"
              min="1"
              max="64"
              v-model.number="settings.threads"
              @input="markDirty"
            />
          </label>

          <label class="ls-checkbox switch">
            <input type="checkbox" v-model="settings.useProxy" @change="markDirty" />
            {{ t("settings.useProxy") }}
          </label>

          <label class="ls-field">
            <span class="label">{{ t("settings.proxy") }}</span>
            <input
              class="ls-input"
              type="text"
              v-model="settings.proxy"
              :disabled="!settings.useProxy"
              @input="markDirty"
            />
          </label>

          <label class="ls-field">
            <span class="label">{{ t("settings.saveDir") }}</span>
            <div class="dir-row">
              <input
                class="ls-input"
                type="text"
                v-model="settings.saveDir"
                :placeholder="saveDirLabel || t('settings.saveDirPlaceholder')"
                @input="markDirty"
              />
              <button
                class="ls-btn icon flat"
                type="button"
                :title="t('settings.browse')"
                :aria-label="t('settings.browse')"
                @click="pickDir"
              >
                <span class="ls-icon" v-html="FOLDER_ICON" />
              </button>
            </div>
          </label>

          <button class="ls-btn no-caps" @click="openSaveDir">{{ t("settings.openDir") }}</button>
        </div>
      </aside>
    </div>

    <footer class="status-bar">
      <span>{{ t("status.queue", { n: runningCount }) }}</span>
      <span>{{ t("status.threads", { n: settings.threads }) }}</span>
      <span>{{ settings.useProxy ? settings.proxy : t("status.proxyOff") }}</span>
      <span class="grow" />
      <span :title="saveDirLabel">{{ saveDirLabel }}</span>
    </footer>
  </div>
</template>
