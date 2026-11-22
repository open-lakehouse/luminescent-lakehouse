// One isolate of the function: the built handler (`dist/index.mjs`) in a
// process of its own, served over HTTP on a loopback port, as Neon serves an
// isolate. Several of them share nothing but the database. Prints one line,
// `isolate {"port":…,"root":…}` (`root`: the demo plan it seeded, with SEED=1), then
// serves until killed.

import { createServer } from "node:http";
import { Readable } from "node:stream";

const mod = await import(new URL("../dist/index.mjs", import.meta.url));
const fn = mod.default;

const server = createServer(async (req, res) => {
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const body = req.method === "GET" || req.method === "HEAD" ? undefined : Buffer.concat(chunks);
  const headers = new Headers();
  for (const [name, value] of Object.entries(req.headers)) {
    if (typeof value === "string") headers.set(name, value);
  }
  const response = await fn.fetch(new Request(`http://${req.headers.host}${req.url}`, { method: req.method, headers, body }));
  res.writeHead(response.status, Object.fromEntries(response.headers));
  if (!response.body) return res.end();
  const stream = Readable.fromWeb(response.body);
  res.on("close", () => stream.destroy());
  stream.pipe(res);
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
const root = process.env.SEED === "1" ? await mod.seedDemo() : undefined;
process.stdout.write(`isolate ${JSON.stringify({ port: server.address().port, root })}\n`);
