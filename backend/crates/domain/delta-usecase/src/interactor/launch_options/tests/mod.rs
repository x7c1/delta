//! Launch-option registry use-case tests.

use delta_model::LaunchOptionPreset;

use crate::interactor::testing::*;

mod a_default_is_one_per_group_per_provider_and_repeatable_names_are_free;
mod a_single_valued_row_is_grouped_by_its_name;
mod create_preserves_each_options_provider;
mod create_refuses_a_dangerous_option_as_default_enabled;
mod create_then_list_returns_registered_options_newest_first;
mod create_valueless_flag_keeps_label_and_value_none;
mod delete_refuses_a_shipped_option;
mod delete_removes_only_the_named_option;
mod expand_leading_tilde;
mod reconcile_inserts_a_declared_preset_as_a_real_row;
mod reconcile_is_idempotent_including_ids;
mod reconcile_preserves_a_ticked_builtin;
mod reconcile_retires_an_undeclared_builtin_and_spares_user_rows;
mod reconcile_updates_declared_content_in_place;
mod rejects_creating_a_second_default_in_one_choice_group;
mod rejects_enabling_a_second_default_in_one_choice_group;
mod set_default_enabled_refuses_a_dangerous_option;
mod set_default_enabled_toggles_in_place;
mod the_null_vocabulary_groups_and_rejects_nothing;

/// A shipped preset, for the reconcile and delete-refusal tests.
fn preset(key: &'static str, label: &'static str, value: &'static str) -> LaunchOptionPreset {
    LaunchOptionPreset {
        key,
        label,
        name: "--model",
        value: Some(value),
        provider: crate::AgentProvider::Claude,
    }
}

/// A test interactor wired with [`FakeLaunchOptionVocabulary`].
fn interactor_with_a_vocabulary() -> TestInteractor {
    interactor().with_launch_option_vocabulary(std::sync::Arc::new(FakeLaunchOptionVocabulary))
}
