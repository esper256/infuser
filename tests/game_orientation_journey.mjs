// Exercise the actual game player. Only the OS orientation boundary is simulated.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const {chromium} = await import(process.env.INFUSER_PLAYWRIGHT || 'playwright');
const browser = await chromium.launch({executablePath:process.env.INFUSER_CHROMIUM});
try {
  for (const [mobile, orientation, start, denyTurn] of [
    [true, undefined, 'portrait', false], [true, 'portrait', 'landscape', false],
    [true, 'landscape', 'portrait', true], [true, 'portrait', 'portrait', false],
    [false, 'portrait', 'landscape', false]
  ]) {
    const page = await browser.newPage({hasTouch:mobile, viewport:{width:390,height:844}});
    await page.addInitScript(({start, denyTurn}) => {
      window.locks = [];
      window.attempts = 0;
      window.turn = direction => {
        Object.defineProperty(screen.orientation, 'type', {configurable:true,value:direction+'-primary'});
        screen.orientation.dispatchEvent(new Event('change'));
      };
      window.turn(start);
      const fullscreen = Element.prototype.requestFullscreen;
      Element.prototype.requestFullscreen = function(...args) {
        if (++window.attempts === 1 && denyTurn) return Promise.reject(new TypeError('Tap required'));
        return fullscreen.apply(this,args);
      };
      Object.defineProperties(screen.orientation, {
        lock:{value:async value => {
          window.locks.push(value);
          if (!document.fullscreenElement) throw new DOMException('Fullscreen required','SecurityError');
        }},
        unlock:{value:() => window.locks.push('released')}
      });
    }, {start, denyTurn});
    await page.route('https://game.test/**', async route => {
      const path = new URL(route.request().url()).pathname;
      const files = {'/_infuser/play':'game-player.html','/_infuser/player.js':'game-player.js',
        '/_infuser/player.css':'game-player.css','/_infuser/screenshot-capture.js':'screenshot-capture.js'};
      if (path === '/_infuser/context') return route.fulfill({json:{manifest:{title:'Game',orientation},library_url:'/library'}});
      const file = files[path];
      if (!file) return route.fulfill({contentType:'text/html',body:'<canvas></canvas>'});
      return route.fulfill({contentType:file.endsWith('js')?'text/javascript':file.endsWith('css')?'text/css':'text/html',
        body:await readFile(new URL('../web/'+file,import.meta.url))});
    });
    await page.goto('https://game.test/_infuser/play');
    await page.waitForFunction(() => !document.querySelector('#screenshot').disabled);
    assert((await page.locator('#game').boundingBox()).y > 0, 'windowed game starts below header');
    const expected = orientation || 'landscape';
    const mismatched = start !== expected;
    assert.deepEqual(await page.evaluate(() => locks), mobile && !mismatched ? [expected] : []);
    assert.equal(await page.locator('#rotate-hint').isVisible(), mobile && mismatched);
    if (mobile && mismatched) {
      await page.evaluate(() => window.dispatchEvent(new Event('resize')));
      assert.equal(await page.evaluate(() => attempts), 0);
      await page.evaluate(dir => window.turn(dir), expected);
      if (denyTurn) {
        await page.getByText('Tap Fullscreen to finish.').waitFor();
        await page.locator('#fullscreen').click();
      }
    } else await page.locator('#fullscreen').click();
    await page.waitForFunction(() => document.querySelector('#fullscreen').ariaLabel === 'Exit fullscreen');
    assert.deepEqual(await page.evaluate(() => locks), mobile ? (
      mismatched && !denyTurn ? [expected] : [expected,expected]) : []);
    const gameBounds = () => page.locator('#game').boundingBox();
    const viewport = await page.evaluate(() => ({x:0,y:0,width:innerWidth,height:innerHeight}));
    assert.deepEqual(await gameBounds(), viewport);
    assert(await page.locator('#player-header').evaluate(header => header.inert));
    if (mobile) {
      const touch = await page.context().newCDPSession(page);
      async function swipeDown(startY, endY) {
        const points = y => [{x:120,y}];
        await touch.send('Input.dispatchTouchEvent', {type:'touchStart',touchPoints:points(startY)});
        await touch.send('Input.dispatchTouchEvent', {type:'touchMove',touchPoints:points(endY)});
        await touch.send('Input.dispatchTouchEvent', {type:'touchEnd',touchPoints:[]});
      }
      await swipeDown(80,140);
      assert(await page.locator('#player-header').evaluate(header => header.inert), 'game gestures leave header closed');
      await swipeDown(8,80);
      await page.waitForSelector('#player-header[data-open]');
      if (!orientation) {
        await page.waitForFunction(() => document.querySelector('#player-header').inert);
        await swipeDown(8,80);
      }
    } else {
      await page.mouse.move(120,8);
      await page.waitForSelector('#player-header[data-open]');
      await page.mouse.move(120,180);
      await page.waitForFunction(() => document.querySelector('#player-header').inert);
      await page.locator('#controls-edge').focus();
      await page.keyboard.press('Enter');
      assert(await page.locator('#library').evaluate(link => link === document.activeElement));
    }
    await page.waitForSelector('#player-header[data-open]');
    assert.deepEqual(await gameBounds(), viewport, 'opening controls does not resize game');
    await page.locator('#fullscreen').click();
    await page.waitForFunction(() => document.querySelector('#fullscreen').ariaLabel === 'Enter fullscreen');
    if (mobile) assert.equal(await page.evaluate(() => locks.at(-1)), 'released');
    assert((await gameBounds()).y > 0);
    assert(await page.locator('#controls-edge').isHidden());
    const count = await page.evaluate(() => attempts);
    await page.evaluate(dir => {window.turn(dir==='portrait'?'landscape':'portrait');window.turn(dir);}, expected);
    assert.equal(await page.evaluate(() => attempts), count, 'rotating after exit does not reopen fullscreen');
    await page.close();
  }
  console.log('PASS: legacy landscape, declared portrait, desktop, both rotation directions, fullscreen-on-turn, tap fallback, edge swipes, mouse and keyboard controls, full-size iframe, auto-hide and exit.');
} finally { await browser.close(); }
