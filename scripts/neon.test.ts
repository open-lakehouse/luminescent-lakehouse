// neon.ts's Neon Auth settings: `bun test ./scripts` (`just tooling-test`).

import { describe, expect, test } from "bun:test";
import config, { authPages, authSetup, TENANT, type Tenant } from "../neon.ts";

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

  test("reads the identity tables from the branch's neondb, not its own database", () => {
    expect(yaml).toContain("database-url: { env: DATABASE_URL }");
  });
});
