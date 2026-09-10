use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use crate::{
    CertificateLimits, RelativeInterfaceCertificate, RipsParams, SparseDistanceMatrix,
    rips_persistence_sparse,
};
use holos_tda_check::{ProofLimits, verify_distributed_interface};

use super::DurableInterfaceStore;
use super::compose::job_id;
use super::model::Progress;

struct TestStore(PathBuf);

impl TestStore {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("holos_distributed_{}_{}", std::process::id(), name));
        if path.exists() {
            fs::remove_dir_all(&path).unwrap();
        }
        Self(path)
    }
}

impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn shard(labels: &[usize], side: usize, modulus: u32) -> RelativeInterfaceCertificate {
    let graph = SparseDistanceMatrix::from_triplets(
        5,
        &[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 3, 1.0),
            (0, 3, 1.0),
            (side, 4, 2.0),
            ((side + 1) % 4, 4, 2.0),
        ],
    )
    .unwrap();
    RelativeInterfaceCertificate::build_labeled(
        &graph,
        labels,
        &RipsParams::new(2).with_modulus(modulus),
        &[0, 1, 2, 3],
        CertificateLimits::default(),
    )
    .unwrap()
}

#[test]
fn commits_recovers_and_independently_replays_a_streaming_fold() {
    let temporary = TestStore::new("commit");
    let store = DurableInterfaceStore::open(&temporary.0).unwrap();
    let shards = [
        shard(&[0, 1, 2, 3, 4], 0, 5),
        shard(&[0, 1, 2, 3, 5], 1, 5),
        shard(&[0, 1, 2, 3, 6], 2, 5),
    ];
    let artifacts: Vec<_> = shards
        .iter()
        .map(|shard| shard.encode(CertificateLimits::default()).unwrap())
        .collect();
    let commit = store
        .commit(&artifacts, &[0, 1, 2, 3], &[], CertificateLimits::default())
        .unwrap();
    assert_eq!(commit.manifest().shards().len(), 3);
    assert_eq!(commit.manifest().folds().len(), 3);
    assert_eq!(commit.work().folds_computed, 2);
    let verified = store
        .verify_manifest(commit.manifest(), CertificateLimits::default())
        .unwrap();
    assert_eq!(verified.diagram().bars, commit.certificate().diagram().bars);
    let mut ids: BTreeSet<_> = commit.manifest().shards().iter().copied().collect();
    ids.extend(commit.manifest().folds().iter().copied());
    ids.insert(commit.manifest().result());
    let objects: Vec<_> = ids
        .into_iter()
        .map(|id| {
            store
                .get(id, CertificateLimits::default().max_bytes)
                .unwrap()
        })
        .collect();
    let independent = verify_distributed_interface(
        &commit.manifest().encode().unwrap(),
        &objects,
        ProofLimits::default(),
    )
    .unwrap();
    assert_eq!(independent.result, *commit.manifest().result().as_bytes());
    assert_eq!(independent.shards, 3);

    fs::remove_file(store.manifest_path(commit.manifest().job())).unwrap();
    let recovered = store
        .commit(&artifacts, &[0, 1, 2, 3], &[], CertificateLimits::default())
        .unwrap();
    assert_eq!(recovered.manifest(), commit.manifest());
    assert_eq!(recovered.work().folds_reused, 3);
    assert_eq!(recovered.work().folds_computed, 0);

    let mut full_edges = vec![(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)];
    full_edges.extend([(0, 4, 2.0), (1, 4, 2.0)]);
    full_edges.extend([(1, 5, 2.0), (2, 5, 2.0)]);
    full_edges.extend([(2, 6, 2.0), (3, 6, 2.0)]);
    let full = SparseDistanceMatrix::from_triplets(7, &full_edges).unwrap();
    assert_eq!(
        recovered.certificate().diagram().bars,
        rips_persistence_sparse(&full, &RipsParams::new(2).with_modulus(5))
            .unwrap()
            .bars
    );
}

#[test]
fn resumes_an_arbitrary_durable_prefix() {
    let temporary = TestStore::new("prefix");
    let store = DurableInterfaceStore::open(&temporary.0).unwrap();
    let certificates = [
        shard(&[0, 1, 2, 3, 4], 0, 5),
        shard(&[0, 1, 2, 3, 5], 1, 5),
        shard(&[0, 1, 2, 3, 6], 2, 5),
    ];
    let artifacts: Vec<_> = certificates
        .iter()
        .map(|item| item.encode(CertificateLimits::default()).unwrap())
        .collect();
    let ids: Vec<_> = artifacts
        .iter()
        .map(|bytes| store.put(bytes).unwrap().0)
        .collect();
    let prefix = RelativeInterfaceCertificate::compose(
        &[&certificates[0], &certificates[1]],
        &[0, 1, 2, 3],
        CertificateLimits::default(),
    )
    .unwrap();
    let prefix_bytes = prefix.encode(CertificateLimits::default()).unwrap();
    let prefix_id = store.put(&prefix_bytes).unwrap().0;
    let job = job_id(2, 5, &[0, 1, 2, 3], &[], &ids);
    store
        .write_progress(
            job,
            &Progress {
                prefix: 2,
                accumulator: prefix_id,
                folds: vec![ids[0], prefix_id],
            },
        )
        .unwrap();

    let commit = store
        .commit_stored(&ids, &[0, 1, 2, 3], &[], CertificateLimits::default())
        .unwrap();
    assert_eq!(commit.work().folds_reused, 2);
    assert_eq!(commit.work().folds_computed, 1);
    assert_eq!(commit.manifest().folds()[1], prefix_id);
    store
        .verify_manifest(commit.manifest(), CertificateLimits::default())
        .unwrap();
}

#[test]
fn rejects_corrupt_objects_and_incompatible_shards() {
    let temporary = TestStore::new("corrupt");
    let store = DurableInterfaceStore::open(&temporary.0).unwrap();
    let first = shard(&[0, 1, 2, 3, 4], 0, 2)
        .encode(CertificateLimits::default())
        .unwrap();
    let incompatible = shard(&[0, 1, 2, 3, 5], 1, 3)
        .encode(CertificateLimits::default())
        .unwrap();
    assert!(
        store
            .commit(
                &[first.clone(), incompatible],
                &[0, 1, 2, 3],
                &[],
                CertificateLimits::default(),
            )
            .is_err()
    );
    let (id, _) = store.put(&first).unwrap();
    fs::write(store.object_path(id), b"corrupt").unwrap();
    assert!(
        store
            .get(id, CertificateLimits::default().max_bytes)
            .is_err()
    );
}
