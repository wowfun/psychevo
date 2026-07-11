#[test]
fn direct_runtime_plan_and_diff_share_the_live_assistant_timeline() {
    let mut projector = GatewayLiveProjector::default();
    let plan = projector
        .project(
            "turn-runtime",
            &RunStreamEvent::value(json!({
                "type": "runtime_plan",
                "body": "- [~] Inspect\n- [ ] Verify",
                "plan": {
                    "runtimeRef": "codex",
                    "threadId": "thread-public",
                    "turnId": "turn-runtime",
                    "steps": [
                        {"step": "Inspect", "status": "in_progress"},
                        {"step": "Verify", "status": "pending"}
                    ]
                }
            })),
        )
        .expect("plan projection");
    let plan_entry = gateway_entry(&plan);
    assert_eq!(plan_entry.blocks.len(), 1);
    assert_eq!(plan_entry.blocks[0].kind, TranscriptBlockKind::Status);
    assert_eq!(plan_entry.blocks[0].title.as_deref(), Some("Plan"));
    assert_eq!(
        plan_entry.blocks[0].body.as_deref(),
        Some("- [~] Inspect\n- [ ] Verify")
    );
    assert_eq!(
        plan_entry.blocks[0].metadata.as_ref().unwrap()["projection"],
        "runtime_plan"
    );

    let diff = projector
        .project(
            "turn-runtime",
            &RunStreamEvent::value(json!({
                "type": "runtime_diff",
                "diff": "--- a/spec.md\n+++ b/spec.md\n@@ -1 +1 @@\n-old\n+new"
            })),
        )
        .expect("diff projection");
    let diff_entry = gateway_entry(&diff);
    assert_eq!(diff_entry.id, plan_entry.id);
    assert_eq!(diff_entry.blocks.len(), 2);
    assert_eq!(diff_entry.blocks[1].kind, TranscriptBlockKind::Diff);
    assert_eq!(diff_entry.blocks[1].title.as_deref(), Some("Changes"));
    assert_eq!(
        diff_entry.blocks[1].metadata.as_ref().unwrap()["projection"],
        "runtime_diff"
    );
}
