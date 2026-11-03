// Where the service UIs come from, for now. mangrove's Unity Catalog UI and
// headwaters' lineage UI aren't published, so the app builds them from source
// in sibling checkouts (with their own dependencies installed: `bun install` in
// mangrove, `npm install` in headwaters/node). `bun run siblings` links them as
// `.siblings/mangrove` and `.siblings/headwaters` (gitignored), which Vite's
// aliases and tsconfig's `paths` read. `MANGROVE_DIR` and `HEADWATERS_DIR`
// choose the checkouts; otherwise the first `mangrove` / `headwaters` next to
// this repository or one of its parents is used (a worktree sits deeper than
// the main checkout). Replaced by pinned release tarballs once the packages are
// published (docs/desktop.md).

import { existsSync, mkdirSync, readlinkSync, realpathSync, rmSync, symlinkSync } from "node:fs";
import path from "node:path";

export const SIBLINGS = ["mangrove", "headwaters"] as const;
export type Sibling = (typeof SIBLINGS)[number];

/** The checkout of `name`: `$<NAME>_DIR`, else the nearest one above `from`. */
export function findSibling(
  name: Sibling,
  from: string,
  env: Record<string, string | undefined> = process.env,
  exists: (p: string) => boolean = existsSync,
): string {
  const override = env[`${name.toUpperCase()}_DIR`];
  if (override) return path.resolve(override);
  let dir = path.resolve(from);
  for (;;) {
    const parent = path.dirname(dir);
    if (parent === dir) {
      throw new Error(
        `no ${name} checkout above ${from}; set ${name.toUpperCase()}_DIR (see docs/desktop.md)`,
      );
    }
    const candidate = path.join(parent, name);
    if (exists(path.join(candidate, "node"))) return candidate;
    dir = parent;
  }
}

/** Link every sibling under `<appDir>/.siblings`, replacing stale links. */
export function linkSiblings(appDir: string): void {
  const root = path.join(appDir, ".siblings");
  mkdirSync(root, { recursive: true });
  for (const name of SIBLINGS) {
    const target = findSibling(name, appDir);
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
 * app. tsconfig.json's `paths` mirror this.
 */
export const SIBLING_SOURCES: Record<string, string> = {
  "@open-lakehouse/unity-catalog-client/react": "mangrove/node/unity-catalog-client/src/react.ts",
  "@open-lakehouse/unity-catalog-client": "mangrove/node/unity-catalog-client/src/index.ts",
  "@open-lakehouse/unity-catalog-ui": "mangrove/node/unity-catalog-ui/src/index.ts",
  "@headwaters/lineage-client": "headwaters/node/lineage-client/src/index.ts",
  "@headwaters/lineage-ui": "headwaters/node/lineage-ui/src/index.ts",
};

/** The real checkout directories behind the links. */
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
