// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! The enumerations, sets and the signature type of `wbInterface.pas`.
//!
//! Enumeration values keep their upstream names (`cpNormal`, `dtStruct`), so
//! that ported code and definition files read like the Pascal source.

use std::fmt;
use std::marker::PhantomData;
use std::ops::{BitAnd, BitOr, Sub};

/// A Pascal enumeration: consecutive ordinals starting at 0.
pub trait PascalEnum: Copy + Eq + 'static {
    /// Every value in declaration order.
    const ALL: &'static [Self];

    /// Position in the declaration, Pascal `Ord`.
    fn ord(self) -> usize;

    /// The identifier of the value, Pascal `GetEnumName`.
    fn enum_name(self) -> &'static str;
}

/// A Pascal `set of` an enumeration with at most 64 values.
pub struct EnumSet<T: PascalEnum> {
    bits: u64,
    marker: PhantomData<T>,
}

impl<T: PascalEnum> EnumSet<T> {
    pub const fn empty() -> Self {
        Self {
            bits: 0,
            marker: PhantomData,
        }
    }

    /// The set of the listed values, Pascal `[a, b, c]`.
    pub fn of(values: &[T]) -> Self {
        let mut result = Self::empty();
        for &value in values {
            result.include(value);
        }
        result
    }

    /// The set as a bit mask: bit `n` is the value with ordinal `n`.
    pub const fn bits(self) -> u64 {
        self.bits
    }

    pub const fn from_bits(bits: u64) -> Self {
        Self {
            bits,
            marker: PhantomData,
        }
    }

    /// Pascal `value in set`.
    pub fn contains(self, value: T) -> bool {
        self.bits & (1 << value.ord()) != 0
    }

    /// Pascal `Include(set, value)`.
    pub fn include(&mut self, value: T) {
        self.bits |= 1 << value.ord();
    }

    /// Pascal `Exclude(set, value)`.
    pub fn exclude(&mut self, value: T) {
        self.bits &= !(1 << value.ord());
    }

    pub fn is_empty(self) -> bool {
        self.bits == 0
    }

    /// Number of values in the set.
    pub fn count(self) -> usize {
        self.bits.count_ones() as usize
    }

    /// The values of the set in declaration order.
    pub fn iter(self) -> impl Iterator<Item = T> {
        T::ALL.iter().copied().filter(move |&value| self.contains(value))
    }
}

impl<T: PascalEnum> Clone for EnumSet<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: PascalEnum> Copy for EnumSet<T> {}

impl<T: PascalEnum> Default for EnumSet<T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T: PascalEnum> PartialEq for EnumSet<T> {
    fn eq(&self, other: &Self) -> bool {
        self.bits == other.bits
    }
}

impl<T: PascalEnum> Eq for EnumSet<T> {}

impl<T: PascalEnum> fmt::Debug for EnumSet<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter().map(PascalEnum::enum_name)).finish()
    }
}

/// Pascal set union `a + b`.
impl<T: PascalEnum> BitOr for EnumSet<T> {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
            marker: PhantomData,
        }
    }
}

/// Pascal set intersection `a * b`.
impl<T: PascalEnum> BitAnd for EnumSet<T> {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        Self {
            bits: self.bits & other.bits,
            marker: PhantomData,
        }
    }
}

/// Pascal set difference `a - b`.
impl<T: PascalEnum> Sub for EnumSet<T> {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            bits: self.bits & !other.bits,
            marker: PhantomData,
        }
    }
}

/// Declares a Pascal enumeration with its upstream value names.
macro_rules! pascal_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$value_meta:meta])* $value:ident,)+ }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[allow(non_camel_case_types)]
        #[repr(u8)]
        pub enum $name {
            $($(#[$value_meta])* $value,)+
        }

        impl PascalEnum for $name {
            const ALL: &'static [Self] = &[$($name::$value,)+];

            fn ord(self) -> usize {
                self as usize
            }

            fn enum_name(self) -> &'static str {
                match self {
                    $($name::$value => stringify!($value),)+
                }
            }
        }
    };
}
pub(crate) use pascal_enum;

pascal_enum! {
    ConflictAll {
        caUnknown,
        caOnlyOne,
        caNoConflict,
        caConflictBenign,
        caOverride,
        caConflict,
        caConflictCritical,
    }
}

pascal_enum! {
    ConflictThis {
        ctUnknown,
        ctIgnored,
        ctNotDefined,
        ctIdenticalToMaster,
        ctOnlyOne,
        ctHiddenByModGroup,
        ctMaster,
        ctConflictBenign,
        ctOverride,
        ctIdenticalToMasterWinsConflict,
        ctConflictWins,
        ctConflictLoses,
    }
}

