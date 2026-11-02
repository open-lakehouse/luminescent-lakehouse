export default function NotFound() {
  return (
    <main className="mx-auto flex min-h-full max-w-md flex-col justify-center gap-3 p-6">
      <h1 className="font-heading text-2xl font-semibold">Not found</h1>
      <p className="text-muted-foreground text-sm">
        Pages live under <code className="font-mono">/b/&lt;endpoint-id&gt;/</code>, where the
        endpoint id names a Neon branch's compute, e.g.{" "}
        <code className="font-mono">ep-cool-darkness-a1b2c3d4</code>.
      </p>
    </main>
  );
}
