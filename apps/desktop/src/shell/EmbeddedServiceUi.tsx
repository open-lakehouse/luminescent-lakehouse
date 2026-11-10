// A service UI with a session of its own (MLflow, Jaeger, marimo, Omnigent), shown
// in place: give it an instance and one of `run.serviceUis`' UIs, mount it where
// it should show, and it manages the rest. The page is not in this document: it
// is a native child webview of the main window (an iframe gets no cookies in
// WKWebView, docs/desktop.md), which the host lays over this component's area. A
// native view paints above the whole document, so it follows the area every frame
// and hides whenever the area is covered: a modal is open (the ⌘K palette, a
// dialog), or a menu or list pops over it. Pop out moves the UI to its own window;
// Show here brings it back. Which UIs are popped out outlives the component, so a
// remount (another tab, and back) leaves the window alone.

import { Button } from "@open-lakehouse/ui-kit";
import { AppWindow, ExternalLink } from "lucide-react";
import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { type EmbedBounds, type ServiceUi, useShellHost } from "./shell-host";
import { message } from "./WorkspaceBar";

/** What pops over the page and must not be painted over: menus, lists and
 *  dialogs. Tooltips and toasts are left out, so hovering or a notification
 *  doesn't blank the page. */
const OVERLAYS =
  '[role="dialog"],[role="alertdialog"],[role="menu"],[role="listbox"],[data-radix-popper-content-wrapper]';

type Area = Pick<DOMRect, "left" | "top" | "width" | "height">;

const overlaps = (a: Area, b: Area) =>
  a.left < b.left + b.width &&
  b.left < a.left + a.width &&
  a.top < b.top + b.height &&
  b.top < a.top + a.height;

/** Where the embed goes for `el`, or `null` to hide it: `el` is hidden or empty, a
 *  modal hides it from assistive tech (Radix marks everything outside a modal
 *  `aria-hidden`), or an overlay overlaps it. */
export function embedBounds(el: HTMLElement, doc: Document = el.ownerDocument): EmbedBounds | null {
  if (doc.hidden) return null;
  if (el.closest('[aria-hidden="true"],[inert]')) return null;
  const area = el.getBoundingClientRect();
  const bounds = {
    x: Math.round(area.left),
    y: Math.round(area.top),
    width: Math.round(area.width),
    height: Math.round(area.height),
  };
  if (bounds.width < 1 || bounds.height < 1) return null;
  for (const overlay of doc.querySelectorAll(OVERLAYS)) {
    if (el.contains(overlay)) continue;
    const r = overlay.getBoundingClientRect();
    if (r.width > 0 && r.height > 0 && overlaps(r, area)) return null;
  }
  return bounds;
}

/** The UIs popped out to their own window, by `<instance>/<module>`. */
const poppedOut = new Set<string>();
const popListeners = new Set<() => void>();
const setPoppedOut = (key: string, out: boolean) => {
  if (out === poppedOut.has(key)) return;
  if (out) poppedOut.add(key);
  else poppedOut.delete(key);
  for (const listener of popListeners) listener();
};
const subscribePopped = (listener: () => void) => {
  popListeners.add(listener);
  return () => {
    popListeners.delete(listener);
  };
};

const sameBounds = (a: EmbedBounds | null, b: EmbedBounds | null) =>
  a === b ||
  (!!a && !!b && a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height);

/** Keep the instance's `module` UI over `ref`'s element while mounted; hidden once
 *  unmounted. Calls go out one at a time, and only on a change. */
function useEmbed(
  ref: React.RefObject<HTMLDivElement | null>,
  instance: string,
  module: string,
  onError: (error: unknown) => void,
) {
  const shell = useShellHost();
  useEffect(() => {
    let sent: EmbedBounds | null = null;
    let queue = Promise.resolve();
    let frame = 0;
    let live = true;
    const send = (bounds: EmbedBounds | null) => {
      sent = bounds;
      queue = queue
        .then(() => shell.run.embedServiceUi(instance, module, bounds))
        .catch((e: unknown) => {
          if (live) onError(e);
        });
    };
    const tick = () => {
      const el = ref.current;
      const bounds = el ? embedBounds(el) : null;
      if (!sameBounds(bounds, sent)) send(bounds);
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      live = false;
      cancelAnimationFrame(frame);
      send(null);
    };
  }, [shell, ref, instance, module, onError]);
}

export function EmbeddedServiceUi({
  instance,
  ui,
}: {
  /** The running instance whose UI it is. */
  instance: string;
  /** The UI, as `run.serviceUis(instance)` lists it. */
  ui: ServiceUi;
}) {
  const shell = useShellHost();
  const key = `${instance}/${ui.moduleId}`;
  const out = useSyncExternalStore(subscribePopped, () => poppedOut.has(key));
  const [error, setError] = useState<string | null>(null);
  const popOut = () => {
    setError(null);
    // The window replaces the embed (the host closes it), after the area unmounts.
    setPoppedOut(key, true);
    void shell.run
      .openServiceWindow(instance, ui.moduleId)
      .catch((e: unknown) => setError(message(e)));
  };
  return (
    <div className="flex h-full min-h-0 flex-col" data-testid={`service-ui-${ui.moduleId}`}>
      <div className="flex h-8 shrink-0 items-center gap-2 border-b px-3 text-xs">
        <span className="font-medium">{ui.name}</span>
        <span className="truncate font-mono text-muted-foreground" title={ui.url}>
          {ui.url}
        </span>
        <Button
          size="sm"
          variant="outline"
          className="ml-auto h-6 shrink-0 gap-1 px-2 text-xs"
          onClick={popOut}
        >
          {out ? <AppWindow className="h-3.5 w-3.5" /> : <ExternalLink className="h-3.5 w-3.5" />}
          {out ? "Focus window" : "Pop out"}
        </Button>
      </div>
      {error ? (
        <p className="p-3 text-xs text-destructive" role="alert">
          {ui.name}: {error}
        </p>
      ) : out ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
          <p>{ui.name} is open in its own window.</p>
          <Button size="sm" variant="outline" onClick={() => setPoppedOut(key, false)}>
            Show here
          </Button>
        </div>
      ) : (
        <EmbedArea instance={instance} module={ui.moduleId} onError={(e) => setError(message(e))} />
      )}
    </div>
  );
}

function EmbedArea({
  instance,
  module,
  onError,
}: {
  instance: string;
  module: string;
  onError: (error: unknown) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // Stable, so the embed isn't torn down and rebuilt on every render.
  const report = useRef(onError);
  report.current = onError;
  const [stableError] = useState(() => (e: unknown) => report.current(e));
  useEmbed(ref, instance, module, stableError);
  return (
    <div ref={ref} className="min-h-0 flex-1" data-testid="service-embed" data-module={module} />
  );
}
