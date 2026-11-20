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
