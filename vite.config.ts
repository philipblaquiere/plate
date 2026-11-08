import { defineConfig } from "vite";

// The web app lives in web/; the Rust API runs on :7878 and is proxied under /api.
// PLATE_API points it at another API (scripts/shot.mjs runs its own on another port).
export default defineConfig({
  root: "web",
  server: {
    port: 5173,
    strictPort: true,
    proxy: { "/api": process.env.PLATE_API ?? "http://127.0.0.1:7878" },
    fs: { allow: [".."] },
  },
  worker: { format: "es" },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    chunkSizeWarningLimit: 4000,
  },
});
