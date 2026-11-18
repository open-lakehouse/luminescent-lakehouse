import { afterEach, describe, expect, test } from "bun:test";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost } from "../host/fake-host";
import { DoctorView, doctorRows } from "./DoctorView";
import { DAEMON_DOWN_REPORT, READY_REPORT } from "./fixtures";
import type { DoctorReport } from "./shell-host";

afterEach(cleanup);

function renderDoctor({ doctor }: { doctor: () => Promise<DoctorReport> }) {
  const shell = createFakeShellHost({ doctor });
  return render(
    <Providers host={createFakeHost({ kind: "desktop" })} shell={shell}>
      <DoctorView />
    </Providers>,
  );
}

describe("doctorRows", () => {
  test("a ready machine is all ok, the Neon and Databricks CLIs only warnings", () => {
    const rows = doctorRows(READY_REPORT);
    expect(rows.map((r) => [r.id, r.level])).toEqual([
      ["docker", "ok"],
      ["compose", "ok"],
      ["daemon", "ok"],
      ["neon", "warn"],
      ["databricks", "warn"],
    ]);
    expect(rows[2]?.status).toBe("OrbStack 28.5.2");
  });

  test("a profile that can't refresh says how to sign in again", () => {
    const rows = doctorRows({
      ...READY_REPORT,
      databricks: { status: "found", path: "/opt/homebrew/bin/databricks", version: "1.26.0" },
      databricksProblems: [
        { instance: "i1", profile: "work", message: "run `databricks auth login -p work`" },
      ],
    });
    expect(rows.find((r) => r.id === "databricks")?.level).toBe("ok");
    const problem = rows.find((r) => r.id === "databricks-i1");
    expect(problem?.level).toBe("error");
    expect(problem?.hint).toContain("databricks auth login -p work");
  });

  test("a stopped daemon says what to do", () => {
    const daemon = doctorRows(DAEMON_DOWN_REPORT)[2];
    expect(daemon?.level).toBe("error");
    expect(daemon?.hint).toContain("Start Docker Desktop");
  });
});

describe("DoctorView", () => {
  test("shows the report", async () => {
    renderDoctor({ doctor: async () => READY_REPORT });
    expect(await screen.findByText("Local environments can run")).toBeDefined();
    expect(screen.getByTestId("doctor-docker").textContent).toContain("29.5.2");
    expect(screen.getByTestId("doctor-path").textContent).toContain("/opt/homebrew/bin");
  });

  test("not ready when the daemon is down, and checks again on demand", async () => {
    let calls = 0;
    renderDoctor({
      doctor: async () => (++calls === 1 ? DAEMON_DOWN_REPORT : READY_REPORT),
    });
    expect(await screen.findByText("Local environments can't run yet")).toBeDefined();
    fireEvent.click(screen.getByRole("button", { name: /Check again/ }));
    expect(await screen.findByText("Local environments can run")).toBeDefined();
    expect(calls).toBe(2);
  });

  test("a failing doctor is an alert, not a crash", async () => {
    renderDoctor({
      doctor: async () => {
        throw new Error("spawn failed");
      },
    });
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("spawn failed"));
  });
});
