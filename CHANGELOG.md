# Changelog

## 0.3.0

### Fixed

- **Iterating a fully expired cache aborted the process.** Every iterator that skips expired
  entries -- `Iter`, `IterMut`, `Drain`, and the set's `Difference` and `Intersection` -- recursed
  once per skipped element. A whole batch expiring at once is the normal state of a TTL cache, so
  `iter` on one overflowed the stack, and with it `len` and `is_empty`, which are built on `iter`.
  They loop now. Reproduced at 200k entries; a regression test walks that path.

### Breaking

- `IterMut` yields `(&K, &mut V)` instead of `(&mut K, &mut V)`, and so does `&mut HashMap` when
  iterated. A mutable key let safe code change an entry's hash, leaving it in a bucket it no
  longer belongs to: still in the table, and unreachable forever. `std` and `hashbrown` hand the
  key out shared for the same reason.

## 0.2.0

A soundness release. Every version up to and including `0.1.9` carries undefined behaviour that
`cargo test` cannot see; upgrading is recommended even if you do not need the new API.

### Fixed (soundness)

- **`HashMap::get_mut` was unsound.** It took `&self` and handed back `&mut V`, so safe code
  could hold two mutable references to the same value at once. It now takes `&mut self`.
  Miri reports UB on the old signature with no `unsafe` in the caller.
- **References were being used after an aliasing write.** Every entry is reachable through two
  live handles -- its bucket in the raw table, and the copy parked in the internal expiration
  table -- and the expiration state is interior-mutable, so a policy that returns
  `Command::Remove` writes through the second handle and invalidates references derived from the
  first. `get`, `get_key_value`, `get_mut`, and the `OccupiedEntry` accessors all read through
  such references afterwards. They now re-derive instead.
- **`Entry::and_replace_entry_with` left a dangling bucket behind.** When the closure returned
  `None` the entry was dropped, but its now-erased bucket was registered back into the
  expiration table for the expiration path to walk into. Its slot is now released.
- **`OccupiedEntry::insert` aliased the table.** It passed a reference *into* the table as the
  lookup key of a mutable lookup on that same table. It uses the bucket it already holds.

`cargo miri test` now runs in CI under both Stacked Borrows and Tree Borrows, which is what
would have caught these.

### Breaking

- `HashMap::get_mut` takes `&mut self`. Code that held two results at once no longer compiles --
  that is the point. As a bonus, it now reaps a batch of expired entries the way every other
  mutable operation does.

### Added

- `HashSet::get_or_insert`, `get_or_insert_owned` and `get_or_insert_with`, completing the set.
  A live element is handed back untouched, so reading one never extends its life; an expired one
  counts as absent and is inserted again.
- `Debug`, `Default`, `PartialEq`, `Eq`, `Extend`, `FromIterator` and `IntoIterator` for both
  `HashMap` and `HashSet`.

  `Extend` and `FromIterator` take the expiration info per element -- `(key, value, init)` and
  `(value, init)` -- because no single policy input could stand in for all of them.

  Equality compares the live entries and does **not** report an access to the policy: comparing
  two caches is not using them. Under `TTIPolicy` a single `==` would otherwise refresh every
  entry on both sides.

### Not implemented, on purpose

- `Clone`. `TTLPolicy` indexes its records by `EntryId`, and a cloned map necessarily gets fresh
  ids, so the clone's policy would point at entries that no longer exist. `TTIStorage` also holds
  an `Arc`, which a clone would share. Cloning a cache would need the policy to be able to remap
  ids; until then the impl would be quietly wrong rather than absent.
- `BitOr` / `BitAnd` / `BitXor` / `Sub` for `HashSet`. They would have to build a new set, and
  there is no expiration info to give the elements of it. Use `union`, `intersection`,
  `difference` and `symmetric_difference`, which iterate instead of allocating.
