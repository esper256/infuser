// Local-only boundaries for the real Prometheus/Grafana notification journey.
import http from 'node:http';
const port=Number(process.argv[2]);
let failed=false,stale=false;
const events=[];
http.createServer(async(req,res)=>{
  if(req.url==='/metrics'){
    const now=Math.floor(Date.now()/1000);
    res.setHeader('Content-Type','text/plain');
    res.end(`infuser_collector_timestamp_seconds ${now-(stale?600:0)}
infuser_check_success{check="controller"} ${failed?0:1}
infuser_maintenance 0
node_memory_MemAvailable_bytes 9000000000
node_memory_MemTotal_bytes 12000000000
node_filesystem_avail_bytes{fstype="ext4",mountpoint="/"} 30000000000
node_filesystem_size_bytes{fstype="ext4",mountpoint="/"} 45000000000
node_filesystem_readonly{fstype="ext4",mountpoint="/"} 0
probe_success 1
probe_ssl_earliest_cert_expiry ${now+30*86400}
`);return;
  }
  if(req.method==='GET'&&req.url==='/events'){res.setHeader('Content-Type','application/json');res.end(JSON.stringify(events));return;}
  let body='';for await(const chunk of req){body+=chunk;if(body.length>65536){res.writeHead(413);res.end();return;}}
  if(req.url==='/state'){const state=JSON.parse(body);failed=state.failed??failed;stale=state.stale??stale;}
  else if(req.url==='/reset')events.length=0;
  else if(['/alerts','/heartbeat'].includes(req.url)){
    if(req.url==='/alerts'&&req.headers.authorization!=='Bearer fixture$token'){res.writeHead(401);res.end();return;}
    events.push({path:req.url,at:Date.now(),body:JSON.parse(body)});if(events.length>120)events.shift();
  }else{res.writeHead(404);res.end();return;}
  res.writeHead(204);res.end();
}).listen(port,'127.0.0.1');
