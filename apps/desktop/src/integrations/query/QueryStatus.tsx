// The Query app's status line: kept apart from QueryView so it renders without
// Monaco (in tests).

/** `3 rows · 41 ms`, `running…`, or nothing before the first run. */
export function QueryStatus({
  running,
  rows,
  elapsedMs,
  failed,
}: {
  running: boolean;
  rows: number;
  elapsedMs: number | null;
  failed: boolean;
}) {
  let text: string | null = null;
  if (running) text = rows > 0 ? `${rows.toLocaleString()} rows…` : "running…";
  else if (failed) text = elapsedMs === null ? null : `failed after ${elapsedMs} ms`;
  else if (elapsedMs !== null)
    text = `${rows.toLocaleString()} ${rows === 1 ? "row" : "rows"} · ${elapsedMs} ms`;
  if (!text) return null;
  return (
    <span className="text-xs text-muted-foreground" data-testid="query-status">
      {text}
    </span>
  );
}
