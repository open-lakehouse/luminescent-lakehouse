import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defaultClientConditions, defineConfig } from "vite";
import { linkSiblings, siblingAliases, siblingRoots } from "./sibling-uis";

// Tauri loads the dev server at this port (src-tauri/tauri.conf.json `devUrl`).
const PORT = 4323;

// The service UIs build from sibling checkouts for now (sibling-uis.ts).
linkSiblings(__dirname);

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Tauri prints its own errors; keep Vite's output from clearing them.
  clearScreen: false,
  resolve: {
    // mangrove's and headwaters' UI packages are consumed from source: the
    // direct imports by alias, their own workspace packages by the `source`
    // export condition.
    conditions: ["source", ...defaultClientConditions],
    alias: siblingAliases(__dirname),
    // One copy of React, the TanStack context libraries, the toaster and the
    // design system for the app and every UI package (hooks and context break
    // across copies). The sibling packages resolve these from here.
    dedupe: [
      "react",
      "react-dom",
      "@tanstack/react-query",
      "@tanstack/react-router",
      "@open-lakehouse/ui-kit",
      "@xyflow/react",
      "@connectrpc/connect",
      "@connectrpc/connect-web",
      "sonner",
    ],
  },
  server: {
    port: PORT,
    strictPort: true,
    // The repository (the Bun workspace root) and the sibling checkouts, which
    // Vite reaches through their real paths.
    fs: { allow: [path.resolve(__dirname, "../.."), ...siblingRoots(__dirname)] },
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
