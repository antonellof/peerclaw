"""Load a simple YAML project layout into a CrewSpec-shaped dict."""

from __future__ import annotations

from pathlib import Path
from typing import Any

import yaml

Json = dict[str, Any]


def load_crew_from_yaml_dir(directory: str | Path) -> Json:
    """Merge `crew.yaml` or `agents.yaml` + `tasks.yaml` into one spec object."""
    d = Path(directory)
    crew_file = d / "crew.yaml"
    if crew_file.is_file():
        return yaml.safe_load(crew_file.read_text()) or {}

    agents_f = d / "agents.yaml"
    tasks_f = d / "tasks.yaml"
    spec: Json = {"name": d.name, "agents": [], "tasks": [], "process": "sequential"}
    if agents_f.is_file():
        data = yaml.safe_load(agents_f.read_text()) or {}
        spec["agents"] = data.get("agents", data if isinstance(data, list) else [])
    if tasks_f.is_file():
        data = yaml.safe_load(tasks_f.read_text()) or {}
        spec["tasks"] = data.get("tasks", data if isinstance(data, list) else [])
    return spec
