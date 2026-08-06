import { fileURLToPath, URL } from "node:url";

import vue from "@vitejs/plugin-vue";
import vueJsx from "@vitejs/plugin-vue-jsx";
import { defineConfig } from "vite";

// Build-only config (no dev server / HMR — the family serves built assets via
// the repo's own tooling). Mirrors the shittim-chest / arona webui setup.
export default defineConfig({
  plugins: [vue(), vueJsx()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  define: {
    __APP_VERSION__: JSON.stringify("0.1.0"),
  },
  build: {
    outDir: "../../dist/webui",
    emptyOutDir: false,
  },
});
