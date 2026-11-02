// The Neon branch policy: which functions every environment branch runs, and
// how each kind of branch is tuned. `neon deploy --branch <name>` applies it
// (`just deploy <branch>` locally, `.github/workflows/neon-deploy.yml` in CI);
// see docs/deployment.md.
//
// Each function ships the prebuilt `dist/` from `just build <fn>` as-is
// (`bundler: "none"`): the native addon can't go through esbuild. Adding a
// service here is what makes CI build and deploy it (scripts/deployed.mjs).
//
// Function `env` is for non-secret tuning only. A secret is never declared
// here: `neon deploy` leaves undeclared keys alone, so secrets are set once
// per branch on Neon and never pass through GitHub.

import { defineConfig } from "@neon/config/v1";

export default defineConfig({
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

  experimental: {
    hooks: {
      // The functions never migrate; they only check the schema at startup.
      // So every deploy first migrates the branch, over the direct URL Neon
      // injects into the hook's environment.
      deploy: { before: "scripts/migrate.sh" },
    },
  },
});
