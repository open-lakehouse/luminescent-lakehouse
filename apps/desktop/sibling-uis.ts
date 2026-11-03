// Where the service UIs come from, for now. mangrove's Unity Catalog UI and
// headwaters' lineage UI aren't published, so the app builds them from source:
// each repository at the commit `siblings.json` pins, fetched into
// `.siblings-src/` with its dependencies installed from its own lockfile, and
// linked as `.siblings/<name>` (both gitignored), which Vite's aliases and
// globals.css read. Vite's config links them on every start; `bun run siblings`
// does it up front (CI). For working on the UIs themselves, use a checkout
// instead: `MANGROVE_DIR` / `HEADWATERS_DIR` name one, and
// `LUMINESCENT_SIBLINGS=local` takes the nearest `mangrove` / `headwaters`
// beside this repository or one of its parents (dependencies installed by you).
// Replaced by pinned release tarballs once the packages are published
// (docs/desktop.md).

import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";

export const SIBLINGS = ["mangrove", "headwaters"] as const;
export type Sibling = (typeof SIBLINGS)[number];

/** A pin in `siblings.json`. */
export interface SiblingPin {
  repository: string;
  rev: string;
}

type Env = Record<string, string | undefined>;

/** How each sibling installs its dependencies, from its own lockfile. */
const INSTALL: Record<Sibling, { cwd: string; argv: string[] }> = {
  mangrove: { cwd: ".", argv: ["bun", "install", "--frozen-lockfile", "--ignore-scripts"] },
  headwaters: {
    cwd: "node",
    argv: ["npm", "ci", "--ignore-scripts", "--no-audit", "--no-fund"],
  },
};

/** Marks a fetched sibling whose dependencies are installed. */
const READY = ".luminescent-ready";

export function readPins(appDir: string): Record<Sibling, SiblingPin> {
  return JSON.parse(readFileSync(path.join(appDir, "siblings.json"), "utf8"));
}

/** The nearest checkout of `name` beside `from` or one of its parents. */
export function findCheckout(
  name: Sibling,
  from: string,
  exists: (p: string) => boolean = existsSync,
): string {
  let dir = path.resolve(from);
  for (;;) {
    const parent = path.dirname(dir);
    if (parent === dir) {
      throw new Error(`no ${name} checkout above ${from}; set ${name.toUpperCase()}_DIR`);
    }
    const candidate = path.join(parent, name);
    if (exists(path.join(candidate, "node"))) return candidate;
    dir = parent;
  }
}

/** Where the pinned `name` is fetched to. */
export function fetchDir(appDir: string, name: Sibling, pin: SiblingPin): string {
  return path.join(appDir, ".siblings-src", `${name}-${pin.rev.slice(0, 12)}`);
}

function run(argv: string[], cwd: string) {
  const [cmd, ...args] = argv;
  if (!cmd) throw new Error("empty command");
  execFileSync(cmd, args, { cwd, stdio: ["ignore", "inherit", "inherit"] });
}

/** Fetch the pinned commit of `name` and install its dependencies, unless done. */
function fetchPinned(appDir: string, name: Sibling, pin: SiblingPin): string {
  const dir = fetchDir(appDir, name, pin);
  if (existsSync(path.join(dir, READY))) return dir;
  console.error(`siblings: fetching ${name} at ${pin.rev}`);
  // Drop this sibling's other revisions, and any half-done fetch of this one.
  const parent = path.dirname(dir);
  mkdirSync(parent, { recursive: true });
  for (const entry of readdirSync(parent)) {
    if (entry.startsWith(`${name}-`)) rmSync(path.join(parent, entry), { recursive: true });
  }
  mkdirSync(dir);
  run(["git", "init", "-q"], dir);
  run(["git", "fetch", "-q", "--depth", "1", pin.repository, pin.rev], dir);
  run(["git", "checkout", "-q", "FETCH_HEAD"], dir);
  const install = INSTALL[name];
  run(install.argv, path.join(dir, install.cwd));
  writeFileSync(path.join(dir, READY), `${pin.rev}\n`);
  return dir;
}

/** The directory `name` builds from (see the top of this file). */
export function resolveSibling(appDir: string, name: Sibling, env: Env = process.env): string {
  const override = env[`${name.toUpperCase()}_DIR`];
  if (override) return path.resolve(override);
  if (env.LUMINESCENT_SIBLINGS === "local") return findCheckout(name, appDir);
  return fetchPinned(appDir, name, readPins(appDir)[name]);
}

/** Link every sibling under `<appDir>/.siblings`, replacing stale links. */
export function linkSiblings(appDir: string, env: Env = process.env): void {
  const root = path.join(appDir, ".siblings");
  mkdirSync(root, { recursive: true });
  for (const name of SIBLINGS) {
    const target = resolveSibling(appDir, name, env);
    const link = path.join(root, name);
    let current: string | undefined;
    try {
      current = readlinkSync(link);
    } catch {}
    if (current === target) continue;
    rmSync(link, { force: true });
    symlinkSync(target, link, "dir");
  }
}

/**
 * The package names the shell imports directly, mapped to their sources under
 * `.siblings`. Their own imports resolve through the sibling's `node_modules`,
 * except the deduplicated packages (vite.config.ts), which resolve from this
 * app. globals.css's `@source` lines follow the same layout.
 */
export const SIBLING_SOURCES: Record<string, string> = {
  "@open-lakehouse/unity-catalog-client/react": "mangrove/node/unity-catalog-client/src/react.ts",
  "@open-lakehouse/unity-catalog-client": "mangrove/node/unity-catalog-client/src/index.ts",
  "@open-lakehouse/unity-catalog-ui": "mangrove/node/unity-catalog-ui/src/index.ts",
  "@headwaters/lineage-client": "headwaters/node/lineage-client/src/index.ts",
  "@headwaters/lineage-ui": "headwaters/node/lineage-ui/src/index.ts",
};

/** The real directories behind the links. */
export function siblingRoots(appDir: string): string[] {
  return SIBLINGS.map((name) => realpathSync(path.join(appDir, ".siblings", name)));
}

/** Exact-match Vite aliases for {@link SIBLING_SOURCES}. */
export function siblingAliases(appDir: string) {
  return Object.entries(SIBLING_SOURCES).map(([name, file]) => ({
    find: new RegExp(`^${name.replace(/[/.]/g, "\\$&")}$`),
    replacement: path.join(appDir, ".siblings", file),
  }));
}

if (import.meta.main) {
  linkSiblings(import.meta.dir);
  for (const name of SIBLINGS) {
    console.log(`${name}: ${readlinkSync(path.join(import.meta.dir, ".siblings", name))}`);
  }
}
