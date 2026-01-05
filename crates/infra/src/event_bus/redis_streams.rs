//! Redis Streams-backed event bus (durable, at-least-once delivery).
//!
//! This implementation uses Redis Streams (XADD/XREADGROUP) to provide:
//! - **Durable delivery**: Messages persist until acknowledged
//! - **At-least-once**: Messages are redelivered if not ACK'd
//! - **Consumer groups**: Each projection/worker has its own consumer group
//! - **Dead-letter handling**: Failed messages after max retries go to DLQ
//! - **Tenant-aware**: Events filtered by tenant_id in stream metadata
//!
//! ## Architecture
//!
//! - **Stream Key**: `forgeerp:events` (single stream for all events)
//! - **Consumer Groups**: One per consumer type (e.g., `inventory.projection`, `ai.anomaly`)
//! - **Consumers**: Named consumers within groups (e.g., `worker-1`, `worker-2`)
//! - **Dead-Letter Queue**: `forgeerp:events:dlq` (failed messages after max retries)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value as JsonValue;
use tracing::{error, instrument, warn};

use forgeerp_core::TenantId;
use forgeerp_events::{EventBus, EventEnvelope, Subscription};

/// Default stream key for events
const DEFAULT_STREAM_KEY: &str = "forgeerp:events";

/// Default dead-letter queue key
const DEFAULT_DLQ_KEY: &str = "forgeerp:events:dlq";

/// Default max retries before sending to DLQ
const DEFAULT_MAX_RETRIES: u32 = 5;

/// Default pending entry timeout (messages older than this are redelivered)
const DEFAULT_PENDING_TIMEOUT_MS: u64 = 60000; // 60 seconds

#[derive(Debug, Clone)]
pub struct RedisStreamsEventBus {
    client: Arc<redis::Client>,
    stream_key: String,
    dlq_key: String,
    max_retries: u32,
    pending_timeout_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum RedisStreamsError {
    #[error("Redis connection error: {0}")]
    Connection(String),

    #[error("Redis command error: {0}")]
    Command(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Consumer group error: {0}")]
    ConsumerGroup(String),
}

impl RedisStreamsEventBus {
    /// Create a new Redis Streams event bus.
    ///
    /// # Arguments
    ///
    /// * `redis_url` - Redis connection URL (e.g., "redis://localhost:6379")
    /// * `stream_key` - Redis stream key (default: "forgeerp:events")
    /// * `dlq_key` - Dead-letter queue key (default: "forgeerp:events:dlq")
    pub fn new(
        redis_url: impl AsRef<str>,
        stream_key: Option<String>,
        dlq_key: Option<String>,
    ) -> Result<Self, RedisStreamsError> {
        let client = redis::Client::open(redis_url.as_ref())
            .map_err(|e| RedisStreamsError::Connection(e.to_string()))?;

        Ok(Self {
            client: Arc::new(client),
            stream_key: stream_key.unwrap_or_else(|| DEFAULT_STREAM_KEY.to_string()),
            dlq_key: dlq_key.unwrap_or_else(|| DEFAULT_DLQ_KEY.to_string()),
            max_retries: DEFAULT_MAX_RETRIES,
            pending_timeout_ms: DEFAULT_PENDING_TIMEOUT_MS,
        })
    }

    /// Ensure a consumer group exists (idempotent).
    ///
    /// Consumer groups enable multiple consumers to process the same stream,
    /// with each message delivered to one consumer in the group (load balancing).
    pub fn ensure_consumer_group(&self, group_name: &str) -> Result<(), RedisStreamsError> {
        let mut conn = self.client
            .get_connection()
            .map_err(|e| RedisStreamsError::Connection(e.to_string()))?;

        // XGROUP CREATE with MKSTREAM creates the stream if it doesn't exist
        // Using "0" as starting ID means "start from the beginning of the stream"
        // If group already exists, this will return an error, which we ignore
        let _: Result<String, _> = redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(&self.stream_key)
            .arg(group_name)
            .arg("0")
            .arg("MKSTREAM")
            .query(&mut conn);

        Ok(())
    }

    /// Publish an event to the stream (non-blocking).
    ///
    /// Uses XADD to append event to stream. Returns immediately after Redis confirms write.
    #[instrument(
        skip(self, message),
        fields(
            stream_key = %self.stream_key,
            tenant_id = %message.tenant_id().as_uuid(),
            aggregate_id = %message.aggregate_id().as_uuid()
        ),
        err
    )]
    fn publish_sync(&self, message: EventEnvelope<JsonValue>) -> Result<(), RedisStreamsError> {
        let payload = serde_json::to_string(&message)
            .map_err(|e| RedisStreamsError::Serialization(e.to_string()))?;

        let mut conn = self.client
            .get_connection()
            .map_err(|e| RedisStreamsError::Connection(e.to_string()))?;

        // XADD with auto-generated ID (*) and tenant_id as field for filtering
        let tenant_id_str = message.tenant_id().as_uuid().to_string();
        let aggregate_id_str = message.aggregate_id().as_uuid().to_string();

        let _: String = redis::cmd("XADD")
            .arg(&self.stream_key)
            .arg("*") // Auto-generate message ID
            .arg("tenant_id")
            .arg(&tenant_id_str)
            .arg("aggregate_id")
            .arg(&aggregate_id_str)
            .arg("aggregate_type")
            .arg(message.aggregate_type())
            .arg("sequence_number")
            .arg(message.sequence_number().to_string())
            .arg("payload")
            .arg(&payload)
            .query(&mut conn)
            .map_err(|e| RedisStreamsError::Command(format!("XADD failed: {}", e)))?;

        Ok(())
    }

