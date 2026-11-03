// The workspace editor: env-editor's `WorkspaceShell` on trestle's shared state
// (the store the `trestle` CLI uses) and the native planner, with a picker for
// the workspaces in that state. On a machine with no workspace yet it creates
// `local` with one environment, `dev`, as the editor's own starter does.

import { PlannerProvider, WorkspaceProvider, WorkspaceShell } from "@open-lakehouse/env-editor";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Input,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  useTheme,
} from "@open-lakehouse/ui-kit";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { type FormEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useShellHost, type WorkspaceInfo } from "./shell-host";

export const WORKSPACES_QUERY_KEY = ["workspaces"] as const;
/** The workspace shown last, to reopen it. */
const LAST_WORKSPACE_KEY = "luminescent.workspace";

const STARTER = { workspace: "local", environment: "dev" };

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

export function WorkspaceView() {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const { theme } = useTheme();
  const workspaces = useQuery({
    queryKey: WORKSPACES_QUERY_KEY,
    queryFn: () => shell.workspaces.list(),
  });
  const location = useQuery({
    queryKey: ["state-location"],
    queryFn: () => shell.workspaces.location(),
  });
  const [selected, setSelected] = useState<string | null>(() =>
    localStorage.getItem(LAST_WORKSPACE_KEY),
  );
  const [problem, setProblem] = useState<string | null>(null);

  const select = (id: string) => {
    setSelected(id);
    localStorage.setItem(LAST_WORKSPACE_KEY, id);
  };

  // A CLI `ws create` (or another window's) shows up in the picker.
  useEffect(
    () =>
      shell.workspaces.subscribe(
        () => void queryClient.invalidateQueries({ queryKey: WORKSPACES_QUERY_KEY }),
      ),
    [shell, queryClient],
  );

  // First run: no workspace anywhere yet.
  const creatingStarter = useRef(false);
  useEffect(() => {
    if (workspaces.data?.length !== 0 || creatingStarter.current) return;
    creatingStarter.current = true;
    shell.workspaces
      .create(STARTER.workspace, STARTER.environment)
      .then((ws) => {
        setSelected(ws.id);
        localStorage.setItem(LAST_WORKSPACE_KEY, ws.id);
        return queryClient.invalidateQueries({ queryKey: WORKSPACES_QUERY_KEY });
      })
      .catch((e: unknown) => setProblem(message(e)));
  }, [workspaces.data, shell, queryClient]);

  const current: WorkspaceInfo | undefined =
    workspaces.data?.find((w) => w.id === selected) ?? workspaces.data?.[0];
  const currentId = current?.id;
  const store = useMemo(
    () => (currentId ? shell.workspaces.store(currentId) : null),
    [shell, currentId],
  );

  let body: ReactNode;
  if (workspaces.isError) {
    body = (
      <Notice tone="error">The shared state is unavailable: {workspaces.error.message}</Notice>
    );
  } else if (problem) {
    body = <Notice tone="error">{problem}</Notice>;
  } else if (!store || !currentId) {
    body = <Notice>Loading workspaces…</Notice>;
  } else {
    body = (
      <PlannerProvider planner={shell.planner}>
        <WorkspaceProvider key={currentId} store={store}>
          <WorkspaceShell colorMode={theme} />
        </WorkspaceProvider>
      </PlannerProvider>
    );
  }

  return (
    <div className="flex h-full flex-col" data-testid="workspace-view">
      <header className="flex h-10 shrink-0 items-center gap-2 border-b bg-sidebar px-3 text-sm">
        <span className="text-muted-foreground">Workspace</span>
        <Select value={currentId ?? ""} onValueChange={select} disabled={!workspaces.data?.length}>
          <SelectTrigger className="h-7 w-48" aria-label="Workspace" data-testid="workspace-picker">
            <SelectValue placeholder="…" />
          </SelectTrigger>
          <SelectContent>
            {workspaces.data?.map((ws) => (
              <SelectItem key={ws.id} value={ws.id}>
                {ws.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <NewWorkspace
          onCreated={(ws) => {
            select(ws.id);
            void queryClient.invalidateQueries({ queryKey: WORKSPACES_QUERY_KEY });
          }}
        />
        <div className="flex-1" />
        {location.data && (
          <span
            className="truncate font-mono text-xs text-muted-foreground"
            title="Shared with the trestle CLI"
            data-testid="state-location"
          >
            {location.data}
          </span>
        )}
      </header>
      <div className="min-h-0 flex-1">{body}</div>
    </div>
  );
}

function Notice({ tone, children }: { tone?: "error"; children: ReactNode }) {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <p
        className={tone === "error" ? "text-sm text-destructive" : "text-sm text-muted-foreground"}
        role={tone === "error" ? "alert" : undefined}
      >
        {children}
      </p>
    </div>
  );
}

function NewWorkspace({ onCreated }: { onCreated: (ws: WorkspaceInfo) => void }) {
  const shell = useShellHost();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setProblem(null);
    shell.workspaces
      .create(name.trim(), STARTER.environment)
      .then((ws) => {
        setOpen(false);
        setName("");
        onCreated(ws);
      })
      .catch((err: unknown) => setProblem(message(err)))
      .finally(() => setBusy(false));
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <Button
        variant="ghost"
        size="sm"
        className="h-7 px-2"
        onClick={() => setOpen(true)}
        aria-label="New workspace"
        data-testid="new-workspace"
      >
        <Plus className="size-4" />
      </Button>
      <DialogContent>
        <form onSubmit={submit} className="space-y-4">
          <DialogHeader>
            <DialogTitle>New workspace</DialogTitle>
            <DialogDescription>
              In trestle's shared state, with one environment ({STARTER.environment}) to start.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-2">
            <Label htmlFor="workspace-name">Name</Label>
            <Input
              id="workspace-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="demo"
            />
            {problem && (
              <p className="text-xs text-destructive" role="alert">
                {problem}
              </p>
            )}
          </div>
          <DialogFooter>
            <Button type="submit" disabled={busy || !name.trim()}>
              Create
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
