// @nile/ui/base: helpers the components share. Not meant for apps to call directly.

/** Add a <style data-nile-ui=id> to the page once. Rules should use :where() so authors can override them. */
export function style(id, text) {
  if (typeof document === "undefined" || !document.head) return;
  if (document.head.querySelector('style[data-nile-ui="' + id + '"]')) return;
  const el = document.createElement("style");
  el.setAttribute("data-nile-ui", id);
  el.textContent = text;
  document.head.appendChild(el);
}

/** Fire a bubbling CustomEvent; returns false when a listener called preventDefault (when cancelable). */
export function emit(el, type, detail, cancelable) {
  const event = new CustomEvent(type, { detail: detail, bubbles: true, cancelable: !!cancelable });
  el.dispatchEvent(event);
  return !event.defaultPrevented;
}

const FOCUSABLE = 'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

/** The elements inside `root` the Tab key can reach, in document order. */
export function focusable(root) {
  return Array.from(root.querySelectorAll(FOCUSABLE)).filter((el) => !el.disabled && el.getAttribute("tabindex") !== "-1");
}

/** An attribute as a number, or `fallback`. */
export function numberAttr(el, name, fallback) {
  const v = el.getAttribute(name);
  const n = v === null || v === "" ? NaN : Number(v);
  return Number.isFinite(n) ? n : fallback;
}

/** An attribute as a boolean: present and not "false"/"off"/"0". */
export function flagAttr(el, name, fallback) {
  const v = el.getAttribute(name);
  if (v === null) return fallback;
  return !(v === "false" || v === "off" || v === "0");
}

/** A CSS length from an attribute: a bare number is px. */
export function lengthAttr(el, name, fallback) {
  const v = el.getAttribute(name);
  if (v === null || v === "") return fallback;
  return /^\d+(\.\d+)?$/.test(v) ? v + "px" : v;
}

/** How many modal layers (dialogs) are open; shortcuts stay quiet while it is above 0. */
export const modal = { depth: 0 };
