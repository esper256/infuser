// Ordinary appearance/layout journeys must not depend on the day CI runs.
// Scope the fixed calendar to themes.js; real app timers and Date.now stay live.
export function ordinaryThemeCalendar(source) {
  const time = new Date(2026, 6, 1, 12).getTime();
  return `(() => { const Date = class extends globalThis.Date { constructor(...args) { super(...(args.length ? args : [${time}])); } };\n${source}\n})();`;
}

// Used by the local TLS fixture proxies, including service-worker requests.
export function pipeWithOrdinaryTheme(path, response, output) {
  if (path.split('?')[0] !== '/assets/themes.js') {
    output.writeHead(response.statusCode, response.headers);response.pipe(output);return;
  }
  const chunks=[];response.on('data',chunk=>chunks.push(chunk));response.on('end',()=>{
    const headers={...response.headers};delete headers['content-length'];
    output.writeHead(response.statusCode,headers);output.end(ordinaryThemeCalendar(Buffer.concat(chunks).toString()));
  });
}
