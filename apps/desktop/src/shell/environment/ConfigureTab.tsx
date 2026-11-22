// An environment's Configure tab, a sub-tab next to Diagram, Files and Changes: what
// its next Start binds, on one flat page. On the left, its sections, each with its
// current choice and a dot when a Start would stop on it. On the right, the section
// picked:
//
// - Neon branch: where it runs on Neon (NeonTargetForm), for an environment that puts
//   anything there;
// - Models: where its agents get models from (ModelsForm), for one that runs MLflow
//   or Omnigent;
// - Variables: its var sets (VariablesEditor).
//
// The section picked is the host's (environment/index.tsx), so a header detail, ⌘K
// and Start can open the tab on one. A Start that lacks a branch or models opens it
// on the section it waits on: its banner says so, and its Save starts it. While the
// environment runs, the branch and the models are shown read-only: Start binds them.

import { Button, cn } from "@open-lakehouse/ui-kit";
import { useQuery } from "@tanstack/react-query";
import { Bot, Braces, Cloud } from "lucide-react";
import type { ReactNode } from "react";
import type { FormState } from "../choices";
import { ModelsForm, missingQueryKey, modelsLabel } from "../ModelsForm";
import { NeonTargetForm } from "../NeonTargetForm";
import { type NeonTarget, useShellHost } from "../shell-host";
import { VariablesEditor } from "../VariablesEditor";
import type { EnvironmentModel } from "./model";
import { busyReason } from "./run";

/** The editor's id of the sub-tab. */
export const CONFIGURE_TAB = "configure";

export type ConfigureSection = "neon" | "models" | "variables";

/** The sections `m` has, in the nav's order. */
export function configureSections(m: EnvironmentModel): ConfigureSection[] {
  return [
    ...(m.onNeon || m.target ? (["neon"] as const) : []),
    ...(m.wantsModels ? (["models"] as const) : []),
    "variables" as const,
  ];
}

/** Why `m`'s branch and models can't change now, if they can't. */
export function lockedReason(m: EnvironmentModel, what: string): string | undefined {
  if (m.running.length > 0) return `Stop ${m.name} to change ${what}: Start binds it`;
  return busyReason(m.name, m.op);
}

