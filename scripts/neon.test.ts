// neon.ts's Neon Auth settings: `bun test ./scripts` (`just tooling-test`).

import { describe, expect, test } from "bun:test";
import config, { authPages, authSetup, TENANT, type Tenant, unityCatalogConfig, vending } from "../neon.ts";

const AUTH_URL = "https://ep-cool-darkness-a1b2c3d4.neonauth.c-7.eu-central-1.aws.neon.tech/neondb/auth";
const PAGES =
  "https://luminescent-lakehouse.vercel.app/a/ep-cool-darkness-a1b2c3d4.neonauth.c-7.eu-central-1.aws.neon.tech/neondb/auth";

describe("authPages", () => {
  test("is /a/ plus the Auth URL without the scheme", () => {
    expect(authPages(TENANT.webOrigin, AUTH_URL)).toBe(PAGES);
    expect(authPages(`${TENANT.webOrigin}/`, `${AUTH_URL}/`)).toBe(PAGES);
  });

  test("refuses what isn't a Neon Auth URL", () => {
    for (const bad of [
      "http://ep-x.neonauth.c-7.eu-central-1.aws.neon.tech/neondb/auth",
      "https://ep-x.neonauth.c-7.eu-central-1.aws.neon.tech:8443/neondb/auth",
      "https://ep-x.neonauth.c-7.eu-central-1.aws.neon.tech/neondb",
      "https://ep-x.neonauth.c-7.eu-central-1.aws.neon.tech/a/b/auth",
    ]) {
      expect(() => authPages(TENANT.webOrigin, bad)).toThrow("not a Neon Auth URL");
    }
  });
});

describe("authSetup", () => {
  const branch = { name: "staging", isDefault: false, authUrl: AUTH_URL };

  test("points the OAuth server at this lakehouse's pages for the branch", () => {
    const setup = authSetup(branch);
    expect(setup.oauthServer).toEqual({
      loginPage: `${PAGES}/sign-in`,
      consentPage: `${PAGES}/consent`,
    });
    expect(setup.trustedDomains).toEqual([TENANT.webOrigin]);
    expect(setup.resources).toBe(TENANT.resources);
  });

  test("another lakehouse brings its own origin, resources and client", () => {
    const theirs: Tenant = {
      webOrigin: "https://auth.example.com",
      resources: { headwaters: "https://lineage.example.com" },
      cliClient: { name: "example CLI", redirectUris: ["http://127.0.0.1:9000/cb"] },
      neonProjectId: "example-project-12345678",
    };
    const setup = authSetup(branch, theirs);
    expect(setup.oauthServer?.loginPage).toStartWith("https://auth.example.com/a/ep-cool-darkness-a1b2c3d4.");
    expect(setup.trustedDomains).toEqual(["https://auth.example.com"]);
    expect(setup.resources).toEqual(theirs.resources);
    expect(setup.cliClient).toEqual(theirs.cliClient);
  });
});

describe("unitycatalog", () => {
  const yaml = config.functions?.unitycatalog?.env?.UC_SERVER_CONFIG_YAML ?? "";

  test("authenticates callers for its own OAuth resource", () => {
    const resource = JSON.stringify(TENANT.resources.unitycatalog);
    expect(yaml).toContain("mode: olai-auth");
    expect(yaml).toContain(`audiences: [${resource}]`);
    expect(yaml).toContain(`resource: ${resource}`);
    expect(yaml).not.toContain("anonymous");
  });

  test("asks the branch's authz function, with its secret", () => {
    expect(yaml).toContain("kind: authorizer");
    expect(yaml).toContain("url: { env: AUTHZ_URL }");
    expect(yaml).toContain("token: { env: AUTHZ_TOKEN_UNITYCATALOG }");
    expect(yaml).not.toContain("constant");
  });

  test("reads the identity tables from the branch's neondb, not its own database", () => {
    expect(yaml).toContain("database-url: { env: DATABASE_URL }");
  });
});

describe("vending", () => {
  test("is on for staging and previews, with a shorter TTL on previews", () => {
    expect(vending("staging")).toEqual({ enabled: true, ttlSecs: 3600 });
    expect(vending("preview/pr-42")).toEqual({ enabled: true, ttlSecs: 900 });
  });

  test("is off for production, any other branch, and a deploy that names none", () => {
    for (const name of ["production", "main", "demo", "staging-2", "previews/x", "", undefined]) {
      expect(vending(name).enabled).toBe(false);
    }
  });

  test("lands in the catalog's config, with the project and the branch's own id", () => {
    const yaml = unityCatalogConfig(vending("preview/pr-1"));
    expect(yaml).toContain("vending:\n  neon:\n    enabled: true\n");
    expect(yaml).toContain(`project-id: "${TENANT.neonProjectId}"`);
    expect(yaml).toContain("api-key: { env: NEON_API_KEY }");
    expect(yaml).toContain("branch-id: { env: NEON_BRANCH_ID }");
    expect(yaml).toContain("ttl-secs: 900");
    expect(yaml).toContain("managed_storage_root: s3://lakehouse/managed");
    expect(yaml).toContain("managed_storage_root_credential: neon");
  });

  test("the deployed document follows LUMINESCENT_NEON_BRANCH, off when unset", () => {
    const yaml = config.functions?.unitycatalog?.env?.UC_SERVER_CONFIG_YAML ?? "";
    expect(yaml).toBe(unityCatalogConfig(vending(process.env.LUMINESCENT_NEON_BRANCH)));
    if (!process.env.LUMINESCENT_NEON_BRANCH) expect(yaml).toContain("enabled: false");
  });

  test("the bucket the managed root names is declared", () => {
    expect(Object.keys(config.buckets ?? {})).toContain("lakehouse");
  });
});
