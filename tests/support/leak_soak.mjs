// Test-only CDP measurements. No browser-side memory estimate or clock control.
import assert from 'node:assert/strict';
import {mkdir, mkdtemp, writeFile} from 'node:fs/promises';
import {join, resolve} from 'node:path';

const metrics = ['JSHeapUsedSize', 'Nodes', 'JSEventListeners', 'Documents', 'Frames'];
const resources = metrics.slice(1);
const heapAllowance = 1024 * 1024;

function graph(snapshot) {
  const {node_fields: nf, edge_fields: ef, node_types: nt, edge_types: et} = snapshot.snapshot.meta;
  const field = name => nf.indexOf(name), edgeField = name => ef.indexOf(name);
  const width = nf.length, edgeWidth = ef.length;
  const nodes = snapshot.nodes, edges = snapshot.edges, strings = snapshot.strings;
  const count = nodes.length / width, starts = new Uint32Array(count + 1);
  let cursor = 0;
  for (let i = 0; i < count; i++) {
    starts[i] = cursor;
    cursor += nodes[i * width + field('edge_count')] * edgeWidth;
  }
  starts[count] = cursor;
  const node = i => ({
    id: nodes[i * width + field('id')],
    type: nt[field('type')][nodes[i * width + field('type')]],
    name: strings[nodes[i * width + field('name')]],
    size: nodes[i * width + field('self_size')],
  });
  const edge = offset => {
    const type = et[edgeField('type')][edges[offset + edgeField('type')]];
    const name = edges[offset + edgeField('name_or_index')];
    return {type, name: ['element', 'hidden'].includes(type) ? String(name) : strings[name], to: edges[offset + edgeField('to_node')] / width};
  };
  return {node, edge, count, starts, edgeWidth};
}

function inventory(g) {
  const ids = new Map(), constructors = new Map();
  for (let i = 0; i < g.count; i++) {
    const n = g.node(i);
    ids.set(n.id, n.size);
    // V8 names object/native nodes by constructor; retain storage/closure kinds too.
    if (!['object', 'native', 'closure', 'array'].includes(n.type)) continue;
    const key = `${n.type}: ${n.name}`;
    const value = constructors.get(key) || {count: 0, bytes: 0};
    value.count++; value.bytes += n.size;
    constructors.set(key, value);
  }
  return {ids, constructors};
}

function growth(baseline, current) {
  const g = graph(current), counts = inventory(g).constructors;
  const constructors = [...counts].map(([constructor, value]) => ({
    constructor, count: value.count, grew: value.count - (baseline.constructors.get(constructor)?.count || 0),
    bytes: value.bytes - (baseline.constructors.get(constructor)?.bytes || 0),
  })).filter(row => row.grew > 0).sort((a, b) => b.bytes - a.bytes || b.grew - a.grew);
  let largest = -1, increase = 0;
  for (let i = 0; i < g.count; i++) {
    const n = g.node(i), delta = n.size - (baseline.ids.get(n.id) || 0);
    if (delta > increase) {largest = i; increase = delta;}
  }
  if (largest < 0) return {constructors, retainer: ['No node grew in self size.']};
  // A shortest strong path from the synthetic GC root, never a weak reference.
  const unseen = 0xffffffff, parent = new Uint32Array(g.count).fill(unseen);
  const parentEdge = new Uint32Array(g.count), queue = new Uint32Array(g.count);
  let read = 0, end = 1; queue[0] = 0; parent[0] = 0;
  while (read < end && parent[largest] === unseen) {
    const from = queue[read++];
    for (let offset = g.starts[from]; offset < g.starts[from + 1]; offset += g.edgeWidth) {
      const e = g.edge(offset);
      if (e.type === 'weak' || parent[e.to] !== unseen) continue;
      parent[e.to] = from; parentEdge[e.to] = offset; queue[end++] = e.to;
    }
  }
  const describe = i => {const n = g.node(i); return `${n.type} ${n.name.slice(0, 120) || '(root)'} @${n.id} (${n.size} bytes)`;};
  const retainer = [];
  if (parent[largest] === unseen) retainer.push(`No strong root path found for ${describe(largest)}`);
  else {
    let at = largest;
    while (at !== 0) {
      const e = g.edge(parentEdge[at]);
      retainer.push(`  --${e.type} ${e.name}--> ${describe(at)}`);
      at = parent[at];
    }
    retainer.push(describe(0)); retainer.reverse();
  }
  return {constructors, largestGrowth: increase, retainer};
}

