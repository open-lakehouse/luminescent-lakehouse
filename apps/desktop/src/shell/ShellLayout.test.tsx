import { afterEach, expect, test } from "bun:test";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { QueryClient } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { Providers } from "../app";
import { READY_REPORT } from "./fixtures";
import { ShellLayout } from "./ShellLayout";

afterEach(cleanup);

test("switches views, and clears the query cache on an environment switch", async () => {
  const host = createFakeHost({ kind: "desktop" });
  const queryClient = new QueryClient();
  let doctorCalls = 0;
  const shell = {
    doctor: async () => {
      doctorCalls++;
      return READY_REPORT;
    },
  };
  render(
    <Providers host={host} shell={shell} queryClient={queryClient}>
      <ShellLayout initialView="doctor" />
    </Providers>,
  );
  expect(await screen.findByText("Local environments can run")).toBeDefined();
  expect(queryClient.getQueryCache().getAll()).toHaveLength(1);

  act(() => host.setEnvironment({ id: "staging", name: "staging" }));
  // The doctor remounts and asks again; the old environment's entries are gone.
  expect(await screen.findByText("Local environments can run")).toBeDefined();
  expect(queryClient.getQueryCache().getAll()).toHaveLength(1);
  expect(doctorCalls).toBe(2);

  fireEvent.click(screen.getByTestId("nav-workspace"));
  expect(await screen.findByTestId("workspace-view")).toBeDefined();
});
