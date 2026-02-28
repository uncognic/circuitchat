use std::error::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub struct NoisePeer<T> {
    stream: T,
    transport: snow::TransportState,
}

impl<T: AsyncRead + AsyncWrite + Unpin> NoisePeer<T> {
    pub async fn connect(mut stream: T, pattern: &str) -> Result<Self, Box<dyn Error>> {
        let params: snow::params::NoiseParams = pattern.parse()?;
        let mut initiator = snow::Builder::new(params).build_initiator()?;

        let mut out_msg = vec![0u8; 65535];
        let len = initiator.write_message(&[], &mut out_msg)?;
        send_frame(&mut stream, &out_msg[..len]).await?;

        let in_msg = recv_frame(&mut stream).await?;
        let mut tmp = vec![0u8; 65535];
        initiator.read_message(&in_msg, &mut tmp)?;

        let transport = initiator.into_transport_mode()?;
        Ok(NoisePeer { stream, transport })
    }

    pub async fn accept(mut stream: T, pattern: &str) -> Result<Self, Box<dyn Error>> {
        let params: snow::params::NoiseParams = pattern.parse()?;
        let mut responder = snow::Builder::new(params).build_responder()?;

        let in_msg = recv_frame(&mut stream).await?;
        let mut tmp = vec![0u8; 65535];
        responder.read_message(&in_msg, &mut tmp)?;

        let mut out_msg = vec![0u8; 65535];
        let len = responder.write_message(&[], &mut out_msg)?;
        send_frame(&mut stream, &out_msg[..len]).await?;

        let transport = responder.into_transport_mode()?;
        Ok(NoisePeer { stream, transport })
    }

    pub async fn send(&mut self, plaintext: &[u8]) -> Result<(), Box<dyn Error>> {
        let mut out = vec![0u8; plaintext.len() + 16];
        let len = self.transport.write_message(plaintext, &mut out)?;
        send_frame(&mut self.stream, &out[..len]).await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Vec<u8>, Box<dyn Error>> {
        let ct = recv_frame(&mut self.stream).await?;
        let mut pt = vec![0u8; ct.len()];
        let len = self.transport.read_message(&ct, &mut pt)?;
        pt.truncate(len);
        Ok(pt)
    }
}

async fn send_frame<W: AsyncWrite + Unpin>(
    stream: &mut W,
    data: &[u8],
) -> Result<(), Box<dyn Error>> {
    let len = (data.len() as u32).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(data).await?;
    stream.flush().await?;
    Ok(())
}

async fn recv_frame<R: AsyncRead + Unpin>(stream: &mut R) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut lenb = [0u8; 4];
    stream.read_exact(&mut lenb).await?;
    let len = u32::from_be_bytes(lenb) as usize;
    if len > 65535 {
        return Err("frame too large".into());
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}
