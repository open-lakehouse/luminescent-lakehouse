// The IPC a Playwright run (`vite build --mode e2e`) answers in place of Rust:
// the UI is exercised in a plain browser, the commands return fixtures (the
// shared state is the in-memory fake, the planner the fixture one). main.ts
// imports this only in that mode, so production builds don't contain it.

import { createFakeFactoryFetch } from "@open-lakehouse/factory-client/testing";
import { FIXTURE_CATALOG } from "@open-lakehouse/stack-wasm/fixtures";
import { createWasmPlanner } from "@open-lakehouse/stack-wasm/stub";
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { FACTORY, OMNIGENT, UC_MCP, UNITY_CATALOG } from "../integrations/services";
import { READY_REPORT } from "../shell/fixtures";
import type { Json, ModelsChoice, ProxyError, VarInput, VarTarget } from "./bindings";
import { createFakeAgent, FACTORY_SESSIONS } from "./fake-agent";
import type { FakeResponse } from "./fake-mcp";
import { fakeMcpAppSandboxUrl } from "./fake-mcp-app";
import { createFakeModels } from "./fake-models";
import { fixtureAnswer, frameChunk } from "./fake-query";
import { createFakeState, type FakeState } from "./fake-state";
import { type FakeUcMcpLog, fakeUcMcp } from "./fake-uc-mcp";
import { createFakeVars } from "./fake-vars";
import { createFakeWorkgraphAgents } from "./fake-workgraph-agents";
import { type FakeWorkgraphMcpLog, fakeWorkgraphMcp, isWorkgraphMcp } from "./fake-workgraph-mcp";

type Args = Record<string, unknown>;

/** `{ status: "error" }` results become rejections, as Rust's `Err` does. */
async function settle(result: Promise<{ status: string; data?: unknown; error?: unknown }>) {
  const r = await result;
  if (r.status === "error") throw r.error;
  return r.data;
}

/** The fixture plan's files are the same whatever is selected; name the selected
 *  modules in its compose.yaml, so a changed selection changes a file too (what
 *  the editor's Changes diff shows). */
function withSelectedModules<P extends { files: { path: string; contents: string }[] }>(
  plan: P,
  selection: { modules: string[] },
): P {
  const line = `# modules: ${[...selection.modules].sort().join(", ")}\n`;
  return {
    ...plan,
    files: plan.files.map((f) =>
      f.path === "compose.yaml" ? { ...f, contents: line + f.contents } : f,
    ),
  };
}

/** Send `messages` on a channel the UI passed, a tick later, as Rust sends them:
 *  `{ index, message }` to the callback mockIPC registered the `Channel` under. */
function sendOn(channel: { id: number }, messages: unknown[]) {
  const internals = (
    window as unknown as {
      __TAURI_INTERNALS__: { runCallback(id: number, data: unknown): void };
    }
  ).__TAURI_INTERNALS__;
  setTimeout(() => {
    messages.forEach((message, index) => {
      internals.runCallback(channel.id, { index, message });
    });
  }, 0);
}

/** A sender for messages on one channel, indexed from 0 as Rust's are, in order. */
function channelSender(channel: { id: number }) {
  const internals = (
    window as unknown as {
      __TAURI_INTERNALS__: { runCallback(id: number, data: unknown): void };
    }
  ).__TAURI_INTERNALS__;
  let index = 0;
  return (message: unknown) => {
    const at = index++;
    setTimeout(() => internals.runCallback(channel.id, { index: at, message }), 0);
  };
}

/** `request` answered by `fetch` (an in-process service's fake): the head, then the
 *  body on its channel as it streams (a Connect server stream stays open). */
async function forwardTo(
  fetch: (request: Request) => Promise<Response>,
  request: { method: string; path: string; headers: [string, string][]; body: number[] | null },
  onBody: { id: number },
) {
  const response = await fetch(
    new Request(new URL(request.path, "http://in-process.invalid"), {
      method: request.method,
      headers: request.headers,
      body: request.body ? new Uint8Array(request.body) : undefined,
    }),
  );
  const send = channelSender(onBody);
  void (async () => {
    const reader = response.body?.getReader();
    try {
      for (;;) {
        const chunk = await reader?.read();
        if (!chunk || chunk.done) break;
        send(chunk.value.slice().buffer);
      }
      send("end");
    } catch (e) {
      send({ error: { kind: "upstream", message: String(e) } });
    }
  })();
  return {
    status: response.status,
    statusText: response.statusText,
    headers: [...response.headers.entries()],
  };
}

