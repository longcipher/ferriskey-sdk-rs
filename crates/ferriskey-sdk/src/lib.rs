//! Temporary FerrisKey SDK scaffold.
//!
//! This crate will become the shared Rust SDK for the FerrisKey API.

pub mod cli;
pub mod client;
pub mod config;
/// Build-time contract normalization and registry helpers.
pub mod contract;
mod encoding;
pub mod error;
pub mod generated;
pub mod transport;

pub use client::{FerriskeySdk, OperationCall, OperationInput, TagClient};
pub use config::{AuthStrategy, SdkConfig};
pub use encoding::DecodedResponse;
pub use error::{SdkError, TransportError};
pub use generated::{OPERATION_COUNT, OPERATION_DESCRIPTORS, PATH_COUNT, SCHEMA_COUNT, TAG_NAMES};
pub use transport::{HpxTransport, SdkRequest, SdkResponse, Transport};

/// A line item in the shopping cart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CartItem {
    /// Human-readable item name.
    pub name: String,
    /// Price per unit in cents.
    pub price_cents: u32,
    /// Selected quantity.
    pub quantity: u32,
}

/// A shopping cart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cart {
    /// Items currently in the cart.
    pub items: Vec<CartItem>,
}

/// An order created from checkout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Order {
    /// Items included in the order.
    pub items: Vec<CartItem>,
    /// Total order value in cents.
    pub total_cents: u32,
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
#[must_use]
pub fn checkout_cart(items: &[CartItem]) -> CheckoutResult {
    let order_items = items.to_vec();
    let total_cents = order_items.iter().map(|item| item.price_cents * item.quantity).sum();

    CheckoutResult {
        order: Order { items: order_items, total_cents },
        cart: Cart { items: Vec::new() },
    }
}

/// Re-exports commonly used items.
pub mod prelude {
    pub use crate::{Cart, CartItem, CheckoutResult, Order, checkout_cart, greeting};
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

    use crate::{
        AuthStrategy, CartItem, FerriskeySdk, SdkConfig, SdkError, SdkRequest, SdkResponse,
        Transport, TransportError, checkout_cart, greeting,
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

    impl Transport for MockTransport {
        fn send(
            &self,
            request: SdkRequest,
        ) -> Pin<Box<dyn Future<Output = Result<SdkResponse, TransportError>> + Send + '_>>
        {
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
            |(name, price_cents, quantity)| CartItem {
                name,
                price_cents: u32::from(price_cents),
                quantity: u32::from(quantity),
            },
        )
    }

    #[test]
    fn greeting_builds_message() {
        assert_eq!(greeting("Rust"), "Hello, Rust!");
    }

    #[test]
    fn checkout_cart_creates_an_order_and_clears_the_cart() {
        let result = checkout_cart(&[
            CartItem { name: "Tea".to_string(), price_cents: 450, quantity: 2 },
            CartItem { name: "Cake".to_string(), price_cents: 350, quantity: 1 },
        ]);

        assert_eq!(result.order.total_cents, 1250);
        assert!(result.cart.items.is_empty());
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
        assert_eq!(captured_requests[0].path, "https://api.ferriskey.test/realms/test",);
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
            let expected_total: u32 = items.iter().map(|item| item.price_cents * item.quantity).sum();
            let result = checkout_cart(&items);

            prop_assert_eq!(result.order.total_cents, expected_total);
        }
    }
}
