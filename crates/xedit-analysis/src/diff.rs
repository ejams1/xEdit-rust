// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: External/Diff/Diff.pas
//
// The upstream unit carries these terms, which apply to this translation
// of it (see NOTICE):
//
// (*******************************************************************************
// * Component         TDiff                                                      *
// * Version:          3.1                                                        *
// * Date:             7 November 2009                                            *
// * Compilers:        Delphi 7 - Delphi2009                                      *
// * Author:           Angus Johnson - angusj-AT-myrealbox-DOT-com                *
// * Copyright:        © 2001-200( Angus Johnson                                  *
// *                                                                              *
// * Licence to use, terms and conditions:                                        *
// *                   The code in the TDiff component is released as freeware    *
// *                   provided you agree to the following terms & conditions:    *
// *                   1. the copyright notice, terms and conditions are          *
// *                   left unchanged                                             *
// *                   2. modifications to the code by other authors must be      *
// *                   clearly documented and accompanied by the modifier's name. *
// *                   3. the TDiff component may be freely compiled into binary  *
// *                   format and no acknowledgement is required. However, a      *
// *                   discrete acknowledgement would be appreciated (eg. in a    *
// *                   program's 'About Box').                                    *
// *                                                                              *
// * Description:      Component to list differences between two integer arrays   *
// *                   using a "longest common subsequence" algorithm.            *
// *                   Typically, this component is used to diff 2 text files     *
// *                   once their individuals lines have been hashed.             *
// *                                                                              *
// * Acknowledgements: The key algorithm in this component is based on:           *
// *                   "An O(ND) Difference Algorithm and its Variations"         *
// *                   By E Myers - Algorithmica Vol. 1 No. 2, 1986, pp. 251-266  *
// *                   http://www.cs.arizona.edu/people/gene/                     *
// *                   http://www.cs.arizona.edu/people/gene/PAPERS/diff.ps       *
// *                                                                              *
// *******************************************************************************)
//
// Modifications (2026, by the xEdit-rust contributors): translated from
// Delphi to Rust. Only the integer compare is translated (the character
// compare, `Cancel` and the component registration are left out, as xEdit
// uses none of them); the diagonal arrays are allocated per recursion level
// instead of once per run; the compare list holds the records by value. The
// algorithm, the order of its oscillations and its tie breaks are unchanged,
// so the compare list is the one `TDiff.Execute` gives.

//! `TDiff`: the longest common subsequence of two integer arrays as a list
//! of compare records. `InitChildren` uses it to align the entries of
//! unsorted arrays across the records of the view.

/// Port of `TChangeKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    None,
    Add,
    Delete,
    Modify,
}

/// Port of `TCompareRec` with the integer case of its variant part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompareRec {
    pub kind: ChangeKind,
    pub old_index1: i32,
    pub old_index2: i32,
    pub int1: i32,
    pub int2: i32,
}

/// Port of `TDiffStats`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffStats {
    pub matches: i32,
    pub adds: i32,
    pub deletes: i32,
    pub modifies: i32,
}

/// Port of `MAXINT`.
const MAXINT: i32 = i32::MAX;
/// Port of `MAX_DIAGONAL`: the largest deviation from the centre diagonal.
const MAX_DIAGONAL: i32 = 0xFF_FFFF;

/// A diagonal array `[-max_oscill - 1 .. max_oscill + 1]`.
struct Diags {
    values: Vec<i32>,
    offset: i32,
}

impl Diags {
    fn get(&self, diag: i32) -> i32 {
        self.values[(diag + self.offset) as usize]
    }

    fn set(&mut self, diag: i32, value: i32) {
        self.values[(diag + self.offset) as usize] = value;
    }
}

/// Port of `TDiff` after `Execute` on two integer arrays.
pub struct Diff<'a> {
    compares: Vec<CompareRec>,
    ints1: &'a [i32],
    ints2: &'a [i32],
    last: CompareRec,
    stats: DiffStats,
    /// Port of `AllowModify`: a run of adds after a run of deletes (or the
    /// reverse) becomes a run of modifies.
    allow_modify: bool,
}

