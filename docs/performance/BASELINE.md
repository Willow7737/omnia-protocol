# Performance Baseline

## Test Configuration
- Nodes: 4 (in-memory)
- Duration: 60 seconds
- Target rate: 100 events/sec
- Event size: 256 bytes

## Baseline Results
To be populated after first load test run.

## Metrics Tracked
- Finalization rate (events/sec)
- Average submission-to-finalization latency
- P99 latency
- Network bandwidth utilization

## How to Run

```bash
# Default configuration
cargo run --bin omnia-load-test

# Custom configuration via environment variables
NUM_NODES=8 DURATION_SECS=120 EVENTS_PER_SEC=500 cargo run --bin omnia-load-test
```

## Environment Variables
| Variable | Default | Description |
|---|---|---|
| `NUM_NODES` | 4 | Number of simulated consensus nodes |
| `DURATION_SECS` | 60 | Test duration in seconds |
| `EVENTS_PER_SEC` | 100 | Target event submission rate |
| `EVENT_SIZE_BYTES` | 256 | Size of each event payload |
