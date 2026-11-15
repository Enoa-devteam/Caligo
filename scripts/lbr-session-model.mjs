/** Offline state model. It has no ETW backend and must not control real sessions. */
const REQUIRED = ['isolated','privateOutput','bounded','noConflict','clockDefined'];
const VALID_ID = /^[a-f0-9]{32}$/;

export class LabSessionModel {
  #state = 'new';
  #handle = null;
  #identity;
  #outcome = 'unknown';
  #cleanup = 'not_needed';

  constructor(identity) {
    if (typeof identity !== 'string' || !VALID_ID.test(identity)) throw new TypeError('invalid lab identity');
    this.#identity = identity;
  }
  snapshot() {
    return Object.freeze({state:this.#state, outcome:this.#outcome, cleanup:this.#cleanup});
  }
  preflight(facts) {
    if (this.#state !== 'new') throw new Error('invalid transition');
    if (!facts || REQUIRED.some(key => facts[key] !== true)) {
      this.#state = 'refused';
      return null;
    }
    this.#state = 'ready';
    return null;
  }
  begin() {
    if (this.#state !== 'ready') throw new Error('invalid transition');
    this.#state = 'starting';
    return Object.freeze({action:'start_lab_session', identity:this.#identity});
  }
  started(result) {
    if (this.#state !== 'starting') throw new Error('invalid transition');
    if (!result || result.kind !== 'started' || result.identity !== this.#identity ||
        typeof result.handle !== 'string' || !VALID_ID.test(result.handle)) {
      // A failed or ambiguous start may have created a session without returning ownership.
      this.#state = 'ownership_unknown';
      this.#cleanup = 'unknown';
      return null;
    }
    this.#handle = result.handle;
    this.#state = 'running';
    this.#cleanup = 'pending';
    return null;
  }
  finish(reason) {
    if (this.#state !== 'running' || !['complete','failure','timeout'].includes(reason)) throw new Error('invalid transition');
    this.#outcome = reason === 'complete' ? 'awaiting_cleanup' : 'unknown';
    this.#state = 'stopping';
    return Object.freeze({action:'stop_owned_lab_session', identity:this.#identity, handle:this.#handle});
  }
  stopped(result) {
    if (this.#state !== 'stopping') throw new Error('invalid transition');
    if (!result || result.kind !== 'stopped' || result.identity !== this.#identity || result.handle !== this.#handle) {
      this.#state = 'cleanup_unknown';
      this.#cleanup = 'unknown';
      this.#outcome = 'unknown';
      return null;
    }
    this.#handle = null;
    this.#state = 'closed';
    this.#cleanup = 'confirmed_by_backend';
    if (this.#outcome === 'awaiting_cleanup') this.#outcome = 'workload_completed';
    return null;
  }
  controllerLost() {
    if (!['starting','running','stopping'].includes(this.#state)) throw new Error('invalid transition');
    this.#handle = null;
    this.#state = 'ownership_unknown';
    this.#cleanup = 'unknown';
    this.#outcome = 'unknown';
  }
}
