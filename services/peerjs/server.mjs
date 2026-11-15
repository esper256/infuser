// Private upstream only. Rust authorizes every client and signaling destination.
import {PeerServer} from 'peer';
PeerServer({
  host: process.env.PEERJS_HOST || '127.0.0.1',
  port: Number(process.env.PORT || 7780),
  path: '/', key: 'peerjs', allow_discovery: false,
  concurrent_limit: 128, expire_timeout: 5000, alive_timeout: 60000,
});
console.log('Private PeerServer ready');
