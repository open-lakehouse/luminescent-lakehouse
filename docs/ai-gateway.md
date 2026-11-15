# The AI Gateway, backed by Databricks

An environment with the `mlflow` module runs MLflow's built-in AI Gateway. The
agents inside it (Omnigent, notebooks, Claude Code pointed at the environment)
get their models from it, and none of them holds a provider key. Today the
gateway reaches Anthropic, OpenAI or Gemini with personal API keys. This
document is the design for letting it reach a Databricks workspace's Unity AI
Gateway instead. It also covers where the Databricks `ug` CLI fits (mostly, it
doesn't). Status: design. Nothing here is built yet; the work is tracked in the
issues linked at the end.

## The gateway today

trestle's `mlflow` module (`crates/stack-topology/src/catalog/baseline.rs`,
image `ghcr.io/mlflow/mlflow:v3.18.0-full`) fronts MLflow's gateway at
`<gateway>/ai-gateway`, exported to clients as `MLFLOW_AI_GATEWAY_URL`:

| Surface | Path | Speaks |
|---|---|---|
| Anthropic | `/ai-gateway/anthropic/v1/messages` | Anthropic Messages |
| OpenAI | `/ai-gateway/openai/v1/{responses,chat/completions}` | OpenAI |
| Unified | `/ai-gateway/mlflow/v1/chat/completions` | OpenAI-compatible, any provider |

