#!/usr/bin/env python3
# /// script
# dependencies = [requests]
# ///


def execute(expression: str) -> dict:
    """
    Simple calculator tool that evaluates math expressions.

    Args:
        expression: The mathematical expression to evaluate (e.g., '42 * 137')

    Returns:
        Dict with 'result' and 'status'
    """

    if not expression:
        return {"status": "error", "error": "Missing 'expression' field"}

    try:
        # Safe evaluation of math expressions
        result = eval(expression, {"__builtins__": {}}, {})
        return {"status": "success", "result": result, "expression": expression}
    except Exception as e:
        return {"status": "error", "error": str(e), "expression": expression}
