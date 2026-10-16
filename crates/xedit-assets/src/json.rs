// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The `JsonDataObjects` document type of the data format units, now held by
//! `xedit_core::json` (the element serialization of `Core/wbHelpers.pas`
//! reads the same library, and `xedit-defs` cannot depend on this crate).
//! The data format units keep reporting upstream exceptions as `DfError`.

pub use xedit_core::json::{Json, JsonDataType, JsonError};

use crate::data_format::DfError;

impl From<JsonError> for DfError {
    fn from(error: JsonError) -> Self {
        DfError(error.0)
    }
}
