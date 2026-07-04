#![cfg_attr(any(), rustfmt::skip)]
#[allow(unused_imports)]
use crate::ui::ui_manager::UIManager;
#[repr(C, align(8))]
/// A node in the display tree returned by `Movie::GetDisplayObjectsTree`. Inherits from
/// [`RefCountImpl`] and embeds a `Scaleform::String` for the name and a
/// `Scaleform::ArrayDataBase<Ptr<AmpMovieObjectDesc>>` for children.
///
/// Layout verified from `GetChildDescTree` at `0x141_A30_410`.
pub struct AmpMovieObjectDesc {
    pub ref_count_impl: crate::ui::scaleform::RefCountImpl,
    /// The clip's instance name, or `"Unnamed"`: a `Scaleform::String`, i.e. a pointer to its
    /// `DataDesc` header (`Size: u64`, `RefCount: i32`), with the NUL-terminated characters
    /// inline at `+0xC`.
    pub name: *const u8,
    /// Pointer to the child array. Each element is a `Ptr<AmpMovieObjectDesc>` (a raw pointer
    /// to another `AmpMovieObjectDesc`).
    pub children: *mut *mut crate::ui::scaleform::AmpMovieObjectDesc,
    /// Number of children in [`children`](AmpMovieObjectDesc::children).
    pub child_count: u64,
    /// Allocated capacity of [`children`](AmpMovieObjectDesc::children).
    pub child_capacity: u64,
}
fn _AmpMovieObjectDesc_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], AmpMovieObjectDesc>([0u8; 0x30]);
    }
    unreachable!()
}
impl AmpMovieObjectDesc {
    pub fn vftable(&self) -> *const crate::ui::scaleform::RefCountImplVftable {
        self.ref_count_impl.vftable() as *const crate::ui::scaleform::RefCountImplVftable
    }
    /// Increments the reference count atomically.
    pub unsafe fn AddRef(&self) {
        unsafe { self.ref_count_impl.AddRef() }
    }
    /// Decrements the reference count atomically. When it reaches zero, calls the virtual
    /// destructor (vtable slot 0).
    pub unsafe fn Release(&self) {
        unsafe { self.ref_count_impl.Release() }
    }
    /// Virtual destructor, slot 0. Called by `Release` when the refcount reaches zero.
    pub unsafe fn destructor(&mut self, flags: u32) {
        unsafe {
            let f = (&raw const (*self.vftable()).destructor).read();
            f(self as *mut Self as _, flags)
        }
    }
}
impl std::convert::AsRef<crate::ui::scaleform::RefCountImpl> for AmpMovieObjectDesc {
    fn as_ref(&self) -> &crate::ui::scaleform::RefCountImpl {
        &self.ref_count_impl
    }
}
impl std::convert::AsMut<crate::ui::scaleform::RefCountImpl> for AmpMovieObjectDesc {
    fn as_mut(&mut self) -> &mut crate::ui::scaleform::RefCountImpl {
        &mut self.ref_count_impl
    }
}
impl std::convert::AsRef<AmpMovieObjectDesc> for AmpMovieObjectDesc {
    fn as_ref(&self) -> &AmpMovieObjectDesc {
        self
    }
}
impl std::convert::AsMut<AmpMovieObjectDesc> for AmpMovieObjectDesc {
    fn as_mut(&mut self) -> &mut AmpMovieObjectDesc {
        self
    }
}
#[derive(Default)]
#[repr(C, align(16))]
/// A display object's presentation state, read and written through
/// [`ValueObjectInterface::GetDisplayInfo`] / [`SetDisplayInfo`](ValueObjectInterface::SetDisplayInfo).
/// [`VarsSet`](DisplayInfo::VarsSet) selects which fields a write applies.
pub struct DisplayInfo {
    pub X: f64,
    pub Y: f64,
    pub Rotation: f64,
    pub XScale: f64,
    pub YScale: f64,
    pub Alpha: f64,
    pub Z: f64,
    pub XRotation: f64,
    pub YRotation: f64,
    pub ZScale: f64,
    pub FOV: f64,
    _field_58: [u8; 8],
    /// `Render::Matrix3x4<float>` (3D view matrix).
    pub ViewMatrix3D: [f32; 12],
    /// `Render::Matrix4x4<float>` (3D projection matrix).
    pub ProjectionMatrix3D: [f32; 16],
    /// `Render::EdgeAAMode`.
    pub EdgeAAMode: u32,
    /// Which fields a [`SetDisplayInfo`](ValueObjectInterface::SetDisplayInfo) call applies
    /// (`V_*` bits; `0x40` = visible).
    pub VarsSet: u16,
    pub Visible: bool,
    _field_d7: [u8; 9],
}
fn _DisplayInfo_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xE0], DisplayInfo>([0u8; 0xE0]);
    }
    unreachable!()
}
impl DisplayInfo {}
impl DisplayInfo {
    /// The [`VarsSet`](DisplayInfo::VarsSet) bit selecting [`Visible`](DisplayInfo::Visible).
    pub const V_VISIBLE: u32 = 64;
}
impl std::convert::AsRef<DisplayInfo> for DisplayInfo {
    fn as_ref(&self) -> &DisplayInfo {
        self
    }
}
impl std::convert::AsMut<DisplayInfo> for DisplayInfo {
    fn as_mut(&mut self) -> &mut DisplayInfo {
        self
    }
}
#[repr(C, align(1))]
/// A Scaleform `MemoryHeap`. Opaque; allocation goes through its vtable (`Alloc` at slot offset
/// `0x50`).
pub struct MemoryHeap {
    _field_0: [u8; 8],
}
fn _MemoryHeap_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], MemoryHeap>([0u8; 0x8]);
    }
    unreachable!()
}
impl MemoryHeap {}
impl std::convert::AsRef<MemoryHeap> for MemoryHeap {
    fn as_ref(&self) -> &MemoryHeap {
        self
    }
}
impl std::convert::AsMut<MemoryHeap> for MemoryHeap {
    fn as_mut(&mut self) -> &mut MemoryHeap {
        self
    }
}
#[repr(C, align(8))]
/// The Scaleform AS3 `MovieRoot` (the `ASMovieRootBase` interface), which `CUIManager::m_Movie`
/// points at. The engine drives the movie through this interface's virtuals; the bound
/// [`SetVariable`](Movie::SetVariable) / [`GetVariable`](Movie::GetVariable) /
/// [`Invoke`](Movie::Invoke) are its concrete implementations. The vtable is `MovieRoot`'s at
/// `0x142_621_780`.
pub struct Movie {
    vftable: *const crate::ui::scaleform::MovieVftable,
    _field_8: [u8; 8],
    /// The backing `GFx::MovieImpl` (`GetDisplayObjectsTree` reads `pMovieImpl->pMainMovie`
    /// through it).
    pub pMovieImpl: *mut crate::ui::scaleform::MovieImpl,
}
fn _Movie_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], Movie>([0u8; 0x18]);
    }
    unreachable!()
}
impl Movie {
    pub fn vftable(&self) -> *const crate::ui::scaleform::MovieVftable {
        self.vftable as *const crate::ui::scaleform::MovieVftable
    }
    pub const SetVariable_ADDRESS: usize = 0x141C47FA0;
    /// Sets an AS3 variable by clip path. The path is a dot-separated string like
    /// `"MCI_hud.MCI_poi_stage._visible"`, resolved from the root timeline. `SetVarType` controls
    /// persistence across clip re-creation (0 = normal). Call on the capture (game update)
    /// thread.
    pub unsafe fn SetVariable(
        &self,
        path: *const u8,
        value: *const crate::ui::scaleform::Value,
        set_var_type: u32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                path: *const u8,
                value: *const crate::ui::scaleform::Value,
                set_var_type: u32,
            ) -> bool = ::std::mem::transmute(Self::SetVariable_ADDRESS);
            f(self as *const Self as _, path, value, set_var_type)
        }
    }
    pub const GetVariable_ADDRESS: usize = 0x141C47DF0;
    /// Reads an AS3 variable by clip path into a [`Value`]. Returns false if the path is not
    /// found. A returned managed value (display object, string) must be released through the
    /// movie's object interface; plain bool/int/number values need no cleanup.
    pub unsafe fn GetVariable(
        &self,
        value: *mut crate::ui::scaleform::Value,
        path: *const u8,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                value: *mut crate::ui::scaleform::Value,
                path: *const u8,
            ) -> bool = ::std::mem::transmute(Self::GetVariable_ADDRESS);
            f(self as *const Self as _, value, path)
        }
    }
    pub const Invoke_ADDRESS: usize = 0x141C49CD0;
    /// Invokes an AS3 method on the movie's root timeline by name. This is the call path used by
    /// the engine's `CUIBase::Invoke`. The `args` are a `GFx::Value` array; `result` receives the
    /// return value (or null). Note the parameter order: `result` comes *before* `args`, unlike
    /// the engine's `CUIBase::Invoke` wrapper (which also adds a timeout).
    pub unsafe fn Invoke(
        &self,
        method_name: *const u8,
        result: *mut ::std::ffi::c_void,
        args: *const ::std::ffi::c_void,
        num_args: u32,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                method_name: *const u8,
                result: *mut ::std::ffi::c_void,
                args: *const ::std::ffi::c_void,
                num_args: u32,
            ) -> bool = ::std::mem::transmute(Self::Invoke_ADDRESS);
            f(self as *const Self as _, method_name, result, args, num_args)
        }
    }
    /// Recursively walks the runtime display tree from the root movie clip: returns a tree of
    /// [`AmpMovieObjectDesc`] nodes, each carrying the clip's instance name (or `"Unnamed"`)
    /// and a child array, calling `DisplayObject::GetName` on every child and recursing into
    /// containers. `heap` is the Scaleform `MemoryHeap*` used for the allocations (e.g.
    /// [`MovieImpl::pHeap`]); the tree is freed by releasing the root node. Slot 35 of the
    /// `ASMovieRootBase` vtable; the implementation is at `0x141_BED_530`. Call on the capture
    /// (game update) thread, where the display tree is stable.
    pub unsafe fn GetDisplayObjectsTree(
        &self,
        heap: *mut crate::ui::scaleform::MemoryHeap,
    ) -> *mut crate::ui::scaleform::AmpMovieObjectDesc {
        unsafe {
            let f = (&raw const (*self.vftable()).GetDisplayObjectsTree).read();
            f(self as *const Self as _, heap)
        }
    }
}
impl Movie {
    /// The `AS3::MovieRoot` vtable address (the `ASMovieRootBase` layout: slot 35 is
    /// [`GetDisplayObjectsTree`](Movie::GetDisplayObjectsTree), slots 49/50/57 are the bound
    /// SetVariable/GetVariable/Invoke). The payload checks a live object against it before
    /// trusting the vtable-indexed calls, since the object's dynamic type is load-bearing here.
    pub const VFTABLE: u64 = 5408691888;
}
impl std::convert::AsRef<Movie> for Movie {
    fn as_ref(&self) -> &Movie {
        self
    }
}
impl std::convert::AsMut<Movie> for Movie {
    fn as_mut(&mut self) -> &mut Movie {
        self
    }
}
#[repr(C, align(8))]
/// The `GFx::MovieImpl` behind [`UIManager::m_Movie`](ui::ui_manager::UIManager::m_Movie): the
/// `GFx::Movie` instance. The rendering-side virtuals live on its own vtable; the AS3 side
/// (SetVariable, Invoke, the display tree) lives on [`pASMovieRoot`](MovieImpl::pASMovieRoot).
pub struct MovieImpl {
    vftable: *const crate::ui::scaleform::MovieImplVftable,
    _field_8: [u8; 16],
    /// The AS3 `MovieRoot` (the [`Movie`] interface): SetVariable, Invoke, and the display tree.
    pub pASMovieRoot: *mut crate::ui::scaleform::Movie,
    _field_20: [u8; 24],
    /// The `GFx::Value::ObjectInterface` for this movie: the dispatcher every `GFx::Value`
    /// display-object/member operation goes through.
    pub pObjectInterface: *mut ::std::ffi::c_void,
    /// The Scaleform `MemoryHeap` the movie allocates from.
    pub pHeap: *mut crate::ui::scaleform::MemoryHeap,
    _field_48: [u8; 8],
    /// The root `DisplayObjContainer` (the main movie clip).
    pub pMainMovie: *mut ::std::ffi::c_void,
    _field_58: [u8; 21240],
    /// The movie's embedded `Render::Context` (the snapshot pipeline: active/pending/displaying
    /// snapshots, the capture locks, and the once-a-frame consumption latch).
    pub RenderContext: crate::ui::scaleform::RenderContext,
}
fn _MovieImpl_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x53F0], MovieImpl>([0u8; 0x53F0]);
    }
    unreachable!()
}
impl MovieImpl {
    pub fn vftable(&self) -> *const crate::ui::scaleform::MovieImplVftable {
        self.vftable as *const crate::ui::scaleform::MovieImplVftable
    }
    pub const CaptureImpl_ADDRESS: usize = 0x14198B7D0;
    /// The concrete implementation behind the [`Capture`](MovieImpl::Capture) virtual (vtable
    /// slot 25): gates on `Context::HasChanges` when `if_changed` is set, suspends the GC, and
    /// runs `Render::Context::Capture` to publish the frame's display-tree changes as the pending
    /// snapshot. `CUIManager::PreRender` calls it right after `Advance`, on the game update
    /// thread with the deferred render lock held -- the only point in the frame where every
    /// display-tree writer is quiescent, which makes it the seam for pre-capture display-list
    /// edits.
    pub unsafe fn CaptureImpl(&mut self, if_changed: bool) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, if_changed: bool) -> u64 = ::std::mem::transmute(
                Self::CaptureImpl_ADDRESS,
            );
            f(self as *mut Self as _, if_changed)
        }
    }
    /// Snapshots the display tree for the render thread (`GFx::Movie` vtable slot 25). Must be
    /// called from the current capture thread (see
    /// [`SetCaptureThread`](MovieImpl::SetCaptureThread)); `if_changed = false` forces a fresh
    /// snapshot. The next `RTHandle::NextCapture` on the render side picks it up.
    pub unsafe fn Capture(&mut self, if_changed: bool) -> u64 {
        unsafe {
            let f = (&raw const (*self.vftable()).Capture).read();
            f(self as *mut Self as _, if_changed)
        }
    }
    /// Reassigns which thread owns the capture (`GFx::Movie` vtable slot 27). The engine hands
    /// ownership between the update thread and the render thread this way
    /// (`RenderOffScreenTextures` does exactly this per frame).
    pub unsafe fn SetCaptureThread(&mut self, thread_id: u32) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetCaptureThread).read();
            f(self as *mut Self as _, thread_id)
        }
    }
}
impl std::convert::AsRef<MovieImpl> for MovieImpl {
    fn as_ref(&self) -> &MovieImpl {
        self
    }
}
impl std::convert::AsMut<MovieImpl> for MovieImpl {
    fn as_mut(&mut self) -> &mut MovieImpl {
        self
    }
}
#[repr(C, align(8))]
pub struct MovieImplVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_12: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_14: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_15: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_16: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_19: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_20: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_21: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_22: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_23: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_24: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    /// Snapshots the display tree for the render thread (`GFx::Movie` vtable slot 25). Must be
    /// called from the current capture thread (see
    /// [`SetCaptureThread`](MovieImpl::SetCaptureThread)); `if_changed = false` forces a fresh
    /// snapshot. The next `RTHandle::NextCapture` on the render side picks it up.
    pub Capture: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        if_changed: bool,
    ) -> u64,
    _vfunc_26: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    /// Reassigns which thread owns the capture (`GFx::Movie` vtable slot 27). The engine hands
    /// ownership between the update thread and the render thread this way
    /// (`RenderOffScreenTextures` does exactly this per frame).
    pub SetCaptureThread: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        thread_id: u32,
    ),
}
fn _MovieImplVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xE0], MovieImplVftable>([0u8; 0xE0]);
    }
    unreachable!()
}
impl MovieImplVftable {}
impl std::convert::AsRef<MovieImplVftable> for MovieImplVftable {
    fn as_ref(&self) -> &MovieImplVftable {
        self
    }
}
impl std::convert::AsMut<MovieImplVftable> for MovieImplVftable {
    fn as_mut(&mut self) -> &mut MovieImplVftable {
        self
    }
}
#[repr(C, align(8))]
pub struct MovieVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_3: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_4: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_5: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_11: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_12: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_14: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_15: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_16: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_19: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_20: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_21: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_22: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_23: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_24: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_25: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_26: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_27: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_28: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_29: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_30: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_31: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_32: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_33: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    _vfunc_34: unsafe extern "system" fn(this: *mut crate::ui::scaleform::Movie),
    /// Recursively walks the runtime display tree from the root movie clip: returns a tree of
    /// [`AmpMovieObjectDesc`] nodes, each carrying the clip's instance name (or `"Unnamed"`)
    /// and a child array, calling `DisplayObject::GetName` on every child and recursing into
    /// containers. `heap` is the Scaleform `MemoryHeap*` used for the allocations (e.g.
    /// [`MovieImpl::pHeap`]); the tree is freed by releasing the root node. Slot 35 of the
    /// `ASMovieRootBase` vtable; the implementation is at `0x141_BED_530`. Call on the capture
    /// (game update) thread, where the display tree is stable.
    pub GetDisplayObjectsTree: unsafe extern "system" fn(
        this: *const crate::ui::scaleform::Movie,
        heap: *mut crate::ui::scaleform::MemoryHeap,
    ) -> *mut crate::ui::scaleform::AmpMovieObjectDesc,
}
fn _MovieVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x120], MovieVftable>([0u8; 0x120]);
    }
    unreachable!()
}
impl MovieVftable {}
impl std::convert::AsRef<MovieVftable> for MovieVftable {
    fn as_ref(&self) -> &MovieVftable {
        self
    }
}
impl std::convert::AsMut<MovieVftable> for MovieVftable {
    fn as_mut(&mut self) -> &mut MovieVftable {
        self
    }
}
#[repr(C, align(8))]
/// Scaleform `RefCountImpl` -- the base class for reference-counted Scaleform objects. Provides
/// a vtable pointer at +0 and an atomic refcount at +8. `AddRef`/`Release` are non-virtual;
/// the vtable's slot 0 is the destructor, called when the refcount reaches zero.
pub struct RefCountImpl {
    vftable: *const crate::ui::scaleform::RefCountImplVftable,
    /// Atomic reference count. Incremented by `AddRef`, decremented by `Release`.
    pub ref_count: u32,
    _field_c: [u8; 4],
}
fn _RefCountImpl_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x10], RefCountImpl>([0u8; 0x10]);
    }
    unreachable!()
}
impl RefCountImpl {
    pub fn vftable(&self) -> *const crate::ui::scaleform::RefCountImplVftable {
        self.vftable as *const crate::ui::scaleform::RefCountImplVftable
    }
    pub const AddRef_ADDRESS: usize = 0x141998430;
    /// Increments the reference count atomically.
    pub unsafe fn AddRef(&self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) = ::std::mem::transmute(
                Self::AddRef_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const Release_ADDRESS: usize = 0x141998440;
    /// Decrements the reference count atomically. When it reaches zero, calls the virtual
    /// destructor (vtable slot 0).
    pub unsafe fn Release(&self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) = ::std::mem::transmute(
                Self::Release_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    /// Virtual destructor, slot 0. Called by `Release` when the refcount reaches zero.
    pub unsafe fn destructor(&mut self, flags: u32) {
        unsafe {
            let f = (&raw const (*self.vftable()).destructor).read();
            f(self as *mut Self as _, flags)
        }
    }
}
impl std::convert::AsRef<RefCountImpl> for RefCountImpl {
    fn as_ref(&self) -> &RefCountImpl {
        self
    }
}
impl std::convert::AsMut<RefCountImpl> for RefCountImpl {
    fn as_mut(&mut self) -> &mut RefCountImpl {
        self
    }
}
#[repr(C, align(8))]
pub struct RefCountImplVftable {
    /// Virtual destructor, slot 0. Called by `Release` when the refcount reaches zero.
    pub destructor: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RefCountImpl,
        flags: u32,
    ),
}
fn _RefCountImplVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], RefCountImplVftable>([0u8; 0x8]);
    }
    unreachable!()
}
impl RefCountImplVftable {}
impl std::convert::AsRef<RefCountImplVftable> for RefCountImplVftable {
    fn as_ref(&self) -> &RefCountImplVftable {
        self
    }
}
impl std::convert::AsMut<RefCountImplVftable> for RefCountImplVftable {
    fn as_mut(&mut self) -> &mut RefCountImplVftable {
        self
    }
}
#[repr(C, align(8))]
/// The Scaleform `Render::ContextImpl::Context`: the display-tree snapshot pipeline between the
/// update thread and the renderer. `Capture` (update thread) merges the active snapshot's changes
/// into the pending snapshot; `RTHandle::NextCapture` (render thread) consumes the pending
/// snapshot into the displaying one, at most once per HAL frame. Only the fields the payload
/// touches are modeled; the declared size covers just the modeled prefix.
pub struct RenderContext {
    _field_0: [u8; 153],
    /// The once-a-frame consumption latch: while set, `RTHandle::NextCapture` keeps the current
    /// displaying snapshot instead of consuming the pending one. Set by the first `NextCapture`
    /// of a HAL frame and cleared by `HAL::EndFrame` (`EndFrameContextNotify`), so within one
    /// `CUIManager::Render` call every draw sees the same snapshot.
    pub NextCaptureCalledInFrame: bool,
    _field_9a: [u8; 6],
}
fn _RenderContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xA0], RenderContext>([0u8; 0xA0]);
    }
    unreachable!()
}
impl RenderContext {}
impl std::convert::AsRef<RenderContext> for RenderContext {
    fn as_ref(&self) -> &RenderContext {
        self
    }
}
impl std::convert::AsMut<RenderContext> for RenderContext {
    fn as_mut(&mut self) -> &mut RenderContext {
        self
    }
}
#[repr(C, align(8))]
/// The Scaleform `Render::D3D1x::HAL` behind `CUIManager::m_RenderHAL`: the D3D11 rendering
/// backend the UI render worker draws through.
pub struct RenderHAL {
    _field_0: [u8; 291752],
    /// The `ID3D11Device` the HAL was initialized with.
    pub pDevice: *mut ::std::ffi::c_void,
    /// The `ID3D11DeviceContext` every HAL draw goes through (possibly a deferred context; see
    /// `UsingDeferredContext` at `0x473C0`). Work recorded on it from the UI render worker is
    /// ordered with the HAL's own draws.
    pub pDeviceContext: *mut ::std::ffi::c_void,
}
fn _RenderHAL_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x473B8], RenderHAL>([0u8; 0x473B8]);
    }
    unreachable!()
}
impl RenderHAL {}
impl std::convert::AsRef<RenderHAL> for RenderHAL {
    fn as_ref(&self) -> &RenderHAL {
        self
    }
}
impl std::convert::AsMut<RenderHAL> for RenderHAL {
    fn as_mut(&mut self) -> &mut RenderHAL {
        self
    }
}
#[repr(C, align(8))]
/// Static clip names placed on the root timeline of `ui/hud.gfx` via PlaceObject2 tags. The root
/// movie (`ui/root.gfx`) creates its clips entirely through ActionScript, so it has no static
/// PlaceObject2 tags; `hud.gfx` uses a mix of static placement and dynamic creation.
///
/// | Instance name | Depth | Char ID |
/// |---|---|---|
/// | `MCI_hud` | 1432 | 1548 |
/// | `MCI_poi_stage` | 180 | 1066 |
/// | `MCI_weapon_selection_wheel` | 182 | 1078 |
/// | `MCI_safe_area_center` | 1 | 1065 |
/// | `MCI_safe_area_top_left` | 839 | 1538 |
/// | `MCI_safe_area_top_middle` | 636 | 1532 |
/// | `MCI_safe_area_top_right` | 562 | 1395 |
/// | `MCI_safe_area_bottom_left` | 450 | 1330 |
/// | `MCI_safe_area_bottom_middle` | 307 | 1286 |
/// | `MCI_safe_area_bottom_right` | 246 | 1161 |
///
/// UI movie registry from `root.gfx` maps each `CUIBase` subclass to its `.gfx` file:
/// `CSharedLibUI` -> `shared_lib`, `COverlayUI` -> `overlay`, `CHUDUI` -> `hud`, `CIntroUI` ->
/// `intro`, `CTitleUI` -> `title`, `CPauseUI` -> `pause`, `CTutorialsUI` -> `tutorials`,
/// `CCreditsUI` -> `credits`, `CLobbyUI` -> `lobby`, `CMainUI` -> `main`, `CCommLinkUI` ->
/// `comm_link`, `CCommCollectiblesUI` -> `comm_collectibles`, `CCommCommunityUI` ->
/// `comm_community`, `CCommMapUI` -> `comm_map`, `CCommSkillUI` -> `comm_skill`,
/// `CCommStatsUI` -> `comm_stats`, `CCommBragsFeatsUI` -> `comm_brags_feats`,
/// `CCommStoreUI` -> `comm_store`, `CRewardUI` -> `reward`, `CROMUI` -> (none).
pub struct ScaleformInfo {}
impl ScaleformInfo {}
impl std::convert::AsRef<ScaleformInfo> for ScaleformInfo {
    fn as_ref(&self) -> &ScaleformInfo {
        self
    }
}
impl std::convert::AsMut<ScaleformInfo> for ScaleformInfo {
    fn as_mut(&mut self) -> &mut ScaleformInfo {
        self
    }
}
#[repr(C, align(8))]
/// A Scaleform `GFx::Value`: the tagged union the AS3 interface traffics in. Starts with a
/// `ListNode<Value>` (managed values are tracked on the movie's `ExternalObjRefs` list); a
/// stack-constructed unmanaged value leaves the list pointers and
/// [`pObjectInterface`](Value::pObjectInterface) null.
pub struct Value {
    /// `ListNode<Value>::pPrev`; null for unmanaged values.
    pub pPrev: *mut crate::ui::scaleform::Value,
    /// `ListNode<Value>::pNext`; null for unmanaged values.
    pub pNext: *mut crate::ui::scaleform::Value,
    /// The owning movie's object interface; null for unmanaged values.
    pub pObjectInterface: *mut crate::ui::scaleform::ValueObjectInterface,
    /// The `ValueType` tag, possibly with `VTC_*` control bits (so not modeled as an enum).
    pub Type: u32,
    _field_1c: [u8; 4],
    /// The value union: `bool` / `i32` / `u32` / `f64` / `*const c_char` / object pointer,
    /// selected by [`Type`](Value::Type).
    pub mValue: u64,
    /// Auxiliary data (e.g. the closure's user pointer).
    pub DataAux: u64,
}
fn _Value_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x30], Value>([0u8; 0x30]);
    }
    unreachable!()
}
impl Value {}
impl Value {
    /// The managed bit: set on values owned by the movie (which must be released through the
    /// object interface, never constructed by hand).
    pub const VTC_MANAGED_BIT: u32 = 64;
    /// The type tag for a boolean value ([`mValue`](Value::mValue) carries the `bool` in its
    /// first byte).
    pub const VT_BOOLEAN: u32 = 2;
    /// The type tag for a display-object value (managed; owned by the movie).
    pub const VT_DISPLAY_OBJECT: u32 = 10;
    /// The type tag for an int value.
    pub const VT_INT: u32 = 3;
    /// The type tag for a number (f64) value.
    pub const VT_NUMBER: u32 = 5;
    /// The type tag for a string value (`mValue` is a `*const c_char`).
    pub const VT_STRING: u32 = 6;
}
impl std::convert::AsRef<Value> for Value {
    fn as_ref(&self) -> &Value {
        self
    }
}
impl std::convert::AsMut<Value> for Value {
    fn as_mut(&mut self) -> &mut Value {
        self
    }
}
#[repr(C, align(1))]
/// The `GFx::Value::ObjectInterface`: the dispatcher for operations on managed values (display
/// objects, arrays). One per movie ([`MovieImpl::pObjectInterface`]); a managed [`Value`] carries
/// its owning interface in [`Value::pObjectInterface`].
pub struct ValueObjectInterface {
    _field_0: [u8; 8],
}
fn _ValueObjectInterface_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], ValueObjectInterface>([0u8; 0x8]);
    }
    unreachable!()
}
impl ValueObjectInterface {
    pub const GetDisplayInfo_ADDRESS: usize = 0x141BB8690;
    /// Reads a display object's `DisplayInfo` (position, scale, alpha, visibility). `data` is the
    /// value's [`mValue`](Value::mValue) payload. Returns false when the value is not a display
    /// object.
    pub unsafe fn GetDisplayInfo(
        &mut self,
        data: *mut ::std::ffi::c_void,
        info: *mut crate::ui::scaleform::DisplayInfo,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                data: *mut ::std::ffi::c_void,
                info: *mut crate::ui::scaleform::DisplayInfo,
            ) -> bool = ::std::mem::transmute(Self::GetDisplayInfo_ADDRESS);
            f(self as *mut Self as _, data, info)
        }
    }
    pub const SetDisplayInfo_ADDRESS: usize = 0x141BAEF00;
    /// Writes the `DisplayInfo` fields selected by [`DisplayInfo::VarsSet`] directly at the
    /// display-object level -- no AVM path resolution and no AS3 property setters, which is what
    /// makes it suitable for high-frequency writes (the game's own `CHUDUI::UpdatePOIs` drives
    /// POI positions and visibility through it every frame).
    pub unsafe fn SetDisplayInfo(
        &mut self,
        data: *mut ::std::ffi::c_void,
        info: *const crate::ui::scaleform::DisplayInfo,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                data: *mut ::std::ffi::c_void,
                info: *const crate::ui::scaleform::DisplayInfo,
            ) -> bool = ::std::mem::transmute(Self::SetDisplayInfo_ADDRESS);
            f(self as *mut Self as _, data, info)
        }
    }
    pub const ObjectRelease_ADDRESS: usize = 0x141BC8D70;
    /// Releases a managed value: unlinks it from the movie's external-references list and drops
    /// the AS3 object reference. Call on the capture thread. `data` is the value's
    /// [`mValue`](Value::mValue) payload.
    pub unsafe fn ObjectRelease(
        &mut self,
        value: *mut crate::ui::scaleform::Value,
        data: *mut ::std::ffi::c_void,
    ) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                value: *mut crate::ui::scaleform::Value,
                data: *mut ::std::ffi::c_void,
            ) = ::std::mem::transmute(Self::ObjectRelease_ADDRESS);
            f(self as *mut Self as _, value, data)
        }
    }
}
impl std::convert::AsRef<ValueObjectInterface> for ValueObjectInterface {
    fn as_ref(&self) -> &ValueObjectInterface {
        self
    }
}
impl std::convert::AsMut<ValueObjectInterface> for ValueObjectInterface {
    fn as_mut(&mut self) -> &mut ValueObjectInterface {
        self
    }
}
#[allow(dead_code)]
impl Value {
    /// An unmanaged boolean value, safe to pass to `Movie::SetVariable` (the movie copies it;
    /// nothing needs releasing).
    pub fn new_boolean(value: bool) -> Self {
        let mut v: Self = unsafe { ::core::mem::zeroed() };
        v.Type = Self::VT_BOOLEAN;
        v.mValue = value as u64;
        v
    }
}
