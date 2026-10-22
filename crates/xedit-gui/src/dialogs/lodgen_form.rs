// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: xEdit/xeLODGenForm.pas (FormCreate, FormShow,
// FormClose, FormKeyDown, bnSplitTreesLODClick, cbBuildAtlasClick,
// cbChunkClick, cbObjectsLODClick, cmbCompDiffuseChange, SelectAll1Click,
// SelectNone1Click) and xEdit/xeLODGenForm.dfm (clbWorldspace, pnlBottom,
// gbObjectsOptions, the combo boxes and labels) and
// xEdit/xeMainForm.pas (mniNavGenerateLODClick: the dialog's fields read
// from the `[<APP> LOD Options]` section and written back; the FO4 trees
// LOD switch-off)

//! `TfrmLODGen`: the worldspace checklist and the object and tree LOD
//! options, run through `lodgen.generate`.
//!
//! The worldspaces and the option values come from a dry run of the command
//! (`mniNavGenerateLODClick`'s listing), the way the form takes them from
//! the settings section. OK sends the checked worldspaces and the options;
//! "Split Trees LOD" (upstream hidden behind Shift) sends `split_trees`.
//! The command also writes the options back to the section, as the form
//! does.

use std::collections::BTreeMap;

use eframe::egui;

use xedit_core::interface::globals::{is_fallout3, is_fallout4, is_skyrim, is_starfield};
use xedit_session::lodgen::{
    Lists, LodgenResponse, LodgenWorldspace, form_lists, image_format_name, str_to_image_format,
};

use crate::dialogs::selection::fit_size;

/// What the dialog asks the shell to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LodgenAction {
    /// `btnOk`: `lodgen.generate` with the checked worldspaces and options.
    Ok,
    /// The hidden `btnSplitTreesLOD`: `lodgen.generate` with `split_trees`.
    SplitTrees,
    /// `btnCancel`.
    Cancelled,
}

/// The enabled and visible state of the controls after the clicks of the
/// form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LodgenRules {
    /// `gbObjectsOptions.Enabled`/`Visible`.
    pub objects_options: bool,
    /// `cmbLODLevel`, `edLODX`, `edLODY`, `edLODX2` and `edLODY2`.
    pub chunk_fields: bool,
    /// `cmbAtlasTextureSize`, `cmbAtlasTextureUVRange`, `cbNoTangents`,
    /// `cbNoVertexColors` and `cmbDefaultAlphaThreshold` (Fallout 3 and New
    /// Vegas disable them).
    pub atlas_texture_options: bool,
    /// `lblLODX2`/`edLODX2` and `lblLODY2`/`edLODY2` (Fallout 3 and New
    /// Vegas only).
    pub second_lod_area: bool,
    /// `cmbCompSpecular` (Fallout 4 and later only).
    pub comp_specular: bool,
    /// `cmbTreesLODBrightness`.
    pub trees_brightness: bool,
    /// `cbTrees3D` (Skyrim with the trees LOD checked).
    pub trees_3d: bool,
    /// `cbTreesLOD` (unchecked and disabled in Fallout 4 and Starfield).
    pub trees_lod: bool,
    /// `cbUseAlphaThreshold`.
    pub use_alpha_threshold: bool,
}

impl LodgenRules {
    /// The state `cbObjectsLODClick`, `cbChunkClick`, `cmbCompDiffuseChange`
    /// and the Fallout switches of `mniNavGenerateLODClick` leave.
    pub fn for_form(objects_lod: bool, trees_lod: bool, chunk: bool, diffuse: &str, trees_3d: bool) -> Self {
        let fallout3 = is_fallout3();
        let fallout4 = is_fallout4() || is_starfield();
        let skyrim = is_skyrim();
        Self {
            objects_options: objects_lod,
            chunk_fields: chunk,
            atlas_texture_options: !fallout3,
            second_lod_area: fallout3,
            comp_specular: is_fallout4(),
            trees_brightness: trees_lod && !fallout4,
            trees_3d: skyrim && trees_lod && trees_3d,
            trees_lod: trees_lod && !fallout4,
            // `if wbIsFallout4 and MatchStr(cmbCompDiffuse.Text, ['8888',
            // 'DXT3', 'DXT5']) then cbUseAlphaThreshold.Enabled := True`.
            use_alpha_threshold: fallout4 && matches!(diffuse, "8888" | "DXT3" | "DXT5"),
        }
    }
}

