//! The wait on an order, as [`icp_events`] tasks: one per leg, so a consumer
//! can tick off "payment received" before "delivered".
//!
//! A purchase is this crate's own operation, not one of `icp-project`'s, so
//! its task is a type of its own rather than a kind of
//! [`icp_project::operations::task::Task`]: the renderer draws any payload
//! that is [`Presented`].

use icp_project::operations::task::{Presentation, Widget};
use serde::Serialize;

use crate::render::Presented;

/// Which leg of a card purchase of cycles is in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PurchasePhase {
    /// The order is payable and the buyer has not paid yet.
    AwaitingPayment,
    /// The card was charged; the gateway is sending the cycles.
    Delivering,
}

/// One leg of waiting on a cycles-gateway order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename = "cycles_purchase")]
pub(crate) struct CyclesPurchaseTask {
    pub order_id: String,
    pub phase: PurchasePhase,
}

impl CyclesPurchaseTask {
    pub(crate) fn new(order_id: impl Into<String>, phase: PurchasePhase) -> Self {
        Self {
            order_id: order_id.into(),
            phase,
        }
    }
}

/// A reporter of the wait's tasks.
pub(crate) type Reporter = icp_events::Reporter<CyclesPurchaseTask>;
/// One leg of the wait, under way.
pub(crate) type TaskReporter = icp_events::TaskReporter<CyclesPurchaseTask>;

impl Presentation for CyclesPurchaseTask {
    /// An order is not about a canister; the cycles go to an identity.
    fn canister(&self) -> Option<&str> {
        None
    }

    fn widget(&self) -> Widget {
        Widget::Indeterminate {
            label: format!("order {}", self.order_id),
        }
    }

    fn running_message(&self) -> Option<&'static str> {
        Some(match self.phase {
            // With its `[order <id>] ✷ ` label this is 63 columns plus the
            // order id, so within 80 for ids up to 17 characters: CyclePay's
            // `co-<n>-<8 hex>` up to order 99999.
            PurchasePhase::AwaitingPayment => {
                "Waiting for payment; Ctrl-C leaves the order payable"
            }
            PurchasePhase::Delivering => "Payment received; delivering cycles...",
        })
    }

    fn success_message(&self) -> Option<String> {
        Some(
            match self.phase {
                PurchasePhase::AwaitingPayment => "Payment received",
                PurchasePhase::Delivering => "Cycles delivered",
            }
            .to_owned(),
        )
    }

    fn failure_message(&self, message: &str) -> Option<String> {
        Some(message.to_owned())
    }
}

impl Presented for CyclesPurchaseTask {
    fn presentation(&self) -> &dyn Presentation {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_format_is_stable() {
        assert_eq!(
            serde_json::to_value(CyclesPurchaseTask::new("o1", PurchasePhase::Delivering)).unwrap(),
            serde_json::json!({ "kind": "cycles_purchase", "order_id": "o1", "phase": "delivering" })
        );
    }
}
