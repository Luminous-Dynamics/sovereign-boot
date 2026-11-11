# Query-meter helper

This is a standard-library-only benchmark adapter helper. It is not a production runtime dependency.

Wrap the actual distance, gradient, projection or neural query callable passed to the method. Every call is counted at the boundary, even if upstream method code invokes the callable through a path that bypasses its own statistics.

Example:

```python
from query_meter import QueryMeter
from mcgrids import McGrids

meter = QueryMeter()
metered_sdf = meter.wrap("distance", sdf_callable)
method = McGrids(
    metered_sdf,
    clip_min=-0.5,
    clip_max=0.5,
    initial_resolution=4,
    num_sample_iters=20,
    num_sample_points=128,
    num_mid_iters=300,
    threshold=1e-4,
)
vertices, faces = method.extract_mesh()

# Persist beside the raw trial JSONL record.
outer_observed = meter.snapshot()
method_reported = {
    "distance_value_evaluations": getattr(method, "query_count", None),
    "reported_query_time_seconds": getattr(method, "sdf_query_time", None),
}
```

The external meter is authoritative for invocations routed through the wrapped callable. The method-reported count remains diagnostic: it may be incomplete or defined differently. The meter counts submitted point rows, not unique points; de-duplicate samples only with a separately versioned canonical coordinate key and report both totals.

Use separate names for distinct authorities, for example `distance`, `gradient`, `projection_distance`, `neural_forward`. If an operation computes gradients internally through the wrapped distance evaluator, both operations will be visible only if both callables are wrapped. Do not infer hidden internal operations.

Five authored standard-library tests cover direct calls, exceptions, invalid shapes, separate evaluators/totals and duplicate names. Run with:

```sh
python3 -m unittest discover -s benchmarks/ouroboros-surface-extraction -p 'test_query_meter.py' -v
```

These tests have not been executed in this environment.