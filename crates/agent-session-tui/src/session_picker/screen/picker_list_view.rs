//! Physical record-row geometry shared with the picker paging budget.
pub(crate) const fn session_rows_are_stacked(viewport_width: usize) -> bool {
    // At phone widths the status and two ages would consume fitting identities.
    viewport_width < 40
}
