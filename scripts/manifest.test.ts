// The release manifest: `bun test ./scripts` (`just tooling-test`).

import { describe, expect, test } from "bun:test";
import config, { INTERNAL_FUNCTIONS, TENANT } from "../neon.ts";
import { type Asset, buildManifest, listed, type ManifestInput, sums } from "./manifest.ts";

const asset = (name: string): Asset => ({ name, sha256: "ab".repeat(32), size: 1 });

const input = (over: Partial<ManifestInput> = {}): ManifestInput => ({
  version: "1.2.3",
  commit: "c0ffee",
  functions: {
    headwaters: { name: "headwaters (OpenLineage)" },
    unitycatalog: { env: { UC_SERVER_CONFIG_YAML: "ui: { serve: false }" } },
  },
  triggers: {
    "headwaters-projection": { function: "headwaters", cron: "*/10 * * * *" } as never,
  },
  resources: {
    headwaters: "https://headwaters.example",
    unitycatalog: "https://uc.example",
  },
  internal: new Set(),
  facts: {
    headwaters: { secrets: [], database: "neondb", migrate: true },
    unitycatalog: { secrets: ["UC_SERVER_KEK"], database: "unitycatalog", migrate: true },
  },
  dependencies: {
    headwaters: { git: "https://github.com/open-lakehouse/headwaters", rev: "1eee" },
    "unitycatalog-server": {
      package: "olai-uc-server",
      git: "https://github.com/open-lakehouse/mangrove",
      rev: "c22b",
      "default-features": false,
    },
    axum: "0.8",
    tracing: { version: "0.1" },
  },
  assets: [
    asset("headwaters-1.2.3-linux-arm64.zip"),
    asset("unitycatalog-1.2.3-linux-arm64.zip"),
    asset("luminescent-1.2.3-aarch64-apple-darwin.tar.gz"),
    asset("luminescent-1.2.3-x86_64-unknown-linux-gnu.tar.gz"),
    asset("luminescent-web-1.2.3.tar.gz"),
    asset("luminescent-1.2.2-aarch64-apple-darwin.tar.gz"),
  ],
  ...over,
});

describe("buildManifest", () => {
  test("describes each function from its sources", () => {
    const m = buildManifest(input());
    expect(m.schema).toBe(1);
    expect(m.version).toBe("1.2.3");
    expect(m.commit).toBe("c0ffee");
    expect(m.functions.headwaters).toEqual({
      name: "headwaters (OpenLineage)",
      asset: asset("headwaters-1.2.3-linux-arm64.zip"),
      env: {},
      secrets: [],
      database: "neondb",
      migrate: true,
      oauthResource: "https://headwaters.example",
      triggers: { "headwaters-projection": { function: "headwaters", cron: "*/10 * * * *" } },
    });
    expect(m.functions.unitycatalog?.env).toEqual({ UC_SERVER_CONFIG_YAML: "ui: { serve: false }" });
    expect(m.functions.unitycatalog?.secrets).toEqual(["UC_SERVER_KEK"]);
    expect(m.functions.unitycatalog?.triggers).toEqual({});
  });

  test("lists the CLI by target and the web tarball, of this version only", () => {
    const m = buildManifest(input());
    expect(Object.keys(m.cli).sort()).toEqual(["aarch64-apple-darwin", "x86_64-unknown-linux-gnu"]);
    expect(m.web?.name).toBe("luminescent-web-1.2.3.tar.gz");
  });

  test("names every asset of the release, and no other version's", () => {
    expect(listed(buildManifest(input())).map((a) => a.name).sort()).toEqual([
      "headwaters-1.2.3-linux-arm64.zip",
      "luminescent-1.2.3-aarch64-apple-darwin.tar.gz",
      "luminescent-1.2.3-x86_64-unknown-linux-gnu.tar.gz",
      "luminescent-web-1.2.3.tar.gz",
      "unitycatalog-1.2.3-linux-arm64.zip",
    ]);
  });

  test("keeps only the git-pinned upstreams", () => {
    expect(buildManifest(input()).upstreams).toEqual({
      headwaters: { git: "https://github.com/open-lakehouse/headwaters", rev: "1eee" },
      "unitycatalog-server": {
        git: "https://github.com/open-lakehouse/mangrove",
        rev: "c22b",
        package: "olai-uc-server",
      },
    });
  });

  test("fails on a function without its bundle, facts or OAuth resource", () => {
    expect(() => buildManifest(input({ assets: [] }))).toThrow("headwaters-1.2.3-linux-arm64.zip");
    expect(() => buildManifest(input({ facts: {} }))).toThrow("scripts/function.sh");
    expect(() => buildManifest(input({ resources: {} }))).toThrow("TENANT.resources");
  });

  test("an internal function has no OAuth resource", () => {
    const m = buildManifest(
      input({
        functions: { headwaters: {}, authz: {} },
        internal: new Set(["authz"]),
        facts: {
          headwaters: { secrets: [], database: "neondb", migrate: true },
          authz: { secrets: ["AUTHZ_TOKEN_UNITYCATALOG"], database: "neondb", migrate: true },
        },
        assets: [asset("headwaters-1.2.3-linux-arm64.zip"), asset("authz-1.2.3-linux-arm64.zip")],
      }),
    );
    expect(m.functions.authz).not.toHaveProperty("oauthResource");
    expect(m.functions.authz?.secrets).toEqual(["AUTHZ_TOKEN_UNITYCATALOG"]);
    expect(m.functions.headwaters?.oauthResource).toBe("https://headwaters.example");
  });

  test("covers every function neon.ts deploys", () => {
    const slugs = Object.keys(config.functions ?? {});
    const assets = slugs.map((s) => asset(`${s}-1.2.3-linux-arm64.zip`));
    const facts = Object.fromEntries(slugs.map((s) => [s, { secrets: [], database: "neondb", migrate: false }]));
    const m = buildManifest(
      input({
        functions: config.functions as ManifestInput["functions"],
        triggers: (config.triggers ?? {}) as ManifestInput["triggers"],
        resources: TENANT.resources,
        internal: INTERNAL_FUNCTIONS,
        facts,
        assets,
      }),
    );
    expect(Object.keys(m.functions)).toEqual(slugs);
    expect(m.functions.headwaters?.triggers).toHaveProperty("headwaters-projection");
  });
});

test("sums are sha256sum's format, sorted by name", () => {
  expect(sums([asset("b"), asset("a")])).toBe(`${"ab".repeat(32)}  a\n${"ab".repeat(32)}  b\n`);
});
