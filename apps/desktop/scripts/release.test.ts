import { expect, test } from "bun:test";
import {
  cargoVersion,
  nextPatch,
  parseCommit,
  prependSection,
  REPO,
  renderSection,
  sectionOf,
  setCargoVersion,
  setLockVersion,
} from "./release";

test("parses squash-merged PR titles", () => {
  expect(parseCommit("feat(desktop): the Models choice (#179)")).toEqual({
    type: "feat",
    scope: "desktop",
    breaking: false,
    description: "the Models choice",
    pr: 179,
  });
  expect(parseCommit("feat(desktop)!: trestle 0.0.22 (#177)")?.breaking).toBe(true);
  expect(parseCommit("fix: a harness fix")).toEqual({
    type: "fix",
    scope: undefined,
    breaking: false,
    description: "a harness fix",
    pr: undefined,
  });
  expect(parseCommit("Merge branch 'main'")).toBeUndefined();
});

test("bumps the patch", () => {
  expect(nextPatch("0.0.9")).toBe("0.0.10");
  expect(nextPatch("1.2.3")).toBe("1.2.4");
  expect(() => nextPatch("0.1")).toThrow();
});

test("groups commits like the root changelog and leaves out chores, tests and CI", () => {
  const commits = [
    "feat(desktop): the Models choice (#179)",
    "fix(desktop)!: trust the host-label origin (#181)",
    "ci(desktop): build releases (#200)",
    "test(desktop): more e2e (#201)",
    "feat(workgraph-mcp): plan review (#191)",
  ]
    .map(parseCommit)
    .filter((c) => c !== undefined);
  const section = renderSection({
    version: "0.0.2",
    previous: "0.0.1",
    date: "2026-11-21",
    commits,
  });
  expect(section).toBe(
    [
      `## [0.0.2](${REPO}/compare/desktop-v0.0.1...desktop-v0.0.2) - 2026-11-21`,
      "",
      "### Added",
      "",
      `- *(desktop)* the Models choice ([#179](${REPO}/pull/179))`,
      `- *(workgraph-mcp)* plan review ([#191](${REPO}/pull/191))`,
      "",
      "### Fixed",
      "",
      `- *(desktop)* [**breaking**] trust the host-label origin ([#181](${REPO}/pull/181))`,
      "",
    ].join("\n"),
  );
});

test("the first release links its tag; a release of chores says so", () => {
  const first = renderSection({ version: "0.0.1", date: "2026-11-20", commits: [] });
  expect(first).toContain(`${REPO}/releases/tag/desktop-v0.0.1`);
  expect(first).toContain("The first published release");
  const chores = renderSection({
    version: "0.0.3",
    previous: "0.0.2",
    date: "2026-11-22",
    commits: [],
  });
  expect(chores).toContain("No user-facing changes.");
});

test("prepends below Unreleased and reads a section back", () => {
  const changelog =
    "# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [0.0.1](x) - 2026-11-20\n\nFirst.\n";
  const section = "## [0.0.2](y) - 2026-11-21\n\n### Added\n\n- a thing\n";
  const out = prependSection(changelog, section);
  expect(out).toBe(
    "# Changelog\n\nIntro.\n\n## [Unreleased]\n\n## [0.0.2](y) - 2026-11-21\n\n### Added\n\n- a thing\n\n## [0.0.1](x) - 2026-11-20\n\nFirst.\n",
  );
  expect(sectionOf(out, "0.0.2")).toBe("### Added\n\n- a thing");
  expect(sectionOf(out, "0.0.1")).toBe("First.");
  expect(sectionOf(out, "0.0.9")).toBeUndefined();
  // A changelog that ends at its Unreleased heading.
  expect(prependSection("# Changelog\n\n## [Unreleased]\n", section)).toBe(
    `# Changelog\n\n## [Unreleased]\n\n${section}`,
  );
});

test("sets the package's version, not a dependency's", () => {
  const toml =
    '[package]\nname = "luminescent-desktop"\nversion = "0.0.1"\nedition.workspace = true\n\n[dependencies]\nfoo = { version = "0.0.1" }\n';
  expect(cargoVersion(toml)).toBe("0.0.1");
  const bumped = setCargoVersion(toml, "0.0.2");
  expect(cargoVersion(bumped)).toBe("0.0.2");
  expect(bumped).toContain('foo = { version = "0.0.1" }');
});

test("sets a workspace member's version in Cargo.lock", () => {
  const lock =
    '[[package]]\nname = "luminescent-desktop"\nversion = "0.0.1"\ndependencies = [\n "x",\n]\n\n[[package]]\nname = "luminescent-desktop-x"\nversion = "0.0.1"\n';
  const out = setLockVersion(lock, "luminescent-desktop", "0.0.2");
  expect(out).toContain('name = "luminescent-desktop"\nversion = "0.0.2"');
  expect(out).toContain('name = "luminescent-desktop-x"\nversion = "0.0.1"');
  expect(() => setLockVersion(lock, "missing", "1.0.0")).toThrow();
});
