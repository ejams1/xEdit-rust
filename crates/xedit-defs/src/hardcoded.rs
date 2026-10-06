// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbHardcoded.pas

//! The hardcoded records of each game: the forms the engine defines
//! without a plugin, such as the player reference. Upstream keeps them as
//! plugins embedded in a data module (`Core/Hardcoded/*.esp`); here they
//! are embedded from `hardcoded/`.

use xedit_core::interface::globals::GameMode;

/// Port of `TwbHardcodedContainer.GetHardCodedDat`: the embedded plugin of
/// the hardcoded records of the game, `None` for a game without one here.
/// The `<GameName>.Hardcoded.Override.dat` file upstream reads next to the
/// program is not supported.
pub fn hardcoded_dat(game_mode: GameMode) -> Option<&'static [u8]> {
    match game_mode {
        GameMode::gmTES5 | GameMode::gmTES5VR | GameMode::gmSSE | GameMode::gmEnderal | GameMode::gmEnderalSE => {
            Some(include_bytes!("../hardcoded/Skyrim.esp"))
        }
        GameMode::gmFO4 | GameMode::gmFO4VR => Some(include_bytes!("../hardcoded/Fallout4.esp")),
        _ => None,
    }
}
