from __future__ import annotations

from peval_py.adapters.common import CommonMessageAdapter


class HermesAdapter(CommonMessageAdapter):
    agent_id = "hermes"
    default_agent_name = "hermes"
