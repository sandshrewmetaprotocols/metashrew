use metashrew_support::index_pointer::KeyValuePointer;
use crate::{get, set};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
pub struct IndexPointer(Arc<Vec<u8>>);

#[allow(dead_code)]
impl KeyValuePointer for IndexPointer {
    fn wrap(word: &Vec<u8>) -> IndexPointer {
        IndexPointer(Arc::<Vec<u8>>::new(word.clone()))
    }
    fn unwrap(&self) -> Arc<Vec<u8>> {
        self.0.clone()
    }
    fn inherits(&mut self, _v: &Self) {}
    fn set(&mut self, v: Arc<Vec<u8>>) {
        set(self.unwrap(), v)
    }
    fn get(&self) -> Arc<Vec<u8>> {
        get(self.unwrap())
    }
}

#[derive(Clone, Default, Debug)]
pub struct IndexCheckpoint(pub HashMap<Arc<Vec<u8>>, Arc<Vec<u8>>>);

impl IndexCheckpoint {
    fn pipe_to(&self, target: &mut IndexCheckpoint) {
        self.0.iter().for_each(|(k, v)| {
            target.0.insert(k.clone(), v.clone());
        });
    }
}

#[derive(Clone, Debug)]
pub struct IndexCheckpointStack(pub Arc<Mutex<Vec<IndexCheckpoint>>>);

impl Default for IndexCheckpointStack {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(vec![IndexCheckpoint::default()])))
    }
}

impl IndexCheckpointStack {
    pub fn depth(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

#[derive(Clone, Debug)]
pub struct AtomicPointer {
    pointer: IndexPointer,
    store: IndexCheckpointStack,
}

impl KeyValuePointer for AtomicPointer {
    fn wrap(word: &Vec<u8>) -> Self {
        AtomicPointer {
            pointer: IndexPointer::wrap(word),
            store: IndexCheckpointStack::default(),
        }
    }
    fn unwrap(&self) -> Arc<Vec<u8>> {
        self.pointer.unwrap()
    }
    fn inherits(&mut self, from: &Self) {
        self.store = from.store.clone()
    }
    fn set(&mut self, v: Arc<Vec<u8>>) {
        self.store
            .0
            .lock()
            .unwrap()
            .last_mut()
            .unwrap()
            .0
            .insert(self.unwrap(), v.clone());
    }
    fn get(&self) -> Arc<Vec<u8>> {
        let unwrapped = self.unwrap();
        match self
            .store
            .0
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|map| map.0.contains_key(&unwrapped))
        {
            Some(map) => map.0.get(&unwrapped).unwrap().clone(),
            None => self.pointer.get(),
        }
    }
}

impl Default for AtomicPointer {
    fn default() -> Self {
        AtomicPointer {
            pointer: IndexPointer::wrap(&Vec::<u8>::new()),
            store: IndexCheckpointStack::default(),
        }
    }
}

impl AtomicPointer {
    pub fn checkpoint(&mut self) {
        self.store
            .0
            .lock()
            .unwrap()
            .push(IndexCheckpoint::default());
    }
    pub fn commit(&mut self) {
        let checkpoints = &mut self.store.0.lock().unwrap();
        if checkpoints.len() > 1 {
            checkpoints
                .pop()
                .unwrap()
                .pipe_to(checkpoints.last_mut().unwrap());
        } else if checkpoints.len() == 1 {
            checkpoints.last().unwrap().0.iter().for_each(|(k, v)| {
                set(k.clone(), v.clone());
            });
        } else {
            panic!("commit() called without checkpoints in memory");
        }
    }
    pub fn rollback(&mut self) {
        self.store.0.lock().unwrap().pop();
    }
    pub fn derive(&self, pointer: &IndexPointer) -> Self {
        AtomicPointer {
            store: self.store.clone(),
            pointer: pointer.clone(),
        }
    }
    pub fn get_pointer(&self) -> IndexPointer {
        return self.pointer.clone();
    }

