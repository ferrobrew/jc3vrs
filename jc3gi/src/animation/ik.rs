#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(4))]
/// A chain entry in the effector-id hash table (`THashTable<int, unsigned int, 1, unsigned short>`
/// bucket-chain element): skeleton bone index to effector id.
pub struct EffectorIdChain {
    pub m_Key: i32,
    pub m_Next: u16,
    _field_6: [u8; 2],
    pub m_Value: u32,
}
fn _EffectorIdChain_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xC], EffectorIdChain>([0u8; 0xC]);
    }
    unreachable!()
}
impl EffectorIdChain {}
impl std::convert::AsRef<EffectorIdChain> for EffectorIdChain {
    fn as_ref(&self) -> &EffectorIdChain {
        self
    }
}
impl std::convert::AsMut<EffectorIdChain> for EffectorIdChain {
    fn as_mut(&mut self) -> &mut EffectorIdChain {
        self
    }
}
#[repr(C, align(8))]
/// The skeleton-bone-index to effector-id map (an open-chained hash table). Built at
/// [`Init`](HumanIK::Init) time: for every used HumanIK node, the node's skeleton bone index keys
/// the node's effector-id mapping (0..44). [`GetEffectorIdFromBoneIndex`](HumanIK::GetEffectorIdFromBoneIndex)
/// queries it.
pub struct EffectorIdTable {
    /// The bucket array: `m_HashTableLength` `u16` slots, each `0xFFFF` (empty) or an index into
    /// `m_ChainPool`.
    pub m_HashTable: *mut u16,
    pub m_ChainPool: *mut crate::animation::ik::EffectorIdChain,
    pub m_HashTableLength: u16,
    _field_12: [u8; 14],
}
fn _EffectorIdTable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x20], EffectorIdTable>([0u8; 0x20]);
    }
    unreachable!()
}
impl EffectorIdTable {}
impl std::convert::AsRef<EffectorIdTable> for EffectorIdTable {
    fn as_ref(&self) -> &EffectorIdTable {
        self
    }
}
impl std::convert::AsMut<EffectorIdTable> for EffectorIdTable {
    fn as_mut(&mut self) -> &mut EffectorIdTable {
        self
    }
}
#[repr(C, align(4))]
/// A queued positional effector target: place the effector at `effector_position` (character-model
/// space) with the given `solve_step`. Interpolation, when enabled, eases the effector's reach
/// weight toward the target at `effector_interpolation_rate`; blend-out eases it back down at
/// `effector_blend_out_rate` when the target is no longer supplied.
pub struct EffectorTargetPosition {
    pub effector: i32,
    /// The desired effector position, in character-model space (the root-relative space of the
    /// character's pose, i.e. `inverse(world) * world_position`).
    pub effector_position: crate::types::math::Vector3,
    pub effector_interpolation: bool,
    pub effector_blend_out: bool,
    _field_12: [u8; 2],
    pub effector_interpolation_rate: f32,
    pub effector_blend_out_rate: f32,
    pub is_valid: bool,
    _field_1d: [u8; 3],
    pub solve_step: crate::animation::ik::SolveStep,
}
fn _EffectorTargetPosition_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x24], EffectorTargetPosition>([0u8; 0x24]);
    }
    unreachable!()
}
impl EffectorTargetPosition {}
impl std::convert::AsRef<EffectorTargetPosition> for EffectorTargetPosition {
    fn as_ref(&self) -> &EffectorTargetPosition {
        self
    }
}
impl std::convert::AsMut<EffectorTargetPosition> for EffectorTargetPosition {
    fn as_mut(&mut self) -> &mut EffectorTargetPosition {
        self
    }
}
#[repr(C, align(4))]
/// A queued rotational effector target: rotate the effector by `effector_rotation_angle` radians
/// about `effector_rotation_axis`, with the given `solve_step`. Interpolation and blend-out
/// behave as for [`EffectorTargetPosition`].
pub struct EffectorTargetRotation {
    pub effector: i32,
    pub effector_rotation_axis: crate::types::math::Vector3,
    pub effector_rotation_axis_type: crate::animation::ik::RotationAxis,
    pub effector_rotation_angle: f32,
    pub effector_interpolation: bool,
    pub effector_blend_out: bool,
    _field_1a: [u8; 2],
    pub effector_interpolation_rate: f32,
    pub effector_blend_out_rate: f32,
    pub is_valid: bool,
    _field_25: [u8; 3],
    pub solve_step: crate::animation::ik::SolveStep,
}
fn _EffectorTargetRotation_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x2C], EffectorTargetRotation>([0u8; 0x2C]);
    }
    unreachable!()
}
impl EffectorTargetRotation {}
impl std::convert::AsRef<EffectorTargetRotation> for EffectorTargetRotation {
    fn as_ref(&self) -> &EffectorTargetRotation {
        self
    }
}
impl std::convert::AsMut<EffectorTargetRotation> for EffectorTargetRotation {
    fn as_mut(&mut self) -> &mut EffectorTargetRotation {
        self
    }
}
#[repr(C, align(8))]
/// The engine's wrapper over an Autodesk HumanIK character solver. It owns the HIK character and
/// state objects, holds the queued effector targets for each [`Pass`], and holds the per-effector
/// control-value arrays (pull / resistance / translation-reach / rotation-reach), indexed by
/// effector id (`0..44`).
///
/// # Per-frame lifecycle
///
/// A character drives its IK inside `CCharacter::UpdatePassFinalizePose_Parallel`, after the
/// animation graph has finalized the local pose and before `CalculateModelSpacePose`. For each
/// [`Pass`], the sequence is:
///
/// 1. Targets are queued (via [`AddEffectorTargetPosition`](HumanIK::AddEffectorTargetPosition) /
///    [`AddEffectorTargetRotation`](HumanIK::AddEffectorTargetRotation)) — the aim/reach IK does
///    this during animation-graph evaluation for `MAIN`; the hand pass queues its own for
///    `SECONDARY`.
/// 2. [`HasTargets`](HumanIK::HasTargets) gates the solve. If there are none, the whole solve for
///    that pass is skipped.
/// 3. [`SetActiveIKPass`](HumanIK::SetActiveIKPass), then
///    [`DriveAllCurrentEffectorControlValues`](HumanIK::DriveAllCurrentEffectorControlValues), then
///    the solve proper: [`CharacterToIKState`](HumanIK::CharacterToIKState) →
///    [`UpdateEffectorsFromTargets`](HumanIK::UpdateEffectorsFromTargets) →
///    [`Solve`](HumanIK::Solve) → [`IKToCharacterState`](HumanIK::IKToCharacterState) (writing the
///    solved pose back into the character's `hkaPose`).
/// 4. [`ResetSolveStep`](HumanIK::ResetSolveStep), then [`ClearTargets`](HumanIK::ClearTargets)
///    drops consumed targets (and returns whether the pass is now empty).
///
/// A target queued before the `HasTargets` gate for a pass is therefore consumed in the same frame.
pub struct HumanIK {
    /// The Autodesk HIK character (`HIKCharacter*`); opaque solver handle.
    pub m_HIKCharacter: u64,
    /// The HIK character state (`HIKCharacterState*`): the current node transforms the solver reads
    /// and writes.
    pub m_HIKCharacterState: u64,
    /// The HIK effector-set state (`HIKEffectorSetState*`): per-effector target transforms and
    /// activation weights.
    pub m_HIKEffectorSetState: u64,
    /// The HIK property-set state (`HIKPropertySetState*`): solver tuning properties.
    pub m_HIKPropertySetState: u64,
    /// One [`PassInfo`] per [`Pass`] (`MAIN`, `SECONDARY`).
    pub m_PassInfo: [crate::animation::ik::PassInfo; 2],
    /// The pass currently being driven; set by [`SetActiveIKPass`](HumanIK::SetActiveIKPass) and
    /// read by the queue/solve helpers.
    pub m_CurrentPass: crate::animation::ik::Pass,
    _field_b4: [u8; 4],
    pub m_HIKNodeAndBonePairs: crate::types::std_vector::Vector<
        crate::animation::ik::NodeAndBonePair,
    >,
    /// A `-2`-terminated list of the used `HIKNodeId`s, in bone-index order.
    pub m_UsedHIKNodeIds: *mut i32,
    pub m_TQS: crate::types::std_vector::Vector<crate::animation::ik::Tqs>,
    pub m_EffectorIds: crate::animation::ik::EffectorIdTable,
    /// The target pull weight per effector (interpolation destination for `m_Pull`).
    pub m_TargetPull: [f32; 44],
    /// The target resistance weight per effector (interpolation destination for `m_Resist`).
    pub m_TargetResist: [f32; 44],
    /// The target translation-reach weight per effector: how strongly a positional target pulls the
    /// effector. Callers write this directly after queuing a positional target (interpolation
    /// destination for `m_ReachT`).
    pub m_TargetReachT: [f32; 44],
    /// The target rotation-reach weight per effector: how strongly a rotational target orients the
    /// effector (interpolation destination for `m_ReachR`).
    pub m_TargetReachR: [f32; 44],
    /// The current pull weight per effector, driven toward `m_TargetPull`.
    pub m_Pull: [f32; 44],
    /// The current resistance weight per effector, driven toward `m_TargetResist`.
    pub m_Resist: [f32; 44],
    /// The current translation-reach weight per effector, driven toward `m_TargetReachT`.
    pub m_ReachT: [f32; 44],
    /// The current rotation-reach weight per effector, driven toward `m_TargetReachR`.
    pub m_ReachR: [f32; 44],
}
fn _HumanIK_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x6A0], HumanIK>([0u8; 0x6A0]);
    }
    unreachable!()
}
impl HumanIK {
    pub const Init_ADDRESS: usize = 0x140408450;
    /// Builds the solver from a skeleton and an Autodesk HIK characterization buffer: creates the
    /// HIK character/state objects, maps each used HIK node to its skeleton bone index (populating
    /// [`m_EffectorIds`](HumanIK::m_EffectorIds)), and zeroes the control-value arrays.
    pub unsafe fn Init(
        &mut self,
        skeleton: u64,
        characterization_buffer: *const u8,
        buffer_size: u64,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                skeleton: u64,
                characterization_buffer: *const u8,
                buffer_size: u64,
            ) = ::std::mem::transmute(Self::Init_ADDRESS);
            f(self as *mut Self as _, skeleton, characterization_buffer, buffer_size)
        }
    }
    pub const SetActiveIKPass_ADDRESS: usize = 0x1403BD1A0;
    /// Selects the pass that subsequent target-queue and solve calls operate on.
    pub unsafe fn SetActiveIKPass(&mut self, pass: crate::animation::ik::Pass) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pass: crate::animation::ik::Pass,
            ) = ::std::mem::transmute(Self::SetActiveIKPass_ADDRESS);
            f(self as *mut Self as _, pass)
        }
    }
    pub const HasTargets_ADDRESS: usize = 0x1403C96B0;
    /// Whether the given pass has any queued position or rotation targets. Gates the pass's solve in
    /// `CCharacter::UpdatePassFinalizePose_Parallel`.
    pub unsafe fn HasTargets(&mut self, pass: crate::animation::ik::Pass) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pass: crate::animation::ik::Pass,
            ) -> bool = ::std::mem::transmute(Self::HasTargets_ADDRESS);
            f(self as *mut Self as _, pass)
        }
    }
    pub const GetEffectorIdFromBoneIndex_ADDRESS: usize = 0x1403E2BF0;
    /// Maps a skeleton bone index to its HumanIK effector id (`0..44`) via
    /// [`m_EffectorIds`](HumanIK::m_EffectorIds), or `-1` if the bone has no effector mapping. The
    /// bone index is in the same space as the character's bone matrices/joints (the value the safe-
    /// bone-index table resolves to). The head bone maps to effector `15`; the chest end effector is
    /// [`GetChestEndEffectorId`](HumanIK::GetChestEndEffectorId).
    pub unsafe fn GetEffectorIdFromBoneIndex(&self, bone_index: u32) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                bone_index: u32,
            ) -> i32 = ::std::mem::transmute(Self::GetEffectorIdFromBoneIndex_ADDRESS);
            f(self as *const Self as _, bone_index)
        }
    }
    pub const GetChestEndEffectorId_ADDRESS: usize = 0x1403BCDD0;
    /// The effector id of the chest end effector (a constant `10`).
    pub unsafe fn GetChestEndEffectorId(&self) -> i32 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> i32 = ::std::mem::transmute(
                Self::GetChestEndEffectorId_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const AddEffectorTargetPosition_ADDRESS: usize = 0x140408860;
    /// Queues a positional effector target on the given pass, or updates the existing target for the
    /// same effector. `pos` is in character-model space. `effector_interpolation` eases the reach
    /// weight in at `effector_interpolation_rate`; `effector_blend_out` eases it out at
    /// `effector_blend_out_rate` once the target stops being supplied. The engine's own hand pass
    /// calls this with `(interpolation=false, interpolation_rate=3.0, blend_out=true,
    /// blend_out_rate=1.5)` and then writes `m_TargetReachT`(HumanIK::m_TargetReachT)`[effector]`
    /// with the desired reach weight.
    ///
    /// **Provenance:** the prototype is verified against the debug PDB.
    pub unsafe fn AddEffectorTargetPosition(
        &mut self,
        effector: i32,
        pos: *const crate::types::math::Vector3,
        solve_step: crate::animation::ik::SolveStep,
        pass: crate::animation::ik::Pass,
        effector_interpolation: bool,
        effector_interpolation_rate: f32,
        effector_blend_out: bool,
        effector_blend_out_rate: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                effector: i32,
                pos: *const crate::types::math::Vector3,
                solve_step: crate::animation::ik::SolveStep,
                pass: crate::animation::ik::Pass,
                effector_interpolation: bool,
                effector_interpolation_rate: f32,
                effector_blend_out: bool,
                effector_blend_out_rate: f32,
            ) = ::std::mem::transmute(Self::AddEffectorTargetPosition_ADDRESS);
            f(
                self as *mut Self as _,
                effector,
                pos,
                solve_step,
                pass,
                effector_interpolation,
                effector_interpolation_rate,
                effector_blend_out,
                effector_blend_out_rate,
            )
        }
    }
    pub const AddEffectorTargetRotation_ADDRESS: usize = 0x140408960;
    /// Queues a rotational effector target about a cardinal [`RotationAxis`], or updates the existing
    /// target for the same effector. `rotation_offset` is in radians. This is the axis-enum overload
    /// of the engine's `AddEffectorTargetRotation`; see
    /// [`AddEffectorTargetRotationVector`](HumanIK::AddEffectorTargetRotationVector) for the
    /// explicit-axis overload.
    pub unsafe fn AddEffectorTargetRotation(
        &mut self,
        effector: i32,
        rotation_offset: f32,
        axis: crate::animation::ik::RotationAxis,
        solve_step: crate::animation::ik::SolveStep,
        pass: crate::animation::ik::Pass,
        effector_interpolation: bool,
        effector_interpolation_rate: f32,
        effector_blend_out: bool,
        effector_blend_out_rate: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                effector: i32,
                rotation_offset: f32,
                axis: crate::animation::ik::RotationAxis,
                solve_step: crate::animation::ik::SolveStep,
                pass: crate::animation::ik::Pass,
                effector_interpolation: bool,
                effector_interpolation_rate: f32,
                effector_blend_out: bool,
                effector_blend_out_rate: f32,
            ) = ::std::mem::transmute(Self::AddEffectorTargetRotation_ADDRESS);
            f(
                self as *mut Self as _,
                effector,
                rotation_offset,
                axis,
                solve_step,
                pass,
                effector_interpolation,
                effector_interpolation_rate,
                effector_blend_out,
                effector_blend_out_rate,
            )
        }
    }
    pub const AddEffectorTargetRotationVector_ADDRESS: usize = 0x140408BB0;
    /// Queues a rotational effector target about an explicit axis vector, or updates the existing
    /// target for the same effector. `rotation_angle` is in radians. This is the explicit-axis
    /// overload of the engine's `AddEffectorTargetRotation`; the aim IK uses it with
    /// [`SolveStep::UPPER_BODY`] on [`Pass::MAIN`] to bend the spine and head toward the aim
    /// direction.
    pub unsafe fn AddEffectorTargetRotationVector(
        &mut self,
        effector: i32,
        rotation_angle: f32,
        rotation_axis: *const crate::types::math::Vector3,
        solve_step: crate::animation::ik::SolveStep,
        pass: crate::animation::ik::Pass,
        effector_interpolation: bool,
        effector_interpolation_rate: f32,
        effector_blend_out: bool,
        effector_blend_out_rate: f32,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                effector: i32,
                rotation_angle: f32,
                rotation_axis: *const crate::types::math::Vector3,
                solve_step: crate::animation::ik::SolveStep,
                pass: crate::animation::ik::Pass,
                effector_interpolation: bool,
                effector_interpolation_rate: f32,
                effector_blend_out: bool,
                effector_blend_out_rate: f32,
            ) = ::std::mem::transmute(Self::AddEffectorTargetRotationVector_ADDRESS);
            f(
                self as *mut Self as _,
                effector,
                rotation_angle,
                rotation_axis,
                solve_step,
                pass,
                effector_interpolation,
                effector_interpolation_rate,
                effector_blend_out,
                effector_blend_out_rate,
            )
        }
    }
    pub const DriveAllCurrentEffectorControlValues_ADDRESS: usize = 0x1403EC970;
    /// Interpolates the active pass's current control values (`m_Pull`(HumanIK::m_Pull) etc.)
    /// toward their targets (`m_TargetPull`(HumanIK::m_TargetPull) etc.) by `dt`, per each queued
    /// target's interpolation/blend-out settings.
    pub unsafe fn DriveAllCurrentEffectorControlValues(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::DriveAllCurrentEffectorControlValues_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const CharacterToIKState_ADDRESS: usize = 0x1403F4390;
    /// Copies the character's current pose (`hkaPose*`) into the HIK character state, in preparation
    /// for a solve.
    pub unsafe fn CharacterToIKState(&mut self, pose: u64) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, pose: u64) = ::std::mem::transmute(
                Self::CharacterToIKState_ADDRESS,
            );
            f(self as *mut Self as _, pose)
        }
    }
    pub const UpdateEffectorsFromTargets_ADDRESS: usize = 0x1403F4530;
    /// Pushes the active pass's queued targets into the HIK effector-set state and promotes the
    /// pass's [`SolveStep`], then applies the current per-effector control values.
    pub unsafe fn UpdateEffectorsFromTargets(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateEffectorsFromTargets_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const Solve_ADDRESS: usize = 0x1403F4920;
    /// Runs the Autodesk HIK solver for the active pass at the pass's accumulated [`SolveStep`].
    pub unsafe fn Solve(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Solve_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const IKToCharacterState_ADDRESS: usize = 0x1403F49D0;
    /// Writes the solved HIK character state back into the character's pose (`hkaPose*`). When
    /// `update_all_bones` is set, every mapped bone is written; otherwise only the affected chain.
    pub unsafe fn IKToCharacterState(&mut self, pose: u64, update_all_bones: bool) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pose: u64,
                update_all_bones: bool,
            ) = ::std::mem::transmute(Self::IKToCharacterState_ADDRESS);
            f(self as *mut Self as _, pose, update_all_bones)
        }
    }
    pub const ResetSolveStep_ADDRESS: usize = 0x1403BD270;
    /// Resets the active pass's accumulated [`SolveStep`] to [`SolveStep::UNDEFINED`].
    pub unsafe fn ResetSolveStep(&mut self, pass: crate::animation::ik::Pass) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pass: crate::animation::ik::Pass,
            ) = ::std::mem::transmute(Self::ResetSolveStep_ADDRESS);
            f(self as *mut Self as _, pass)
        }
    }
    pub const ClearTargets_ADDRESS: usize = 0x1404020F0;
    /// Drops targets whose reach weight has fully blended out, and marks the rest not-valid for the
    /// next frame. Returns whether the pass is now empty of targets.
    pub unsafe fn ClearTargets(&mut self, pass: crate::animation::ik::Pass) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                pass: crate::animation::ik::Pass,
            ) -> bool = ::std::mem::transmute(Self::ClearTargets_ADDRESS);
            f(self as *mut Self as _, pass)
        }
    }
    pub const ResetProperties_ADDRESS: usize = 0x1403BD260;
    /// Resets the solver tuning properties applied during floor-contact setup.
    pub unsafe fn ResetProperties(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ResetProperties_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ResetAllTargetEffectorControlValues_ADDRESS: usize = 0x1403BCE40;
    /// Zeroes the four target control-value arrays (`m_TargetPull`(HumanIK::m_TargetPull),
    /// `m_TargetResist`(HumanIK::m_TargetResist), `m_TargetReachT`(HumanIK::m_TargetReachT),
    /// `m_TargetReachR`(HumanIK::m_TargetReachR)).
    pub unsafe fn ResetAllTargetEffectorControlValues(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::ResetAllTargetEffectorControlValues_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl HumanIK {
    /// The number of effector-control slots: the valid range of an effector id (`0..44`) and the
    /// length of each per-effector control-value array.
    pub const EFFECTOR_SLOTS: u64 = 44;
}
impl std::convert::AsRef<HumanIK> for HumanIK {
    fn as_ref(&self) -> &HumanIK {
        self
    }
}
impl std::convert::AsMut<HumanIK> for HumanIK {
    fn as_mut(&mut self) -> &mut HumanIK {
        self
    }
}
#[repr(C, align(8))]
/// A HumanIK-node-to-skeleton-bone mapping, built at [`Init`](HumanIK::Init) time for every HumanIK
/// node the characterization uses.
pub struct NodeAndBonePair {
    /// The skeleton bone index this HumanIK node drives.
    pub bone_index: i32,
    /// The `HIKNodeId` (Autodesk HumanIK node identifier).
    pub hik_node_id: i32,
}
fn _NodeAndBonePair_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], NodeAndBonePair>([0u8; 0x8]);
    }
    unreachable!()
}
impl NodeAndBonePair {}
impl std::convert::AsRef<NodeAndBonePair> for NodeAndBonePair {
    fn as_ref(&self) -> &NodeAndBonePair {
        self
    }
}
impl std::convert::AsMut<NodeAndBonePair> for NodeAndBonePair {
    fn as_mut(&mut self) -> &mut NodeAndBonePair {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// The IK pass an effector target belongs to, and the pass currently being driven. The engine keeps
/// one independent [`PassInfo`] per pass. `MAIN` is the general body-IK pass (aim IK, reach IK);
/// `SECONDARY` is the hand/grip pass. Each pass is solved separately per frame, gated on whether it
/// has targets.
pub enum Pass {
    MAIN = 0isize as _,
    SECONDARY = 1isize as _,
    NUM = 2isize as _,
}
fn _Pass_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], Pass>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// The per-pass state: the accumulated [`SolveStep`] for the pass and the queued position and
/// rotation targets. [`HumanIK`] holds one of these per [`Pass`].
pub struct PassInfo {
    pub m_SolveStep: crate::animation::ik::SolveStep,
    _field_4: [u8; 4],
    pub m_EffectorTargetPositions: crate::types::std_vector::Vector<
        crate::animation::ik::EffectorTargetPosition,
    >,
    pub m_EffectorTargetRotations: crate::types::std_vector::Vector<
        crate::animation::ik::EffectorTargetRotation,
    >,
}
fn _PassInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x48], PassInfo>([0u8; 0x48]);
    }
    unreachable!()
}
impl PassInfo {}
impl std::convert::AsRef<PassInfo> for PassInfo {
    fn as_ref(&self) -> &PassInfo {
        self
    }
}
impl std::convert::AsMut<PassInfo> for PassInfo {
    fn as_mut(&mut self) -> &mut PassInfo {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// The rotation axis selector for [`AddEffectorTargetRotation`](HumanIK::AddEffectorTargetRotation):
/// a single cardinal axis or all three.
pub enum RotationAxis {
    X = 0isize as _,
    Y = 1isize as _,
    Z = 2isize as _,
    XYZ = 3isize as _,
}
fn _RotationAxis_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], RotationAxis>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// The IK solve step requested for an effector target, and the accumulated step for a pass. Each
/// target carries its own step; the pass's step is promoted to the maximum of its targets' steps
/// (with arm-combining special cases) before [`Solve`](HumanIK::Solve) maps it to the Autodesk
/// HumanIK solver bitmask. Higher values solve more of the body.
pub enum SolveStep {
    UNDEFINED = 0isize as _,
    SPINE_ONLY = 1isize as _,
    SPINE_HEAD_ONLY = 2isize as _,
    RIGHT_ARM = 3isize as _,
    LEFT_ARM = 4isize as _,
    ARMS = 5isize as _,
    SPINE_HEAD_LOWER_BODY = 6isize as _,
    UPPER_BODY = 7isize as _,
    FULL_BODY_NO_PULL = 8isize as _,
    FULL_BODY = 9isize as _,
}
fn _SolveStep_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], SolveStep>([0u8; 0x4]);
    }
    unreachable!()
}
#[repr(C, align(8))]
/// A cached translation/quaternion/scale triple for a HumanIK node (one per used node), populated
/// while transferring the pose to and from the solver.
pub struct Tqs {
    pub pt: [f32; 4],
    pub pq: [f32; 4],
    pub ps: [f32; 4],
}
fn _Tqs_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], Tqs>([0u8; 0x30]);
    }
    unreachable!()
}
impl Tqs {}
impl std::convert::AsRef<Tqs> for Tqs {
    fn as_ref(&self) -> &Tqs {
        self
    }
}
impl std::convert::AsMut<Tqs> for Tqs {
    fn as_mut(&mut self) -> &mut Tqs {
        self
    }
}
pub const NHandIKTask_Update_ADDRESS: usize = 0x140816430;
/// The per-frame hand-IK driver. Sources its targets from weapon grip positions.
unsafe fn NHandIKTask_Update(
    ctx: *mut crate::state::StateContext,
    p1: *mut ::std::ffi::c_void,
    p2: *mut ::std::ffi::c_void,
) {
    unsafe {
        let f: unsafe extern "system" fn(
            ctx: *mut crate::state::StateContext,
            p1: *mut ::std::ffi::c_void,
            p2: *mut ::std::ffi::c_void,
        ) = ::std::mem::transmute(NHandIKTask_Update_ADDRESS);
        f(ctx, p1, p2)
    }
}
