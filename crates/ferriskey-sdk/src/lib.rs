//! FerrisKey Rust SDK
//!
//! ## Design Philosophy
//!
//! This SDK is built with a focus on:
//!
//! 1. **Zero Custom Macros**: All abstractions use native Rust generics, traits, and type-state
//!    patterns. No procedural macros are introduced.
//!
//! 2. **Type-Driven Design**: Invalid states are unrepresentable at compile time. The builder
//!    pattern uses phantom types to track configuration state.
//!
//! 3. **tower::Service Integration**: The transport layer is built on `tower::Service`, enabling
//!    seamless composition of middleware (retry, timeout, rate-limiting).
//!
//! 4. **Extension Traits**: Functionality is organized via extension traits, allowing opt-in
//!    features without bloating the core API.
//!
//! ## Quick Start
//!
//! ```no_run
//! use ferriskey_sdk::prelude::*;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Configure the SDK
//! let config = SdkConfig::builder("https://api.ferriskey.com")
//!     .auth(AuthStrategy::Bearer("your-token".into()))
//!     .build();
//!
//! // Create SDK with transport
//! let sdk = FerriskeySdk::builder(config).transport(HpxTransport::default()).build();
//!
//! // Execute operations
//! let input = OperationInput::builder().path_param("realm", "master").build();
//!
//! let response = sdk.execute_operation("getRealm", input).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Middleware Composition
//!
//! ```no_run
//! use ferriskey_sdk::{AuthStrategy, FerriskeySdk, HpxTransport, SdkConfig};
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = SdkConfig::builder("https://api.ferriskey.com")
//!     .auth(AuthStrategy::Bearer("your-token".into()))
//!     .build();
//!
//! let transport = HpxTransport::default();
//!
//! let sdk = FerriskeySdk::builder(config).transport(transport).build();
//! # Ok(())
//! # }
//! ```

pub mod cli;
pub mod client;
pub mod config;
/// Build-time contract normalization and registry helpers.
pub mod contract;
mod encoding;
pub mod error;
pub mod generated;
pub mod transport;

// Re-export core types for ergonomic imports
pub use client::{
    Configured, FerriskeySdk, FerriskeySdkBuilder, OperationCall, OperationInput,
    OperationInputBuilder, SdkExt, TagClient, Unconfigured,
};
pub use config::{AuthStrategy, AuthStrategyExt, BaseUrlSet, SdkConfig, SdkConfigBuilder};
pub use encoding::DecodedResponse;
pub use error::{SdkError, TransportError};
pub use generated::{OPERATION_COUNT, OPERATION_DESCRIPTORS, PATH_COUNT, SCHEMA_COUNT, TAG_NAMES};
// Re-export tower for middleware composition
pub use tower;
pub use transport::{
    HpxTransport, MethodSet, PathSet, SdkRequest, SdkRequestBuilder, SdkResponse, Transport,
    TransportExt,
};

/// A line item in the shopping cart.
///
/// ## Immutability
///
/// `CartItem` is immutable once created. Use the builder pattern for
/// complex item construction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CartItem {
    /// Human-readable item name.
    pub name: String,
    /// Price per unit in cents.
    pub price_cents: u32,
    /// Selected quantity.
    pub quantity: u32,
}

impl CartItem {
    /// Create a new cart item.
    #[must_use]
    pub fn new(name: impl Into<String>, price_cents: u32, quantity: u32) -> Self {
        Self { name: name.into(), price_cents, quantity }
    }

    /// Calculate the total price for this line item.
    #[must_use]
    pub const fn total_cents(&self) -> u32 {
        self.price_cents * self.quantity
    }
}

/// A shopping cart.
///
/// ## Type-State Pattern for Cart Lifecycle
///
/// The cart can be extended with type-state markers to enforce valid
/// transitions (e.g., empty → populated → checked out).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Cart {
    /// Items currently in the cart.
    pub items: Vec<CartItem>,
}

