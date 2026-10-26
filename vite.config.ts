import { defineConfig } from "vite";

// The web app lives in web/; the Rust API runs on :7878 and is proxied under /api.
export default defineConfig({
  root: "web",
  server: {
    port: 5173,
    strictPort: true,
    proxy: { "/api": "http://127.0.0.1:7878" },
    fs: { allow: [".."] },
  },
  worker: { format: "es" },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    chunkSizeWarningLimit: 4000,
  },
});
