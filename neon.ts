// The Neon branch policy: which functions every environment branch runs, and
// how each kind of branch is tuned. `neon deploy --branch <name>` applies it,
// after scripts/migrate.sh has migrated the branch (`just deploy <branch>`
// locally, `.github/workflows/neon-deploy.yml` in CI); see docs/deployment.md.
//
// Each function ships the prebuilt `dist/` from `just build <fn>` as-is
// (`bundler: "none"`): the native addon can't go through esbuild. Adding a
// service here is what makes CI build and deploy it (scripts/deployed.mjs).
//
// Function `env` is for non-secret tuning only. A secret is never declared
// here: `neon deploy` leaves undeclared keys alone, so secrets are set once
// per branch on Neon and never pass through GitHub.
//
// Neon Auth (Managed Better Auth) is on for every branch. `auth: true` is all
// `neon.ts` can say about it; `authSetup` below is the rest, which
// scripts/auth-setup.ts applies after each deploy (see docs/auth.md).

import { defineConfig } from "@neon/config/v1";

export default defineConfig({
  auth: true,

  functions: {
    headwaters: {
      name: "headwaters (OpenLineage)",
      source: "crates/headwaters-neon/function/dist",
      bundler: "none",
    },
  },

  triggers: {
    // Folds what an evicted isolate left unprojected (headwaters ADR 0016).
    // A cadence under Neon's 5 min scale-to-zero window keeps the branch
    // compute awake; see crates/headwaters-neon/README.md, "Choosing the
    // cadence".
    "headwaters-projection": {
      type: "schedule",
      function: "headwaters",
      cron: "*/10 * * * *",
      functionPath: "/internal/projection/run",
    },
  },

  branch: (branch) => {
    if (branch.isDefault) {
      // Production. Children of a protected branch get fresh Postgres
      // credentials, so staging and previews never hold production's.
      return { protected: true };
    }
    if (branch.name.startsWith("preview/")) {
      // Per-PR branches (preview.yml, created from staging) are deleted when
      // the PR closes; the TTL is the backstop. Keyed on the name rather than
      // `!branch.exists`: CI creates the branch before it deploys.
      return {
        ttl: "7d",
        postgres: {
          computeSettings: {
            autoscalingLimitMinCu: 0.25,
            autoscalingLimitMaxCu: 1,
            suspendTimeout: "5m",
          },
        },
      };
    }
    return {};
  },
});

/** What scripts/auth-setup.ts makes true on a branch's Neon Auth. */
export interface AuthSetup {
  /**
   * The OAuth protected resource of each function, by slug: the `aud` of the
   * tokens it accepts. An identifier, so it needn't resolve, and the same on
   * every branch (each branch has its own issuer and signing key). Every
   * deployed function needs one.
   */
  resources: Record<string, string>;
  /** Origins Neon Auth may redirect to: the web app and its login and consent pages. */
  trustedDomains: string[];
  /**
   * The OAuth server for the CLI, MCP and partner apps. It can only be enabled
   * with a login and a consent page to send users to, so it stays off until the
   * web app hosts them.
   */
  oauthServer?: { loginPage: string; consentPage: string; mcpResource?: string };
  /**
   * The public client the CLI signs in with: authorization code + PKCE with a
   * loopback redirect, which has to match exactly, port included. Created once
   * the OAuth server is on; child branches inherit it, client id included.
   */
  cliClient: { name: string; redirectUris: string[] };
}

export function authSetup(_branch: { name: string; isDefault: boolean }): AuthSetup {
  return {
    resources: {
      headwaters: "https://headwaters.luminescent-lakehouse.dev",
      opensharing: "https://opensharing.luminescent-lakehouse.dev",
    },
    trustedDomains: [],
    cliClient: { name: "luminescent CLI", redirectUris: ["http://127.0.0.1:8765/callback"] },
  };
}