The one-shot `mlflow-gateway-init` container (`seed_gateway.py`) creates one
endpoint per `provider:model` pair in the `mlflow.ai_gateway_models` knob. It
names each endpoint after the model id, so a client asks for `claude-opus-5-5`
as it would upstream. The provider keys (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`,
`GEMINI_API_KEY`) reach the container as bare Compose pass-throughs from the
process that runs `docker compose up`. MLflow stores them encrypted in its
database. The Envoy route drops whatever credential a client sends, so MLflow
always uses the stored key. trestle's `stack-topology` README has the full
reference.

Two gaps already exist:

- The module doesn't declare the provider keys as secrets, so the desktop's
  secret providers (process env, `trestle` keychain) never resolve them for the
  launch. They come only from the app's own environment. Launched from the
  Finder, that environment has none, and the gateway seeds nothing.
- MLflow encrypts the stored keys with a fixed passphrase
  (`MLFLOW_CRYPTO_KEK_PASSPHRASE=lakehouse-dev-kek` in `mlflow.env.jinja`), in a
  database that may sit on a Neon branch. That is good enough for a throwaway
  dev key, but not for a service principal's secret.

## Why Databricks

Most people with a Databricks workspace have access to its models through Unity
AI Gateway, but not a personal Anthropic or OpenAI key. Backing the local
gateway with the workspace gives every environment's agents the models the
organization governs (budgets, permissions, usage logs), under the same model
ids. MLflow still traces and counts every call locally.

MLflow already does most of this. Its `databricks` provider
(`mlflow/gateway/providers/databricks.py`, checked in the pinned 3.18.0 image):

- sends a Unity Catalog model name (`system.ai.claude-opus-5-5`: three parts)
  to `{host}/ai-gateway/mlflow/v1`, the workspace's Unity AI Gateway. A plain
  serving endpoint name goes to `/serving-endpoints` instead;
- passes Anthropic Messages, OpenAI chat, responses and embeddings, and Gemini
  through unchanged, so Claude Code → local MLflow → Databricks needs no
  translation between APIs;
- authenticates through the Databricks SDK. With OAuth M2M (`auth_mode:
  oauth_m2m`, `client_id`, `client_secret`, the host as `api_base`), the SDK
  mints and refreshes the token on its own (`server/gateway_api.py`). A personal
  access token is a static `api_key`;
- forwards a client's other headers (only `host`, `content-length` and
  credentials are dropped), so Databricks-specific headers such as
  `x-databricks-use-coding-agent-mode` still reach the workspace.

## Design

### Credentials: a service principal per workspace

A Databricks
service principal with OAuth M2M is the default. It refreshes itself, so nothing
on the host has to keep a token alive while an environment runs. A personal
access token is the fallback for anyone who can't create one. Both live where
every other secret of an environment lives: trestle env vars whose values are
keychain references (`trestle var set --secret`, keychain service `trestle`).
They sit at the workspace scope, so every environment in it inherits them:

| Variable | Secret | Used for |
|---|---|---|
| `DATABRICKS_HOST` | no | the workspace URL (`api_base`) |
| `DATABRICKS_CLIENT_ID` | no | M2M |
| `DATABRICKS_CLIENT_SECRET` | yes | M2M |
| `DATABRICKS_TOKEN` | yes | PAT fallback, when no client id is set |

The names are the Databricks SDK's, so a CLI user can export them in a shell,
just as they do today.

### trestle: a `databricks` provider in the seed

- `ai_gateway_models` accepts `databricks:<catalog>.<schema>.<model>`, e.g.
  `databricks:system.ai.claude-opus-5-5`. The endpoint is named after the last
  part (`claude-opus-5-5`), so Omnigent's defaults and every client keep their
  model ids whichever provider backs them.
- The seed stores one `databricks` secret, M2M when a client id is set and the
  PAT otherwise. Unlike the other secrets, it is updated whenever the resolved
  value differs, so a rotated service principal secret takes effect on the next
  Start.
- A second preset, `MLFLOW_AI_GATEWAY_MODELS_DATABRICKS`, covers the same three
  Claude models from `system.ai`.
- The module declares the four `DATABRICKS_*` variables and the three provider
  keys as optional secrets. The runtime's secret providers then resolve them
  into the launch, and they still reach the container only by name, never
  rendered into the instance directory. That also closes the first gap above.
- `MLFLOW_CRYPTO_KEK_PASSPHRASE` becomes a required secret instead of the fixed
  value, which closes the second gap.

### Desktop: credentials in the variable editor (#82)

- The variable editors gain presets for "Databricks service principal" (host,
  client id, client secret), "Databricks token" (host, token) and the provider
  keys. Each writes the variables above, with the secrets in the keychain.
- The effective-env view shows them masked, with the workspace as their layer.
- The MLflow passphrase is minted per environment on the first Start that needs
  it, in the keychain, exactly like `UC_SERVER_KEK` (`shell-core/src/kek.rs`).
  `kek.rs` generalizes from that one variable to the secrets modules declare as
  minted, so the second key doesn't add a copy of it.

No secret reaches SQLite or the instance directory, which is #82's own bar.

### Desktop: connecting an agent

The Apps view's MLflow tab shows the snippet
for pointing Claude Code or Codex at the instance, built from the plan's
`MLFLOW_AI_GATEWAY_URL`:

```sh
export ANTHROPIC_BASE_URL="$MLFLOW_AI_GATEWAY_URL/anthropic"
export ANTHROPIC_API_KEY=via-mlflow-ai-gateway   # placeholder: the gateway drops it
export ANTHROPIC_MODEL=claude-opus-5-5
```

## Where `ug` fits

`ug` (Unity Gateway CLI, `uv tool install git+https://github.com/databricks/unity-gateway`)
logs in to a workspace and configures Claude Code, Codex, Gemini CLI and others
to call its Unity AI Gateway. It also sets up MCP servers, skills, budgets and
model routing. We considered two uses and adopt neither:

- **`ug` as the login that backs the local gateway.** `ug` signs in through the
  Databricks CLI's OAuth (U2M), whose tokens last about an hour. The container
  has neither the CLI nor its token cache. The host would have to mint tokens
  (`databricks auth token --profile …`) and push each one into MLflow
  (`UpdateGatewaySecret`) for as long as the instance runs. A service principal
  needs none of that. This stays a possible later option for people who can't
  get a service principal. It would build on the public `databricks auth`
  commands, not on `ug`'s hidden `auth-token` command or its private
  `~/.ucode/state.json`.
- **`ug` pointed at the local gateway.** It can't be. `ug` builds every URL as
  `https://<workspace host>` and configures agents from Databricks-only APIs:
  SCIM `Me`, Unity Catalog `model-services`, coding-agent configs, budgets.
  Because the local gateway serves the same paths, an agent can still point at it
  by hand with the snippet above.

So the two serve different purposes. For coding against Databricks itself,
`ug claude` is the better experience: budgets, routing, MCP servers and skills.
The local gateway is for the agents and code inside an environment, and for
anyone who wants their calls traced in the environment's MLflow.

## Limits

- The call takes an extra hop (local MLflow, then the workspace), which adds
  latency and puts two usage logs on the same request.
- The service principal needs access to the model services it serves
  (`system.ai.*` by default). The calls count against its budgets, not the
  developer's.
- One identity per workspace: every developer's calls through an environment
  run as the service principal. MLflow would forward a credential-carrying
  agent's own `Authorization` header (it recognizes `claude-cli`, `codex` and
  `GeminiCLI` user agents), but the Envoy route strips client credentials on
  purpose. Per-user identity would need that turned off for a Databricks-backed
  gateway, and is not part of this design.
- The seed runs on Start. Changing the credentials in the editor takes effect on
  the next Start, not on the running instance.

## Work

- trestle: the `databricks` provider in the AI Gateway seed, the declared
  secrets, the required passphrase (trestle issue, linked from #82).
- #82: the credential presets, and minting the MLflow passphrase.
- #83: the "connect an agent" snippet.
