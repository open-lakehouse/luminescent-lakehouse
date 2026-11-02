import path from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react-swc";
import { defineConfig } from "vite";

export default defineConfig({
  // Fixed, so the localhost origin Neon Auth sees (and trusts) doesn't move.
  server: { port: 4322, strictPort: true },
  preview: { port: 4322, strictPort: true },
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": path.resolve(__dirname, "./src") },
  },
});
