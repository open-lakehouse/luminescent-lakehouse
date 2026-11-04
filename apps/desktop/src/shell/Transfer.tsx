// Import and export, as `trestle ws import` / `ws export` / `env export` do: a
// workspace directory (`trestle-workspace.toml` with one environment directory per
// member) or an environment directory (`env.toml` and its module manifests). The
// directories come from the native picker; an export writes a new directory under
// the one picked, never over files.

import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@open-lakehouse/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { FolderInput, FolderOutput } from "lucide-react";
import { useState } from "react";
import { type Imported, useShellHost, type WorkspaceInfo } from "./shell-host";
import { message } from "./WorkspaceBar";

type Status = { tone: "ok" | "error"; text: string } | null;

function importedText(imported: Imported, into: WorkspaceInfo | undefined): string {
  return imported.kind === "workspace"
    ? `Imported workspace ${imported.workspace.name} (${imported.workspace.environments} environments)`
    : `Imported ${imported.environment.name} into ${into?.name ?? "the workspace"} as revision 1`;
}

/** Import and Export, next to the workspace picker. */
export function Transfer({
  workspace,
  onImported,
}: {
  workspace: WorkspaceInfo | undefined;
  /** A workspace was imported: open it. */
  onImported: (workspaceId: string) => void;
}) {
  const shell = useShellHost();
  const [status, setStatus] = useState<Status>(null);
  const [exporting, setExporting] = useState(false);

  const run = async (action: () => Promise<string | null>) => {
    try {
      const text = await action();
      if (text) setStatus({ tone: "ok", text });
    } catch (e) {
      setStatus({ tone: "error", text: message(e) });
    }
  };

  const importDir = () =>
    run(async () => {
      const dir = await shell.workspaces.pickDirectory(
        "Import a workspace directory or an environment directory",
      );
      if (!dir) return null;
      const imported = await shell.workspaces.import(dir, workspace?.id ?? null);
      if (imported.kind === "workspace") onImported(imported.workspace.id);
      return importedText(imported, workspace);
    });

  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        className="h-7 px-2"
        onClick={() => void importDir()}
        aria-label="Import"
        title="Import a workspace or an environment directory"
        data-testid="import"
      >
        <FolderInput className="size-4" />
      </Button>
      <Button
        variant="ghost"
        size="sm"
        className="h-7 px-2"
        disabled={!workspace}
        onClick={() => setExporting(true)}
        aria-label="Export"
        title="Export the workspace or a revision"
        data-testid="export"
      >
        <FolderOutput className="size-4" />
      </Button>
      {status && (
        <span
          className={
            status.tone === "error"
              ? "truncate text-xs text-destructive"
              : "truncate text-xs text-muted-foreground"
          }
          role={status.tone === "error" ? "alert" : "status"}
          data-testid="transfer-status"
        >
          {status.text}
        </span>
      )}
      {workspace && (
        <ExportDialog
          open={exporting}
          onOpenChange={setExporting}
          workspace={workspace}
          onDone={(next) => {
            setExporting(false);
            setStatus(next);
          }}
        />
      )}
    </>
  );
}

function ExportDialog({
  open,
  onOpenChange,
  workspace,
  onDone,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  workspace: WorkspaceInfo;
  onDone: (status: Status) => void;
}) {
  const shell = useShellHost();
  const environments = useQuery({
    queryKey: ["export-environments", workspace.id],
    queryFn: () => shell.workspaces.environments(workspace.id),
    enabled: open,
  });
  const [picked, setPicked] = useState<Record<string, number>>({});
  const [problem, setProblem] = useState<string | null>(null);

  const exportTo = async (title: string, write: (parent: string) => Promise<string>) => {
    setProblem(null);
    try {
      const parent = await shell.workspaces.pickDirectory(title);
      if (!parent) return;
      onDone({ tone: "ok", text: `Exported to ${await write(parent)}` });
    } catch (e) {
      setProblem(message(e));
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent data-testid="export-dialog">
        <DialogHeader>
          <DialogTitle>Export</DialogTitle>
          <DialogDescription>
            Into a new directory under the one you choose, as the trestle CLI writes it.
          </DialogDescription>
        </DialogHeader>
        <ul className="space-y-2 text-sm">
          <li className="flex items-center gap-2">
            <span className="flex-1">
              Workspace <span className="font-medium">{workspace.name}</span>
              <span className="text-muted-foreground"> · each environment's head revision</span>
            </span>
            <Button
              size="sm"
              variant="outline"
              data-testid="export-workspace"
              onClick={() =>
                void exportTo(`Export ${workspace.name} into…`, (parent) =>
                  shell.workspaces.exportWorkspace(workspace.id, parent),
                )
              }
            >
              Choose folder…
            </Button>
          </li>
          {environments.data?.map((env) => {
            const head = env.head;
            const revision = picked[env.id] ?? head;
            return (
              <li key={env.id} className="flex items-center gap-2">
                <span className="flex-1">
                  Environment <span className="font-medium">{env.name}</span>
                </span>
                {head === null ? (
                  <span className="text-xs text-muted-foreground">no revision yet</span>
                ) : (
                  <>
                    <select
                      aria-label={`Revision of ${env.name}`}
                      className="h-8 rounded-md border bg-background px-2 text-sm"
                      value={revision ?? undefined}
                      onChange={(e) => setPicked({ ...picked, [env.id]: Number(e.target.value) })}
                    >
                      {Array.from({ length: head }, (_, i) => head - i).map((n) => (
                        <option key={n} value={n}>
                          #{n}
                        </option>
                      ))}
                    </select>
                    <Button
                      size="sm"
                      variant="outline"
                      data-testid={`export-revision-${env.name}`}
                      onClick={() =>
                        void exportTo(`Export ${env.name} #${revision} into…`, (parent) =>
                          shell.workspaces.exportRevision(env.id, revision as number, parent),
                        )
                      }
                    >
                      Choose folder…
                    </Button>
                  </>
                )}
              </li>
            );
          })}
        </ul>
        {problem && (
          <p className="text-xs text-destructive" role="alert">
            {problem}
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}
