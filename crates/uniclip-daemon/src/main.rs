use std::time::Duration;

fn now_ms() -> u64 {
    // 获取系统时间
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    t.as_millis() as u64
}

fn main() {
    let device_id = "dev-local-1";

    let mut recent = uniclip_core::RecentSet::new(2048, Duration::from_secs(60));

    let item1 = uniclip_core::make_text_item(device_id, now_ms(), "hello".to_string());
    let item2 = uniclip_core::make_text_item(device_id, now_ms(), "hello".to_string());
    let item3 = uniclip_core::make_text_item(device_id, now_ms(), "hello2".to_string());

    println!("item1 id={}", item1.id);
    println!("item2 id={}", item2.id);
    println!("item3 id={}", item3.id);

    println!("new item1? {}", recent.check_and_remember(&item1.id)); // true
    println!("new item2? {}", recent.check_and_remember(&item2.id)); // false
    println!("new item3? {}", recent.check_and_remember(&item3.id)); // true

    let msg = uniclip_proto::WireMessage::ClipboardPush { item: item1 };
    println!("wire msg = {:?}", msg);
}
