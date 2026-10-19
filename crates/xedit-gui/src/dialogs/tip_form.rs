// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeTipForm.pas (FormCreate, FormDestroy,
// NextTip, lblNextTipClick, cbShowTipClick, ShowTip, HideTip) and
// xEdit/xeTipForm.dfm (Shape1, textTip, StaticText1, cbShowTip,
// lblNextTip)

//! `TfrmTip`: the tip of the day. The tips are the release's own list
//! (`fcEditTips`, decompressed from `xeTipForm.dfm`, MPL 2.0); a
//! `<app>Tips.Override.txt` next to the program wins, as upstream's file
//! does. `wbShowTip` is `[Options] ShowTip` and is written when the dialog
//! closes.

use eframe::egui;

/// `fcEditTips` of the form.
pub const TIPS: &str = include_str!("../../resources/tips.txt");

/// What the dialog asks the shell to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TipAction {
    /// The window closed: `[Options] ShowTip` (`cbShowTip.Checked`).
    Closed { show_tip: bool },
}

/// `TfrmTip`'s state.
#[derive(Clone, Debug)]
pub struct TipState {
    /// `slTips`.
    pub tips: Vec<String>,
    /// The tip `textTip.Caption` shows.
    pub current: String,
    /// `cbShowTip.Checked`.
    pub show_tip: bool,
    /// The generator of `Random(slTips.Count)`, a linear congruential
    /// generator seeded from the clock (`Randomize` of the unit's
    /// initialization).
    pub seed: u64,
    /// How many consecutive `NextTip`s were asked for (the screen check
    /// reads it).
    pub shown: usize,
}

impl TipState {
    /// `FormCreate`: the override file, then the embedded list, blank lines
    /// dropped, then `NextTip`.
    pub fn new(program_folder: &std::path::Path, app_name: &str, show_tip: bool) -> Self {
        let override_file = program_folder.join(format!("{app_name}Tips.Override.txt"));
        let mut tips = std::fs::read_to_string(&override_file)
            .ok()
            .map(|text| parse_tips(&text))
            .unwrap_or_default();
        if tips.is_empty() {
            tips = parse_tips(TIPS);
        }
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or(0x2545_F491_4F6C_DD1D);
        let mut state = Self {
            tips,
            current: String::new(),
            show_tip,
            seed,
            shown: 0,
        };
        state.next_tip();
        state
    }

    /// `NextTip`: `slTips[Random(slTips.Count)]`.
    pub fn next_tip(&mut self) {
        if self.tips.is_empty() {
            return;
        }
        // The Delphi `Random(n)` of a 32-bit generator: `seed` is the state.
        self.seed = self
            .seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let index = ((self.seed >> 33) as usize) % self.tips.len();
        self.current = self.tips[index].clone();
        self.shown += 1;
    }

    /// The index the random picked last, for the tests of the rotation.
    pub fn last_index(&self) -> Option<usize> {
        self.tips.iter().position(|tip| *tip == self.current)
    }

    /// `cbShowTipClick`: `wbShowTip := cbShowTip.Checked`.
    pub fn set_show_tip(&mut self, show: bool) {
        self.show_tip = show;
    }
}

/// `slTips.LoadFromFile`'s tail: the blank lines removed.
pub fn parse_tips(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim_end_matches('\r').to_owned())
        .filter(|line| !line.trim().is_empty())
        .collect()
}

/// Draws the tip (`Shape1`, `StaticText1` "Tip", `textTip`, "Next >" and
/// the check box).
pub fn show(ctx: &egui::Context, state: &mut TipState) -> Option<TipAction> {
    let mut open = true;
    egui::Window::new("Tip")
        .id(egui::Id::unique("frmTip"))
        .collapsible(false)
        .resizable(false)
        .default_size(egui::vec2(534.0, 213.0))
        .default_pos(egui::pos2(60.0, 60.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Tip");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.link("Next >").clicked() {
                        state.next_tip();
                    }
                });
            });
            ui.separator();
            ui.add_space(6.0);
            ui.label(&state.current);
            ui.add_space(10.0);
            let mut show_tip = state.show_tip;
            if ui
                .checkbox(&mut show_tip, "Show tip (you can enable back in Options)")
                .changed()
            {
                state.set_show_tip(show_tip);
            }
        });
    if !open {
        return Some(TipAction::Closed {
            show_tip: state.show_tip,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_tips_parse_and_rotate() {
        let tips = parse_tips(TIPS);
        assert!(tips.len() > 50, "{} tips", tips.len());
        assert!(tips.iter().all(|tip| !tip.trim().is_empty()));
        let mut state = TipState {
            tips: tips.clone(),
            current: String::new(),
            show_tip: true,
            seed: 1,
            shown: 0,
        };
        state.next_tip();
        let first = state.last_index().unwrap();
        state.next_tip();
        let second = state.last_index().unwrap();
        assert!(first < tips.len() && second < tips.len());
        assert_eq!(state.shown, 2);
        // The generator walks; over many draws both indices recur.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            state.next_tip();
            seen.insert(state.last_index().unwrap());
        }
        assert!(seen.len() > 10);
    }

    #[test]
    fn the_show_tip_flag_follows_the_check_box() {
        let mut state = TipState {
            tips: vec!["one".to_owned()],
            current: "one".to_owned(),
            show_tip: true,
            seed: 0,
            shown: 0,
        };
        state.set_show_tip(false);
        assert!(!state.show_tip);
        assert_eq!(
            TipAction::Closed { show_tip: false },
            TipAction::Closed { show_tip: false }
        );
    }

    #[test]
    fn an_empty_tip_list_keeps_the_caption_empty() {
        let mut state = TipState {
            tips: Vec::new(),
            current: String::new(),
            show_tip: true,
            seed: 0,
            shown: 0,
        };
        state.next_tip();
        assert!(state.current.is_empty());
        assert_eq!(state.shown, 0);
    }
}
