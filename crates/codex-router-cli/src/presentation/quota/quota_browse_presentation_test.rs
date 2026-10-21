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
