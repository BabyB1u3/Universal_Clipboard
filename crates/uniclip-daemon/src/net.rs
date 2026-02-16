use anyhow::{anyhow, Result};
use std::io::{Read, Write};
use std::net::TcpStream;

pub fn send_frame(stream: &mut TcpStream, payload: &[u8]) -> Result<()> {
    let len: u32 = payload
        .len()
        .try_into()
        .map_err(|_| anyhow!("payload too large"))?;

    // 4 bytes big-endian length prefix
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

pub fn recv_frame(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;

    // 简单防御：避免对端发一个超大长度
    const MAX_FRAME: usize = 2 * 1024 * 1024; // 2MB
    if len > MAX_FRAME {
        return Err(anyhow!("frame too large: {}", len));
    }

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf)?;
    Ok(buf)
}
