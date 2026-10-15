// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The order in which a Delphi `TDictionary<string, ...>` hands out its
//! keys (`for var Key in Dict.Keys`), which upstream code depends on where
//! the first of two entries wins: `TwbContainerHandler.BuildCache` keeps the
//! first name of a file or folder hash it meets while it walks the names of
//! every container (`ccAll`), so of two names with the same hash the one
//! earlier in the dictionary's bucket array is the one `ResolveFileHash`
//! gives.
//!
//! The model is the RTL's (`System.Generics.Collections`): an open
//! addressing table whose length is a power of two (`SetCapacity` takes the
//! smallest one from 4 whose three quarters hold the capacity of the
//! constructor), a key's bucket the low bits of
//! `GetHashCode and $7FFFFFFF`, linear probing to the next free bucket,
//! growth to twice the length before an add once the count reaches three
//! quarters of it (`Rehash`, which moves the items in their old bucket order),
//! and enumeration in bucket order. The hash of a string is the default
//! comparer's, FNV-1a of the UTF-16 code units ([`string_hash_code`]).

/// `THashFNV1a32.GetHashValue`: the 32-bit FNV-1a hash of the bytes
/// (offset basis `$811C9DC5`, prime `$01000193`).
pub fn fnv1a32(data: &[u8]) -> u32 {
    data.iter().fold(0x811C_9DC5u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193)
    })
}

/// The default `IEqualityComparer<string>.GetHashCode` of the RTL the
/// oracle was built with: [`fnv1a32`] of the UTF-16 code units of the string.
///
/// Measured, not read: the RTL source is not part of the upstream checkout.
/// `parity resources` (`fnv-hashes`) puts ten pairs of names with one TES4
/// hash in the data folder and reads which name of each pair the oracle
/// resolves the hash to; of Bob Jenkins' `hashlittle` and FNV-1 and FNV-1a
/// over the UTF-16 or the ANSI bytes, with tables of 2^4 to 2^31 buckets,
/// only FNV-1a of the UTF-16 bytes with 2^21 buckets gives all ten.
pub fn string_hash_code(key: &str) -> i32 {
    let bytes: Vec<u8> = key.encode_utf16().flat_map(u16::to_le_bytes).collect();
    fnv1a32(&bytes) as i32
}

/// A `TDictionary<string, TwbNothing>` (upstream `TwbResourceDict`) that
/// keeps its keys in the RTL's bucket order. Only adding and enumerating are
/// modelled, which is what the cache of the container handler does.
#[derive(Debug, Clone)]
pub struct StringDictionary {
    /// The buckets: the hash code and the key of an item, `None` for an
    /// empty bucket (`EMPTY_HASH`).
    items: Vec<Option<(i32, String)>>,
    count: usize,
    grow_threshold: usize,
}

impl StringDictionary {
    /// `TDictionary.Create(ACapacity)`.
    pub fn with_capacity(capacity: usize) -> Self {
        let mut dictionary = Self {
            items: Vec::new(),
            count: 0,
            grow_threshold: 0,
        };
        // `SetCapacity`: the smallest power of two from 4 whose growth
        // threshold (three quarters of it) holds that many items. Measured on
        // the 4.1.5q oracle (`parity resources`, `fnv-hashes`): the
        // `TDictionary.Create(1000000)` of `BuildCache` has 2^21 buckets, not
        // the 2^20 the plain power of two above the capacity would give.
        if capacity > 0 {
            let mut length = 4usize;
            while (length >> 1) + (length >> 2) < capacity {
                length <<= 1;
            }
            dictionary.rehash(length);
        }
        dictionary
    }

    /// The number of keys.
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn hash(key: &str) -> i32 {
        string_hash_code(key) & 0x7FFF_FFFF
    }

    /// `GetBucketIndex`: the bucket of the key (`Ok`) or the free bucket it
    /// would take (`Err`).
    fn bucket_index(items: &[Option<(i32, String)>], key: &str, hash: i32) -> Result<usize, usize> {
        if items.is_empty() {
            return Err(0);
        }
        let mask = items.len() - 1;
        let mut index = (hash as usize) & mask;
        loop {
            match &items[index] {
                None => return Err(index),
                Some((item_hash, item_key)) if *item_hash == hash && item_key == key => return Ok(index),
                Some(_) => index = (index + 1) & mask,
            }
        }
    }

