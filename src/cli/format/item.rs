//! Product backlog item text formatting.

use pinto::backlog::BacklogItem;
use pinto::service::ItemDetail;

pub(crate) use super::{DetailOptions, ListLongOptions};

/// Format the compact Product Backlog Item list.
pub(crate) fn format_list(items: &[BacklogItem]) -> String {
    super::format_list(items)
}

/// Format the detailed Product Backlog Item list.
pub(crate) fn format_list_long(
    items: &[BacklogItem],
    max_width: usize,
    options: ListLongOptions,
) -> String {
    super::format_list_long(items, max_width, options)
}

/// Format a Product Backlog Item with its relationships and common DoD.
pub(crate) fn format_detail(
    detail: &ItemDetail,
    common_dod: Option<&str>,
    options: DetailOptions,
) -> String {
    super::format_detail(detail, common_dod, options)
}
