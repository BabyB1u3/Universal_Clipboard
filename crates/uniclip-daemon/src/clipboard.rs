use anyhow::Result;

pub trait ClipboardBackend {
    fn get_text(&mut self) -> Result<Option<String>>;
    fn set_text(&mut self, text: &str) -> Result<()>;
}

pub struct ArboardClipboard {
    cb: arboard::Clipboard,
}

impl ArboardClipboard {
    pub fn new() -> Result<Self> {
        Ok(Self { cb: arboard::Clipboard::new()? })
    }
}

impl ClipboardBackend for ArboardClipboard {
    fn get_text(&mut self) -> Result<Option<String>> {
        match self.cb.get_text() {
            Ok(s) => Ok(Some(s)),
            Err(_) => Ok(None), // 不是 text / 剪贴板不可读 / 权限问题等，忽略
            // TODO: 错误处理
        }
    }

    fn set_text(&mut self, text: &str) -> Result<()> {
        self.cb.set_text(text.to_string())?;
        Ok(())
    }
}
