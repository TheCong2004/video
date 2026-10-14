// SPDX-License-Identifier: AGPL-3.0-or-later
//! Account-scoped LIVE product scripts through the existing Promax gateway.

use crate::workflows::{WorkflowClient, WorkflowError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;

const ROUTE: &str = "/api/v1/artcraft/live-studio/playlist";

/// A product script. Selecting it prepares speech, without pinning a Shop item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct LiveProduct {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub title: String,
    pub price: String,
    pub script: String,
    pub duration: u32,
    pub enabled: bool,
}

impl LiveProduct {
    /// Normalize editor whitespace and reject invalid drafts before submission.
    pub fn validated(&self) -> Result<Self, ProductsError> { normalized(self) }
}

/// Server-owned catalogue and its currently selected script.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct LivePlaylist {
    pub products: Vec<LiveProduct>,
    pub active_product_id: Option<String>,
}

/// A comment match and the server's resulting playlist selection.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct ProductMatch {
    pub product: Option<LiveProduct>,
    pub playlist: LivePlaylist,
}

/// No server bodies, credentials or customer scripts are included in errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductsError {
    /// Invalid local draft, with no request sent.
    Input,
    /// A confirmed transport, account or permission failure.
    Remote(WorkflowError),
    /// A write may have succeeded; refresh before changing the draft again.
    UnknownOutcome,
}

fn valid_id(id: &str) -> bool {
    id.strip_prefix("prod_").is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix.len() <= 90
            && suffix.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    })
}

fn normalized(product: &LiveProduct) -> Result<LiveProduct, ProductsError> {
    let mut product = product.clone();
    product.title = product.title.trim().to_owned();
    product.price = product.price.trim().to_owned();
    product.script = product.script.trim().to_owned();
    if product.id.as_deref().is_some_and(|id| !valid_id(id))
        || product.title.is_empty()
        || product.title.chars().count() > 300
        || product.price.chars().count() > 100
        || product.script.chars().count() > 10000
        || !(10..=3600).contains(&product.duration)
        || product.title.chars().any(char::is_control)
        || product.price.chars().any(char::is_control)
        || product.script.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(ProductsError::Input);
    }
    Ok(product)
}

fn parse_product(value: &Value) -> Result<LiveProduct, ProductsError> {
    let product: LiveProduct = serde_json::from_value(value.clone())
        .map_err(|_| ProductsError::Remote(WorkflowError::InvalidResponse))?;
    if product.id.as_deref().is_none_or(|id| !valid_id(id)) {
        return Err(ProductsError::Remote(WorkflowError::InvalidResponse));
    }
    normalized(&product).map_err(|_| ProductsError::Remote(WorkflowError::InvalidResponse))
}

fn parse_playlist(value: Value) -> Result<LivePlaylist, ProductsError> {
    let list = value.get("products").and_then(Value::as_array)
        .filter(|rows| rows.len() <= 200)
        .ok_or(ProductsError::Remote(WorkflowError::InvalidResponse))?;
    let products = list.iter().map(parse_product).collect::<Result<Vec<_>, _>>()?;
    let mut ids = HashSet::new();
    if products.iter().any(|p| !ids.insert(p.id.as_deref())) {
        return Err(ProductsError::Remote(WorkflowError::InvalidResponse));
    }
    let runtime = value.get("runtime").and_then(Value::as_object)
        .ok_or(ProductsError::Remote(WorkflowError::InvalidResponse))?;
    let active_product_id = match runtime.get("active_product_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) if valid_id(id) => {
            // Legacy save can leave a stale runtime selection after deletion.
            products.iter().any(|p| p.id.as_ref() == Some(id)).then(|| id.clone())
        }
        _ => return Err(ProductsError::Remote(WorkflowError::InvalidResponse)),
    };
    Ok(LivePlaylist { products, active_product_id })
}

fn submitted(error: WorkflowError) -> ProductsError {
    match error {
        WorkflowError::Authentication | WorkflowError::Forbidden
        | WorkflowError::RateLimited | WorkflowError::Configuration => ProductsError::Remote(error),
        _ => ProductsError::UnknownOutcome,
    }
}

impl WorkflowClient {
    /// Fetch the current account's saved LIVE product scripts.
    pub fn live_products(&self) -> Result<LivePlaylist, ProductsError> {
        parse_playlist(self.get(ROUTE).map_err(ProductsError::Remote)?)
    }

    /// Save one complete edited catalogue, once, after an explicit user action.
    pub fn live_products_save(&self, products: &[LiveProduct]) -> Result<LivePlaylist, ProductsError> {
        if products.len() > 200 { return Err(ProductsError::Input); }
        let products = products.iter().map(normalized).collect::<Result<Vec<_>, _>>()?;
        let mut ids = HashSet::new();
        if products.iter().filter_map(|p| p.id.as_ref()).any(|id| !ids.insert(id)) {
            return Err(ProductsError::Input);
        }
        let mut payload = Vec::with_capacity(products.len());
        for product in &products {
            let mut value = serde_json::to_value(product).map_err(|_| ProductsError::Input)?;
            value["auto_pin"] = Value::Bool(false);
            payload.push(value);
        }
        let result = self.post(ROUTE, &json!({"operation":"save","payload":{"products":payload}}))
            .map_err(submitted)?;
        let playlist = parse_playlist(result).map_err(|_| ProductsError::UnknownOutcome)?;
        if playlist.products.len() != products.len()
            || products.iter().zip(&playlist.products).any(|(sent, stored)| {
                let mut expected = sent.clone();
                if expected.id.is_none() { expected.id = stored.id.clone(); }
                expected != *stored
            })
        { return Err(ProductsError::UnknownOutcome); }
        Ok(playlist)
    }

