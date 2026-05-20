//! Message-passing types shared across game subsystems.
//!
//! Mirrors `EnumMessageType`, `MessageDispatcherImpl`, and associated payload
//! types from the Java reference implementation (`openjill-core-api`).

use std::collections::HashMap;

use crate::runtime::InventoryObject;

/// Message types that cross subsystem boundaries.
///
/// Mirrors `EnumMessageType` from the Java reference implementation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageType {
    /// Player movement notification.
    PlayerMove,
    /// Request for the player's current position.
    PlayerGetPosition,
    /// Inventory item pickup or removal.
    InventoryItem,
    /// Life count change.
    InventoryLife,
    /// Score point change.
    InventoryPoint,
    /// Text for the status bar overlay.
    StatusBarText,
    /// Generic object event.
    Object,
    /// Replace an existing object in the active scene.
    ReplaceObject,
    /// Create a new object in the active scene.
    CreateObject,
    /// Trigger activation.
    Trigger,
    /// Background layer event.
    Background,
    /// Level-change checkpoint; transition forward to a new level.
    CheckpointChangeLevel,
    /// Level-change checkpoint; transition backward to the previous level.
    CheckpointChangeLevelPrevious,
    /// Player died; restart the current level from the beginning.
    DieRestartLevel,
    /// Display a message box overlay.
    MessageBox,
    /// Change the active player character.
    ChangePlayerCharacter,
}

/// Payload for a `CheckpointChangeLevel` or `CheckpointChangeLevelPrevious` message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeLevelPayload {
    /// JN filename for the destination level, e.g. `"JN1L03.JN1"`.
    pub level_file: String,
    /// Destination level index; `MAP_LEVEL` (`-1`) for the world-map screen.
    pub level_number: i32,
}

/// Data attached to one dispatched message.
///
/// Each variant corresponds to the payload type expected by its [`MessageType`].
/// Variants map as follows:
///
/// | Variant | Used by |
/// |---------|---------|
/// | `ChangeLevel` | `CheckpointChangeLevel`, `CheckpointChangeLevelPrevious` |
/// | `InventoryItem` | `InventoryItem` |
/// | `Count` | `InventoryLife`, `InventoryPoint` |
/// | `Text` | `StatusBarText`, `MessageBox` |
/// | `None` | `DieRestartLevel` and other stateless messages |
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessagePayload {
    /// Level-change data.
    ChangeLevel(ChangeLevelPayload),
    /// Inventory item carried in the message.
    InventoryItem(InventoryObject),
    /// Integer count (lives delta, score delta, etc.).
    Count(i32),
    /// Text string (status bar text, message box content).
    Text(String),
    /// No payload; used by stateless messages such as `DieRestartLevel`.
    None,
}

/// Receiver registered with [`MessageDispatcher`] for one message type.
pub trait MessageHandler: Send {
    /// Process one message of the subscribed type.
    fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload);
}

/// Routes messages to registered handlers; queues messages sent before any handler exists.
///
/// Matches `MessageDispatcherImpl` behavior from the Java reference implementation:
/// a message sent before any handler is registered for its type is enqueued and
/// delivered to the first handler that subscribes for that type.
pub struct MessageDispatcher {
    /// Registered handlers keyed by message type.
    subscribers: HashMap<MessageType, Vec<Box<dyn MessageHandler>>>,
    /// Messages queued because no handler existed at send time, keyed by type.
    pending: HashMap<MessageType, Vec<MessagePayload>>,
}

impl MessageDispatcher {
    /// Creates a dispatcher with no subscribers and an empty pending queue.
    pub fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    /// Registers `handler` for `msg_type`.
    ///
    /// Any messages that were queued before this subscription are delivered
    /// immediately to all currently registered handlers (including the new one),
    /// in the order they were sent.
    pub fn subscribe(&mut self, msg_type: MessageType, handler: Box<dyn MessageHandler>) {
        self.subscribers.entry(msg_type).or_default().push(handler);
        if let Some(queued) = self.pending.remove(&msg_type) {
            for payload in queued {
                for h in self.subscribers.get_mut(&msg_type).unwrap() {
                    h.handle(msg_type, &payload);
                }
            }
        }
    }

