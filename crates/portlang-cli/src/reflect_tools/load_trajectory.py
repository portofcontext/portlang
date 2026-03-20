#!/usr/bin/env python3
# /// script
# dependencies = ["pydantic"]
# ///

import json
from pathlib import Path
from typing import Optional, List

from pydantic import BaseModel


# ── Output types ──────────────────────────────────────────────────────────────

class VerifierResult(BaseModel):
    name: str
    passed: bool
    stdout: str
    stderr: str


class Step(BaseModel):
    step_number: int
    action_type: str               # "tool_call" | "text_output" | "stop"
    tool: Optional[str]            # tool name if action_type == "tool_call"
    tool_params: Optional[dict]    # tool inputs (action dict minus type/tool keys)
    result_preview: str            # first 800 chars of result
    result_truncated: bool         # True if result was longer than the preview
    result_length: int             # total length of full result string
    rejected: bool
    input_tokens: int
    output_tokens: int
    verifier_results: List[VerifierResult]


class Trajectory(BaseModel):
    field_name: str
    trajectory_id: str
    goal: str
    model_name: str
    outcome: str
    outcome_message: str
    total_cost_usd: float
    total_tokens: int
    duration_seconds: float
    steps: List[Step]


# ── Tool entry point ──────────────────────────────────────────────────────────

def load_trajectory(field_name: str, trajectory_id: str) -> Trajectory:
    """
    Load a specific trajectory and return its full step-by-step data.

    Each step includes the action type, tool name and parameters, a preview of the
    result (truncated if large), token counts, and any verifier results. Use
    result_truncated and result_length to identify steps with oversized tool outputs
    that may be contributing to context bloat.

    Args:
        field_name: Name of the field (subdirectory under /workspace/trajectories/)
        trajectory_id: Trajectory filename without .json extension (e.g. "20260319-160736-c38c9564")

    Returns:
        Trajectory with all steps structured for analysis
    """
    base = Path("/workspace/trajectories")
    traj_dir = base / field_name
    if not traj_dir.exists():
        alt = field_name.replace("-", "_") if "-" in field_name else field_name.replace("_", "-")
        traj_dir = base / alt
    path = traj_dir / f"{trajectory_id}.json"
    data = json.loads(path.read_text())

    outcome = data.get("outcome", {})
    outcome_type = outcome.get("type", "unknown") if isinstance(outcome, dict) else str(outcome)
    outcome_msg = outcome.get("message", "") if isinstance(outcome, dict) else ""

    # Parse duration
    from datetime import datetime, timezone
    def _parse_dt(s: str) -> datetime:
        return datetime.fromisoformat(s.replace("Z", "+00:00"))

    started = _parse_dt(data["started_at"])
    ended = _parse_dt(data["ended_at"])
    duration = (ended - started).total_seconds()

    steps: List[Step] = []
    for raw in data.get("steps", []):
        action = raw.get("action", {})
        action_type = action.get("type", "unknown")

        tool_name: Optional[str] = None
        tool_params: Optional[dict] = None
        if action_type == "tool_call":
            tool_name = action.get("tool")
            tool_params = {k: v for k, v in action.items() if k not in ("type", "tool")}

        result = raw.get("result", "") or ""
        preview_limit = 2000
        result_preview = result[:preview_limit]
        result_truncated = len(result) > preview_limit

        verifier_results = [
            VerifierResult(
                name=vr.get("name", ""),
                passed=vr.get("passed", False),
                stdout=(vr.get("stdout") or "")[:400],
                stderr=(vr.get("stderr") or "")[:400],
            )
            for vr in raw.get("verifier_results", [])
        ]

        steps.append(Step(
            step_number=raw.get("step_number", 0),
            action_type=action_type,
            tool=tool_name,
            tool_params=tool_params,
            result_preview=result_preview,
            result_truncated=result_truncated,
            result_length=len(result),
            rejected=raw.get("rejected", False),
            input_tokens=raw.get("input_tokens", 0),
            output_tokens=raw.get("output_tokens", 0),
            verifier_results=verifier_results,
        ))

    return Trajectory(
        field_name=data.get("field_name", field_name),
        trajectory_id=trajectory_id,
        goal=data.get("goal", ""),
        model_name=data.get("model_name", ""),
        outcome=outcome_type,
        outcome_message=outcome_msg,
        total_cost_usd=round(data.get("total_cost", 0) / 1_000_000, 4),
        total_tokens=data.get("total_tokens", 0),
        duration_seconds=round(duration, 1),
        steps=steps,
    )
