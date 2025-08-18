use asyn_object_pool::{BundledPool, Resettable};
use rand;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::time::sleep; // Add this import

// Simplified error type
#[derive(Error, Debug)]
pub enum ExampleError {
    #[error("Example {0} error: {1}")]
    Error(String, String),
}

// Example 1: Database Connection Pool Simulation
#[derive(Debug)]
struct DatabaseConnection {
    id: u32,
    connected: bool,
    query_count: u32,
}

impl DatabaseConnection {
    fn new(id: u32) -> Self {
        println!("Creating new database connection {}", id);
        Self {
            id,
            connected: true,
            query_count: 0,
        }
    }

    async fn execute_query(&mut self, query: &str) -> Result<String, ExampleError> {
        if !self.connected {
            return Err(ExampleError::Error(
                "Database".to_string(),
                "Connection not established".to_string(),
            ));
        }

        // Simulate query execution time
        sleep(Duration::from_millis(10)).await;
        self.query_count += 1;

        // Simulate occasional query failures
        if self.query_count % 7 == 0 {
            return Err(ExampleError::Error(
                "Database".to_string(),
                format!("Query execution failed: {}", query),
            ));
        }

        Ok(format!(
            "Query '{}' executed on connection {}",
            query, self.id
        ))
    }
}

impl Resettable for DatabaseConnection {
    fn reset(&mut self) {
        // Reset connection state for reuse
        self.query_count = 0;
        self.connected = true;
        println!("Reset database connection {}", self.id);
    }
}

// Example 2: HTTP Client Pool
#[derive(Debug)]
struct HttpClient {
    base_url: String,
    request_count: u32,
    timeout: Duration,
}

impl HttpClient {
    fn new(base_url: String) -> Self {
        println!("Creating new HTTP client for {}", base_url);
        Self {
            base_url,
            request_count: 0,
            timeout: Duration::from_secs(30),
        }
    }

    async fn get(&mut self, path: &str) -> Result<String, ExampleError> {
        // Simulate HTTP request
        sleep(Duration::from_millis(50)).await;
        self.request_count += 1;

        // Simulate occasional HTTP errors
        if self.request_count % 5 == 0 {
            return Err(ExampleError::Error(
                "HTTP".to_string(),
                format!("Request failed with status: 500"),
            ));
        }

        if path.contains("timeout") {
            return Err(ExampleError::Error(
                "HTTP".to_string(),
                "Network timeout".to_string(),
            ));
        }

        Ok(format!(
            "GET {}{} - Response from request #{}",
            self.base_url, path, self.request_count
        ))
    }
}

impl Resettable for HttpClient {
    fn reset(&mut self) {
        self.request_count = 0;
        self.timeout = Duration::from_secs(30);
        println!("Reset HTTP client for {}", self.base_url);
    }
}

// Example 3: Buffer Pool for processing data
#[derive(Debug)]
struct ProcessingBuffer {
    data: Vec<u8>,
    processed_items: usize,
    capacity: usize,
}

impl ProcessingBuffer {
    fn new() -> Self {
        Self {
            data: Vec::with_capacity(1024),
            processed_items: 0,
            capacity: 1024,
        }
    }

    async fn process_data(&mut self, input: &[u8]) -> Result<usize, ExampleError> {
        // Simulate data processing
        sleep(Duration::from_millis(5)).await;

        if self.data.len() + input.len() > self.capacity {
            return Err(ExampleError::Error(
                "Processing".to_string(),
                format!(
                    "Buffer overflow: capacity {}, attempted {}",
                    self.capacity,
                    self.data.len() + input.len()
                ),
            ));
        }

        self.data.extend_from_slice(input);
        self.processed_items += 1;

        Ok(self.data.len())
    }
}

impl Resettable for ProcessingBuffer {
    fn reset(&mut self) {
        self.data.clear();
        self.processed_items = 0;
    }
}

