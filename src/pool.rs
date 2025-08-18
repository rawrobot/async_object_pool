use crossbeam_queue::ArrayQueue;
use std::fmt::{Debug, Formatter};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Weak};

use crate::Resettable;

/// A lock-free, thread-safe, sized object pool.
///
/// # Examples
///
/// Basic usage:
///
/// ```
/// use asyn_object_pool::{BundledPool, Resettable};
///
/// #[derive(Debug)]
/// struct Connection {
///     id: u32,
/// }
///
/// impl Resettable for Connection {
///     fn reset(&mut self) {
///         // Reset connection state
///     }
/// }
///
/// let pool = BundledPool::new(2, 5, || Connection { id: 1 });
///
/// // Take an object from the pool
/// let conn = pool.take();
/// assert_eq!(conn.id, 1);
///
/// // Object is automatically returned when dropped
/// drop(conn);
/// assert_eq!(pool.available(), 2);
/// ```
///
/// Using try_take for non-blocking access:
///
/// ```
/// use asyn_object_pool::{BundledPool, Resettable};
///
/// #[derive(Debug)]
/// struct Resource { value: i32 }
///
/// impl Resettable for Resource {
///     fn reset(&mut self) { self.value = 0; }
/// }
///
/// let pool = BundledPool::new(0, 1, || Resource { value: 42 });
///
/// // Pool is empty, try_take returns None
/// assert!(pool.try_take().is_none());
/// ```
///
/// this pool begins with an initial capacity and will continue creating new objects on request when none are available.
/// pooled objects are returned to the pool on destruction (with an extra provision to optionally "reset" the state of
/// an object for re-use).
///
/// if, during an attempted return, a pool already has `maximum_capacity` objects in the pool, the pool will throw away
/// that object.
#[derive(Debug)]
pub struct BundledPool<T: Resettable>
where
    T: Debug,
{
    data: Arc<PoolData<T>>,
}

impl<T: Resettable> BundledPool<T>
where
    T: Debug,
{
    /// Creates a new `BundledPool<T>`.
    ///
    /// # Arguments
    ///
    /// * `initial_capacity` - Number of objects to pre-allocate
    /// * `maximum_capacity` - Maximum number of objects the pool can hold
    /// * `create` - Factory function to create new objects
    ///
    /// # Panics
    ///
    /// Panics if `initial_capacity > maximum_capacity`.
    ///
    /// # Examples
    ///
    /// ```
    /// use asyn_object_pool::{BundledPool, Resettable};
    ///
    /// #[derive(Debug)]
    /// struct Counter { count: usize }
    ///
    /// impl Resettable for Counter {
    ///     fn reset(&mut self) { self.count = 0; }
    /// }
    ///
    /// let pool = BundledPool::new(1, 3, || Counter { count: 0 });
    /// assert_eq!(pool.available(), 1);
    /// assert_eq!(pool.capacity(), 3);
    /// ```
    pub fn new<F: Fn() -> T + Sync + Send + 'static>(
        initial_capacity: usize,
        maximum_capacity: usize,
        create: F,
    ) -> BundledPool<T> {
        assert!(
            initial_capacity <= maximum_capacity,
            "initial_capacity ({}) must be <= maximum_capacity ({})",
            initial_capacity,
            maximum_capacity
        );

        let items = ArrayQueue::new(maximum_capacity);

        // Pre-allocate objects more efficiently
        for _ in 0..initial_capacity {
            let obj = create();
            // This should never fail due to our assertion above
            if items.push(obj).is_err() {
                unreachable!("invariant: items.len() always less than maximum_capacity");
            }
        }

        let data = PoolData {
            items,
            create: Box::new(create),
        };

        BundledPool {
            data: Arc::new(data),
        }
    }

    /// Takes an item from the pool, creating one if none are available.
    ///
    /// This method always succeeds but may allocate a new object if the pool is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use asyn_object_pool::{BundledPool, Resettable};
    ///
    /// #[derive(Debug)]
    /// struct Item { id: u32 }
    ///
    /// impl Resettable for Item {
    ///     fn reset(&mut self) {}
    /// }
    ///
    /// let pool = BundledPool::new(1, 2, || Item { id: 42 });
    ///
    /// let item1 = pool.take();
    /// let item2 = pool.take(); // Creates new object since pool is empty
    ///
    /// assert_eq!(item1.id, 42);
    /// assert_eq!(item2.id, 42);
    /// ```
    #[inline]
    pub fn take(&self) -> BundledPoolItem<T> {
        let object = self
            .data
            .items
            .pop()
            .unwrap_or_else(|| (self.data.create)());

        BundledPoolItem {
            data: Arc::downgrade(&self.data),
            object: Some(object),
        }
    }

    /// Attempts to take an item from the pool without allocating.
    ///
    /// Returns `None` if no objects are available in the pool.
    ///
    /// # Examples
    ///
    /// ```
    /// use asyn_object_pool::{BundledPool, Resettable};
    ///
    /// #[derive(Debug)]
    /// struct Resource;
    ///
    /// impl Resettable for Resource {
    ///     fn reset(&mut self) {}
    /// }
    ///
    /// let pool = BundledPool::new(1, 2, || Resource);
    ///
    /// // First try_take succeeds
    /// let item = pool.try_take();
    /// assert!(item.is_some());
    ///
    /// // Second try_take fails (pool is empty)
    /// let item2 = pool.try_take();
    /// assert!(item2.is_none());
    /// ```
    #[inline]
    pub fn try_take(&self) -> Option<BundledPoolItem<T>> {
        self.data.items.pop().map(|object| BundledPoolItem {
            data: Arc::downgrade(&self.data),
            object: Some(object),
        })
    }

    /// returns the number of free objects in the pool.
    #[inline]
    pub fn available(&self) -> usize {
        self.data.items.len()
    }

    /// returns the number of objects currently in use. does not include objects that have been detached.
    #[inline]
    pub fn used(&self) -> usize {
        Arc::weak_count(&self.data)
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.data.items.capacity()
    }
}

