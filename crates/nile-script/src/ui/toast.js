// @nile/ui/toast: short messages that appear in a corner and go away on their own.
//
//   toast("Receipt saved");
//   toast.success("Payment received", { duration: 2500 });
//   toast.error("Printer is offline", { action: { label: "Retry", onClick: retry } });
//   const t = toast("Sending...", { duration: 0 });  t.update("Sent"); t.close();
//
// Options: type (info|success|warning|error), duration in ms (default 3500; 0 keeps it until closed), position
// (bottom-right default, bottom, bottom-left, top-right, top, top-left), action { label, onClick }, closable
// (default true), html (true = the message is HTML; default plain text). Hovering pauses the timer. At most
// `toast.max` (5) show at once. Toasts are announced to screen readers (role=status, errors role=alert).
// Style with --nile-toast-bg / -color / -radius, the type colours --nile-success / -error / -warning / -info, or
// the .nile-toast classes.

import { style } from "@nile/ui/base";

const CSS = `
:where(.nile-toasts) { position: fixed; z-index: 1200; display: flex; flex-direction: column; gap: 8px; padding: 16px; pointer-events: none; max-width: 100%; box-sizing: border-box; }
:where(.nile-toasts[data-position="bottom-right"]) { right: 0; bottom: 0; align-items: flex-end; }
:where(.nile-toasts[data-position="bottom-left"]) { left: 0; bottom: 0; align-items: flex-start; }
:where(.nile-toasts[data-position="bottom"]) { left: 0; right: 0; bottom: 0; align-items: center; }
:where(.nile-toasts[data-position="top-right"]) { right: 0; top: 0; align-items: flex-end; flex-direction: column-reverse; }
:where(.nile-toasts[data-position="top-left"]) { left: 0; top: 0; align-items: flex-start; flex-direction: column-reverse; }
:where(.nile-toasts[data-position="top"]) { left: 0; right: 0; top: 0; align-items: center; flex-direction: column-reverse; }
:where(.nile-toast) { pointer-events: auto; display: flex; align-items: center; gap: 12px; width: max-content; min-width: 220px; max-width: 420px; padding: 10px 14px; box-sizing: border-box;
  background: var(--nile-toast-bg, #2b2d31); color: var(--nile-toast-color, #f4f4f5); border-radius: var(--nile-toast-radius, 6px); border-left: 4px solid var(--nile-info, #3b82f6);
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.35); font-size: 14px; opacity: 0; transform: translateY(8px); transition: opacity 0.18s ease, transform 0.18s ease; }
:where(.nile-toast.nile-in) { opacity: 1; transform: none; }
:where(.nile-toast[data-type="success"]) { border-left-color: var(--nile-success, #22c55e); }
:where(.nile-toast[data-type="warning"]) { border-left-color: var(--nile-warning, #f59e0b); }
:where(.nile-toast[data-type="error"]) { border-left-color: var(--nile-error, #ef4444); }
:where(.nile-toast) > .nile-toast-text { flex: 1; min-width: 0; }
:where(.nile-toast) > button { background: transparent; color: inherit; border: 0; padding: 2px 6px; cursor: pointer; }
:where(.nile-toast) > button.nile-toast-action { color: var(--nile-toast-action, #93c5fd); font-weight: 600; }
`;

const DEFAULT_MS = 3500;
const regions = {};
const live = [];

function region(position) {
  style("toast", CSS);
  let box = regions[position];
  if (box && box.isConnected !== false && box.parentNode) return box;
  box = document.createElement("div");
  box.className = "nile-toasts";
  box.setAttribute("data-position", position);
  document.body.appendChild(box);
  regions[position] = box;
  return box;
}

export function toast(message, options) {
  const o = options || {};
  const type = o.type || "info";
  const position = o.position || "bottom-right";
  const el = document.createElement("div");
  el.className = "nile-toast";
  el.setAttribute("data-type", type);
  el.setAttribute("role", type === "error" ? "alert" : "status");
  const text = document.createElement("div");
  text.className = "nile-toast-text";
  const put = (m) => {
    if (o.html) text.innerHTML = String(m);
    else text.textContent = String(m);
  };
  put(message);
  el.appendChild(text);

  const handle = { element: el, closed: false };
  let timer = null;
  let rest = o.duration === undefined ? DEFAULT_MS : o.duration;
  let started = 0;
  const arm = () => {
    clearTimeout(timer);
    if (rest > 0) {
      started = Date.now();
      timer = setTimeout(() => handle.close(), rest);
    }
  };
  handle.close = () => {
    if (handle.closed) return;
    handle.closed = true;
    clearTimeout(timer);
    const at = live.indexOf(handle);
    if (at >= 0) live.splice(at, 1);
    el.classList.remove("nile-in");
    setTimeout(() => {
      if (el.parentNode) el.parentNode.removeChild(el);
    }, 190);
    if (o.onClose) o.onClose();
  };
  handle.update = (m, patch) => {
    put(m);
    if (patch && patch.type) {
      el.setAttribute("data-type", patch.type);
    }
    if (patch && patch.duration !== undefined) {
      rest = patch.duration;
      arm();
    }
  };

  if (o.action && o.action.label) {
    const a = document.createElement("button");
    a.className = "nile-toast-action";
    a.textContent = o.action.label;
    a.addEventListener("click", () => {
      if (o.action.onClick) o.action.onClick(handle);
      handle.close();
    });
    el.appendChild(a);
  }
  if (o.closable !== false) {
    const x = document.createElement("button");
    x.setAttribute("aria-label", "Dismiss");
    x.textContent = "×";
    x.addEventListener("click", () => handle.close());
    el.appendChild(x);
  }
  el.addEventListener("mouseenter", () => {
    if (rest > 0) {
      clearTimeout(timer);
      rest = Math.max(800, rest - (Date.now() - started));
    }
  });
  el.addEventListener("mouseleave", () => {
    if (!handle.closed && (o.duration === undefined || o.duration > 0)) arm();
  });

  region(position).appendChild(el);
  live.push(handle);
  while (live.length > toast.max) live[0].close();
  setTimeout(() => el.classList.add("nile-in"), 16);
  arm();
  return handle;
}

toast.max = 5;
toast.info = (m, o) => toast(m, Object.assign({}, o, { type: "info" }));
toast.success = (m, o) => toast(m, Object.assign({}, o, { type: "success" }));
toast.warning = (m, o) => toast(m, Object.assign({}, o, { type: "warning" }));
toast.error = (m, o) => toast(m, Object.assign({}, o, { type: "error" }));
/** Close every toast that is showing. */
toast.clear = () => live.slice().forEach((h) => h.close());
