use super::*;

#[test]
fn prove_writes_one_dag_for_the_complete_trajectory() {
    let initial = TempFile::new(
        "proof_initial.sparse",
        "0 1 1.0\n1 2 1.0\n2 3 1.0\n0 3 1.0\n3 4 1.0\n4 5 1.0\n5 6 1.0\n3 6 1.0\n",
    );
    let update = TempFile::new(
        "proof_update.sparse",
        "0 1 1.1\n1 2 1.0\n2 3 1.0\n0 3 1.0\n3 4 1.0\n4 5 1.0\n5 6 1.0\n3 6 1.0\n",
    );
    let output = TempFile::new("trajectory.hpf", "");
    let out = run(&[
        "prove",
        initial.path().to_str().unwrap(),
        output.path().to_str().unwrap(),
        update.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--modulus",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("wrote 2 snapshots"),
        "{}",
        stdout(&out)
    );
    let bytes = std::fs::read(output.path()).unwrap();
    assert!(bytes.starts_with(b"HOLOSPF\0"));
}

#[test]
fn index_writes_an_initial_checkpoint_and_warm_record() {
    let initial = TempFile::new(
        "index_initial.sparse",
        "0 1 0.25\n0 2 1.0\n1 2 1.5\n0 3 1.2\n1 3 1.7\n0 4 1.1\n1 4 1.6\n0 5 1.3\n1 5 1.8\n",
    );
    let update = TempFile::new(
        "index_update.sparse",
        "0 1 0.25\n0 2 1.01\n1 2 1.5\n0 3 1.2\n1 3 1.7\n0 4 1.1\n1 4 1.6\n0 5 1.3\n1 5 1.8\n",
    );
    let snapshot = TempFile::new("index.hip", "");
    let delta = TempFile::new("index.hdp", "");
    let out = run(&[
        "index",
        initial.path().to_str().unwrap(),
        snapshot.path().to_str().unwrap(),
        "--update",
        update.path().to_str().unwrap(),
        "--record",
        delta.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--modulus",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("one initial checkpoint"),
        "{}",
        stdout(&out)
    );
    assert!(
        std::fs::read(snapshot.path())
            .unwrap()
            .starts_with(b"HOLOSIP\0")
    );
    assert!(
        std::fs::read(delta.path())
            .unwrap()
            .starts_with(b"HOLOSDP\0")
    );
}

#[test]
fn index_cli_writes_an_independently_checked_h2_checkpoint() {
    let graph = TempFile::new(
        "index_h2.sparse",
        "0 2 1.02\n0 3 1.03\n0 4 1.04\n0 5 1.05\n1 2 1.03\n1 3 1.04\n1 4 1.05\n1 5 1.06\n2 4 1.06\n2 5 1.07\n3 4 1.07\n3 5 1.08\n",
    );
    let snapshot = TempFile::new("index_h2.hip", "");
    let out = run(&[
        "index",
        graph.path().to_str().unwrap(),
        snapshot.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--dim",
        "2",
        "--modulus",
        "5",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    assert!(stdout(&out).contains("through dimension 2"));
    let bytes = std::fs::read(snapshot.path()).unwrap();
    let (_, checked) = IndexProofState::verify_snapshot(&bytes, ProofLimits::default()).unwrap();
    assert!(checked.triangle_columns_checked > 0);
}

#[test]
fn interface_cli_writes_an_independently_checked_relative_core() {
    let graph = TempFile::new(
        "relative.sparse",
        "0 1 1.0\n1 2 1.0\n2 3 1.0\n0 3 1.0\n0 4 2.0\n1 4 2.0\n2 5 2.5\n3 5 2.5\n",
    );
    let artifact = TempFile::new("relative.hri", "");
    let out = run(&[
        "interface",
        graph.path().to_str().unwrap(),
        artifact.path().to_str().unwrap(),
        "--format",
        "sparse",
        "--dim",
        "2",
        "--modulus",
        "5",
        "--protect",
        "0",
        "--protect",
        "1",
        "--protect",
        "2",
        "--protect",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let bytes = std::fs::read(artifact.path()).unwrap();
    assert!(is_relative_interface(&bytes));
    let checked = verify_relative_interface(&bytes, ProofLimits::default()).unwrap();
    assert_eq!(checked.max_dim, 2);
    assert!(checked.input_cells >= checked.core_cells);
}

#[test]
fn merge_interfaces_cli_commits_an_independently_checked_manifest() {
    let child = |labels: &[usize], side: usize| {
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
            &RipsParams::new(2).with_modulus(3),
            &[0, 1, 2, 3],
            CertificateLimits::default(),
        )
        .unwrap()
        .encode(CertificateLimits::default())
        .unwrap()
    };
    let first = TempFile::new_bytes("first.hri", &child(&[0, 1, 2, 3, 4], 0));
    let second = TempFile::new_bytes("second.hri", &child(&[0, 1, 2, 3, 5], 1));
    let manifest_file = TempFile::new("distributed.hdm", "");
    let result_file = TempFile::new("distributed.hri", "");
    let store_path = std::env::temp_dir().join(format!(
        "holos_cli_test_{}_distributed_store",
        std::process::id()
    ));
    if store_path.exists() {
        std::fs::remove_dir_all(&store_path).unwrap();
    }
    let out = run(&[
        "merge-interfaces",
        store_path.to_str().unwrap(),
        manifest_file.path().to_str().unwrap(),
        result_file.path().to_str().unwrap(),
        first.path().to_str().unwrap(),
        second.path().to_str().unwrap(),
        "--separator",
        "0",
        "--separator",
        "1",
        "--separator",
        "2",
        "--separator",
        "3",
    ]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let manifest_bytes = std::fs::read(manifest_file.path()).unwrap();
    let manifest = DistributedInterfaceManifest::decode(&manifest_bytes, 1 << 30).unwrap();
    let store = DurableInterfaceStore::open(&store_path).unwrap();
    let mut ids: std::collections::BTreeSet<_> = manifest.shards().iter().copied().collect();
    ids.extend(manifest.folds().iter().copied());
    ids.insert(manifest.result());
    let objects: Vec<_> = ids
        .into_iter()
        .map(|id| store.get(id, 1 << 30).unwrap())
        .collect();
    let checked =
        verify_distributed_interface(&manifest_bytes, &objects, ProofLimits::default()).unwrap();
    assert_eq!(checked.shards, 2);
    assert_eq!(
        std::fs::read(result_file.path()).unwrap(),
        store.get(manifest.result(), 1 << 30).unwrap()
    );
    std::fs::remove_dir_all(&store_path).unwrap();
}
