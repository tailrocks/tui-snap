'use strict';
// Thin tui-snap client (A09). Spawns `tuisnap --machine`, sends one Op JSON
// object per line on stdin, reads one envelope JSON object per line on
// stdout. Responses resolve in order; op errors reject verbatim with the
// engine's {code, message, session}. No assertions live here: even the
// `assert` op just round-trips to the shared Rust engine.

const { spawn } = require('node:child_process');
const readline = require('node:readline');

class OpError extends Error {
  constructor(code, message, session) {
    super(session ? `[${code}] ${session}: ${message}` : `[${code}] ${message}`);
    this.name = 'OpError';
    this.code = code;
    this.session = session;
  }
}

class Client {
  constructor(bin = process.env.TUISNAP_BIN || 'tuisnap') {
    this.bin = bin;
    this.proc = null;
    this.rl = null;
    this.pending = [];
    this.started = false;
  }

  start(argvExtra = []) {
    if (this.started) return this;
    this.proc = spawn(this.bin, ['--machine', ...argvExtra], {
      stdio: ['pipe', 'pipe', 'inherit'],
    });
    this.rl = readline.createInterface({ input: this.proc.stdout });
    this.rl.on('line', (line) => {
      const next = this.pending.shift();
      if (!next) return;
      let env;
      try {
        env = JSON.parse(line);
      } catch (e) {
        next.reject(new Error(`bad envelope: ${e.message}: ${line}`));
        return;
      }
      if (env.ok) next.resolve(env.result);
      else {
        const err = env.error || {};
        next.reject(new OpError(err.code || 'op-failed', err.message || 'unknown error', err.session));
      }
    });
    this.proc.on('error', (e) => this.#failAll(e));
    this.proc.on('exit', () => this.#failAll(new Error(`${this.bin} exited`)));
    this.started = true;
    return this;
  }

  #failAll(e) {
    const pending = this.pending;
    this.pending = [];
    for (const p of pending) p.reject(e);
  }

  call(op) {
    this.start();
    return new Promise((resolve, reject) => {
      this.pending.push({ resolve, reject });
      this.proc.stdin.write(JSON.stringify(op) + '\n', (e) => {
        if (e) {
          this.pending.pop();
          reject(e);
        }
      });
    });
  }

  async close() {
    if (!this.started) return;
    this.started = false;
    this.rl.close();
    this.proc.stdin.end();
    await new Promise((resolve) => this.proc.once('exit', resolve));
  }
}

module.exports = { Client, OpError };
