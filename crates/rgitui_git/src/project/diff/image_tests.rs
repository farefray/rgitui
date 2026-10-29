use super::*;
use rgitui_test_support::TempRepo;

// Valid 1×1 grayscale PNGs with different pixel values.
const BLACK: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x00\x00\x00\x00\x3a\x7e\x9b\x55\x00\x00\x00\x0aIDAT\x78\x9c\x63\x60\x00\x00\x00\x02\x00\x01\x48\xaf\xa4\x71\x00\x00\x00\x00IEND\xae\x42\x60\x82";
const GRAY: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x00\x00\x00\x00\x3a\x7e\x9b\x55\x00\x00\x00\x0aIDAT\x78\x9c\x63\xa8\x07\x00\x00\x81\x00\x80\xd3\x94\x53\x4a\x00\x00\x00\x00IEND\xae\x42\x60\x82";
const WHITE: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x00\x00\x00\x00\x3a\x7e\x9b\x55\x00\x00\x00\x0aIDAT\x78\x9c\x63\xf8\x0f\x00\x01\x01\x01\x00\xb1\x38\xf6\x14\x00\x00\x00\x00IEND\xae\x42\x60\x82";

fn commit_png(fixture: &TempRepo, path: &str, bytes: &[u8]) -> git2::Oid {
    fixture.write_file_bytes(path, bytes);
    fixture.stage(path);
    fixture.commit("PNG snapshot")
}

fn assert_image(file: &FileDiff, before: Option<&[u8]>, after: Option<&[u8]>) {
    let image = file.image.as_ref().expect("PNG snapshot");
    assert_eq!(image.error, None);
    assert_eq!(image.before.as_deref(), before);
    assert_eq!(image.after.as_deref(), after);
    assert!(file.hunks.is_empty());
    assert_eq!((file.additions, file.deletions), (0, 0));
}

#[test]
fn png_untracked_and_staged_addition_on_unborn_head() {
    let fixture = TempRepo::init();
    let path = "nested/image[1].PnG";
    fixture.write_file_bytes(path, BLACK);
    let diff = compute_file_diff(fixture.path(), Path::new(path), false).unwrap();
    assert_eq!(diff.path, Path::new(path));
    assert_eq!(diff.kind, FileChangeKind::Added);
    assert_image(&diff, None, Some(BLACK));

    fixture.stage(path);
    fixture.write_file_bytes(path, WHITE);
    let diff = compute_file_diff(fixture.path(), Path::new(path), true).unwrap();
    assert_eq!(diff.kind, FileChangeKind::Added);
    assert_image(&diff, None, Some(BLACK));
}

#[test]
fn png_staged_and_unstaged_use_distinct_snapshots() {
    let fixture = TempRepo::init();
    commit_png(&fixture, "image.png", BLACK);
    fixture.write_file_bytes("image.png", GRAY);
    fixture.stage("image.png");
    fixture.write_file_bytes("image.png", WHITE);

    let staged = compute_file_diff(fixture.path(), Path::new("image.png"), true).unwrap();
    assert_image(&staged, Some(BLACK), Some(GRAY));
    let unstaged = compute_file_diff(fixture.path(), Path::new("image.png"), false).unwrap();
    assert_eq!(unstaged.kind, FileChangeKind::Modified);
    assert_image(&unstaged, Some(GRAY), Some(WHITE));
}

#[test]
fn png_deleted_workdir_and_index_sides() {
    let fixture = TempRepo::init();
    commit_png(&fixture, "image.png", BLACK);
    fixture.write_file_bytes("image.png", GRAY);
    fixture.stage("image.png");
    std::fs::remove_file(fixture.path().join("image.png")).unwrap();
    let unstaged = compute_file_diff(fixture.path(), Path::new("image.png"), false).unwrap();
    assert_eq!(unstaged.kind, FileChangeKind::Deleted);
    assert_image(&unstaged, Some(GRAY), None);

    let mut index = fixture.repo().index().unwrap();
    index.remove_path(Path::new("image.png")).unwrap();
    index.write().unwrap();
    // A recreated workdir file must not become the staged after-side.
    fixture.write_file_bytes("image.png", WHITE);
    let staged = compute_file_diff(fixture.path(), Path::new("image.png"), true).unwrap();
    assert_eq!(staged.kind, FileChangeKind::Deleted);
    assert_image(&staged, Some(BLACK), None);
}

