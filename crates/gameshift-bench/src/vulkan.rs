//! RAII Vulkan instance, with no dependency on a window system or a shell tool.
use ash::{vk, Entry};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    ffi::{CStr, CString},
    fmt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

/// Validation is a development tool, never silently treated as enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationMode {
    Disabled,
    IfAvailable,
    Required,
}

/// An actionable failure rather than a fabricated Vulkan device.
#[derive(Debug)]
pub enum ProbeError {
    Loader(String),
    Vulkan {
        operation: &'static str,
        result: vk::Result,
    },
    ValidationUnavailable,
    NoDevice,
}
impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Loader(message) => write!(f, "Chargeur Vulkan inaccessible : {message}"),
            Self::Vulkan { operation, result } => write!(f, "{operation} : {result:?}"),
            Self::ValidationUnavailable => write!(f, "Couche VK_LAYER_KHRONOS_validation absente ; installer vulkan-validation-layers pour la validation de développement"),
            Self::NoDevice => write!(f, "Aucun GPU Vulkan accessible. Vérifier les droits d’accès à /dev/dri et les pilotes depuis la session graphique."),
        }
    }
}
impl Error for ProbeError {}
fn api_error(operation: &'static str, result: vk::Result) -> ProbeError {
    ProbeError::Vulkan { operation, result }
}

/// Queue families, including the number of valid timestamp counter bits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueCapabilities {
    pub family_index: u32,
    pub queue_count: u32,
    pub graphics: bool,
    pub compute: bool,
    pub timestamp_valid_bits: u32,
}

/// Driver-reported capabilities; presentation support needs an actual surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuCapabilities {
    pub enumeration_index: usize,
    pub name: String,
    pub kind: String,
    pub vendor_id: u32,
    pub device_id: u32,
    pub device_uuid: Option<String>,
    pub api_version: String,
    pub driver_version_raw: u32,
    pub driver_name: Option<String>,
    pub driver_info: Option<String>,
    pub device_local_heap_bytes: u64,
    pub max_image_dimension_2d: u32,
    pub timestamp_period_ns: f32,
    pub queues: Vec<QueueCapabilities>,
    pub extensions: Vec<String>,
    pub swapchain_extension: bool,
}
impl GpuCapabilities {
    /// Necessary capabilities only: this does not prove surface presentation works.
    pub fn raster_candidate(&self) -> bool {
        self.swapchain_extension && self.queues.iter().any(|q| q.graphics && q.queue_count > 0)
    }
    /// Timestamp counters must exist on a graphics queue before GPU timing is attempted.
    pub fn graphics_timestamps(&self) -> bool {
        self.timestamp_period_ns.is_finite()
            && self.timestamp_period_ns > 0.0
            && self
                .queues
                .iter()
                .any(|q| q.graphics && q.queue_count > 0 && q.timestamp_valid_bits > 0)
    }
}

/// A capability snapshot with explicit validation and environment warnings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VulkanReport {
    pub loader_api_version: String,
    pub instance_api_version: String,
    pub validation_enabled: bool,
    pub debug_messages_enabled: bool,
    pub warnings: Vec<String>,
    pub gpus: Vec<GpuCapabilities>,
}

