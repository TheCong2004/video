//! Import only facts present in a supplied product link and its public image IDs.
//! Shopee pages are client-rendered; prices/reviews are deliberately never guessed.
use serde::{Deserialize, Serialize};
use std::{io::Read, time::Duration};

/// Provenance and unverified display-name hint supplied by the URL itself.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(missing_docs)]
pub struct ProductLink {
    pub source_url: String,
    pub shop_id: String,
    pub item_id: String,
    pub title_hint: String,
    pub image_ids: Vec<String>,
}
fn image_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
/// Parse a Shopee VN product URL without following redirects or calling private APIs.
pub fn parse_shopee_link(input: &str) -> Result<ProductLink, String> {
    if input.len() > 8192 {
        return Err("Link sản phẩm quá dài.".into());
    }
    let url = url::Url::parse(input.trim()).map_err(|_| "Link sản phẩm không hợp lệ.")?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("shopee.vn" | "www.shopee.vn"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(
            "Dùng link sản phẩm HTTPS đầy đủ từ shopee.vn; link rút gọn chưa được hỗ trợ.".into(),
        );
    }
    let (title, shop, item) = if let Some((title, ids)) =
        url.path().trim_start_matches('/').rsplit_once("-i.")
    {
        let (shop, item) = ids.split_once('.').ok_or("Link thiếu mã sản phẩm.")?;
        (title.to_owned(), shop.to_owned(), item.to_owned())
    } else {
        let parts: Vec<_> = url.path().trim_matches('/').split('/').collect();
        if parts.len() != 3 || parts[0] != "product" {
            return Err("Link chưa có mã shop và sản phẩm. Hãy dùng link sản phẩm đầy đủ.".into());
        }
        (String::new(), parts[1].into(), parts[2].into())
    };
    if [&shop, &item]
        .iter()
        .any(|id| id.is_empty() || id.len() > 20 || !id.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Mã shop hoặc sản phẩm không hợp lệ.".into());
    }
    let encoded = format!("title={title}");
    let title_hint = url::form_urlencoded::parse(encoded.as_bytes())
        .next()
        .map(|(_, v)| v.replace('-', " "))
        .unwrap_or_default();
    if title_hint.len() > 1024 {
        return Err("Tên trong link sản phẩm quá dài.".into());
    }
    let mut image_ids = vec![];
    for (_, ids) in url.query_pairs().filter(|(key, _)| key == "image_ids") {
        for id in ids.split(',') {
            if !image_id(id) {
                return Err("Mã ảnh trong link không hợp lệ.".into());
            }
            if !image_ids.iter().any(|s| s == id) {
                image_ids.push(id.to_owned());
            }
            if image_ids.len() > 10 {
                return Err("Chỉ nhập tối đa 10 ảnh từ một link.".into());
            }
        }
    }
    Ok(ProductLink {
        source_url: url.to_string(),
        shop_id: shop,
        item_id: item,
        title_hint,
        image_ids,
    })
}
/// Fetch an image only from the fixed public Shopee CDN, with no cookies or redirects.
pub fn fetch_shopee_image(id: &str) -> Result<Vec<u8>, String> {
    if !image_id(id) {
        return Err("Mã ảnh không hợp lệ.".into());
    }
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .timeout(Duration::from_secs(20))
        .build();
    let response = agent
        .get(&format!("https://down-vn.img.susercontent.com/file/{id}"))
        .call()
        .map_err(
            |_| "Không tải được ảnh công khai của sản phẩm. Chọn ảnh/video có sẵn trên máy.",
        )?;
    if !response
        .header("Content-Type")
        .unwrap_or("")
        .starts_with("image/")
    {
        return Err("Máy chủ không trả về ảnh sản phẩm.".into());
    }
    let mut bytes = vec![];
    response
        .into_reader()
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Không đọc được ảnh sản phẩm.")?;
    if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return Err("Ảnh rỗng hoặc vượt giới hạn 8 MB.".into());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn product_ids_are_taken_from_product_path_not_referral_query() {
        let link = parse_shopee_link("https://shopee.vn/Khoai-s%C3%A2m-%C4%91%E1%BA%A5t-1kg-i.1185752780.54703416741?image_ids=vn-11134207-820l4-meyda3stmo0404&vShopId=1506174776&vItemId=46753575976").unwrap();
        assert_eq!(link.shop_id, "1185752780");
        assert_eq!(link.item_id, "54703416741");
        assert_eq!(link.title_hint, "Khoai sâm đất 1kg");
        assert_eq!(link.image_ids.len(), 1);
    }
    #[test]
    fn arbitrary_hosts_paths_and_image_traversal_are_rejected() {
        for url in [
            "https://shopee.vn.evil.test/product/1/2",
            "https://user:pass@shopee.vn/product/1/2",
            "http://shopee.vn/product/1/2",
            "https://shopee.vn/product/1/2?image_ids=../secret",
            "https://shopee.vn/product/a/2",
            "https://shopee.vn/product/1/2?image_ids=https://localhost",
        ] {
            assert!(parse_shopee_link(url).is_err(), "{url}");
        }
    }
    #[test]
    fn no_image_or_title_is_invented_for_canonical_links() {
        let link = parse_shopee_link("https://shopee.vn/product/1/2").unwrap();
        assert!(link.image_ids.is_empty());
        assert!(link.title_hint.is_empty());
    }
}
