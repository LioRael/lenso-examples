"""Expected output for the reference App's deterministic TypeScript excerpt."""

import re


# ECMAScript WhiteSpace and LineTerminator characters used by /\s+/u and trim.
_WHITESPACE = re.compile(
    r"[\u0009-\u000d\u0020\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff]+"
)


def expected_excerpt(text: str, limit: int) -> str:
    normalized = _WHITESPACE.sub(" ", text).strip(" ")
    characters = list(normalized)
    if len(characters) <= limit:
        return normalized
    return "".join(characters[: limit - 1]) + "…"
