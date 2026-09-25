"""Report only bounded Problem identity for unexpected server errors."""

import json
import re
import sys


def report_http_server_error(error):
    if not 500 <= error.code <= 599:
        return
    try:
        problem = json.loads(error.read(4096).decode("utf-8"))
    except (AttributeError, OSError, UnicodeError, ValueError):
        problem = None
    code = problem.get("code") if isinstance(problem, dict) else None
    if not isinstance(code, str) or re.fullmatch(r"[a-z][a-z0-9_]{0,63}", code) is None:
        code = "unavailable"
    print(f"HTTP {error.code} Problem code={code}", file=sys.stderr)
