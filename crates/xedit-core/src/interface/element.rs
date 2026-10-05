// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! The element interfaces (`IwbElement` and its descendants).
//!
//! The traits grow with the port: a method is added when the first ported
//! caller needs it. The implementations are in the port of
//! `wbImplementation.pas`.

use std::sync::Arc;

use xedit_io::Encoding;

use super::def::{NamedDef, ValueDef};
use super::form_id::FormID;
use super::misc::Variant;
use super::types::{ElementType, TriBool};

/// A reference to an element, upstream `IwbElement`.
pub type ElementRef = Arc<dyn Element>;

/// An element argument that upstream allows to be `nil`.
pub type ElementArg<'a> = Option<&'a ElementRef>;

/// A reference to a file, upstream `IwbFile`.
pub type FileRef = Arc<dyn File>;

/// A reference to a main record, upstream `IwbMainRecord`.
pub type MainRecordRef = Arc<dyn MainRecord>;

/// The data of an element: the bytes from upstream `aBasePtr` up to `aEndPtr`.
/// `None` is a `nil` base pointer.
pub type DataPtr<'a> = Option<&'a [u8]>;

/// Upstream `IwbElement`.
pub trait Element: Send + Sync {
    fn get_full_path(&self) -> String;

    fn get_edit_value(&self) -> String;

    fn get_links_to(&self) -> Option<ElementRef>;

    fn get_element_type(&self) -> ElementType;

    fn get_def(&self) -> Option<Arc<dyn NamedDef>>;

    fn get_value_def(&self) -> Option<Arc<dyn ValueDef>>;

    fn get_native_value(&self) -> Variant;

    /// The element that contains this one.
    fn get_container(&self) -> Option<ElementRef>;

    /// Upstream `_File`: the file that contains this element.
    fn get_file(&self) -> Option<FileRef>;

    fn get_containing_main_record(&self) -> Option<MainRecordRef>;

    fn get_path(&self) -> String;

    /// Whether the element stores an ID into the string tables.
    fn get_localized(&self) -> TriBool;

    /// `Supports(element, IwbContainer)`.
    fn as_container(&self) -> Option<&dyn Container> {
        None
    }
}

/// Upstream `IwbFile`.
pub trait File: Container {
    /// Upstream `Encoding[aTranslatable]`: the encoding of the strings of the file.
    fn get_encoding(&self, translatable: bool) -> Encoding;

    fn get_is_localized(&self) -> bool;
}

/// Upstream `IwbMainRecord`.
pub trait MainRecord: Container {
    fn get_load_order_form_id(&self) -> FormID;
}

/// Upstream `IwbContainer`, with the methods of `IwbContainerBase`.
pub trait Container: Element {
    /// Upstream `ElementNativeValues[aPath]`: the native value of the element
    /// at `path`, or an empty variant when there is none.
    fn get_element_native_value(&self, path: &str) -> Variant;
}
