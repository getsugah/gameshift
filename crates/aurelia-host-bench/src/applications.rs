//! Actual host zstd/libpng/OpenSSL, resolved through controlled function pointers.
use super::{resolve, Provider};
use serde::Serialize;
use std::{
    ffi::{c_char, c_void, CStr, CString},
    hint::black_box,
    io::Write,
    process::{Command, Stdio},
    time::Instant,
};
#[derive(Serialize)]
struct Sample {
    operation: String,
    logical_units: u64,
    iterations: u64,
    elapsed_seconds: f64,
    rate: f64,
    verified: bool,
}
#[derive(Serialize)]
struct LibraryReport {
    library: String,
    state: String,
    version: Option<String>,
    providers: Vec<Provider>,
    samples: Vec<Sample>,
    error: Option<String>,
}
struct Library {
    handle: *mut c_void,
    providers: Vec<Provider>,
}
impl Library {
    unsafe fn open(name: &str) -> Result<Self, String> {
        let name = CString::new(name).map_err(|e| e.to_string())?;
        let handle = libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            return Err("Bibliothèque hôte indisponible".into());
        }
        Ok(Self {
            handle,
            providers: vec![],
        })
    }
    unsafe fn symbol(&mut self, name: &str) -> Result<*mut c_void, String> {
        resolve(self.handle, name, &mut self.providers)
    }
}
impl Drop for Library {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.handle);
        }
    }
}
macro_rules! load {
    ($lib:expr,$name:literal,$t:ty) => {
        std::mem::transmute::<*mut c_void, $t>($lib.symbol($name)?)
    };
}
fn measure(
    name: &str,
    units: usize,
    op: &mut impl FnMut(bool) -> Result<(), String>,
) -> Result<Vec<Sample>, String> {
    op(false)?;
    op(true)?;
    let start = Instant::now();
    let mut probes = 0;
    while start.elapsed().as_millis() < 50 {
        op(false)?;
        probes += 1;
    }
    let n = ((0.55 / (start.elapsed().as_secs_f64() / probes as f64)).ceil() as u64).max(1);
    let mut samples = vec![];
    for _ in 0..3 {
        let start = Instant::now();
        for _ in 0..n {
            op(false)?;
        }
        let elapsed = start.elapsed().as_secs_f64();
        op(true)?;
        samples.push(Sample {
            operation: name.into(),
            logical_units: n * units as u64,
            iterations: n,
            elapsed_seconds: elapsed,
            rate: n as f64 * units as f64 / elapsed,
            verified: true,
        });
    }
    Ok(samples)
}
fn data(bytes: usize) -> Vec<u8> {
    (0..bytes)
        .map(|i| {
            if (i / 4096) % 4 == 0 {
                ((i.wrapping_mul(37) ^ (i >> 9)) & 255) as u8
            } else {
                (i % 251) as u8
            }
        })
        .collect()
}
unsafe fn zstd(lib: &mut Library) -> Result<(String, Vec<Sample>), String> {
    let version = load!(
        lib,
        "ZSTD_versionString",
        unsafe extern "C" fn() -> *const c_char
    );
    let bound = load!(
        lib,
        "ZSTD_compressBound",
        unsafe extern "C" fn(usize) -> usize
    );
    let compress = load!(
        lib,
        "ZSTD_compress",
        unsafe extern "C" fn(*mut c_void, usize, *const c_void, usize, i32) -> usize
    );
    let decompress = load!(
        lib,
        "ZSTD_decompress",
        unsafe extern "C" fn(*mut c_void, usize, *const c_void, usize) -> usize
    );
    let is_error = load!(lib, "ZSTD_isError", unsafe extern "C" fn(usize) -> u32);
    let source = data(8 * 1024 * 1024);
    let mut compressed = vec![0; bound(source.len())];
    let mut decoded = vec![0; source.len()];
    let mut length = 0;
    let mut samples = measure("zstd_compress_bytes", source.len(), &mut |verify| {
        if verify {
            let n = decompress(
                decoded.as_mut_ptr().cast(),
                decoded.len(),
                compressed.as_ptr().cast(),
                length,
            );
            if n != source.len() || decoded != source {
                return Err("Validation zstd compression échouée".into());
            }
        } else {
            length = compress(
                compressed.as_mut_ptr().cast(),
                compressed.len(),
                black_box(source.as_ptr().cast()),
                source.len(),
                3,
            );
            if is_error(length) != 0 {
                return Err("Compression zstd échouée".into());
            }
        }
        Ok(())
    })?;
    samples.extend(measure(
        "zstd_decompress_bytes",
        source.len(),
        &mut |verify| {
            if verify {
                if decoded != source {
                    return Err("Validation zstd décompression échouée".into());
                }
            } else {
                let n = decompress(
                    decoded.as_mut_ptr().cast(),
                    decoded.len(),
                    compressed.as_ptr().cast(),
                    length,
                );
                if n != source.len() {
                    return Err("Décompression zstd échouée".into());
                }
            }
            Ok(())
        },
    )?);
    Ok((
        CStr::from_ptr(version()).to_string_lossy().into_owned(),
        samples,
    ))
}
#[repr(C)]
struct Image {
    opaque: *mut c_void,
    version: u32,
    width: u32,
    height: u32,
    format: u32,
    flags: u32,
    colormap: u32,
    warning: u32,
    message: [u8; 64],
}
impl Image {
    fn new() -> Self {
        Self {
            opaque: std::ptr::null_mut(),
            version: 1,
            width: 512,
            height: 512,
            format: 3,
            flags: 0,
            colormap: 0,
            warning: 0,
            message: [0; 64],
        }
    }
}
unsafe fn png(lib: &mut Library) -> Result<(String, Vec<Sample>), String> {
    let version = load!(
        lib,
        "png_get_libpng_ver",
        unsafe extern "C" fn(*const c_void) -> *const c_char
    );
    let write = load!(
        lib,
        "png_image_write_to_memory",
        unsafe extern "C" fn(
            *mut Image,
            *mut c_void,
            *mut usize,
            i32,
            *const c_void,
            i32,
            *const c_void,
        ) -> i32
    );
    let begin = load!(
        lib,
        "png_image_begin_read_from_memory",
        unsafe extern "C" fn(*mut Image, *const c_void, usize) -> i32
    );
    let finish = load!(
        lib,
        "png_image_finish_read",
        unsafe extern "C" fn(*mut Image, *const c_void, *mut c_void, i32, *mut c_void) -> i32
    );
    let free = load!(lib, "png_image_free", unsafe extern "C" fn(*mut Image));
    let source = data(512 * 512 * 4);
    let mut image = Image::new();
    let mut required = 0;
    if write(
        &mut image,
        std::ptr::null_mut(),
        &mut required,
        0,
        source.as_ptr().cast(),
        0,
        std::ptr::null(),
    ) != 1
    {
        free(&mut image);
        return Err("Taille PNG impossible".into());
    }
    free(&mut image);
    let mut encoded = vec![0; required];
    let mut length = encoded.len();
    let mut decoded = vec![0; source.len()];
    let decode = |encoded: &[u8], destination: &mut [u8]| -> Result<(), String> {
        let mut image = Image::new();
        if begin(&mut image, encoded.as_ptr().cast(), encoded.len()) != 1 {
            free(&mut image);
            return Err("En-tête PNG invalide".into());
        }
        if image.width != 512 || image.height != 512 {
            free(&mut image);
            return Err("Dimensions PNG inattendues".into());
        }
        image.format = 3;
        let ok = finish(
            &mut image,
            std::ptr::null(),
            destination.as_mut_ptr().cast(),
            0,
            std::ptr::null_mut(),
        );
        free(&mut image);
        if ok != 1 {
            return Err("Décodage PNG échoué".into());
        }
        Ok(())
    };
    let mut samples = measure("png_encode_pixels", 512 * 512, &mut |verify| {
        if verify {
            decode(&encoded[..length], &mut decoded)?;
            if decoded != source {
                return Err("Pixels PNG encodés incorrects".into());
            }
        } else {
            let mut image = Image::new();
            length = encoded.len();
            let ok = write(
                &mut image,
                encoded.as_mut_ptr().cast(),
                &mut length,
                0,
                source.as_ptr().cast(),
                0,
                std::ptr::null(),
            );
            free(&mut image);
            if ok != 1 {
                return Err("Encodage PNG échoué".into());
            }
        }
        Ok(())
    })?;
    samples.extend(measure("png_decode_pixels", 512 * 512, &mut |verify| {
        if verify {
            if decoded != source {
                return Err("Pixels PNG décodés incorrects".into());
            }
        } else {
            decode(&encoded[..length], &mut decoded)?;
        }
        Ok(())
    })?);
    Ok((
        CStr::from_ptr(version(std::ptr::null()))
            .to_string_lossy()
            .into_owned(),
        samples,
    ))
}
unsafe fn crypto(lib: &mut Library) -> Result<(String, Vec<Sample>), String> {
    let version = load!(
        lib,
        "OpenSSL_version",
        unsafe extern "C" fn(i32) -> *const c_char
    );
    let sha = load!(
        lib,
        "SHA256",
        unsafe extern "C" fn(*const u8, usize, *mut u8) -> *mut u8
    );
    let source = data(8 * 1024 * 1024);
    let mut process = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    process
        .stdin
        .take()
        .ok_or("Entrée hash absente")?
        .write_all(&source)
        .map_err(|e| e.to_string())?;
    let result = process.wait_with_output().map_err(|e| e.to_string())?;
    if !result.status.success() {
        return Err("Vérification SHA-256 indépendante indisponible".into());
    }
    let expected = String::from_utf8(result.stdout)
        .map_err(|e| e.to_string())?
        .split_whitespace()
        .next()
        .ok_or("Hash absent")?
        .to_owned();
    let mut output = [0; 32];
    let samples = measure("openssl_sha256_bytes", source.len(), &mut |verify| {
        if verify {
            let got = output
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            if got != expected {
                return Err("SHA-256 différent de la vérification indépendante".into());
            }
        } else if sha(
            black_box(source.as_ptr()),
            source.len(),
            output.as_mut_ptr(),
        )
        .is_null()
        {
            return Err("SHA-256 échoué".into());
        }
        Ok(())
    })?;
    Ok((
        CStr::from_ptr(version(0)).to_string_lossy().into_owned(),
        samples,
    ))
}
pub fn run() -> Result<serde_json::Value, String> {
    for key in ["LD_LIBRARY_PATH", "LD_PRELOAD", "LD_AUDIT"] {
        if std::env::var_os(key).is_some_and(|v| !v.is_empty()) {
            return Err(format!("Environnement non isolé : {key}"));
        }
    }
    let mut reports = Vec::new();
    for (name, soname, execute) in [
        (
            "zstd",
            "libzstd.so.1",
            zstd as unsafe fn(&mut Library) -> Result<(String, Vec<Sample>), String>,
        ),
        ("png", "libpng16.so.16", png),
        ("openssl", "libcrypto.so.3", crypto),
    ] {
        let report = unsafe {
            match Library::open(soname) {
                Err(e) => LibraryReport {
                    library: name.into(),
                    state: "unavailable".into(),
                    version: None,
                    providers: vec![],
                    samples: vec![],
                    error: Some(e),
                },
                Ok(mut lib) => {
                    let result = execute(&mut lib);
                    match result {
                        Ok((version, samples)) => LibraryReport {
                            library: name.into(),
                            state: "executed".into(),
                            version: Some(version),
                            providers: std::mem::take(&mut lib.providers),
                            samples,
                            error: None,
                        },
                        Err(e) => LibraryReport {
                            library: name.into(),
                            state: "failed".into(),
                            version: None,
                            providers: std::mem::take(&mut lib.providers),
                            samples: vec![],
                            error: Some(e),
                        },
                    }
                }
            }
        };
        reports.push(report);
    }
    Ok(
        serde_json::json!({"protocol":"aurelia-host-applications-v1","engine_sha256":std::env::current_exe().ok().and_then(|p|super::sha256(&p)),"seed":"deterministic-pattern-v1","png_image_struct_bytes":std::mem::size_of::<Image>(),"libraries":reports,"notes":["Functions resolved from host libraries; IFUNC/CPU dispatch not forced.","zstd level 3; libpng simplified RGBA 512x512 API including its allocations; SHA-256 checked independently outside timing."]}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn png_layout_matches_stable_c_abi() {
        assert_eq!(std::mem::size_of::<Image>(), 104);
    }
}
