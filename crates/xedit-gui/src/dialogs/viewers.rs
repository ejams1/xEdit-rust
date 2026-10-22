// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeLogAnalyzerForm.dfm (pnlClient, vstForms,
// Splitter1, memoText), xEdit/xeLocalizationForm.dfm (Panel1, vetStrings,
// Splitter1, memoText), xEdit/xeViewElementsForm.pas (AddElement's memos,
// btnCopyToClipboard, btnCompareClick, FindDialog1Find),
// xEdit/xeRichEditForm.pas (BuildTOC, TOCSelected, tbrZoomChange,
// FormKeyDown's search) and the RTF text of fcWhatsNew, fcMessage and
// fcMessageThanks

//! The viewers the step-6 dialogs share: one split tree and memo viewer
//! (the Log Analyzer's forms tree above its memo and the Localization
//! editor's strings tree left of its memo), one memo pane, and one
//! RTF/text viewer with the table of contents, the zoom and the search of
//! `xeRichEditForm`.
//!
//! The RTF content of the help and developer messages is the release's own
//! (`fcWhatsNew`, `fcMessage` and `fcMessageThanks` of the 4.1.5q DFMs,
//! decompressed and kept under `resources/`, MPL 2.0 as the unit headers
//! say). The parser covers the subset those documents use: the character
//! and paragraph formatting, the font table it skips, and the cp1252
//! escapes; a full RTF engine is not needed for three static documents.

use eframe::egui;

use crate::widgets::conflict::ConflictColors;
use crate::widgets::virtual_tree::{TreeSource, TreeState, TreeView};

/// Where the tree sits relative to the memo (`Splitter1.Align` of the two
/// DFMs).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitOrientation {
    /// `alTop`: the tree above the memo (`xeLogAnalyzerForm`).
    TreeTop,
    /// `alLeft`: the tree left of the memo (`xeLocalizationForm`).
    TreeLeft,
}

/// The result of one frame of the split viewer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SplitOutput {
    /// The row clicked this frame.
    pub clicked: Option<usize>,
    /// The row double-clicked this frame.
    pub double_clicked: Option<usize>,
}

/// One split tree+memo pane. The caller owns the source, the tree state and
/// the memo text; this only lays them out as the DFMs do.
pub struct SplitViewer<'a> {
    pub id: &'a str,
    pub source: &'a mut dyn TreeSource,
    pub state: &'a mut TreeState,
    pub memo: &'a str,
    pub orientation: SplitOrientation,
    /// The splitter position in pixels (`vstForms.Height` 265,
    /// `vetStrings.Width` 329).
    pub split: &'a mut f32,
    pub colors: &'a ConflictColors,
    pub header: bool,
}

impl SplitViewer<'_> {
    pub fn show(self, ui: &mut egui::Ui) -> SplitOutput {
        let SplitViewer {
            id,
            source,
            state,
            memo,
            orientation,
            split,
            colors,
            header,
        } = self;
        let mut output = SplitOutput::default();
        match orientation {
            SplitOrientation::TreeTop => {
                let tree_height = ui.available_height().min(*split).max(40.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), tree_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let tree = TreeView {
                            id: ui.scope_id().with(id),
                            source,
                            state,
                            colors,
                            header,
                        }
                        .show(ui);
                        output.clicked = tree.clicked;
                        output.double_clicked = tree.double_clicked;
                    },
                );
                // `TSplitter` (`Height = 3`).
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 4.0), egui::Sense::drag());
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
                if response.dragged() {
                    *split = (*split + response.drag_delta().y).clamp(40.0, ui.available_height().max(80.0));
                }
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::ZERO,
                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                );
                memo_pane(ui, id, memo, ui.available_height().max(40.0));
            }
            SplitOrientation::TreeLeft => {
                let width = ui.available_width().min(*split).max(60.0);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, ui.available_height()),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            let tree = TreeView {
                                id: ui.scope_id().with(id),
                                source,
                                state,
                                colors,
                                header,
                            }
                            .show(ui);
                            output.clicked = tree.clicked;
                            output.double_clicked = tree.double_clicked;
                        },
                    );
                    // `TSplitter` (`Width = 3`).
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(4.0, ui.available_height()), egui::Sense::drag());
                    if response.hovered() || response.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if response.dragged() {
                        *split = (*split + response.drag_delta().x).clamp(60.0, ui.available_width().max(120.0));
                    }
                    ui.painter().rect_filled(
                        rect,
                        egui::CornerRadius::ZERO,
                        ui.visuals().widgets.noninteractive.bg_stroke.color,
                    );
                    memo_pane(ui, id, memo, ui.available_height());
                });
            }
        }
        output
    }
}

