import { describe, expect, test } from "bun:test";
import { branchFromAuthHost } from "./branch";

const HOST = "ep-cool-darkness-a1b2c3d4.neonauth.c-7.eu-central-1.aws.neon.tech";
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
