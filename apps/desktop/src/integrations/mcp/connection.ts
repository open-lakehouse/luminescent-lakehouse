// The desktop's own MCP connection to one of an environment's MCP servers with Apps
// (`uc mcp serve`, trestle's `uc-mcp` module; the factory's `workgraph-mcp`, at the
// `factory` binding's `/mcp`), next to the agent's.
// Omnigent flattens tool results to text, so the Agent view reads MCP App resources and
// re-runs read-only tools here, and an app's own tools (`apply_grants`, visible to apps
// only) go through here too.
//
// It speaks Streamable HTTP over the shell's service proxy (`hostFetch`): the proxy
// sends only the path, so the URL's origin is a placeholder. It declares the MCP
// Apps extension, so the server lists its app-only tools to this connection (and
// never to the agent's): in `initialize`, and again in every request's `_meta`
// (`withClientCapabilities`), since a stateless server (the hosted `workgraph`
// function) keeps nothing from `initialize` for a 2025-11-25 client like this SDK.
//
// Only types are imported statically: the SDK and ext-apps load on the first
// connect, so importing this module costs the shell nothing.

import type {
  CallToolResult,
  Client,
  FetchLike,
  ReadResourceResult,
  Tool,
} from "@modelcontextprotocol/client";
import type {
  McpUiResourceCsp,
  McpUiResourcePermissions,
} from "@modelcontextprotocol/ext-apps/app-bridge";
import { hostFetch } from "@open-lakehouse/host";
import { FACTORY, UC_MCP } from "../services";

/** The MCP Apps extension's id and the one resource type the desktop renders. */
export const MCP_UI_EXTENSION = "io.modelcontextprotocol/ui";
export const MCP_APP_MIME_TYPE = "text/html;profile=mcp-app";

/** Where trestle's `uc-mcp` module serves MCP (`uc mcp serve --path /uc-mcp/mcp`).
 *  The module has no base path, so its binding root is the gateway's `/` and the
 *  proxy sends this path as is. The origin is never sent. */
export const UC_MCP_URL = "http://uc-mcp.invalid/uc-mcp/mcp";

const CLIENT_INFO = { name: "luminescent-desktop", version: "0.0.0" };

/** What the connection declares: the MCP Apps extension, with the desktop's one type. */
const CLIENT_CAPABILITIES = {
  extensions: { [MCP_UI_EXTENSION]: { mimeTypes: [MCP_APP_MIME_TYPE] } },
};

/** Where a request declares its client's capabilities itself (SEP-2575). */
export const CLIENT_CAPABILITIES_META = "io.modelcontextprotocol/clientCapabilities";

/**
 * `fetch` that adds `capabilities` to the `_meta` of every JSON-RPC request it posts
 * (`initialize` and notifications aside), so a stateless server reads them per request.
 * A server with sessions reads the same declaration from either place. The SDK (2.4)
 * speaks 2025-11-25 at most and doesn't send them itself.
 */
export function withClientCapabilities(fetch: FetchLike, capabilities: object): FetchLike {
  const declare = (message: unknown): unknown => {
    if (typeof message !== "object" || message === null) return message;
    const m = message as { id?: unknown; method?: unknown; params?: Record<string, unknown> };
    if (m.id === undefined || typeof m.method !== "string" || m.method === "initialize")
      return message;
    const params = m.params ?? {};
    const meta = (params._meta as Record<string, unknown> | undefined) ?? {};
    return {
      ...m,
      params: { ...params, _meta: { ...meta, [CLIENT_CAPABILITIES_META]: capabilities } },
    };
  };
  return (url, init) => {
    if (init?.method !== "POST" || typeof init.body !== "string") return fetch(url, init);
    let body: unknown;
    try {
      body = JSON.parse(init.body);
    } catch {
      return fetch(url, init);
    }
    const declared = Array.isArray(body) ? body.map(declare) : declare(body);
    return fetch(url, { ...init, body: JSON.stringify(declared) });
  };
}

/** An MCP server whose tools may render as MCP Apps in an agent's session: how the agent
 *  names it, and where the desktop reaches it. */
export interface McpAppServer {
  /** The server's name in the agent's config: its tools are `<name>__<tool>`. */
  name: string;
  /** The host binding the desktop connects through. */
  module: string;
  /** The MCP endpoint under the binding's root (the origin is never sent). */
  url: string;
}

/** mangrove's `uc mcp serve`, as `agents/lakehouse/config.yaml` names it. */
export const UC_APPS: McpAppServer = { name: "uc", module: UC_MCP, url: UC_MCP_URL };

/** Where the factory serves `workgraph-mcp` under its binding: `/mcp` of the in-process
 *  reference factory (which answers it as the person at the desktop) or of an external
 *  factory-server (with the binding's token, an `--mcp-app-token`). The origin is never
 *  sent. */