/// The `memoText` of the two forms: a read-only, selectable, scrolling text
/// pane with the viewer font.
pub fn memo_pane(ui: &mut egui::Ui, id: &str, text: &str, height: f32) {
    let font = crate::theme::messages_font_id(&crate::theme::Fonts::default());
    egui::ScrollArea::both()
        .id_salt(format!("{id}-memo"))
        .auto_shrink([false, false])
        .max_height(height)
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(text).font(font))
                    .selectable(true)
                    .wrap_mode(egui::TextWrapMode::Extend),
            );
        });
}

//=== The RTF text viewer ====================================================

/// One run of identically formatted text (`TAtrributes` of a rich edit
/// range).
#[derive(Clone, Debug, PartialEq)]
pub struct RtfRun {
    pub text: String,
    pub bold: bool,
    /// Font size in points (`\fs24` is 12).
    pub size: f32,
}

/// One paragraph: the runs between two `\par`s.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RtfParagraph {
    pub runs: Vec<RtfRun>,
}

impl RtfParagraph {
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }
}

/// One entry of the table of contents: a bold run of the document, nested by
/// font size as `BuildTOC` nests the `ITextRange`s.
#[derive(Clone, Debug, PartialEq)]
pub struct TocEntry {
    /// 0 for a top-level entry; one more than its parent otherwise.
    pub depth: usize,
    pub text: String,
    /// The paragraph the entry starts in, for `TOCSelected`.
    pub paragraph: usize,
}

/// A parsed RTF document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RtfDocument {
    pub paragraphs: Vec<RtfParagraph>,
    pub toc: Vec<TocEntry>,
}

