#![cfg_attr(any(), rustfmt::skip)]
#[repr(C, align(8))]
/// A node in the display tree returned by `Movie::GetDisplayObjectsTree`. Inherits from
/// [`RefCountImpl`] and embeds a `Scaleform::String` for the name and a
/// `Scaleform::ArrayDataBase<Ptr<AmpMovieObjectDesc>>` for children.
///
/// Layout verified from `GetChildDescTree` at `0x141_A30_410`.
pub struct AmpMovieObjectDesc {
    pub ref_count_impl: crate::ui::scaleform::RefCountImpl,
    /// The clip's instance name, or `"Unnamed"`. A `Scaleform::String` (heap-allocated with SSO);
    /// the first 8 bytes are a `*const c_char` pointing into the internal buffer.
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
    /// [`MovieImpl::pHeap`]); the tree is freed by releasing the root node. Slot 9 (offset
    /// `0x48`); the implementation is at `0x141_BED_530`. Call on the capture (game update)
    /// thread, where the display tree is stable.
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
    /// The `MovieRoot` vtable address. The payload checks a live object against it before
    /// trusting the vtable-indexed calls, since `m_Movie`'s dynamic type is load-bearing here.
    pub const VFTABLE: u64 = 5408692096;
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
/// The `GFx::MovieImpl` behind a [`Movie`]: the movie instance state, including the heap all of
/// its allocations come from.
pub struct MovieImpl {
    _field_0: [u8; 56],
    /// The `GFx::Value::ObjectInterface` for this movie: the dispatcher every `GFx::Value`
    /// display-object/member operation goes through.
    pub pObjectInterface: *mut ::std::ffi::c_void,
    /// The Scaleform `MemoryHeap` the movie allocates from.
    pub pHeap: *mut crate::ui::scaleform::MemoryHeap,
    _field_48: [u8; 8],
    /// The root `DisplayObjContainer` (the main movie clip).
    pub pMainMovie: *mut ::std::ffi::c_void,
}
fn _MovieImpl_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x58], MovieImpl>([0u8; 0x58]);
    }
    unreachable!()
}
impl MovieImpl {}
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
    /// Recursively walks the runtime display tree from the root movie clip: returns a tree of
    /// [`AmpMovieObjectDesc`] nodes, each carrying the clip's instance name (or `"Unnamed"`)
    /// and a child array, calling `DisplayObject::GetName` on every child and recursing into
    /// containers. `heap` is the Scaleform `MemoryHeap*` used for the allocations (e.g.
    /// [`MovieImpl::pHeap`]); the tree is freed by releasing the root node. Slot 9 (offset
    /// `0x48`); the implementation is at `0x141_BED_530`. Call on the capture (game update)
    /// thread, where the display tree is stable.
    pub GetDisplayObjectsTree: unsafe extern "system" fn(
        this: *const crate::ui::scaleform::Movie,
        heap: *mut crate::ui::scaleform::MemoryHeap,
    ) -> *mut crate::ui::scaleform::AmpMovieObjectDesc,
}
fn _MovieVftable_size_check() {
    unsafe {
        ::std::mem::transmute::<[u8; 0x50], MovieVftable>([0u8; 0x50]);
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
    pub pObjectInterface: *mut ::std::ffi::c_void,
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
