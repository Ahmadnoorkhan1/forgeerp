# Postgres Event Store: Command → DB Commit Flow

## Mermaid Diagram

```mermaid
sequenceDiagram
    participant Client
    participant CommandDispatcher
    participant PostgresEventStore
    participant PostgresDB as PostgreSQL
    
    Client->>CommandDispatcher: execute_command(tenant_id, cmd)
    
    Note over CommandDispatcher: 1. Load aggregate
    
    CommandDispatcher->>PostgresEventStore: load_stream(tenant_id, agg_id)
    PostgresEventStore->>PostgresDB: BEGIN (read transaction)
    PostgresEventStore->>PostgresDB: SELECT * FROM events<br/>WHERE tenant_id=? AND aggregate_id=?<br/>ORDER BY sequence_number
    PostgresDB-->>PostgresEventStore: Stream events (sequence 1..N)
    PostgresEventStore-->>CommandDispatcher: Vec<StoredEvent>
    
    Note over CommandDispatcher: 2. Rehydrate aggregate<br/>3. Handle command<br/>4. Generate events
    
    CommandDispatcher->>PostgresEventStore: append(events, ExpectedVersion::Exact(current_version))
    
    Note over PostgresEventStore: Atomic append with<br/>optimistic concurrency control
    
    PostgresEventStore->>PostgresDB: BEGIN (write transaction)
    
    PostgresEventStore->>PostgresDB: SELECT MAX(sequence_number),<br/>MAX(aggregate_type)<br/>FROM events<br/>WHERE tenant_id=? AND aggregate_id=?
    PostgresDB-->>PostgresEventStore: (current_version, aggregate_type)
    
    alt Version mismatch
        PostgresEventStore->>PostgresDB: ROLLBACK
        PostgresEventStore-->>CommandDispatcher: EventStoreError::Concurrency
        CommandDispatcher-->>Client: Conflict error
    else Version matches
        loop For each event
            PostgresEventStore->>PostgresDB: INSERT INTO events<br/>(event_id, tenant_id, aggregate_id,<br/>sequence_number, event_type, ...)<br/>VALUES (..., current_version + N, ...)
        end
        
        alt Unique constraint violation (concurrent append)
            PostgresDB-->>PostgresEventStore: Error 23505 (unique violation)
            PostgresEventStore->>PostgresDB: ROLLBACK
            PostgresEventStore-->>CommandDispatcher: EventStoreError::Concurrency
            CommandDispatcher-->>Client: Conflict error
        else Success
            PostgresEventStore->>PostgresDB: COMMIT
            PostgresDB-->>PostgresEventStore: Events persisted
            PostgresEventStore-->>CommandDispatcher: Vec<StoredEvent>
            CommandDispatcher-->>Client: Success
        end
    end
```

## Key Points

1. **Tenant Isolation**: Every query includes `tenant_id` in the WHERE clause, making cross-tenant access impossible.

2. **Optimistic Concurrency**: 
   - Version check happens within a transaction
   - Unique constraint on `(tenant_id, aggregate_id, sequence_number)` prevents concurrent inserts
   - If two transactions try to append simultaneously, one will fail with a unique constraint violation

3. **Atomicity**: All events in a batch are inserted within a single transaction (all-or-nothing).

4. **Tracing**: All database operations are instrumented with tracing spans for observability.

