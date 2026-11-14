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

/**
 * What makes a lakehouse this one, rather than another deployed from its own
 * repository: the origin of the web pages its OAuth server sends users to, the
 * OAuth resource identifiers, and the CLI's client. A deployer sets their own
 * here (docs/web.md, "Self-hosting"); the rest of `authSetup` follows from it.
 */
export interface Tenant {
  /**
   * The origin serving apps/web's pages: the shared deployment, or one of the
   * deployer's (the release's `luminescent-web` tarball on any static host).
   * The pages keep nothing; the users and their sessions stay in each
   * branch's Neon Auth.
   */
  webOrigin: string;
  /**
   * The OAuth protected resource of each function, by slug: the `aud` of the
   * tokens it accepts. An identifier, so it needn't resolve. Every branch, and
   * every lakehouse, has its own issuer, so sharing these is safe.
   */
  resources: Record<string, string>;
  /** The CLI's public client (see `AuthSetup.cliClient`). */
  cliClient: { name: string; redirectUris: string[] };
  /**
   * The Neon project the branches are in. Unity Catalog's credential vending
   * needs it (`vending.neon.project-id`): Neon Functions get their branch id,
   * but not their project's.
   */
  neonProjectId: string;
}

/**
 * Functions only the other functions call, with a per-service secret: no
 * OAuth resource, and no user ever holds a token for them.
 */
export const INTERNAL_FUNCTIONS: ReadonlySet<string> = new Set(["authz"]);

/** This repository's lakehouse. */
export const TENANT: Tenant = {
  webOrigin: "https://luminescent-lakehouse.vercel.app",
  resources: {
    headwaters: "https://headwaters.luminescent-lakehouse.dev",
    opensharing: "https://opensharing.luminescent-lakehouse.dev",
    unitycatalog: "https://unitycatalog.luminescent-lakehouse.dev",
  },
  cliClient: { name: "luminescent CLI", redirectUris: ["http://127.0.0.1:8765/callback"] },
  neonProjectId: "cool-poetry-28343550",
};

/**
 * The bucket every branch has for its tables: Unity Catalog's managed storage
 * root and the external location the deploy registers over it
 * (scripts/storage-seed.ts). A child branch inherits its parent's objects,
 * copy-on-write.
 */
export const LAKEHOUSE_BUCKET = "lakehouse";

/** Unity Catalog's credential for the branch's storage (scripts/storage-seed.ts). */
export const STORAGE_CREDENTIAL = "neon";

/** Trusted Neon credential vending on one branch: `vending.neon` of the catalog's config. */
export interface Vending {
  enabled: boolean;
  /** How long a vended key lives (900 to 43200). Revocation lags, so this is the control. */
  ttlSecs: number;
}

/**
 * Whether the `unitycatalog` function vends Neon storage keys on a branch
 * (docs/auth.md, "Credential vending on Neon"). A key reaches all of the
 * branch's storage, so it's on only where that is meant: staging, and the
 * previews off it, with a short TTL so expired keys show up across runs.
 * Production, and a deploy that doesn't name its branch, stay off.
 *
 * Function env can't vary per branch in neon.ts (the `branch` closure only
 * tunes runtimes and domains), so the deploy names the branch it targets in
 * LUMINESCENT_NEON_BRANCH (neon-deploy.yml, `just deploy`).
 */
export function vending(branchName: string | undefined): Vending {
  if (branchName === "staging") return { enabled: true, ttlSecs: 3600 };
  if (branchName?.startsWith("preview/")) return { enabled: true, ttlSecs: 900 };
  return { enabled: false, ttlSecs: 3600 };
}

/**
 * The Unity Catalog server's configuration document (UC_SERVER_CONFIG_YAML)
 * for a branch. The same document trestle deploys `unity-catalog-rs` with
 * (stack-topology's templates/modules/unity-catalog-rs/uc-server.yaml,
 * `NeonRequirements.functions[].env`); keep the two in step.
 */
export function unityCatalogConfig(v: Vending, tenant: Tenant = TENANT): string {
  return [
    `managed_storage_root: s3://${LAKEHOUSE_BUCKET}/managed`,
    `managed_storage_root_credential: ${STORAGE_CREDENTIAL}`,
    "ui: { serve: false }",
    "auth:",
    "  mode: olai-auth",
    "  olai:",
    "    issuer: { env: NEON_AUTH_BASE_URL }",
    "    jwks-url: { env: NEON_AUTH_JWKS_URL }",
    `    audiences: [${JSON.stringify(tenant.resources.unitycatalog)}]`,
    "    accept-session-tokens: true",
    "    database-url: { env: DATABASE_URL }",
    `    resource: ${JSON.stringify(tenant.resources.unitycatalog)}`,
    "policy:",
    "  kind: authorizer",
    "  url: { env: AUTHZ_URL }",
    "  token: { env: AUTHZ_TOKEN_UNITYCATALOG }",
    "encryption:",
    "  active: { id: kek-1, key: { env: UC_SERVER_KEK } }",
    "vending:",
    "  neon:",
    `    enabled: ${v.enabled}`,
    `    project-id: ${JSON.stringify(tenant.neonProjectId)}`,
    "    api-key: { env: NEON_API_KEY }",
    "    branch-id: { env: NEON_BRANCH_ID }",
    `    ttl-secs: ${v.ttlSecs}`,
  ].join("\n");
}