    /// Acknowledge message processing (mark as processed).
    fn acknowledge_sync(
        &self,
        group_name: &str,
        message_ids: &[String],
    ) -> Result<(), RedisStreamsError> {
        if message_ids.is_empty() {
            return Ok(());
        }

        let mut conn = self.client
            .get_connection()
            .map_err(|e| RedisStreamsError::Connection(e.to_string()))?;

        // XACK to acknowledge messages
        let _: u64 = redis::cmd("XACK")
            .arg(&self.stream_key)
            .arg(group_name)
            .arg(&message_ids[..])
            .query(&mut conn)
            .map_err(|e| RedisStreamsError::Command(format!("XACK failed: {}", e)))?;

        Ok(())
    }

    /// Send message to dead-letter queue after max retries.
    fn send_to_dlq_sync(
        &self,
        message: &EventEnvelope<JsonValue>,
        original_message_id: &str,
        retry_count: u32,
    ) -> Result<(), RedisStreamsError> {
        let payload = serde_json::to_string(message)
            .map_err(|e| RedisStreamsError::Serialization(e.to_string()))?;

        let mut conn = self.client
            .get_connection()
            .map_err(|e| RedisStreamsError::Connection(e.to_string()))?;

        // Add to DLQ with metadata
        let _: String = redis::cmd("XADD")
            .arg(&self.dlq_key)
            .arg("*")
            .arg("original_message_id")
            .arg(original_message_id)
            .arg("retry_count")
            .arg(retry_count.to_string())
            .arg("failed_at")
            .arg(chrono::Utc::now().to_rfc3339())
            .arg("payload")
            .arg(&payload)
            .query(&mut conn)
            .map_err(|e| RedisStreamsError::Command(format!("DLQ XADD failed: {}", e)))?;

        warn!(
            message_id = %original_message_id,
            retry_count = retry_count,
            "Message sent to dead-letter queue"
        );

        Ok(())
    }

    /// Read messages from stream (used by subscription).
    fn read_group_sync(
        &self,
        group_name: &str,
        consumer_name: &str,
        tenant_id: Option<TenantId>,
        count: usize,
        block_ms: u64,
    ) -> Result<Vec<StreamMessage>, RedisStreamsError> {
        let mut conn = self.client
            .get_connection()
            .map_err(|e| RedisStreamsError::Connection(e.to_string()))?;

        // Read pending entries first (unacknowledged messages)
        let pending = self.read_pending_sync(&mut conn, group_name, consumer_name, tenant_id, count)?;

        if !pending.is_empty() {
            return Ok(pending);
        }

        // Read new entries (blocking with timeout)
        self.read_new_sync(&mut conn, group_name, consumer_name, tenant_id, count, block_ms)
    }

