# @tuisnap/client

Thin Node client for tui-snap (backlog A09). It spawns `tuisnap --machine`
and sends typed ops; every check runs in the shared Rust engine. This package
has **zero assertion semantics** — op errors propagate verbatim.

## Use

```js
const { Client } = require('@tuisnap/client');

const c = new Client(process.env.TUISNAP_BIN || 'tuisnap');
try {
  const v = await c.call({ type: 'version' });
  const r = await c.call({ type: 'assert', check: 'text-contains', text: 'hi', needle: 'h' });
} finally {
  await c.close();
}
```

`call(op)` resolves with the `OpResult` or rejects with an `OpError`
(`code`, `message`, `session`) copied verbatim from the engine envelope.

## Example

```sh
TUISNAP_BIN=../../target/debug/tuisnap node example.js
```
