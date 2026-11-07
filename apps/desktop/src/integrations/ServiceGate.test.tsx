import { afterEach, expect, test } from "bun:test";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { act, cleanup, render, screen } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost } from "../host/fake-host";
import { ServiceGate } from "./ServiceGate";

afterEach(cleanup);

test("names the environment that doesn't serve it", () => {
  render(
    <Providers host={createFakeHost({ kind: "desktop" })} shell={createFakeShellHost()}>
      <ServiceGate moduleId="headwaters" name="Lineage" environment="dev #2">
        <p>the lineage</p>
      </ServiceGate>
    </Providers>,
  );
  expect(screen.getByTestId("unbound-headwaters").textContent).toContain("dev #2 doesn't serve");
});

test("shows the view once the environment binds the service", () => {
  const host = createFakeHost({ kind: "desktop" });
  render(
    <Providers host={host} shell={createFakeShellHost()}>
      <ServiceGate moduleId="unity-catalog" name="Unity Catalog">
        <p>the catalog</p>
      </ServiceGate>
    </Providers>,
  );
  expect(screen.getByTestId("unbound-unity-catalog")).toBeDefined();
  expect(screen.queryByText("the catalog")).toBeNull();

  act(() => host.setService("unity-catalog", {}));
  expect(screen.getByText("the catalog")).toBeDefined();
});
