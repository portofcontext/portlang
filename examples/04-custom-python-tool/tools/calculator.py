#!/usr/bin/env python3
# /// script
# dependencies = []
# ///

def execute(input):
    """
    Simple calculator tool that evaluates math expressions.

    Args:
        input: Dict with 'expression' key containing the math expression

    Returns:
        Dict with 'result' and 'status'
    """
    expression = input.get("expression")

    if not expression:
        return {
            "status": "error",
            "error": "Missing 'expression' field"
        }

    try:
        # Safe evaluation of math expressions
        result = eval(expression, {"__builtins__": {}}, {})
        return {
            "status": "success",
            "result": result,
            "expression": expression
        }
    except Exception as e:
        return {
            "status": "error",
            "error": str(e),
            "expression": expression
        }
