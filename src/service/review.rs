//! Sprint Review record services.

use crate::error::{Error, Result};
use crate::review::SprintReview;
use crate::service::{open_board, open_board_locked};
use crate::sprint::SprintId;
use crate::storage::{SprintRepository, SprintReviewRepository};
use chrono::Utc;
use std::path::Path;

/// Create the one Review belonging to an existing Sprint.
///
/// The parent may be planned, active, or closed. A second Review for the same Sprint is rejected
/// before any write.
///
/// # Errors
///
/// Returns [`Error::SprintNotFound`], [`Error::ReviewExists`], or persistence/Git errors.
pub async fn create_sprint_review(
    project_dir: &Path,
    sprint_id: &SprintId,
    body: String,
) -> Result<SprintReview> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    SprintRepository::load(&repo, sprint_id).await?;
    match SprintReviewRepository::load(&repo, sprint_id).await {
        Ok(_) => return Err(Error::ReviewExists(sprint_id.clone())),
        Err(Error::ReviewNotFound(_)) => {}
        Err(error) => return Err(error),
    }

    let review = SprintReview::new(sprint_id.clone(), body, Utc::now());
    SprintReviewRepository::save(&repo, &review).await?;
    repo.commit(&format!("pinto: add review {}", review.id))
        .await?;
    Ok(review)
}

/// Replace the body of an existing Sprint Review.
///
/// # Errors
///
/// Returns [`Error::ReviewNotFound`] or persistence/Git errors. A missing parent does not prevent
/// editing a previously stored record because the Review itself is the target of this operation.
pub async fn edit_sprint_review(
    project_dir: &Path,
    sprint_id: &SprintId,
    body: String,
) -> Result<SprintReview> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut review = SprintReviewRepository::load(&repo, sprint_id).await?;
    review.update_body(body, Utc::now());
    SprintReviewRepository::save(&repo, &review).await?;
    repo.commit(&format!("pinto: update review {}", review.id))
        .await?;
    Ok(review)
}

/// Load one Sprint Review without modifying the board.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::ReviewNotFound`], or persistence errors.
pub async fn show_sprint_review(project_dir: &Path, sprint_id: &SprintId) -> Result<SprintReview> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    SprintReviewRepository::load(&repo, sprint_id).await
}

/// List all Sprint Reviews in creation order.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`] or persistence errors.
pub async fn list_sprint_reviews(project_dir: &Path) -> Result<Vec<SprintReview>> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    SprintReviewRepository::list(&repo).await
}
