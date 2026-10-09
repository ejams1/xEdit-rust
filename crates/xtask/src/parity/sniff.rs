// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `cargo xtask parity sniff`: the operations of Sniff run on the corpus
//! archives by the port and by the oracle, compared file by file.
//!
//! Each case is an operation with the settings of its section in the
//! settings ini (`CASES`). For every archive of the games the operation
//! supports that holds files it processes, `Sniff.exe` of the release runs
//! in its automation mode (`-OP:<title> -I:<archive> -O:<folder> -S:<ini>
//! -LOG:<file> -skip:yes -threads:<n>`) and the port runs in this process
//! with the same settings, its outputs hashed as they come. Compared are:
//!
//! - the output of each file (an FNV-1a hash of the bytes written, or none),
//! - the error of each file Sniff skipped (`Skipped: <file>: <message>`;
//!   two access violations count as the same error, as the port names no
//!   address),
//! - the other lines of the log (the `Updated:` and `Unchanged:` lines and
//!   what the processor reports), as a multiset: Sniff's threads add their
//!   lines in the order they finish, the port in file order,
//! - the counts of the summary line, and the log file a processor writes
//!   itself (`{log}` in the settings is the path of that file).
//!
//! Sniff's threads share state and rarely fail a file with an access
//! violation that does not repeat when the file runs alone, or write it
//! differently; such a file, and every file whose output or error differs
//! from the port's (up to `MAX_ALONE` per archive), is run again on its own
//! (`-P:<file> -threads:1`) before it counts. The
//! oracle's results are cached in `<cache>/<tag>/sniff-oracle/<case>/`,
//! keyed by the archive and the settings. With `--sample <n>` the first `n`
//! files of each archive are unpacked into a folder, which is the input of
//! both (the loose file path of Sniff); without it the archive is.
//!
//! A case whose operation copies from a source folder (`Prep`) gets one
//! made by the port itself over the unpacked files: `{source}` in its
//! settings is that folder. A case whose operation reads files beside the
//! input (`Add blocks from skeleton`) gets an input folder of its own
//! (`Prep::DeathSkeleton`).
//!
//! Sniff starts hidden on the harness's own desktop (`hidden.rs`). The work
//! folder of an archive is removed once it compares equal, and only the
//! outputs that differ stay otherwise, unless `--keep`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use xedit_assets::sniff::main_form::{OutputSink, RunError, RunOptions, run as port_run};
use xedit_assets::sniff::processor::{
    GameType, MemIniFile, extract_file_name, same_text, string_list_file_bytes, string_list_lines,
};
use xedit_assets::sniff::procs::PROCS;
use xedit_io::archive::Archive;
use xedit_io::encoding::ansi_string;

use super::hidden::HiddenCommand;
use super::nif::{archive_key, fnv, windows_path};
use super::{GAMES, Game, cache_dir, required_var};

const USAGE: &str = "usage: cargo xtask parity sniff [--case <name or name part>]... [--game <game>]... \
                     [--archive <name part>]... [--sample <n>] [--threads <n>] [--show <n>] [--keep] [--refresh-oracle] [--list]";

/// A run of an operation with its settings.
struct Case {
    /// The name of the case on the command line and in the report.
    name: &'static str,
    /// The title of the operation (`-OP:`).
    operation: &'static str,
    /// What the case needs besides the archive: the source folder of the
    /// operations that copy from the files of another one, or an input
    /// folder of its own.
    prep: Prep,
    /// The values of the operation's section of the settings ini. `{log}`
    /// is replaced with the path of a log file the processor writes and
    /// `{source}` with the prepared source folder.
    settings: &'static [(&'static str, &'static str)],
}

/// The preparation of a case whose settings name `{source}` or whose
/// input is not the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Prep {
    #[default]
    None,
    /// `Copy anim priorities`: the priorities of the controlled blocks of
    /// the KF files set to 33 (the universal tweaker).
    Priorities,
    /// `Copy anim controlled blocks`: every controlled block of the KF
    /// files renamed to `XYZ`, so the destination has none of the tokens
    /// (the universal tweaker).
    RenameControlledBlocks,
    /// The transform of every `NiAVObject` scaled by 1.5 (the universal
    /// tweaker).
    TransformScale,
    /// Every texture name of the shader texture sets set to `x.dds` (the
    /// universal tweaker).
    Textures,
    /// The transform of every node baked into the geometry (`Apply
    /// transformation`), so the geometry of the files differs.
    Baked,
    /// The input is a folder with a `death.kf` of the archive and a
    /// `skeleton.nif` beside it (`Add blocks from skeleton` reads one from
    /// the folder of the animation).
    DeathSkeleton,
}

impl Prep {
    fn name(self) -> &'static str {
        match self {
            Prep::None => "",
            Prep::Priorities => "priorities",
            Prep::RenameControlledBlocks => "rename-controlled",
            Prep::TransformScale => "transform-scale",
            Prep::Textures => "textures",
            Prep::Baked => "baked",
            Prep::DeathSkeleton => "",
        }
    }

    /// The operation the port runs over the unpacked files to make the
    /// source folder, with its settings; `None` for the preps that make
    /// the input folder.
    fn operation(self) -> Option<(&'static str, &'static str)> {
        Some(match self {
            Prep::Priorities => (
                "Universal tweaker",
                "[Universaltweaker]\r\nProcessedFiles=*.kf\r\nsBlocks=NiControllerSequence\r\n\
                 sPath=Controlled Blocks\\[*]\\Priority\r\nsValue=33\r\n",
            ),
            Prep::RenameControlledBlocks => (
                "Universal tweaker",
                "[Universaltweaker]\r\nProcessedFiles=*.kf\r\nsBlocks=NiControllerSequence\r\n\
                 sPath=Controlled Blocks\\[*]\\Node Name\r\nsValue=XYZ\r\n",
            ),
            Prep::TransformScale => (
                "Universal tweaker",
                "[Universaltweaker]\r\nProcessedFiles=*.nif\r\nsBlocks=NiAVObject\r\nbDescendants=1\r\n\
                 sPath=Transform\\Scale\r\niValueMode=2\r\nsValue=1.5\r\n",
            ),
            Prep::Textures => (
                "Universal tweaker",
                "[Universaltweaker]\r\nProcessedFiles=*.nif\r\nsBlocks=BSShaderTextureSet\r\n\
                 sPath=Textures\\[*]\r\nsValue=x.dds\r\n",
            ),
            Prep::Baked => ("Apply transformation", "[Applytransformation]\r\n"),
            Prep::None | Prep::DeathSkeleton => return None,
        })
    }
}

