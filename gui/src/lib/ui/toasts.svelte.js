/**
 * Toast store.
 *
 * The old build rendered errors as a `.banner` inside normal document flow,
 * so every error pushed the entire page down and every dismissal pulled it
 * back up — a layout shift on the most stressful moment in the app. Toasts
 * are absolutely positioned and never affect layout.
 *
 * `error` toasts stay until dismissed (an error you didn't see is an error
 * that didn't get reported); `success` and `info` auto-expire.
 */

let seq = 0;

/** @type {{id:number, tone:string, title:string, detail:string|null, action:{label:string,run:()=>void}|null}[]} */
export const toasts = $state([]);

function push(tone, title, { detail = null, action = null, ttl } = {}) {
  const id = ++seq;
  toasts.push({ id, tone, title, detail, action });
  if (ttl) setTimeout(() => dismiss(id), ttl);
  return id;
}

export function dismiss(id) {
  const i = toasts.findIndex((t) => t.id === id);
  if (i >= 0) toasts.splice(i, 1);
}

export const toast = {
  success: (title, opts) => push("ok", title, { ttl: 3200, ...opts }),
  info: (title, opts) => push("info", title, { ttl: 4200, ...opts }),
  warn: (title, opts) => push("warn", title, { ttl: 6000, ...opts }),
  /** Sticky by default — pass `ttl` to override. */
  error: (title, opts) => push("danger", title, opts),
};
