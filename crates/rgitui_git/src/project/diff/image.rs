//! Snapshot loading with explicit provenance: only index→workdir diffs may
//! read the filesystem. Historical and staged sides always use delta blob IDs.
use anyhow::Result;
use cap_fs_ext::{DirExt as _, FollowSymlinks, OpenOptionsFollowExt as _};
use git2::{Delta, DiffDelta, DiffFile, FileMode, Repository};
use rgitui_i18n::msg;
use std::io::Read as _;
use std::path::{Component, Path};
use std::sync::Arc;

use crate::project::messages::{error, error_message};
use crate::types::ImageDiff;

pub(super) const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;
const MAX_DIFF_IMAGE_BYTES: usize = 64 * 1024 * 1024;

/// Charges encoded bytes before allocation. Once a side exceeds the remaining
/// budget, all subsequent previews are unavailable. Failed attempts may consume
/// budget too, so a diff cannot repeatedly allocate and discard large sides.
pub(super) struct Budget {
    remaining: usize,
    exhausted: bool,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            remaining: MAX_DIFF_IMAGE_BYTES,
            exhausted: false,
        }
    }
}

impl Budget {
    fn reserve(&mut self, size: usize) -> Result<()> {
        if self.exhausted || size > self.remaining {
            self.exhausted = true;
            return Err(error(msg!("git.preview.budget_exhausted")));
        }
        self.remaining -= size;
        self.exhausted = self.remaining == 0;
        Ok(())
    }
}

fn is_png(path: Option<&Path>) -> bool {
    path.and_then(Path::extension)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}

fn check_size(size: u64) -> Result<()> {
    anyhow::ensure!(
        size <= MAX_IMAGE_BYTES as u64,
        error(msg!("git.preview.too_large"))
    );
    Ok(())
}

fn check_mode(file: &DiffFile<'_>) -> Result<()> {
    anyhow::ensure!(
        matches!(
            file.mode(),
            FileMode::Blob | FileMode::BlobExecutable | FileMode::BlobGroupWritable
        ),
        error(msg!("git.preview.not_regular"))
    );
    Ok(())
}

fn blob_bytes(repo: &Repository, file: DiffFile<'_>, budget: &mut Budget) -> Result<Arc<[u8]>> {
    check_mode(&file)?;
    // Check the object header and reserve budget before materializing the blob.
    let (size, _) = repo.odb()?.read_header(file.id())?;
    check_size(size as u64)?;
    budget.reserve(size)?;
    let blob = repo.find_blob(file.id())?;
    check_size(blob.size() as u64)?;
    if blob.size() > size {
        budget.reserve(blob.size() - size)?;
    }
    Ok(Arc::from(blob.content()))
}

fn workdir_bytes(repo: &Repository, file: DiffFile<'_>, budget: &mut Budget) -> Result<Arc<[u8]>> {
    check_mode(&file)?;
    let path = file
        .path()
        .ok_or_else(|| error(msg!("git.preview.missing_path")))?;
    let workdir = repo
        .workdir()
        .ok_or_else(|| error(msg!("git.preview.no_worktree")))?;
    let mut directory = cap_std::fs::Dir::open_ambient_dir(workdir, cap_std::ambient_authority())?;
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(error(msg!("git.preview.invalid_path")));
        };
        if components.peek().is_some() {
            // Handle-relative no-follow traversal prevents parent symlink swaps
            // from redirecting the read outside the working tree.
            directory = directory.open_dir_nofollow(name)?;
            continue;
        }
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let input = directory.open_with(name, &options)?;
        let metadata = input.metadata()?;
        anyhow::ensure!(metadata.is_file(), error(msg!("git.preview.not_regular")));
        check_size(metadata.len())?;
        // Single-file workdir diffs get their own budget, but bound growth too.
        let read_limit = MAX_IMAGE_BYTES.min(budget.remaining);
        budget.reserve(metadata.len() as usize)?;
        let mut bytes = Vec::new();
        input.take(read_limit as u64 + 1).read_to_end(&mut bytes)?;
        check_size(bytes.len() as u64)?;
        if bytes.len() > metadata.len() as usize {
            budget.reserve(bytes.len() - metadata.len() as usize)?;
        }
        return Ok(Arc::from(bytes));
    }
    Err(error(msg!("git.preview.invalid_path")))
}

fn load_sides(
    repo: &Repository,
    delta: &DiffDelta<'_>,
    after_is_workdir: bool,
    budget: &mut Budget,
) -> Result<ImageDiff> {
    if budget.exhausted {
        return Err(error(msg!("git.preview.budget_exhausted")));
    }
    let before = if matches!(delta.status(), Delta::Added | Delta::Untracked) {
        None
    } else {
        Some(blob_bytes(repo, delta.old_file(), budget)?)
    };
    let after = if delta.status() == Delta::Deleted {
        None
    } else if after_is_workdir {
        Some(workdir_bytes(repo, delta.new_file(), budget)?)
    } else {
        Some(blob_bytes(repo, delta.new_file(), budget)?)
    };
    Ok(ImageDiff {
        before,
        after,
        error: None,
    })
}

/// Preview failures belong to this file, never to the enclosing commit/stash.
pub(super) fn load(
    repo: &Repository,
    delta: &DiffDelta<'_>,
    after_is_workdir: bool,
    budget: &mut Budget,
) -> Option<ImageDiff> {
    if !is_png(delta.old_file().path()) && !is_png(delta.new_file().path()) {
        return None;
    }
    Some(
        load_sides(repo, delta, after_is_workdir, budget).unwrap_or_else(|failure| ImageDiff {
            before: None,
            after: None,
            error: Some(
                msg!(
                    "git.preview.unavailable",
                    reason = error_message(&failure).translated()
                )
                .translated()
                .to_string(),
            ),
        }),
    )
}