export async function leakSoak(cdp, {directory = '.local/leak-soak', settle = async () => {}} = {}) {
  await mkdir(directory, {recursive: true});
  const output = await mkdtemp(join(resolve(directory), 'run-'));
  await cdp.send('Performance.enable');
  await cdp.send('HeapProfiler.enable');
  const rows = [];
  let baseline, baselineHeap;
  async function snapshot(label) {
    const chunks = [], receive = ({chunk}) => chunks.push(chunk);
    cdp.on('HeapProfiler.addHeapSnapshotChunk', receive);
    try {await cdp.send('HeapProfiler.takeHeapSnapshot', {reportProgress: false});}
    finally {cdp.off('HeapProfiler.addHeapSnapshotChunk', receive);}
    const json = chunks.join(''), file = join(output, `${label}.heapsnapshot`);
    await writeFile(file, json);
    return {file, heap: JSON.parse(json)};
  }
  async function measure(label) {
    await settle();
    for (let i = 0; i < 3; i++) await cdp.send('HeapProfiler.collectGarbage');
    const response = await cdp.send('Performance.getMetrics');
    const available = new Map(response.metrics.map(m => [m.name, m.value]));
    const row = {checkpoint: label};
    for (const name of metrics) {
      assert(Number.isFinite(available.get(name)), `CDP did not report ${name}`);
      row[name] = available.get(name);
    }
    rows.push(row);
    const shot = await snapshot(label);
    return {row, ...shot};
  }
  async function checkpoint(label) {
    assert(/^[a-z0-9-]+$/.test(label), 'Use a filename-safe checkpoint label');
    const {row, heap} = await measure(label);
    if (!baseline) {baseline = row; baselineHeap = inventory(graph(heap));}
    else {
      for (const key of resources) assert.equal(row[key], baseline[key], `${label}: ${key} changed from baseline`);
      assert(Math.abs(row.JSHeapUsedSize - baseline.JSHeapUsedSize) <= heapAllowance, `${label}: post-GC heap moved more than 1 MiB from baseline`);
    }
    console.table([row]);
  }
  async function finish() {
    assert(rows.length >= 4, 'Need a baseline and at least three checkpoints');
    const last = rows.slice(-3).map(row => row.JSHeapUsedSize);
    assert(!(last[0] < last[1] && last[1] < last[2]), 'Post-GC heap is strictly increasing across the last three checkpoints');
    console.table(rows);
    console.log(`Heap snapshots: ${output}`);
  }
  async function failure(error) {
    const {file, heap} = await measure('failure');
    const detail = growth(baselineHeap || {ids: new Map(), constructors: new Map()}, heap);
    console.error(`Leak soak failed: ${error.message}\nFailure snapshot: ${file}`);
    console.table(rows);
    console.error('Constructor counts that grew:');
    for (const row of detail.constructors) console.error(`${row.constructor}: ${row.count} (+${row.grew}); self-size delta ${row.bytes} bytes`);
    console.error(`Retainer path for largest growing node (+${detail.largestGrowth || 0} bytes):\n${detail.retainer.join('\n')}`);
    await writeFile(join(output, 'failure.json'), JSON.stringify({error: error.message, checkpoints: rows, ...detail}, null, 2));
  }
  return {checkpoint, finish, failure, output};
}