export const WORKGRAPH_MCP_URL = "http://factory.invalid/mcp";

/** The factory's `workgraph-mcp`, as the factory's agents name it (`workgraph`): plan
 *  review (`open_plan_review`, `ui://factory/plan`) and any extension's views. The
 *  `factory` binding doesn't change with the environment. */
export const WORKGRAPH_APPS: McpAppServer = {
  name: "workgraph",
  module: FACTORY,
  url: WORKGRAPH_MCP_URL,
};

/** An MCP App's View, as `resources/read` returned it. */
export interface McpAppResource {
  uri: string;
  html: string;
  meta: {
    csp?: McpUiResourceCsp;
    permissions?: McpUiResourcePermissions;
    prefersBorder?: boolean;
  };
}

export interface CallToolOptions {
  /** Who asked: an app's calls are refused for tools not visible to apps. */
  from?: "agent" | "app";
  signal?: AbortSignal;
}

/** An app asked for a tool it may not call (not listed, or visible to the model only). */
export class McpToolNotCallableError extends Error {
  constructor(readonly tool: string) {
    super(`An MCP App may not call ${tool}`);
    this.name = "McpToolNotCallableError";
  }
}

export interface McpConnection {
  readonly environment: string;
  /** The server's name, as {@link McpAppServer.name}. */
  readonly server: string;
  /** The server's tools, app-only ones included. Cached until the server says they
   *  changed, or the session is replaced. */
  tools(): Promise<Tool[]>;
  tool(name: string): Promise<Tool | undefined>;
  /** The `ui://` resource a tool renders (`_meta.ui.resourceUri`), if any. */
  uiResourceUri(tool: Tool): string | undefined;
  /** Whether the tool is safe to re-run for its full result (`readOnlyHint`). */
  isReadOnly(tool: Tool): boolean;
  /** Whether an app may call it (visibility includes `"app"`; the default does). */
  isAppCallable(tool: Tool): boolean;
  /** Whether the agent sees it (visibility includes `"model"`; the default does). */
  isModelVisible(tool: Tool): boolean;
  /** An MCP App's View. Cached per URI for the connection's lifetime. */
  readApp(uri: string): Promise<McpAppResource>;
  callTool(
    name: string,
    args?: Record<string, unknown>,
    options?: CallToolOptions,
  ): Promise<CallToolResult>;
  close(): Promise<void>;
}

export interface McpConnectionOptions {
  /** The server; defaults to {@link UC_APPS}. */
  server?: McpAppServer;
  /** Defaults to the host's binding of the server's module. */
  fetch?: FetchLike;
  /** Defaults to the server's URL. */
  url?: string;
}

type Sdk = typeof import("@modelcontextprotocol/client");
type Apps = typeof import("@modelcontextprotocol/ext-apps/app-bridge");

/** Opens a connection. Prefer {@link mcpConnection}, which keeps one per environment and
 *  server. */
export async function connectMcp(
  environment: string,
  options: McpConnectionOptions = {},
): Promise<McpConnection> {
  const [sdk, apps] = await Promise.all([
    import("@modelcontextprotocol/client"),
    import("@modelcontextprotocol/ext-apps/app-bridge"),
  ]);
  return new Connection(environment, options, sdk, apps);
}

/** By environment, then server name. */
const connections = new Map<string, Map<string, Promise<McpConnection>>>();

/** The environment's connection to `server`, opened on first use. The session itself
 *  starts lazily (on the first request), so this only fails if the SDK can't load. */
export function mcpConnection(
  environment: string,
  server: McpAppServer = UC_APPS,
  options?: Omit<McpConnectionOptions, "server">,
): Promise<McpConnection> {
  let servers = connections.get(environment);
  if (!servers) {
    servers = new Map();
    connections.set(environment, servers);
  }
  let connection = servers.get(server.name);
  if (!connection) {
    const opened = connectMcp(environment, { ...options, server });
    servers.set(server.name, opened);
    opened.catch(() => {
      if (connections.get(environment)?.get(server.name) === opened) {
        connections.get(environment)?.delete(server.name);
      }
    });
    connection = opened;
  }
  return connection;
}

/** Closes every connection but `keep`'s (the current environment's, after a switch). */
export async function closeMcpConnections(keep?: string): Promise<void> {
  const closing = [...connections].filter(([environment]) => environment !== keep);
  for (const [environment] of closing) connections.delete(environment);
  await Promise.allSettled(
    closing.flatMap(([, servers]) => [...servers.values()].map(async (c) => (await c).close())),
  );
}

class Connection implements McpConnection {
  private client: Promise<Client> | undefined;
  private toolList: Promise<Tool[]> | undefined;
  private readonly apps = new Map<string, Promise<McpAppResource>>();
  private closed = false;
  private readonly target: McpAppServer;

