//! Resolve locally installed hardware names without network or external commands.

/// Numeric identifiers read from a single PCI device, never inferred from VRAM.
pub(crate) struct Ids {
    pub vendor: u16,
    pub device: u16,
    pub revision: Option<u8>,
    pub subsystem: Option<(u16, u16)>,
}

#[derive(Default)]
pub(crate) struct PciNames {
    pub vendor: Option<String>,
    pub device: Option<String>,
    pub subsystem: Option<String>,
}

pub(crate) fn hex(input: &str) -> Option<u16> {
    u16::from_str_radix(input.trim().trim_start_matches("0x"), 16).ok()
}

/// pci.ids hierarchy is vendor -> device -> subsystem; never cross those scopes.
pub(crate) fn pci_names(text: &str, ids: &Ids) -> PciNames {
    let mut result = PciNames::default();
    let mut vendor_matches = false;
    let mut device_matches = false;
    for line in text
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        if let Some(row) = line.strip_prefix("\t\t") {
            if !vendor_matches || !device_matches {
                continue;
            }
            let mut fields = row.split_whitespace();
            let Some(vendor) = fields.next().and_then(hex) else {
                continue;
            };
            let Some(device) = fields.next().and_then(hex) else {
                continue;
            };
            let name = fields.collect::<Vec<_>>().join(" ");
            if ids.subsystem == Some((vendor, device)) && !name.is_empty() {
                result.subsystem = Some(name);
            }
        } else if let Some(row) = line.strip_prefix('\t') {
            let mut fields = row.split_whitespace();
            device_matches = vendor_matches && fields.next().and_then(hex) == Some(ids.device);
            if device_matches {
                let name = fields.collect::<Vec<_>>().join(" ");
                if !name.is_empty() {
                    result.device = Some(name);
                }
            }
        } else {
            let mut fields = line.split_whitespace();
            vendor_matches = fields.next().and_then(hex) == Some(ids.vendor);
            device_matches = false;
            if vendor_matches {
                let name = fields.collect::<Vec<_>>().join(" ");
                if !name.is_empty() {
                    result.vendor = Some(name);
                }
            }
        }
    }
    result
}

/// libdrm amdgpu.ids matches both device and revision, and applies only to AMD.
pub(crate) fn amd_name<'a>(text: &'a str, ids: &Ids) -> Option<&'a str> {
    if ids.vendor != 0x1002 {
        return None;
    }
    let revision = ids.revision?;
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .find_map(|line| {
            let mut fields = line.splitn(3, ',');
            let device = fields.next().and_then(hex)?;
            let rev = fields.next().and_then(hex)?;
            let name = fields.next()?.trim();
            (device == ids.device && rev == u16::from(revision) && !name.is_empty()).then_some(name)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_same_device_by_revision_and_vendor() {
        let text = "# device, revision, name\n1.0.0\n747E, C8, AMD Radeon RX 7800 XT\n747E, FF, AMD Radeon RX 7700 XT\n";
        let mut ids = Ids {
            vendor: 0x1002,
            device: 0x747e,
            revision: Some(0xc8),
            subsystem: None,
        };
        assert_eq!(amd_name(text, &ids), Some("AMD Radeon RX 7800 XT"));
        ids.revision = Some(0xff);
        assert_eq!(amd_name(text, &ids), Some("AMD Radeon RX 7700 XT"));
        ids.revision = None;
        assert_eq!(amd_name(text, &ids), None);
        ids.vendor = 0x10de;
        ids.revision = Some(0xc8);
        assert_eq!(amd_name(text, &ids), None);
    }

    #[test]
    fn scopes_subsystems_to_their_vendor_and_device() {
        let text = "1002  AMD\n\t747e  Shared GPU family\n\t\t1eae 7801  Specific board\n\t9999  Other GPU\n\t\t1eae 7801  Wrong device\n10de  NVIDIA\n\t747e  Wrong vendor\n\t\t1eae 7801  Wrong board\n";
        let mut ids = Ids {
            vendor: 0x1002,
            device: 0x747e,
            revision: None,
            subsystem: Some((0x1eae, 0x7801)),
        };
        let names = pci_names(text, &ids);
        assert_eq!(names.vendor.as_deref(), Some("AMD"));
        assert_eq!(names.device.as_deref(), Some("Shared GPU family"));
        assert_eq!(names.subsystem.as_deref(), Some("Specific board"));
        ids.device = 0x1234;
        let names = pci_names(text, &ids);
        assert!(names.device.is_none());
        assert!(names.subsystem.is_none());
    }
}