impl RtfDocument {
    pub fn text(&self) -> String {
        self.paragraphs
            .iter()
            .map(RtfParagraph::text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The number of characters the document shows, for the caret of
    /// `FixZoom`.
    pub fn len(&self) -> usize {
        self.paragraphs
            .iter()
            .map(|paragraph| paragraph.text().chars().count() + 1)
            .sum()
    }

    pub fn is_empty(&self) -> bool {
        self.paragraphs.is_empty()
    }
}

/// The state of an RTF viewer instance: the zoom, the TOC pane, the search
/// box and the first-match position.
#[derive(Clone, Debug, PartialEq)]
pub struct RtfViewerState {
    /// `tbrZoom.Position` (25..400, 100 is the document's own size).
    pub zoom: i32,
    /// `btnTOC.Checked` and `pnlTOC.Visible`.
    pub toc_visible: bool,
    /// `pnlTOC.Width` (`TOCWidth` of the settings).
    pub toc_width: f32,
    /// `cbDontShowAgain.Checked`.
    pub dont_show_again: bool,
    /// `edSearch.Text`.
    pub search: String,
    /// The paragraph the last search found, to scroll to it.
    pub search_hit: Option<usize>,
    /// The paragraph the last search started from
    /// (`reMain.SelStart + reMain.SelLength`).
    pub search_from: usize,
    /// The paragraph the search reports it could not find, shown once.
    pub search_error: Option<String>,
}

impl Default for RtfViewerState {
    fn default() -> Self {
        Self {
            zoom: 100,
            toc_visible: false,
            toc_width: 185.0,
            dont_show_again: false,
            search: String::new(),
            search_hit: None,
            search_from: 0,
            search_error: None,
        }
    }
}

/// `reMain.FindText(edSearch.Text, start, 1000000, [stSetSelection])` over
/// the paragraphs: the first paragraph at or after `from` whose text holds
/// the search text, without regard to case (`SameText` semantics of the
/// rich edit's search).
pub fn find_text(document: &RtfDocument, search: &str, from: usize) -> Option<usize> {
    let needle = search.to_lowercase();
    if needle.is_empty() {
        return None;
    }
    let start = from.min(document.paragraphs.len());
    document.paragraphs[start..]
        .iter()
        .position(|paragraph| paragraph.text().to_lowercase().contains(&needle))
        .map(|offset| start + offset)
}

/// The viewer's own key handling (`FormKeyDown`): ctrl+plus/minus/0 change
/// the zoom, alt+T toggles the table of contents, ctrl+F focuses the search
/// box and F3 repeats the search. Returns whether the key was consumed.
pub fn rtf_key(state: &mut RtfViewerState, document: &RtfDocument, key: egui::Key, modifiers: egui::Modifiers) -> bool {
    if modifiers.alt && key == egui::Key::T {
        state.toc_visible = !state.toc_visible;
        return true;
    }
    if modifiers.command {
        match key {
            egui::Key::Plus | egui::Key::Equals => {
                state.zoom = (state.zoom + 10).min(400);
                return true;
            }
            egui::Key::Minus => {
                state.zoom = (state.zoom - 10).max(25);
                return true;
            }
            egui::Key::Num0 => {
                state.zoom = 100;
                return true;
            }
            _ => {}
        }
    }
    if key == egui::Key::F3 {
        if let Some(hit) = find_text(document, &state.search, state.search_from) {
            state.search_hit = Some(hit);
            state.search_from = hit;
        } else {
            state.search_error = Some(format!("There are no further occurences of \"{}\".", state.search));
        }
        return true;
    }
    false
}

/// Draws the document as the rich edit does, with the TOC pane, the zoom,
/// the search box and the OK/checkbox row the caller supplies through
/// [`RtfViewer::bottom_row`].
pub struct RtfViewer<'a> {
    pub id: &'a str,
    pub document: &'a RtfDocument,
    pub state: &'a mut RtfViewerState,
    /// Whether `btnOk` is drawn (`xeRichEditForm` hides it in the What's New
    /// tab of the main form).
    pub ok_visible: bool,
    /// Whether the OK button is enabled (`tmrEnableButton`).
    pub ok_enabled: bool,
    /// Whether the don't-show-again check box is drawn.
    pub dont_show_again_visible: bool,
    /// The caption of the don't-show-again check box.
    pub dont_show_again_caption: &'a str,
}

impl RtfViewer<'_> {
    /// Draws the search row and the body. Returns whether `btnOk` was
    /// clicked and whether the check box changed.
    pub fn show(&mut self, ui: &mut egui::Ui) -> RtfViewerOutput {
        let mut output = RtfViewerOutput::default();
        let document = self.document;
        let state = &mut *self.state;
        ui.horizontal(|ui| {
            ui.label("&Search:");
            let search = ui.add(
                egui::TextEdit::singleline(&mut state.search)
                    .id(egui::Id::unique(self.id).with("search"))
                    .hint_text("Find"),
            );
            if search.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                if let Some(hit) = find_text(document, &state.search, 0) {
                    state.search_hit = Some(hit);
                    state.search_from = hit;
                    state.search_error = None;
                } else if !state.search.is_empty() {
                    state.search_error = Some(format!("Could not find \"{}\".", state.search));
                }
            }
            let toc = ui.add(egui::Button::new("TOC").selected(state.toc_visible));
            if toc.clicked() {
                state.toc_visible = !state.toc_visible;
            }
            ui.label("&Zoom:");
            ui.add(egui::Slider::new(&mut state.zoom, 25..=400).step_by(10.0));
        });
        if let Some(message) = state.search_error.clone() {
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
        let body_height = (ui.available_height() - if self.ok_visible { 32.0 } else { 26.0 }).max(60.0);
        ui.horizontal_top(|ui| {
            if state.toc_visible {
                let width = state.toc_width.clamp(120.0, ui.available_width() * 0.6);
                ui.allocate_ui_with_layout(
                    egui::vec2(width, body_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt(format!("{}-toc", self.id))
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for (index, entry) in document.toc.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        ui.add_space(entry.depth as f32 * 12.0);
                                        if ui.selectable_label(false, &entry.text).clicked() {
                                            state.search_hit = Some(entry.paragraph);
                                            state.search_from = entry.paragraph;
                                            let _ = index;
                                        }
                                    });
                                }
                            });
                    },
                );
                // `splTOC` drag.
                let (rect, response) = ui.allocate_exact_size(egui::vec2(4.0, body_height), egui::Sense::drag());
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
                if response.dragged() {
                    state.toc_width = (state.toc_width + response.drag_delta().x).clamp(120.0, 600.0);
                }
                ui.painter().rect_filled(
                    rect,
                    egui::CornerRadius::ZERO,
                    ui.visuals().widgets.noninteractive.bg_stroke.color,
                );
            }
            // The body takes the width that is left (a `horizontal` child
            // would otherwise be laid out at its minimal width, one
            // character per line).
            let width = ui.available_width().max(80.0);
            ui.allocate_ui_with_layout(
                egui::vec2(width, body_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let scroll_to = state.search_hit.take();
                    let zoom = state.zoom as f32 / 100.0;
                    // The What's New document has thousands of paragraphs;
                    // only the rows the viewport shows are laid out.
                    let row_height = 20.0 * zoom;
                    egui::ScrollArea::vertical()
                        .id_salt(format!("{}-body", self.id))
                        .auto_shrink([false, false])
                        .max_height(body_height)
                        .show_rows(ui, row_height, document.paragraphs.len(), |ui, range| {
                            ui.set_width(ui.available_width());
                            for index in range {
                                let Some(paragraph) = document.paragraphs.get(index) else {
                                    continue;
                                };
                                let response = ui.add(paragraph_label(paragraph, zoom));
                                if scroll_to == Some(index) {
                                    response.scroll_to_me(Some(egui::Align::Center));
                                }
                            }
                        });
                },
            );
        });
        ui.separator();
        ui.horizontal(|ui| {
            if self.dont_show_again_visible {
                let checkbox = ui.checkbox(&mut state.dont_show_again, self.dont_show_again_caption);
                if checkbox.changed() {
                    output.dont_show_again_changed = true;
                }
            }
            if self.ok_visible {
                output.ok_clicked = ui.add_enabled(self.ok_enabled, egui::Button::new("OK")).clicked();
            }
        });
        output
    }
}

