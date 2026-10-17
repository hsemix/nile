import native from '@native';

function encode(params) {
  if (!Array.isArray(params)) throw new TypeError('SQLite params must be an array');
  const buffers = [];
  const values = params.map(value => {
    if (typeof value === 'bigint') return { $i64: String(value) };
    if (value instanceof Uint8Array || value instanceof ArrayBuffer) {
      const index = buffers.length;
      buffers.push(value instanceof Uint8Array ? value : new Uint8Array(value));
      return { $blob: index };
    }
    if (typeof value === 'number' && (!Number.isFinite(value) || (Number.isInteger(value) && !Number.isSafeInteger(value)))) {
      throw Object.assign(new Error('Use BigInt for integers outside the JS safe range'), { code: 'SQLITE_INTEGER' });
    }
    if (value === null || typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') return value;
    throw new TypeError('SQLite parameters support null, strings, numbers, booleans, BigInt and binary buffers');
  });
  return { values, buffers };
}
function decode(packet) {
  const cell = value => {
    if (value && typeof value === 'object') {
      if ('$i64' in value && Object.keys(value).length === 1) return BigInt(value.$i64);
      if ('$blob' in value && Object.keys(value).length === 1) return packet.buffers[value.$blob];
    }
    return value;
  };
  const record = row => {
    const result = {};
    for (const key of Object.keys(row)) Object.defineProperty(result, key, { value: cell(row[key]), enumerable: true, writable: true, configurable: true });
    return result;
  };
  if (packet.value === null) return null;
  return Array.isArray(packet.value) ? packet.value.map(record) : record(packet.value);
}

export async function open(path, options = {}) {
  const packet = await native.call('sqlite.open', { path, options });
  const handle = packet.value.handle;
  let state = 'open';
  let closing = null;
  const statement = (operation, sql, params = []) => {
    if (state !== 'open') return Promise.reject(Object.assign(new Error('Connection is closed'), { code: 'SQLITE_CLOSED' }));
    try {
      const { values, buffers } = encode(params);
      const request = native.call(operation, { handle, sql, params: values }, buffers);
      const result = request.then(decode);
      result.cancel = request.cancel;
      return result;
    } catch (error) { return Promise.reject(error); }
  };
  return Object.freeze({
    execute(sql, params) { return statement('sqlite.execute', sql, params); },
    query(sql, params) { return statement('sqlite.query', sql, params); },
    close() {
      if (state !== 'open') return closing;
      state = 'closing';
      closing = native.call('sqlite.close', { handle }).then(packet => {
        state = 'closed';
        return decode(packet);
      }, error => {
        // Retry only a confirmed rejection before execution. Keep ambiguous
        // failures blocked: the native connection may already have closed.
        if (error.preExecution === true) {
          state = 'open';
          closing = null;
        }
        throw error;
      });
      return closing;
    }
  });
}
export default Object.freeze({ open });
