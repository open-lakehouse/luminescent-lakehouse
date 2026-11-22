// One variable's entry: its name (unless fixed), whether it is a secret, its value, and
// Set. A secret's value is typed into a password field and goes to the keychain; the
// var set gets only its reference (shell-core's `vars`). The variable editors and the
// Models dialog's provider keys both set variables through it, so both go through one
// write path (`VarsHost.set`) and one way of reporting a failed write (a toast).

import { Button, Input, Label, Switch } from "@open-lakehouse/ui-kit";
import { useQueryClient } from "@tanstack/react-query";
import { type KeyboardEvent, useState } from "react";
import { toast } from "sonner";
import { useShellHost, type VarTarget } from "./shell-host";
import { message } from "./WorkspaceBar";

/** Everything a variable write can change: the var sets, what Start would miss, the
 *  effective env. */
export function invalidateVars(queryClient: ReturnType<typeof useQueryClient>) {
  for (const queryKey of [["vars"], ["missing-secrets"], ["effective-env"]]) {
    void queryClient.invalidateQueries({ queryKey });
  }
}

/** Where a write went, in a few words. */
export function scopeWords(target: VarTarget): string {
  switch (target.scope) {
    case "workspace":
      return "the workspace";
    case "environment":
      return "the environment";
    case "draft":
      return "the next revision";
    case "local":
      return "this machine";
    case "instance":
      return "the instance";
  }
}

/** Set a variable, and say so (or what went wrong) in a toast. Resolves to whether it
 *  was written. */
export function useSetVar() {
  const shell = useShellHost();
  const queryClient = useQueryClient();
  return async (
    target: VarTarget,
    key: string,
    value: string,
    secret: boolean,
  ): Promise<boolean> => {
    try {
      const went = await shell.vars.set(target, key, {
        kind: secret ? "secret" : "literal",
        value,
      });
      if (went.scope !== target.scope) {
        toast.success(`${key} is kept on ${scopeWords(went)}`, {
          description:
            "A secret on a team's store stays on this machine: no colleague's keychain holds it.",
        });
      } else {
        toast.success(`${key} set on ${scopeWords(went)}`);
      }
      return true;
    } catch (e) {
      toast.error(`${key}: ${message(e)}`);
      return false;
    } finally {
      invalidateVars(queryClient);
    }
  };
}

export function VarField({
  target,
  fixedKey,
  secretOnly,
  secretDisabled,
  testId,
  placeholder,
}: {
  target: VarTarget;
  /** The variable's name, when it is given (a provider key). */
  fixedKey?: string;
  /** Always a secret (no switch). */
  secretOnly?: boolean;
  /** Why a secret can't go here, if it can't (a team's workspace). */
  secretDisabled?: string;
  testId: string;
  placeholder?: string;
}) {
  const setVar = useSetVar();
  const [key, setKey] = useState(fixedKey ?? "");
  const [value, setValue] = useState("");
  const [secret, setSecret] = useState(!!secretOnly);
  const [saving, setSaving] = useState(false);
  const isSecret = !secretDisabled && (secretOnly || secret);

  // Not a <form>: the Models dialog's fields sit inside its own form.
  const submit = async () => {
    const name = (fixedKey ?? key).trim();
    if (!name) return;
    setSaving(true);
    const ok = await setVar(target, name, value, isSecret);
    setSaving(false);
    if (ok) {
      setValue("");
      if (!fixedKey) setKey("");
    }
  };

  const onEnter = (e: KeyboardEvent) => {
    if (e.key !== "Enter" || !(e.target instanceof HTMLInputElement)) return;
    e.preventDefault();
    e.stopPropagation();
    void submit();
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: Enter in either input sets it, as a form would
    <div className="flex items-center gap-2" onKeyDown={onEnter} data-testid={testId}>
      {fixedKey ? (
        <span className="w-44 shrink-0 truncate font-mono text-xs">{fixedKey}</span>
      ) : (
        <Input
          aria-label="Name"
          className="h-8 w-44 shrink-0 font-mono text-xs"
          placeholder="NAME"
          value={key}
          onChange={(e) => setKey(e.target.value)}
          data-testid={`${testId}-key`}
        />
      )}
      <Input
        aria-label={fixedKey ? `${fixedKey} value` : "Value"}
        className="h-8 min-w-0 flex-1 font-mono text-xs"
        type={isSecret ? "password" : "text"}
        autoComplete="off"
        placeholder={placeholder ?? (isSecret ? "secret, kept in the keychain" : "value")}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        data-testid={`${testId}-value`}
      />
      {!secretOnly && (
        <div className="flex shrink-0 items-center gap-1.5" title={secretDisabled}>
          <Switch
            id={`${testId}-secret`}
            checked={isSecret}
            disabled={!!secretDisabled}
            onCheckedChange={setSecret}
            data-testid={`${testId}-secret`}
          />
          <Label htmlFor={`${testId}-secret`} className="text-xs">
            Secret
          </Label>
        </div>
      )}
      <Button
        type="button"
        size="sm"
        className="h-8"
        disabled={saving || !(fixedKey ?? key).trim() || (isSecret && !value)}
        onClick={() => void submit()}
        data-testid={`${testId}-set`}
      >
        Set
      </Button>
    </div>
  );
}