pub(crate) struct Instance {
    // Retain the dynamic loader until after all Vulkan objects have been destroyed.
    pub(crate) _entry: Entry,
    pub(crate) instance: ash::Instance,
    debug: Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    pub(crate) validation_enabled: bool,
    pub(crate) errors: Arc<AtomicUsize>,
}
impl Instance {
    pub(crate) fn monitored(&self) -> bool {
        self.debug.is_some()
    }
}
impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: All child devices and surfaces have been destroyed by their owners.
        unsafe {
            if let Some((loader, messenger)) = &self.debug {
                loader.destroy_debug_utils_messenger(*messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

unsafe extern "system" fn debug_message(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _kinds: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user: *mut std::ffi::c_void,
) -> vk::Bool32 {
    if !data.is_null() {
        // SAFETY: Vulkan supplies valid callback data and a NUL-terminated message for this call.
        let message = unsafe { (*data).p_message };
        if !message.is_null() {
            let message = unsafe { CStr::from_ptr(message) }.to_string_lossy();
            if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
                if !_user.is_null() {
                    // SAFETY: user data points to the AtomicUsize owned by the live Instance.
                    unsafe { &*(_user as *const AtomicUsize) }.fetch_add(1, Ordering::Relaxed);
                }
                tracing::error!(%message, "Vulkan validation");
            } else {
                tracing::warn!(%message, "Vulkan validation");
            }
        }
    }
    vk::FALSE
}

fn version(raw: u32) -> String {
    format!(
        "{}.{}.{}",
        vk::api_version_major(raw),
        vk::api_version_minor(raw),
        vk::api_version_patch(raw)
    )
}
pub(crate) fn array_text(array: &[std::ffi::c_char]) -> String {
    // These Vulkan fixed-size arrays are specified to contain a NUL terminator.
    // Read within their bounds even if a defective driver violates that requirement.
    let bytes: Vec<u8> = array
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Inspect Vulkan without opening a window or creating a logical device.
/// Software devices are reported explicitly rather than accepted as hardware results.
pub(crate) fn create_instance(
    validation: ValidationMode,
    required_extensions: &[*const std::ffi::c_char],
) -> Result<(Instance, u32, u32), ProbeError> {
    // SAFETY: Entry resolves the system Vulkan loader and owns the library handle.
    let entry = unsafe { Entry::load() }.map_err(|e| ProbeError::Loader(e.to_string()))?;
    // SAFETY: Vulkan enumeration functions only write their documented output structures.
    let loader_version = unsafe { entry.try_enumerate_instance_version() }
        .map_err(|e| api_error("Version du chargeur Vulkan", e))?
        .unwrap_or(vk::API_VERSION_1_0);
    let requested_version = loader_version.min(vk::API_VERSION_1_2);
    let layers = unsafe { entry.enumerate_instance_layer_properties() }
        .map_err(|e| api_error("Liste des couches Vulkan", e))?;
    let validation_available = layers
        .iter()
        .any(|l| array_text(&l.layer_name) == "VK_LAYER_KHRONOS_validation");
    if validation == ValidationMode::Required && !validation_available {
        return Err(ProbeError::ValidationUnavailable);
    }
    let validation_enabled = validation != ValidationMode::Disabled && validation_available;
    let extensions = unsafe { entry.enumerate_instance_extension_properties(None) }
        .map_err(|e| api_error("Extensions d’instance Vulkan", e))?;
    let debug_available = extensions
        .iter()
        .any(|e| array_text(&e.extension_name) == "VK_EXT_debug_utils");
    let application = CString::new("GameShift").expect("constant without NUL");
    let validation_name =
        CString::new("VK_LAYER_KHRONOS_validation").expect("constant without NUL");
    let layer_names: Vec<_> = if validation_enabled {
        vec![validation_name.as_ptr()]
    } else {
        vec![]
    };
    let mut extension_names: Vec<_> = if validation_enabled && debug_available {
        vec![ash::ext::debug_utils::NAME.as_ptr()]
    } else {
        vec![]
    };
    extension_names.extend_from_slice(required_extensions);
    let app = vk::ApplicationInfo::default()
        .application_name(&application)
        .api_version(requested_version);
    let info = vk::InstanceCreateInfo::default()
        .application_info(&app)
        .enabled_layer_names(&layer_names)
        .enabled_extension_names(&extension_names);
    // SAFETY: All referenced names/info remain alive during instance creation.
    let instance = unsafe { entry.create_instance(&info, None) }
        .map_err(|e| api_error("Création de l’instance Vulkan", e))?;
    let mut owner = Instance {
        _entry: entry,
        instance,
        debug: None,
        validation_enabled,
        errors: Arc::new(AtomicUsize::new(0)),
    };
    if validation_enabled && debug_available {
        let debug = ash::ext::debug_utils::Instance::new(&owner._entry, &owner.instance);
        let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
            .message_severity(
                vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                    | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
            )
            .message_type(
                vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
            )
            .pfn_user_callback(Some(debug_message))
            .user_data(Arc::as_ptr(&owner.errors).cast_mut().cast());
        // SAFETY: The callback uses no user pointer; messenger is destroyed before its instance.
        let messenger = unsafe { debug.create_debug_utils_messenger(&info, None) }
            .map_err(|e| api_error("Messages de validation Vulkan", e))?;
        owner.debug = Some((debug, messenger));
    }
    Ok((owner, loader_version, requested_version))
}

/// Inspect Vulkan without opening a surface or creating a logical device.
pub fn probe(validation: ValidationMode) -> Result<VulkanReport, ProbeError> {
    let (owner, loader_version, requested_version) = create_instance(validation, &[])?;
    let validation_enabled = owner.validation_enabled;
    let devices = unsafe { owner.instance.enumerate_physical_devices() }
        .map_err(|e| api_error("Détection des GPU Vulkan", e))?;
    if devices.is_empty() {
        return Err(ProbeError::NoDevice);
    }
    let mut gpus = Vec::new();
    for (enumeration_index, physical) in devices.into_iter().enumerate() {
        // SAFETY: The physical device handle belongs to the live instance.
        let properties = unsafe { owner.instance.get_physical_device_properties(physical) };
        let memory = unsafe {
            owner
                .instance
                .get_physical_device_memory_properties(physical)
        };
        let queues = unsafe {
            owner
                .instance
                .get_physical_device_queue_family_properties(physical)
        };
        let extensions = unsafe {
            owner
                .instance
                .enumerate_device_extension_properties(physical)
        }
        .map_err(|e| api_error("Extensions du GPU Vulkan", e))?;
        let mut device_uuid = None;
        let mut driver_name = None;
        let mut driver_info = None;
        if requested_version >= vk::API_VERSION_1_1 && properties.api_version >= vk::API_VERSION_1_1
        {
            let mut ids = vk::PhysicalDeviceIDProperties::default();
            let mut drivers = vk::PhysicalDeviceDriverProperties::default();
            let mut extended = vk::PhysicalDeviceProperties2::default().push_next(&mut ids);
            if properties.api_version >= vk::API_VERSION_1_2
                && requested_version >= vk::API_VERSION_1_2
            {
                extended = extended.push_next(&mut drivers);
            }
            // SAFETY: Requested structures are core in the negotiated API versions above.
            unsafe {
                owner
                    .instance
                    .get_physical_device_properties2(physical, &mut extended);
            }
            device_uuid = Some(ids.device_uuid.iter().map(|b| format!("{b:02x}")).collect());
            if properties.api_version >= vk::API_VERSION_1_2
                && requested_version >= vk::API_VERSION_1_2
            {
                driver_name = Some(array_text(&drivers.driver_name));
                driver_info = Some(array_text(&drivers.driver_info));
            }
        }
        let mut extensions: Vec<_> = extensions
            .iter()
            .map(|e| array_text(&e.extension_name))
            .collect();
        extensions.sort();
        let kind = match properties.device_type {
            vk::PhysicalDeviceType::DISCRETE_GPU => "GPU dédié",
            vk::PhysicalDeviceType::INTEGRATED_GPU => "GPU intégré",
            vk::PhysicalDeviceType::VIRTUAL_GPU => "GPU virtuel",
            vk::PhysicalDeviceType::CPU => "Rendu logiciel (CPU)",
            _ => "Autre périphérique Vulkan",
        };
        gpus.push(GpuCapabilities {
            enumeration_index,
            name: array_text(&properties.device_name),
            kind: kind.into(),
            vendor_id: properties.vendor_id,
            device_id: properties.device_id,
            device_uuid,
            api_version: version(properties.api_version),
            driver_version_raw: properties.driver_version,
            driver_name,
            driver_info,
            device_local_heap_bytes: memory.memory_heaps[..memory.memory_heap_count as usize]
                .iter()
                .filter(|h| h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL))
                .map(|h| h.size)
                .sum(),
            max_image_dimension_2d: properties.limits.max_image_dimension2_d,
            timestamp_period_ns: properties.limits.timestamp_period,
            queues: queues
                .into_iter()
                .enumerate()
                .map(|(i, q)| QueueCapabilities {
                    family_index: i as u32,
                    queue_count: q.queue_count,
                    graphics: q.queue_flags.contains(vk::QueueFlags::GRAPHICS),
                    compute: q.queue_flags.contains(vk::QueueFlags::COMPUTE),
                    timestamp_valid_bits: q.timestamp_valid_bits,
                })
                .collect(),
            swapchain_extension: extensions.iter().any(|e| e == "VK_KHR_swapchain"),
            extensions,
        });
    }
    let mut warnings = Vec::new();
    if !validation_enabled {
        warnings.push("Validation Vulkan désactivée ou couche indisponible.".into());
    }
    if gpus.iter().any(|g| g.kind == "Rendu logiciel (CPU)") {
        warnings.push("Un périphérique de rendu logiciel est présent ; il ne représente pas les performances d’une carte graphique.".into());
    }
    warnings.push(
        "Présentation Wayland/X11 non testée : elle exige une fenêtre et une surface Vulkan."
            .into(),
    );
    Ok(VulkanReport {
        loader_api_version: version(loader_version),
        instance_api_version: version(requested_version),
        validation_enabled,
        debug_messages_enabled: owner.debug.is_some(),
        warnings,
        gpus,
    })
}
