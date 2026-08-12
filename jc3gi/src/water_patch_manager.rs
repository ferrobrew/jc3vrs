#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The water patch manager (`CWaterPatchManager`, a `Base::CSingle` singleton): owns the water
/// reflection pipeline (the distant backdrop, atmosphere, cloud, mesh, and distant-light
/// reflection passes, their dedicated reflection camera, and the distant/full reflection
/// textures), the top-down foam and wake textures, and the CPU-side wave table, and drives the
/// per-frame WaveWorks simulation step via `DoWaveWorksSimulation` from the game update.
pub struct WaterPatchManager {
    _field_0: [u8; 264],
    /// The camera the water reflection passes render from. `UpdateThread` rebuilds its transform
    /// each frame by mirroring the active camera across the water plane, and
    /// `UpdateReflectionCamera` copies the active camera's FOV, near, far, and aspect onto it; the
    /// reflection pre-passes (`PRE_RP_REFLECTION_PRE` through `PRE_RP_REFLECTION_POST`) then draw
    /// the mirrored backdrop, atmosphere, clouds, meshes, and distant lights from it into the
    /// distant/full reflection textures.
    pub m_ReflectionCamera: *mut crate::camera::camera::Camera,
    _field_110: [u8; 274],
    /// Whether the water uses the screen-space water reflection path. When set,
    /// `CNvWaterHighEndRenderBlock::Draw` re-binds the water's fragment reflection slot to the
    /// distant (backdrop) reflection texture in place of the full reflection texture, and
    /// `CNvWaterHighEndRenderBlock::Setup` bakes the flag into the fragment constants for the
    /// shader's reflection-path select.
    pub m_EnableScreenSpaceWaterReflection: bool,
    _field_223: [u8; 5],
}
fn _WaterPatchManager_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x228], WaterPatchManager>([0u8; 0x228]);
    }
    unreachable!()
}
impl WaterPatchManager {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5418079800usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl WaterPatchManager {}
impl std::convert::AsRef<WaterPatchManager> for WaterPatchManager {
    fn as_ref(&self) -> &WaterPatchManager {
        self
    }
}
impl std::convert::AsMut<WaterPatchManager> for WaterPatchManager {
    fn as_mut(&mut self) -> &mut WaterPatchManager {
        self
    }
}
