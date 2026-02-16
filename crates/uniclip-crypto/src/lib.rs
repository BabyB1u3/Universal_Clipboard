use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, SigningKey, VerifyingKey, Signer};
use rand::rngs::OsRng;
use std::fs;
use std::path::Path;
use zeroize::Zeroize;

/// 本机长期身份：Ed25519
pub struct Identity {
    signing: SigningKey,
}

impl Identity {
    /// 生成新的身份（第一次启动用）
    pub fn generate() -> Self {
        let mut csprng = OsRng;
        let signing = SigningKey::generate(&mut csprng);
        Self { signing }
    }

    /// 从磁盘加载（明文存储 MVP）
    /// 文件内容：base64(32-byte seed)
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let s = fs::read_to_string(path)?;
        let seed_b64 = s.trim();

        let seed = STANDARD
            .decode(seed_b64)
            .map_err(|e| anyhow!("invalid base64 seed: {}", e))?;

        if seed.len() != 32 {
            return Err(anyhow!("seed length must be 32, got {}", seed.len()));
        }

        let mut seed32 = [0u8; 32];
        seed32.copy_from_slice(&seed);

        // 从 seed 恢复 SigningKey
        let signing = SigningKey::from_bytes(&seed32);

        // 尽量清理临时敏感数据
        let mut seed_vec = seed;
        seed_vec.zeroize();
        seed32.zeroize();

        Ok(Self { signing })
    }

    /// 保存到磁盘（明文存储 MVP）
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();

        // SigningKey::to_bytes() 返回 32-byte seed
        let mut seed = self.signing.to_bytes();
        let b64 = STANDARD.encode(seed);

        // 清理 seed
        seed.zeroize();

        fs::write(path, b64)?;
        Ok(())
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }

    /// 以 base64 导出公钥（给 UI/配对二维码用）
    pub fn public_key_b64(&self) -> String {
        let pk = self.verifying_key().to_bytes(); // 32 bytes
        STANDARD.encode(pk)
    }

    pub fn sign(&self, msg: &[u8]) -> Signature {
        self.signing.sign(msg)
    }
}
