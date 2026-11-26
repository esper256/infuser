// Ordinary appearance/layout journeys must not depend on the day CI runs.
// Scope the fixed calendar to themes.js; real app timers and Date.now stay live.
export function ordinaryThemeCalendar(source) {
  const time = new Date(2026, 6, 1, 12).getTime();
  return `(() => { const Date = class extends globalThis.Date { constructor(...args) { super(...(args.length ? args : [${time}])); } };\n${source}\n})();`;
}
