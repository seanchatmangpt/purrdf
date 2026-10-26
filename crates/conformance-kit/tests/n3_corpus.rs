// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. <paudley@blackcatinformatics.ca>
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! The N3 corpus decodes under the strict portable contract.

use std::path::PathBuf;
use std::sync::Arc;

use purrdf_conformance_kit::graph::Reader;
use purrdf_conformance_kit::{inventory, manifest};
use purrdf_core::{GraphMatch, RdfDataset};

const CORPUS: &str = "../../corpora/n3";

fn read(root: &PathBuf, path: &str) -> Vec<u8> {
    let full = root.join(path);
    std::fs::read(&full).unwrap_or_else(|error| panic!("read {}: {error}", full.display()))
}

fn dataset(body: &[u8], base: &str) -> Arc<RdfDataset> {
    purrdf_rdf::parse_dataset(body, "text/turtle", Some(base)).expect("parse manifest as N3/Turtle")
}

#[test]
fn n3_corpus_decodes_and_manifests_reach_every_case() {
    let root = PathBuf::from(CORPUS);

    let catalog = inventory::decode_catalog(&read(&root, "catalog.json"))
        .expect("catalog decodes under the strict contract");
    assert_eq!(catalog.suites.len(), 1);
    assert_eq!(catalog.suites[0].kind, "n3");
    assert_eq!(catalog.case_count, 9);

    let inv = inventory::decode_inventory(&read(&root, "n3/inventory.json"))
        .expect("inventory decodes under the strict contract");
    assert_eq!(inv.cases.len(), 9);
    let mut seen = std::collections::BTreeSet::new();
    for case in &inv.cases {
        assert!(seen.insert(&case.id), "duplicate case id {}", case.id);
        assert_eq!(case.selected_profiles().unwrap(), ["n3-submission-2008"]);
        for path in case
            .query
            .iter()
            .chain(case.shapes.iter())
            .chain(case.data.iter().map(|input| &input.path))
        {
            let full = root.join(path);
            assert!(
                full.is_file(),
                "referenced fixture missing: {}",
                full.display()
            );
        }
        let manifest_path = case.manifest.as_deref().expect("case manifest declared");
        assert!(root.join(manifest_path).is_file());
    }
    assert_eq!(seen.len(), 9);

    for case in &inv.cases {
        let bytes = read(&root, case.manifest.as_deref().expect("declared"));
        let ds = dataset(&bytes, "http://example.org/n3/manifest.ttl");
        let reader = Reader {
            dataset: &ds,
            graph: GraphMatch::Any,
        };
        manifest::decode(reader).expect("case manifest decodes");
    }

    let suite_ds = dataset(&read(&root, "n3/manifest.ttl"), "http://example.org/n3/n3/manifest.ttl");
    let suite = manifest::decode(Reader {
        dataset: &suite_ds,
        graph: GraphMatch::Any,
    })
    .expect("suite manifest decodes");
    assert_eq!(suite.includes.len(), 9, "suite includes every case manifest");

    let root_ds = dataset(&read(&root, "manifest.ttl"), "http://example.org/n3/manifest.ttl");
    let root_doc = manifest::decode(Reader {
        dataset: &root_ds,
        graph: GraphMatch::Any,
    })
    .expect("root manifest decodes");
    assert_eq!(
        root_doc.includes,
        vec!["http://example.org/n3/n3/manifest.ttl".to_string()]
    );
}
