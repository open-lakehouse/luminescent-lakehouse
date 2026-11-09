import { expect, test } from "bun:test";
import {
  cliTarget,
  downCommand,
  healthCommand,
  logsCommand,
  quote,
  restartCommand,
  upCommand,
} from "./cli";

test("a target names the workspace, the environment and maybe a revision", () => {
  expect(cliTarget("local", "dev")).toBe("local/dev");
  expect(cliTarget("local", "dev", 3)).toBe("local/dev@3");
  expect(cliTarget("local", "dev", null)).toBe("local/dev");
});

test("up runs on the environment's Neon branch when it has one", () => {
  const target = cliTarget("local", "dev", 3);
  expect(upCommand(target)).toBe("trestle up local/dev@3");
  expect(upCommand(target, { kind: "existing", branch: "demo" })).toBe(
    "trestle up local/dev@3 --neon-branch demo",
  );
  expect(upCommand(target, { kind: "new", ttlHours: null })).toBe(
    "trestle up local/dev@3 --neon-branch new",
  );
  expect(upCommand(target, { kind: "new", ttlHours: 12 })).toBe(
    "trestle up local/dev@3 --neon-branch new --ttl 12h",
  );
});

test("the lifecycle verbs take the same target", () => {
  expect(downCommand("local/dev")).toBe("trestle down local/dev");
  expect(logsCommand("local/dev@2")).toBe("trestle logs --follow local/dev@2");
  expect(healthCommand("local/dev@2")).toBe("trestle health local/dev@2");
  expect(restartCommand("trestle down local/dev", "trestle up local/dev@2")).toBe(
    "trestle down local/dev && trestle up local/dev@2",
  );
});

test("a branch name that isn't one shell word is quoted", () => {
  expect(quote("preview/pr-12")).toBe("preview/pr-12");
  expect(quote("my branch")).toBe("'my branch'");
  expect(quote("it's")).toBe(`'it'\\''s'`);
});
