import { describe, expect, test } from "bun:test";
import { authUrl, isEndpoint } from "./endpoint";

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

describe("authUrl", () => {
  test("rebuilds the branch's Neon Auth URL", () => {
    expect(authUrl("ep-cool-darkness-a1b2c3d4", env)).toBe(
      "https://ep-cool-darkness-a1b2c3d4.neonauth.c-7.eu-central-1.aws.neon.tech/neondb/auth",
    );
  });

  test("refuses a non-endpoint", () => {
    expect(() => authUrl("ep-a.evil.com", env)).toThrow("not a Neon endpoint id");
  });

  test("names the missing configuration", () => {
    expect(() => authUrl("ep-x", { VITE_NEON_AUTH_DB: "neondb" })).toThrow(
      "VITE_NEON_AUTH_HOST_SUFFIX",
    );
    expect(() => authUrl("ep-x", { VITE_NEON_AUTH_HOST_SUFFIX: "h" })).toThrow("VITE_NEON_AUTH_DB");
  });
});
