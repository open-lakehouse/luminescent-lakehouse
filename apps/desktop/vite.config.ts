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
    // One React for the app and the @open-lakehouse packages (hooks and context
    // break across copies).
    dedupe: ["react", "react-dom"],
  },
  server: { port: PORT, strictPort: true },
  preview: { port: PORT, strictPort: true },
  build: {
    // The webviews Tauri 2 supports: WKWebView (macOS 11+) and WebKitGTK 2.40+.
    target: "safari15",
    sourcemap: false,
    // One app chunk of ~2.3 MB (env-editor with ELK and Shiki), loaded from disk.
    chunkSizeWarningLimit: 3000,
  },
});
