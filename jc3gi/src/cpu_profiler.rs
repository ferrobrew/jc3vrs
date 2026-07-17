#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// The engine's fixed 15-phase CPU frame profiler (`g_CpuProfiler`): phase brackets record QPC
/// begin/end pairs into a triple-buffered counter ring, and a once-per-frame update converts the
/// completed ring slot into per-phase milliseconds with a rolling 30-frame peak.
///
/// In the release build the machinery is dead: [`Update`](CpuProfiler::Update) is compiled to an
/// empty function, and the phase brackets that write [`m_Counters`](CpuProfiler::m_Counters) are
/// compiled out of `CGame::Update` and the game states, so every field stays zero. The consumers
/// remain -- `CBorkReport`'s telemetry serialises [`m_Time`](CpuProfiler::m_Time) via
/// [`GetScopeName`](CpuProfiler::GetScopeName), and `CGraphicsEngine::DispatchDraw` forwards
/// [`m_Index`](CpuProfiler::m_Index) to the draw task -- but they only ever read zeros.
pub struct CpuProfiler {
    /// The last completed frame's milliseconds per phase, converted from
    /// [`m_Counters`](CpuProfiler::m_Counters) by [`Update`](CpuProfiler::Update).
    pub m_Time: [f32; 15],
    /// The rolling peak per phase over the 30-frame peak window.
    pub m_Peak: [f32; 15],
    /// The counter ring position; [`Update`](CpuProfiler::Update) reads slot `(m_Index + 1) % 3`.
    pub m_Index: i32,
    _field_7c: [u8; 4],
    /// QPC begin/end pairs, per phase, per ring slot: `[phase][ring slot][begin, end]`.
    pub m_Counters: [[[u64; 2]; 3]; 15],
    /// The running peak within the current 30-frame window, promoted into
    /// [`m_Peak`](CpuProfiler::m_Peak) when the window rolls.
    pub m_LocalPeak: [f32; 15],
    _field_38c: [u8; 4],
}
fn _CpuProfiler_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x390], CpuProfiler>([0u8; 0x390]);
    }
    unreachable!()
}
impl CpuProfiler {
    pub unsafe fn get() -> Option<&'static mut Self> {
        unsafe {
            let ptr: *mut Self = *(5417315824usize as *mut *mut Self);
            ptr.as_mut()
        }
    }
}
impl CpuProfiler {
    pub const Update_ADDRESS: usize = 0x140062490;
    /// Converts the completed ring slot's QPC deltas into [`m_Time`](CpuProfiler::m_Time)
    /// milliseconds and rolls the peak window. Compiled to an empty function in the release build;
    /// the once-per-frame call at the top of [`Update`](game::Game::Update) remains.
    pub unsafe fn Update(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Update_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const GetScopeName_ADDRESS: usize = 0x1400624A0;
    /// The phase's display name from the static 15-entry name table at `0x142D3A150`
    /// (`"FRAME"`, `"DRAW"`, `"UPDATE_ALL"`, ...). Does not read `self`.
    pub unsafe fn GetScopeName(&self, id: crate::cpu_profiler::CpuScopeId) -> *const u8 {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                id: crate::cpu_profiler::CpuScopeId,
            ) -> *const u8 = ::std::mem::transmute(Self::GetScopeName_ADDRESS);
            f(self as *const Self as _, id)
        }
    }
}
impl std::convert::AsRef<CpuProfiler> for CpuProfiler {
    fn as_ref(&self) -> &CpuProfiler {
        self
    }
}
impl std::convert::AsMut<CpuProfiler> for CpuProfiler {
    fn as_mut(&mut self) -> &mut CpuProfiler {
        self
    }
}
#[repr(i32)]
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The fixed frame-phase ids of the engine's CPU profiler ([`CpuProfiler`]). Each phase is one
/// slot in the profiler's per-frame time and counter arrays; the names come from the static table
/// behind [`GetScopeName`](CpuProfiler::GetScopeName).
pub enum CpuScopeId {
    CPU_SCOPE_ID_FRAME = 0isize as _,
    CPU_SCOPE_ID_DRAW = 1isize as _,
    CPU_SCOPE_ID_UPDATE_ALL = 2isize as _,
    CPU_SCOPE_ID_RENDER_ALL = 3isize as _,
    CPU_SCOPE_ID_PREUPDATE = 4isize as _,
    CPU_SCOPE_ID_PRESIM_SYSTEMS = 5isize as _,
    CPU_SCOPE_ID_PRESIM_OBJECTS = 6isize as _,
    CPU_SCOPE_ID_PHYSICS_UPDATE = 7isize as _,
    CPU_SCOPE_ID_POSTSIM_UPDATE = 8isize as _,
    CPU_SCOPE_ID_POSTSIM_SYSTEMS = 9isize as _,
    CPU_SCOPE_ID_WAIT_FRAME = 10isize as _,
    CPU_SCOPE_ID_RENDER_UPDATE = 11isize as _,
    CPU_SCOPE_ID_RENDER_SYSTEMS = 12isize as _,
    CPU_SCOPE_ID_WAIT_FLIP = 13isize as _,
    CPU_SCOPE_ID_WAIT_UI = 14isize as _,
    CPU_SCOPE_ID_COUNT = 15isize as _,
}
fn _CpuScopeId_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x4], CpuScopeId>([0u8; 0x4]);
    }
    unreachable!()
}
