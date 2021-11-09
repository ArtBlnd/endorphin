# Endorphin
> Key-Value based in-memory cache library which supports **Custom Expiration Policies** and same interface as standard HashMap and HashSet.

```
use endorphin::HashMap;
use endorphin::LazyFixedTTLPolicy;

use std::time::Duration;
use std::thread::sleep;

let mut cache = HashMap::new(LazyFixedTTLPolicy::new(Duration::from_secs(30)));
cache.insert("expired_after", "30 seconds!", ());

cache.get("expired_after").is_none(); // false
sleep(Duration::from_secs(30));
cache.get("expired_after").is_none(); // true
```

Currently, we provide two pre-defined policies. `LazyFixedTTLPolicy` and `TTLPolicy` we'll make more on future release


`LazyFixedTTLPolicy` uses **Lazy Expiration** which is like other cache crates. `cached`, `ttl_cache` which expire items when you access it after expired.  
`TTLPolicy` uses **Active Expiration** which expires even you don't access to expired entries.
