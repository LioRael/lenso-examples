use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

static NEXT_TEST_ID: AtomicUsize = AtomicUsize::new(0);

fn test_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "lenso-stateful-storage-{}-{label}-{}",
        std::process::id(),
        NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn independent_adapters_serialize_one_owned_transaction_boundary() {
    let root = test_path("concurrency");
    let path = root.join("counter.json");
    let adapter = FileStateAdapter::new(&path);
    adapter.setup().expect("setup should succeed");
    let threads = (0..4)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || {
                let adapter = FileStateAdapter::new(path);
                for _ in 0..50 {
                    adapter
                        .increment_counter("shared", 1)
                        .expect("increment should be serialized");
                }
            })
        })
        .collect::<Vec<_>>();
    for thread in threads {
        thread.join().expect("worker should complete");
    }
    assert_eq!(
        adapter.read_counter("shared").expect("read should succeed"),
        Some((200, 200))
    );
    std::fs::remove_dir_all(root).expect("test storage should be removed");
}

#[test]
fn explicit_recovery_restores_a_synced_temporary_document() {
    let root = test_path("recovery");
    let path = root.join("counter.json");
    let adapter = FileStateAdapter::new(&path);
    adapter.setup().expect("setup should succeed");
    adapter
        .increment_counter("saved", 7)
        .expect("state should be written");
    let bytes = std::fs::read(&path).expect("document should be readable");
    std::fs::write(adapter.temporary_path(), bytes).expect("temporary state should be written");
    std::fs::remove_file(&path).expect("rename interruption should be simulated");
    assert!(matches!(
        adapter.verify_ready(),
        Err(StateStorageError::RecoveryRequired { .. })
    ));
    assert_eq!(
        adapter.recover().expect("recovery should succeed"),
        RecoveryOutcome::Restored { schema_version: 1 }
    );
    assert_eq!(
        adapter.read_counter("saved").expect("state should recover"),
        Some((7, 1))
    );
    std::fs::remove_dir_all(root).expect("test storage should be removed");
}
