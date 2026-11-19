import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defaultClientConditions, defineConfig, type Plugin } from "vite";

// Tauri loads the dev server at this port (src-tauri/tauri.conf.json `devUrl`).
const PORT = 4323;

/** The Playwright build's stand-in for the desktop's `mcp-app://` scheme
 *  (src-tauri/src/mcp_app.rs): the same files and CSP header, on a second origin (the
 *  page's port + 1, the fakes' `fakeMcpAppSandboxUrl`), for the page's origin only. */
function mcpAppSandbox(): Plugin {
  const dir = path.resolve(import.meta.dirname, "src-tauri/assets/mcp-app");
  const read = (file: string) => readFileSync(path.join(dir, file), "utf8");
  const listen = (port: number | undefined, close: (fn: () => void) => void) => {
    const page = port ?? PORT;
    const origins = [`http://localhost:${page}`];
    const sandbox = createServer((req, res) => {
      const file = { "/mcp-app/sandbox.html": "sandbox.html", "/mcp-app/sandbox.js": "sandbox.js" }[
        (req.url ?? "").split("?")[0]
      ];
      if (!file || req.method !== "GET") {
        res.statusCode = 404;
        res.end();
        return;
      }
      const body = file.endsWith(".js")
        ? read(file).replace("/*__MCP_APP_PARENT_ORIGINS__*/ []", JSON.stringify(origins))
        : read(file);
      res.setHeader("Content-Type", file.endsWith(".js") ? "text/javascript" : "text/html");
      res.setHeader(
        "Content-Security-Policy",
        read("csp.txt").trim().replace("{parent_origins}", origins.join(" ")),
      );
      res.setHeader("Cache-Control", "no-store");
      res.end(body);
    });
    sandbox.listen(page + 1, "localhost");
    close(() => sandbox.close());
  };
  return {
    name: "mcp-app-sandbox",
    apply: (_config, { mode }) => mode === "e2e",
    configureServer: (server) =>
      listen(server.config.server.port, (fn) => server.httpServer?.on("close", fn)),
    configurePreviewServer: (server) =>
      listen(server.config.preview.port, (fn) => server.httpServer.on("close", fn)),
  };
}

export default defineConfig({
  plugins: [react(), tailwindcss(), mcpAppSandbox()],
  // Tauri prints its own errors; keep Vite's output from clearing them.
  clearScreen: false,
  // mangrove's Catalog shows its table preview only with this on (and a query
  // runner registered, which src/main.ts does). Its prebuilt package reads the
  // flag through `import.meta.env`, which this build replaces.
  define: { "import.meta.env.VITE_ENABLE_PREVIEW": JSON.stringify("true") },
  resolve: {
    // shadcn's import alias, for the components copied from registries (chatcn).
    alias: { "@": path.resolve(import.meta.dirname, "src") },
    // The factory's packages (packages/factory-*, Bun workspace members) export their
    // TypeScript source under this condition, and ship no dist/.
    conditions: ["factory-source", ...defaultClientConditions],
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
  optimizeDeps: {
    // @open-lakehouse/editor's dist/ imports Monaco's workers with Vite's
    // `?worker` suffix, which the dependency optimizer (rolldown) can't load.
    // Leave those imports to Vite's own pipeline, which serves each as a
    // worker module, and pre-bundle the rest as usual.
    rolldownOptions: {
      plugins: [
        {
          name: "external-workers",
          async resolveId(id, importer) {
            const [path, query] = id.split("?");
            if (!query || !/(^|&)worker\b/.test(query)) return null;
            // By its file path: the bare name doesn't resolve from the
            // optimizer's output directory.
            const file = await this.resolve(path, importer, { skipSelf: true });
            return file ? { id: `${file.id}?${query}`, external: true } : null;
          },
        },
      ],
    },
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