impl<T: Resettable> Clone for BundledPool<T>
where
    T: Debug,
{
    #[inline]
    fn clone(&self) -> Self {
        Self {
            data: Arc::clone(&self.data),
        }
    }
}

// data shared by a `BundledPool`.
struct PoolData<T> {
    items: ArrayQueue<T>,
    create: Box<dyn Fn() -> T + Sync + Send + 'static>,
}

impl<T: Resettable + Debug> Debug for PoolData<T> {
    fn fmt(&self, formatter: &mut Formatter) -> Result<(), std::fmt::Error> {
        formatter
            .debug_struct("PoolData")
            .field("items", &self.items)
            .field("create", &"Box<dyn Fn() -> T>")
            .finish()
    }
}

/// an object, checked out from a dynamic pool object.
#[derive(Debug)]
pub struct BundledPoolItem<T: Resettable> {
    data: Weak<PoolData<T>>,
    object: Option<T>,
}

impl<T: Resettable> BundledPoolItem<T> {
    #[inline]
    pub fn into_arc(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Detaches this instance from the pool, returning the inner object.
    ///
    /// The detached object will not be returned to the pool when dropped.
    ///
    /// # Examples
    ///
    /// ```
    /// use asyn_object_pool::{BundledPool, Resettable};
    ///
    /// #[derive(Debug, PartialEq)]
    /// struct Data { value: i32 }
    ///
    /// impl Resettable for Data {
    ///     fn reset(&mut self) { self.value = 0; }
    /// }
    ///
    /// let pool = BundledPool::new(1, 2, || Data { value: 42 });
    /// let item = pool.take();
    ///
    /// // Detach the object
    /// let data = item.detach();
    /// assert_eq!(data.value, 42);
    ///
    /// // Pool doesn't get the object back
    /// assert_eq!(pool.available(), 0);
    /// ```
    #[inline]
    pub fn detach(mut self) -> T {
        self.object
            .take()
            .expect("invariant: object is always `some`.")
    }
}

impl<T: Resettable> AsRef<T> for BundledPoolItem<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        self.object
            .as_ref()
            .expect("invariant: object is always `some`.")
    }
}

impl<T: Resettable> Deref for BundledPoolItem<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &T {
        self.object
            .as_ref()
            .expect("invariant: object is always `some`.")
    }
}

impl<T: Resettable> DerefMut for BundledPoolItem<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        self.object
            .as_mut()
            .expect("invariant: object is always `some`.")
    }
}