  constructor(
    readonly environment: string,
    private readonly options: McpConnectionOptions,
    private readonly sdk: Sdk,
    private readonly ui: Apps,
  ) {
    this.target = options.server ?? UC_APPS;
  }

  get server(): string {
    return this.target.name;
  }

  tools(): Promise<Tool[]> {
    if (!this.toolList) {
      const list = this.request(async (client) => {
        const tools: Tool[] = [];
        let cursor: string | undefined;
        do {
          const page = await client.listTools(cursor ? { cursor } : undefined);
          tools.push(...page.tools);
          cursor = page.nextCursor;
        } while (cursor);
        return tools;
      });
      this.toolList = list;
      list.catch(() => {
        if (this.toolList === list) this.toolList = undefined;
      });
    }
    return this.toolList;
  }

  async tool(name: string): Promise<Tool | undefined> {
    return (await this.tools()).find((t) => t.name === name);
  }

  uiResourceUri(tool: Tool): string | undefined {
    return this.ui.getToolUiResourceUri(tool);
  }

  isReadOnly(tool: Tool): boolean {
    return tool.annotations?.readOnlyHint === true;
  }

  isAppCallable(tool: Tool): boolean {
    return !this.ui.isToolVisibilityModelOnly(tool);
  }

  isModelVisible(tool: Tool): boolean {
    return !this.ui.isToolVisibilityAppOnly(tool);
  }

  readApp(uri: string): Promise<McpAppResource> {
    let app = this.apps.get(uri);
    if (!app) {
      app = this.request((client) => client.readResource({ uri })).then((r) => appResource(uri, r));
      this.apps.set(uri, app);
      app.catch(() => this.apps.delete(uri));
    }
    return app;
  }

  async callTool(
    name: string,
    args?: Record<string, unknown>,
    options: CallToolOptions = {},
  ): Promise<CallToolResult> {
    if (options.from === "app") {
      const tool = await this.tool(name);
      if (!tool || !this.isAppCallable(tool)) throw new McpToolNotCallableError(name);
    }
    return this.request((client) =>
      client.callTool({ name, arguments: args }, { signal: options.signal }),
    );
  }

  async close(): Promise<void> {
    this.closed = true;
    const client = this.client;
    this.reset();
    await client?.then((c) => c.close()).catch(() => {});
  }

  /** Runs `op` on the session, starting a new one once if the server lost it (a 404:
   *  it restarted, or expired the session). */
  private async request<T>(op: (client: Client) => Promise<T>): Promise<T> {
    const client = await this.session();
    try {
      return await op(client);
    } catch (e) {
      if (!this.isSessionLost(e)) throw e;
      if (this.client) {
        const stale = this.client;
        this.reset();
        stale.then((c) => c.close()).catch(() => {});
      }
      return op(await this.session());
    }
  }

  private session(): Promise<Client> {
    if (this.closed) return Promise.reject(new Error("The MCP connection is closed"));
    if (!this.client) {
      const { Client, StreamableHTTPClientTransport } = this.sdk;
      const client = new Client(CLIENT_INFO, { capabilities: CLIENT_CAPABILITIES });
      client.setNotificationHandler("notifications/tools/list_changed", () => {
        this.toolList = undefined;
      });
      const transport = new StreamableHTTPClientTransport(
        new URL(this.options.url ?? this.target.url),
        {
          fetch: withClientCapabilities(
            this.options.fetch ?? hostFetch(this.target.module),
            CLIENT_CAPABILITIES,
          ),
        },
      );
      const connected = client.connect(transport).then(() => client);
      this.client = connected;
      connected.catch(() => {
        if (this.client === connected) this.client = undefined;
      });
    }
    return this.client;
  }

  private reset(): void {
    this.client = undefined;
    this.toolList = undefined;
    this.apps.clear();
  }

  private isSessionLost(e: unknown): boolean {
    return this.sdk.SdkHttpError.isInstance(e) && e.status === 404;
  }
}

function appResource(uri: string, result: ReadResourceResult): McpAppResource {
  const content = result.contents.find((c) => c.uri === uri) ?? result.contents[0];
  if (!content) throw new Error(`${uri} has no contents`);
  if (content.mimeType !== MCP_APP_MIME_TYPE) {
    throw new Error(`${uri} is ${content.mimeType ?? "untyped"}, not an MCP App`);
  }
  const html =
    "text" in content && typeof content.text === "string"
      ? content.text
      : "blob" in content && typeof content.blob === "string"
        ? decodeBase64(content.blob)
        : undefined;
  if (html === undefined) throw new Error(`${uri} has no HTML`);
  const ui = (content._meta as { ui?: McpAppResource["meta"] } | undefined)?.ui ?? {};
  return {
    uri,
    html,
    meta: { csp: ui.csp, permissions: ui.permissions, prefersBorder: ui.prefersBorder },
  };
}

function decodeBase64(blob: string): string {
  const bytes = Uint8Array.from(atob(blob), (c) => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}