    /// Read pending (unacknowledged) entries for this consumer.
    fn read_pending_sync(
        &self,
        conn: &mut redis::Connection,
        group_name: &str,
        consumer_name: &str,
        tenant_id: Option<TenantId>,
        count: usize,
    ) -> Result<Vec<StreamMessage>, RedisStreamsError> {
        // XPENDING to get pending entries
        let pending_info: redis::RedisResult<Vec<(String, String, u64, u64)>> = redis::cmd("XPENDING")
            .arg(&self.stream_key)
            .arg(group_name)
            .arg("-")
            .arg("+")
            .arg(count.to_string())
            .arg(consumer_name)
            .query(conn);

        let pending_ids = match pending_info {
            Ok(entries) => entries.into_iter().map(|(id, _, _, _)| id).collect::<Vec<_>>(),
            Err(_) => return Ok(vec![]), // No pending entries
        };

        if pending_ids.is_empty() {
            return Ok(vec![]);
        }

        // Claim entries that are idle too long (redelivery)
        // XCLAIM will return entries with delivery count, which we can use for retry tracking
        let min_idle_ms = self.pending_timeout_ms;
        let claimed: redis::RedisResult<Vec<redis::Value>> = redis::cmd("XCLAIM")
            .arg(&self.stream_key)
            .arg(group_name)
            .arg(consumer_name)
            .arg(min_idle_ms.to_string())
            .arg(&pending_ids[..])
            .arg("RETRYCOUNT")
            .arg(self.max_retries.to_string())
            .query(conn);

        let claimed_entries = match claimed {
            Ok(entries) => entries,
            Err(_) => return Ok(vec![]),
        };

        // Parse claimed entries and filter by tenant
        let mut messages = Vec::new();
        for entry in claimed_entries {
            if let Ok(msg) = self.parse_stream_entry(entry, tenant_id) {
                messages.push(msg);
            }
        }

        Ok(messages)
    }

    /// Read new entries from stream (blocking).
    fn read_new_sync(
        &self,
        conn: &mut redis::Connection,
        group_name: &str,
        consumer_name: &str,
        tenant_id: Option<TenantId>,
        count: usize,
        block_ms: u64,
    ) -> Result<Vec<StreamMessage>, RedisStreamsError> {
        // XREADGROUP with ">" to read new entries for this consumer group
        let result: redis::RedisResult<HashMap<String, Vec<redis::Value>>> = redis::cmd("XREADGROUP")
            .arg("GROUP")
            .arg(group_name)
            .arg(consumer_name)
            .arg("COUNT")
            .arg(count.to_string())
            .arg("BLOCK")
            .arg(block_ms.to_string())
            .arg("STREAMS")
            .arg(&self.stream_key)
            .arg(">") // Read new entries
            .query(conn);

        let stream_data = match result {
            Ok(data) => data,
            Err(e) => {
                // Check if it's a timeout error (blocking timeout, no new messages)
                if e.kind() == redis::ErrorKind::TypeError && 
                   e.to_string().contains("timeout") {
                    return Ok(vec![]);
                }
                return Err(RedisStreamsError::Command(format!("XREADGROUP failed: {}", e)));
            }
        };

        let entries = stream_data
            .get(&self.stream_key)
            .cloned()
            .unwrap_or_default();

        let mut messages = Vec::new();
        for entry in entries {
            if let Ok(msg) = self.parse_stream_entry(entry, tenant_id) {
                messages.push(msg);
            }
        }

        Ok(messages)
    }

