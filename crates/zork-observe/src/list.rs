use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    ops::{Index, IndexMut, Range},
    sync::Arc,
};

/// An indexed immutable snapshot with structurally shared storage. Cloning a
/// snapshot shares both the RRB tree and each record's payload. Mutations copy
/// only the affected tree path/chunks, never every record's String or metadata.
#[derive(Debug)]
pub struct List<T>(imbl::Vector<Arc<T>>);

impl<T> Clone for List<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Default for List<T> {
    fn default() -> Self {
        Self(imbl::Vector::new())
    }
}

impl<T> List<T> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
            // imbl's inline vectors have no shared tree root to compare.
            || (self.len() == other.len() && self.len() <= 32
                && self.0.iter().zip(other.0.iter()).all(|(a, b)| Arc::ptr_eq(a, b)))
    }
    pub fn get(&self, index: usize) -> Option<&T> {
        self.0.get(index).map(Arc::as_ref)
    }
    pub fn shared(&self, index: usize) -> Option<Arc<T>> {
        self.0.get(index).cloned()
    }
    pub fn first(&self) -> Option<&T> {
        self.get(0)
    }
    pub fn last(&self) -> Option<&T> {
        self.len().checked_sub(1).and_then(|index| self.get(index))
    }
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator + Clone + '_ {
        self.0.iter().map(Arc::as_ref)
    }
    pub fn push(&mut self, value: T) {
        self.push_shared(Arc::new(value));
    }
    pub fn push_shared(&mut self, value: Arc<T>) {
        self.0.push_back(value);
    }
    pub fn push_front(&mut self, value: T) {
        self.0.push_front(Arc::new(value));
    }
    pub fn push_front_shared(&mut self, value: Arc<T>) {
        self.0.push_front(value);
    }
    pub fn remove(&mut self, index: usize) -> Arc<T> {
        self.0.remove(index)
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn append(&mut self, other: Self) {
        // RRB concatenation promotes even a one-element vector into a tree and
        // joins both middles. Small tails can use the bounded tail buffer.
        if other.len() <= 32 {
            for value in other.0 {
                self.0.push_back(value);
            }
            return;
        }
        self.0.append(other.0);
    }
    pub fn slice(&self, range: Range<usize>) -> Self {
        assert!(
            range.start <= range.end && range.end <= self.len(),
            "list slice out of bounds"
        );
        let mut items = self.0.skip(range.start);
        items.truncate(range.len());
        Self(items)
    }
    pub fn splice(&mut self, remove: Range<usize>, insert: Self) {
        assert!(
            remove.start <= remove.end && remove.end <= self.len(),
            "list splice out of bounds"
        );
        if remove.is_empty() && insert.is_empty() {
            return;
        }
        if insert.len() <= 32 {
            if remove.len() == insert.len() {
                for (offset, value) in insert.0.into_iter().enumerate() {
                    self.0[remove.start + offset] = value;
                }
                return;
            }
            if remove.end == self.len() {
                self.0.truncate(remove.start);
                self.append(insert);
                return;
            }
            if remove == (0..0) {
                for value in insert.0.into_iter().rev() {
                    self.0.push_front(value);
                }
                return;
            }
        }
        let tail = self.0.split_off(remove.end);
        self.0.truncate(remove.start);
        self.0.append(insert.0);
        self.0.append(tail);
    }
    pub fn from_shared(values: impl IntoIterator<Item = Arc<T>>) -> Self {
        Self(values.into_iter().collect())
    }
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T>
    where
        T: Clone,
    {
        self.0.get_mut(index).map(Arc::make_mut)
    }
    pub fn set_shared(&mut self, index: usize, value: Arc<T>) -> bool
    where
        T: PartialEq,
    {
        if self.0[index] == value {
            return false;
        }
        self.0[index] = value;
        true
    }
}

impl<T> Index<usize> for List<T> {
    type Output = T;
    fn index(&self, index: usize) -> &T {
        &self.0[index]
    }
}

impl<T: Clone> IndexMut<usize> for List<T> {
    fn index_mut(&mut self, index: usize) -> &mut T {
        self.get_mut(index).expect("list index out of bounds")
    }
}

impl<T: PartialEq> PartialEq for List<T> {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other) || (self.len() == other.len() && self.iter().eq(other.iter()))
    }
}

impl<T: Eq> Eq for List<T> {}

impl<T> FromIterator<T> for List<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from_shared(iter.into_iter().map(Arc::new))
    }
}

impl<T> From<Vec<T>> for List<T> {
    fn from(value: Vec<T>) -> Self {
        value.into_iter().collect()
    }
}

impl<T: Serialize> Serialize for List<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.iter())
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for List<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<T>::deserialize(deserializer).map(Self::from)
    }
}

/// A splice against the state after the preceding edit in its batch.
#[derive(Debug)]
pub struct ListEdit<T> {
    pub remove: Range<usize>,
    pub insert: List<T>,
}

