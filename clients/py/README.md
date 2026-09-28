# tuisnap-client

Thin Python client for tui-snap (backlog A09). It spawns `tuisnap --machine`
and sends typed ops; every check runs in the shared Rust engine. This package
has **zero assertion semantics** — op errors propagate verbatim.

## Use

```python
from tuisnap_client import Client

with Client() as c:  # or Client("/path/to/tuisnap"), or TUISNAP_BIN
    v = c.call({"type": "version"})
    r = c.call({"type": "assert", "check": "text-contains", "text": "hi", "needle": "h"})
```

`call(op)` returns the `OpResult` dict or raises `OpError` (`code`,
`message`, `session`) copied verbatim from the engine envelope.

## Example

```sh
TUISNAP_BIN=../../target/debug/tuisnap python3 example.py
```
