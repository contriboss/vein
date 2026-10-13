use super::*;
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn test_new_creates_storage() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());
    assert_eq!(storage.root, temp_dir.path());
}

#[tokio::test]
async fn test_prepare_creates_root_directory() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage_path = temp_dir.path().join("storage_root");
    let storage = FilesystemStorage::new(storage_path.clone());

    assert!(!storage_path.exists());
    storage.prepare().await.unwrap();
    assert!(storage_path.exists());
    assert!(storage_path.is_dir());
}

#[tokio::test]
async fn test_prepare_succeeds_if_directory_exists() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());

    storage.prepare().await.unwrap();
    storage.prepare().await.unwrap();
}

#[tokio::test]
async fn test_resolve_joins_paths() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());

    let resolved = storage.resolve("gems/rack-3.0.0.gem");
    assert_eq!(resolved, temp_dir.path().join("gems/rack-3.0.0.gem"));
}

#[tokio::test]
async fn test_resolve_handles_nested_paths() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());

    let resolved = storage.resolve("a/b/c/file.gem");
    assert_eq!(resolved, temp_dir.path().join("a/b/c/file.gem"));
}

#[tokio::test]
async fn test_open_read_returns_none_for_missing_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());

    let result = storage.open_read("missing.gem").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_open_read_returns_handle_for_existing_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());
    storage.prepare().await.unwrap();

    let path = storage.resolve("test.gem");
    fs::write(&path, b"content").await.unwrap();

    let handle = storage.open_read("test.gem").await.unwrap();
    assert!(handle.is_some());

    let mut handle = handle.unwrap();
    let mut buf = Vec::new();
    handle.file.read_to_end(&mut buf).await.unwrap();
    assert_eq!(buf, b"content");
}

#[tokio::test]
async fn test_create_temp_writer_creates_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());
    storage.prepare().await.unwrap();

    let mut temp_file = storage.create_temp_writer("test/file.gem").await.unwrap();
    temp_file.file_mut().write_all(b"data").await.unwrap();
    temp_file.commit().await.unwrap();

    let final_path = storage.resolve("test/file.gem");
    let data = fs::read(final_path).await.unwrap();
    assert_eq!(data, b"data");
}

#[tokio::test]
async fn test_commit_moves_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());
    storage.prepare().await.unwrap();

    let mut temp_file = storage.create_temp_writer("test/file.gem").await.unwrap();
    temp_file.file_mut().write_all(b"data").await.unwrap();
    let tmp_path = temp_file.tmp_path.clone();
    let final_path = temp_file.final_path.clone();
    temp_file.commit().await.unwrap();

    assert!(!tmp_path.exists());
    assert!(final_path.exists());
}

#[tokio::test]
async fn test_rollback_removes_temp_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let storage = FilesystemStorage::new(temp_dir.path().to_path_buf());
    storage.prepare().await.unwrap();

    let temp_file = storage.create_temp_writer("test/file.gem").await.unwrap();
    let tmp_path = temp_file.tmp_path.clone();
    temp_file.rollback().await.unwrap();

    assert!(!tmp_path.exists());
    assert!(!storage.resolve("test/file.gem").exists());
}

#[tokio::test]
async fn test_temp_path_generation() {
    let final_path = PathBuf::from("foo/bar.gem");
    let tmp_path = temp_path_for(&final_path);
    assert!(
        tmp_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("bar.gem.tmp-")
    );
}

#[test]
fn test_should_retry_logic() {
    // Should retry these errors
    let would_block = std::io::Error::from(ErrorKind::WouldBlock);
    assert!(should_retry(&would_block));

    let interrupted = std::io::Error::from(ErrorKind::Interrupted);
    assert!(should_retry(&interrupted));

    // Should NOT retry these errors
    let not_found = std::io::Error::from(ErrorKind::NotFound);
    assert!(!should_retry(&not_found));

    let permission_denied = std::io::Error::from(ErrorKind::PermissionDenied);
    assert!(!should_retry(&permission_denied));
}
