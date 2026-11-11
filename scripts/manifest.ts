// Writes a release's `manifest.json`: what a deployer's tooling needs to know
// about each function the release ships, next to the assets themselves
// (docs/deployment.md, "Release assets"). Each fact has one source:
//
// - neon.ts: the functions, their `env` and triggers;
// - `TENANT.resources` in neon.ts: each function's OAuth resource;
// - scripts/function.sh: its secrets, its database, whether it migrates;
// - the root Cargo.toml: the upstream pins the release was built from;
// - the assets in the directory: their names and sha256.
//
// Usage: bun scripts/manifest.ts <version> <assets-dir>
// (`just release-assets`, deploy.yml's release job). Needs `bun install`.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** Bumped on any change a reader must handle; additions don't bump it. */
export const MANIFEST_SCHEMA = 1;

export const MANIFEST = "manifest.json";
export const SUMS = "SHA256SUMS";

/** A file attached to the release. */
export interface Asset {
  name: string;
  sha256: string;
  size: number;
}

/** What scripts/function.sh says about a function. */
export interface FunctionFacts {
  /** Env vars set once per branch, never in neon.ts. */
  secrets: string[];
  /** The database it lives in on the branch; created when missing. */
  database: string;
  /** Whether `luminescent-migrate <slug>` runs before every deploy. */
  migrate: boolean;
}

export interface ManifestFunction extends FunctionFacts {
  name?: string;
  /** The zipped `dist/` that `neon deploy` ships, unbundled. */
  asset: Asset;
  /** Non-secret env, as neon.ts deploys it. */
  env: Record<string, string>;
  /** The `aud` of the tokens it accepts. */
  oauthResource: string;
  /** neon.ts's triggers that call it, by name. */
  triggers: Record<string, unknown>;
}

export interface Manifest {
  schema: number;
  version: string;
  commit?: string;
  /** The service crates the addons and migrations are built from. */
  upstreams: Record<string, { git: string; rev: string; package?: string }>;
  functions: Record<string, ManifestFunction>;
  /** `luminescent-<version>-<target>.tar.gz`: luminescent-migrate and luminescent-admin, by target. */
  cli: Record<string, Asset>;
  /** `luminescent-web-<version>.tar.gz`: the self-hostable auth pages. */
  web?: Asset;
}

interface NeonFunction {
  name?: string;
  env?: Record<string, string>;
}

interface NeonTrigger {
  function?: string;
}

export interface ManifestInput {
  version: string;
  commit?: string;
  functions: Record<string, NeonFunction>;
  triggers: Record<string, NeonTrigger>;
  resources: Record<string, string>;
  facts: Record<string, FunctionFacts>;
  /** The root Cargo.toml's `[workspace.dependencies]`. */
  dependencies: Record<string, unknown>;
  assets: Asset[];
}

export const functionAsset = (slug: string, version: string): string =>
  `${slug}-${version}-linux-arm64.zip`;
export const webAsset = (version: string): string => `luminescent-web-${version}.tar.gz`;
const cliRe = (version: string): RegExp =>
  new RegExp(`^luminescent-${version.replace(/[.+]/g, "\\$&")}-(.+)\\.tar\\.gz$`);

export function buildManifest(input: ManifestInput): Manifest {
  const { version } = input;
  const byName = new Map(input.assets.map((a) => [a.name, a]));

  const functions: Record<string, ManifestFunction> = {};
  for (const [slug, fn] of Object.entries(input.functions)) {
    const asset = byName.get(functionAsset(slug, version));
    if (!asset) throw new Error(`no ${functionAsset(slug, version)} for function '${slug}'`);
    const facts = input.facts[slug];
    if (!facts) throw new Error(`scripts/function.sh has nothing for function '${slug}'`);
    const oauthResource = input.resources[slug];
    if (!oauthResource) throw new Error(`TENANT.resources has no OAuth resource for '${slug}' (neon.ts)`);
    functions[slug] = {
      ...(fn.name ? { name: fn.name } : {}),
      asset,
      env: fn.env ?? {},
      ...facts,
      oauthResource,
      triggers: Object.fromEntries(
        Object.entries(input.triggers).filter(([, t]) => t.function === slug),
      ),
    };
  }

  const cli: Record<string, Asset> = {};
  for (const asset of input.assets) {
    const target = cliRe(version).exec(asset.name)?.[1];
    if (target) cli[target] = asset;
  }

  const upstreams: Manifest["upstreams"] = {};
  for (const [key, dep] of Object.entries(input.dependencies)) {
    if (typeof dep !== "object" || dep === null) continue;
    const { git, rev, package: pkg } = dep as { git?: string; rev?: string; package?: string };
    if (git && rev) upstreams[key] = { git, rev, ...(pkg ? { package: pkg } : {}) };
  }

  const web = byName.get(webAsset(version));
  return {
    schema: MANIFEST_SCHEMA,
    version,
    ...(input.commit ? { commit: input.commit } : {}),
    upstreams,
    functions,
    cli,
    ...(web ? { web } : {}),
  };
}

