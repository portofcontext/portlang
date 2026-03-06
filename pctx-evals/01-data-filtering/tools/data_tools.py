"""Data tools for Code Mode evaluation"""

import json
from typing import List, Dict, Any


def load_json(filepath: str) -> List[Dict[str, Any]]:
    """Load a JSON file and return its contents.

    Args:
        filepath: Path to the JSON file

    Returns:
        List of dictionaries from the JSON file
    """
    with open(filepath, 'r') as f:
        return json.load(f)


def save_json(data: Any, filepath: str) -> Dict[str, str]:
    """Save data to a JSON file.

    Args:
        data: Data to save (must be JSON-serializable)
        filepath: Path where to save the file

    Returns:
        Success message
    """
    with open(filepath, 'w') as f:
        json.dump(data, f, indent=2)
    return {"status": "success", "path": filepath}


def filter_by_age(users: List[Dict[str, Any]], min_age: int) -> List[Dict[str, Any]]:
    """Filter users by minimum age.

    Args:
        users: List of user dictionaries
        min_age: Minimum age (inclusive)

    Returns:
        Filtered list of users
    """
    return [u for u in users if u.get('age', 0) >= min_age]


def sort_by_field(items: List[Dict[str, Any]], field: str) -> List[Dict[str, Any]]:
    """Sort a list of dictionaries by a specific field.

    Args:
        items: List of dictionaries to sort
        field: Field name to sort by

    Returns:
        Sorted list
    """
    return sorted(items, key=lambda x: x.get(field, ''))