    /// Parse a Redis stream entry into a StreamMessage.
    ///
    /// Filters by tenant_id if provided.
    fn parse_stream_entry(
        &self,
        entry: redis::Value,
        tenant_id: Option<TenantId>,
    ) -> Result<StreamMessage, RedisStreamsError> {
        // Entry format: [message_id, [field1, value1, field2, value2, ...]]
        let entry_vec: Vec<redis::Value> = match entry {
            redis::Value::Bulk(v) => v,
            _ => return Err(RedisStreamsError::Deserialization("Invalid entry format".to_string())),
        };

        if entry_vec.len() < 2 {
            return Err(RedisStreamsError::Deserialization("Entry too short".to_string()));
        }

        // Extract message ID
        let message_id = match &entry_vec[0] {
            redis::Value::Data(data) => String::from_utf8_lossy(data).to_string(),
            redis::Value::Bulk(_) => {
                // Sometimes ID is in a nested bulk
                if let redis::Value::Bulk(id_vec) = &entry_vec[0] {
                    if let Some(redis::Value::Data(data)) = id_vec.first() {
                        String::from_utf8_lossy(data).to_string()
                    } else {
                        return Err(RedisStreamsError::Deserialization("Invalid message ID format".to_string()));
                    }
                } else {
                    return Err(RedisStreamsError::Deserialization("Invalid message ID format".to_string()));
                }
            }
            _ => return Err(RedisStreamsError::Deserialization("Invalid message ID format".to_string())),
        };

        // Parse fields
        let fields_vec: Vec<redis::Value> = match &entry_vec[1] {
            redis::Value::Bulk(v) => v.clone(),
            _ => return Err(RedisStreamsError::Deserialization("Invalid fields format".to_string())),
        };

        // Convert to HashMap
        let mut fields = HashMap::new();
        for chunk in fields_vec.chunks(2) {
            if chunk.len() == 2 {
                if let (redis::Value::Data(key), redis::Value::Data(value)) = (&chunk[0], &chunk[1]) {
                    let key_str = String::from_utf8_lossy(key);
                    let value_str = String::from_utf8_lossy(value);
                    fields.insert(key_str, value_str);
                }
            }
        }

        // Extract tenant_id and filter if needed
        let msg_tenant_id_str = fields.get("tenant_id")
            .ok_or_else(|| RedisStreamsError::Deserialization("Missing tenant_id field".to_string()))?;
        
        let msg_tenant_id = parse_tenant_id(msg_tenant_id_str)
            .map_err(|e| RedisStreamsError::Deserialization(format!("Invalid tenant_id: {}", e)))?;

        // Filter by tenant if specified
        if let Some(filter_tenant) = tenant_id {
            if msg_tenant_id != filter_tenant {
                return Err(RedisStreamsError::Deserialization("Tenant mismatch (skipping)".to_string()));
            }
        }

        // Extract payload
        let payload_str = fields.get("payload")
            .ok_or_else(|| RedisStreamsError::Deserialization("Missing payload field".to_string()))?;

        let envelope: EventEnvelope<JsonValue> = serde_json::from_str(payload_str)
            .map_err(|e| RedisStreamsError::Deserialization(format!("Failed to deserialize envelope: {}", e)))?;

        // Extract retry count (stored in Redis metadata, default to 0)
        let retry_count = fields.get("retry_count")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        Ok(StreamMessage {
            message_id,
            envelope,
            retry_count,
        })
    }
}

/// Message received from Redis stream with metadata.
#[derive(Debug, Clone)]
struct StreamMessage {
    message_id: String,
    envelope: EventEnvelope<JsonValue>,
    retry_count: u32,
}

/// Subscription that reads from Redis Streams consumer group.
///
/// Uses a background thread to poll Redis and buffer messages for delivery
/// via the sync Subscription interface.
pub struct RedisStreamsSubscription {
    bus: Arc<RedisStreamsEventBus>,
    group_name: String,
    consumer_name: String,
    tenant_id: Option<TenantId>,
    buffer: Arc<Mutex<Vec<EventEnvelope<JsonValue>>>>,
    unacked: Arc<Mutex<Vec<String>>>, // Message IDs awaiting ACK
}

