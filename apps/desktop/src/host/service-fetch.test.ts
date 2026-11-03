import { afterEach, describe, expect, test } from "bun:test";
import { ServiceUnavailableError } from "@open-lakehouse/host";
import type { Channel } from "@tauri-apps/api/core";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import type { ProxyRequest, ResponseHead } from "./bindings";
import { type BodyMessage, serviceFetch } from "./service-fetch";

interface Sent {
  requestId: number;
  request: ProxyRequest;
  onBody: Channel<BodyMessage>;
}

/** Answer `service_fetch` with `respond`; record every call. */
function mockProxy(respond: (sent: Sent) => ResponseHead | Promise<ResponseHead>) {
  const calls: { cmd: string; payload: unknown }[] = [];
  mockIPC(async (cmd, payload) => {
    calls.push({ cmd, payload });
    if (cmd === "service_fetch") return respond(payload as unknown as Sent);
    if (cmd === "service_fetch_cancel") return null;
    throw new Error(`unexpected ${cmd}`);
  });
  return calls;
}

const OK: ResponseHead = {
  status: 200,
  statusText: "OK",
  headers: [["content-type", "text/plain"]],
};
const bytes = (s: string) => new TextEncoder().encode(s).buffer as ArrayBuffer;

afterEach(() => clearMocks());

describe("serviceFetch", () => {
  test("sends a module and a path, never a URL, and streams the body", async () => {
    let sent: Sent | undefined;
    mockProxy((s) => {
      sent = s;
      // The body follows the head, as it does from Rust.
      setTimeout(() => {
        s.onBody.onmessage(bytes("hello "));
        s.onBody.onmessage(bytes("world"));
        s.onBody.onmessage("end");
      }, 0);
      return OK;
    });
    const response = await serviceFetch("unity-catalog")(
      "https://ignored.example/api/2.1/unity-catalog/catalogs?max_results=1",
      { method: "POST", headers: { accept: "application/json" }, body: "{}" },
    );
    expect(sent?.request).toEqual({
      moduleId: "unity-catalog",
      method: "POST",
      path: "/api/2.1/unity-catalog/catalogs?max_results=1",
      headers: expect.arrayContaining([["accept", "application/json"]]),
      body: [123, 125],
    });
    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toBe("text/plain");
    expect(await response.text()).toBe("hello world");
  });

  test("relative paths work and GETs carry no body", async () => {
    let sent: Sent | undefined;
    mockProxy((s) => {
      sent = s;
      setTimeout(() => s.onBody.onmessage("end"), 0);
      return OK;
    });
    const response = await serviceFetch("lineage")("/v1/jobs");
    expect(sent?.request.path).toBe("/v1/jobs");
    expect(sent?.request.body).toBeNull();
    expect(await response.text()).toBe("");
  });

  test("an unbound module is ServiceUnavailableError", async () => {
    mockIPC(async () => {
      throw { kind: "unbound", moduleId: "unity-catalog" };
    });
    const error = await serviceFetch("unity-catalog")("/").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ServiceUnavailableError);
    expect((error as ServiceUnavailableError).moduleId).toBe("unity-catalog");
  });

  test("an upstream failure is a TypeError, like a network error", async () => {
    mockIPC(async () => {
      throw { kind: "upstream", message: "connection refused" };
    });
    const error = await serviceFetch("mlflow")("/").catch((e: unknown) => e);
    expect(error).toBeInstanceOf(TypeError);
    expect(String(error)).toContain("connection refused");
  });

  test("an error mid-body errors the stream", async () => {
    mockProxy((s) => {
      setTimeout(() => {
        s.onBody.onmessage(bytes("part"));
        s.onBody.onmessage({ error: { kind: "upstream", message: "reset" } });
      }, 0);
      return OK;
    });
    const response = await serviceFetch("mlflow")("/");
    expect(response.text()).rejects.toThrow("reset");
  });

  test("204 has no body", async () => {
    mockProxy(() => ({ status: 204, statusText: "No Content", headers: [] }));
    const response = await serviceFetch("mlflow")("/", { method: "DELETE" });
    expect(response.status).toBe(204);
    expect(response.body).toBeNull();
  });

  test("cancelling the body cancels it in Rust", async () => {
    let requestId = -1;
    const calls = mockProxy((s) => {
      requestId = s.requestId;
      setTimeout(() => s.onBody.onmessage(bytes("first")), 0);
      return OK;
    });
    const response = await serviceFetch("mlflow")("/");
    const reader = response.body?.getReader();
    await reader?.read();
    await reader?.cancel();
    expect(calls).toContainEqual({ cmd: "service_fetch_cancel", payload: { requestId } });
  });

  test("aborting before the head rejects, and cancels once the head is in", async () => {
    let release: (head: ResponseHead) => void = () => {};
    let requestId = -1;
    const calls = mockProxy(
      (s) =>
        new Promise<ResponseHead>((resolve) => {
          requestId = s.requestId;
          release = resolve;
        }),
    );
    const controller = new AbortController();
    const pending = serviceFetch("mlflow")("/", { signal: controller.signal });
    await Bun.sleep(0);
    controller.abort();
    expect(pending).rejects.toThrow();
    release(OK);
    await Bun.sleep(0);
    await Bun.sleep(0);
    expect(calls).toContainEqual({ cmd: "service_fetch_cancel", payload: { requestId } });
  });
});
