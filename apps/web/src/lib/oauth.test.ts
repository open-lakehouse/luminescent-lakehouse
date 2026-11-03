import { describe, expect, test } from "bun:test";
import { consentRequest, isLoopback } from "./oauth";

const query = (q: Record<string, string | string[]>) => {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(q)) for (const x of [v].flat()) p.append(k, x);
  return p;
};

describe("consentRequest", () => {
  test("reads the client, scopes, resources and redirect host", () => {
    expect(
      consentRequest(
        query({
          client_id: "abc",
          redirect_uri: "https://app.example.com/cb",
          scope: "openid  offline_access",
          resource: ["https://headwaters.luminescent-lakehouse.dev", "https://x.dev"],
          sig: "s",
        }),
      ),
    ).toEqual({
      clientId: "abc",
      scopes: ["openid", "offline_access"],
      resources: ["https://headwaters.luminescent-lakehouse.dev", "https://x.dev"],
      redirectUri: "https://app.example.com/cb",
      redirectHost: "app.example.com",
      loopback: false,
    });
  });

  test("flags a loopback redirect, port included in the host", () => {
    const r = consentRequest(
      query({ client_id: "c", redirect_uri: "http://127.0.0.1:8765/callback" }),
    );
    expect(r?.loopback).toBe(true);
    expect(r?.redirectHost).toBe("127.0.0.1:8765");
  });

  test("is undefined without a client or a valid redirect URI", () => {
    expect(consentRequest(query({ redirect_uri: "https://a.dev/" }))).toBeUndefined();
    expect(consentRequest(query({ client_id: "c" }))).toBeUndefined();
    expect(consentRequest(query({ client_id: "c", redirect_uri: "not a url" }))).toBeUndefined();
  });
});

describe("isLoopback", () => {
  test("localhost, 127.0.0.0/8 and ::1", () => {
    for (const u of [
      "http://localhost:1/",
      "http://127.0.0.1/",
      "http://127.4.5.6:9/",
      "http://[::1]:8/",
    ]) {
      expect(isLoopback(new URL(u))).toBe(true);
    }
  });

  test("not lookalikes", () => {
    for (const u of [
      "https://localhost.evil.com/",
      "https://127.0.0.1.evil.com/",
      "https://example.com/",
      "http://10.0.0.1/",
    ]) {
      expect(isLoopback(new URL(u))).toBe(false);
    }
  });
});
