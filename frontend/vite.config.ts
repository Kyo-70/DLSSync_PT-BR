import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

const host = process.env.TAURI_DEV_HOST;
const webviewTarget = "chrome105";

export default defineConfig(({ mode }) => ({
  plugins: [svelte()],
  resolve: {
    alias: mode === "nexus" || process.env.VITE_DLSSYNC_DISTRIBUTION === "nexus"
      ? [{ find: "@tauri-apps/plugin-updater", replacement: fileURLToPath(new URL("./src/lib/noAppUpdater.ts", import.meta.url)) }]
      : [],
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: webviewTarget,
    chunkSizeWarningLimit: 700,
    minify: !process.env.TAURI_DEBUG ? "oxc" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    rolldownOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return;
          if (id.includes("@tauri-apps")) return "tauri";
          if (id.includes("svelte")) return "svelte";
          if (id.includes("@lucide") || id.includes("simple-icons")) return "icons";
          if (id.includes("@fontsource")) return "fonts";
          return "vendor";
        },
      },
    },
  },
}));
