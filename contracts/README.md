# Scene Pack v1 contract snapshot

This directory vendors the declarative Scene Pack v1 schema and the independent
Draft 2020-12 validation harness from
[Luminous-Dynamics/luminous-platform](https://github.com/Luminous-Dynamics/luminous-platform)
at immutable upstream commit
[`f54ecf4b3ced00ca6c8964e83f63e6d285f49e75`](https://github.com/Luminous-Dynamics/luminous-platform/commit/f54ecf4b3ced00ca6c8964e83f63e6d285f49e75).

The schema and negative-fixture corpus are kept as source snapshots so this
repository's CI does not silently validate against a moving `main` branch.
`contracts/UPSTREAM_SNAPSHOT.json` records the upstream commit and original Git
blob IDs. `tools/ambient_validation/verify_vendor_snapshot.py` recomputes each
Git blob ID from the local bytes and fails on drift. This is an identity/drift
check tied to the pinned Git commit, not a claim that Git SHA-1 provides
collision-resistant cryptographic authentication.
The positive manifest is the existing byte-pinned
`tests/fixtures/first-germination.scene.json`.

The Python validator checks standard JSON Schema validity and cross-field /
package invariants. The Rust parser has a separate test that applies the shared
negative fixtures (except the valid-but-adapter-unsupported GPU capability case)
to ensure its acceptance boundary stays aligned with this contract. Runtime
capability negotiation remains the responsibility of each host adapter.

To validate locally:

    python -m pip install --disable-pip-version-check -r tools/ambient_validation/requirements.txt
    python tools/ambient_validation/validate_scene_pack.py tests/fixtures/first-germination.scene.json
    python -m unittest discover -s tools/ambient_validation -p 'test_*.py' -v
