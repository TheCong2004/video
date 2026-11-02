//! Actual native payloads used by the bundled runtime integration test.
fn main() {
    let commands = [
        ("chat", "QA_Temple", "Cầu an", 0),
        ("like", "QA_Temple", "", 0),
        ("gift", "QA_Temple", "Hoa hồng", 1),
        ("gift", "QA_Temple", "Đèn hoa sen", 30),
        ("gift", "QA_Temple", "Chuông vàng", 199),
        ("fill", "", "", 15),
        ("fill", "", "", 36),
        ("fill", "", "", 64),
        ("fill", "", "", 100),
        ("mo", "", "", 0),
        ("bell", "", "", 0),
        ("bots", "", "", 1),
        ("bots", "", "", 0),
        ("skin", "", "nun", 0),
        ("pin", "Sản phẩm kiểm thử", "199.000đ", 0),
        ("stop", "", "", 0),
        ("clear", "", "", 0),
    ]
    .into_iter()
    .map(|(a, n, t, c)| concat_studio::temple_live::command(a, n, t, c).unwrap())
    .collect::<Vec<_>>();
    println!("{}", serde_json::to_string(&commands).unwrap());
}
