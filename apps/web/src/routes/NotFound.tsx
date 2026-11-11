export default function NotFound() {
  return (
    <main className="mx-auto flex min-h-full max-w-md flex-col justify-center gap-3 p-6">
      <h1 className="font-heading text-2xl font-semibold">Not found</h1>
      <p className="text-muted-foreground text-sm">
        Pages live under <code className="font-mono">/a/&lt;auth-host&gt;/&lt;db&gt;/</code>: a Neon
        branch's Auth URL without the scheme, e.g.{" "}
        <code className="font-mono break-all">
          /a/ep-cool-darkness-a1b2c3d4.neonauth.c-7.eu-central-1.aws.neon.tech/neondb/
        </code>
        .
      </p>
    </main>
  );
}
