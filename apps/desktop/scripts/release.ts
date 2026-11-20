// The desktop app's releases (docs/desktop-release.md). A release is a commit on
// main whose `src-tauri/Cargo.toml` version has no `desktop-v<version>` tag yet:
// desktop-release.yml then builds, tags and publishes it. This script prepares
// that commit and gives the workflow its release notes.
//
//   bun scripts/release.ts prepare [version]   bump the app (default: the next patch)
//                                              and prepend its CHANGELOG.md section
//   bun scripts/release.ts notes <version>     print that version's CHANGELOG.md section
//   bun scripts/release.ts version             print the app's version
//
// release-plz can't do this: its `git_only` mode opens the git repository at the
// workspace's directory, and this workspace is nested in the repository's.

import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

export const REPO = "https://github.com/open-lakehouse/luminescent-lakehouse";
export const TAG_PREFIX = "desktop-v";
export const PACKAGE = "luminescent-desktop";

/** What the app is built from, relative to the repository root: the app, and the
 * root workspace's crates and packages it builds by path. A commit touching none
 * of them isn't in the app's changelog. */
export const PATHS = [
  "apps/desktop",
  "crates/omnigent-client",
  "crates/factory-core",
  "crates/factory-proto",
  "crates/factory-server",
  "crates/plan-tree",
  "crates/workgraph-mcp",
  "packages/factory-client",
  "packages/factory-ui",
];

/** Changelog groups by conventional-commit type, in the order they're listed
 * (the root release-plz.toml's parsers). Other types are left out. */
const GROUPS: [string, string][] = [
  ["feat", "Added"],
  ["fix", "Fixed"],
  ["perf", "Performance"],
  ["refactor", "Changed"],
  ["docs", "Documentation"],
  ["build", "Build"],
];

export type Commit = {
  type: string;
  scope?: string;
  breaking: boolean;
  description: string;
  pr?: number;
};

/** A squash-merged PR title, `type(scope)!: description (#123)`; undefined when it
 * isn't a conventional commit. */
export function parseCommit(subject: string): Commit | undefined {
  const m = /^(\w+)(?:\(([^)]+)\))?(!)?: (.+?)(?: \(#(\d+)\))?$/.exec(subject.trim());
  if (!m) return undefined;
  const [, type, scope, bang, description, pr] = m;
  return {
    type: type.toLowerCase(),
    scope,
    breaking: bang === "!",
    description,
    pr: pr ? Number(pr) : undefined,
  };
}

export function nextPatch(version: string): string {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(version);
  if (!m) throw new Error(`not a release version: ${version}`);
  return `${m[1]}.${m[2]}.${Number(m[3]) + 1}`;
}

function line(c: Commit): string {
  const scope = c.scope ? `*(${c.scope})* ` : "";
  const breaking = c.breaking ? "[**breaking**] " : "";
  const pr = c.pr ? ` ([#${c.pr}](${REPO}/pull/${c.pr}))` : "";
  return `- ${scope}${breaking}${c.description}${pr}`;
}

/** The CHANGELOG.md section of a release, in the root changelog's format. Without
 * a previous release, it is the first one. */
export function renderSection(opts: {
  version: string;
  previous?: string;
  date: string;
  commits: Commit[];
}): string {
  const { version, previous, date, commits } = opts;
  const tag = `${TAG_PREFIX}${version}`;
  const link = previous
    ? `${REPO}/compare/${TAG_PREFIX}${previous}...${tag}`
    : `${REPO}/releases/tag/${tag}`;
  const out = [`## [${version}](${link}) - ${date}`, ""];
  if (!previous) out.push("The first published release of the desktop app.", "");
  for (const [type, title] of GROUPS) {
    const group = commits.filter((c) => c.type === type);
    if (group.length === 0) continue;
    out.push(`### ${title}`, "", ...group.map(line), "");
  }
  if (previous && out.length === 2) out.push("No user-facing changes.", "");
  return out.join("\n");
}

/** Insert a section after `## [Unreleased]`, or before the first release's. */
export function prependSection(changelog: string, section: string): string {
  const unreleased = /^## \[Unreleased\]\n*/m.exec(changelog);
  if (unreleased) {
    const rest = changelog.slice(unreleased.index + unreleased[0].length);
    const head = changelog.slice(0, unreleased.index);
    return `${head}## [Unreleased]\n\n${section}${rest ? `\n${rest}` : ""}`;
  }
  const first = /^## \[/m.exec(changelog);
  if (first) {
    return `${changelog.slice(0, first.index)}${section}\n${changelog.slice(first.index)}`;
  }
  return `${changelog.trimEnd()}\n\n${section}`;
}

/** A release's section without its heading: the GitHub Release's body. */
export function sectionOf(changelog: string, version: string): string | undefined {
  const lines = changelog.split("\n");
  const start = lines.findIndex((l) => l.startsWith(`## [${version}]`));
  if (start < 0) return undefined;
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((l) => l.startsWith("## ["));
  return (end < 0 ? rest : rest.slice(0, end)).join("\n").trim();
}

/** The `[package]` version of a Cargo.toml. */
export function cargoVersion(toml: string): string {
  const pkg = /^\[package\]\n(?:(?!^\[)[\s\S])*?^version = "([^"]+)"/m.exec(toml);
  if (!pkg) throw new Error("no [package] version");
  return pkg[1];
}

