import json
import os


def load_config(path: str) -> dict:
    """Load JSON config from a file."""
    with open(path) as f:
        return json.load(f)


def sanitize_filename(name: str) -> str:
    """Remove unsafe characters from a filename."""
    return "".join(c for c in name if c.isalnum() or c in "._-")


def read_env(key: str, default: str = "") -> str:
    """Read an environment variable with a fallback default."""
    return os.environ.get(key, default)
