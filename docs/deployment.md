# Deployment

The functions run on Neon project `cool-poetry-28343550`. Each environment is a
Neon branch with its own functions, its own Postgres and its own URLs, so a
deploy to one never touches another.

| Environment | Neon branch | Deployed by | When |
|---|---|---|---|
| Preview | `preview/pr-<n>`, off `staging` | [`preview.yml`](../.github/workflows/preview.yml) | every push to a same-repo PR; deleted when the PR closes |
| Staging | `staging`, off production | [`deploy.yml`](../.github/workflows/deploy.yml) | every push to `main` |
| Production | the project's default branch (protected) | [`production.yml`](../.github/workflows/production.yml) | a manual run on `main`, approved in the `production` environment |

[`neon.ts`](../neon.ts) says what each branch runs: the functions, their
triggers and the per-branch tuning (preview branches get a small compute and a
7 day TTL; production is protected). All three workflows call
[`neon-deploy.yml`](../.github/workflows/neon-deploy.yml), which:

1. builds the linux-arm64 bundle of every function in `neon.ts`;
2. creates the Neon branch if it doesn't exist yet;
3. migrates the branch ([`scripts/migrate.sh`](../scripts/migrate.sh), over its
   direct connection string, looked up at run time), then runs `neon deploy`,
   which deploys the functions and triggers;
4. applies the branch's Neon Auth settings
   ([`scripts/auth-setup.ts`](../scripts/auth-setup.ts); see [auth.md](auth.md));
5. checks `GET /health` on each deployed function. The handler starts the
   service, and checks its schema, before it answers, so a failed start fails
   the deploy.

The deploy job runs in the matching GitHub environment, so each PR and commit
links to its deployment. On previews, `preview.yml`'s `e2e` job then runs an
OAuth sign-in through the web app's pages for the branch, with a throwaway user
it deletes afterwards ([web.md](web.md#end-to-end-test)).

## Promoting to production

Run **production** from the Actions tab on `main`. It deploys the head of
`main`, or the commit you give it, but only if that commit has a successful
staging deployment. A reviewer approves the run in the `production`
environment.

To roll back, run it again with an earlier commit that staged cleanly. If the
data is the problem, Neon can
[restore the branch](https://neon.com/docs/postgres/backup-restore/branch-restore) to a
point in time. Migrations only move forward, so redeploying code that predates
a migration fails its schema check: restore the branch first.

## Secrets

GitHub holds one secret and one variable, both written by the
[Neon GitHub integration](https://neon.com/docs/guides/neon-github-integration):
`NEON_API_KEY` and `NEON_PROJECT_ID`. Neon creates the key and revokes it when
the integration is disconnected. No database URL is stored anywhere: Neon
injects `DATABASE_URL` and `DATABASE_URL_UNPOOLED` into each function, and the
migration step looks the direct URL up with the API key and masks it.

The key is account-scoped: it can reach every project on the account. So only
the deploy and cleanup jobs see it, every Neon call names the project, fork PRs
never deploy, and nothing runs under `pull_request_target`. If that is too
broad, create a project-scoped key, store it under a different name in the
three environments, and pass that to `neon-deploy.yml` instead.

A function's own secrets stay on Neon. `neon deploy` only writes the `env` keys
`neon.ts` declares and leaves the rest alone, so a secret is set once per
branch, in the Neon Console or with
`neon functions deploy <slug> --branch <branch> --src <dist> --no-bundle --env KEY=value`,
and is never declared in `neon.ts` or stored in GitHub. headwaters needs none.
opensharing will (`SHARING_SERVER_CONFIG_YAML`, `SHARING_PROVIDER_TOKEN`, its
KEKs); set them on staging and production before adding it to `neon.ts`.

## Deploying by hand

```bash
bun install
bunx neon login
just deploy <branch>        # e.g. a personal dev branch; asks before a protected one
```

## Adding a service

Host it as a crate (see [AGENTS.md](../AGENTS.md)), make sure its handler
serves `GET /health`, set its secrets on each branch, and add it to the
`functions` of `neon.ts`, with its OAuth resource in `authSetup`. The deploy
workflows pick it up from there.

## One-time setup

- The Neon GitHub integration, connected to this repository.
- The Neon Auth production checklist ([auth.md](auth.md#production-checklist)).
- The `preview`, `staging` and `production` environments in the repository
  settings. `production` requires a reviewer and only admits `main`; `staging`
  only admits `main`.
- The first staging deploy creates `staging`; previews need it to exist.
