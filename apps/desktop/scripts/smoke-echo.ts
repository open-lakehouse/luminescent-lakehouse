// The local service `--smoke` streams through the Rust proxy: `GET /stream?chunks=N`
// answers `chunk 0\n` … `chunk N-1\n`, one chunk every 30 ms, so the body must
// arrive in pieces. Prints its port, then serves until killed.
//
//   bun scripts/smoke-echo.ts [port]   (default: any free port)

const server = Bun.serve({
  port: Number(process.argv[2] ?? 0),
  hostname: "127.0.0.1",
  fetch(request) {
    const url = new URL(request.url);
    if (url.pathname !== "/stream") return new Response("not found", { status: 404 });
    const chunks = Number(url.searchParams.get("chunks") ?? 5);
    let i = 0;
    const encoder = new TextEncoder();
    const body = new ReadableStream<Uint8Array>({
      async pull(controller) {
        if (i >= chunks) return controller.close();
        await Bun.sleep(30);
        controller.enqueue(encoder.encode(`chunk ${i++}\n`));
      },
    });
    return new Response(body, { headers: { "content-type": "text/plain" } });
  },
});
console.log(server.port);
