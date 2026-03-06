"""Pipeline transformation tools for Code Mode"""

import csv
import json
from typing import List, Dict, Any
from collections import defaultdict


def load_csv(filepath: str) -> List[Dict[str, Any]]:
    """Load CSV file and return list of dictionaries.

    Args:
        filepath: Path to CSV file

    Returns:
        List of dictionaries with typed numeric fields
    """
    records = []
    with open(filepath, 'r') as f:
        reader = csv.DictReader(f)
        for row in reader:
            # Convert numeric fields
            if 'price' in row:
                row['price'] = float(row['price'])
            if 'quantity' in row:
                row['quantity'] = int(row['quantity'])
            records.append(row)
    return records


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
