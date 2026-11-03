import { afterEach, expect, test } from "bun:test";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost } from "../host/fake-host";
import { ShellLayout } from "../shell/ShellLayout";
import { ServiceGate } from "./ServiceGate";

afterEach(cleanup);

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

test("the rail's service views explain an unbound service", () => {
  const host = createFakeHost({ kind: "desktop" });
  render(
    <Providers host={host} shell={createFakeShellHost()}>
      <ShellLayout />
    </Providers>,
  );
  fireEvent.click(screen.getByTestId("nav-catalog"));
  expect(screen.getByText("Unity Catalog isn't bound")).toBeDefined();
  fireEvent.click(screen.getByTestId("nav-lineage"));
  expect(screen.getByTestId("unbound-headwaters")).toBeDefined();
});