impl<T> Clone for ListEdit<T> {
    fn clone(&self) -> Self {
        Self {
            remove: self.remove.clone(),
            insert: self.insert.clone(),
        }
    }
}

impl<T> ListEdit<T> {
    pub fn apply(&self, list: &mut List<T>) -> bool {
        if self.remove.start > self.remove.end || self.remove.end > list.len() {
            return false;
        }
        list.splice(self.remove.clone(), self.insert.clone());
        true
    }

    /// Coalesce only when the new edit is wholly inside the preceding inserted
    /// range, or is its immediate continuation. Other edits stay ordered.
    pub fn push_coalesced(edits: &mut Vec<Self>, next: Self) {
        if let Some(last) = edits.last_mut() {
            let end = last.remove.start + last.insert.len();
            if next.remove.start >= last.remove.start && next.remove.end <= end {
                last.insert.splice(
                    next.remove.start - last.remove.start..next.remove.end - last.remove.start,
                    next.insert,
                );
                return;
            }
        }
        if !next.remove.is_empty() || !next.insert.is_empty() {
            edits.push(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct Payload(usize, Arc<AtomicUsize>);
    impl Clone for Payload {
        fn clone(&self) -> Self {
            self.1.fetch_add(1, Ordering::Relaxed);
            Self(self.0, self.1.clone())
        }
    }

    #[test]
    fn large_snapshot_edits_copy_only_the_record_being_changed() {
        let copies = Arc::new(AtomicUsize::new(0));
        let mut list: List<_> = (0..100_000).map(|i| Payload(i, copies.clone())).collect();
        let old = list.clone();
        list.push(Payload(100_000, copies.clone()));
        list.push_front(Payload(100_001, copies.clone()));
        list[50_000].0 = 7;
        assert_eq!(copies.load(Ordering::Relaxed), 1);
        assert_eq!(old.len(), 100_000);
        assert_eq!(old[49_999].0, 49_999);
        assert_eq!(list[50_000].0, 7);
        assert!(Arc::ptr_eq(
            &old.shared(7).unwrap(),
            &list.shared(8).unwrap()
        ));
    }

    #[test]
    fn coalescing_matches_sequential_splices_including_empty_insertions() {
        let mut expected: List<_> = (0..80).collect();
        let original = expected.clone();
        let mut edits = Vec::new();
        // Deterministic mixed inserts, removals and replacements, including
        // adjacent appends and updates inside a preceding inserted range.
        let mut random = 19_u64;
        for step in 0..2_000 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let start = random as usize % (expected.len() + 1);
            let remove = (random >> 32) as usize % 4;
            let end = (start + remove).min(expected.len());
            let insert = (0..(step % 4)).map(|i| 1000 + step * 4 + i).collect();
            let edit = ListEdit {
                remove: start..end,
                insert,
            };
            assert!(edit.apply(&mut expected));
            ListEdit::push_coalesced(&mut edits, edit);
        }
        let mut actual = original;
        for edit in edits {
            assert!(edit.apply(&mut actual));
        }
        assert_eq!(actual, expected);
    }

    #[test]
    fn consecutive_appends_and_tail_updates_form_one_edit() {
        let mut edits = Vec::new();
        for index in 100..200 {
            ListEdit::push_coalesced(
                &mut edits,
                ListEdit {
                    remove: index..index,
                    insert: vec![index].into(),
                },
            );
        }
        for index in 0..100 {
            ListEdit::push_coalesced(
                &mut edits,
                ListEdit {
                    remove: 199..200,
                    insert: vec![index].into(),
                },
            );
        }
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].remove, 100..100);
        assert_eq!(edits[0].insert.len(), 100);
        assert_eq!(edits[0].insert.last(), Some(&99));
    }

    #[test]
    fn sparse_splices_match_vec_across_chunk_boundaries_and_keep_old_snapshots() {
        let mut expected = (0..100_000).collect::<Vec<_>>();
        let mut actual: List<_> = expected.iter().copied().collect();
        let old = actual.clone();
        let mut random = 29_u64;
        for step in 0..600 {
            random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
            let remove = (random >> 32) as usize % 36;
            let start = match step % 4 {
                0 => 0,
                1 => expected.len().saturating_sub(remove),
                _ => random as usize % (expected.len() + 1),
            };
            let end = (start + remove).min(expected.len());
            let count = if step % 3 == 0 {
                end - start
            } else {
                step % 36
            };
            let insert = (0..count)
                .map(|i| 200_000 + step * 36 + i)
                .collect::<Vec<_>>();
            actual.splice(start..end, insert.clone().into());
            expected.splice(start..end, insert);
            if step % 17 == 0 {
                assert!(actual.iter().copied().eq(expected.iter().copied()));
            }
        }
        assert!(actual.iter().copied().eq(expected));
        assert!(old.iter().copied().eq(0..100_000));
    }
}
