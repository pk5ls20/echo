use std::io::Read;

pub fn stream_pipeline<S, F>(stream: &mut S, mut processor: F) -> std::io::Result<()>
where
    S: Read + Unpin,
    F: FnMut(&[u8]),
{
    let mut buf = [0_u8; 8192];
    loop {
        let n = stream.read(&mut buf)?;
        if n == 0 {
            break;
        }
        processor(&buf[..n]);
    }
    Ok(())
}