export default defineConfig({
  auth: true,

  buckets: {
    // Private: only the functions' own key and vended keys reach it.
    [LAKEHOUSE_BUCKET]: {},
  },

  functions: {
    headwaters: {
      name: "headwaters (OpenLineage)",
      source: "crates/headwaters-neon/function/dist",
      bundler: "none",
    },
    authz: {
      // The Cedar policy decision point the other functions ask
      // (crates/authz-neon). No secret here: each calling service's
      // AUTHZ_TOKEN_<SERVICE> is set once per branch on Neon
      // (docs/deployment.md, "Secrets"); with none set, it answers every
      // request with a 401.
      name: "authz (Cedar PDP)",
      source: "crates/authz-neon/function/dist",
      bundler: "none",
    },
    unitycatalog: {
      name: "Unity Catalog",
      source: "crates/unitycatalog-neon/function/dist",
      bundler: "none",
      env: {
        // No secret here: the key-encryption key (`openssl rand -base64 32`)
        // is UC_SERVER_KEK, set once per branch on Neon. Child branches copy
        // it with the function. The catalog lives in the branch's
        // `unitycatalog` database (crates/unitycatalog-neon/README.md).
        //
        // Callers authenticate with the branch's Neon Auth (an OAuth access
        // token for the `unitycatalog` resource, or a web session token) or an
        // `olk_` API key from `authz.api_keys` (docs/auth.md). Admin and ban
        // status come from `neon_auth."user"`. Those tables are in `neondb`,
        // so `database-url` names the branch's own DATABASE_URL: the addon
        // points only the catalog's backend at `unitycatalog`, never the
        // variable. The function connects as `neondb_owner` until the
        // least-privilege role (#26).
        //
        // The `authz` function decides every request (policies in
        // crates/authz-neon/function/policies/unitycatalog.cedar). AUTHZ_URL
        // is that branch's own `authz`, which function/index.ts derives from
        // the request's host unless it is set; AUTHZ_TOKEN_UNITYCATALOG is a
        // secret, the same value on both functions (docs/deployment.md).
        //
        // With vending on (staging and previews, `vending` above), it also
        // vends Neon storage keys to callers holding EXTERNAL USE, with
        // NEON_API_KEY, a project-scoped Neon API key set once per branch
        // like the KEK (docs/deployment.md, "Secrets").
        UC_SERVER_CONFIG_YAML: unityCatalogConfig(vending(process.env.LUMINESCENT_NEON_BRANCH)),
      },
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
  /** `Tenant.resources`: every deployed function needs one, except `INTERNAL_FUNCTIONS`. */
  resources: Record<string, string>;
  /** Origins Neon Auth may redirect to: the web app and its login and consent pages. */
  trustedDomains: string[];
  /**
   * The OAuth server for the CLI, MCP and partner apps. It can only be enabled
   * with a login and a consent page to send users to: the web app's pages for
   * this branch (apps/web, docs/web.md).
   */
  oauthServer?: { loginPage: string; consentPage: string; mcpResource?: string };
  /**
   * The public client the CLI signs in with: authorization code + PKCE with a
   * loopback redirect, which has to match exactly, port included. Created once
   * the OAuth server is on; child branches inherit it, client id included.
   */
  cliClient: { name: string; redirectUris: string[] };
}

/**
 * Where the web app serves a branch's auth pages: `/a/` + its Auth URL
 * without the scheme (`/a/<auth-host>/<db>/auth`), so one deployment serves
 * every branch, of any project, in any region (docs/web.md).
 */
export function authPages(webOrigin: string, authUrl: string): string {
  const url = new URL(authUrl);
  const path = url.pathname.replace(/\/+$/, "");
  if (url.protocol !== "https:" || url.port || !/^\/[A-Za-z0-9_-]+\/auth$/.test(path)) {
    throw new Error(`not a Neon Auth URL: ${authUrl}`);
  }
  return `${webOrigin.replace(/\/+$/, "")}/a/${url.host}${path}`;
}

/** `authUrl` is the branch's Neon Auth URL (`neon neon-auth status`'s `base_url`). */
export function authSetup(
  branch: { name: string; isDefault: boolean; authUrl: string },
  tenant: Tenant = TENANT,
): AuthSetup {
  const pages = authPages(tenant.webOrigin, branch.authUrl);
  return {
    resources: tenant.resources,
    trustedDomains: [tenant.webOrigin],
    oauthServer: { loginPage: `${pages}/sign-in`, consentPage: `${pages}/consent` },
    cliClient: tenant.cliClient,
  };
}
