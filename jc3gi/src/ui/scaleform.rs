#![cfg_attr(any(), rustfmt::skip)]
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
#[repr(C, align(8))]
/// A pinned `DisplayHandle<TreeRoot>` as returned by
/// [`GetDisplayHandle`](MovieImpl::GetDisplayHandle): one refcounted `HandleData` pointer.
pub struct DisplayHandle {
    /// The refcounted `RTHandle::HandleData` (starts with `RefCountImpl`, so
    /// [`RefCountImpl::AddRef`] pins a copied handle).
    pub pData: *mut crate::ui::scaleform::RefCountImpl,
}
fn _DisplayHandle_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], DisplayHandle>([0u8; 0x8]);
    }
    unreachable!()
}
impl DisplayHandle {}
impl std::convert::AsRef<DisplayHandle> for DisplayHandle {
    fn as_ref(&self) -> &DisplayHandle {
        self
    }
}
impl std::convert::AsMut<DisplayHandle> for DisplayHandle {
    fn as_mut(&mut self) -> &mut DisplayHandle {
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
/// A Scaleform `GFx::DisplayObjectBase`: the display-side object behind a display-list entry.
/// Reached from a managed display-object [`Value`] at `mValue + 0x88` (guarded by the traits
/// check `GetDisplayInfo` performs on `mValue + 0x28`).
pub struct DisplayObjectBase {
    _field_0: [u8; 8],
}
fn _DisplayObjectBase_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], DisplayObjectBase>([0u8; 0x8]);
    }
    unreachable!()
}
impl DisplayObjectBase {
    pub const GetRenderNode_ADDRESS: usize = 0x1419EB6F0;
    /// The object's render-tree node, created lazily. Capture (game update) thread only.
    pub unsafe fn GetRenderNode(&self) -> *mut crate::ui::scaleform::TreeNode {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *mut crate::ui::scaleform::TreeNode = ::std::mem::transmute(
                Self::GetRenderNode_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<DisplayObjectBase> for DisplayObjectBase {
    fn as_ref(&self) -> &DisplayObjectBase {
        self
    }
}
impl std::convert::AsMut<DisplayObjectBase> for DisplayObjectBase {
    fn as_mut(&mut self) -> &mut DisplayObjectBase {
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
#[derive(Copy, Clone)]
#[repr(C, align(4))]
/// A `GFx::MouseEvent` (a `GFx::Event` with the mouse payload): the argument
/// `CUIManager::SendMouseEvents` builds for [`MovieImpl::HandleEvent`]. `x` and `y` are in
/// movie-viewport pixels: window-client pixels minus the centering offset
/// `(cached viewport size - movie rectangle size) / 2`, i.e. relative to the top-left corner of
/// the centered movie rectangle (see
/// [`UIManager::m_MovieScaleWidth`](ui::ui_manager::UIManager::m_MovieScaleWidth)).
pub struct MouseEvent {
    /// The `GFx::Event::Type` tag (`TYPE_MOUSE_*`).
    pub Type: u32,
    /// `GFx::KeyModifiers::States` (ctrl/alt/shift bits); the engine always sends 0.
    pub Modifiers: u8,
    _field_5: [u8; 3],
    pub x: f32,
    pub y: f32,
    /// The wheel amount in Flash line units; the engine sends `lZ / 120 * 3` from the DirectInput
    /// mouse's z axis.
    pub ScrollDelta: f32,
    /// The button index for down/up events: `0` left, `1` right.
    pub Button: u32,
    pub MouseIndex: u32,
}
fn _MouseEvent_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x1C], MouseEvent>([0u8; 0x1C]);
    }
    unreachable!()
}
impl MouseEvent {}
impl MouseEvent {
    /// The [`Type`](MouseEvent::Type) of a button-press event.
    pub const TYPE_MOUSE_DOWN: u32 = 2;
    /// The [`Type`](MouseEvent::Type) of a mouse-move event.
    pub const TYPE_MOUSE_MOVE: u32 = 1;
    /// The [`Type`](MouseEvent::Type) of a button-release event.
    pub const TYPE_MOUSE_UP: u32 = 3;
    /// The [`Type`](MouseEvent::Type) of a wheel event ([`ScrollDelta`](MouseEvent::ScrollDelta)
    /// carries the amount).
    pub const TYPE_MOUSE_WHEEL: u32 = 4;
}
impl std::convert::AsRef<MouseEvent> for MouseEvent {
    fn as_ref(&self) -> &MouseEvent {
        self
    }
}
impl std::convert::AsMut<MouseEvent> for MouseEvent {
    fn as_mut(&mut self) -> &mut MouseEvent {
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
    _field_58: [u8; 48],
    /// The movie's render-tree root ([`TreeRoot`]): the entry `CUIManager::Render` draws via the
    /// display handle. The whole display tree's render nodes hang off it.
    pub pRenderRoot: *mut crate::ui::scaleform::TreeRoot,
    _field_90: [u8; 16],
    /// The movie's `GFx::Viewport` (0x34 bytes: buffer size, view rectangle, scissor, flags,
    /// scale, and aspect). [`TreeRoot::SetViewport`] takes it directly (copying the 0x2C
    /// `Render::Viewport` prefix).
    pub Viewport: [u8; 52],
    _field_d4: [u8; 60],
    /// The stage-to-viewport matrix (`Matrix2x4<float>`: two rows of `[sx, shx, shy, sy? tx, ty]`
    /// layout) the movie sets on [`pRenderRoot`](MovieImpl::pRenderRoot) via `TreeNode::SetMatrix`
    /// whenever the viewport changes.
    pub ViewportMatrix: [f32; 8],
    _field_130: [u8; 21024],
    /// The movie's embedded `Render::Context` (the snapshot pipeline: active/pending/displaying
    /// snapshots, the capture locks, and the once-a-frame consumption latch).
    pub RenderContext: crate::ui::scaleform::RenderContext,
}
fn _MovieImpl_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x5468], MovieImpl>([0u8; 0x5468]);
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
    /// Returns the movie's display handle (`GFx::Movie` vtable slot 26): the pinned
    /// `DisplayHandle<TreeRoot>` whose `pData` the renderer copies (with an `AddRef`) into a
    /// stack [`RTHandle`] before consuming the frame's capture.
    pub unsafe fn GetDisplayHandle(
        &mut self,
    ) -> *mut crate::ui::scaleform::DisplayHandle {
        unsafe {
            let f = (&raw const (*self.vftable()).GetDisplayHandle).read();
            f(self as *mut Self as _)
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
    /// Routes a `GFx::Event` to the movie (`GFx::Movie` vtable slot 35). For a
    /// [`MouseEvent`] the movie records the new mouse state, which the next `Advance`
    /// processes into AS3 mouse events (hover, press, release). Call on the capture (game
    /// update) thread. `CUIManager::SendMouseEvents` is the engine's only mouse feeder, and
    /// only emits a move event on frames where the DirectInput mouse reported a non-zero
    /// delta.
    pub unsafe fn HandleEvent(
        &mut self,
        event: *const crate::ui::scaleform::MouseEvent,
    ) -> u32 {
        unsafe {
            let f = (&raw const (*self.vftable()).HandleEvent).read();
            f(self as *mut Self as _, event)
        }
    }
    /// Directly overwrites one mouse's state (`GFx::Movie` vtable slot 37): position in
    /// movie-viewport pixels (the same space as [`MouseEvent`]) plus a button bitmask (bit 0
    /// left, bit 1 right), bypassing the event objects. Processed on the next `Advance`, like
    /// [`HandleEvent`](MovieImpl::HandleEvent).
    pub unsafe fn NotifyMouseState(
        &mut self,
        x: f32,
        y: f32,
        buttons: u32,
        mouse_index: u32,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).NotifyMouseState).read();
            f(self as *mut Self as _, x, y, buttons, mouse_index)
        }
    }
    /// Sets how many mice the movie tracks (`GFx::Movie` vtable slot 43); `0` disables mouse
    /// processing entirely. `CUIManager::RestoreAfterReset` sets `1` when a DirectInput mouse
    /// device exists and `0` otherwise.
    pub unsafe fn SetMouseCursorCount(&mut self, count: u32) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetMouseCursorCount).read();
            f(self as *mut Self as _, count)
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
    /// Returns the movie's display handle (`GFx::Movie` vtable slot 26): the pinned
    /// `DisplayHandle<TreeRoot>` whose `pData` the renderer copies (with an `AddRef`) into a
    /// stack [`RTHandle`] before consuming the frame's capture.
    pub GetDisplayHandle: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
    ) -> *mut crate::ui::scaleform::DisplayHandle,
    /// Reassigns which thread owns the capture (`GFx::Movie` vtable slot 27). The engine hands
    /// ownership between the update thread and the render thread this way
    /// (`RenderOffScreenTextures` does exactly this per frame).
    pub SetCaptureThread: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        thread_id: u32,
    ),
    _vfunc_28: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_29: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_30: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_31: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_32: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_33: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_34: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    /// Routes a `GFx::Event` to the movie (`GFx::Movie` vtable slot 35). For a
    /// [`MouseEvent`] the movie records the new mouse state, which the next `Advance`
    /// processes into AS3 mouse events (hover, press, release). Call on the capture (game
    /// update) thread. `CUIManager::SendMouseEvents` is the engine's only mouse feeder, and
    /// only emits a move event on frames where the DirectInput mouse reported a non-zero
    /// delta.
    pub HandleEvent: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        event: *const crate::ui::scaleform::MouseEvent,
    ) -> u32,
    _vfunc_36: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    /// Directly overwrites one mouse's state (`GFx::Movie` vtable slot 37): position in
    /// movie-viewport pixels (the same space as [`MouseEvent`]) plus a button bitmask (bit 0
    /// left, bit 1 right), bypassing the event objects. Processed on the next `Advance`, like
    /// [`HandleEvent`](MovieImpl::HandleEvent).
    pub NotifyMouseState: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        x: f32,
        y: f32,
        buttons: u32,
        mouse_index: u32,
    ),
    _vfunc_38: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_39: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_40: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_41: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    _vfunc_42: unsafe extern "system" fn(this: *mut crate::ui::scaleform::MovieImpl),
    /// Sets how many mice the movie tracks (`GFx::Movie` vtable slot 43); `0` disables mouse
    /// processing entirely. `CUIManager::RestoreAfterReset` sets `1` when a DirectInput mouse
    /// device exists and `0` otherwise.
    pub SetMouseCursorCount: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::MovieImpl,
        count: u32,
    ),
}
fn _MovieImplVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x160], MovieImplVftable>([0u8; 0x160]);
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
/// A stack `Render::ContextImpl::RTHandle` over a copied
/// [`DisplayHandle::pData`] (AddRef'd first, destructed after use).
pub struct RTHandle {
    /// The shared, refcounted handle data.
    pub pData: *mut crate::ui::scaleform::RefCountImpl,
}
fn _RTHandle_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x8], RTHandle>([0u8; 0x8]);
    }
    unreachable!()
}
impl RTHandle {
    pub const NextCapture_ADDRESS: usize = 0x1419A7970;
    /// Consumes the context's pending snapshot into the displaying one (at most once per HAL
    /// frame: the context's [`NextCaptureCalledInFrame`](RenderContext::NextCaptureCalledInFrame)
    /// latch short-circuits repeats). Returns whether the handle is valid to draw. `notify` is
    /// the HAL's context notify ([`RenderHAL::GetContextNotify`]); `frame_id` is 0.
    pub unsafe fn NextCapture(
        &mut self,
        notify: *mut ::std::ffi::c_void,
        frame_id: u64,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                notify: *mut ::std::ffi::c_void,
                frame_id: u64,
            ) -> bool = ::std::mem::transmute(Self::NextCapture_ADDRESS);
            f(self as *mut Self as _, notify, frame_id)
        }
    }
    pub const GetRenderEntry_ADDRESS: usize = 0x1419A4620;
    /// The handle's root entry in the current displaying snapshot, for `HAL::Draw`.
    pub unsafe fn GetRenderEntry(&self) -> *mut crate::ui::scaleform::TreeRoot {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
            ) -> *mut crate::ui::scaleform::TreeRoot = ::std::mem::transmute(
                Self::GetRenderEntry_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
    pub const Destruct_ADDRESS: usize = 0x1419A4600;
    /// The RTHandle destructor (releases the handle data).
    pub unsafe fn Destruct(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::Destruct_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<RTHandle> for RTHandle {
    fn as_ref(&self) -> &RTHandle {
        self
    }
}
impl std::convert::AsMut<RTHandle> for RTHandle {
    fn as_mut(&mut self) -> &mut RTHandle {
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
    _field_9a: [u8; 86],
    /// The per-slot snapshot frame ids, indexed like `pSnapshots`: `[0]` the active snapshot
    /// (incremented by every `Capture`), `[1]` pending, `[2]` displaying, `[3]` finalizing. The
    /// gap between `[0]` and `[2]` is how far the displayed UI trails the update thread -- a
    /// growing gap means the render side is not consuming captures.
    pub SnapshotFrameIds: [u64; 4],
    _field_110: [u8; 8],
}
fn _RenderContext_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x118], RenderContext>([0u8; 0x118]);
    }
    unreachable!()
}
impl RenderContext {
    pub const CreateEntryTreeRoot_ADDRESS: usize = 0x141994560;
    /// Creates a fresh [`TreeRoot`] entry in this context (`Context::CreateEntry<TreeRoot>`):
    /// allocates its `NodeData` from the context heap and registers the entry. The root starts
    /// parentless with no viewport; give it one via [`TreeRoot::SetViewport`] and a stage matrix
    /// via [`TreeNode::SetMatrix`] before drawing it. Call on the capture (game update) thread.
    pub unsafe fn CreateEntryTreeRoot(&mut self) -> *mut crate::ui::scaleform::TreeRoot {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::ui::scaleform::TreeRoot = ::std::mem::transmute(
                Self::CreateEntryTreeRoot_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
    pub const CreateEntryTreeContainer_ADDRESS: usize = 0x141990AF0;
    /// Creates a fresh empty [`TreeContainer`] entry in this context
    /// (`Context::CreateEntry<TreeContainer>`). Call on the capture (game update) thread.
    pub unsafe fn CreateEntryTreeContainer(
        &mut self,
    ) -> *mut crate::ui::scaleform::TreeContainer {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
            ) -> *mut crate::ui::scaleform::TreeContainer = ::std::mem::transmute(
                Self::CreateEntryTreeContainer_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
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
    vftable: *const crate::ui::scaleform::RenderHALVftable,
    _field_8: [u8; 291744],
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
impl RenderHAL {
    pub fn vftable(&self) -> *const crate::ui::scaleform::RenderHALVftable {
        self.vftable as *const crate::ui::scaleform::RenderHALVftable
    }
    pub const CreateRenderTarget_ADDRESS: usize = 0x141DE1110;
    /// Builds a [`RenderTarget`] from D3D color/depth views (`ID3D11RenderTargetView*` /
    /// `ID3D11DepthStencilView*`); the concrete implementation behind vtable slot 111.
    /// `RenderOffScreenTextures` creates one per off-screen draw and releases it afterwards.
    /// Render thread only.
    pub unsafe fn CreateRenderTarget(
        &mut self,
        color: *mut ::std::ffi::c_void,
        depth: *mut ::std::ffi::c_void,
    ) -> *mut crate::ui::scaleform::RenderTarget {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                color: *mut ::std::ffi::c_void,
                depth: *mut ::std::ffi::c_void,
            ) -> *mut crate::ui::scaleform::RenderTarget = ::std::mem::transmute(
                Self::CreateRenderTarget_ADDRESS,
            );
            f(self as *mut Self as _, color, depth)
        }
    }
    pub const Draw_ADDRESS: usize = 0x1419B4850;
    /// Draws a [`TreeRoot`]'s displaying-snapshot subtree into the bound target
    /// (`Scaleform::Render::HAL::Draw`). Render thread only, within `BeginFrame`/`BeginScene`.
    pub unsafe fn Draw(&mut self, root: *mut crate::ui::scaleform::TreeRoot) {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                root: *mut crate::ui::scaleform::TreeRoot,
            ) = ::std::mem::transmute(Self::Draw_ADDRESS);
            f(self as *mut Self as _, root)
        }
    }
    /// Binds `target` as the frame's display render target (slot 3). `CUIManager::Render`
    /// calls it with `m_RenderBuffer` before `BeginFrame`, paired with a trailing
    /// [`PopRenderTarget`](RenderHAL::PopRenderTarget).
    pub unsafe fn SetRenderTarget(
        &mut self,
        target: *mut crate::ui::scaleform::RenderTarget,
        set_state: bool,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).SetRenderTarget).read();
            f(self as *mut Self as _, target, set_state)
        }
    }
    /// Pushes `target` for nested rendering within a frame (slot 4), as
    /// `RenderOffScreenTextures` does per off-screen movie. `frame_rect` is `[x0, y0, x1,
    /// y1]` floats; `clear_color` points at a packed color (0 = transparent).
    pub unsafe fn PushRenderTarget(
        &mut self,
        target: *mut crate::ui::scaleform::RenderTarget,
        flags: u64,
        frame_rect: *const f32,
        clear_color: *const u32,
    ) {
        unsafe {
            let f = (&raw const (*self.vftable()).PushRenderTarget).read();
            f(self as *mut Self as _, target, flags, frame_rect, clear_color)
        }
    }
    /// Pops the pushed (or set) render target (slot 5).
    pub unsafe fn PopRenderTarget(&mut self, flags: u32) {
        unsafe {
            let f = (&raw const (*self.vftable()).PopRenderTarget).read();
            f(self as *mut Self as _, flags)
        }
    }
    /// Begins the HAL frame (slot 11). `EndFrame` clears every context's once-a-frame
    /// consumption latch via `EndFrameContextNotify`.
    pub unsafe fn BeginFrame(&mut self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).BeginFrame).read();
            f(self as *mut Self as _)
        }
    }
    /// Ends the HAL frame (slot 12).
    pub unsafe fn EndFrame(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).EndFrame).read();
            f(self as *mut Self as _)
        }
    }
    /// Begins a scene (draw batch) within the frame (slot 15).
    pub unsafe fn BeginScene(&mut self) -> bool {
        unsafe {
            let f = (&raw const (*self.vftable()).BeginScene).read();
            f(self as *mut Self as _)
        }
    }
    /// Ends the scene (slot 16).
    pub unsafe fn EndScene(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).EndScene).read();
            f(self as *mut Self as _)
        }
    }
    /// The HAL's `Render::ContextImpl::RenderNotify` (slot 19), passed to
    /// [`RTHandle::NextCapture`].
    pub unsafe fn GetContextNotify(&mut self) -> *mut ::std::ffi::c_void {
        unsafe {
            let f = (&raw const (*self.vftable()).GetContextNotify).read();
            f(self as *mut Self as _)
        }
    }
}
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
pub struct RenderHALVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_2: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    /// Binds `target` as the frame's display render target (slot 3). `CUIManager::Render`
    /// calls it with `m_RenderBuffer` before `BeginFrame`, paired with a trailing
    /// [`PopRenderTarget`](RenderHAL::PopRenderTarget).
    pub SetRenderTarget: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
        target: *mut crate::ui::scaleform::RenderTarget,
        set_state: bool,
    ),
    /// Pushes `target` for nested rendering within a frame (slot 4), as
    /// `RenderOffScreenTextures` does per off-screen movie. `frame_rect` is `[x0, y0, x1,
    /// y1]` floats; `clear_color` points at a packed color (0 = transparent).
    pub PushRenderTarget: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
        target: *mut crate::ui::scaleform::RenderTarget,
        flags: u64,
        frame_rect: *const f32,
        clear_color: *const u32,
    ),
    /// Pops the pushed (or set) render target (slot 5).
    pub PopRenderTarget: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
        flags: u32,
    ),
    _vfunc_6: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_7: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_8: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_9: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_10: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    /// Begins the HAL frame (slot 11). `EndFrame` clears every context's once-a-frame
    /// consumption latch via `EndFrameContextNotify`.
    pub BeginFrame: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
    ) -> bool,
    /// Ends the HAL frame (slot 12).
    pub EndFrame: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_13: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_14: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    /// Begins a scene (draw batch) within the frame (slot 15).
    pub BeginScene: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
    ) -> bool,
    /// Ends the scene (slot 16).
    pub EndScene: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_17: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    _vfunc_18: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderHAL),
    /// The HAL's `Render::ContextImpl::RenderNotify` (slot 19), passed to
    /// [`RTHandle::NextCapture`].
    pub GetContextNotify: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderHAL,
    ) -> *mut ::std::ffi::c_void,
}
fn _RenderHALVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0xA0], RenderHALVftable>([0u8; 0xA0]);
    }
    unreachable!()
}
impl RenderHALVftable {}
impl std::convert::AsRef<RenderHALVftable> for RenderHALVftable {
    fn as_ref(&self) -> &RenderHALVftable {
        self
    }
}
impl std::convert::AsMut<RenderHALVftable> for RenderHALVftable {
    fn as_mut(&mut self) -> &mut RenderHALVftable {
        self
    }
}
#[repr(C, align(8))]
/// A Scaleform `Render::RenderTarget`: the target wrapper `SetRenderTarget`/`PushRenderTarget`
/// take. `RenderOffScreenTextures` creates one per draw from D3D views
/// ([`RenderHAL::CreateRenderTarget`]) and releases it after the pop.
pub struct RenderTarget {
    vftable: *const crate::ui::scaleform::RenderTargetVftable,
    _field_8: [u8; 16],
}
fn _RenderTarget_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], RenderTarget>([0u8; 0x18]);
    }
    unreachable!()
}
impl RenderTarget {
    pub fn vftable(&self) -> *const crate::ui::scaleform::RenderTargetVftable {
        self.vftable as *const crate::ui::scaleform::RenderTargetVftable
    }
    /// Releases the target (refcounted; slot 2).
    pub unsafe fn Release(&mut self) {
        unsafe {
            let f = (&raw const (*self.vftable()).Release).read();
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<RenderTarget> for RenderTarget {
    fn as_ref(&self) -> &RenderTarget {
        self
    }
}
impl std::convert::AsMut<RenderTarget> for RenderTarget {
    fn as_mut(&mut self) -> &mut RenderTarget {
        self
    }
}
#[repr(C, align(8))]
pub struct RenderTargetVftable {
    _vfunc_0: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderTarget),
    _vfunc_1: unsafe extern "system" fn(this: *mut crate::ui::scaleform::RenderTarget),
    /// Releases the target (refcounted; slot 2).
    pub Release: unsafe extern "system" fn(
        this: *mut crate::ui::scaleform::RenderTarget,
    ),
}
fn _RenderTargetVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x18], RenderTargetVftable>([0u8; 0x18]);
    }
    unreachable!()
}
impl RenderTargetVftable {}
impl std::convert::AsRef<RenderTargetVftable> for RenderTargetVftable {
    fn as_ref(&self) -> &RenderTargetVftable {
        self
    }
}
impl std::convert::AsMut<RenderTargetVftable> for RenderTargetVftable {
    fn as_mut(&mut self) -> &mut RenderTargetVftable {
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
#[repr(C, align(1))]
/// A render-tree container node (`Render::TreeContainer`): a [`TreeNode`] whose `NodeData` holds
/// a child `TreeNodeArray`. The display side mirrors every `DisplayObjContainer` into one, and
/// addresses children by *cached numeric index* (`GFx::DisplayList`'s `TreeIndex`) -- so removing
/// a child behind the display list's back shifts its siblings' cached indices; swap in a
/// placeholder node instead when a child must leave without the display list knowing.
pub struct TreeContainer {
    _field_0: [u8; 56],
}
fn _TreeContainer_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], TreeContainer>([0u8; 0x38]);
    }
    unreachable!()
}
impl TreeContainer {
    pub const Insert_ADDRESS: usize = 0x1419E6720;
    /// Inserts `node` at `index` (entry-change bit 0x100): refcounts the node, sets its
    /// [`pParent`](TreeNode::pParent), and propagates. Capture (game update) thread only.
    pub unsafe fn Insert(
        &mut self,
        index: u64,
        node: *mut crate::ui::scaleform::TreeNode,
    ) -> bool {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *mut Self,
                index: u64,
                node: *mut crate::ui::scaleform::TreeNode,
            ) -> bool = ::std::mem::transmute(Self::Insert_ADDRESS);
            f(self as *mut Self as _, index, node)
        }
    }
    pub const Remove_ADDRESS: usize = 0x1419E6790;
    /// Removes `count` children starting at `index` (entry-change bit 0x200): nulls each child's
    /// [`pParent`](TreeNode::pParent) and drops its refcount (destroying it at zero). Capture
    /// (game update) thread only.
    pub unsafe fn Remove(&mut self, index: u64, count: u64) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, index: u64, count: u64) = ::std::mem::transmute(
                Self::Remove_ADDRESS,
            );
            f(self as *mut Self as _, index, count)
        }
    }
    pub const GetAt_ADDRESS: usize = 0x1419EA460;
    /// The child at `index` of the active snapshot's child array, or null out of range.
    pub unsafe fn GetAt(&self, index: u64) -> *mut crate::ui::scaleform::TreeNode {
        unsafe {
            let f: unsafe extern "system" fn(
                this: *const Self,
                index: u64,
            ) -> *mut crate::ui::scaleform::TreeNode = ::std::mem::transmute(
                Self::GetAt_ADDRESS,
            );
            f(self as *const Self as _, index)
        }
    }
    pub const GetSize_ADDRESS: usize = 0x14197DA10;
    /// The active snapshot's child count.
    pub unsafe fn GetSize(&self) -> u64 {
        unsafe {
            let f: unsafe extern "system" fn(this: *const Self) -> u64 = ::std::mem::transmute(
                Self::GetSize_ADDRESS,
            );
            f(self as *const Self as _)
        }
    }
}
impl std::convert::AsRef<TreeContainer> for TreeContainer {
    fn as_ref(&self) -> &TreeContainer {
        self
    }
}
impl std::convert::AsMut<TreeContainer> for TreeContainer {
    fn as_mut(&mut self) -> &mut TreeContainer {
        self
    }
}
#[repr(C, align(8))]
/// A Scaleform render-tree node: a `Render::ContextImpl::Entry` (a 56-byte slot in a
/// 0x1000-aligned entry page) whose per-snapshot `NodeData` carries the transform, visibility,
/// and bounds. [`TreeContainer`] and [`TreeRoot`] share this header; a pointer to either casts to
/// this freely.
pub struct TreeNode {
    _field_0: [u8; 8],
    /// The entry's reference count. [`TreeContainer::Insert`] increments it and
    /// [`TreeContainer::Remove`] decrements it (destroying the entry at zero), so moving a node
    /// between containers must hold an extra count across the remove.
    pub RefCount: u64,
    /// The displaying-snapshot `EntryData` the renderer reads (low bit carries a flag).
    pub pNative: *mut ::std::ffi::c_void,
    /// The renderer's `TreeCacheNode` for this entry, once drawn.
    pub pRenderer: *mut ::std::ffi::c_void,
    /// The parent container entry, or null while detached. [`TreeContainer::Remove`] nulls it;
    /// a null parent on a node the display side owned means the display list dropped it.
    pub pParent: *mut crate::ui::scaleform::TreeNode,
    _field_28: [u8; 16],
}
fn _TreeNode_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], TreeNode>([0u8; 0x38]);
    }
    unreachable!()
}
impl TreeNode {
    pub const SetMatrix_ADDRESS: usize = 0x1419E7230;
    /// Writes the node's 2D transform (`Matrix2x4<float>`, 8 floats) through the entry-change
    /// protocol. Call on the capture (game update) thread.
    pub unsafe fn SetMatrix(&mut self, matrix: *const f32) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, matrix: *const f32) = ::std::mem::transmute(
                Self::SetMatrix_ADDRESS,
            );
            f(self as *mut Self as _, matrix)
        }
    }
    pub const DestroyHelper_ADDRESS: usize = 0x1419A6110;
    /// Destroys the entry (`Entry::destroyHelper`): queues its `NodeData` on the context's
    /// destroyed-nodes list (freed at the next capture) and frees the slot. Call only when
    /// [`RefCount`](TreeNode::RefCount) reached zero, on the capture (game update) thread.
    pub unsafe fn DestroyHelper(&mut self) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self) = ::std::mem::transmute(
                Self::DestroyHelper_ADDRESS,
            );
            f(self as *mut Self as _)
        }
    }
}
impl std::convert::AsRef<TreeNode> for TreeNode {
    fn as_ref(&self) -> &TreeNode {
        self
    }
}
impl std::convert::AsMut<TreeNode> for TreeNode {
    fn as_mut(&mut self) -> &mut TreeNode {
        self
    }
}
#[repr(C, align(1))]
/// A render-tree root (`Render::TreeRoot`): a [`TreeContainer`] whose `NodeData` adds the
/// viewport and background color. `HAL::Draw` takes one; the engine draws several per frame
/// through the same HAL (the UI movie, the off-screen movies, the debug text).
pub struct TreeRoot {
    _field_0: [u8; 56],
}
fn _TreeRoot_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x38], TreeRoot>([0u8; 0x38]);
    }
    unreachable!()
}
impl TreeRoot {
    pub const SetViewport_ADDRESS: usize = 0x1419E6860;
    /// Writes the root's viewport (entry-change bit 0x1000, self-comparing). `viewport` is a
    /// `GFx::Viewport` (e.g. [`MovieImpl::Viewport`]); the 0x2C `Render::Viewport` prefix is
    /// copied. Capture (game update) thread only.
    pub unsafe fn SetViewport(&mut self, viewport: *const u8) {
        unsafe {
            let f: unsafe extern "system" fn(this: *mut Self, viewport: *const u8) = ::std::mem::transmute(
                Self::SetViewport_ADDRESS,
            );
            f(self as *mut Self as _, viewport)
        }
    }
}
impl std::convert::AsRef<TreeRoot> for TreeRoot {
    fn as_ref(&self) -> &TreeRoot {
        self
    }
}
impl std::convert::AsMut<TreeRoot> for TreeRoot {
    fn as_mut(&mut self) -> &mut TreeRoot {
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