#[test]
fn png_historical_initial_modified_deleted_and_comparison_ignore_checkout() {
    let fixture = TempRepo::init();
    let initial = commit_png(&fixture, "image.PNG", BLACK);
    let modified = commit_png(&fixture, "image.PNG", GRAY);
    let mut index = fixture.repo().index().unwrap();
    index.remove_path(Path::new("image.PNG")).unwrap();
    index.write().unwrap();
    let deleted = fixture.commit("delete PNG");
    fixture.write_file_bytes("image.PNG", WHITE);
    fixture.stage("image.PNG");

    for (oid, before, after, kind) in [
        (initial, None, Some(BLACK), FileChangeKind::Added),
        (modified, Some(BLACK), Some(GRAY), FileChangeKind::Modified),
        (deleted, Some(GRAY), None, FileChangeKind::Deleted),
    ] {
        let diff = compute_commit_diff(fixture.path(), oid).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].kind, kind);
        assert_image(&diff.files[0], before, after);
    }

    // An arbitrary, reversed tree comparison must use the actual endpoints.
    let repo = fixture.repo();
    let from = repo.find_commit(modified).unwrap().tree().unwrap();
    let to = repo.find_commit(initial).unwrap().tree().unwrap();
    let diff = repo
        .diff_tree_to_tree(Some(&from), Some(&to), None)
        .unwrap();
    let parsed = parse_multi_file_diff(repo, &diff).unwrap();
    assert_image(&parsed.files[0], Some(GRAY), Some(BLACK));

    // Historical snapshots also work with no working tree at all.
    let bare_dir = tempfile::tempdir().unwrap();
    let bare = git2::build::RepoBuilder::new()
        .bare(true)
        .clone(fixture.path().to_str().unwrap(), bare_dir.path())
        .unwrap();
    let diff = compute_commit_diff(bare.path(), modified).unwrap();
    assert_image(&diff.files[0], Some(BLACK), Some(GRAY));
}

#[test]
fn png_stash_uses_stash_tree_not_current_index_or_workdir() {
    let fixture = TempRepo::init();
    commit_png(&fixture, "image.png", BLACK);
    fixture.write_file_bytes("image.png", GRAY);
    let mut repo = Repository::open(fixture.path()).unwrap();
    repo.stash_save(&fixture.signature(), "PNG stash", None)
        .unwrap();
    fixture.write_file_bytes("image.png", WHITE);
    fixture.stage("image.png");
    let diff = compute_stash_diff(fixture.path(), 0).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert_image(&diff.files[0], Some(BLACK), Some(GRAY));
}

#[test]
fn multi_diff_preserves_non_png_binary_without_text_hunks() {
    let fixture = TempRepo::init();
    fixture.write_file_bytes("data.bin", b"\0binary");
    fixture.stage("data.bin");
    let oid = fixture.commit("binary addition");
    let tree = fixture.repo().find_commit(oid).unwrap().tree().unwrap();
    let diff = fixture
        .repo()
        .diff_tree_to_tree(None, Some(&tree), None)
        .unwrap();
    // libgit2 versions may return either None or a patch with zero hunks.
    if let Some(patch) = git2::Patch::from_diff(&diff, 0).unwrap() {
        assert_eq!(patch.num_hunks(), 0);
    }
    let parsed = parse_multi_file_diff(fixture.repo(), &diff).unwrap();
    assert_eq!(parsed.files.len(), 1);
    assert_eq!(parsed.files[0].path, Path::new("data.bin"));
    assert_eq!(parsed.files[0].kind, FileChangeKind::Added);
    assert!(parsed.files[0].image.is_none());
    assert!(parsed.files[0].hunks.is_empty());
    assert_eq!((parsed.total_additions, parsed.total_deletions), (0, 0));
}

