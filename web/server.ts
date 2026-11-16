// Serves web/public for development and for the PWA test: the manifest and icons without auth, `no-cache`, with the right types,
// and a CSP that names the manifest. No service worker (a cached shell is the stale-page bug). Standard library only.
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
};
const CSP = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; manifest-src 'self'; frame-ancestors 'none'";

export function makeServer(): Server {
  return createServer(async (req, res) => {
    const path = new URL(req.url ?? '/', 'http://x').pathname;
    const type = Object.hasOwn(TYPES, path) ? TYPES[path] : undefined;
    if (!type) return void res.writeHead(404, { 'Content-Type': 'text/plain' }).end('not found');
    try {
      const body = await readFile(ROOT + (path === '/' ? 'index.html' : path.slice(1)));
      res.writeHead(200, { 'Content-Type': type, 'Cache-Control': 'no-cache', 'Content-Security-Policy': CSP, 'X-Content-Type-Options': 'nosniff' }).end(body);
    } catch {
      res.writeHead(500, { 'Content-Type': 'text/plain' }).end('error');
    }
  });
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const port = Number(process.argv[2] ?? 8080);
  makeServer().listen(port, () => console.log(`FM1-NTT web shell on http://localhost:${port}`));
}
