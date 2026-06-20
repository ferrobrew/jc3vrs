# Skeleton, pose, and bone override

JC3 characters are posed via a Havok `hkaPose`. This covers how the pose is stored and updated, how to read and override individual bones, and where in the frame to do it so the change reaches both the camera and the rendered mesh. It's the foundation for driving the player's head from the HMD and, later, full-body IK. Addresses are for the **20206564** build (the one the mod targets — `jc3gi/build.rs`; the `1227440` def dir is a different, unused build, so don't mix its `0x1436xxxxx` addresses in).

## Bone store: hkaPose, model-space

The animation controller (`CAnimationControl`, the mod's `AnimationController`) holds a `CPoseProducer` at `controller+184`, wrapping two `hkaPose` objects (current + a secondary/ragdoll pose). Each `hkaPose` keeps, at fixed offsets: a local-space (parent-relative) transform buffer at `+0x08` (48 bytes/bone, `hkQsTransform` = translation vec4, rotation quat, scale vec4), a **model-space (character-root-relative)** buffer at `+0x18`, a per-bone dirty-flag array at `+0x28`, sync flags at `+0x38` (model) / `+0x39` (local), and the parent-index table at `*(pose)+0x18` (int16 per bone, `0xFFFF` = root). Those raw offsets were inferred from the decompiled `accessBoneModelSpace` / `syncModelSpace` and aren't needed if you use the Joint API below.

Bones are **model-space, not parent-local**: a bone's world transform is `characterWorld · boneModelSpace`. This matches how the mod already reads the eye bones (`character_matrix * eye_matrix` in `calculate_head_position`).

## Read / write API (Joint)

The controller exposes a model-space `Joint` API, already in the bindings:
- `AnimationController::GetBoneMatrix(idx, *Matrix4)` (`0x14043FE70`) — builds a `Matrix4` from the model-space joint. `Character::GetSafeBoneMatrix(safeIdx, …)` resolves through a hash→index table to this.
- `AnimationController::GetJoint(idx, *Joint)` (`0x14043FF90`) — reads the model-space `Joint`.
- `AnimationController::SetJoint(idx, *Joint)` (`0x14043FFF0`) — writes a model-space `Joint` and **propagates to all descendants** (recomputes their model space); marks the bone local-dirty.

`Joint` is `{ AlignedVector3 Translation; AlignedQuat Orientation; AlignedVector3 Scale }`, model-space; the quaternion is Havok order (x, y, z, w).

So overriding a bone is a `SetJoint`, not a buffer patch. To place the head at a desired world transform relative to the body: `desiredHeadModel = inverse(characterWorldT1) · desiredHeadWorld`, then `SetJoint(HEAD, desiredHeadModel)`. `SetJoint` re-derives descendants but not ancestors — fine for a head/eyes override (the neck above is left as the animation set it).

## Hierarchy and root

The root for "head relative to body" is the character world matrix `m_WorldMatrixT0/T1` (`Character+0x27F0`). The chain is SPINE → SPINE1 → SPINE2 → STERNUM → NECK → HEAD, with `fLeftEye`/`fRightEye` as facial children of HEAD (looked up by name hash via `GetBoneIndex`). Because everything is model-space, you don't walk the chain to place a bone — one `inverse(characterWorld)` multiply suffices.

## Frame order, and where to override

The pose is finalized in the SIM phase and consumed (camera, then skinning) in the RENDER phase:

    SIM:    UpdatePassFinalizePose_Parallel (0x1407F9B10)
              -> HumanIK pass, SyncPoses, CalculateModelSpacePose   => model-space pose finalized
              -> UpdatePropEffects (0x1407C2380)                    <- last call; the mod already hooks this
    RENDER: CGameCameraManager::UpdateRender (0x1407F4560)
              -> CCameraTree::UpdateRenderContexts (0x140465AD0)    <- mod camera hook (reads bones)
              -> CCamera::UpdateRender (0x1400C3020)
            ... KickSkinningJob (0x141E68F90) -> render submit

Override at the **existing `UpdatePropEffects` hook** (`0x1407C2380`), in its post-call block: it's the last thing in `UpdatePassFinalizePose_Parallel`, after the model-space pose is built, in the SIM phase — so a `SetJoint` there is consumed by both the camera (which reads `GetSafeBoneMatrix(HEAD)` and the eye bones from the same model-space buffer in the RENDER phase) and the skinning job. The mod already does `SetJoint` here (the head-hide/scale hack), which proves the override reaches the render. No camera-code change is needed: the camera is already placed at the eye-bone average (`calculate_head_position`), so once HEAD is player-driven the camera follows the player's eyes for free.

## Near-term: head and eyes match the player

In the local-player branch of the prop-effects hook: build `desiredHeadModel` from the player's real head pose (the HMD, or the mouse stand-in) relative to the body, `GetJoint(HEAD)` to keep the existing scale, overwrite Translation + Orientation, `SetJoint(HEAD)`. Child propagation carries the eye bones along. Optionally drive NECK too for a smoother blend, and the eye bones directly for gaze. Verify at runtime that `SetJoint(HEAD)` actually moves `fLeftEye`/`fRightEye` — the head-hide hack scales head and jaw/lip bones separately, which weakly hints that some facial bones may be siblings rather than strict children.

Build the head orientation from a look direction. The engine matrices are D3D-style — row-major, row-vector (`clip = p · M`, `VP = View · Proj`) — so a transform's basis vectors are its *rows*: `data[0..2]` = right (+X), `data[4..6]` = up (+Y), `data[8..10]` = +Z basis (forward = −`data[8..10]`), `data[12..14]` = translation; right-handed, Y-up (rendering §2.6). glam is column-vector, and `from_cols_array` / `to_cols_array` bridge the two by transposing — which is why the mod's glam matrix math works without an explicit transpose. So build the rotation in glam with `right`, `trueUp`, `−fwd`, `pos` as the *columns* (`right = normalize(cross(up, fwd))`, `trueUp = cross(fwd, right)`) and `to_cols_array` it into the engine matrix.

## Deferred: full-body IK

To make the body follow (crouch, lean, bend down to smell the roses), feed the engine's own HumanIK pass effector targets — `AddEffectorTargetPosition` (`0x140408860`), driven inside `UpdatePassFinalizePose_Parallel` **before** `CalculateModelSpacePose`, so HumanIK-target writes must happen *earlier* than the prop-effects seam — or `SetJoint` the whole upper chain post-finalize. The HumanIK route blends with the existing animation and is the cleaner long-term path; it's out of near-term scope.
