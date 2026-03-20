#!/usr/bin/env python3
# /// script
# dependencies = ["pydantic"]
# ///

import json
from pathlib import Path
from typing import List

from pydantic import BaseModel


class TrajectoryMeta(BaseModel):
    trajectory_id: str
    timestamp: str
    outcome: str
    total_steps: int
    total_tokens: int
    cost_usd: float
    passed_verifiers: int
    failed_verifiers: int


class ListResult(BaseModel):
    field_name: str
    found: int
    trajectories: List[TrajectoryMeta]


def list_trajectories(field_name: str, limit: int = 10) -> ListResult:
    """
    List the most recent trajectories for a given field.

    Returns summary metadata for each trajectory sorted by recency (newest first).
    Use this to get an overview before loading individual trajectories in detail.

    Args:
        field_name: Name of the field (must match a subdirectory under /workspace/trajectories/)
        limit: Maximum number of trajectories to return (default 10)

    Returns:
        ListResult with per-trajectory metadata including outcome, token usage, and cost
    """
    # Try the name as-is, then with dashes↔underscores swapped (field dirs use underscores
    # even when the field file lives in a dash-named directory, e.g. "10-create-coupon" →
    # trajectories are stored under "10_create_coupon").
    base = Path("/workspace/trajectories")
    traj_dir = base / field_name
    if not traj_dir.exists():
        alt = field_name.replace("-", "_") if "-" in field_name else field_name.replace("_", "-")
        traj_dir = base / alt
    if not traj_dir.exists():
        return ListResult(field_name=field_name, found=0, trajectories=[])

    files = sorted(traj_dir.glob("*.json"), key=lambda p: p.name, reverse=True)[:limit]

    results: list[TrajectoryMeta] = []
    for f in files:
        try:
            data = json.loads(f.read_text())
        except Exception:
            continue

        outcome = data.get("outcome", {})
        outcome_type = outcome.get("type", "unknown") if isinstance(outcome, dict) else str(outcome)

        # Count verifier results on the stop step
        passed = failed = 0
        for step in data.get("steps", []):
            for vr in step.get("verifier_results", []):
                if vr.get("passed"):
                    passed += 1
                else:
                    failed += 1

        results.append(TrajectoryMeta(
            trajectory_id=f.stem,
            timestamp=data.get("started_at", f.stem[:15]),
            outcome=outcome_type,
            total_steps=len(data.get("steps", [])),
            total_tokens=data.get("total_tokens", 0),
            cost_usd=round(data.get("total_cost", 0) / 1_000_000, 4),
            passed_verifiers=passed,
            failed_verifiers=failed,
        ))

    return ListResult(field_name=field_name, found=len(results), trajectories=results)
