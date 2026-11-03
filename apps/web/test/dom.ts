// `bun test` preload: a browser-like global DOM (happy-dom) for the component
// tests. Tests without components don't touch it.
import { GlobalRegistrator } from "@happy-dom/global-registrator";

GlobalRegistrator.register({ url: "http://localhost:4322/" });
