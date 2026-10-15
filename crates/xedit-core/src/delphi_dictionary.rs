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
//! addressing table whose length is a power of two (`SetCapacity` rounds the
//! capacity of the constructor up from 4), a key's bucket the low bits of
//! `GetHashCode and $7FFFFFFF`, linear probing to the next free bucket,
//! growth to twice the length before an add once the count reaches three
//! quarters of it (`Rehash`, which moves the items in their old bucket order),
//! and enumeration in bucket order. The hash of a string is the default
//! comparer's, Bob Jenkins' `hashlittle` of the UTF-16 code units
//! (`THashBobJenkins.GetHashValue(Value[1], Length(Value) * 2, 0)`).

/// Bob Jenkins' `hashlittle` (lookup3), as `THashBobJenkins.HashLittle`:
/// the bytes read as little-endian words, no final mix for an empty tail.
pub fn hash_little(data: &[u8], init_val: u32) -> u32 {
    fn mix(a: &mut u32, b: &mut u32, c: &mut u32) {
        *a = a.wrapping_sub(*c);
        *a ^= c.rotate_left(4);
        *c = c.wrapping_add(*b);
        *b = b.wrapping_sub(*a);
        *b ^= a.rotate_left(6);
        *a = a.wrapping_add(*c);
        *c = c.wrapping_sub(*b);
        *c ^= b.rotate_left(8);
        *b = b.wrapping_add(*a);
        *a = a.wrapping_sub(*c);
        *a ^= c.rotate_left(16);
        *c = c.wrapping_add(*b);
        *b = b.wrapping_sub(*a);
        *b ^= a.rotate_left(19);
        *a = a.wrapping_add(*c);
        *c = c.wrapping_sub(*b);
        *c ^= b.rotate_left(4);
        *b = b.wrapping_add(*a);
    }
    fn finish(a: &mut u32, b: &mut u32, c: &mut u32) {
        *c ^= *b;
        *c = c.wrapping_sub(b.rotate_left(14));
        *a ^= *c;
        *a = a.wrapping_sub(c.rotate_left(11));
        *b ^= *a;
        *b = b.wrapping_sub(a.rotate_left(25));
        *c ^= *b;
        *c = c.wrapping_sub(b.rotate_left(16));
        *a ^= *c;
        *a = a.wrapping_sub(c.rotate_left(4));
        *b ^= *a;
        *b = b.wrapping_sub(a.rotate_left(14));
        *c ^= *b;
        *c = c.wrapping_sub(b.rotate_left(24));
    }
    let word = |bytes: &[u8]| -> u32 {
        let mut value = 0u32;
        for (index, byte) in bytes.iter().enumerate() {
            value |= u32::from(*byte) << (8 * index);
        }
        value
    };
    let start = 0xDEAD_BEEFu32.wrapping_add(data.len() as u32).wrapping_add(init_val);
    let (mut a, mut b, mut c) = (start, start, start);
    let mut rest = data;
    while rest.len() > 12 {
        a = a.wrapping_add(word(&rest[0..4]));
        b = b.wrapping_add(word(&rest[4..8]));
        c = c.wrapping_add(word(&rest[8..12]));
        mix(&mut a, &mut b, &mut c);
        rest = &rest[12..];
    }
    if rest.is_empty() {
        return c;
    }
    a = a.wrapping_add(word(&rest[..rest.len().min(4)]));
    if rest.len() > 4 {
        b = b.wrapping_add(word(&rest[4..rest.len().min(8)]));
    }
    if rest.len() > 8 {
        c = c.wrapping_add(word(&rest[8..]));
    }
    finish(&mut a, &mut b, &mut c);
    c
}

/// The default `IEqualityComparer<string>.GetHashCode` of the RTL: the hash
/// of the UTF-16 code units of the string.
pub fn string_hash_code(key: &str) -> i32 {
    let bytes: Vec<u8> = key.encode_utf16().flat_map(u16::to_le_bytes).collect();
    hash_little(&bytes, 0) as i32
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
        // `SetCapacity`: the smallest power of two from 4 that holds it.
        if capacity > 0 {
            let mut length = 4usize;
            while length < capacity {
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
    fn hash_little_matches_lookup3() {
        // The test vectors of lookup3.c's driver5: hashlittle("", 0) and
        // the "Four score and seven years ago" lines.
        assert_eq!(hash_little(b"", 0), 0xDEAD_BEEF);
        assert_eq!(hash_little(b"", 0xDEAD_BEEF), 0xBD5B_7DDE);
        assert_eq!(hash_little(b"Four score and seven years ago", 0), 0x1777_0551);
        assert_eq!(hash_little(b"Four score and seven years ago", 1), 0xCD62_8161);
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
        let keys: Vec<&str> = dictionary.keys().collect();
        assert_eq!(keys.len(), 100);
        // Bucket order, not the order of the adds.
        assert_ne!(keys, names.iter().map(String::as_str).collect::<Vec<_>>());
        for key in &keys {
            assert!(dictionary.contains(key));
        }
    }
}
