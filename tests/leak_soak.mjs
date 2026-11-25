import assert from 'node:assert/strict';
import {readFile, mkdtemp, rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {EventEmitter} from 'node:events';
import test from 'node:test';
import {leakSoak, checkHeap, heapAllowance, heapGrowth, heapInventory, heapTrend} from './support/leak_soak.mjs';

const row = (i, bytes) => ({checkpoint: i ? `cycles-${i * 50}` : 'baseline', JSHeapUsedSize: bytes, Nodes: 40, JSEventListeners: 12, Documents: 1, Frames: 1});
const rows = values => values.map((bytes, i) => row(i, bytes));

test('2,000-cycle reference stays inside the smaller cap and has no sustained trend', async () => {
  const {checkpoints} = JSON.parse(await readFile(new URL('./fixtures/leak-soak-2000.json', import.meta.url)));
  for (let i = 1; i <= checkpoints.length; i++) checkHeap(checkpoints.slice(0, i));
  assert.equal(Math.max(...checkpoints.map(r => r.JSHeapUsedSize)) - checkpoints[0].JSHeapUsedSize, 216928);
});
test('sawtooth slow growth fails below the absolute cap, even with a final dip', () => {
  const samples = rows([0, 20000, 18000, 40000, 38000, 60000, 58000, 80000, 78000]);
  assert(samples.at(-1).JSHeapUsedSize < heapAllowance);
  assert.equal(heapTrend(samples).sustained, true);
  assert.throws(() => checkHeap(samples), /sustained post-GC heap growth/);
});
test('bounded oscillation and a single allocation step are not sustained growth', () => {
  for (const values of [[0, 20000, 18000, 20000, 18000, 20000, 18000, 20000, 18000], [0, 0, 0, 0, 0, 80000, 80000, 80000, 80000]]) {
    assert.equal(heapTrend(rows(values)).sustained, false);
    checkHeap(rows(values));
  }
  assert.equal(heapTrend(rows([0, 20000, 40000, 60000])).sustained, false);
});
test('absolute bytes and each resource remain strict gates', () => {
  checkHeap(rows([0, heapAllowance]));
  assert.throws(() => checkHeap(rows([0, heapAllowance + 1])), /320 KiB/);
  for (const metric of ['Nodes', 'JSEventListeners', 'Documents', 'Frames']) {
    const samples = rows([0, 0]); samples[1][metric]++;
    assert.throws(() => checkHeap(samples), /changed from baseline/);
  }
});

// Tiny V8-shaped graphs make attribution deterministic, including alternative
// retaining paths and an inspector table larger than the actual leaked array.
function snapshot(items) {
  const types = ['synthetic', 'object', 'native', 'array', 'code'], edgeTypes = ['property', 'internal', 'weak'];
  const strings = [], str = value => {let i = strings.indexOf(value); if (i < 0) {i = strings.length; strings.push(value);} return i;};
  const nodes = [], edges = [];
  for (const [type, name, id, size, links = []] of items) {
    nodes.push(types.indexOf(type), str(name), id, size, links.length);
    for (const [kind, label, target] of links) edges.push(edgeTypes.indexOf(kind), str(label), target * 5);
  }
  return {snapshot: {meta: {node_fields: ['type', 'name', 'id', 'self_size', 'edge_count'], edge_fields: ['type', 'name_or_index', 'to_node'], node_types: [types], edge_types: [edgeTypes]}}, nodes, edges, strings};
}
const baseline = snapshot([
  ['synthetic', '', 1, 0, [['internal', 'window', 1], ['internal', 'inspector', 3]]],
  ['object', 'Window', 2, 32, [['property', 'slowLeak', 2]]],
  ['object', 'Array', 3, 32, [['internal', 'elements', 5]]],
  ['native', 'blink::NetworkResourcesData', 4, 32, [['internal', 'table', 4]]],
  ['array', '(table)', 5, 64], ['array', '(object elements)', 6, 64],
]);
function growing({alternate = false, weak = false} = {}) {
  return snapshot([
    ['synthetic', '', 1, 0, [['internal', 'window', 1], ['internal', 'inspector', 3]]],
    ['object', 'Window', 2, 32, [[weak ? 'weak' : 'property', 'slowLeak', 2]]],
    ['object', 'Array', 3, 32, [['internal', 'elements', 5]]],
    ['native', 'blink::NetworkResourcesData', 4, 32, [['internal', 'table', 4], ...(alternate ? [['internal', 'alsoHeld', 5]] : [])]],
    ['array', '(table)', 5, 1000000], ['array', '(object elements)', 6, 10000],
  ]);
}
test('larger DevTools retention cannot displace the leaked array path', () => {
  const report = heapGrowth(heapInventory(baseline), growing());
  assert.match(report.candidates[0].retainer.join('\n'), /property slowLeak/);
  assert.doesNotMatch(report.candidates[0].retainer.join('\n'), /NetworkResourcesData/);
  assert.match(report.instrumentation[0].retainer.join('\n'), /NetworkResourcesData/);
  assert(report.instrumentation[0].largestGrowth > report.candidates[0].largestGrowth);
  assert(report.constructors.some(r => r.constructor === 'array: (object elements)' && r.grew === 0 && r.bytes === 9936));
});
test('an alternate application root path wins over an inspector path', () => {
  const report = heapGrowth(heapInventory(baseline), growing({alternate: true}));
  assert.match(report.candidates[0].retainer.join('\n'), /property slowLeak/);
});
test('weak edges never become strong retainer evidence', () => {
  const report = heapGrowth(heapInventory(baseline), growing({weak: true}));
  assert.equal(report.candidates.length, 0);
  assert.match(report.unreachable[0].retainer[0], /No strong root path/);
});
test('test harness and DevTools console retainers are labelled instrumentation', () => {
  const initial = heapInventory(snapshot([['synthetic', '', 1, 0]]));
  for (const label of ['__leakSoak', '25 / DevTools console']) {
    const report = heapGrowth(initial, snapshot([
      ['synthetic', '', 1, 0, [['property', label, 1]]],
      ['object', 'Object', 2, 24, [['property', 'retained', 2]]],
      ['array', '(object elements)', 3, 4096],
    ]));
    assert.equal(report.candidates.length, 0);
    assert.match(report.instrumentation[0].retainer.join('\n'), /retained/);
  }
});
test('many smaller retained arrays rank by aggregate growth', () => {
  const initial = heapInventory(snapshot([['synthetic', '', 1, 0]]));
  const report = heapGrowth(initial, snapshot([
    ['synthetic', '', 1, 0, [['property', 'arrays', 1], ['property', 'other', 4]]],
    ['object', 'Array', 2, 16, [['property', 'a', 2], ['property', 'b', 3]]],
    ['array', '(object elements)', 3, 400], ['array', '(object elements)', 4, 400],
    ['object', 'Other', 5, 700],
  ]));
  assert.equal(report.candidates[0].constructor, 'array: (object elements)');
  assert.equal(report.candidates[0].newOrExpandedBytes, 800);
});

test('small final increases alone do not imply sustained growth', () => {
  const samples = rows([0, 1000, 1500, 2000, 1800, 1900, 2000, 2100, 2200]);
  assert(samples.at(-3).JSHeapUsedSize < samples.at(-2).JSHeapUsedSize);
  assert(samples.at(-2).JSHeapUsedSize < samples.at(-1).JSHeapUsedSize);
  checkHeap(samples);
});

test('compiled-code growth is visible in the type summary, without calling it an application retainer', () => {
  const initial = heapInventory(snapshot([['synthetic', '', 1, 0]]));
  const report = heapGrowth(initial, snapshot([
    ['synthetic', '', 1, 0, [['internal', 'compiled', 1]]], ['code', 'system / Code', 2, 12000],
  ]));
  assert.deepEqual(report.typeGrowth.find(row => row.type === 'code'), {type: 'code', countDelta: 1, bytesDelta: 12000});
  assert.equal(report.candidates.length, 0);
});

test('a hung interaction still gets three GCs and a failure snapshot', async t => {
  const directory = await mkdtemp(join(tmpdir(), 'infuser-soak-test-'));
  t.after(() => rm(directory, {recursive: true, force: true}));
  t.mock.method(console, 'error', () => {}); t.mock.method(console, 'table', () => {});
  const cdp = new EventEmitter(); let garbageCollections = 0;
  cdp.send = async method => {
    if (method === 'HeapProfiler.collectGarbage') garbageCollections++;
    if (method === 'Performance.getMetrics') return {metrics: Object.entries(row(0, 100)).filter(([name]) => name !== 'checkpoint').map(([name, value]) => ({name, value}))};
    if (method === 'HeapProfiler.takeHeapSnapshot') cdp.emit('HeapProfiler.addHeapSnapshotChunk', {chunk: JSON.stringify(snapshot([['synthetic', '', 1, 0]]))});
  };
  const soak = await leakSoak(cdp, {directory, settle: () => {throw Error('Must not await hung interaction');}});
  await soak.failure(Error('voice cycle deadline'));
  assert.equal(garbageCollections, 3);
  assert.equal(JSON.parse(await readFile(join(soak.output, 'failure.json'))).error, 'voice cycle deadline');
  assert(JSON.parse(await readFile(join(soak.output, 'failure.heapsnapshot'))).snapshot);
});

test('long grid calibration fits unchanged gates after its fixed compilation warmup', async () => {
  const data = JSON.parse(await readFile(new URL('./fixtures/leak-grid-calibration.json', import.meta.url)));
  const samples = data.checkpoints.filter(([cycle]) => cycle >= 400).map(([cycle, JSHeapUsedSize, Nodes, JSEventListeners, Documents, Frames]) => ({
    checkpoint: cycle === 400 ? 'baseline' : `cycles-${cycle - 400}`, JSHeapUsedSize, Nodes, JSEventListeners, Documents, Frames,
  }));
  assert.equal(samples.at(-1).checkpoint, 'cycles-1600');
  for (let i = 1; i <= samples.length; i++) checkHeap(samples.slice(0, i));
});
