class PsychevoError(Exception):
    """Base SDK error."""


class ProtocolError(PsychevoError):
    def __init__(self, code: int, message: str, data: object = None) -> None:
        super().__init__(message)
        self.code = code
        self.data = data


class TransportError(PsychevoError):
    """The App Server transport ended or produced invalid framing."""
