#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The screen-space ambient-occlusion render pass.
///
/// It maintains a two-slot temporal AO history, blended in when `m_EnableTemporalFilter` is set. The
/// history index advances once per render-block draw (an inlined `SetNextHistoryBuffer` at the end of
/// the apply), not in [`RotateRenderFrameData`], so a twice-per-frame stereo render double-steps the
/// history and blends each eye's AO against the other's.
///
/// The render block's final composite draw lives inside the `m_EnableTemporalFilter` branch, so
/// clearing that flag disables the whole apply (no AO at all). The per-eye reset lever is instead
/// `m_FirstPass`: the engine uses it to mean "no valid history, compute fresh", weighting the temporal
/// shader fully toward the current AO. Forcing it on before each stereo eye keeps the apply running
/// while making each eye compute AO from its own depth.
pub struct SSAOPass {
    _field_0: [u8; 2480],
    /// Enables the temporal filter, blending the current AO against the previous frame's result
    /// through the history ping-pong. Defaults on. Also gates the render block's final composite draw,
    /// so clearing it disables AO entirely -- prefer `m_FirstPass` for per-eye reset.
    pub m_EnableTemporalFilter: bool,
    pub m_EnableDisocclusionTest: bool,
    pub m_EnableDeltaWeighting: bool,
    /// The per-eye reset lever. When set, it skips the history resolve and weights the temporal shader
    /// fully toward the current AO; the render block clears it after the first apply. Forcing it back
    /// on before each eye makes every dispatch compute fresh AO with no cross-eye history blend.
    pub m_FirstPass: bool,
    _field_9b4: [u8; 236],
}
fn _SSAOPass_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xAA0], SSAOPass>([0u8; 0xAA0]);
    }
    unreachable!()
}
impl SSAOPass {
    pub const Draw_ADDRESS: usize = 0x1401A3ED0;
    /// Sets up the render context and enqueues the AO render block onto the active draw list.
    pub unsafe fn Draw(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Draw_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<SSAOPass> for SSAOPass {
    fn as_ref(&self) -> &SSAOPass {
        self
    }
}
impl std::convert::AsMut<SSAOPass> for SSAOPass {
    fn as_mut(&mut self) -> &mut SSAOPass {
        self
    }
}
