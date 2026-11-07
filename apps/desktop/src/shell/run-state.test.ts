import { describe, expect, test } from "bun:test";
import { editorState, moduleStatus, runState, serves } from "./run-state";
import type { InstanceStatus, ServiceRow } from "./shell-host";

export const row = (over: Partial<ServiceRow> = {}): ServiceRow => ({
  service: "postgres",
  module: "postgres",
  state: "running",
  health: "healthy",
  status: "Up",
  exitCode: 0,
  ports: [],
  ready: true,
  ...over,
});

export const instance = (over: Partial<InstanceStatus> = {}): InstanceStatus => ({
  id: "i",
  revision: 1,
  desired: "running",
  slot: 0,
  project: "local_dev_r1",
  gatewayUrl: "http://localhost:9080",
  services: [row()],
  branch: null,
  ...over,
});

describe("runState", () => {
  test("from what is wanted and what Docker reports", () => {
    expect(runState(undefined)).toBe("stopped");
    expect(runState(instance({ desired: "stopped", services: [] }))).toBe("stopped");
    expect(runState(instance({ desired: "stopped" }))).toBe("stopping");
    expect(runState(instance())).toBe("running");
    expect(runState(instance({ services: [] }))).toBe("starting");
    expect(runState(instance({ services: [row({ health: "starting", ready: false })] }))).toBe(
      "starting",
    );
    expect(runState(instance({ services: [row({ health: "unhealthy", ready: false })] }))).toBe(
      "unhealthy",
    );
    expect(
      runState(instance({ services: [row({ state: "exited", exitCode: 1, ready: false })] })),
    ).toBe("unhealthy");
    // A one-shot job that finished doesn't keep the instance from running.
    expect(
      runState(
        instance({
          services: [row(), row({ service: "gateway-certs", state: "exited", ready: false })],
        }),
      ),
    ).toBe("running");
  });

  test("reads as the editor's states", () => {
    expect(editorState("unhealthy")).toBe("degraded");
    expect(editorState("running")).toBe("running");
  });
});

describe("a module's state on the canvas", () => {
  test("is its service's container, whatever compose calls it", () => {
    const i = instance({
      services: [
        row({ service: "unitycatalog", module: "unity-catalog" }),
        row({ service: "rustfs", module: "rustfs", health: "unhealthy", ready: false }),
        row({ service: "rustfs-init", module: null, state: "exited", exitCode: 0, ready: false }),
        row({ service: "mlflow", module: "mlflow", health: "starting", ready: false }),
      ],
    });
    expect(moduleStatus(i, "unity-catalog")).toMatchObject({ state: "running", label: "healthy" });
    expect(moduleStatus(i, "unitycatalog")).toBeUndefined();
    expect(moduleStatus(i, "rustfs")).toMatchObject({ state: "failed", label: "unhealthy" });
    expect(moduleStatus(i, "mlflow")).toMatchObject({ state: "starting" });
    expect(moduleStatus(i, "headwaters")).toBeUndefined();
    expect(serves(i, "unity-catalog")).toBe(true);
    expect(serves(i, "unitycatalog")).toBe(false);
    expect(serves(i, "headwaters")).toBe(false);
  });

  test("a module is as well as its worst service", () => {
    const i = instance({
      services: [
        row({ service: "mlflow", module: "mlflow" }),
        row({ service: "mlflow-gateway", module: "mlflow", health: "starting", ready: false }),
      ],
    });
    expect(moduleStatus(i, "mlflow")).toMatchObject({ state: "starting" });
  });

  test("an exited container says its code", () => {
    const i = instance({
      services: [row({ service: "mlflow", module: "mlflow", state: "exited", exitCode: 3 })],
    });
    expect(moduleStatus(i, "mlflow")).toMatchObject({ state: "failed", label: "exited (3)" });
  });
});
