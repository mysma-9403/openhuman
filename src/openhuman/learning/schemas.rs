//! Controller schemas for the learning domain.

use serde_json::{Map, Value};

use crate::core::all::{ControllerFuture, RegisteredController};
use crate::core::{ControllerSchema, FieldSchema, TypeSchema};
use crate::openhuman::config::rpc as config_rpc;
use crate::rpc::RpcOutcome;

pub fn all_learning_controller_schemas() -> Vec<ControllerSchema> {
    vec![
        learning_schemas("learning_linkedin_enrichment"),
        learning_schemas("learning_save_profile"),
        learning_schemas("learning_rebuild_cache"),
        learning_schemas("learning_cache_stats"),
        learning_schemas("learning_list_facets"),
        learning_schemas("learning_get_facet"),
        learning_schemas("learning_facet_provenance"),
        learning_schemas("learning_update_facet"),
        learning_schemas("learning_pin_facet"),
        learning_schemas("learning_unpin_facet"),
        learning_schemas("learning_forget_facet"),
        learning_schemas("learning_reset_cache"),
    ]
}

pub fn all_learning_registered_controllers() -> Vec<RegisteredController> {
    vec![
        RegisteredController {
            schema: learning_schemas("learning_linkedin_enrichment"),
            handler: handle_linkedin_enrichment,
        },
        RegisteredController {
            schema: learning_schemas("learning_save_profile"),
            handler: handle_save_profile,
        },
        RegisteredController {
            schema: learning_schemas("learning_rebuild_cache"),
            handler: handle_rebuild_cache,
        },
        RegisteredController {
            schema: learning_schemas("learning_cache_stats"),
            handler: handle_cache_stats,
        },
        RegisteredController {
            schema: learning_schemas("learning_list_facets"),
            handler: handle_list_facets,
        },
        RegisteredController {
            schema: learning_schemas("learning_get_facet"),
            handler: handle_get_facet,
        },
        RegisteredController {
            schema: learning_schemas("learning_facet_provenance"),
            handler: handle_facet_provenance,
        },
        RegisteredController {
            schema: learning_schemas("learning_update_facet"),
            handler: handle_update_facet,
        },
        RegisteredController {
            schema: learning_schemas("learning_pin_facet"),
            handler: handle_pin_facet,
        },
        RegisteredController {
            schema: learning_schemas("learning_unpin_facet"),
            handler: handle_unpin_facet,
        },
        RegisteredController {
            schema: learning_schemas("learning_forget_facet"),
            handler: handle_forget_facet,
        },
        RegisteredController {
            schema: learning_schemas("learning_reset_cache"),
            handler: handle_reset_cache,
        },
    ]
}

