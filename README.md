# Async Object Pool

A lock-free, thread-safe, sized object pool for Rust applications, optimized for async environments.

## Features

- **Lock-free**: Uses `crossbeam-queue` for high-performance concurrent access
- **Thread-safe**: Safe to share across multiple async tasks and threads
- **Bounded**: Configurable initial and maximum capacity
- **Auto-reset**: Objects are automatically reset when returned to the pool
- **Detachable**: Objects can be detached from pool tracking when needed

## Quick Start

Add to your `Cargo.toml`:
```toml
[dependencies]
asyn_object_pool = "0.1.0"
```

## Usage

```rust
use asyn_object_pool::{BundledPool, Resettable};
use std::sync::Arc;

#[derive(Debug)]
struct Connection {
    id: u32,
    active: bool,
}

impl Resettable for Connection {
    fn reset(&mut self) {
        self.active = true;
    }
}

#[tokio::main]
async fn main() {
    let pool = Arc::new(BundledPool::new(
        2,  // initial capacity
        5,  // maximum capacity
        || Connection { id: 1, active: true }
    ));

    // Take an object from the pool
    let mut conn = pool.take();
    
    // Use the object
    conn.active = false;
    
    // Object is automatically returned and reset when dropped
}
```

## API Reference

### `BundledPool<T>`

The main pool type for managing objects of type `T` where `T: Resettable + Debug`.

#### Methods

- **`new(initial_capacity, maximum_capacity, create_fn) -> BundledPool<T>`**
  - Creates a new pool with specified capacities and object factory function
  - Panics if `initial_capacity > maximum_capacity`

- **`take() -> BundledPoolItem<T>`**
  - Takes an object from the pool, creating a new one if none available
  - Always succeeds (may allocate)

- **`try_take() -> Option<BundledPoolItem<T>>`**
  - Attempts to take an object from the pool
  - Returns `None` if no objects available (never allocates)

- **`available() -> usize`**
  - Returns the number of free objects in the pool

- **`used() -> usize`**
  - Returns the number of objects currently in use

- **`capacity() -> usize`**
  - Returns the maximum capacity of the pool

### `BundledPoolItem<T>`

A wrapper around pooled objects that automatically returns them to the pool when dropped.

#### Methods

- **`detach(self) -> T`**
  - Removes the object from pool tracking and returns the inner object
  - The object will not be returned to the pool when dropped

- **`into_arc(self) -> Arc<Self>`**
  - Converts the item into an `Arc` for shared ownership

#### Traits

- **`Deref<Target = T>`** - Direct access to the inner object
- **`DerefMut`** - Mutable access to the inner object  
- **`AsRef<T>`** - Reference access to the inner object

### `Resettable` Trait

Objects stored in the pool must implement this trait to be reset when returned.

```rust
pub trait Resettable {
    fn reset(&mut self);
}
```

#### Built-in Implementations

- **`Option<T: Resettable>`** - Resets the inner value if `Some`
- **`(T1: Resettable, T2: Resettable)`** - Resets both tuple elements

## Examples

Run the comprehensive example:
```bash
cargo run --example tokio_example
```

## License

MIT
