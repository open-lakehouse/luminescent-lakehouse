import { describe, expect, test } from "bun:test";
import { branchFromAuthHost, branchFromEndpoint, isEndpoint } from "./branch";

const HOST = "ep-cool-darkness-a1b2c3d4.neonauth.c-7.eu-central-1.aws.neon.tech";
const env = {
  VITE_NEON_AUTH_HOST_SUFFIX: "neonauth.c-7.eu-central-1.aws.neon.tech",
  VITE_NEON_AUTH_DB: "neondb",
};

describe("isEndpoint", () => {
  test("accepts endpoint ids", () => {
    expect(isEndpoint("ep-cool-darkness-a1b2c3d4")).toBe(true);
    expect(isEndpoint("ep-x")).toBe(true);
  });

  test("rejects anything else", () => {
    for (const bad of [
      undefined,
      "",
      "ep-",
      "EP-COOL-DARKNESS",
      "ep-Cool",
      "cool-darkness-a1b2c3d4",
      "ep-a/b",
      "ep-a.evil.com",
      "ep-a..b",
    ]) {
      expect(isEndpoint(bad)).toBe(false);
    }
  });
});

describe("branchFromAuthHost", () => {
  test("is the Auth URL, for any region and database", () => {
    expect(branchFromAuthHost(HOST, "neondb")).toEqual({
      authUrl: `https://${HOST}/neondb/auth`,
      base: `/a/${HOST}/neondb`,
      label: HOST,
    });
    const azure = "ep-x-1.neonauth.c-2.westus3.azure.neon.tech";
    expect(branchFromAuthHost(azure, "app_db")?.authUrl).toBe(`https://${azure}/app_db/auth`);
  });

  test("refuses hosts that aren't a Neon Auth server", () => {
    for (const bad of [
      "evil.com",
      "ep-x.evil.com",
      "ep-x.neon.tech",
      "neonauth.c-7.eu-central-1.aws.neon.tech",
      `${HOST}.evil.com`,
      `${HOST}:8443`,
      "ep-x.neonauth..neon.tech",
      "EP-X.neonauth.c-7.aws.neon.tech",
      `user@${HOST}`,
      `${HOST}/x`,
    ]) {
      expect(branchFromAuthHost(bad, "neondb")).toBeUndefined();
    }
  });

  test("refuses a database that isn't one path segment", () => {
    for (const bad of ["", "a/b", "..", "neon db", "a?b", "x".repeat(64)]) {
      expect(branchFromAuthHost(HOST, bad)).toBeUndefined();
    }
    expect(branchFromAuthHost(undefined, "neondb")).toBeUndefined();
    expect(branchFromAuthHost(HOST, undefined)).toBeUndefined();
  });
});

describe("branchFromEndpoint", () => {
  test("rebuilds the Auth URL from the build's settings", () => {
    expect(branchFromEndpoint("ep-cool-darkness-a1b2c3d4", env)).toEqual({
      authUrl: `https://${HOST}/neondb/auth`,
      base: "/b/ep-cool-darkness-a1b2c3d4",
      label: "ep-cool-darkness-a1b2c3d4",
    });
  });

  test("refuses a non-endpoint", () => {
    expect(branchFromEndpoint("ep-a.evil.com", env)).toBeUndefined();
    expect(branchFromEndpoint(undefined, env)).toBeUndefined();
  });

  test("is off in a build without the settings", () => {
    expect(branchFromEndpoint("ep-x", {})).toBeUndefined();
    expect(branchFromEndpoint("ep-x", { VITE_NEON_AUTH_DB: "neondb" })).toBeUndefined();
    expect(
      branchFromEndpoint("ep-x", { VITE_NEON_AUTH_HOST_SUFFIX: env.VITE_NEON_AUTH_HOST_SUFFIX }),
    ).toBeUndefined();
  });
});