    /// Dispatches a message to all handlers registered for `msg_type`.
    ///
    /// When no handler is registered the message is enqueued and will be
    /// delivered to the first handler that subscribes for that type.
    pub fn send(&mut self, msg_type: MessageType, payload: MessagePayload) {
        match self.subscribers.get_mut(&msg_type) {
            Some(handlers) if !handlers.is_empty() => {
                for h in handlers {
                    h.handle(msg_type, &payload);
                }
            }
            _ => {
                self.pending.entry(msg_type).or_default().push(payload);
            }
        }
    }

    /// Removes all subscribers and discards all pending queued messages.
    pub fn clear(&mut self) {
        self.subscribers.clear();
        self.pending.clear();
    }
}

impl Default for MessageDispatcher {
    /// Returns a dispatcher with no subscribers and an empty pending queue.
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{
        ChangeLevelPayload, MessageDispatcher, MessageHandler, MessagePayload, MessageType,
    };

    /// Shared recording buffer used by `RecordingHandler` to capture received messages.
    type RecordingBuffer = Arc<Mutex<Vec<(MessageType, MessagePayload)>>>;

    /// Test helper: records each delivered message into a shared buffer.
    ///
    /// The `Arc<Mutex<...>>` allows the test to inspect recorded messages after
    /// the handler has been moved into the dispatcher.
    struct RecordingHandler(RecordingBuffer);

    impl RecordingHandler {
        /// Creates a new handler and returns it together with a handle to its buffer.
        fn new() -> (Self, RecordingBuffer) {
            let buf: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
            (Self(Arc::clone(&buf)), buf)
        }
    }