impl<T: Resettable> Drop for BundledPoolItem<T> {
    fn drop(&mut self) {
        if let Some(mut object) = self.object.take() {
            object.reset();
            if let Some(pool) = self.data.upgrade() {
                // Ignore the result - if the pool is full, we just drop the object
                let _ = pool.items.push(object);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[derive(Debug, PartialEq)]
    struct TestObj {
        value: usize,
    }

    impl Resettable for TestObj {
        fn reset(&mut self) {
            self.value = 0;
        }
    }

    fn make_test_obj(val: usize) -> TestObj {
        TestObj { value: val }
    }

    #[test]
    fn test_pool_creation_and_take() {
        let pool = BundledPool::new(2, 4, move || make_test_obj(42));
        assert_eq!(pool.available(), 2);

        let item = pool.take();
        assert_eq!(item.value, 42);
        assert_eq!(pool.available(), 1);
    }

    #[test]
    fn test_try_take_some() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 42 });
        let item = pool.try_take();
        assert!(item.is_some());
        assert_eq!(item.as_ref().unwrap().value, 42);
    }

    #[test]
    fn test_pool_return_and_reset() {
        let pool = BundledPool::new(1, 2, move || make_test_obj(7));
        {
            let mut item = pool.take();
            item.value = 99;
            // item dropped here, should be reset and returned to pool
        }
        assert_eq!(pool.available(), 1);
        let item = pool.take();
        assert_eq!(item.value, 0); // Should be reset
    }

    #[test]
    fn test_pool_detach() {
        let pool = BundledPool::new(1, 2, move || make_test_obj(5));
        let item = pool.take();
        let obj = item.detach();
        assert_eq!(obj.value, 5);
        assert_eq!(pool.available(), 0); // Not returned to pool
    }

    #[test]
    fn test_try_take_none() {
        let pool = BundledPool::new(0, 1, move || make_test_obj(1));
        assert!(pool.try_take().is_none());
    }

    #[test]
    fn test_pool_capacity() {
        let pool = BundledPool::new(1, 3, move || make_test_obj(1));
        assert_eq!(pool.capacity(), 3);
    }

