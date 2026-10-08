// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Sniff (`S'Lanter's NIF Helper`): batch operations on NIF, KF and
//! material files of a folder or an archive. `processor` is the framework
//! of `SniffProcessor.pas`, `main_form` the automation mode of the main
//! form, and each `proc_*` module one unit of `Sniff/Proc`. `procs` lists
//! the operations in the order of the main form, with the ones not ported
//! yet.

pub mod main_form;
pub mod perl_regex;
pub mod processor;
pub mod procs;

pub mod proc_adjust_transform;
pub mod proc_apply_transform;
pub mod proc_attach_parent;
pub mod proc_check_for_errors;
pub mod proc_convert_root_node;
pub mod proc_fix_exported_kf_anim;
pub mod proc_havok_search_material;
pub mod proc_havok_settings_update;
pub mod proc_inertia_update;
pub mod proc_json_converter;
pub mod proc_ragdoll_constraint_update;
pub mod proc_remove_nodes;
pub mod proc_remove_unused_nodes;
pub mod proc_replace_assets;
pub mod proc_set_missing_names;
pub mod proc_shader_flags_update;
pub mod proc_soft_particles;
pub mod proc_tangents;
pub mod proc_universal_fixer;
pub mod proc_universal_tweaker;
pub mod proc_unskin_mesh;
pub mod proc_update_bounds;
pub mod proc_walls_reflection_flag;
