<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { t } from "../i18n";

defineProps<{
  theme: string;
  localeId: string;
}>();

const emit = defineEmits<{
  toggleTheme: [];
  cycleLocale: [];
}>();

const custom = ref(false);
const maximized = ref(false);

const SUN_ICON = `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="currentColor">
  <circle cx="12" cy="12" r="4.25"/>
  <rect x="11" y="1.6" width="2" height="3.8" rx="1" transform="rotate(0 12 12)"/>
  <rect x="11" y="1.6" width="2" height="3.8" rx="1" transform="rotate(45 12 12)"/>
  <rect x="11" y="1.6" width="2" height="3.8" rx="1" transform="rotate(90 12 12)"/>
  <rect x="11" y="1.6" width="2" height="3.8" rx="1" transform="rotate(135 12 12)"/>
  <rect x="11" y="1.6" width="2" height="3.8" rx="1" transform="rotate(180 12 12)"/>
  <rect x="11" y="1.6" width="2" height="3.8" rx="1" transform="rotate(225 12 12)"/>
  <rect x="11" y="1.6" width="2" height="3.8" rx="1" transform="rotate(270 12 12)"/>
  <rect x="11" y="1.6" width="2" height="3.8" rx="1" transform="rotate(315 12 12)"/>
</svg>`;

const LANG_ICON = `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round">
  <circle cx="12" cy="12" r="9"/>
  <path d="M3 12h18"/>
  <path d="M12 3c2.6 3.2 3.8 6.2 3.8 9s-1.2 5.8-3.8 9c-2.6-3.2-3.8-6.2-3.8-9s1.2-5.8 3.8-9z"/>
</svg>`;

const MIN_ICON = `<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1" stroke-linecap="square"><path d="M2.5 6h7"/></svg>`;
const MAX_ICON = `<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1"><rect x="2.5" y="2.5" width="7" height="7"/></svg>`;
const RESTORE_ICON = `<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1"><path d="M4 3.5h4.5V8"/><rect x="2.5" y="4.5" width="5.5" height="5.5"/></svg>`;
const CLOSE_ICON = `<svg viewBox="0 0 12 12" width="12" height="12" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1" stroke-linecap="square"><path d="M3 3l6 6M9 3L3 9"/></svg>`;

async function syncMax() {
  maximized.value = await getCurrentWindow().isMaximized();
}

onMounted(async () => {
  custom.value = await invoke<boolean>("uses_custom_titlebar");
  if (!custom.value) return;
  document.documentElement.dataset.titlebar = "custom";
  const win = getCurrentWindow();
  await syncMax();
  await win.onResized(() => {
    void syncMax();
  });
});

function minimize() {
  void getCurrentWindow().minimize();
}
function toggleMax() {
  void getCurrentWindow().toggleMaximize();
}
function close() {
  void getCurrentWindow().close();
}
</script>

<template>
  <header class="titlebar" data-tauri-drag-region>
    <div class="brand">
      <img class="brand-logo" src="../assets/logo.png" alt="" />
      <span>lapdw</span>
    </div>
    <div class="spacer" />
    <div class="titlebar-actions no-drag">
      <button
        type="button"
        class="ls-btn icon flat round"
        :title="t('theme.toggle')"
        :aria-label="t('theme.toggle')"
        :aria-pressed="theme === 'light'"
        @click="emit('toggleTheme')"
        v-html="SUN_ICON"
      ></button>
      <button
        type="button"
        class="ls-btn icon flat round"
        :title="t('locale.title')"
        :aria-label="t('locale.title')"
        @click="emit('cycleLocale')"
        v-html="LANG_ICON"
      ></button>
    </div>
    <div v-if="custom" class="window-controls no-drag">
      <button
        type="button"
        :aria-label="t('window.minimize')"
        :title="t('window.minimize')"
        @click="minimize"
        v-html="MIN_ICON"
      />
      <button
        type="button"
        :aria-label="maximized ? t('window.restore') : t('window.maximize')"
        :title="maximized ? t('window.restore') : t('window.maximize')"
        @click="toggleMax"
        v-html="maximized ? RESTORE_ICON : MAX_ICON"
      />
      <button
        type="button"
        class="close"
        :aria-label="t('window.close')"
        :title="t('window.close')"
        @click="close"
        v-html="CLOSE_ICON"
      />
    </div>
  </header>
</template>
