import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Fixed port expected by src-tauri/tauri.conf.json (devUrl).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    sourcemap: false,
  },
  test: {
    environment: "jsdom",
  },
});
