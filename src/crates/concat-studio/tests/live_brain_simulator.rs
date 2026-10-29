// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deterministic P1 mock-path integration coverage.
use concat_studio::live_brain::{
    CommentSimulator, LiveBrain, LiveFeatureFlags, ProductSession, ProductSnapshot,
    simulated_comment,
};

#[test]
fn vietnamese_comment_flows_through_facts_queue_and_fake_speech() {
    let mut products = ProductSession::default();
    products
        .upsert(ProductSnapshot {
            schema_version: 1,
            tenant_id: "demo-tenant".into(),
            store_id: "demo-store".into(),
            sku: "DEMO-01".into(),
            revision: 7,
            currency: "VND".into(),
            title: "Xoài sấy".into(),
            price_vnd: Some(89_000),
            stock_state: "available".into(),
            observed_at: "2026-10-27T00:00:00Z".into(),
            valid_until: "2099-01-01T00:00:00Z".into(),
            verification_source: "mock".into(),
            ..Default::default()
        })
        .unwrap();
    products.feature("DEMO-01").unwrap();
    let mut simulator = CommentSimulator::new(LiveBrain { products });
    assert!(!LiveFeatureFlags::default().live_enabled);

    let accepted = simulator
        .ingest(
            simulated_comment("comment-1", "Shop ơi, xoài sấy giá bao nhiêu?"),
            1_000,
            "2026-10-27T00:00:01Z",
            4,
        )
        .unwrap();
    assert!(accepted);
    let task = simulator
        .dispatch_next("2026-10-27T00:00:02Z", 4)
        .unwrap()
        .unwrap();
    assert_eq!(task.expected_session_revision, 4);
    assert!(task.speech.text.contains("89.000đ"));
    assert_eq!(task.product_ref.as_ref().unwrap().revision, 7);
    assert_eq!(simulator.speech.submitted, [task.task_id]);
}

#[test]
fn simulator_never_calls_platform_or_live_engine() {
    let mut simulator = CommentSimulator::new(LiveBrain {
        products: ProductSession::default(),
    });
    assert!(
        simulator
            .ingest(
                simulated_comment("comment-2", "Chào shop!"),
                0,
                "2026-10-27T00:00:00Z",
                0,
            )
            .unwrap()
    );
    let task = simulator
        .dispatch_next("2026-10-27T00:00:01Z", 0)
        .unwrap()
        .unwrap();
    assert_eq!(task.motion.gesture, "smile");
    assert_eq!(simulator.speech.submitted.len(), 1);
}
