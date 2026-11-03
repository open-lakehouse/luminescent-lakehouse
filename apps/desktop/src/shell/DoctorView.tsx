// The doctor: which Docker, Compose, daemon and Neon CLI the shell would use,
// and what to do about whatever is missing.

import {
  Badge,
  Button,
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@open-lakehouse/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import type { ReactNode } from "react";
import {
  type Daemon,
  type DaemonFlavour,
  type DoctorReport,
  isReady,
  type Tool,
  useShellHost,
} from "./shell-host";

export const DOCTOR_QUERY_KEY = ["doctor"] as const;

const FLAVOURS: Record<DaemonFlavour, string> = {
  dockerDesktop: "Docker Desktop",
  orbStack: "OrbStack",
  colima: "Colima",
  rancherDesktop: "Rancher Desktop",
  podman: "Podman",
  engine: "Docker Engine",
};

type Level = "ok" | "warn" | "error";

const BADGE: Record<Level, "success" | "outline" | "destructive"> = {
  ok: "success",
  warn: "outline",
  error: "destructive",
};

interface Row {
  id: string;
  name: string;
  level: Level;
  status: string;
  detail?: string;
  hint?: string;
}

function toolRow(
  id: string,
  name: string,
  tool: Tool,
  missing: { level: Level; hint: string },
): Row {
  switch (tool.status) {
    case "found":
      return { id, name, level: "ok", status: tool.version, detail: tool.path };
    case "broken":
      return { id, name, level: "error", status: "not working", detail: tool.message };
    case "missing":
      return { id, name, level: missing.level, status: "not found", hint: missing.hint };
  }
}

function daemonRow(daemon: Daemon): Row {
  const base = { id: "daemon", name: "Docker daemon" };
  switch (daemon.status) {
    case "up":
      return {
        ...base,
        level: "ok",
        status: `${FLAVOURS[daemon.flavour]} ${daemon.version}`,
        detail: [daemon.os, daemon.context && `context ${daemon.context}`]
          .filter(Boolean)
          .join(" · "),
      };
    case "down":
      return {
        ...base,
        level: "error",
        status: "not running",
        detail: daemon.message,
        hint: "Start Docker Desktop, OrbStack or Colima, then check again.",
      };
    case "unknown":
      return { ...base, level: "error", status: "unknown", hint: "Needs the docker CLI." };
  }
}

export function doctorRows(report: DoctorReport): Row[] {
  return [
    toolRow("docker", "docker CLI", report.docker, {
      level: "error",
      hint: "Install Docker Desktop, OrbStack or Colima (brew install colima docker).",
    }),
    toolRow("compose", "Docker Compose", report.compose, {
      level: "error",
      hint: "Install the Compose plugin (brew install docker-compose).",
    }),
    daemonRow(report.daemon),
    toolRow("neon", "Neon CLI", report.neon, {
      level: "warn",
      hint: "Optional, for Neon branches: npm install -g neon (or set NEON_CLI).",
    }),
  ];
}

export function DoctorView() {
  const shell = useShellHost();
  const query = useQuery({ queryKey: DOCTOR_QUERY_KEY, queryFn: () => shell.doctor() });

  let content: ReactNode;
  if (query.isPending) {
    content = <p className="text-sm text-muted-foreground">Checking the toolchain…</p>;
  } else if (query.isError) {
    content = (
      <p className="text-sm text-destructive" role="alert">
        The doctor failed: {query.error.message}
      </p>
    );
  } else {
    content = <Report report={query.data} />;
  }

  return (
    <div className="h-full overflow-auto p-8" data-testid="doctor-view">
      <div className="mx-auto flex max-w-3xl flex-col gap-6">
        <header className="flex items-start justify-between gap-4">
          <div>
            <h1 className="text-xl font-semibold">Doctor</h1>
            <p className="text-sm text-muted-foreground">
              What the shell finds on this machine to run local environments.
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => query.refetch()}
            disabled={query.isFetching}
          >
            <RefreshCw className={query.isFetching ? "size-4 animate-spin" : "size-4"} />
            Check again
          </Button>
        </header>
        {content}
      </div>
    </div>
  );
}

function Report({ report }: { report: DoctorReport }) {
  const ready = isReady(report);
  return (
    <>
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2" data-testid="doctor-summary">
            <Badge variant={ready ? "success" : "destructive"}>
              {ready ? "Ready" : "Not ready"}
            </Badge>
            {ready ? "Local environments can run" : "Local environments can't run yet"}
          </CardTitle>
        </CardHeader>
        <CardContent className="p-0">
          <ul className="divide-y">
            {doctorRows(report).map((row) => (
              <li
                key={row.id}
                className="flex items-start gap-4 px-6 py-3"
                data-testid={`doctor-${row.id}`}
              >
                <div className="w-40 shrink-0 text-sm font-medium">{row.name}</div>
                <div className="min-w-0 flex-1 text-sm">
                  <div className="flex items-center gap-2">
                    <Badge variant={BADGE[row.level]}>{row.status}</Badge>
                  </div>
                  {row.detail && (
                    <div
                      className="mt-1 truncate font-mono text-xs text-muted-foreground"
                      title={row.detail}
                    >
                      {row.detail}
                    </div>
                  )}
                  {row.hint && <div className="mt-1 text-xs text-muted-foreground">{row.hint}</div>}
                </div>
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>
      <Card>
        <CardHeader>
          <CardTitle className="text-base">PATH</CardTitle>
          <CardDescription>
            The login shell's PATH, ahead of the one the app was launched with.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <ol className="font-mono text-xs text-muted-foreground" data-testid="doctor-path">
            {report.path.map((entry) => (
              <li key={entry}>{entry}</li>
            ))}
          </ol>
        </CardContent>
      </Card>
    </>
  );
}
