// The current workspace, shared by the views (the editor, Run): which one is
// open, the picker over the workspaces in trestle's shared state, and a dialog
// to create one. On a machine with no workspace yet, `local` with one
// environment, `dev`, is created, as the editor's own starter does.

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
} from "@open-lakehouse/ui-kit";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import {
  createContext,
  type FormEvent,
  type ReactNode,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { useShellHost, type WorkspaceInfo } from "./shell-host";

export const WORKSPACES_QUERY_KEY = ["workspaces"] as const;
/** The workspace shown last, to reopen it. */
const LAST_WORKSPACE_KEY = "luminescent.workspace";

const STARTER = { workspace: "local", environment: "dev" };

export function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

interface Selected {
  id: string | null;
  select(id: string): void;
}

const SelectedContext = createContext<Selected | null>(null);

/** Which workspace the views show, kept across views and launches. */
export function CurrentWorkspaceProvider({ children }: { children: ReactNode }) {
  const [id, setId] = useState<string | null>(() => localStorage.getItem(LAST_WORKSPACE_KEY));
  const select = (next: string) => {
    setId(next);
    localStorage.setItem(LAST_WORKSPACE_KEY, next);
  };
  return <SelectedContext.Provider value={{ id, select }}>{children}</SelectedContext.Provider>;
}

export interface CurrentWorkspace {
  /** The workspace shown (the last opened, else the first), once known. */
  current: WorkspaceInfo | undefined;
  workspaces: WorkspaceInfo[] | undefined;
  select(id: string): void;
  /** Why there is no workspace to show. */
  problem: string | null;
}

/** The current workspace and the list it is picked from, live with the shared state. */
export function useCurrentWorkspace(): CurrentWorkspace {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const selected = useContext(SelectedContext);
  if (!selected) throw new Error("useCurrentWorkspace needs a <CurrentWorkspaceProvider>");
  const workspaces = useQuery({
    queryKey: WORKSPACES_QUERY_KEY,
    queryFn: () => shell.workspaces.list(),
  });
  const [problem, setProblem] = useState<string | null>(null);

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
  const { select } = selected;
  useEffect(() => {
    if (workspaces.data?.length !== 0 || creatingStarter.current) return;
    creatingStarter.current = true;
    shell.workspaces
      .create(STARTER.workspace, STARTER.environment)
      .then((ws) => {
        select(ws.id);
        return queryClient.invalidateQueries({ queryKey: WORKSPACES_QUERY_KEY });
      })
      .catch((e: unknown) => setProblem(message(e)));
  }, [workspaces.data, shell, queryClient, select]);

  return {
    current: workspaces.data?.find((w) => w.id === selected.id) ?? workspaces.data?.[0],
    workspaces: workspaces.data,
    select,
    problem: workspaces.isError
      ? `The shared state is unavailable: ${workspaces.error.message}`
      : problem,
  };
}

/** The bar above a view: the workspace picker, a new-workspace button, and where the
 *  state lives. */
export function WorkspaceBar({ workspace }: { workspace: CurrentWorkspace }) {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const location = useQuery({
    queryKey: ["state-location"],
    queryFn: () => shell.workspaces.location(),
  });
  const { current, workspaces, select } = workspace;
  return (
    <header className="flex h-10 shrink-0 items-center gap-2 border-b bg-sidebar px-3 text-sm">
      <span className="text-muted-foreground">Workspace</span>
      <Select value={current?.id ?? ""} onValueChange={select} disabled={!workspaces?.length}>
        <SelectTrigger className="h-7 w-48" aria-label="Workspace" data-testid="workspace-picker">
          <SelectValue placeholder="…" />
        </SelectTrigger>
        <SelectContent>
          {workspaces?.map((ws) => (
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
  );
}

export function Notice({ tone, children }: { tone?: "error"; children: ReactNode }) {
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
