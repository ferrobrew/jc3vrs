#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The screen-space ambient-occlusion render pass.
///
/// It maintains a two-slot temporal AO history, blended in when `m_EnableTemporalFilter` is set. The
/// history index advances once per render-block draw (an inlined `SetNextHistoryBuffer` at the end of
/// the apply), not in the per-frame list rotation.
///
/// The render block's final composite draw lives inside the `m_EnableTemporalFilter` branch, so
/// clearing that flag disables the whole apply (no AO at all). `m_FirstPass` is the engine's "no valid
/// history, compute fresh" lever: it skips the history resolve and weights the temporal shader fully
/// toward the current AO. The render block clears it after the first apply.
pub struct SSAOPass {
    _field_0: [u8; 2464],
    /// The previously-resolved slot of the two-slot temporal AO history; the resolve samples it. It
    /// advances once per draw (the inlined `SetNextHistoryBuffer`), independent of `m_FirstPass`.
    pub m_PrevFrameIndex: u32,
    /// The current slot the resolve writes and the final composite samples; advances with
    /// [`m_PrevFrameIndex`](SSAOPass::m_PrevFrameIndex).
    pub m_CurrFrameIndex: u32,
    _field_9a8: [u8; 8],
    /// Enables the temporal filter, blending the current AO against the previous frame's result
    /// through the history ping-pong. Defaults on. Also gates the render block's final composite draw,
    /// so clearing it disables AO entirely -- prefer `m_FirstPass` to reset the history.
    pub m_EnableTemporalFilter: bool,
    pub m_EnableDisocclusionTest: bool,
    pub m_EnableDeltaWeighting: bool,
    /// The "no valid history" lever. When set, it skips the history resolve and weights the temporal
    /// shader fully toward the current AO; the render block clears it after the first apply.
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