impl ConflictAll {
    /// Upstream `wbNameConflictAll`.
    pub fn name(self) -> &'static str {
        match self {
            ConflictAll::caUnknown => "",
            ConflictAll::caOnlyOne => "Single Record",
            ConflictAll::caNoConflict => "Multiple but no conflict",
            ConflictAll::caConflictBenign => "Benign Conflict",
            ConflictAll::caOverride => "Override without conflict",
            ConflictAll::caConflict => "Conflict",
            ConflictAll::caConflictCritical => "Critical Conflict",
        }
    }
}

impl ConflictThis {
    /// Upstream `wbNameConflictThis`.
    pub fn name(self) -> &'static str {
        match self {
            ConflictThis::ctUnknown => "",
            ConflictThis::ctIgnored => "Ignored",
            ConflictThis::ctNotDefined => "Not Defined",
            ConflictThis::ctIdenticalToMaster => "Identical to Master",
            ConflictThis::ctOnlyOne => "Single Record",
            ConflictThis::ctHiddenByModGroup => "Hidden by Mod Group",
            ConflictThis::ctMaster => "Master",
            ConflictThis::ctConflictBenign => "Benign conflict",
            ConflictThis::ctOverride => "Override without conflict",
            ConflictThis::ctIdenticalToMasterWinsConflict => "Identical to Master but conflict winner",
            ConflictThis::ctConflictWins => "Conflict winner",
            ConflictThis::ctConflictLoses => "Conflict loser",
        }
    }
}

pascal_enum! {
    ConflictPriority {
        cpIgnore,
        cpBenignIfAdded,
        cpBenign,
        cpOverride,
        cpTranslate,
        cpNormal,
        cpNormalIgnoreEmpty,
        cpCritical,
        cpFormID,
    }
}

/// A record, group or subrecord signature: four bytes as stored in the file.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Signature(pub [u8; 4]);

/// `StrToSignature` was called with fewer than four characters.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("\"{0}\" is not a valid signature")]
pub struct InvalidSignature(pub String);

impl Signature {
    pub const fn new(bytes: &[u8; 4]) -> Self {
        Self(*bytes)
    }

    /// Port of `StrToSignature`: the first four characters of `s`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, InvalidSignature> {
        // Upstream converts to an AnsiString first. Signatures are ASCII; any
        // other character becomes '?' as it would for an unmappable character.
        let mut bytes = s.chars().map(|c| if c.is_ascii() { c as u8 } else { b'?' });
        match (bytes.next(), bytes.next(), bytes.next(), bytes.next()) {
            (Some(a), Some(b), Some(c), Some(d)) => Ok(Self([a, b, c, d])),
            _ => Err(InvalidSignature(s.to_owned())),
        }
    }

    /// Port of `IntToSignature`: the bytes of the integer in file order.
    pub const fn from_int(value: u32) -> Self {
        Self(value.to_le_bytes())
    }

    pub const fn to_int(self) -> u32 {
        u32::from_le_bytes(self.0)
    }
}

/// The signature as a Delphi `string`. Delphi converts the character array as
/// a zero-terminated string in the system code page, so the text ends before
/// the first zero byte.
// The system code page of the oracle machine is 1252.
impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let length = self.0.iter().position(|&byte| byte == 0).unwrap_or(4);
        let text = xedit_io::Encoding::Mbcs(1252)
            .get_string(&self.0[..length])
            .unwrap_or_default();
        f.write_str(&text)
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({self})")
    }
}

pascal_enum! {
    KnownSubRecord {
        ksrEditorID,
        ksrFullName,
        ksrBaseRecord,
        ksrGridCell,
        ksrBaseFormComponents,
    }
}

/// Signatures of the subrecords xEdit knows by role, indexed by [`KnownSubRecord`].
pub type KnownSubRecordSignatures = [Signature; 5];

/// Upstream `wbKnownSubRecordSignatures`, the default of every game.
pub const KNOWN_SUB_RECORD_SIGNATURES: KnownSubRecordSignatures = [
    Signature::new(b"EDID"),
    Signature::new(b"FULL"),
    Signature::new(b"NAME"),
    Signature::new(b"XCLC"),
    Signature::new(b"____"),
];

pascal_enum! {
    IntType {
        it0,
        itU8,
        itS8,
        itU16,
        itS16,
        itU32,
        itS32,
        itU64,
        itS64,
        itU24,
        itU6to30,
    }
}

pascal_enum! {
    DefType {
        dtRecord,
        dtSubRecord,
        dtSubRecordArray,
        dtSubRecordStruct,
        dtSubRecordUnion,
        dtString,
        dtLString,
        dtLenString,
        dtByteArray,
        dtInteger,
        dtIntegerFormater,
        dtIntegerFormaterUnion,
        dtFlag,
        dtFloat,
        dtGuid,
        dtArray,
        dtStruct,
        dtUnion,
        dtResolvable,
        dtEmpty,
        dtStructChapter,
    }
}

