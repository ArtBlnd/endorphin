# Endorphin
Key-Value based in-memory cache library which supports **Active Expiration**
Also with custom **Expiration Policies**

## PLEASE READ THIS
**NOTE THAT THIS PROJECT IS ON VERY EARLY STAGE. NOT FOR PRODUCTION USE**

## About Endorphin
endorphin provides in memory Key-Value cache with TTL based expiration policy. like other cache crate. cached, lru_cache etc 

## Difference between other cache crate.
most of ttl based cache crate expires Key-Value pair when read occurs, which is **Lazy Expiration**. this causes major memory
leaks if the case that you use does not call expired key after its expired (For example, HTTP cache). Endorphin cleans expired
key if internal tick is triggered(currently we use fixed tick rate). which is **Active Expiration**.
