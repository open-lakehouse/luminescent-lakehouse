import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defineConfig } from "vite";

const PORT = 4330;
// factory-server; `just factory-dev` starts it on this port.
const API = process.env.FACTORY_API ?? "http://127.0.0.1:7410";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    // Our workspace packages from source.
    conditions: ["factory-source"],
    // One copy of each context-carrying library (as luminescent-lakehouse's
    // desktop app does): hooks, context and registries break across copies.
    dedupe: [
      "react",
      "react-dom",
      "@tanstack/react-query",
      "@open-lakehouse/ui-kit",
      "@open-lakehouse/host",
      "@open-lakehouse/editor",
      "monaco-editor",
      "@xyflow/react",
      "@connectrpc/connect",
      "@connectrpc/connect-web",
    ],
  },
  optimizeDeps: {
    // @open-lakehouse/editor's dist/ imports Monaco's workers with Vite's
    // `?worker` suffix, which the dependency optimizer can't load; leave those
    // to Vite's own pipeline (the same plugin as the desktop app's).
    rolldownOptions: {
      plugins: [
        {
          name: "external-workers",
          async resolveId(id, importer) {
            const [file, query] = id.split("?");
            if (!query || !/(^|&)worker\b/.test(query)) return null;
            const resolved = await this.resolve(file, importer, { skipSelf: true });
            return resolved ? { id: `${resolved.id}?${query}`, external: true } : null;
          },
        },
      ],
    },
  },
  server: {
    port: PORT,
    strictPort: true,
    // The Connect API, so the app talks to its own origin in dev.
    proxy: { "/factory.v1.FactoryService": { target: API, changeOrigin: true } },
    fs: { allow: [path.resolve(__dirname, "../..")] },
  },
  preview: { port: PORT, strictPort: true },
  // Monaco (with the SQL languages mangrove's editor registers) loads lazily as one large chunk.
  build: { chunkSizeWarningLimit: 9000 },
});