/** An empty Unity Catalog: a list (`/catalogs`, `/schemas`, …) has nothing in it, and
 *  nothing else exists. Answers with the head; the body follows on its channel. */
function emptyCatalog(path: string, send: (messages: unknown[]) => void) {
  const list = new URL(path, "http://uc.invalid").pathname.match(/\/unity-catalog\/([a-z-]+)$/);
  const ok = list !== null;
  const body = ok ? { [(list[1] ?? "").replace(/-/g, "_")]: [] } : { error_code: "NOT_FOUND" };
  send([new TextEncoder().encode(JSON.stringify(body)).buffer, "end"]);
  return {
    status: ok ? 200 : 404,
    statusText: ok ? "OK" : "Not Found",
    headers: [["content-type", "application/json"]],
  };
}

export function installIpcMock() {
  const state = createFakeState(FIXTURE_CATALOG.default_selection as Json);
  // Playwright writes to it as the trestle CLI would (e2e builds only).
  (window as { __LL_FAKE_STATE__?: FakeState }).__LL_FAKE_STATE__ = state;
  const planner = createWasmPlanner();
  void state.listen(() => void emit("state-changed", null));
  void state.listenInstances((change) => void emit("instance-changed", { change }));
  // The app's settings, as src-tauri keeps them (settings.json).
  let settings = {
    quit: "keep",
    oneAtATime: [] as string[],
    lastInstance: null as string | null,
    neonTargets: {} as Record<
      string,
      { project: string | null; branch: { kind: string; branch?: string; ttlHours?: number } }
    >,
    workgraphAgents: {} as Record<string, unknown[]>,
  };
  // The current environment, as src-tauri's run commands keep it.
  let current: unknown = null;
  let currentKey = "none";
  const announce = (next: unknown, key?: string) => {
    current = next;
    currentKey = key ?? "none";
    void emit("environment-changed", { key: currentKey, current: next });
  };
  // The agent: a scripted Omnigent (the grant demo, and the demo plan's sessions), and
  // mangrove's `uc mcp serve`.
  const agent = createFakeAgent({ sessions: FACTORY_SESSIONS });
  // The shell's in-process factory: the demo plan, whatever the environment.
  const factory = createFakeFactoryFetch({ seed: "demo" });
  const ucMcp: FakeUcMcpLog = { calls: [] };
  window.__LL_UC_MCP__ = ucMcp;
  // The factory's workgraph-mcp at `/mcp`, with the real plan review view.
  const workgraphMcp: FakeWorkgraphMcpLog = { calls: [] };
  window.__LL_WORKGRAPH_MCP__ = workgraphMcp;
  const agentLog = {
    sent: agent.sent,
    interrupts: agent.interrupts,
    opened: [] as string[],
  };
  (window as { __LL_AGENT__?: typeof agentLog }).__LL_AGENT__ = agentLog;
  const subscriptions = new Map<string, () => void>();
  // The Models choice (shell-core's `models`), on the same state.
  const vars = createFakeVars(state);
  (window as { __LL_VARS__?: typeof vars }).__LL_VARS__ = vars;
  const models = createFakeModels(state, {
    isSet: vars.isSet,
    branchOf: (environment) => {
      const target = settings.neonTargets[environment];
      return target
        ? target.branch.kind === "existing"
          ? (target.branch.branch ?? "")
          : "new"
        : null;
    },
  });
  (window as { __LL_MODELS__?: typeof models }).__LL_MODELS__ = models;
  const workgraphAgents = createFakeWorkgraphAgents();
  (window as { __LL_WORKGRAPH_AGENTS__?: typeof workgraphAgents }).__LL_WORKGRAPH_AGENTS__ =
    workgraphAgents;
  // Its errors, as Tauri returns a command's `Err`.
  const fail = (kind: string) => (e: unknown) => {
    throw { kind, message: e instanceof Error ? e.message : String(e) };
  };
  /** As `fail`, keeping the kind the fake gave the error (`notFound`, `store`, …). */
  const failAs = (fallback: string) => (e: unknown) => {
    const kind = (e as { kind?: string } | null)?.kind ?? fallback;
    throw { kind, message: e instanceof Error ? e.message : String(e) };
  };
  let neonBranches = [
    { id: "br-production", name: "production", default: true, owned: false, expiresAt: null },
    { id: "br-staging", name: "staging", default: false, owned: false, expiresAt: null },
    {
      id: "br-scratch",
      name: "scratch",
      default: false,
      owned: true,
      expiresAt: "2026-11-14T00:00:00Z",
    },
  ];

  mockIPC(
    async (cmd, payload) => {
      const a = (payload ?? {}) as Args;
      const s = (k: string) => a[k] as string;
      switch (cmd) {
        case "doctor":
          return READY_REPORT;
        case "list_services":
          // The in-process factory, and a current instance's Unity Catalog (which lists
          // nothing), Omnigent and uc-mcp.
          return [FACTORY, ...(current ? [UNITY_CATALOG, UC_MCP, OMNIGENT] : [])].map(
            (moduleId) => ({ moduleId, placement: "local" }),
          );
        case "service_fetch": {
          const { request, onBody } = a as {
            request: {
              moduleId: string;
              method: string;
              path: string;
              headers: [string, string][];
              body: number[] | null;
            };
            onBody: { id: number };
          };
          const respond = (response: FakeResponse) => {
            sendOn(
              onBody,
              response.body === undefined
                ? ["end"]
                : [new TextEncoder().encode(response.body).buffer, "end"],
            );
            return {
              status: response.status,
              statusText: response.status === 200 ? "OK" : "",
              headers: response.headers,
            };
          };
          if (request.moduleId === FACTORY && isWorkgraphMcp(request.path)) {
            return respond(fakeWorkgraphMcp(workgraphMcp, request));
          }
          if (request.moduleId === FACTORY) return forwardTo(factory.fetch, request, onBody);
          if (current && request.moduleId === UC_MCP) return respond(fakeUcMcp(ucMcp, request));
          if (!current || request.moduleId !== UNITY_CATALOG) {
            const error: ProxyError = { kind: "unbound", moduleId: request.moduleId };
            throw error;
          }
          return emptyCatalog(request.path, (messages) => sendOn(onBody, messages));
        }
        case "service_fetch_cancel":
          return null;
        case "query_run": {
          const result = fixtureAnswer(
            (a.instance as string | null) ?? null,
            (a.request as { sql: string }).sql,
          );
          if ("error" in result) throw result.error;
          sendOn(a.onChunk as { id: number }, [...result.chunks.map(frameChunk), "end"]);
          return null;
        }
        case "query_cancel":
          return null;
        case "state_location":
          return settle(state.stateLocation());
        case "state_workspaces":
          return settle(state.stateWorkspaces());
        case "state_create_workspace":
          return settle(state.stateCreateWorkspace(s("name"), a.mesh as boolean, s("firstEnv")));
        case "state_workspace":
          return settle(state.stateWorkspace(s("id")));
        case "state_create_environment":
          return settle(
            state.stateCreateEnvironment(s("workspace"), s("name"), (a.selection ?? null) as Json),
          );
        case "state_delete_environment":
          return settle(state.stateDeleteEnvironment(s("id")));
        case "state_set_selection":
          return settle(
            state.stateSetSelection(s("id"), a.selection as Json, a.expected as number),
          );
        case "state_rename_environment":
          return settle(state.stateRenameEnvironment(s("id"), s("name")));
        case "state_revisions":
          return settle(state.stateRevisions(s("id")));
        case "state_revision":
          return settle(state.stateRevision(s("id"), a.number as number));
        case "state_commit":
          return settle(state.stateCommit(s("id"), (a.message as string | null) ?? null));
        case "state_import":
          return settle(
            state.stateImport(
              s("dir"),
              (a.workspace as string) ?? null,
              (a.name as string) ?? null,
            ),
          );
        case "state_export_workspace":
          return settle(state.stateExportWorkspace(s("id"), s("parent")));
        case "state_export_revision":
          return settle(state.stateExportRevision(s("id"), a.number as number, s("parent")));
        case "pick_directory":
          // Playwright picks this folder; an import picks the export made into it.
          return (window as { __LL_PICK__?: string }).__LL_PICK__ ?? "/fake/exports";
        case "run_workspace":
          return settle(state.runWorkspace(s("workspace")));
        case "run_start": {
          const env = s("environment");
          let oneAtATime = false;
          for (const id of settings.oneAtATime) {
            const snapshot = await state.stateWorkspace(id);
            if (snapshot.status === "ok" && snapshot.data.environments.some((e) => e.id === env)) {
              oneAtATime = true;
            }
          }
          const target = settings.neonTargets[env];
          const started = (await settle(
            state.runStart(env, oneAtATime, (target?.branch as never) ?? null),
          )) as { id: string; revision: number; branch: { name: string } | null };
          if (target?.branch.kind === "new" && started.branch) {
            settings = {
              ...settings,
              neonTargets: {
                ...settings.neonTargets,
                [env]: { ...target, branch: { kind: "existing", branch: started.branch.name } },
              },
            };
          }
          announce(
            {
              kind: "instance",
              instanceId: started.id,
              workspaceId: "",
              workspace: "local",
              environmentId: s("environment"),
              environment: "dev",
              revision: started.revision,
            },
            `instance:${started.id}`,
          );
          return started;
        }
        case "run_stop":
          await settle(state.runStop(s("environment")));
          if ((current as { environmentId?: string } | null)?.environmentId === s("environment"))
            announce(null);
          return null;
        case "vars_get":
          return vars
            .get(s("environment"), (a.instance as string | null) ?? null)
            .catch(failAs("notFound"));
        case "vars_set":
          return vars
            .set(a.target as VarTarget, s("key"), a.input as VarInput)
            .catch(failAs("invalid"));
        case "vars_unset":
          await vars.unset(a.target as VarTarget, s("key")).catch(failAs("store"));
          return null;
        case "run_effective_env":
          return vars.effective(s("instance")).catch(fail("other"));
        case "agents_get":
          return workgraphAgents.get(s("environment"));
        case "agents_enable":
          return workgraphAgents
            .enable(s("environment"), a.roles as string[])
            .catch(failAs("other"));
        case "agents_disable":
          return workgraphAgents.disable(s("environment")).catch(failAs("other"));
        case "agents_rotate":
          return workgraphAgents.rotate(s("environment"), s("role")).catch(failAs("other"));
        case "agents_set_key":
          return workgraphAgents
            .setKey(s("environment"), s("role"), s("value"))
            .catch(fail("other"));
        case "models_get":
          return models.get(s("id"));
        case "models_set":
          return models.set(s("id"), a.choice as ModelsChoice).catch(fail("invalid"));
        case "databricks_profiles":
          return models.profiles().catch(fail("databricks"));
        case "run_missing_secrets":
          return models.missingSecrets(s("environment"));
        case "models_neon_gateway":
          return models.neonGateway(s("environment")).catch(fail("neon"));
        case "run_connect_agent":
          return {
            gatewayUrl: "http://localhost:9080/ai-gateway",
            models: ["claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"],
            upstream: "databricks",
            profile: "work",
            branch: null,
          };
        case "run_service_uis":
          return [
            {
              moduleId: "mlflow",
              name: "MLflow",
              url: "http://local-dev-r1.localhost:9080/mlflow/",
            },
          ];
        case "service_embeds_supported":
          return true;
        case "embed_service_ui":
          (window as { __LL_EMBEDS__?: unknown[] }).__LL_EMBEDS__ = [
            ...((window as { __LL_EMBEDS__?: unknown[] }).__LL_EMBEDS__ ?? []),
            { module: s("module"), bounds: a.bounds ?? null },
          ];
          return null;
        case "open_service_window":
          (window as { __LL_OPENED__?: string[] }).__LL_OPENED__ = [
            ...((window as { __LL_OPENED__?: string[] }).__LL_OPENED__ ?? []),
            s("module"),
          ];
          return null;
        case "instance_logs":
        case "instance_logs_stop":
          // Playwright doesn't stream logs: the viewer is covered by bun test.
          return null;
        case "run_reconciled":
          return { adopted: [], stale: [], orphans: [] };
        case "settings_get":
          return settings;
        case "settings_set_quit":
          settings = { ...settings, quit: s("quit") };
          return settings;
        case "settings_set_one_at_a_time": {
          const rest = settings.oneAtATime.filter((w) => w !== s("workspace"));
          settings = { ...settings, oneAtATime: a.on ? [...rest, s("workspace")] : rest };
          return settings;
        }
        case "run_stop_instance":
          await settle(state.runStopInstance(s("instance")));
          if ((current as { instanceId?: string } | null)?.instanceId === s("instance"))
            announce(null);
          return null;
        case "run_current":
          return { key: currentKey, current };
        case "neon_branches":
          return {
            projectId: "cool-poetry-28343550",
            source: "default",
            parent: "staging",
            branches: neonBranches,
          };
        case "neon_projects":
          return [
            { id: "cool-poetry-28343550", name: "luminescent-eu", org: "open-lakehouse" },
            { id: "quiet-lake-11", name: "luminescent-us", org: "open-lakehouse" },
          ];
        case "neon_target_set": {
          const { [s("environment")]: _, ...rest } = settings.neonTargets;
          settings = {
            ...settings,
            neonTargets: a.target ? { ...rest, [s("environment")]: a.target as never } : rest,
          };
          return settings;
        }
        case "neon_needs":
          return settle(state.neonNeeds(s("environment")));
        case "neon_create_branch": {
          const branch = {
            id: `br-mock-${neonBranches.length + 1}`,
            name: s("name"),
            default: false,
            owned: true,
            expiresAt: null,
          };
          neonBranches = [...neonBranches, branch];
          return branch;
        }
        case "neon_delete_branch": {
          const branch = neonBranches.find((b) => b.name === s("branch"));
          if (!branch?.owned) throw { kind: "neon", message: `${s("branch")} isn't owned` };
          neonBranches = neonBranches.filter((b) => b !== branch);
          return branch;
        }
        case "neon_reset_branch":
          return null;
        case "agent_status":
          return agent.status(s("instance"), s("agent"));
        case "session_create":
          return agent.create(s("instance"), s("agent"), (a.title as string | null) ?? null);
        case "session_subscribe": {
          const send = channelSender(a.onEvent as { id: number });
          subscriptions.get(s("sessionId"))?.();
          subscriptions.set(
            s("sessionId"),
            await agent.subscribe(s("instance"), s("sessionId"), send),
          );
          return null;
        }
        case "session_unsubscribe":
          subscriptions.get(s("sessionId"))?.();
          subscriptions.delete(s("sessionId"));
          return null;
        case "session_send":
          return agent.send(s("instance"), s("sessionId"), s("text"));
        case "session_interrupt":
          await agent.interrupt(s("instance"), s("sessionId"));
          return null;
        case "open_external":
          agentLog.opened.push(s("url"));
          return null;
        case "planner_catalog":
          return planner.catalog();
        case "planner_plan":
          return withSelectedModules(
            await planner.plan(a.selection as never, (a.target as string) ?? undefined),
            a.selection as { modules: string[] },
          );
        default:
          throw new Error(`ipc-mock: unexpected command ${cmd}`);
      }
    },
    { shouldMockEvents: true },
  );
  // `convertFileSrc` (mockIPC doesn't mock it): the MCP Apps sandbox comes from the
  // fakes' second origin, which the Playwright build's server serves.
  const internals = (
    window as unknown as {
      __TAURI_INTERNALS__: { convertFileSrc?: (path: string, protocol?: string) => string };
    }
  ).__TAURI_INTERNALS__;
  internals.convertFileSrc = (path, protocol = "asset") => {
    if (protocol === "mcp-app" && path === "sandbox.html") return fakeMcpAppSandboxUrl();
    return `${protocol}://localhost/${encodeURIComponent(path)}`;
  };
}
