// The PWA contract: the page links the manifest and an apple-touch-icon, and the manifest and every icon it names are served
// with the right type, no-cache, and are real PNGs of the size they claim. Run with `node --test web/`.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { AddressInfo } from 'node:net';
import { makeServer } from './server.ts';

const server = makeServer();
await new Promise<void>((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;

test('the page links the manifest, the apple icon and the mobile meta tags', async () => {
  const html = await (await fetch(base + '/')).text();
  for (const needle of ['rel="manifest"', 'rel="apple-touch-icon"', 'name="theme-color"', 'mobile-web-app-capable', 'apple-mobile-web-app-capable', 'apple-mobile-web-app-title', 'apple-mobile-web-app-status-bar-style'])
    assert.ok(html.includes(needle), `index.html lacks ${needle}`);
});

test('the manifest is complete and every icon it names is a real PNG', async () => {
  const res = await fetch(base + '/manifest.webmanifest');
  assert.equal(res.headers.get('content-type'), 'application/manifest+json');
  assert.equal(res.headers.get('cache-control'), 'no-cache');
  assert.match(res.headers.get('content-security-policy') ?? '', /manifest-src 'self'/);
  const m = await res.json();
  for (const k of ['id', 'name', 'short_name', 'description', 'start_url', 'scope', 'display', 'background_color', 'theme_color']) assert.ok(m[k], `manifest lacks ${k}`);
  assert.equal(m.display, 'standalone');
  assert.ok(m.icons.some((i: any) => i.sizes === '192x192'));
  assert.ok(m.icons.some((i: any) => i.sizes === '512x512' && i.purpose === 'any'));
  assert.ok(m.icons.some((i: any) => i.sizes === '512x512' && i.purpose === 'maskable'));
  for (const icon of [...m.icons.map((i: any) => i.src), '/apple-touch-icon.png']) {
    const r = await fetch(base + icon);
    assert.equal(r.status, 200, icon);
    assert.equal(r.headers.get('content-type'), 'image/png', icon);
    assert.equal(r.headers.get('cache-control'), 'no-cache', icon);
    const b = Buffer.from(await r.arrayBuffer());
    assert.deepEqual([...b.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10], icon);
    const want = m.icons.find((i: any) => i.src === icon)?.sizes ?? '180x180';
    assert.equal(`${b.readUInt32BE(16)}x${b.readUInt32BE(20)}`, want, icon);
  }
});

test('unknown paths are 404, not the shell', async () => {
  assert.equal((await fetch(base + '/nope')).status, 404);
  assert.equal((await fetch(base + '/../core/Cargo.toml')).status, 404);
});

test.after(() => server.close());