    // Get the current depth of the checkpoint stack
    pub fn checkpoint_depth(&self) -> usize {
        self.store.depth()
    }
}

use crate::byte_view::ByteView;
#[allow(unused_imports)]
use core::prelude;

/// Hierarchical key-value storage abstraction for Bitcoin indexers
///
/// [`KeyValuePointer`] provides a powerful abstraction for building complex
/// data structures on top of simple key-value stores. It enables hierarchical
/// key organization, type-safe value storage, and efficient list operations.
///
/// # Core Operations
///
/// ## Key Management
/// - `wrap`: Create pointer from raw key bytes
/// - `unwrap`: Extract raw key bytes
/// - `select`: Create child pointer with extended key
/// - `keyword`: Extend key with string suffix
///
/// ## Value Operations
/// - `get`/`set`: Raw byte value access
/// - `get_value`/`set_value`: Type-safe value access via [`ByteView`]
/// - `nullify`: Set value to zero/empty state
///
/// ## List Operations
/// - `append`/`append_value`: Add items to end of list
/// - `pop`/`pop_value`: Remove and return last item
/// - `length`: Get current list length
/// - `get_list`/`get_list_values`: Retrieve all list items
///
/// ## Linked List Operations
/// - `append_ll`: Add item to linked list
/// - `delete_value`: Remove item from linked list
/// - `map_ll`: Iterate over linked list items
///
/// # Implementation Requirements
///
/// Implementors must provide:
/// - Key wrapping/unwrapping for raw byte access
/// - Value get/set operations for storage backend
/// - Inheritance mechanism for sharing configuration
///
/// # Thread Safety
///
/// The trait itself doesn't enforce thread safety, but implementations
/// should consider concurrent access patterns for multi-threaded indexers.
///
/// # Example Implementation
///
/// ```rust,ignore
/// struct IndexPointer {
///     key: Arc<Vec<u8>>,
///     // ... storage backend reference
/// }
///
/// impl KeyValuePointer for IndexPointer {
///     fn wrap(word: &Vec<u8>) -> Self {
///         Self { key: Arc::new(word.clone()) }
///     }
///
///     fn unwrap(&self) -> Arc<Vec<u8>> {
///         self.key.clone()
///     }
///
///     // ... implement other required methods
/// }
/// ```
pub trait KeyValuePointer {
    /// Create a new pointer from raw key bytes
    ///
    /// This is the primary constructor for creating pointers from byte keys.
    /// The key represents the full path in the hierarchical key space.
    ///
    /// # Parameters
    ///
    /// - `word`: Raw key bytes representing the storage key
    ///
    /// # Returns
    ///
    /// A new pointer instance wrapping the provided key
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let ptr = IndexPointer::wrap(&b"balances/user123".to_vec());
    /// ```
    fn wrap(word: &Vec<u8>) -> Self;

    /// Extract the raw key bytes from this pointer
    ///
    /// Returns the underlying key bytes that identify this pointer's
    /// location in the key-value store.
    ///
    /// # Returns
    ///
    /// Arc-wrapped vector containing the raw key bytes
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let key_bytes = ptr.unwrap();
    /// println!("Key: {}", String::from_utf8_lossy(&key_bytes));
    /// ```
    fn unwrap(&self) -> Arc<Vec<u8>>;

    /// Set the value at this pointer's key
    ///
    /// Stores raw byte data at the key location represented by this pointer.
    /// This is the low-level storage operation that other methods build upon.
    ///
    /// # Parameters
    ///
    /// - `v`: Arc-wrapped byte vector to store
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// ptr.set(Arc::new(b"some_data".to_vec()));
    /// ```
    fn set(&mut self, v: Arc<Vec<u8>>);

    /// Get the value at this pointer's key
    ///
    /// Retrieves raw byte data from the key location represented by this pointer.
    /// Returns empty vector if no value exists at this key.
    ///
    /// # Returns
    ///
    /// Arc-wrapped byte vector containing the stored data
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let data = ptr.get();
    /// ```
    fn get(&self) -> Arc<Vec<u8>>;

    /// Inherit configuration from another pointer
    ///
    /// This method allows pointers to inherit settings or context from
    /// parent pointers, enabling shared configuration across hierarchies.
    ///
    /// # Parameters
    ///
    /// - `from`: Pointer to inherit configuration from
    fn inherits(&mut self, from: &Self);
    fn select(&self, word: &Vec<u8>) -> Self
    where
        Self: Sized,
    {
        let mut key = (*self.unwrap()).clone();
        key.extend(word);
        let mut ptr = Self::wrap(&key);
        ptr.inherits(self);
        ptr
    }
    fn from_keyword(word: &str) -> Self
    where
        Self: Sized,
    {
        Self::wrap(&word.as_bytes().to_vec())
    }
    fn keyword(&self, word: &str) -> Self
    where
        Self: Sized,
    {
        let mut key = (*self.unwrap()).clone();
        key.extend(word.to_string().into_bytes());
        let mut ptr = Self::wrap(&key);
        ptr.inherits(self);
        ptr
    }

    fn set_value<T: ByteView>(&mut self, v: T) {
        self.set(Arc::new(v.to_bytes()));
    }

    fn get_value<T: ByteView>(&self) -> T {
        let cloned = self.get().as_ref().clone();
        if cloned.is_empty() {
            T::zero()
        } else {
            T::from_bytes(cloned)
        }
    }

    fn select_value<T: ByteView>(&self, key: T) -> Self
    where
        Self: Sized,
    {
        self.select(key.to_bytes().as_ref())
    }
    fn length_key(&self) -> Self
    where
        Self: Sized,
    {
        self.keyword(&"/length".to_string())
    }
    fn head_key(&self) -> Self
    where
        Self: Sized,
    {
        self.keyword(&"/head".to_string())
    }
    fn next_key(&self, i: u32) -> Self
    where
        Self: Sized,
    {
        self.keyword(&"/next".to_string()).select_value(i)
    }
    fn length(&self) -> u32
    where
        Self: Sized,
    {
        self.length_key().get_value::<u32>()
    }
    fn select_index(&self, index: u32) -> Self
    where
        Self: Sized,
    {
        self.keyword(&format!("/{}", index))
    }