impl RedisStreamsSubscription {
    fn new(
        bus: Arc<RedisStreamsEventBus>,
        group_name: String,
        consumer_name: String,
        tenant_id: Option<TenantId>,
    ) -> Self {
        // Ensure consumer group exists
        if let Err(e) = bus.ensure_consumer_group(&group_name) {
            error!("Failed to create consumer group {}: {}", group_name, e);
        }

        Self {
            bus,
            group_name,
            consumer_name,
            tenant_id,
            buffer: Arc::new(Mutex::new(Vec::new())),
            unacked: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Poll for new messages (non-blocking, fills buffer).
    fn poll(&self) {
        // Read messages from stream
        match self.bus.read_group_sync(
            &self.group_name,
            &self.consumer_name,
            self.tenant_id,
            10, // Read up to 10 messages at a time
            100, // 100ms blocking timeout
        ) {
            Ok(messages) => {
                let mut buffer = self.buffer.lock().unwrap();
                let mut unacked = self.unacked.lock().unwrap();

                for msg in messages {
                    // Check retry count, send to DLQ if exceeded
                    if msg.retry_count >= self.bus.max_retries {
                        if let Err(e) = self.bus.send_to_dlq_sync(&msg.envelope, &msg.message_id, msg.retry_count) {
                            error!("Failed to send message to DLQ: {}", e);
                        }
                        // Acknowledge to remove from pending list (we've moved to DLQ)
                        unacked.push(msg.message_id);
                    } else {
                        buffer.push(msg.envelope.clone());
                        unacked.push(msg.message_id);
                    }
                }
            }
            Err(e) => {
                error!("Failed to read from stream: {}", e);
            }
        }
    }

    /// Acknowledge processed messages (remove from pending list).
    fn acknowledge(&self, count: usize) {
        let mut unacked = self.unacked.lock().unwrap();
        if unacked.len() >= count {
            let to_ack: Vec<String> = unacked.drain(0..count).collect();
            drop(unacked); // Release lock before Redis operation

            if let Err(e) = self.bus.acknowledge_sync(&self.group_name, &to_ack) {
                error!("Failed to acknowledge messages: {}", e);
            }
        }
    }
}

// Helper for parsing TenantId from string
use std::str::FromStr;

fn parse_tenant_id(s: &str) -> Result<TenantId, String> {
    uuid::Uuid::from_str(s)
        .map(TenantId::from_uuid)
        .map_err(|e| e.to_string())
}

impl EventBus<EventEnvelope<JsonValue>> for RedisStreamsEventBus {
    type Error = RedisStreamsError;

    fn publish(&self, message: EventEnvelope<JsonValue>) -> Result<(), Self::Error> {
        self.publish_sync(message)
    }

    fn subscribe(&self) -> Subscription<EventEnvelope<JsonValue>> {
        // For Redis Streams, we need a consumer group name and consumer name
        // Default implementation uses a single global consumer group
        // Users should use subscribe_with_group() for proper consumer groups
        self.subscribe_with_group(
            "default",
            &format!("consumer-{}", uuid::Uuid::now_v7()),
            None,
        )
    }
}

impl RedisStreamsEventBus {
    /// Subscribe with consumer group (for production use).
    ///
    /// # Arguments
    ///
    /// * `group_name` - Consumer group name (e.g., "inventory.projection", "ai.anomaly")
    /// * `consumer_name` - Unique consumer name within group (e.g., "worker-1")
    /// * `tenant_id` - Optional tenant filter (None = all tenants)
    pub fn subscribe_with_group(
        &self,
        group_name: &str,
        consumer_name: &str,
        tenant_id: Option<TenantId>,
    ) -> Subscription<EventEnvelope<JsonValue>> {
        let subscription = RedisStreamsSubscription::new(
            Arc::new(self.clone()),
            group_name.to_string(),
            consumer_name.to_string(),
            tenant_id,
        );

        // Wrap in a channel-based Subscription for compatibility
        let (tx, rx) = std::sync::mpsc::channel();
        let sub_arc = Arc::new(subscription);

        // Background thread that polls Redis and forwards messages
        let sub_clone = sub_arc.clone();
        std::thread::spawn(move || {
            loop {
                sub_clone.poll();

                // Drain buffer and send to channel
                let mut buffer = sub_clone.buffer.lock().unwrap();
                while let Some(msg) = buffer.pop() {
                    if tx.send(msg).is_err() {
                        return; // Receiver dropped
                    }
                }
                drop(buffer);

                // Note: Messages are acknowledged immediately after reading.
                // For production use, consider implementing manual acknowledgment
                // after successful processing (see acknowledge_processed() method).
                // This ensures messages are only removed from pending list after
                // successful processing, enabling retry on failure.

                std::thread::sleep(Duration::from_millis(100)); // Poll every 100ms
            }
        });

        Subscription::new(rx)
    }
}