    impl MessageHandler for RecordingHandler {
        /// Appends the received message to the shared buffer.
        fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
            self.0.lock().unwrap().push((msg_type, payload.clone()));
        }
    }

    /// Unit under test: `MessageDispatcher::send` before any `subscribe`.
    ///
    /// Preconditions: a dispatcher has no subscribers; a message is sent.
    ///
    /// Invariants asserted: the message is not delivered immediately (no handler
    /// exists to receive it), confirmed by subscribing afterwards and seeing it
    /// arrive then rather than at send time.
    #[test]
    fn send_before_subscribe_queues_message() {
        let mut dispatcher = MessageDispatcher::new();
        let payload = MessagePayload::Count(42);
        dispatcher.send(MessageType::InventoryPoint, payload.clone());

        // Nothing delivered yet - no handlers registered.
        let (handler, buf) = RecordingHandler::new();
        assert!(
            buf.lock().unwrap().is_empty(),
            "message must not arrive before subscribe"
        );

        dispatcher.subscribe(MessageType::InventoryPoint, Box::new(handler));
        let received = buf.lock().unwrap();
        assert_eq!(
            received.len(),
            1,
            "queued message must be delivered on subscribe"
        );
        assert_eq!(received[0], (MessageType::InventoryPoint, payload));
    }

    /// Unit under test: `MessageDispatcher::subscribe` flushes multiple queued messages.
    ///
    /// Preconditions: two messages are sent before any handler is registered;
    /// a recording handler is then subscribed; a second handler is subscribed after.
    ///
    /// Invariants asserted: both queued messages are delivered to the first handler
    /// in send order; the second handler receives nothing retroactively because the
    /// queue was already drained.
    #[test]
    fn subscribe_flushes_pending_queue_in_send_order() {
        let mut dispatcher = MessageDispatcher::new();

        let payload_a = MessagePayload::Count(1);
        let payload_b = MessagePayload::Count(2);
        dispatcher.send(MessageType::InventoryPoint, payload_a.clone());
        dispatcher.send(MessageType::InventoryPoint, payload_b.clone());

        let (handler_a, buf_a) = RecordingHandler::new();
        dispatcher.subscribe(MessageType::InventoryPoint, Box::new(handler_a));

        {
            let received = buf_a.lock().unwrap();
            assert_eq!(received.len(), 2);
            assert_eq!(received[0], (MessageType::InventoryPoint, payload_a));
            assert_eq!(received[1], (MessageType::InventoryPoint, payload_b));
        }

        let (handler_b, buf_b) = RecordingHandler::new();
        dispatcher.subscribe(MessageType::InventoryPoint, Box::new(handler_b));
        assert!(
            buf_b.lock().unwrap().is_empty(),
            "second subscriber must not receive messages queued before its registration"
        );
    }

    /// Unit under test: `MessageDispatcher::send` with an active subscriber.
    ///
    /// Preconditions: a recording handler is registered before any message is sent.
    ///
    /// Invariants asserted: the message is delivered immediately on `send`.
    #[test]
    fn send_with_subscriber_delivers_immediately() {
        let mut dispatcher = MessageDispatcher::new();
        let (handler, buf) = RecordingHandler::new();
        dispatcher.subscribe(MessageType::InventoryLife, Box::new(handler));

        let payload = MessagePayload::Count(3);
        dispatcher.send(MessageType::InventoryLife, payload.clone());

        let received = buf.lock().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0], (MessageType::InventoryLife, payload));
    }

    /// Unit under test: multiple handlers registered for the same type.
    ///
    /// Preconditions: two recording handlers are subscribed to the same message
    /// type before a message is sent.
    ///
    /// Invariants asserted: both handlers receive the message.
    #[test]
    fn multiple_subscribers_all_receive_message() {
        let mut dispatcher = MessageDispatcher::new();

        let (handler_a, buf_a) = RecordingHandler::new();
        dispatcher.subscribe(MessageType::StatusBarText, Box::new(handler_a));

        let (handler_b, buf_b) = RecordingHandler::new();
        dispatcher.subscribe(MessageType::StatusBarText, Box::new(handler_b));

        let payload = MessagePayload::Text(String::from("level 1"));
        dispatcher.send(MessageType::StatusBarText, payload.clone());

        assert_eq!(buf_a.lock().unwrap().len(), 1);
        assert_eq!(buf_b.lock().unwrap().len(), 1);
        assert_eq!(
            buf_a.lock().unwrap()[0],
            (MessageType::StatusBarText, payload.clone())
        );
        assert_eq!(
            buf_b.lock().unwrap()[0],
            (MessageType::StatusBarText, payload)
        );
    }

    /// Unit under test: `MessageDispatcher::clear`.
    ///
    /// Preconditions: a handler is registered and a pending message exists for a
    /// different type; `clear` is called.
    ///
    /// Invariants asserted: after `clear`, a handler subscribed for the previously
    /// queued type receives nothing (queue discarded), and the previously registered
    /// handler receives no further messages (subscriber removed).
    #[test]
    fn clear_removes_all_subscribers_and_pending_messages() {
        let mut dispatcher = MessageDispatcher::new();

        // Enqueue a message for a type with no subscriber yet.
        dispatcher.send(
            MessageType::CheckpointChangeLevel,
            MessagePayload::ChangeLevel(ChangeLevelPayload {
                level_file: String::from("JN1L01.JN1"),
                level_number: 1,
            }),
        );

        // Register a handler for a different type.
        let (live_handler, live_buf) = RecordingHandler::new();
        dispatcher.subscribe(MessageType::DieRestartLevel, Box::new(live_handler));

        dispatcher.clear();

        // Subscribe for the type that had a queued message; must receive nothing.
        let (post_handler, post_buf) = RecordingHandler::new();
        dispatcher.subscribe(MessageType::CheckpointChangeLevel, Box::new(post_handler));
        assert!(
            post_buf.lock().unwrap().is_empty(),
            "queued messages before clear must be discarded"
        );

        // Send to the type that had a live handler before clear; must not arrive.
        dispatcher.send(MessageType::DieRestartLevel, MessagePayload::None);
        assert!(
            live_buf.lock().unwrap().is_empty(),
            "cleared subscriber must not receive post-clear messages"
        );
    }
}
