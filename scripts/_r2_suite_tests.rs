    // Output metrics and output digests in these fixtures are synthetic.
    // Their campaign input bytes are the frozen real R2 preregistration only.
    fn write_r2_suite(directory: &Path) {
        write_suite_for_contract(directory, false, None, &R2_CONTRACT);
    }

    fn edit_json_file(path: &Path, edit: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        edit(&mut value);
        fs::write(path, canonical_json(&value).unwrap()).unwrap();
    }

    fn refresh_r2_records(root: &Path, count: usize) {
        let directory = root.join(format!("retain-{count:02}-of-27"));
        edit_json_file(&directory.join("manifest.json"), |manifest| {
            for record in manifest["records"].as_array_mut().unwrap() {
                let path = directory.join(record["filename"].as_str().unwrap());
                record["sha256"] = json!(sha256_hex(&fs::read(path).unwrap()));
            }
        });
        let summary = verify_kv_campaign_directory(&directory).unwrap();
        fs::write(
            root.join(format!("verification-retain-{count:02}-of-27.json")),
            canonical_json(&serde_json::to_value(summary).unwrap()).unwrap(),
        ).unwrap();
    }

    #[test]
    fn r2_verifies_exact_frozen_inputs_and_is_deterministic() {
        let directory = TempDirectory::new("r2-valid");
        write_r2_suite(directory.path());
        let summary = verify_kv_campaign_suite_r2_directory(directory.path()).unwrap();
        let again = verify_kv_campaign_suite_r2_directory(directory.path()).unwrap();
        assert_eq!(serde_json::to_string(&summary).unwrap(), serde_json::to_string(&again).unwrap());
        assert_eq!(summary.schema, R2_CONTRACT.verification_schema);
        assert_eq!(summary.kvlab_suite_provider_revision, R2_CONTRACT.provider_revision);
        assert_eq!(summary.kvlab_preregistration_revision, R2_CONTRACT.preregistration_revision);
        assert_eq!(summary.nnis_runtime_revision, R2_CONTRACT.runtime_revision);
        assert_eq!(summary.prospect_launch_verifier_revision, R2_CONTRACT.launch_verifier_revision);
        assert_eq!(summary.trace_sha256, PREREGISTERED_TRACE_SHA256);
        assert_eq!(summary.baseline_logical_kv_bytes, 27 * BYTES_PER_TOKEN);
        assert_eq!(summary.campaigns.len(), 3);
        for (index, campaign) in summary.campaigns.iter().enumerate() {
            assert_eq!(campaign.retained_count, RETAIN_COUNTS[index]);
            assert_eq!(campaign.campaign.campaign_spec_sha256(), R2_CONTRACT.campaign_sha256[index]);
            assert_eq!(campaign.logical_retained_bytes, RETAIN_COUNTS[index] as u64 * BYTES_PER_TOKEN);
            assert_eq!(campaign.campaign.record_count(), 2);
        }
    }

    #[test]
    fn r1_and_r2_commands_do_not_accept_each_others_suite() {
        let r1 = TempDirectory::new("r1-disjoint");
        let r2 = TempDirectory::new("r2-disjoint");
        write_suite(r1.path(), false);
        write_r2_suite(r2.path());
        assert!(verify_kv_campaign_suite_directory(r1.path()).is_ok());
        assert!(verify_kv_campaign_suite_r2_directory(r2.path()).is_ok());
        assert!(matches!(verify_kv_campaign_suite_directory(r2.path()), Err(KvCampaignSuiteError::UnsupportedSchema)));
        assert!(matches!(verify_kv_campaign_suite_r2_directory(r1.path()), Err(KvCampaignSuiteError::UnsupportedSchema)));
    }

    #[test]
    fn r2_rejects_coherently_rehashed_input_selection_and_evaluation_substitutions() {
        for drift in ["input", "selection", "evaluation"] {
            let directory = TempDirectory::new("r2-rehashed");
            write_suite_for_contract(directory.path(), false, Some(drift), &R2_CONTRACT);
            assert!(matches!(verify_kv_campaign_suite_r2_directory(directory.path()),
                Err(KvCampaignSuiteError::ProvenanceMismatch("preregistered campaign bytes"))), "accepted {drift}");
        }
    }

    #[test]
    fn r2_rejects_each_drifted_manifest_identity() {
        for field in ["kvlab_preregistration_revision", "kvlab_execution_revision",
                      "nnis_runtime_revision", "prospect_verifier_revision", "model_id",
                      "model_revision", "source_model_sha256", "runtime_backend"] {
            let directory = TempDirectory::new("r2-identity");
            write_r2_suite(directory.path());
            edit_json_file(&directory.path().join("suite-manifest.json"), |v| v[field] = json!("drifted"));
            assert!(matches!(verify_kv_campaign_suite_r2_directory(directory.path()), Err(KvCampaignSuiteError::ProvenanceMismatch(_))), "accepted {field}");
        }
    }

    #[test]
    fn r2_rejects_preflight_receipts_and_unknown_schemas() {
        for schema in ["kvlab.smollm2-r2-position-suite-preflight/v1", "unknown/v1", "kvlab.smollm2-r2-position-suite-result/v2"] {
            let directory = TempDirectory::new("r2-schema");
            write_r2_suite(directory.path());
            edit_json_file(&directory.path().join("suite-manifest.json"), |v| v["schema"] = json!(schema));
            assert!(matches!(verify_kv_campaign_suite_r2_directory(directory.path()), Err(KvCampaignSuiteError::UnsupportedSchema)));
        }
    }

    #[test]
    fn r2_rejects_manifest_structure_order_paths_and_device_drift() {
        for change in ["missing", "duplicate", "order", "path", "extra", "device", "float"] {
            let directory = TempDirectory::new("r2-manifest");
            write_r2_suite(directory.path());
            edit_json_file(&directory.path().join("suite-manifest.json"), |v| match change {
                "missing" => { v["campaigns"].as_array_mut().unwrap().pop(); },
                "duplicate" => v["campaigns"][1] = v["campaigns"][0].clone(),
                "order" => v["campaigns"].as_array_mut().unwrap().swap(0, 1),
                "path" => v["campaigns"][0]["output_directory"] = json!("../outside"),
                "extra" => v["unrecognized"] = json!(true),
                "device" => v["device_ordinal"] = json!(2147483648_u64),
                "float" => v["campaigns"][0]["retained_count"] = json!(7.0),
                _ => unreachable!(),
            });
            assert!(verify_kv_campaign_suite_r2_directory(directory.path()).is_err(), "accepted {change}");
        }
    }

    #[test]
    fn r2_rejects_noncanonical_and_duplicate_manifest_keys() {
        for duplicate in [false, true] {
            let directory = TempDirectory::new("r2-canonical");
            write_r2_suite(directory.path());
            let path = directory.path().join("suite-manifest.json");
            let payload = fs::read_to_string(&path).unwrap();
            let invalid = if duplicate {
                payload.replacen('{', "{\"device_ordinal\":0,", 1)
            } else { format!("{payload}\n") };
            fs::write(&path, invalid).unwrap();
            assert!(matches!(verify_kv_campaign_suite_r2_directory(directory.path()), Err(KvCampaignSuiteError::NonCanonicalManifest)));
        }
    }

    #[test]
    fn r2_rejects_published_summary_tampering() {
        let directory = TempDirectory::new("r2-summary");
        write_r2_suite(directory.path());
        edit_json_file(&directory.path().join("verification-retain-07-of-27.json"), |v| v["observations"][0]["metrics"][0]["candidate_value"] = json!(42.0));
        assert!(matches!(verify_kv_campaign_suite_r2_directory(directory.path()), Err(KvCampaignSuiteError::PublishedVerificationMismatch(7))));
    }

    #[test]
    fn r2_rejects_cross_budget_baseline_output_drift() {
        let directory = TempDirectory::new("r2-baseline-output");
        write_suite_for_contract(directory.path(), true, None, &R2_CONTRACT);
        assert!(matches!(verify_kv_campaign_suite_r2_directory(directory.path()), Err(KvCampaignSuiteError::CrossCampaignBaselineMismatch)));
    }

    #[test]
    fn r2_rejects_coherently_rehashed_cross_budget_baseline_metric_drift() {
        let directory = TempDirectory::new("r2-baseline-metric");
        write_r2_suite(directory.path());
        for index in 0..2 {
            let path = directory.path().join(format!("retain-20-of-27/selection-{index:03}.json"));
            edit_json_file(&path, |v| {
                let metric = &mut v["metrics"][0];
                metric["baseline_value"] = json!(1.5);
                metric["delta"] = json!(metric["candidate_value"].as_f64().unwrap() - 1.5);
            });
        }
        refresh_r2_records(directory.path(), 20);
        assert!(matches!(verify_kv_campaign_suite_r2_directory(directory.path()), Err(KvCampaignSuiteError::CrossCampaignBaselineMismatch)));
    }

    #[test]
    fn r2_rejects_missing_evidence_and_unexpected_entries() {
        for extra in [false, true] {
            let directory = TempDirectory::new("r2-file-set");
            write_r2_suite(directory.path());
            if extra { fs::write(directory.path().join("not-evidence.txt"), "x").unwrap(); }
            else { fs::remove_file(directory.path().join("retain-07-of-27/selection-000.json")).unwrap(); }
            assert!(verify_kv_campaign_suite_r2_directory(directory.path()).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn r2_rejects_symlinks_at_every_evidence_boundary() {
        use std::os::unix::fs::symlink;
        for relative in ["suite-manifest.json", "verification-retain-07-of-27.json",
                         "retain-07-of-27/campaign.json", "retain-07-of-27/manifest.json",
                         "retain-07-of-27/selection-000.json"] {
            let directory = TempDirectory::new("r2-symlink");
            let outside = TempDirectory::new("r2-symlink-target");
            write_r2_suite(directory.path());
            let path = directory.path().join(relative);
            let target = outside.path().join("payload.json");
            fs::rename(&path, &target).unwrap();
            symlink(&target, &path).unwrap();
            assert!(matches!(verify_kv_campaign_suite_r2_directory(directory.path()), Err(KvCampaignSuiteError::EntryTypeMismatch(_))), "accepted symlink {relative}");
        }
    }

    #[test]
    fn r1_summary_schema_and_pins_stay_unchanged() {
        let directory = TempDirectory::new("r1-regression");
        write_suite(directory.path(), false);
        let summary = verify_kv_campaign_suite_directory(directory.path()).unwrap();
        assert_eq!(summary.schema, VERIFICATION_SCHEMA_V1);
        assert_eq!(summary.kvlab_suite_provider_revision, KVLAB_SUITE_PROVIDER_REVISION);
        assert_eq!(summary.kvlab_preregistration_revision, KVLAB_PREREGISTRATION_REVISION);
        assert_eq!(summary.nnis_runtime_revision, NNIS_RUNTIME_REVISION);
        assert_eq!(summary.prospect_launch_verifier_revision, PROSPECT_LAUNCH_VERIFIER_REVISION);
    }
