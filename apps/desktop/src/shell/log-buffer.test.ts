import { expect, test } from "bun:test";
import { LogBuffer } from "./log-buffer";
import type { LogEntry } from "./shell-host";

const line = (service: string, text: string): LogEntry => ({
  service,
  container: `${service}-1`,
  timestamp: null,
  text,
});

test("keeps the newest lines, oldest first, and counts what it dropped", () => {
  const buffer = new LogBuffer(3);
  buffer.push([line("a", "1"), line("b", "2")]);
  expect(buffer.lines().map((l) => l.text)).toEqual(["1", "2"]);
  buffer.push([line("a", "3"), line("b", "4"), line("a", "5")]);
  expect(buffer.lines().map((l) => l.text)).toEqual(["3", "4", "5"]);
  expect(buffer.length).toBe(3);
  expect(buffer.dropped).toBe(2);
});

test("filters by service; no filter is every service", () => {
  const buffer = new LogBuffer(10);
  buffer.push([line("postgres", "1"), line("rustfs", "2"), line("postgres", "3")]);
  expect(buffer.lines(new Set(["postgres"])).map((l) => l.text)).toEqual(["1", "3"]);
  expect(buffer.lines(new Set()).length).toBe(3);
  expect(buffer.lines(new Set(["envoy"]))).toEqual([]);
});
