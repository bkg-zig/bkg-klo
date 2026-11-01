// Null-Dependency statischer Server (Node-Pfad fuer Railway).
// Rust-Gateway bleibt Primaerpfad; diese Datei spiegelt /api/health|agents|floor.
const http = require('http'), fs = require('fs'), path = require('path');
const root = path.join(__dirname, 'web-dist');
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.png': 'image/png' };
function office() {
  try { return JSON.parse(fs.readFileSync(path.join(__dirname, '.bkgclaw', 'office.json'), 'utf8')); }
  catch { return { width: 20, height: 14, desks: [], employees: [], tasks: [] }; }
}
http.createServer((req, res) => {
  if (req.url === '/api/health') return res.end(JSON.stringify({ ok: true, service: 'bkgclaw-web' }));
  if (req.url === '/api/agents') return res.end(JSON.stringify({ ok: true, data: office().employees }));
  if (req.url === '/api/floor' || req.url === '/api/office') return res.end(JSON.stringify({ ok: true, data: office() }));
  let file = path.join(root, req.url === '/' ? 'native.html' : req.url.split('?')[0]);
  if (!file.startsWith(root) || !fs.existsSync(file)) { res.writeHead(404); return res.end('not found'); }
  res.setHeader('content-type', types[path.extname(file)] || 'application/octet-stream');
  fs.createReadStream(file).pipe(res);
}).listen(process.env.PORT || 8787, () => console.log('bkgclaw-web on ' + (process.env.PORT || 8787)));
