#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// SSAO render pass. Maintains a two-slot temporal AO history (m_AO_HistoryBufferTexture[2] /
/// m_TemporalFilterRenderSetup[2], selected by m_PrevFrameIndex / m_CurrFrameIndex and advanced by
/// SetNextHistoryBuffer) that is blended in when m_EnableTemporalFilter is set.
pub struct SSAOPass {
    _field_0: [u8; 2480],
    /// Enables the temporal filter: the current AO is blended against the previous frame's result
    /// through the history ping-pong. Set by the constructor (defaults on). The history advances once
    /// per pass, so the filter assumes the pass runs once per frame.
    pub m_EnableTemporalFilter: bool,
    _field_9b1: [u8; 239],
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
