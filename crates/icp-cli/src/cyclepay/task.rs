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
            label: format!("order {}", order_number(&self.order_id)),
        }
    }

    fn running_message(&self) -> Option<&'static str> {
        Some(match self.phase {
            // With its `[order co-<n>] ✷ ` label this is 66 columns plus the
            // digits of the order's number, so within 80 up to 14 of them.
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

/// `co-1234` for CyclePay's `co-1234-<32 hex>`: the order's number, which
/// no other order shares, without the random part, which only paying the
/// order needs. It keeps the live line within 80 columns whatever the id's
/// length; the whole id is printed with the link, in the lines off a
/// terminal, and in the JSON event. An id of any other shape is shown whole.
fn order_number(order_id: &str) -> &str {
    match order_id.match_indices('-').nth(1) {
        Some((at, _)) => &order_id[..at],
        None => order_id,
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

    /// CyclePay's ids are `co-<n>-<32 hex>`, 37 to 41 characters today: the
    /// live line names the order by its number, and fits 80 columns.
    #[test]
    fn a_purchase_is_labelled_by_its_order_number() {
        let label =
            |id: &str| match CyclesPurchaseTask::new(id, PurchasePhase::AwaitingPayment).widget() {
                Widget::Indeterminate { label } => label,
                _ => panic!("a purchase is an indeterminate task"),
            };
        let id = "co-1234-0123456789abcdef0123456789abcdef";
        assert_eq!(label(id), "order co-1234");
        assert_eq!(label("co-6-5e64b574"), "order co-6");
        assert_eq!(label("o1"), "order o1");

        let task = CyclesPurchaseTask::new(id, PurchasePhase::AwaitingPayment);
        let line = format!("[{}] ✷ {}", label(id), task.running_message().unwrap());
        assert!(
            line.chars().count() <= 80,
            "{} columns: {line}",
            line.chars().count()
        );
        // The event still carries the whole id.
        assert_eq!(
            serde_json::to_value(&task).unwrap()["order_id"],
            serde_json::json!(id)
        );
    }

    #[test]
    fn wire_format_is_stable() {
        assert_eq!(
            serde_json::to_value(CyclesPurchaseTask::new("o1", PurchasePhase::Delivering)).unwrap(),
            serde_json::json!({ "kind": "cycles_purchase", "order_id": "o1", "phase": "delivering" })
        );
    }
}
