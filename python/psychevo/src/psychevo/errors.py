class PsychevoError(Exception):
    """Base SDK error."""


class ProtocolError(PsychevoError):
    def __init__(self, code: int, message: str, data: object = None) -> None:
        super().__init__(message)
        self.code = code
        self.data = data


class TransportError(PsychevoError):
    """The App Server transport ended or produced invalid framing."""


class RequestTimeoutError(PsychevoError):
    """An RPC exceeded its deadline after delivery may have begun."""

    def __init__(
        self,
        method: str,
        timeout: float,
        delivery_unknown: bool,
    ) -> None:
        delivery = "delivery is unknown" if delivery_unknown else "request was not delivered"
        super().__init__(
            f"{method} timed out after {timeout:g}s; {delivery}; the SDK will not retry"
        )
        self.method = method
        self.timeout = timeout
        self.delivery_unknown = delivery_unknown
