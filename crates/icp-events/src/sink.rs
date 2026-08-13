use std::{
    fmt,
    sync::{Arc, Mutex},
};

use crate::event::Event;

/// The destination for the events an operation emits.
///
/// Implementations must be cheap and non-blocking: [`emit`](EventSink::emit) is
/// called from inside operations, often while other work is in flight.
pub trait EventSink: fmt::Debug + Send + Sync {
    /// Record or render a single event.
    fn emit(&self, event: Event);
}

impl<T: EventSink + ?Sized> EventSink for Arc<T> {
    fn emit(&self, event: Event) {
        (**self).emit(event);
    }
}

/// A sink that throws every event away.
///
/// Useful for code paths that should stay silent, and as a default in tests that
/// do not care about output.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiscardSink;

impl EventSink for DiscardSink {
    fn emit(&self, _event: Event) {}
}

/// A sink that keeps every event it is given, in order.
///
/// This is what makes operations unit-testable: run the operation against a
/// `RecordingSink` and assert on the resulting `Vec<Event>`.
#[derive(Debug, Default)]
pub struct RecordingSink {
    events: Mutex<Vec<Event>>,
}

impl RecordingSink {
    /// Create an empty recorder.
    pub fn new() -> Self {
        Self::default()
    }

    /// A snapshot of everything recorded so far, in emission order.
    pub fn events(&self) -> Vec<Event> {
        self.events.lock().expect("recording sink poisoned").clone()
    }

    /// Drain everything recorded so far, leaving the recorder empty.
    pub fn take(&self) -> Vec<Event> {
        std::mem::take(&mut *self.events.lock().expect("recording sink poisoned"))
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, event: Event) {
        self.events
            .lock()
            .expect("recording sink poisoned")
            .push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{NoticeLevel, TaskId};

    fn notice(message: &str) -> Event {
        Event::Notice {
            level: NoticeLevel::Info,
            message: message.into(),
        }
    }

    #[test]
    fn recording_sink_preserves_order() {
        let sink = RecordingSink::new();
        sink.emit(notice("one"));
        sink.emit(notice("two"));

        assert_eq!(sink.events(), vec![notice("one"), notice("two")]);
    }

    #[test]
    fn take_drains_the_recorder() {
        let sink = RecordingSink::new();
        sink.emit(notice("one"));

        assert_eq!(sink.take(), vec![notice("one")]);
        assert!(sink.events().is_empty());
    }

    #[test]
    fn discard_sink_keeps_nothing() {
        // Nothing to assert beyond it not panicking; it exists so silent code paths
        // do not need an `Option<Reporter>`.
        DiscardSink.emit(notice("dropped"));
    }

    #[test]
    fn arc_forwards_to_the_inner_sink() {
        let sink = Arc::new(RecordingSink::new());
        EventSink::emit(&sink, notice("through the arc"));

        assert_eq!(sink.events(), vec![notice("through the arc")]);
    }

    #[test]
    fn sinks_are_usable_from_several_threads() {
        let sink = Arc::new(RecordingSink::new());
        let threads: Vec<_> = (0..4)
            .map(|i| {
                let sink = sink.clone();
                std::thread::spawn(move || {
                    sink.emit(Event::TaskMessage {
                        id: TaskId(i),
                        message: format!("from {i}"),
                    })
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }

        assert_eq!(sink.events().len(), 4);
    }
}