/// What the RTF viewer's controls did this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RtfViewerOutput {
    pub ok_clicked: bool,
    pub dont_show_again_changed: bool,
}

/// One paragraph as a laid-out label with the runs' bold and size.
fn paragraph_label(paragraph: &RtfParagraph, zoom: f32) -> egui::Label {
    let mut job = egui::text::LayoutJob::default();
    for run in &paragraph.runs {
        if run.text.is_empty() {
            continue;
        }
        let mut format = egui::TextFormat {
            font_id: egui::FontId::proportional(run.size * zoom),
            ..Default::default()
        };
        if run.bold {
            // egui has no bold face of the proportional font: a stronger
            // colour carries the emphasis (UPSTREAM-QUIRK: the port has no
            // bold font file to load).
            format.color = egui::Color32::from_gray(40);
        }
        job.append(&run.text, 0.0, format);
    }
    egui::Label::new(job).wrap_mode(egui::TextWrapMode::Wrap)
}

/// Parses RTF bytes into paragraphs, bold runs and a table of contents.
pub fn parse_rtf(bytes: &[u8]) -> RtfDocument {
    let text = String::from_utf8_lossy(bytes);
    let mut document = RtfDocument::default();
    let mut paragraph = RtfParagraph::default();
    let mut bold = false;
    let mut size = 12.0f32;
    // Group state saved on `{` and restored on `}`.
    let mut stack: Vec<(bool, f32, bool)> = Vec::new();
    // A skip group (`{\fonttbl`, `{\*\...}`, `{\colortbl`, ...) drops its
    // text, but the groups inside it nest.
    let mut skip_depth = 0usize;
    let mut chars = text.chars().peekable();
    let push = |paragraph: &mut RtfParagraph, bold: bool, size: f32, run: &str| {
        if run.is_empty() {
            return;
        }
        if let Some(last) = paragraph.runs.last_mut()
            && last.bold == bold
            && last.size == size
        {
            last.text.push_str(run);
            return;
        }
        paragraph.runs.push(RtfRun {
            text: run.to_owned(),
            bold,
            size,
        });
    };
    while let Some(ch) = chars.next() {
        match ch {
            '{' => {
                stack.push((bold, size, skip_depth > 0));
                // A group whose first token is a destination we do not show.
                let mut lookahead = String::new();
                let mut clone = chars.clone();
                // A destination follows the opening brace's backslash
                // (`{onttbl`).
                if clone.peek() == Some(&'\\') {
                    clone.next();
                }
                while let Some(&next) = clone.peek() {
                    if next == '\\' || next == '{' || next == '}' || next.is_whitespace() {
                        break;
                    }
                    lookahead.push(next);
                    clone.next();
                }
                if matches!(
                    lookahead.as_str(),
                    "fonttbl" | "colortbl" | "stylesheet" | "info" | "pict" | "object" | "header" | "footer"
                ) {
                    skip_depth += 1;
                }
            }
            '}' => {
                if let Some((old_bold, old_size, skipped)) = stack.pop() {
                    bold = old_bold;
                    size = old_size;
                    if !skipped && skip_depth > 0 {
                        skip_depth -= 1;
                    }
                }
            }
            '\\' => {
                let Some(&next) = chars.peek() else { continue };
                match next {
                    '\\' | '{' | '}' => {
                        chars.next();
                        if skip_depth == 0 {
                            push(&mut paragraph, bold, size, &next.to_string());
                        }
                    }
                    '\'' => {
                        // `\'hh`: a byte of the document's code page.
                        chars.next();
                        let high = chars.next().and_then(|c| c.to_digit(16));
                        let low = chars.next().and_then(|c| c.to_digit(16));
                        if let (Some(high), Some(low)) = (high, low)
                            && skip_depth == 0
                        {
                            let byte = (high * 16 + low) as u8;
                            push(&mut paragraph, bold, size, &cp1252(byte).to_string());
                        }
                    }
                    '*' => {
                        // `{\*\...}`: a destination the reader can ignore.
                        // The increment pairs with this group's own `}`, so
                        // the group's saved state stops the decrement only
                        // for a group that was already skipped.
                        chars.next();
                        skip_depth += 1;
                        if let Some(top) = stack.last_mut() {
                            top.2 = false;
                        }
                    }
                    '\n' | '\r' => {
                        chars.next();
                    }
                    _ => {
                        let mut word = String::new();
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_alphabetic() {
                                word.push(c);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        let mut parameter = String::new();
                        let mut negative = false;
                        if chars.peek() == Some(&'-') {
                            negative = true;
                            chars.next();
                        }
                        while let Some(&c) = chars.peek() {
                            if c.is_ascii_digit() {
                                parameter.push(c);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        // A single space ends a control word.
                        if chars.peek() == Some(&' ') {
                            chars.next();
                        }
                        if skip_depth > 0 {
                            continue;
                        }
                        match word.as_str() {
                            "par" | "line" => {
                                document.paragraphs.push(std::mem::take(&mut paragraph));
                            }
                            "tab" => push(&mut paragraph, bold, size, "\t"),
                            "b" => bold = parameter.parse::<i32>().map(|value| value != 0).unwrap_or(true),
                            "fs" => {
                                if let Ok(half_points) = parameter.parse::<f32>() {
                                    size = (half_points / 2.0).max(1.0);
                                }
                            }
                            "u" => {
                                // `\uN?`: the code unit, then a fallback
                                // character in a skipped `\'hh`.
                                let mut parsed = parameter.parse::<i64>().unwrap_or(0);
                                if negative {
                                    parsed = -parsed;
                                }
                                let value = if parsed < 0 { parsed + 65536 } else { parsed } as u32;
                                if let Some(c) = char::from_u32(value) {
                                    push(&mut paragraph, bold, size, &c.to_string());
                                }
                                if chars.peek() == Some(&'?') {
                                    chars.next();
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            '\r' | '\n' => {}
            _ => {
                if skip_depth == 0 {
                    push(&mut paragraph, bold, size, &ch.to_string());
                }
            }
        }
    }
    if !paragraph.runs.is_empty() {
        document.paragraphs.push(paragraph);
    }
    document.toc = build_toc(&document.paragraphs);
    document
}

/// `BuildTOC`: the bold runs that are not hyperlinks, nested by descending
/// font size; `TR.Font.Size` is the run's size and the parent of a run is
/// the nearest preceding run of a larger size.
fn build_toc(paragraphs: &[RtfParagraph]) -> Vec<TocEntry> {
    let mut entries: Vec<TocEntry> = Vec::new();
    // The sizes of the ancestors of the entry being placed.
    let mut ancestors: Vec<f32> = Vec::new();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        let mut run_text = String::new();
        let mut run_size = 0.0f32;
        let flush = |text: &str, size: f32, ancestors: &mut Vec<f32>, entries: &mut Vec<TocEntry>| {
            let title = text.trim();
            if title.is_empty() || title.starts_with("HYPERLINK") {
                return;
            }
            while ancestors.last().is_some_and(|parent| *parent <= size) {
                ancestors.pop();
            }
            entries.push(TocEntry {
                depth: ancestors.len(),
                text: title.to_owned(),
                paragraph: index,
            });
            ancestors.push(size);
        };
        for run in &paragraph.runs {
            if run.bold {
                if run_text.is_empty() {
                    run_size = run.size;
                }
                run_text.push_str(&run.text);
            } else if !run_text.is_empty() {
                flush(&run_text, run_size, &mut ancestors, &mut entries);
                run_text.clear();
            }
        }
        if !run_text.is_empty() {
            flush(&run_text, run_size, &mut ancestors, &mut entries);
        }
    }
    entries
}

/// The Windows-1252 character of a byte (the RTF documents' code page is
/// `\ansicpg1252`).
fn cp1252(byte: u8) -> char {
    match byte {
        0x80 => '€',
        0x82 => '‚',
        0x83 => 'ƒ',
        0x84 => '„',
        0x85 => '…',
        0x86 => '†',
        0x87 => '‡',
        0x88 => 'ˆ',
        0x89 => '‰',
        0x8A => 'Š',
        0x8B => '‹',
        0x8C => 'Œ',
        0x8E => 'Ž',
        0x91 => '‘',
        0x92 => '’',
        0x93 => '“',
        0x94 => '”',
        0x95 => '•',
        0x96 => '–',
        0x97 => '—',
        0x98 => '˜',
        0x99 => '™',
        0x9A => 'š',
        0x9B => '›',
        0x9C => 'œ',
        0x9E => 'ž',
        0x9F => 'Ÿ',
        other => other as char,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_rtf_document_keeps_its_paragraphs_and_bold_runs() {
        let rtf =
            br"{\rtf1\ansi{\fonttbl{\f0 Segoe UI;}}\pard\b\fs36 Request for Mod Authors\b0\fs24\par Body text.\par}";
        let document = parse_rtf(rtf);
        assert_eq!(document.paragraphs.len(), 2);
        assert_eq!(document.paragraphs[0].text(), "Request for Mod Authors");
        assert_eq!(document.paragraphs[1].text(), "Body text.");
        assert!(document.paragraphs[0].runs[0].bold);
        assert_eq!(document.paragraphs[0].runs[0].size, 18.0);
        assert_eq!(document.toc.len(), 1);
        assert_eq!(document.toc[0].text, "Request for Mod Authors");
        assert_eq!(document.toc[0].depth, 0);
    }

    #[test]
    fn a_skip_group_is_not_shown() {
        let document = parse_rtf(br"{\rtf1{\info{\author someone}}Shown\par}");
        assert_eq!(document.text(), "Shown");
    }

    #[test]
    fn the_escapes_are_decoded() {
        let document = parse_rtf(br"{\rtf1 caf\'e9 \tab done\par}");
        assert_eq!(document.text(), "café \tdone");
    }

    #[test]
    fn the_toc_nests_by_font_size() {
        let rtf = br"{\rtf1\b\fs36 Top\b0\fs24\par\b\fs32 Child\b0\fs24\par\b\fs36 Next\b0\par}";
        let document = parse_rtf(rtf);
        let toc: Vec<(usize, &str)> = document
            .toc
            .iter()
            .map(|entry| (entry.depth, entry.text.as_str()))
            .collect();
        assert_eq!(toc, vec![(0, "Top"), (1, "Child"), (0, "Next")]);
    }

    #[test]
    fn the_search_starts_at_the_from_paragraph() {
        let mut document = RtfDocument::default();
        for text in ["alpha", "beta", "alpha"] {
            document.paragraphs.push(RtfParagraph {
                runs: vec![RtfRun {
                    text: text.to_owned(),
                    bold: false,
                    size: 12.0,
                }],
            });
        }
        assert_eq!(find_text(&document, "alpha", 0), Some(0));
        assert_eq!(find_text(&document, "alpha", 1), Some(2));
        assert_eq!(find_text(&document, "beta", 0), Some(1));
        assert_eq!(find_text(&document, "gamma", 0), None);
        assert_eq!(find_text(&document, "", 0), None);
    }

    #[test]
    fn the_zoom_keys_follow_the_form() {
        let document = RtfDocument::default();
        let mut state = RtfViewerState::default();
        assert!(rtf_key(
            &mut state,
            &document,
            egui::Key::Equals,
            egui::Modifiers::COMMAND
        ));
        assert_eq!(state.zoom, 110);
        assert!(rtf_key(
            &mut state,
            &document,
            egui::Key::Minus,
            egui::Modifiers::COMMAND
        ));
        assert_eq!(state.zoom, 100);
        assert!(rtf_key(
            &mut state,
            &document,
            egui::Key::T,
            egui::Modifiers {
                alt: true,
                ..egui::Modifiers::NONE
            }
        ));
        assert!(state.toc_visible);
    }

    #[test]
    fn the_upstream_tips_parse() {
        let document = parse_rtf(include_bytes!("../../resources/whats_new.rtf"));
        assert!(!document.is_empty());
        assert!(document.paragraphs.len() > 50);
        assert!(!document.toc.is_empty());
        assert!(document.text().contains("Request for Mod Authors"));
    }
}
