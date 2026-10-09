// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The asset formats of xEdit: the data format framework of
//! `wbDataFormat` with the NIF, material, LOD and FUZ formats built on it,
//! the NIF maths, the lightweight NIF scanner, and the operations of
//! Sniff on them.

pub mod asset;
pub mod data_format;
pub mod data_format_material;
pub mod data_format_misc;
pub mod data_format_nif;
pub mod data_format_nif_types;
pub mod imaging;
pub mod json;
pub mod lod;
pub mod mesh_optimize;
pub mod nif_math;
pub mod nif_scanner;
pub mod sniff;
pub mod variant;
