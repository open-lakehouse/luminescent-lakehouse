// Doctor reports for tests, Storybook-style fakes and the Playwright IPC mock.

import type { DoctorReport } from "./shell-host";

export const READY_REPORT: DoctorReport = {
  docker: { status: "found", path: "/opt/homebrew/bin/docker", version: "29.5.2" },
  compose: { status: "found", path: "/opt/homebrew/bin/docker", version: "5.6.1" },
  daemon: {
    status: "up",
    version: "28.5.2",
    flavour: "orbStack",
    context: "orbstack",
    os: "OrbStack",
  },
  neon: { status: "missing" },
  path: ["/opt/homebrew/bin", "/usr/bin", "/bin"],
};

export const DAEMON_DOWN_REPORT: DoctorReport = {
  ...READY_REPORT,
  daemon: {
    status: "down",
    message: "Cannot connect to the Docker daemon at unix:///var/run/docker.sock.",
  },
};
