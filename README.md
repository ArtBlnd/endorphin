# Endorphin
-----------
### Key-Value based in-memory cache library.

## About Endorphin
endorphin provides in memory Key-Value cache with TTL based expiration policy. like other cache crate. cached, lru_cache etc 

## Difference between other cache crate.
most of ttl based cache crate expires Key-Value pair when read occurs, which is **Lazy Expiration**. this causes major memory
leaks if the case that you use does not call expired key after its expired. (For example, HTTP cache)

## TODO
- [ ] Implement SyncCache (Send, Sync)
- [X] Implement UnsendCache (!Send, !Sync)
- [ ] Implement UnsyncCache (Send, !Sync)
- [ ] Implement house keeper which clean-up cache on background(something similar to GC)
- [ ] Improve performance on clean-up
- [ ] Add TTI(time to idle) feature