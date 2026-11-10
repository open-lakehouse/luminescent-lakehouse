import { expect, test } from "bun:test";
import { diffHeight } from "./diffHeight";

test("a diff is as tall as its longer side, within bounds", () => {
  expect(diffHeight(undefined, "a\nb")).toBe(4 * 19 + 8);
  expect(diffHeight("x\n".repeat(10), "x\n".repeat(12))).toBe(13 * 19 + 8);
  expect(diffHeight(undefined, "x\n".repeat(500))).toBe(420);
});
