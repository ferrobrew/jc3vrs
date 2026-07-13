#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The game world object manager: owns the character occlusion BFBC state and drives the per-frame
/// character occlusion cull. Each frame, [`StartCommitAddRemove`](GameWorldObjectManager::StartCommitAddRemove)
/// copies the camera manager's active camera into [`m_OcclusionCamera`](GameWorldObjectManager::m_OcclusionCamera)
/// and dispatches [`ProcessCharacterOcclusion`](GameWorldObjectManager::ProcessCharacterOcclusion) via a
/// CPU fragment. That function builds a BFBC cull frustum from the active camera via
/// [`GetBFBCFrustumParamsForCameraAndTime`](graphics_engine::graphics_engine::OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime),
/// then runs `BFBCProcess` with `E_BFBC_PROCESSFUNCTION_FRUSTUM_CULL` to cull character occlusion —
/// hiding characters the frustum rejects. The frustum is built from the single active camera's
/// projection.
pub struct GameWorldObjectManager {
    _field_0: [u8; 2552],
    /// The camera the character occlusion cull builds its BFBC frustum against. Set each frame by
    /// [`StartCommitAddRemove`](GameWorldObjectManager::StartCommitAddRemove) from
    /// [`CameraManager::m_ActiveCamera`](camera::camera_manager::CameraManager::m_ActiveCamera).
    pub m_OcclusionCamera: *const crate::camera::camera::Camera,
    _field_a00: [u8; 65680],
    /// The BFBC cull context for character occlusion.
    pub m_CharacterBFBCContext: *mut crate::spawn_system::SBFBCContext,
}
fn _GameWorldObjectManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10A98], GameWorldObjectManager>([0u8; 0x10A98]);
    }
    unreachable!()
}
impl GameWorldObjectManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418086696usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl GameWorldObjectManager {
    pub const StartCommitAddRemove_ADDRESS: usize = 0x1404BB8B0;
    /// Copies the camera manager's active camera into
    /// [`m_OcclusionCamera`](GameWorldObjectManager::m_OcclusionCamera), along with its frustum planes
    /// and transform, then dispatches
    /// [`ProcessCharacterOcclusion`](GameWorldObjectManager::ProcessCharacterOcclusion) via a CPU
    /// fragment (`CallProxy`).
    pub unsafe fn StartCommitAddRemove(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::StartCommitAddRemove_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const ProcessCharacterOcclusion_ADDRESS: usize = 0x1404BB7E0;
    /// Builds a BFBC cull frustum from [`m_OcclusionCamera`](GameWorldObjectManager::m_OcclusionCamera)
    /// via [`GetBFBCFrustumParamsForCameraAndTime`](graphics_engine::graphics_engine::OccluderCollectionManager::GetBFBCFrustumParamsForCameraAndTime),
    /// then runs `BFBCProcess` with `E_BFBC_PROCESSFUNCTION_FRUSTUM_CULL` to cull character occlusion.
    /// Dispatched asynchronously from
    /// [`StartCommitAddRemove`](GameWorldObjectManager::StartCommitAddRemove).
    pub unsafe fn ProcessCharacterOcclusion(&mut self) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) -> u64 = ::std::mem::transmute(
                Self::ProcessCharacterOcclusion_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<GameWorldObjectManager> for GameWorldObjectManager {
    fn as_ref(&self) -> &GameWorldObjectManager {
        self
    }
}
impl std::convert::AsMut<GameWorldObjectManager> for GameWorldObjectManager {
    fn as_mut(&mut self) -> &mut GameWorldObjectManager {
        self
    }
}
