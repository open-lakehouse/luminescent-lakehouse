# Model connectivity for environments

The agents inside an environment (Omnigent, notebooks, Claude Code pointed at
it) need models. None of them should hold a provider key. This document is the
design for where they get models from:

- through the environment's MLflow AI Gateway, which calls one **upstream**:
  provider keys, a **Databricks workspace** reached through one of the user's
  Databricks CLI profiles, or the **Neon AI Gateway** of the environment's
  branch;
- or, in an environment without MLflow, from Omnigent's host calling that
  upstream directly.

Status: design. Nothing here is built yet. The work is tracked in the issues
linked at the end.

The first design, merged in #102, assumed a Databricks service principal. The
Agent app (#150) ran into the problem with that right away. People have
Databricks CLI profiles, not service principals or Anthropic keys, and the demo
ran on MLflow endpoints made by hand around a token that expires in an hour.
The service principal is still an option for later ([Later](#later)).

## The gateway today

trestle's `mlflow` module (`crates/stack-topology/src/catalog/baseline.rs`,
image `ghcr.io/mlflow/mlflow:v3.18.0-full`) fronts MLflow's gateway at
`<gateway>/ai-gateway`. Clients get it as `MLFLOW_AI_GATEWAY_URL`:

| Surface | Path | Speaks |
|---|---|---|
| Anthropic | `/ai-gateway/anthropic/v1/messages` | Anthropic Messages |
| OpenAI | `/ai-gateway/openai/v1/{responses,chat/completions}` | OpenAI |
| Unified | `/ai-gateway/mlflow/v1/chat/completions` | OpenAI-compatible, any provider |

The one-shot `mlflow-gateway-init` container (`seed_gateway.py`) creates one
endpoint per `provider:model` pair in the `mlflow.ai_gateway_models` knob. It
names each endpoint after the model id, so a client asks for `claude-opus-5-5`
as it would upstream. It knows three providers, `anthropic`, `openai` and
`gemini`. Their keys reach the container as bare Compose pass-throughs from the
process that runs `docker compose up`, and MLflow stores them encrypted in its
database. The Envoy route drops whatever credential a client sends.

trestle's `omnigent` module requires `mlflow`. Its host container points the
harnesses at the gateway (`ANTHROPIC_BASE_URL=$MLFLOW_AI_GATEWAY_URL/anthropic`,
`OPENAI_BASE_URL=…/openai/v1`) with placeholder keys. Its knobs `model` and
`fast_model` pick the model ids.

The gaps:

- The module doesn't declare the provider keys as secrets, so the desktop's
  secret providers (process env, `trestle` keychain) never resolve them for the
  launch. A desktop launched from the Finder has none in its environment, and
  the gateway seeds nothing.
- MLflow encrypts the stored keys with a fixed passphrase
  (`MLFLOW_CRYPTO_KEK_PASSPHRASE=lakehouse-dev-kek` in `mlflow.env.jinja`), in a
  database that may sit on a Neon branch.
- No upstream other than personal provider keys.

## Upstreams

Each environment uses one upstream. The desktop's Start dialog and the
environment's settings show it as **Models**:

| Upstream | Credential | Lifetime | Who refreshes it |
|---|---|---|---|
| Provider keys | `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY` | long | nobody |
| Databricks profile | a profile in `~/.databrickscfg` (U2M) | about an hour | the Databricks SDK, from the desktop's token endpoint |
| Neon AI Gateway | the branch's `NEON_AI_GATEWAY_TOKEN` | long | nobody |

Whichever upstream is chosen, MLflow's endpoints keep the model ids clients
already use (`claude-opus-5-5`), so Omnigent's knobs and the agents don't change.

### Databricks profile

Most people with a Databricks workspace can use its models through Unity AI
Gateway and have signed in with the Databricks CLI (`databricks auth login`). The
profiles that produces are U2M (`auth_type: databricks-cli`), and their access
tokens last about an hour. The containers have neither the CLI nor its token
cache. So the host has to mint the tokens, and keep minting them for as long as
the environment runs. The desktop does that.

**Choosing a profile.** The desktop finds the `databricks` CLI the way it finds
the Neon CLI (`toolchain.rs`, the login-shell `PATH`). The Doctor shows it.
`databricks auth profiles --skip-validate -o json` lists the profiles (name,
host, auth type), and the user picks one for the environment. It is stored as
two literal variables, `DATABRICKS_CONFIG_PROFILE` and `DATABRICKS_HOST`. No
token is stored.

**The token endpoint.** The Databricks SDK has a credential strategy for exactly
this case, `metadata-service`. Given `DATABRICKS_HOST` and
`DATABRICKS_METADATA_SERVICE_URL`, it `GET`s the URL (headers
`X-Databricks-Metadata-Version: 1`, `X-Databricks-Host: <host>`) for
`{access_token, token_type, expires_on}`, and fetches it again before the token
expires. The desktop serves that URL for each running instance that uses a
profile. A request makes it run `databricks auth token -p <profile> -o json`,
which refreshes from the CLI's token cache when needed, and it answers with the
result.

- The URL carries a random path segment, minted per instance and never stored.
  It is bound to the address the containers reach the host on
  (`host.docker.internal`). A request whose `X-Databricks-Host` isn't the
  profile's host gets a 403.
- The endpoint lives as long as the instance. When the instance stops, the URL
  stops answering.
- When the CLI can't refresh (the refresh token expired or was revoked), the
  endpoint answers 503. The desktop shows "run `databricks auth login -p
  <profile>`" on the instance, and the Doctor shows it too.

**MLflow.** The pinned image's `databricks` provider
(`mlflow/gateway/providers/databricks.py`) builds a `WorkspaceClient` from only
the fields its secret sets. Fields it doesn't set fall back to the SDK's default
chain, the environment included, and `config.authenticate()` runs on every
request. So the seed stores a `databricks` secret with no token, and the
`mlflow` container gets `DATABRICKS_HOST` and `DATABRICKS_METADATA_SERVICE_URL`.
Then the SDK refreshes on its own, and nothing pushes tokens into MLflow. Two
details come from the trestle#361 comment:

- In 3.18, Unity Catalog names (`system.ai.claude-opus-5-5`) send Anthropic
  Messages to `{host}/ai-gateway/mlflow/v1/anthropic/v1/messages`, which the
  workspace refuses. Serving-endpoint names work
  (`{host}/serving-endpoints/anthropic/v1/messages`). So the seed takes
  `databricks:databricks-claude-opus-5-5`, and names the endpoint
  `claude-opus-5-5`, without the `databricks-` prefix.
- The provider forwards a client's other headers, so Databricks-specific ones
  such as `x-databricks-use-coding-agent-mode` still reach the workspace.

The first work package confirms two points against the image: whether the
gateway accepts a `databricks` secret with no token, and whether the SDK's
default chain sees the container's environment. If either fails, the fallback
is to push the token instead. The desktop re-mints before expiry and updates
MLflow's `databricks` secret over `/api/3.0/mlflow/gateway`, which is the call
the hand-made demo setup used.

**From the CLI.** `trestle up` has no process that outlives it, so the CLI
doesn't serve the endpoint. A CLI user can set `DATABRICKS_TOKEN` from
`databricks auth token`, and it works until it expires.

### Neon AI Gateway

A Neon branch with `aiGateway` enabled has its own gateway, served by Databricks.
It speaks native Anthropic Messages at `/anthropic` and OpenAI Responses at
`/openai/v1`, and authorizes requests with a Neon credential (`nt_live_…`) that
doesn't expire. Neon gives a branch two variables, `NEON_AI_GATEWAY_BASE_URL`
(the bare host) and `NEON_AI_GATEWAY_TOKEN`.

- trestle's Neon source (`olai-stack-runtime`'s `NeonBranchSource`) exposes the
  two variables for a bound branch with the gateway enabled, the way it exposes
  `NEON_S3_*` today.
- The seed's `neon` upstream uses MLflow's own providers with their base URLs
  overridden. The `anthropic` provider gets
  `anthropic_api_base=$NEON_AI_GATEWAY_BASE_URL/anthropic/v1`, the `openai`
  provider gets `openai_api_base=…/openai/v1`, and the token is the key for
  both.
- The desktop offers the option only for an instance on a branch. It lists the
  models from the branch's `GET /v1/models`, only those with `enabled: true`.
  The limits that come with the gateway show up as errors on the option: a
  paid plan, prepaid credits, and four AWS regions.

MLflow still traces every call, and Neon's usage logs show the same calls.

### Omnigent without MLflow

An environment that wants an agent but not MLflow runs Omnigent's host straight
against the upstream. trestle's `omnigent` module stops requiring `mlflow`.
Without it, the host takes its harness configuration from the upstream:

- Neon: `ANTHROPIC_BASE_URL=$NEON_AI_GATEWAY_BASE_URL/anthropic`,
  `OPENAI_BASE_URL=…/openai/v1`, and the token as both keys.
- Databricks profile: Omnigent's own Databricks support (0.22), which pins the
  Claude harness to the workspace's AI Gateway and hands it a bearer through an
  `apiKeyHelper`. The helper runs `databricks auth token`, which the container
  lacks, and then falls back to the SDK (`python3 -m
  omnigent.inner.databricks_token`). Mounting a `.databrickscfg` whose profile
  holds `host` and `metadata_service_url` makes that fallback use the desktop's
  token endpoint. This is a spike in the second work package.
- Provider keys: the keys themselves, declared as secrets of the `omnigent`
  module.

Nothing traces these calls in the environment. With MLflow in the environment,
the host uses the gateway, as it does today.

## The passphrase and the declared secrets

- The `mlflow` module declares the provider keys, `DATABRICKS_HOST`,
  `DATABRICKS_METADATA_SERVICE_URL` and the `NEON_AI_GATEWAY_*` pair as optional
  secrets. The runtime's secret providers resolve them into the launch. They
  reach the containers by name only and are never rendered into the instance
  directory. That closes the first gap.
- `MLFLOW_CRYPTO_KEK_PASSPHRASE` becomes a required secret. The desktop mints it
  per environment on the first Start that needs it, in the keychain, as it mints
  `UC_SERVER_KEK` (`shell-core/src/kek.rs`). `kek.rs` generalizes to the secrets
  modules declare as minted. That closes the second gap.

No secret reaches SQLite or the instance directory, which is #82's own bar.

## Connecting an agent

The Apps view's MLflow tab shows the snippet for pointing Claude Code or Codex
at the instance, built from the plan's `MLFLOW_AI_GATEWAY_URL`:

```sh
export ANTHROPIC_BASE_URL="$MLFLOW_AI_GATEWAY_URL/anthropic"
export ANTHROPIC_API_KEY=via-mlflow-ai-gateway   # placeholder: the gateway drops it
export ANTHROPIC_MODEL=claude-opus-5-5
```

## Where `ug` fits

`ug` (Unity Gateway CLI, `uv tool install git+https://github.com/databricks/unity-gateway`)
signs in to a workspace and configures Claude Code, Codex, Gemini CLI and others
to call its Unity AI Gateway. It also sets up MCP servers, skills, budgets and
model routing. We don't build on it:

- Its login is the Databricks CLI's U2M, the same thing the token endpoint
  builds on. The endpoint uses the public `databricks auth token`, not `ug`'s
  hidden `auth-token` command or its private `~/.ucode/state.json`.
- It can't point at the local gateway. `ug` builds every URL as `https://<workspace
  host>` and configures agents from Databricks-only APIs (SCIM `Me`, Unity
  Catalog `model-services`, coding-agent configs, budgets). An agent can still
  point at the local gateway by hand with the snippet above.

For coding against Databricks itself, `ug claude` is the better experience. The
local gateway is for the agents and code inside an environment, and for anyone
who wants their calls traced in the environment's MLflow.

## Limits

- An extra hop (local MLflow, then the workspace or Neon) adds latency, and the
  same request shows up in two usage logs.
- A Databricks-backed environment keeps working only while the desktop runs and
  the profile can refresh. Calls fail when the desktop quits, even if the
  containers keep running.
- Calls run as the profile's user, not as each agent's user. The Envoy route
  strips client credentials on purpose.
- The upstream is fixed at Start. Changing it takes effect on the next Start.

## Later

- A Databricks service principal (OAuth M2M), or a personal access token, as
  workspace-scoped keychain variables entered through presets in the variable
  editor (#82). MLflow's provider takes `client_id`/`client_secret` and
  refreshes on its own. This is for environments that have to run without the
  desktop.
- Unity Catalog model names, once MLflow routes Anthropic Messages under
  `{host}/ai-gateway/anthropic/…`.

## Work

Tracking: #170.

1. trestle#361: the upstreams in the gateway seed (`databricks` with
   serving-endpoint names and the default chain, `neon`), the declared secrets,
   the required passphrase, the Neon source's AI Gateway variables.
2. trestle#391: Omnigent without MLflow.
3. #168: the desktop's Databricks profiles, the Models choice, the token
   endpoint, the minted passphrase.
4. #169: the Neon AI Gateway option, the connect-an-agent snippet (#83), the
   Agent demo on a stock stack.
