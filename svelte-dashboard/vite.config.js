import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  base: "/",
  build: {
    outDir: "dist",
  },
  // Dev only: the dashboard calls same-origin `/api/*`, so point it at a local
  // `codasaurus serve`. Production builds are served by that binary directly.
  server: {
    proxy: {
      "/api": {
        target: process.env.CODASAURUS_API ?? "http://127.0.0.1:3000",
        changeOrigin: true,
      },
    },
  },
});
