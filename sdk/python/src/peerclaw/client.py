from __future__ import annotations

import os
from typing import Any, AsyncIterator, Iterator

import httpx

Json = dict[str, Any] | list[Any] | Any


class PeerclawClient:
    """Sync + async client for a node's `--web` HTTP API."""

    def __init__(self, base_url: str | None = None, *, timeout: float = 120.0) -> None:
        self.base_url = (base_url or os.environ.get("PEERCLAW_BASE_URL") or "http://127.0.0.1:8080").rstrip(
            "/"
        )
        self._timeout = timeout

    def _url(self, path: str) -> str:
        return f"{self.base_url}{path if path.startswith('/') else '/' + path}"

    # --- Crews ---
    def validate_crew(self, spec: Json) -> Json:
        with httpx.Client(timeout=self._timeout) as c:
            r = c.post(self._url("/api/crews/validate"), json=spec)
            r.raise_for_status()
            return r.json()

    def kickoff_crew(
        self,
        spec: Json,
        inputs: Json | None = None,
        *,
        distributed: bool = False,
        pod_id: str | None = None,
        campaign_id: str | None = None,
    ) -> Json:
        body: dict[str, Json] = {"spec": spec, "inputs": inputs or {}}
        body["distributed"] = distributed
        if pod_id is not None:
            body["pod_id"] = pod_id
        if campaign_id is not None:
            body["campaign_id"] = campaign_id
        with httpx.Client(timeout=self._timeout) as c:
            r = c.post(self._url("/api/crews/kickoff"), json=body)
            r.raise_for_status()
            return r.json()

    def get_crew_run(self, run_id: str) -> Json:
        with httpx.Client(timeout=self._timeout) as c:
            r = c.get(self._url(f"/api/crews/runs/{run_id}"))
            r.raise_for_status()
            return r.json()

    def list_crew_runs(self) -> list[Json]:
        with httpx.Client(timeout=self._timeout) as c:
            r = c.get(self._url("/api/crews/runs"))
            r.raise_for_status()
            return r.json()

    def crew_run_events(self, run_id: str) -> Iterator[str]:
        """SSE lines (raw) from `/api/crews/runs/:id/stream`."""
        with httpx.Client(timeout=None) as c:
            with c.stream("GET", self._url(f"/api/crews/runs/{run_id}/stream")) as r:
                r.raise_for_status()
                for line in r.iter_lines():
                    if line:
                        yield line

    # --- Flows ---
    def validate_flow(self, spec: Json) -> Json:
        with httpx.Client(timeout=self._timeout) as c:
            r = c.post(self._url("/api/flows/validate"), json=spec)
            r.raise_for_status()
            return r.json()

    def kickoff_flow(self, spec: Json, inputs: Json | None = None) -> Json:
        body = {"spec": spec, "inputs": inputs or {}}
        with httpx.Client(timeout=self._timeout) as c:
            r = c.post(self._url("/api/flows/kickoff"), json=body)
            r.raise_for_status()
            return r.json()

    def get_flow_run(self, run_id: str) -> Json:
        with httpx.Client(timeout=self._timeout) as c:
            r = c.get(self._url(f"/api/flows/runs/{run_id}"))
            r.raise_for_status()
            return r.json()

    # --- Async variants ---
    async def akickoff_crew(
        self,
        spec: Json,
        inputs: Json | None = None,
        *,
        distributed: bool = False,
        pod_id: str | None = None,
        campaign_id: str | None = None,
    ) -> Json:
        body: dict[str, Json] = {"spec": spec, "inputs": inputs or {}, "distributed": distributed}
        if pod_id is not None:
            body["pod_id"] = pod_id
        if campaign_id is not None:
            body["campaign_id"] = campaign_id
        async with httpx.AsyncClient(timeout=self._timeout) as c:
            r = await c.post(self._url("/api/crews/kickoff"), json=body)
            r.raise_for_status()
            return r.json()

    async def astream_crew(self, run_id: str) -> AsyncIterator[str]:
        async with httpx.AsyncClient(timeout=None) as c:
            async with c.stream("GET", self._url(f"/api/crews/runs/{run_id}/stream")) as r:
                r.raise_for_status()
                async for line in r.aiter_lines():
                    if line:
                        yield line
