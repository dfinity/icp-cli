use std::collections::HashMap;

use tracing::{
    Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, layer};

/// A visitor that collects field-value pairs from tracing events and spans
/// into a HashMap where both keys and values are strings.
#[derive(Default)]
pub struct FieldCollector(HashMap<String, String>);

impl Visit for FieldCollector {
    /// Records a field and its debug representation as a string.
    /// This method is called for each field when visiting tracing data.
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(
            field.name().to_owned(), // key: the field name as a string
            format!("{value:?}"),    // value: the debug representation of the field value
        );
    }
}

/// A tracing layer that processes events by collecting their fields
/// and printing them to stdout.
pub struct EventLayer;

impl<S: Subscriber> Layer<S> for EventLayer {
    /// Called when a tracing event occurs. Collects all fields from the event
    /// and prints them as a debug representation of the HashMap.
    fn on_event(&self, event: &tracing::Event<'_>, _: layer::Context<'_, S>) {
        // Create a new field collector to gather event data
        let mut v = FieldCollector::default();

        // Visit all fields in the event and collect them
        event.record(&mut v);

        // Output the collected fields to stdout
        println!("{:?}", v.0);
    }
}
