use super::*;

#[test]
fn all_schemas_returns_eleven() {
    assert_eq!(all_learning_controller_schemas().len(), 11);
}

#[test]
fn all_controllers_returns_eleven() {
    assert_eq!(all_learning_registered_controllers().len(), 11);
}

#[test]
fn save_profile_schema_shape() {
    let s = learning_schemas("learning_save_profile");
    assert_eq!(s.namespace, "learning");
    assert_eq!(s.function, "save_profile");
    assert!(s.inputs.iter().any(|f| f.name == "markdown" && f.required));
}

#[test]
fn linkedin_enrichment_schema() {
    let s = learning_schemas("learning_linkedin_enrichment");
    assert_eq!(s.namespace, "learning");
    assert_eq!(s.function, "linkedin_enrichment");
    // Optional `profile_url` input: the frontend supplies one when it
    // has already discovered the URL via the webview-driven Gmail
    // helper, letting the pipeline skip its Composio-only stage 1.
    assert_eq!(s.inputs.len(), 1);
    assert_eq!(s.inputs[0].name, "profile_url");
    assert!(!s.inputs[0].required);
    assert!(!s.outputs.is_empty());
}

#[test]
fn unknown_function_returns_unknown() {
    let s = learning_schemas("nonexistent");
    assert_eq!(s.function, "unknown");
}

#[test]
fn facet_to_json_includes_cue_families_and_evidence_refs() {
    use std::collections::HashMap;
    use tinymemory_api::host::EvidenceRef;
    use tinymemory_api::provider::{FacetState, FacetType, ProfileFacet, UserState};

    let mut cue_families = HashMap::new();
    cue_families.insert("explicit".to_string(), 3u32);
    cue_families.insert("structural".to_string(), 1u32);

    let facet = ProfileFacet {
        facet_id: "f1".into(),
        facet_type: FacetType::Preference,
        key: "style/verbosity".into(),
        value: "terse".into(),
        confidence: 0.8,
        evidence_count: 4,
        source_segment_ids: None,
        first_seen_at: 1000.0,
        last_seen_at: 1200.0,
        state: FacetState::Active,
        stability: 0.9,
        user_state: UserState::Auto,
        evidence_refs: vec![EvidenceRef::Episodic { episodic_id: 42 }],
        class: Some("style".into()),
        cue_families: Some(cue_families),
    };

    // Populated provenance round-trips through the serializer.
    let json = facet_to_json(&facet);
    assert_eq!(json["cue_families"]["explicit"].as_u64(), Some(3));
    assert_eq!(json["cue_families"]["structural"].as_u64(), Some(1));
    assert_eq!(json["evidence_refs"][0]["type"].as_str(), Some("episodic"));
    assert_eq!(json["evidence_refs"][0]["episodic_id"].as_i64(), Some(42));

    // Empty/None provenance serializes to []/null (present, not dropped).
    let bare = ProfileFacet {
        evidence_refs: vec![],
        cue_families: None,
        ..facet
    };
    let json = facet_to_json(&bare);
    assert_eq!(json["evidence_refs"].as_array().map(Vec::len), Some(0));
    assert!(json["cue_families"].is_null());
}

#[test]
fn schemas_and_controllers_match() {
    let s = all_learning_controller_schemas();
    let c = all_learning_registered_controllers();
    assert_eq!(s[0].function, c[0].schema.function);
}

#[test]
fn list_facets_schema_shape() {
    let s = learning_schemas("learning_list_facets");
    assert_eq!(s.namespace, "learning");
    assert_eq!(s.function, "list_facets");
    assert!(s.inputs.iter().any(|f| f.name == "class" && !f.required));
    assert!(s.outputs.iter().any(|f| f.name == "facets"));
    assert!(s.outputs.iter().any(|f| f.name == "count"));
}

#[test]
fn get_facet_schema_shape() {
    let s = learning_schemas("learning_get_facet");
    assert_eq!(s.function, "get_facet");
    assert!(s.inputs.iter().any(|f| f.name == "class" && f.required));
    assert!(s.inputs.iter().any(|f| f.name == "key" && f.required));
}

#[test]
fn update_facet_schema_shape() {
    let s = learning_schemas("learning_update_facet");
    assert_eq!(s.function, "update_facet");
    assert!(s.inputs.iter().any(|f| f.name == "value" && f.required));
}

#[test]
fn pin_facet_schema_shape() {
    let s = learning_schemas("learning_pin_facet");
    assert_eq!(s.function, "pin_facet");
}

#[test]
fn unpin_facet_schema_shape() {
    let s = learning_schemas("learning_unpin_facet");
    assert_eq!(s.function, "unpin_facet");
}

#[test]
fn forget_facet_schema_shape() {
    let s = learning_schemas("learning_forget_facet");
    assert_eq!(s.function, "forget_facet");
}

#[test]
fn reset_cache_schema_shape() {
    let s = learning_schemas("learning_reset_cache");
    assert_eq!(s.function, "reset_cache");
    assert!(s.outputs.iter().any(|f| f.name == "deleted"));
    assert!(s.outputs.iter().any(|f| f.name == "pinned_preserved"));
}
