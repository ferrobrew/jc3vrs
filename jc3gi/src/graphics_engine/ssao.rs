#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// SSAO render pass. Maintains a two-slot temporal AO history (m_AO_HistoryBufferTexture[2] /
/// m_TemporalFilterRenderSetup[2], selected by m_PrevFrameIndex / m_CurrFrameIndex and advanced by
/// SetNextHistoryBuffer) that is blended in when m_EnableTemporalFilter is set.
///
/// The history index advances once per CRenderBlockSSAO::Draw (inlined SetNextHistoryBuffer at the
/// end of the apply), not in RotateRenderFrameData -- so a twice-per-frame stereo render double-steps
/// the history and blends each eye's AO against the other's. The render block's final composite draw
/// lives inside `if (m_EnableTemporalFilter)`, so clearing that flag disables the whole apply (no SSAO
/// at all). The per-eye reset lever is m_FirstPass: the engine uses it to mean "no valid history,
/// compute fresh", and passes it to the temporal shader as a current-vs-history weight. Forcing it on
/// before each stereo eye keeps the apply running while making each eye compute AO from its own depth.
pub struct SSAOPass {
    _field_0: [u8; 2480],
    /// Enables the temporal filter: the current AO is blended against the previous frame's result
    /// through the history ping-pong. Set by the constructor (defaults on). Also gates the render
    /// block's final composite draw, so clearing it disables SSAO entirely -- prefer m_FirstPass for
    /// per-eye reset.
    pub m_EnableTemporalFilter: bool,
    pub m_EnableDisocclusionTest: bool,
    pub m_EnableDeltaWeighting: bool,
    /// Per-eye reset lever. On (the constructor default for the very first frame) it tells the temporal
    /// shader to weight fully toward the current AO and skip the history resolve (`!m_FirstPass &&
    /// m_Blur`); the render block clears it after the first apply. Forcing it back on before each eye
    /// makes every dispatch compute fresh AO with no cross-eye history blend.
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
    /// Sets up the SSAO render context and enqueues the SSAO render block onto the active draw list.
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