export function setCargoVersion(toml: string, version: string): string {
  cargoVersion(toml);
  return toml.replace(/^(\[package\]\n(?:(?!^\[)[\s\S])*?^version = ")[^"]+(")/m, `$1${version}$2`);
}

/** Set a workspace member's version in Cargo.lock (no resolve, so no registry). */
export function setLockVersion(lock: string, name: string, version: string): string {
  const entry = new RegExp(`(\\[\\[package\\]\\]\\nname = "${name}"\\nversion = ")[^"]+(")`);
  if (!entry.test(lock)) throw new Error(`${name} isn't in Cargo.lock`);
  return lock.replace(entry, `$1${version}$2`);
}

// The CLI.

const app = dirname(import.meta.dir);
const manifest = join(app, "src-tauri", "Cargo.toml");
const lockfile = join(app, "Cargo.lock");
const changelogFile = join(app, "CHANGELOG.md");

function git(...args: string[]): string {
  const r = spawnSync("git", args, { cwd: app, encoding: "utf8" });
  if (r.status !== 0) throw new Error(`git ${args.join(" ")}: ${r.stderr.trim()}`);
  return r.stdout.trim();
}

/** The newest `desktop-v*` tag reachable from HEAD, as a version. */
function previousRelease(): string | undefined {
  const r = spawnSync("git", ["describe", "--tags", "--abbrev=0", "--match", `${TAG_PREFIX}*`], {
    cwd: app,
    encoding: "utf8",
  });
  return r.status === 0 ? r.stdout.trim().slice(TAG_PREFIX.length) : undefined;
}

function commitsSince(previous: string): Commit[] {
  const root = git("rev-parse", "--show-toplevel");
  const r = spawnSync(
    "git",
    ["log", "--format=%s", `${TAG_PREFIX}${previous}..HEAD`, "--", ...PATHS],
    { cwd: root, encoding: "utf8" },
  );
  if (r.status !== 0) throw new Error(`git log: ${r.stderr.trim()}`);
  return r.stdout
    .split("\n")
    .filter(Boolean)
    .map(parseCommit)
    .filter((c): c is Commit => c !== undefined);
}

function prepare(requested?: string) {
  const toml = readFileSync(manifest, "utf8");
  const current = cargoVersion(toml);
  const previous = previousRelease();
  // Before the first release, the version in Cargo.toml is the one to release.
  const version = requested ?? (previous ? nextPatch(current) : current);
  if (previous && version === previous) throw new Error(`${version} is already released`);
  writeFileSync(manifest, setCargoVersion(toml, version));
  writeFileSync(lockfile, setLockVersion(readFileSync(lockfile, "utf8"), PACKAGE, version));
  const section = renderSection({
    version,
    previous,
    date: new Date().toISOString().slice(0, 10),
    commits: previous ? commitsSince(previous) : [],
  });
  writeFileSync(changelogFile, prependSection(readFileSync(changelogFile, "utf8"), section));
  console.log(`desktop v${version}: src-tauri/Cargo.toml, Cargo.lock, CHANGELOG.md`);
  console.log(`commit it as "chore(desktop): release v${version}" and open a PR`);
}

if (import.meta.main) {
  const [cmd, arg] = process.argv.slice(2);
  if (cmd === "prepare") prepare(arg);
  else if (cmd === "version") console.log(cargoVersion(readFileSync(manifest, "utf8")));
  else if (cmd === "notes" && arg) {
    const notes = sectionOf(readFileSync(changelogFile, "utf8"), arg);
    if (notes === undefined) {
      console.error(`CHANGELOG.md has no section for ${arg}`);
      process.exit(1);
    }
    console.log(notes);
  } else {
    console.error("usage: release.ts prepare [version] | notes <version> | version");
    process.exit(2);
  }
}