async fn database_pool_example() -> Result<(), ExampleError> {
    use std::sync::atomic::{AtomicU32, Ordering};

    let connection_id = Arc::new(AtomicU32::new(0));
    let db_pool = Arc::new(BundledPool::new(
        2, // initial capacity
        5, // maximum capacity
        {
            let connection_id = Arc::clone(&connection_id);
            move || {
                let id = connection_id.fetch_add(1, Ordering::SeqCst) + 1;
                DatabaseConnection::new(id)
            }
        },
    ));

    println!(
        "Created database pool with {} available connections",
        db_pool.available()
    );

    // Simulate multiple concurrent database operations
    let mut handles = vec![];

    for i in 0..10 {
        let pool = Arc::clone(&db_pool);
        let handle = tokio::spawn(async move {
            let mut conn = pool.take();
            let query = format!("SELECT * FROM users WHERE id = {}", i);

            match conn.execute_query(&query).await {
                Ok(result) => {
                    println!("Task {}: {}", i, result);
                    Ok(())
                }
                Err(e) => {
                    println!("Task {}: Error - {}", i, e);
                    Err(e)
                }
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete and collect results
    let mut success_count = 0;
    let mut error_count = 0;

    for handle in handles {
        match handle
            .await
            .map_err(|e| ExampleError::Error("Tokio".to_string(), e.to_string()))?
        {
            Ok(_) => success_count += 1,
            Err(_) => error_count += 1,
        }
    }

    println!(
        "Database operations completed: {} successful, {} failed",
        success_count, error_count
    );
    println!(
        "Final pool state - Available: {}, Used: {}",
        db_pool.available(),
        db_pool.used()
    );

    Ok(())
}

async fn http_client_pool_example() -> Result<(), ExampleError> {
    let client_pool = Arc::new(BundledPool::new(
        1, // initial capacity
        3, // maximum capacity
        || HttpClient::new("https://api.example.com".to_string()),
    ));

    println!(
        "Created HTTP client pool with {} available clients",
        client_pool.available()
    );

    // Simulate concurrent HTTP requests with some that will fail
    let endpoints = vec!["/users", "/posts", "/timeout", "/comments", "/profile"];
    let mut handles = vec![];

    for (i, endpoint) in endpoints.iter().enumerate() {
        let pool = Arc::clone(&client_pool);
        let endpoint = endpoint.to_string();

        let handle = tokio::spawn(async move {
            let mut client = pool.take();

            match client.get(&endpoint).await {
                Ok(response) => {
                    println!("Request {}: {}", i, response);
                    Ok(())
                }
                Err(e) => {
                    println!("Request {}: Error - {}", i, e);
                    Err(e)
                }
            }
        });
        handles.push(handle);
    }

    let mut success_count = 0;
    let mut error_count = 0;

    for handle in handles {
        match handle
            .await
            .map_err(|e| ExampleError::Error("Tokio".to_string(), e.to_string()))?
        {
            Ok(_) => success_count += 1,
            Err(_) => error_count += 1,
        }
    }

    println!(
        "HTTP requests completed: {} successful, {} failed",
        success_count, error_count
    );
    println!(
        "HTTP client pool - Available: {}, Used: {}",
        client_pool.available(),
        client_pool.used()
    );

    Ok(())
}

async fn concurrent_processing_example() -> Result<(), ExampleError> {
    let buffer_pool = Arc::new(BundledPool::new(
        2, // initial capacity
        4, // maximum capacity
        ProcessingBuffer::new,
    ));

    println!(
        "Created processing buffer pool with {} available buffers",
        buffer_pool.available()
    );

    // Simulate concurrent data processing with some oversized chunks
    let data_chunks: Vec<Vec<u8>> = (0..8)
        .map(|i| {
            if i == 5 {
                vec![i as u8; 2000] // This will cause a buffer overflow
            } else {
                vec![i as u8; 100] // Normal size
            }
        })
        .collect();

    let mut handles = vec![];

    for (i, chunk) in data_chunks.into_iter().enumerate() {
        let pool = Arc::clone(&buffer_pool);

        let handle = tokio::spawn(async move {
            let mut buffer = pool.take();

            match buffer.process_data(&chunk).await {
                Ok(processed_size) => {
                    println!(
                        "Processed chunk {} - Buffer size: {} bytes",
                        i, processed_size
                    );
                    Ok(())
                }
                Err(e) => {
                    println!("Failed to process chunk {}: {}", i, e);
                    Err(e)
                }
            }
        });
        handles.push(handle);
    }

    let mut success_count = 0;
    let mut error_count = 0;

    for handle in handles {
        match handle
            .await
            .map_err(|e| ExampleError::Error("Tokio".to_string(), e.to_string()))?
        {
            Ok(_) => success_count += 1,
            Err(_) => error_count += 1,
        }
    }

    println!(
        "Concurrent processing completed: {} successful, {} failed",
        success_count, error_count
    );
    println!(
        "Buffer pool - Available: {}, Used: {}",
        buffer_pool.available(),
        buffer_pool.used()
    );

    Ok(())
}

async fn stress_test_example() -> Result<(), ExampleError> {
    let pool = Arc::new(BundledPool::new(
        5,  // initial capacity
        10, // maximum capacity
        || DatabaseConnection::new(rand::random::<u32>()),
    ));

    println!("Starting stress test with {} tasks...", 100);
    let start_time = Instant::now();

    let mut handles = vec![];

    // Create 100 concurrent tasks
    for i in 0..100 {
        let pool = Arc::clone(&pool);

        let handle = tokio::spawn(async move {
            // Each task performs multiple operations
            for j in 0..5 {
                let mut conn = pool.take();
                let query = format!("SELECT * FROM table_{} WHERE id = {}", i, j);

                if let Ok(result) = conn.execute_query(&query).await {
                    if i % 20 == 0 && j == 0 {
                        // Print some results to show progress
                        println!("Task {}: {}", i, result);
                    }
                }

                // Small delay between operations
                sleep(Duration::from_millis(1)).await;
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        handle
            .await
            .map_err(|e| ExampleError::Error("Tokio".to_string(), e.to_string()))?;
    }

    let duration = start_time.elapsed();
    println!("Stress test completed in {:?}", duration);
    println!(
        "Final pool state - Available: {}, Used: {}, Capacity: {}",
        pool.available(),
        pool.used(),
        pool.capacity()
    );

    Ok(())
}

// Example of using the pool with detached objects
async fn detached_object_example() -> Result<(), ExampleError> {
    let pool = Arc::new(BundledPool::new(1, 3, || {
        HttpClient::new("https://detached.example.com".to_string())
    }));

    println!("=== Detached Object Example ===");

    // Take an object and detach it
    let client_item = pool.take();
    let mut detached_client = client_item.detach(); // This removes it from pool tracking

    println!(
        "Pool after detach - Available: {}, Used: {}",
        pool.available(),
        pool.used()
    );

    // Use the detached client
    let response = detached_client.get("/detached-endpoint").await?;
    println!("Detached client response: {}", response);

    // The detached client won't be returned to the pool when dropped
    drop(detached_client);

    println!(
        "Pool after dropping detached client - Available: {}, Used: {}",
        pool.available(),
        pool.used()
    );

    Ok(())
}

// Example showing try_take usage
async fn try_take_example() -> Result<(), ExampleError> {
    let pool = Arc::new(BundledPool::new(
        0, // No initial objects
        2, // Small capacity
        || ProcessingBuffer::new(),
    ));

    println!("=== Try Take Example ===");
    println!("Pool starts empty - Available: {}", pool.available());

    // try_take on empty pool returns None
    match pool.try_take() {
        Some(_) => println!("Got an object (unexpected)"),
        None => println!("No objects available (expected)"),
    }

    // take() will create a new object
    let buffer1 = pool.take();
    println!(
        "After take() - Available: {}, Used: {}",
        pool.available(),
        pool.used()
    );

    // try_take still returns None because the object is in use
    match pool.try_take() {
        Some(_) => println!("Got an object (unexpected)"),
        None => println!("No objects available - object is in use"),
    }

    // Drop the first buffer to return it to pool
    drop(buffer1);
    println!(
        "After dropping buffer - Available: {}, Used: {}",
        pool.available(),
        pool.used()
    );

    // Now try_take should succeed
    match pool.try_take() {
        Some(buffer) => {
            println!("Successfully got object with try_take()");
            drop(buffer);
        }
        None => println!("Still no objects available (unexpected)"),
    }

    Ok(())
}

// Example showing pool behavior under different load patterns
async fn load_pattern_example() -> Result<(), ExampleError> {
    let pool = Arc::new(BundledPool::new(1, 3, || {
        HttpClient::new("https://load-test.example.com".to_string())
    }));

    println!("=== Load Pattern Example ===");

    // Pattern 1: Burst load
    println!("Testing burst load pattern...");
    let start = Instant::now();
    let mut handles = vec![];

    for i in 0..10 {
        let pool = Arc::clone(&pool);
        let handle = tokio::spawn(async move {
            let mut client = pool.take();
            let result = client.get(&format!("/burst/{}", i)).await;
            println!("Burst request {}: {:?}", i, result.is_ok());
        });
        handles.push(handle);
    }

    for handle in handles {
        handle
            .await
            .map_err(|e| ExampleError::Error("Tokio".to_string(), e.to_string()))?;
    }
    println!("Burst load completed in {:?}", start.elapsed());

    // Wait a bit for objects to return to pool
    sleep(Duration::from_millis(100)).await;

    // Pattern 2: Sustained load
    println!("Testing sustained load pattern...");
    let start = Instant::now();

    for batch in 0..3 {
        let mut batch_handles = vec![];

        for i in 0..5 {
            let pool = Arc::clone(&pool);
            let handle = tokio::spawn(async move {
                let mut client = pool.take();
                let result = client.get(&format!("/sustained/{}/{}", batch, i)).await;
                println!("Sustained request {}-{}: {:?}", batch, i, result.is_ok());
            });
            batch_handles.push(handle);
        }

        // Wait for this batch to complete before starting the next
        for handle in batch_handles {
            handle
                .await
                .map_err(|e| ExampleError::Error("Tokio".to_string(), e.to_string()))?;
        }

        sleep(Duration::from_millis(50)).await; // Small delay between batches
    }

    println!("Sustained load completed in {:?}", start.elapsed());
    println!(
        "Final pool state - Available: {}, Used: {}",
        pool.available(),
        pool.used()
    );

    Ok(())
}

// Example demonstrating error handling with the pool
async fn error_handling_example() -> Result<(), ExampleError> {
    #[derive(Debug)]
    struct FlakyConnection {
        id: u32,
        failure_rate: f32,
        call_count: u32,
    }

    impl FlakyConnection {
        fn new(id: u32) -> Self {
            Self {
                id,
                failure_rate: 0.3, // 30% failure rate
                call_count: 0,
            }
        }

        async fn unreliable_operation(&mut self) -> Result<String, &'static str> {
            self.call_count += 1;
            sleep(Duration::from_millis(10)).await;

            if rand::random::<f32>() < self.failure_rate {
                Err("Operation failed")
            } else {
                Ok(format!(
                    "Success from connection {} (call #{})",
                    self.id, self.call_count
                ))
            }
        }
    }

    impl Resettable for FlakyConnection {
        fn reset(&mut self) {
            self.call_count = 0;
            // Don't reset failure_rate to maintain realistic behavior
        }
    }

    let pool = Arc::new(BundledPool::new(2, 4, || {
        FlakyConnection::new(rand::random::<u32>() % 1000)
    }));

    println!("=== Error Handling Example ===");
    println!("Testing with flaky connections (30% failure rate)...");

    let mut success_count = 0;
    let mut failure_count = 0;
    let mut handles = vec![];

    for i in 0..20 {
        let pool = Arc::clone(&pool);
        let handle = tokio::spawn(async move {
            let mut conn = pool.take();

            // Retry logic
            for attempt in 1..=3 {
                match conn.unreliable_operation().await {
                    Ok(result) => {
                        println!("Task {}: {} (attempt {})", i, result, attempt);
                        return Ok(());
                    }
                    Err(e) => {
                        if attempt < 3 {
                            println!("Task {}: {} - retrying... (attempt {})", i, e, attempt);
                            sleep(Duration::from_millis(10)).await;
                        } else {
                            println!("Task {}: {} - giving up after {} attempts", i, e, attempt);
                            return Err(e);
                        }
                    }
                }
            }
            Err("Max retries exceeded")
        });
        handles.push(handle);
    }

    for handle in handles {
        match handle
            .await
            .map_err(|e| ExampleError::Error("Tokio".to_string(), e.to_string()))?
        {
            Ok(_) => success_count += 1,
            Err(_) => failure_count += 1,
        }
    }

    println!(
        "Results: {} successes, {} failures",
        success_count, failure_count
    );
    println!(
        "Pool state - Available: {}, Used: {}",
        pool.available(),
        pool.used()
    );

    Ok(())
}

async fn shared_pool_example() -> Result<(), ExampleError> {
    let db_pool = Arc::new(BundledPool::new(2, 5, || {
        DatabaseConnection::new(rand::random::<u32>())
    }));

    println!("=== Shared Pool Example ===");

    // Simulate a web server scenario where different handlers share the same pool
    let user_service_pool = Arc::clone(&db_pool);
    let order_service_pool = Arc::clone(&db_pool);
    let analytics_service_pool = Arc::clone(&db_pool);

    let user_task = tokio::spawn(async move { handle_user_requests(user_service_pool).await });

    let order_task = tokio::spawn(async move { handle_order_requests(order_service_pool).await });

    let analytics_task =
        tokio::spawn(async move { handle_analytics_requests(analytics_service_pool).await });

    // Wait for all services to complete - handle JoinError manually
    let user_result = user_task
        .await
        .map_err(|e| ExampleError::Error("Tokio".to_string(), e.to_string()))?;
    let order_result = order_task
        .await
        .map_err(|e| ExampleError::Error("Tokio".to_string(), e.to_string()))?;
    let analytics_result = analytics_task
        .await
        .map_err(|e| ExampleError::Error("Tokio".to_string(), e.to_string()))?;

    user_result?;
    order_result?;
    analytics_result?;

    println!("All services completed successfully");
    println!(
        "Final pool state - Available: {}, Used: {}",
        db_pool.available(),
        db_pool.used()
    );

    Ok(())
}

async fn handle_user_requests(
    pool: Arc<BundledPool<DatabaseConnection>>,
) -> Result<(), ExampleError> {
    for i in 0..3 {
        let mut conn = pool.take();
        let query = format!("SELECT * FROM users WHERE id = {}", i);
        let result = conn.execute_query(&query).await?;
        println!("User Service: {}", result);
        sleep(Duration::from_millis(10)).await;
    }
    Ok(())
}

async fn handle_order_requests(
    pool: Arc<BundledPool<DatabaseConnection>>,
) -> Result<(), ExampleError> {
    for i in 0..3 {
        let mut conn = pool.take();
        let query = format!("SELECT * FROM orders WHERE user_id = {}", i);
        let result = conn.execute_query(&query).await?;
        println!("Order Service: {}", result);
        sleep(Duration::from_millis(15)).await;
    }
    Ok(())
}

async fn handle_analytics_requests(
    pool: Arc<BundledPool<DatabaseConnection>>,
) -> Result<(), ExampleError> {
    for i in 0..2 {
        let mut conn = pool.take();
        let query = format!("SELECT COUNT(*) FROM events WHERE date = '{}'", i);
        let result = conn.execute_query(&query).await?;
        println!("Analytics Service: {}", result);
        sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

fn draw_line() {
    println!("\n{}\n", "=".repeat(50));
}

#[tokio::main]
async fn main() -> Result<(), ExampleError> {
    println!("=== Tokio Async Object Pool Examples ===\n");

    // Example 1: Database Connection Pool
    println!("1. Database Connection Pool Example");
    database_pool_example().await?;

    draw_line();

    // Example 2: HTTP Client Pool
    println!("2. HTTP Client Pool Example");
    http_client_pool_example().await?;

    draw_line();

    // Example 3: Concurrent Processing with Buffer Pool
    println!("3. Concurrent Processing Example");
    concurrent_processing_example().await?;

    draw_line();

    // Example 4: High Concurrency Stress Test
    println!("4. High Concurrency Stress Test");
    stress_test_example().await?;

    draw_line();

    // Example 5: Detached Objects
    println!("5. Detached Object Example");
    detached_object_example().await?;

    draw_line();

    // Example 6: Try Take Usage
    println!("6. Try Take Example");
    try_take_example().await?;

    draw_line();

    // Example 7: Shared Pool Across Services
    println!("7. Shared Pool Example");
    shared_pool_example().await?;

    draw_line();

    // Example 8: Load Patterns
    println!("8. Load Pattern Example");
    load_pattern_example().await?;

    draw_line();

    // Example 9: Error Handling
    println!("9. Error Handling Example");
    error_handling_example().await?;

    println!("\n=== All Examples Completed Successfully! ===");

    Ok(())
}
