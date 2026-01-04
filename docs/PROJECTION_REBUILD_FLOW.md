# Projection Rebuild Flow

## Mermaid Diagram

```mermaid
sequenceDiagram
    participant Admin
    participant ProjectionWorker
    participant EventStore as PostgresEventStore
    participant PostgresDB as PostgreSQL
    participant Projection
    participant ReadModelStore as PostgresInventoryStore
    participant CursorStore as PostgresCursorStore
    
    Note over Admin: Schema change or bug fix requires rebuild
    
    Admin->>ProjectionWorker: Trigger rebuild for tenant
    
    Note over ProjectionWorker: 1. Load all events for tenant
    
    ProjectionWorker->>EventStore: load_all_events(tenant_id)
    EventStore->>PostgresDB: SELECT * FROM events<br/>WHERE tenant_id=?<br/>ORDER BY tenant_id, aggregate_id, sequence_number
    PostgresDB-->>EventStore: [EventEnvelope1, EventEnvelope2, ...]
    EventStore-->>ProjectionWorker: Vec<EventEnvelope>
    
    Note over ProjectionWorker: 2. Clear existing read models and cursors
    
    ProjectionWorker->>ReadModelStore: clear_tenant(tenant_id)
    ReadModelStore->>PostgresDB: SELECT clear_tenant_read_models(?)
    PostgresDB-->>ReadModelStore: DELETE FROM inventory_stock WHERE tenant_id=?
    
    ProjectionWorker->>CursorStore: clear_cursors(tenant_id, "inventory.stock")
    CursorStore->>PostgresDB: SELECT clear_tenant_offsets(?, ?)
    PostgresDB-->>CursorStore: DELETE FROM projection_offsets WHERE tenant_id=? AND projection_name=?
    
    Note over ProjectionWorker: 3. Replay events deterministically
    
    ProjectionWorker->>ProjectionWorker: Sort events by:<br/>(tenant_id, aggregate_id, sequence_number)
    
    loop For each event (in sequence order)
        ProjectionWorker->>Projection: apply_envelope(event)
        
        Note over Projection: Check cursor (from CursorStore or memory)
        
        Projection->>CursorStore: get_cursor(tenant_id, aggregate_id, "inventory.stock")
        CursorStore->>PostgresDB: SELECT last_sequence_number<br/>FROM projection_offsets<br/>WHERE tenant_id=? AND aggregate_id=? AND projection_name=?
        PostgresDB-->>CursorStore: last_sequence_number (or NULL)
        CursorStore-->>Projection: Option<u64>
        
        alt Event already processed (seq <= cursor)
            Projection-->>ProjectionWorker: Skip (idempotent)
        else Event not processed yet
            Note over Projection: Deserialize event, validate tenant isolation
            
            Projection->>Projection: Apply business logic<br/>(update read model state)
            
            Projection->>ReadModelStore: upsert(tenant_id, item_id, read_model)
            ReadModelStore->>PostgresDB: INSERT INTO inventory_stock<br/>ON CONFLICT DO UPDATE
            PostgresDB-->>ReadModelStore: Read model persisted
            
            Projection->>CursorStore: update_cursor(tenant_id, aggregate_id, sequence_number)
            CursorStore->>PostgresDB: INSERT INTO projection_offsets<br/>ON CONFLICT DO UPDATE<br/>SET last_sequence_number=?
            PostgresDB-->>CursorStore: Cursor persisted
            
            Projection-->>ProjectionWorker: Event processed
        end
    end
    
    ProjectionWorker-->>Admin: Rebuild complete
    
    Note over ReadModelStore,CursorStore: Read model and cursors now match event stream
```

## Key Points

1. **Deterministic Rebuilds**: Events are sorted by `(tenant_id, aggregate_id, sequence_number)` before replay, ensuring deterministic ordering regardless of event store query order.

2. **Drop-and-Rebuild Safe**: The rebuild process:
   - Clears all read model data for the tenant
   - Clears all projection offsets for the tenant
   - Replays all events from scratch
   - Rebuilds read models and cursors atomically per event

3. **Resume After Crash**: Cursors are persisted to `projection_offsets` table. If the projection crashes:
   - On restart, it can query `projection_offsets` to find the last processed sequence_number
   - It can resume from the next event instead of replaying everything

4. **Idempotent Processing**: Events with `sequence_number <= cursor` are ignored, ensuring idempotent processing even if events are replayed.

5. **Tenant Isolation**: All queries include `tenant_id` in WHERE clauses, ensuring cross-tenant access is impossible.

## Rebuild vs Resume

- **Rebuild**: Clear everything and replay all events (deterministic, used for schema changes/bug fixes)
- **Resume**: Start from last cursor position (efficient, used after crashes/restarts)

Both operations use the same `apply_envelope` method, which automatically handles cursor checks and idempotency.