    /// Prepare one saved product; does not play audio or change TikTok Shop.
    pub fn live_product_select(&self, id: &str) -> Result<LivePlaylist, ProductsError> {
        if !valid_id(id) { return Err(ProductsError::Input); }
        let result = self.post(ROUTE, &json!({"operation":"select","payload":{"id":id}}))
            .map_err(submitted)?;
        let playlist = parse_playlist(result).map_err(|_| ProductsError::UnknownOutcome)?;
        if playlist.active_product_id.as_deref() != Some(id) {
            return Err(ProductsError::UnknownOutcome);
        }
        Ok(playlist)
    }

    /// Prepare the next enabled script using the existing server rotation order.
    pub fn live_product_next(&self) -> Result<LivePlaylist, ProductsError> {
        let result = self.post(ROUTE, &json!({"operation":"next","payload":{}})).map_err(submitted)?;
        parse_playlist(result).map_err(|_| ProductsError::UnknownOutcome)
    }

    /// Match a viewer comment and select a product without generating AI text.
    pub fn live_product_respond(&self, comment: &str) -> Result<ProductMatch, ProductsError> {
        if comment.trim().is_empty() || comment.chars().count() > 2000 {
            return Err(ProductsError::Input);
        }
        let value = self.post(ROUTE, &json!({"operation":"respond","payload":{"comment":comment}}))
            .map_err(submitted)?;
        let playlist = parse_playlist(value.get("playlist").cloned().unwrap_or(Value::Null))
            .map_err(|_| ProductsError::UnknownOutcome)?;
        let product = match value.get("product") {
            Some(Value::Null) => None,
            Some(product) => Some(parse_product(product).map_err(|_| ProductsError::UnknownOutcome)?),
            None => return Err(ProductsError::UnknownOutcome),
        };
        if product.as_ref().is_some_and(|p| !p.enabled || !playlist.products.contains(p)
            || p.id != playlist.active_product_id) {
            return Err(ProductsError::UnknownOutcome);
        }
        Ok(ProductMatch { product, playlist })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::{Read, Write}, net::TcpListener, thread};

    fn product() -> LiveProduct {
        LiveProduct { id: Some("prod_a".into()), title: "Coffee".into(), price: "50".into(),
            script: "Coffee script\nLine two".into(), duration: 180, enabled: true }
    }
    fn state() -> Value { json!({"products":[product()],"runtime":{"active_product_id":"prod_a"}}) }
    fn fixture(data: Value, status: u16) -> (WorkflowClient, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let worker = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket.set_read_timeout(Some(std::time::Duration::from_secs(3))).unwrap();
            let mut request = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let n = socket.read(&mut buffer).unwrap();
                request.extend_from_slice(&buffer[..n]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end].lines().find_map(|line| line.to_ascii_lowercase()
                        .strip_prefix("content-length:").and_then(|n| n.trim().parse::<usize>().ok())).unwrap_or(0);
                    if request.len() >= end + 4 + length { break; }
                }
                if n == 0 { break; }
            }
            let body = json!({"ok":true,"data":data}).to_string();
            write!(socket,"HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (WorkflowClient::new(&origin, Some("fixture-token".into())).unwrap(), worker)
    }

    #[test]
    fn catalogue_rejects_duplicate_ids_and_malformed_runtime() {
        assert_eq!(parse_playlist(state()).unwrap().products, vec![product()]);
        assert!(parse_playlist(json!({"products":[product(),product()],"runtime":{}})).is_err());
        assert!(parse_playlist(json!({"products":[],"runtime":{"active_product_id":7}})).is_err());
        let mut value = state(); value["products"] = json!([]);
        assert_eq!(parse_playlist(value).unwrap().active_product_id, None);
    }
    #[test]
    fn read_uses_authenticated_existing_route() {
        let (client, worker) = fixture(state(), 200);
        assert_eq!(client.live_products().unwrap().products.len(), 1);
        let request = worker.join().unwrap();
        assert!(request.starts_with("GET /api/v1/artcraft/live-studio/playlist "));
        assert!(request.to_lowercase().contains("authorization: bearer fixture-token"));
    }
    #[test]
    fn save_receipt_preserves_fields_and_never_enables_shop_pinning() {
        let (client, worker) = fixture(state(), 200);
        assert!(client.live_products_save(&[product()]).is_ok());
        let request = worker.join().unwrap();
        let body: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["operation"], "save");
        assert_eq!(body["payload"]["products"][0]["auto_pin"], false);
        assert_eq!(body["payload"]["products"][0]["script"], product().script);
    }
    #[test]
    fn invalid_draft_is_rejected_before_network_and_unknown_write_is_not_retried() {
        let client = WorkflowClient::new("http://127.0.0.1:1", Some("fixture".into())).unwrap();
        let mut invalid = product(); invalid.duration = 9;
        assert_eq!(client.live_products_save(&[invalid]), Err(ProductsError::Input));
        assert_eq!(client.live_product_select("other/route"), Err(ProductsError::Input));
        let (client, worker) = fixture(json!({}), 200);
        assert_eq!(client.live_products_save(&[product()]), Err(ProductsError::UnknownOutcome));
        worker.join().unwrap();
    }
    #[test]
    fn select_and_respond_validate_server_selection() {
        let (client, worker) = fixture(state(), 200);
        assert_eq!(client.live_product_select("prod_a").unwrap().active_product_id.as_deref(), Some("prod_a"));
        worker.join().unwrap();
        let (client, worker) = fixture(json!({"product":product(),"playlist":state()}), 200);
        assert_eq!(client.live_product_respond("coffee").unwrap().product, Some(product()));
        worker.join().unwrap();
    }
}
