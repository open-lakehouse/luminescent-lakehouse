// A variable's value where a row has room for one line of it (the variable editors'
// tables, the Environment panel): truncated to the row, the whole value in its
// tooltip, and a click that wraps it in place to read all of it. A literal can be
// copied. A secret's text is its mask or its `keychain:<key>` reference, never its
// value, so it can be read but isn't offered for copying.

import { Button, cn } from "@open-lakehouse/ui-kit";
import { Copy } from "lucide-react";
import { type ReactNode, useState } from "react";
import { copyToClipboard } from "./clipboard";

export function ValueText({
  value,
  secret,
  title,
  children,
  testId,
}: {
  /** The text a copy puts on the clipboard: a literal's value. */
  value: string;
  /** A secret: shown, never copied. */
  secret?: boolean;
  /** The tooltip; defaults to `value`. */
  title?: string;
  /** What is shown, if not `value` itself (a mask, a reference, `empty`). */
  children?: ReactNode;
  testId?: string;
}) {
  const [wrapped, setWrapped] = useState(false);
  return (
    <span className="flex min-w-0 flex-1 items-start gap-1">
      <button
        type="button"
        className={cn(
          "min-w-0 flex-1 cursor-text text-left font-mono text-muted-foreground",
          wrapped ? "whitespace-pre-wrap break-all" : "truncate",
        )}
        title={title ?? value}
        aria-expanded={wrapped}
        onClick={() => setWrapped((w) => !w)}
        data-testid={testId}
        data-wrapped={wrapped}
      >
        {children ?? (value || <em>empty</em>)}
      </button>
      {!secret && value && (
        <Button
          type="button"
          size="sm"
          variant="ghost"
          className="h-5 w-5 shrink-0 p-0 [&_svg]:h-3 [&_svg]:w-3"
          aria-label="Copy the value"
          title="Copy the value"
          onClick={() => copyToClipboard(value)}
          data-testid={testId && `${testId}-copy`}
        >
          <Copy />
        </Button>
      )}
    </span>
  );
}
