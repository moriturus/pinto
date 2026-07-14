//! Detect columns that exceed their WIP (work in progress) limits.
//!
//! Match the per-column limits in [`crate::config::WipConfig`] against item counts and report
//! violations for `move` and `board` warnings.

use super::open_board;
use crate::backlog::{BacklogItem, Status};
use crate::config::WipConfig;
use crate::error::Result;
use std::path::Path;

/// A column whose item count exceeds its WIP limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WipViolation {
    /// Column (workflow status) name.
    pub column: String,
    /// Maximum number of items allowed in the column.
    pub limit: u32,
    /// Current number of items in the column (`> limit`).
    pub count: usize,
}

/// Compare each column limit in `config` with the corresponding item count and return the columns
/// that exceed their limits.
///
/// - Always empty (check disabled) when `config.enabled` is `false`.
/// - Columns not in `config.limits` are treated as unlimited.
/// - If the number is exactly at the upper limit, it is not exceeded (only `count > limit` is violated).
///
/// Results are ordered by column name because `limits` is a `BTreeMap`.
#[must_use]
pub fn wip_violations(config: &WipConfig, items: &[BacklogItem]) -> Vec<WipViolation> {
    if !config.enabled {
        return Vec::new();
    }
    config
        .limits
        .iter()
        .filter_map(|(column, &limit)| {
            let status = Status::new(column);
            let count = items.iter().filter(|it| it.status == status).count();
            (count as u64 > u64::from(limit)).then(|| WipViolation {
                column: column.clone(),
                limit,
                count,
            })
        })
        .collect()
}

/// Load the board in `project_dir` and return the columns that exceed the WIP limit.
///
/// Read `[wip]` configuration and all PBIs, then delegate to [`wip_violations`]. Return
/// [`crate::error::Error::NotInitialized`] for an uninitialized board.
pub async fn check_wip(project_dir: &Path) -> Result<Vec<WipViolation>> {
    let (_board_dir, repo, config) = open_board(project_dir).await?;
    let items = crate::storage::BacklogItemRepository::list(&repo).await?;
    Ok(wip_violations(&config.wip, &items))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::service::test_support::init_temp;
    use crate::service::{NewItem, add_item, move_item};

    /// Create `n` dummy PBIs with the specified status for violation tests.
    fn items_in(status: &str, n: usize) -> Vec<BacklogItem> {
        (0..n)
            .map(|i| {
                BacklogItem::new(
                    crate::backlog::ItemId::new("T", i as u32 + 1),
                    format!("item {i}"),
                    Status::new(status),
                    crate::rank::Rank::after(None),
                    chrono::Utc::now(),
                )
                .expect("valid item")
            })
            .collect()
    }

    fn cfg(enabled: bool, limits: &[(&str, u32)]) -> WipConfig {
        WipConfig {
            enabled,
            limits: limits.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        }
    }

    #[test]
    fn reports_violation_when_count_exceeds_limit() {
        let items = items_in("in-progress", 4);
        let v = wip_violations(&cfg(true, &[("in-progress", 3)]), &items);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].column, "in-progress");
        assert_eq!(v[0].limit, 3);
        assert_eq!(v[0].count, 4);
    }

    #[test]
    fn no_violation_when_count_at_or_below_limit() {
        let items = items_in("in-progress", 3);
        assert!(wip_violations(&cfg(true, &[("in-progress", 3)]), &items).is_empty());
    }

    #[test]
    fn columns_without_a_configured_limit_are_unlimited() {
        // Reviews have no configured upper limit, so any number of reviews is valid.
        let items = items_in("review", 100);
        assert!(wip_violations(&cfg(true, &[("in-progress", 1)]), &items).is_empty());
    }

    #[test]
    fn disabled_config_yields_no_violations() {
        let items = items_in("in-progress", 10);
        assert!(wip_violations(&cfg(false, &[("in-progress", 1)]), &items).is_empty());
    }

    #[test]
    fn multiple_violations_are_sorted_by_column_name() {
        let mut items = items_in("in-progress", 2);
        items.extend(items_in("review", 2));
        let v = wip_violations(&cfg(true, &[("review", 1), ("in-progress", 1)]), &items);
        let cols: Vec<_> = v.iter().map(|x| x.column.as_str()).collect();
        assert_eq!(cols, ["in-progress", "review"], "BTreeMap 由来で昇順");
    }

    #[tokio::test]
    async fn check_wip_reads_board_and_detects_overflow() {
        let dir = init_temp().await;
        // Set the in-progress upper limit of 1 to the default config.
        let path = dir.path().join(".pinto").join("config.toml");
        let mut config = Config::load(&path).await.unwrap();
        config.wip.limits.insert("in-progress".to_string(), 1);
        config.save(&path).await.unwrap();

        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        move_item(dir.path(), &a.id, "in-progress").await.unwrap();
        move_item(dir.path(), &b.id, "in-progress").await.unwrap();

        let v = check_wip(dir.path()).await.unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].column, "in-progress");
        assert_eq!(v[0].count, 2);
        assert_eq!(v[0].limit, 1);
    }
}
