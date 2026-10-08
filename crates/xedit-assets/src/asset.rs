// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One type for every file of the data format units, chosen by kind:
//! what the session commands and the tools work with.

use crate::data_format::{DfError, El, R, Tree};
use crate::data_format_material::MaterialFile;
use crate::data_format_misc::{MiscFile, create_misc_file};
use crate::data_format_nif::NifFile;

/// The kind of a data format file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// `TwbNifFile`: `*.nif` and `*.kf`.
    Nif,
    /// `TwbBGSMFile`.
    Bgsm,
    /// `TwbBGEMFile`.
    Bgem,
    /// `TwbLODSettingsTES5File`: `*.lod`.
    LodSettingsTes5,
    /// `TwbLODSettingsFO3File`: `*.dlodsettings`.
    LodSettingsFo3,
    /// `TwbLODTreeLSTFile`: `*.lst`.
    LodTreeLst,
    /// `TwbLODTreeBTTFile`: `*.btt` and `*.dtl`.
    LodTreeBtt,
    /// `TwbFUZFile`.
    Fuz,
    /// `TwbDDSFile`: the header of a DDS texture.
    Dds,
}

impl AssetKind {
    /// The names `kind` takes in requests, with the kind.
    pub const NAMES: &'static [(&'static str, AssetKind)] = &[
        ("nif", AssetKind::Nif),
        ("bgsm", AssetKind::Bgsm),
        ("bgem", AssetKind::Bgem),
        ("lod", AssetKind::LodSettingsTes5),
        ("dlodsettings", AssetKind::LodSettingsFo3),
        ("lst", AssetKind::LodTreeLst),
        ("btt", AssetKind::LodTreeBtt),
        ("fuz", AssetKind::Fuz),
        ("dds", AssetKind::Dds),
    ];

    pub fn name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(_, kind)| *kind == self)
            .map_or("nif", |(name, _)| name)
    }

    pub fn from_name(name: &str) -> Option<AssetKind> {
        Self::NAMES
            .iter()
            .find(|(known, _)| known.eq_ignore_ascii_case(name))
            .map(|(_, kind)| *kind)
    }

    /// The kind of a file by its extension.
    pub fn from_path(path: &str) -> Option<AssetKind> {
        let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match extension.as_str() {
            "nif" | "kf" => AssetKind::Nif,
            "bgsm" => AssetKind::Bgsm,
            "bgem" => AssetKind::Bgem,
            "lod" => AssetKind::LodSettingsTes5,
            "dlodsettings" => AssetKind::LodSettingsFo3,
            "lst" => AssetKind::LodTreeLst,
            "btt" | "dtl" => AssetKind::LodTreeBtt,
            "fuz" => AssetKind::Fuz,
            "dds" => AssetKind::Dds,
            _ => return None,
        })
    }
}

/// A file of one of the kinds, as an element tree.
pub struct AssetFile {
    pub kind: AssetKind,
    pub tree: Tree,
    pub root: El,
}

impl AssetFile {
    /// An empty file of the kind (`Create`).
    pub fn new(kind: AssetKind) -> R<AssetFile> {
        let (tree, root) = match kind {
            AssetKind::Nif => {
                let file = NifFile::new()?;
                (file.tree, file.root)
            }
            AssetKind::Bgsm => {
                let file = MaterialFile::new_bgsm()?;
                (file.tree, file.root)
            }
            AssetKind::Bgem => {
                let file = MaterialFile::new_bgem()?;
                (file.tree, file.root)
            }
            AssetKind::LodSettingsTes5 => create_misc_file(MiscFile::LodSettingsTes5)?,
            AssetKind::LodSettingsFo3 => create_misc_file(MiscFile::LodSettingsFo3)?,
            AssetKind::LodTreeLst => create_misc_file(MiscFile::LodTreeLst)?,
            AssetKind::LodTreeBtt => create_misc_file(MiscFile::LodTreeBtt)?,
            AssetKind::Fuz => create_misc_file(MiscFile::Fuz)?,
            AssetKind::Dds => create_misc_file(MiscFile::Dds)?,
        };
        Ok(AssetFile { kind, tree, root })
    }

    /// `LoadFromData`.
    pub fn load(kind: AssetKind, data: &[u8]) -> R<AssetFile> {
        let mut file = AssetFile::new(kind)?;
        file.tree.load_from_data(file.root, data)?;
        Ok(file)
    }

    /// `SaveToData`.
    pub fn save(&mut self) -> R<Vec<u8>> {
        self.tree.save_to_data(self.root)
    }

    /// `ToText`.
    pub fn to_text(&mut self) -> R<String> {
        self.tree.to_text(self.root, 0)
    }

    /// `ToJSON`. Materials have no JSON writer upstream.
    pub fn to_json(&mut self, compact: bool) -> R<String> {
        match self.kind {
            AssetKind::Bgsm | AssetKind::Bgem => Err(DfError::new("Not implemented")),
            _ => self.tree.to_json(self.root, compact),
        }
    }

    /// `FromJSON`: for materials the material editor's form.
    pub fn from_json(kind: AssetKind, text: &str) -> R<AssetFile> {
        match kind {
            AssetKind::Bgsm | AssetKind::Bgem => {
                let mut file = if kind == AssetKind::Bgsm {
                    MaterialFile::new_bgsm()?
                } else {
                    MaterialFile::new_bgem()?
                };
                file.from_json(text)?;
                Ok(AssetFile {
                    kind,
                    tree: file.tree,
                    root: file.root,
                })
            }
            _ => {
                let mut file = AssetFile::new(kind)?;
                file.tree.from_json(file.root, text)?;
                Ok(file)
            }
        }
    }
}
