include!("quota_browse_compatibility_test.rs");
include!("quota_browse_rendering_test.rs");
include!("quota_presentation_support_test.rs");
include!("quota_provider_filters_test.rs");

include!("quota_help_presentation_test.rs");

#[path = "quota_choice_dialog_test.rs"]
mod quota_choice_dialog_test;

#[path = "quota_storyboard_fixture_test.rs"]
mod quota_storyboard_fixture_test;

#[path = "quota_ux_clarity_test.rs"]
pub(super) mod quota_ux_clarity_test;

#[path = "quota_visual_contract_test.rs"]
mod quota_visual_contract_test;

#[path = "quota_group_observation_test.rs"]
mod quota_group_observation_test;

#[path = "quota_normal_browse_pose_test.rs"]
mod quota_normal_browse_pose_test;

#[path = "quota_hard_fixture_consistency_test.rs"]
mod quota_hard_fixture_consistency_test;
