// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Ported from xEdit: Core/wbInterface.pas

//! The global settings of `wbInterface.pas`.
//!
//! Upstream keeps its configuration in unit-level variables that the program
//! sets at startup and that code everywhere reads. The port keeps the same
//! shape: each variable is a process-wide atomic with a getter and a setter
//! named after the upstream variable. One process therefore serves one game
//! mode, as upstream does. Tests that change a setting must hold
//! [`test_lock`] so that they do not run in parallel.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, RwLock};

use xedit_io::Encoding;

use super::types::{KNOWN_SUB_RECORD_SIGNATURES, KnownSubRecord, KnownSubRecordSignatures, Signature};

macro_rules! atomic_type {
    (bool) => {
        AtomicBool
    };
    (i32) => {
        AtomicI32
    };
}

/// Declares one atomic per upstream variable with its getter and setter.
macro_rules! globals {
    ($($(#[$meta:meta])* $get:ident, $set:ident, $upstream:ident: $kind:tt = $default:expr;)*) => {
        $(
            #[allow(non_upper_case_globals)]
            static $upstream: atomic_type!($kind) = <atomic_type!($kind)>::new($default);

            $(#[$meta])*
            #[doc = concat!("Upstream `", stringify!($upstream), "`.")]
            #[inline]
            pub fn $get() -> $kind {
                $upstream.load(Ordering::Relaxed)
            }

            #[doc = concat!("Sets upstream `", stringify!($upstream), "`.")]
            pub fn $set(value: $kind) {
                $upstream.store(value, Ordering::Relaxed);
            }
        )*

        /// Restores every setting declared here to its upstream default.
        pub fn reset_globals() {
            $($upstream.store($default, Ordering::Relaxed);)*
        }
    };
}

/// Declares one string per upstream variable with its getter and setter.
macro_rules! string_globals {
    ($($(#[$meta:meta])* $get:ident, $set:ident, $upstream:ident;)*) => {
        $(
            #[allow(non_upper_case_globals)]
            static $upstream: RwLock<String> = RwLock::new(String::new());

            $(#[$meta])*
            #[doc = concat!("Upstream `", stringify!($upstream), "`.")]
            pub fn $get() -> String {
                $upstream.read().unwrap().clone()
            }

            #[doc = concat!("Sets upstream `", stringify!($upstream), "`.")]
            pub fn $set(value: &str) {
                *$upstream.write().unwrap() = value.to_owned();
            }
        )*

        /// Clears every string setting declared here.
        pub fn reset_string_globals() {
            $($upstream.write().unwrap().clear();)*
        }
    };
}

globals! {
    force_terminate, set_force_terminate, wbForceTerminate: bool = false;
    display_load_order_form_id, set_display_load_order_form_id, wbDisplayLoadOrderFormID: bool = false;
    pretty_form_id, set_pretty_form_id, wbPrettyFormID: bool = false;
    simple_records, set_simple_records, wbSimpleRecords: bool = true;
    decode_texture_hashes, set_decode_texture_hashes, wbDecodeTextureHashes: bool = true;
    fixup_pgrd, set_fixup_pgrd, wbFixupPGRD: bool = false;
    i_know_what_im_doing, set_i_know_what_im_doing, wbIKnowWhatImDoing: bool = false;
    hide_unused, set_hide_unused, wbHideUnused: bool = true;
    hide_ignored, set_hide_ignored, wbHideIgnored: bool = true;
    hide_never_show, set_hide_never_show, wbHideNeverShow: bool = true;
    show_form_version, set_show_form_version, wbShowFormVersion: bool = false;
    show_flag_enum_value, set_show_flag_enum_value, wbShowFlagEnumValue: bool = false;
    show_group_record_count, set_show_group_record_count, wbShowGroupRecordCount: bool = false;
    show_file_flags, set_show_file_flags, wbShowFileFlags: bool = false;
    display_shorter_names, set_display_shorter_names, wbDisplayShorterNames: bool = false;
    sort_sub_records, set_sort_sub_records, wbSortSubRecords: bool = false;
    sort_flst, set_sort_flst, wbSortFLST: bool = false;
    can_sort_info, set_can_sort_info, wbCanSortINFO: bool = false;
    sort_info, set_sort_info, wbSortINFO: bool = false;
    fill_pnam, set_fill_pnam, wbFillPNAM: bool = false;
    fill_inom, set_fill_inom, wbFillINOM: bool = true;
    fill_inoa, set_fill_inoa, wbFillINOA: bool = true;
    remove_offset_data, set_remove_offset_data, wbRemoveOffsetData: bool = true;
    edit_allowed, set_edit_allowed, wbEditAllowed: bool = false;
    flags_as_array, set_flags_as_array, wbFlagsAsArray: bool = false;
    delay_load_records, set_delay_load_records, wbDelayLoadRecords: bool = true;
    extended_int_unknowns, set_extended_int_unknowns, wbExtendedIntUnknowns: bool = true;
    more_info_for_unknown, set_more_info_for_unknown, wbMoreInfoForUnknown: bool = false;
    more_info_for_index, set_more_info_for_index, wbMoreInfoForIndex: bool = false;
    make_unknown_elements_unique, set_make_unknown_elements_unique, wdMakeUnknownElementsUnique: bool = false;
    translation_mode, set_translation_mode, wbTranslationMode: bool = false;
    test_write, set_test_write, wbTestWrite: bool = false;
    /// add wbNewHeaderAddon value to the headers of mainrecords and GRUP records
    force_new_header, set_force_new_header, wbForceNewHeader: bool = false;
    require_load_order, set_require_load_order, wbRequireLoadOrder: bool = false;
    create_contained_in, set_create_contained_in, wbCreateContainedIn: bool = true;
    vwd_in_temporary, set_vwd_in_temporary, wbVWDInTemporary: bool = false;
    vwd_as_quest_children, set_vwd_as_quest_children, wbVWDAsQuestChildren: bool = false;
    resolve_alias, set_resolve_alias, wbResolveAlias: bool = true;
    actor_template_hide, set_actor_template_hide, wbActorTemplateHide: bool = true;
    clamp_form_id, set_clamp_form_id, wbClampFormID: bool = true;
    align_array_elements, set_align_array_elements, wbAlignArrayElements: bool = true;
    align_array_limit, set_align_array_limit, wbAlignArrayLimit: i32 = 5000;
    copy_is_running, set_copy_is_running, wbCopyIsRunning: i32 = 0;
    size_of_main_record_struct, set_size_of_main_record_struct, wbSizeOfMainRecordStruct: i32 = 0;
    ignore_light, set_ignore_light, wbIgnoreLight: bool = false;
    pseudo_light, set_pseudo_light, wbPseudoLight: bool = false;
    ignore_medium, set_ignore_medium, wbIgnoreMedium: bool = false;
    pseudo_medium, set_pseudo_medium, wbPseudoMedium: bool = false;
    has_added_light_support, set_has_added_light_support, wbHasAddedLightSupport: bool = false;
    has_added_medium_support, set_has_added_medium_support, wbHasAddedMediumSupport: bool = false;
    has_added_optimized_support, set_has_added_optimized_support, wbHasAddedOptimizedSupport: bool = false;
    has_added_update_support, set_has_added_update_support, wbHasAddedUpdateSupport: bool = false;
    allow_edit_hedr_version, set_allow_edit_hedr_version, wbAllowEditHEDRVersion: bool = false;
    allow_edit_game_master, set_allow_edit_game_master, wbAllowEditGameMaster: bool = false;
    ignore_update, set_ignore_update, wbIgnoreUpdate: bool = false;
    pseudo_update, set_pseudo_update, wbPseudoUpdate: bool = false;
    allow_direct_save, set_allow_direct_save, wbAllowDirectSave: bool = false;
    /// must be set before DefineDefs
    allow_master_files_edit, set_allow_master_files_edit, wbAllowMasterFilesEdit: bool = false;
    can_add_scripts, set_can_add_scripts, wbCanAddScripts: bool = true;
    can_add_script_properties, set_can_add_script_properties, wbCanAddScriptProperties: bool = true;
    edit_info_use_short_name, set_edit_info_use_short_name, wbEditInfoUseShortName: bool = false;
    dev_mode, set_dev_mode, wbDevMode: bool = false;
    strip_empty_masters, set_strip_empty_masters, wbStripEmptyMasters: bool = false;
    strip_masters, set_strip_masters, wbStripMasters: bool = false;
    always_sorted, set_always_sorted, wbAlwaysSorted: bool = false;
    never_sorted, set_never_sorted, wbNeverSorted: bool = false;
    themes_supported, set_themes_supported, wbThemesSupported: bool = true;
    report_mod_groups, set_report_mod_groups, wbReportModGroups: bool = false;
    require_ctrl_for_dbl_click, set_require_ctrl_for_dbl_click, wbRequireCtrlForDblClick: bool = false;
    focus_added_element, set_focus_added_element, wbFocusAddedElement: bool = true;
    check_non_cpn_chars, set_check_non_cpn_chars, wbCheckNonCPNChars: bool = false;
    show_string_bytes, set_show_string_bytes, wbShowStringBytes: bool = false;
    reset_modified_on_save, set_reset_modified_on_save, wbResetModifiedOnSave: bool = true;
    always_save_onam, set_always_save_onam, wbAlwaysSaveOnam: bool = false;
    always_save_onam_force, set_always_save_onam_force, wbAlwaysSaveOnamForce: bool = false;
    manual_cleaning_allow, set_manual_cleaning_allow, wbManualCleaningAllow: bool = false;
    manual_cleaning_hide, set_manual_cleaning_hide, wbManualCleaningHide: bool = false;
    convert_int_form_id, set_convert_int_form_id, wbConvertIntFormID: bool = false;
    shrink_buttons, set_shrink_buttons, wbShrinkButtons: bool = false;
    collapse_record_header, set_collapse_record_header, wbCollapseRecordHeader: bool = true;
    collapse_object_bounds, set_collapse_object_bounds, wbCollapseObjectBounds: bool = true;
    collapse_models, set_collapse_models, wbCollapseModels: bool = true;
    collapse_factions, set_collapse_factions, wbCollapseFactions: bool = true;
    collapse_faction_relations, set_collapse_faction_relations, wbCollapseFactionRelations: bool = true;
    collapse_fragments, set_collapse_fragments, wbCollapseFragments: bool = true;
    collapse_items, set_collapse_items, wbCollapseItems: bool = true;
    collapse_leveled_items, set_collapse_leveled_items, wbCollapseLeveledItems: bool = true;
    collapse_equip_slots, set_collapse_equip_slots, wbCollapseEquipSlots: bool = true;
    collapse_object_properties, set_collapse_object_properties, wbCollapseObjectProperties: bool = true;
    collapse_script_properties, set_collapse_script_properties, wbCollapseScriptProperties: bool = true;
    collapse_conditions, set_collapse_conditions, wbCollapseConditions: bool = true;
    collapse_benign_array, set_collapse_benign_array, wbCollapseBenignArray: bool = true;
    collapse_rgba, set_collapse_rgba, wbCollapseRGBA: bool = true;
    collapse_vec3, set_collapse_vec3, wbCollapseVec3: bool = true;
    collapse_pos_rot, set_collapse_pos_rot, wbCollapsePosRot: bool = true;
    collapse_range, set_collapse_range, wbCollapseRange: bool = true;
    collapse_arma_bone_data, set_collapse_arma_bone_data, wbCollapseARMABoneData: bool = true;
    collapse_race_bone_data, set_collapse_race_bone_data, wbCollapseRACEBoneData: bool = true;
    collapse_script_data, set_collapse_script_data, wbCollapseScriptData: bool = true;
    collapse_head_parts, set_collapse_head_parts, wbCollapseHeadParts: bool = true;
    collapse_body_parts, set_collapse_body_parts, wbCollapseBodyParts: bool = true;
    collapse_model_info_texture, set_collapse_model_info_texture, wbCollapseModelInfoTexture: bool = true;
    collapse_model_info_textures, set_collapse_model_info_textures, wbCollapseModelInfoTextures: bool = true;
    collapse_model_info_addons, set_collapse_model_info_addons, wbCollapseModelInfoAddons: bool = true;
    collapse_model_info_material, set_collapse_model_info_material, wbCollapseModelInfoMaterial: bool = true;
    collapse_model_info_materials, set_collapse_model_info_materials, wbCollapseModelInfoMaterials: bool = true;
    collapse_model_info, set_collapse_model_info, wbCollapseModelInfo: bool = true;
    collapse_model_info_header, set_collapse_model_info_header, wbCollapseModelInfoHeader: bool = true;
    collapse_time_interpolator, set_collapse_time_interpolator, wbCollapseTimeInterpolator: bool = true;
    collapse_time_interpolators, set_collapse_time_interpolators, wbCollapseTimeInterpolators: bool = true;
    collapse_time_interpolators_mult_add, set_collapse_time_interpolators_mult_add, wbCollapseTimeInterpolatorsMultAdd: bool = true;
    collapse_blue_print_item, set_collapse_blue_print_item, wbCollapseBluePrintItem: bool = true;
    collapse_placement, set_collapse_placement, wbCollapsePlacement: bool = true;
    collapse_vertices, set_collapse_vertices, wbCollapseVertices: bool = true;
    collapse_rdsa, set_collapse_rdsa, wbCollapseRDSA: bool = true;
    collapse_flags, set_collapse_flags, wbCollapseFlags: bool = true;
    collapse_transforms, set_collapse_transforms, wbCollapseTransforms: bool = true;
    collapse_sounds, set_collapse_sounds, wbCollapseSounds: bool = true;
    collapse_destruction, set_collapse_destruction, wbCollapseDestruction: bool = true;
    collapse_locations, set_collapse_locations, wbCollapseLocations: bool = true;
    collapse_navmesh, set_collapse_navmesh, wbCollapseNavmesh: bool = true;
    /// catch all for things not explicitly defined with their own value
    collapse_other, set_collapse_other, wbCollapseOther: bool = true;
    collapse_perk, set_collapse_perk, wbCollapsePerk: bool = true;
    collapse_keywords, set_collapse_keywords, wbCollapseKeywords: bool = true;
    collapse_faction_ranks, set_collapse_faction_ranks, wbCollapseFactionRanks: bool = true;
    collapse_ownership, set_collapse_ownership, wbCollapseOwnership: bool = true;
    collapse_object_palette_defaults, set_collapse_object_palette_defaults, wbCollapseObjectPaletteDefaults: bool = true;
    collapse_traversal, set_collapse_traversal, wbCollapseTraversal: bool = true;
    collapse_base_form_component, set_collapse_base_form_component, wbCollapseBaseFormComponent: bool = true;
    collapse_vehicle_config, set_collapse_vehicle_config, wbCollapseVehicleConfig: bool = true;
    collapse_weather_time_of_day, set_collapse_weather_time_of_day, wbCollapseWeatherTimeOfDay: bool = true;
    collapse_weather_cloud_textures, set_collapse_weather_cloud_textures, wbCollapseWeatherCloudTextures: bool = true;
    collapse_weather_cloud_speed, set_collapse_weather_cloud_speed, wbCollapseWeatherCloudSpeed: bool = true;
    collapse_weather_cloud_alphas, set_collapse_weather_cloud_alphas, wbCollapseWeatherCloudAlphas: bool = true;
    collapse_ragdoll, set_collapse_ragdoll, wbCollapseRagdoll: bool = true;
    collapse_direction_rotation, set_collapse_direction_rotation, wbCollapseDirectionRotation: bool = true;
    collapse_max_height_data, set_collapse_max_height_data, wbCollapseMaxHeightData: bool = true;
    collapse_aliases, set_collapse_aliases, wbCollapseAliases: bool = true;
    collapse_quest_stage, set_collapse_quest_stage, wbCollapseQuestStage: bool = true;
    collapse_quest_log, set_collapse_quest_log, wbCollapseQuestLog: bool = true;
    collapse_quest_objective, set_collapse_quest_objective, wbCollapseQuestObjective: bool = true;
    collapse_quest_objective_target, set_collapse_quest_objective_target, wbCollapseQuestObjectiveTarget: bool = true;
    collapse_script_entry, set_collapse_script_entry, wbCollapseScriptEntry: bool = true;
    dont_draw_color_text, set_dont_draw_color_text, wbDontDrawColorText: bool = true;
    report_injected, set_report_injected, wbReportInjected: bool = true;
    no_full_in_short_name, set_no_full_in_short_name, wbNoFullInShortName: bool = true;
    no_index_in_alias_summary, set_no_index_in_alias_summary, wbNoIndexInAliasSummary: bool = true;
    extended_light, set_extended_light, wbExtendedLight: bool = false;
    always_fast_assign, set_always_fast_assign, wbAlwaysFastAssign: bool = false;
    show_raw_data, set_show_raw_data, wbShowRawData: bool = false;
    compare_raw_data, set_compare_raw_data, wbCompareRawData: bool = false;
    disable_form_id_check, set_disable_form_id_check, wbDisableFormIDCheck: bool = false;
    complex_file_file_id, set_complex_file_file_id, wbComplexFileFileID: bool = false;
    /// adds all masters of masters when adding a master and prevents cleaning them
    enforce_all_masters, set_enforce_all_masters, wbEnforceAllMasters: bool = false;
    cs, set_cs, wbCS: bool = false;
    obme, set_obme, wbOBME: bool = false;
    vresl, set_vresl, wbVRESL: bool = false;
    allow_make_partial, set_allow_make_partial, wbAllowMakePartial: bool = false;
    hedr_next_object_id, set_hedr_next_object_id, wbHEDRNextObjectID: i32 = 0x800;
    dont_save, set_dont_save, wbDontSave: bool = false;
    dont_cache, set_dont_cache, wbDontCache: bool = false;
    dont_cache_load, set_dont_cache_load, wbDontCacheLoad: bool = false;
    dont_cache_save, set_dont_cache_save, wbDontCacheSave: bool = false;
    cache_records_threshold, set_cache_records_threshold, wbCacheRecordsThreshold: i32 = 500;
    auto_compare_selected_limit, set_auto_compare_selected_limit, wbAutoCompareSelectedLimit: i32 = 5;
    udr_set_xesp, set_udr_set_xesp, wbUDRSetXESP: bool = true;
    udr_set_scale, set_udr_set_scale, wbUDRSetScale: bool = false;
    udr_set_z, set_udr_set_z, wbUDRSetZ: bool = true;
    udr_set_mstt, set_udr_set_mstt, wbUDRSetMSTT: bool = true;
    master_update_filter_onam, set_master_update_filter_onam, wbMasterUpdateFilterONAM: bool = false;
    master_update_fix_persistence, set_master_update_fix_persistence, wbMasterUpdateFixPersistence: bool = true;
    allow_internal_edit, set_allow_internal_edit, wbAllowInternalEdit: bool = true;
    show_internal_edit, set_show_internal_edit, wbShowInternalEdit: bool = false;
    report_mode, set_report_mode, wbReportMode: bool = false;
    report_unused, set_report_unused, wbReportUnused: bool = false;
    report_required, set_report_required, wbReportRequired: bool = false;
    report_unused_data, set_report_unused_data, wbReportUnusedData: bool = true;
    report_unknown_form_ids, set_report_unknown_form_ids, wbReportUnknownFormIDs: bool = true;
    report_unknown_halfs, set_report_unknown_halfs, wbReportUnknownHalfs: bool = false;
    report_unknown_floats, set_report_unknown_floats, wbReportUnknownFloats: bool = true;
    report_unknown_doubles, set_report_unknown_doubles, wbReportUnknownDoubles: bool = true;
    report_unknown_strings, set_report_unknown_strings, wbReportUnknownStrings: bool = true;
    report_unknown_l_strings, set_report_unknown_l_strings, wbReportUnknownLStrings: bool = true;
    report_empty, set_report_empty, wbReportEmpty: bool = true;
    report_sometimes_empty, set_report_sometimes_empty, wbReportSometimesEmpty: bool = true;
    report_form_ids, set_report_form_ids, wbReportFormIDs: bool = true;
    report_not_found_but_allowed_form_ids, set_report_not_found_but_allowed_form_ids, wbReportNotFoundButAllowedFormIDs: bool = false;
    report_unknown_flags, set_report_unknown_flags, wbReportUnknownFlags: bool = true;
    report_unknown_enums, set_report_unknown_enums, wbReportUnknownEnums: bool = true;
    report_form_id_not_allowed_references, set_report_form_id_not_allowed_references, wbReportFormIDNotAllowedReferences: bool = true;
    report_unknown, set_report_unknown, wbReportUnknown: bool = false;
    show_data_size_in_value, set_show_data_size_in_value, wbShowDataSizeInValue: bool = false;
    sub_record_errors_only, set_sub_record_errors_only, wbSubRecordErrorsOnly: bool = false;
    report_unknown_step, set_report_unknown_step, wbReportUnknownStep: i32 = 1;
    more_info_for_required, set_more_info_for_required, wbMoreInfoForRequired: bool = false;
    more_info_for_decider, set_more_info_for_decider, wbMoreInfoForDecider: bool = false;
    track_all_editor_id, set_track_all_editor_id, wbTrackAllEditorID: bool = false;
    show_tip, set_show_tip, wbShowTip: bool = true;
    patron, set_patron, wbPatron: bool = false;
    no_git_hub_check, set_no_git_hub_check, wbNoGitHubCheck: bool = false;
    no_nexus_mods_check, set_no_nexus_mods_check, wbNoNexusModsCheck: bool = false;
    check_expected_bytes, set_check_expected_bytes, wbCheckExpectedBytes: bool = true;
    angle_digits, set_angle_digits, wbAngleDigits: i32 = 4;
    /// 1= starting offset, 2 = Count, 3 = Offsets, size and count
    dump_offset, set_dump_offset, wbDumpOffset: i32 = 0;
    should_load_mo_hook_file, set_should_load_mo_hook_file, wbShouldLoadMOHookFile: bool = false;
    allow_esp_masters, set_allow_esp_masters, wbAllowESPMasters: bool = false;
    allow_esp_masters_on_save, set_allow_esp_masters_on_save, wbAllowESPMastersOnSave: bool = false;
    starfield_is_a_bug_infested_hellhole, set_starfield_is_a_bug_infested_hellhole, wbStarfieldIsABugInfestedHellhole: bool = false;
    red_pill, set_red_pill, wbRedPill: bool = false;
    always_load_game_master, set_always_load_game_master, wbAlwaysLoadGameMaster: bool = true;
    speed_over_memory, set_speed_over_memory, wbSpeedOverMemory: bool = false;
    dark_mode, set_dark_mode, wbDarkMode: bool = false;
}

string_globals! {
    sub_mode, set_sub_mode, wbSubMode;
    app_name, set_app_name, wbAppName;
    application_title, set_application_title, wbApplicationTitle;
    /// Name of the exe, usually also name of the game master.
    game_name, set_game_name, wbGameName;
    game_exe_name, set_game_exe_name, wbGameExeName;
    creation_club_content_file_name, set_creation_club_content_file_name, wbCreationClubContentFileName;
    nexus_mods_url, set_nexus_mods_url, wbNexusModsUrl;
    /// Name of the game master, usually `game_name` plus `.esm`, different for Fallout 76.
    game_master_esm, set_game_master_esm, wbGameMasterEsm;
    /// Game title name used for the AppData and My Games folders.
    game_name2, set_game_name2, wbGameName2;
    /// Registry name.
    game_name_reg, set_game_name_reg, wbGameNameReg;
    tool_name, set_tool_name, wbToolName;
    source_name, set_source_name, wbSourceName;
    language, set_language, wbLanguage;
    game_steam_id, set_game_steam_id, wbGameSteamID;
    program_path, set_program_path, wbProgramPath;
    data_path, set_data_path, wbDataPath;
    the_game_ini_file_name, set_the_game_ini_file_name, wbTheGameIniFileName;
    custom_ini_file_name, set_custom_ini_file_name, wbCustomIniFileName;
}

/// Game modes, ordered by release date as upstream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum GameMode {
    gmTES3,
    gmTES4,
    gmTES4R,
    gmFO3,
    gmFNV,
    gmTES5,
    gmEnderal,
    gmFO4,
    gmSSE,
    gmTES5VR,
    gmEnderalSE,
    gmFO4VR,
    gmFO76,
    gmSF1,
}

impl GameMode {
    pub const ALL: [GameMode; 14] = [
        GameMode::gmTES3,
        GameMode::gmTES4,
        GameMode::gmTES4R,
        GameMode::gmFO3,
        GameMode::gmFNV,
        GameMode::gmTES5,
        GameMode::gmEnderal,
        GameMode::gmFO4,
        GameMode::gmSSE,
        GameMode::gmTES5VR,
        GameMode::gmEnderalSE,
        GameMode::gmFO4VR,
        GameMode::gmFO76,
        GameMode::gmSF1,
    ];

    /// The name of the enumeration value without its `gm` prefix, as upstream
    /// derives it with `GetEnumName` for switches and titles.
    pub fn name(self) -> &'static str {
        match self {
            GameMode::gmTES3 => "TES3",
            GameMode::gmTES4 => "TES4",
            GameMode::gmTES4R => "TES4R",
            GameMode::gmFO3 => "FO3",
            GameMode::gmFNV => "FNV",
            GameMode::gmTES5 => "TES5",
            GameMode::gmEnderal => "Enderal",
            GameMode::gmFO4 => "FO4",
            GameMode::gmSSE => "SSE",
            GameMode::gmTES5VR => "TES5VR",
            GameMode::gmEnderalSE => "EnderalSE",
            GameMode::gmFO4VR => "FO4VR",
            GameMode::gmFO76 => "FO76",
            GameMode::gmSF1 => "SF1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum ToolMode {
    tmView,
    tmEdit,
    tmDump,
    tmExport,
    tmOnamUpdate,
    tmMasterUpdate,
    tmMasterRestore,
    tmLODgen,
    tmScript,
    tmTranslate,
    tmESMify,
    tmESPify,
    tmSortAndCleanMasters,
    tmCheckForErrors,
    tmCheckForITM,
    tmCheckForDR,
    tmGenerateSEQ,
}

impl ToolMode {
    pub const ALL: [ToolMode; 17] = [
        ToolMode::tmView,
        ToolMode::tmEdit,
        ToolMode::tmDump,
        ToolMode::tmExport,
        ToolMode::tmOnamUpdate,
        ToolMode::tmMasterUpdate,
        ToolMode::tmMasterRestore,
        ToolMode::tmLODgen,
        ToolMode::tmScript,
        ToolMode::tmTranslate,
        ToolMode::tmESMify,
        ToolMode::tmESPify,
        ToolMode::tmSortAndCleanMasters,
        ToolMode::tmCheckForErrors,
        ToolMode::tmCheckForITM,
        ToolMode::tmCheckForDR,
        ToolMode::tmGenerateSEQ,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum ToolSource {
    tsPlugins,
    tsSaves,
}

#[allow(non_upper_case_globals)]
static wbGameMode: AtomicU8 = AtomicU8::new(GameMode::gmTES3 as u8);
#[allow(non_upper_case_globals)]
static wbToolMode: AtomicU8 = AtomicU8::new(ToolMode::tmView as u8);
#[allow(non_upper_case_globals)]
static wbToolSource: AtomicU8 = AtomicU8::new(ToolSource::tsPlugins as u8);

static HEADER_SIGNATURE: RwLock<Signature> = RwLock::new(Signature::new(b"TES4"));

/// Upstream `wbHeaderSignature`: the signature of the file header record.
pub fn header_signature() -> Signature {
    *HEADER_SIGNATURE.read().unwrap()
}

pub fn set_header_signature(value: Signature) {
    *HEADER_SIGNATURE.write().unwrap() = value;
}

static HEDR_VERSION: RwLock<f64> = RwLock::new(1.0);

/// Upstream `wbHEDRVersion`: the version the file header of the game has.
pub fn hedr_version() -> f64 {
    *HEDR_VERSION.read().unwrap()
}

pub fn set_hedr_version(value: f64) {
    *HEDR_VERSION.write().unwrap() = value;
}

static GROUP_ORDER: RwLock<Vec<Signature>> = RwLock::new(Vec::new());

/// Upstream `wbAddGroupOrder`: the next group signature in the order of the groups of a file.
pub fn wb_add_group_order(a_signature: Signature) {
    GROUP_ORDER.write().unwrap().push(a_signature);
}

/// Upstream `wbGetGroupOrder`: the position of the group, or -1.
pub fn wb_get_group_order(a_signature: Signature) -> i32 {
    GROUP_ORDER
        .read()
        .unwrap()
        .iter()
        .position(|signature| *signature == a_signature)
        .map_or(-1, |position| position as i32)
}

/// Empties the group order, for the tests.
pub fn clear_group_order() {
    GROUP_ORDER.write().unwrap().clear();
}

static IGNORE_RECORDS: RwLock<Vec<Signature>> = RwLock::new(Vec::new());
static OFFICIAL_DLC: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Upstream `wbIgnoreRecords`: the signatures of the records that are skipped.
pub fn ignore_records() -> Vec<Signature> {
    IGNORE_RECORDS.read().unwrap().clone()
}

/// Upstream `wbIgnoreRecords.Add`.
pub fn ignore_records_add(signature: Signature) {
    IGNORE_RECORDS.write().unwrap().push(signature);
}

/// Upstream `wbOfficialDLC`: the file names of the official DLC of the game.
pub fn official_dlc() -> Vec<String> {
    OFFICIAL_DLC.read().unwrap().clone()
}

/// Upstream `SetLength(wbOfficialDLC, ...)`.
pub fn set_official_dlc_length(length: i32) {
    OFFICIAL_DLC
        .write()
        .unwrap()
        .resize(usize::try_from(length).unwrap_or(0), String::new());
}

/// Upstream `wbOfficialDLC[aIndex] := ...`.
pub fn set_official_dlc_at(index: i32, value: &str) {
    OFFICIAL_DLC.write().unwrap()[usize::try_from(index).expect("a non-negative index")] = value.to_owned();
}

/// Upstream `wbGameMode`.
#[inline]
pub fn game_mode() -> GameMode {
    GameMode::ALL[wbGameMode.load(Ordering::Relaxed) as usize]
}

/// Sets upstream `wbGameMode`.
pub fn set_game_mode(value: GameMode) {
    wbGameMode.store(value as u8, Ordering::Relaxed);
}

/// Upstream `wbToolMode`.
pub fn tool_mode() -> ToolMode {
    ToolMode::ALL[wbToolMode.load(Ordering::Relaxed) as usize]
}

/// Sets upstream `wbToolMode`.
pub fn set_tool_mode(value: ToolMode) {
    wbToolMode.store(value as u8, Ordering::Relaxed);
}

/// Upstream `wbToolSource`.
pub fn tool_source() -> ToolSource {
    if wbToolSource.load(Ordering::Relaxed) == ToolSource::tsSaves as u8 {
        ToolSource::tsSaves
    } else {
        ToolSource::tsPlugins
    }
}

/// Sets upstream `wbToolSource`.
pub fn set_tool_source(value: ToolSource) {
    wbToolSource.store(value as u8, Ordering::Relaxed);
}

pub fn is_morrowind() -> bool {
    matches!(game_mode(), GameMode::gmTES3)
}

pub fn is_oblivion() -> bool {
    matches!(game_mode(), GameMode::gmTES4 | GameMode::gmTES4R)
}

pub fn is_oblivion_r() -> bool {
    matches!(game_mode(), GameMode::gmTES4R)
}

pub fn is_fallout3() -> bool {
    matches!(game_mode(), GameMode::gmFO3 | GameMode::gmFNV)
}

pub fn is_fallout_nv() -> bool {
    matches!(game_mode(), GameMode::gmFNV)
}

pub fn is_skyrim() -> bool {
    matches!(
        game_mode(),
        GameMode::gmTES5 | GameMode::gmEnderal | GameMode::gmTES5VR | GameMode::gmSSE | GameMode::gmEnderalSE
    )
}

pub fn is_skyrim_se() -> bool {
    matches!(
        game_mode(),
        GameMode::gmTES5VR | GameMode::gmSSE | GameMode::gmEnderalSE
    )
}

pub fn is_fallout4() -> bool {
    matches!(game_mode(), GameMode::gmFO4 | GameMode::gmFO4VR)
}

pub fn is_fallout76() -> bool {
    matches!(game_mode(), GameMode::gmFO76)
}

pub fn is_starfield() -> bool {
    matches!(game_mode(), GameMode::gmSF1)
}

#[inline]
pub fn is_light_supported() -> bool {
    matches!(
        game_mode(),
        GameMode::gmSSE | GameMode::gmEnderalSE | GameMode::gmFO4 | GameMode::gmSF1
    ) || has_added_light_support()
}

#[inline]
pub fn is_medium_supported() -> bool {
    matches!(game_mode(), GameMode::gmSF1) || has_added_medium_support()
}

pub fn is_blueprint_supported() -> bool {
    matches!(game_mode(), GameMode::gmSF1)
}

pub fn is_update_supported() -> bool {
    matches!(game_mode(), GameMode::gmSF1) || has_added_update_support()
}

/// An encoding as an integer: 0 is UTF-8, any other value a code page.
fn encoding_to_bits(encoding: Encoding) -> u32 {
    match encoding {
        Encoding::Utf8 => 0,
        Encoding::Mbcs(code_page) => code_page,
    }
}

fn encoding_from_bits(bits: u32) -> Encoding {
    if bits == 0 {
        Encoding::Utf8
    } else {
        Encoding::Mbcs(bits)
    }
}

#[allow(non_upper_case_globals)]
static wbEncoding: AtomicU32 = AtomicU32::new(1252);
#[allow(non_upper_case_globals)]
static wbEncodingTrans: AtomicU32 = AtomicU32::new(1252);
#[allow(non_upper_case_globals)]
static wbEncodingVMAD: AtomicU32 = AtomicU32::new(0);

/// Upstream `wbEncoding`: the encoding of strings that are not translated.
pub fn encoding() -> Encoding {
    encoding_from_bits(wbEncoding.load(Ordering::Relaxed))
}

pub fn set_encoding(value: Encoding) {
    wbEncoding.store(encoding_to_bits(value), Ordering::Relaxed);
}

/// Upstream `wbEncodingTrans`: the encoding of translatable strings.
pub fn encoding_trans() -> Encoding {
    encoding_from_bits(wbEncodingTrans.load(Ordering::Relaxed))
}

pub fn set_encoding_trans(value: Encoding) {
    wbEncodingTrans.store(encoding_to_bits(value), Ordering::Relaxed);
}

/// Upstream `wbEncodingVMAD`: the encoding of script data.
pub fn encoding_vmad() -> Encoding {
    encoding_from_bits(wbEncodingVMAD.load(Ordering::Relaxed))
}

pub fn set_encoding_vmad(value: Encoding) {
    wbEncodingVMAD.store(encoding_to_bits(value), Ordering::Relaxed);
}

#[allow(non_upper_case_globals)]
static _InternalEditCount: AtomicI32 = AtomicI32::new(0);
#[allow(non_upper_case_globals)]
static _BlockInternalEdit: AtomicBool = AtomicBool::new(false);

/// Port of `wbBeginInternalEdit`. Each `true` result needs one [`end_internal_edit`].
pub fn begin_internal_edit(force: bool) -> bool {
    let result = edit_allowed() || ((allow_internal_edit() || force) && !_BlockInternalEdit.load(Ordering::Relaxed));
    if result {
        _InternalEditCount.fetch_add(1, Ordering::Relaxed);
    }
    result
}

pub fn end_internal_edit() {
    _InternalEditCount.fetch_sub(1, Ordering::Relaxed);
}

pub fn is_internal_edit() -> bool {
    _InternalEditCount.load(Ordering::Relaxed) > 0
}

/// Upstream `wbKnownSubRecordSignatures`: the signatures of the known
/// subrecords of the record definitions that do not give their own.
/// Morrowind changes them.
static KNOWN_SUB_RECORD_SIGNATURES_NOW: RwLock<KnownSubRecordSignatures> = RwLock::new(KNOWN_SUB_RECORD_SIGNATURES);

pub fn known_sub_record_signatures() -> KnownSubRecordSignatures {
    *KNOWN_SUB_RECORD_SIGNATURES_NOW.read().unwrap()
}

pub fn set_known_sub_record_signature(role: KnownSubRecord, signature: Signature) {
    KNOWN_SUB_RECORD_SIGNATURES_NOW.write().unwrap()[super::types::PascalEnum::ord(role)] = signature;
}

/// Upstream `TProc`: a procedure without arguments.
pub type TProc = std::sync::Arc<dyn Fn() + Send + Sync>;

static RESOURCES_LOADED_HANDLERS: Mutex<Vec<TProc>> = Mutex::new(Vec::new());

/// Port of `wbRegisterResourcesLoadedHandler`: a definition unit that reads
/// data from the resources (Starfield's Wwise sound bank index) runs once
/// the archives and the data folder are added.
pub fn wb_register_resources_loaded_handler(handler: Option<TProc>) {
    if let Some(handler) = handler {
        RESOURCES_LOADED_HANDLERS.lock().unwrap().push(handler);
    }
}

/// Port of `wbResourcesLoaded`: runs the handlers in the order they were
/// registered.
pub fn wb_resources_loaded() {
    let handlers = RESOURCES_LOADED_HANDLERS.lock().unwrap().clone();
    for handler in handlers {
        handler();
    }
}

/// Forgets the handlers, for a new set of definitions.
pub fn clear_resources_loaded_handlers() {
    RESOURCES_LOADED_HANDLERS.lock().unwrap().clear();
}

/// Restores every setting of this module to its upstream default.
pub fn reset() {
    clear_resources_loaded_handlers();
    *KNOWN_SUB_RECORD_SIGNATURES_NOW.write().unwrap() = KNOWN_SUB_RECORD_SIGNATURES;
    set_encoding(Encoding::Mbcs(1252));
    set_encoding_trans(Encoding::Mbcs(1252));
    set_encoding_vmad(Encoding::Utf8);
    _InternalEditCount.store(0, Ordering::Relaxed);
    _BlockInternalEdit.store(false, Ordering::Relaxed);
    reset_globals();
    reset_string_globals();
    set_game_mode(GameMode::gmTES3);
    set_tool_mode(ToolMode::tmView);
    set_tool_source(ToolSource::tsPlugins);
}

/// Serializes tests that change settings. The guard resets all settings when
/// it is taken, so that each test starts from the upstream defaults.
pub fn test_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    reset();
    guard
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_upstream() {
        let _guard = test_lock();
        assert!(simple_records());
        assert!(!display_load_order_form_id());
        assert_eq!(align_array_limit(), 5000);
        assert_eq!(hedr_next_object_id(), 0x800);
        assert_eq!(game_mode(), GameMode::gmTES3);
    }

    #[test]
    fn light_support_follows_game_mode() {
        let _guard = test_lock();
        assert!(!is_light_supported());
        set_game_mode(GameMode::gmFO4);
        assert!(is_light_supported() && is_fallout4() && !is_medium_supported());
        set_game_mode(GameMode::gmTES5);
        assert!(!is_light_supported() && is_skyrim() && !is_skyrim_se());
        set_has_added_light_support(true);
        assert!(is_light_supported());
        set_game_mode(GameMode::gmSF1);
        assert!(is_medium_supported() && is_update_supported() && is_blueprint_supported());
    }

    #[test]
    fn strings_are_set_and_reset() {
        let _guard = test_lock();
        set_game_name("Fallout4");
        assert_eq!(game_name(), "Fallout4");
        reset();
        assert_eq!(game_name(), "");
    }
}
