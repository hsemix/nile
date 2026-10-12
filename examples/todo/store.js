// A tiny observable store, imported by the app as an ES module.
export function createStore(items = []) {
  const listeners = [];
  return {
    items,
    subscribe: (fn) => listeners.push(fn),
    add(text) { items.push({ text, done: false }); listeners.forEach((f) => f()); },
    toggle(i) { items[i].done = !items[i].done; listeners.forEach((f) => f()); },
    get remaining() { return items.filter((t) => !t.done).length; },
  };
}