impl<'a> Diff<'a> {
    /// Port of `TDiff.Create`, `AllowModify` and `Execute(pints1, pints2,
    /// len1, len2)`.
    pub fn execute(ints1: &'a [i32], ints2: &'a [i32], allow_modify: bool) -> Self {
        let mut diff = Diff {
            compares: Vec::new(),
            ints1,
            ints2,
            // Port of `Clear`.
            last: CompareRec {
                kind: ChangeKind::None,
                old_index1: -1,
                old_index2: -1,
                int1: 0,
                int2: 0,
            },
            stats: DiffStats::default(),
            allow_modify,
        };
        let mut len1 = ints1.len() as i32;
        let mut len2 = ints2.len() as i32;
        // The first length, for the trailing matches.
        let saved_len = len1 - 1;
        // Ignore the matches at the end ("top matches").
        let (mut x1, mut x2) = (0, 0);
        while len1 > 0 && len2 > 0 && ints1[(len1 - 1) as usize] == ints2[(len2 - 1) as usize] {
            len1 -= 1;
            len2 -= 1;
        }
        if len1 != 0 || len2 != 0 {
            // Ignore the matches at the start too.
            while len1 > 0 && len2 > 0 && ints1[x1 as usize] == ints2[x2 as usize] {
                len1 -= 1;
                len2 -= 1;
                x1 += 1;
                x2 += 1;
            }
            diff.compares.reserve((len1 + len2) as usize);
            diff.recursive_diff_int(x1, x2, len1, len2);
        }
        // Append the trailing matches.
        while diff.last.old_index1 < saved_len {
            diff.last.kind = ChangeKind::None;
            diff.last.old_index1 += 1;
            diff.last.old_index2 += 1;
            diff.last.int1 = ints1[diff.last.old_index1 as usize];
            diff.last.int2 = ints2[diff.last.old_index2 as usize];
            diff.compares.push(diff.last);
            diff.stats.matches += 1;
        }
        diff
    }

    /// Port of `Count`.
    pub fn count(&self) -> usize {
        self.compares.len()
    }

    /// Port of `Compares`.
    pub fn compares(&self) -> &[CompareRec] {
        &self.compares
    }

    /// Port of `DiffStats`.
    pub fn stats(&self) -> DiffStats {
        self.stats
    }

    /// Port of `InitDiagArrays`.
    fn init_diag_arrays(max_oscill: i32, len1: i32, len2: i32) -> (Diags, Diags) {
        // The extra diagonal at each end of the arrays.
        let max_oscill = max_oscill + 1;
        let size = (max_oscill * 2 + 1) as usize;
        let mut f_diag = Diags {
            values: vec![-MAXINT; size],
            offset: max_oscill,
        };
        f_diag.set(0, -1);
        let mut b_diag = Diags {
            values: vec![MAXINT; size],
            offset: max_oscill,
        };
        b_diag.set(len1 - len2, len1 - 1);
        (f_diag, b_diag)
    }