pub type DefTypes = EnumSet<DefType>;

/// Upstream `dtArrays`.
pub fn dt_arrays() -> DefTypes {
    EnumSet::of(&[DefType::dtSubRecordArray, DefType::dtArray])
}

/// Upstream `dtNonValues`.
pub fn dt_non_values() -> DefTypes {
    EnumSet::of(&[
        DefType::dtRecord,
        DefType::dtSubRecord,
        DefType::dtSubRecordArray,
        DefType::dtSubRecordStruct,
        DefType::dtSubRecordUnion,
        DefType::dtArray,
        DefType::dtStruct,
        DefType::dtUnion,
        DefType::dtStructChapter,
    ])
}

pascal_enum! {
    DefFlag {
        dfAllowAnyMember,
        dfArrayCanBeEmpty,
        dfArrayStaticSize,
        dfCollapsed,
        dfDontAssign,
        dfDontSave,
        dfExcludeFromBuildRef,
        dfFastAssign,
        dfFloatSometimesBroken,
        dfHideText,
        dfIncludeValueInDisplaySignature,
        dfIndexEditorID,
        dfInternalEditOnly,
        dfIsRecordFlags,
        dfMergeIfMultiple,
        dfMustBeUnion,
        dfNeedsPrepareSave,
        dfNoCopyAsOverride,
        dfNoMove,
        dfNoReport,
        dfNotAlignable,
        dfRemoveLastOnly,
        dfSkipImplicitEdit,
        dfStructFirstNotRequired,
        dfSummaryExcludeNULL,
        dfSummaryMembersNoName,
        dfSummaryMembersShowIgnore,
        dfSummaryNoName,
        dfSummaryNoPassthrough,
        dfSummaryNoSortKey,
        dfSummarySelfAsShortName,
        dfSummaryShowIgnore,
        dfTemplate,
        dfTranslatable,
        dfUnionStaticResolve,
        dfUseLoadOrder,
        /// Not implemented for all definitions.
        dfZeroSortKey,
        dfTerminator,
        dfHasZeroTerminator,
        dfNoZeroTerminator,
        dfCanContainFormID,
        dfCanContainReflection,
        dfCanContainUnmappedFormID,
        dfIsReflection,
        dfUnmappedFormID,
        dfAfterSetOnIDUpdate,
    }
}

pub type DefFlags = EnumSet<DefFlag>;

/// Upstream `_DefFlagsInheritUp`.
pub fn def_flags_inherit_up() -> DefFlags {
    EnumSet::of(&[
        DefFlag::dfNoReport,
        DefFlag::dfExcludeFromBuildRef,
        DefFlag::dfDontSave,
        DefFlag::dfDontAssign,
        DefFlag::dfSummaryExcludeNULL,
        DefFlag::dfInternalEditOnly,
        DefFlag::dfUnmappedFormID,
        DefFlag::dfUseLoadOrder,
        DefFlag::dfIsRecordFlags,
    ])
}

/// Upstream `_DefFlagsInheritDown`.
pub fn def_flags_inherit_down() -> DefFlags {
    EnumSet::of(&[
        DefFlag::dfCanContainFormID,
        DefFlag::dfCanContainReflection,
        DefFlag::dfCanContainUnmappedFormID,
    ])
}

/// Upstream `_DefFlagsDontClone`.
pub fn def_flags_dont_clone() -> DefFlags {
    EnumSet::of(&[DefFlag::dfTemplate])
}

/// Upstream `wbAssignThis`.
pub const ASSIGN_THIS: i32 = i32::MIN;
/// Upstream `wbAssignAdd`.
pub const ASSIGN_ADD: i32 = i32::MAX;

pascal_enum! {
    ElementType {
        etFile,
        etMainRecord,
        etGroupRecord,
        etSubRecord,
        etSubRecordStruct,
        etSubRecordArray,
        etSubRecordUnion,
        etArray,
        etStruct,
        etValue,
        etFlag,
        etStringListTerminator,
        etUnion,
        etStructChapter,
        etTemplate,
    }
}

pub type ElementTypes = EnumSet<ElementType>;

pascal_enum! {
    ElementState {
        esModified,
        esInternalModified,
        esUnsaved,
        esSortKeyValid,
        esExtendedSortKeyValid,
        esHidden,
        esParentHidden,
        esParentHiddenChecked,
        esNotReachable,
        esReachable,
        esTagged,
        esResolving,
        esNotSuitableToAddTo,
        /// Used in the script adapter as a default value.
        esDummy,
        esConstructionComplete,
        esDestroying,
        esChangeNotified,
        esModifiedUpdated,
        esSorting,
        esFound,
        esLocalized,
        esNotLocalized,
        esOptionalAndMissing,
        esEndingUpdate,
        // The following entries must match `ElementErrorType`.
        esReportedErrorReading,
        esReportedErrorUnusedData,
        esInternalLoad,
    }
}

