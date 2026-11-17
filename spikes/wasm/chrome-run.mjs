// Drives a headless Chrome over the DevTools protocol (no dependencies) to run the spike page and print its result.
//   node spikes/wasm/chrome-run.mjs <url> [timeout s]
import { spawn } from 'node:child_process';
const [url, to = '120'] = process.argv.slice(2), port = 9333;
const chrome = spawn('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', ['--headless=new', '--no-first-run', '--disable-gpu', '--user-data-dir=' + process.env.TMPDIR + '/chrome-spike', '--remote-debugging-port=' + port, 'about:blank'], { stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let tabs; for (let i = 0; i < 50; i++) { try { tabs = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); if (tabs[0]) break; } catch {} await sleep(200); }
if (!tabs) { console.log('Chrome did not start'); chrome.kill(); process.exit(1); }
const ws = new WebSocket(tabs.find((t) => t.type === 'page').webSocketDebuggerUrl); await new Promise((r) => (ws.onopen = r));
let id = 0; const send = (method, params = {}) => new Promise((res) => { const me = ++id; const h = (m) => { const d = JSON.parse(m.data); if (d.id === me) { ws.removeEventListener('message', h); res(d.result); } }; ws.addEventListener('message', h); ws.send(JSON.stringify({ id: me, method, params })); });
await send('Page.navigate', { url });
for (let i = 0; i < +to; i++) { await sleep(1000); const r = await send('Runtime.evaluate', { expression: "document.getElementById('speed').textContent", returnByValue: true });
  const t = r?.result?.value || ''; if (t.includes('xRealtime') || t.startsWith('error')) { console.log(t); break; } if (i === +to - 1) console.log('timed out; last:', t); }
ws.close(); chrome.kill();