    /// Port of `RecursiveDiffInt`.
    fn recursive_diff_int(&mut self, offset1: i32, offset2: i32, len1: i32, len2: i32) {
        if len1 == 0 {
            debug_assert!(len2 > 0, "oops!");
            self.add_change_ints(offset1, len2, ChangeKind::Add);
            return;
        } else if len2 == 0 {
            self.add_change_ints(offset1, len1, ChangeKind::Delete);
            return;
        } else if len1 == 1 && len2 == 1 {
            debug_assert!(self.ints1[offset1 as usize] != self.ints2[offset2 as usize], "oops!");
            self.add_change_ints(offset1, 1, ChangeKind::Delete);
            self.add_change_ints(offset1, 1, ChangeKind::Add);
            return;
        }

        let ints1 = self.ints1;
        let ints2 = self.ints2;
        let eq = |i1: i32, i2: i32| ints1[i1 as usize] == ints2[i2 as usize];

        let max_oscill = len1.max(len2).min(MAX_DIAGONAL);
        let (mut f_diag, mut b_diag) = Self::init_diag_arrays(max_oscill, len1, len2);
        let len_delta = len1 - len2;
        let odd = len_delta % 2 != 0;

        // Assumes the prior filter of the top and bottom matches.
        let mut oscill = 1;
        while oscill <= max_oscill {
            // The forward oscillation, keeping the diagonal in the grid.
            let mut diag = oscill;
            while diag > len1 {
                diag -= 2;
            }
            while diag >= (-oscill).max(-len2) {
                let mut x1 = if f_diag.get(diag - 1) < f_diag.get(diag + 1) {
                    f_diag.get(diag + 1)
                } else {
                    f_diag.get(diag - 1) + 1
                };
                let mut x2 = x1 - diag;
                while x1 < len1 - 1 && x2 < len2 - 1 && eq(offset1 + x1 + 1, offset2 + x2 + 1) {
                    x1 += 1;
                    x2 += 1;
                }
                f_diag.set(diag, x1);
                // `fDiag[diag]` is always below `bDiag[diag]` here when
                // `lenDelta` is even.
                if odd && f_diag.get(diag) >= b_diag.get(diag) {
                    x1 += 1;
                    x2 += 1;
                    let (end1, end2) = (x1, x2);
                    while x1 > 0 && x2 > 0 && eq(offset1 + x1 - 1, offset2 + x2 - 1) {
                        x1 -= 1;
                        x2 -= 1;
                    }
                    self.recursive_diff_int(offset1, offset2, x1, x2);
                    self.recursive_diff_int(offset1 + end1, offset2 + end2, len1 - end1, len2 - end2);
                    return;
                }
                diag -= 2;
            }

            // The backward oscillation, keeping the diagonal in the grid.
            let mut diag = len_delta + oscill;
            while diag > len1 {
                diag -= 2;
            }
            while diag >= (len_delta - oscill).max(-len2) {
                let mut x1 = if b_diag.get(diag - 1) < b_diag.get(diag + 1) {
                    b_diag.get(diag - 1)
                } else {
                    b_diag.get(diag + 1) - 1
                };
                let mut x2 = x1 - diag;
                while x1 > -1 && x2 > -1 && eq(offset1 + x1, offset2 + x2) {
                    x1 -= 1;
                    x2 -= 1;
                }
                b_diag.set(diag, x1);
                if b_diag.get(diag) <= f_diag.get(diag) {
                    x1 += 1;
                    x2 += 1;
                    self.recursive_diff_int(offset1, offset2, x1, x2);
                    while x1 < len1 && x2 < len2 && eq(offset1 + x1, offset2 + x2) {
                        x1 += 1;
                        x2 += 1;
                    }
                    self.recursive_diff_int(offset1 + x1, offset2 + x2, len1 - x1, len2 - x2);
                    return;
                }
                diag -= 2;
            }
            oscill += 1;
        }
        // Upstream raises `oops - error in RecursiveDiffInt()` here, which
        // the oscillations never reach.
        unreachable!("oops - error in RecursiveDiffInt()");
    }

