// `bun test` preload: a browser-like global DOM (happy-dom) for the component
// tests. Tests without components don't touch it.
import { GlobalRegistrator } from "@happy-dom/global-registrator";

GlobalRegistrator.register({ url: "http://localhost:4330/" });

// React Flow measures its viewport and nodes; happy-dom has no layout, so a
// no-op observer is enough for rendering.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;
