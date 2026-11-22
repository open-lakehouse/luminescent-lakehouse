// Copying to the clipboard, the one way the shell does it: the text goes on the
// clipboard and a toast says so (or what went wrong).

import { toast } from "sonner";
import { message } from "./WorkspaceBar";

/** Put `text` on the clipboard, and say so. */
export function copyToClipboard(text: string) {
  void navigator.clipboard.writeText(text).then(
    () => toast.success("Copied to the clipboard", { description: text }),
    (e: unknown) => toast.error(`Copy: ${message(e)}`),
  );
}
