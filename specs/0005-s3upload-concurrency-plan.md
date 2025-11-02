# S3 Upload Concurrency Plan

## Current State

The current implementation processes files **sequentially**:
- One file at a time in upload mode
- One file at a time in URL-only mode
- No parallelization or concurrent operations

This is inefficient for:
- Multiple small files
- Network I/O bound operations
- High-latency connections to S3

## Proposed Concurrency Strategy (MPSC Channels)

### 1. Concurrent Upload Architecture

```
┌─────────────────┐
│  Collect Files  │
└────────┬────────┘
         │
         ▼
┌──────────────────────────────────┐
│  Producer: Send files to channel │
│  (Bounded channel)                │
└────────┬─────────────────────────┘
         │
         │  mpsc::channel(capacity)
         │
    ┌────┴────────────────┐
    │                     │
┌───▼────┐  ┌──────────┐  ┌──────────┐
│Worker 1│  │Worker 2  │  │Worker N  │
│  Task  │  │  Task    │  │  Task    │
└───┬────┘  └────┬─────┘  └────┬─────┘
    │            │             │
    │  Compare → Upload → URL  │
    │            │             │
    └────────┬───┴─────────────┘
             │
             ▼
    ┌────────────────┐
    │  Collect Stats │
    │ Display Summary│
    └────────────────┘
```

**Key Components:**
1. **Producer**: Main thread sends file paths to channel
2. **Channel**: Bounded mpsc with capacity = max_concurrent
3. **Worker Tasks**: N workers receive from channel, process files
4. **Stats Collector**: Thread-safe stats using Arc<AtomicUsize>

### 2. Concurrency Control with MPSC Channels

**Use tokio::sync::mpsc for Work Distribution:**
- Bounded channel naturally limits concurrent operations
- Channel capacity = max_concurrent workers
- Backpressure: Producer blocks when channel is full
- Clean shutdown when channel is closed

**Benefits over Semaphore:**
- More explicit work distribution pattern
- Better separation of concerns (producer/consumer)
- Natural backpressure mechanism
- Easier to extend (e.g., priority queues, work stealing)

**Suggested Defaults:**
- Small files (< 10MB): 8 workers
- Large files (> 100MB): 4 workers
- Default: 4 workers (safe for most use cases)

### 3. Implementation Approach with MPSC Channels

**Worker Pool Pattern with mpsc::channel:**

```rust
use tokio::sync::mpsc;
use std::sync::Arc;

// Create bounded channel
let (tx, mut rx) = mpsc::channel(max_concurrent);

// Spawn worker tasks
let mut workers = Vec::new();
for worker_id in 0..max_concurrent {
    let mut rx = rx.clone(); // Clone receiver for this worker
    let s3_client = s3_client.clone();
    let stats = Arc::clone(&stats);
    let multi = Arc::clone(&multi);

    workers.push(tokio::spawn(async move {
        while let Some(file_path) = rx.recv().await {
            process_upload(&s3_client, &file_path, &stats, &multi).await;
        }
    }));
}
drop(rx); // Drop original receiver

// Producer: Send files to channel
for file in files {
    tx.send(file).await.unwrap();
}
drop(tx); // Close channel to signal workers to exit

// Wait for all workers to complete
for worker in workers {
    worker.await.unwrap();
}
```

**Pros:**
- Clear producer-consumer pattern
- Bounded channel provides natural backpressure
- Workers process jobs as they become available
- Clean shutdown mechanism (drop sender)
- Easy to add work queue monitoring

**Cons:**
- Slightly more complex than semaphore
- Need to manage worker lifecycle

### 4. Progress Bar Strategy for Concurrent Uploads

**Challenge:** Multiple concurrent uploads need separate progress bars

**Solution:** Use `indicatif::MultiProgress`
```rust
let multi = MultiProgress::new();

// For each file, create a progress bar
tasks.push(tokio::spawn(async move {
    let pb = multi.add(ProgressBar::new(file_size));
    upload_with_progress(&client, &file, pb).await
}));
```

### 5. Thread-Safe Statistics

Use `Arc<Mutex<Stats>>` or atomic counters:

```rust
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Default)]
struct Stats {
    uploaded: AtomicUsize,
    skipped: AtomicUsize,
    failed: AtomicUsize,
}

// Update atomically
stats.uploaded.fetch_add(1, Ordering::Relaxed);
```

### 6. CLI Configuration

Add new flag:
```rust
/// Maximum number of concurrent uploads
#[arg(long, short = 'c', default_value = "4")]
max_concurrent: usize,
```

### 7. Error Handling Strategy

**Per-file errors shouldn't stop other uploads:**
```rust
// Collect results
let results: Vec<Result<_, _>> = join_all(tasks).await;

// Separate successes from failures
for result in results {
    match result {
        Ok(success) => stats.uploaded += 1,
        Err(e) => {
            eprintln!("Error: {}", e);
            stats.failed += 1;
        }
    }
}
```

### 8. Implementation Steps

1. ✅ Create this plan document
2. Add `--max-concurrent` CLI flag
3. Convert `Stats` to use atomic counters or Arc<Mutex>
4. Implement concurrent upload function using tokio::spawn + Semaphore
5. Update progress bar creation for concurrent tasks
6. Implement concurrent URL-only mode
7. Add proper error collection and reporting
8. Test with various concurrency levels
9. Update documentation with concurrency info

### 9. Performance Expectations

**Before (Sequential):**
- 10 files × 5 seconds each = 50 seconds total

**After (4 concurrent):**
- 10 files ÷ 4 concurrent × 5 seconds = ~12.5 seconds total
- **~4x speedup** for I/O bound operations

**After (8 concurrent):**
- 10 files ÷ 8 concurrent × 5 seconds = ~6.25 seconds total
- **~8x speedup** (if network can handle it)

### 10. Safety Considerations

- **S3 Rate Limits:** AWS S3 supports high request rates, but consider:
  - 3,500 PUT requests/second per prefix
  - 5,500 GET requests/second per prefix
- **Network Bandwidth:** Don't saturate upload bandwidth
- **Memory Usage:** Each concurrent upload holds file data in memory
- **Recommended:** Default to 4 concurrent, allow up to 16 max

### 11. Advanced Features (Future)

- Auto-detect optimal concurrency based on file sizes
- Adaptive concurrency based on error rates
- Progress bar shows overall upload speed
- ETA calculation for remaining files

## Recommendation

**Use Option A (tokio::spawn + Semaphore)** because:
1. More control over concurrency
2. Better error handling per task
3. Easier to integrate progress bars
4. Natural fit for the existing tokio-based codebase
5. Clear separation of concerns
