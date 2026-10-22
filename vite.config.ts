import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath, URL } from "node:url";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte()],
  // Keep Vite's output from hiding Rust compiler errors in `tauri dev`.
  clearScreen: false,
  build: {
    // Both windows are built: the overlay (index.html) and the settings window (settings.html).
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        settings: fileURLToPath(new URL("./settings.html", import.meta.url)),
      },
    },
  },
  server: {
    // Tauri's devUrl is fixed, so fail loudly rather than drift to another port.
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
