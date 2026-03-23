#!/usr/bin/env python3
"""
Security and quality pre-flight checker for Python code review.

Usage: python3 scripts/check.py <file>
       python3 scripts/check.py --help

Output: JSON array of findings written to stdout. Each finding has:
  line  - 1-based line number
  check - check name (see list below)
  text  - the offending line (stripped)

Exit codes:
  0 - completed successfully (findings may still be present)
  1 - usage error or file could not be read

Checks:
  hardcoded-secret    - password/secret/token/key assigned a string literal
  sql-format-string   - SQL query built with f-string or % formatting
  weak-hash           - hashlib.md5 or hashlib.sha1 used
  broad-except        - bare `except:` or `except Exception:` that swallows errors
"""

import json
import re
import sys
from pathlib import Path

CHECKS: dict[str, re.Pattern] = {
    "hardcoded-secret": re.compile(
        r'(?:password|secret|token|api_key)\s*=\s*["\'][^"\']{4,}["\']',
        re.IGNORECASE,
    ),
    "sql-format-string": re.compile(
        r'(?:execute|query)\s*\(\s*(?:f["\']|["\'][^"\']*(?:%s|%\(|\{))',
        re.IGNORECASE,
    ),
    "weak-hash": re.compile(
        r'hashlib\.(md5|sha1)\s*\(',
        re.IGNORECASE,
    ),
    "broad-except": re.compile(
        r'except\s*(?:Exception\s*)?:',
    ),
}


def check_file(path: str) -> list[dict]:
    try:
        lines = Path(path).read_text().splitlines()
    except OSError as e:
        print(f"Error: cannot read {path}: {e}", file=sys.stderr)
        sys.exit(1)

    findings = []
    for lineno, line in enumerate(lines, start=1):
        for name, pattern in CHECKS.items():
            if pattern.search(line):
                findings.append({
                    "line": lineno,
                    "check": name,
                    "text": line.strip(),
                })

    return findings


if __name__ == "__main__":
    if "--help" in sys.argv or len(sys.argv) < 2:
        print(__doc__)
        sys.exit(0 if "--help" in sys.argv else 1)

    results = check_file(sys.argv[1])
    print(json.dumps(results, indent=2))
