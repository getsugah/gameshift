//! Fixed read-only request alongside the existing exclusive GPU profile session.
use super::*;
use gameshift_core::rapl::Snapshot;
use std::{os::fd::AsRawFd, os::unix::net::UnixStream};
#[derive(Default)]
pub struct Cache {
    last: Option<Instant>,
    snapshot: Option<Snapshot>,
}
fn request(stream: &mut UnixStream) -> Result<bool> {
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| e.to_string())?;
    let mut first = [0u8; 1];
    // Peek one byte, so the original JSON protocol is handed off unchanged.
    let count = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            first.as_mut_ptr().cast(),
            1,
            libc::MSG_PEEK,
        )
    };
    if count != 1 {
        return Err("Connexion de contrôle vide ou expirée".into());
    }
    if first[0] != b'R' {
        return Ok(false);
    }
    let mut fixed = [0u8; 6];
    stream.read_exact(&mut fixed).map_err(|e| e.to_string())?;
    if &fixed != b"RAPL1\n" {
        return Err("Requête de lecture non autorisée".into());
    }
    Ok(true)
}
impl Cache {
    /// The caller must validate SO_PEERCRED against install::owner before invoking this.
    pub fn handle(&mut self, stream: &mut UnixStream) -> Result<bool> {
        if !request(stream)? {
            return Ok(false);
        }
        if self
            .last
            .is_none_or(|t| t.elapsed() >= Duration::from_millis(500))
        {
            self.snapshot = Some(gameshift_core::rapl::read(Path::new("/")));
            self.last = Some(Instant::now());
        }
        let bytes = serde_json::to_vec(self.snapshot.as_ref().ok_or("Relevé RAPL absent")?)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 65536 {
            return Err("Réponse RAPL trop volumineuse".into());
        }
        stream
            .set_write_timeout(Some(Duration::from_millis(250)))
            .map_err(|e| e.to_string())?;
        stream.write_all(&bytes).map_err(|e| e.to_string())?;
        Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_rapl_request_accepts_no_path_and_does_not_consume_gpu_json() {
        for input in [
            b"RAPL1\n".as_slice(),
            b"RAPL /etc/shadow\n",
            b"{\"command\":\"ping\"}\n",
        ] {
            let (mut client, mut server) = UnixStream::pair().unwrap();
            client.write_all(input).unwrap();
            let result = request(&mut server);
            if input == b"RAPL1\n" {
                assert!(result.unwrap());
            } else if input[0] == b'R' {
                assert!(result.is_err());
            } else {
                assert!(!result.unwrap());
                let mut read = vec![0; input.len()];
                server.read_exact(&mut read).unwrap();
                assert_eq!(read, input);
            }
        }
    }
    #[test]
    fn rapl_cache_is_independent_of_gpu_sessions() {
        let cached = Snapshot {
            clock_id: "test".into(),
            errors: vec!["Aucun domaine RAPL".into()],
            ..Default::default()
        };
        let mut cache = Cache {
            last: Some(Instant::now()),
            snapshot: Some(cached),
        };
        let (mut client, mut server) = UnixStream::pair().unwrap();
        client.write_all(b"RAPL1\n").unwrap();
        assert!(cache.handle(&mut server).unwrap());
        drop(server);
        let reply: Snapshot = serde_json::from_reader(client).unwrap();
        assert_eq!(reply.clock_id, "test");
    }
}