impl Cart {
    /// Create an empty cart.
    #[must_use]
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Add an item to the cart.
    pub fn add_item(&mut self, item: CartItem) {
        self.items.push(item);
    }

    /// Calculate the total cart value in cents.
    #[must_use]
    pub fn total_cents(&self) -> u32 {
        self.items.iter().map(CartItem::total_cents).sum()
    }

    /// Check if the cart is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// An order created from checkout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Order {
    /// Items included in the order.
    pub items: Vec<CartItem>,
    /// Total order value in cents.
    pub total_cents: u32,
}

impl Order {
    /// Create a new order from items.
    #[must_use]
    pub fn from_items(items: Vec<CartItem>) -> Self {
        let total_cents = items.iter().map(CartItem::total_cents).sum();
        Self { items, total_cents }
    }
}

/// The result of checking out a cart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckoutResult {
    /// The newly created order.
    pub order: Order,
    /// The emptied cart after checkout.
    pub cart: Cart,
}

/// Builds a temporary greeting string for the CLI scaffold.
#[must_use]
pub fn greeting(name: &str) -> String {
    format!("Hello, {name}!")
}

/// Creates an order from the provided items and clears the cart.
///
/// ## Functional Style
///
/// This function is pure—it takes items and returns a result without
/// side effects. The cart is not mutated; a new empty cart is returned.
#[must_use]
pub fn checkout_cart(items: &[CartItem]) -> CheckoutResult {
    CheckoutResult { order: Order::from_items(items.to_vec()), cart: Cart::new() }
}

