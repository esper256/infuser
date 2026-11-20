// Chrome clicks Continue itself. A local HTTPS proxy only supplies a Secure
// origin for the __Host- cookie and forwards the browser's Origin and
// Sec-Fetch-Site unchanged. Nothing in this file sets those headers.
import http from 'node:http';
import https from 'node:https';
import {mkdtempSync, readFileSync, rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {execFileSync} from 'node:child_process';
const playwrightPath = process.env.INFUSER_PLAYWRIGHT || 'playwright-core';
const {chromium} = await import(playwrightPath);
const [base, path] = process.argv.slice(2);
if (!base || !path) {
  console.error('usage: member_link_browser.mjs BASE PATH');
  process.exit(2);
}
const dir = mkdtempSync(join(tmpdir(), 'infuser-member-link-'));
const key = join(dir, 'key.pem');
const cert = join(dir, 'cert.pem');
execFileSync('openssl', ['req', '-x509', '-newkey', 'rsa:2048', '-keyout', key, '-out', cert, '-days', '1', '-nodes', '-subj', '/CN=127.0.0.1'], {stdio: 'ignore'});
const proxy = https.createServer({key: readFileSync(key), cert: readFileSync(cert)}, (req, res) => {
  const headers = {...req.headers, host: 'games.example.com'};
  if (req.method === 'POST') {
    const logged = req.url.startsWith('/member-link/') ? '/member-link/(token)' : req.url.split('?')[0];
    console.log(`browser POST ${logged} origin=${headers.origin || '(missing)'} site=${headers['sec-fetch-site'] || '(missing)'} referer=${headers.referer || '(missing)'}`);
  }
  const call = http.request(base + req.url, {method: req.method, headers}, result => {
    res.writeHead(result.statusCode, result.headers);
    result.pipe(res);
  });
  call.on('error', () => {
    res.writeHead(502);
    res.end();
  });
  req.pipe(call);
});
await new Promise(resolve => proxy.listen(0, '127.0.0.1', resolve));
const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.INFUSER_CHROMIUM || '/usr/bin/google-chrome',
  args: ['--no-sandbox', '--disable-dev-shm-usage'],
});
try {
  const context = await browser.newContext({ignoreHTTPSErrors: true});
  const page = await context.newPage();
  page.setDefaultTimeout(15000);
  await page.addLocatorHandler(page.locator('#profile-dialog[open]'), async () => {
    await page.locator('#close-profile').click();
  });
  await page.goto(`https://127.0.0.1:${proxy.address().port}${path}`);
  await page.getByRole('button', {name: 'Continue'}).click();
  await page.locator('#greeting', {hasText: 'Signed in as Gob.'}).waitFor({state: 'attached'});
  console.log('browser Continue signed in as Gob');
} finally {
  await browser.close();
  proxy.close();
  rmSync(dir, {recursive: true, force: true});
}