pub fn learning_schemas(function: &str) -> ControllerSchema {
    match function {
        "learning_linkedin_enrichment" => ControllerSchema {
            namespace: "learning",
            function: "linkedin_enrichment",
            description: "Search Gmail for LinkedIn profile URLs, scrape the profile via Apify, \
                          and persist the result to memory. Runs the full enrichment pipeline.",
            inputs: vec![FieldSchema {
                name: "profile_url",
                ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                comment: "Pre-found LinkedIn profile URL (skips the Gmail-search stage). \
                          The frontend supplies this when it has already located the URL via \
                          the webview-driven `gmail_find_linkedin_profile_url` Tauri command.",
                required: false,
            }],
            outputs: vec![
                FieldSchema {
                    name: "profile_url",
                    ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                    comment: "LinkedIn profile URL found in Gmail, if any.",
                    required: false,
                },
                FieldSchema {
                    name: "profile_data",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "Scraped LinkedIn profile JSON from Apify, if successful.",
                    required: false,
                },
                FieldSchema {
                    name: "log",
                    ty: TypeSchema::Array(Box::new(TypeSchema::String)),
                    comment: "Human-readable log of each pipeline stage.",
                    required: true,
                },
            ],
        },
        "learning_save_profile" => ControllerSchema {
            namespace: "learning",
            function: "save_profile",
            description: "Persist a markdown profile to `{workspace_dir}/PROFILE.md`. \
                          When `summarize=true`, runs the body through the LLM compressor \
                          first (same prompt as the LinkedIn-enrichment pipeline) so callers \
                          can hand in raw scraped material and get the same end-state.",
            inputs: vec![
                FieldSchema {
                    name: "markdown",
                    ty: TypeSchema::String,
                    comment: "Markdown body to persist (or to summarize first).",
                    required: true,
                },
                FieldSchema {
                    name: "summarize",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Bool)),
                    comment: "Compress through LLM before writing (default false).",
                    required: false,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "path",
                    ty: TypeSchema::String,
                    comment: "Absolute path of the written PROFILE.md.",
                    required: true,
                },
                FieldSchema {
                    name: "bytes",
                    ty: TypeSchema::U64,
                    comment: "Bytes written.",
                    required: true,
                },
            ],
        },
        "learning_rebuild_cache" => ControllerSchema {
            namespace: "learning",
            function: "rebuild_cache",
            description: "Manually trigger a stability-detector rebuild cycle. \
                          Drains the candidate buffer, scores all (class, key) pairs, \
                          applies class budgets, and persists the updated ambient cache. \
                          Returns rebuild statistics.",
            inputs: vec![],
            outputs: vec![
                FieldSchema {
                    name: "added",
                    ty: TypeSchema::U64,
                    comment: "Facet rows newly created in this cycle.",
                    required: true,
                },
                FieldSchema {
                    name: "evicted",
                    ty: TypeSchema::U64,
                    comment: "Facet rows demoted to Dropped or deleted.",
                    required: true,
                },
                FieldSchema {
                    name: "kept",
                    ty: TypeSchema::U64,
                    comment: "Facet rows carried over unchanged.",
                    required: true,
                },
                FieldSchema {
                    name: "total_size",
                    ty: TypeSchema::U64,
                    comment: "Total Active rows after the rebuild.",
                    required: true,
                },
            ],
        },
        "learning_cache_stats" => ControllerSchema {
            namespace: "learning",
            function: "cache_stats",
            description: "Return current ambient cache statistics — total row count, \
                          per-state breakdown, and per-class breakdown.",
            inputs: vec![],
            outputs: vec![
                FieldSchema {
                    name: "total",
                    ty: TypeSchema::U64,
                    comment: "Total rows in the cache (all states).",
                    required: true,
                },
                FieldSchema {
                    name: "active",
                    ty: TypeSchema::U64,
                    comment: "Rows with state=active.",
                    required: true,
                },
                FieldSchema {
                    name: "provisional",
                    ty: TypeSchema::U64,
                    comment: "Rows with state=provisional.",
                    required: true,
                },
                FieldSchema {
                    name: "candidate",
                    ty: TypeSchema::U64,
                    comment: "Rows with state=candidate.",
                    required: true,
                },
                FieldSchema {
                    name: "dropped",
                    ty: TypeSchema::U64,
                    comment: "Rows with state=dropped.",
                    required: true,
                },
                FieldSchema {
                    name: "by_class",
                    ty: TypeSchema::Json,
                    comment: "Map of class name → row count (active rows only).",
                    required: true,
                },
            ],
        },
        "learning_list_facets" => ControllerSchema {
            namespace: "learning",
            function: "list_facets",
            description: "List all facets in the ambient cache (active + provisional). \
                          Optionally filter by class.",
            inputs: vec![FieldSchema {
                name: "class",
                ty: TypeSchema::Option(Box::new(TypeSchema::String)),
                comment:
                    "Optional class filter: style | identity | tooling | veto | goal | channel.",
                required: false,
            }],
            outputs: vec![
                FieldSchema {
                    name: "facets",
                    ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                    comment:
                        "Array of facet objects with key, value, state, user_state, stability.",
                    required: true,
                },
                FieldSchema {
                    name: "count",
                    ty: TypeSchema::U64,
                    comment: "Total number of facets returned.",
                    required: true,
                },
            ],
        },
        "learning_get_facet" => ControllerSchema {
            namespace: "learning",
            function: "get_facet",
            description:
                "Fetch a single facet by class and key suffix (e.g. class=style, key=verbosity).",
            inputs: vec![
                FieldSchema {
                    name: "class",
                    ty: TypeSchema::String,
                    comment: "Facet class: style | identity | tooling | veto | goal | channel.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment:
                        "Key suffix within the class (e.g. \"verbosity\" for style/verbosity).",
                    required: true,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "facet",
                    ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                    comment: "The facet object, or null if not found.",
                    required: false,
                },
                FieldSchema {
                    name: "found",
                    ty: TypeSchema::Bool,
                    comment: "Whether the facet was found.",
                    required: true,
                },
            ],
        },
        "learning_facet_provenance" => ControllerSchema {
            namespace: "learning",
            function: "facet_provenance",
            description: "Provenance for one facet: the observations recorded for its \
                 (class, key) that the assistant learned from, rendered for a \"what I know \
                 about you\" surface. Refs are facet-KEY-scoped — the stability detector \
                 merges every candidate's evidence for the key before persisting the single \
                 winning value, so this may include observations that reinforced a prior or \
                 competing value, not only the current one. Each entry carries a `type`, a \
                 human-readable `label`, and the source's own identifiers.",
            inputs: vec![
                FieldSchema {
                    name: "class",
                    ty: TypeSchema::String,
                    comment: "Facet class: style | identity | tooling | veto | goal | channel.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment:
                        "Key suffix within the class (e.g. \"verbosity\" for style/verbosity).",
                    required: true,
                },
            ],
            outputs: vec![
                FieldSchema {
                    name: "found",
                    ty: TypeSchema::Bool,
                    comment: "Whether the facet exists.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment: "The resolved full facet key (class/suffix).",
                    required: true,
                },
                FieldSchema {
                    // `I64`, not `U64`: the wire value is `ProfileFacet::evidence_count`
                    // (an `i32`, emitted raw), so the signed type matches the source.
                    name: "evidence_count",
                    ty: TypeSchema::I64,
                    comment: "Total observations that reinforced the facet (may exceed the \
                        number of traceable sources below).",
                    required: true,
                },
                FieldSchema {
                    name: "evidence",
                    ty: TypeSchema::Array(Box::new(TypeSchema::Json)),
                    comment: "Key-scoped observations for this facet, each `{ type, label, \
                        …ref-specific ids }` — e.g. episodic / tree_topic / document_chunk / \
                        email_message / provider. Not filtered to the winning value (see \
                        description).",
                    required: true,
                },
            ],
        },
        "learning_update_facet" => ControllerSchema {
            namespace: "learning",
            function: "update_facet",
            description: "Update the value of an existing facet and pin it (user_state=Pinned). \
                          Returns the updated facet.",
            inputs: vec![
                FieldSchema {
                    name: "class",
                    ty: TypeSchema::String,
                    comment: "Facet class: style | identity | tooling | veto | goal | channel.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment: "Key suffix within the class.",
                    required: true,
                },
                FieldSchema {
                    name: "value",
                    ty: TypeSchema::String,
                    comment: "New value to set.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "facet",
                ty: TypeSchema::Json,
                comment: "The updated facet object.",
                required: true,
            }],
        },
        "learning_pin_facet" => ControllerSchema {
            namespace: "learning",
            function: "pin_facet",
            description:
                "Pin a facet (user_state=Pinned): locks Active regardless of stability score.",
            inputs: vec![
                FieldSchema {
                    name: "class",
                    ty: TypeSchema::String,
                    comment: "Facet class.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment: "Key suffix within the class.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "facet",
                ty: TypeSchema::Json,
                comment: "The updated facet object.",
                required: true,
            }],
        },
        "learning_unpin_facet" => ControllerSchema {
            namespace: "learning",
            function: "unpin_facet",
            description:
                "Unpin a facet (user_state=Auto): returns stability management to the detector.",
            inputs: vec![
                FieldSchema {
                    name: "class",
                    ty: TypeSchema::String,
                    comment: "Facet class.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment: "Key suffix within the class.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "facet",
                ty: TypeSchema::Json,
                comment: "The updated facet object.",
                required: true,
            }],
        },
        "learning_forget_facet" => ControllerSchema {
            namespace: "learning",
            function: "forget_facet",
            description:
                "Forget a facet (user_state=Forgotten): locks Dropped and blocks re-promotion.",
            inputs: vec![
                FieldSchema {
                    name: "class",
                    ty: TypeSchema::String,
                    comment: "Facet class.",
                    required: true,
                },
                FieldSchema {
                    name: "key",
                    ty: TypeSchema::String,
                    comment: "Key suffix within the class.",
                    required: true,
                },
            ],
            outputs: vec![FieldSchema {
                name: "facet",
                ty: TypeSchema::Option(Box::new(TypeSchema::Json)),
                comment: "The facet in its Dropped state, or null if it didn't exist.",
                required: false,
            }],
        },
        "learning_reset_cache" => ControllerSchema {
            namespace: "learning",
            function: "reset_cache",
            description: "Reset the ambient cache: delete all Auto rows, preserve Pinned rows. \
                          The next rebuild repopulates from the substrate.",
            inputs: vec![],
            outputs: vec![
                FieldSchema {
                    name: "deleted",
                    ty: TypeSchema::U64,
                    comment: "Number of Auto rows deleted.",
                    required: true,
                },
                FieldSchema {
                    name: "pinned_preserved",
                    ty: TypeSchema::U64,
                    comment: "Number of Pinned rows kept.",
                    required: true,
                },
            ],
        },
        _ => ControllerSchema {
            namespace: "learning",
            function: "unknown",
            description: "Unknown learning controller.",
            inputs: vec![],
            outputs: vec![FieldSchema {
                name: "error",
                ty: TypeSchema::String,
                comment: "Lookup error details.",
                required: true,
            }],
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_schemas_returns_twelve() {
        assert_eq!(all_learning_controller_schemas().len(), 12);
    }

    #[test]
    fn all_controllers_returns_twelve() {
        assert_eq!(all_learning_registered_controllers().len(), 12);
    }

    #[test]
    fn facet_provenance_schema_shape() {
        let s = learning_schemas("learning_facet_provenance");
        assert_eq!(s.namespace, "learning");
        assert_eq!(s.function, "facet_provenance");
        let required_inputs: Vec<_> = s
            .inputs
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name)
            .collect();
        assert_eq!(required_inputs, vec!["class", "key"]);
        let out_names: Vec<_> = s.outputs.iter().map(|f| f.name).collect();
        assert_eq!(
            out_names,
            vec!["found", "key", "evidence_count", "evidence"]
        );
    }

    #[test]
    fn provenance_entry_renders_every_variant() {
        use crate::openhuman::learning::candidate::EvidenceRef as E;

        // One assertion row per `EvidenceRef` arm so the exhaustive `match` in
        // `provenance_entry` is not just compiled but verified to emit the
        // intended `type` / `label` / id fields. Ordered to mirror the enum.
        let episodic = provenance_entry(&E::Episodic { episodic_id: 42 });
        assert_eq!(episodic["type"], "episodic");
        assert_eq!(episodic["label"], "Episodic memory #42");
        assert_eq!(episodic["episodic_id"], 42);

        let window = provenance_entry(&E::EpisodicWindow {
            from_id: 3,
            to_id: 7,
        });
        assert_eq!(window["type"], "episodic_window");
        assert_eq!(window["label"], "Episodic memories #3–#7");
        assert_eq!(window["from_id"], 3);
        assert_eq!(window["to_id"], 7);

        let summary = provenance_entry(&E::SourceSummary {
            summary_id: "sum-9".into(),
        });
        assert_eq!(summary["type"], "source_summary");
        assert_eq!(summary["label"], "Source summary sum-9");
        assert_eq!(summary["summary_id"], "sum-9");

        let topic = provenance_entry(&E::TreeTopic {
            topic_id: "roadmap".into(),
        });
        assert_eq!(topic["type"], "tree_topic");
        assert_eq!(topic["label"], "Topic \"roadmap\"");
        assert_eq!(topic["topic_id"], "roadmap");

        let chunk = provenance_entry(&E::DocumentChunk {
            source_id: "doc:5".into(),
            chunk_id: "c2".into(),
        });
        assert_eq!(chunk["type"], "document_chunk");
        assert_eq!(chunk["label"], "Document doc:5");
        assert_eq!(chunk["source_id"], "doc:5");
        assert_eq!(chunk["chunk_id"], "c2");

        let email = provenance_entry(&E::EmailMessage {
            source_id: "gmail:abc".into(),
            message_id: "m1".into(),
        });
        assert_eq!(email["type"], "email_message");
        assert_eq!(email["label"], "Email in gmail:abc");
        assert_eq!(email["source_id"], "gmail:abc");
        assert_eq!(email["message_id"], "m1");

        let provider = provenance_entry(&E::Provider {
            toolkit: "gmail".into(),
            connection_id: "conn1".into(),
            field: "timezone".into(),
        });
        assert_eq!(provider["type"], "provider");
        assert_eq!(provider["label"], "gmail · timezone");
        assert_eq!(provider["toolkit"], "gmail");
        assert_eq!(provider["connection_id"], "conn1");
        assert_eq!(provider["field"], "timezone");

        let tool_call = provenance_entry(&E::ToolCall {
            tool_name: "web_search".into(),
            episodic_id: 11,
        });
        assert_eq!(tool_call["type"], "tool_call");
        assert_eq!(tool_call["label"], "Tool call web_search");
        assert_eq!(tool_call["tool_name"], "web_search");
        assert_eq!(tool_call["episodic_id"], 11);

        let weight = provenance_entry(&E::TreeSourceWeight {
            window_label: "w-2024".into(),
        });
        assert_eq!(weight["type"], "tree_source_weight");
        assert_eq!(weight["label"], "Source weight w-2024");
        assert_eq!(weight["window_label"], "w-2024");

        // The batch wrapper preserves order and length.
        let entries = provenance_entries(&[
            E::Episodic { episodic_id: 1 },
            E::Episodic { episodic_id: 2 },
        ]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["episodic_id"], 1);
        assert_eq!(entries[1]["episodic_id"], 2);
    }

    #[test]
    fn build_provenance_payload_found_and_not_found() {
        use crate::openhuman::learning::candidate::EvidenceRef;
        use crate::openhuman::memory_store::profile::{
            FacetState, FacetType, ProfileFacet, UserState,
        };

        // Not-found: the None branch reports an honest empty result.
        let missing = build_provenance_payload("style/verbosity", None);
        assert_eq!(missing["found"], false);
        assert_eq!(missing["key"], "style/verbosity");
        assert_eq!(missing["evidence_count"], 0);
        assert_eq!(missing["evidence"].as_array().unwrap().len(), 0);

        // Found: `evidence_count` passes through raw and each stored ref is
        // rendered into the `evidence` array in order.
        let facet = ProfileFacet {
            facet_id: "f1".into(),
            facet_type: FacetType::Preference,
            key: "style/verbosity".into(),
            value: "terse".into(),
            confidence: 0.9,
            evidence_count: 5,
            source_segment_ids: None,
            first_seen_at: 1000.0,
            last_seen_at: 1200.0,
            state: FacetState::Active,
            stability: 0.8,
            user_state: UserState::Auto,
            evidence_refs: vec![
                EvidenceRef::Episodic { episodic_id: 1 },
                EvidenceRef::TreeTopic {
                    topic_id: "roadmap".into(),
                },
            ],
            class: Some("style".into()),
            cue_families: None,
        };
        let found = build_provenance_payload("style/verbosity", Some(&facet));
        assert_eq!(found["found"], true);
        assert_eq!(found["key"], "style/verbosity");
        // Passthrough of the raw i32 count (may exceed traceable sources).
        assert_eq!(found["evidence_count"], 5);
        let evidence = found["evidence"].as_array().unwrap();
        assert_eq!(evidence.len(), 2);
        assert_eq!(evidence[0]["type"], "episodic");
        assert_eq!(evidence[1]["type"], "tree_topic");
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
        use crate::openhuman::learning::candidate::EvidenceRef;
        use crate::openhuman::memory_store::profile::{
            FacetState, FacetType, ProfileFacet, UserState,
        };
        use std::collections::HashMap;

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
}