/// Re-exports commonly used items.
///
/// ## Design Decision: Prelude Module
///
/// A prelude module provides a single import point for the most commonly
/// used types, reducing import boilerplate in user code.
pub mod prelude {
    pub use crate::{
        // Config & Auth
        AuthStrategy,
        AuthStrategyExt,
        Cart,
        CartItem,
        CheckoutResult,
        // Client
        FerriskeySdk,
        // Transport
        HpxTransport,
        OperationInput,
        OperationInputBuilder,
        Order,
        SdkConfig,
        // Errors
        SdkError,
        SdkRequest,
        SdkResponse,
        Transport,
        TransportError,
        TransportExt,
        // Functions
        checkout_cart,
        greeting,
    };
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        future::Future,
        pin::Pin,
        sync::{Arc, Mutex},
    };

    use proptest::prelude::*;
    use serde::Deserialize;
    use tower::Service;

    use crate::{
        AuthStrategy, CartItem, FerriskeySdk, SdkConfig, SdkError, SdkRequest, SdkResponse,
        TransportError, checkout_cart, greeting,
    };

    #[derive(Clone, Debug)]
    struct MockTransport {
        captured_requests: Arc<Mutex<Vec<SdkRequest>>>,
        response: SdkResponse,
    }

    impl MockTransport {
        fn new(response: SdkResponse) -> Self {
            Self { captured_requests: Arc::new(Mutex::new(Vec::new())), response }
        }

        fn captured_requests(&self) -> Vec<SdkRequest> {
            self.captured_requests
                .lock()
                .expect("captured requests mutex should not be poisoned")
                .clone()
        }
    }

    /// Implement tower::Service for MockTransport
    /// This demonstrates that any Service<SdkRequest> is automatically a Transport
    impl Service<SdkRequest> for MockTransport {
        type Response = SdkResponse;
        type Error = TransportError;
        type Future = Pin<Box<dyn Future<Output = Result<SdkResponse, TransportError>> + Send>>;

        fn poll_ready(
            &mut self,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn call(&mut self, request: SdkRequest) -> Self::Future {
            let captured_requests = Arc::clone(&self.captured_requests);
            let response = self.response.clone();

            Box::pin(async move {
                captured_requests
                    .lock()
                    .expect("captured requests mutex should not be poisoned")
                    .push(request);
                Ok(response)
            })
        }
    }

    fn successful_response(body: impl Into<Vec<u8>>) -> SdkResponse {
        SdkResponse { body: body.into(), headers: BTreeMap::new(), status: 200 }
    }

    fn cart_item_strategy() -> impl Strategy<Value = CartItem> {
        ("[A-Za-z][A-Za-z0-9 ]{0,15}", 0_u16..10_000, 0_u16..100).prop_map(
            |(name, price_cents, quantity)| {
                CartItem::new(name, u32::from(price_cents), u32::from(quantity))
            },
        )
    }

    #[test]
    fn greeting_builds_message() {
        assert_eq!(greeting("Rust"), "Hello, Rust!");
    }

    #[test]
    fn checkout_cart_creates_an_order_and_clears_the_cart() {
        let result = checkout_cart(&[CartItem::new("Tea", 450, 2), CartItem::new("Cake", 350, 1)]);

        assert_eq!(result.order.total_cents, 1250);
        assert!(result.cart.items.is_empty());
    }

    #[test]
    fn cart_item_total_calculation() {
        let item = CartItem::new("Widget", 1000, 3);
        assert_eq!(item.total_cents(), 3000);
    }

    #[tokio::test]
    async fn transport_and_auth_core_injects_bearer_header() {
        let transport = MockTransport::new(successful_response(br#"{"ok":true}"#.to_vec()));
        let sdk = FerriskeySdk::new(
            SdkConfig::new(
                "https://api.ferriskey.test",
                AuthStrategy::Bearer("secret-token".to_string()),
            ),
            transport.clone(),
        );
        let mut request = SdkRequest::new("GET", "/realms/test");
        request.requires_auth = true;

        let response = sdk.execute(request).await.expect("request should succeed");
        let captured_requests = transport.captured_requests();

        assert_eq!(response.status, 200);
        assert_eq!(captured_requests.len(), 1);
        assert_eq!(
            captured_requests[0].headers.get("authorization"),
            Some(&"Bearer secret-token".to_string()),
        );
        assert_eq!(captured_requests[0].path, "https://api.ferriskey.test/realms/test");
    }

    #[tokio::test]
    async fn transport_and_auth_core_rejects_missing_auth() {
        let transport = MockTransport::new(successful_response(Vec::new()));
        let sdk = FerriskeySdk::new(
            SdkConfig::new("https://api.ferriskey.test", AuthStrategy::None),
            transport.clone(),
        );
        let mut request = SdkRequest::new("GET", "/realms/secured");
        request.requires_auth = true;

        let error = sdk.execute(request).await.expect_err("missing auth should fail");

        assert!(matches!(error, SdkError::MissingAuth));
        assert!(transport.captured_requests().is_empty());
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct ExamplePayload {
        ok: bool,
    }

    #[tokio::test]
    async fn transport_and_auth_core_reports_status_body_mismatch() {
        let transport = MockTransport::new(successful_response(br#"not-json"#.to_vec()));
        let sdk = FerriskeySdk::new(
            SdkConfig::new("https://api.ferriskey.test", AuthStrategy::None),
            transport,
        );
        let request = SdkRequest::new("GET", "/realms/test");

        let error = sdk
            .execute_json::<ExamplePayload>(request, 200)
            .await
            .expect_err("invalid JSON should fail decoding");

        assert!(matches!(error, SdkError::Decode(_)));
    }

    proptest! {
        #[test]
        fn checkout_cart_preserves_generated_items(
            items in proptest::collection::vec(cart_item_strategy(), 0..16),
        ) {
            let result = checkout_cart(&items);

            prop_assert_eq!(result.order.items, items);
            prop_assert!(result.cart.items.is_empty());
        }

        #[test]
        fn checkout_cart_total_matches_generated_line_items(
            items in proptest::collection::vec(cart_item_strategy(), 0..16),
        ) {
            let expected_total: u32 = items.iter().map(CartItem::total_cents).sum();
            let result = checkout_cart(&items);

            prop_assert_eq!(result.order.total_cents, expected_total);
        }
    }
}