    /// Port of `AddChangeInts`.
    fn add_change_ints(&mut self, offset1: i32, range: i32, change_kind: ChangeKind) {
        // First the unchanged items before the change.
        while self.last.old_index1 < offset1 - 1 {
            self.last.kind = ChangeKind::None;
            self.last.old_index1 += 1;
            self.last.old_index2 += 1;
            self.last.int1 = self.ints1[self.last.old_index1 as usize];
            self.last.int2 = self.ints2[self.last.old_index2 as usize];
            self.compares.push(self.last);
            self.stats.matches += 1;
        }
        match change_kind {
            ChangeKind::Add => {
                for _ in 0..range {
                    // A run of adds after a run of deletes becomes modifies.
                    if self.last.kind == ChangeKind::Delete && self.allow_modify {
                        let mut j = self.compares.len() - 1;
                        while j > 0 && self.compares[j - 1].kind == ChangeKind::Delete {
                            j -= 1;
                        }
                        self.compares[j].kind = ChangeKind::Modify;
                        self.stats.deletes -= 1;
                        self.stats.modifies += 1;
                        self.last.old_index2 += 1;
                        self.compares[j].old_index2 = self.last.old_index2;
                        self.compares[j].int2 = self.ints2[self.last.old_index2 as usize];
                        if j == self.compares.len() - 1 {
                            self.last.kind = ChangeKind::Modify;
                        }
                        continue;
                    }
                    self.last.kind = ChangeKind::Add;
                    self.last.int1 = 0;
                    self.last.old_index2 += 1;
                    self.last.int2 = self.ints2[self.last.old_index2 as usize];
                    self.compares.push(self.last);
                    self.stats.adds += 1;
                }
            }
            ChangeKind::Delete => {
                for _ in 0..range {
                    // A run of deletes after a run of adds becomes modifies.
                    if self.last.kind == ChangeKind::Add && self.allow_modify {
                        let mut j = self.compares.len() - 1;
                        while j > 0 && self.compares[j - 1].kind == ChangeKind::Add {
                            j -= 1;
                        }
                        self.compares[j].kind = ChangeKind::Modify;
                        self.stats.adds -= 1;
                        self.stats.modifies += 1;
                        self.last.old_index1 += 1;
                        self.compares[j].old_index1 = self.last.old_index1;
                        self.compares[j].int1 = self.ints1[self.last.old_index1 as usize];
                        if j == self.compares.len() - 1 {
                            self.last.kind = ChangeKind::Modify;
                        }
                        continue;
                    }
                    self.last.kind = ChangeKind::Delete;
                    self.last.int2 = 0;
                    self.last.old_index1 += 1;
                    self.last.int1 = self.ints1[self.last.old_index1 as usize];
                    self.compares.push(self.last);
                    self.stats.deletes += 1;
                }
            }
            ChangeKind::None | ChangeKind::Modify => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The kinds and indices of the compare list, as `<kind><i1>:<i2>`.
    fn script(left: &[i32], right: &[i32]) -> Vec<String> {
        Diff::execute(left, right, false)
            .compares()
            .iter()
            .map(|rec| {
                let kind = match rec.kind {
                    ChangeKind::None => "=",
                    ChangeKind::Add => "+",
                    ChangeKind::Delete => "-",
                    ChangeKind::Modify => "~",
                };
                format!("{kind}{}:{}", rec.old_index1, rec.old_index2)
            })
            .collect()
    }

    /// A compare list replays to both inputs.
    fn check_replay(left: &[i32], right: &[i32]) {
        let diff = Diff::execute(left, right, false);
        let mut l = Vec::new();
        let mut r = Vec::new();
        for rec in diff.compares() {
            match rec.kind {
                ChangeKind::None => {
                    assert_eq!(rec.int1, rec.int2);
                    l.push(rec.int1);
                    r.push(rec.int2);
                }
                ChangeKind::Delete => l.push(rec.int1),
                ChangeKind::Add => r.push(rec.int2),
                ChangeKind::Modify => unreachable!(),
            }
        }
        assert_eq!(l, left);
        assert_eq!(r, right);
    }

    #[test]
    fn equal_arrays_are_all_matches() {
        assert_eq!(script(&[1, 2, 3], &[1, 2, 3]), ["=0:0", "=1:1", "=2:2"]);
    }

    #[test]
    fn insertions_and_deletions() {
        assert_eq!(script(&[1, 2, 3], &[2, 3]), ["-0:-1", "=1:0", "=2:1"]);
        assert_eq!(script(&[2, 3], &[1, 2, 3]), ["+-1:0", "=0:1", "=1:2"]);
        assert_eq!(script(&[1, 3], &[1, 2, 3]), ["=0:0", "+0:1", "=1:2"]);
    }

    #[test]
    fn replays_to_both_inputs() {
        let cases: &[(&[i32], &[i32])] = &[
            (&[1, 2], &[2, 1]),
            (&[1, 2, 3, 4, 5], &[5, 4, 3, 2, 1]),
            (&[1, 1, 2, 2, 3], &[2, 1, 3, 3, 1, 2]),
            (&[], &[1, 2]),
            (&[1, 2], &[]),
            (&[7, 1, 7, 2, 7, 3], &[1, 7, 2, 7, 3, 7, 4]),
            (&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9], &[9, 0, 2, 1, 3, 5, 4, 6, 8, 7]),
        ];
        for (left, right) in cases {
            check_replay(left, right);
        }
    }

    #[test]
    fn modifies_join_adds_and_deletes() {
        let diff = Diff::execute(&[1, 2, 3], &[1, 4, 3], true);
        let kinds: Vec<ChangeKind> = diff.compares().iter().map(|rec| rec.kind).collect();
        assert_eq!(kinds, [ChangeKind::None, ChangeKind::Modify, ChangeKind::None]);
        assert_eq!(diff.stats().modifies, 1);
    }
}
