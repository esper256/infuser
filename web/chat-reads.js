// A message counts only while some of it intersects the open log.
export function seenLimit(boxes, viewTop, viewBottom) {
  let max = 0;
  for (const box of boxes) {
    if (box.height && box.bottom > viewTop && box.top < viewBottom) max = Math.max(max, box.id);
  }
  return max;
}

// The cursor never moves backward, and a hidden or closed window does not advance it.
export function advancedRead(sent, seen, active) {
  if (!active || !(seen > sent)) return sent;
  return seen;
}

// Place every reader (including the current user) on the latest visible message at or before their cursor.
export function placeReads(messageIds, reads) {
  const buckets = new Map();
  for (const read of reads || []) {
    if (!read) continue;
    let target = 0;
    for (const id of messageIds) if (id <= read.message && id > target) target = id;
    if (!target) continue;
    const list = buckets.get(target) || [];
    list.push(read);
    buckets.set(target, list);
  }
  for (const list of buckets.values()) {
    list.sort((a, b) => (a.name || '').localeCompare(b.name || '') || (a.id || '').localeCompare(b.id || ''));
  }
  return buckets;
}
