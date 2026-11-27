// Monaco loads only on demand: from the package's entry, every path to it
// goes through a dynamic `import()`, which a host's bundler splits into a chunk
// of its own (docs/factory/packages.md). A static import anywhere on the way
// would put Monaco in the host's first load.

import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const MONACO = ["monaco-editor", "@monaco-editor/react", "@open-lakehouse/editor"];

const transpiler = new Bun.Transpiler({ loader: "tsx" });

/** The module `spec` names from `from`, if it's one of ours. */
function local(from: string, spec: string): string | undefined {
  if (!spec.startsWith(".")) return undefined;
  const path = resolve(dirname(from), spec);
  for (const p of [path.replace(/\.js$/, ".ts"), path.replace(/\.js$/, ".tsx")]) {
    try {
      readFileSync(p);
      return p;
    } catch {}
  }
  throw new Error(`${from}: can't resolve ${spec}`);
}

/** Every bare import reachable from `entry` through static imports only,
 * with the chain that reaches it. */
function staticReach(entry: string): Map<string, string[]> {
  const out = new Map<string, string[]>();
  const seen = new Set<string>();
  const walk = (file: string, chain: string[]) => {
    if (seen.has(file)) return;
    seen.add(file);
    for (const imp of transpiler.scanImports(readFileSync(file, "utf8"))) {
      if (imp.kind === "dynamic-import") continue;
      const next = local(file, imp.path);
      if (next) walk(next, [...chain, next]);
      else if (!out.has(imp.path)) out.set(imp.path, chain);
    }
  };
  walk(entry, [entry]);
  return out;
}

describe("Monaco's chunk", () => {
  const src = join(import.meta.dir, "..");

  test("no static import from the entry reaches Monaco", () => {
    const reach = staticReach(join(src, "index.ts"));
    const hits = MONACO.filter((m) =>
      [...reach.keys()].some((k) => k === m || k.startsWith(`${m}/`)),
    );
    expect(hits.map((h) => `${h} via ${reach.get(h)?.join(" → ")}`)).toEqual([]);
    // The check sees through the package: React Flow is a static import.
    expect(reach.has("@xyflow/react")).toBe(true);
  });

  test("the editors themselves do import it", () => {
    const reach = staticReach(join(src, "monaco", "MarkdownEditor.tsx"));
    expect(reach.has("@monaco-editor/react")).toBe(true);
  });
});