#[test]
fn png_oversized_preview_does_not_hide_text_in_commit_or_stash() {
    let fixture = TempRepo::init();
    let path = fixture.path().join("large.png");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(image::MAX_IMAGE_BYTES as u64 + 1).unwrap();
    drop(file);
    let diff = compute_file_diff(fixture.path(), Path::new("large.png"), false).unwrap();
    assert_preview_error(diff.image.as_ref().unwrap(), "20 MiB");
    fixture.stage("large.png");
    let diff = compute_file_diff(fixture.path(), Path::new("large.png"), true).unwrap();
    assert_preview_error(diff.image.as_ref().unwrap(), "20 MiB");
    fixture.write_file("text.txt", "before\n");
    fixture.stage("text.txt");
    let oid = fixture.commit("oversized PNG and text");
    let diff = compute_commit_diff(fixture.path(), oid).unwrap();
    assert_mixed_preview_error(&diff, "20 MiB");

    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_len(image::MAX_IMAGE_BYTES as u64 + 2).unwrap();
    drop(file);
    fixture.write_file("text.txt", "after\n");
    let mut repo = Repository::open(fixture.path()).unwrap();
    repo.stash_save(&fixture.signature(), "oversized preview", None)
        .unwrap();
    assert_mixed_preview_error(&compute_stash_diff(fixture.path(), 0).unwrap(), "20 MiB");
}

fn assert_preview_error(image: &ImageDiff, expected: &str) {
    assert!(image.before.is_none());
    assert!(image.after.is_none());
    let error = image.error.as_deref().expect("preview error");
    assert!(error.contains(expected), "{error}");
}

fn assert_mixed_preview_error(diff: &CommitDiff, expected: &str) {
    assert_eq!(diff.files.len(), 2);
    let image = diff
        .files
        .iter()
        .find_map(|file| file.image.as_ref())
        .unwrap();
    assert_preview_error(image, expected);
    let text = diff
        .files
        .iter()
        .find(|file| file.path == Path::new("text.txt"))
        .unwrap();
    assert!(!text.hunks.is_empty());
    assert!(text.additions > 0);
    assert_eq!(diff.total_additions, text.additions);
}

#[test]
fn png_non_png_text_diff_has_no_image() {
    let fixture = TempRepo::init();
    let oid = fixture.commit_file("image.png.txt", "plain text\n", "text");
    let diff = compute_commit_diff(fixture.path(), oid).unwrap();
    assert!(diff.files[0].image.is_none());
    assert_eq!(diff.total_additions, 1);
}

#[test]
#[cfg(any(unix, windows))]
fn png_symlink_workdir_is_not_followed() {
    let fixture = TempRepo::init();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("private.png");
    std::fs::write(&target, WHITE).unwrap();
    let link = fixture.path().join("link.png");
    fixture.write_file_bytes("link.png", BLACK);
    let mut options = DiffOptions::new();
    options.include_untracked(true).show_untracked_content(true);
    let diff = fixture
        .repo()
        .diff_index_to_workdir(None, Some(&mut options))
        .unwrap();
    std::fs::remove_file(&link).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_file(&target, &link) {
        if error.kind() == std::io::ErrorKind::PermissionDenied
            || error.raw_os_error() == Some(1314)
        {
            return;
        }
        panic!("{error}");
    }
    // The delta still describes a regular file: opening must independently
    // reject a link swapped in after the diff was computed.
    let preview = image::load(
        fixture.repo(),
        &diff.get_delta(0).unwrap(),
        true,
        &mut image::Budget::default(),
    )
    .unwrap();
    assert_preview_error(&preview, "Image preview unavailable");
    let file = compute_file_diff(fixture.path(), Path::new("link.png"), false).unwrap();
    assert_preview_error(file.image.as_ref().unwrap(), "Image preview unavailable");
    assert_eq!(std::fs::read(target).unwrap(), WHITE);
}

