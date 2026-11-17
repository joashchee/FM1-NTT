// Static server for the spike page: node spikes/wasm/serve.mjs <wasm> <dir with firmware> [port]. Serves the page, worker, wasm and the firmware folder at /fw/.
import { createServer } from 'node:http'; import { readFileSync, existsSync } from 'node:fs'; import { fileURLToPath } from 'node:url'; import { basename } from 'node:path';
const [wasm, fwdir, port = '8099'] = process.argv.slice(2); const here = fileURLToPath(new URL('.', import.meta.url));
const T = { html: 'text/html', js: 'text/javascript', wasm: 'application/wasm' };
const srv = createServer((q, r) => { const p = new URL(q.url, 'http://x').pathname; let f = null;
  if (p === '/' || p === '/index.html') f = here + 'index.html'; else if (['/worker.js','/live.html','/live-worker.js','/live-worklet.js'].includes(p)) f = here + p.slice(1);
  else if (p === '/fm1ntt_wasm_spike.wasm') f = wasm; else if (p.startsWith('/fw/')) f = fwdir + '/' + basename(p);
  if (!f || !existsSync(f)) return r.writeHead(404).end('no'); r.writeHead(200, { 'content-type': T[f.split('.').pop()] || 'application/octet-stream', 'cache-control': 'no-cache' }).end(readFileSync(f)); }).listen(+port, () => console.log('http://localhost:' + port + '  (pages: / and /live.html)'));
srv.on('error', (e) => { console.error(e.code === 'EADDRINUSE' ? 'Port ' + port + ' is busy (an older serve.mjs still running?). Stop it with: lsof -ti:' + port + ' | xargs kill   or pass another port as the 3rd argument.' : e.message); process.exit(1); });