    fn drop_index(&self, index: u32) -> ()
    where
        Self: Sized,
    {
        let mut idx = self.keyword(&format!("/{}", index));
        idx.nullify();
    }
    fn get_list(&self) -> Vec<Arc<Vec<u8>>>
    where
        Self: Sized,
    {
        let mut result: Vec<Arc<Vec<u8>>> = vec![];
        for i in 0..self.length() {
            result.push(self.select_index(i as u32).get().clone());
        }
        result
    }
    fn get_list_values<T: ByteView>(&self) -> Vec<T>
    where
        Self: Sized,
    {
        let mut result: Vec<T> = vec![];
        for i in 0..self.length() {
            result.push(self.select_index(i as u32).get_value());
        }
        result
    }
    fn nullify(&mut self) {
        self.set(Arc::from(vec![0]))
    }
    fn set_or_nullify(&mut self, v: Arc<Vec<u8>>) {
        let val = Arc::try_unwrap(v).unwrap();
        if <usize>::from_bytes(val.clone()) == 0 {
            self.nullify();
        } else {
            self.set(Arc::from(val));
        }
    }

    fn pop(&self) -> Arc<Vec<u8>>
    where
        Self: Sized,
    {
        let mut length_key = self.length_key();
        let length = length_key.get_value::<u32>();

        if length == 0 {
            return Arc::new(Vec::new()); // Return empty Vec if there are no elements
        }

        let new_length = length - 1;
        length_key.set_value::<u32>(new_length); // Update the length
        self.select_index(new_length).get() // Return the value at the new length
    }

    fn pop_value<T: ByteView>(&self) -> T
    where
        Self: Sized,
    {
        let mut length_key = self.length_key();
        let length = length_key.get_value::<u32>();

        if length == 0 {
            return T::from_bytes(Vec::new()); // Return a default value if there are no elements
        }

        let new_length = length - 1;
        length_key.set_value::<u32>(new_length); // Update the length
        self.select_index(new_length).get_value::<T>() // Return the value at the new length
    }

    fn append(&self, v: Arc<Vec<u8>>)
    where
        Self: Sized,
    {
        let mut new_index = self.extend();
        new_index.set(v);
    }
    fn append_ll(&self, v: Arc<Vec<u8>>)
    where
        Self: Sized,
    {
        let mut new_index = self.extend_ll();
        new_index.set(v);
    }
    fn append_value<T: ByteView>(&self, v: T)
    where
        Self: Sized,
    {
        let mut new_index = self.extend();
        new_index.set_value(v);
    }

    fn extend(&self) -> Self
    where
        Self: Sized,
    {
        let mut length_key = self.length_key();
        let length = length_key.get_value::<u32>();
        length_key.set_value::<u32>(length + 1);
        self.select_index(length)
    }
    fn extend_ll(&self) -> Self
    where
        Self: Sized,
    {
        let mut length_key = self.length_key();
        let length = length_key.get_value::<u32>();
        if length > 0 {
            let mut next_key = self.next_key(length - 1);
            next_key.set_value(length);
        }
        length_key.set_value::<u32>(length + 1);
        self.select_index(length)
    }
    fn prefix(&self, keyword: &str) -> Self
    where
        Self: Sized,
    {
        let mut val = keyword.to_string().into_bytes();
        val.extend((*self.unwrap()).clone());
        let mut ptr = Self::wrap(&val);
        ptr.inherits(self);
        ptr
    }
    fn set_next_for(&self, i: u32, v: u32) -> ()
    where
        Self: Sized,
    {
        let mut next_key = self.next_key(i);
        next_key.set_value(v);
    }
    fn delete_value(&self, i: u32) -> ()
    where
        Self: Sized,
    {
        let mut head_key = self.head_key();
        if i == head_key.get_value::<u32>() {
            let next = self.next_key(i).get_value::<u32>();
            head_key.set_value::<u32>(next);
        } else {
            let mut prev = self.next_key(i - 1);
            let next = self.next_key(i).get_value::<u32>();
            prev.set_value::<u32>(next);
        }
        self.drop_index(i);
    }
    fn map_ll<T>(&self, mut f: impl FnMut(&mut Self, u32) -> T) -> Vec<T>
    where
        Self: Sized + Clone,
    {
        let length_key = self.length_key();
        let length = length_key.get_value::<u32>();
        let mut result = Vec::new();
        let mut i: u32 = self.head_key().get_value::<u32>();
        while i < length {
            let item = self.select_index(i);
            let mut item_mut = item.clone();
            result.push(f(&mut item_mut, i));
            i = self.next_key(i).get_value::<u32>();
            if i == 0 {
                break;
            }
        }
        result
    }
}
