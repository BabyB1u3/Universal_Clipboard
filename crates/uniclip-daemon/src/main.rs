mod clipboard;

use clipboard::{ArboardClipboard, ClipboardBackend};
use std::time::{Duration, SystemTime};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn main() -> anyhow::Result<()> {
    let device_id = "dev-local-1"; //TODO: Load from config
    let mut recent = uniclip_core::RecentSet::new(2048, Duration::from_secs(60));

    let mut cb = ArboardClipboard::new()?;

    println!("uniclip-daemon polling clipboard (text) ...");

    let mut last_seen_id: Option<String> = None;

    loop {
        if let Some(text) = cb.get_text()? {
            // 构造 item + 计算 id
            let item = uniclip_core::make_text_item(device_id, now_ms(), text);

            // 如果 id 没变，跳过
            if last_seen_id.as_deref() == Some(&item.id) {
                std::thread::sleep(Duration::from_millis(300));
                continue;
            }
            last_seen_id = Some(item.id.clone());

            // recent 去重
            //TODO: 收到网络的 item 也会塞进 recent，用来断环
            if !recent.check_and_remember(&item.id) {
                std::thread::sleep(Duration::from_millis(300));
                continue;
            }

            // MVP test
            println!("[LOCAL NEW] id={} from={} at={} payload={:?}",
                item.id, item.from_device_id, item.created_at_ms, item.payload
            );
        }

        std::thread::sleep(Duration::from_millis(300));
    }
}
