# Psychevo Python SDK

`psychevo` is the async Python client for the Psychevo Framework. It starts the
exact-version `psychevo-app-server-bin` executable over stdio by default and
also supports an explicit executable or authenticated WebSocket endpoint.

```python
from pathlib import Path

import psychevo


async with psychevo.Client() as client:
    thread = await client.start_thread(cwd=Path.cwd())
    turn = await thread.start_turn("Inspect this repository")
    result = await turn.wait()
    print(result.final_answer)
```

Python 3.11 or newer is required. The API is async-only. See the
[SDK guide](https://github.com/wowfun/psychevo/blob/main/docs/sdk.md) for
events, controls, callbacks, reconnects, and transport configuration.
