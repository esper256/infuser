// Mirrors Godot 4.7.2's web preloader: count decompressed body bytes against the
// uncompressed size recorded at export, then instantiate the wasm module.
// Node's fetch cannot set the game Host header, so this uses raw HTTP and the
// same decompress-then-count steps the browser performs.
import http from "node:http";
import { createBrotliDecompress, createGunzip } from "node:zlib";

const [url, host, token, path, expectedText] = process.argv.slice(2);
const expected = Number(expectedText);
const target = new URL(path, url);

const response = await new Promise((resolve, reject) => {
  const request = http.request(
    target,
    {
      headers: {
        host,
        authorization: `Bearer ${token}`,
        origin: `https://${host}`,
        "accept-encoding": "br, gzip",
      },
    },
    resolve,
  );
  request.on("error", reject);
  request.end();
});

if (response.statusCode !== 200) {
  console.error(`status ${response.statusCode}`);
  process.exit(1);
}

const encoding = response.headers["content-encoding"] ?? "";
const compressed = Number(response.headers["content-length"] ?? "0");
const decoder =
  encoding === "br"
    ? createBrotliDecompress()
    : encoding === "gzip"
      ? createGunzip()
      : null;
const source = decoder ? response.pipe(decoder) : response;
const chunks = [];
let loaded = 0;
let updates = 0;
for await (const chunk of source) {
  loaded += chunk.length;
  updates += 1;
  chunks.push(chunk);
}
if (loaded !== expected || updates < 1) {
  console.error(`loaded ${loaded} updates ${updates} expected ${expected}`);
  process.exit(1);
}
const bytes = Buffer.concat(chunks, loaded);
const module = await WebAssembly.instantiate(bytes);
if (module.instance.exports.ping() !== 7) {
  console.error("wasm export did not run");
  process.exit(1);
}
console.log(JSON.stringify({ encoding, compressed, loaded, updates }));
