use spacescan::constants;
use spacescan::runner::worker_threads_for;

#[test]
fn explicit_worker_threads_are_preserved() {
    assert_eq!(worker_threads_for(12), Some(12));
}

#[test]
fn auto_worker_threads_use_rayon_default() {
    assert_eq!(worker_threads_for(constants::cli::DEFAULT_THREADS), None);
}