/// `TfrmLODGen` as data.
#[derive(Clone, Debug, PartialEq)]
pub struct LodgenState {
    /// `clbWorldspace`: the worldspaces of the dry run with their checks.
    pub worldspaces: Vec<LodgenWorldspace>,
    /// `cbObjectsLOD`.
    pub objects_lod: bool,
    /// `cbTreesLOD`.
    pub trees_lod: bool,
    /// `cbTrees3D`.
    pub trees_3d: bool,
    /// `gbObjectsOptions`: `cbBuildAtlas`.
    pub build_atlas: bool,
    pub no_tangents: bool,
    pub no_vertex_colors: bool,
    pub use_alpha_threshold: bool,
    pub use_backlight_power: bool,
    /// `cmbDefaultAlphaThreshold.Text`.
    pub default_alpha_threshold: String,
    /// `cbChunk` with `cmbLODLevel` and the four edit boxes.
    pub chunk: bool,
    pub lod_level: String,
    pub lod_x: String,
    pub lod_y: String,
    pub lod_x2: String,
    pub lod_y2: String,
    /// `cmbAtlasWidth`, `cmbAtlasHeight`, `cmbAtlasTextureSize`,
    /// `cmbAtlasTextureUVRange`.
    pub atlas_width: String,
    pub atlas_height: String,
    pub atlas_texture_size: String,
    pub atlas_texture_uv_range: String,
    /// `cmbCompDiffuse`, `cmbCompNormal`, `cmbCompSpecular`.
    pub comp_diffuse: String,
    pub comp_normal: String,
    pub comp_specular: String,
    /// `cmbTreesLODBrightness.Text`.
    pub trees_brightness: String,
    /// The combo boxes' items (`FormCreate`).
    pub lists: Lists,
    /// `btnSplitTreesLOD.Visible`: upstream shows it only while Shift is
    /// held when the dialog opens (Skyrim and the Fallouts before 4).
    pub split_visible: bool,
    /// `ShowMessage('Select worldspace(s) for LOD generation')`, which
    /// `FormClose` answers with `Action := caNone`.
    pub error: Option<String>,
}

impl Default for LodgenState {
    fn default() -> Self {
        Self::from_worldspaces(Vec::new(), &BTreeMap::new())
    }
}

/// Reads an option of the dry-run response by its name (the command reports
/// the settings section's values).
fn option<'a>(options: &'a BTreeMap<String, String>, name: &str, default: &'a str) -> &'a str {
    options
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
        .unwrap_or(default)
}

