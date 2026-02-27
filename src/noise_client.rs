use std::error::Error;
use std::io::{Read, Write};

pub struct NoiseClient<T> {
    stream: T,
    transport: snow::TransportState,
}

impl<T: Read + Write> NoiseClient<T> {
    pub fn connect(mut stream: T, pattern: &str) -> Result<Self, Box<dyn Error>> {
        let params: snow::params::NoiseParams = pattern.parse()?;
        let mut initiator = snow::Builder::new(params).build_initiator()?;

        let mut out_msg = vec![0u8; 65535];
        let len = initiator.write_message(&[], &mut out_msg)?;
        send_frame(&mut stream, &out_msg[..len])?;

        let in_msg = recv_frame(&mut stream)?;
        let mut tmp = vec![0u8; 65535];
        initiator.read_message(&in_msg, &mut tmp)?;

        let transport = initiator.into_transport_mode()?;

        Ok(NoiseClient { stream, transport })
    }

    pub fn accept(mut stream: T, pattern: &str) -> Result<Self, Box<dyn Error>> {
        let params: snow::params::NoiseParams = pattern.parse()?;
        let mut responder = snow::Builder::new(params).build_responder()?;

        let in_msg = recv_frame(&mut stream)?;
        let mut tmp = vec![0u8; 65535];
        responder.read_message(&in_msg, &mut tmp)?;

        let mut out_msg = vec![0u8; 65535];
        let len = responder.write_message(&[], &mut out_msg)?;
        send_frame(&mut stream, &out_msg[..len])?;

        let transport = responder.into_transport_mode()?;
        Ok(NoiseClient { stream, transport })
    }

    pub fn send(&mut self, plaintext: &[u8]) -> Result<(), Box<dyn Error>> {
        let mut out = vec![0u8; plaintext.len() + 1024];
        let len = self.transport.write_message(plaintext, &mut out)?;
        send_frame(&mut self.stream, &out[..len])?;
        Ok(())
    }

    pub fn recv(&mut self) -> Result<Vec<u8>, Box<dyn Error>> {
        let ct = recv_frame(&mut self.stream)?;
        let mut pt = vec![0u8; ct.len()];
        let len = self.transport.read_message(&ct, &mut pt)?;
        pt.truncate(len);
        Ok(pt)
    }
}

fn send_frame<W: Write>(stream: &mut W, data: &[u8]) -> Result<(), Box<dyn Error>> {
    let len = (data.len() as u32).to_be_bytes();
    stream.write_all(&len)?;
    stream.write_all(data)?;
    stream.flush()?;
    Ok(())
}

fn recv_frame<R: Read>(stream: &mut R) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut lenb = [0u8; 4];
    stream.read_exact(&mut lenb)?;
    let len = u32::from_be_bytes(lenb) as usize;
    const MAX_FRAME: usize = 65535;
    if len > MAX_FRAME {
        return Err(Box::<dyn Error>::from("frame too large"));
    }
    let mut buf = vec![0u8; len];
    let mut read = 0usize;
    while read < len {
        match stream.read(&mut buf[read..]) {
            Ok(0) => {
                return Err(Box::<dyn Error>::from(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "failed to fill whole buffer",
                )));
            }
            Ok(n) => read += n,
            Err(e) => return Err(Box::<dyn Error>::from(e)),
        }
    }
    Ok(buf)
}
