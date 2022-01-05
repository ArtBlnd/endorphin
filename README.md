# Endorphin
Key-Value based in-memory cache library which supports **Custom Expiration Policies** with standard HashMap, HashSet interface.

## Example
```rust
use std::thread::sleep;
use std::time::Duration;

use endorphin::policy::TTLPolicy;
use endorphin::HashMap;

fn main() {
    let mut cache = HashMap::new(TTLPolicy::new());

    cache.insert("Still", "Alive", Duration::from_secs(3));
    cache.insert("Gonna", "Die", Duration::from_secs(1));

    sleep(Duration::from_secs(1));

    assert_eq!(cache.get(&"Still"), Some(&"Alive"));
    assert_eq!(cache.get(&"Gonna"), None);
}

```

Currently, we are providing four pre-defined policies. 

 - `LazyFixedTTLPolicy` uses **Lazy Expiration** as other cache crates do, it expires items when you access entry after its TTL.
 - `TTLPolicy` uses **Active Expiration** which expires even you don't access to expired entries.
 - `TTIPolicy` uses **Active Expiration** which expires even you don't access to expired entries.
 - `MixedPolicy` is mixed policy of TTL and TTI