/** Every asset the manifest names. */
export function listed(m: Manifest): Asset[] {
  return [...Object.values(m.functions).map((f) => f.asset), ...Object.values(m.cli), ...(m.web ? [m.web] : [])];
}

/** `sha256sum`'s format, which `sha256sum -c` and `shasum -a 256 -c` check. */
export function sums(assets: Asset[]): string {
  return [...assets]
    .sort((a, b) => a.name.localeCompare(b.name))
    .map((a) => `${a.sha256}  ${a.name}\n`)
    .join("");
}

/** The files in `dir`, but for those named in `except`. */
function readAssets(dir: string, except: string[]): Asset[] {
  return readdirSync(dir)
    .filter((name) => !except.includes(name) && statSync(join(dir, name)).isFile())
    .map((name) => {
      const bytes = readFileSync(join(dir, name));
      return { name, sha256: createHash("sha256").update(bytes).digest("hex"), size: bytes.length };
    });
}

function functionFacts(slug: string): FunctionFacts {
  const out = execFileSync(
    "bash",
    ["-c", 'source scripts/function.sh "$1" && printf "%s\\n%s\\n%s" "$SECRETS" "$DATABASE" "$NEEDS_PG"', "_", slug],
    { encoding: "utf8" },
  );
  const [secrets = "", database = "", needsPg = ""] = out.split("\n");
  if (!database) throw new Error(`scripts/function.sh sets no DATABASE for '${slug}'`);
  return { secrets: secrets.split(" ").filter(Boolean), database, migrate: needsPg === "1" };
}

if (import.meta.main) {
  const [version, dir] = process.argv.slice(2);
  if (!version || !dir) {
    console.error("usage: bun scripts/manifest.ts <version> <assets-dir>");
    process.exit(2);
  }
  process.chdir(join(import.meta.dir, ".."));
  const { default: config, TENANT } = await import("../neon.ts");
  const cargo = Bun.TOML.parse(readFileSync("Cargo.toml", "utf8")) as {
    workspace?: { dependencies?: Record<string, unknown> };
  };
  const functions = (config.functions ?? {}) as Record<string, NeonFunction>;
  const commit =
    process.env.GITHUB_SHA ?? execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim();
  const assets = readAssets(dir, [MANIFEST, SUMS]);
  const manifest = buildManifest({
    version,
    commit,
    functions,
    triggers: (config.triggers ?? {}) as Record<string, NeonTrigger>,
    resources: TENANT.resources,
    facts: Object.fromEntries(Object.keys(functions).map((slug) => [slug, functionFacts(slug)])),
    dependencies: cargo.workspace?.dependencies ?? {},
    assets,
  });
  writeFileSync(join(dir, MANIFEST), `${JSON.stringify(manifest, null, 2)}\n`);
  // The release's assets, the manifest included; anything else in `dir`
  // (another version's leftovers) is not part of it.
  const self = readAssets(dir, []).find((a) => a.name === MANIFEST);
  if (!self) throw new Error(`${MANIFEST} not written`);
  writeFileSync(join(dir, SUMS), sums([...listed(manifest), self]));
  console.log(`→ ${join(dir, MANIFEST)}: ${Object.keys(manifest.functions).join(", ")}; ${join(dir, SUMS)}`);
}
