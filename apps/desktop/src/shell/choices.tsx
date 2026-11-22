// The pieces the configuration dialogs (NeonTargetDialog, ModelsDialog) share: a
// dialog body with the padding ui-kit's header and footer have, one style for
// native selects (the tests drive them with `change` events), and a list of
// options drawn as one bordered group of radio rows.

import { cn } from "@open-lakehouse/ui-kit";
import type { ReactNode } from "react";

/** A native `<select>` (or text input) at the height and look of ui-kit's `Input`. */
export const fieldClass =
  "h-8 w-full rounded-md border border-input bg-background px-2 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50";

/** A dialog's body, between its header and its footer. ui-kit's `DialogContent` is a
 *  grid, whose item would otherwise grow to its widest unbreakable line (a long
 *  value): `min-w-0` keeps it to the dialog's width. */
export function DialogBody({ children }: { children: ReactNode }) {
  return <div className="min-w-0 space-y-5 px-5 py-4">{children}</div>;
}

/** A labelled section of a dialog's body. */
export function Section({
  label,
  htmlFor,
  children,
}: {
  label: string;
  /** The control `label` names; a group names itself (`aria-label`). */
  htmlFor?: string;
  children: ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      {htmlFor ? (
        <label htmlFor={htmlFor} className="text-xs font-medium text-muted-foreground">
          {label}
        </label>
      ) : (
        <p className="text-xs font-medium text-muted-foreground">{label}</p>
      )}
      {children}
    </div>
  );
}

/** One bordered group of `ChoiceRow`s, scrolling past a few. */
export function ChoiceList({ label, children }: { label: string; children: ReactNode }) {
  return (
    <fieldset
      aria-label={label}
      className="max-h-64 divide-y overflow-auto rounded-md border text-sm"
    >
      {children}
    </fieldset>
  );
}

/** One option of a `ChoiceList`: a radio, its title and a muted line, tags on the
 *  right, and while it is picked, `children` (its own settings). */
export function ChoiceRow({
  name,
  checked,
  onPick,
  disabled,
  testId,
  title,
  hint,
  tags,
  children,
}: {
  name: string;
  checked: boolean;
  onPick: () => void;
  disabled?: boolean;
  testId?: string;
  title: ReactNode;
  hint?: ReactNode;
  tags?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className={cn(checked && "bg-muted/40", disabled && "text-muted-foreground")}>
      <label
        className={cn(
          "flex items-start gap-2.5 px-3 py-2",
          disabled ? "cursor-not-allowed" : "cursor-pointer hover:bg-muted/50",
        )}
      >
        <input
          type="radio"
          name={name}
          className="mt-0.5"
          checked={checked}
          disabled={disabled}
          onChange={onPick}
          data-testid={testId}
        />
        <span className="min-w-0 flex-1">
          <span className="block">{title}</span>
          {hint && <span className="block text-xs text-muted-foreground">{hint}</span>}
        </span>
        {tags && <span className="flex shrink-0 items-center gap-1">{tags}</span>}
      </label>
      {checked && children && <div className="space-y-1.5 px-3 pb-2.5 pl-8">{children}</div>}
    </div>
  );
}
