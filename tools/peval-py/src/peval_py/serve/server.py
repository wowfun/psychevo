from __future__ import annotations

import sys

from peval_py.serve.constants import *  # noqa: F401,F403
from peval_py.serve.handler import *  # noqa: F401,F403
from peval_py.serve.lifecycle import *  # noqa: F401,F403
from peval_py.serve.payloads import *  # noqa: F401,F403
from peval_py.serve.reporting import *  # noqa: F401,F403
from peval_py.serve.runtime import *  # noqa: F401,F403
from peval_py.serve.sources import *  # noqa: F401,F403

if __name__ == "__main__":
    print("peval_py.serve is not a standalone entry point", file=sys.stderr)