fn option_bool(options: &BTreeMap<String, String>, name: &str, default: bool) -> bool {
    matches!(
        option(options, name, if default { "1" } else { "0" })
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// The image format of an option, which the command writes as its ordinal
/// (`ImageFormatToStr` of the value).
fn option_format(options: &BTreeMap<String, String>, name: &str, default: &str) -> String {
    let text = option(options, name, "");
    match text.trim().parse::<i64>() {
        Ok(ordinal) => image_format_name(ordinal),
        Err(_) if !text.is_empty() => text.to_owned(),
        Err(_) => default.to_owned(),
    }
}

impl LodgenState {
    /// `mniNavGenerateLODClick` after the worldspace list and the section
    /// are read.
    pub fn from_response(response: &LodgenResponse) -> Self {
        Self::from_worldspaces(response.worldspaces.clone(), &response.options)
    }

    fn from_worldspaces(worldspaces: Vec<LodgenWorldspace>, options: &BTreeMap<String, String>) -> Self {
        let mut state = Self {
            worldspaces,
            objects_lod: option_bool(options, "ObjectsLOD", true),
            trees_lod: option_bool(options, "TreesLOD", true),
            trees_3d: option_bool(options, "Trees3D", false),
            build_atlas: option_bool(options, "BuildAtlas", true),
            no_tangents: option_bool(options, "ObjectsNoTangents", false),
            no_vertex_colors: option_bool(options, "ObjectsNoVertexColors", false),
            use_alpha_threshold: option_bool(options, "ObjectsUseAlphaThreshold", false),
            use_backlight_power: option_bool(options, "ObjectsUseBacklightPower", false),
            default_alpha_threshold: option(options, "DefaultAlphaThreshold", "128").to_owned(),
            chunk: option_bool(options, "Chunk", false),
            lod_level: option(options, "LODLevel", "").to_owned(),
            lod_x: option(options, "LODX", "").to_owned(),
            lod_y: option(options, "LODY", "").to_owned(),
            lod_x2: option(options, "LODX2", "").to_owned(),
            lod_y2: option(options, "LODY2", "").to_owned(),
            atlas_width: option(options, "AtlasWidth", "4096").to_owned(),
            atlas_height: option(options, "AtlasHeight", "4096").to_owned(),
            atlas_texture_size: option(options, "AtlasTextureSize", "512").to_owned(),
            atlas_texture_uv_range: option(options, "AtlasTextureUVRange", "1.1").to_owned(),
            comp_diffuse: option_format(options, "AtlasDiffuseFormat", "DXT5"),
            comp_normal: option_format(options, "AtlasNormalFormat", "BC5"),
            comp_specular: option_format(options, "AtlasSpecularFormat", "DXT1"),
            trees_brightness: option(options, "TreesBrightness", "0").to_owned(),
            lists: form_lists(),
            split_visible: false,
            error: None,
        };
        // `if wbIsFallout4 or wbIsStarfield then cbTreesLOD.Checked := False`.
        if is_fallout4() || is_starfield() {
            state.trees_lod = false;
        }
        state
    }

    /// The enabled state of the controls for the current values.
    pub fn rules(&self) -> LodgenRules {
        LodgenRules::for_form(
            self.objects_lod,
            self.trees_lod,
            self.chunk,
            &self.comp_diffuse,
            self.trees_3d,
        )
    }

    /// `SelectAll1Click`: the enabled items checked.
    pub fn select_all(&mut self) {
        for worldspace in &mut self.worldspaces {
            worldspace.checked = true;
        }
    }

    /// `SelectNone1Click`.
    pub fn select_none(&mut self) {
        for worldspace in &mut self.worldspaces {
            worldspace.checked = false;
        }
    }

    /// The worldspaces `worldspaces` of the request: the checked editor IDs.
    pub fn checked_worldspaces(&self) -> Vec<String> {
        self.worldspaces
            .iter()
            .filter(|worldspace| worldspace.checked)
            .map(|worldspace| worldspace.editor_id.clone())
            .collect()
    }

    /// `FormClose`: `if ModalResult = mrOk then` at least one worldspace
    /// must be checked, else `ShowMessage` and `Action := caNone`.
    pub fn commit(&mut self) -> bool {
        if self.checked_worldspaces().is_empty() {
            self.error = Some("Select worldspace(s) for LOD generation".to_owned());
            false
        } else {
            self.error = None;
            true
        }
    }

    /// `cbBuildAtlasClick`: Fallout 3 and New Vegas have to build the
    /// combined atlas.
    pub fn build_atlas_clicked(&mut self) {
        if !self.build_atlas && is_fallout3() {
            self.build_atlas = true;
            self.error =
                Some("Fallout 3 and New Vegas LODs won't work without combined atlas of LOD textures".to_owned());
        }
    }

    /// The options map `lodgen.generate` takes, with the form's names and
    /// the command's value shapes.
    pub fn to_options(&self) -> BTreeMap<String, String> {
        let b = |value: bool| if value { "1" } else { "0" }.to_owned();
        let mut options = BTreeMap::new();
        options.insert("ObjectsLOD".to_owned(), b(self.objects_lod));
        options.insert("TreesLOD".to_owned(), b(self.trees_lod));
        options.insert("Trees3D".to_owned(), b(self.trees_3d));
        options.insert("BuildAtlas".to_owned(), b(self.build_atlas));
        options.insert("AtlasWidth".to_owned(), self.atlas_width.clone());
        options.insert("AtlasHeight".to_owned(), self.atlas_height.clone());
        options.insert("AtlasTextureSize".to_owned(), self.atlas_texture_size.clone());
        options.insert("AtlasTextureUVRange".to_owned(), self.atlas_texture_uv_range.clone());
        options.insert(
            "AtlasDiffuseFormat".to_owned(),
            (str_to_image_format(&self.comp_diffuse) as i32).to_string(),
        );
        options.insert(
            "AtlasNormalFormat".to_owned(),
            (str_to_image_format(&self.comp_normal) as i32).to_string(),
        );
        options.insert(
            "AtlasSpecularFormat".to_owned(),
            (str_to_image_format(&self.comp_specular) as i32).to_string(),
        );
        options.insert("DefaultAlphaThreshold".to_owned(), self.default_alpha_threshold.clone());
        options.insert("ObjectsNoTangents".to_owned(), b(self.no_tangents));
        options.insert("ObjectsNoVertexColors".to_owned(), b(self.no_vertex_colors));
        options.insert("ObjectsUseAlphaThreshold".to_owned(), b(self.use_alpha_threshold));
        options.insert("ObjectsUseBacklightPower".to_owned(), b(self.use_backlight_power));
        options.insert("Chunk".to_owned(), b(self.chunk));
        options.insert("LODLevel".to_owned(), self.lod_level.clone());
        options.insert("LODX".to_owned(), self.lod_x.clone());
        options.insert("LODY".to_owned(), self.lod_y.clone());
        options.insert("LODX2".to_owned(), self.lod_x2.clone());
        options.insert("LODY2".to_owned(), self.lod_y2.clone());
        options.insert("TreesBrightness".to_owned(), self.trees_brightness.clone());
        options
    }

    /// The `lodgen.generate` request (`LodgenRequest`'s fields). `settings`
    /// is the settings file the section is read from and written back to.
    pub fn to_params(&self, settings: Option<String>, split_trees: bool) -> serde_json::Value {
        let mut params = serde_json::json!({
            "worldspaces": self.checked_worldspaces(),
            "options": self.to_options(),
            "split_trees": split_trees,
            "dry_run": false,
        });
        if let Some(settings) = settings
            && let Some(object) = params.as_object_mut()
        {
            object.insert("settings".to_owned(), serde_json::Value::String(settings));
        }
        params
    }
}

/// Draws the dialog.
pub fn show(ctx: &egui::Context, state: &mut LodgenState) -> Option<LodgenAction> {
    let mut action = None;
    let rules = state.rules();
    let mut open = true;
    egui::Window::new("LODGen")
        .id(egui::Id::unique("frmLODGen"))
        .collapsible(false)
        .resizable(true)
        .default_size(fit_size(ctx, egui::vec2(520.0, 560.0)))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Worldspaces");
                if ui.small_button("+").on_hover_text("Select All").clicked() {
                    state.select_all();
                }
                if ui.small_button("-").on_hover_text("Select None").clicked() {
                    state.select_none();
                }
                if state.split_visible && ui.button("Split Trees LOD").clicked() {
                    action = Some(LodgenAction::SplitTrees);
                }
            });
            egui::ScrollArea::vertical()
                .id_salt("lodgen-worldspaces")
                .max_height(170.0)
                .show(ui, |ui| {
                    for worldspace in &mut state.worldspaces {
                        ui.checkbox(&mut worldspace.checked, &worldspace.name);
                    }
                });
            ui.separator();
            ui.checkbox(&mut state.objects_lod, "Objects LOD");
            ui.add_enabled(rules.trees_lod, egui::Checkbox::new(&mut state.trees_lod, "Trees LOD"));
            ui.add_enabled(
                rules.trees_3d,
                egui::Checkbox::new(&mut state.trees_3d, "Trees LOD in 3D"),
            );
            ui.horizontal(|ui| {
                ui.add_enabled(rules.trees_brightness, egui::Label::new("Trees LOD brightness"));
                ui.add_enabled_ui(rules.trees_brightness, |ui| {
                    combo(
                        ui,
                        "lodgen-trees-brightness",
                        &mut state.trees_brightness,
                        &state.lists.brightness,
                    );
                });
            });
            ui.separator();
            ui.add_enabled_ui(rules.objects_options, |ui| {
                ui.label("Objects options");
                if ui.checkbox(&mut state.build_atlas, "Build atlas").changed() {
                    state.build_atlas_clicked();
                }
                ui.add_enabled(
                    rules.atlas_texture_options,
                    egui::Checkbox::new(&mut state.no_tangents, "No tangents"),
                );
                ui.add_enabled(
                    rules.atlas_texture_options,
                    egui::Checkbox::new(&mut state.no_vertex_colors, "No vertex colors"),
                );
                ui.add_enabled(
                    rules.use_alpha_threshold,
                    egui::Checkbox::new(&mut state.use_alpha_threshold, "Use alpha threshold"),
                );
                ui.checkbox(&mut state.use_backlight_power, "Use backlight power");
                ui.horizontal(|ui| {
                    ui.label("Default alpha threshold");
                    ui.add_enabled_ui(rules.atlas_texture_options, |ui| {
                        combo(
                            ui,
                            "lodgen-alpha",
                            &mut state.default_alpha_threshold,
                            &state.lists.alpha_threshold,
                        );
                    });
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.chunk, "Chunk");
                    ui.add_enabled_ui(rules.chunk_fields, |ui| {
                        combo(ui, "lodgen-lod-level", &mut state.lod_level, &state.lists.lod_level);
                        ui.add(egui::TextEdit::singleline(&mut state.lod_x).desired_width(50.0));
                        ui.add(egui::TextEdit::singleline(&mut state.lod_y).desired_width(50.0));
                        if rules.second_lod_area {
                            ui.add(egui::TextEdit::singleline(&mut state.lod_x2).desired_width(50.0));
                            ui.add(egui::TextEdit::singleline(&mut state.lod_y2).desired_width(50.0));
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Atlas width");
                    combo(
                        ui,
                        "lodgen-atlas-width",
                        &mut state.atlas_width,
                        &state.lists.atlas_size,
                    );
                    ui.label("height");
                    combo(
                        ui,
                        "lodgen-atlas-height",
                        &mut state.atlas_height,
                        &state.lists.atlas_size,
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Texture size");
                    ui.add_enabled_ui(rules.atlas_texture_options, |ui| {
                        combo(
                            ui,
                            "lodgen-texture-size",
                            &mut state.atlas_texture_size,
                            &state.lists.texture_size,
                        );
                    });
                    ui.label("UV range");
                    ui.add_enabled_ui(rules.atlas_texture_options, |ui| {
                        combo(
                            ui,
                            "lodgen-uv-range",
                            &mut state.atlas_texture_uv_range,
                            &state.lists.uv_range,
                        );
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Diffuse");
                    combo(ui, "lodgen-diffuse", &mut state.comp_diffuse, &state.lists.compression);
                    ui.label("Normal");
                    combo(ui, "lodgen-normal", &mut state.comp_normal, &state.lists.compression);
                    ui.label("Specular");
                    ui.add_enabled_ui(rules.comp_specular, |ui| {
                        combo(
                            ui,
                            "lodgen-specular",
                            &mut state.comp_specular,
                            &state.lists.compression,
                        );
                    });
                });
            });
            if let Some(error) = &state.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() && state.commit() {
                    action = Some(LodgenAction::Ok);
                }
                if ui.button("Cancel").clicked() {
                    action = Some(LodgenAction::Cancelled);
                }
            });
        });
    if !open && action.is_none() {
        action = Some(LodgenAction::Cancelled);
    }
    action
}

/// One of the form's combo boxes.
fn combo(ui: &mut egui::Ui, id: &str, value: &mut String, items: &[String]) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.clone())
        .show_ui(ui, |ui| {
            for item in items {
                ui.selectable_value(value, item.clone(), item);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use xedit_core::interface::globals::{GameMode, game_mode, set_game_mode, test_lock};

    fn response() -> LodgenResponse {
        LodgenResponse {
            worldspaces: vec![
                LodgenWorldspace {
                    editor_id: "Tamriel".to_owned(),
                    name: "Tamriel".to_owned(),
                    checked: true,
                },
                LodgenWorldspace {
                    editor_id: "Solstheim".to_owned(),
                    name: "Solstheim".to_owned(),
                    checked: false,
                },
            ],
            options: BTreeMap::new(),
            resources: Vec::new(),
            messages: Vec::new(),
            seed: None,
            dry_run: true,
        }
    }

    #[test]
    fn the_dry_run_fills_the_form() {
        let _guard = test_lock();
        let previous = game_mode();
        set_game_mode(GameMode::gmSSE);
        let mut response = response();
        response.options.insert("ObjectsLOD".to_owned(), "1".to_owned());
        response.options.insert("TreesLOD".to_owned(), "0".to_owned());
        response.options.insert("AtlasWidth".to_owned(), "2048".to_owned());
        // The command writes the `TImageFormat` ordinal: 200 is `ifDXT1`.
        response
            .options
            .insert("AtlasDiffuseFormat".to_owned(), "200".to_owned());
        let state = LodgenState::from_response(&response);
        assert_eq!(state.worldspaces.len(), 2);
        assert!(state.worldspaces[0].checked);
        assert!(state.objects_lod);
        assert!(!state.trees_lod);
        assert_eq!(state.atlas_width, "2048");
        // `ImageFormatToStr(ifDXT1)` = 'DXT1' for ordinal 4.
        assert_eq!(state.comp_diffuse, "DXT1");
        assert_eq!(state.checked_worldspaces(), vec!["Tamriel".to_owned()]);
        set_game_mode(previous);
    }

    #[test]
    fn select_all_and_none_follow_the_list() {
        let mut state = LodgenState::from_response(&response());
        state.select_none();
        assert!(state.checked_worldspaces().is_empty());
        state.select_all();
        assert_eq!(state.checked_worldspaces().len(), 2);
    }

    #[test]
    fn ok_needs_a_checked_worldspace() {
        let mut state = LodgenState::from_response(&response());
        state.select_none();
        assert!(!state.commit());
        assert_eq!(state.error.as_deref(), Some("Select worldspace(s) for LOD generation"));
        state.select_all();
        assert!(state.commit());
        assert!(state.error.is_none());
    }

    #[test]
    fn the_rules_follow_the_fallout_switches() {
        let _guard = test_lock();
        let previous = game_mode();
        // `cbObjectsLODClick` of Fallout 3 and New Vegas disables the atlas
        // texture options and shows the second LOD area (`LODX2`/`LODY2`).
        set_game_mode(GameMode::gmFNV);
        let rules = LodgenRules::for_form(true, true, true, "DXT1", true);
        assert!(!rules.atlas_texture_options);
        assert!(rules.second_lod_area);
        assert!(!rules.comp_specular);
        assert!(!rules.trees_3d);

        // Fallout 4: the trees LOD is off, the specular format and the alpha
        // threshold of DXT5 apply, `cbTrees3D` is never enabled.
        set_game_mode(GameMode::gmFO4);
        let rules = LodgenRules::for_form(true, true, true, "DXT5", true);
        assert!(!rules.trees_lod);
        assert!(rules.comp_specular);
        assert!(rules.use_alpha_threshold);
        assert!(!rules.trees_3d);
        let rules = LodgenRules::for_form(true, true, true, "DXT1", true);
        assert!(!rules.use_alpha_threshold);

        // Skyrim: the trees 3D option needs Skyrim and the trees LOD.
        set_game_mode(GameMode::gmSSE);
        let rules = LodgenRules::for_form(true, true, true, "DXT1", true);
        assert!(rules.trees_lod);
        assert!(rules.trees_3d);
        assert!(rules.atlas_texture_options);
        assert!(!rules.second_lod_area);
        set_game_mode(previous);
    }

    #[test]
    fn the_options_carry_the_form_values_in_the_commands_shapes() {
        let _guard = test_lock();
        let mut state = LodgenState::from_response(&response());
        state.objects_lod = false;
        state.comp_diffuse = "DXT1".to_owned();
        state.comp_normal = "BC5".to_owned();
        let options = state.to_options();
        assert_eq!(options["ObjectsLOD"], "0");
        assert_eq!(
            options["AtlasDiffuseFormat"],
            (str_to_image_format("DXT1") as i32).to_string()
        );
        assert_eq!(
            options["AtlasNormalFormat"],
            (str_to_image_format("BC5") as i32).to_string()
        );
        assert_eq!(options["AtlasWidth"], "4096");
        let params = state.to_params(Some("xEdit.ini".to_owned()), false);
        assert_eq!(params["worldspaces"][0], "Tamriel");
        assert_eq!(params["settings"], "xEdit.ini");
        assert_eq!(params["split_trees"], false);
        assert_eq!(params["dry_run"], false);
    }
}
