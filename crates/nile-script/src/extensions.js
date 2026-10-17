// The native package loader supplies this bridge only to configured wrapper modules.
(() => {
  let disposed = false;
  const pending = new Map();
  const subscriptions = new Map();
  const key = (host, id) => host + ':' + id;
  const encode = bytes => {
    let text = '';
    for (const byte of bytes) text += String.fromCharCode(byte);
    return btoa(text);
  };
  const decode = text => Uint8Array.from(atob(text), c => c.charCodeAt(0));
  const failure = (code, message, details = {}) => Object.assign(new Error(message), { code }, details);
  globalThis.__nileExtensionScope = host => {
    const start = (operation, args, buffers, listener) => {
      if (disposed) throw failure('CONTEXT_CLOSED', 'Extension context closed');
      const result = JSON.parse(__nativeExtStart(host, operation, JSON.stringify(args), buffers.map(encode), !!listener));
      if (result.error) throw failure('DISPATCH', result.error, { preExecution: true, dispatchStatus: null });
      const id = result.id;
      const token = key(host, id);
      const promise = new Promise((resolve, reject) => pending.set(token, { resolve, reject }));
      if (listener) subscriptions.set(token, listener);
      const cancel = () => {
        __nativeExtCancel(host, id);
        const request = pending.get(token);
        pending.delete(token);
        subscriptions.delete(token);
        if (request) request.reject(failure('CANCELLED', 'Extension call cancelled; native work may still be running.'));
      };
      promise.cancel = cancel;
      return { promise, cancel };
    };
    return Object.freeze({
      call(operation, args = {}, buffers = []) {
        try { return start(operation, args, buffers, null).promise; }
        catch (error) { return Promise.reject(error); }
      },
      subscribe(operation, args, listener, buffers = []) {
        if (typeof listener !== 'function') throw new TypeError('A subscription requires a listener');
        const request = start(operation, args, buffers, listener);
        return { ready: request.promise, unsubscribe: request.cancel };
      }
    });
  };
  globalThis.__nativeExtDeliver = packet => {
    const token = key(packet.host, packet.call);
    const value = { value: JSON.parse(packet.json), buffers: packet.buffers.map(decode) };
    if (packet.event) {
      const listener = subscriptions.get(token);
      if (listener) {
        try { listener(value); } catch (error) { console.error(error); }
      }
      return;
    }
    const request = pending.get(token);
    if (!request) return;
    pending.delete(token);
    if (packet.status === 0) request.resolve(value);
    else {
      subscriptions.delete(token);
      // NILE_OVERLOADED (4) from dispatch guarantees that no work was accepted.
      // Other dispatch failures can be ambiguous; ordinary operation errors never
      // acquire this flag merely by returning the same status or error code.
      request.reject(failure(packet.code, packet.message, {
        dispatchStatus: packet.dispatchStatus,
        preExecution: packet.dispatchStatus === 4
      }));
    }
  };
  globalThis.__nativeExtDispose = () => {
    disposed = true;
    for (const request of pending.values()) request.reject(failure('CONTEXT_CLOSED', 'Extension context closed'));
    pending.clear();
    subscriptions.clear();
  };
})();
