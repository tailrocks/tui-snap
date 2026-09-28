'use strict';
// Example: version + capabilities round trip through the thin client.
// Usage: TUISNAP_BIN=/path/to/tuisnap node example.js
const { Client } = require('./index');

(async () => {
  const c = new Client();
  try {
    const version = await c.call({ type: 'version' });
    console.log(JSON.stringify(version));
    const caps = await c.call({ type: 'capabilities' });
    console.log(JSON.stringify(caps));
  } finally {
    await c.close();
  }
})().catch((e) => {
  console.error(`example failed: ${e.message}`);
  process.exit(1);
});
