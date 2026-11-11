# Ouroboros prerequisite integration base

This branch is an integration-only base for [dual-contouring PR #47](https://github.com/Luminous-Dynamics/sovereign-boot/pull/47).

It starts from the current `feat/ouroboros-identity-core-v1` feature line and stages the exact source snapshots required by the dual-contouring crate:

| Prerequisite | Source branch | Source content SHA |
|---|---|---|
| SDF lab manifest | `research/ouroboros-sdf-lab-v1` | `e976158f2827477ef0a07fe08ec88dfddb953ad3` |
| SDF lab lockfile | `research/ouroboros-sdf-lab-v1` | `55e227875b6f746855179fc6ca06f3d69d331170` |
| SDF lab README | `research/ouroboros-sdf-lab-v1` | `9063704b2501de691984464646c49aec8d0ec58e` |
| SDF source | `research/ouroboros-sdf-lab-v1` | `2e8cda2fb428e53b775a7e9dc8ac8b5990d35465` |
| Mesh audit manifest | `research/ouroboros-mesh-audit-v1` | `b05aadd4683b190d9b75d2894655268170599ef4` |
| Mesh audit lockfile | `research/ouroboros-mesh-audit-v1` | `eeaacb88bc98da6e8c4eaa90223ddb910c337f4a` |
| Mesh audit README | `research/ouroboros-mesh-audit-v1` | `635c0d5b665663d57bd8225ac46d8756ff37eaad` |
| Mesh audit source | `research/ouroboros-mesh-audit-v1` | `ab184ac9066dbefd48707743abd1d089aa0681ef` |

The additional face-skin example and ten-point reference probe fixture from the SDF branch are also staged unchanged.

## Why this branch exists

The SDF and mesh-audit research branches were opened from a common earlier feature-base commit; the feature base subsequently advanced. They are sibling PRs and do not each contain the other's source crate. The dual-contouring crate imports both by local Cargo path, so pointing its PR at either sibling would leave one dependency missing.

This branch provides a coherent, reviewable source tree for the downstream PR without changing either prerequisite PR. It is not a replacement for them and does not claim their code has compiled.

## Retirement path

1. Review and merge the SDF lab and mesh-audit prerequisites into the feature line in their intended order.
2. Verify that the resulting feature branch contains the same crate contents.
3. Retarget PR #47 from this integration base to `feat/ouroboros-identity-core-v1`.
4. Retire this branch once no open PR depends on it.

Do not merge this integration base into the feature line as a separate product change; its contents are the prerequisite snapshots already proposed in PRs #35 and #45.