fn handle_linkedin_enrichment(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let preset_profile_url = params
            .get("profile_url")
            .and_then(Value::as_str)
            .map(str::to_string);
        let config = config_rpc::load_config_with_timeout().await?;
        let result =
            super::linkedin_enrichment::run_linkedin_enrichment(&config, preset_profile_url)
                .await
                .map_err(|e| format!("linkedin enrichment failed: {e:#}"))?;

        let payload = serde_json::json!({
            "profile_url": result.profile_url,
            "profile_data": result.profile_data,
            "stages": result.stages,
            "log": result.log,
        });

        RpcOutcome::new(payload, result.log.clone()).into_cli_compatible_json()
    })
}

fn handle_save_profile(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let markdown = params
            .get("markdown")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "missing required `markdown`".to_string())?;
        let summarize = params
            .get("summarize")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let config = config_rpc::load_config_with_timeout().await?;

        let body = if summarize {
            super::linkedin_enrichment::summarise_profile_with_llm(&config, &markdown)
                .await
                .map_err(|e| format!("LLM summarisation failed: {e:#}"))?
        } else {
            markdown
        };

        let path = config.workspace_dir.join("PROFILE.md");
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("create workspace dir failed: {e}"))?;
        }
        tokio::fs::write(&path, &body)
            .await
            .map_err(|e| format!("write PROFILE.md failed: {e}"))?;

        let bytes = body.len();
        let path_display = path.display().to_string();
        let payload = serde_json::json!({
            "path": path_display,
            "bytes": bytes,
        });
        let log = vec![format!(
            "learning.save_profile: wrote {bytes} bytes to {path_display} (summarize={summarize})"
        )];
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