    #[test]
    fn test_arc_sharing_and_thread_safety() {
        let pool = Arc::new(BundledPool::new(1, 2, move || make_test_obj(123)));

        let mut handles = vec![];
        for _ in 0..10 {
            let pool = Arc::clone(&pool);
            handles.push(thread::spawn(move || {
                let mut item = pool.take();
                item.value += 1;
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        // All items dropped and returned, pool should not panic or deadlock
        assert!(pool.available() <= 2);
    }

    #[test]
    fn test_pool_clone() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 2 });
        let pool_clone = pool.clone();
        assert_eq!(pool.available(), pool_clone.available());

        let _item = pool_clone.take();
        assert_eq!(pool.available(), 0);
        assert_eq!(pool_clone.available(), 0);
    }

    #[test]
    fn test_pool_data_debug_fmt() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 3 });
        let debug_str = format!("{:?}", pool);
        assert!(debug_str.contains("PoolData"));
        assert!(debug_str.contains("items"));
        assert!(debug_str.contains("Box<dyn Fn() -> T>"));
    }

    #[test]
    fn test_used_count() {
        let pool = BundledPool::new(2, 4, move || TestObj { value: 1 });
        assert_eq!(pool.used(), 0);

        let item1 = pool.take();
        assert_eq!(pool.used(), 1);

        let item2 = pool.take();
        assert_eq!(pool.used(), 2);

        drop(item1);
        assert_eq!(pool.used(), 1);

        drop(item2);
        assert_eq!(pool.used(), 0);
    }

    #[test]
    fn test_bundled_pool_item_as_ref() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 99 });
        let item = pool.take();
        let inner_ref = item.as_ref();
        assert_eq!(inner_ref.value, 99);
    }

    #[test]
    fn test_bundled_pool_item_into_arc() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 123 });
        let item = pool.take();
        let arc_item = item.into_arc();
        assert_eq!(arc_item.value, 123);
    }

    #[test]
    fn test_bundled_pool_item_detach_comprehensive() {
        let pool = BundledPool::new(2, 4, move || TestObj { value: 42 });

        // Test detach removes item from pool tracking
        let initial_available = pool.available();
        let item = pool.take();
        assert_eq!(pool.available(), initial_available - 1);
        assert_eq!(pool.used(), 1);

        // Detach the item
        let detached_obj = item.detach();
        assert_eq!(detached_obj.value, 42);

        // Pool should not track the detached item anymore
        assert_eq!(pool.used(), 0);
        assert_eq!(pool.available(), initial_available - 1); // Still one less available

        // Detached object should not be returned to pool when dropped
        drop(detached_obj);
        assert_eq!(pool.available(), initial_available - 1); // Still one less
    }

    #[test]
    fn test_bundled_pool_item_detach_modified_object() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 10 });

        let mut item = pool.take();
        item.value = 999; // Modify the object

        let detached_obj = item.detach();
        assert_eq!(detached_obj.value, 999); // Should preserve modifications

        // Take another item - should be newly created, not the modified one
        let new_item = pool.take();
        assert_eq!(new_item.value, 10); // Fresh object from factory
    }

    #[test]
    fn test_bundled_pool_item_deref() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 777 });
        let item = pool.take();

        // Test deref allows direct access to inner object
        assert_eq!(item.value, 777);

        // Test deref works with method calls
        let debug_str = format!("{:?}", *item);
        assert!(debug_str.contains("777"));

        // Test deref works with pattern matching
        match item.value {
            777 => assert!(true),
            _ => panic!("Deref not working correctly"),
        }
    }

    #[test]
    fn test_bundled_pool_item_deref_mut() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 100 });
        let mut item = pool.take();

        // Test deref_mut allows modification
        item.value = 200;
        assert_eq!(item.value, 200);

        // Test deref_mut with compound assignment
        item.value += 50;
        assert_eq!(item.value, 250);

        // Test deref_mut with method that takes &mut self
        let old_value = item.value;
        item.value *= 2;
        assert_eq!(item.value, old_value * 2);

        // When dropped, should be reset and returned to pool
        drop(item);

        let new_item = pool.take();
        assert_eq!(new_item.value, 0); // Should be reset
    }

    #[test]
    fn test_bundled_pool_item_as_ref_comprehensive() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 555 });
        let item = pool.take();

        // Test as_ref returns correct reference
        let obj_ref: &TestObj = item.as_ref();
        assert_eq!(obj_ref.value, 555);

        // Test as_ref can be used in generic contexts
        fn check_value<T: AsRef<TestObj>>(item: &T) -> usize {
            item.as_ref().value
        }

        assert_eq!(check_value(&item), 555);

        // Test as_ref doesn't consume the item
        let _ref1 = item.as_ref();
        let _ref2 = item.as_ref(); // Should work multiple times
        assert_eq!(item.value, 555); // Item still accessible
    }

    #[test]
    fn test_bundled_pool_item_trait_interactions() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 333 });
        let mut item = pool.take();

        // Test that all traits work together
        let as_ref_value = item.as_ref().value;
        let deref_value = item.value;
        assert_eq!(as_ref_value, deref_value);

        // Modify through deref_mut
        item.value = 444;

        // Check through as_ref
        assert_eq!(item.as_ref().value, 444);

        // Check through deref
        assert_eq!(item.value, 444);

        // Detach and verify final state
        let detached = item.detach();
        assert_eq!(detached.value, 444);
    }

    #[test]
    fn test_bundled_pool_item_multiple_deref_patterns() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 888 });
        let mut item = pool.take();

        // Test various deref patterns
        assert_eq!((*item).value, 888);
        assert_eq!(item.value, 888);

        // Test deref_mut patterns
        (*item).value = 999;
        assert_eq!(item.value, 999);

        item.value = 111;
        assert_eq!((*item).value, 111);
    }

    #[test]
    fn test_bundled_pool_item_as_ref_with_generics() {
        let pool = BundledPool::new(1, 2, move || TestObj { value: 666 });
        let item = pool.take();

        // Test as_ref works in generic function
        fn process_as_ref<T>(item: T) -> usize
        where
            T: AsRef<TestObj>,
        {
            item.as_ref().value
        }

        // Test as_ref works with borrowing
        fn process_as_ref_borrowed<T>(item: &T) -> usize
        where
            T: AsRef<TestObj>,
        {
            item.as_ref().value
        }

        assert_eq!(process_as_ref_borrowed(&item), 666);
        assert_eq!(process_as_ref(item), 666);
    }
}