pub type ElementStates = EnumSet<ElementState>;

pascal_enum! {
    EditType {
        etDefault,
        etComboBox,
        etCheckComboBox,
    }
}

pascal_enum! {
    ElementErrorType {
        eeReading,
        eeUnusedData,
    }
}

pascal_enum! {
    ResetModified {
        rmNo,
        rmYes,
        rmSetInternal,
    }
}

pascal_enum! {
    TriBool {
        tbUnknown,
        tbFalse,
        tbTrue,
    }
}

pascal_enum! {
    ContainerState {
        csInit,
        csInitOnce,
        csInitDone,
        csInitializing,
        csReseting,
        csRefsBuild,
        csAsCreatedEmpty,
        csSortedBySortOrder,
        csCollapsed,
        csExpanded,
        csConstructionCompleted,
    }
}

pub type ContainerStates = EnumSet<ContainerState>;

pascal_enum! {
    FileState {
        fsIsNew,
        fsIsCompareLoad,
        fsIsDeltaPatch,
        fsOnlyHeader,
        fsIsHardcoded,
        fsIsGameMaster,
        fsIsTemporary,
        fsHasNoFormID,
        fsRefsBuild,
        fsRefsBuilding,
        fsIsGhost,
        fsMemoryMapped,
        fsScanning,
        fsPseudoLight,
        fsLightCompatible,
        fsPseudoMedium,
        fsMediumCompatible,
        fsPseudoUpdate,
        fsUpdateCompatible,
        fsIsOfficial,
        fsCompareToHasSameMasters,
        fsAddToMap,
        fsMastersUpdating,
    }
}

pub type FileStates = EnumSet<FileState>;

pascal_enum! {
    CallbackType {
        ctToStr,
        ctToSortKey,
        ctCheck,
        ctToEditValue,
        ctEditType,
        ctEditInfo,
        ctLinksTo,
        ctBuildRef,
        ctFromEditValue,
        ctFromInt,
        ctFromNativeValue,
        ctToInt,
        ctToNativeValue,
        ctToSummary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_names_and_ordinals() {
        assert_eq!(DefType::dtStructChapter.ord(), 20);
        assert_eq!(DefType::dtLString.enum_name(), "dtLString");
        assert_eq!(DefFlag::ALL.len(), 46);
        assert_eq!(ConflictPriority::cpFormID.ord(), 8);
        assert!(ConflictPriority::cpBenign < ConflictPriority::cpNormal);
        assert_eq!(ConflictThis::ctConflictLoses.name(), "Conflict loser");
    }

    #[test]
    fn set_operations() {
        let mut set: DefFlags = EnumSet::of(&[DefFlag::dfNoReport, DefFlag::dfTemplate]);
        assert!(set.contains(DefFlag::dfTemplate) && !set.contains(DefFlag::dfCollapsed));
        set.include(DefFlag::dfAfterSetOnIDUpdate);
        assert_eq!(set.count(), 3);
        assert_eq!(set & def_flags_inherit_up(), EnumSet::of(&[DefFlag::dfNoReport]));
        assert_eq!(
            set - def_flags_dont_clone(),
            EnumSet::of(&[DefFlag::dfNoReport, DefFlag::dfAfterSetOnIDUpdate])
        );
        set.exclude(DefFlag::dfNoReport);
        assert_eq!(
            (set | def_flags_inherit_down()).iter().next(),
            Some(DefFlag::dfTemplate)
        );
        assert!(DefFlags::empty().is_empty());
        assert!(dt_arrays().contains(DefType::dtArray) && dt_non_values().contains(DefType::dtUnion));
    }

    #[test]
    fn signatures() {
        let sig = Signature::from_str("EDID").unwrap();
        assert_eq!(sig, Signature::new(b"EDID"));
        assert_eq!(sig.to_string(), "EDID");
        assert_eq!(Signature::from_int(sig.to_int()), sig);
        assert_eq!(sig.to_int(), 0x4449_4445);
        assert_eq!(Signature::from_str("EDIDX").unwrap(), sig);
        assert_eq!(
            Signature::from_str("EDI").unwrap_err().to_string(),
            "\"EDI\" is not a valid signature"
        );
        assert_eq!(Signature([0, b'I', b'A', b'D']).to_string(), "");
        assert_eq!(Signature([b'A', 0x80, 0, b'D']).to_string(), "A\u{20ac}");
        assert_eq!(
            KNOWN_SUB_RECORD_SIGNATURES[KnownSubRecord::ksrGridCell.ord()].to_string(),
            "XCLC"
        );
    }
}
