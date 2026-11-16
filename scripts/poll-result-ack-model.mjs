// Offline state model only: this module is not wired to the QQ bridge.
// The caller must supply a unique listener generation and acknowledge only after
// independently accepting the complete batch. This is not a journal ACK.
export class PollResultAckModel {
  #generation; #entries = new Map(); #flight = null; #revision = 0; #batch = 0;
  #capacity; #maxBatch; #maxBytes; #limit;
  constructor({generation, capacity = 128, maxBatch = 32, maxBytes = 2048, sequenceLimit = Number.MAX_SAFE_INTEGER}) {
    if (typeof generation !== 'string' || !generation || Buffer.byteLength(generation) > 128 ||
        ![capacity, maxBatch, maxBytes, sequenceLimit].every(n => Number.isSafeInteger(n) && n > 0))
      throw new TypeError('invalid model limits or generation');
    this.#generation = generation; this.#capacity = capacity; this.#maxBatch = maxBatch;
    this.#maxBytes = maxBytes; this.#limit = sequenceLimit;
  }
  put(key, value) {
    if (typeof key !== 'string' || !key || Buffer.byteLength(key) > 128 ||
        !value || !['pending', 'accepted', 'unknown', 'done', 'rejected'].includes(value.status))
      return false;
    if (!this.#entries.has(key) && this.#entries.size >= this.#capacity) return false;
    if (this.#revision >= this.#limit) return false;
    let json;
    try { json = JSON.stringify(value); } catch { return false; }
    if (typeof json !== 'string' || Buffer.byteLength(json) > this.#maxBytes) return false;
    this.#entries.set(key, {revision: ++this.#revision, value: JSON.parse(json)});
    return true;
  }
  poll() {
    if (this.#flight) return structuredClone(this.#flight.public);
    if (this.#batch >= this.#limit) return null;
    const entries = [...this.#entries].filter(([, e]) => !['pending', 'accepted'].includes(e.value.status)).slice(0, this.#maxBatch);
    if (!entries.length) return null;
    const token = {generation: this.#generation, batch: ++this.#batch};
    const publicBatch = {token, results: entries.map(([key, e]) => ({key, value: structuredClone(e.value)}))};
    this.#flight = {public: publicBatch, revisions: entries.map(([key, e]) => [key, e.revision])};
    return structuredClone(publicBatch);
  }
  acknowledge(token) {
    if (!this.#flight || !token || token.generation !== this.#generation ||
        token.batch !== this.#flight.public.token.batch) return false;
    for (const [key, revision] of this.#flight.revisions) {
      if (this.#entries.get(key)?.revision === revision) this.#entries.delete(key);
    }
    this.#flight = null;
    return true;
  }
  inspect(key) { const e = this.#entries.get(key); return e ? structuredClone(e.value) : undefined; }
  get size() { return this.#entries.size; }
}
