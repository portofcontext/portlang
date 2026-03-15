#!/usr/bin/env python3
# /// script
# dependencies = ["pydantic"]
# ///

from pydantic import BaseModel
from typing import Optional


class CalculatorResult(BaseModel):
    status: str
    result: Optional[float] = None
    expression: Optional[str] = None
    error: Optional[str] = None


def execute(expression: str) -> CalculatorResult:
    """
    Simple calculator tool that evaluates math expressions.

    Args:
        expression: The mathematical expression to evaluate (e.g., '42 * 137')

    Returns:
        CalculatorResult with status, result, expression, and optional error
    """

    if not expression:
        return CalculatorResult(status="error", error="Missing 'expression' field")

    try:
        # Safe evaluation of math expressions
        result = eval(expression, {"__builtins__": {}}, {})
        return CalculatorResult(status="success", result=float(result), expression=expression)
    except Exception as e:
        return CalculatorResult(status="error", error=str(e), expression=expression)
