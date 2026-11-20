import test from 'node:test';
import assert from 'node:assert/strict';
import { seenLimit, advancedRead } from '../web/chat-reads.js';

const boxes = [
  {id: 1, top: 0, bottom: 40, height: 40},
  {id: 2, top: 40, bottom: 80, height: 40},
  {id: 3, top: 80, bottom: 120, height: 40},
  {id: 4, top: 120, bottom: 160, height: 40},
];

test('only the messages actually inside the log count', () => {
  assert.equal(seenLimit(boxes, 40, 80), 2);
  assert.equal(seenLimit(boxes, 30, 90), 3);
  assert.equal(seenLimit(boxes, 160, 200), 0);
  assert.equal(seenLimit([{id: 9, top: 0, bottom: 0, height: 0}], 0, 100), 0);
});

test('the cursor moves forward only while chat is open and the page is visible', () => {
  assert.equal(advancedRead(2, 4, true), 4);
  assert.equal(advancedRead(4, 2, true), 4);
  assert.equal(advancedRead(4, 4, true), 4);
  assert.equal(advancedRead(1, 4, false), 1);
});
