# Model culling

How Just Cause 3 decides whether a model instance's geometry reaches a render pass. Written as the game
is; the VR mod's widening of these frustums is a separate concern (see the culling hooks in `payload`).

There are **two** independent visibility gates for a model, at different stages:

## 1. Instance-level BFBC cull (build phase)

`CModelCollection::DoCulling` (release `0x140_4E7_950`, **mislabeled `CAOVolumeManager::AddToRenderThread`
in the IDB**) runs the broad-phase (BFBC) scene cull over model instances and passes each survivor to
`ShowUpdate` → `CModelInstance::AddToRender`. This level tests against the occluder manager's
`m_CullingCamera` (`this+0x8`) — the same camera the mod's scene-cull widen already covers.

## 2. Per-render-block frustum cull (draw-list build)

`CModelInstance::AddToRender` (release `0x140_319_BD0`, **mislabeled `CModelInstance::AddToPass` in the
IDB**) builds the per-pass draw lists for one instance's render blocks. It constructs an
`SAddtoRenderContext` whose **`m_Camera` is the camera manager's active camera**
(`CCameraManager::Instance` `0x142_ED0_E20`, `m_ActiveCamera` at `+0x5C0`), then dispatches to
`NModelSystem::ForEachRb<UseBBoxRBView, CoarsePreZ>` selected by two globals:

- `g_UseBoundingBoxModelRBView` = `0x142_D4B_7B5`
- `g_EnableCoarsePreZModels` = `0x142_D4B_7B4`

Both are statically initialized to `1`, so release always runs the `<1,1>` instantiation
(`0x140_316_E50`; the `<0,1>` `0x140_317_650` and `<1,0>` `0x140_317_E50` variants also test). Each of
these calls `CCamera::IsBoxVisible(context.m_Camera, blockWorldAABB)` **per render block** and, on
failure, skips that block for *every* pass (`if (!IsBoxVisible(ctx.m_Camera, box)) continue`). Only the
`<0,0>` variant (`0x140_318_2C0`) has no test (and loses coarse pre-Z). Thresholds live at `0x142_D4B_7B8`
(0.15) and `0x142_D4B_7BC` (0.01).

`CCamera::IsBoxVisible` (`0x140_09C_210`) tests a world-space AABB against the camera's precomputed
frustum planes (`m_FrustumPlane` at `+0x414` plus AAB acceleration data), rebuilt each frame by
`Camera::UpdateFrustum` (`0x140_0B2_FC0`) from the centre `m_ViewProjection` (`+0x314`).

**Consequence:** a model's render blocks are re-culled against the **narrow active-camera frustum** after
the broad BFBC pass, using a frustum distinct from the occluder-cull and terrain cameras. A large
multi-block building therefore disappears at the edge of a wider (e.g. combined-eye) view even though the
BFBC pass admitted the instance. Sprites (`CRenderBlockParticle`) and gathered lights use other paths that
don't route through `ForEachRb`.

## The same active-camera frustum elsewhere

`CCamera::IsBoxVisible` against `m_ActiveCamera` also gates:

- `CModelInstanceManager::StartFade` — an instance "not in view" hides **instantly** instead of fading
  (edge pop rather than a dissolve).
- `CRoadMeshManager::UpdateAddToRenderer` (`0x140_533_300`).
- `CLightManager::UpdateRender` / `AddFarLightsToPass` far-light cells (`0x140_0CF_EE0` / `0x140_0B9_C10`).

## Related: spawn visibility

`CSpawnSystem::Update` (`0x140_EFE_E90`; spawn singleton `0x142_F18_998`, `m_SpawnBFBC` `+0x168`,
`m_SpawnBFBCParams` `+0x170`, `m_SpawnBFBCState` `+0x178`, `m_CenterPos` `+0x194`, `m_ViewPos` `+0x1A0`,
`m_ViewDir` `+0x1AC`) calls `GetBFBCFrustumParamsForCameraAndTime` at its tail, passing the camera
manager's `m_ActiveCamera` (not the occluder manager's `m_CullingCamera`). The resulting BFBC params are
used in `CSpawnFactoryImpl::CheckInternal` (`0x140_F09_430`) as a "don't (de)spawn while visible" gate.
Its budgets are characters and vehicles, so it governs NPC/vehicle (de)spawns near the view edge, not
buildings. Because the frustum is built from the active camera's narrow centre projection, the spawn
gate does not account for the wider VR eye frusta — the mod widens it via the `get_bfbc_frustum_params`
detour (`widen_spawn_cull`).

## Related: character occlusion

`CGameWorldObjectManager::StartCommitAddRemove` (`0x140_4B_B8B0`; singleton `0x142_F17_128`) copies the
camera manager's `m_ActiveCamera` into `m_OcclusionCamera` (`+0x9F8`) each frame, then dispatches
`SCharacterOcclusionHandler::ProcessCharacterOcclusion` (`0x140_4B_B7E0`) via a CPU fragment. That
function calls `GetBFBCFrustumParamsForCameraAndTime` with the active camera, then runs `BFBCProcess`
with `E_BFBC_PROCESSFUNCTION_FRUSTUM_CULL` to cull character occlusion — hiding characters the frustum
rejects. As with the spawn system, the frustum is built from the narrow centre camera, so characters
visible to an offset VR eye can be occlusion-culled. The mod widens this through the same
`get_bfbc_frustum_params` detour.

## IDB symbol caveats

Several `CModelInstanceManager`-area names in the release IDB are misassigned from the debug-symbol port:
`AddToPass` at `0x140_319_BD0` is really `AddToRender`; `CAOVolumeManager::AddToRenderThread` at
`0x140_4E7_950` is really `CModelCollection::DoCulling`; `DumpModelInstanceHistogram` at `0x140_4F3_010`
is a distance-streaming task. Rename before further work.

Findings from a decompilation sweep of the release binary; not yet runtime-verified. Quickest confirmation:
zero `0x142_D4B_7B4` / `0x142_D4B_7B5` live and check whether building edge-pop disappears.
