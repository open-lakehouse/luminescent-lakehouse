#!/usr/bin/env bun
// Build a factory package into `dist/`: `tsc -p tsconfig.build.json` emits ESM,
// declarations and maps, one module per source file. No bundler, so a dynamic
// `import()` (factory-ui's Monaco editors) stays a module of its own, and the
// consumer's bundler splits it into a chunk that loads only when it's used.
// Then every bare import in `dist/` must be a declared dependency or peer, so a
// tarball can't ship an import its consumers won't have installed.
//
// Run from the package directory (each package's `build` script does):
//   bun ../scripts/build-package.ts
//
// Workspace consumers never need this: they resolve the `factory-source`
// export condition to `src/`. Adapted from trestle's node/scripts/build-package.ts.
import { readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { $, Glob } from "bun";

const pkgDir = process.cwd();
const dist = join(pkgDir, "dist");

rmSync(dist, { recursive: true, force: true });
await $`bunx tsc -p tsconfig.build.json`.cwd(pkgDir);

const pkg = JSON.parse(readFileSync(join(pkgDir, "package.json"), "utf8"));
const declared = new Set<string>([
  pkg.name,
  ...Object.keys(pkg.dependencies ?? {}),
  ...Object.keys(pkg.peerDependencies ?? {}),
]);

const problems = undeclaredImports(dist, declared);
if (problems.length > 0) {
  console.error(`build-package: ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log(`build-package: built ${pkg.name} into dist/`);

/** `@scope/name/sub` → `@scope/name`; `name/sub` → `name`. */
function packageOf(spec: string): string {
  return spec.startsWith("@")
    ? spec.split("/").slice(0, 2).join("/")
    : (spec.split("/")[0] ?? spec);
}

/** Every bare import in `dir`'s JS and declarations that `declared` doesn't name. */
export function undeclaredImports(dir: string, declared: Set<string>): string[] {
  const out: string[] = [];
  for (const rel of new Glob("**/*.{js,d.ts}").scanSync({ cwd: dir })) {
    const code = readFileSync(join(dir, rel), "utf8");
    for (const [, spec] of code.matchAll(/\b(?:from|import)\s*\(?\s*["']([^"'.\s][^"'\s]*)["']/g)) {
      if (spec && !spec.startsWith("node:") && !declared.has(packageOf(spec))) {
        out.push(`${rel} imports undeclared "${spec}"`);
      }
    }
  }
  return out;
}
