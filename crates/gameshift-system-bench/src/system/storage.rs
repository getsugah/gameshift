//! Sequential filesystem I/O, with an unlinked private file and direct I/O.
//! Never opens a block device or modifies an existing file.
use super::{metric, Category, State};
use std::{
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
    time::Instant,
};

const BYTES: usize = 256 * 1024 * 1024;
const BLOCK: usize = 1024 * 1024;
struct Aligned(*mut u8, usize);
impl Aligned {
    fn new(size: usize) -> Result<Self, String> {
        let mut p = std::ptr::null_mut();
        // O_DIRECT requires aligned address, offsets and transfer lengths.
        let error = unsafe { libc::posix_memalign(&mut p, 4096, size) };
        if error != 0 {
            return Err(std::io::Error::from_raw_os_error(error).to_string());
        }
        let mut buffer = Self(p.cast(), size);
        let mut seed = 0x4155_5245_4c49_4133u64;
        for byte in buffer.bytes() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            *byte = seed as u8;
        }
        Ok(buffer)
    }
    fn bytes(&mut self) -> &mut [u8] {
        // Allocation is owned until Drop; this is its only mutable accessor.
        unsafe { std::slice::from_raw_parts_mut(self.0, self.1) }
    }
}
impl Drop for Aligned {
    fn drop(&mut self) {
        unsafe {
            libc::free(self.0.cast());
        }
    }
}

fn measure(directory: &Path, bytes: usize, block: usize) -> Result<Category, String> {
    let started = Instant::now();
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let mut buffer = Aligned::new(block)?;
    let mut expected = Aligned::new(block)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = directory.join(format!("aurelia-{}-{nonce}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_DIRECT | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
        .map_err(|e| format!("Fichier privé / O_DIRECT indisponible : {e}"))?;
    // Only this newly created file is unlinked. The descriptor keeps it alive;
    // cancellation, panic or process death also releases its disk space.
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    let mut filesystem = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(file.as_raw_fd(), filesystem.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let filesystem = unsafe { filesystem.assume_init() };
    if [0x01021994, 0x858458f6].contains(&filesystem.f_type) {
        return Err("Système de fichiers mémoire : mesure de stockage indisponible".into());
    }
    let device = file.metadata().map_err(|e| e.to_string())?.dev();
    let device = format!("{}:{}", libc::major(device), libc::minor(device));
    let volume = std::fs::read_to_string("/proc/self/mountinfo")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                let (prefix, suffix) = line.split_once(" - ")?;
                if prefix.split_whitespace().nth(2)? != device {
                    return None;
                }
                let mut fields = suffix.split_whitespace();
                let filesystem = fields.next()?;
                let source = fields.next()?;
                Some(format!("{source} · {filesystem}"))
            })
        })
        .unwrap_or_else(|| format!("Volume {device}"));
    let mut fs = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::fstatvfs(file.as_raw_fd(), fs.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let fs = unsafe { fs.assume_init() };
    let available = u128::from(fs.f_bavail) * u128::from(fs.f_frsize);
    if available < bytes as u128 + 512 * 1024 * 1024 {
        return Err("Espace libre insuffisant (réserve de 512 Mio)".into());
    }
    let mut writes = Vec::new();
    let mut reads = Vec::new();
    let mut samples = Vec::new();
    for repetition in 0..3 {
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        buffer.bytes().copy_from_slice(expected.bytes());
        let start = Instant::now();
        for _ in 0..bytes / block {
            file.write_all(buffer.bytes())
                .map_err(|e| format!("Écriture directe indisponible : {e}"))?;
        }
        file.sync_data().map_err(|e| e.to_string())?;
        let write_seconds = start.elapsed().as_secs_f64();
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let start = Instant::now();
        for _ in 0..bytes / block {
            file.read_exact(buffer.bytes()).map_err(|e| e.to_string())?;
            if buffer.bytes() != expected.bytes() {
                return Err("Vérification des données stockées échouée".into());
            }
        }
        let read_seconds = start.elapsed().as_secs_f64();
        writes.push(bytes as f64 / 1e9 / write_seconds);
        reads.push(bytes as f64 / 1e9 / read_seconds);
        samples.push(serde_json::json!({"repetition":repetition,"bytes":bytes,"write_seconds":write_seconds,"read_seconds":read_seconds,"verified":true}));
    }
    Ok(Category {
        id:"storage".into(), state:State::Executed, elapsed_seconds:started.elapsed().as_secs_f64(),
        metrics:vec![metric("storage/read", "storage", "Lecture séquentielle vérifiée", "Go/s", reads, &format!("direct-sequential-v1/{bytes}/{block}/read-verified")), metric("storage/write", "storage", "Écriture séquentielle synchronisée", "Go/s", writes, &format!("direct-sequential-v1/{bytes}/{block}/write-fdatasync"))],
        raw:serde_json::json!({"directory":directory,"volume":volume,"device":device,"filesystem_type":filesystem.f_type,"bytes":bytes,"block_bytes":block,"pattern":"xorshift64-seed-415552454c494133-repeated-block","direct_io":true,"write_sync":"fdatasync","read_verification_in_timing":true,"samples":samples}),
        notes:vec!["Système de fichiers du dossier d’état GameShift ; O_DIRECT évite le cache de pages Linux. Les caches du périphérique et du contrôleur restent possibles. Ce test séquentiel ne mesure ni IOPS aléatoires ni endurance. La lecture inclut la vérification CPU ; l’écriture inclut fdatasync. Aucun accès brut au disque.".into()],
    })
}
pub fn run() -> Category {
    let directory = crate::store::root().join("storage-work");
    measure(&directory, BYTES, BLOCK)
        .unwrap_or_else(|e| Category::missing("storage", State::Unavailable, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_file_is_verified_and_never_replaces_existing_data() {
        let directory =
            std::env::temp_dir().join(format!("aurelia-storage-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let existing = directory.join("user-data");
        std::fs::write(&existing, b"preserve").unwrap();
        match measure(&directory, 8192, 4096) {
            Ok(c) => {
                assert_eq!(c.metrics.len(), 2);
                assert!(c
                    .metrics
                    .iter()
                    .all(|m| m.values.len() == 3 && m.values.iter().all(|v| *v > 0.)));
            }
            Err(e) => assert!(
                e.contains("indisponible") || e.contains("insuffisant"),
                "{e}"
            ),
        }
        assert_eq!(std::fs::read(&existing).unwrap(), b"preserve");
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_file(existing).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
