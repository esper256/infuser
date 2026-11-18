// Exercise the actual game player. Only the OS orientation boundary is simulated.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
const {chromium} = await import(process.env.INFUSER_PLAYWRIGHT || 'playwright');
const browser = await chromium.launch({executablePath:process.env.INFUSER_CHROMIUM});
try {
  for (const [mobile, orientation] of [[true, undefined], [true, 'portrait'], [false, 'portrait']]) {
    const page = await browser.newPage({hasTouch:mobile, viewport:{width:390,height:844}});
    await page.addInitScript(() => {
      window.locks = [];
      Object.defineProperties(screen.orientation, {
        lock:{value:async value => {
          window.locks.push(value);
          if (!document.fullscreenElement) throw new DOMException('Fullscreen required','SecurityError');
        }},
        unlock:{value:() => window.locks.push('released')}
      });
    });
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
    const expected = orientation || 'landscape';
    assert.deepEqual(await page.evaluate(() => locks), mobile ? [expected] : []);
    assert.equal(await page.locator('#rotate-hint').isVisible(), mobile && expected === 'landscape');
    await page.locator('#fullscreen').click();
    await page.waitForFunction(() => document.querySelector('#fullscreen').ariaLabel === 'Exit fullscreen');
    assert.deepEqual(await page.evaluate(() => locks), mobile ? [expected,expected] : []);
    await page.locator('#fullscreen').click();
    await page.waitForFunction(() => document.querySelector('#fullscreen').ariaLabel === 'Enter fullscreen');
    assert.deepEqual(await page.evaluate(() => locks), mobile ? [expected,expected,'released'] : []);
    await page.close();
  }
  console.log('PASS: legacy landscape, declared portrait, desktop, rejected startup lock, fullscreen retry and cleanup.');
} finally { await browser.close(); }
