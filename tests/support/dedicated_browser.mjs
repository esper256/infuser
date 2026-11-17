// Loopback TLS proxy for synthetic dedicated-server browser journeys only.
import fs from 'node:fs';
import http from 'node:http';
import https from 'node:https';
export const {chromium}=await import(process.env.INFUSER_PLAYWRIGHT || '../../services/peerjs/node_modules/playwright/index.mjs');
export const browserOptions={headless:true,executablePath:process.env.INFUSER_CHROMIUM,args:['--ignore-certificate-errors','--no-proxy-server','--host-resolver-rules=MAP *.games.example.com 127.0.0.1, MAP games.example.com 127.0.0.1']};
export async function startProxy(config) {
const headers=req=>({...req.headers,host:req.headers.host.split(':')[0],...(req.headers.origin?{origin:new URL(req.headers.origin).origin.replace(/:\d+$/,'')}: {})});
const proxy=https.createServer({cert:fs.readFileSync(config.cert),key:fs.readFileSync(config.key)},(req,res)=>{
  const upstream=http.request(config.api+req.url,{method:req.method,headers:headers(req)},r=>{
    if((req.url.startsWith('/api/apps/') && req.url.endsWith('/launch') || req.url==='/api/voice' && req.method==='POST') && r.statusCode===200){
      let body='';r.on('data',c=>body+=c);r.on('end',()=>{
        const value=JSON.parse(body);if(value.url){const url=new URL(value.url);url.port=String(proxy.address().port);value.url=url.href;}
        const h={...r.headers};delete h['content-length'];res.writeHead(r.statusCode,h);res.end(JSON.stringify(value));
      });
    }else{res.writeHead(r.statusCode,r.headers);r.pipe(res);}
  });
  upstream.on('error',()=>{res.writeHead(502);res.end();});req.pipe(upstream);
});
proxy.on('upgrade',(req,socket,head)=>{
  socket.on('error',()=>{});
  const upstream=http.request(config.api+req.url,{headers:headers(req)});
  upstream.on('upgrade',(response,backend,backendHead)=>{
    socket.write('HTTP/1.1 101 Switching Protocols\r\n'+Object.entries(response.headers).map(([k,v])=>k+': '+v).join('\r\n')+'\r\n\r\n');
    if(backendHead.length)socket.write(backendHead);if(head.length)backend.write(head);
    backend.on('error',()=>socket.destroy());socket.on('close',()=>backend.destroy());backend.on('close',()=>socket.destroy());backend.pipe(socket);socket.pipe(backend);
  });
  upstream.on('response',r=>{socket.end(`HTTP/1.1 ${r.statusCode} Rejected\r\nConnection: close\r\nContent-Length: 0\r\n\r\n`);r.resume();});
  upstream.on('error',()=>socket.destroy());upstream.end();
});
await new Promise(resolve=>proxy.listen(0,'127.0.0.1',resolve));
return proxy;
}
