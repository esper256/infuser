// Operator integration test: real OAuth2 Proxy, disposable OIDC provider, no Google credentials.
import assert from 'node:assert/strict';
import {generateKeyPairSync, randomBytes, sign} from 'node:crypto';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {mkdtemp, readFile, rm, writeFile} from 'node:fs/promises';
import http from 'node:http';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {setTimeout as delay} from 'node:timers/promises';

assert(process.env.INFUSER_OAUTH2_PROXY, 'Set INFUSER_OAUTH2_PROXY to the deployed proxy version’s binary');
const {chromium} = await import(process.env.INFUSER_PLAYWRIGHT || '../services/peerjs/node_modules/playwright/index.mjs');
const {privateKey, publicKey} = generateKeyPairSync('rsa', {modulusLength: 2048});
const jwk = {...publicKey.export({format: 'jwk'}), kid: 'fixture', alg: 'RS256', use: 'sig'};
const codes = new Map();
const encode = value => Buffer.from(JSON.stringify(value)).toString('base64url');
let issuer, proxy, browser, proxyLog = '', refreshes = 0;
const provider = http.createServer(async (req, res) => {
  const url = new URL(req.url, issuer);
  const json = body => {res.setHeader('Content-Type', 'application/json'); res.end(JSON.stringify(body));};
  if (url.pathname === '/.well-known/openid-configuration') return json({
    issuer, authorization_endpoint: issuer + '/authorize', token_endpoint: issuer + '/token',
    jwks_uri: issuer + '/keys', response_types_supported: ['code'], subject_types_supported: ['public'],
    id_token_signing_alg_values_supported: ['RS256'], token_endpoint_auth_methods_supported: ['client_secret_basic'],
  });
  if (url.pathname === '/keys') return json({keys: [jwk]});
  if (url.pathname === '/authorize') {
    const code = randomBytes(16).toString('hex');
    codes.set(code, url.searchParams.get('nonce'));
    const target = new URL(url.searchParams.get('redirect_uri'));
    target.searchParams.set('code', code); target.searchParams.set('state', url.searchParams.get('state'));
    res.writeHead(302, {Location: target.href}); return res.end();
  }
  if (url.pathname === '/token') {
    const chunks = []; for await (const chunk of req) chunks.push(chunk);
    const form = new URLSearchParams(Buffer.concat(chunks).toString());
    if (form.get('grant_type') === 'refresh_token') {refreshes++; res.statusCode = 400; return json({error: 'invalid_grant'});}
    const code = form.get('code');
    if (!codes.has(code)) {res.statusCode = 400; return json({error: 'invalid_grant'});}
    const now = Math.floor(Date.now() / 1000);
    const payload = encode({iss: issuer, aud: 'fixture', sub: 'fixture-user', email: 'player@example.com',
      email_verified: true, nonce: codes.get(code), iat: now, exp: now + 2});
    codes.delete(code);
    const unsigned = encode({alg: 'RS256', kid: 'fixture'}) + '.' + payload;
    return json({access_token: 'fixture-access', token_type: 'Bearer', expires_in: 2,
      id_token: unsigned + '.' + sign('RSA-SHA256', Buffer.from(unsigned), privateKey).toString('base64url')});
  }
  res.statusCode = 404; res.end();
});
const temporary = await mkdtemp(path.join(tmpdir(), 'infuser-login-'));
async function stopProxy() {
  if (!proxy?.pid || proxy.exitCode !== null) return;
  const exited = once(proxy, 'exit'); proxy.kill('SIGTERM'); await exited;
}
try {
  provider.listen(0, '127.0.0.1'); await once(provider, 'listening');
  issuer = `http://127.0.0.1:${provider.address().port}`;
  const reserve = http.createServer(); reserve.listen(0, '127.0.0.1'); await once(reserve, 'listening');
  const port = reserve.address().port; await new Promise(resolve => reserve.close(resolve));
  const base = `http://127.0.0.1:${port}`;
  const secret = randomBytes(32).toString('base64url');
  const source = await readFile(new URL('../deploy/oauth2-proxy.example.cfg', import.meta.url), 'utf8');
  assert.match(source, /^cookie_expire = "168h"$/m);
  assert.match(source, /^cookie_refresh = "0s"$/m);
  assert.match(source, /^session_cookie_minimal = true$/m);
  async function startProxy(legacy = false) {
    // Exercise the shipped settings, replacing only endpoints, test credentials,
    // localhost cookie constraints and lifetime to keep expiry checks fast.
    const replacements = {
      provider: 'oidc', http_address: `127.0.0.1:${port}`, redirect_url: base + '/oauth2/callback',
      cookie_name: 'infuser_fixture', cookie_secure: false, whitelist_domains: [`127.0.0.1:${port}`],
      cookie_expire: '12s', ...(legacy ? {cookie_refresh: '1s', session_cookie_minimal: false} : {}),
    };
    let config = source;
    for (const [key, value] of Object.entries(replacements)) config = config.replace(new RegExp(`^${key} = .*$`, 'm'), `${key} = ${JSON.stringify(value)}`);
    config += `\noidc_issuer_url = ${JSON.stringify(issuer)}\nclient_id = "fixture"\nclient_secret = "fixture"\ncookie_secret = ${JSON.stringify(secret)}\nskip_claims_from_profile_url = true\n`;
    const file = path.join(temporary, 'proxy.cfg'); await writeFile(file, config, {mode: 0o600});
    proxy = spawn(process.env.INFUSER_OAUTH2_PROXY, ['--config=' + file], {stdio: ['ignore', 'pipe', 'pipe']});
    proxy.on('error', error => {proxyLog += String(error);});
    for (const output of [proxy.stdout, proxy.stderr]) output.on('data', data => {proxyLog += data;});
    for (let attempt = 0; attempt < 100; attempt++) {
      if (await fetch(base + '/ping').then(r => r.ok).catch(() => false)) return;
      if (proxy.exitCode !== null) throw new Error('Proxy exited: ' + proxyLog);
      await delay(50);
    }
    throw new Error('Proxy did not start: ' + proxyLog);
  }
  browser = await chromium.launch({headless: true,
    ...(process.env.INFUSER_CHROMIUM ? {executablePath: process.env.INFUSER_CHROMIUM} : {})});
  const auth = context => context.request.get(base + '/oauth2/auth');
  async function login(context) {
    const page = await context.newPage();
    const response = await page.goto(base + '/oauth2/start?rd=' + encodeURIComponent(base + '/'));
    assert.equal(response.status(), 202); assert.equal((await auth(context)).status(), 202);
    return page;
  }
  await startProxy(true);
  const original = await browser.newContext();
  await login(original);
  const oldCookies = (await original.cookies()).filter(c => c.name === 'infuser_fixture');
  assert.equal(oldCookies.length, 1);
  await delay(3100);
  assert.equal((await auth(original)).status(), 401, 'Reproduce token expiry without a refresh token');
  await stopProxy();
  await startProxy();
  const migrated = await browser.newContext(); await migrated.addCookies(oldCookies);
  assert.equal((await auth(migrated)).status(), 202, 'Previously issued cookies survive the config change');
  const current = await browser.newContext();
  const page = await login(current);
  const cookies = (await current.cookies()).filter(c => c.name === 'infuser_fixture');
  assert(cookies[0].value.length < oldCookies[0].value.length, 'New cookies omit provider tokens');
  await delay(3100);
  assert.equal((await page.reload()).status(), 202, 'Reload survives provider token expiry');
  const second = await current.newPage();
  assert.equal((await second.goto(base + '/')).status(), 202, 'A new tab keeps the shared login');
  const responses = await Promise.all(Array.from({length: 8}, () => auth(current)));
  for (const response of responses) {
    assert.equal(response.status(), 202);
    assert.equal(response.headers()['x-auth-request-user'], 'fixture-user');
    assert.equal(response.headers()['x-auth-request-email'], 'player@example.com');
    assert.equal(response.headers()['x-auth-request-access-token'], undefined);
    assert.equal(response.headers()['set-cookie'], undefined, 'No concurrent cookie rotation');
  }
  assert.equal(refreshes, 0);
  const rawCookie = `${cookies[0].name}=${cookies[0].value}`;
  assert.equal((await fetch(base + '/oauth2/auth')).status, 401);
  assert.equal((await fetch(base + '/oauth2/auth', {headers: {cookie: rawCookie + 'tampered'}})).status, 401);
  // Replay directly to test server-side expiry, not merely browser cookie removal.
  await delay(Math.max(0, cookies[0].expires * 1000 - Date.now()) + 1500);
  assert.equal((await fetch(base + '/oauth2/auth', {headers: {cookie: rawCookie}})).status, 401);
  console.log('Passed: reproduced old expiry; existing cookies, reload, new tabs and concurrent requests work; forged/expired sessions fail.');
} catch (error) {
  console.error(proxyLog); throw error;
} finally {
  await browser?.close(); await stopProxy();
  await new Promise(resolve => provider.close(resolve));
  await rm(temporary, {recursive: true, force: true});
}
