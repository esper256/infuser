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
        '/_infuser/activity-input.js':'activity-input.js','/_infuser/player.css':'game-player.css','/_infuser/screenshot-capture.js':'screenshot-capture.js'};
      if (path === '/_infuser/context') return route.fulfill({json:{manifest:{title:'Game',orientation},library_url:'/library'}});
      const file = files[path];
      if (!file) return route.fulfill({contentType:'text/html',body:'<canvas></canvas>'});
      return route.fulfill({contentType:file.endsWith('js')?'text/javascript':file.endsWith('css')?'text/css':'text/html',
        body:await readFile(new URL('../web/'+file,import.meta.url))});
    });
    await page.goto('https://game.test/_infuser/play');
    await page.waitForFunction(() => !document.querySelector('#screenshot').disabled);
    assert((await page.locator('#game').boundingBox()).y > 0, 'windowed game starts below header');
    if (!mobile) {
      const canvas = page.frameLocator('#game').locator('canvas');
      const reported = page.waitForRequest(r => new URL(r.url()).pathname === '/_infuser/input' && r.method() === 'POST');
      await canvas.click();
      assert.equal((await reported).postData(), null, 'input in the game reports activity without input contents');
    }
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
    if (!mobile) {
      const resolution = page.locator('#resolution');
      assert.equal(await resolution.isHidden(), true, 'resolution control is hidden when the display is already within 1080p');
      await resolution.evaluate(button => button.click());
      assert.equal(await page.evaluate(() => window.infuserHdCap()), true, 'a sub-1080p click does not leave the 1080p cap');
      await page.setViewportSize({width:3840,height:2160});
      await page.waitForFunction(() => {
        const button = document.getElementById('resolution');
        const win = document.getElementById('game').contentWindow;
        return !button.hidden && button.textContent === 'HD' && win.innerWidth * win.innerHeight <= 1920 * 1080 + 1920;
      });
      const scaled = await page.evaluate(() => {
        const frame = document.getElementById('game');
        const box = frame.getBoundingClientRect();
        const vp = document.getElementById('viewport').getBoundingClientRect();
        const win = frame.contentWindow;
        const button = document.getElementById('resolution');
        return {
          text: button.textContent,
          label: button.getAttribute('aria-label'),
          title: button.title,
          pressed: button.getAttribute('aria-pressed'),
          hidden: button.hidden,
          visualMatchesViewport: Math.abs(box.width - vp.width) < 1 && Math.abs(box.height - vp.height) < 1,
          innerPixels: win.innerWidth * win.innerHeight,
          dpr: win.devicePixelRatio,
        };
      });
      assert.equal(scaled.hidden, false);
      assert.equal(scaled.text, 'HD');
      assert.equal(scaled.label, 'Rendering HD (1080p) — switch to MAX');
      assert.equal(scaled.title, scaled.label);
      assert.equal(scaled.pressed, null);
      assert(scaled.visualMatchesViewport, '4K player visually fills the 4K viewport');
      assert(scaled.innerPixels <= 1920 * 1080 + 1920, '4K player caps inner layout viewport to 1080p budget');
      assert(scaled.dpr <= 1, 'capped 4K player does not exceed a device pixel ratio of 1');
      await resolution.click();
      const native = await page.evaluate(() => {
        const win = document.getElementById('game').contentWindow;
        const button = document.getElementById('resolution');
        return {
          text: button.textContent,
          label: button.getAttribute('aria-label'),
          title: button.title,
          pressed: button.getAttribute('aria-pressed'),
          mode: button.dataset.mode,
          innerWidth: win.innerWidth,
        };
      });
      assert.equal(native.text, 'MAX');
      assert.equal(native.label, 'Rendering MAX (native) — switch to HD');
      assert.equal(native.title, native.label);
      assert.equal(native.pressed, null);
      assert.equal(native.mode, 'max');
      assert.equal(native.innerWidth, 3840);
      await resolution.click();
      assert.equal(await resolution.textContent(), 'HD');
      assert.equal(await resolution.getAttribute('aria-label'), 'Rendering HD (1080p) — switch to MAX');
    }
    const count = await page.evaluate(() => attempts);
    await page.evaluate(dir => {window.turn(dir==='portrait'?'landscape':'portrait');window.turn(dir);}, expected);
    assert.equal(await page.evaluate(() => attempts), count, 'rotating after exit does not reopen fullscreen');
    await page.close();
  }
  console.log('PASS: legacy landscape, declared portrait, desktop, both rotation directions, fullscreen-on-turn, tap fallback, edge swipes, mouse and keyboard controls, full-size iframe, auto-hide, exit, and HD/MAX resolution label.');
} finally { await browser.close(); }