export function ConfigureTab({
  m,
  workspaceId,
  section: wanted,
  onSection,
  waiting,
  onCancelWaiting,
  onSaveTarget,
  onModelsSaved,
}: {
  m: EnvironmentModel;
  workspaceId: string;
  /** The section picked (one `m` lacks shows its first). */
  section: ConfigureSection | undefined;
  onSection: (section: ConfigureSection) => void;
  /** Whether a Start waits on the section shown. */
  waiting: boolean;
  onCancelWaiting: () => void;
  /** Save the Neon branch picked (and go on with a Start waiting on it). */
  onSaveTarget: (target: NeonTarget) => void;
  /** The models were saved, nothing a Start needs missing. */
  onModelsSaved: () => void;
}) {
  const shell = useShellHost();
  const sections = configureSections(m);
  const section = wanted && sections.includes(wanted) ? wanted : (sections[0] ?? "variables");
  const missing = useQuery({
    queryKey: missingQueryKey(m.env.id),
    queryFn: () => shell.models.missingSecrets(m.env.id),
    enabled: m.wantsModels && !!m.env.head,
  });
  const neonLocked = lockedReason(m, "its branch");
  const modelsLocked = lockedReason(m, "its models");

  const items: Record<ConfigureSection, NavItem> = {
    neon: {
      label: "Neon branch",
      icon: <Cloud />,
      value: m.neon?.label ?? "none picked",
      warn: m.needs?.needed && !m.target ? "Start needs a branch" : undefined,
    },
    models: {
      label: "Models",
      icon: <Bot />,
      value: m.models?.chosen ? modelsLabel(m.models) : "none chosen",
      warn:
        m.models && !m.models.chosen
          ? "Start asks for them"
          : (missing.data?.required.length ?? 0) > 0
            ? "A secret Start needs is set nowhere"
            : undefined,
    },
    variables: {
      label: "Variables",
      icon: <Braces />,
      value: "var sets and secrets",
    },
  };
  const startLabel = waiting ? "Save and start" : "Save";
  const actions =
    (testId: string, locked: string | undefined) =>
    ({ saving, canSubmit, saved }: FormState) => (
      <div className="flex items-center justify-end gap-2 pt-4">
        {waiting && (
          <Button type="button" variant="ghost" onClick={onCancelWaiting}>
            Don't start
          </Button>
        )}
        <Button
          type="submit"
          disabled={!!locked || saving || !canSubmit || saved}
          title={locked}
          data-testid={testId}
        >
          {startLabel}
        </Button>
      </div>
    );

  return (
    <div className="flex h-full min-h-0" data-testid="configure" data-section={section}>
      <nav aria-label="Configure" className="w-52 shrink-0 space-y-0.5 border-r p-2">
        {sections.map((id) => (
          <NavButton
            key={id}
            item={items[id]}
            active={id === section}
            onClick={() => onSection(id)}
            testId={`configure-nav-${id}`}
          />
        ))}
      </nav>
      <div className="min-w-0 flex-1 overflow-auto">
        <div className="max-w-3xl space-y-4 px-6 py-5">
          {section === "neon" && (
            <Section
              title={`Neon branch for ${m.name}`}
              description="Start binds it; the environment stays on it until you pick another."
              waiting={waiting}
              locked={neonLocked}
              testId="configure-neon"
            >
              <fieldset disabled={!!neonLocked} className="min-w-0">
                <NeonTargetForm
                  // A saved target (here, in the CLI) starts the draft over.
                  key={`${m.env.id}:${JSON.stringify(m.target ?? null)}`}
                  workspaceId={workspaceId}
                  needs={m.needs}
                  target={m.target}
                  onSave={onSaveTarget}
                  actions={actions("neon-target-save", neonLocked)}
                />
              </fieldset>
            </Section>
          )}
          {section === "models" && (
            <Section
              title={`Models for ${m.name}`}
              description="Where its agents get models from. It applies at the next Start."
              waiting={waiting}
              locked={modelsLocked}
              testId="configure-models"
            >
              <fieldset disabled={!!modelsLocked} className="min-w-0">
                <ModelsForm
                  key={m.env.id}
                  environmentId={m.env.id}
                  neonBranch={m.neon?.label ?? null}
                  startAfter={waiting}
                  onSaved={onModelsSaved}
                  actions={actions("models-save", modelsLocked)}
                />
              </fieldset>
            </Section>
          )}
          {section === "variables" && (
            <Section
              title={`Variables of ${m.name}`}
              description="What its modules read, applied at the next Start. A secret's value goes to the keychain; only a reference to it is stored."
              waiting={false}
              testId="configure-variables"
            >
              <VariablesEditor
                key={m.env.id}
                environmentId={m.env.id}
                environment={m.name}
                instance={m.shown ? { id: m.shown.id, revision: m.shown.revision } : null}
              />
            </Section>
          )}
        </div>
      </div>
    </div>
  );
}

interface NavItem {
  label: string;
  icon: ReactNode;
  /** The current choice, in a few words. */
  value: string;
  /** Why a Start would stop on it, if it would. */
  warn?: string;
}

function NavButton({
  item,
  active,
  onClick,
  testId,
}: {
  item: NavItem;
  active: boolean;
  onClick: () => void;
  testId: string;
}) {
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex w-full items-start gap-2 rounded-md px-2.5 py-1.5 text-left text-sm",
        active ? "bg-muted text-foreground" : "text-muted-foreground hover:bg-muted/50",
      )}
      onClick={onClick}
      data-testid={testId}
      data-warn={item.warn ? "true" : undefined}
    >
      <span className="mt-0.5 inline-flex shrink-0 [&_svg]:h-4 [&_svg]:w-4">{item.icon}</span>
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5">
          <span className="truncate font-medium">{item.label}</span>
          {item.warn && (
            <span
              className="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-500"
              title={item.warn}
              role="img"
              aria-label={item.warn}
            />
          )}
        </span>
        <span className="block truncate text-xs text-muted-foreground" title={item.value}>
          {item.value}
        </span>
      </span>
    </button>
  );
}

function Section({
  title,
  description,
  waiting,
  locked,
  testId,
  children,
}: {
  title: string;
  description: string;
  /** A Start waits on it. */
  waiting: boolean;
  /** Why it is read-only now, if it is. */
  locked?: string;
  testId: string;
  children: ReactNode;
}) {
  return (
    <section className="min-w-0 space-y-4" aria-label={title} data-testid={testId}>
      <header className="space-y-1">
        <h2 className="text-sm font-semibold">{title}</h2>
        <p className="text-xs text-muted-foreground">{description}</p>
      </header>
      {waiting && (
        <p
          className="rounded-md border border-primary/40 bg-primary/5 px-3 py-2 text-xs"
          role="status"
          data-testid="configure-waiting"
        >
          Start is waiting on this: save it to start.
        </p>
      )}
      {locked && (
        <p className="text-xs text-muted-foreground" data-testid="configure-locked">
          {locked}.
        </p>
      )}
      {children}
    </section>
  );
}
