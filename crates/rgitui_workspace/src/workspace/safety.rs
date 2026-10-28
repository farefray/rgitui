//! Pure runtime routing rules. Paths alone must never resurrect a closed tab.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RepositoryTarget<Id> {
    pub project_id: Id,
    pub worktree_path: PathBuf,
}

impl<Id: PartialEq> RepositoryTarget<Id> {
    pub fn matches(&self, project_id: &Id, worktree_path: &Path) -> bool {
        &self.project_id == project_id && self.worktree_path == worktree_path
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OperationTarget<Id> {
    pub repository: RepositoryTarget<Id>,
    pub operation_id: u64,
}

impl<Id: PartialEq> OperationTarget<Id> {
    pub fn matches(&self, project_id: &Id, operation_id: u64) -> bool {
        &self.repository.project_id == project_id && self.operation_id == operation_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(id: u64, path: &str) -> RepositoryTarget<u64> {
        RepositoryTarget {
            project_id: id,
            worktree_path: path.into(),
        }
    }

    #[test]
    fn operation_numbers_are_namespaced_by_project() {
        let op = OperationTarget {
            repository: target(1, "repo"),
            operation_id: 1,
        };
        assert!(op.matches(&1, 1));
        assert!(!op.matches(&2, 1));
        assert!(!op.matches(&1, 2));
    }

    #[test]
    fn reopened_path_is_not_the_original_runtime_target() {
        let original = target(1, "repo");
        assert!(!original.matches(&2, Path::new("repo")));
    }

    #[test]
    fn completion_requires_both_origin_and_checkout() {
        let original = target(1, "repo/worktree");
        assert!(original.matches(&1, Path::new("repo/worktree")));
        assert!(!original.matches(&1, Path::new("repo")));
        assert!(!original.matches(&2, Path::new("repo/worktree")));
    }

    #[test]
    fn completing_one_operation_keeps_other_projects_running() {
        let finished = OperationTarget {
            repository: target(1, "first"),
            operation_id: 7,
        };
        let mut running = vec![(1, 7), (2, 7), (1, 8)];
        running.retain(|(project, operation)| !finished.matches(project, *operation));
        assert_eq!(running, [(2, 7), (1, 8)]);
    }

    #[test]
    fn success_does_not_settle_another_checkout_failure() {
        let failure = target(1, "repo/worktree-a");
        assert_ne!(failure, target(1, "repo/worktree-b"));
        assert_ne!(failure, target(2, "repo/worktree-a"));
        assert_eq!(failure, target(1, "repo/worktree-a"));
    }

    #[test]
    fn closed_target_does_not_fall_back_to_another_tab() {
        let original = target(1, "repo");
        let live = [target(2, "repo"), target(3, "other")];
        assert!(!live
            .iter()
            .any(|tab| original.matches(&tab.project_id, &tab.worktree_path)));
    }
}
