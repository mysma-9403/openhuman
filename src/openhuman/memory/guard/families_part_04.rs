// ── The v1.13.7 typed-ingestion round + Answer ──────────────────────────────
//
// Five families the ingestion round added to the contract, wired the day the
// registry pinned the release: the guard's audit invariant (advertised ==
// reachable-through-the-guard) is what forced this file to exist alongside
// the re-pin rather than after it.

use crate::openhuman::memory::api::provider::operations::{
    AnswerRequest, AnswerResponse, MemoryAnswer, MemoryConversationIngest,
    MemoryDocumentIngest, MemoryEventIngest, MemoryLearningIngest, RawMemoryEvent,
};
use tinymemory_api::learning::LearningCandidate;

decorator!(
    /// Guarded [`MemoryDocumentIngest`].
    GuardedDocumentIngest,
    dyn MemoryDocumentIngest,
    as_document_ingest,
    DocumentIngest
);
decorator!(
    /// Guarded [`MemoryConversationIngest`].
    GuardedConversationIngest,
    dyn MemoryConversationIngest,
    as_conversation_ingest,
    ConversationIngest
);
decorator!(
    /// Guarded [`MemoryLearningIngest`].
    GuardedLearningIngest,
    dyn MemoryLearningIngest,
    as_learning_ingest,
    LearningIngest
);
decorator!(
    /// Guarded [`MemoryEventIngest`].
    GuardedEventIngest,
    dyn MemoryEventIngest,
    as_event_ingest,
    EventIngest
);
decorator!(
    /// Guarded [`MemoryAnswer`].
    GuardedAnswer,
    dyn MemoryAnswer,
    as_answer,
    Answer
);

/// Steps 3 + 4 over one ingest item, shared by the typed-ingest decorators:
/// stamp provenance, redact on egress — the same admission
/// [`GuardedIngest::admit`] applies on the legacy family.
fn admit_typed_item(policy: &GuardPolicy, mut item: IngestItem) -> IngestItem {
    item.taint = policy.stamp_taint(item.taint);
    item.content = policy.redact_outbound(&item.content).into_owned();
    item
}

#[async_trait]
impl MemoryDocumentIngest for GuardedDocumentIngest {
    async fn ingest_document(&self, document: IngestItem) -> Result<IngestOutcome, MemoryError> {
        let namespace = document.namespace.clone().unwrap_or_else(|| "-".to_string());
        self.policy.admit_write(
            Capability::DocumentIngest,
            "document_ingest.ingest_document",
            &namespace,
            true,
        )?;
        let document = admit_typed_item(&self.policy, document);
        trace_allowed(
            &self.policy,
            "document_ingest.ingest_document",
            &namespace,
            document.content.chars().count(),
        );
        self.family()?.ingest_document(document).await
    }
}

#[async_trait]
impl MemoryConversationIngest for GuardedConversationIngest {
    async fn ingest_conversation(
        &self,
        messages: Vec<IngestItem>,
    ) -> Result<IngestOutcome, MemoryError> {
        self.policy.admit_write(
            Capability::ConversationIngest,
            "conversation_ingest.ingest_conversation",
            NO_NAMESPACE,
            true,
        )?;
        let messages: Vec<IngestItem> = messages
            .into_iter()
            .map(|m| admit_typed_item(&self.policy, m))
            .collect();
        trace_allowed(
            &self.policy,
            "conversation_ingest.ingest_conversation",
            NO_NAMESPACE,
            messages.iter().map(|m| m.content.chars().count()).sum(),
        );
        self.family()?.ingest_conversation(messages).await
    }
}

#[async_trait]
impl MemoryLearningIngest for GuardedLearningIngest {
    async fn ingest_learning(
        &self,
        learning: LearningCandidate,
    ) -> Result<IngestOutcome, MemoryError> {
        // No content redaction: a learning candidate is already extracted
        // structure, not raw user text — provenance is the driver's to stamp
        // from the evidence pointer it carries.
        self.policy.admit_write(
            Capability::LearningIngest,
            "learning_ingest.ingest_learning",
            NO_NAMESPACE,
            true,
        )?;
        trace_allowed(
            &self.policy,
            "learning_ingest.ingest_learning",
            NO_NAMESPACE,
            0,
        );
        self.family()?.ingest_learning(learning).await
    }
}

#[async_trait]
impl MemoryEventIngest for GuardedEventIngest {
    async fn ingest_event(&self, event: RawMemoryEvent) -> Result<IngestOutcome, MemoryError> {
        self.policy.admit_write(
            Capability::EventIngest,
            "event_ingest.ingest_event",
            NO_NAMESPACE,
            true,
        )?;
        trace_allowed(&self.policy, "event_ingest.ingest_event", NO_NAMESPACE, 0);
        self.family()?.ingest_event(event).await
    }
}

#[async_trait]
impl MemoryAnswer for GuardedAnswer {
    async fn answer(&self, request: AnswerRequest) -> Result<AnswerResponse, MemoryError> {
        // A read-shaped family: retrieval plus synthesis, no persistence.
        self.policy
            .admit_read(Capability::Answer, "answer.answer", NO_NAMESPACE, false)?;
        trace_allowed(&self.policy, "answer.answer", NO_NAMESPACE, 0);
        self.family()?.answer(request).await
    }
}
