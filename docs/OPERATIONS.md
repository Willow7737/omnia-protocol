# Operations Runbook

## Database Migration: sled → rocksdb

sled 0.34 is not production-ready. A migration to rocksdb is planned.

### Risks of sled 0.34
- Known crash consistency issues (data loss on power failure)
- No ongoing maintenance
- Not recommended by its own author for production

### Migration Steps (TBD)
1. Add `rocksdb` dependency
2. Implement `RocksDbSlashingStore`
3. Implement `RocksDbNonceStore`
4. Add migration tool to convert sled data to rocksdb
5. Swap default in `main.rs`
