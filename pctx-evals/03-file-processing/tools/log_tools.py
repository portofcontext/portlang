"""Log processing tools for Code Mode"""

import json
import re
from typing import List, Dict, Any


def read_log(filepath: str) -> List[str]:
    """Read a log file and return lines.

    Args:
        filepath: Path to log file

    Returns:
        List of log lines
    """
    with open(filepath, 'r') as f:
        return [line.strip() for line in f if line.strip()]


def filter_by_pattern(lines: List[str], pattern: str) -> List[str]:
    """Filter lines matching a regex pattern.

    Args:
        lines: List of strings to filter
        pattern: Regex pattern to match

    Returns:
        Filtered lines
    """
    regex = re.compile(pattern)
    return [line for line in lines if regex.search(line)]


def count_by_keyword(lines: List[str], keywords: List[str]) -> Dict[str, int]:
    """Count occurrences of keywords in lines.

    Args:
        lines: List of strings to search
        keywords: Keywords to count

    Returns:
        Dictionary mapping keywords to counts
    """
    counts = {kw: 0 for kw in keywords}
    for line in lines:
        for kw in keywords:
            if kw in line:
                counts[kw] += 1
    return counts


def save_json(data: Any, filepath: str) -> Dict[str, str]:
    """Save data as JSON.

    Args:
        data: Data to save
        filepath: Output file path

    Returns:
        Success message
    """
    with open(filepath, 'w') as f:
        json.dump(data, f, indent=2)
    return {"status": "success"}
