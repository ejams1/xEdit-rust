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

/// A reference to an element, upstream `IwbElement`.
pub type ElementRef = Arc<dyn Element>;

/// An element argument that upstream allows to be `nil`.
pub type ElementArg<'a> = Option<&'a ElementRef>;

/// The data of an element: the bytes from upstream `aBasePtr` up to `aEndPtr`.
/// `None` is a `nil` base pointer.
pub type DataPtr<'a> = Option<&'a [u8]>;

/// Upstream `IwbElement`.
pub trait Element: Send + Sync {
    fn get_full_path(&self) -> String;

    fn get_edit_value(&self) -> String;

    fn get_links_to(&self) -> Option<ElementRef>;
}
