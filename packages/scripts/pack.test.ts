import { describe, expect, test } from "bun:test";
import { packedProblems, stamp } from "./pack.js";

const siblings = new Set(["@open-lakehouse/factory-client", "@open-lakehouse/factory-ui"]);

const ui = {
  name: "@open-lakehouse/factory-ui",
  version: "0.0.0",
  exports: {
    ".": { "factory-source": "./src/index.ts", types: "./src/index.ts", import: "./dist/index.js" },
  },
  scripts: { build: "bun ../scripts/build-package.ts" },
  dependencies: {
    "@open-lakehouse/factory-client": "workspace:*",
    "@tanstack/react-query": "^5.105.0",
    "@xyflow/react": "^12.4.0",
  },
  peerDependencies: { react: "^19.0.0" },
  devDependencies: { "@open-lakehouse/ui-kit": "https://example.invalid/ui-kit.tgz" },
};

describe("stamping a package for its tarball", () => {
  test("pins the version, moves the consumer's libraries to peers, exports dist only", () => {
    const out = stamp("factory-ui", ui, "0.1.2", siblings);
    expect(out.version).toBe("0.1.2");
    expect(out.dependencies).toEqual({ "@xyflow/react": "^12.4.0" });
    expect(out.peerDependencies).toEqual({
      "@open-lakehouse/factory-client": "0.1.2",
      "@tanstack/react-query": "^5.0.0",
      react: "^19.0.0",
    });
    expect(out.exports).toEqual({ ".": { types: "./dist/index.d.ts", import: "./dist/index.js" } });
    expect(out.devDependencies).toBeUndefined();
    expect(out.scripts).toBeUndefined();
    expect(packedProblems(out)).toEqual([]);
  });

  test("pins a workspace dependency that stays a dependency", () => {
    const pkg = { ...ui, dependencies: { ...ui.dependencies } };
    const out = stamp("other", pkg, "0.1.2", siblings);
    expect(out.dependencies?.["@open-lakehouse/factory-client"]).toBe("0.1.2");
  });

  test("an unstamped manifest is caught", () => {
    expect(packedProblems(ui)).toEqual([
      "dependencies @open-lakehouse/factory-client@workspace:*",
      "export . → ./src/index.ts",
      "export . → ./src/index.ts",
      "version 0.0.0",
    ]);
  });

  test("a workspace dependency on something unreleased fails", () => {
    const pkg = { ...ui, dependencies: { "@open-lakehouse/other": "workspace:*" } };
    expect(() => stamp("other", pkg, "0.1.2", siblings)).toThrow("isn't released");
  });
});