    /// `Rehash`: the items in their bucket order into a table of the new
    /// length.
    fn rehash(&mut self, length: usize) {
        let old = std::mem::replace(&mut self.items, vec![None; length]);
        self.grow_threshold = (length >> 1) + (length >> 2);
        for (hash, key) in old.into_iter().flatten() {
            let index = match Self::bucket_index(&self.items, &key, hash) {
                Ok(index) | Err(index) => index,
            };
            self.items[index] = Some((hash, key));
        }
    }

    /// `TryAdd`: adds the key when it is not there; whether it was added.
    pub fn try_add(&mut self, key: &str) -> bool {
        if self.count >= self.grow_threshold {
            // `Grow`.
            let length = if self.items.is_empty() { 4 } else { self.items.len() * 2 };
            self.rehash(length);
        }
        let hash = Self::hash(key);
        match Self::bucket_index(&self.items, key, hash) {
            Ok(_) => false,
            Err(index) => {
                self.items[index] = Some((hash, key.to_owned()));
                self.count += 1;
                true
            }
        }
    }

    /// `ContainsKey`.
    pub fn contains(&self, key: &str) -> bool {
        Self::bucket_index(&self.items, key, Self::hash(key)).is_ok()
    }

    /// `Keys`: the keys in bucket order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.items.iter().flatten().map(|(_, key)| key.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_matches_the_reference_values() {
        // The test vectors of the FNV reference: "" and "a" and "foobar".
        assert_eq!(fnv1a32(b""), 0x811C_9DC5);
        assert_eq!(fnv1a32(b"a"), 0xE40C_292C);
        assert_eq!(fnv1a32(b"foobar"), 0xBF9C_F968);
        // The UTF-16 code units of the string.
        assert_eq!(string_hash_code("a"), fnv1a32(&[0x61, 0]) as i32);
    }

    #[test]
    fn the_oracle_resolves_hash_pairs_in_this_order() {
        // Four of the pairs of `fnv-hashes` and the name the oracle resolved
        // the hash to: the one earlier in the bucket order of
        // `TDictionary.Create(1000000)` (2^21 buckets).
        let mut dictionary = StringDictionary::with_capacity(1_000_000);
        let pairs = [
            ("q090ynp5wzz", "qdmeof78jzz", "qdmeof78jzz"),
            ("qhovs53ymzz", "qk2uo11tizz", "qhovs53ymzz"),
            ("qo387f11rzz", "qscye1afizz", "qscye1afizz"),
            ("q7ormo6mmzz", "qxthlady0zz", "q7ormo6mmzz"),
        ];
        for (a, b, _) in pairs {
            dictionary.try_add(&format!("textures\\probe\\{a}.dds"));
            dictionary.try_add(&format!("textures\\probe\\{b}.dds"));
        }
        let keys: Vec<&str> = dictionary.keys().collect();
        for (a, b, winner) in pairs {
            let position = |name: &str| keys.iter().position(|key| key.contains(name)).unwrap();
            let first = if position(a) < position(b) { a } else { b };
            assert_eq!(first, winner);
        }
    }

    #[test]
    fn keys_come_in_bucket_order_and_survive_growth() {
        let mut dictionary = StringDictionary::with_capacity(0);
        let names: Vec<String> = (0..100).map(|i| format!("name{i}")).collect();
        for name in &names {
            assert!(dictionary.try_add(name));
        }
        assert!(!dictionary.try_add("name7"));
        assert_eq!(dictionary.len(), 100);
        // 100 items grow the table to 256 buckets (the threshold of 128 is 96).
        assert_eq!(dictionary.items.len(), 256);
        // A capacity is a number of items: a million take 2^21 buckets.
        assert_eq!(StringDictionary::with_capacity(1_000_000).items.len(), 1 << 21);
        assert_eq!(StringDictionary::with_capacity(3).items.len(), 4);
        assert_eq!(StringDictionary::with_capacity(4).items.len(), 8);
        let keys: Vec<&str> = dictionary.keys().collect();
        assert_eq!(keys.len(), 100);
        // Bucket order, not the order of the adds.
        assert_ne!(keys, names.iter().map(String::as_str).collect::<Vec<_>>());
        for key in &keys {
            assert!(dictionary.contains(key));
        }
    }
}
