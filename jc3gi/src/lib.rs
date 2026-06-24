#![allow(
    dead_code,
    non_snake_case,
    non_camel_case_types,
    non_upper_case_globals,
    clippy::missing_safety_doc,
    clippy::unnecessary_cast,
    clippy::module_inception
)]
#![cfg_attr(any(), rustfmt::skip)]
#[macro_export]
macro_rules! __bitflags {
    (
        $(#[$outer:meta])* $vis:vis struct $Name:ident : $T:ty { $($(#[$inner:meta])*
        const $Flag:ident = $value:expr;)* }
    ) => {
        $(#[$outer])* #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(transparent)] $vis struct $Name { bits : $T, } #[allow(dead_code)] impl
        $Name { $($(#[$inner])* $vis const $Flag : Self = Self { bits : $value };)* #[doc
        = " Returns an empty set of flags."] #[inline] pub const fn empty() -> Self {
        Self { bits : 0 } } #[doc = " Returns the set containing all flags."] pub const
        fn all() -> Self { Self { bits : 0 $(| Self::$Flag .bits)* } } #[doc =
        " Returns the raw value of the flags currently stored."] #[inline] pub const fn
        bits(& self) -> $T { self.bits } #[doc =
        " Convert from underlying bit representation, unless that"] #[doc =
        " representation contains bits that do not correspond to a flag."] #[inline] pub
        const fn from_bits(bits : $T) -> ::core::option::Option < Self > { let truncated
        = bits & Self::all().bits; if truncated == bits {
        ::core::option::Option::Some(Self { bits }) } else { ::core::option::Option::None
        } } #[doc = " Convert from underlying bit representation, dropping any bits"]
        #[doc = " that do not correspond to flags."] #[inline] pub const fn
        from_bits_truncate(bits : $T) -> Self { Self { bits : bits & Self::all().bits } }
        #[doc = " Convert from underlying bit representation as is, without"] #[doc =
        " checking whether any bits that do not correspond to a flag are"] #[doc =
        " set."] #[inline] pub const fn from_bits_retain(bits : $T) -> Self { Self { bits
        } } #[doc = " Returns `true` if no flags are currently stored."] #[inline] pub
        const fn is_empty(& self) -> bool { self.bits == 0 } #[doc =
        " Returns `true` if all flags are currently set."] #[inline] pub const fn
        is_all(& self) -> bool { Self::all().bits | self.bits == self.bits } #[doc =
        " Returns `true` if there are flags common to both `self` and"] #[doc =
        " `other`."] #[inline] pub const fn intersects(& self, other : Self) -> bool {
        self.bits & other.bits != 0 } #[doc =
        " Returns `true` if all of the flags in `other` are contained"] #[doc =
        " within `self`."] #[inline] pub const fn contains(& self, other : Self) -> bool
        { self.bits & other.bits == other.bits } #[doc =
        " Inserts the specified flags in-place."] #[inline] pub fn insert(& mut self,
        other : Self) { self.bits |= other.bits; } #[doc =
        " Removes the specified flags in-place."] #[inline] pub fn remove(& mut self,
        other : Self) { self.bits &= ! other.bits; } #[doc =
        " Toggles the specified flags in-place."] #[inline] pub fn toggle(& mut self,
        other : Self) { self.bits ^= other.bits; } #[doc =
        " Inserts or removes the specified flags depending on the passed"] #[doc =
        " value."] #[inline] pub fn set(& mut self, other : Self, value : bool) { if
        value { self.insert(other); } else { self.remove(other); } } #[doc =
        " Returns the intersection between the flags in `self` and"] #[doc = " `other`."]
        #[inline] pub const fn intersection(self, other : Self) -> Self { Self { bits :
        self.bits & other.bits } } #[doc =
        " Returns the union of the flags in `self` and `other`."] #[inline] pub const fn
        union(self, other : Self) -> Self { Self { bits : self.bits | other.bits } }
        #[doc = " Returns the difference between the flags in `self` and `other`."]
        #[inline] pub const fn difference(self, other : Self) -> Self { Self { bits :
        self.bits & ! other.bits } } #[doc =
        " Returns the symmetric difference between the flags in `self` and"] #[doc =
        " `other`."] #[inline] pub const fn symmetric_difference(self, other : Self) ->
        Self { Self { bits : self.bits ^ other.bits } } #[doc =
        " Returns the complement of this set of flags."] #[inline] pub const fn
        complement(self) -> Self { Self::from_bits_truncate(! self.bits) } } impl
        ::core::ops::BitOr for $Name { type Output = Self; #[inline] fn bitor(self, other
        : Self) -> Self { self.union(other) } } impl ::core::ops::BitOrAssign for $Name {
        #[inline] fn bitor_assign(& mut self, other : Self) { self.insert(other) } } impl
        ::core::ops::BitAnd for $Name { type Output = Self; #[inline] fn bitand(self,
        other : Self) -> Self { self.intersection(other) } } impl
        ::core::ops::BitAndAssign for $Name { #[inline] fn bitand_assign(& mut self,
        other : Self) { self.bits &= other.bits } } impl ::core::ops::BitXor for $Name {
        type Output = Self; #[inline] fn bitxor(self, other : Self) -> Self { self
        .symmetric_difference(other) } } impl ::core::ops::BitXorAssign for $Name {
        #[inline] fn bitxor_assign(& mut self, other : Self) { self.toggle(other) } }
        impl ::core::ops::Sub for $Name { type Output = Self; #[inline] fn sub(self,
        other : Self) -> Self { self.difference(other) } } impl ::core::ops::SubAssign
        for $Name { #[inline] fn sub_assign(& mut self, other : Self) { self
        .remove(other) } } impl ::core::ops::Not for $Name { type Output = Self;
        #[inline] fn not(self) -> Self { self.complement() } } impl ::core::iter::Extend
        <$Name > for $Name { fn extend < T : ::core::iter::IntoIterator < Item = Self >>
        (& mut self, iterator : T) { for item in iterator { self.insert(item) } } } impl
        ::core::iter::FromIterator <$Name > for $Name { fn from_iter < T :
        ::core::iter::IntoIterator < Item = Self >> (iterator : T) -> Self { let mut
        result = Self::empty(); result.extend(iterator); result } } impl
        ::core::fmt::Binary for $Name { fn fmt(& self, f : & mut ::core::fmt::Formatter
        <'_ >) -> ::core::fmt::Result { ::core::fmt::Binary::fmt(& self.bits, f) } } impl
        ::core::fmt::Octal for $Name { fn fmt(& self, f : & mut ::core::fmt::Formatter
        <'_ >) -> ::core::fmt::Result { ::core::fmt::Octal::fmt(& self.bits, f) } } impl
        ::core::fmt::LowerHex for $Name { fn fmt(& self, f : & mut ::core::fmt::Formatter
        <'_ >) -> ::core::fmt::Result { ::core::fmt::LowerHex::fmt(& self.bits, f) } }
        impl ::core::fmt::UpperHex for $Name { fn fmt(& self, f : & mut
        ::core::fmt::Formatter <'_ >) -> ::core::fmt::Result {
        ::core::fmt::UpperHex::fmt(& self.bits, f) } }
    };
}
pub mod aim;
pub mod animation;
pub mod audio;
pub mod camera;
pub mod character;
pub mod clock;
pub mod environment;
pub mod game;
pub mod graphics_engine;
pub mod hash;
pub mod input;
pub mod state;
pub mod types;
pub mod ui;
pub mod window;
