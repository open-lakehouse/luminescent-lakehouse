// The MCP Apps sandbox proxy (src-tauri/assets/mcp-app/sandbox.js): the View's CSP
// and frame attributes, and the relay, on a fake window pair.

import { describe, expect, test } from "bun:test";
import {
  domains,
  isJsonRpc,
  start,
  viewAllow,
  viewCsp,
  viewSandbox,
  withCsp,
} from "../../src-tauri/assets/mcp-app/sandbox.js";

const SHELL = "tauri://localhost";

describe("the View's CSP", () => {
  test("allows inline code and declared https domains only", () => {
    expect(
      domains([
        "https://cdn.example.com",
        "http://plain.example.com",
        "https://x.com; script-src *",
        3,
      ]),
    ).toBe("https://cdn.example.com");
    const csp = viewCsp({
      connectDomains: ["https://api.example.com"],
      resourceDomains: ["https://cdn.example.com"],
    });
    expect(csp).toContain("default-src 'none'");
    expect(csp).toContain("script-src 'unsafe-inline' https://cdn.example.com");
    expect(csp).toContain("connect-src https://api.example.com");
    expect(viewCsp()).toContain("connect-src 'none'");
    expect(viewCsp()).toContain("frame-src 'none'");
    expect(viewCsp()).toContain("base-uri 'none'");
  });

  test("goes first in <head>, else after the doctype", () => {
    expect(withCsp("<html><head><title>x</title></head></html>")).toMatch(
      /^<html><head><meta http-equiv="Content-Security-Policy" content="[^"]+"><title>/,
    );
    expect(withCsp("<!doctype html><p>x")).toMatch(/^<!doctype html><meta http-equiv=/);
    expect(withCsp("<p>x")).toMatch(/^<meta http-equiv=/);
  });

  test("the frame gets scripts and forms, never same-origin", () => {
    expect(viewSandbox()).toBe("allow-scripts allow-forms");
    expect(viewSandbox("allow-same-origin allow-popups allow-top-navigation")).toBe(
      "allow-scripts allow-forms allow-popups",
    );
    expect(viewAllow({ camera: {}, clipboardWrite: {} })).toBe("camera; clipboard-write");
    expect(viewAllow(undefined)).toBe("");
  });

  test("only JSON-RPC 2.0 messages count", () => {
    expect(isJsonRpc({ jsonrpc: "2.0", method: "x" })).toBe(true);
    expect(isJsonRpc({ method: "x" })).toBe(false);
    expect(isJsonRpc("x")).toBe(false);
  });
});

/** A window the proxy runs in, framed by `origin`; what it posts is recorded. */
function fakeProxy(origin: string) {
  const posted: { data: unknown; origin: string }[] = [];
  const parent = { postMessage: (data: unknown, o: string) => posted.push({ data, origin: o }) };
  const toView: unknown[] = [];
  const viewWindow = { postMessage: (data: unknown) => toView.push(data) };
  let listener: ((e: { data: unknown; source: unknown; origin: string }) => void) | undefined;
  let frame: Record<string, unknown> | undefined;
  const win = {
    parent,
    location: { ancestorOrigins: [origin] },
    document: {
      referrer: "",
      createElement: () => {
        const attrs: Record<string, string> = {};
        frame = {
          attrs,
          contentWindow: viewWindow,
          setAttribute: (k: string, v: string) => (attrs[k] = v),
        };
        return frame;
      },
      body: { appendChild: () => {} },
    },
    addEventListener: (_: string, fn: typeof listener) => {
      listener = fn;
    },
  };
  return {
    win: win as unknown as Window,
    posted,
    toView,
    frame: () => frame,
    fromShell: (data: unknown, o = SHELL) => listener?.({ data, source: parent, origin: o }),
    fromView: (data: unknown) => listener?.({ data, source: viewWindow, origin: "null" }),
    fromOther: (data: unknown) => listener?.({ data, source: {}, origin: SHELL }),
  };
}

const rpc = (method: string, params: object = {}) => ({ jsonrpc: "2.0", method, params });

describe("the proxy", () => {
  test("does nothing unless framed by an allowed origin", () => {
    expect(start(fakeProxy("https://evil.example.com").win, [SHELL])).toBe(false);
    const p = fakeProxy(SHELL);
    expect(start(p.win, [SHELL])).toBe(true);
    expect(p.posted).toEqual([
      { data: rpc("ui/notifications/sandbox-proxy-ready"), origin: SHELL },
    ]);
  });

  test("mounts the View once, under its CSP, from the shell only", () => {
    const p = fakeProxy(SHELL);
    start(p.win, [SHELL]);
    const ready = rpc("ui/notifications/sandbox-resource-ready", {
      html: "<html><head></head><body><script>1</script></body></html>",
      csp: { connectDomains: ["https://api.example.com"] },
    });
    p.fromShell(ready, "https://evil.example.com");
    p.fromOther(ready);
    expect(p.frame()).toBeUndefined();
    p.fromShell(ready);
    const frame = p.frame() as { attrs: Record<string, string>; srcdoc: string };
    expect(frame.attrs.sandbox).toBe("allow-scripts allow-forms");
    expect(frame.srcdoc).toContain("connect-src https://api.example.com");
    p.fromShell(rpc("ui/notifications/sandbox-resource-ready", { html: "<p>again" }));
    expect((p.frame() as { srcdoc: string }).srcdoc).toBe(frame.srcdoc);
  });

  test("relays between the shell and the View, but not its own messages", () => {
    const p = fakeProxy(SHELL);
    start(p.win, [SHELL]);
    p.fromView(rpc("ui/initialize"));
    expect(p.posted).toHaveLength(1); // no View yet: nothing relayed
    p.fromShell(rpc("ui/notifications/sandbox-resource-ready", { html: "<p>x" }));
    p.fromView({ jsonrpc: "2.0", id: 0, method: "ui/initialize", params: {} });
    p.fromView(rpc("ui/notifications/sandbox-proxy-ready"));
    p.fromView({ not: "rpc" });
    expect(p.posted.slice(1)).toEqual([
      { data: { jsonrpc: "2.0", id: 0, method: "ui/initialize", params: {} }, origin: SHELL },
    ]);
    p.fromShell({ jsonrpc: "2.0", id: 0, result: {} });
    p.fromShell(rpc("ui/notifications/sandbox-other"));
    p.fromShell({ jsonrpc: "2.0", id: 1, result: {} }, "https://evil.example.com");
    p.fromOther({ jsonrpc: "2.0", id: 2, result: {} });
    expect(p.toView).toEqual([{ jsonrpc: "2.0", id: 0, result: {} }]);
  });
});
