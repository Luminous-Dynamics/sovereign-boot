"""Boundary-level query accounting for surface-extraction benchmarks.

This module is benchmark instrumentation, not production geometry code. Wrap
the actual evaluator passed into a method. The meter counts every submitted
point row even when the method bypasses its own query counter.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass
from threading import Lock
from time import perf_counter_ns
from typing import Any, Callable


class MeterInputError(ValueError):
    """Raised when the wrapper cannot unambiguously count queried points."""


@dataclass(frozen=True)
class MeterSnapshot:
    name: str
    calls_attempted: int
    points_submitted: int
    wall_time_ns: int
    failed_calls: int

    def to_dict(self) -> dict[str, int | str]:
        return asdict(self)


class MeteredCallable:
    """Transparent callable wrapper with thread-safe query accounting.

    Expected input is a batch of 3D query points with shape (N, 3), as used by
    NumPy/Torch based geometry methods. The wrapper preserves return values
    and exceptions. Calls are counted before invoking the wrapped function, so
    failed invocations remain visible as attempted information requests.
    """

    def __init__(self, name: str, function: Callable[..., Any]) -> None:
        if not name:
            raise ValueError("meter name must be non-empty")
        if not callable(function):
            raise TypeError("wrapped function must be callable")
        self._name = name
        self._function = function
        self._lock = Lock()
        self._calls_attempted = 0
        self._points_submitted = 0
        self._wall_time_ns = 0
        self._failed_calls = 0

    @staticmethod
    def _batch_size(points: Any) -> int:
        shape = getattr(points, "shape", None)
        if shape is not None:
            try:
                dimensions = tuple(int(value) for value in shape)
            except (TypeError, ValueError, OverflowError) as exc:
                raise MeterInputError("query points expose an invalid shape") from exc
            if len(dimensions) != 2 or dimensions[1] != 3 or dimensions[0] < 0:
                raise MeterInputError("query points must have shape (N, 3)")
            return dimensions[0]

        try:
            rows = len(points)
        except TypeError as exc:
            raise MeterInputError("query points must be a batch with shape (N, 3)") from exc
        if rows == 0:
            return 0
        try:
            first = points[0]
            if len(first) != 3:
                raise MeterInputError("each query point must have three coordinates")
        except (TypeError, IndexError) as exc:
            raise MeterInputError("query points must have shape (N, 3)") from exc
        return rows

    def __call__(self, points: Any, *args: Any, **kwargs: Any) -> Any:
        count = self._batch_size(points)
        with self._lock:
            self._calls_attempted += 1
            self._points_submitted += count

        start = perf_counter_ns()
        failed = False
        try:
            return self._function(points, *args, **kwargs)
        except BaseException:
            failed = True
            raise
        finally:
            elapsed = max(0, perf_counter_ns() - start)
            with self._lock:
                self._wall_time_ns += elapsed
                if failed:
                    self._failed_calls += 1

    def snapshot(self) -> MeterSnapshot:
        with self._lock:
            return MeterSnapshot(
                name=self._name,
                calls_attempted=self._calls_attempted,
                points_submitted=self._points_submitted,
                wall_time_ns=self._wall_time_ns,
                failed_calls=self._failed_calls,
            )


class QueryMeter:
    """Named collection of independently accounted evaluator callables."""

    def __init__(self) -> None:
        self._meters: dict[str, MeteredCallable] = {}
        self._lock = Lock()

    def wrap(self, name: str, function: Callable[..., Any]) -> MeteredCallable:
        with self._lock:
            if name in self._meters:
                raise ValueError(f"meter already registered: {name}")
            meter = MeteredCallable(name, function)
            self._meters[name] = meter
            return meter

    def snapshot(self) -> dict[str, dict[str, int | str]]:
        with self._lock:
            items = sorted(self._meters.items())
        return {name: meter.snapshot().to_dict() for name, meter in items}

    def totals(self) -> dict[str, int]:
        # Copy the registry under its lock, then snapshot each independently.
        # Calls can continue concurrently without holding the registry lock.
        with self._lock:
            meters = list(self._meters.values())
        snapshots = [meter.snapshot() for meter in meters]
        return {
            "calls_attempted": sum(item.calls_attempted for item in snapshots),
            "points_submitted": sum(item.points_submitted for item in snapshots),
            "wall_time_ns": sum(item.wall_time_ns for item in snapshots),
            "failed_calls": sum(item.failed_calls for item in snapshots),
        }
