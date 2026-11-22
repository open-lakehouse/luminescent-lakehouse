// Where an environment's models come from (docs/ai-gateway.md, "Upstreams"): the
// providers' own keys, a Databricks CLI profile, whose tokens the app serves to the
// containers while the environment runs (docs/desktop.md, "The token endpoint"), or the
// Neon AI Gateway of the environment's branch, whose token the branch's binding gives
// the launch. Asked at the first Start of an environment that runs MLflow or Omnigent,
// and from its "Models" action; it also lists what a Start would miss: the declared
// secrets nothing sets.

import {
  Badge,
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@open-lakehouse/ui-kit";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type FormEvent, useState } from "react";
import { ChoiceList, ChoiceRow, DialogBody, fieldClass } from "./choices";
import { Failure } from "./RunPanel";
import {
  type GatewayProblem,
  type ModelsChoice,
  type ModelsInfo,
  type NeonGateway,
  type SecretNeeds,
  useShellHost,
  type VarEntry,
  type VarsView,
  type VarTarget,
} from "./shell-host";
import { VarField } from "./VarField";
import { useVars } from "./VariablesDialog";
import { message } from "./WorkspaceBar";

export const modelsQueryKey = (environment: string) => ["models", environment] as const;
export const missingQueryKey = (environment: string) => ["missing-secrets", environment] as const;

/** The provider keys MLflow's gateway (or Omnigent's host) reads. */
const PROVIDER_KEYS = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY"];

/** Where `key` is set among the environment's var sets, the strongest first (as the
 *  runtime resolves them), or `null`. */
export function whereSet(view: VarsView | undefined, key: string): string | null {
  if (!view) return null;
  const sets: [string, VarEntry[] | undefined][] = [
    ["this machine", view.local],
    [`revision ${view.revision?.number ?? ""}`.trim(), view.revision?.vars],
    ["the environment", view.environment],
    ["the workspace", view.workspace],
  ];
  return sets.find(([, vars]) => vars?.some((v) => v.key === key))?.[0] ?? null;
}

/** The provider keys, each with where it is set and a field to set it: a keychain secret
 *  on the workspace, so every environment inherits it (on a team's store, this machine's
 *  vars for the environment: colleagues' keychains don't hold it). The variable editors'
 *  write path (`VarsHost.set`). */
function ProviderKeys({ environmentId }: { environmentId: string }) {
  const vars = useVars(environmentId, null);
  const view = vars.data;
  if (vars.isError) return <Failure error={vars.error} />;
  if (!view) return null;
  const target: VarTarget = view.shared
    ? { scope: "local", environment: environmentId }
    : { scope: "workspace", workspace: view.workspaceId };
  return (
    <div className="space-y-1.5" data-testid="models-keys-fields">
      {PROVIDER_KEYS.map((key) => {
        const where = whereSet(view, key);
        return (
          <div key={key} className="space-y-0.5">
            <VarField
              target={target}
              fixedKey={key}
              secretOnly
              placeholder={where ? "set: type to replace" : "not set"}
              testId={`models-key-${key}`}
            />
            {where && (
              <p
                className="pl-1 text-[11px] text-muted-foreground"
                data-testid={`models-key-${key}-where`}
              >
                Set on {where}
              </p>
            )}
          </div>
        );
      })}
      <p className="text-[11px] text-muted-foreground">
        {view.shared
          ? "Kept in this machine's keychain, for this environment."
          : "Kept in the keychain, on the workspace: every environment of it inherits them."}{" "}
        Variables has the rest.
      </p>
    </div>
  );
}

/** The choice in a few words, for the header and the action. */
export function modelsLabel(info: ModelsInfo): string {
  switch (info.choice.kind) {
    case "databricksProfile":
      return `Databricks · ${info.choice.profile}`;
    case "neonAiGateway":
      return "Neon AI Gateway";
    case "keys":
      return "Provider keys";
  }
}

type Kind = ModelsChoice["kind"];

export const neonGatewayQueryKey = (environment: string) => ["neon-gateway", environment] as const;

/** A gateway problem, in a sentence. */
export function problemText(problem: GatewayProblem): string {
  switch (problem.kind) {
    case "region":
      return `The project's region${problem.region ? ` (${problem.region})` : ""} has no Neon AI Gateway.`;
    case "paidPlan":
      return "The Neon AI Gateway needs a paid Neon plan (Launch or Scale).";
    case "credits":
      return "The organization has no Neon AI Gateway credits: its models stay locked until credits are added.";
    case "noModels":
      return `The account may call none of the gateway's models${problem.reason ? ` (${problem.reason})` : ""}.`;
    case "unreachable":
      return `The gateway didn't answer: ${problem.message}`;
  }
}

/** Whether nothing can be done on the branch but move it or change plans: Save waits. */
const blocks = (problem: GatewayProblem | null) =>
  problem?.kind === "region" || problem?.kind === "paidPlan";

/** The branch's gateway, under the picked Neon option: its models, or what is wrong. */
function NeonGatewayStatus({
  gateway,
  checking,
  onRecheck,
}: {
  gateway: NeonGateway;
  checking: boolean;
  /** Ask again: after adding credits, or changing plans. */
  onRecheck: () => void;
}) {
  const shell = useShellHost();
  const { problem } = gateway;
  const link =
    problem?.kind === "credits" && problem.recoveryUrl ? problem.recoveryUrl : gateway.consoleUrl;
  if (problem) {
    return (
      <div
        className="space-y-1.5 rounded-md border border-destructive/40 bg-destructive/5 px-2.5 py-2 text-xs"
        role="alert"
        data-testid="models-neon-problem"
        data-problem={problem.kind}
      >
        <p className="text-destructive">{problemText(problem)}</p>
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-6 px-2 text-xs"
          onClick={() => void shell.openExternal(link)}
          data-testid="models-neon-console"
        >
          {problem.kind === "credits"
            ? "Add credits in the Neon Console"
            : "Open in the Neon Console"}
        </Button>{" "}
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="h-6 px-2 text-xs"
          disabled={checking}
          onClick={onRecheck}
          data-testid="models-neon-recheck"
        >
          {checking ? "Checking…" : "Check again"}
        </Button>
      </div>
    );
  }
  const shown = gateway.models.slice(0, 8);
  return (
    <div className="space-y-1.5 text-xs" data-testid="models-neon-models">
      <p className="text-muted-foreground">
        {gateway.models.length} model{gateway.models.length === 1 ? "" : "s"} on branch{" "}
        <span className="font-mono">{gateway.branchName}</span>
        {gateway.locked > 0 ? `, ${gateway.locked} locked for the account` : ""}:
      </p>
      <div className="flex flex-wrap gap-1">
        {shown.map((m) => (
          <Badge key={m.id} variant="outline" className="font-mono" title={m.name}>
            {m.id}
          </Badge>
        ))}
        {gateway.models.length > shown.length && (
          <Badge variant="outline">+{gateway.models.length - shown.length} more</Badge>
        )}
      </div>
      {gateway.missingSeeded.length > 0 && (
        <p className="text-destructive" data-testid="models-neon-missing">
          The environment asks for{" "}
          <span className="font-mono">{gateway.missingSeeded.join(", ")}</span>, which the gateway
          doesn't serve to the account: those calls fail.
        </p>
      )}
    </div>
  );
}

export function ModelsDialog({
  environmentId,
  environment,
  neonBranch,
  startAfter,
  onSaved,
  onClose,
}: {
  /** The store's id. */
  environmentId: string;
  environment: string;
  /** The Neon branch it runs on, or will at the next Start: the Neon option needs one. */
  neonBranch: string | null;
  /** Opened from Start: the button saves and starts. */
  startAfter: boolean;
  /** Saved, and nothing a Start needs is missing: start now, if `startAfter`. */
  onSaved: () => void;
  onClose: () => void;
}) {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  const info = useQuery({
    queryKey: modelsQueryKey(environmentId),
    queryFn: () => shell.models.get(environmentId),
  });
  const missing = useQuery({
    queryKey: missingQueryKey(environmentId),
    queryFn: () => shell.models.missingSecrets(environmentId),
  });
  const [kind, setKind] = useState<Kind | null>(null);
  const [profile, setProfile] = useState<string | null>(null);
  const current = info.data?.choice;
  const chosenKind: Kind =
    kind ?? (current?.kind === "neonAiGateway" && !neonBranch ? "keys" : (current?.kind ?? "keys"));
  const gateway = useQuery({
    queryKey: neonGatewayQueryKey(environmentId),
    queryFn: () => shell.models.neonGateway(environmentId),
    staleTime: 60_000,
    retry: false,
    enabled: chosenKind === "neonAiGateway" && !!neonBranch,
  });
  const profiles = useQuery({
    queryKey: ["databricks-profiles"],
    queryFn: () => shell.models.profiles(),
    staleTime: 60_000,
    enabled: chosenKind === "databricksProfile",
  });
  const pickedProfile =
    profile ??
    (current?.kind === "databricksProfile" ? current.profile : null) ??
    profiles.data?.find((p) => p.authType === "databricks-cli")?.name ??
    profiles.data?.[0]?.name ??
    null;
  const picked = profiles.data?.find((p) => p.name === pickedProfile);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const [needsCommit, setNeedsCommit] = useState(false);
  const [blocked, setBlocked] = useState<SecretNeeds | null>(null);

  const choice: ModelsChoice | null =
    chosenKind === "keys"
      ? { kind: "keys" }
      : chosenKind === "neonAiGateway"
        ? gateway.data && !blocks(gateway.data.problem)
          ? { kind: "neonAiGateway" }
          : null
        : picked
          ? { kind: "databricksProfile", profile: picked.name, host: picked.host }
          : null;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!choice) return;
    setSaving(true);
    setError(null);
    try {
      const applied = await shell.models.set(environmentId, choice);
      await queryClient.invalidateQueries({ queryKey: modelsQueryKey(environmentId) });
      if (applied.needsCommit) {
        setNeedsCommit(true);
        return;
      }
      // A commit changes the head: the run view and what it needs.
      void queryClient.invalidateQueries({ queryKey: ["run"] });
      const after = await queryClient.fetchQuery({
        queryKey: missingQueryKey(environmentId),
        queryFn: () => shell.models.missingSecrets(environmentId),
        staleTime: 0,
      });
      if (startAfter && after.required.length > 0) {
        setBlocked(after);
        return;
      }
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e : new Error(message(e)));
    } finally {
      setSaving(false);
    }
  };

  const needs = blocked ?? missing.data;
  const noProviderKey =
    chosenKind === "keys" &&
    !!needs &&
    PROVIDER_KEYS.every((k) => needs.optional.some((n) => n.name === k));
  const wantsModels = info.data?.wantsModels ?? true;

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent data-testid="models-dialog" className="max-w-lg">
        <form onSubmit={submit} className="min-w-0">
          <DialogHeader>
            <DialogTitle>Models for {environment}</DialogTitle>
            <DialogDescription>
              {wantsModels
                ? "Where its agents get models from. It applies at the next Start."
                : "It runs neither MLflow nor Omnigent, so nothing in it takes models."}
            </DialogDescription>
          </DialogHeader>
          <DialogBody>
            {info.isError && <Failure error={info.error} />}
            {wantsModels && (
              <ChoiceList label="Models">
                <ChoiceRow
                  name="models"
                  checked={chosenKind === "keys"}
                  onPick={() => setKind("keys")}
                  testId="models-keys"
                  title="Provider keys"
                  hint="The providers' own keys, kept in the keychain (or the app's environment)"
                >
                  <ProviderKeys environmentId={environmentId} />
                </ChoiceRow>
                <ChoiceRow
                  name="models"
                  checked={chosenKind === "databricksProfile"}
                  onPick={() => setKind("databricksProfile")}
                  testId="models-databricks"
                  title="Databricks workspace"
                  hint="Through a Databricks CLI profile; tokens are minted while it runs, never stored"
                >
                  {profiles.isPending && (
                    <p className="text-xs text-muted-foreground">Reading the CLI's profiles…</p>
                  )}
                  {profiles.isError && <Failure error={profiles.error} />}
                  {profiles.data?.length === 0 && (
                    <p className="text-xs text-muted-foreground">
                      The CLI has no profiles: run{" "}
                      <span className="font-mono">databricks auth login</span> first.
                    </p>
                  )}
                  {profiles.data && profiles.data.length > 0 && (
                    <select
                      aria-label="Profile"
                      className={fieldClass}
                      value={pickedProfile ?? ""}
                      onChange={(e) => setProfile(e.target.value)}
                      data-testid="models-profile"
                    >
                      {profiles.data.map((p) => (
                        <option key={p.name} value={p.name}>
                          {p.name} · {p.host || "no host"}
                          {p.authType ? ` (${p.authType})` : ""}
                        </option>
                      ))}
                    </select>
                  )}
                  {picked && picked.authType !== "databricks-cli" && (
                    <p className="text-xs text-muted-foreground">
                      Only U2M profiles (<span className="font-mono">databricks-cli</span>) are
                      tested; this one signs in as{" "}
                      <span className="font-mono">{picked.authType ?? "unknown"}</span>.
                    </p>
                  )}
                </ChoiceRow>
                <ChoiceRow
                  name="models"
                  checked={chosenKind === "neonAiGateway"}
                  onPick={() => setKind("neonAiGateway")}
                  disabled={!neonBranch}
                  testId="models-neon"
                  title="Neon AI Gateway"
                  hint={
                    neonBranch ? (
                      <>
                        The AI Gateway of branch <span className="font-mono">{neonBranch}</span>
                        {", whose token comes from the branch at Start, never stored"}
                      </>
                    ) : (
                      <span data-testid="models-neon-reason">
                        Pick a Neon branch for the environment first: it is the branch's gateway
                      </span>
                    )
                  }
                >
                  {gateway.isPending && (
                    <p className="text-xs text-muted-foreground">Asking the branch's gateway…</p>
                  )}
                  {gateway.isError && <Failure error={gateway.error} />}
                  {gateway.data && (
                    <NeonGatewayStatus
                      gateway={gateway.data}
                      checking={gateway.isFetching}
                      onRecheck={() => void gateway.refetch()}
                    />
                  )}
                </ChoiceRow>
              </ChoiceList>
            )}
            {needs && needs.required.length > 0 && (
              <div
                className="space-y-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2.5 text-sm"
                role="alert"
                data-testid="models-missing"
              >
                <p className="font-medium text-destructive">Start fails without these secrets</p>
                <ul className="space-y-0.5">
                  {needs.required.map((n) => (
                    <li key={n.name}>
                      <span className="font-mono">{n.name}</span>
                      {n.help && <span className="text-xs text-muted-foreground"> · {n.help}</span>}
                    </li>
                  ))}
                </ul>
                <p className="text-xs text-muted-foreground">
                  Set them in Variables (as secrets, in the keychain), or in the environment the app
                  starts from.
                </p>
              </div>
            )}
            {noProviderKey && wantsModels && (
              <p className="text-xs text-muted-foreground" data-testid="models-no-keys">
                No provider key is set yet: the gateway will have no models.
              </p>
            )}
            {needsCommit && (
              <p
                className="text-sm text-destructive"
                role="alert"
                data-testid="models-needs-commit"
              >
                Saved in the draft, which has other changes too: commit it (History) to start with
                these models.
              </p>
            )}
            {error && <Failure error={error} />}
          </DialogBody>
          <DialogFooter>
            <Button type="button" variant="ghost" onClick={onClose}>
              {needsCommit ? "Close" : "Cancel"}
            </Button>
            <Button
              type="submit"
              disabled={saving || !choice || needsCommit || !wantsModels}
              data-testid="models-save"
            >
              {startAfter ? "Save and start" : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