#[test]
#[cfg(any(unix, windows))]
fn png_symlink_parent_swapped_after_diff_is_not_followed() {
    let fixture = TempRepo::init();
    fixture.write_file_bytes("nested/image.png", BLACK);
    let mut options = DiffOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .show_untracked_content(true);
    let diff = fixture
        .repo()
        .diff_index_to_workdir(None, Some(&mut options))
        .unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("image.png"), WHITE).unwrap();
    let parent = fixture.path().join("nested");
    std::fs::remove_file(parent.join("image.png")).unwrap();
    std::fs::remove_dir(&parent).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path(), &parent).unwrap();
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_dir(outside.path(), &parent) {
        if error.kind() == std::io::ErrorKind::PermissionDenied
            || error.raw_os_error() == Some(1314)
        {
            return;
        }
        panic!("{error}");
    }
    let preview = image::load(
        fixture.repo(),
        &diff.get_delta(0).unwrap(),
        true,
        &mut image::Budget::default(),
    )
    .unwrap();
    assert_preview_error(&preview, "Image preview unavailable");
}

#[test]
fn png_symlink_blob_does_not_hide_unrelated_text() {
    let fixture = TempRepo::init();
    let repo = fixture.repo();
    let mut tree = repo.treebuilder(None).unwrap();
    tree.insert("link.png", repo.blob(b"outside.png").unwrap(), 0o120000)
        .unwrap();
    tree.insert("text.txt", repo.blob(b"hello\n").unwrap(), 0o100644)
        .unwrap();
    let oid = fixture.commit_tree(Some("HEAD"), "symlink and text", tree.write().unwrap(), &[]);
    assert_mixed_preview_error(
        &compute_commit_diff(fixture.path(), oid).unwrap(),
        "regular files",
    );
}

#[test]
fn png_commit_budget_counts_both_sides_and_stops_later_previews() {
    let fixture = TempRepo::init();
    let repo = fixture.repo();
    let side_size = 17 * 1024 * 1024;
    let before = repo.blob(&vec![0; side_size]).unwrap();
    let after = repo.blob(&vec![1; side_size]).unwrap();
    let mut tree = repo.treebuilder(None).unwrap();
    for path in ["a.png", "b.png"] {
        tree.insert(path, before, 0o100644).unwrap();
    }
    let initial = fixture.commit_tree(Some("HEAD"), "before", tree.write().unwrap(), &[]);
    for path in ["a.png", "b.png"] {
        tree.insert(path, after, 0o100644).unwrap();
    }
    tree.insert("z.png", repo.blob(BLACK).unwrap(), 0o100644)
        .unwrap();
    tree.insert("text.txt", repo.blob(b"still visible\n").unwrap(), 0o100644)
        .unwrap();
    let oid = fixture.commit_tree(Some("HEAD"), "after", tree.write().unwrap(), &[initial]);
    let diff = compute_commit_diff(fixture.path(), oid).unwrap();
    assert_eq!(diff.files.len(), 4);
    let first = diff.files[0].image.as_ref().unwrap();
    assert_eq!(first.error, None);
    assert_eq!(first.before.as_ref().unwrap().len(), side_size);
    assert_eq!(first.after.as_ref().unwrap().len(), side_size);
    // b.png's before-side fits, but its after-side exceeds the remaining
    // budget. Discard both sides, then reject even the small z.png preview.
    for path in ["b.png", "z.png"] {
        let file = diff
            .files
            .iter()
            .find(|file| file.path == Path::new(path))
            .unwrap();
        assert_preview_error(file.image.as_ref().unwrap(), "64 MiB");
    }
    let retained: usize = diff
        .files
        .iter()
        .filter_map(|file| file.image.as_ref())
        .flat_map(|image| [&image.before, &image.after])
        .flatten()
        .map(|bytes| bytes.len())
        .sum();
    assert_eq!(retained, 2 * side_size);
    assert_eq!(diff.total_additions, 1);
    let text = diff
        .files
        .iter()
        .find(|file| file.path == Path::new("text.txt"))
        .unwrap();
    assert!(!text.hunks.is_empty());
    // Budget belongs to one result, not the repository or process.
    let again = compute_commit_diff(fixture.path(), oid).unwrap();
    assert!(again.files[0].image.as_ref().unwrap().error.is_none());
}
