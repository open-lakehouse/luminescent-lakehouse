// Builds the MCP App workgraph-mcp serves as `ui://factory/plan`: one HTML file
// with every script and style inlined (vite-plugin-singlefile). Hosts load it
// in a sandbox with no network and no base URL, so it can't fetch chunks,
// workers or fonts. `scripts/sync.ts` copies it into the crate.
//
// factory-ui's Monaco module (the text diff, the Markdown editor) is replaced
// by a stub: the review offers the tree diff only, and Monaco's workers can't
// be inlined.

import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defaultClientConditions, defineConfig, type Plugin } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";

const MONACO = path.resolve(__dirname, "../factory-ui/src/monaco/index");
const STUB = path.resolve(__dirname, "src/monaco-stub.tsx");

function noMonaco(): Plugin {
  return {
    name: "factory-plan-review:no-monaco",
    enforce: "pre",
    async resolveId(id, importer, options) {
      if (!id.includes("monaco/index")) return null;
      const resolved = await this.resolve(id, importer, { ...options, skipSelf: true });
      return resolved && resolved.id.replace(/\.[jt]sx?$/, "") === MONACO ? STUB : null;
    },
  };
}

export default defineConfig({
  plugins: [noMonaco(), react(), tailwindcss(), viteSingleFile()],
  resolve: {
    // The workspace packages from source.
    conditions: ["factory-source", ...defaultClientConditions],
    dedupe: [
      "react",
      "react-dom",
      "@tanstack/react-query",
      "@open-lakehouse/ui-kit",
      "@open-lakehouse/host",
      "@xyflow/react",
      "@connectrpc/connect",
    ],
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // One inlined chunk by design.
    chunkSizeWarningLimit: 8192,
  },
});
