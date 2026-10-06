// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbHardcoded.pas

//! The hardcoded records of each game: the forms the engine defines
//! without a plugin, such as the player reference. Upstream keeps them as
//! plugins embedded in a data module (`Core/wbHardcoded.dfm`, with copies in
//! `Core/Hardcoded`); here they are embedded from `hardcoded/`. The
//! Morrowind plugin exists only in the data module and was extracted from
//! it; the Enderal plugin there is identical to the Skyrim one.

/// Port of `TwbHardcodedContainer.GetHardCodedDat`: the embedded plugin of
/// the hardcoded records of the game, found by the game name (`wbGameName`)
/// as upstream finds its `fc<GameName>` container. `None` for a game
/// without one. The `<GameName>.Hardcoded.Override.dat` file upstream reads
/// next to the program is not supported.
pub fn hardcoded_dat(game_name: &str) -> Option<&'static [u8]> {
    Some(match game_name {
        "Morrowind" => include_bytes!("../hardcoded/Morrowind.esm"),
        "Oblivion" => include_bytes!("../hardcoded/Oblivion.esp"),
        "Fallout3" => include_bytes!("../hardcoded/Fallout3.esp"),
        "FalloutNV" => include_bytes!("../hardcoded/FalloutNV.esp"),
        "Skyrim" | "Enderal" => include_bytes!("../hardcoded/Skyrim.esp"),
        "Fallout4" => include_bytes!("../hardcoded/Fallout4.esp"),
        "Fallout76" => include_bytes!("../hardcoded/Fallout76.esp"),
        "Starfield" => include_bytes!("../hardcoded/Starfield.esm"),
        _ => return None,
    })
}
