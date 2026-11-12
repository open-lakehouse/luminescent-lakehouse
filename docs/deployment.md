# Deployment

The functions run on Neon project `cool-poetry-28343550`. Each environment is a
Neon branch with its own functions, its own Postgres and its own URLs, so a
deploy to one never touches another.

| Environment | Neon branch | Deployed by | When |
|---|---|---|---|
| Preview | `preview/pr-<n>`, off `staging` | [`preview.yml`](../.github/workflows/preview.yml) | every push to a same-repo PR, one run at a time per PR; deleted when the PR closes, after any deploy still running |
| Staging | `staging`, off production | [`deploy.yml`](../.github/workflows/deploy.yml) | every push to `main` |
| Production | the project's default branch (protected) | [`production.yml`](../.github/workflows/production.yml) | each release (merging the release PR), or a manual run on `main`; approved in the `production` environment either way |

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

## Releases

A release is a version of what is deployed: the functions, their migrations,
`neon.ts` and the web app's pages. [release-plz](https://release-plz.dev)
([`release-plz.toml`](../release-plz.toml)) versions it through
`luminescent-lakehouse`, a placeholder package in the root
[`Cargo.toml`](../Cargo.toml) whose `include` lists the files no crate owns.
Commits to those files or to the four crates count toward the next release.
Commits that touch only `apps/desktop`, the docs or the workflows don't count.

On each push to `main`, [`deploy.yml`](../.github/workflows/deploy.yml) runs:

1. **staging**, as above;
2. **release**: if the commit's version has no `v<version>` tag yet, release-plz
   tags it and creates the GitHub Release from [`CHANGELOG.md`](../CHANGELOG.md).
   Otherwise this step does nothing. Only a commit that staged gets tagged;
3. **production**, only for a release: [`production.yml`](../.github/workflows/production.yml)
   with the released commit, waiting for a reviewer's approval in the
   `production` environment;
4. **release PR**: release-plz opens or updates the `chore: release v<next>` PR,
   which bumps the version and adds the commits since the last release to
   `CHANGELOG.md`. Review the changelog there. It's opened with the default
   token, so no checks run on it (GitHub runs no workflows for that token's PRs);
   push an empty commit to its branch if you want CI on it.

So shipping to production means merging the release PR and approving the
production run. The conventional commit types set the version bump, and release
notes skip `chore`, `ci` and `test` commits. Service releases are never marked as
GitHub's "latest" release, which is kept for the desktop app's updater.

If staging fails, nothing is tagged. Fix it and push again, and the release goes
out with that push.

### Release assets

Each release carries what it takes to deploy it without building anything,
from another repository and another Neon account
([#108](https://github.com/open-lakehouse/luminescent-lakehouse/issues/108)). `deploy.yml`'s
`assets` job attaches them after tagging:

| Asset | What |
|---|---|
| `<fn>-<version>-linux-arm64.zip` | Each `neon.ts` function's `dist/`: the bundle that staged in the same run, as `neon deploy` ships it (unbundled) |
| `luminescent-<version>-<target>.tar.gz` | `luminescent-migrate` and `luminescent-admin` for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` (glibc 2.28 or newer) and `aarch64-apple-darwin`. The migrations are embedded, so they apply the schema the release's functions check |
| `luminescent-web-<version>.tar.gz` | The self-hostable auth pages ([web.md](web.md#self-hosting)) |
| `manifest.json` | Per function: its asset and sha256, `env`, secrets, database, whether it migrates, OAuth resource and triggers; the CLI tarballs by target; the web tarball; the upstream pins and the commit |
| `SHA256SUMS` | All of the above, for `sha256sum -c` |

Every asset has a build provenance attestation:
`gh attestation verify <file> --repo open-lakehouse/luminescent-lakehouse`.

The manifest takes each fact from one place: the functions, their `env` and
triggers from `neon.ts`, the OAuth resources from `TENANT.resources`, and
`SECRETS`, `DATABASE` and `NEEDS_PG` from
[`scripts/function.sh`](../scripts/function.sh). Its `schema` only changes
for a change a reader has to handle. To assemble the assets locally, after
`just build <fn>` for each function (and optionally
`just cli-archive <version> <target>`):

```bash
just release-assets <version>    # into target/release-assets/
```

## Promoting to production by hand

Run **production** from the Actions tab on `main`. It deploys the head of
`main`, or the commit you give it, but only if that commit has a successful
staging deployment. A reviewer approves the run in the `production`
environment. Use this to ship between releases, and to roll back.

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
unitycatalog needs `UC_SERVER_KEK`, its key-encryption key; see
[its README](../crates/unitycatalog-neon/README.md#secrets). opensharing will
(`SHARING_SERVER_CONFIG_YAML`, `SHARING_PROVIDER_TOKEN`, its KEKs); set them on
staging and production before adding it to `neon.ts`.

A branch created from another copies the parent's functions, with their
environment, so previews (and branches `trestle up --neon-branch new` creates)
get staging's secrets. A branch the function is added to later doesn't: set
the secret there by hand.

## Deploying by hand

```bash
bun install
bunx neon login
just deploy <branch>        # e.g. a personal dev branch; asks before a protected one
```

## Adding a service

Host it as a crate (see [AGENTS.md](../AGENTS.md)), make sure its handler
serves `GET /health`, set its secrets on each branch, and add it to the
`functions` of `neon.ts`, with its OAuth resource in `TENANT.resources`. The deploy
workflows pick it up from there.

## One-time setup

- The Neon GitHub integration, connected to this repository.
- The Neon Auth production checklist ([auth.md](auth.md#production-checklist)).
- The `preview`, `staging` and `production` environments in the repository
  settings. `production` requires a reviewer and only admits `main`; `staging`
  only admits `main`.
- "Allow GitHub Actions to create and approve pull requests" in the repository's
  Actions settings, for the release PR.
- The first staging deploy creates `staging`; previews need it to exist.
