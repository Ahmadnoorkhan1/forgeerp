//! Redis pub/sub-backed event bus (optional).
//!
//! Note: Redis pub/sub is not durable (messages can be dropped if subscribers
//! are offline). For durable at-least-once processing, Redis Streams (or a
//! broker) would be used later. This implementation is intentionally minimal.

use std::sync::mpsc;
use std::thread;

use redis::Commands;
use serde_json::Value as JsonValue;

use forgeerp_events::{EventBus, EventEnvelope, Subscription};

#[derive(Debug)]
pub enum RedisBusError {
    Redis(String),
    Serialize(String),
}

/// Redis pub/sub bus for JSON event envelopes.
#[derive(Debug, Clone)]
pub struct RedisPubSubEventBus {
    client: redis::Client,
    channel: String,
}

impl RedisPubSubEventBus {
    pub fn new(redis_url: impl AsRef<str>, channel: impl Into<String>) -> Result<Self, RedisBusError> {
        let client = redis::Client::open(redis_url.as_ref())
            .map_err(|e| RedisBusError::Redis(e.to_string()))?;
        Ok(Self {
            client,
            channel: channel.into(),
        })
    }
}

impl EventBus<EventEnvelope<JsonValue>> for RedisPubSubEventBus {
    type Error = RedisBusError;

    fn publish(&self, message: EventEnvelope<JsonValue>) -> Result<(), Self::Error> {
        let payload = serde_json::to_string(&message)
            .map_err(|e| RedisBusError::Serialize(e.to_string()))?;

        let mut conn = self
            .client
            .get_connection()
            .map_err(|e| RedisBusError::Redis(e.to_string()))?;

        let _: i64 = conn
            .publish(&self.channel, payload)
            .map_err(|e| RedisBusError::Redis(e.to_string()))?;

        Ok(())
    }

    fn subscribe(&self) -> Subscription<EventEnvelope<JsonValue>> {
        let (tx, rx) = mpsc::channel();

        let client = self.client.clone();
        let channel = self.channel.clone();

        // Background thread that receives pub/sub messages and forwards them.
        thread::spawn(move || {
            let mut conn = match client.get_connection() {
                Ok(c) => c,
                Err(_) => return,
            };

            let mut pubsub = conn.as_pubsub();
            if pubsub.subscribe(channel).is_err() {
                return;
            }

            loop {
                let msg = match pubsub.get_message() {
                    Ok(m) => m,
                    Err(_) => return,
                };

                let payload: String = match msg.get_payload() {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                let envelope: EventEnvelope<JsonValue> = match serde_json::from_str(&payload) {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                if tx.send(envelope).is_err() {
                    return;
                }
            }
        });

        Subscription::new(rx)
    }
}


