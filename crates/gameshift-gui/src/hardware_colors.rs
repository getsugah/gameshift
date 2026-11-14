//! Shared semantic hardware colors, independent of any particular page.
pub fn class(name: &str) -> &'static str {
    let name = name.to_lowercase();
    if name.contains("amd") || name.contains("radeon") || name.contains("ryzen") {
        "gs-vendor-amd"
    } else if name.contains("intel") {
        "gs-vendor-intel"
    } else {
        "gs-vendor-neutral"
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn names_choose_vendor_without_case_assumptions() {
        assert_eq!(super::class("AMD Ryzen"), "gs-vendor-amd");
        assert_eq!(super::class("Intel Core"), "gs-vendor-intel");
        assert_eq!(super::class("Unknown"), "gs-vendor-neutral");
        assert_eq!(super::class("NVIDIA GeForce"), "gs-vendor-neutral");
    }
}
