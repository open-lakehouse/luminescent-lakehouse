import { afterEach, describe, expect, test } from "bun:test";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Providers } from "../app";
import { DoctorView, doctorRows } from "./DoctorView";
import { DAEMON_DOWN_REPORT, READY_REPORT } from "./fixtures";
import type { ShellHost } from "./shell-host";

afterEach(cleanup);

function renderDoctor(shell: ShellHost) {
  return render(
    <Providers host={createFakeHost({ kind: "desktop" })} shell={shell}>
      <DoctorView />
    </Providers>,
  );
}

describe("doctorRows", () => {
  test("a ready machine is all ok, Neon only a warning", () => {
    const rows = doctorRows(READY_REPORT);
    expect(rows.map((r) => [r.id, r.level])).toEqual([
      ["docker", "ok"],
      ["compose", "ok"],
      ["daemon", "ok"],
      ["neon", "warn"],
    ]);
    expect(rows[2]?.status).toBe("OrbStack 28.5.2");
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
