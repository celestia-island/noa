import vue from "@vitejs/plugin-vue";
import vueJsx from "@vitejs/plugin-vue-jsx";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vitest/config";

// Mirrors vite.config.ts (alias + vue/jsx transforms); the roster smoke
// rides happy-dom (localStorage, real DOM mounting) — the same environment
// the family's webui suites use.
export default defineConfig({
  plugins: [vue(), vueJsx()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  test: {
    environment: "happy-dom",
  },
});
