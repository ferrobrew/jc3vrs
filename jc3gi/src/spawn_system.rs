#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The opaque BFBC context: the per-system beam-frustum-based-culling state that `BFBCProcess` reads
/// and writes. Its layout is internal to the BFBC backend and not mapped here.
pub struct SBFBCContext {}
impl SBFBCContext {}
impl std::convert::AsRef<SBFBCContext> for SBFBCContext {
    fn as_ref(&self) -> &SBFBCContext {
        self
    }
}
impl std::convert::AsMut<SBFBCContext> for SBFBCContext {
    fn as_mut(&mut self) -> &mut SBFBCContext {
        self
    }
}
#[repr(C, align(8))]
/// The opaque BFBC process parameter block: the per-camera cull-frustum parameters produced by
/// [`GetBFBCFrustumParamsForCameraAndTime`](graphics_engine::graphics_engine::OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime)
/// and consumed by `BFBCProcess`. See
/// [`BFBCFrustumCullParameters`](crate::graphics_engine::graphics_engine::BFBCFrustumCullParameters) for the
/// one mapped field (`m_FrustumCount`).
pub struct SBFBCProcessParameter {}
impl SBFBCProcessParameter {}
impl std::convert::AsRef<SBFBCProcessParameter> for SBFBCProcessParameter {
    fn as_ref(&self) -> &SBFBCProcessParameter {
        self
    }
}
impl std::convert::AsMut<SBFBCProcessParameter> for SBFBCProcessParameter {
    fn as_mut(&mut self) -> &mut SBFBCProcessParameter {
        self
    }
}
#[repr(C, align(8))]
/// The opaque BFBC process state: the per-camera cache slot handle returned alongside the parameter
/// block.
pub struct SBFBCProcessState {}
impl SBFBCProcessState {}
impl std::convert::AsRef<SBFBCProcessState> for SBFBCProcessState {
    fn as_ref(&self) -> &SBFBCProcessState {
        self
    }
}
impl std::convert::AsMut<SBFBCProcessState> for SBFBCProcessState {
    fn as_mut(&mut self) -> &mut SBFBCProcessState {
        self
    }
}
#[repr(C, align(8))]
/// The spawn system: manages streaming (de)spawn of characters and vehicles around the player. Each
/// frame, [`Update`](SpawnSystem::Update) builds a BFBC cull frustum from the camera manager's active
/// camera and uses it as a "don't (de)spawn while visible" gate in `CSpawnFactoryImpl::CheckInternal`, so
/// objects are not spawned or despawned inside the player's view. Its budgets are characters and
/// vehicles, so it governs NPC/vehicle (de)spawns near the view edge, not buildings.
pub struct SpawnSystem {
    _field_0: [u8; 360],
    /// The BFBC cull context the spawn system's visibility gate runs against.
    pub m_SpawnBFBC: *mut crate::spawn_system::SBFBCContext,
    /// The BFBC cull-frustum parameters, populated each frame by
    /// [`GetBFBCFrustumParamsForCameraAndTime`](graphics_engine::graphics_engine::OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime)
    /// against the active camera.
    pub m_SpawnBFBCParams: *const crate::spawn_system::SBFBCProcessParameter,
    /// The BFBC cache-slot state handle, populated alongside the parameters.
    pub m_SpawnBFBCState: *mut crate::spawn_system::SBFBCProcessState,
    _field_180: [u8; 20],
    /// The center position the spawn system streams around, set by
    /// [`SetCenterPosition`](SpawnSystem::SetCenterPosition).
    pub m_CenterPos: crate::types::math::Vector3,
    /// The view position for the spawn visibility gate, set by
    /// [`SetCenterPosition`](SpawnSystem::SetCenterPosition).
    pub m_ViewPos: crate::types::math::Vector3,
    /// The view direction for the spawn visibility gate, set by
    /// [`SetCenterPosition`](SpawnSystem::SetCenterPosition).
    pub m_ViewDir: crate::types::math::Vector3,
}
fn _SpawnSystem_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1B8], SpawnSystem>([0u8; 0x1B8]);
    }
    unreachable!()
}
impl SpawnSystem {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418092952usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl SpawnSystem {
    pub const Update_ADDRESS: usize = 0x140EFEE90;
    /// The per-frame spawn update: walks every resource definition, evaluates despawn ranks against the
    /// view position, and at its tail builds a BFBC cull frustum from the camera manager's active camera
    /// ([`CameraManager::m_ActiveCamera`](camera::camera_manager::CameraManager::m_ActiveCamera)) via
    /// [`GetBFBCFrustumParamsForCameraAndTime`](graphics_engine::graphics_engine::OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime),
    /// storing the result in [`m_SpawnBFBCParams`](SpawnSystem::m_SpawnBFBCParams) and
    /// [`m_SpawnBFBCState`](SpawnSystem::m_SpawnBFBCState) for the factory's visibility gate.
    pub unsafe fn Update(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const SetCenterPosition_ADDRESS: usize = 0x1412763B0;
    /// Sets the spawn center, view position, and view direction used by the visibility gate.
    pub unsafe fn SetCenterPosition(
        &mut self,
        centerpos: *const crate::types::math::Vector3,
        viewpos: *const crate::types::math::Vector3,
        viewdir: *const crate::types::math::Vector3,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                centerpos: *const crate::types::math::Vector3,
                viewpos: *const crate::types::math::Vector3,
                viewdir: *const crate::types::math::Vector3,
            ) = ::std::mem::transmute(Self::SetCenterPosition_ADDRESS);
            f(self as *mut Self as _, centerpos, viewpos, viewdir)
        }
    }
}
impl std::convert::AsRef<SpawnSystem> for SpawnSystem {
    fn as_ref(&self) -> &SpawnSystem {
        self
    }
}
impl std::convert::AsMut<SpawnSystem> for SpawnSystem {
    fn as_mut(&mut self) -> &mut SpawnSystem {
        self
    }
}
