#![cfg_attr(any(), rustfmt::skip)]
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
/// The effector slot a HumanIK node drives, as assigned by the engine's node-to-effector mapping.
/// The mapping is a fixed switch over `HIKNodeId`, so these ids are constant across characterizations;
/// a node with no effector maps to [`EFFECTOR_SLOTS`](crate::animation::ik::HumanIK::EFFECTOR_SLOTS) instead, and
/// [`GetEffectorIdFromBoneIndex`](crate::animation::ik::HumanIK::GetEffectorIdFromBoneIndex) reports `-1` for a bone that
/// is not mapped at all.
///
/// Only nodes the characterization actually uses are present in a given solver, so an id here is the
/// slot a node *would* occupy rather than a promise that the rig has that node.
pub enum Effector {
    HIPS = 0isize as _,
    LEFT_ANKLE = 1isize as _,
    RIGHT_ANKLE = 2isize as _,
    LEFT_WRIST = 3isize as _,
    RIGHT_WRIST = 4isize as _,
    LEFT_KNEE = 5isize as _,
    RIGHT_KNEE = 6isize as _,
    LEFT_ELBOW = 7isize as _,
    RIGHT_ELBOW = 8isize as _,
    WAIST = 9isize as _,
    CHEST_END = 10isize as _,
    LEFT_FOOT = 11isize as _,
    RIGHT_FOOT = 12isize as _,
    LEFT_SHOULDER = 13isize as _,
    RIGHT_SHOULDER = 14isize as _,
    HEAD = 15isize as _,
    LEFT_HIP = 16isize as _,
    RIGHT_HIP = 17isize as _,
}
fn _Effector_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], Effector>([0u8; 0x4]);
    }
    unreachable!()
}
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
/// [`Init`](crate::animation::ik::HumanIK::Init) time: for every used HumanIK node, the node's skeleton bone index keys
/// the node's [`Effector`](crate::animation::ik::Effector) id. [`GetEffectorIdFromBoneIndex`](crate::animation::ik::HumanIK::GetEffectorIdFromBoneIndex)
/// queries it.
///
/// The key is the bone whose *name matches the HumanIK node's own name*: `Init` takes
/// `HIKNodeNameFromNodeId(node)`, hashes it, and resolves it through
/// `CSkeleton::GetBoneIndexByHashedName`. Membership therefore follows Autodesk's node naming rather
/// than any of the engine's own bone-naming schemes, and the two coincide only where a rig happens
/// to use the same name for the same joint. A bone the characterization does not name is absent from
/// the table entirely. To go the other way — from an effector to the bone it drives — walk
/// [`m_HIKNodeAndBonePairs`](crate::animation::ik::HumanIK::m_HIKNodeAndBonePairs), which holds the same relation in a form
/// that can be scanned.
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
    /// Whether this entry was (re-)queued since the last solve. Set by
    /// [`AddEffectorTargetPosition`](crate::animation::ik::HumanIK::AddEffectorTargetPosition), cleared by
    /// [`ClearTargets`](crate::animation::ik::HumanIK::ClearTargets) after the solve consumes the pass. A cleared entry is
    /// *retained* — [`UpdateEffectorsFromTargets`](crate::animation::ik::HumanIK::UpdateEffectorsFromTargets) keeps
    /// applying its (now stale) position while
    /// [`DriveAllCurrentEffectorControlValues`](crate::animation::ik::HumanIK::DriveAllCurrentEffectorControlValues)
    /// blends its reach weight toward zero — and is removed only once that reach reaches zero. So a
    /// set flag distinguishes a target actively supplied this frame from a leftover blending out.
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
/// behave as for [`EffectorTargetPosition`](crate::animation::ik::EffectorTargetPosition).
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
    /// Whether this entry was (re-)queued since the last solve; the semantics are those of
    /// [`EffectorTargetPosition::is_valid`](crate::animation::ik::EffectorTargetPosition::is_valid).
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
/// state objects, holds the queued effector targets for each [`Pass`](crate::animation::ik::Pass), and holds the per-effector
/// control-value arrays (pull / resistance / translation-reach / rotation-reach), indexed by
/// effector id (`0..44`).
///
/// # Per-frame lifecycle
///
/// A character drives its IK inside `CCharacter::UpdatePassFinalizePose_Parallel`, after the
/// animation graph has finalized the local pose and before `CalculateModelSpacePose`. For each
/// [`Pass`](crate::animation::ik::Pass), the sequence is:
///
/// 1. Targets are queued (via [`AddEffectorTargetPosition`](crate::animation::ik::HumanIK::AddEffectorTargetPosition) /
///    [`AddEffectorTargetRotation`](Self::AddEffectorTargetRotation)) — the aim/reach IK does
///    this during animation-graph evaluation for `MAIN`; the hand pass queues its own for
///    `SECONDARY`.
/// 2. [`HasTargets`](crate::animation::ik::HumanIK::HasTargets) gates the solve. If there are none, the whole solve for
///    that pass is skipped.
/// 3. [`SetActiveIKPass`](crate::animation::ik::HumanIK::SetActiveIKPass), then
///    [`DriveAllCurrentEffectorControlValues`](Self::DriveAllCurrentEffectorControlValues), then
///    the solve proper: [`CharacterToIKState`](Self::CharacterToIKState) →
///    [`UpdateEffectorsFromTargets`](Self::UpdateEffectorsFromTargets) →
///    [`Solve`](Self::Solve) → [`IKToCharacterState`](Self::IKToCharacterState) (writing the
///    solved pose back into the character's `hkaPose`).
/// 4. [`ResetSolveStep`](crate::animation::ik::HumanIK::ResetSolveStep), then [`ClearTargets`](crate::animation::ik::HumanIK::ClearTargets)
///    invalidates consumed targets (and returns whether the pass is now empty).
///
/// A target queued before the `HasTargets` gate for a pass is therefore consumed in the same frame.
/// It is not *removed* that frame, though: `ClearTargets` only clears the entry's `is_valid` flag,
/// and the entry — stale data and all — keeps being applied by
/// [`UpdateEffectorsFromTargets`](crate::animation::ik::HumanIK::UpdateEffectorsFromTargets) with a decaying reach weight
/// until a later `ClearTargets` removes it (immediately on the next frame without the blend-out
/// flag, after the blend-out decay with it). A supplier that re-queues per frame re-validates the
/// same entry before the gate each time, so the entry's data never goes stale.
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
    /// One [`PassInfo`](crate::animation::ik::PassInfo) per [`Pass`](crate::animation::ik::Pass) (`MAIN`, `SECONDARY`).
    pub m_PassInfo: [crate::animation::ik::PassInfo; 2],
    /// The pass currently being driven; set by [`SetActiveIKPass`](crate::animation::ik::HumanIK::SetActiveIKPass) and
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
    /// The target pull weight per effector: how much satisfying this effector's target propagates
    /// into the rest of the body rather than being absorbed by the chain the effector terminates.
    /// An effector reached with zero pull moves only its local chain, leaving the torso on the
    /// animated pose. Copied into [`m_Pull`](crate::animation::ik::HumanIK::m_Pull) — verbatim, not eased — by
    /// [`DriveAllCurrentEffectorControlValues`](crate::animation::ik::HumanIK::DriveAllCurrentEffectorControlValues), for
    /// each effector carrying a queued *positional* target. The foot and hip IK writes this
    /// alongside [`m_TargetReachT`](crate::animation::ik::HumanIK::m_TargetReachT); the aim IK, which queues only rotational
    /// targets, leaves it alone.
    pub m_TargetPull: [f32; 44],
    /// The target resistance weight per effector: how much this effector resists being displaced by
    /// other effectors' pull. Copied into [`m_Resist`](crate::animation::ik::HumanIK::m_Resist) — verbatim, not eased — by
    /// [`DriveAllCurrentEffectorControlValues`](crate::animation::ik::HumanIK::DriveAllCurrentEffectorControlValues), for
    /// each effector carrying a queued *rotational* target.
    pub m_TargetResist: [f32; 44],
    /// The target translation-reach weight per effector: how strongly a positional target pulls the
    /// effector. Callers write this directly after queuing a positional target (interpolation
    /// destination for `m_ReachT`).
    pub m_TargetReachT: [f32; 44],
    /// The target rotation-reach weight per effector: how strongly a rotational target orients the
    /// effector (interpolation destination for `m_ReachR`).
    pub m_TargetReachR: [f32; 44],
    /// The current pull weight per effector, driven toward
    /// [`m_TargetPull`](crate::animation::ik::HumanIK::m_TargetPull) and pushed into the effector-set state via
    /// `HIKSetPull` by
    /// [`UpdateEffectorsFromTargets`](crate::animation::ik::HumanIK::UpdateEffectorsFromTargets).
    pub m_Pull: [f32; 44],
    /// The current resistance weight per effector, driven toward
    /// [`m_TargetResist`](crate::animation::ik::HumanIK::m_TargetResist) and pushed into the effector-set state via
    /// `HIKSetResist` by
    /// [`UpdateEffectorsFromTargets`](crate::animation::ik::HumanIK::UpdateEffectorsFromTargets).
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
    /// [`m_EffectorIds`](crate::animation::ik::HumanIK::m_EffectorIds)), and zeroes the control-value arrays.
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
    /// [`m_EffectorIds`](crate::animation::ik::HumanIK::m_EffectorIds), or `-1` if the bone has no effector mapping. The
    /// bone index is in the same space as the character's bone matrices/joints (the value the safe-
    /// bone-index table resolves to). The head bone maps to effector `15`; the chest end effector is
    /// [`GetChestEndEffectorId`](crate::animation::ik::HumanIK::GetChestEndEffectorId).
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
    /// A pass holds at most one position target per effector: if an entry for `effector` already
    /// exists — including one [`ClearTargets`](crate::animation::ik::HumanIK::ClearTargets) has invalidated but not yet
    /// removed — it is overwritten in place and re-validated rather than a second entry being added.
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
    /// Queues a rotational effector target about a cardinal [`RotationAxis`](crate::animation::ik::RotationAxis), or updates the existing
    /// target for the same effector. `rotation_offset` is in radians. This is the axis-enum overload
    /// of the engine's `AddEffectorTargetRotation`; see
    /// [`AddEffectorTargetRotationVector`](crate::animation::ik::HumanIK::AddEffectorTargetRotationVector) for the
    /// explicit-axis overload. The one-live-entry-per-effector update semantics are those of
    /// [`AddEffectorTargetPosition`](crate::animation::ik::HumanIK::AddEffectorTargetPosition), on the pass's rotation list.
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
    /// [`SolveStep::UPPER_BODY`](crate::animation::ik::SolveStep::UPPER_BODY) on [`Pass::MAIN`](crate::animation::ik::Pass::MAIN) to bend the spine and head toward the aim
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
    ///
    /// The pass's two target lists are walked separately, and each drives a different pair of
    /// control values:
    ///
    /// - The **position**-target list drives [`m_ReachT`](crate::animation::ik::HumanIK::m_ReachT), and copies
    ///   [`m_TargetPull`](crate::animation::ik::HumanIK::m_TargetPull) into [`m_Pull`](crate::animation::ik::HumanIK::m_Pull) verbatim, with no
    ///   interpolation.
    /// - The **rotation**-target list drives [`m_ReachR`](crate::animation::ik::HumanIK::m_ReachR) from
    ///   [`m_TargetReachR`](crate::animation::ik::HumanIK::m_TargetReachR) the same way, and copies
    ///   [`m_TargetResist`](crate::animation::ik::HumanIK::m_TargetResist) into [`m_Resist`](crate::animation::ik::HumanIK::m_Resist) verbatim.
    ///
    /// The reach drive branches on the entry's `is_valid` flag:
    ///
    /// - **Valid** (queued since the last solve): the reach is `SpeedLerp`ed toward the target reach
    ///   when the entry carries the interpolation flag, and snapped to it otherwise. The blend-out
    ///   flag plays no part.
    /// - **Invalid** (a leftover [`ClearTargets`](crate::animation::ik::HumanIK::ClearTargets) has not yet removed): the
    ///   reach is `SpeedLerp`ed toward zero at the blend-out rate when the entry carries the
    ///   blend-out flag, and zeroed outright otherwise. The interpolation flag plays no part.
    ///
    /// So an effector's pull is only refreshed in a frame where it carries a positional target, and
    /// its resistance only in a frame where it carries a rotational one — and both copies run for
    /// invalid leftovers too, all the way until removal.
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
    /// pass's [`SolveStep`](crate::animation::ik::SolveStep), then applies the current per-effector control values.
    ///
    /// Every entry in the pass's lists is applied, whether or not it `is_valid`: an invalidated
    /// leftover keeps steering its effector with its stale data while its reach weight blends out.
    /// A position target replaces the effector state's translation and keeps its rotation; a
    /// rotation target multiplies its angle-axis quaternion onto the effector state's current
    /// rotation (an *offset*, not an absolute orientation — a zero angle targets the state as it
    /// stands) and keeps its translation. The effector states themselves were just rebuilt from the
    /// character's pre-solve pose (`HIKEffectorSetFromCharacter` at entry), so "current" here means
    /// this frame's animated pose.
    ///
    /// The final stage loops over all [`EFFECTOR_SLOTS`](crate::animation::ik::HumanIK::EFFECTOR_SLOTS) slots — not just the
    /// ones with queued targets — and pushes four values into the effector-set state:
    /// `HIKSetTranslationActive` from [`m_ReachT`](crate::animation::ik::HumanIK::m_ReachT), `HIKSetRotationActive` from
    /// [`m_ReachR`](crate::animation::ik::HumanIK::m_ReachR), `HIKSetPull` from [`m_Pull`](crate::animation::ik::HumanIK::m_Pull), and
    /// `HIKSetResist` from [`m_Resist`](crate::animation::ik::HumanIK::m_Resist).
    pub unsafe fn UpdateEffectorsFromTargets(&mut self, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, dt: f32) = ::std::mem::transmute(
                Self::UpdateEffectorsFromTargets_ADDRESS,
            );
            f(self as *mut Self as _, dt)
        }
    }
    pub const Solve_ADDRESS: usize = 0x1403F4920;
    /// Runs the Autodesk HIK solver for the active pass at the pass's accumulated [`SolveStep`](crate::animation::ik::SolveStep).
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
    /// Resets the active pass's accumulated [`SolveStep`](crate::animation::ik::SolveStep) to [`SolveStep::UNDEFINED`](crate::animation::ik::SolveStep::UNDEFINED).
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
    ///
    /// An entry is removed only when its effector's current reach weight is zero *and* it is
    /// already invalid; every other entry survives with `is_valid` cleared, and
    /// [`UpdateEffectorsFromTargets`](crate::animation::ik::HumanIK::UpdateEffectorsFromTargets) keeps applying it until a
    /// later `ClearTargets` removes it. With the blend-out flag set that takes as long as the reach
    /// takes to decay (`SpeedLerp` at the blend-out rate); without it,
    /// [`DriveAllCurrentEffectorControlValues`](crate::animation::ik::HumanIK::DriveAllCurrentEffectorControlValues) zeroes
    /// the reach on the next driven frame and the entry dies there.
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
/// A HumanIK-node-to-skeleton-bone mapping, built at [`Init`](crate::animation::ik::HumanIK::Init) time for every HumanIK
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
/// one independent [`PassInfo`](crate::animation::ik::PassInfo) per pass. `MAIN` is the general body-IK pass (aim IK, reach IK);
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
/// The per-pass state: the accumulated [`SolveStep`](crate::animation::ik::SolveStep) for the pass and the queued position and
/// rotation targets. [`HumanIK`](crate::animation::ik::HumanIK) holds one of these per [`Pass`](crate::animation::ik::Pass).
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
/// The rotation axis selector for [`AddEffectorTargetRotation`](crate::animation::ik::HumanIK::AddEffectorTargetRotation):
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
/// (with arm-combining special cases) before [`Solve`](crate::animation::ik::HumanIK::Solve) maps it to the Autodesk
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
