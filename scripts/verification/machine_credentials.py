"""Normalize machine credentials before hashing or constructing Bearer headers."""


def normalize_machine_credential(value: str) -> str:
    """Return the exact raw bytes-as-text from raw or Bearer-prefixed input."""
    raw = value.removeprefix("Bearer ")
    if not raw or "\r" in raw or "\n" in raw:
        raise ValueError("machine credential is unavailable or malformed")
    return raw
