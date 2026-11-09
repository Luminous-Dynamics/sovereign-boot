# Waywallen renderer protocol contract model

This dependency-free Rust crate is an executable invariant model for the first Ouroboros Waywallen adapter.

It models protocol v3 / renderer spawn v10 ordering and rejects:
- wrong protocol or spawn versions,
- Ready before Init or missing the producer release-syncobj,
- malformed flattened format-capability arrays,
- directives for unadvertised formats or extents,
- invalid pool generations, slot counts and plane-FD counts,
- frames without an acquire SYNC_FD, outside the current pool, or with non-monotonic frame/release sequences.

It does **not** open sockets, pass SCM_RIGHTS descriptors, allocate DMA-BUFs, create Vulkan objects, or signal sync objects. It is a pure state-transition oracle for tests and future adapter code. A production adapter must map the actual bridge events and descriptor ownership onto this model and still qualify real fence/buffer lifecycles on target GPUs.

Upstream compatibility is pinned in `integrations/waywallen/README.md`. Changing the protocol or spawn versions requires updating this crate's constants and conformance fixtures deliberately.