/// The cases of the check: every ported operation with its defaults, and
/// with the settings that take other paths through it.
const CASES: &[Case] = &[
    Case {
        name: "tangents",
        operation: "Update tangents and binormals",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "tangents-add",
        operation: "Update tangents and binormals",
        settings: &[("bAddIfMissing", "1"), ("bFaceNormals", "1")],
        prep: Prep::None,
    },
    Case {
        name: "bounds",
        operation: "Update bounds",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "optimize",
        operation: "Optimize mesh",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "optimize-vertex-cache",
        operation: "Optimize mesh",
        settings: &[("bOverdraw", "0"), ("bVertexFetch", "0")],
        prep: Prep::None,
    },
    Case {
        name: "optimize-overdraw",
        operation: "Optimize mesh",
        settings: &[("bVertexCache", "0"), ("bVertexFetch", "0")],
        prep: Prep::None,
    },
    Case {
        name: "optimize-fetch",
        operation: "Optimize mesh",
        settings: &[("bVertexCache", "0"), ("bOverdraw", "0")],
        prep: Prep::None,
    },
    Case {
        name: "optimize-triangulate",
        operation: "Optimize mesh",
        settings: &[
            ("bTriangulate", "1"),
            ("bVertexCache", "0"),
            ("bOverdraw", "0"),
            ("bVertexFetch", "0"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "optimize-stripify",
        operation: "Optimize mesh",
        settings: &[("bStripify", "1"), ("bOverdraw", "0"), ("bVertexFetch", "0")],
        prep: Prep::None,
    },
    Case {
        name: "replace-assets",
        operation: "Search and replace assets",
        settings: &[
            ("sReplacements", "textures\\#13#10tex\\#13#10.dds#13#10.DDS#13#10"),
            ("bFixAbsolute", "1"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "replace-assets-regexp",
        operation: "Search and replace assets",
        settings: &[
            (
                "sReplacements",
                "^(.+)_(d|n)\\.dds$#13#10$1_\\u2.dds#13#10#13#10data\\#13#10",
            ),
            ("bRegExp", "1"),
            ("bReportOnly", "1"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "json",
        operation: "Convert to and from JSON",
        settings: &[("bToJson", "1"), ("sDigits", "8"), ("iRotation", "1")],
        prep: Prep::None,
    },
    Case {
        name: "tweaker",
        operation: "Universal tweaker",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "tweaker-report",
        operation: "Universal tweaker",
        settings: &[
            ("bReportOnly", "1"),
            ("sBlocks", "NiAVObject"),
            ("bDescendants", "1"),
            ("sPath", "Name"),
            ("iValueMode", "3"),
            ("sValue", "x$1"),
            ("bOldValueCheck", "1"),
            ("iOldValueMode", "10"),
            ("sOldValue", "^(\\w+)Node"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "tweaker-math",
        operation: "Universal tweaker",
        settings: &[
            ("sBlocks", "NiAVObject"),
            ("bDescendants", "1"),
            ("sPath", "Transform\\Scale"),
            ("iValueMode", "2"),
            ("sValue", "1.5"),
            ("bOldValueCheck", "1"),
            ("sOldPath", "Flags"),
            ("iOldValueMode", "8"),
            ("sOldValue", "2"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "tweaker-array",
        operation: "Universal tweaker",
        settings: &[
            ("sBlocks", "BSShaderTextureSet"),
            ("sPath", "Textures\\[*]"),
            ("iValueMode", "4"),
            ("sValue", "x\\"),
            ("bOldValueCheck", "1"),
            ("iOldValueMode", "6"),
            ("sOldValue", "textures\\"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "fixer",
        operation: "Universal fixer",
        settings: &[("bSaveLog", "1"), ("sLogFile", "{log}")],
        prep: Prep::None,
    },
    Case {
        name: "apply-transform",
        operation: "Apply transformation",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "apply-transform-all",
        operation: "Apply transformation",
        settings: &[
            ("bSkipSkinned", "0"),
            ("bSkipAnimated", "0"),
            ("bSkipCollision", "0"),
            ("bSkipRoot", "0"),
            ("bSkipControllerManager", "0"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "adjust-transform",
        operation: "Adjust transformation",
        settings: &[("sPosZ", "10.5"), ("sScale", "2")],
        prep: Prep::None,
    },
    Case {
        name: "adjust-transform-names",
        operation: "Adjust transformation",
        settings: &[
            ("sNames", "Bip01, Scene Root"),
            ("bExactMatch", "0"),
            ("iMode", "3"),
            ("sRotY", "45"),
            ("sPosX", "-3"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "attach-parent",
        operation: "Attach parent NiNode",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "remove-nodes",
        operation: "Remove nodes",
        settings: &[("sNames", "EditorMarker"), ("bExactMatch", "0")],
        prep: Prep::None,
    },
    Case {
        name: "remove-nodes-type",
        operation: "Remove nodes",
        settings: &[("iMode", "2"), ("sType", "NiStringExtraData")],
        prep: Prep::None,
    },
    Case {
        name: "remove-unused",
        operation: "Remove unused nodes",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "convert-block",
        operation: "Convert block type",
        settings: &[("sNodeFrom", "NiNode"), ("sNodeTo", "BSFadeNode"), ("bRoot", "1")],
        prep: Prep::None,
    },
    Case {
        name: "convert-block-all",
        operation: "Convert block type",
        settings: &[("sNodeFrom", "BSFadeNode"), ("sNodeTo", "NiNode")],
        prep: Prep::None,
    },
    Case {
        name: "unskin",
        operation: "Unskin mesh",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "missing-names",
        operation: "Set missing names",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "fix-kf",
        operation: "Fix 3DS exported KF",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "copy-geometry",
        operation: "Copy geometry blocks",
        settings: &[("sSourceDirectory", "{source}")],
        prep: Prep::Baked,
    },
    Case {
        name: "copy-geometry-transform",
        operation: "Copy geometry blocks",
        settings: &[
            ("sSourceDirectory", "{source}"),
            ("bCopyGeom", "0"),
            ("bCopyTransform", "1"),
        ],
        prep: Prep::TransformScale,
    },
    Case {
        name: "copy-geometry-shader",
        operation: "Copy geometry blocks",
        settings: &[
            ("sSourceDirectory", "{source}"),
            ("bCopyGeom", "0"),
            ("bCopyShader", "1"),
            ("bCopyTextureSet", "1"),
        ],
        prep: Prep::Textures,
    },
    // The single file mode (`bMatchingFiles=0`) has no case: `Sniff.exe`
    // hangs in it (a dialog, with one thread too) as soon as a block of the
    // source matches one of the file (`Copy geometry blocks`), so the mode
    // is covered by the unit test of the processor instead.
    Case {
        name: "vertex-paint",
        operation: "Vertex color painting",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "vertex-paint-adjust",
        operation: "Vertex color painting",
        settings: &[
            ("iMode", "1"),
            ("iAdjustMod", "0"),
            ("sAdjustH", "0.5"),
            ("sAdjustS", "1.2"),
            ("sAdjustL", "1.1"),
            ("sAdjustA", "0.9"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "vertex-paint-adjust-add",
        operation: "Vertex color painting",
        settings: &[
            ("iMode", "1"),
            ("iAdjustMod", "1"),
            ("sAdjustH", "0.1"),
            ("sAdjustS", "-0.2"),
            ("sAdjustL", "0.1"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "vertex-paint-remove",
        operation: "Vertex color painting",
        settings: &[("iMode", "2"), ("bAllWhite", "1"), ("sName", "Tri")],
        prep: Prep::None,
    },
    Case {
        name: "vertex-paint-replace",
        operation: "Vertex color painting",
        settings: &[("iMode", "3"), ("sColor2", "FF00FF00"), ("bSkipColor", "1")],
        prep: Prep::None,
    },
    Case {
        name: "group-shapes",
        operation: "Group shapes",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "group-shapes-split",
        operation: "Group shapes",
        settings: &[("bSplit", "1"), ("bAllFeatures", "1")],
        prep: Prep::None,
    },
    Case {
        name: "merge-shapes",
        operation: "Merge shapes",
        settings: &[("sNames", "Scene Root")],
        prep: Prep::None,
    },
    Case {
        name: "merge-shapes-exact",
        operation: "Merge shapes",
        settings: &[("sNames", "Scene Root"), ("bExactMatch", "1")],
        prep: Prep::None,
    },
    Case {
        name: "merge-properties",
        operation: "Merge properties",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "merge-properties-named",
        operation: "Merge properties",
        settings: &[
            ("bIgnoreName", "0"),
            (
                "sBlocks",
                "NiMaterialProperty,NiAlphaProperty,NiTexturingProperty,BSShaderPPLightingProperty,BSLightingShaderProperty",
            ),
        ],
        prep: Prep::None,
    },
    Case {
        name: "lod-node",
        operation: "Add NiLODNode",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "lod-node-screen",
        operation: "Add NiLODNode",
        settings: &[("sLODData", "NiScreenLODData"), ("sProportions", "0.5#13#10#13#100.25")],
        prep: Prep::None,
    },
    Case {
        name: "bounding-box",
        operation: "Add bounding box",
        settings: &[("sCenterZ", "12.5"), ("sExtentX", "4"), ("sFlags", "4")],
        prep: Prep::None,
    },
    Case {
        name: "root-collision",
        operation: "Add RootCollisionNode",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "find-textures",
        operation: "Find textures",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "find-textures-filter",
        operation: "Find textures",
        settings: &[("sFormats", "71,77"), ("sMipMaps", "Yes"), ("sBlock Compressed", "Yes")],
        prep: Prep::None,
    },
    Case {
        name: "find-textures-header",
        operation: "Find textures",
        settings: &[("bHeaderDump", "1"), ("sFormats", "71"), ("sMipMaps", "Yes")],
        prep: Prep::None,
    },
    Case {
        name: "find-textures-copy",
        operation: "Find textures",
        settings: &[("bReportOnly", "0"), ("sFormats", "71")],
        prep: Prep::None,
    },
    Case {
        name: "copy-controlled",
        operation: "Copy anim controlled blocks",
        settings: &[("sSourceDirectory", "{source}")],
        prep: Prep::RenameControlledBlocks,
    },
    Case {
        name: "copy-priorities",
        operation: "Copy anim priorities",
        settings: &[("sSourceDirectory", "{source}")],
        prep: Prep::Priorities,
    },
    Case {
        name: "remove-controlled",
        operation: "Remove controlled blocks",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "remove-controlled-others",
        operation: "Remove controlled blocks",
        settings: &[
            ("sNames", "Bip01 Spine, Tail"),
            ("bExactMatch", "0"),
            ("bNotMatching", "1"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "quadratic-to-linear",
        operation: "Quadratic to linear anim",
        settings: &[("sNames", "Bip01"), ("bExactMatch", "0")],
        prep: Prep::None,
    },
    Case {
        name: "optimize-kf",
        operation: "Optimize Animations",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "headtracking",
        operation: "Add headtracking anim",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "headtracking-clamp",
        operation: "Add headtracking anim",
        settings: &[
            ("bCycleClampOnly", "1"),
            ("sKeyValue14", "-1.5"),
            ("sKeyValue23", "0.5"),
            ("sKeyTime2", "33"),
            ("sKeyTime3", "66"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "facial",
        operation: "Add facial anim",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "facial-remove",
        operation: "Add facial anim",
        settings: &[
            ("bRemoveExisting", "1"),
            (
                "sMods",
                "99 Anger 0 1#13#10100 Happy 0.5 0.5 1 0#13#10100 HeadYaw 0 0.4",
            ),
        ],
        prep: Prep::None,
    },
    Case {
        name: "jam-anim",
        operation: "Add NiTransformData",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "jam-anim-translation",
        operation: "Add NiTransformData",
        settings: &[("bAddRotation", "0")],
        prep: Prep::None,
    },
    Case {
        name: "wei-explosion",
        operation: "Weijiesen's blow up thing",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "skeleton-death",
        operation: "Add blocks from skeleton",
        settings: &[("sNames", "Weapon,HeadAnims")],
        prep: Prep::DeathSkeleton,
    },
    Case {
        name: "skeleton-death-any",
        operation: "Add blocks from skeleton",
        settings: &[("bExactMatch", "0"), ("sNames", "Bip01")],
        prep: Prep::DeathSkeleton,
    },
    Case {
        name: "havok-settings",
        operation: "Update Havok settings",
        settings: &[
            ("sMass", "5"),
            ("sFriction", "0.25"),
            ("sRestitution", "0.4"),
            ("sRadius", " 0.1 "),
        ],
        prep: Prep::None,
    },
    Case {
        name: "inertia",
        operation: "Update Havok inertia",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "inertia-penetration",
        operation: "Update Havok inertia",
        settings: &[
            ("bPenetrationUpdate", "1"),
            ("bPenetrationStaticsUpdate", "1"),
            ("sDepthMult", "0.3"),
            ("sMult", "\"1 Head=4\",\"2 Body=\""),
        ],
        prep: Prep::None,
    },
    Case {
        name: "ragdoll",
        operation: "Update ragdoll constraint",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "ragdoll-convert",
        operation: "Update ragdoll constraint",
        settings: &[("bConvert", "1")],
        prep: Prep::None,
    },
    Case {
        name: "havok-material",
        operation: "Search for Havok material",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "havok-material-replace",
        operation: "Search for Havok material",
        settings: &[
            ("iGame", "1"),
            ("sMaterialSearch", "FO_HAV_MAT_STONE"),
            ("sMaterialReplace", "fo_hav_mat_metal"),
            ("bSkipRoot", "1"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "shader-flags",
        operation: "Update shader flags",
        settings: &[("iGame", "1"), ("iFlags", "3"), ("iFlags2", "16")],
        prep: Prep::None,
    },
    Case {
        name: "shader-flags-report",
        operation: "Update shader flags",
        settings: &[("iGame", "0"), ("iMode", "2"), ("iFlags", "4096"), ("bReportOnly", "1")],
        prep: Prep::None,
    },
    Case {
        name: "walls-reflection",
        operation: "Real Time Reflections - NVSE",
        settings: &[("sNormalIntensity", "0.5")],
        prep: Prep::None,
    },
    Case {
        name: "check-errors",
        operation: "Check for errors",
        settings: &[("ProcessedFiles", "*.nif, *.kf")],
        prep: Prep::None,
    },
    Case {
        name: "check-errors-all",
        operation: "Check for errors",
        settings: &[
            ("ProcessedFiles", "*.nif, *.kf"),
            ("Check NiAlphaProperty", "1"),
            ("Clamped tiling UVs", "1"),
            ("Optional checks", "1"),
            ("Repeated denegerate tris in strips", "1"),
            ("Unsupported mesh formats", "1"),
        ],
        prep: Prep::None,
    },
    Case {
        name: "check-errors-dds",
        operation: "Check for errors",
        settings: &[("ProcessedFiles", "*.dds"), ("Unsupported texture formats", "1")],
        prep: Prep::None,
    },
    Case {
        name: "transform-info",
        operation: "Transform information",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "transform-info-no-scale",
        operation: "Transform information",
        settings: &[("bRotation", "0"), ("bSkipEmpty", "0")],
        prep: Prep::None,
    },
    Case {
        name: "analyze-mesh",
        operation: "Analyze mesh",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "analyze-mesh-shapes",
        operation: "Analyze mesh",
        settings: &[("bPerShape", "1"), ("bThreshold", "0"), ("sCacheSize", "32")],
        prep: Prep::None,
    },
    Case {
        name: "havok-info",
        operation: "Havok information",
        settings: &[(
            "sFields",
            "\"Inertia Tensor\",Friction,\"Motion System\",\"Penetration Depth\"",
        )],
        prep: Prep::None,
    },
    Case {
        name: "havok-info-same-line",
        operation: "Havok information",
        settings: &[
            ("bSameLine", "1"),
            ("sFields", "Restitution,\"Max Linear Velocity\",\"Inertia Tensor\""),
        ],
        prep: Prep::None,
    },
    Case {
        name: "unwelded",
        operation: "Find unwelded vertices",
        settings: &[],
        prep: Prep::None,
    },
    Case {
        name: "unwelded-report",
        operation: "Find unwelded vertices",
        settings: &[("sDistance", "0.01"), ("bSkipSame", "1"), ("bReportVertices", "1")],
        prep: Prep::None,
    },
    Case {
        name: "draw-calls",
        operation: "Find excessive draw calls",
        settings: &[("sCallsNum", "3")],
        prep: Prep::None,
    },
    Case {
        name: "find-uvs",
        operation: "Find UVs",
        settings: &[("sUMax", "1"), ("sVMax", "1.5")],
        prep: Prep::None,
    },
    Case {
        name: "soft-particles",
        operation: "Vanilla Plus Particles - NVSE",
        settings: &[],
        prep: Prep::None,
    },
];

/// The game of the harness for a game of Sniff.
fn harness_game(game: GameType) -> &'static str {
    match game {
        GameType::Tes3 => "tes3",
        GameType::Tes4 => "tes4",
        GameType::Fo3 => "fo3",
        GameType::Fnv => "fnv",
        GameType::Tes5 => "tes5",
        GameType::Sse => "sse",
        GameType::Fo4 => "fo4",
    }
}

struct Options {
    cases: Vec<String>,
    games: Vec<String>,
    archives: Vec<String>,
    sample: Option<usize>,
    threads: usize,
    /// How many files of each outcome and lines of each log difference the
    /// report names for an archive.
    show: usize,
    /// Keep the work folders (Sniff's outputs, the logs) of the archives
    /// that compared equal, and every output of the others.
    keep: bool,
    /// Run Sniff again and replace the cached results.
    refresh_oracle: bool,
    list: bool,
}

fn parse(args: &[&str]) -> Result<Options> {
    let mut options = Options {
        cases: Vec::new(),
        games: Vec::new(),
        archives: Vec::new(),
        sample: None,
        threads: std::thread::available_parallelism().map_or(4, |count| count.get()),
        show: 3,
        keep: false,
        refresh_oracle: false,
        list: false,
    };
    let mut rest = args.iter();
    while let Some(&arg) = rest.next() {
        match arg {
            "--case" => options.cases.push(rest.next().context(USAGE)?.to_lowercase()),
            "--game" => options.games.push(rest.next().context(USAGE)?.to_lowercase()),
            "--archive" => options.archives.push(rest.next().context(USAGE)?.to_lowercase()),
            "--sample" => options.sample = Some(rest.next().context(USAGE)?.parse()?),
            "--threads" => options.threads = rest.next().context(USAGE)?.parse::<usize>()?.max(1),
            "--show" => options.show = rest.next().context(USAGE)?.parse()?,
            "--keep" => options.keep = true,
            "--refresh-oracle" => options.refresh_oracle = true,
            "--list" => options.list = true,
            _ => bail!(USAGE),
        }
    }
    Ok(options)
}

/// The settings ini of a case, with `{log}` as `log`.
fn settings_text(case: &Case, log: &Path, source: &Path) -> String {
    let section = case.operation.replace(' ', "");
    let mut text = format!("[Main]\r\nPopupWarning=0\r\n[{section}]\r\n");
    for (name, value) in case.settings {
        let value = value
            .replace("{log}", &windows_path(log))
            .replace("{source}", &windows_path(source));
        text.push_str(&format!("{name}={value}\r\n"));
    }
    text
}

/// Whether a case names the prepared source folder.
fn uses_source(case: &Case) -> bool {
    case.settings.iter().any(|(_, value)| value.contains("{source}"))
}

/// The path of a file up to and including its folder delimiter.
fn folder_of(name: &str) -> String {
    match name.rfind(['\\', '/']) {
        Some(index) => name[..=index].to_owned(),
        None => String::new(),
    }
}

/// The folder `Add blocks from skeleton` reads: the `death.kf` of the
/// archive with a `skeleton.nif` beside it (the operation reads the
/// skeleton from the folder of the animation). `false` when the archive
/// holds no `death.kf`.
fn death_skeleton_folder(archive: &Archive, dir: &Path) -> Result<bool> {
    let death = archive
        .files()
        .iter()
        .find(|entry| same_text(extract_file_name(&entry.name.replace('\\', "/")), "death.kf"));
    let Some(death) = death else {
        return Ok(false);
    };
    // The `skeleton.nif` of the same folder, as a game ships them; the
    // first one of the archive when the folder has none.
    let skeleton = archive
        .files()
        .iter()
        .find(|entry| {
            same_text(extract_file_name(&entry.name.replace('\\', "/")), "skeleton.nif")
                && folder_of(&entry.name).eq_ignore_ascii_case(&folder_of(&death.name))
        })
        .or_else(|| {
            archive
                .files()
                .iter()
                .find(|entry| same_text(extract_file_name(&entry.name.replace('\\', "/")), "skeleton.nif"))
        });
    let Some(skeleton) = skeleton else {
        return Ok(false);
    };

    let done = dir.join(".complete");
    if done.exists() {
        return Ok(true);
    }
    let _ = fs::remove_dir_all(dir);
    let death_folder = folder_of(&death.name);
    for (entry, name) in [
        (death, death.name.clone()),
        (skeleton, format!("{death_folder}skeleton.nif")),
    ] {
        let path = dir.join(name.replace('\\', "/"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            &path,
            archive.unpack_entry(entry).map_err(|error| anyhow::anyhow!(error.0))?,
        )?;
    }
    fs::create_dir_all(dir)?;
    fs::write(done, "")?;
    Ok(true)
}

/// What one side gave for one input (an archive or a sample folder).
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
struct Results {
    /// The hash of each output, by lower-case path.
    outputs: BTreeMap<String, u64>,
    /// The error of each skipped file, by lower-case path.
    errors: BTreeMap<String, String>,
    /// The other lines of the log, sorted.
    log: Vec<String>,
    /// The lines of the processor's own log file, sorted.
    extra: Vec<String>,
    /// The counts of the summary line: updated and processed.
    updated: usize,
    processed: usize,
    /// The files whose output or error differed from the port's and that
    /// Sniff ran again alone (`rerun_differences`).
    /// (Named `alone_runs`: the `alone` and `run_alone` of earlier caches
    /// missed the renamed outputs of the JSON converter and the log lines
    /// of a file that failed among the others, so those caches run their
    /// differences alone again.)
    #[serde(default)]
    alone_runs: Vec<String>,
}

/// Files that differ from the port's are run again alone, up to this many
/// per archive: more is a difference of the port, not a race of Sniff.
const MAX_ALONE: usize = 40;

/// Whether an error of Sniff is a crash of its threads rather than of the
/// file: an access violation or an invalid pointer.
fn is_crash(message: &str) -> bool {
    message.starts_with("Access violation") || message.contains("Invalid pointer operation")
}

/// Two errors are the same; two access violations are.
fn same_error(a: &str, b: &str) -> bool {
    a == b || (a.starts_with("Access violation") && b.starts_with("Access violation"))
}

/// The file of a `Skipped: <file>: <message>` line: the path ends at the
/// first `: ` after one of the extensions.
fn split_skipped<'a>(rest: &'a str, extensions: &[String]) -> Option<(&'a str, &'a str)> {
    let lower = rest.to_lowercase();
    let end = extensions
        .iter()
        .filter_map(|ext| {
            let pattern = format!(".{ext}: ");
            lower.find(&pattern).map(|index| index + pattern.len() - 2)
        })
        .min()?;
    Some((&rest[..end], &rest[end + 2..]))
}

/// Reads the lines of a log into `results`: the skipped files, the summary
/// line and the rest.
fn read_log(lines: &[String], extensions: &[String], results: &mut Results) {
    for line in lines {
        if let Some(rest) = line.strip_prefix("Skipped: ")
            && let Some((name, message)) = split_skipped(rest, extensions)
        {
            results.errors.insert(name.to_lowercase(), message.to_owned());
            continue;
        }
        if let Some(rest) = line.strip_prefix("Done. Updated ") {
            // `Done. Updated %d files out of %d, elapsed time %s.`
            let mut numbers = rest
                .split(|c: char| !c.is_ascii_digit())
                .filter(|part| !part.is_empty())
                .map(|part| part.parse::<usize>().unwrap_or(0));
            results.updated = numbers.next().unwrap_or(0);
            results.processed = numbers.next().unwrap_or(0);
            continue;
        }
        results.log.push(line.clone());
    }
    results.log.sort();
}

/// The lines of a text file as `TStringList.LoadFromFile` reads it.
fn file_lines(path: &Path) -> Vec<String> {
    match fs::read(path) {
        Ok(bytes) => string_list_lines(&ansi_string(&bytes)),
        Err(_) => Vec::new(),
    }
}

/// The outputs below `dir` by lower-case relative path, hashed.
fn hash_outputs(dir: &Path, outputs: &mut BTreeMap<String, u64>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(dir)?.to_string_lossy().replace('/', "\\");
        outputs.insert(relative.to_lowercase(), fnv(&fs::read(entry.path())?));
    }
    Ok(())
}

/// Frees the work folder of an archive once it is compared: removed when
/// everything was equal, else only Sniff's outputs of the files that
/// differ, the logs and the settings stay.
fn clean_work(work: &Path, differing: &BTreeSet<String>, log_differs: bool) -> Result<()> {
    if differing.is_empty() && !log_differs {
        if work.exists() {
            fs::remove_dir_all(work).with_context(|| format!("removing {}", work.display()))?;
        }
        return Ok(());
    }
    let _ = fs::remove_file(work.join("Sniff.exe"));
    for folder in ["oracle-out", "port-out"] {
        let dir = work.join(folder);
        if !dir.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(&dir) {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry.path().strip_prefix(&dir)?.to_string_lossy().replace('/', "\\");
            if !differing.contains(&relative.to_lowercase()) {
                fs::remove_file(entry.path())?;
            }
        }
    }
    Ok(())
}

/// Runs Sniff on `input` and returns what it gave.
#[allow(clippy::too_many_arguments)]
fn run_sniff(
    sniff: &Path,
    work: &Path,
    case: &Case,
    source: &Path,
    input: &Path,
    path_filter: Option<&str>,
    threads: usize,
    extensions: &[String],
) -> Result<Results> {
    fs::create_dir_all(work)?;
    let exe = work.join("Sniff.exe");
    if !exe.exists() {
        fs::copy(sniff, &exe)?;
    }
    let out = work.join("oracle-out");
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out)?;
    let extra = work.join("oracle-extra.log");
    let _ = fs::remove_file(&extra);
    let ini = work.join("oracle.ini");
    fs::write(&ini, settings_text(case, &extra, source))?;
    let log = work.join("oracle.log");
    let _ = fs::remove_file(&log);
    let mut command = HiddenCommand::new(&exe);
    command
        .current_dir(work)
        .arg(format!("-S:{}", windows_path(&ini)))
        .arg(format!("-OP:{}", case.operation))
        .arg(format!("-I:{}", windows_path(input)))
        .arg(format!("-O:{}", windows_path(&out)))
        .arg(format!("-LOG:{}", windows_path(&log)))
        .arg("-skip:yes")
        .arg("-all:no")
        .arg(if input.is_dir() { "-subdir:yes" } else { "-subdir:no" })
        .arg(format!("-threads:{threads}"));
    if let Some(filter) = path_filter {
        command.arg(format!("-P:{filter}"));
    }
    let mut child = command.spawn().context("starting Sniff")?;
    let size = if input.is_file() { fs::metadata(input)?.len() } else { 0 };
    let timeout = Duration::from_secs(600 + size / 1_000_000);
    match child.wait_timeout(timeout)? {
        Some(0) => {}
        Some(code) => bail!("Sniff ended with exit code {code:#x}"),
        None => bail!("Sniff did not finish within {} s", timeout.as_secs()),
    }
    ensure!(
        log.exists(),
        "Sniff wrote no log {} (an error before the run?)",
        log.display()
    );
    let mut results = Results::default();
    read_log(&file_lines(&log), extensions, &mut results);
    hash_outputs(&out, &mut results.outputs)?;
    results.extra = file_lines(&extra);
    results.extra.sort();
    let _ = fs::remove_dir_all(&out);
    Ok(results)
}

/// Runs the files whose error is a crash again, each alone on one thread,
/// and puts what they give in place of the crash.
#[allow(clippy::too_many_arguments)]
fn rerun_crashes(
    sniff: &Path,
    work: &Path,
    case: &Case,
    source: &Path,
    input: &Path,
    extensions: &[String],
    results: &mut Results,
) -> Result<()> {
    let crashed: Vec<String> = results
        .errors
        .iter()
        .filter(|(_, message)| is_crash(message))
        .map(|(name, _)| name.clone())
        .collect();
    for name in crashed {
        let rerun = run_sniff(sniff, work, case, source, input, Some(&name), 1, extensions)?;
        let error = rerun.errors.get(&name).cloned();
        println!("oracle rerun  {name}: {}", error.as_deref().unwrap_or("no error"));
        results.errors.remove(&name);
        if let Some(error) = error {
            results.errors.insert(name.clone(), error);
        }
        // The output may be named otherwise (`Convert to and from JSON`
        // writes `<name>.json`); the run alone wrote only this file's.
        results
            .outputs
            .extend(rerun.outputs.iter().map(|(output, hash)| (output.clone(), *hash)));
        results.log.extend(rerun.log);
        results.log.sort();
        results.extra.extend(rerun.extra);
        results.extra.sort();
        results.updated += rerun.updated;
        results.alone_runs.push(name);
    }
    results.alone_runs.sort();
    Ok(())
}

/// Runs Sniff again alone on each file whose output or error differs from
/// the port's and that has not been run alone yet: Sniff's threads share
/// state, and rarely one writes a file differently from a run of the file
/// alone (`DLC05ElevatorNavCut.nif` in `Remove nodes`). The output and the
/// error of the run alone count. Returns whether the results changed.
#[allow(clippy::too_many_arguments)]
fn rerun_differences(
    sniff: &Path,
    work: &Path,
    case: &Case,
    source: &Path,
    input: &Path,
    extensions: &[String],
    oracle: &mut Results,
    port: &Results,
) -> Result<bool> {
    let mut names: Vec<String> = oracle
        .outputs
        .keys()
        .chain(oracle.errors.keys())
        .chain(port.outputs.keys())
        .chain(port.errors.keys())
        .filter(|name| {
            let errors = match (port.errors.get(*name), oracle.errors.get(*name)) {
                (Some(a), Some(b)) => same_error(a, b),
                (None, None) => true,
                _ => false,
            };
            !(errors && port.outputs.get(*name) == oracle.outputs.get(*name))
        })
        .filter(|name| !oracle.alone_runs.contains(&input_name(name, extensions)))
        .cloned()
        .collect();
    names.sort();
    names.dedup();
    if names.is_empty() || names.len() > MAX_ALONE {
        return Ok(false);
    }
    for name in names {
        let source_name = input_name(&name, extensions);
        let rerun = run_sniff(sniff, work, case, source, input, Some(&source_name), 1, extensions)?;
        let output = rerun.outputs.get(&name).copied();
        let error = rerun.errors.get(&source_name).cloned();
        println!(
            "oracle alone  {name}: {}",
            match (&output, &error) {
                (_, Some(error)) => error.clone(),
                (Some(hash), None) => format!("output {hash}"),
                (None, None) => "unchanged".to_owned(),
            }
        );
        oracle.outputs.remove(&name);
        let failed = oracle.errors.remove(&source_name).is_some() | oracle.errors.remove(&name).is_some();
        if let Some(hash) = output {
            oracle.outputs.insert(name.clone(), hash);
        }
        if let Some(error) = &error {
            oracle.errors.insert(source_name.clone(), error.clone());
        }
        // A file that failed among the others wrote no log lines there:
        // the run alone gives them (`Updated:`, the processor's report).
        if failed && error.is_none() {
            oracle.log.extend(rerun.log);
            oracle.log.sort();
            oracle.extra.extend(rerun.extra);
            oracle.extra.sort();
            oracle.updated += rerun.updated;
        }
        oracle.alone_runs.push(source_name);
    }
    oracle.alone_runs.sort();
    Ok(true)
}

/// The input file of an output: `<name>.json` of `Convert to and from
/// JSON` comes from `<name>` when that is a file the operation takes.
fn input_name(name: &str, extensions: &[String]) -> String {
    if let Some(stem) = name.strip_suffix(".json")
        && extensions.iter().any(|ext| stem.ends_with(&format!(".{ext}")))
    {
        return stem.to_owned();
    }
    name.to_owned()
}

/// Runs the port on `input` with the settings of the case.
fn run_port(
    work: &Path,
    case: &Case,
    source: &Path,
    input: &Path,
    threads: usize,
    extensions: &[String],
) -> Result<Results> {
    let out = work.join("port-out");
    fs::create_dir_all(&out)?;
    let extra = work.join("port-extra.log");
    let _ = fs::remove_file(&extra);
    let settings = MemIniFile::from_text(&settings_text(case, &extra, source));
    let outputs: Arc<Mutex<BTreeMap<String, u64>>> = Arc::default();
    let sink_outputs = outputs.clone();
    let options = RunOptions {
        operation: case.operation.to_owned(),
        input: windows_path(input),
        output: windows_path(&out),
        path_contains: Some(String::new()),
        subdir: Some(true),
        skip_on_errors: Some(true),
        copy_all: Some(false),
        threads: Some(threads as i32),
        dry_run: false,
        sink: Some(OutputSink(Arc::new(move |name: &str, data: &[u8]| {
            sink_outputs.lock().unwrap().insert(name.to_lowercase(), fnv(data));
        }))),
    };
    let report = match port_run(Some(settings), &options) {
        Ok(report) => report,
        Err(RunError::Aborted { error, .. }) => bail!("the port stopped: {error}"),
        Err(error) => bail!("the port did not run: {error}"),
    };
    // The log as Sniff writes and reads it: in the ANSI code page.
    let lines = string_list_lines(&ansi_string(&string_list_file_bytes(&report.messages)));
    let mut results = Results::default();
    read_log(&lines, extensions, &mut results);
    results.outputs = std::mem::take(&mut *outputs.lock().unwrap());
    results.extra = file_lines(&extra);
    results.extra.sort();
    Ok(results)
}

/// The outcome of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Outcome {
    Equal,
    EqualError,
    Unchanged,
    OutputDifferent,
    PortOnly,
    OracleOnly,
    ErrorDifferent,
    PortFailed,
    OracleFailed,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Outcome::Equal => "equal",
            Outcome::EqualError => "equal-error",
            Outcome::Unchanged => "unchanged",
            Outcome::OutputDifferent => "output-different",
            Outcome::PortOnly => "port-only-output",
            Outcome::OracleOnly => "oracle-only-output",
            Outcome::ErrorDifferent => "error-different",
            Outcome::PortFailed => "port-failed",
            Outcome::OracleFailed => "oracle-failed",
        }
    }

    /// Whether an outcome makes a run fail: the differences of the port
    /// against the oracle. `oracle-failed` is Sniff's own error or crash
    /// (the `Access violation` of `Remove nodes` on three Fallout 4
    /// Creation Club meshes repeats alone), and `port-only-output` is an
    /// output the oracle's threads lost (`group-shapes-split` at 16 threads
    /// loses 714 of them, which compare equal alone, so that case runs with
    /// `--threads 1`); neither is the port's difference.
    fn fatal(self) -> bool {
        matches!(
            self,
            Outcome::OutputDifferent | Outcome::OracleOnly | Outcome::ErrorDifferent | Outcome::PortFailed
        )
    }
}

/// The outcomes that make a run fail, with their counts.
fn failures(totals: &BTreeMap<Outcome, usize>) -> Vec<(Outcome, usize)> {
    totals
        .iter()
        .filter(|(outcome, _)| outcome.fatal())
        .map(|(outcome, count)| (*outcome, *count))
        .collect()
}

/// The differences of two sorted line lists: the lines only one side has.
fn line_differences(port: &[String], oracle: &[String]) -> (Vec<String>, Vec<String>) {
    let mut counts: HashMap<&str, i64> = HashMap::new();
    for line in port {
        *counts.entry(line).or_default() += 1;
    }
    for line in oracle {
        *counts.entry(line).or_default() -= 1;
    }
    let mut only_port = Vec::new();
    let mut only_oracle = Vec::new();
    for (line, count) in counts {
        for _ in 0..count.max(0) {
            only_port.push(line.to_owned());
        }
        for _ in 0..(-count).max(0) {
            only_oracle.push(line.to_owned());
        }
    }
    only_port.sort();
    only_oracle.sort();
    (only_port, only_oracle)
}

/// Unpacks the first `count` files of `archive` the operation processes
/// into `dir`, once.
fn sample_folder(archive: &Archive, dir: &Path, count: usize, extensions: &[String]) -> Result<()> {
    let done = dir.join(".complete");
    if done.exists() {
        return Ok(());
    }
    let _ = fs::remove_dir_all(dir);
    let mut taken = 0;
    for entry in archive.files() {
        if taken >= count {
            break;
        }
        let lower = entry.name.to_lowercase();
        if !extensions.iter().any(|ext| lower.ends_with(&format!(".{ext}"))) {
            continue;
        }
        let path = dir.join(entry.name.replace('\\', "/"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            &path,
            archive.unpack_entry(entry).map_err(|error| anyhow::anyhow!(error.0))?,
        )?;
        taken += 1;
    }
    fs::create_dir_all(dir)?;
    fs::write(done, "")?;
    Ok(())
}

/// The source folder of the operations that copy from the files of the same
/// path in another folder (`Copy anim priorities`): the files of the
/// archive with the priorities of their controlled blocks set to 33 by the
/// port's universal tweaker. Made once.
fn prepare_source(archive: &Archive, dir: &Path, extensions: &[String], threads: usize, prep: Prep) -> Result<()> {
    let (operation, text) = prep
        .operation()
        .with_context(|| format!("{} names {{source}} but has no preparation", prep.name()))?;
    let done = dir.join(".complete");
    if done.exists() {
        return Ok(());
    }
    let raw = dir.with_extension("raw");
    sample_folder(archive, &raw, usize::MAX, extensions)?;
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir)?;
    let settings = MemIniFile::from_text(text);
    let options = RunOptions {
        operation: operation.to_owned(),
        input: windows_path(&raw),
        output: windows_path(dir),
        path_contains: Some(String::new()),
        subdir: Some(true),
        skip_on_errors: Some(true),
        copy_all: Some(false),
        threads: Some(threads as i32),
        dry_run: false,
        sink: None,
    };
    if let Err(error) = port_run(Some(settings), &options) {
        bail!("preparing {}: {error}", dir.display());
    }
    fs::write(done, "")?;
    Ok(())
}

pub fn run(tag: &str, args: &[&str]) -> Result<()> {
    let options = parse(args)?;
    if options.list {
        for case in CASES {
            println!("{:<24} {}", case.name, case.operation);
        }
        return Ok(());
    }
    let sniff = PathBuf::from(required_var("XEDIT_ORACLE_DIR")?).join("Sniff.exe");
    ensure!(sniff.exists(), "{} does not exist", sniff.display());
    let cache = cache_dir()?.join(tag).join("sniff-oracle");
    let scratch = match std::env::var_os("XEDIT_PARITY_SCRATCH") {
        Some(dir) => PathBuf::from(dir).join(tag),
        None => cache_dir()?.join(tag),
    };

    let mut totals: BTreeMap<Outcome, usize> = BTreeMap::new();
    let mut log_differences = 0;
    let mut report = String::new();
    let say = |line: String, report: &mut String| {
        println!("{line}");
        report.push_str(&line);
        report.push('\n');
    };

    for case in CASES {
        // A case's full name selects that case only, another text every
        // case whose name holds it.
        let selects = |part: &String| {
            case.name == part.as_str()
                || (!CASES.iter().any(|other| other.name == part.as_str()) && case.name.contains(part.as_str()))
        };
        if !options.cases.is_empty() && !options.cases.iter().any(selects) {
            continue;
        }
        let entry = PROCS
            .iter()
            .find(|entry| entry.title == case.operation)
            .with_context(|| format!("no operation {}", case.operation))?;
        let proc = (entry
            .create
            .with_context(|| format!("{} is not ported", case.operation))?)();
        // The extensions of the operation, or those of the case's
        // `ProcessedFiles` (`*.nif, *.kf`), which override them.
        let extensions: Vec<String> = match case.settings.iter().find(|(name, _)| *name == "ProcessedFiles") {
            Some((_, value)) => value
                .split(',')
                .map(|part| part.trim().trim_start_matches("*.").to_lowercase())
                .filter(|ext| !ext.is_empty())
                .collect(),
            None => proc.base().extensions.clone(),
        };
        let games: Vec<&'static Game> = proc
            .base()
            .supported_games
            .iter()
            .filter_map(|game| GAMES.iter().find(|known| known.name == harness_game(*game)))
            .filter(|game| options.games.is_empty() || options.games.iter().any(|name| name == game.name))
            .collect();
        let settings_key = fnv(settings_text(case, Path::new("{log}"), Path::new("{source}")).as_bytes());

        for game in games {
            let Some(data) = std::env::var_os(game.data_var).map(PathBuf::from) else {
                say(
                    format!("skipped       {}: {} is not set", game.name, game.data_var),
                    &mut report,
                );
                continue;
            };
            let mut archives: Vec<PathBuf> = fs::read_dir(&data)
                .with_context(|| format!("reading {}", data.display()))?
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| {
                    let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
                    (name.ends_with(".bsa") || name.ends_with(".ba2"))
                        && (options.archives.is_empty() || options.archives.iter().any(|part| name.contains(part)))
                })
                .collect();
            archives.sort();
            for archive_path in archives {
                let archive = match Archive::open(&archive_path) {
                    Ok(archive) => archive,
                    Err(error) => {
                        say(
                            format!("skipped       {}: {error}", archive_path.display()),
                            &mut report,
                        );
                        continue;
                    }
                };
                let files = archive
                    .files()
                    .iter()
                    .filter(|entry| {
                        let lower = entry.name.to_lowercase();
                        extensions.iter().any(|ext| lower.ends_with(&format!(".{ext}")))
                    })
                    .count();
                if files == 0 {
                    continue;
                }
                let archive_name = archive_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .replace(' ', "_");
                let mut key = format!("{}-{settings_key:016x}", archive_key(&archive_path)?);
                // The input folder of a case that needs one of its own.
                let input = if case.prep == Prep::DeathSkeleton {
                    // `Add blocks from skeleton` reads `death.kf` and the
                    // `skeleton.nif` beside it from the input folder, which
                    // an archive does not give.
                    let dir = scratch.join("sniff-death").join(game.name).join(&archive_name);
                    if !death_skeleton_folder(&archive, &dir)? {
                        say(
                            format!(
                                "skipped       {}: no death.kf with a skeleton.nif",
                                archive_path.display()
                            ),
                            &mut report,
                        );
                        continue;
                    }
                    key.push_str("-deathskeleton");
                    dir
                } else {
                    match options.sample {
                        Some(count) => {
                            // The sample holds the first files of the case's
                            // extensions; a sample of NIFs only keeps the name
                            // (and cache key) it had before other extensions
                            // got their own samples.
                            let tag = if extensions == ["nif"] {
                                String::new()
                            } else {
                                format!("-{}", extensions.join("+"))
                            };
                            key.push_str(&format!("-sample{count}{tag}"));
                            let dir = scratch
                                .join("sniff-sample")
                                .join(game.name)
                                .join(&archive_name)
                                .join(format!("{count}{tag}"));
                            if let Err(error) = sample_folder(&archive, &dir, count, &extensions) {
                                say(
                                    format!("skipped       {}: {error:#}", archive_path.display()),
                                    &mut report,
                                );
                                continue;
                            }
                            dir
                        }
                        None => archive_path.clone(),
                    }
                };
                // The source folder of the operations that copy from one.
                let source = scratch
                    .join("sniff-source")
                    .join(case.prep.name())
                    .join(game.name)
                    .join(&archive_name);
                if uses_source(case) {
                    prepare_source(&archive, &source, &extensions, options.threads, case.prep)?;
                }
                drop(archive);
                let work = scratch.join("sniff-work").join(case.name).join(&archive_name);

                // The oracle, from the cache or run now.
                let cached = cache.join(case.name).join(game.name).join(format!("{key}.json"));
                let start = Instant::now();
                let mut oracle = if cached.exists() && !options.refresh_oracle {
                    serde_json::from_slice::<Results>(&fs::read(&cached)?)?
                } else {
                    let mut results =
                        run_sniff(&sniff, &work, case, &source, &input, None, options.threads, &extensions)?;
                    rerun_crashes(&sniff, &work, case, &source, &input, &extensions, &mut results)?;
                    fs::create_dir_all(cached.parent().unwrap())?;
                    fs::write(&cached, serde_json::to_vec(&results)?)?;
                    say(
                        format!(
                            "oracle        {} {} ({} outputs, {} errors, {:.0} s)",
                            case.name,
                            archive_path.display(),
                            results.outputs.len(),
                            results.errors.len(),
                            start.elapsed().as_secs_f64()
                        ),
                        &mut report,
                    );
                    results
                };

                let start = Instant::now();
                let port = match run_port(&work, case, &source, &input, options.threads, &extensions) {
                    Ok(port) => port,
                    Err(error) => {
                        say(
                            format!("port failed   {} {}: {error:#}", case.name, archive_path.display()),
                            &mut report,
                        );
                        *totals.entry(Outcome::PortFailed).or_default() += 1;
                        continue;
                    }
                };
                if rerun_differences(&sniff, &work, case, &source, &input, &extensions, &mut oracle, &port)? {
                    fs::write(&cached, serde_json::to_vec(&oracle)?)?;
                }

                // File by file.
                let mut names: Vec<&String> = oracle
                    .outputs
                    .keys()
                    .chain(oracle.errors.keys())
                    .chain(port.outputs.keys())
                    .chain(port.errors.keys())
                    .collect();
                names.sort();
                names.dedup();
                let mut counts: BTreeMap<Outcome, usize> = BTreeMap::new();
                let mut shown: BTreeMap<Outcome, usize> = BTreeMap::new();
                let mut details = Vec::new();
                let mut differing: BTreeSet<String> = BTreeSet::new();
                for name in names {
                    let outcome = match (
                        port.errors.get(name),
                        oracle.errors.get(name),
                        port.outputs.get(name),
                        oracle.outputs.get(name),
                    ) {
                        (Some(a), Some(b), _, _) if same_error(a, b) => Outcome::EqualError,
                        (Some(_), Some(_), _, _) => Outcome::ErrorDifferent,
                        (Some(_), None, _, _) => Outcome::PortFailed,
                        (None, Some(_), _, _) => Outcome::OracleFailed,
                        (None, None, Some(a), Some(b)) if a == b => Outcome::Equal,
                        (None, None, Some(_), Some(_)) => Outcome::OutputDifferent,
                        (None, None, Some(_), None) => Outcome::PortOnly,
                        (None, None, None, Some(_)) => Outcome::OracleOnly,
                        (None, None, None, None) => Outcome::Unchanged,
                    };
                    *counts.entry(outcome).or_default() += 1;
                    if !matches!(outcome, Outcome::Equal | Outcome::EqualError | Outcome::Unchanged) {
                        differing.insert(name.clone());
                        let seen = shown.entry(outcome).or_default();
                        *seen += 1;
                        if *seen <= options.show {
                            details.push(format!(
                                "    {} {name}: port {:?} {:?}, oracle {:?} {:?}",
                                outcome.name(),
                                port.outputs.get(name),
                                port.errors.get(name),
                                oracle.outputs.get(name),
                                oracle.errors.get(name)
                            ));
                        }
                    }
                }
                // The files that the summary counts but that wrote nothing.
                let silent = oracle.processed.saturating_sub(counts.values().sum());
                if silent > 0 {
                    *counts.entry(Outcome::Unchanged).or_default() += silent;
                }
                for (outcome, count) in &counts {
                    *totals.entry(*outcome).or_default() += count;
                }

                let (log_port, log_oracle) = line_differences(&port.log, &oracle.log);
                let (extra_port, extra_oracle) = line_differences(&port.extra, &oracle.extra);
                let counts_differ = port.updated != oracle.updated || port.processed != oracle.processed;
                let log_differs = !log_port.is_empty()
                    || !log_oracle.is_empty()
                    || !extra_port.is_empty()
                    || !extra_oracle.is_empty()
                    || counts_differ;
                if log_differs {
                    log_differences += 1;
                }
                let summary: Vec<String> = counts
                    .iter()
                    .map(|(outcome, count)| format!("{count} {}", outcome.name()))
                    .collect();
                say(
                    format!(
                        "{:<22} {:<5} {}: {} files: {}; log {} ({} lines){} ({:.0} s)",
                        case.name,
                        game.name,
                        archive_path.file_name().unwrap_or_default().to_string_lossy(),
                        files,
                        summary.join(", "),
                        if log_differs { "different" } else { "equal" },
                        oracle.log.len() + oracle.extra.len(),
                        if counts_differ {
                            format!(
                                ", counts port {}/{} oracle {}/{}",
                                port.updated, port.processed, oracle.updated, oracle.processed
                            )
                        } else {
                            String::new()
                        },
                        start.elapsed().as_secs_f64()
                    ),
                    &mut report,
                );
                for line in details {
                    say(line, &mut report);
                }
                for (label, lines) in [
                    ("log port only", &log_port),
                    ("log oracle only", &log_oracle),
                    ("log file port only", &extra_port),
                    ("log file oracle only", &extra_oracle),
                ] {
                    for line in lines.iter().take(options.show) {
                        say(format!("    {label}: {}", line.replace('\t', "\\t")), &mut report);
                    }
                }
                if !options.keep {
                    clean_work(&work, &differing, log_differs)?;
                }
            }
        }
    }
    let summary: Vec<String> = totals
        .iter()
        .map(|(outcome, count)| format!("{count} {}", outcome.name()))
        .collect();
    say(
        format!("total: {}; logs different: {log_differences}", summary.join(", ")),
        &mut report,
    );
    fs::create_dir_all(&scratch)?;
    fs::write(scratch.join("sniff-report.txt"), report)?;
    // The logs are compared and reported, but a difference there does not
    // fail the run: on a file Sniff crashes on alone (`oracle-failed`,
    // which is not a failure) the port's `Updated:` line and the summary
    // counts differ by construction, which the oracle's own crash
    // explains, not the port.
    let failed: Vec<String> = failures(&totals)
        .into_iter()
        .map(|(outcome, count)| format!("{count} {}", outcome.name()))
        .collect();
    ensure!(
        failed.is_empty(),
        "the port differs from the oracle: {} (report: {})",
        failed.join(", "),
        scratch.join("sniff-report.txt").display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_run_passes() {
        let mut totals = BTreeMap::new();
        totals.insert(Outcome::Equal, 123853);
        totals.insert(Outcome::EqualError, 87);
        totals.insert(Outcome::Unchanged, 197521);
        // Sniff's own crash and the output its threads lost stay non-fatal.
        totals.insert(Outcome::OracleFailed, 3);
        totals.insert(Outcome::PortOnly, 714);
        assert!(failures(&totals).is_empty());
    }

    #[test]
    fn a_run_with_a_difference_fails() {
        let mut totals = BTreeMap::new();
        totals.insert(Outcome::Equal, 10);
        totals.insert(Outcome::OutputDifferent, 2);
        assert_eq!(failures(&totals), vec![(Outcome::OutputDifferent, 2)]);
    }
}
