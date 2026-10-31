use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use keel_core::{
    CancelToken, Event, InteractionPolicy, InteractionPort, InteractionRequest,
    InteractionResponse, OnTimeout,
};

use crate::event_log::EventLog;

/// Host port for headless runs: every interaction is deferred immediately.
pub struct DeferAllInteractionPort;

#[async_trait]
impl InteractionPort for DeferAllInteractionPort {
    async fn request(
        &self,
        _request: InteractionRequest,
        _cancel: &CancelToken,
    ) -> InteractionResponse {
        InteractionResponse::Deferred
    }
}

/// Applies the interaction policy, enforces the deadline and writes audit
/// events. Hosts only implement [`InteractionPort`].
pub struct InteractionBroker {
    port: Arc<dyn InteractionPort>,
    policy: InteractionPolicy,
    events: Arc<EventLog>,
}

impl InteractionBroker {
    pub fn new(
        port: Arc<dyn InteractionPort>,
        policy: InteractionPolicy,
        events: Arc<EventLog>,
    ) -> Self {
        Self {
            port,
            policy,
            events,
        }
    }

    pub fn policy(&self) -> InteractionPolicy {
        self.policy
    }

    pub async fn request(
        &self,
        request: InteractionRequest,
        cancel: &CancelToken,
    ) -> InteractionResponse {
        self.events.append(Event::InteractionRequested {
            turn_id: request.turn_id,
            interaction_id: request.id,
            kind: request.kind.as_str().to_owned(),
        });

        let timeout = request
            .timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(self.policy.timeout);

        let response = match self.policy.on_timeout {
            OnTimeout::Block => {
                tokio::select! {
                    _ = cancel.cancelled() => InteractionResponse::Cancelled,
                    response = self.port.request(request.clone(), cancel) => response,
                }
            }
            _ => {
                tokio::select! {
                    _ = cancel.cancelled() => InteractionResponse::Cancelled,
                    result = tokio::time::timeout(timeout, self.port.request(request.clone(), cancel)) => {
                        match result {
                            Ok(response) => response,
                            Err(_) => timeout_response(self.policy.on_timeout),
                        }
                    }
                }
            }
        };

        self.events.append(Event::InteractionResolved {
            turn_id: request.turn_id,
            interaction_id: request.id,
            outcome: response.outcome(),
        });
        response
    }
}

fn timeout_response(on_timeout: OnTimeout) -> InteractionResponse {
    match on_timeout {
        OnTimeout::Fail => InteractionResponse::TimedOut,
        OnTimeout::Block | OnTimeout::DeferAndContinue | OnTimeout::EndTurn => {
            InteractionResponse::Deferred
        }
    }
}
