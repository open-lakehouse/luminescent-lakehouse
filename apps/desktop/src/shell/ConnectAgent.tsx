// The MLflow tab's "Connect an agent": the snippets that point Claude Code or Codex on
// the laptop at the instance's MLflow AI Gateway (connect-agent.ts builds them, from
// `run.connectAgent`). A dialog over the tab: the embedded page hides while it is open.

import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@open-lakehouse/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { Copy } from "lucide-react";
import { useState } from "react";
import { DialogBody } from "./choices";
import { copyToClipboard } from "./clipboard";
import { claudeCodeSnippet, codexSnippet } from "./connect-agent";
import { Failure } from "./RunPanel";
import { useShellHost } from "./shell-host";

type Agent = "claude" | "codex";

const AGENTS: { id: Agent; label: string }[] = [
  { id: "claude", label: "Claude Code" },
  { id: "codex", label: "Codex" },
];

export function ConnectAgentDialog({
  instance,
  onClose,
}: {
  instance: string;
  onClose: () => void;
}) {
  const shell = useShellHost();
  const connect = useQuery({
    queryKey: ["connect-agent", instance],
    queryFn: () => shell.run.connectAgent(instance),
  });
  const [agent, setAgent] = useState<Agent>("claude");
  const snippet = connect.data
    ? agent === "claude"
      ? claudeCodeSnippet(connect.data)
      : codexSnippet(connect.data)
    : null;
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent data-testid="connect-agent" className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>Connect an agent</DialogTitle>
          <DialogDescription>
            Point Claude Code or Codex on this machine at the environment's MLflow AI Gateway: its
            calls use the environment's models, with no key of their own.
          </DialogDescription>
        </DialogHeader>
        <DialogBody>
          {connect.isError && <Failure error={connect.error} />}
          {connect.data === null && (
            <p className="text-sm text-muted-foreground" data-testid="connect-agent-none">
              The instance has no MLflow AI Gateway yet: start it with MLflow.
            </p>
          )}
          {connect.data && (
            <div className="space-y-2">
              <div className="flex gap-1" role="tablist" aria-label="Agent">
                {AGENTS.map((a) => (
                  <Button
                    key={a.id}
                    type="button"
                    size="sm"
                    role="tab"
                    aria-selected={agent === a.id}
                    variant={agent === a.id ? "secondary" : "ghost"}
                    onClick={() => setAgent(a.id)}
                    data-testid={`connect-agent-${a.id}`}
                  >
                    {a.label}
                  </Button>
                ))}
              </div>
              <pre
                className="max-h-80 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-xs leading-relaxed"
                data-testid="connect-agent-snippet"
              >
                {snippet}
              </pre>
            </div>
          )}
        </DialogBody>
        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onClose}>
            Close
          </Button>
          <Button
            type="button"
            disabled={!snippet}
            onClick={() => snippet && copyToClipboard(snippet)}
            data-testid="connect-agent-copy"
          >
            <Copy className="mr-1 h-3.5 w-3.5" />
            Copy
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
