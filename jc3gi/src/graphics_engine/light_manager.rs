#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The scene light manager: gathers the frame's visible lights and prepares the per-frame lighting
/// render state.
pub struct LightManager {}
impl LightManager {
    pub const CopyLightsToUpdate_ADDRESS: usize = 0x1400C6860;
    /// Copies the frame's visible point and spot lights into the parity-buffered update lists and
    /// configures the volumetric-lighting passes. Reads
    /// [`enable_low_res_spot_light_volume`] (and whether any volumetric spot lights are present this
    /// frame) to select the reduced- or full-resolution spot-light-cone render setup, toggle the
    /// low-res upsampling pass, and set the spot-light-cone block type's low-res flag. `reserved_slots`
    /// is the count of shadow-atlas slices reserved for other casters; `dt` drives the per-light fade.
    pub unsafe fn CopyLightsToUpdate(&mut self, reserved_slots: u32, dt: f32) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                reserved_slots: u32,
                dt: f32,
            ) = ::std::mem::transmute(Self::CopyLightsToUpdate_ADDRESS);
            f(self as *mut Self as _, reserved_slots, dt)
        }
    }
}
impl std::convert::AsRef<LightManager> for LightManager {
    fn as_ref(&self) -> &LightManager {
        self
    }
}
impl std::convert::AsMut<LightManager> for LightManager {
    fn as_mut(&mut self) -> &mut LightManager {
        self
    }
}
/// Whether the engine renders volumetric spot-light cones at reduced resolution (the engine's
/// `g_EnableLowResSpotLightVolume`). When set — the default — the per-frame light gather routes the
/// spot-light-cone render block through a quarter-resolution render setup, enables the low-res
/// upsampling pass, and sets the cone block type's low-res flag; when clear, the cones render at full
/// resolution into the main render setup. It is also treated as clear for any frame that has no
/// visible volumetric spot lights. Read once per frame by
/// [`LightManager::CopyLightsToUpdate`](LightManager::CopyLightsToUpdate).
pub unsafe fn get_enable_low_res_spot_light_volume() -> &'static mut bool {
    unsafe { &mut *(0x142D3A6F2 as *mut bool) }
}
