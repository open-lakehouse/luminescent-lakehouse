// Where an embedded service UI goes, or whether it hides: the native view paints
// over the document, so anything popping over its area hides it.

import { afterEach, expect, test } from "bun:test";
import { embedBounds } from "./EmbeddedServiceUi";

type Area = { left: number; top: number; width: number; height: number };

/** `el` laid out at `area` (happy-dom lays out nothing). */
function at<T extends HTMLElement>(el: T, area: Area): T {
  el.getBoundingClientRect = () =>
    ({
      ...area,
      x: area.left,
      y: area.top,
      right: area.left + area.width,
      bottom: area.top + area.height,
    }) as DOMRect;
  return el;
}

function tabArea(area: Area = { left: 240.4, top: 72.6, width: 800.5, height: 500 }) {
  const el = at(document.createElement("div"), area);
  document.body.append(el);
  return el;
}

afterEach(() => {
  document.body.replaceChildren();
});

test("the tab's area, in whole pixels", () => {
  expect(embedBounds(tabArea())).toEqual({ x: 240, y: 73, width: 801, height: 500 });
});

test("an empty area shows nothing", () => {
  expect(embedBounds(tabArea({ left: 0, top: 0, width: 0, height: 400 }))).toBe(null);
});

test("an open modal hides it: Radix hides everything outside it", () => {
  const root = document.createElement("div");
  document.body.append(root);
  const el = at(document.createElement("div"), { left: 0, top: 0, width: 800, height: 500 });
  root.append(el);
  expect(embedBounds(el)).not.toBe(null);
  root.setAttribute("aria-hidden", "true");
  expect(embedBounds(el)).toBe(null);
});

test("a menu over the area hides it; one beside it doesn't", () => {
  const el = tabArea({ left: 240, top: 100, width: 800, height: 500 });
  const menu = document.createElement("div");
  menu.setAttribute("role", "menu");
  document.body.append(menu);
  at(menu, { left: 900, top: 40, width: 200, height: 50 });
  expect(embedBounds(el)).not.toBe(null);
  at(menu, { left: 900, top: 80, width: 200, height: 50 });
  expect(embedBounds(el)).toBe(null);
});

test("a closed (unlaid) overlay doesn't hide it", () => {
  const el = tabArea();
  const dialog = document.createElement("div");
  dialog.setAttribute("role", "dialog");
  document.body.append(at(dialog, { left: 0, top: 0, width: 0, height: 0 }));
  expect(embedBounds(el)).not.toBe(null);
});
