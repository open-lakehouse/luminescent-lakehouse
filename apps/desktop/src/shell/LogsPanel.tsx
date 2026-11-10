// An instance's logs in the bottom panel's Logs tab: `docker compose logs --follow` streamed over a
// channel into a ring buffer of the newest lines, filtered by service. Follow keeps
// the newest line in view; scrolling up turns it off.

import { Button, cn } from "@open-lakehouse/ui-kit";
import { useEffect, useMemo, useRef, useState } from "react";
import { LOG_CAPACITY, LogBuffer } from "./log-buffer";
import { type InstanceStatus, useShellHost } from "./shell-host";

/** Lines per service fetched when the viewer opens. */
export const LOG_TAIL = 500;

export function LogsPanel({ instance }: { instance: InstanceStatus }) {
  const shell = useShellHost();
  const buffer = useRef(new LogBuffer(LOG_CAPACITY));
  // Bumped per batch: the buffer itself is mutable.
  const [version, setVersion] = useState(0);
  const [ended, setEnded] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const [follow, setFollow] = useState(true);
  const [only, setOnly] = useState<ReadonlySet<string>>(new Set());
  const scroller = useRef<HTMLDivElement>(null);

  useEffect(() => {
    buffer.current = new LogBuffer(LOG_CAPACITY);
    setVersion((v) => v + 1);
    setEnded(false);
    setProblem(null);
    return shell.run.logs(instance.id, { tail: LOG_TAIL, follow: true }, (message) => {
      switch (message.kind) {
        case "lines":
          buffer.current.push(message.lines);
          setVersion((v) => v + 1);
          break;
        case "end":
          setEnded(true);
          break;
        case "error":
          setProblem(message.message);
          break;
      }
    });
  }, [shell, instance.id]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` marks a new batch in the buffer.
  const lines = useMemo(() => buffer.current.lines(only), [only, version]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: scroll after each batch.
  useEffect(() => {
    const el = scroller.current;
    if (follow && el) el.scrollTop = el.scrollHeight;
  }, [follow, lines]);

  const services = [...new Set(instance.services.map((s) => s.service))].sort();
  const toggle = (service: string) => {
    const next = new Set(only);
    if (next.has(service)) next.delete(service);
    else next.add(service);
    setOnly(next);
  };

  return (
    <div className="flex h-full min-h-0 flex-col gap-2 py-1" data-testid="logs">
      <div className="flex flex-wrap items-center gap-1">
        <Button
          size="sm"
          variant={only.size === 0 ? "secondary" : "ghost"}
          className="h-6 px-2 text-xs"
          onClick={() => setOnly(new Set())}
          aria-pressed={only.size === 0}
        >
          all
        </Button>
        {services.map((service) => (
          <Button
            key={service}
            size="sm"
            variant={only.has(service) ? "secondary" : "ghost"}
            className="h-6 px-2 font-mono text-xs"
            onClick={() => toggle(service)}
            aria-pressed={only.has(service)}
            data-testid={`logs-filter-${service}`}
          >
            {service}
          </Button>
        ))}
        <div className="flex-1" />
        <Button
          size="sm"
          variant={follow ? "secondary" : "ghost"}
          className="h-6 px-2 text-xs"
          onClick={() => setFollow(!follow)}
          aria-pressed={follow}
          data-testid="logs-follow"
        >
          Follow
        </Button>
      </div>
      <div
        ref={scroller}
        className="min-h-0 flex-1 overflow-auto rounded-md border bg-muted/30 p-2 font-mono text-[11px] leading-4"
        onScroll={(e) => {
          const el = e.currentTarget;
          const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 8;
          if (follow && !atBottom) setFollow(false);
        }}
        role="log"
        aria-label={`Logs of ${instance.project}`}
        data-testid="logs-lines"
      >
        {buffer.current.dropped > 0 && (
          <div className="text-muted-foreground">
            … {buffer.current.dropped} earlier lines dropped (the newest {LOG_CAPACITY} are kept)
          </div>
        )}
        {lines.map((line, i) => (
          // Lines have no identity of their own; their position in the buffer is stable
          // until it wraps, which re-renders everything anyway.
          // biome-ignore lint/suspicious/noArrayIndexKey: see above.
          <div key={i} className="whitespace-pre-wrap break-all">
            <span className={cn("text-muted-foreground", only.size === 1 && "hidden")}>
              {line.service} |{" "}
            </span>
            {line.text}
          </div>
        ))}
        {lines.length === 0 && !problem && (
          <div className="text-muted-foreground">{ended ? "No logs." : "Waiting for logs…"}</div>
        )}
        {problem && <div className="text-destructive">{problem}</div>}
        {ended && lines.length > 0 && (
          <div className="text-muted-foreground">— the containers stopped logging —</div>
        )}
      </div>
    </div>
  );
}
