// SPDX-License-Identifier: AGPL-3.0-or-later
//! Official Meta Graph/Reels API only. Tokens are DPAPI sealed, never part of a
//! campaign plan, process arguments, model context, Debug, or log message.
use super::{
    decimal_id,
    store::{valid_post_url, TaskFailure},
    Asset,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};
const ENTROPY: &str = "https://concat-facebook-page-grants.invalid/v1";
#[derive(Clone, Serialize, Deserialize)]
struct PageSecret {
    page_id: String,
    name: String,
    token: String,
    tasks: Vec<String>,
    verified_at: i64,
}
#[derive(Clone, Serialize, Deserialize, Default)]
struct Credentials {
    version: String,
    pages: Vec<PageSecret>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectedPage {
    pub page_id: String,
    pub name: String,
    pub verified_at: i64,
}
pub struct Facebook {
    path: PathBuf,
    origin: String,
    upload_origin: String,
}
impl Facebook {
    pub fn new(config: &Path) -> Self {
        Self {
            path: config.join("studio/facebook-pages.dpapi"),
            origin: "https://graph.facebook.com".into(),
            upload_origin: "https://rupload.facebook.com".into(),
        }
    }
    fn load(&self) -> Result<Credentials, String> {
        if !self.path.exists() {
            return Ok(Credentials::default());
        }
        let data = std::fs::read(&self.path).map_err(|_| "Không đọc được quyền Page")?;
        if data.len() > 1024 * 1024 {
            return Err("Kho quyền Page quá lớn".into());
        }
        let clear = crate::auth::unprotect(&data, ENTROPY)
            .map_err(|_| "Không giải mã được quyền Page của người dùng Windows hiện tại")?;
        serde_json::from_slice(&clear).map_err(|_| "Kho quyền Page không hợp lệ".into())
    }
    pub fn connected(&self) -> Result<Vec<ConnectedPage>, String> {
        Ok(self
            .load()?
            .pages
            .into_iter()
            .map(|p| ConnectedPage {
                page_id: p.page_id,
                name: p.name,
                verified_at: p.verified_at,
            })
            .collect())
    }
    fn version(version: &str) -> Result<(), String> {
        if version.len() < 4
            || version.len() > 8
            || !version.starts_with('v')
            || !version.ends_with(".0")
            || !version[1..version.len() - 2]
                .bytes()
                .all(|b| b.is_ascii_digit())
        {
            return Err("Nhập phiên bản Graph API hợp lệ từ cấu hình Meta App, ví dụ vNN.0".into());
        }
        Ok(())
    }
    fn request(
        &self,
        version: &str,
        method: &str,
        path: &str,
        token: &str,
        body: Option<&Value>,
    ) -> Result<Value, TaskFailure> {
        Self::version(version).map_err(TaskFailure::input)?;
        if path.contains("..") || path.contains('#') || path.len() > 2048 {
            return Err(TaskFailure::input("Graph path không hợp lệ"));
        }
        let request = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(Duration::from_secs(60))
            .build()
            .request(method, &format!("{}/{version}/{path}", self.origin))
            .set("Authorization", &format!("Bearer {token}"));
        let response = if let Some(body) = body {
            request
                .set("Content-Type", "application/json")
                .send_string(&body.to_string())
        } else {
            request.call()
        };
        let response=match response{Ok(response)=>response,Err(ureq::Error::Status(401|403,_))=>return Err(TaskFailure::input("Facebook từ chối quyền Page hoặc token hết hạn. Kết nối lại bằng quyền chính thức.")),Err(ureq::Error::Status(429,_))=>return Err(TaskFailure::known("Facebook rate limit; chờ theo chính sách retry")),Err(ureq::Error::Status(code,_)) if (400..500).contains(&code)=>return Err(TaskFailure::input("Facebook từ chối yêu cầu; kiểm tra định dạng, quyền và cấu hình Meta App")),Err(_)=>return Err(if method=="GET"{TaskFailure::known("Không đọc được trạng thái Facebook")}else{TaskFailure::unknown("Kết quả gửi Facebook chưa rõ; chỉ được đối chiếu, không tự gửi lại")})};
        let mut bytes = vec![];
        response
            .into_reader()
            .take(2 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| TaskFailure::unknown("Phản hồi Facebook bị ngắt"))?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(TaskFailure::unknown("Phản hồi Facebook quá lớn"));
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| TaskFailure::unknown("Phản hồi Facebook không hợp lệ"))?;
        if value.get("error").is_some() {
            return Err(TaskFailure::input(
                "Facebook trả lỗi quyền hoặc tham số; không báo thành công",
            ));
        }
        Ok(value)
    }
    /// Read official User-token permissions and Pages; persist ONLY the selected
    /// Page tokens returned by /me/accounts, encrypted for this Windows user.
    pub fn connect(
        &self,
        version: &str,
        user_token: &str,
        allowed: &[String],
    ) -> Result<Vec<ConnectedPage>, String> {
        Self::version(version)?;
        if user_token.len() < 20
            || user_token.len() > 8192
            || user_token.chars().any(char::is_control)
            || allowed.is_empty()
            || allowed.iter().any(|id| !decimal_id(id))
        {
            return Err("Nhập User access token hợp lệ và các Page ID được phép dùng".into());
        }
        let permissions = self
            .request(version, "GET", "me/permissions", user_token, None)
            .map_err(|e| e.message)?;
        for permission in [
            "pages_manage_posts",
            "pages_read_engagement",
            "pages_show_list",
        ] {
            if !permissions["data"].as_array().is_some_and(|a| {
                a.iter()
                    .any(|p| p["permission"] == permission && p["status"] == "granted")
            }) {
                return Err(format!("Meta App chưa được cấp {permission}"));
            }
        }
        // Explicitly bounded discovery. Do not follow arbitrary paging URLs.
        let list = self
            .request(
                version,
                "GET",
                "me/accounts?fields=id,name,access_token,tasks&limit=100",
                user_token,
                None,
            )
            .map_err(|e| e.message)?;
        let mut pages = vec![];
        for id in allowed {
            let page=list["data"].as_array().and_then(|a|a.iter().find(|p|p["id"]==id.as_str())).ok_or("Một Page được chọn không thuộc danh sách quyền hiện tại; tối đa 100 Page mỗi lần kết nối")?;
            let tasks = page["tasks"]
                .as_array()
                .ok_or("Facebook chưa trả nhiệm vụ quản lý Page")?;
            if !tasks.iter().any(|t| {
                matches!(
                    t.as_str(),
                    Some(
                        "CREATE_CONTENT"
                            | "PROFILE_PLUS_CREATE_CONTENT"
                            | "PROFILE_PLUS_FULL_CONTROL"
                    )
                )
            }) {
                return Err("Tài khoản không có quyền tạo nội dung trên Page đã chọn".into());
            }
            let token = page["access_token"]
                .as_str()
                .filter(|t| !t.is_empty())
                .ok_or("Facebook chưa trả Page token")?;
            let verified = self
                .request(version, "GET", "me?fields=id,name", token, None)
                .map_err(|e| e.message)?;
            if verified["id"] != id.as_str() {
                return Err("Page token không khớp Page ID".into());
            }
            pages.push(PageSecret {
                page_id: id.clone(),
                name: page["name"].as_str().unwrap_or(id).into(),
                token: token.into(),
                tasks: tasks
                    .iter()
                    .filter_map(|v| v.as_str().map(Into::into))
                    .collect(),
                verified_at: super::store::now(),
            });
        }
        let bytes = serde_json::to_vec(&Credentials {
            version: version.into(),
            pages,
        })
        .map_err(|_| "Không mã hóa được cấu hình")?;
        let bytes = crate::auth::protect(&bytes, ENTROPY)
            .map_err(|_| "Không lưu được token mã hóa; không có chế độ lưu plaintext")?;
        std::fs::create_dir_all(self.path.parent().ok_or("Không có thư mục cấu hình")?)
            .map_err(|_| "Không mở được cấu hình")?;
        let temp = self
            .path
            .with_extension(format!("{:016x}.tmp", rand::random::<u64>()));
        std::fs::write(&temp, bytes).map_err(|_| "Không ghi được cấu hình")?;
        std::fs::rename(temp, &self.path).map_err(|_| "Không cập nhật được cấu hình")?;
        self.connected()
    }
    fn page(&self, page_id: &str) -> Result<(String, PageSecret), TaskFailure> {
        let store = self.load().map_err(TaskFailure::input)?;
        let page = store
            .pages
            .into_iter()
            .find(|p| p.page_id == page_id)
            .ok_or_else(|| {
                TaskFailure::input("Page này chưa được cấp quyền chính thức cho Concat")
            })?;
        Self::version(&store.version).map_err(TaskFailure::input)?;
        Ok((store.version, page))
    }
    pub fn verify_page(&self, id: &str) -> Result<(), TaskFailure> {
        let (version, page) = self.page(id)?;
        let result = self.request(&version, "GET", "me?fields=id", &page.token, None)?;
        if result["id"] != id {
            return Err(TaskFailure::input("Quyền Page không còn khớp"));
        }
        Ok(())
    }
    pub fn start(&self, page_id: &str) -> Result<(String, String), TaskFailure> {
        let (version, page) = self.page(page_id)?;
        let result = self.request(
            &version,
            "POST",
            &format!("{page_id}/video_reels"),
            &page.token,
            Some(&json!({"upload_phase":"start"})),
        )?;
        let id = result["video_id"]
            .as_str()
            .filter(|id| decimal_id(id))
            .ok_or_else(|| TaskFailure::unknown("Không nhận được video_id; không tạo phiên mới"))?;
        let upload = result["upload_url"]
            .as_str()
            .ok_or_else(|| TaskFailure::unknown("Không nhận được upload URL"))?;
        self.upload_url(upload, &version, id)?;
        Ok((id.into(), upload.into()))
    }
    fn upload_url(&self, raw: &str, version: &str, id: &str) -> Result<(), TaskFailure> {
        let expected = format!("{}/video-upload/{version}/{id}", self.upload_origin);
        if raw != expected {
            return Err(TaskFailure::unknown(
                "Upload URL không khớp miền, phiên bản và video ID chính thức",
            ));
        }
        Ok(())
    }
    pub fn upload(
        &self,
        page_id: &str,
        video_id: &str,
        upload_url: &str,
        asset: &Asset,
    ) -> Result<(), TaskFailure> {
        let (version, page) = self.page(page_id)?;
        self.upload_url(upload_url, &version, video_id)?;
        if super::quality::hash(&asset.path).map_err(TaskFailure::input)? != asset.sha256 {
            return Err(TaskFailure::input("File đã bị sửa trước upload"));
        }
        let file = std::fs::File::open(&asset.path)
            .map_err(|_| TaskFailure::input("Không mở được video"))?;
        let size = file
            .metadata()
            .map_err(|_| TaskFailure::input("Không đọc được file size"))?
            .len();
        let response = ureq::AgentBuilder::new()
            .redirects(0)
            .timeout(Duration::from_secs(600))
            .build()
            .post(upload_url)
            .set("Authorization", &format!("OAuth {}", page.token))
            .set("offset", "0")
            .set("file_size", &size.to_string())
            .set("Content-Type", "application/octet-stream")
            .set("Content-Length", &size.to_string())
            .send(file)
            .map_err(|_| {
                TaskFailure::unknown("Upload chưa xác định; đối chiếu video_id trước khi gửi thêm")
            })?;
        let mut bytes = vec![];
        response
            .into_reader()
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| TaskFailure::unknown("Không nhận được biên nhận upload"))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| TaskFailure::unknown("Biên nhận upload không hợp lệ"))?;
        if value["success"] != true {
            return Err(TaskFailure::unknown("Upload chưa được Facebook xác nhận"));
        }
        Ok(())
    }
    pub fn finish(
        &self,
        page_id: &str,
        video_id: &str,
        title: &str,
        caption: &str,
    ) -> Result<(), TaskFailure> {
        if !decimal_id(video_id) || title.len() > 1000 || caption.len() > 16000 {
            return Err(TaskFailure::input("Metadata Facebook không hợp lệ"));
        }
        let (version, page) = self.page(page_id)?;
        let result=self.request(&version,"POST",&format!("{page_id}/video_reels"),&page.token,Some(&json!({"video_id":video_id,"upload_phase":"finish","video_state":"PUBLISHED","title":title,"description":caption})))?;
        if result["success"] != true {
            return Err(TaskFailure::unknown("Facebook chưa xác nhận gửi xuất bản"));
        }
        Ok(())
    }
    pub fn status(&self, page_id: &str, video_id: &str) -> Result<Value, TaskFailure> {
        if !decimal_id(video_id) {
            return Err(TaskFailure::input("Video ID không hợp lệ"));
        }
        let (version, page) = self.page(page_id)?;
        self.request(
            &version,
            "GET",
            &format!("{video_id}?fields=id,status,permalink_url"),
            &page.token,
            None,
        )
    }
    pub fn published(value: &Value, video_id: &str) -> Option<String> {
        let url = value["permalink_url"].as_str()?;
        if value["id"] == video_id
            && value["status"]["publishing_phase"]["status"] == "complete"
            && valid_post_url(url)
        {
            Some(url.into())
        } else {
            None
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    };
    fn server(
        replies: Vec<Option<Value>>,
    ) -> (
        String,
        Arc<Mutex<Vec<(String, String)>>>,
        std::thread::JoinHandle<()>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(vec![]));
        let saved = requests.clone();
        let thread = std::thread::spawn(move || {
            for reply in replies {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut headers = Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {
                    let mut c = [0];
                    socket.read_exact(&mut c).unwrap();
                    headers.push(c[0]);
                    assert!(headers.len() < 65536);
                }
                let headers = String::from_utf8(headers).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':')
                            .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                            .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; length];
                socket.read_exact(&mut body).unwrap();
                saved.lock().unwrap().push((
                    headers.lines().next().unwrap().into(),
                    String::from_utf8_lossy(&body).into_owned(),
                ));
                if let Some(reply) = reply {
                    let bytes = reply.to_string();
                    write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",bytes.len(),bytes).unwrap();
                }
            }
        });
        (origin, requests, thread)
    }
    fn fixture(origin: String) -> (Facebook, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("facebook-campaign-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(root.join("studio")).unwrap();
        let api = Facebook {
            path: root.join("studio/facebook-pages.dpapi"),
            origin: origin.clone(),
            upload_origin: origin,
        };
        let store = Credentials {
            version: "v99.0".into(),
            pages: vec![PageSecret {
                page_id: "123".into(),
                name: "Fixture only".into(),
                token: "fixture-page-secret-not-a-live-token".into(),
                tasks: vec!["CREATE_CONTENT".into()],
                verified_at: 0,
            }],
        };
        std::fs::write(
            &api.path,
            crate::auth::protect(&serde_json::to_vec(&store).unwrap(), ENTROPY).unwrap(),
        )
        .unwrap();
        (api, root)
    }
    #[test]
    fn official_page_identity_is_verified_before_encrypting_only_allowed_tokens() {
        let (origin, requests, thread) = server(vec![
            Some(
                json!({"data":[{"permission":"pages_manage_posts","status":"granted"},{"permission":"pages_read_engagement","status":"granted"},{"permission":"pages_show_list","status":"granted"}]}),
            ),
            Some(
                json!({"data":[{"id":"123","name":"Authorized fixture","access_token":"fixture-page-token","tasks":["PROFILE_PLUS_CREATE_CONTENT"]}]}),
            ),
            Some(json!({"id":"123","name":"Authorized fixture"})),
        ]);
        let (api, root) = fixture(origin);
        let connected = api
            .connect(
                "v99.0",
                "fixture-user-access-token-for-tests",
                &["123".into()],
            )
            .unwrap();
        assert_eq!(connected[0].page_id, "123");
        thread.join().unwrap();
        let ciphertext = std::fs::read(&api.path).unwrap();
        assert!(!String::from_utf8_lossy(&ciphertext).contains("fixture-page-token"));
        assert_eq!(requests.lock().unwrap().len(), 3);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn finish_transport_loss_is_unknown_and_never_automatically_replayed() {
        let (origin, requests, thread) = server(vec![None]);
        let (api, root) = fixture(origin);
        let result = api
            .finish("123", "456", "Fixture title", "Fixture caption")
            .unwrap_err();
        assert!(result.unknown);
        thread.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn wrong_page_wrong_upload_origin_or_incomplete_publish_never_claims_success() {
        let (api, root) = fixture("http://127.0.0.1:1".into());
        assert!(api.page("999").is_err());
        assert!(api
            .upload_url(
                "https://evil.invalid/video-upload/v99.0/456",
                "v99.0",
                "456"
            )
            .is_err());
        assert!(Facebook::published(&json!({"id":"456","status":{"publishing_phase":{"status":"not_started"}},"permalink_url":"https://www.facebook.com/reel/456"}),"456").is_none());
        assert!(Facebook::published(&json!({"id":"456","status":{"publishing_phase":{"status":"complete"}},"permalink_url":"https://evil.invalid/456"}),"456").is_none());
        assert_eq!(Facebook::published(&json!({"id":"456","status":{"publishing_phase":{"status":"complete"}},"permalink_url":"https://www.facebook.com/reel/456"}),"456").unwrap(),"https://www.facebook.com/reel/456");
        std::fs::remove_dir_all(root).unwrap();
    }
}
