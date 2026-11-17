// Serves web/public for development and for the PWA test: the manifest and icons without auth, `no-cache`, with the right types,
// and a CSP that names the manifest. No service worker (a cached shell is the stale-page bug). Standard library only.
// The web build (web/build.sh) adds fm1ntt.js, build-info.json (no-cache) and fm1ntt-<hash>.wasm (named by its hash, so cached for good).
//   node web/server.ts [port]        (default 8080)
import { createServer, type Server } from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const ROOT = fileURLToPath(new URL('./public/', import.meta.url));
const TYPES: Record<string, string> = {
  '/': 'text/html; charset=utf-8',
  '/index.html': 'text/html; charset=utf-8',
  '/manifest.webmanifest': 'application/manifest+json',
  '/icon-192.png': 'image/png',
  '/icon-512.png': 'image/png',
  '/icon-maskable-512.png': 'image/png',
  '/apple-touch-icon.png': 'image/png',
  '/fm1ntt.js': 'text/javascript; charset=utf-8',
  '/build-info.json': 'application/json',
};
const WASM = /^\/fm1ntt-[0-9a-f]{12}\.wasm$/;
// 'wasm-unsafe-eval' lets the page compile WebAssembly (the core, and the small modules its JIT makes); it does not allow eval() of text. worker-src: the core runs in a Worker.
const CSP = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; connect-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; manifest-src 'self'; frame-ancestors 'none'";

export function makeServer(): Server {
  return createServer(async (req, res) => {
    const path = new URL(req.url ?? '/', 'http://x').pathname;
    const isWasm = WASM.test(path);
    const type = isWasm ? 'application/wasm' : Object.hasOwn(TYPES, path) ? TYPES[path] : undefined;
    if (!type) return void res.writeHead(404, { 'Content-Type': 'text/plain' }).end('not found');
    try {
      const body = await readFile(ROOT + (path === '/' ? 'index.html' : path.slice(1)));
      res.writeHead(200, { 'Content-Type': type, 'Cache-Control': isWasm ? 'public, max-age=31536000, immutable' : 'no-cache', 'Content-Security-Policy': CSP, 'X-Content-Type-Options': 'nosniff' }).end(body);
    } catch (e) {
      // a build that has not been made yet is a 404, not a server error
      if ((e as NodeJS.ErrnoException).code === 'ENOENT') return void res.writeHead(404, { 'Content-Type': 'text/plain' }).end('not found');
      res.writeHead(500, { 'Content-Type': 'text/plain' }).end('error');
    }
  });
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const port = Number(process.argv[2] ?? 8080);
  makeServer().listen(port, () => console.log(`FM1-NTT web shell on http://localhost:${port}`));
}
