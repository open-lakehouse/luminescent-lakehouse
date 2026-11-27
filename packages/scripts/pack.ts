#!/usr/bin/env bun
// Pack @open-lakehouse/factory-client and @open-lakehouse/factory-ui into the
// tarballs a `factory-v<version>` GitHub Release carries
// (docs/factory/packages.md):
//
//   bun packages/scripts/pack.ts [version] [out-dir]
//
// `version` defaults to packages/Cargo.toml's (the release placeholder's);
// `out-dir` to target/release-assets. Each package is built (build-package.ts),
// its package.json stamped for consumers, packed with `bun pm pack`, and the
// package.json put back, so the tree is left as it was. Stamping:
//
// - `version` is the release's: package.json stays 0.0.0 in git.
// - `workspace:` specs on a sibling become its exact version: `bun pm pack`
//   would otherwise take `0.0.0` from bun.lock.
// - The context-carrying libraries the consumer provides move from
//   `dependencies` to `peerDependencies` (PEERS). In the workspace they stay
//   plain dependencies, which keeps bun.lock's hoisting as it is.
// - `exports` point at dist/ only: the `factory-source` condition and the
//   source `types` are for workspace consumers.
// - devDependencies and scripts go: consumers never install or run them.
//
// The tarballs are named `open-lakehouse-<name>-<version>.tgz`, with a
// SHA256SUMS beside them.
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { $ } from "bun";

const packages = resolve(import.meta.dir, "..");
const repo = resolve(packages, "..");

/** The packages released, in dependency order. */
export const PACKAGES = ["factory-client", "factory-ui"] as const;

/** By package: dependencies that ship as peers, with the range consumers get
 * (`exact` for a sibling, pinned to the release). */
export const PEERS: Record<string, Record<string, string>> = {
  "factory-ui": {
    "@open-lakehouse/factory-client": "exact",
    "@tanstack/react-query": "^5.0.0",
  },
};

type Manifest = Record<string, unknown> & {
  name: string;
  dependencies?: Record<string, string>;
  peerDependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
  exports?: Record<string, Record<string, string>>;
};

/** The placeholder crate's version. */
export function placeholderVersion(): string {
  const toml = readFileSync(join(packages, "Cargo.toml"), "utf8");
  const version = /^version = "([^"]+)"$/m.exec(toml)?.[1];
  if (!version) throw new Error("no version in packages/Cargo.toml");
  return version;
}

/** `pkg` as its tarball ships it (see the header). */
export function stamp(
  dir: string,
  pkg: Manifest,
  version: string,
  siblings: Set<string>,
): Manifest {
  const out: Manifest = { ...pkg, version };
  delete out.devDependencies;
  delete out.scripts;
  const deps = { ...(pkg.dependencies ?? {}) };
  const peers = { ...(pkg.peerDependencies ?? {}) };
  for (const [name, range] of Object.entries(PEERS[dir] ?? {})) {
    if (!(name in deps)) throw new Error(`${pkg.name}: ${name} isn't a dependency to make a peer`);
    delete deps[name];
    peers[name] = range === "exact" ? version : range;
  }
  for (const field of [deps, peers]) {
    for (const [name, spec] of Object.entries(field)) {
      if (!spec.startsWith("workspace:")) continue;
      if (!siblings.has(name)) throw new Error(`${pkg.name}: ${name} (${spec}) isn't released`);
      field[name] = version;
    }
  }
  out.dependencies = sorted(deps);
  out.peerDependencies = sorted(peers);
  out.exports = Object.fromEntries(
    Object.entries(pkg.exports ?? {}).map(([path, conds]) => {
      const js = conds.import;
      if (!js?.startsWith("./dist/"))
        throw new Error(`${pkg.name}: export ${path} has no dist import`);
      return [path, { types: js.replace(/\.js$/, ".d.ts"), import: js }];
    }),
  );
  return out;
}

/** Problems with a packed package.json: unresolved siblings, source exports. */
export function packedProblems(pkg: Manifest): string[] {
  const bad: string[] = [];
  for (const field of ["dependencies", "peerDependencies"] as const) {
    for (const [name, spec] of Object.entries(pkg[field] ?? {})) {
      if (
        name.startsWith("@open-lakehouse/factory-") &&
        (spec.startsWith("workspace:") || spec === "0.0.0")
      ) {
        bad.push(`${field} ${name}@${spec}`);
      }
    }
  }
  for (const [path, conds] of Object.entries(pkg.exports ?? {})) {
    for (const target of Object.values(conds)) {
      if (!target.startsWith("./dist/")) bad.push(`export ${path} → ${target}`);
    }
  }
  if (pkg.version === "0.0.0") bad.push("version 0.0.0");
  return bad;
}

function sorted(o: Record<string, string>): Record<string, string> {
  return Object.fromEntries(Object.entries(o).sort(([a], [b]) => a.localeCompare(b)));
}

if (import.meta.main) {
  const version = process.argv[2] || placeholderVersion();
  const out = resolve(repo, process.argv[3] || "target/release-assets");
  mkdirSync(out, { recursive: true });
  const manifests = PACKAGES.map((dir) => {
    const path = join(packages, dir, "package.json");
    const text = readFileSync(path, "utf8");
    return { dir, path, text, pkg: JSON.parse(text) as Manifest };
  });
  const siblings = new Set(manifests.map((m) => m.pkg.name));
  const sums: string[] = [];
  for (const { dir, path, text, pkg } of manifests) {
    const cwd = join(packages, dir);
    await $`bun run build`.cwd(cwd);
    const tgz = `${pkg.name.replace(/^@/, "").replace("/", "-")}-${version}.tgz`;
    const changelog = join(cwd, "CHANGELOG.md");
    try {
      writeFileSync(path, `${JSON.stringify(stamp(dir, pkg, version, siblings), null, 2)}\n`);
      copyFileSync(join(packages, "CHANGELOG.md"), changelog);
      await $`bun pm pack --ignore-scripts --quiet --filename ${join(out, tgz)}`.cwd(cwd);
    } finally {
      writeFileSync(path, text);
      rmSync(changelog, { force: true });
    }
    const packed = JSON.parse(await $`tar -xzOf ${join(out, tgz)} package/package.json`.text());
    const bad = packedProblems(packed);
    if (bad.length) throw new Error(`${tgz}: ${bad.join(", ")}`);
    const sha = new Bun.CryptoHasher("sha256").update(readFileSync(join(out, tgz))).digest("hex");
    sums.push(`${sha}  ${tgz}`);
    console.log(`→ ${join(out, tgz)}`);
  }
  writeFileSync(join(out, "SHA256SUMS.factory"), `${sums.join("\n")}\n`);
}
