import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defineConfig } from "vite";

// Tauri loads the dev server at this port (src-tauri/tauri.conf.json `devUrl`).
const PORT = 4323;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Tauri prints its own errors; keep Vite's output from clearing them.
  clearScreen: false,
  resolve: {
    // One copy of React, the TanStack context libraries, the toaster, the
    // design system, the host seam and the service clients for the app and
    // every UI package: hooks, context and the clients' registries break
    // across copies. The lock should already hold one of each; this guards it.
    dedupe: [
      "react",
      "react-dom",
      "@tanstack/react-query",
      "@tanstack/react-router",
      "@open-lakehouse/ui-kit",
      "@open-lakehouse/host",
      "@open-lakehouse/unity-catalog-client",
      "@open-lakehouse/lineage-client",
      "@open-lakehouse/editor",
      "monaco-editor",
      "@xyflow/react",
      "@connectrpc/connect",
      "@connectrpc/connect-web",
      "sonner",
    ],
  },
  server: {
    port: PORT,
    strictPort: true,
    // The repository: the Bun workspace root, whose store holds the packages.
    fs: { allow: [path.resolve(__dirname, "../..")] },
  },
  preview: { port: PORT, strictPort: true },
  build: {
    // The webviews Tauri 2 supports: WKWebView (macOS 11+) and WebKitGTK 2.40+.
    target: "safari15",
    sourcemap: false,
    // One app chunk of ~2.3 MB (env-editor with ELK and Shiki), loaded from disk.
    chunkSizeWarningLimit: 3000,
  },
});
