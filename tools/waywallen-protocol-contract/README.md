# Waywallen renderer protocol contract model

This dependency-free Rust crate is an executable invariant model for the first Ouroboros Waywallen adapter.

It models protocol v3 / renderer spawn v10 ordering and rejects:
- wrong protocol or spawn versions,
- Ready before Init or missing the producer release-syncobj,
- malformed flattened format-capability arrays,
- directives for unadvertised formats or extents,
- invalid pool generations, slot counts and plane-FD counts,
- frames without an acquire SYNC_FD, outside the current pool, or with non-monotonic frame/release sequences.


The `render_target` module also models the Auto-resolution subprotocol:
- revisioned capability publication, idempotent repeats and same-revision conflict rejection,
- daemon extent selection with fixed-aspect, alignment and hardware ceilings,
- config revision ordering, capability-revision binding, producer format/sync/color/memory mask checks,
- single in-flight request semantics,
- ignored stale results and exact extent + pool-generation verification before accepting status 0,
- LKG preservation on failure and a new revision after scheme rejection.

The test producer intentionally mirrors the current C bridge Vulkan pool's default capability subset: timeline sync, sRGB/limited/premultiplied color, and host-visible memory. It does not claim other color encodings are compatible; see [compatibility issue #22](https://github.com/Luminous-Dynamics/sovereign-boot/issues/22).

It does **not** open sockets, pass SCM_RIGHTS descriptors, allocate DMA-BUFs, create Vulkan objects, render actual targets, or signal sync objects. It is a pure state-transition oracle for tests and future adapter code. A production adapter must map the actual bridge events and descriptor ownership onto this model and still qualify real fence/buffer lifecycles on target GPUs.

Upstream compatibility is pinned in `integrations/waywallen/README.md`. Changing the protocol or spawn versions requires updating this crate's constants and conformance fixtures deliberately.