fn handle_rebuild_cache(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use crate::openhuman::learning::cache::FacetCache;
        use crate::openhuman::learning::stability_detector::StabilityDetector;
        use std::time::{SystemTime, UNIX_EPOCH};

        tracing::debug!("[learning.rebuild_cache] manual rebuild requested via RPC");

        let client = crate::openhuman::memory::global::client_if_ready()
            .ok_or_else(|| "memory client not ready".to_string())?;
        let conn = client.profile_conn();
        let cache = FacetCache::new(conn);
        let detector = StabilityDetector::new(cache);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();

        let outcome = detector
            .rebuild(now)
            .map_err(|e| format!("rebuild failed: {e:#}"))?;

        let log = vec![format!(
            "learning.rebuild_cache: added={} evicted={} kept={} total={}",
            outcome.added, outcome.evicted, outcome.kept, outcome.total_size,
        )];

        let payload = serde_json::json!({
            "added": outcome.added,
            "evicted": outcome.evicted,
            "kept": outcome.kept,
            "total_size": outcome.total_size,
        });

        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

fn handle_cache_stats(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use crate::openhuman::learning::cache::FacetCache;
        use crate::openhuman::memory_store::profile::FacetState;

        tracing::debug!("[learning.cache_stats] cache stats requested via RPC");

        let client = crate::openhuman::memory::global::client_if_ready()
            .ok_or_else(|| "memory client not ready".to_string())?;
        let conn = client.profile_conn();
        let cache = FacetCache::new(conn);

        let all_facets = cache
            .list_all()
            .map_err(|e| format!("list_all failed: {e:#}"))?;

        let total = all_facets.len();
        let active = all_facets
            .iter()
            .filter(|f| f.state == FacetState::Active)
            .count();
        let provisional = all_facets
            .iter()
            .filter(|f| f.state == FacetState::Provisional)
            .count();
        let candidate = all_facets
            .iter()
            .filter(|f| f.state == FacetState::Candidate)
            .count();
        let dropped = all_facets
            .iter()
            .filter(|f| f.state == FacetState::Dropped)
            .count();

        // Per-class count (Active rows only, keyed by class field or key prefix).
        let mut by_class: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for f in all_facets.iter().filter(|f| f.state == FacetState::Active) {
            let cls = f
                .class
                .clone()
                .or_else(|| f.key.split_once('/').map(|(p, _)| p.to_string()))
                .unwrap_or_else(|| "_other".to_string());
            *by_class.entry(cls).or_insert(0) += 1;
        }

        let log = vec![format!(
            "learning.cache_stats: total={total} active={active} provisional={provisional} \
             candidate={candidate} dropped={dropped}"
        )];

        let payload = serde_json::json!({
            "total": total,
            "active": active,
            "provisional": provisional,
            "candidate": candidate,
            "dropped": dropped,
            "by_class": by_class,
        });

        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── Helper: shared cache access ───────────────────────────────────────────────

/// Build a [`FacetCache`] from the global memory client, or return a string error.
fn get_cache() -> Result<crate::openhuman::learning::cache::FacetCache, String> {
    let client = crate::openhuman::memory::global::client_if_ready()
        .ok_or_else(|| "memory client not ready".to_string())?;
    Ok(crate::openhuman::learning::cache::FacetCache::new(
        client.profile_conn(),
    ))
}

/// Build the full facet key from class string + key suffix.
/// E.g. (`"style"`, `"verbosity"`) → `"style/verbosity"`.
fn full_key(class_str: &str, key_suffix: &str) -> String {
    format!("{class_str}/{key_suffix}")
}

/// Serialize a [`ProfileFacet`] to a serde_json [`Value`] for RPC output.
fn facet_to_json(f: &crate::openhuman::memory_store::profile::ProfileFacet) -> serde_json::Value {
    serde_json::json!({
        "key": f.key,
        "value": f.value,
        "state": f.state.as_str(),
        "user_state": f.user_state.as_str(),
        "stability": f.stability,
        "confidence": f.confidence,
        "evidence_count": f.evidence_count,
        "first_seen_at": f.first_seen_at,
        "last_seen_at": f.last_seen_at,
        "class": f.class,
        // Provenance the store already persists and row_to_facet hydrates, but
        // the serializer previously dropped: the citations behind the facet and
        // the per-cue-family evidence counts. `None`/empty serialize to
        // `null`/`[]`.
        "cue_families": f.cue_families,
        "evidence_refs": f.evidence_refs,
    })
}

/// Render one [`EvidenceRef`] as a UI-friendly provenance entry: a stable
/// `type` discriminator, a short human-readable `label`, and the ref's own
/// identifier fields so a caller can deep-link into the source later. Pure —
/// no cross-store lookups — so it is cheap and deterministic.
fn provenance_entry(r: &crate::openhuman::learning::candidate::EvidenceRef) -> serde_json::Value {
    use crate::openhuman::learning::candidate::EvidenceRef as E;
    match r {
        E::Episodic { episodic_id } => serde_json::json!({
            "type": "episodic",
            "label": format!("Episodic memory #{episodic_id}"),
            "episodic_id": episodic_id,
        }),
        E::EpisodicWindow { from_id, to_id } => serde_json::json!({
            "type": "episodic_window",
            "label": format!("Episodic memories #{from_id}–#{to_id}"),
            "from_id": from_id,
            "to_id": to_id,
        }),
        E::SourceSummary { summary_id } => serde_json::json!({
            "type": "source_summary",
            "label": format!("Source summary {summary_id}"),
            "summary_id": summary_id,
        }),
        E::TreeTopic { topic_id } => serde_json::json!({
            "type": "tree_topic",
            "label": format!("Topic \"{topic_id}\""),
            "topic_id": topic_id,
        }),
        E::DocumentChunk {
            source_id,
            chunk_id,
        } => serde_json::json!({
            "type": "document_chunk",
            "label": format!("Document {source_id}"),
            "source_id": source_id,
            "chunk_id": chunk_id,
        }),
        E::EmailMessage {
            source_id,
            message_id,
        } => serde_json::json!({
            "type": "email_message",
            "label": format!("Email in {source_id}"),
            "source_id": source_id,
            "message_id": message_id,
        }),
        E::Provider {
            toolkit,
            connection_id,
            field,
        } => serde_json::json!({
            "type": "provider",
            "label": format!("{toolkit} · {field}"),
            "toolkit": toolkit,
            "connection_id": connection_id,
            "field": field,
        }),
        E::ToolCall {
            tool_name,
            episodic_id,
        } => serde_json::json!({
            "type": "tool_call",
            "label": format!("Tool call {tool_name}"),
            "tool_name": tool_name,
            "episodic_id": episodic_id,
        }),
        E::TreeSourceWeight { window_label } => serde_json::json!({
            "type": "tree_source_weight",
            "label": format!("Source weight {window_label}"),
            "window_label": window_label,
        }),
    }
}

/// Render a facet's stored evidence refs as an ordered list of provenance
/// entries for the "what the assistant learned about you" surface. The refs are
/// key-scoped (the detector merges all candidate evidence for a `(class, key)`
/// before persisting the winning value), so this reflects observations for the
/// facet key, not proof of the current value specifically — callers should frame
/// it that way.
fn provenance_entries(refs: &[crate::openhuman::learning::candidate::EvidenceRef]) -> Vec<Value> {
    refs.iter().map(provenance_entry).collect()
}

// ── list_facets ───────────────────────────────────────────────────────────────

fn handle_list_facets(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use crate::openhuman::memory_store::profile::FacetState;

        tracing::debug!("[learning.list_facets] called");

        let class_filter = params
            .get("class")
            .and_then(Value::as_str)
            .map(str::to_string);

        let cache = get_cache()?;

        // list_all returns all states (active + provisional + candidate + dropped).
        let all = cache
            .list_all()
            .map_err(|e| format!("list_all failed: {e:#}"))?;

        let facets: Vec<serde_json::Value> = all
            .iter()
            .filter(|f| {
                // Expose Active and Provisional rows to the user.
                f.state == FacetState::Active || f.state == FacetState::Provisional
            })
            .filter(|f| {
                if let Some(cls) = &class_filter {
                    f.class.as_deref() == Some(cls.as_str())
                        || f.key.starts_with(&format!("{cls}/"))
                } else {
                    true
                }
            })
            .map(facet_to_json)
            .collect();

        let count = facets.len();
        let log = vec![format!(
            "learning.list_facets: returned {count} facets (class_filter={:?})",
            class_filter
        )];

        let payload = serde_json::json!({ "facets": facets, "count": count });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── get_facet ─────────────────────────────────────────────────────────────────

fn handle_get_facet(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let class_str = params
            .get("class")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `class`".to_string())?
            .to_string();
        let key_suffix = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `key`".to_string())?
            .to_string();

        let fk = full_key(&class_str, &key_suffix);
        tracing::debug!("[learning.get_facet] key={fk}");

        let cache = get_cache()?;
        let facet = cache.get(&fk).map_err(|e| format!("get failed: {e:#}"))?;

        let (found, facet_val) = match &facet {
            Some(f) => (true, facet_to_json(f)),
            None => (false, serde_json::Value::Null),
        };

        let log = vec![format!("learning.get_facet: key={fk} found={found}")];
        let payload = serde_json::json!({ "facet": facet_val, "found": found });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── facet_provenance ──────────────────────────────────────────────────────────

fn handle_facet_provenance(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        let class_str = params
            .get("class")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `class`".to_string())?
            .to_string();
        let key_suffix = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `key`".to_string())?
            .to_string();

        let fk = full_key(&class_str, &key_suffix);
        tracing::debug!("[learning.facet_provenance] key={fk}");

        let cache = get_cache()?;
        let facet = cache.get(&fk).map_err(|e| format!("get failed: {e:#}"))?;

        let payload = build_provenance_payload(&fk, facet.as_ref());
        let found = payload["found"].as_bool().unwrap_or(false);
        let sources = payload["evidence"].as_array().map_or(0, Vec::len);
        let log = vec![format!(
            "learning.facet_provenance: key={fk} found={found} sources={sources}"
        )];
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

/// Assemble the `learning.facet_provenance` JSON payload from a resolved facet.
///
/// Pure — split out of [`handle_facet_provenance`] (which owns the async
/// param-parse + cache plumbing) so the found / not-found shape can be
/// unit-tested without a live [`FacetCache`]. A missing facet renders as
/// `found=false` with an empty evidence list and a zero count.
fn build_provenance_payload(
    fk: &str,
    facet: Option<&crate::openhuman::memory_store::profile::ProfileFacet>,
) -> Value {
    let (found, evidence, evidence_count) = match facet {
        Some(f) => (true, provenance_entries(&f.evidence_refs), f.evidence_count),
        None => (false, Vec::new(), 0),
    };
    serde_json::json!({
        "found": found,
        "key": fk,
        "evidence_count": evidence_count,
        "evidence": evidence,
    })
}

// ── update_facet ──────────────────────────────────────────────────────────────

fn handle_update_facet(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use crate::openhuman::memory_store::profile::UserState;

        let class_str = params
            .get("class")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `class`".to_string())?
            .to_string();
        let key_suffix = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `key`".to_string())?
            .to_string();
        let new_value = params
            .get("value")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `value`".to_string())?
            .to_string();

        let fk = full_key(&class_str, &key_suffix);
        tracing::debug!("[learning.update_facet] key={fk} value={new_value}");

        let cache = get_cache()?;

        let mut facet = cache
            .get(&fk)
            .map_err(|e| format!("get failed: {e:#}"))?
            .ok_or_else(|| format!("facet not found: {fk}"))?;

        // Update value and pin so this survives future rebuilds.
        facet.value = new_value.clone();
        facet.user_state = UserState::Pinned;

        cache
            .upsert(&facet)
            .map_err(|e| format!("upsert failed: {e:#}"))?;

        let updated = cache
            .get(&fk)
            .map_err(|e| format!("re-read failed: {e:#}"))?
            .ok_or_else(|| "facet disappeared after upsert".to_string())?;

        let log = vec![format!(
            "learning.update_facet: key={fk} new_value={new_value} user_state=pinned"
        )];
        let payload = serde_json::json!({ "facet": facet_to_json(&updated) });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── pin_facet ─────────────────────────────────────────────────────────────────

fn handle_pin_facet(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use crate::openhuman::memory_store::profile::UserState;

        let class_str = params
            .get("class")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `class`".to_string())?
            .to_string();
        let key_suffix = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `key`".to_string())?
            .to_string();

        let fk = full_key(&class_str, &key_suffix);
        tracing::debug!("[learning.pin_facet] key={fk}");

        let cache = get_cache()?;
        let updated = cache
            .set_user_state(&fk, UserState::Pinned)
            .map_err(|e| format!("set_user_state failed: {e:#}"))?;

        if !updated {
            return Err(format!("facet not found: {fk}"));
        }

        let facet = cache
            .get(&fk)
            .map_err(|e| format!("re-read failed: {e:#}"))?
            .ok_or_else(|| "facet disappeared after update".to_string())?;

        let log = vec![format!("learning.pin_facet: key={fk} user_state=pinned")];
        let payload = serde_json::json!({ "facet": facet_to_json(&facet) });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── unpin_facet ───────────────────────────────────────────────────────────────

fn handle_unpin_facet(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use crate::openhuman::memory_store::profile::UserState;

        let class_str = params
            .get("class")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `class`".to_string())?
            .to_string();
        let key_suffix = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `key`".to_string())?
            .to_string();

        let fk = full_key(&class_str, &key_suffix);
        tracing::debug!("[learning.unpin_facet] key={fk}");

        let cache = get_cache()?;
        let updated = cache
            .set_user_state(&fk, UserState::Auto)
            .map_err(|e| format!("set_user_state failed: {e:#}"))?;

        if !updated {
            return Err(format!("facet not found: {fk}"));
        }

        let facet = cache
            .get(&fk)
            .map_err(|e| format!("re-read failed: {e:#}"))?
            .ok_or_else(|| "facet disappeared after update".to_string())?;

        let log = vec![format!("learning.unpin_facet: key={fk} user_state=auto")];
        let payload = serde_json::json!({ "facet": facet_to_json(&facet) });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── forget_facet ──────────────────────────────────────────────────────────────

fn handle_forget_facet(params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use crate::openhuman::memory_store::profile::{FacetState, UserState};

        let class_str = params
            .get("class")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `class`".to_string())?
            .to_string();
        let key_suffix = params
            .get("key")
            .and_then(Value::as_str)
            .ok_or_else(|| "missing required `key`".to_string())?
            .to_string();

        let fk = full_key(&class_str, &key_suffix);
        tracing::debug!("[learning.forget_facet] key={fk}");

        let cache = get_cache()?;

        let facet_before = cache.get(&fk).map_err(|e| format!("get failed: {e:#}"))?;

        let facet_json = if let Some(mut f) = facet_before {
            // Mark Forgotten + Dropped so it doesn't resurface.
            f.user_state = UserState::Forgotten;
            f.state = FacetState::Dropped;
            cache
                .upsert(&f)
                .map_err(|e| format!("upsert failed: {e:#}"))?;
            let updated = cache
                .get(&fk)
                .map_err(|e| format!("re-read failed: {e:#}"))?
                .unwrap_or(f);
            facet_to_json(&updated)
        } else {
            serde_json::Value::Null
        };

        let log = vec![format!(
            "learning.forget_facet: key={fk} state=dropped user_state=forgotten"
        )];
        let payload = serde_json::json!({ "facet": facet_json });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}

// ── reset_cache ───────────────────────────────────────────────────────────────

fn handle_reset_cache(_params: Map<String, Value>) -> ControllerFuture {
    Box::pin(async move {
        use crate::openhuman::memory_store::profile::UserState;

        tracing::debug!("[learning.reset_cache] called");

        let cache = get_cache()?;

        let all = cache
            .list_all()
            .map_err(|e| format!("list_all failed: {e:#}"))?;

        let pinned_preserved = all
            .iter()
            .filter(|f| f.user_state == UserState::Pinned)
            .count();

        // Delete all non-Pinned rows.
        let mut deleted = 0usize;
        for f in &all {
            if f.user_state != UserState::Pinned && cache.delete(&f.key).unwrap_or(false) {
                deleted += 1;
            }
        }

        tracing::info!(
            "[learning.reset_cache] deleted={deleted} pinned_preserved={pinned_preserved}"
        );

        let log = vec![format!(
            "learning.reset_cache: deleted={deleted} pinned_preserved={pinned_preserved}"
        )];
        let payload = serde_json::json!({
            "deleted": deleted,
            "pinned_preserved": pinned_preserved,
        });
        RpcOutcome::new(payload, log).into_cli_compatible_json()
    })
}
