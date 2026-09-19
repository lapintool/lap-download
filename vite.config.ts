import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import process from "node:process";

const host = process.env.TAURI_DEV_HOST;
const port = Number(process.env.LAPDW_DEV_PORT || 3415);

export default defineConfig(() => ({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port,
    // Port is pre-selected by scripts/dev.mjs; fail loudly if stolen mid-start.
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: port + 1,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
