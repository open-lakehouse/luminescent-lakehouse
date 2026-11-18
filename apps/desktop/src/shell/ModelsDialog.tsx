// Where an environment's models come from (docs/ai-gateway.md, "Upstreams"): the
// providers' own keys, or a Databricks CLI profile, whose tokens the app serves to the
// containers while the environment runs (docs/desktop.md, "The token endpoint"). The
// Neon AI Gateway of the environment's branch comes next (#169). Asked at the first
// Start of an environment that runs MLflow or Omnigent, and from its "Models" action;
// it also lists what a Start would miss: the declared secrets nothing sets.

import {
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
import { Failure } from "./RunPanel";
import { type ModelsChoice, type ModelsInfo, type SecretNeeds, useShellHost } from "./shell-host";
import { message } from "./WorkspaceBar";

export const modelsQueryKey = (environment: string) => ["models", environment] as const;
export const missingQueryKey = (environment: string) => ["missing-secrets", environment] as const;

/** The provider keys MLflow's gateway (or Omnigent's host) reads. */
const PROVIDER_KEYS = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY"];

/** The choice in a few words, for the header and the action. */
export function modelsLabel(info: ModelsInfo): string {
  switch (info.choice.kind) {
    case "databricksProfile":
      return `Databricks · ${info.choice.profile}`;
    case "neonAiGateway":
      return "Neon AI Gateway";
    case "keys":
      return "provider keys";
  }
}

type Kind = "keys" | "databricksProfile";

export function ModelsDialog({
  environmentId,
  environment,
  startAfter,
  onSaved,
  onClose,
}: {
  /** The store's id. */
  environmentId: string;
  environment: string;
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
    kind ?? (current?.kind === "databricksProfile" ? "databricksProfile" : "keys");
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
      <DialogContent data-testid="models-dialog">
        <form onSubmit={submit} className="space-y-4">
          <DialogHeader>
            <DialogTitle>Models for {environment}</DialogTitle>
            <DialogDescription>
              {wantsModels
                ? "Where its MLflow AI Gateway (or, without MLflow, Omnigent's host) gets models from. It applies at the next Start."
                : "It runs neither MLflow nor Omnigent, so nothing in it takes models."}
            </DialogDescription>
          </DialogHeader>
          {info.isError && <Failure error={info.error} />}
          {wantsModels && (
            <fieldset className="space-y-2 text-sm" aria-label="Models">
              <label className="flex items-start gap-2">
                <input
                  type="radio"
                  name="models"
                  checked={chosenKind === "keys"}
                  onChange={() => setKind("keys")}
                  data-testid="models-keys"
                />
                <span>
                  Provider keys
                  <span className="block text-xs text-muted-foreground">
                    <span className="font-mono">ANTHROPIC_API_KEY</span>,{" "}
                    <span className="font-mono">OPENAI_API_KEY</span> or{" "}
                    <span className="font-mono">GEMINI_API_KEY</span>, as variables or in the app's
                    environment.
                  </span>
                </span>
              </label>
              <label className="flex items-start gap-2">
                <input
                  type="radio"
                  name="models"
                  checked={chosenKind === "databricksProfile"}
                  onChange={() => setKind("databricksProfile")}
                  data-testid="models-databricks"
                />
                <span className="flex-1">
                  A Databricks workspace, through a profile of the Databricks CLI
                  <span className="block text-xs text-muted-foreground">
                    The app mints its tokens while the environment runs; none is stored.
                  </span>
                </span>
              </label>
              {chosenKind === "databricksProfile" && (
                <div className="space-y-1 pl-6">
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
                      className="w-full rounded border bg-background px-1 py-0.5"
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
                </div>
              )}
              <label className="flex items-start gap-2 text-muted-foreground">
                <input type="radio" name="models" disabled data-testid="models-neon" />
                <span>
                  The Neon AI Gateway of its branch
                  <span className="block text-xs">Coming next (#169).</span>
                </span>
              </label>
            </fieldset>
          )}
          {needs && needs.required.length > 0 && (
            <div
              className="space-y-1 rounded-md border border-destructive/40 p-3 text-sm"
              role="alert"
              data-testid="models-missing"
            >
              <p className="text-destructive">Start fails without these secrets:</p>
              <ul className="space-y-1">
                {needs.required.map((n) => (
                  <li key={n.name}>
                    <span className="font-mono">{n.name}</span>
                    {n.help && <span className="text-xs text-muted-foreground"> · {n.help}</span>}
                  </li>
                ))}
              </ul>
              <p className="text-xs text-muted-foreground">
                Set them as variables (
                <span className="font-mono">trestle var set &lt;ws&gt;/&lt;env&gt; --secret</span>)
                or in the environment the app starts from.
              </p>
            </div>
          )}
          {noProviderKey && wantsModels && (
            <p className="text-xs text-muted-foreground" data-testid="models-no-keys">
              No provider key is set yet: the gateway will have no models.
            </p>
          )}
          {needsCommit && (
            <p className="text-sm text-destructive" role="alert" data-testid="models-needs-commit">
              Saved in the draft, which has other changes too: commit it (History) to start with
              these models.
            </p>
          )}
          {error && <Failure error={error} />}
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
